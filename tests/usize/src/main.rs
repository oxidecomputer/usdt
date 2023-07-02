//! Integration test verifying that we correctly compile usize/isize probes.

// Copyright 2023 Oxide Computer Company
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

#![cfg_attr(usdt_need_feat_asm, feature(asm))]
#![cfg_attr(usdt_need_feat_asm_sym, feature(asm_sym))]

#[usdt::provider]
mod usize__test {
    fn emit_usize(_: usize) {}
    fn emit_isize(_: &isize) {}
    fn emit_u8(_: u8) {}
}

fn main() {
    usdt::register_probes().unwrap();
    usize__test::emit_usize!(|| 1usize);
    usize__test::emit_isize!(|| &1isize);
    usize__test::emit_u8!(|| 1);
}
