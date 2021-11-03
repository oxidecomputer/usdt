#![feature(asm)]
#![deny(warnings)]

pub use usdt::register_probes;

include!(concat!(env!("OUT_DIR"), "/test.rs"));

pub fn dummy() {
    test::here__i__am!();
    test::here__i__am!();
}
