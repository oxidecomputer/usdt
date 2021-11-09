#![feature(asm)]
#![cfg_attr(target_os = "macos", feature(asm_sym))]
#![deny(warnings)]

fn main() {
    fake_lib::register_probes().unwrap();
    fake_lib::dummy();
}

#[cfg(test)]
mod test {
    // We just want to make sure that main builds and runs.
    #[test]
    fn test_main() {
        super::main();
    }
}
