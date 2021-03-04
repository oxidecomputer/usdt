# `usdt`

Dust your Rust with USDT probes.

## Overview

`usdt` exposes statically-defined DTrace probes to Rust code. Users write a provider definition
as usual, in a D language script. The crate exports a procedural macro, `usdt::dtrace_provider`,
which generates Rust code to call into these probe functions.

## Important note

This crate crate is a bit schizophrenic. It currently operates in two mutually-
exclusive variants. In the "static library" variant, a build.rs script is used to compile an
FFI interface between Rust and C, and uses the normal DTrace mechanisms for defining the probes
in C. The benefits of this approach are simplicity and less maintainence costs on the crate
developers. The drawbacks are an increased onus on the _user_ of the crate, who much maintain
a build.rs script, and an additional function call overhead in the FFI, which is not guaranteed
to be inlined.

The alternative variant is "asm". In this variant, there is no build-time setup, and no FFI.
Instead, inline assembly is generated in the procedural macro. The details of this assembly
are beyond the scope of this README, but can be found in `dtrace-parser/src/parser/asm.rs`.
The benefits of this approach are less work by the crate consumer, who no longer needs to
maintain a build script or link a static library. There is additionally no function call
overhead. However, this approach requires a nightly compiler due to its use of inline ASM.
This variant is opted into with the `"asm"` feature, e.g., `cargo +nightly build --features asm`.

> Important: The "asm" variant is in development, and is not currently plumbed all the way
through to DTrace.

The `probe-test` crate in this project implements a complete example which shows both of
these variants.

## Example

The `probe-test` binary crate in this package implements a complete example.

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

In the "static library" variant of the project, connecting these probes with Rust code
requires a build script, which at a minimum looks like:

```rust
use usdt::build_providers;

fn main() {
	build_providers("test.d").unwrap();
}
```

This generates the C-side of the FFI, and compiles and links it into your crate as a
static library. This step is unnecessary in the "asm" variant of the project.

Using the probes in Rust code looks like the following, which is in `probe-test/src/main.rs`.

```rust
/// An example using the `usdt` crate, in both static library and asm variants.

// If we're using the assembly variant of the package, the actual ASM feature must be opted into.
// This statement is behind `cfg_attr` to easily support both variants, but if only the ASM variant
// is desired, this may be simplified to `#![feature(asm)]`.
#![cfg_attr(feature = "asm", feature(asm))]

use std::thread::sleep;
use std::time::Duration;

// Import the `dtrace_provider` procedural macro, which generates Rust code to call the probes
// defined in the given provider file.
use usdt::dtrace_provider;

// Call the macro, which generates a Rust macro for each probe in the provider.
dtrace_provider!("probe-test/test.d");

fn main() {
    let duration = Duration::from_secs(1);
    let mut counter: u8 = 0;
    loop {
        // Call the "start" probe which accepts a u8. In both the static library and ASM variants
        // of this crate, these arguments are type-checked at compile time.
        test_start!(counter);

        // Do some work.
        sleep(duration);

        // Call the "stop" probe, which accepts a &str and a u8.
        test_stop!("the probe has fired", counter);

        counter = counter.wrapping_add(1);
    }
}
```

We can see that this is hooked up with DTrace by running the example and listing the expected
probes by name.

```bash
$ cargo run
```

And in another terminal, list the matching probes with:

```bash
$ sudo dtrace -l -n test*:::
   ID   PROVIDER            MODULE                          FUNCTION NAME
 3011  test65946        probe-test                       _test_start start
 3012  test65946        probe-test                        _test_stop stop
 ```

## Installing `dusty`

The `dusty` executable can be installed for easier reference. At the time of
writing, this package doesn't exist on crates.io, but it can be installed
from Git via:

```bash
$ cargo install --git https://github.com/oxidecomputer/usdt usdt
```
