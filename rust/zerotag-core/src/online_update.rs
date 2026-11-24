// SPDX-License-Identifier: MIT
//! Optional online update of the catalogue: downloads the amiibo list of https://www.amiiboapi.org (the same
//! source as the bundled catalogue) so that newly announced figures show up. Off by default; nothing but the
//! public list is requested and nothing about the user or the device is sent.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::time::{Duration, Instant};

const URL: &str = "https://www.amiiboapi.org/api/amiibo/";
const TIMEOUT_SECONDS: u64 = 60;

fn with_suffix(dest: &Path, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}{suffix}", dest.display()))
}

/// Last non-empty line of the file the child wrote its errors to, for the message shown to the user.
fn last_line(path: &Path) -> String {
    let text = fs::read_to_string(path).unwrap_or_default();
    let last = text.lines().rev().find(|l| !l.is_empty()).unwrap_or("");
    last.chars().take(80).collect()
}

fn looks_like_amiibo_list(text: &str) -> bool {
    text.trim_start().starts_with('{') && text.contains("\"amiibo\"") && text.contains("\"head\"")
}

/// Downloads the amiibo list to `dest` with curl (no shell involved; HTTPS only, 60 s limit). The file is written
/// to `dest.part`, checked to be a JSON object containing an "amiibo" array and renamed over `dest` only then,
/// so a failed download never damages the previous copy. `cancel` is polled while waiting.
pub fn download_amiibo_list(dest: impl AsRef<Path>, cancel: &AtomicBool) -> Result<(), String> {
    download(Command::new("curl"), URL, dest.as_ref(), cancel)
}

fn download(mut curl: Command, url: &str, dest: &Path, cancel: &AtomicBool) -> Result<(), String> {
    let part = with_suffix(dest, ".part");
    let errfile = with_suffix(dest, ".err");
    let _ = fs::remove_file(&part);
    let err = fs::File::create(&errfile).map_err(|e| format!("cannot create {}: {e}", errfile.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&errfile, fs::Permissions::from_mode(0o600));
    }
    let cleanup = |also_part: bool| {
        let _ = fs::remove_file(&errfile);
        if also_part {
            let _ = fs::remove_file(&part);
        }
    };

    let spawned = curl
        .args(["--fail", "--silent", "--show-error", "--location", "--proto", "=https"])
        .args(["--max-time", &TIMEOUT_SECONDS.to_string(), "--max-filesize", "8000000"])
        .args(["--user-agent", "zerotag-catalogue-update", "--output"])
        .arg(&part)
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(err)
        .spawn();
    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => {
            cleanup(true);
            return Err(if e.kind() == ErrorKind::NotFound {
                "curl is not installed (sudo apt install curl)".into()
            } else {
                format!("cannot run curl: {e}")
            });
        }
    };

    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => {}
            Err(e) => {
                cleanup(true);
                return Err(format!("waitpid: {e}"));
            }
        }
        let late = start.elapsed() > Duration::from_secs(TIMEOUT_SECONDS + 10);
        if cancel.load(Relaxed) || late {
            let _ = child.kill();
            let _ = child.wait();
            cleanup(true);
            return Err(if cancel.load(Relaxed) { "cancelled" } else { "timeout" }.into());
        }
        std::thread::sleep(Duration::from_millis(100));
    };

    let result = if !status.success() {
        let why = last_line(&errfile);
        Err(if why.is_empty() { "download failed".into() } else { why })
    } else if !looks_like_amiibo_list(&fs::read_to_string(&part).unwrap_or_default()) {
        Err("unexpected answer from the server".into())
    } else {
        fs::rename(&part, dest).map_err(|e| format!("rename: {e}"))
    };
    cleanup(result.is_err());
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answer_check() {
        assert!(looks_like_amiibo_list("  {\"amiibo\":[{\"head\":\"0x01\"}]}"));
        assert!(!looks_like_amiibo_list("<html>captive portal</html>"));
        assert!(!looks_like_amiibo_list("{\"error\":1}"));
        assert!(!looks_like_amiibo_list(""));
    }

    #[test]
    fn missing_curl_is_reported() {
        let d = tempfile::tempdir().unwrap();
        let dest = d.path().join("x.json");
        let e = download(Command::new("definitely-not-curl-zerotag"), URL, &dest, &AtomicBool::new(false)).unwrap_err();
        assert!(e.contains("curl is not installed"), "{e}");
        assert!(!dest.exists() && fs::read_dir(d.path()).unwrap().count() == 0);
    }

    #[test]
    fn rejects_non_https_without_touching_the_previous_copy() {
        let d = tempfile::tempdir().unwrap();
        let dest = d.path().join("x.json");
        fs::write(&dest, "previous").unwrap();
        let e = download(Command::new("curl"), "http://127.0.0.1:9/x", &dest, &AtomicBool::new(false)).unwrap_err();
        assert!(!e.is_empty());
        assert_eq!(fs::read_to_string(&dest).unwrap(), "previous");
        assert_eq!(fs::read_dir(d.path()).unwrap().count(), 1); // no .part / .err left
    }

    #[test]
    fn cancel_stops_the_child() {
        let d = tempfile::tempdir().unwrap();
        // a fake "curl" that hangs: `sleep` ignores the curl options only if run through sh
        let mut fake = Command::new("sh");
        fake.args(["-c", "sleep 30", "--"]);
        let cancel = AtomicBool::new(true);
        let t = Instant::now();
        let e = download(fake, URL, &d.path().join("x.json"), &cancel).unwrap_err();
        assert_eq!(e, "cancelled");
        assert!(t.elapsed() < Duration::from_secs(5));
    }

    /// Network test, run by hand: `cargo test -- --ignored online`.
    #[test]
    #[ignore]
    fn online_download() {
        let d = tempfile::tempdir().unwrap();
        let dest = d.path().join("amiibo_online.json");
        download_amiibo_list(&dest, &AtomicBool::new(false)).unwrap();
        let db = crate::amiibo::Database::load(&dest).unwrap();
        assert!(db.entries().len() > 900);
    }
}
