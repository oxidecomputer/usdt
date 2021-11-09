#![feature(asm)]
#![cfg_attr(target_os = "macos", feature(asm_sym))]
#![deny(warnings)]

use usdt::register_probes;

include!(concat!(env!("OUT_DIR"), "/test.rs"));

fn main() {
    register_probes().unwrap();

    zero::here__i__am!(|| ());
    zero::here__i__am!();
}

#[cfg(test)]
mod test {
    // We just want to make sure that main builds and runs.
    #[test]
    fn test_main() {
        super::main();
    }
}
