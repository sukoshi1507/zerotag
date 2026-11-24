// SPDX-License-Identifier: MIT
//! Amiibo thumbnails: `<dir>/<id>.png` (71x88 RGBA artwork, not shipped with the app: fetched by
//! tools/fetch_amiibo_images.py, or found in ~/.zerotag/images). Decoded and scaled on demand, cached.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

/// A scaled picture, straight (non-premultiplied) RGBA.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sprite {
    pub w: usize,
    pub h: usize,
    pub rgba: Vec<u8>,
}

pub struct Thumbnails {
    dirs: Vec<PathBuf>,
    cache: RefCell<HashMap<(String, i32), Option<Rc<Sprite>>>>,
}

impl Thumbnails {
    /// `dirs` are searched in order (the installed assets, then the user's folder).
    pub fn new(dirs: Vec<PathBuf>) -> Self {
        Self { dirs, cache: RefCell::new(HashMap::new()) }
    }

    /// The thumbnail of the amiibo `id`, scaled to `height` pixels (the width follows the 71:88 ratio).
    pub fn get(&self, id: &str, height: i32) -> Option<Rc<Sprite>> {
        let key = (id.to_owned(), height);
        if let Some(hit) = self.cache.borrow().get(&key) {
            return hit.clone();
        }
        let sprite = self.dirs.iter().find_map(|d| decode(&d.join(format!("{id}.png")))).map(|src| Rc::new(scale_to_height(&src, height)));
        self.cache.borrow_mut().insert(key, sprite.clone());
        sprite
    }
}

fn decode(path: &std::path::Path) -> Option<Sprite> {
    let file = std::fs::File::open(path).ok()?;
    let mut dec = png::Decoder::new(std::io::BufReader::new(file));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width as usize, info.height as usize);
    let data = &buf[..info.buffer_size()];
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => data.to_vec(),
        png::ColorType::Rgb => data.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => data.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Grayscale => data.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return None, // EXPAND already turned palettes into RGB(A)
    };
    (rgba.len() == w * h * 4 && w > 0 && h > 0).then_some(Sprite { w, h, rgba })
}

/// Area-averaged scaling (premultiplied alpha, so transparent borders do not bleed colour in).
pub fn scale_to_height(src: &Sprite, height: i32) -> Sprite {
    let th = height.max(1) as usize;
    let tw = ((src.w * th + src.h / 2) / src.h).max(1);
    let mut out = vec![0u8; tw * th * 4];
    for ty in 0..th {
        let (y0, y1) = (ty * src.h / th, ((ty + 1) * src.h).div_ceil(th).min(src.h));
        for tx in 0..tw {
            let (x0, x1) = (tx * src.w / tw, ((tx + 1) * src.w).div_ceil(tw).min(src.w));
            let (mut r, mut g, mut b, mut a, mut n) = (0u32, 0u32, 0u32, 0u32, 0u32);
            for sy in y0..y1.max(y0 + 1) {
                for sx in x0..x1.max(x0 + 1) {
                    let p = &src.rgba[(sy * src.w + sx) * 4..][..4];
                    let al = p[3] as u32;
                    r += p[0] as u32 * al;
                    g += p[1] as u32 * al;
                    b += p[2] as u32 * al;
                    a += al;
                    n += 1;
                }
            }
            let o = &mut out[(ty * tw + tx) * 4..][..4];
            if a > 0 {
                o[0] = (r / a) as u8;
                o[1] = (g / a) as u8;
                o[2] = (b / a) as u8;
            }
            o[3] = (a / n.max(1)) as u8;
        }
    }
    Sprite { w: tw, h: th, rgba: out }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: usize, h: usize, px: [u8; 4]) -> Sprite {
        Sprite { w, h, rgba: px.iter().copied().cycle().take(w * h * 4).collect() }
    }

    #[test]
    fn scaling_keeps_colour_and_ratio() {
        let s = scale_to_height(&solid(71, 88, [200, 100, 50, 255]), 20);
        assert_eq!((s.w, s.h), (16, 20)); // 71 * 20 / 88 = 16.1
        assert!(s.rgba.chunks(4).all(|p| p == [200, 100, 50, 255]));
        let big = scale_to_height(&solid(71, 88, [1, 2, 3, 255]), 96);
        assert_eq!((big.w, big.h), (77, 96));
    }

    #[test]
    fn transparent_pixels_do_not_bleed_colour() {
        // left half opaque red, right half fully transparent (with a garbage colour)
        let mut src = solid(4, 4, [0, 255, 0, 0]);
        for y in 0..4 {
            for x in 0..2 {
                src.rgba[(y * 4 + x) * 4..][..4].copy_from_slice(&[255, 0, 0, 255]);
            }
        }
        let s = scale_to_height(&src, 2);
        assert_eq!(&s.rgba[0..4], &[255, 0, 0, 255]);
        assert_eq!(s.rgba[7], 0); // transparent stays transparent
    }

    #[test]
    fn decodes_and_caches_a_png_and_reports_missing_ones() {
        let d = tempfile::tempdir().unwrap();
        let px: Vec<u8> = (0..8 * 8 * 4).map(|i| if i % 4 == 3 { 255 } else { (i * 3) as u8 }).collect();
        zerotag_core::screenshot::write_png(&d.path().join("0123456789abcdef.png"), &px, 8, 8).unwrap();
        let t = Thumbnails::new(vec![d.path().join("nope"), d.path().to_owned()]);
        let a = t.get("0123456789abcdef", 4).unwrap();
        assert_eq!((a.w, a.h), (4, 4));
        assert!(Rc::ptr_eq(&a, &t.get("0123456789abcdef", 4).unwrap())); // cached
        assert!(t.get("ffffffffffffffff", 4).is_none());
    }
}
