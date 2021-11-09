#![feature(asm)]
#![cfg_attr(target_os = "macos", feature(asm_sym))]

usdt::dtrace_provider!("non-existent.d");

fn main() { }
