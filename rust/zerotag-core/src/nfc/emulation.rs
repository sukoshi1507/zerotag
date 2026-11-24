// SPDX-License-Identifier: MIT
//! NFC-A card emulation (NTAG215) on the ST25R3916. Port of st25r3916_emulation.cpp.
//!
//! The register sequence and the passive-target state machine (off / idle / ready / active / halt) follow
//! M5Stack's M5Unit-NFC `emulation_layer_a_ST25R3916.cpp` (MIT, (c) 2025 M5Stack Technology CO LTD). The chip
//! answers REQA/WUPA/anticollision/SELECT on its own from the passive-target memory; the Type 2 command set is
//! served here, in software, from a 540-byte tag image.

use super::chip::{Bus, Chip, Error};
use super::regs::*;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};
use std::sync::Arc;
use std::time::{Duration, Instant};

const MODE_IRQ: u32 = IRQ_WU_A | IRQ_WU_AX | IRQ_RXE_PTA;
const DEFAULT_IRQ: u32 = IRQ_NFCT | IRQ_RXS | IRQ_EON | IRQ_EOF | IRQ_CRC | IRQ_ERR1 | IRQ_ERR2 | IRQ_PAR;
const MODE_BITRATE_DETECTION: u8 = TARG | (0x09 << 3); // target, NFC-A, bit rate detection
const MODE_LISTEN_NFC_A: u8 = TARG | (0x01 << 3); // target, NFC-A

const PAGES: usize = 135;
const PASSWORD_PAGE: usize = 133;
/// A read session ends when the reader has been away this long (a Flipper, for instance, reads in several short
/// sessions separated by field drops: the UID must not change between them).
pub const ROTATION_DELAY: Duration = Duration::from_secs(3);
const ACK: u8 = 0x0A;
const NAK: u8 = 0x00;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    Off,
    Idle,
    Ready,
    Active,
    Halt,
}

impl State {
    fn name(self) -> &'static str {
        match self {
            State::Off => "OFF",
            State::Idle => "IDLE",
            State::Ready => "READY",
            State::Active => "ACTIVE",
            State::Halt => "HALT",
        }
    }
}

/// What to send back to the reader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    None,
    Data(Vec<u8>),
    Ack,
    Nak,
    Halt,
}

/// Live counters, readable from another thread while the emulation runs.
#[derive(Default)]
pub struct EmulationStats {
    /// READ / FAST_READ / GET_VERSION / READ_SIG / PWD_AUTH answered.
    pub reads: AtomicU32,
    /// WRITE commands accepted (the image in memory changed).
    pub writes: AtomicU32,
    /// UID rotations done after a read session.
    pub rotations: AtomicU32,
    /// A reader field is present.
    pub field: AtomicBool,
    /// A reader has selected the tag.
    pub selected: AtomicBool,
}

type Image = [u8; 540];

#[derive(Default)]
pub struct EmulationOptions<'a> {
    pub log: Option<Box<dyn FnMut(&str) + 'a>>,
    /// Polled; true ends the emulation.
    pub stop: Option<Box<dyn Fn() -> bool + 'a>>,
    pub stats: Option<Arc<EmulationStats>>,
    /// Called (on the emulation thread) when a read session has ended, i.e. the reader field dropped after at
    /// least one answered read. It may rewrite the image in place (new UID, re-signed) and returns true on
    /// success; the chip is then reprogrammed with the new UID.
    pub rotate_uid: Option<Box<dyn FnMut(&mut Image) -> bool + 'a>>,
    /// Called each time the image changed: a WRITE or a UID rotation.
    pub on_change: Option<Box<dyn FnMut(&Image) + 'a>>,
}

fn hex(d: &[u8]) -> String {
    d.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")
}

