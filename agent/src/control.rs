//! Actions the agent can take: container start/stop/restart, reading
//! container logs, and rebooting the machine.
//!
//! Every function returns either a success value or an (HTTP status, message)
//! pair that main.rs sends straight back to the caller.

use std::time::Duration;

use crate::exec::{self, RunError};

pub type ActionResult<T> = Result<T, (u16, String)>;

const ACTION_TIMEOUT: Duration = Duration::from_secs(60); // `docker stop` waits up to 10s by default
const LOGS_TIMEOUT: Duration = Duration::from_secs(15);
const REBOOT_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_LOG_BYTES: usize = 512 * 1024;
pub const MAX_LOG_LINES: u32 = 2000;

/// Docker container names are letters, digits, `_`, `.` and `-`, and can't
/// start with punctuation. Checking this also means a name can never be
/// mistaken for a command-line flag like `--help`.
pub fn valid_container_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

pub fn container_action(name: &str, action: &str) -> ActionResult<()> {
    if !matches!(action, "start" | "stop" | "restart") {
        return Err((404, format!("unknown action '{action}'")));
    }
    if !valid_container_name(name) {
        return Err((400, "invalid container name".into()));
    }
    let out =
        exec::run("docker", &[action, name], ACTION_TIMEOUT, 64 * 1024).map_err(docker_err)?;
    if out.success {
        Ok(())
    } else {
        Err((500, exec::first_line(&out.text).to_string()))
    }
}

pub fn container_logs(name: &str, lines: u32) -> ActionResult<String> {
    if !valid_container_name(name) {
        return Err((400, "invalid container name".into()));
    }
    let tail = lines.clamp(1, MAX_LOG_LINES).to_string();
    let out = exec::run(
        "docker",
        &["logs", "--tail", &tail, "--timestamps", name],
        LOGS_TIMEOUT,
        MAX_LOG_BYTES,
    )
    .map_err(docker_err)?;
    if out.success {
        Ok(out.text)
    } else {
        Err((500, exec::first_line(&out.text).to_string()))
    }
}

pub fn reboot() -> ActionResult<()> {
    let out =
        exec::run("systemctl", &["reboot"], REBOOT_TIMEOUT, 16 * 1024).map_err(|e| match e {
            RunError::NotFound => (501, "systemctl not found".to_string()),
            RunError::TimedOut => (504, "systemctl reboot timed out".to_string()),
            RunError::Other(e) => (500, e),
        })?;
    if out.success {
        Ok(())
    } else {
        let msg = exec::first_line(&out.text);
        Err((
            500,
            format!("{msg} (is the polkit rule from deploy/ installed on this server?)"),
        ))
    }
}

fn docker_err(e: RunError) -> (u16, String) {
    match e {
        RunError::NotFound => (501, "docker is not installed on this server".into()),
        RunError::TimedOut => (504, "docker took too long to respond".into()),
        RunError::Other(e) => (500, e),
    }
}

#[cfg(test)]
mod tests {
    use super::valid_container_name as ok;

    #[test]
    fn container_names() {
        assert!(ok("inbox-zero"));
        assert!(ok("web_1.2"));
        assert!(!ok(""));
        assert!(!ok("--help"));
        assert!(!ok("-rf"));
        assert!(!ok("a b"));
        assert!(!ok("a;rm"));
        assert!(!ok("../x"));
        assert!(!ok(&"a".repeat(129)));
    }
}
