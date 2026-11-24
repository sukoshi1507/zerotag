// SPDX-License-Identifier: MIT
//! Runs the NTAG215 emulation on a worker thread so a UI can show live counters, and hands back the final image
//! so the caller can store what the reader wrote (port of emulation_session.cpp).

use super::chip::{Bus, Chip};
use super::emulation::{EmulationOptions, EmulationStats};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub type Image = [u8; 540];
pub type RotateUid = Box<dyn FnMut(&mut Image) -> bool + Send>;

#[derive(Debug, Clone)]
pub struct SessionResult {
    /// `Some(reason)` when the emulation could not run.
    pub error: Option<String>,
    /// The reader wrote to the tag (or the UID was rotated).
    pub changed: bool,
    /// Final content (equal to the initial one when not `changed`).
    pub image: Image,
}

struct Snapshot {
    image: Image,
    dirty: bool,
    last_change: Instant,
}

pub struct EmulationSession<B: Bus + Send + 'static> {
    stats: Arc<EmulationStats>,
    stop: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    snapshot: Arc<Mutex<Snapshot>>,
    worker: Option<JoinHandle<(Chip<B>, SessionResult)>>,
}

impl<B: Bus + Send + 'static> EmulationSession<B> {
    /// Takes the chip for the duration of the session; `finish` gives it back.
    /// `seconds == 0`: run until `request_stop`. `rotate_uid` is called after each read session.
    pub fn start(mut chip: Chip<B>, image: &Image, seconds: u64, rotate_uid: Option<RotateUid>) -> Self {
        let stats = Arc::new(EmulationStats::default());
        let stop = Arc::new(AtomicBool::new(false));
        let running = Arc::new(AtomicBool::new(true));
        let snapshot = Arc::new(Mutex::new(Snapshot { image: *image, dirty: false, last_change: Instant::now() }));
        let initial = *image;
        let mut work = *image;
        let worker = {
            let (stats, stop, running, snapshot) = (stats.clone(), stop.clone(), running.clone(), snapshot.clone());
            std::thread::spawn(move || {
                let on_change: Box<dyn FnMut(&Image) + Send> = {
                    let snapshot = snapshot.clone();
                    Box::new(move |data: &Image| {
                        let mut s = snapshot.lock().unwrap();
                        s.image = *data;
                        s.dirty = true;
                        s.last_change = Instant::now();
                    })
                };
                let options = EmulationOptions {
                    log: None,
                    stop: Some(Box::new(move || stop.load(Relaxed))),
                    stats: Some(stats),
                    rotate_uid: rotate_uid.map(|f| f as Box<dyn FnMut(&mut Image) -> bool>),
                    on_change: Some(on_change),
                };
                let outcome = chip.emulate_ntag215(&mut work, seconds, options);
                running.store(false, Relaxed);
                let result =
                    SessionResult { error: outcome.err().map(|e| e.0), changed: work != initial, image: work };
                (chip, result)
            })
        };
        Self { stats, stop, running, snapshot, worker: Some(worker) }
    }

    pub fn request_stop(&self) {
        self.stop.store(true, Relaxed);
    }

    pub fn running(&self) -> bool {
        self.running.load(Relaxed)
    }

    pub fn stats(&self) -> &EmulationStats {
        &self.stats
    }

    /// Autosave support: `Some(image)` once when the tag changed (a write or a UID rotation) and nothing changed
    /// for `quiet` (the reader is done with its burst of writes).
    pub fn poll_autosave(&self, quiet: Duration) -> Option<Image> {
        let mut s = self.snapshot.lock().unwrap();
        if !s.dirty || s.last_change.elapsed() < quiet {
            return None;
        }
        s.dirty = false;
        Some(s.image)
    }

    /// The image as the reader currently sees it (UID included).
    pub fn current_image(&self) -> Image {
        self.snapshot.lock().unwrap().image
    }

    /// Waits for the worker to end (after `request_stop` or the timeout); returns the chip and the outcome.
    pub fn finish(mut self) -> (Chip<B>, SessionResult) {
        self.worker.take().expect("finish consumes the session").join().expect("emulation thread panicked")
    }
}

impl<B: Bus + Send + 'static> Drop for EmulationSession<B> {
    fn drop(&mut self) {
        self.stop.store(true, Relaxed);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}