/// NTAG215 command set. `rx` has the CRC already stripped. Returns the reply and an optional log note.
pub fn handle_type2(image: &mut Image, rx: &[u8]) -> (Reply, Option<&'static str>) {
    let n = rx.len();
    let page = |image: &Image, p: usize| -> [u8; 4] { image[p * 4..p * 4 + 4].try_into().unwrap() };
    let mut note = None;
    let reply = match rx.first().copied() {
        Some(0x30) if n == 2 => {
            // READ: 4 pages, wraps around at the end of the memory
            if rx[1] as usize >= PAGES {
                Reply::Nak
            } else {
                Reply::Data((0..16).map(|i| image[(rx[1] as usize * 4 + i) % (PAGES * 4)]).collect())
            }
        }
        Some(0x3A) if n == 3 => {
            // FAST_READ
            let (first, last) = (rx[1] as usize, rx[2] as usize);
            if first > last || last >= PAGES {
                Reply::Nak
            } else {
                Reply::Data(image[first * 4..last * 4 + 4].to_vec())
            }
        }
        // GET_VERSION: NTAG215
        Some(0x60) if n == 1 => Reply::Data(vec![0x00, 0x04, 0x04, 0x02, 0x01, 0x00, 0x11, 0x03]),
        // READ_SIG: the originality signature is not reproduced
        Some(0x3C) if n == 2 => Reply::Data(vec![0; 32]),
        Some(0x1B) if n == 5 => {
            // PWD_AUTH: always accepted, the PACK stored in the image is returned
            if rx[1..5] != page(image, PASSWORD_PAGE) {
                note = Some("  PWD_AUTH password differs from the image (accepted anyway)");
            }
            Reply::Data(page(image, PASSWORD_PAGE + 1)[..2].to_vec())
        }
        Some(0xA2) if n == 6 => {
            // WRITE one page
            if rx[1] < 2 || rx[1] as usize >= PAGES {
                Reply::Nak
            } else {
                let off = rx[1] as usize * 4;
                image[off..off + 4].copy_from_slice(&rx[2..6]);
                Reply::Ack
            }
        }
        Some(0x50) if n == 2 && rx[1] == 0x00 => Reply::Halt, // HLTA
        _ => Reply::None,
    };
    (reply, note)
}

struct Emulation<'c, 'o, B: Bus> {
    chip: &'c mut Chip<B>,
    image: &'c mut Image,
    opts: EmulationOptions<'o>,
    stored: u32,
    bitrate: u8, // 0: 106, 1: 212, 2: 424
    bitrate_valid: bool,
    data_flag: bool,
    wakeup: bool,
    state: State,
    session_reads: u32,
    /// When the UID is to be rotated: the reader left after at least one answered read.
    rotation_due: Option<Instant>,
}

fn fail(step: &str) -> Error {
    Error(format!("emulation setup failed at: {step}"))
}

impl<B: Bus> Emulation<'_, '_, B> {
    fn log(&mut self, s: &str) {
        if let Some(f) = self.opts.log.as_mut() {
            f(s);
        }
    }

    // The register helpers below ignore bus errors like the C++ driver (a lost transfer is retried by the loop).
    fn cmd(&mut self, c: u8) {
        let _ = self.chip.direct_command(c);
    }
    fn modify(&mut self, reg: u8, clear: u8, set: u8) {
        let _ = self.chip.modify_reg(reg, clear, set, false);
    }
    fn write(&mut self, reg: u8, v: u8) {
        let _ = self.chip.write_reg(reg, v, false);
    }
    fn write_b(&mut self, reg: u8, v: u8) {
        let _ = self.chip.write_reg(reg, v, true);
    }

    fn write_mask_interrupts(&mut self, mask: u32) {
        let _ = self.chip.write_regs(REG_MASK_MAIN_INTERRUPT, &mask.to_be_bytes(), false);
    }

    fn read_mask_interrupts(&mut self) -> Option<u32> {
        let mut d = [0u8; 4];
        self.chip.read_regs(REG_MASK_MAIN_INTERRUPT, &mut d, false).ok()?;
        Some(u32::from_be_bytes(d))
    }

    /// `unmask` bits are enabled (mask bit cleared), `mask` bits are disabled.
    fn modify_mask_interrupts(&mut self, unmask: u32, mask: u32) {
        if let Some(current) = self.read_mask_interrupts() {
            let next = (current & !unmask) | mask;
            if next != current {
                self.write_mask_interrupts(next);
            }
        }
    }

    fn load_pt_memory_a(&mut self, data: &[u8]) -> io::Result<()> {
        let mut tx = [0u8; 16];
        tx[0] = OP_LOAD_PT_MEMORY_A;
        tx[1..=data.len()].copy_from_slice(data);
        let mut rx = [0u8; 16];
        self.chip.bus.transfer(&tx[..=data.len()], &mut rx[..=data.len()])
    }

