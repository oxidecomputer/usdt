//! Small example which tests that this whole thing works. Specifically, this constructs and
//! registers a single probe with arguments, and then verifies that this probe is visible to the
//! `dtrace(1)` command-line tool.
// Copyright 2021 Oxide Computer Company

#![feature(asm)]
#![cfg_attr(target_os = "macos", feature(asm_sym))]

use usdt::register_probes;

include!(concat!(env!("OUT_DIR"), "/test.rs"));

fn main() {
    doesit::work!(|| (0, "something"));
}

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
        let dtrace = root_command("dtrace")
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
