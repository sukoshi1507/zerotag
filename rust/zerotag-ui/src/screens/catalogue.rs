// SPDX-License-Identifier: MIT
//! The catalogue screen: search field, list of characters, detail panel (port of render_catalogue).

use crate::i18n::{tr, Lang};
use crate::style::*;
use crate::widgets::Painter;
use crate::{Str, SCREEN_W};

/// What the list needs to know about one catalogue entry.
#[derive(Debug, Clone, Default)]
pub struct CatalogueRow {
    pub name: String,
    pub amiibo_series: String,
    pub owned: bool,
    /// Release date is in the future.
    pub upcoming: bool,
    /// Learnt from the online update.
    pub online: bool,
}

pub struct CatalogueView<'a> {
    pub lang: Lang,
    pub rows: &'a [CatalogueRow],
    /// Size of the whole catalogue (the bar shows "visible / total").
    pub total: usize,
    pub selected: usize,
    pub top: usize,
    pub query: &'a str,
    /// "Series: Super Smash Bros." when a facet filter is active.
    pub filter_chip: Option<String>,
    pub keys_ok: bool,
    pub banner: Option<(&'a str, bool)>,
    pub battery: Option<(i32, bool)>,
}

pub fn render(p: &mut Painter, v: &CatalogueView) {
    let t = |s: Str| tr(v.lang, s);
    p.canvas.fill_rect(0, 0, SCREEN_W, crate::SCREEN_H, BACKGROUND);
    p.frame();

    let mut subtitle = format!("{} / {}", v.rows.len(), v.total);
    if let Some(chip) = &v.filter_chip {
        subtitle += &format!("  {chip}");
    }
    p.bar(t(Str::CatalogueTitle), &subtitle, v.battery);
    p.search_field(v.query, t(Str::SearchPlaceholder), true, LIST_W - 20);
    p.rect(LIST_W, CONTENT_TOP + 2, 2, FOOT_TOP - CONTENT_TOP - 2, BORDER);

    if v.rows.is_empty() {
        p.text(t(Str::NoResult), 10, false, TEXT, 12, 64, Some(LIST_W - 24));
    }

    let first = v.top.min(v.rows.len().saturating_sub(1));
    for row in 0..ROWS as usize {
        let idx = first + row;
        let Some(e) = v.rows.get(idx) else { break };
        let badge = if e.owned {
            t(Str::Owned)
        } else if e.upcoming {
            t(Str::BadgeSoon)
        } else if e.online {
            t(Str::BadgeNew)
        } else {
            ""
        };
        p.list_row(LIST_TOP + row as i32 * ROW_H, idx == v.selected, &e.name, &e.amiibo_series, badge, false);
    }
    p.scrollbar(v.rows.len(), first as i32);

    if let Some(e) = v.rows.get(v.selected) {
        let px = LIST_W + 8;
        p.canvas.bordered(px, 26, 36, 62, SURFACE, 2, CREAM);
        p.thumb_holder(px + 2 + 2, 30, 54);
        let name_w = SCREEN_W - px - 42 - 8;
        p.paragraph(&e.name, 11, true, TEXT, px + 42, 26, name_w);
        let tw = SCREEN_W - px - 8;
        p.text(&e.amiibo_series, 9, false, DIM, px, 92, Some(tw));
        p.canvas.bordered(px, 106, tw, 40, BUTTON, 2, BORDER);
        let (nx, nw) = (px + 2 + 3, tw - 10);
        let (title, title_color, body) = if !v.keys_ok {
            (Str::NoteKeysTitle, WARNING, Str::NoteKeysBody)
        } else if e.owned {
            (Str::NoteOwnedTitle, PRIMARY, Str::NoteOwnedBody)
        } else {
            (Str::NoteNewTitle, WARNING, Str::NoteNewBody)
        };
        p.text(t(title), 9, true, title_color, nx, 106 + 2 + 1, Some(nw));
        p.paragraph(t(body), 9, false, DIM, nx, 106 + 2 + 13, nw);
    }
    p.footer(
        &[
            (t(Str::KeyUpDown), t(Str::HintChoose)),
            (t(Str::KeyEnter), t(Str::HintCreate)),
            (t(Str::KeyTab), t(Str::HintLibrary)),
            ("F2", t(Str::HintFilter)),
        ],
        v.banner,
    );
}
