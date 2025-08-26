//! Small example which tests that this whole thing works. Specifically, this constructs and
//! registers a single probe with arguments, and then verifies that this probe is visible to the
//! `dtrace(1)` command-line tool.

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

#![allow(non_snake_case)]

use usdt::register_probes;

include!(concat!(env!("OUT_DIR"), "/test.rs"));

fn main() {
    does__it::work!(|| (0, "something"));
}

// Dissuade the compiler from inlining this, which would ruin the test for `probefunc`.
#[inline(never)]
#[allow(dead_code)]
fn run_test(rx: std::sync::mpsc::Receiver<()>) {
    register_probes().unwrap();
    does__it::work!(|| (0, "something"));
    let _ = rx.recv();
}

#[cfg(test)]
mod tests {
    use super::run_test;

    #[cfg(not(target_os = "linux"))]
    mod dtrace {
        use super::run_test;
        use std::process::Stdio;
        use std::sync::mpsc::channel;
        use std::thread;

        #[test]
        fn test_does_it_work() {
            use usdt_tests_common::root_command;
            let (send, recv) = channel();
            let thr = thread::spawn(move || run_test(recv));
            let dtrace = std::process::Command::new(root_command())
                .arg("dtrace")
                .arg("-l")
                .arg("-v")
                .arg("-n")
                .arg("does__it*:::")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .expect("Could not start DTrace");
            let output = dtrace
                .wait_with_output()
                .expect("Failed to read DTrace stdout");

            // Kill the test thread
            let _ = send.send(());

            // Collect the actual output
            let output = String::from_utf8_lossy(&output.stdout);
            println!("{}", output);

            // Check the line giving the full description of the probe
            let mut lines = output.lines().skip_while(|line| !line.contains("does__it"));
            let line = lines
                .next()
                .expect("Expected a line containing the provider name");
            let mut parts = line.split_whitespace();
            let _ = parts.next().expect("Expected an ID");

            let provider = parts.next().expect("Expected a provider name");
            assert!(
                provider.starts_with("does__it"),
                "Provider name appears incorrect: {}",
                provider
            );

            let module = parts.next().expect("Expected a module name");
            assert!(
                module.starts_with("does_it_work"),
                "Module name appears incorrect: {}",
                module
            );

            let mangled_function = parts.next().expect("Expected a mangled function name");
            assert!(
                mangled_function.contains("does_it_work8run_test"),
                "Mangled function name appears incorrect: {}",
                mangled_function
            );

            // Verify the argument types
            let mut lines = lines.skip_while(|line| !line.contains("args[0]"));
            let first = lines
                .next()
                .expect("Expected a line with the argument description")
                .trim();
            assert_eq!(
                first, "args[0]: uint8_t",
                "Argument is incorrect: {}",
                first
            );
            let second = lines
                .next()
                .expect("Expected a line with the argument description")
                .trim();
            assert_eq!(
                second, "args[1]: char *",
                "Argument is incorrect: {}",
                second
            );

            thr.join().expect("Failed to join test runner thread");
        }
    }

    #[cfg(target_os = "linux")]
    mod stap {
        use super::run_test;
        use std::process::Stdio;
        use std::sync::mpsc::channel;
        use std::thread;

        #[test]
        fn test_does_it_work() {
            // Note: other stap tests use bpftrace, but here we use readelf.
            // The reason for this is that while bpftrace can be used to read
            // out USDTs in processes, it does not print out their argument
            // types even with the verbose flag.
            // This is the readelf format we expect to see:
            // ```
            // Displaying notes found in: .note.stapsdt
            //   Owner                Data size        Description
            //   stapsdt              0x00000034       NT_STAPSDT (SystemTap probe descriptors)
            //     Provider: does__it
            //     Name: work
            //     Location: 0x00000000000618b4, Base: 0x0000000000000000, Semaphore: 0x000000000011ccc8
            //     Arguments: 1@%dil 8@%rsi
            // ```
            let (send, recv) = channel();
            let thr = thread::spawn(move || run_test(recv));
            let test_exe = std::env::current_exe().unwrap();
            let readelf = std::process::Command::new("readelf")
                .arg("-n")
                .arg(&test_exe)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .expect("Could not start readelf");
            let output = readelf
                .wait_with_output()
                .expect("Failed to read readelf stdout");

            // Kill the test thread
            let _ = send.send(());

            // Collect the actual output
            let output = String::from_utf8_lossy(&output.stdout);
            println!("{}", output);

            // Skip lines until we find the first line of interest.
            let mut lines = output.lines().skip_while(|line| !line.contains("does__it"));
            // "Provider: does__it"
            let line = lines
                .next()
                .expect("Expected a line containing the provider name");
            let line = line.trim();
            assert_eq!(
                line, "Provider: does__it",
                "Provider name line appears incorrect: {}",
                line
            );

            // "Name: work"
            let line = lines
                .next()
                .expect("Expected a line containing the probe name");
            let line = line.trim();
            assert_eq!(
                line, "Name: work",
                "Probe name line appears incorrect: {}",
                line
            );

            // "Location: 0x00001234, Base: 0x0001234, Semaphore: 0x0001234"
            let line = lines.next().expect("Expected an addresses line");
            let mut parts = line.trim().split_whitespace();
            assert_eq!(
                parts.next().expect("Expected a 'Location:' text"),
                "Location:",
                "Addresses line appears incorrect: {}",
                line
            );
            let location_address = parts.next().expect("Expected a location address");
            assert!(
                location_address.starts_with("0x")
                    && location_address.ends_with(",")
                    && usize::from_str_radix(&location_address[2..location_address.len() - 1], 16)
                        .is_ok(),
                "Location address appears incorrect: {}",
                location_address
            );
            assert_eq!(
                parts.next().expect("Expected a 'Base:' text"),
                "Base:",
                "Addresses line appears incorrect: {}",
                line
            );
            let base_address = parts.next().expect("Expected a base address");
            assert!(
                base_address.starts_with("0x")
                    && base_address.ends_with(",")
                    && usize::from_str_radix(&base_address[2..base_address.len() - 1], 16).is_ok(),
                "Base address appears incorrect: {}",
                base_address
            );
            assert_eq!(
                parts.next().expect("Expected a 'Semaphore:' text"),
                "Semaphore:",
                "Addresses line appears incorrect: {}",
                line
            );
            let semaphore_address = parts.next().expect("Expected a semaphore address");
            assert!(
                semaphore_address.starts_with("0x")
                    && usize::from_str_radix(&semaphore_address[2..], 16).is_ok(),
                "Semaphore address appears incorrect: {}",
                semaphore_address
            );

            // Verify the argument types
            let line = lines.next().expect("Expected a line containing arguments");
            let line = line.trim();
            assert_eq!(
                line, "Arguments: 1@%dil 8@%rsi",
                "Arguments line appears incorrect: {}",
                line
            );

            thr.join().expect("Failed to join test runner thread");
        }
    }
}
