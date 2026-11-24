// SPDX-License-Identifier: MIT
//! The settings screen: language, UID rotation, sound, online update, Cap / keys / folder status, help, manual.

use super::facets::ROW_H30;
use crate::i18n::{tr, Lang};
use crate::style::*;
use crate::widgets::Painter;
use crate::{Str, SCREEN_H, SCREEN_W};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok,
    Warning,
    Error,
}

#[derive(Debug, Clone, Default)]
pub struct SettingsRow {
    pub title: String,
    pub detail: String,
    /// Right side: "< Français >", "< ON >"...
    pub value: String,
    pub status: Option<Status>,
}

impl Default for Status {
    fn default() -> Self {
        Status::Ok
    }
}

pub struct SettingsView<'a> {
    pub lang: Lang,
    pub rows: &'a [SettingsRow],
    pub selected: usize,
    pub top: usize,
    pub banner: Option<(&'a str, bool)>,
    pub battery: Option<(i32, bool)>,
}

/// The value of a toggle as the row shows it: "< ON >" / "< OFF >" in the current language.
pub fn toggle_value(lang: Lang, on: bool) -> String {
    format!("< {} >", tr(lang, if on { Str::ValueOn } else { Str::ValueOff }))
}

pub fn render(p: &mut Painter, v: &SettingsView) {
    let t = |s: Str| tr(v.lang, s);
    p.canvas.fill_rect(0, 0, SCREEN_W, SCREEN_H, BACKGROUND);
    p.frame();
    p.bar("ZEROTAG", t(Str::SetupTitle), v.battery);
    for slot in 0..4usize {
        let i = v.top + slot;
        let Some(row) = v.rows.get(i) else { break };
        let y = CONTENT_TOP + 3 + slot as i32 * ROW_H30;
        let sel = i == v.selected;
        if sel {
            p.rect(INSET + 1, y, INNER_W - 2, ROW_H30 - 2, SELECTED_ROW);
            p.cursor(INSET + 4, y + 10);
        }
        p.text(&row.title, 11, sel, TEXT, 24, y + 1, Some(150));
        if !row.detail.is_empty() {
            p.text(&row.detail, 9, false, DIM, 24, y + 15, Some(INNER_W - 52));
        }
        if !row.value.is_empty() {
            p.text_right(&row.value, 11, true, PRIMARY, INSET + 8, y + 7);
        }
        if let Some(status) = row.status {
            let c = match status {
                Status::Ok => PRIMARY,
                Status::Warning => WARNING,
                Status::Error => ERROR,
            };
            p.rect(SCREEN_W - INSET - 16, y + 10, 8, 8, c);
        }
    }
    p.edge_bar(v.rows.len(), v.top as i32, ROW_H30);
    p.footer(
        &[(t(Str::KeyUpDown), t(Str::HintChoose)), (t(Str::KeyEnter), t(Str::HintChange)), (t(Str::KeyEsc), t(Str::HintBack))],
        v.banner,
    );
}
