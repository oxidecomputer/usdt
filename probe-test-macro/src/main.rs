//! An example using the `usdt` crate, generating the probes via a procedural macro
#![feature(asm)]

use std::thread::sleep;
use std::time::Duration;

// Import the `dtrace_provider` procedural macro, which generates Rust code to call the probes
// defined in the given provider file.
use usdt::{dtrace_provider, register_probes};

// Call the macro, which generates a Rust macro for each probe in the provider.
dtrace_provider!("test.d");

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
