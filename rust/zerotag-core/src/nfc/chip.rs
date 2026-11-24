// SPDX-License-Identifier: MIT
//! ST25R3916 driver: register access, NFC-A reader mode, Type 2 commands.
//! Port of src/platform/st25r3916.cpp; init sequence after M5Stack's M5Unit-NFC (MIT).

use super::regs::*;
use std::any::Any;
use std::fmt;
use std::io;
use std::thread::sleep;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error(format!("SPI transfer failed: {e}"))
    }
}

/// Full-duplex SPI transfer: `rx` has the same length as `tx`.
pub trait Bus {
    fn transfer(&mut self, tx: &[u8], rx: &mut [u8]) -> io::Result<()>;
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tag {
    pub uid: Vec<u8>,
    pub atqa: u16,
    pub sak: u8,
}

const IDENTITY_TYPE_MASK: u8 = 0xF8;
const IDENTITY_TYPE_SHIFTED: u8 = 0x28;
const MAX_FRAME: usize = 40;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

pub struct Chip<B: Bus> {
    pub(crate) bus: B,
    identity: u8,
    // Dropped after `bus`: releases the Cap power rail once the chip is closed.
    _guard: Option<Box<dyn Any>>,
}

impl<B: Bus> Drop for Chip<B> {
    fn drop(&mut self) {
        self.field_off();
    }
}

impl<B: Bus> Chip<B> {
    /// Checks the IC identity register. `guard` is kept alive for the life of the chip (power rail).
    pub fn open(mut bus: B, guard: Option<Box<dyn Any>>) -> Result<Self, Error> {
        let tx = [OP_READ_REGISTER | REG_IC_IDENTITY, 0];
        let mut rx = [0u8; 2];
        bus.transfer(&tx, &mut rx)?;
        let identity = rx[1];
        if identity & IDENTITY_TYPE_MASK != IDENTITY_TYPE_SHIFTED {
            return Err(Error(format!(
                "unexpected IC identity 0x{identity:02X} (expected type bits 0x28): is the Cap CC1101 installed and powered?"
            )));
        }
        Ok(Self { bus, identity, _guard: guard })
    }

    /// Raw value of register 0x3F: type in bits 7..3 (0x28 expected), revision in 2..0.
    pub fn ic_identity(&self) -> u8 {
        self.identity
    }

    pub fn revision(&self) -> u8 {
        self.identity & 0x07
    }

    // ---- register access. Frame: [opcode|address] [data...]; space B has a 0xFB prefix; a direct command is 1 byte.

    fn xfer(&mut self, tx: &[u8]) -> io::Result<()> {
        let mut rx = [0u8; MAX_FRAME + 2];
        self.bus.transfer(tx, &mut rx[..tx.len()])
    }

    fn too_long() -> io::Error {
        io::Error::new(io::ErrorKind::InvalidInput, "frame too long")
    }

    pub(crate) fn write_regs(&mut self, reg: u8, data: &[u8], space_b: bool) -> io::Result<()> {
        if data.len() + 2 > MAX_FRAME {
            return Err(Self::too_long());
        }
        let mut tx = [0u8; MAX_FRAME];
        let mut n = 0;
        if space_b {
            tx[n] = PREFIX_SPACE_B;
            n += 1;
        }
        tx[n] = OP_WRITE_REGISTER | (reg & 0x3F);
        n += 1;
        tx[n..n + data.len()].copy_from_slice(data);
        n += data.len();
        self.xfer(&tx[..n])
    }

    pub(crate) fn read_regs(&mut self, reg: u8, out: &mut [u8], space_b: bool) -> io::Result<()> {
        if out.len() + 2 > MAX_FRAME {
            return Err(Self::too_long());
        }
        let mut tx = [0u8; MAX_FRAME];
        let mut rx = [0u8; MAX_FRAME];
        let mut head = 0;
        if space_b {
            tx[head] = PREFIX_SPACE_B;
            head += 1;
        }
        tx[head] = OP_READ_REGISTER | (reg & 0x3F);
        head += 1;
        let total = head + out.len();
        self.bus.transfer(&tx[..total], &mut rx[..total])?;
        out.copy_from_slice(&rx[head..total]);
        Ok(())
    }

