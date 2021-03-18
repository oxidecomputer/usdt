#![feature(asm)]

usdt::dtrace_provider!("../../../tests/compile-errors/providers/type-mismatch.d");

fn main() {
    let arg: u8 = 0;
    mismatch_bad!(arg);
}
