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

## TODO

- [ ] Write the C code that actually implements the shared probe function.
- [ ] The generated functions currently just print the function name. They should call
the above `extern "C"` function.
