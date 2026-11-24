// SPDX-License-Identifier: MIT
//! The scan screen (F4 in the library): waits for a figure on the Cap, reads it, saves it.

use super::emulation::{classify_error, ErrorKind};
use crate::i18n::{format, tr, Lang};
use crate::style::*;
use crate::widgets::{clip_chars, Painter};
use crate::{Str, SCREEN_H, SCREEN_W};

/// What the screen shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanPhaseView {
    Opening,
    /// `issue`: the last attempt failed because of this.
    Waiting { issue: Option<ScanIssueView> },
    Reading { progress: u32 },
    Saved { name: Option<String>, entry_id: String, file: String, uid: String },
    Failed { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanIssueView {
    NotNtag215,
    Interrupted { page: u32 },
}

pub struct ScanView<'a> {
    pub lang: Lang,
    pub phase: &'a ScanPhaseView,
    pub banner: Option<(&'a str, bool)>,
    pub battery: Option<(i32, bool)>,
}

pub const NTAG_PAGES: u32 = 135;

pub fn render(p: &mut Painter, v: &ScanView) {
    let t = |s: Str| tr(v.lang, s);
    p.canvas.fill_rect(0, 0, SCREEN_W, SCREEN_H, BACKGROUND);
    p.frame();
    p.bar(t(Str::ScanTitle), "NTAG215", v.battery);

    let reading = matches!(v.phase, ScanPhaseView::Reading { .. });
    let saved = matches!(v.phase, ScanPhaseView::Saved { .. });
    // Rings as on the emulation screen: red while the figure is read, gold once it is saved.
    let (cx, cy) = (68.0f32, 86.0f32);
    p.canvas.ring(cx, cy, 56.0, 2.0, BACKGROUND, if reading { ACCENT } else { DARK_RED });
    let inner = if saved {
        PRIMARY
    } else if reading {
        ACCENT
    } else {
        DARK_RED
    };
    p.canvas.ring(cx, cy, 44.0, 2.0, BACKGROUND, inner);
    p.canvas.ring(cx, cy, 28.0, 2.0, SURFACE, CREAM);
    match v.phase {
        ScanPhaseView::Saved { entry_id, .. } if !entry_id.is_empty() => p.thumb(entry_id, cx as i32 - 14, cy as i32 - 22, 46),
        _ => p.triforce(cx as i32 - 9, cy as i32 - 4),
    }

    let x = 138;
    let w = SCREEN_W - x - 10;
    match v.phase {
        ScanPhaseView::Opening => {
            p.paragraph(t(Str::ScanOpening), 12, true, TEXT, x, 28, w);
        }
        ScanPhaseView::Waiting { issue } => {
            p.paragraph(t(Str::ScanWaiting), 12, true, TEXT, x, 28, w);
            p.paragraph(t(Str::ScanWaitHelp), 9, false, DIM, x, 46, w);
            match issue {
                Some(ScanIssueView::NotNtag215) => {
                    p.paragraph(t(Str::ScanNotNtag), 9, false, ERROR, x, 92, w);
                }
                Some(ScanIssueView::Interrupted { page }) => {
                    p.paragraph(&format(v.lang, Str::ScanReadFailed, &[page]), 9, false, ERROR, x, 92, w);
                }
                None => {}
            }
        }
        ScanPhaseView::Reading { progress } => {
            p.paragraph(t(Str::ScanReading), 12, true, WARNING, x, 28, w);
            p.canvas.bordered(x, 60, w, 8, BUTTON, 2, BORDER);
            let inner = w - 4;
            if *progress > 0 {
                p.rect(x + 2, 62, inner * *progress as i32 / NTAG_PAGES as i32, 4, PRIMARY);
            }
            p.text(&format(v.lang, Str::ScanProgress, &[progress, &NTAG_PAGES]), 9, false, TEXT, x, 74, Some(w));
            p.paragraph(t(Str::ScanKeepHelp), 9, false, DIM, x, 90, w);
        }
        ScanPhaseView::Saved { name, file, uid, .. } => {
            p.paragraph(t(Str::ScanSaved), 12, true, PRIMARY, x, 28, w);
            p.text(name.as_deref().unwrap_or_else(|| t(Str::UnknownCharacter)), 11, true, TEXT, x, 48, Some(w));
            p.text(file, 9, false, DIM, x, 66, Some(w));
            p.text(&format!("UID {uid}"), 9, false, TEXT, x, 80, Some(w));
        }
        ScanPhaseView::Failed { message } => {
            p.paragraph(t(Str::ScanFailed), 12, true, ERROR, x, 28, w);
            p.paragraph(&clip_chars(message, 90), 9, false, TEXT, x, 48, w);
            let hint = match classify_error(message) {
                ErrorKind::Overlay => Str::ErrSpi,
                ErrorKind::Permissions => Str::ErrPerms,
                ErrorKind::Generic => Str::ErrGeneric,
            };
            p.paragraph(t(hint), 9, false, DIM, x, 92, w);
        }
    }
    p.footer(&[(t(Str::KeyEsc), t(Str::HintBack))], v.banner);
}
