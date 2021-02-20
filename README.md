# `usdt`

Dust your Rust with USDT probes.

## Overview

`usdt` exposes statically-defined DTrace probes to Rust code. Users write a provider definition
as usual, in a D language script, and use the `dtrace_provider!` macro to generate Rust code
that defines probe points. One then fires the probe by calling a normal Rust function.

## Example

Consider a provider definition, in a file `"provider.d"`.

```d
provider foo {
	probe bar(uint8_t);
};
```

One can then load this provider into Rust and generate functions that correspond to the defined
probe(s). For example:

```rust
use usdt::dtrace_provider;

dtrace_provider!("provider.d");

fn main() {
	let x: u8 = 10;
	foo::bar(x); // Fire the probe
}
```

This all currently relies on a native static library, generated from a build script, which must
be linked into any final artifact. This linking itself happens automatically, but users must
currently tell Cargo where to find the library. It is located in `./target/native`, relative to
this project's root.

So to build the example that comes with this package, run:

```bash
$ cargo build
$ RUSTFLAGS="-L ./target/native" cargo run --example simple
```

To build an out-of-tree application or library, do so with:

```bash
$ RUSTFLAGS="-L /path/to/usdt/target/native" cargo build
```

Hopefully this build process will be smoothed out in the future.
