// SPDX-License-Identifier: MIT
//! Scan of a real amiibo: reads the 135 pages of an NTAG215 placed on the Cap (port of scan_read_tag / scan_worker).

use crate::amiibo::{Entry, TagImage, NTAG215_SIZE};
use crate::nfc::{Bus, Chip, Error};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering::SeqCst};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

pub const NTAG_PAGES: u32 = 135; // 540 bytes

/// The two Type 2 commands the scan needs (the chip in practice, a fake tag in the tests).
pub trait Type2 {
    fn get_version(&mut self) -> Option<[u8; 8]>;
    /// READ: 4 pages starting at `page`.
    fn read(&mut self, page: u8) -> Option<[u8; 16]>;
}

impl<B: Bus> Type2 for Chip<B> {
    fn get_version(&mut self) -> Option<[u8; 8]> {
        self.type2_get_version()
    }

    fn read(&mut self, page: u8) -> Option<[u8; 16]> {
        self.type2_read(page)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadIssue {
    /// The tag answered but is not an NTAG215.
    NotNtag215,
    /// The tag stopped answering (removed or moved) while reading `page`.
    Interrupted { page: u32 },
}

/// Reads GET_VERSION, then the pages in blocks of four. `progress` receives the number of pages read so far.
pub fn read_ntag215(tag: &mut impl Type2, progress: &AtomicU32) -> Result<TagImage, ReadIssue> {
    let version = tag.get_version().ok_or(ReadIssue::Interrupted { page: 0 })?;
    // NTAG family (version[2] = 0x04), storage 0x11 = NTAG215
    if version[2] != 0x04 || version[6] != 0x11 {
        return Err(ReadIssue::NotNtag215);
    }
    // Blocks of 4 pages: 0, 4, ..., 128 cover pages 0-131; the last block starts at 131 so that it ends on page 134.
    let mut image = [0u8; NTAG215_SIZE];
    let starts = (0..=128u32).step_by(4).chain(std::iter::once(NTAG_PAGES - 4));
    for page in starts {
        let block = tag.read(page as u8).ok_or(ReadIssue::Interrupted { page })?;
        image[page as usize * 4..page as usize * 4 + 16].copy_from_slice(&block);
        progress.store((page + 4).min(NTAG_PAGES), SeqCst);
    }
    Ok(image)
}

/// File name (without extension) of a new backup: the character when it is in the catalogue, else the UID. Same
/// convention as `Library::create` (name_UIDsuffix); the suffix keeps two copies of one character apart.
pub fn backup_stem(entry: Option<&Entry>, image: &TagImage) -> String {
    let uid: String = [0, 1, 2, 4, 5, 6, 7].iter().map(|&i| format!("{:02X}", image[i])).collect();
    let Some(entry) = entry else { return uid };
    let mut stem: String = entry.name.bytes().map(|b| if b.is_ascii_alphanumeric() { b as char } else { '_' }).collect();
    if stem.is_empty() {
        stem = entry.id.clone();
    }
    stem.truncate(48);
    format!("{stem}_{}", &uid[6..])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanPhase {
    Opening,
    Waiting,
    Reading,
    /// The image is read and waits to be taken.
    Done,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanIssue {
    None,
    NotNtag215,
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanSnapshot {
    pub phase: ScanPhase,
    pub progress: u32,
    pub issue: ScanIssue,
    pub issue_page: u32,
    pub error: String,
}

/// State shared between the scan thread and the UI.
pub struct ScanShared {
    phase: AtomicU8,
    progress: AtomicU32,
    issue: AtomicU8,
    issue_page: AtomicU32,
    error: Mutex<String>,
    image: Mutex<Option<TagImage>>,
    stop: AtomicBool,
}

impl ScanShared {
    fn new() -> Self {
        Self {
            phase: AtomicU8::new(0),
            progress: AtomicU32::new(0),
            issue: AtomicU8::new(0),
            issue_page: AtomicU32::new(0),
            error: Mutex::new(String::new()),
            image: Mutex::new(None),
            stop: AtomicBool::new(false),
        }
    }

    fn set_phase(&self, p: ScanPhase) {
        self.phase.store(p as u8, SeqCst);
    }

    pub fn snapshot(&self) -> ScanSnapshot {
        let phase = match self.phase.load(SeqCst) {
            0 => ScanPhase::Opening,
            1 => ScanPhase::Waiting,
            2 => ScanPhase::Reading,
            3 => ScanPhase::Done,
            _ => ScanPhase::Failed,
        };
        let issue = match self.issue.load(SeqCst) {
            1 => ScanIssue::NotNtag215,
            2 => ScanIssue::Interrupted,
            _ => ScanIssue::None,
        };
        ScanSnapshot {
            phase,
            progress: self.progress.load(SeqCst),
            issue,
            issue_page: self.issue_page.load(SeqCst),
            error: self.error.lock().unwrap().clone(),
        }
    }
}

/// The scan thread: owns the chip (powered on by `open`, off again when the thread ends) and polls for a tag.
pub struct ScanSession {
    shared: Arc<ScanShared>,
    worker: Option<JoinHandle<()>>,
}

impl ScanSession {
    /// `open` powers the Cap and opens the chip; it runs on the worker thread so the UI never waits for it.
    pub fn start<B: Bus + Send + 'static>(open: impl FnOnce() -> Result<Chip<B>, Error> + Send + 'static) -> Self {
        let shared = Arc::new(ScanShared::new());
        let s = shared.clone();
        let worker = std::thread::spawn(move || {
            let chip = open().and_then(|mut c| c.init_nfc_a().map(|_| c)); // init_nfc_a makes the chip a reader
            let mut chip = match chip {
                Ok(c) => c,
                Err(e) => {
                    *s.error.lock().unwrap() = e.0;
                    s.set_phase(ScanPhase::Failed);
                    return;
                }
            };
            while !s.stop.load(SeqCst) {
                s.set_phase(ScanPhase::Waiting);
                if chip.detect_tag().is_err() {
                    // REQA/WUPA: nothing in the field yet
                    std::thread::sleep(Duration::from_millis(120));
                    continue;
                }
                s.set_phase(ScanPhase::Reading);
                s.progress.store(0, SeqCst);
                s.issue.store(0, SeqCst);
                match read_ntag215(&mut chip, &s.progress) {
                    Ok(image) => {
                        *s.image.lock().unwrap() = Some(image);
                        s.set_phase(ScanPhase::Done);
                        return;
                    }
                    Err(issue) => {
                        // Tag removed or unreadable: back to waiting, the message stays until the next attempt.
                        match issue {
                            ReadIssue::NotNtag215 => s.issue.store(1, SeqCst),
                            ReadIssue::Interrupted { page } => {
                                s.issue_page.store(page, SeqCst);
                                s.issue.store(2, SeqCst);
                            }
                        }
                        std::thread::sleep(Duration::from_millis(200));
                    }
                }
            }
        });
        Self { shared, worker: Some(worker) }
    }

    pub fn snapshot(&self) -> ScanSnapshot {
        self.shared.snapshot()
    }

    /// The image once the phase is `Done`; joins the thread (the chip is powered off then).
    pub fn finish(mut self) -> Option<TagImage> {
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
        self.shared.image.lock().unwrap().take()
    }

    /// Stops the scan and powers the Cap off.
    pub fn stop(mut self) {
        self.shared.stop.store(true, SeqCst);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

impl Drop for ScanSession {
    fn drop(&mut self) {
        self.shared.stop.store(true, SeqCst);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tag in memory. `fail_at`: the page whose READ gets no answer.
    struct FakeTag {
        mem: [u8; 540],
        version: [u8; 8],
        fail_at: Option<u8>,
        reads: Vec<u8>,
    }

    impl FakeTag {
        fn ntag215() -> Self {
            let mut mem = [0u8; 540];
            for (i, b) in mem.iter_mut().enumerate() {
                *b = (i * 7 + 3) as u8;
            }
            Self { mem, version: [0x00, 0x04, 0x04, 0x02, 0x01, 0x00, 0x11, 0x03], fail_at: None, reads: Vec::new() }
        }
    }

    impl Type2 for FakeTag {
        fn get_version(&mut self) -> Option<[u8; 8]> {
            Some(self.version)
        }
        fn read(&mut self, page: u8) -> Option<[u8; 16]> {
            self.reads.push(page);
            if self.fail_at == Some(page) {
                return None;
            }
            // like a real NTAG: 16 bytes, wrapping at the end of the memory
            Some(std::array::from_fn(|i| self.mem[(page as usize * 4 + i) % 540]))
        }
    }

    #[test]
    fn reads_the_whole_tag_in_blocks_ending_on_page_134() {
        let mut tag = FakeTag::ntag215();
        let progress = AtomicU32::new(0);
        let image = read_ntag215(&mut tag, &progress).unwrap();
        assert_eq!(image, tag.mem);
        assert_eq!(progress.load(SeqCst), 135);
        assert_eq!(tag.reads.len(), 34); // pages 0, 4, ..., 128 and 131
        assert_eq!(tag.reads.last(), Some(&131));
    }

    #[test]
    fn a_tag_that_is_not_an_ntag215_is_refused_before_any_read() {
        let mut tag = FakeTag::ntag215();
        tag.version[6] = 0x0F; // NTAG213
        assert_eq!(read_ntag215(&mut tag, &AtomicU32::new(0)), Err(ReadIssue::NotNtag215));
        tag.version = [0x00, 0x04, 0x03, 0x01, 0x01, 0x00, 0x0E, 0x03]; // Ultralight EV1
        assert_eq!(read_ntag215(&mut tag, &AtomicU32::new(0)), Err(ReadIssue::NotNtag215));
        assert!(tag.reads.is_empty());
    }

    #[test]
    fn removing_the_figure_reports_the_page_and_keeps_the_progress() {
        let mut tag = FakeTag::ntag215();
        tag.fail_at = Some(40);
        let progress = AtomicU32::new(0);
        assert_eq!(read_ntag215(&mut tag, &progress), Err(ReadIssue::Interrupted { page: 40 }));
        assert_eq!(progress.load(SeqCst), 40);
    }

    #[test]
    fn file_names_follow_the_library_convention() {
        let mut image = [0u8; 540];
        image[..8].copy_from_slice(&[0x04, 0x11, 0x22, 0xBC, 0x33, 0x44, 0x55, 0x66]);
        assert_eq!(backup_stem(None, &image), "04112233445566");
        let entry = Entry { id: "01010000000e0002".into(), name: "Link - Ocarina of Time".into(), ..Default::default() };
        assert_eq!(backup_stem(Some(&entry), &image), "Link___Ocarina_of_Time_33445566");
        let nameless = Entry { id: "0123456789abcdef".into(), ..Default::default() };
        assert_eq!(backup_stem(Some(&nameless), &image), "0123456789abcdef_33445566");
    }

    #[test]
    fn a_failing_open_ends_in_the_failed_phase_with_the_reason() {
        let s = ScanSession::start(|| -> Result<Chip<NoBus>, Error> { Err(Error("cannot open /dev/spidev0.2: No such file".into())) });
        for _ in 0..200 {
            if s.snapshot().phase == ScanPhase::Failed {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let snap = s.snapshot();
        assert_eq!(snap.phase, ScanPhase::Failed);
        assert!(snap.error.contains("spidev"));
        assert!(s.finish().is_none());
    }

    struct NoBus;
    impl Bus for NoBus {
        fn transfer(&mut self, _tx: &[u8], _rx: &mut [u8]) -> std::io::Result<()> {
            Ok(())
        }
    }
}
