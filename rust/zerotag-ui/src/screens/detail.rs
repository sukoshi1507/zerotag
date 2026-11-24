// SPDX-License-Identifier: MIT
//! The detail screen of one backup: identity, signature, UID rotation, file, and the action buttons.

use super::library::Signature;
use crate::i18n::{tr, Lang};
use crate::style::*;
use crate::widgets::{clip_chars, Painter};
use crate::{Str, SCREEN_H, SCREEN_W};

/// Which destructive action waits for a second key press.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Confirm {
    #[default]
    None,
    Reset,
    Delete,
}

pub struct DetailView<'a> {
    pub lang: Lang,
    /// `false`: the file disappeared ("not found" screen).
    pub found: bool,
    /// Catalogue id for the picture, empty when the character is unknown.
    pub entry_id: &'a str,
    pub name: &'a str,
    pub amiibo_series: &'a str,
    /// "The Legend of Zelda - Figure", or `None` when the character is not in the catalogue.
    pub game_and_type: Option<String>,
    pub favorite: bool,
    pub signature: Signature,
    pub uid_rotation: bool,
    pub uid: &'a str,
    pub head: u32,
    pub tail: u32,
    pub file: &'a str,
    pub confirm: Confirm,
    pub banner: Option<(&'a str, bool)>,
    pub battery: Option<(i32, bool)>,
}

pub fn render(p: &mut Painter, v: &DetailView) {
    let t = |s: Str| tr(v.lang, s);
    p.canvas.fill_rect(0, 0, SCREEN_W, SCREEN_H, BACKGROUND);
    p.frame();
    if !v.found {
        p.bar("ZEROTAG", t(Str::NotFound), v.battery);
        p.footer(&[(t(Str::KeyEsc), t(Str::HintBack))], v.banner);
        return;
    }
    p.bar(v.name, v.amiibo_series, v.battery);

    p.canvas.bordered(12, 28, 84, 116, SURFACE, 2, CREAM);
    p.thumb(v.entry_id, 12 + (84 - 55) / 2, 38, 96);

    let x = 106;
    let w = SCREEN_W - x - 10;
    p.text(v.name, 14, true, TEXT, x, 25, Some(w - if v.favorite { 14 } else { 0 }));
    if v.favorite {
        p.pixel_star(x + w - 7, 27, PRIMARY);
    }
    let sub = v.game_and_type.clone().unwrap_or_else(|| t(Str::UnknownCharacter).to_owned());
    p.text(&sub, 9, false, DIM, x, 44, Some(w));

    let sig_color = v.signature.color();
    p.chip(x, 59, 106, 15, t(v.signature.label()), 9, false, sig_color, sig_color);
    let rotation = format!("{} {}", t(Str::ChipRotation), t(if v.uid_rotation { Str::ValueOn } else { Str::ValueOff }));
    p.chip(
        x + 110,
        59,
        94,
        15,
        &rotation,
        9,
        false,
        if v.uid_rotation { PRIMARY } else { TEXT },
        if v.uid_rotation { PRIMARY } else { CREAM },
    );

    let id = format!("{:08x} {:08x}", v.head, v.tail);
    let rows = [("UID", v.uid.to_owned()), ("Id", id), (t(Str::LabelFile), v.file.to_owned())];
    let mut y = 80;
    for (k, val) in rows {
        p.text(k, 9, false, DIM, x, y, Some(36));
        p.text(&clip_chars(&val, 30), 9, false, TEXT, x + 38, y, Some(w - 38));
        y += 13;
    }

    p.button(x, 123, 108, t(Str::ButtonEmulate), ACCENT, CREAM, TEXT, true);
    let (reset_label, reset_border) = match v.confirm {
        Confirm::Reset => (t(Str::ButtonResetAsk), WARNING),
        _ => (t(Str::ButtonReset), CREAM),
    };
    p.button(x + 112, 123, 54, reset_label, SURFACE, reset_border, TEXT, false);
    let (del_label, del_border) = match v.confirm {
        Confirm::Delete => (t(Str::ButtonDeleteAsk), ERROR),
        _ => (t(Str::ButtonDelete), 0xa8606a),
    };
    p.button(x + 170, 123, 40, del_label, SURFACE, del_border, ERROR, false);

    p.footer(
        &[
            (t(Str::KeyEnter), t(Str::HintEmulate)),
            ("R", t(Str::HintNewUid)),
            ("U", t(Str::HintRotation)),
            (t(Str::KeyDel), t(Str::HintDelete)),
        ],
        v.banner,
    );
}