    pub(crate) fn write_reg(&mut self, reg: u8, value: u8, space_b: bool) -> io::Result<()> {
        self.write_regs(reg, &[value], space_b)
    }

    pub(crate) fn read_reg(&mut self, reg: u8, space_b: bool) -> io::Result<u8> {
        let mut v = [0u8];
        self.read_regs(reg, &mut v, space_b)?;
        Ok(v[0])
    }

    pub(crate) fn modify_reg(&mut self, reg: u8, clear: u8, set: u8, space_b: bool) -> io::Result<()> {
        let v = self.read_reg(reg, space_b)?;
        self.write_reg(reg, (v & !clear) | set, space_b)
    }

    pub(crate) fn direct_command(&mut self, cmd: u8) -> io::Result<()> {
        self.xfer(&[cmd])
    }

    /// Reads one register of space A.
    pub fn read_register(&mut self, address: u8) -> io::Result<u8> {
        self.read_reg(address, false)
    }

    // The error interrupt register is cleared by reading the main one, so it is read first.
    pub(crate) fn read_interrupts(&mut self) -> io::Result<u32> {
        let error = self.read_reg(REG_ERROR_INTERRUPT, false)?;
        let mut mt = [0u8; 2];
        self.read_regs(REG_MAIN_INTERRUPT, &mut mt, false)?;
        let passive = self.read_reg(REG_PASSIVE_TARGET_INTERRUPT, false)?;
        Ok((mt[0] as u32) << 24 | (mt[1] as u32) << 16 | (error as u32) << 8 | passive as u32)
    }

    pub(crate) fn clear_interrupts(&mut self) -> io::Result<()> {
        self.read_interrupts().map(|_| ())
    }

    /// Polls the interrupt registers (the IRQ pin is not used). Returns the accumulated bits once one matches
    /// `mask`, or the bits seen so far when the timeout elapses.
    pub(crate) fn wait_interrupt(&mut self, mask: u32, timeout_ms: u64) -> u32 {
        let deadline = Instant::now() + ms(timeout_ms);
        let mut seen = 0;
        loop {
            if let Ok(v) = self.read_interrupts() {
                seen |= v;
                if seen & mask != 0 {
                    return seen;
                }
            }
            if Instant::now() >= deadline {
                return seen;
            }
        }
    }

    pub(crate) fn fifo_size(&mut self) -> io::Result<u16> {
        let mut s = [0u8; 2];
        self.read_regs(REG_FIFO_STATUS1, &mut s, false)?;
        Ok(s[0] as u16 | ((s[1] as u16 & 0xC0) << 2))
    }

