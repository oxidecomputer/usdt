// Copyright 2026 Oxide Computer Company
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

use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;
use std::process::Child;
use std::process::Command;
use std::process::ExitStatus;
use std::process::Stdio;
use std::sync::mpsc;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::RecvTimeoutError;
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;
use std::time::Instant;

/// A sentinel that tracer programs should print on its own line from their
/// BEGIN probe. This is used to notify any test programs that all the other
/// probes have been enabled and that the tracer is fully running.
pub const BEGIN_SENTINEL: &str = "BEGIN";

// Maximum time to wait for the tracer to start up.
//
// Newer versions of `bpftrace` compile C code when a probe USDT probe is
// attached, which is kind of nuts. That can slow attachment, especially on
// loaded CI machines.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);

// Maximum time to wait for a tracer program to complete, after starting.
const EXIT_TIMEOUT: Duration = Duration::from_secs(10);

/// The output of a tracer which has exited.
#[derive(Debug)]
pub struct TracerOutput {
    pub status: ExitStatus,
    /// Lines printed to stdout after the begin sentinel.
    pub stdout: String,
    /// Lines printed to stderr, collected after the program is complete.
    pub stderr: String,
}

/// A platform-specific tracer subprocess, such as DTrace or bpftrace.
pub struct Tracer {
    // Actual child subprocess
    child: Child,
    // Queue receiving stdout.
    //
    // This is read on a separate thread, so we can start the tracer, run some
    // probes in the current process, and still receive output like the begin
    // sentinel or actual data.
    stdout: Receiver<String>,
    // All lines received from the stdout reader thread.
    stdout_lines: Vec<String>,
    // Which line has the begin sentinel.
    sentinel_index: Option<usize>,
    // Thread reading stderr.
    stderr: Option<JoinHandle<String>>,
}

impl Tracer {
    /// Spawn the tracer program.
    pub fn spawn(mut cmd: Command) -> Self {
        let mut child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("Failed to spawn tracer");
        let stdout = child.stdout.take().expect("piped stdout");

        // Spawn thread to read lines from stdout.
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut stderr = child.stderr.take().expect("piped stderr");
        let stderr = thread::spawn(move || {
            let mut output = String::new();
            let _ = stderr.read_to_string(&mut output);
            output
        });
        Self {
            child,
            stdout: rx,
            stdout_lines: Vec::new(),
            sentinel_index: None,
            stderr: Some(stderr),
        }
    }

    /// Wait for the begin sentinel on its own line, panicking if the tracer
    /// exits or does not print it in time.
    pub fn wait_for_sentinel(&mut self) {
        // Some versions of bpftrace emit internal C compiler warnings on
        // stdout, which is really unhelpful. We need to look for the sentinel
        // entirely alone on a line to be sure it's not referring to the
        // _source_ the tracer tried to compile.
        if !self.read_lines_until(STARTUP_TIMEOUT, |line| line == BEGIN_SENTINEL) {
            let (stdout, stderr) = self.output();
            panic!("tracer exited before startup\nstdout:\n{stdout}\nstderr:\n{stderr}");
        }
        self.sentinel_index = Some(self.stdout_lines.len());
    }

    /// Wait for the tracer to exit, panicking if it does not do so within the
    /// timeout.
    pub fn wait_for_exit(mut self) -> TracerOutput {
        self.read_lines_until(EXIT_TIMEOUT, |_| false);
        let status = self.child.wait().expect("Failed to wait for tracer");
        let start = self.sentinel_index.unwrap_or(0);
        let stdout = self.stdout_lines[start..].join("\n");
        let (_, stderr) = self.output();
        TracerOutput {
            status,
            stdout,
            stderr,
        }
    }

    /// Collect lines from stdout until `stop` returns true or a timeout.
    ///
    /// This reads each line of stdout, and calls `stop()` on it. If `stop()`
    /// returns true, then we stop reading and return. This returns `true` if we
    /// `stop()` returned true on a line. If the tracer exits without printing a
    /// matching line, return `false`.
    ///
    /// If `timeout` is reached before `stop()` returns true, kill the tracer
    /// process, attempt to grab whatever output it has, and panic.
    fn read_lines_until(&mut self, timeout: Duration, stop: impl Fn(&str) -> bool) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            match self
                .stdout
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            {
                Ok(line) => {
                    let done = stop(&line);
                    self.stdout_lines.push(line);
                    if done {
                        return true;
                    }
                }
                Err(RecvTimeoutError::Disconnected) => return false,
                Err(RecvTimeoutError::Timeout) => {
                    self.kill();
                    let (stdout, stderr) = self.output();
                    panic!(
                        "tracer timed out after {timeout:?}\nstdout:\n{stdout}\nstderr:\n{stderr}"
                    );
                }
            }
        }
    }

    /// Kill the tracer program.
    fn kill(&mut self) {
        // We run the program as root, so we have to kill it as root too.
        Command::new(root_command())
            .arg("kill")
            .arg(self.child.id().to_string())
            .status()
            .expect("Failed to kill tracer subprocess");
        let _ = self.child.wait();
    }

    /// Return all of stdout we've read so far, and all of stderr.
    ///
    /// This should only be called once the tracer has exited, and panics if
    /// called more than once.
    fn output(&mut self) -> (String, String) {
        let stderr = self
            .stderr
            .take()
            .map(|thr| thr.join().unwrap())
            .unwrap_or_default();
        (self.stdout_lines.join("\n"), stderr)
    }
}

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
