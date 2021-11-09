//! Test that we can't name types into the provider module using a relative import

// Copyright 2021 Oxide Computer Company

#![feature(asm)]
#![cfg_attr(target_os = "macos", feature(asm_sym))]

#[derive(serde::Serialize)]
struct Expected {
    x: u8
}

#[usdt::provider]
mod my_provider {
    use super::Expected;
    fn my_probe(_: Expected) {}
}

fn main() {
    my_provider::my_probe!(|| Different { x: 0 });
}
