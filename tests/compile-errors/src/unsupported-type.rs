#![feature(asm)]
#![cfg_attr(target_os = "macos", feature(asm_sym))]

usdt::dtrace_provider!("../../../tests/compile-errors/providers/unsupported-type.d");

fn main() {
    let bad: u8 = 0;
    unsupported::bad!(|| (bad));
}
