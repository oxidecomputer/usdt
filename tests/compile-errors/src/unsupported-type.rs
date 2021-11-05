#![feature(asm)]

usdt::dtrace_provider!("../../../tests/compile-errors/providers/unsupported-type.d");

fn main() {
    let bad: u8 = 0;
    unsupported::bad!(|| (bad));
}
