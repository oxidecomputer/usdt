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

#![cfg_attr(usdt_need_feat_asm, feature(asm))]
#![cfg_attr(usdt_need_feat_asm_sym, feature(asm_sym))]

use usdt::register_probes;

include!(concat!(env!("OUT_DIR"), "/test.rs"));

fn main() {
    doesit::work!(|| (0, "something"));
}

// Dissuade the compiler from inlining this, which would ruin the test for `probefunc`.
#[inline(never)]
#[allow(dead_code)]
fn run_test(rx: std::sync::mpsc::Receiver<()>) {
    register_probes().unwrap();
    doesit::work!(|| (0, "something"));
    let _ = rx.recv();
}

#[cfg(test)]
mod tests {
    use super::run_test;
    use std::process::Stdio;
    use std::sync::mpsc::channel;
    use std::thread;
    use usdt_tests_common::root_command;

    #[test]
    fn test_does_it_work() {
        let (send, recv) = channel();
        let thr = thread::spawn(move || run_test(recv));
        let dtrace = std::process::Command::new(root_command())
            .arg("dtrace")
            .arg("-l")
            .arg("-v")
            .arg("-n")
            .arg("doesit*:::")
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
        let mut lines = output.lines().skip_while(|line| !line.contains("doesit"));
        let line = lines
            .next()
            .expect("Expected a line containing the provider name");
        let mut parts = line.split_whitespace();
        let _ = parts.next().expect("Expected an ID");

        let provider = parts.next().expect("Expected a provider name");
        assert!(
            provider.starts_with("doesit"),
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
