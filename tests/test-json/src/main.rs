//! Integration test verifying JSON output, including when serialization fails.

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
    use std::time::Duration;
    use tokio::io::AsyncReadExt;
    use tokio::process::Child;
    use tokio::process::Command;
    use tokio::sync::mpsc::channel;
    use tokio::sync::mpsc::Receiver;
    use tokio::sync::mpsc::Sender;
    use tokio::time::Instant;
    use usdt_tests_common::root_command;

    // Maximum duration to wait for DTrace, controlling total test duration
    const MAX_WAIT: Duration = Duration::from_secs(30);

    // A sentinel printed by DTrace, so we know when it starts up successfully.
    const BEGIN_SENTINEL: &str = "BEGIN";

    // Fire the test probes in sequence, when a notification is received on the channel.
    async fn fire_test_probes(mut recv: Receiver<()>) {
        usdt::register_probes().unwrap();

        // Wait for notification from the main thread
        println!("Test runner waiting for first notification");
        recv.recv().await.unwrap();
        println!("Test runner firing first probe");

        // Fire the good probe until the main thread signals us to continue.
        let data = ProbeArg::default();
        test_json::good!(|| &data);
        println!("Test runner fired first probe");
        println!("Test runner awaiting notification to continue");
        recv.recv().await.unwrap();
        println!("Test runner received notification to continue");

        // Fire the bad probe.
        println!("Test runner firing second probe");
        let data = NotJsonSerializable::default();
        test_json::bad!(|| &data);
        println!("Test runner fired second probe");
    }

    // Run DTrace as a subprocess, waiting for the JSON output of the provided probe.
    async fn run_dtrace_and_return_json(tx: &Sender<()>, probe_name: &str) -> Value {
        // Start the DTrace subprocess, and don't exit if the probe doesn't exist.
        let mut dtrace = Command::new(root_command())
            .arg("dtrace")
            .arg("-Z")
            .arg("-q")
            .arg("-n")
            // The test probe we're interested in listening for.
            .arg(format!(
                "test_json{}:::{} {{ printf(\"%s\", copyinstr(arg0)); exit(0); }}",
                std::process::id(),
                probe_name
            ))
            .arg("-n")
            // An output printed by DTrace when it starts, to coordinate with the test thread
            // firing the probe itself.
            .arg(format!("BEGIN {{ trace(\"{}\"); }}", BEGIN_SENTINEL))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("Failed to spawn DTrace subprocess");

        // Wait for DTrace to correctly start up before notifying the test thread to start
        // firing probes.
        let now = Instant::now();
        wait_for_begin_sentinel(&mut dtrace, &now).await;

        // We should now see the probe having fired exactly once. Grab the output data, kill the
        // DTrace process, and then parse the output as JSON.
        tx.send(()).await.unwrap();

        // Wait for the process to finish, up to a pretty generous limit.
        let output = tokio::time::timeout_at(now + MAX_WAIT, dtrace.wait_with_output())
            .await
            .expect(&format!("DTrace did not complete within {:?}", MAX_WAIT))
            .expect("Failed to wait for DTrace subprocess");
        assert!(
            output.status.success(),
            "DTrace process failed:\n{:?}",
            String::from_utf8_lossy(&output.stderr),
        );
        let stdout = std::str::from_utf8(&output.stdout).expect("Non-UTF8 stdout");
        println!("DTrace output\n{}\n", stdout);
        let json: Value = serde_json::from_str(&stdout).unwrap();
        json
    }

    // Check DTrace subprocess stdout for the begin sentinel, telling us the program has spawned
    // successfully.
    async fn wait_for_begin_sentinel(dtrace: &mut Child, now: &Instant) {
        let mut output = String::new();
        let stdout = dtrace.stdout.as_mut().expect("Expected piped stdout");
        let max_time = *now + MAX_WAIT;

        // Try to read data from stdout, up to the maximum wait time. This may take multiple reads,
        // though it's pretty unlikely.
        while now.elapsed() < MAX_WAIT {
            let read_task = tokio::time::timeout_at(max_time, async {
                let mut bytes = vec![0; 128];
                stdout.read(&mut bytes).await.map(|_| bytes)
            });
            match read_task.await {
                Ok(read_result) => {
                    let chunk = read_result.expect("Failed to read DTrace stdout");
                    output.push_str(std::str::from_utf8(&chunk).expect("Non-UTF8 stdout"));
                    if output.contains(BEGIN_SENTINEL) {
                        println!("DTrace started up successfully");
                        return;
                    }
                }
                _ => {}
            }
            println!("DTrace not yet ready");
            continue;
        }
        panic!("DTrace failed to startup within {:?}", MAX_WAIT);
    }

    #[tokio::test]
    async fn test_json_support() {
        let (tx, rx) = channel(4);
        let test_task = tokio::task::spawn(fire_test_probes(rx));

        let json = run_dtrace_and_return_json(&tx, "good").await;
        assert!(json.get("ok").is_some());
        assert!(json.get("err").is_none());
        assert_eq!(json["ok"]["value"], Value::from(1));
        assert_eq!(json["ok"]["buffer"], Value::from(vec![1, 2, 3]));

        // Tell the thread to continue with the bad probe
        let json = run_dtrace_and_return_json(&tx, "bad").await;
        assert!(json.get("ok").is_none());
        assert!(json.get("err").is_some());
        assert_eq!(json["err"], Value::from(SERIALIZATION_ERROR));

        test_task.await.unwrap();
    }
}
