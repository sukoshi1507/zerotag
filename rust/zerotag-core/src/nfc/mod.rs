// SPDX-License-Identifier: MIT
//! ST25R3916 NFC front end on the M5Stack Cap CC1101, driven from Linux userspace.
//!
//! Topology verified on a CardputerZero:
//!  - SPI0, mode 1, kernel-managed CS on GPIO22. /dev/spidev0.2 only exists once the
//!    `spi0-spidev2-gpio22-overlay` device-tree overlay is loaded (root, `sudo /usr/lib/zerotag/cap-setup`).
//!  - The Cap is unpowered until GPIO26 (POWER_EN) is high and /sys/class/leds/ext_5v_out is on.
//!    Without power every register reads 0x00.

mod chip;
mod emulation;
mod hw;
mod session;
pub mod regs;

pub use chip::{Bus, Chip, Error, Tag};
pub use emulation::{handle_type2, EmulationOptions, EmulationStats, Reply};
pub use session::{EmulationSession, RotateUid, SessionResult};
pub use hw::{open_cap, CapConfig, SpiBus};
