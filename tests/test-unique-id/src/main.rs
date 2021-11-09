//! Integration test for `usdt::UniqueId`

// Copyright 2021 Oxide Computer Company
#![feature(asm)]
#![cfg_attr(target_os = "macos", feature(asm_sym))]

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
    use std::thread;
    use std::time::Duration;
    use subprocess::Exec;
    use usdt::UniqueId;

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

        // Actually verify that the same value is received by DTrace.
        let sudo = if cfg!(target_os = "illumos") {
            "pfexec"
        } else {
            "sudo"
        };
        let mut dtrace = Exec::cmd(sudo)
            .arg("/usr/sbin/dtrace")
            .arg("-q")
            .arg("-n")
            .arg(r#"with_ids*:::waypoint_from_thread { printf("%d\n", arg0); exit(0); }"#)
            .stdin(subprocess::NullFile)
            .stderr(subprocess::Redirection::Pipe)
            .stdout(subprocess::Redirection::Pipe)
            .popen()
            .expect("Failed to run DTrace");
        thread::sleep(Duration::from_millis(1000));
        let id = UniqueId::new();
        let id2 = id.clone();
        let thr = thread::spawn(move || {
            with_ids::waypoint_from_thread!(|| (&id2, "we're in a thread"));
        });
        thr.join().unwrap();

        const TIMEOUT: Duration = Duration::from_secs(10);
        let mut comm = dtrace.communicate_start(None).limit_time(TIMEOUT);
        if dtrace
            .wait_timeout(TIMEOUT)
            .expect("DTrace command failed")
            .is_none()
        {
            std::process::Command::new(sudo)
                .arg("kill")
                .arg(format!("{}", dtrace.pid().unwrap()))
                .spawn()
                .expect("Failed to spawn kill")
                .wait()
                .expect("Failed to kill DTrace subprocess");
            panic!("DTrace didn't exit within timeout of {:?}", TIMEOUT);
        }
        let (stdout, stderr) = comm.read_string().expect("Failed to read DTrace output");
        let stdout = stdout.unwrap_or_else(|| String::from("<EMPTY>"));
        let stderr = stderr.unwrap_or_else(|| String::from("<EMPTY>"));
        let actual_id: u64 = stdout.trim().parse().expect(&format!(
            concat!(
                "Expected a u64\n",
                "stdout\n",
                "------\n",
                "{}\n",
                "stderr\n",
                "------\n",
                "{}"
            ),
            stdout, stderr
        ));

        assert_eq!(actual_id, id.as_u64());
    }
}
