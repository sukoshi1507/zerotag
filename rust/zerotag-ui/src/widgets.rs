// SPDX-License-Identifier: MIT
//! Famicom-style building blocks (port of the helpers of zerotag_ui.cpp: frame, bar, footer, list rows...).

use crate::canvas::{Canvas, Rgb};
use crate::font::Fonts;
use crate::style::*;
use crate::{SCREEN_H, SCREEN_W};

/// Everything a screen needs to draw: the canvas and the fonts.
pub struct Painter<'a> {
    pub canvas: &'a mut Canvas,
    pub fonts: &'a Fonts,
}

impl Painter<'_> {
    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: Rgb) {
        self.canvas.fill_rect(x, y, w, h, c);
    }

    /// One line of text, cut with "..." when `max_w` is given and the text is longer.
    pub fn text(&mut self, s: &str, px: u32, bold: bool, color: Rgb, x: i32, y: i32, max_w: Option<i32>) {
        match max_w {
            Some(w) => {
                let clipped = self.fonts.clip(s, px, bold, w);
                self.fonts.draw(self.canvas, &clipped, x, y, px, bold, color);
            }
            None => {
                self.fonts.draw(self.canvas, s, x, y, px, bold, color);
            }
        }
    }

    /// Wrapped text; returns the number of lines drawn.
    pub fn paragraph(&mut self, s: &str, px: u32, bold: bool, color: Rgb, x: i32, y: i32, w: i32) -> usize {
        let lines = self.fonts.wrap(s, px, bold, w);
        let lh = self.fonts.line_height(px, bold);
        for (i, l) in lines.iter().enumerate() {
            self.fonts.draw(self.canvas, l, x, y + i as i32 * lh, px, bold, color);
        }
        lines.len()
    }

    /// Text centred in the box (x, y, w, h).
    pub fn text_centered(&mut self, s: &str, px: u32, bold: bool, color: Rgb, x: i32, y: i32, w: i32, h: i32) {
        let tw = self.fonts.width(s, px, bold);
        let th = self.fonts.line_height(px, bold);
        self.fonts.draw(self.canvas, s, x + (w - tw) / 2, y + (h - th) / 2, px, bold, color);
    }

    /// 9x4 pixel triforce drawn with 2x2 cells.
    pub fn triforce(&mut self, x: i32, y: i32) {
        const ROWS: [&str; 4] = ["....Y....", "...YYY...", "..Y...Y..", ".YYY.YYY."];
        for (r, row) in ROWS.iter().enumerate() {
            for (c, ch) in row.bytes().enumerate() {
                if ch == b'Y' {
                    self.rect(x + c as i32 * 2, y + r as i32 * 2, 2, 2, PRIMARY);
                }
            }
        }
    }

    /// 8x7 pointing-right triangle, NES menu style.
    pub fn cursor(&mut self, x: i32, y: i32) {
        self.rect(x, y, 2, 7, ACCENT);
        self.rect(x + 2, y + 1, 2, 5, ACCENT);
        self.rect(x + 4, y + 2, 2, 3, ACCENT);
        self.rect(x + 6, y + 3, 2, 1, ACCENT);
    }

    /// 7x7 five-point star, one pixel per cell.
    pub fn pixel_star(&mut self, x: i32, y: i32, color: Rgb) {
        const ROWS: [&str; 7] = ["...X...", "...X...", "XXXXXXX", ".XXXXX.", "..XXX..", ".XX.XX.", "XX...XX"];
        for (r, row) in ROWS.iter().enumerate() {
            for (c, ch) in row.bytes().enumerate() {
                if ch == b'X' {
                    self.rect(x + c as i32, y + r as i32, 1, 1, color);
                }
            }
        }
    }

    /// Cream outline (1 px), red band (2 px), dark line (1 px); notched corners and gold studs.
    pub fn frame(&mut self) {
        let (w, h) = (SCREEN_W, SCREEN_H);
        self.rect(0, 0, w, 1, CREAM);
        self.rect(0, h - 1, w, 1, CREAM);
        self.rect(0, 0, 1, h, CREAM);
        self.rect(w - 1, 0, 1, h, CREAM);
        self.rect(1, 1, w - 2, 2, ACCENT);
        self.rect(1, h - 3, w - 2, 2, ACCENT);
        self.rect(1, 1, 2, h - 2, ACCENT);
        self.rect(w - 3, 1, 2, h - 2, ACCENT);
        self.rect(3, 3, w - 6, 1, BACKGROUND);
        self.rect(3, h - 4, w - 6, 1, BACKGROUND);
        self.rect(3, 3, 1, h - 6, BACKGROUND);
        self.rect(w - 4, 3, 1, h - 6, BACKGROUND);
        for (x, y) in [(0, 0), (w - 2, 0), (0, h - 2), (w - 2, h - 2)] {
            self.rect(x, y, 2, 2, BACKGROUND);
        }
        for (x, y) in [(INSET, INSET), (w - INSET - 2, INSET), (INSET, h - INSET - 2), (w - INSET - 2, h - INSET - 2)] {
            self.rect(x, y, 2, 2, PRIMARY);
        }
    }

    /// Title bar: triforce, bold title, dim subtitle, battery level at the right.
    pub fn bar(&mut self, title: &str, subtitle: &str, battery: Option<(i32, bool)>) {
        self.rect(INSET, INSET, INNER_W, BAR_H, SURFACE);
        self.rect(INSET, INSET + BAR_H - 1, INNER_W, 1, ACCENT);
        self.triforce(INSET + 6, INSET + 5);
        self.text(title, 12, true, TEXT, INSET + 30, INSET + 2, None);
        let sx = INSET + 30 + self.fonts.width(title, 12, true) + 6;
        self.text(subtitle, 10, false, DIM, sx, INSET + 3, Some(SCREEN_W - sx - 50));
        if let Some((percent, charging)) = battery.filter(|b| b.0 >= 0) {
            let s = format!("{percent}%{}", if charging { "+" } else { "" });
            let w = self.fonts.width(&s, 10, true);
            self.text(&s, 10, true, PRIMARY, SCREEN_W - (INSET + 6) - w, INSET + 3, None);
        }
    }

    /// Footer: either a banner message, or key caps with their labels.
    pub fn footer(&mut self, hints: &[(&str, &str)], banner: Option<(&str, bool)>) {
        self.rect(INSET, FOOT_TOP, INNER_W, FOOT_H, BAR);
        self.rect(INSET, FOOT_TOP, INNER_W, 1, BORDER);
        if let Some((msg, is_error)) = banner {
            self.text(msg, 10, false, if is_error { ERROR } else { PRIMARY }, INSET + 6, FOOT_TOP + 3, Some(INNER_W - 12));
            return;
        }
        let mut x = INSET + 6;
        for (key, label) in hints {
            let kw = self.fonts.width(key, 9, false) + 8;
            self.canvas.bordered(x, FOOT_TOP + 3, kw, 13, SURFACE, 2, CREAM);
            self.text_centered(key, 9, false, TEXT, x + 2, FOOT_TOP + 3 + 2, kw - 4, 13 - 4);
            self.text(label, 9, false, DIM, x + kw + 3, FOOT_TOP + 3, None);
            x += kw + 3 + self.fonts.width(label, 9, false) + 7;
        }
    }

    /// Search field below the bar; shows the placeholder when the query is empty, else the query and a caret.
    pub fn search_field(&mut self, query: &str, placeholder: &str, focused: bool, width: i32) {
        self.canvas.bordered(10, 26, width, 16, BUTTON, 2, if focused { ACCENT } else { CREAM });
        let (s, color) = if query.is_empty() { (placeholder.to_owned(), DIM) } else { (format!("{query}_"), TEXT) };
        self.text(&s, 10, false, color, 10 + 2 + 3, 26 + 1, Some(width - 12));
    }

    /// Scroll bar of a list of `count` rows, `first` being the first visible one.
    pub fn scrollbar(&mut self, count: usize, first: i32) {
        if count <= ROWS as usize {
            return;
        }
        let track = ROWS * ROW_H;
        let bar_h = 6.max(track * ROWS / count as i32);
        let bar_y = LIST_TOP + (track - bar_h) * first / 1.max(count as i32 - ROWS);
        self.rect(LIST_W - 6, bar_y, 2, bar_h, BORDER);
    }

    /// Placeholder thumbnail holder (surface colour, rounded); the picture is drawn over it when available.
    pub fn thumb_holder(&mut self, x: i32, y: i32, h: i32) -> (i32, i32) {
        let w = 8.max(h * 51 / 88);
        self.canvas.fill_rounded(x, y, w, h, SURFACE);
        (w, h)
    }

    /// One row of the catalogue / library list.
    #[allow(clippy::too_many_arguments)]
    pub fn list_row(&mut self, y: i32, selected: bool, name: &str, sub: &str, badge: &str, favorite: bool) {
        if selected {
            self.rect(INSET + 1, y, LIST_W - INSET - 3, ROW_H - 2, SELECTED_ROW);
            self.cursor(INSET + 4, y + 8);
        }
        self.thumb_holder(20, y + 2, 20);
        let tw = LIST_W - 36 - if badge.is_empty() { if favorite { 20 } else { 8 } } else { 50 };
        self.text(name, 11, selected, TEXT, 36, y + 2, Some(tw));
        self.text(sub, 9, false, DIM, 36, y + 14, Some(LIST_W - 44));
        if !badge.is_empty() {
            self.text(badge, 9, false, PRIMARY, LIST_W - 50, y + 4, Some(44));
        }
        if favorite {
            self.pixel_star(LIST_W - 16, y + 4, PRIMARY);
        }
    }
}