    /// Reads at most `buf.len()` bytes of the FIFO; an empty FIFO is an error.
    pub(crate) fn read_fifo(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let bytes = self.fifo_size()? as usize;
        if bytes == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "FIFO empty"));
        }
        let n = bytes.min(buf.len());
        if n + 1 > MAX_FRAME {
            return Err(Self::too_long());
        }
        let mut tx = [0u8; MAX_FRAME];
        let mut rx = [0u8; MAX_FRAME];
        tx[0] = OP_READ_FIFO;
        self.bus.transfer(&tx[..n + 1], &mut rx[..n + 1])?;
        buf[..n].copy_from_slice(&rx[1..n + 1]);
        Ok(n)
    }

    /// Each load appends to the FIFO, so long frames are sent in chunks that fit the transfer buffer.
    pub(crate) fn write_fifo(&mut self, data: &[u8]) -> io::Result<()> {
        for chunk in data.chunks(32) {
            let mut tx = [0u8; 33];
            tx[0] = OP_LOAD_FIFO;
            tx[1..=chunk.len()].copy_from_slice(chunk);
            self.xfer(&tx[..=chunk.len()])?;
        }
        Ok(())
    }

    /// No-response timer, 64/fc steps (4.72 us), as selected by the default timer control value.
    pub(crate) fn set_no_response_timer_ms(&mut self, ms: u64) -> io::Result<()> {
        let steps = (ms * 1000 * 13_560_000 + 64 * 1_000_000 - 1) / (64 * 1_000_000);
        let nrt = steps.clamp(1, 0xFFFF) as u16;
        self.write_regs(REG_NO_RESPONSE_TIMER1, &nrt.to_be_bytes(), false)
    }

    // ---- initialisation (M5Unit-NFC power-on sequence)

    fn enable_oscillator(&mut self) -> bool {
        if self.modify_reg(REG_OPERATION_CONTROL, 0, OP_CONTROL_EN, false).is_err() {
            return false;
        }
        let deadline = Instant::now() + ms(50);
        loop {
            if let Ok(aux) = self.read_reg(REG_AUXILIARY_DISPLAY, false) {
                if aux & OSC_OK != 0 {
                    return true;
                }
            }
            if Instant::now() >= deadline {
                return false;
            }
        }
    }

    /// Brings the chip out of reset, starts the oscillator and configures NFC-A at 106 kbit/s.
    pub fn init_nfc_a(&mut self) -> Result<(), Error> {
        fn fail(step: &str) -> Error {
            Error(format!("ST25R3916 init failed at: {step}"))
        }
        // Register writes whose failure the C++ driver also ignores.
        macro_rules! w {
            ($r:expr, $v:expr) => {
                let _ = self.write_reg($r, $v, false);
            };
            ($r:expr, $v:expr, B) => {
                let _ = self.write_reg($r, $v, true);
            };
        }
        let _ = self.direct_command(CMD_STOP_ALL_ACTIVITIES);
        let _ = self.modify_reg(REG_OPERATION_CONTROL, OP_CONTROL_TX_EN | OP_CONTROL_RX_EN, 0, false);
        sleep(ms(2));

        self.direct_command(CMD_SET_DEFAULT).map_err(|_| fail("set default"))?;
        sleep(ms(2));
        // Prevents the internal overheat protection from triggering (datasheet: FCh / 04h / 10h).
        self.xfer(&[PREFIX_TEST_ACCESS, 0x04, 0x10]).map_err(|_| fail("test access"))?;

        // I/O: SPI, MISO pull-downs, VDD 3.3 V (as M5Unit-NFC does by default).
        let io_ok = self
            .write_reg(REG_IO_CONFIGURATION2, MISO_PD1 | MISO_PD2 | IO_DRV_LVL | SUP_3V, false)
            .and_then(|_| self.write_reg(REG_TX_DRIVER, 13 << 4, false))
            .and_then(|_| self.modify_reg(REG_IO_CONFIGURATION1, 0, 0x07, false));
        io_ok.map_err(|_| fail("io configuration"))?;
        w!(REG_RESISTIVE_AM_MODULATION, 0x80, B);
        let _ = self.modify_reg(REG_IO_CONFIGURATION2, 0, AAT_EN, false);
        w!(REG_RESISTIVE_AM_MODULATION, 0x00, B);
        w!(REG_FIELD_DETECTOR_ACTIVATION, 0x13);
        w!(REG_FIELD_DETECTOR_DEACTIVATION, 0x02);
        let _ = self.modify_reg(REG_NFCIP1_PASSIVE_TARGET, 0xF0, 0x50, false);
        w!(REG_PASSIVE_TARGET_MODULATION, 0x5F);
        w!(REG_EMD_SUPPRESSION, 0x40, B);
        w!(REG_ANTENNA_TUNING_CONTROL1, 0x82);
        w!(REG_ANTENNA_TUNING_CONTROL2, 0x82);
        let _ = self.modify_reg(REG_OPERATION_CONTROL, 0, 0x03, false); // external field detector automatic
        let _ = self.direct_command(CMD_CLEAR_FIFO);

        if !self.enable_oscillator() {
            return Err(fail("oscillator"));
        }
        let _ = self.direct_command(CMD_ADJUST_REGULATORS);
        sleep(ms(5));

        // NFC-A, 106 kbit/s in both directions.
        let mode_ok = self
            .write_reg(REG_MODE_DEFINITION, 0x08 | 0x01, false)
            .and_then(|_| self.write_reg(REG_BITRATE_DEFINITION, 0x00, false))
            .and_then(|_| self.write_reg(REG_ISO14443A_SETTINGS, 0x00, false));
        mode_ok.map_err(|_| fail("nfc-a mode"))?;
        let _ = self.modify_reg(REG_AUXILIARY_DEFINITION, DIS_CORR, 0, false);
        w!(REG_OVERSHOOT_PROTECTION1, 0x40, B);
        w!(REG_OVERSHOOT_PROTECTION2, 0x03, B);
        w!(REG_UNDERSHOOT_PROTECTION1, 0x40, B);
        w!(REG_UNDERSHOOT_PROTECTION2, 0x03, B);
        w!(REG_CORRELATOR_CONFIGURATION1, 0x47, B);
        w!(REG_CORRELATOR_CONFIGURATION2, 0x00, B);
        let rx_ok = self
            .write_reg(REG_RECEIVER_CONFIGURATION1, 0x08, false)
            .and_then(|_| self.write_reg(REG_RECEIVER_CONFIGURATION2, 0x2D, false))
            .and_then(|_| self.write_reg(REG_RECEIVER_CONFIGURATION3, 0xD8, false))
            .and_then(|_| self.write_reg(REG_RECEIVER_CONFIGURATION4, 0x22, false));
        rx_ok.map_err(|_| fail("receiver"))?;
        let _ = self.direct_command(CMD_RESET_RX_GAIN);

        // Switch the RF field on.
        let _ = self.direct_command(CMD_NFC_INITIAL_FIELD_ON);
        sleep(ms(5));
        self.modify_reg(REG_OPERATION_CONTROL, 0, OP_CONTROL_TX_EN | OP_CONTROL_RX_EN, false)
            .map_err(|_| fail("field on"))?;
        sleep(ms(10));
        Ok(())
    }

    pub fn field_off(&mut self) {
        let _ = self.direct_command(CMD_STOP_ALL_ACTIVITIES);
        let _ = self.modify_reg(REG_OPERATION_CONTROL, OP_CONTROL_TX_EN | OP_CONTROL_RX_EN, 0, false);
    }

    // ---- ISO 14443-3A: REQA/WUPA, anticollision, select

    /// Sends `frame`, waits for `wait_mask`, reads the answer. Returns the interrupt bits seen and the answer
    /// length (None when the transfer failed or no answer came).
    fn transmit(
        &mut self,
        frame: &[u8],
        with_crc: bool,
        wait_mask: u32,
        timeout_ms: u64,
        rx: &mut [u8],
    ) -> (u32, Option<usize>) {
        let sent = (|| -> io::Result<()> {
            self.clear_interrupts()?;
            self.direct_command(CMD_CLEAR_FIFO)?;
            self.write_fifo(frame)?;
            let bits = (frame.len() << 3) as u16;
            self.write_regs(REG_NUMBER_OF_TRANSMITTED_BYTES1, &bits.to_be_bytes(), false)?;
            self.direct_command(if with_crc { CMD_TRANSMIT_WITH_CRC } else { CMD_TRANSMIT_WITHOUT_CRC })
        })();
        if sent.is_err() {
            return (0, None);
        }
        let irq = self.wait_interrupt(wait_mask, timeout_ms);
        if irq & IRQ_RXE == 0 {
            return (irq, None);
        }
        (irq, self.read_fifo(rx).ok())
    }

    fn request(&mut self, wakeup: bool) -> Option<u16> {
        self.set_no_response_timer_ms(10).ok()?;
        self.write_reg(REG_ISO14443A_SETTINGS, ANTCL, false).ok()?;
        self.modify_reg(REG_AUXILIARY_DEFINITION, 0, NO_CRC_RX, false).ok()?;
        self.clear_interrupts().ok()?;
        self.direct_command(CMD_CLEAR_FIFO).ok()?;
        self.direct_command(if wakeup { CMD_TRANSMIT_WUPA } else { CMD_TRANSMIT_REQA }).ok()?;
        let irq = self.wait_interrupt(IRQ_RXE, 15);
        let mut rx = [0u8; 2];
        if irq & IRQ_RXE == 0 || self.read_fifo(&mut rx).ok()? != 2 {
            return None;
        }
        Some(rx[0] as u16 | (rx[1] as u16) << 8)
    }

    fn anticollision(&mut self, level: u8) -> Result<[u8; 5], String> {
        let setup = self
            .set_no_response_timer_ms(10)
            .and_then(|_| self.write_reg(REG_ISO14443A_SETTINGS, ANTCL, false))
            .and_then(|_| self.modify_reg(REG_AUXILIARY_DEFINITION, NO_CRC_RX, 0, false));
        if setup.is_err() {
            return Err("anticollision setup failed".into());
        }
        let frame = [0x91 + level * 2, 0x20];
        let mut out = [0u8; 5];
        let (irq, n) = self.transmit(&frame, false, IRQ_RXE | IRQ_COL, 15, &mut out);
        let collision = "collision (several tags in the field)";
        if n.is_none() {
            return Err(if irq & IRQ_COL != 0 { collision } else { "no answer to anticollision" }.into());
        }
        if irq & IRQ_COL != 0 {
            return Err(collision.into());
        }
        if n != Some(5) || out[0] ^ out[1] ^ out[2] ^ out[3] != out[4] {
            return Err("anticollision answer failed its check byte".into());
        }
        Ok(out)
    }

    fn select_level(&mut self, level: u8, id: &[u8; 5]) -> Option<u8> {
        self.write_reg(REG_ISO14443A_SETTINGS, 0x00, false).ok()?;
        self.modify_reg(REG_AUXILIARY_DEFINITION, NO_CRC_RX, 0, false).ok()?;
        let mut frame = [0u8; 7];
        frame[0] = 0x91 + level * 2;
        frame[1] = 0x70;
        frame[2..].copy_from_slice(id);
        let mut rx = [0u8; 4];
        match self.transmit(&frame, true, IRQ_RXE, 15, &mut rx) {
            (_, Some(n)) if n >= 1 => Some(rx[0]),
            _ => None,
        }
    }

    /// REQA/WUPA + anticollision + select (cascade levels 1..3). Errs with "no tag" when nothing answers.
    pub fn detect_tag(&mut self) -> Result<Tag, String> {
        let mut tag = Tag::default();
        // WUPA also wakes tags left in the HALT state.
        tag.atqa = self.request(true).ok_or("no tag")?;
        for level in 1..=3u8 {
            let id = self.anticollision(level)?;
            let sak = self.select_level(level, &id).ok_or("select failed")?;
            let cascade = id[0] == 0x88;
            tag.uid.extend_from_slice(if cascade { &id[1..4] } else { &id[0..4] });
            tag.sak = sak;
            if sak & 0x04 == 0 {
                // An all-zero UID is a spurious answer (RXE with an empty/stale FIFO), not a tag.
                if tag.uid.iter().all(|&b| b == 0) {
                    return Err("no tag".into());
                }
                return Ok(tag); // cascade bit clear: UID complete
            }
        }
        Err("UID longer than 10 bytes".into())
    }

    fn type2(&mut self, cmd: &[u8], rx: &mut [u8], min: usize) -> bool {
        let ready = self
            .write_reg(REG_ISO14443A_SETTINGS, 0x00, false)
            .and_then(|_| self.modify_reg(REG_AUXILIARY_DEFINITION, NO_CRC_RX, 0, false))
            .and_then(|_| self.set_no_response_timer_ms(10));
        ready.is_ok() && matches!(self.transmit(cmd, true, IRQ_RXE, 15, rx), (_, Some(n)) if n >= min)
    }

    /// NTAG GET_VERSION (0x60) on a tag selected by `detect_tag`. CRC is handled by the chip.
    pub fn type2_get_version(&mut self) -> Option<[u8; 8]> {
        let mut rx = [0u8; 16];
        self.type2(&[0x60], &mut rx, 8).then(|| rx[..8].try_into().unwrap())
    }

    /// READ (0x30): 4 pages starting at `page`.
    pub fn type2_read(&mut self, page: u8) -> Option<[u8; 16]> {
        let mut rx = [0u8; 24];
        self.type2(&[0x30, page], &mut rx, 16).then(|| rx[..16].try_into().unwrap())
    }

    /// Reads pages 0..=131 of the tag in `image` (540 bytes; pages 130-134 are not signed and not needed).
    pub fn dump_ntag215(&mut self) -> Option<[u8; 540]> {
        let mut image = [0u8; 540];
        for first in (0..=128u8).step_by(4) {
            let page = self.type2_read(first)?;
            let off = first as usize * 4;
            let n = (540 - off).min(16);
            image[off..off + n].copy_from_slice(&page[..n]);
        }
        Some(image)
    }
}