    /// Transmits `bits` bits (the last byte partially when bits % 8 != 0), with or without CRC.
    fn transmit_bits(&mut self, frame: &[u8], bits: usize, with_crc: bool) {
        let ok = (|| -> io::Result<()> {
            self.chip.direct_command(CMD_CLEAR_FIFO)?;
            self.chip.write_fifo(&frame[..(bits + 7) / 8])?;
            self.chip.write_regs(REG_NUMBER_OF_TRANSMITTED_BYTES1, &(bits as u16).to_be_bytes(), false)?;
            self.chip.direct_command(if with_crc { CMD_TRANSMIT_WITH_CRC } else { CMD_TRANSMIT_WITHOUT_CRC })
        })();
        let _ = ok;
    }

    /// Diagnostic: the passive-target memory as the chip holds it (UID, ATQA, SAKs), "Read PT memory A" = 0xBF.
    fn read_pt_memory_a(&mut self) -> Option<[u8; 15]> {
        let tx = [0xBFu8; 16];
        let mut rx = [0u8; 16];
        self.chip.bus.transfer(&tx, &mut rx).ok()?;
        rx[1..].try_into().ok()
    }

    /// 7-byte UID from pages 0-2 (the BCC bytes are not part of it), ATQA 0x0044, SAK 0x00.
    fn load_identity(&mut self) -> bool {
        let im = &*self.image;
        let mut pt = [0u8; 15];
        pt[..3].copy_from_slice(&im[0..3]);
        pt[3..7].copy_from_slice(&im[4..8]);
        pt[10] = 0x44; // ATQA LSB
        pt[11] = 0x00;
        pt[12] = 0x04; // SAK level 1: cascade bit set (UID incomplete)
        pt[13] = 0x00; // SAK level 2
        pt[14] = 0x00; // SAK level 3
        self.log(&format!("UID {}", hex(&pt[..7])));
        let a = self.chip.modify_reg(REG_AUXILIARY_DEFINITION, 0x30, 0x10, false).is_ok(); // 7-byte UID
        a && self.load_pt_memory_a(&pt).is_ok()
    }

    fn poll(&mut self) {
        if let Ok(v) = self.chip.read_interrupts() {
            self.stored |= v;
        }
    }

    fn take(&mut self, bits: u32) -> u32 {
        let hit = self.stored & bits;
        self.stored &= !hit;
        hit
    }

    fn field_present(&mut self) -> bool {
        matches!(self.chip.read_reg(REG_AUXILIARY_DISPLAY, false), Ok(v) if v & EXT_FIELD_DETECTED != 0)
    }

    fn read_bitrate(&mut self) {
        let v = self.chip.read_reg(REG_BITRATE_DETECTION_DISPLAY, false).unwrap_or(0);
        self.bitrate = ((v >> 4) & 0x03).min(2);
        self.bitrate_valid = true;
    }

