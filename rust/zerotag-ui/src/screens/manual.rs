// SPDX-License-Identifier: MIT
//! The manual: five key-reference pages over a demoscene backdrop (raster bars, starfield, sine scroller).
//! Port of render_manual / manual_animate: every frame is a pure function of the page and the elapsed time.

use crate::i18n::{tr, Lang};
use crate::style::*;
use crate::widgets::Painter;
use crate::{Str, SCREEN_H, SCREEN_W};

pub const PAGES: usize = 5;
const SCROLLER_TOP: i32 = 130;
const SCROLL_SLOTS: i32 = 36;
const SCROLL_ADVANCE: i32 = 9;
const STARS: i32 = 14;

/// Copper bar colours: (edge, middle).
const BAR_COLORS: [(u32, u32); 3] = [(0x6b2a30, 0xe8503f), (0x7a5a10, 0xf3c93a), (0x6e675a, 0xf1ebd8)];
const STAR_COLORS: [u32; 3] = [0x5a4a4a, 0x8a7466, 0xf1ebd8];
const CYCLE: [u32; 4] = [0xf1ebd8, 0xf3c93a, 0xe8503f, 0xf08a2a];

pub struct ManualView<'a> {
    pub lang: Lang,
    pub page: usize,
    /// Seconds since the screen was entered (drives every animation).
    pub seconds: f64,
    pub banner: Option<(&'a str, bool)>,
    pub battery: Option<(i32, bool)>,
}

/// Top row of copper bar `i`: a stack that slides up and down under the title bar.
pub fn bar_y(seconds: f64, i: usize) -> i32 {
    26 + (3.0 + 3.0 * (seconds * 2.0 + i as f64 * 2.1).sin()) as i32
}

/// Position of star `i`: three speeds, wrapping inside the scroller band.
pub fn star_pos(seconds: f64, i: i32) -> (i32, i32, i32) {
    let speed = 1 + i % 3;
    let x0 = (i * 97) % 304;
    let span = 304;
    let x = (((x0 - (seconds * speed as f64 * 14.0) as i32) % span) + span) % span + 8;
    (x, SCROLLER_TOP + 3 + (i * 5) % 13, speed)
}

/// First visible character of the scroller and the sub-character shift in pixels.
pub fn scroll_offset(seconds: f64) -> (i32, i32) {
    let pos = (seconds * 55.0) as i32;
    (pos / SCROLL_ADVANCE, pos % SCROLL_ADVANCE)
}

/// (key cap, label) lines and paragraphs of a page.
fn page_content(lang: Lang, page: usize) -> (Vec<(String, String)>, Vec<String>) {
    let t = |s: Str| tr(lang, s).to_owned();
    let l = |k: &str, s: Str| (k.to_owned(), t(s));
    match page {
        0 => (
            vec![
                (t(Str::KeyUpDown), t(Str::HelpChoose)),
                (t(Str::KeyEnter), t(Str::HelpOpen)),
                l("a-z", Str::HelpTypeFilter),
                (t(Str::KeyTab), t(Str::HelpToCatalogue)),
                l("F2 F3", Str::HelpFavTabs),
                l("F4", Str::HelpScan),
                l("F1", Str::HelpSettings),
            ],
            vec![],
        ),
        1 => (
            vec![
                (t(Str::KeyUpDown), t(Str::HelpChoose)),
                l("a-z", Str::HelpTypeSearch),
                (t(Str::KeyEnter), t(Str::HelpCreate)),
                l("F2", Str::HelpFilter),
                (t(Str::KeyTab), t(Str::HelpToLibrary)),
                (t(Str::KeyEsc), t(Str::HelpClearBack)),
            ],
            vec![],
        ),
        2 => (
            vec![
                (t(Str::KeyEnter), t(Str::HelpEmulate)),
                l("R", Str::HelpNewUid),
                l("U", Str::HelpRotation),
                l("F2", Str::HintFavorite),
                (t(Str::KeyDel), t(Str::HelpDelete)),
                (t(Str::KeyEsc), t(Str::HelpStopSave)),
                (String::new(), t(Str::HelpAutosave)),
            ],
            vec![],
        ),
        3 => (vec![], vec![t(Str::ManScanText1), t(Str::ManScanText2), t(Str::ManScanText3)]),
        _ => (vec![], vec![t(Str::ManCredits1), t(Str::ManCredits2), t(Str::ManCredits3)]),
    }
}

