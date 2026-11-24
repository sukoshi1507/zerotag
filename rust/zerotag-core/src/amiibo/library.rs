// SPDX-License-Identifier: MIT
//! A folder of 540-byte NTAG215 backups (port of amiibo_library.cpp). Characters are identified from the model
//! info stored in clear in the image (pages 21-22), so no key is needed to list a collection; the keys, when
//! given, are only used to check the signatures.

use super::crypto::Keys;
use super::db::{Database, Entry};
use super::image::{generate, verify, TagImage, NTAG215_SIZE};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

const MODEL_INFO: usize = 0x54; // pages 21-22: head (4 bytes) then tail (4 bytes), big endian

/// A backup found in the library folder.
#[derive(Debug, Clone)]
pub struct Item {
    pub path: PathBuf,
    pub head: u32,
    pub tail: u32,
    /// `None` when the character is not in the catalogue.
    pub entry: Option<Entry>,
    /// Keys were available.
    pub signature_checked: bool,
    /// Meaningful only when `signature_checked`.
    pub signature_ok: bool,
    /// "04:AA:..." for display.
    pub uid: String,
}

#[derive(Debug, Default)]
pub struct Library {
    dir: PathBuf,
    items: Vec<Item>,
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes(b[..4].try_into().unwrap())
}

fn uid_text(i: &TagImage) -> String {
    [i[0], i[1], i[2], i[4], i[5], i[6], i[7]].iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":")
}

fn file_stem_for(entry: &Entry, image: &TagImage) -> String {
    let mut stem: String = entry.name.bytes().map(|b| if b.is_ascii_alphanumeric() { b as char } else { '_' }).collect();
    if stem.is_empty() {
        stem = entry.id.clone();
    }
    stem.truncate(48);
    format!("{stem}_{:02X}{:02X}{:02X}{:02X}", image[4], image[5], image[6], image[7])
}

pub fn read_image(path: impl AsRef<Path>) -> Result<TagImage, String> {
    let path = path.as_ref();
    let data = fs::read(path).map_err(|_| format!("cannot open {}", path.display()))?;
    data.try_into().map_err(|_| format!("{} is not a 540-byte image", path.display()))
}

/// Atomic (temporary file + rename), mode 0600. Used to keep what a console wrote during emulation.
pub fn write_image(path: impl AsRef<Path>, image: &TagImage) -> Result<(), String> {
    let path = path.as_ref();
    let tmp = PathBuf::from(format!("{}.tmp", path.display()));
    let write = || -> std::io::Result<()> {
        let mut f = OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
        f.write_all(image)?;
        f.flush()?;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))
    };
    if write().is_err() {
        let _ = fs::remove_file(&tmp);
        return Err(format!("cannot write {}", tmp.display()));
    }
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("cannot replace {}: {e}", path.display())
    })
}