    fn enter(&mut self, next: State) -> State {
        match next {
            State::Off => {
                self.data_flag = false;
                self.bitrate_valid = false;
                self.wakeup = false;
                self.cmd(CMD_STOP_ALL_ACTIVITIES);
                self.modify(REG_OPERATION_CONTROL, 0, OP_CONTROL_RX_EN);
                self.modify(REG_NFCIP1_PASSIVE_TARGET, NFCIP_PT_AUTO_A, 0);
                self.cmd(CMD_GO_TO_SENSE);
                self.modify(REG_ISO14443A_SETTINGS, 0x20, 0);
                self.write_mask_interrupts(0xFFFF_FFFF);
                let _ = self.chip.clear_interrupts();
                self.modify_mask_interrupts(IRQ_OSC | DEFAULT_IRQ | MODE_IRQ, 0);
                self.stored = 0;
                self.modify(REG_AUXILIARY_DEFINITION, 0, NO_CRC_RX);
                self.modify(REG_MODE_DEFINITION, 0x78, MODE_BITRATE_DETECTION & 0x78);
                if self.field_present() {
                    return State::Idle;
                }
                // The oscillator and the receiver stay on: phones poll with field bursts of a few tens of ms,
                // shorter than the oscillator start-up time, so the chip must be able to answer at once.
                self.modify(REG_OPERATION_CONTROL, OP_CONTROL_TX_EN, OP_CONTROL_EN | OP_CONTROL_RX_EN);
                State::Off
            }
            State::Idle => {
                self.data_flag = false;
                if let Ok(op) = self.chip.read_reg(REG_OPERATION_CONTROL, false) {
                    if op & OP_CONTROL_EN == 0 {
                        self.modify(REG_OPERATION_CONTROL, 0, OP_CONTROL_EN | OP_CONTROL_RX_EN);
                        if let Ok(aux) = self.chip.read_reg(REG_AUXILIARY_DISPLAY, false) {
                            if aux & OSC_OK == 0 && self.chip.wait_interrupt(IRQ_OSC, 1000) & IRQ_OSC == 0 {
                                self.log("oscillator did not start");
                                return State::Off;
                            }
                        }
                    } else {
                        self.take(IRQ_OSC);
                    }
                } else {
                    self.take(IRQ_OSC);
                }
                self.modify(REG_AUXILIARY_DEFINITION, 0, NO_CRC_RX);
                if self.state == State::Active && !self.wakeup {
                    self.modify(REG_NFCIP1_PASSIVE_TARGET, NFCIP_PT_AUTO_A, 0);
                    self.cmd(CMD_GO_TO_SENSE);
                }
                self.cmd(CMD_CLEAR_FIFO);
                self.cmd(CMD_UNMASK_RECEIVE_DATA);
                self.wakeup = false;
                State::Idle
            }
            State::Ready => {
                self.data_flag = false;
                if self.take(IRQ_EOF) != 0 {
                    return State::Off;
                }
                self.modify(REG_AUXILIARY_DEFINITION, NO_CRC_RX, 0);
                self.write(REG_BITRATE_DEFINITION, (self.bitrate << 4) | self.bitrate);
                self.modify(REG_OPERATION_CONTROL, OP_CONTROL_WU, 0);
                self.write(REG_MODE_DEFINITION, MODE_LISTEN_NFC_A);
                State::Ready
            }
            State::Active => {
                self.data_flag = false;
                self.modify(REG_NFCIP1_PASSIVE_TARGET, 0, NFCIP_PT_AUTO_A); // the software answers from now on
                self.take(IRQ_PAR | IRQ_CRC | IRQ_ERR2 | IRQ_ERR1);
                self.modify_mask_interrupts(IRQ_RXE, 0);
                State::Active
            }
            State::Halt => {
                self.data_flag = false;
                self.modify(REG_NFCIP1_PASSIVE_TARGET, NFCIP_PT_AUTO_A, 0);
                self.cmd(CMD_GO_TO_SLEEP);
                self.modify(REG_MODE_DEFINITION, 0xFB, MODE_BITRATE_DETECTION & 0xFB);
                self.modify(REG_ISO14443A_SETTINGS, 0x20, 0);
                self.cmd(CMD_UNMASK_RECEIVE_DATA);
                self.modify_mask_interrupts(
                    IRQ_NFCT | IRQ_RXS | IRQ_CRC | IRQ_ERR1 | IRQ_ERR2 | IRQ_PAR | IRQ_EON | IRQ_EOF | MODE_IRQ,
                    0,
                );
                if !self.field_present() {
                    return State::Off;
                }
                State::Halt
            }
        }
    }

    /// The reader field dropped after at least one answered read: schedules the UID rotation (it only happens once
    /// the reader has stayed away for `ROTATION_DELAY`).
    fn on_session_ended(&mut self) {
        if self.session_reads > 0 && self.opts.rotate_uid.is_some() {
            self.rotation_due = Some(Instant::now() + ROTATION_DELAY);
        }
    }

    fn rotate_if_due(&mut self) {
        if self.state == State::Off && self.rotation_due.is_some_and(|t| Instant::now() >= t) {
            self.rotate_now();
        }
    }

    /// Gives the image a new UID (the caller re-signs it) and reprograms the chip with it.
    fn rotate_now(&mut self) {
        self.rotation_due = None;
        if let Some(mut rotate) = self.opts.rotate_uid.take() {
            let ok = rotate(self.image);
            self.opts.rotate_uid = Some(rotate);
            if ok {
                if !self.load_identity() {
                    self.log("could not load the new UID into the chip");
                }
                if let Some(s) = &self.opts.stats {
                    s.rotations.fetch_add(1, Relaxed);
                }
                if let Some(f) = self.opts.on_change.as_mut() {
                    f(self.image);
                }
            } else {
                self.log("UID rotation failed, keeping the current UID");
            }
        }
        self.session_reads = 0;
    }

