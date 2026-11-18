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
    use std::process::Command;
    use usdt_tests_common::root_command;
    use usdt_tests_common::Tracer;
    use usdt_tests_common::BEGIN_SENTINEL;

    /// Build a DTrace command printing the JSON argument of the provided probe.
    #[cfg(not(target_os = "linux"))]
    fn tracer_command(probe_name: &str) -> Command {
        // Don't exit if the probe doesn't exist.
        let mut cmd = Command::new(root_command());
        cmd.arg("dtrace")
            .arg("-Z")
            .arg("-q")
            .arg("-n")
            .arg(format!(
                "test_json{}:::{} {{ printf(\"%s\", copyinstr(arg0)); exit(0); }}",
                std::process::id(),
                probe_name
            ))
            .arg("-n")
            .arg(format!("BEGIN {{ printf(\"{BEGIN_SENTINEL}\\n\"); }}"));
        cmd
    }

    /// Build a bpftrace command printing the JSON argument of the provided
    /// probe, attached to this process using its PID.
    #[cfg(target_os = "linux")]
    fn tracer_command(probe_name: &str) -> Command {
        // Note: bpftrace does not support multiple -e (program) arguments, so
        // both the test probe and the BEGIN probe are given in the same
        // program. The two probes are _space_ separated, not comma-separated,
        // otherwise they'll fail to parse.
        let mut cmd = Command::new(root_command());
        cmd.arg("bpftrace")
            .arg("-e")
            .arg(format!(
                "usdt:*:test_json:{} {{ printf(\"%s\\n\", str(arg0)); exit(); }} BEGIN {{ printf(\"{}\\n\"); }}",
                probe_name, BEGIN_SENTINEL
            ))
            .arg("-p")
            .arg(std::process::id().to_string());
        cmd
    }

    /// Start the tracer emitting the named probe, call `fire` once the tracer
    /// has started, and return the JSON the probe prints.
    fn trace_json(probe_name: &str, fire: impl FnOnce()) -> Value {
        let mut tracer = Tracer::spawn(tracer_command(probe_name));
        tracer.wait_for_sentinel();
        fire();
        let output = tracer.wait_for_exit();
        assert!(
            output.status.success(),
            "tracer process failed:\n{}",
            output.stderr,
        );
        println!("tracer output\n{}\n", output.stdout);
        serde_json::from_str(&output.stdout).unwrap()
    }

    #[test]
    fn test_json_support() {
        usdt::register_probes().unwrap();

        let json = trace_json("good", || {
            let data = ProbeArg::default();
            test_json::good!(|| &data);
        });
        assert!(json.get("ok").is_some());
        assert!(json.get("err").is_none());
        assert_eq!(json["ok"]["value"], Value::from(1));
        assert_eq!(json["ok"]["buffer"], Value::from(vec![1, 2, 3]));

        let json = trace_json("bad", || {
            let data = NotJsonSerializable::default();
            test_json::bad!(|| &data);
        });
        assert!(json.get("ok").is_none());
        assert!(json.get("err").is_some());
        assert_eq!(json["err"], Value::from(SERIALIZATION_ERROR));
    }
}