impl Library {
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// An empty library for `dir` (not scanned, maybe not created yet): `create` makes the folder on first use.
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Self { dir: dir.as_ref().to_owned(), items: Vec::new() }
    }

    /// Scans `dir` (not recursive) for *.bin files of 540 bytes.
    pub fn scan(dir: impl AsRef<Path>, db: &Database, keys: Option<&Keys>) -> Result<Self, String> {
        let dir = dir.as_ref();
        if !dir.is_dir() {
            return Err(format!("not a directory: {}", dir.display()));
        }
        let rd = fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        let mut items = Vec::new();
        for de in rd.flatten() {
            let path = de.path();
            let is_bin = path.extension().is_some_and(|e| e == "bin");
            let Ok(meta) = de.metadata() else { continue };
            if !meta.is_file() || !is_bin || meta.len() != NTAG215_SIZE as u64 {
                continue;
            }
            let Ok(image) = read_image(&path) else { continue };
            let (mut head, mut tail) = (be32(&image[MODEL_INFO..]), be32(&image[MODEL_INFO + 4..]));
            let (mut checked, mut ok) = (false, false);
            if let Some(k) = keys {
                checked = true;
                if let Some((h, t)) = verify(k, &image) {
                    ok = true;
                    (head, tail) = (h, t);
                }
            }
            items.push(Item {
                path,
                head,
                tail,
                entry: db.find(head, tail).cloned(),
                signature_checked: checked,
                signature_ok: ok,
                uid: uid_text(&image),
            });
        }
        let key = |i: &Item| i.entry.as_ref().map_or_else(|| "~".to_owned(), |e| e.name.to_ascii_lowercase());
        items.sort_by(|a, b| key(a).cmp(&key(b)).then_with(|| a.path.cmp(&b.path)));
        Ok(Self { dir: dir.to_owned(), items })
    }

    /// Case-insensitive search over the catalogue fields of the identified items.
    pub fn search<'a>(&'a self, db: &Database, query: &str) -> Vec<&'a Item> {
        self.search_indices(db, query).into_iter().map(|i| &self.items[i]).collect()
    }

    /// Same as `search`, as indices into `items()`.
    pub fn search_indices(&self, db: &Database, query: &str) -> Vec<usize> {
        let hits: Vec<&str> = db.search(query).iter().map(|e| e.id.as_str()).collect();
        (0..self.items.len())
            .filter(|&i| self.items[i].entry.as_ref().is_some_and(|e| hits.contains(&e.id.as_str())))
            .collect()
    }

    /// Generates a blank signed image for `entry`, stores it in the folder and returns its path.
    pub fn create(&mut self, keys: &Keys, entry: &Entry) -> Result<PathBuf, String> {
        if self.dir.as_os_str().is_empty() {
            return Err("library folder not set (call scan first)".into());
        }
        let image = generate(keys, entry.head, entry.tail).map_err(|e| e.to_string())?;
        let _ = fs::create_dir_all(&self.dir);
        let path = self.dir.join(format!("{}.bin", file_stem_for(entry, &image)));
        if path.exists() {
            return Err(format!("{} already exists", path.display()));
        }
        write_image(&path, &image)?;
        self.items.push(Item {
            path: path.clone(),
            head: entry.head,
            tail: entry.tail,
            entry: Some(entry.clone()),
            signature_checked: true,
            signature_ok: true,
            uid: uid_text(&image),
        });
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_keys() -> Keys {
        let mut b = [0u8; 160];
        for (i, x) in b.iter_mut().enumerate() {
            *x = (i as u8).wrapping_mul(37).wrapping_add(11);
        }
        for set in [0usize, 80] {
            b[set + 16..set + 30].copy_from_slice(b"unfixed infos\0");
            b[set + 30] = 0;
            b[set + 31] = 14;
        }
        Keys::from_bytes(&b).unwrap()
    }

    fn db() -> Database {
        Database::from_json(
            br#"{"amiibo":[
              {"id":"01010000000e0002","name":"Link","character":"Link","gameSeries":"The Legend of Zelda","amiiboSeries":"Super Smash Bros.","type":"Figure"},
              {"id":"0000000002380602","name":"8-Bit Mario","character":"Mario","gameSeries":"Super Mario","amiiboSeries":"8-bit Mario","type":"Figure"}]}"#,
        )
        .unwrap()
    }

    #[test]
    fn create_scan_search_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let (keys, db) = (fake_keys(), db());
        let mut lib = Library::scan(dir.path(), &db, Some(&keys)).unwrap();
        assert!(lib.items().is_empty());

        let p1 = lib.create(&keys, db.find(0x01010000, 0x000e0002).unwrap()).unwrap();
        let p2 = lib.create(&keys, db.find(0, 0x02380602).unwrap()).unwrap();
        assert!(p1.file_name().unwrap().to_str().unwrap().starts_with("Link_"));
        assert!(p2.file_name().unwrap().to_str().unwrap().starts_with("8_Bit_Mario_"));
        assert_eq!(fs::metadata(&p1).unwrap().permissions().mode() & 0o777, 0o600);
        // noise that must be ignored
        fs::write(dir.path().join("short.bin"), [0u8; 10]).unwrap();
        fs::write(dir.path().join("notes.txt"), [0u8; 540]).unwrap();

        let lib = Library::scan(dir.path(), &db, Some(&keys)).unwrap();
        assert_eq!(lib.items().len(), 2);
        assert_eq!(lib.items()[0].entry.as_ref().unwrap().name, "8-Bit Mario"); // sorted by name
        assert!(lib.items().iter().all(|i| i.signature_checked && i.signature_ok));
        assert_eq!(lib.items()[1].uid.len(), 20);
        assert_eq!(lib.search(&db, "zelda").len(), 1);

        // without keys the model info in clear still identifies the character
        let nokeys = Library::scan(dir.path(), &db, None).unwrap();
        assert!(nokeys.items().iter().all(|i| i.entry.is_some() && !i.signature_checked));
    }

    #[test]
    fn tampered_image_fails_its_signature_and_duplicate_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let (keys, db) = (fake_keys(), db());
        let mut lib = Library::scan(dir.path(), &db, Some(&keys)).unwrap();
        let p = lib.create(&keys, db.find(0x01010000, 0x000e0002).unwrap()).unwrap();
        let mut img = read_image(&p).unwrap();
        img[0x100] ^= 1;
        write_image(&p, &img).unwrap();
        let lib2 = Library::scan(dir.path(), &db, Some(&keys)).unwrap();
        assert!(lib2.items()[0].signature_checked && !lib2.items()[0].signature_ok);
        assert!(read_image(dir.path().join("missing.bin")).is_err());
    }

    #[test]
    fn scan_of_a_missing_folder_is_an_error_but_create_makes_the_folder() {
        assert!(Library::scan("/nonexistent/zerotag", &db(), None).is_err());
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("not/yet/there");
        let (keys, db) = (fake_keys(), db());
        let mut lib = Library::new(&dir);
        assert!(lib.items().is_empty() && lib.dir() == dir);
        let p = lib.create(&keys, db.find(0x01010000, 0x000e0002).unwrap()).unwrap();
        assert!(p.starts_with(&dir) && p.exists());
        assert_eq!(Library::scan(&dir, &db, Some(&keys)).unwrap().items().len(), 1);
    }
}
