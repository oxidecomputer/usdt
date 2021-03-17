//! An example using the `usdt` crate, generating the probes via a build script.
#![feature(asm)]

mod test;

use usdt::register_probes;

// Include the Rust implementation generated by the build script.
include!(concat!(env!("OUT_DIR"), "/test.rs"));

fn main() {
    register_probes().unwrap();

    let counter: u8 = 0;

    test_start!(|| (counter));

    test_stop!(|| ("the probe has fired", counter));
}
