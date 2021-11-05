//! Test that a zero-argument probe is correctly type-checked

// Copyright 2021 Oxide Computer Company

#![feature(asm)]

#[usdt::provider]
mod my_provider {
    fn my_probe() {}
}

fn main() {
    my_provider::my_probe!(|| "This should fail");
}