pub fn render(p: &mut Painter, v: &ManualView) {
    let t = |s: Str| tr(v.lang, s);
    let s = v.seconds;
    let page = v.page % PAGES;
    p.canvas.fill_rect(0, 0, SCREEN_W, SCREEN_H, BACKGROUND);

    let titles = [Str::ManLibrary, Str::ManCatalogue, Str::ManEmulation, Str::ManScan, Str::ManCredits];
    p.bar("ZEROTAG", &format!("{}  {}/{}", t(titles[page]), page + 1, PAGES), v.battery);

    // Copper bars under the title bar: a dark / bright / dark stack that slides up and down.
    for (i, (edge, mid)) in BAR_COLORS.iter().enumerate() {
        let y = bar_y(s, i);
        p.rect(INSET + 1, y, INNER_W - 2, 1, *edge);
        p.rect(INSET + 1, y + 1, INNER_W - 2, 2, *mid);
        p.rect(INSET + 1, y + 3, INNER_W - 2, 1, *edge);
    }

    // Page content.
    let (lines, paragraphs) = page_content(v.lang, page);
    let mut y = 36;
    for (key, label) in &lines {
        if !key.is_empty() {
            let kw = p.fonts.width(key, 9, false) + 8;
            p.canvas.bordered(12, y, kw, 12, SURFACE, 2, CREAM);
            p.text_centered(key, 9, false, TEXT, 14, y + 2, kw - 4, 8);
        }
        p.text(label, 9, false, if key.is_empty() { DIM } else { TEXT }, 78, y, Some(SCREEN_W - 78 - 12));
        y += 13;
    }
    let mut py = 40;
    for (i, para) in paragraphs.iter().enumerate() {
        p.paragraph(para, 10, i == 0, if i == 0 { PRIMARY } else { TEXT }, 12, py, SCREEN_W - 24);
        py += 28;
    }

    // Sine scroller band with its starfield.
    p.rect(INSET, SCROLLER_TOP, INNER_W, FOOT_TOP - SCROLLER_TOP, BAR);
    p.rect(INSET, SCROLLER_TOP, INNER_W, 1, ACCENT);
    for i in 0..STARS {
        let (x, y, speed) = star_pos(s, i);
        let size = if speed == 3 { 2 } else { 1 };
        p.rect(x, y, size, size, STAR_COLORS[speed as usize - 1]);
    }
    let text: Vec<char> = t(Str::ManualScroller).chars().collect();
    let (first, frac) = scroll_offset(s);
    if !text.is_empty() {
        for i in 0..SCROLL_SLOTS {
            let c = text[(first + i) as usize % text.len()];
            let x = INSET + i * SCROLL_ADVANCE - frac;
            let y = SCROLLER_TOP + 3 + (2.0 * (s * 4.0 + i as f64 * 0.45).sin()) as i32;
            let color = CYCLE[((first + i + (s * 4.0) as i32) % 4) as usize];
            p.text(&c.to_string(), 11, true, color, x, y, None);
        }
    }
    p.footer(&[("< >", t(Str::ManualPage)), (t(Str::KeyEsc), t(Str::HintBack))], v.banner);
    // Drawn last, as in the original: it covers the scroller's characters that slide under the border.
    p.frame();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bars_stay_between_the_title_bar_and_the_content() {
        for step in 0..400 {
            let s = step as f64 * 0.05;
            for i in 0..3 {
                let y = bar_y(s, i);
                assert!((26..=32).contains(&y), "{y}");
                assert!(y + 3 < 36); // the first content line starts at 36
            }
        }
        assert_eq!(bar_y(0.0, 0), 29); // 3 + 3 sin 0
    }

    #[test]
    fn stars_wrap_inside_the_screen_and_move_left_at_three_speeds() {
        for step in 0..600 {
            let s = step as f64 * 0.1;
            for i in 0..STARS {
                let (x, y, speed) = star_pos(s, i);
                assert!((8..312).contains(&x), "{x}");
                assert!((133..146).contains(&y), "{y}");
                assert!((1..=3).contains(&speed));
            }
        }
        assert_eq!(star_pos(0.0, 0), (8, 133, 1));
        // star 2 has speed 3: 3 * 14 = 42 px per second towards the left
        assert_eq!(star_pos(1.0, 2).0, (((2i32 * 97) % 304 - 42).rem_euclid(304)) + 8);
    }

    #[test]
    fn the_scroller_advances_at_55_pixels_per_second() {
        assert_eq!(scroll_offset(0.0), (0, 0));
        assert_eq!(scroll_offset(1.0), (6, 1)); // 55 px = 6 characters + 1 px
        assert_eq!(scroll_offset(2.0), (12, 2));
    }

    #[test]
    fn every_page_renders_in_every_language_at_any_time() {
        let Ok(fonts) = crate::Fonts::load(Lang::Ja) else { return };
        let mut canvas = crate::Canvas::new(SCREEN_W, SCREEN_H, 0);
        for lang in [Lang::Fr, Lang::En, Lang::Ja, Lang::Zh] {
            for page in 0..PAGES {
                for s in [0.0, 0.7, 13.37, 600.0] {
                    let mut p = Painter { canvas: &mut canvas, fonts: &fonts, thumbs: None };
                    render(&mut p, &ManualView { lang, page, seconds: s, banner: None, battery: Some((90, true)) });
                }
            }
        }
        // the frame is on top: the border pixels are the frame's even where the scroller passes
        assert_eq!(canvas.get(0, 140), Some(CREAM));
        assert_eq!(canvas.get(1, 140), Some(ACCENT));
    }
}
