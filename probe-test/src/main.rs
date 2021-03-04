//! An example using the `usdt` crate, in both static library and asm variants.
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
