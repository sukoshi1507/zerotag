// SPDX-License-Identifier: MIT
//! Saves the screen as a PNG in ~/Pictures/Screenshots (port of screenshot_service.cpp). The caller provides the
//! pixels (RGBA, row-major); the UI layer owns the framebuffer.

use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};

pub fn screenshot_directory() -> PathBuf {
    match std::env::var_os("HOME").filter(|h| !h.is_empty()) {
        Some(home) => Path::new(&home).join("Pictures").join("Screenshots"),
        None => PathBuf::from("/tmp/Screenshots"),
    }
}

/// `zerotag_YYYYMMDD_HHMMSS_mmm.png`, local time.
pub fn timestamp_filename() -> String {
    chrono::Local::now().format("zerotag_%Y%m%d_%H%M%S_%3f.png").to_string()
}

pub fn write_png(path: &Path, rgba: &[u8], width: u32, height: u32) -> Result<(), String> {
    if rgba.len() != width as usize * height as usize * 4 {
        return Err("Invalid snapshot".into());
    }
    let file = File::create(path).map_err(|_| "Unable to open PNG file".to_string())?;
    let mut enc = png::Encoder::new(BufWriter::new(file), width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|_| "Unable to initialize PNG encoder".to_string())?;
    writer.write_image_data(rgba).map_err(|_| "PNG encoding failed".to_string())
}

/// Writes `rgba` into `dir` under a timestamped name and returns the path.
pub fn save_into(dir: &Path, rgba: &[u8], width: u32, height: u32) -> Result<PathBuf, String> {
    fs::create_dir_all(dir).map_err(|e| format!("Unable to create screenshot directory: {e}"))?;
    let path = dir.join(timestamp_filename());
    write_png(&path, rgba, width, height)?;
    Ok(path)
}

/// Same, in ~/Pictures/Screenshots.
pub fn save(rgba: &[u8], width: u32, height: u32) -> Result<PathBuf, String> {
    save_into(&screenshot_directory(), rgba, width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filename_shape() {
        let n = timestamp_filename();
        assert!(n.starts_with("zerotag_") && n.ends_with(".png"), "{n}");
        assert_eq!(n.len(), "zerotag_20261124_093000_123.png".len(), "{n}");
    }

    #[test]
    fn png_roundtrip() {
        let d = tempfile::tempdir().unwrap();
        let (w, h) = (4u32, 3u32);
        let px: Vec<u8> = (0..w * h * 4).map(|i| (i * 7) as u8).collect();
        let p = save_into(&d.path().join("shots"), &px, w, h).unwrap();
        let dec = png::Decoder::new(File::open(&p).unwrap());
        let mut reader = dec.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!((info.width, info.height), (w, h));
        assert_eq!(&buf[..info.buffer_size()], &px[..]);
        assert!(write_png(&p, &px[..10], w, h).is_err()); // wrong size
    }
}
