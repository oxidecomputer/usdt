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

mod inner;

fn main() {
    usdt::register_probes().expect("Could not register probes");
    // Verify that we can call the probe from its full path.
    inner::probes::am_i_visible!(|| ());

    // This is an overly-cautious test for macOS. We define extern symbols inside each expanded
    // probe macro, with a link-name for a symbol that the macOS linker will generate for us. This
    // checks that there is no issue defining these locally-scoped extern symbols multiple times.
    inner::probes::am_i_visible!(|| ());
}
