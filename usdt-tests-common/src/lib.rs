use std::process::Command;

#[cfg(target_os = "illumos")]
pub fn root_command(name: &str) -> Command {
    // On illumos systems, we prefer pfexec(1) but allow some other command to
    // be specified through the environment.
    let pfexec = std::env::var("PFEXEC").unwrap_or_else(|_| "/usr/bin/pfexec".to_string());
    let mut cmd = Command::new(pfexec);
    cmd.arg(name);
    cmd
}

#[cfg(not(target_os = "illumos"))]
pub fn root_command(name: &str) -> Command {
    let mut cmd = Command::new("sudo");
    cmd.arg(name);
    cmd
}
