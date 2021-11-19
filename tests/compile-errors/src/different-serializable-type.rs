//! Test that passing a type that is serializable, but not the same concrete type as the probe
//! signature, fails compilation.

// Copyright 2021 Oxide Computer Company
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
