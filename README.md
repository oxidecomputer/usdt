# `usdt`

Dust your Rust with USDT probes.

## Overview

`usdt` exposes statically-defined [DTrace probes][1] to Rust code. Users write a _provider_
definition, in either the D language or directly in Rust code. The _probes_ of the provider
can then be compiled into Rust code that fire the probes. These are visible via the `dtrace`
command-line tool.

There are three mechanisms for converting the D probe definitions into Rust.

1. A `build.rs` script
2. A function-like procedural macro, `usdt::dtrace_provider`.
3. An attribute macro, `usdt::provider`.

The generated code is the same in all cases, though the third provides a bit more flexibility
than the first two. See [below][Serializable types] for more details, but briefly, the third
form supports probe arguments of any type that implement [`serde::Seralize`][2]. These different
versions are shown in the crates `probe-test-{build,macro,attr}` respectively.

> Note: This crate uses inline assembly to work its magic. As such a nightly Rust toolchain is
required, and the functionality is hidden behind the `"asm"` feature flag. A nightly toolchain
can be installed with `rustup toolchain install nightly`. See [the notes](#notes) for a
discussion.

## Example

The `probe-test-build` binary crate in this package implements a complete example, using the
build-time code generation.

The starting point is a D script, called `"test.d"`. It looks like:

```d
provider test {
	probe start(uint8_t);
	probe stop(char*, uint8_t);
};
```

This script defines a single provider, `test`, with two probes, `start` and `stop`,
with a different set of arguments. (Numeric primitive types and `&str`s are currently
supported.)

This provider definition must be converted into Rust code, which can be done in a simple
build script:


```rust
use usdt::Builder;

fn main() {
	Builder::new("test.d").build().unwrap();
}
```

This generates a file in the directory `OUT_DIR` which contains the generated Rust macros
that fire the probes. Unless it is changed, this file is named the same as the provider
definition file, so `test.rs` in this case.

Using the probes in Rust code looks like the following, which is in `probe-test-build/src/main.rs`.

```rust
//! An example using the `usdt` crate, generating the probes via a build script.
#![feature(asm)]

use std::thread::sleep;
use std::time::Duration;

use usdt::register_probes;

// Include the Rust implementation generated by the build script.
include!(concat!(env!("OUT_DIR"), "/test.rs"));

fn main() {
    let duration = Duration::from_secs(1);
    let mut counter: u8 = 0;

    // NOTE: One _must_ call this function in order to actually register the probes with DTrace.
    // Without this, it won't be possible to list, enable, or see the probes via `dtrace(1)`.
    register_probes().unwrap();

    loop {
        // Call the "start" probe which accepts a u8.
        test_start!(|| (counter));

        // Do some work.
        sleep(duration);

        // Call the "stop" probe, which accepts a &str and a u8.
        test_stop!(|| ("the probe has fired", counter));

        counter = counter.wrapping_add(1);
    }
}
```

Note that the `#![feature(asm)]` attribute is required. One can also see that the Rust code
is included directly using the `include!` macro. The probe definitions are converted into Rust
macros, named by the provider and probe. In our case, the first probe is converted into a macro
`test_start!`.

> IMPORTANT: It's important to note that the application _must_ call `usdt::register_probes()`
in order to actually register the probe points with DTrace. Failing to do this will not impact
the application's functionality, but it will be impossible to list, enable, or otherwise see the
probes with the `dtrace(1)` tool without this.

We can see that this is hooked up with DTrace by running the example and listing the expected
probes by name.

```bash
$ cargo +nightly run --features asm
```

And in another terminal, list the matching probes with:

```bash
$ sudo dtrace -l -n test*:::
   ID   PROVIDER            MODULE                          FUNCTION NAME
 2865  test14314  probe-test-build _ZN16probe_test_build4main17h906db832bb52ab01E [probe_test_build::main::h906db832bb52ab01] start
 2866  test14314  probe-test-build _ZN16probe_test_build4main17h906db832bb52ab01E [probe_test_build::main::h906db832bb52ab01] stop
 ```

## Probe arguments

One can see that the probe macros are called with closures, rather than with the probe
arguments directly. This has two purposes.

First, it indicates that the probe arguments may not be evaluated. DTrace generates
"is-enabled" probes for defined probe, which is a simple way to check if the probe has
currently been enabled. The arguments are only unpacked if the probe is enabled, and
so users _must not_ rely on side-effects. The closure helps indicate this.

The second point of this is efficiency. Again, the arguments are not evaluated if the
probe is not enabled. The closure is only evaluated internally _after_ the probe is
verified to be enabled, which avoid the unnecessary work of argument marshalling if
the probe is disabled.

## Procedural macro version

The procedural macro version of this crate can be seen in the `probe-test-macro` example,
which is nearly identical to the above example. However, there is no build.rs script,
so in place of the `include!` macro, one finds the procedural macro:

```rust
dtrace_provider!("test.d");
```

This macro generates the same macros as seen above, but does at the time the source
itself is compiled. This may be easier for some use cases, as there is no build script.
However, procedural macros have downsides. It can be difficult to understand their
internals, especially when things fail. Additionally, the macro is run on every compile,
even if the provider definition is unchanged. This may be negligible for small provider
definitions, but users may see a noticeable increase in compile times when many probes
are defined.

## Serializable types

As described above, the three forms of defining a provider a _nearly_ equivalent. The
only distinction is in the support of types implementing [`serde::Serialize`][2]. This uses
DTrace's [JSON functionality][3] -- Any serializable type is serialized to JSON with
[`serde_json::to_string()`][4], and the string may be unpacked and inspected in DTrace
scripts with the `json` function. For example, imagine we have the type:

```rust
#[derive(serde::Serialize)]
pub struct Arg {
    val: u8,
    data: Vec<String>,
}
```

and a probe definition:

```rust
#[usdt::provider]
mod my_provider {
    use super::Arg;
    fn my_probe(_: &Arg) {}
}
```

Values of type `Arg` may be used in the generated probe macros. In a DTrace script, one can
look at the data in the argument like:

```
dtrace -n 'my_probe* { printf("%s", json(copyinstr(arg0), "ok.val")); }' # prints `Arg::val`.
```

The `json` function also supports nested objects and array indexing, so one could also do:

```
dtrace -n 'my_probe* { printf("%s", json(copyinstr(arg0), "ok.data[0]")); }' # prints `Arg::data[0]`.
```

See the `probe-test-attr` example for more details and usage.

### Serialization is fallible

Note that in the above examples, the first key of the JSON blob being accessed is `"ok"`. This
is because the `serde_json::to_string` function is fallible, returning a `Result`. This is mapped
into JSON in a natural way:

- `Ok(_) => {"ok": _}`
- `Err(_) => {"err": _}`

In the error case, the [`Error`][serde-json-error] returned is formatted using its `Display`
implementation. This isn't an academic concern. It's quite easy to build types that successfully
compile, and yet fail to serialize at runtime, even with types that `#[derive(Serialize)]`. See
[this issue][serde-runtime-fail] for details.

## A note about registration

Note that the `usdt::register_probes()` function is called at the top of main in the above
example. This method is required to actually register the probes with the DTrace kernel
module. This presents a quandary for library developers who wish to instrument their
code, as consumers of their library may forget to (or choose not to) call this function.
There are potential workarounds to this problem (init-sections, other magic), but each
comes with significant tradeoffs. As such the current recommendation is:

> Library developers are encouraged to re-export the `usdt::register_probes` (or a
function calling it), and document to their users that this function should be called to
guarantee that probes are registered.

## Notes

The `usdt` crate requires a nightly toolchain, as it relies on the currently-unstable [inline
asm][inline-asm] feature. However, the crate contains an empty, no-op implementation, which
generates all the same probe macros, but with empty bodies. This may be selected by passing
the `--no-default-features` flag when building the crate, or by using `default-features = false`
in the [`[dependencies]` table][feature-deps] of one's `Cargo.toml`.

Library developers may use `usdt` as an optional dependency, gated by a feature, for example
named `usdt-probes` or similar. This feature would imply the `usdt/asm` feature, but the `usdt`
crate could be used with the no-op implementation by default. For example, your `Cargo.toml`
might contain

```
[dependencies]
usdt = { version = "*", optional = true, default-features = false }

# ... later

[features]
usdt-probes = ["usdt/asm"]
```

This allows users to opt into probes if they're willing to accept a nightly toolchain.

### The Rust `asm` feature

Recall from the example that the `usdt` crate relies on inline `asm`, which is [not yet][asm-issue] a
stable Rust feature. This means that the code _calling_ the generated probe macros must
be in a module where the `asm!` macro can be used, i.e., where the `feature(asm)` configuration
directive is applicable. Macros-by-example, like those generated by the `usdt` procedural
macros, should be defined at the crate root, so that they may be called from anywhere in the
crate. The `feature(asm)` directive should also be at the crate root, again, so the generated
macros can be called from anywhere in the crate.

In the special case of a library, with the re-exporting feature flag as described
previously, this would look something like `#![cfg_attr(feature = "usdt-probes", feature(asm))]`,
again, at the crate root of the library.

## References

[1]: https://illumos.org/books/dtrace/chp-usdt.html#chp-usdt
[2]: https://docs.rs/serde/1.0.130/serde/trait.Serialize.html
[3]: https://sysmgr.org/blog/2012/11/29/dtrace_and_json_together_at_last/
[4]: https://docs.rs/serde_json/1.0.68/serde_json/fn.to_string.html
[serde-json-error]: https://docs.serde.rs/serde_json/error/struct.Error.html
[serde-runtime-fail]: https://github.com/serde-rs/serde/issues/1307
[inline-asm]: https://github.com/rust-lang/rust/issues/72016
[feature-deps]: https://doc.rust-lang.org/cargo/reference/features.html#dependency-features
[asm-issue]: https://github.com/rust-lang/rust/issues/72016
