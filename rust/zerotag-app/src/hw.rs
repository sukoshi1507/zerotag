// SPDX-License-Identifier: MIT
//! What the application needs from the hardware, as a trait (so the logic can be tested without a Cap), and the
//! real implementation on top of zerotag-core.

use std::path::Path;
use std::sync::atomic::Ordering::Relaxed;
use std::time::Duration;
use zerotag_core::cap_setup::{self, CapSetupResult};
use zerotag_core::nfc::{open_cap, CapConfig, EmulationSession, RotateUid, SessionResult, SpiBus};

pub type Image = [u8; 540];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LiveStats {
    pub reads: u32,
    pub writes: u32,
    pub rotations: u32,
    pub field: bool,
    pub selected: bool,
}

pub trait Hardware {
    /// The SPI device of the Cap exists (the overlay is loaded).
    fn spi_available(&self) -> bool;
    /// Opens the chip and returns its revision.
    fn probe(&mut self) -> Result<u8, String>;
    fn start_emulation(&mut self, image: &Image, rotate: Option<RotateUid>) -> Result<(), String>;
    fn emulation_running(&self) -> bool;
    fn stats(&self) -> LiveStats;
    /// The image as the reader currently sees it (UID included).
    fn current_image(&self) -> Option<Image>;
    fn poll_autosave(&mut self, quiet: Duration) -> Option<Image>;
    /// Stops the emulation, powers the Cap off and returns the outcome (`None` when nothing was running).
    fn stop_emulation(&mut self) -> Option<SessionResult>;
    fn run_cap_setup(&mut self, password: &mut String) -> CapSetupResult;
}

#[derive(Default)]
pub struct RealHardware {
    cfg: CapConfig,
    session: Option<EmulationSession<SpiBus>>,
}

impl Hardware for RealHardware {
    fn spi_available(&self) -> bool {
        Path::new(&self.cfg.spi_device).exists()
    }

    fn probe(&mut self) -> Result<u8, String> {
        open_cap(&self.cfg).map(|chip| chip.revision()).map_err(|e| e.0)
    }

    fn start_emulation(&mut self, image: &Image, rotate: Option<RotateUid>) -> Result<(), String> {
        let chip = open_cap(&self.cfg).map_err(|e| e.0)?;
        let log: Option<zerotag_core::nfc::LogFn> = std::env::var_os("ZEROTAG_DEBUG")
            .map(|_| Box::new(|m: &str| eprintln!("emu: {m}")) as zerotag_core::nfc::LogFn);
        self.session = Some(EmulationSession::start(chip, image, 0, rotate, log));
        Ok(())
    }

    fn emulation_running(&self) -> bool {
        self.session.as_ref().is_some_and(|s| s.running())
    }

    fn stats(&self) -> LiveStats {
        self.session.as_ref().map_or_else(LiveStats::default, |s| {
            let st = s.stats();
            LiveStats {
                reads: st.reads.load(Relaxed),
                writes: st.writes.load(Relaxed),
                rotations: st.rotations.load(Relaxed),
                field: st.field.load(Relaxed),
                selected: st.selected.load(Relaxed),
            }
        })
    }

    fn current_image(&self) -> Option<Image> {
        self.session.as_ref().map(|s| s.current_image())
    }

    fn poll_autosave(&mut self, quiet: Duration) -> Option<Image> {
        self.session.as_ref().and_then(|s| s.poll_autosave(quiet))
    }

    fn stop_emulation(&mut self) -> Option<SessionResult> {
        let session = self.session.take()?;
        session.request_stop();
        let (chip, result) = session.finish();
        drop(chip); // powers the Cap off
        Some(result)
    }

    fn run_cap_setup(&mut self, password: &mut String) -> CapSetupResult {
        cap_setup::run_cap_setup(password)
    }
}
