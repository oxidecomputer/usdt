//! Integration test for `usdt::UniqueId`

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

#[usdt::provider]
mod with_ids {
    use usdt::UniqueId;
    fn start_work(_: &UniqueId) {}
    fn waypoint_from_thread(_: &UniqueId, message: &str) {}
    fn work_finished(_: &UniqueId, result: u64) {}
}

fn main() {}

#[cfg(test)]
mod tests {
    use super::with_ids;
    use std::process::Command;
    use std::thread;
    use std::time::Duration;
    use usdt::UniqueId;
    use usdt_tests_common::root_command;
    use usdt_tests_common::Tracer;
    use usdt_tests_common::BEGIN_SENTINEL;

    #[cfg(not(target_os = "linux"))]
    fn tracer_command() -> Command {
        let mut cmd = Command::new(root_command());
        cmd.arg("/usr/sbin/dtrace")
            .arg("-q")
            .arg("-n")
            .arg(r#"with_ids*:::waypoint_from_thread { printf("%u\n", arg0); exit(0); }"#)
            .arg("-n")
            .arg(format!(r#"BEGIN {{ printf("{BEGIN_SENTINEL}\n"); }}"#));
        cmd
    }

    #[cfg(target_os = "linux")]
    fn tracer_command() -> Command {
        // Note: bpftrace does not support multiple -e (program) arguments, so
        // both probes are given in the same program.
        let mut cmd = Command::new(root_command());
        cmd.arg("bpftrace")
            .arg("-q")
            .arg("-e")
            .arg(format!(
                concat!(
                    r#"usdt:*:with_ids:waypoint_from_thread {{ printf("%lu\n", arg0); exit(); }} "#,
                    r#"BEGIN {{ printf("{}\n"); }}"#,
                ),
                BEGIN_SENTINEL,
            ))
            .arg("-p")
            .arg(std::process::id().to_string());
        cmd
    }

    #[test]
    fn test_unique_ids() {
        usdt::register_probes().unwrap();
        let id = UniqueId::new();
        with_ids::start_work!(|| &id);
        let id2 = id.clone();
        let thr = thread::spawn(move || {
            for _ in 0..10 {
                with_ids::waypoint_from_thread!(|| (&id2, "we're in a thread"));
                thread::sleep(Duration::from_millis(10));
            }
            id2.as_u64()
        });
        let result = thr.join().unwrap();
        with_ids::work_finished!(|| (&id, result));
        assert_eq!(result, id.as_u64());

        // Actually verify that the same value is received by the tracer, once
        // it has started up.
        let mut tracer = Tracer::spawn(tracer_command());
        tracer.wait_for_sentinel();
        let id = UniqueId::new();
        let id2 = id.clone();
        let thr = thread::spawn(move || {
            with_ids::waypoint_from_thread!(|| (&id2, "we're in a thread"));
        });
        thr.join().unwrap();
        let output = tracer.wait_for_exit();
        let stdout = output.stdout;
        let stderr = output.stderr;
        eprintln!("stdout: {}", stdout.trim());
        eprintln!("stderr: {}", stderr.trim());

        // Some versions of bpftrace print warnings from a bundled C compiler
        // directly to stdout. We need to ignore these, but also find the
        // expected ID. Look for that by finding lines that are only digits,
        // and parse them as a u64.
        let ids: Vec<u64> = stdout
            .lines()
            .filter(|line| !line.is_empty() && line.bytes().all(|b| b.is_ascii_digit()))
            .map(|line| line.parse().expect("parseable as a u64"))
            .collect();
        assert_eq!(
            ids,
            [id.as_u64()],
            concat!(
                "Expected exactly one line with a u64\n",
                "stdout\n",
                "------\n",
                "{}\n",
                "stderr\n",
                "------\n",
                "{}"
            ),
            stdout,
            stderr
        );
    }
}
