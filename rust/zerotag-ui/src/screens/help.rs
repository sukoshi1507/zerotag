// SPDX-License-Identifier: MIT
//! The help screen (Fn+H): the keys of the screen it was opened from.

use crate::i18n::{tr, Lang};
use crate::style::*;
use crate::widgets::Painter;
use crate::{Str, SCREEN_H, SCREEN_W};

/// The screen the help was opened from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpContext {
    Library,
    Catalogue,
    Facets,
    Detail,
    Emulating,
    Settings,
}

/// (key cap, what it does). An empty key cap is a plain note.
pub fn lines(lang: Lang, ctx: HelpContext) -> Vec<(String, String)> {
    let t = |s: Str| tr(lang, s).to_owned();
    let l = |k: &str, s: Str| (k.to_owned(), t(s));
    match ctx {
        HelpContext::Catalogue => vec![
            (t(Str::KeyUpDown), t(Str::HelpChoose)),
            l("a-z", Str::HelpTypeSearch),
            (t(Str::KeyEnter), t(Str::HelpCreate)),
            (t(Str::KeyTab), t(Str::HelpToLibrary)),
            l("F2", Str::HelpFilter),
            (t(Str::KeyEsc), t(Str::HelpClearBack)),
        ],
        HelpContext::Facets => vec![
            (t(Str::KeyUpDown), t(Str::HelpChoose)),
            (t(Str::KeyEnter), t(Str::HelpFacetPick)),
            (t(Str::KeyEsc), t(Str::HelpBack)),
        ],
        HelpContext::Detail => vec![
            (t(Str::KeyEnter), t(Str::HelpEmulate)),
            l("R", Str::HelpNewUid),
            l("U", Str::HelpRotation),
            (t(Str::KeyDel), t(Str::HelpDelete)),
            (t(Str::KeyEsc), t(Str::HelpBack)),
            l("F2", Str::HintFavorite),
        ],
        HelpContext::Emulating => vec![(t(Str::KeyEsc), t(Str::HelpStopSave)), (String::new(), t(Str::HelpAutosave))],
        HelpContext::Settings => vec![
            (t(Str::KeyUpDown), t(Str::HelpChoose)),
            (t(Str::KeyEnter), t(Str::HelpChange)),
            l("< >", Str::HelpChange),
            (t(Str::KeyEsc), t(Str::HelpBack)),
        ],
        HelpContext::Library => vec![
            (t(Str::KeyUpDown), t(Str::HelpChoose)),
            (t(Str::KeyEnter), t(Str::HelpOpen)),
            l("a-z", Str::HelpTypeFilter),
            (t(Str::KeyTab), t(Str::HelpToCatalogue)),
            l("F1", Str::HelpSettings),
            l("F4", Str::HelpScan),
            (t(Str::KeyEsc), t(Str::HelpHoldQuit)),
            l("F2", Str::HelpFavTabs),
        ],
    }
}

pub struct HelpView<'a> {
    pub lang: Lang,
    pub context: HelpContext,
    pub banner: Option<(&'a str, bool)>,
    pub battery: Option<(i32, bool)>,
}

pub fn render(p: &mut Painter, v: &HelpView) {
    let t = |s: Str| tr(v.lang, s);
    p.canvas.fill_rect(0, 0, SCREEN_W, SCREEN_H, BACKGROUND);
    p.frame();
    p.bar("ZEROTAG", t(Str::HelpTitle), v.battery);
    let mut y = CONTENT_TOP + 6;
    for (key, label) in lines(v.lang, v.context) {
        if !key.is_empty() {
            let kw = p.fonts.width(&key, 9, false) + 8;
            p.canvas.bordered(12, y, kw, 13, SURFACE, 2, CREAM);
            p.text_centered(&key, 9, false, TEXT, 14, y + 2, kw - 4, 9);
        }
        p.text(&label, 10, false, if key.is_empty() { DIM } else { TEXT }, 78, y, Some(SCREEN_W - 78 - 12));
        y += 17;
    }
    // The footer is drawn last: the eighth line of the library help runs under it, as in the original.
    p.footer(&[(t(Str::KeyEsc), t(Str::HintBack))], v.banner);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_context_has_its_lines_in_every_language() {
        let counts = [
            (HelpContext::Library, 8),
            (HelpContext::Catalogue, 6),
            (HelpContext::Facets, 3),
            (HelpContext::Detail, 6),
            (HelpContext::Emulating, 2),
            (HelpContext::Settings, 4),
        ];
        for lang in [Lang::Fr, Lang::En, Lang::Ja, Lang::Zh] {
            for (ctx, n) in counts {
                let l = lines(lang, ctx);
                assert_eq!(l.len(), n, "{ctx:?}");
                assert!(l.iter().all(|(_, text)| !text.is_empty()));
            }
        }
        assert_eq!(lines(Lang::En, HelpContext::Detail)[0], ("ENTER".into(), "Emulate: Cap against the console".into()));
        assert!(lines(Lang::Fr, HelpContext::Emulating)[1].0.is_empty()); // a plain note has no key cap
    }
}
