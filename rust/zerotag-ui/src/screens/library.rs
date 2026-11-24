// SPDX-License-Identifier: MIT
//! The library screen: the user's backups with a filter field, the tab chip and a details panel.

use crate::i18n::{tr, Lang};
use crate::style::*;
use crate::widgets::Painter;
use crate::{Str, SCREEN_H, SCREEN_W};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    All,
    Favorites,
    Recent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Signature {
    #[default]
    Unchecked,
    Valid,
    Invalid,
}

impl Signature {
    pub fn label(self) -> Str {
        match self {
            Signature::Unchecked => Str::SigUnchecked,
            Signature::Valid => Str::SigValid,
            Signature::Invalid => Str::SigInvalid,
        }
    }

    pub fn color(self) -> u32 {
        match self {
            Signature::Unchecked => DIM,
            Signature::Valid => PRIMARY,
            Signature::Invalid => ERROR,
        }
    }
}

/// One backup of the library as the list and the panel show it.
#[derive(Debug, Clone, Default)]
pub struct LibraryRow {
    /// Catalogue id for the thumbnail, empty when the character is unknown.
    pub id: String,
    pub name: String,
    /// Amiibo series, or the UID when the character is not in the catalogue.
    pub sub: String,
    pub favorite: bool,
    pub amiibo_series: String,
    pub game_series: String,
    pub uid: String,
    pub signature: Signature,
    /// The character is in the catalogue (series lines are shown).
    pub known: bool,
}

pub struct LibraryView<'a> {
    pub lang: Lang,
    pub rows: &'a [LibraryRow],
    /// Number of backups in the folder (the bar shows it, whatever the filter).
    pub total: usize,
    pub selected: usize,
    pub top: usize,
    pub query: &'a str,
    pub tab: Tab,
    pub keys_ok: bool,
    pub banner: Option<(&'a str, bool)>,
    pub battery: Option<(i32, bool)>,
}

pub fn render(p: &mut Painter, v: &LibraryView) {
    let t = |s: Str| tr(v.lang, s);
    p.canvas.fill_rect(0, 0, SCREEN_W, SCREEN_H, BACKGROUND);
    p.frame();
    p.bar("ZEROTAG", &format!("{} - {}", t(Str::LibraryTitle), v.total), v.battery);
    p.search_field(v.query, t(Str::FilterPlaceholder), !v.query.is_empty(), 108);

    // Tab chip (F3): All / Favourites / Recent
    let tab_active = v.tab != Tab::All;
    let label = match v.tab {
        Tab::Favorites => t(Str::TabFavorites),
        Tab::Recent => t(Str::TabRecent),
        Tab::All => t(Str::TabAll),
    };
    let color = if tab_active { PRIMARY } else { TEXT };
    p.canvas.bordered(122, 26, 60, 16, BUTTON, 2, if tab_active { PRIMARY } else { CREAM });
    let clipped = p.fonts.clip(label, 9, tab_active, 54);
    p.text_centered(&clipped, 9, tab_active, color, 124, 28, 56, 12);
    p.rect(LIST_W, CONTENT_TOP + 2, 2, FOOT_TOP - CONTENT_TOP - 2, BORDER);

    if v.rows.is_empty() {
        let (line1, line2) = if v.total > 0 && v.query.is_empty() && tab_active {
            (t(if v.tab == Tab::Favorites { Str::EmptyFavorites } else { Str::EmptyRecent }), "")
        } else {
            (
                t(if v.total == 0 { Str::LibraryEmpty } else { Str::NoResult }),
                t(if v.keys_ok { Str::LibraryHowTo } else { Str::KeysMissingF1 }),
            )
        };
        p.paragraph(line1, 10, false, TEXT, 12, 64, LIST_W - 24);
        p.paragraph(line2, 9, false, DIM, 12, 86, LIST_W - 24);
    }

    let first = v.top.min(v.rows.len().saturating_sub(1));
    for row in 0..ROWS as usize {
        let idx = first + row;
        let Some(r) = v.rows.get(idx) else { break };
        p.list_row(LIST_TOP + row as i32 * ROW_H, idx == v.selected, &r.id, &r.name, &r.sub, "", r.favorite);
    }
    p.scrollbar(v.rows.len(), first as i32);

    if let Some(r) = v.rows.get(v.selected) {
        let px = LIST_W + 8;
        p.canvas.bordered(px, 26, 36, 62, SURFACE, 2, CREAM);
        p.thumb(&r.id, px + 2 + 2, 30, 54);
        let name_w = SCREEN_W - px - 42 - 8 - if r.favorite { 10 } else { 0 };
        p.paragraph(&r.name, 11, true, TEXT, px + 42, 26, name_w);
        if r.favorite {
            p.pixel_star(SCREEN_W - INSET - 9, 27, PRIMARY);
        }
        let tw = SCREEN_W - px - 8;
        if r.known {
            p.text(&r.amiibo_series, 9, false, DIM, px, 92, Some(tw));
            p.text(&r.game_series, 9, false, DIM, px, 104, Some(tw));
        }
        p.text(&r.uid, 9, false, TEXT, px, 118, Some(tw));
        p.text(t(r.signature.label()), 9, false, r.signature.color(), px, 131, Some(tw));
    }
    p.footer(
        &[
            (t(Str::KeyUpDown), t(Str::HintChoose)),
            (t(Str::KeyEnter), t(Str::HintOpen)),
            (t(Str::KeyTab), t(Str::HintCatalogue)),
            ("F1", t(Str::HintSettings)),
        ],
        v.banner,
    );
}
