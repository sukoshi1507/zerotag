// SPDX-License-Identifier: MIT
//! Catalogue filter screen (F2): first the category (series, game, type), then one of its values.

use crate::i18n::{tr, Lang};
use crate::style::*;
use crate::widgets::Painter;
use crate::{Str, SCREEN_H, SCREEN_W};

/// One selectable line: a title and the value shown at the right (a count).
#[derive(Debug, Clone)]
pub struct FacetRow {
    pub title: String,
    pub right: String,
}

pub struct FacetsView<'a> {
    pub lang: Lang,
    /// Name of the category being browsed (second stage), `None` on the first stage.
    pub browsing: Option<&'a str>,
    pub rows: &'a [FacetRow],
    pub selected: usize,
    pub top: usize,
    pub banner: Option<(&'a str, bool)>,
    pub battery: Option<(i32, bool)>,
}

pub const ROW_H30: i32 = 30;

pub fn facet_row(p: &mut Painter, y: i32, selected: bool, title: &str, right: &str) {
    if selected {
        p.rect(INSET + 1, y, INNER_W - 10, 28, SELECTED_ROW);
        p.cursor(INSET + 4, y + 10);
    }
    p.text(title, 11, selected, TEXT, 24, y + 8, Some(if right.is_empty() { INNER_W - 40 } else { 190 }));
    if !right.is_empty() {
        p.text_right(right, 11, true, PRIMARY, INSET + 8, y + 8);
    }
}

pub fn render(p: &mut Painter, v: &FacetsView) {
    let t = |s: Str| tr(v.lang, s);
    p.canvas.fill_rect(0, 0, SCREEN_W, SCREEN_H, BACKGROUND);
    p.frame();
    p.bar(t(Str::FacetsTitle), v.browsing.unwrap_or(""), v.battery);
    if v.rows.is_empty() {
        p.text(t(Str::NoResult), 10, false, TEXT, 12, CONTENT_TOP + 12, Some(SCREEN_W - 24));
    }
    for slot in 0..ROWS as usize {
        let i = v.top + slot;
        let Some(r) = v.rows.get(i) else { break };
        facet_row(p, CONTENT_TOP + 3 + slot as i32 * ROW_H30, i == v.selected, &r.title, &r.right);
    }
    if v.browsing.is_some() {
        p.edge_bar(v.rows.len(), v.top as i32, ROW_H30);
    }
    let action = if v.browsing.is_some() { Str::HintFilter } else { Str::HintOpen };
    p.footer(
        &[(t(Str::KeyUpDown), t(Str::HintChoose)), (t(Str::KeyEnter), t(action)), (t(Str::KeyEsc), t(Str::HintBack))],
        v.banner,
    );
}
