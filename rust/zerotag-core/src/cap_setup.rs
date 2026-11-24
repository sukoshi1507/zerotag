// SPDX-License-Identifier: MIT
//! Runs the root helper that loads the SPI overlay of the Cap (/usr/lib/zerotag/cap-setup) through sudo, with a
//! password typed in the application (port of cap_setup.cpp). No shell is involved and the password never appears
//! in an argument, an environment variable or a log.

use crate::nfc::CapConfig;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use zeroize::Zeroize;

const HELPER: &str = "/usr/lib/zerotag/cap-setup";
const TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok,
    PasswordRefused,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapSetupResult {
    pub status: Status,
    /// Last line printed by the helper (or the reason); never contains the password.
    pub message: String,
}

/// True when the SPI device of the Cap exists (the overlay is loaded).
pub fn spi_available() -> bool {
    Path::new(&CapConfig::default().spi_device).exists()
}

fn last_line(text: &str) -> String {
    text.trim_end().lines().last().unwrap_or("").to_owned()
}

/// Runs `sudo -S -p "" /usr/lib/zerotag/cap-setup`, feeding `password` on its standard input (an empty string works
/// when sudo needs none). The password is overwritten and cleared before returning. Blocks for 20 s at most.
pub fn run_cap_setup(password: &mut String) -> CapSetupResult {
    let mut cmd = Command::new("sudo");
    cmd.args(["-S", "-p", "", HELPER]);
    run_helper(cmd, password)
}

fn failed(message: impl Into<String>) -> CapSetupResult {
    CapSetupResult { status: Status::Failed, message: message.into() }
}

fn run_helper(mut cmd: Command, password: &mut String) -> CapSetupResult {
    let (mut reader, writer) = match os_pipe::pipe() {
        Ok(p) => p,
        Err(e) => {
            password.zeroize();
            return failed(e.to_string());
        }
    };
    let writer_err = match writer.try_clone() {
        Ok(w) => w,
        Err(e) => {
            password.zeroize();
            return failed(e.to_string());
        }
    };
    cmd.env("LC_ALL", "C") // stable messages to recognise
        .stdin(Stdio::piped())
        .stdout(writer)
        .stderr(writer_err);
    let spawned = cmd.spawn();
    drop(cmd); // closes our copies of the pipe's write end, so the reader sees EOF when the child exits
    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => {
            password.zeroize();
            return failed(if e.kind() == std::io::ErrorKind::NotFound { "sudo not found".into() } else { e.to_string() });
        }
    };

    // Feed the password (one line), then close the pipe. The child may exit before reading it: ignore EPIPE.
    let mut line = std::mem::take(password);
    line.push('\n');
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(line.as_bytes());
    }
    line.zeroize();

    // Collect what the helper prints, with a timeout.
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut buf = [0u8; 256];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let deadline = Instant::now() + TIMEOUT;
    let mut output = Vec::new();
    let mut timed_out = false;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            timed_out = true;
            break;
        }
        match rx.recv_timeout(left.min(Duration::from_millis(500))) {
            Ok(chunk) => {
                if output.len() < 2048 {
                    output.extend_from_slice(&chunk);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break, // EOF: the child closed its output
        }
    }
    if timed_out {
        let _ = child.kill();
        let _ = child.wait();
        return failed("timeout");
    }
    let code = child.wait().ok().and_then(|s| s.code()).unwrap_or(-1);
    let output = String::from_utf8_lossy(&output).into_owned();
    if code == 0 {
        return CapSetupResult { status: Status::Ok, message: last_line(&output) };
    }
    // sudo prints these when the password is wrong or one is needed but cannot be given.
    const REFUSED: [&str; 6] = [
        "incorrect password",
        "Sorry",
        "a password is required",
        "no password was provided",
        "is not in the sudoers",
        "may not run sudo",
    ];
    let status = if REFUSED.iter().any(|p| output.contains(p)) { Status::PasswordRefused } else { Status::Failed };
    CapSetupResult { status, message: if code == 127 { "sudo not found".into() } else { last_line(&output) } }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in for `sudo -S`: reads the password from stdin like sudo does.
    fn fake(script: &str) -> Command {
        let mut c = Command::new("sh");
        c.args(["-c", script, "--"]);
        c
    }

    const FAKE_SUDO: &str = r#"read p; if [ "$p" = "secret" ]; then echo "overlay loaded"; echo "/dev/spidev0.2 is ready."; \
        else echo "Sorry, try again." >&2; exit 1; fi"#;

    #[test]
    fn right_password_runs_the_helper_and_wipes_the_password() {
        let mut pw = String::from("secret");
        let r = run_helper(fake(FAKE_SUDO), &mut pw);
        assert_eq!(r, CapSetupResult { status: Status::Ok, message: "/dev/spidev0.2 is ready.".into() });
        assert!(pw.is_empty());
    }

    #[test]
    fn wrong_password_is_reported_as_refused() {
        let mut pw = String::from("nope");
        let r = run_helper(fake(FAKE_SUDO), &mut pw);
        assert_eq!(r.status, Status::PasswordRefused);
        assert_eq!(r.message, "Sorry, try again.");
        assert!(pw.is_empty());
    }

    #[test]
    fn other_failures_and_exit_before_reading() {
        let mut pw = String::from("x");
        assert_eq!(run_helper(fake("echo boom; exit 3"), &mut pw), failed("boom"));
        let mut pw = String::from("x");
        assert_eq!(run_helper(Command::new("definitely-not-sudo-zerotag"), &mut pw).message, "sudo not found");
        assert!(pw.is_empty());
    }

    #[test]
    fn empty_password_and_helper_that_never_reads_stdin() {
        let mut pw = String::new();
        let r = run_helper(fake("exit 0"), &mut pw);
        assert_eq!(r.status, Status::Ok);
    }
}
