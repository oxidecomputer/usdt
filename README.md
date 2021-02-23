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
	probe stop();
};
```

This package requires calling those functions from Rust, which is done via FFI.
This requires building a static library and linking it with the Rust code. Most
of this rote, and can be automated with a build script.

The binary in the main `usdt` crate can be used to generate a build script
from the provider definition. From the `probe-test` directory, run:

```bash
$ cargo run --bin dusty -- buildgen --emit file test.d
```

> NOTE: One can install the `dusty` binary as a standalone executable. This
is recommended to avoid requiring a local copy of this entire project, in addition
to the dependency in Cargo.toml. See below for details.

This generates C code that calls the DTrace probes, and spits out a build script
at `probe-test/build.rs`. These together implement the C-side of the FFI.

The Rust side of the FFI is implemented with the `usdt::dtrace_provider!` macro.
Given the path to the same provider file, this generates Rust functions that
call into the generated C code. These are normal Rust functions, and are named
by `provider::probe`. The file `probe-test/src/main.rs` shows how to invoke
this macro and use the generated functions:

```rust
use std::thread::sleep;
use std::time::Duration;

use usdt::dtrace_provider;

dtrace_provider!("probe-test/test.d");

fn main() {
    let duration = Duration::from_secs(1);
    loop {
        test::start();
        sleep(duration);
        test::stop(1.0);
    }
}
```

The functions `test::{start,stop}` call out to the C functions via FFI,
which then call the DTrace probes themselves. This `probe-test` example
does an infinite loop, calling the two probes. One can verify that this
actually hooks up to DTrace correctly by running the example. From the
`probe-test` directory:

```bash
$ cargo run
```

And in another terminal:

```bash
$ sudo dtrace -n test*:::
```

The output should look similar to:

```bash
dtrace: description 'test*:::' matched 2 probes
CPU     ID                    FUNCTION:NAME
  6   2373                        stop:stop
  6   2372                      start:start
  1   2373                        stop:stop
  1   2372                      start:start
  7   2373                        stop:stop
  7   2372                      start:start
  6   2373                        stop:stop
  6   2372                      start:start
  6   2373                        stop:stop
  6   2372                      start:start
  4   2373                        stop:stop
  4   2372                      start:start
```

## Installing `dusty`

The `dusty` executable can be installed for easier reference. At the time of writing, this
package doesn't exist on crates.io, but it can be installed from Git via:

```bash
$ cargo install --git https://github.com/oxidecomputer/usdt usdt
```
