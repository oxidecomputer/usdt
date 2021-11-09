#![feature(asm)]
#![cfg_attr(target_os = "macos", feature(asm_sym))]

usdt::dtrace_provider!("../../../tests/compile-errors/providers/type-mismatch.d");

fn main() {
    let bad: f32 = 0.0;
    mismatch::bad!(|| (bad));
}
