//! Small example which tests that this whole thing works. Specifically, this constructs and
//! registers a single probe with arguments, and then verifies that this probe is visible to the
//! `dtrace(1)` command-line tool.
// Copyright 2021 Oxide Computer Company

#![feature(asm)]

use usdt::register_probes;

include!(concat!(env!("OUT_DIR"), "/test.rs"));

fn main() {
    doesit_work!(|| (0, "something"));
}

#[allow(dead_code)]
fn run_test(rx: std::sync::mpsc::Receiver<()>) {
    register_probes().unwrap();
    doesit_work!(|| (0, "something"));
    let _ = rx.recv();
}

#[cfg(test)]
mod tests {
    use super::run_test;
    use std::process::{Command, Stdio};
    use std::sync::mpsc::channel;
    use std::thread;

    #[test]
    fn test_does_it_work() {
        let (send, recv) = channel();
        let thr = thread::spawn(move || run_test(recv));
        let dtrace = Command::new("sudo")
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

        let function = parts.next().expect("Expected a function name");
        assert!(
            function.contains("[does_it_work::run_test::"),
            "Function name appears incorrect: {}",
            function
        );

        let probe = parts.next().expect("Expected a probe name");
        assert_eq!(probe, "work", "Probe name appears incorrect: {}", probe);

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
