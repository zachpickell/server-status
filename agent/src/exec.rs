//! Running external commands (docker, systemctl) safely: no shell, a
//! timeout, and stdout + stderr captured together in the order they were
//! written.

use std::io::Read;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

pub struct Output {
    pub success: bool,
    /// stdout and stderr interleaved, as the program wrote them.
    pub text: String,
}

pub enum RunError {
    /// The program isn't installed.
    NotFound,
    TimedOut,
    Other(String),
}

/// Run `program args...`, killing it if it takes longer than `timeout`.
/// Only the last `max_bytes` of output are kept.
pub fn run(
    program: &str,
    args: &[&str],
    timeout: Duration,
    max_bytes: usize,
) -> Result<Output, RunError> {
    // One pipe for both stdout and stderr keeps them in order.
    let (mut reader, writer) = std::io::pipe().map_err(|e| RunError::Other(e.to_string()))?;
    let writer2 = writer
        .try_clone()
        .map_err(|e| RunError::Other(e.to_string()))?;

    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(writer)
        .stderr(writer2);

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(RunError::NotFound),
        Err(e) => return Err(RunError::Other(e.to_string())),
    };
    // Drop our copies of the pipe's write end, otherwise the reader never
    // sees end-of-file.
    drop(cmd);

    let reader_thread = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = reader.read_to_end(&mut buf);
        buf
    });

    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(RunError::TimedOut);
            }
            Ok(None) => thread::sleep(Duration::from_millis(25)),
            Err(e) => return Err(RunError::Other(e.to_string())),
        }
    };

    let mut buf = reader_thread.join().unwrap_or_default();
    if buf.len() > max_bytes {
        buf.drain(..buf.len() - max_bytes);
    }

    Ok(Output {
        success: status.success(),
        text: String::from_utf8_lossy(&buf).into_owned(),
    })
}

/// The first non-empty line of some output, for short error messages.
pub fn first_line(text: &str) -> &str {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
}
