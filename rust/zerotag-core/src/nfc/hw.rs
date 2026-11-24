// SPDX-License-Identifier: MIT
//! Real hardware: spidev bus and the Cap power rail (GPIO26 + ext_5v_out LED).

use super::chip::{Bus, Chip, Error};
use gpiocdev::line::Value;
use gpiocdev::Request;
use spidev::{SpiModeFlags, Spidev, SpidevOptions, SpidevTransfer};
use std::io;
use std::thread::sleep;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct CapConfig {
    pub spi_device: String,
    pub gpio_chip: String,
    pub power_gpio: u32,
    pub power_led: String,
    pub spi_speed_hz: u32,
}

impl Default for CapConfig {
    fn default() -> Self {
        Self {
            spi_device: "/dev/spidev0.2".into(),
            gpio_chip: "/dev/gpiochip0".into(),
            power_gpio: 26,
            power_led: "/sys/class/leds/ext_5v_out/brightness".into(),
            spi_speed_hz: 1_000_000,
        }
    }
}

pub struct SpiBus {
    dev: Spidev,
    speed_hz: u32,
}

impl SpiBus {
    pub fn open(path: &str, speed_hz: u32) -> io::Result<Self> {
        let mut dev = Spidev::open(path)?;
        let opts = SpidevOptions::new().bits_per_word(8).max_speed_hz(speed_hz).mode(SpiModeFlags::SPI_MODE_1).build();
        dev.configure(&opts)?;
        Ok(Self { dev, speed_hz })
    }
}

impl Bus for SpiBus {
    fn transfer(&mut self, tx: &[u8], rx: &mut [u8]) -> io::Result<()> {
        let mut t = SpidevTransfer::read_write(tx, rx);
        t.speed_hz = self.speed_hz;
        t.bits_per_word = 8;
        self.dev.transfer(&mut t)
    }
}

/// Powers the Cap; releases GPIO26 and switches the LED off when dropped.
struct Power {
    led: String,
    _line: Request,
}

impl Power {
    fn on(cfg: &CapConfig) -> Result<Self, Error> {
        std::fs::write(&cfg.power_led, "1").map_err(|e| Error(format!("cannot write {}: {e}", cfg.power_led)))?;
        let line = Request::builder()
            .on_chip(&cfg.gpio_chip)
            .with_consumer("zerotag")
            .with_line(cfg.power_gpio)
            .as_output(Value::Active)
            .request();
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                let _ = std::fs::write(&cfg.power_led, "0");
                return Err(Error(format!("cannot request GPIO{}: {e}", cfg.power_gpio)));
            }
        };
        // Give the Cap supply time to settle before the first SPI access.
        sleep(Duration::from_millis(300));
        Ok(Self { led: cfg.power_led.clone(), _line: line })
    }
}

impl Drop for Power {
    fn drop(&mut self) {
        // `_line` is released right after this (back to its default state).
        let _ = std::fs::write(&self.led, "0");
    }
}

/// Powers the Cap, opens the SPI device and checks the IC identity register.
pub fn open_cap(cfg: &CapConfig) -> Result<Chip<SpiBus>, Error> {
    let power = Power::on(cfg)?;
    let bus = SpiBus::open(&cfg.spi_device, cfg.spi_speed_hz)
        .map_err(|e| Error(format!("cannot open {}: {e} (is the spi0-spidev2-gpio22 overlay loaded?)", cfg.spi_device)))?;
    Chip::open(bus, Some(Box::new(power)))
}
