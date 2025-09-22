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

#![deny(warnings)]

use usdt::register_probes;

include!(concat!(env!("OUT_DIR"), "/provider.rs"));

fn main() {
    register_probes().unwrap();

    let counter: u8 = 0;
    stuff::start_work!(|| counter);
    stuff::stop_work!(|| ("the probe has fired", counter));
    stuff::noargs!(|| ());
    stuff::noargs!();
}

#[cfg(test)]
mod test {
    // We just want to make sure that main builds and runs.
    #[test]
    fn test_main() {
        super::main();
    }
}
