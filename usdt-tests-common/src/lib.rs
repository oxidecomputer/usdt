// Copyright 2024 Oxide Computer Company
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

#[cfg(target_os = "illumos")]
pub fn root_command() -> String {
    // On illumos systems, we prefer pfexec(1) but allow some other command to
    // be specified through the environment.
    std::env::var("PFEXEC").unwrap_or_else(|_| "/usr/bin/pfexec".to_string())
}

#[cfg(not(target_os = "illumos"))]
pub fn root_command() -> String {
    String::from("sudo")
}
