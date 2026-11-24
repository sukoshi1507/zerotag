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
    /// The last frame written (RGB565), to send only the rows that changed: the panel is on a SPI bus that the NFC
    /// Cap may share, and a full-screen refresh (150 KB) would hold the bus long enough to make a reader give up.
    previous: std::cell::RefCell<Vec<u8>>,
}

/// Inclusive ranges of rows (of `row_bytes` bytes) that differ between two frames of the same size.
pub fn changed_row_runs(previous: &[u8], current: &[u8], row_bytes: usize) -> Vec<(usize, usize)> {
    let mut runs: Vec<(usize, usize)> = Vec::new();
    if previous.len() != current.len() || row_bytes == 0 {
        return vec![(0, current.len() / row_bytes.max(1) - 1)];
    }
    for (y, (a, b)) in previous.chunks(row_bytes).zip(current.chunks(row_bytes)).enumerate() {
        if a != b {
            match runs.last_mut() {
                Some(last) if last.1 + 1 == y => last.1 = y,
                _ => runs.push((y, y)),
            }
        }
    }
    runs
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
        Ok(Self { file, stride, width: w, previous: std::cell::RefCell::new(Vec::new()) })
    }

    /// Writes the rows of the canvas that changed since the last call (the whole canvas the first time).
    pub fn present(&self, canvas: &Canvas) -> io::Result<()> {
        let data = canvas.to_rgb565();
        let row = self.width * 2;
        let mut previous = self.previous.borrow_mut();
        let runs = if previous.is_empty() {
            vec![(0, data.len() / row - 1)]
        } else {
            changed_row_runs(&previous, &data, row)
        };
        for (y0, y1) in runs {
            let rows = &data[y0 * row..(y1 + 1) * row];
            if self.stride == row {
                self.file.write_all_at(rows, (y0 * self.stride) as u64)?;
            } else {
                for (i, line) in rows.chunks(row).enumerate() {
                    self.file.write_all_at(line, ((y0 + i) * self.stride) as u64)?;
                }
            }
        }
        *previous = data;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::changed_row_runs;

    #[test]
    fn only_the_changed_rows_are_reported_and_adjacent_ones_merge() {
        let a = vec![0u8; 10 * 4];
        let mut b = a.clone();
        assert!(changed_row_runs(&a, &b, 4).is_empty());
        b[4 * 2] = 1; // row 2
        b[4 * 3 + 1] = 1; // row 3
        b[4 * 7 + 3] = 1; // row 7
        assert_eq!(changed_row_runs(&a, &b, 4), [(2, 3), (7, 7)]);
        assert_eq!(changed_row_runs(&a, &[0u8; 8], 4), [(0, 1)]); // size mismatch: everything
    }
}
