#![feature(asm)]
#![cfg_attr(target_os = "macos", feature(asm_sym))]

usdt::dtrace_provider!("../../../tests/compile-errors/providers/type-mismatch.d");

fn main() {
    let arg: u8 = 0;
    mismatch::bad!(arg);
}
