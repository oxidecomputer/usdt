//! Integration test verifying that provider modules are renamed correctly.

// Copyright 2021 Oxide Computer Company

#![feature(asm)]
#![cfg_attr(target_os = "macos", feature(asm_sym))]

#[usdt::provider(provider = "something", probe_format = "probe_{probe}")]
mod probes {
    fn something() {}
}

fn main() {
    usdt::register_probes().unwrap();
    probes::probe_something!(|| ());
}
