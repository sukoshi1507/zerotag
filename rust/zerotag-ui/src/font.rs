// SPDX-License-Identifier: MIT
//! Text rendering with the system Noto Sans CJK (Regular and Medium, like the C++ version): Latin, accents and
//! Japanese from one family. Glyphs are rasterised by fontdue and cached.

use crate::canvas::{Canvas, Rgb};
use crate::Lang;
use fontdue::{Font, FontSettings, Metrics};
use std::cell::RefCell;
use std::collections::HashMap;

const REGULAR: [&str; 2] =
    ["/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc"];
const MEDIUM: [&str; 2] =
    ["/usr/share/fonts/opentype/noto/NotoSansCJK-Medium.ttc", "/usr/share/fonts/noto-cjk/NotoSansCJK-Medium.ttc"];

type Glyph = (Metrics, Vec<u8>);

pub struct Fonts {
    regular: Font,
    bold: Font,
    face: u32,
    cache: RefCell<HashMap<(char, u32, bool), Glyph>>,
}

fn load_one(paths: &[&str], face: u32) -> Result<Font, String> {
    for p in paths {
        if let Ok(data) = std::fs::read(p) {
            // face in the collection: 0 = Japanese, 2 = Simplified Chinese (same Latin letters, different Han glyphs)
            return Font::from_bytes(data, FontSettings { collection_index: face, ..Default::default() })
                .map_err(|e| format!("{p}: {e}"));
        }
    }
    Err(format!("Noto Sans CJK not found (looked in {})", paths.join(", ")))
}

impl Fonts {
    /// Loads the fonts for `lang` (French and English use the Japanese face, whose Latin letters are the same).
    pub fn load(lang: Lang) -> Result<Self, String> {
        let face = lang.cjk_face();
        Ok(Self {
            regular: load_one(&REGULAR, face)?,
            bold: load_one(&MEDIUM, face)?,
            face,
            cache: RefCell::new(HashMap::new()),
        })
    }

    /// True when the loaded face is the right one for `lang` (no reload needed when switching fr <-> en <-> ja).
    pub fn serves(&self, lang: Lang) -> bool {
        self.face == lang.cjk_face()
    }

    fn font(&self, bold: bool) -> &Font {
        if bold {
            &self.bold
        } else {
            &self.regular
        }
    }

    fn ascent(&self, px: u32, bold: bool) -> i32 {
        self.font(bold).horizontal_line_metrics(px as f32).map_or(px as i32, |m| m.ascent.round() as i32)
    }

    /// Height of one line (ascent + descent), as LVGL's lv_font_get_line_height.
    pub fn line_height(&self, px: u32, bold: bool) -> i32 {
        self.font(bold).horizontal_line_metrics(px as f32).map_or(px as i32, |m| (m.ascent - m.descent).round() as i32)
    }

    fn glyph<R>(&self, c: char, px: u32, bold: bool, f: impl FnOnce(&Glyph) -> R) -> R {
        let mut cache = self.cache.borrow_mut();
        let g = cache.entry((c, px, bold)).or_insert_with(|| self.font(bold).rasterize(c, px as f32));
        f(g)
    }

    fn advance(&self, c: char, px: u32, bold: bool) -> f32 {
        self.glyph(c, px, bold, |g| g.0.advance_width)
    }

    pub fn width(&self, text: &str, px: u32, bold: bool) -> i32 {
        text.chars().map(|c| self.advance(c, px, bold)).sum::<f32>().round() as i32
    }

    /// Draws `text` with its top-left corner at (x, y); returns the width drawn.
    pub fn draw(&self, canvas: &mut Canvas, text: &str, x: i32, y: i32, px: u32, bold: bool, color: Rgb) -> i32 {
        let baseline = y + self.ascent(px, bold);
        let mut pen = x as f32;
        for c in text.chars() {
            let adv = self.glyph(c, px, bold, |(m, bitmap)| {
                let gx = pen.round() as i32 + m.xmin;
                let gy = baseline - m.ymin - m.height as i32;
                for row in 0..m.height {
                    for col in 0..m.width {
                        canvas.blend(gx + col as i32, gy + row as i32, color, bitmap[row * m.width + col]);
                    }
                }
                m.advance_width
            });
            pen += adv;
        }
        (pen - x as f32).round() as i32
    }

    /// Cuts `text` with "..." so that it fits `max_w` (LVGL's DOTS long mode); unchanged when it already fits.
    pub fn clip(&self, text: &str, px: u32, bold: bool, max_w: i32) -> String {
        if self.width(text, px, bold) <= max_w {
            return text.to_owned();
        }
        let dots = self.width("...", px, bold);
        let mut out = String::new();
        let mut w = 0.0;
        for c in text.chars() {
            let a = self.advance(c, px, bold);
            if (w + a).round() as i32 + dots > max_w {
                break;
            }
            w += a;
            out.push(c);
        }
        out.push_str("...");
        out
    }

