# `usdt`

Dust your Rust with USDT probes.

## Overview

`usdt` exposes statically-defined DTrace probes to Rust code. Users write a provider definition
as usual, in a D language script. This is used to generate a static library and Rust code, which
together provide the glue to call DTrace probe functions from Rust.

## Example

The `probe-test` binary crate in this package implements a complete example.

The starting point is a D script, called `"test.d"`. It looks like:

```d
provider test {
	probe start();
	probe stop(float);
};
```

This script defines a single provider, `test`, with two probes, `start` and `stop`,
with a different set of arguments. (Numeric primitive types and `Strings` are currently
supported.) The goal of this example is to describe how we can call those functions,
and thus fire DTrace probes, from Rust code.

Calling the probes is done via C foreign-function interface (FFI) in Rust. We begin
building a static library from C code, which defines C functions that fire the DTrace
probes. We can then link this static library with the Rust code, and call the C functions
through standard FFI mechanisms. Most of this process is entirely rote, and can be
done in a [build script](https://doc.rust-lang.org/cargo/reference/build-scripts.html).
This will generate the C code and compile it into a static library before building
one's crate. However, this build script must be generated from your provider definition
file. As long as the provider file is not _renamed_, this manual step need only be
done once. The library will be created at build time, and will re-run if your provider
file changes.

The binary in the main `usdt` crate, called `dusty`, can be used to generate this
build script from the provider definition. `dusty` is the only binary in the project,
so it can be invoked from the project root directory with:

```bash
$ cargo run -- buildgen probe-test/test.d > probe-test/build.rs
```

> NOTE: One can also install the `dusty` binary as a standalone executable. This
is recommended to avoid requiring a local copy of this entire project, in addition
to the dependency in Cargo.toml. See below for details.

This generates C code that calls the DTrace probes, and spits out a build script
at `probe-test/build.rs`. These together implement the C-side of the FFI.

The Rust side of the FFI is implemented with the `usdt::dtrace_provider!` macro.
Given the path to the same provider file, this generates Rust code that allows
calling into the generated C code. Note that for a variety of reasons, these
are Rust _macros_, and are named by `provider_probe!`. The file
`probe-test/src/main.rs` shows how to invoke this macro and use the generated
functions:

```rust
use std::thread::sleep;
use std::time::Duration;

use usdt::dtrace_provider;

dtrace_provider!("probe-test/test.d");

fn main() {
    let duration = Duration::from_secs(1);
    loop {
        test_start!();
        sleep(duration);
        test_stop!(1.0);
    }
}
```

The macros `test_{start,stop}` call out to the C functions via FFI,
which then call the DTrace probes themselves. This `probe-test` example
does an infinite loop, calling the two probes. One can verify that this
actually hooks up to DTrace correctly by running the example. From the
`probe-test` directory:

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

## A note about strings

This project supports sending strings into probes. Because these cross an FFI
boundary into C, they must be valid, null-terminated C strings. The generated
macros will convert passed string types (`String`, `&str`, and others) into
a `std::ffi::CString`. This will panic if the conversion fails, which means
it is currently the _caller's_ responsibility to make sure their string types
do not contain an intervening null byte.

## Installing `dusty`

The `dusty` executable can be installed for easier reference. At the time of
writing, this package doesn't exist on crates.io, but it can be installed
from Git via:

```bash
$ cargo install --git https://github.com/oxidecomputer/usdt usdt
```

The above process of generated the build script can then be invoked directly
in the `probe-test` directory. The `--file` option can be used to control
whether `dusty` emits the build script to the standard output (the default)
or directly to a file called `build.rs` in the current directory.

```bash
$ cd probe-test
$ dusty buildgen test.d --file
```

Note that you _cannot_ do:

```bash
$ dusty buildgen test.d > build.rs
```

Shell redirection causes the file `build.rs` to be created _before_ the
shell forks and execs the `cargo` command. So when `cargo` runs, it dutifully
tries to run a build step using this new (and empty) `build.rs`. That obviously
fails, so use the `--file` option instead.
