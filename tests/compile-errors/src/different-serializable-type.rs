//! Test that passing a type that is serializable, but not the same concrete type as the probe
//! signature, fails compilation.

// Copyright 2021 Oxide Computer Company

#![feature(asm)]
#![cfg_attr(target_os = "macos", feature(asm_sym))]

#[derive(serde::Serialize)]
struct Expected {
    x: u8
}

#[derive(serde::Serialize)]
struct Different {
    x: u8
}

#[usdt::provider]
mod my_provider {
    use crate::Expected;
    fn my_probe(_: Expected) {}
}

fn main() {
    my_provider::my_probe!(|| Different { x: 0 });
}