#[cfg(test)]
pub(crate) mod mock {
    use super::*;

    /// Records every transfer; answers reads from a register file (space A only).
    pub struct MockBus {
        pub sent: Vec<Vec<u8>>,
        pub regs: [u8; 64],
    }

    impl Default for MockBus {
        fn default() -> Self {
            Self { sent: Vec::new(), regs: [0; 64] }
        }
    }

    impl Bus for MockBus {
        fn transfer(&mut self, tx: &[u8], rx: &mut [u8]) -> io::Result<()> {
            self.sent.push(tx.to_vec());
            if tx.len() >= 2 && tx[0] & 0xC0 == OP_READ_REGISTER {
                for (i, r) in rx.iter_mut().enumerate().skip(1) {
                    *r = self.regs[((tx[0] & 0x3F) as usize + i - 1) % 64];
                }
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::mock::MockBus;
    use super::*;

    fn chip() -> Chip<MockBus> {
        let mut bus = MockBus::default();
        bus.regs[REG_IC_IDENTITY as usize] = 0x2A;
        Chip::open(bus, None).unwrap()
    }

    #[test]
    fn identity_is_checked() {
        let bus = MockBus::default(); // every register reads 0x00: Cap unpowered
        let err = Chip::open(bus, None).err().unwrap();
        assert!(err.0.contains("0x00"), "{}", err.0);
        let c = chip();
        assert_eq!((c.ic_identity(), c.revision()), (0x2A, 2));
    }

    #[test]
    fn frames_match_the_datasheet_layout() {
        let mut c = chip();
        c.bus.sent.clear();
        c.write_reg(REG_TX_DRIVER, 0xD0, false).unwrap();
        c.write_reg(REG_EMD_SUPPRESSION, 0x40, true).unwrap();
        c.direct_command(CMD_CLEAR_FIFO).unwrap();
        c.write_fifo(&[1, 2, 3]).unwrap();
        c.set_no_response_timer_ms(10).unwrap();
        assert_eq!(c.bus.sent[0], [0x28, 0xD0]);
        assert_eq!(c.bus.sent[1], [0xFB, 0x05, 0x40]);
        assert_eq!(c.bus.sent[2], [0xDB]);
        assert_eq!(c.bus.sent[3], [0x80, 1, 2, 3]);
        // 10 ms at 13.56 MHz / 64 = 2119 steps = 0x0847
        assert_eq!(c.bus.sent[4], [0x10, 0x08, 0x47]);
    }

    #[test]
    fn interrupts_are_packed_main_timer_error_passive() {
        let mut c = chip();
        c.bus.regs[REG_MAIN_INTERRUPT as usize] = 0x10; // RXE
        c.bus.regs[REG_MAIN_INTERRUPT as usize + 1] = 0x08; // EOF
        c.bus.regs[REG_ERROR_INTERRUPT as usize] = 0x80; // CRC
        c.bus.regs[REG_PASSIVE_TARGET_INTERRUPT as usize] = 0x01; // WU_A
        let v = c.read_interrupts().unwrap();
        assert_eq!(v, IRQ_RXE | IRQ_EOF | IRQ_CRC | IRQ_WU_A);
    }

    #[test]
    fn fifo_size_uses_the_two_high_bits() {
        let mut c = chip();
        c.bus.regs[REG_FIFO_STATUS1 as usize] = 0x12;
        c.bus.regs[REG_FIFO_STATUS1 as usize + 1] = 0x40;
        assert_eq!(c.fifo_size().unwrap(), 0x112);
    }
}
