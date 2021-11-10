//! Test verifying that renaming the provider/probes in various ways works when using a build
//! script.

// Copyright 2021 Oxide Computer Company

#![feature(asm)]
#![cfg_attr(target_os = "macos", feature(asm_sym))]
include!(concat!(env!("OUT_DIR"), "/test.rs"));

fn main() {
    usdt::register_probes().unwrap();

    // Renamed the module that the probes are generated to `still_test`. So naming them as
    // `test::start_work` will fail.
    still_test::start_work!(|| 0);
}
