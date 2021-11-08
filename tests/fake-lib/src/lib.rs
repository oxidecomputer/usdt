#![feature(asm)]
#![cfg_attr(target_os = "macos", feature(asm_sym))]
#![deny(warnings)]

pub use usdt::register_probes;

include!(concat!(env!("OUT_DIR"), "/test.rs"));

pub fn dummy() {
    test::here__i__am!();
    test::here__i__am!();
}
