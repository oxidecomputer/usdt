//! Integration test verifying JSON output, including when serialization fails.

// Copyright 2021 Oxide Computer Company

#![feature(asm)]
#![cfg_attr(target_os = "macos", feature(asm_sym))]

use serde::{Serialize, Serializer};

// Expected error message from serialization failure
const SERIALIZATION_ERROR: &str = "nonono";

#[derive(Debug, Serialize)]
pub struct ProbeArg {
    value: u8,
    buffer: Vec<i64>,
}

impl Default for ProbeArg {
    fn default() -> Self {
        ProbeArg {
            value: 1,
            buffer: vec![1, 2, 3],
        }
    }
}

// A type that intentionally fails serialization
#[derive(Debug, Default)]
pub struct NotJsonSerializable {
    _x: u8,
}

impl Serialize for NotJsonSerializable {
    fn serialize<S: Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom(SERIALIZATION_ERROR))
    }
}

#[usdt::provider]
mod test_json {
    use crate::{NotJsonSerializable, ProbeArg as GoodArg};
    fn good(_: &GoodArg) {}
    fn bad(_: &NotJsonSerializable) {}
}

fn main() {
    usdt::register_probes().unwrap();
    let arg = ProbeArg::default();
    test_json::good!(|| &arg);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::process::Stdio;
    use std::sync::mpsc::Sender;
    use std::sync::mpsc::{channel, Receiver};
    use std::thread::sleep;
    use std::time::Duration;
    use std::time::Instant;
    use usdt_tests_common::root_command;

    // Duration the thread firing probes waits after receiving a notification.
    //
    // This is required to make sure DTrace is "ready" to receive the probe, which takes a bit of time
    // after the process itself starts.
    const SLEEP_DURATION: Duration = Duration::from_secs(1);

    // Maximum duration to wait for DTrace, controlling total test duration
    const MAX_WAIT: Duration = Duration::from_secs(30);

    fn run_test(recv: Receiver<()>) {
        usdt::register_probes().unwrap();

        // Wait for notification from the main thread
        let _ = recv.recv().unwrap();
        sleep(SLEEP_DURATION);
        println!("Test runner firing first probe");

        // Fire the good probe until the main thread signals us to continue.
        let data = ProbeArg::default();
        test_json::good!(|| &data);
        println!("Test runner awaiting notification");
        let _ = recv.recv().unwrap();

        // Fire the bad probe.
        sleep(SLEEP_DURATION);
        println!("Test runner firing second probe");
        let data = NotJsonSerializable::default();
        test_json::bad!(|| &data);
    }

    #[test]
    fn test_json_support() {
        let (tx, rx) = channel();
        let test_thread = std::thread::spawn(|| run_test(rx));

        fn run_dtrace_and_return_json(tx: &Sender<()>, probe_name: &str) -> Value {
            // Start the DTrace subprocess, and don't exit if the probe doesn't exist.
            let mut dtrace = root_command("dtrace")
                .arg("-Z")
                .arg("-q")
                .arg("-n")
                .arg(format!(
                    "test_json{}:::{} {{ printf(\"%s\", copyinstr(arg0)); exit(0); }}",
                    std::process::id(),
                    probe_name
                ))
                .stdin(Stdio::piped())
                .stderr(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            // We should now see the probe having fired exactly once. Grab the output data, kill the
            // DTrace process, and then parse the output as JSON.
            tx.send(()).unwrap();

            // Wait for the process to finish, up to a pretty generous limit.
            let now = Instant::now();
            while matches!(dtrace.try_wait(), Ok(None)) && now.elapsed() < MAX_WAIT {
                println!("DTrace still running");
                sleep(SLEEP_DURATION);
            }
            assert!(
                now.elapsed() < MAX_WAIT,
                "DTrace did not complete within {:?}",
                MAX_WAIT
            );
            let output = dtrace.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "DTrace process failed:\n{}",
                String::from_utf8(output.stderr).unwrap()
            );
            println!("DTrace output\n\n{:#?}", output);
            let json: Value = serde_json::from_slice(&output.stdout).unwrap();
            json
        }

        let json = run_dtrace_and_return_json(&tx, "good");
        assert!(json.get("ok").is_some());
        assert!(json.get("err").is_none());
        assert_eq!(json["ok"]["value"], Value::from(1));
        assert_eq!(json["ok"]["buffer"], Value::from(vec![1, 2, 3]));

        // Tell the thread to continue with the bad probe
        let json = run_dtrace_and_return_json(&tx, "bad");
        assert!(json.get("ok").is_none());
        assert!(json.get("err").is_some());
        assert_eq!(json["err"], Value::from(SERIALIZATION_ERROR));

        test_thread.join().unwrap();
    }
}
