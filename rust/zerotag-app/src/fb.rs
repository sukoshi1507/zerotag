// SPDX-License-Identifier: MIT
//! The panel of the CardputerZero as a Linux framebuffer: 320x170, 16 bits per pixel (RGB565).

use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::unix::fs::FileExt;
use zerotag_ui::Canvas;

pub struct Framebuffer {
    file: File,
    stride: usize,
    width: usize,
}

/// Finds the framebuffer whose size is `w`x`h` at 16 bpp, returning its device path and line length in bytes.
fn find(w: usize, h: usize) -> Option<(String, usize)> {
    let mut found = fs::read_dir("/sys/class/graphics").ok()?.flatten().filter_map(|e| {
        let name = e.file_name().to_string_lossy().into_owned();
        name.strip_prefix("fb")?.parse::<u32>().ok()?;
        let read = |f: &str| fs::read_to_string(e.path().join(f)).ok().map(|s| s.trim().to_owned());
        let size = read("virtual_size")?;
        let bpp: usize = read("bits_per_pixel")?.parse().ok()?;
        let stride: usize = read("stride")?.parse().ok()?;
        (size == format!("{w},{h}") && bpp == 16).then(|| (format!("/dev/{name}"), stride))
    });
    found.next()
}

impl Framebuffer {
    pub fn open(w: usize, h: usize) -> Result<Self, String> {
        let (path, stride) = find(w, h).ok_or_else(|| format!("no {w}x{h} 16 bpp framebuffer found"))?;
        let file = OpenOptions::new().write(true).open(&path).map_err(|e| format!("cannot open {path}: {e}"))?;
        Ok(Self { file, stride, width: w })
    }

    /// Writes the whole canvas.
    pub fn present(&self, canvas: &Canvas) -> io::Result<()> {
        let data = canvas.to_rgb565();
        let row = self.width * 2;
        if self.stride == row {
            return self.file.write_all_at(&data, 0);
        }
        for (y, line) in data.chunks(row).enumerate() {
            self.file.write_all_at(line, (y * self.stride) as u64)?;
        }
        Ok(())
    }
}
