//! An example using the `usdt` crate, generating the probes via a build script.

// Copyright 2022 Oxide Computer Company
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

#![cfg_attr(not(usdt_stable_asm), feature(asm))]
#![cfg_attr(not(usdt_stable_asm_sym), feature(asm_sym))]

use std::thread::sleep;
use std::time::Duration;

use usdt::register_probes;

// Include the Rust implementation generated by the build script.
include!(concat!(env!("OUT_DIR"), "/test.rs"));

fn main() {
    let duration = Duration::from_secs(1);
    let mut counter: u8 = 0;

    // NOTE: One _must_ call this function in order to actually register the probes with DTrace.
    // Without this, it won't be possible to list, enable, or see the probes via `dtrace(1)`.
    register_probes().unwrap();

    loop {
        // Call the "start_work" probe which accepts a u8.
        test::start_work!(|| (counter));

        // Do some work.
        sleep(duration);

        // Call the "stop_work" probe, which accepts a string, u8, and string.
        test::stop_work!(|| (
            format!("the probe has fired {}", counter),
            counter,
            format!("{:x}", counter)
        ));

        counter = counter.wrapping_add(1);
    }
}
