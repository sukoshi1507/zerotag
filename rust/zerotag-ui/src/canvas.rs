// SPDX-License-Identifier: MIT
//! A pixel canvas (0xRRGGBB) with clipped drawing primitives, and the conversions the outputs need.

pub type Rgb = u32;

pub struct Canvas {
    pub w: i32,
    pub h: i32,
    px: Vec<Rgb>,
}

fn mix(bg: Rgb, fg: Rgb, alpha: u8) -> Rgb {
    let a = alpha as u32;
    let ch = |shift: u32| {
        let b = (bg >> shift) & 0xFF;
        let f = (fg >> shift) & 0xFF;
        ((b * (255 - a) + f * a + 127) / 255) << shift
    };
    ch(16) | ch(8) | ch(0)
}

impl Canvas {
    pub fn new(w: i32, h: i32, bg: Rgb) -> Self {
        Self { w, h, px: vec![bg; (w * h) as usize] }
    }

    pub fn get(&self, x: i32, y: i32) -> Option<Rgb> {
        (x >= 0 && y >= 0 && x < self.w && y < self.h).then(|| self.px[(y * self.w + x) as usize])
    }

    pub fn set(&mut self, x: i32, y: i32, c: Rgb) {
        if x >= 0 && y >= 0 && x < self.w && y < self.h {
            self.px[(y * self.w + x) as usize] = c;
        }
    }

    /// Blends `c` over the pixel with coverage `alpha` (0..=255).
    pub fn blend(&mut self, x: i32, y: i32, c: Rgb, alpha: u8) {
        if alpha == 0 {
            return;
        }
        if let Some(bg) = self.get(x, y) {
            self.set(x, y, if alpha == 255 { c } else { mix(bg, c, alpha) });
        }
    }

    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: Rgb) {
        for yy in y.max(0)..(y + h).min(self.h) {
            for xx in x.max(0)..(x + w).min(self.w) {
                self.px[(yy * self.w + xx) as usize] = c;
            }
        }
    }

    /// A filled box with a border of `bw` pixels drawn inside it (like a CSS border-box).
    pub fn bordered(&mut self, x: i32, y: i32, w: i32, h: i32, fill: Rgb, bw: i32, border: Rgb) {
        self.fill_rect(x, y, w, h, border);
        self.fill_rect(x + bw, y + bw, w - 2 * bw, h - 2 * bw, fill);
    }

    /// Like `fill_rect` but with the four corner pixels cut (a radius of 2 px at this scale).
    pub fn fill_rounded(&mut self, x: i32, y: i32, w: i32, h: i32, c: Rgb) {
        let corners = [(x, y), (x + w - 1, y), (x, y + h - 1), (x + w - 1, y + h - 1)];
        let saved: Vec<_> = corners.iter().map(|&(cx, cy)| self.get(cx, cy)).collect();
        self.fill_rect(x, y, w, h, c);
        for (&(cx, cy), old) in corners.iter().zip(saved) {
            if let Some(old) = old {
                self.set(cx, cy, old);
            }
        }
    }

    pub fn to_rgba(&self) -> Vec<u8> {
        self.px.iter().flat_map(|&c| [(c >> 16) as u8, (c >> 8) as u8, c as u8, 255]).collect()
    }

    /// RGB565, little-endian, row-major: the format of the CardputerZero framebuffer.
    pub fn to_rgb565(&self) -> Vec<u8> {
        self.px
            .iter()
            .map(|&c| {
                let (r, g, b) = ((c >> 16) & 0xFF, (c >> 8) & 0xFF, c & 0xFF);
                (((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3)) as u16
            })
            .flat_map(u16::to_le_bytes)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drawing_is_clipped() {
        let mut c = Canvas::new(10, 10, 0);
        c.fill_rect(-5, -5, 8, 8, 0xFFFFFF);
        assert_eq!(c.get(0, 0), Some(0xFFFFFF));
        assert_eq!(c.get(2, 2), Some(0xFFFFFF));
        assert_eq!(c.get(3, 3), Some(0));
        c.set(100, 100, 1); // no panic
        assert_eq!(c.get(10, 0), None);
    }

    #[test]
    fn border_is_inside_the_box() {
        let mut c = Canvas::new(8, 8, 0);
        c.bordered(1, 1, 6, 6, 0x00FF00, 2, 0xFF0000);
        assert_eq!(c.get(1, 1), Some(0xFF0000));
        assert_eq!(c.get(2, 4), Some(0xFF0000));
        assert_eq!(c.get(3, 3), Some(0x00FF00));
        assert_eq!(c.get(6, 6), Some(0xFF0000));
        assert_eq!(c.get(7, 7), Some(0));
    }

    #[test]
    fn rounded_keeps_the_corner_pixels() {
        let mut c = Canvas::new(6, 6, 0x111111);
        c.fill_rounded(1, 1, 4, 4, 0xFFFFFF);
        assert_eq!(c.get(1, 1), Some(0x111111));
        assert_eq!(c.get(4, 4), Some(0x111111));
        assert_eq!(c.get(2, 1), Some(0xFFFFFF));
    }

    #[test]
    fn blend_and_conversions() {
        let mut c = Canvas::new(2, 1, 0x000000);
        c.blend(0, 0, 0xFFFFFF, 128);
        assert_eq!(c.get(0, 0), Some(0x808080));
        c.blend(1, 0, 0xFF8000, 255);
        assert_eq!(c.to_rgba()[4..8], [0xFF, 0x80, 0x00, 0xFF]);
        // pure red = 0xF800 little-endian
        let mut r = Canvas::new(1, 1, 0xFF0000);
        assert_eq!(r.to_rgb565(), [0x00, 0xF8]);
        r.set(0, 0, 0x00FF00);
        assert_eq!(r.to_rgb565(), [0xE0, 0x07]);
    }
}
