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

        // Call the "stop" probe, which accepts a string, u8, and string.
        test_stop!(|| (
            format!("the probe has fired {}", counter),
            counter,
            format!("{:x}", counter)
        ));

        counter = counter.wrapping_add(1);
    }
}
