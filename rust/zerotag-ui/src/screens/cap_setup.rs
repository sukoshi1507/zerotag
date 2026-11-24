// SPDX-License-Identifier: MIT
//! Asks for the sudo password that loads the SPI overlay of the Cap. The password itself is never drawn.

use crate::i18n::{tr, Lang};
use crate::style::*;
use crate::widgets::Painter;
use crate::{Str, SCREEN_H, SCREEN_W};

pub struct CapSetupView<'a> {
    pub lang: Lang,
    /// Number of characters typed (one asterisk each).
    pub password_len: usize,
    pub busy: bool,
    pub message: Option<&'a str>,
    pub battery: Option<(i32, bool)>,
}

pub fn render(p: &mut Painter, v: &CapSetupView) {
    let t = |s: Str| tr(v.lang, s);
    p.canvas.fill_rect(0, 0, SCREEN_W, SCREEN_H, BACKGROUND);
    p.frame();
    p.bar("ZEROTAG", t(Str::CapSetupTitle), v.battery);
    p.text(t(Str::CapSetupTitle), 12, true, WARNING, 14, CONTENT_TOP + 8, Some(SCREEN_W - 28));
    p.paragraph(t(Str::CapSetupText), 10, false, TEXT, 14, CONTENT_TOP + 28, SCREEN_W - 28);

    p.text(t(Str::CapPassword), 9, false, DIM, 14, 90, Some(80));
    p.canvas.bordered(14, 102, SCREEN_W - 28, 18, BUTTON, 2, if v.busy { BORDER } else { ACCENT });
    let stars = "*".repeat(v.password_len) + if v.busy { "" } else { "_" };
    p.text(&stars, 12, true, TEXT, 14 + 2 + 4, 102 + 2, Some(SCREEN_W - 44));
    if v.busy {
        p.text(t(Str::CapBusy), 10, false, PRIMARY, 14, 124, Some(SCREEN_W - 28));
    } else if let Some(m) = v.message {
        p.paragraph(m, 10, false, ERROR, 14, 124, SCREEN_W - 28);
    }
    p.footer(&[(t(Str::KeyEnter), t(Str::HintValidate)), (t(Str::KeyEsc), t(Str::HintSkip))], None);
}