    fn change(&mut self, next: State) {
        let actual = self.enter(next);
        if actual != self.state {
            let msg = format!("state {} -> {}", self.state.name(), actual.name());
            self.log(&msg);
        }
        let previous = self.state;
        self.state = actual;
        if self.opts.log.is_some() && matches!(actual, State::Ready | State::Active) && actual != previous {
            if let Some(pt) = self.read_pt_memory_a() {
                self.log(&format!("  pt memory: {}", hex(&pt)));
            }
        }
        if actual == State::Off && previous != State::Off {
            self.on_session_ended();
        } else if actual != State::Off {
            self.rotation_due = None; // the reader is back: still the same session
        }
        if let Some(s) = &self.opts.stats {
            s.field.store(actual != State::Off, Relaxed);
            s.selected.store(actual == State::Active, Relaxed);
        }
    }

    fn send_reply(&mut self, r: &Reply) {
        match r {
            Reply::Data(d) => self.transmit_bits(d, d.len() * 8, true),
            Reply::Ack => self.transmit_bits(&[ACK], 4, false),
            Reply::Nak => self.transmit_bits(&[NAK], 4, false),
            Reply::None | Reply::Halt => {}
        }
    }

    fn run(&mut self, deadline: Instant) -> u32 {
        self.change(State::Off);
        self.log("emulation running, present a reader");
        let mut frames = 0u32;
        while Instant::now() < deadline && !self.opts.stop.as_ref().is_some_and(|s| s()) {
            self.poll();
            self.rotate_if_due();
            match self.state {
                State::Off => {
                    if self.take(IRQ_EON) != 0 {
                        self.change(State::Idle);
                    }
                }
                State::Idle => self.step_idle(),
                State::Ready => {
                    let mask = IRQ_EOF | if self.wakeup { IRQ_WU_AX } else { IRQ_WU_A };
                    let irq = self.take(mask);
                    if irq != 0 {
                        self.change(if irq & IRQ_EOF != 0 { State::Off } else { State::Active });
                    }
                }
                State::Active => self.step_active(&mut frames),
                State::Halt => self.step_halt(),
            }
        }
        frames
    }

    fn step_idle(&mut self) {
        let irq = self.take(IRQ_NFCT | IRQ_RXE | IRQ_EOF | IRQ_RXE_PTA);
        if irq == 0 {
            return;
        }
        if irq & IRQ_NFCT != 0 {
            self.read_bitrate();
        }
        if irq & IRQ_EOF != 0 && !self.data_flag {
            self.change(State::Off);
            return;
        }
        if irq & IRQ_RXE != 0 && self.bitrate_valid {
            self.data_flag = true;
            self.cmd(CMD_CLEAR_FIFO);
            self.cmd(CMD_UNMASK_RECEIVE_DATA);
            return;
        }
        if irq & IRQ_RXE_PTA != 0 && self.bitrate_valid && self.bitrate == 0 {
            if let Ok(pta) = self.chip.read_reg(REG_PASSIVE_TARGET_DISPLAY, false) {
                if pta & 0x0F > PTA_STATE_IDLE {
                    self.change(State::Ready);
                }
            }
        }
    }

    fn step_halt(&mut self) {
        let irq = self.take(IRQ_NFCT | IRQ_RXE | IRQ_EOF | IRQ_RXE_PTA);
        if irq == 0 {
            return;
        }
        if irq & IRQ_NFCT != 0 {
            self.read_bitrate();
        }
        if irq & IRQ_EOF != 0 {
            self.change(State::Off);
            return;
        }
        if irq & IRQ_RXE != 0 && self.bitrate_valid {
            self.cmd(CMD_CLEAR_FIFO);
            self.cmd(CMD_UNMASK_RECEIVE_DATA);
            return;
        }
        if irq & IRQ_RXE_PTA != 0 && self.bitrate_valid && self.bitrate == 0 {
            if let Ok(pta) = self.chip.read_reg(REG_PASSIVE_TARGET_DISPLAY, false) {
                if pta & 0x0F > PTA_STATE_HALT {
                    self.wakeup = true;
                    self.change(State::Ready);
                }
            }
        }
    }

