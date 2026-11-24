// SPDX-License-Identifier: MIT
//! Per-user state of the library: favourites and recently emulated backups (port of library_state.cpp).
//! Backups are named by their file name (not their path), so the state survives a move of the library
//! folder. The .bin files are never touched.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

pub const RECENT_MAX: usize = 10;
const MAX_LINES: usize = 2000; // the file is tiny: anything bigger is not ours

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LibraryState {
    /// In the order they were marked.
    pub favorites: Vec<String>,
    /// Most recent first, at most `RECENT_MAX`.
    pub recent: Vec<String>,
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() < 256 && !name.contains(['\r', '\n'])
}

impl LibraryState {
    pub fn is_favorite(&self, name: &str) -> bool {
        self.favorites.iter().any(|n| n == name)
    }

    /// Returns true when `name` became a favourite, false when it was removed from them.
    pub fn toggle_favorite(&mut self, name: &str) -> bool {
        if !valid_name(name) {
            return false;
        }
        if self.is_favorite(name) {
            self.favorites.retain(|n| n != name);
            return false;
        }
        self.favorites.push(name.to_owned());
        true
    }

    /// Moves `name` to the top of the recent list.
    pub fn mark_emulated(&mut self, name: &str) {
        if !valid_name(name) {
            return;
        }
        self.recent.retain(|n| n != name);
        self.recent.insert(0, name.to_owned());
        self.recent.truncate(RECENT_MAX);
    }

    /// Drops `name` from both lists (the file was deleted).
    pub fn forget(&mut self, name: &str) {
        self.favorites.retain(|n| n != name);
        self.recent.retain(|n| n != name);
    }

    /// A missing file is an empty state (not an error). Unknown or malformed lines are ignored.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = fs::read(path).map_err(|_| format!("could not open {}", path.display()))?;
        Ok(Self::parse(&String::from_utf8_lossy(&text)))
    }

    pub(crate) fn parse(text: &str) -> Self {
        let mut s = Self::default();
        for line in text.lines().take(MAX_LINES) {
            let Some((key, value)) = line.split_once('=') else { continue };
            if !valid_name(value) {
                continue;
            }
            match key {
                "favorite" if !s.is_favorite(value) => s.favorites.push(value.to_owned()),
                "recent" if !s.recent.iter().any(|n| n == value) && s.recent.len() < RECENT_MAX => {
                    s.recent.push(value.to_owned())
                }
                _ => {}
            }
        }
        s
    }

    /// Atomic write (temporary file, fsync, rename), mode 0600, parent folder created with mode 0700 when missing.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), String> {
        let path = path.as_ref();
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            if !parent.is_dir() {
                fs::create_dir_all(parent).map_err(|e| format!("could not create {}: {e}", parent.display()))?;
                let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
            }
        }
        let mut content = String::from("# ZeroTag library state (favourites, recent). Safe to delete.\n");
        for n in self.favorites.iter().filter(|n| valid_name(n)) {
            content += &format!("favorite={n}\n");
        }
        for n in self.recent.iter().filter(|n| valid_name(n)) {
            content += &format!("recent={n}\n");
        }
        let tmp = format!("{}.tmp", path.display());
        let write = || -> std::io::Result<()> {
            let mut f = OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
            f.write_all(content.as_bytes())?;
            f.sync_all()
        };
        if write().is_err() {
            let _ = fs::remove_file(&tmp);
            return Err(format!("write failed for {tmp}"));
        }
        fs::rename(&tmp, path).map_err(|e| {
            let _ = fs::remove_file(&tmp);
            format!("could not replace {}: {e}", path.display())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn favourites_toggle_and_recent_order() {
        let mut s = LibraryState::default();
        assert!(s.toggle_favorite("a.bin"));
        assert!(s.is_favorite("a.bin"));
        assert!(!s.toggle_favorite("a.bin"));
        assert!(!s.toggle_favorite("bad\nname")); // invalid names are refused
        for i in 0..12 {
            s.mark_emulated(&format!("{i}.bin"));
        }
        s.mark_emulated("5.bin");
        assert_eq!(s.recent.len(), RECENT_MAX);
        assert_eq!(s.recent[0], "5.bin");
        assert_eq!(s.recent.iter().filter(|n| *n == "5.bin").count(), 1);
        s.forget("5.bin");
        assert!(!s.recent.contains(&"5.bin".to_owned()));
    }

    #[test]
    fn parse_ignores_junk_and_duplicates() {
        let s = LibraryState::parse("# c\nfavorite=a.bin\nfavorite=a.bin\nnoise\nrecent=b.bin\nrecent=\nother=x\n");
        assert_eq!(s.favorites, ["a.bin"]);
        assert_eq!(s.recent, ["b.bin"]);
    }

    #[test]
    fn save_load_roundtrip_with_private_modes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new/sub/state.txt");
        let mut s = LibraryState::default();
        s.toggle_favorite("x.bin");
        s.mark_emulated("y.bin");
        s.save(&path).unwrap();
        assert_eq!(LibraryState::load(&path).unwrap(), s);
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(fs::metadata(path.parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
        assert!(!dir.path().join("new/sub/state.txt.tmp").exists());
        assert_eq!(LibraryState::load(dir.path().join("missing")).unwrap(), LibraryState::default());
    }
}
