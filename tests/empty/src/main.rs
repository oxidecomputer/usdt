#![feature(asm)]
#![deny(warnings)]

use usdt::register_probes;

include!(concat!(env!("OUT_DIR"), "/provider.rs"));

fn main() {
    register_probes().unwrap();

    let counter: u8 = 0;
    stuff_start!(|| (counter));
    stuff_stop!(|| ("the probe has fired", counter));
}

#[cfg(test)]
mod test {
    // We just want to make sure that main builds and runs.
    #[test]
    fn test_main() {
        super::main();
    }
}