    fn step_active(&mut self, frames: &mut u32) {
        let irq = self.take(IRQ_EOF | IRQ_RXE);
        if irq == 0 {
            return;
        }
        if irq & IRQ_EOF != 0 {
            self.change(State::Off);
            return;
        }
        let err = self.take(IRQ_PAR | IRQ_CRC | IRQ_ERR2 | IRQ_ERR1);
        let mut rx = [0u8; 64];
        let mut n = 0;
        if self.chip.fifo_size().unwrap_or(0) > 0 {
            n = self.chip.read_fifo(&mut rx).unwrap_or(0);
        }
        if err != 0 || n <= 2 {
            self.log(&format!("  rx error irq={} len={}", err >> 8, n));
            self.cmd(CMD_CLEAR_FIFO);
            self.cmd(CMD_UNMASK_RECEIVE_DATA);
            self.change(if self.wakeup { State::Halt } else { State::Idle });
            return;
        }
        let n = n - 2; // CRC_A
        *frames += 1;
        let (reply, note) = handle_type2(self.image, &rx[..n]);
        if let Some(note) = note {
            self.log(note);
        }
        self.send_reply(&reply);
        match &reply {
            Reply::Data(_) => {
                self.session_reads += 1;
                if let Some(s) = &self.opts.stats {
                    s.reads.fetch_add(1, Relaxed);
                }
            }
            Reply::Ack => {
                if let Some(s) = &self.opts.stats {
                    s.writes.fetch_add(1, Relaxed);
                }
                if let Some(f) = self.opts.on_change.as_mut() {
                    f(self.image);
                }
            }
            _ => {}
        }
        let what = match &reply {
            Reply::Data(d) => format!("  -> {} bytes", d.len()),
            Reply::Ack => "  -> ACK".into(),
            Reply::Nak => "  -> NAK".into(),
            Reply::Halt => "  -> HLTA".into(),
            Reply::None => "  -> (no answer)".into(),
        };
        self.log(&format!("  rx {}{}", hex(&rx[..n]), what));
        if reply == Reply::Halt {
            self.modify_mask_interrupts(0, IRQ_RXE);
            self.change(State::Halt);
        }
    }
}

