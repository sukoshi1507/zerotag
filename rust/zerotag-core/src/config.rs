// SPDX-License-Identifier: MIT
//! Application settings (port of application_config.cpp): an INI file with an `[application]` section.
//! Updating one key keeps the rest of the file (comments, other sections).

use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicationConfig {
    pub dark_mode: bool,
    /// fr, en, ja
    pub language: String,
    /// New UID after each read session.
    pub uid_rotation: bool,
    /// Jingles and the manual's music.
    pub sound: bool,
    /// Optional download of the amiibo list (Settings > Online); off = fully offline.
    pub online: bool,
}

impl Default for ApplicationConfig {
    fn default() -> Self {
        Self { dark_mode: true, language: "fr".into(), uid_rotation: false, sound: true, online: false }
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "yes" | "true" | "1" => Some(true),
        "no" | "false" | "0" => Some(false),
        _ => None,
    }
}

fn is_section_header(line: &str) -> bool {
    let n = line.trim();
    n.len() >= 2 && n.starts_with('[') && n.ends_with(']')
}

fn is_section(line: &str, expected: &str) -> bool {
    is_section_header(line) && {
        let n = line.trim();
        n[1..n.len() - 1].trim().to_ascii_lowercase() == expected
    }
}

fn is_key(line: &str, expected: &str) -> bool {
    line.split_once('=').is_some_and(|(k, _)| k.trim().to_ascii_lowercase() == expected)
}

impl ApplicationConfig {
    /// Starts from the defaults; a missing file is an error (the caller keeps the defaults).
    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        let mut c = Self::default();
        c.load_into(path)?;
        Ok(c)
    }

    /// Reads the file over the current values. An invalid `dark_mode` stops the read with an error.
    pub fn load_into(&mut self, path: impl AsRef<Path>) -> Result<(), String> {
        let text = fs::read(path).map_err(|_| "could not open file".to_string())?;
        let text = String::from_utf8_lossy(&text);
        let mut section = String::new();
        for (i, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                section = line[1..line.len() - 1].trim().to_ascii_lowercase();
                continue;
            }
            let Some((k, v)) = line.split_once('=') else { continue };
            if section != "application" {
                continue;
            }
            match k.trim().to_ascii_lowercase().as_str() {
                "uid_rotation" => self.uid_rotation = parse_bool(v).unwrap_or(self.uid_rotation),
                "sound" => self.sound = parse_bool(v).unwrap_or(self.sound),
                "online" => self.online = parse_bool(v).unwrap_or(self.online),
                "language" => self.language = v.trim().to_ascii_lowercase(),
                "dark_mode" => {
                    self.dark_mode = parse_bool(v).ok_or_else(|| format!("invalid dark_mode value at line {}", i + 1))?
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Writes (or updates) `dark_mode`, keeping the rest of the file.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), String> {
        save_setting(path, "dark_mode", if self.dark_mode { "yes" } else { "no" })
    }
}

/// Writes (or updates) one key of the `[application]` section, keeping the rest of the file.
pub fn save_setting(path: impl AsRef<Path>, key: &str, value: &str) -> Result<(), String> {
    let path = path.as_ref();
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(|e| format!("could not create parent directory: {e}"))?;
    }
    let old = fs::read(path).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    let mut lines: Vec<String> = old.lines().map(str::to_owned).collect();

    // Find the [application] section and its end.
    let mut begin = None;
    let mut end = lines.len();
    for (i, l) in lines.iter().enumerate() {
        match begin {
            None if is_section(l, "application") => begin = Some(i + 1),
            Some(_) if is_section_header(l) => {
                end = i;
                break;
            }
            _ => {}
        }
    }
    let entry = format!("{key}={value}");
    match begin {
        None => {
            if lines.last().is_some_and(|l| !l.is_empty()) {
                lines.push(String::new());
            }
            lines.push("[application]".into());
            lines.push(entry);
        }
        Some(begin) => {
            if let Some(i) = (begin..end).find(|&i| is_key(&lines[i], key)) {
                lines[i] = entry;
            } else {
                let mut pos = end;
                while pos > begin && lines[pos - 1].trim().is_empty() {
                    pos -= 1;
                }
                lines.insert(pos, entry);
            }
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    fs::write(path, out).map_err(|_| "could not open file for writing".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_load() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("c.ini");
        fs::write(&p, "# c\n[Other]\nsound=no\n[Application]\nLanguage = EN\nuid_rotation=true\nsound=0\nonline=yes\ndark_mode=no\n").unwrap();
        let c = ApplicationConfig::load(&p).unwrap();
        assert_eq!(
            c,
            ApplicationConfig { dark_mode: false, language: "en".into(), uid_rotation: true, sound: false, online: true }
        );
        assert!(ApplicationConfig::load(d.path().join("missing")).is_err());
    }

    #[test]
    fn invalid_dark_mode_is_an_error_but_junk_in_other_keys_is_ignored() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("c.ini");
        fs::write(&p, "[application]\nsound=maybe\ndark_mode=perhaps\n").unwrap();
        let e = ApplicationConfig::load(&p).unwrap_err();
        assert_eq!(e, "invalid dark_mode value at line 3");
        fs::write(&p, "[application]\nsound=maybe\n").unwrap();
        assert!(ApplicationConfig::load(&p).unwrap().sound); // kept the default
    }

    #[test]
    fn save_setting_updates_in_place_and_keeps_the_rest() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("sub/c.ini");
        save_setting(&p, "sound", "no").unwrap(); // creates folder, file and section
        assert_eq!(fs::read_to_string(&p).unwrap(), "[application]\nsound=no\n");

        fs::write(&p, "# keep me\n[application]\nsound=yes\nlanguage=fr\n\n[later]\nx=1\n").unwrap();
        save_setting(&p, "sound", "no").unwrap(); // replaced
        save_setting(&p, "online", "yes").unwrap(); // inserted at the end of the section, before the blank line
        assert_eq!(
            fs::read_to_string(&p).unwrap(),
            "# keep me\n[application]\nsound=no\nlanguage=fr\nonline=yes\n\n[later]\nx=1\n"
        );
        ApplicationConfig { dark_mode: false, ..Default::default() }.save(&p).unwrap();
        assert!(!ApplicationConfig::load(&p).unwrap().dark_mode);
    }
}