    /// Greedy word wrap to `max_w`. Breaks at spaces; a word (or Japanese text, which has no spaces) longer than the
    /// line is broken between characters. Explicit newlines are kept.
    pub fn wrap(&self, text: &str, px: u32, bold: bool, max_w: i32) -> Vec<String> {
        let mut lines = Vec::new();
        for para in text.split('\n') {
            let mut line = String::new();
            for word in para.split(' ') {
                let candidate = if line.is_empty() { word.to_owned() } else { format!("{line} {word}") };
                if self.width(&candidate, px, bold) <= max_w {
                    line = candidate;
                    continue;
                }
                if !line.is_empty() {
                    lines.push(std::mem::take(&mut line));
                }
                // the word alone may still be too long: break it between characters
                let mut cur = String::new();
                for c in word.chars() {
                    cur.push(c);
                    if self.width(&cur, px, bold) > max_w && cur.chars().count() > 1 {
                        cur.pop();
                        lines.push(std::mem::take(&mut cur));
                        cur.push(c);
                    }
                }
                line = cur;
            }
            lines.push(line);
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fonts() -> Option<Fonts> {
        Fonts::load(Lang::Ja).ok() // skipped on a machine without Noto CJK
    }

    #[test]
    fn metrics_are_sane() {
        let Some(f) = fonts() else { return };
        assert!(f.line_height(10, false) >= 10 && f.line_height(10, false) <= 16);
        assert!(f.width("Catalogue", 10, false) > f.width("Cat", 10, false));
        assert!(f.width("Catalogue", 10, true) >= f.width("Catalogue", 10, false));
        assert!(f.width("日本語", 10, false) >= 25); // CJK glyphs are about one em wide
        assert_eq!(f.width("", 10, false), 0);
    }

    #[test]
    fn drawing_marks_pixels_inside_the_line_box() {
        let Some(f) = fonts() else { return };
        let mut c = Canvas::new(80, 30, 0);
        let w = f.draw(&mut c, "Zelda é", 2, 4, 11, false, 0xFFFFFF);
        assert!(w > 20);
        let lit: Vec<(i32, i32)> =
            (0..30).flat_map(|y| (0..80).map(move |x| (x, y))).filter(|&(x, y)| c.get(x, y) != Some(0)).collect();
        assert!(lit.len() > 30);
        assert!(lit.iter().all(|&(x, y)| x >= 2 && x <= 2 + w + 1 && y >= 4 && y < 4 + f.line_height(11, false) + 2));
    }

    #[test]
    fn clip_and_wrap() {
        let Some(f) = fonts() else { return };
        let s = "Ganondorf - Tears of the Kingdom";
        let clipped = f.clip(s, 11, false, 100);
        assert!(clipped.ends_with("...") && f.width(&clipped, 11, false) <= 100, "{clipped}");
        assert_eq!(f.clip("Link", 11, false, 100), "Link");
        let lines = f.wrap("Entrée crée une image vierge signée.", 9, false, 100);
        assert!(lines.len() >= 2 && lines.iter().all(|l| f.width(l, 9, false) <= 100), "{lines:?}");
        assert_eq!(lines.join(" "), "Entrée crée une image vierge signée.");
        let ja = f.wrap("Enterで署名済みの空データを作成します。", 9, false, 60);
        assert!(ja.len() >= 2 && ja.iter().all(|l| f.width(l, 9, false) <= 60), "{ja:?}");
        assert_eq!(ja.concat(), "Enterで署名済みの空データを作成します。");
        let zh = f.wrap("按Enter创建带签名的空白镜像。", 9, false, 60);
        assert!(zh.len() >= 2 && zh.iter().all(|l| f.width(l, 9, false) <= 60), "{zh:?}");
    }

    #[test]
    fn chinese_face_is_selected_by_language() {
        let (Some(ja), Ok(zh)) = (fonts(), Fonts::load(Lang::Zh)) else { return };
        assert!(ja.serves(Lang::Fr) && ja.serves(Lang::Ja) && !ja.serves(Lang::Zh));
        assert!(zh.serves(Lang::Zh) && !zh.serves(Lang::En));
        // 直 / 骨 are drawn differently in the two faces: the same character must not rasterise identically
        let a = ja.glyph('直', 20, false, |g| g.1.clone());
        let b = zh.glyph('直', 20, false, |g| g.1.clone());
        assert_ne!(a, b);
    }
}