impl<B: Bus> Chip<B> {
    /// Emulates the NTAG215 in `image` for `seconds` (0: until `options.stop` returns true). Writes from the reader
    /// modify the image in place; what happens is reported through `options`.
    pub fn emulate_ntag215(&mut self, image: &mut Image, seconds: u64, options: EmulationOptions<'_>) -> Result<(), Error> {
        // Reader-mode init brings the chip into a known state (oscillator, regulators, receiver); the emulation
        // then reconfigures it as a passive target.
        self.init_nfc_a()?;
        self.field_off();

        let mut emu = Emulation {
            chip: self,
            image,
            opts: options,
            stored: 0,
            bitrate: 0,
            bitrate_valid: false,
            data_flag: false,
            wakeup: false,
            state: State::Off,
            session_reads: 0,
            rotation_due: None,
        };
        if !emu.load_identity() {
            return Err(fail("passive target memory"));
        }
        emu.write(REG_MODE_DEFINITION, MODE_BITRATE_DETECTION); // target, NFC-A, bit rate detection
        // Automatic answers for NFC-A only.
        emu.modify(REG_NFCIP1_PASSIVE_TARGET, 0x0B, 0x0C);
        emu.modify(REG_TIMER_EMV_CONTROL, 0xE0, 0x08); // MRT step 512/fc
        emu.write(REG_MASK_RECEIVER_TIMER, 4); // ~100 us
        emu.modify(REG_ISO14443A_SETTINGS, 0xE0, 0x00); // parity on, no NFCIP-1
        emu.write(REG_ANTENNA_TUNING_CONTROL1, 0x00);
        emu.write(REG_ANTENNA_TUNING_CONTROL2, 0xFF);
        emu.write_b(REG_OVERSHOOT_PROTECTION1, 0x00);
        emu.write_b(REG_OVERSHOOT_PROTECTION2, 0x00);
        emu.write_b(REG_UNDERSHOOT_PROTECTION1, 0x00);
        emu.write_b(REG_UNDERSHOOT_PROTECTION2, 0x00);
        emu.cmd(CMD_UNMASK_RECEIVE_DATA);

        let deadline = Instant::now() + Duration::from_secs(if seconds > 0 { seconds } else { 24 * 3600 });
        let frames = emu.run(deadline);

        if emu.rotation_due.is_some() {
            emu.rotate_now(); // stopped while a rotation was pending: the saved image carries the new UID
        }
        if let Some(s) = &emu.opts.stats {
            s.field.store(false, Relaxed);
            s.selected.store(false, Relaxed);
        }
        emu.log(&format!("emulation finished, {frames} command frame(s) served"));

        // Leave the chip in a quiet state.
        emu.cmd(CMD_STOP_ALL_ACTIVITIES);
        emu.write(REG_MODE_DEFINITION, 0x00);
        emu.modify(REG_NFCIP1_PASSIVE_TARGET, 0, 0x0D);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image() -> Image {
        let mut im = [0u8; 540];
        for (i, b) in im.iter_mut().enumerate() {
            *b = i as u8;
        }
        im[133 * 4..133 * 4 + 4].copy_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]); // password
        im[134 * 4..134 * 4 + 2].copy_from_slice(&[0x80, 0x80]); // PACK
        im
    }

    #[test]
    fn read_returns_16_bytes_and_wraps() {
        let mut im = image();
        assert_eq!(handle_type2(&mut im, &[0x30, 4]).0, Reply::Data((16..32u8).collect()));
        let Reply::Data(d) = handle_type2(&mut im, &[0x30, 133]).0 else { panic!() };
        assert_eq!(d.len(), 16);
        assert_eq!(&d[8..12], &im[0..4]); // wrapped to page 0 after page 134
        assert_eq!(handle_type2(&mut im, &[0x30, 135]).0, Reply::Nak);
    }

    #[test]
    fn fast_read_get_version_sig() {
        let mut im = image();
        assert_eq!(handle_type2(&mut im, &[0x3A, 4, 5]).0, Reply::Data(im[16..24].to_vec()));
        assert_eq!(handle_type2(&mut im, &[0x3A, 5, 4]).0, Reply::Nak);
        assert_eq!(handle_type2(&mut im, &[0x3A, 0, 135]).0, Reply::Nak);
        assert_eq!(handle_type2(&mut im, &[0x60]).0, Reply::Data(vec![0, 4, 4, 2, 1, 0, 0x11, 3]));
        assert_eq!(handle_type2(&mut im, &[0x3C, 0]).0, Reply::Data(vec![0; 32]));
    }

    #[test]
    fn pwd_auth_always_accepted_returns_pack() {
        let mut im = image();
        let (r, note) = handle_type2(&mut im, &[0x1B, 0xAA, 0xBB, 0xCC, 0xDD]);
        assert_eq!((r, note), (Reply::Data(vec![0x80, 0x80]), None));
        let (r, note) = handle_type2(&mut im, &[0x1B, 1, 2, 3, 4]);
        assert_eq!(r, Reply::Data(vec![0x80, 0x80]));
        assert!(note.is_some());
    }

    #[test]
    fn write_changes_the_image_but_not_the_header_pages() {
        let mut im = image();
        assert_eq!(handle_type2(&mut im, &[0xA2, 4, 9, 8, 7, 6]).0, Reply::Ack);
        assert_eq!(&im[16..20], &[9, 8, 7, 6]);
        let before = im;
        assert_eq!(handle_type2(&mut im, &[0xA2, 1, 9, 8, 7, 6]).0, Reply::Nak);
        assert_eq!(handle_type2(&mut im, &[0xA2, 135, 9, 8, 7, 6]).0, Reply::Nak);
        assert_eq!(im, before);
    }

    #[test]
    fn halt_and_garbage() {
        let mut im = image();
        assert_eq!(handle_type2(&mut im, &[0x50, 0x00]).0, Reply::Halt);
        assert_eq!(handle_type2(&mut im, &[0x50, 0x01]).0, Reply::None);
        assert_eq!(handle_type2(&mut im, &[0x30]).0, Reply::None); // wrong length
        assert_eq!(handle_type2(&mut im, &[]).0, Reply::None);
        assert_eq!(handle_type2(&mut im, &[0xEE, 1, 2]).0, Reply::None);
    }
}
