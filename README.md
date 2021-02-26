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
	probe start(uint8_t);
	probe stop(char*, uint8_t);
};
```

This script defines a single provider, `test`, with two probes, `start` and `stop`,
with a different set of arguments. (Numeric primitive types and `Strings` are currently
supported.) Connecting these probes with Rust code requires a build script, which at a
minimum looks like:

```rust
use usdt::build_providers;

fn main() {
	build_providers("test.d").unwrap();
}
```

The will compile and link a C library which provides a foreign function interface (FFI)
from which our Rust code may fire the probes. These Rust side of the FFI is generated
by placing the `usdt::dtrace_provider!` macro in the crate from which the probes are
called. The `probe-test` example looks like this:

```rust
use std::thread::sleep;
use std::time::Duration;

use usdt::dtrace_provider;

dtrace_provider!("probe-test/test.d");

fn main() {
    let duration = Duration::from_secs(1);
    let mut counter: u8 = 0;
    loop {
        test_start!(counter);
        sleep(duration);
        test_stop!("the probe has fired", counter);
        counter = counter.wrapping_add(1);
    }
}
```

The macros `test_{start,stop}` (or generally `<provider>_<probe>`), call out via FFI
to the DTrace probes themselves. This `probe-test` example does an infinite loop,
calling the two probes. One can verify that this actually hooks up to DTrace correctly
by running the example. From the `probe-test` directory:

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

## Implementation details

Calling the probes is done via C foreign-function interface (FFI) in Rust. This requires
building a static library from C code, which defines C functions that fire the DTrace
probes. We can then link this static library with the Rust code, and call the C functions
through standard FFI mechanisms. Most of this process is entirely rote, and can be
done automatically. The library will be created at build time, and will re-run if your
provider file changes. This is the purpose of the `build_providers` function.

The binary in the main `usdt` crate, called `dusty`, can be used to inspect the emitted
code. It can be run with

```bash
$ cargo run -p usdt -- probe-test/test.d
```

This displays the Rust half of the FFI that calls the DTrace probes. One can also display
the C declaration or definition with `-f decl` or `-f defn` respectively.

## A note about strings

This project supports sending strings into probes. Because these cross an FFI
boundary into C, they must be valid, null-terminated C strings. The generated
macros do some trickery to make sure that (1) the string data sent to the probe
is valid, and (2) avoid unnecessary overhead.

The approach is to internally send both the string data and its length to the
FFI function, and then, if the probe is enabled, copy the required number of
bytes into a local string. This requires an allocation, but only when the probe
is enabled.

## Installing `dusty`

The `dusty` executable can be installed for easier reference. At the time of
writing, this package doesn't exist on crates.io, but it can be installed
from Git via:

```bash
$ cargo install --git https://github.com/oxidecomputer/usdt usdt
```
