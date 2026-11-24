// SPDX-License-Identifier: MIT
//! The emulation screen: pulsing rings around the thumbnail, reader state, live counters, stop button.

use crate::i18n::{format, tr, Lang};
use crate::style::*;
use crate::widgets::{clip_chars, Painter};
use crate::{Str, SCREEN_H, SCREEN_W};

/// What the error text of a failed start says about the cause (the same substring tests as the C++ version).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// The SPI overlay is not loaded.
    Overlay,
    /// Not allowed to open the SPI / GPIO devices.
    Permissions,
    Generic,
}

pub fn classify_error(error: &str) -> ErrorKind {
    let perms = ["Permission", "denied", "open", "GPIO"].iter().any(|k| error.contains(k));
    let overlay = ["spidev", "No such file"].iter().any(|k| error.contains(k));
    if overlay {
        ErrorKind::Overlay
    } else if perms {
        ErrorKind::Permissions
    } else {
        ErrorKind::Generic
    }
}

pub struct EmulationView<'a> {
    pub lang: Lang,
    /// Name shown in the bar (empty when the file is gone).
    pub item_name: &'a str,
    pub entry_id: &'a str,
    pub running: bool,
    /// A reader field is present.
    pub field: bool,
    /// A reader has selected the tag.
    pub selected: bool,
    pub error: Option<&'a str>,
    pub reads: u32,
    pub writes: u32,
    pub rotations: u32,
    pub seconds: u32,
    pub uid: &'a str,
    pub uid_rotation: bool,
    pub banner: Option<(&'a str, bool)>,
    pub battery: Option<(i32, bool)>,
}

pub fn render(p: &mut Painter, v: &EmulationView) {
    let t = |s: Str| tr(v.lang, s);
    p.canvas.fill_rect(0, 0, SCREEN_W, SCREEN_H, BACKGROUND);
    p.frame();
    let accent = if v.error.is_some() {
        ERROR
    } else if v.selected {
        PRIMARY
    } else {
        CREAM
    };
    p.bar(t(Str::EmulationTitle), v.item_name, v.battery);

    // rings and thumbnail
    let (cx, cy) = (68.0f32, 86.0f32);
    p.canvas.ring(cx, cy, 56.0, 2.0, BACKGROUND, if v.selected { ACCENT } else { DARK_RED });
    p.canvas.ring(cx, cy, 44.0, 2.0, BACKGROUND, if v.field { ACCENT } else { DARK_RED });
    p.canvas.ring(cx, cy, 28.0, 2.0, SURFACE, CREAM);
    if !v.item_name.is_empty() {
        p.thumb(v.entry_id, cx as i32 - 14, cy as i32 - 22, 46);
    }

    let x = 138;
    let w = SCREEN_W - x - 10;
    if let Some(error) = v.error {
        p.paragraph(t(Str::EmuFailed), 12, true, ERROR, x, 28, w);
        p.paragraph(&clip_chars(error, 90), 9, false, TEXT, x, 48, w);
        let hint = match classify_error(error) {
            ErrorKind::Overlay => Str::ErrSpi,
            ErrorKind::Permissions => Str::ErrPerms,
            ErrorKind::Generic => Str::ErrGeneric,
        };
        p.paragraph(t(hint), 9, false, DIM, x, 88, w);
    } else {
        let state = if v.selected {
            Str::EmuConnected
        } else if v.field {
            Str::EmuDetected
        } else {
            Str::EmuWaiting
        };
        p.paragraph(t(state), 12, true, accent, x, 28, w);
        p.paragraph(t(if v.selected { Str::EmuConnHelp } else { Str::EmuWaitHelp }), 9, false, DIM, x, 46, w);

        p.text(t(Str::EmuReads), 9, false, DIM, x, 90, Some(60));
        p.text(&v.reads.to_string(), 9, false, TEXT, x + 64, 90, None);
        p.text(t(Str::EmuWrites), 9, false, DIM, x, 102, Some(60));
        let writes = if v.writes > 0 { format(v.lang, Str::EmuWritesPending, &[&v.writes]) } else { "0".into() };
        p.text(&writes, 9, false, if v.writes > 0 { WARNING } else { TEXT }, x + 64, 102, Some(w - 64));
        p.text("UID", 9, false, DIM, x, 114, Some(60));
        p.text(v.uid, 9, false, TEXT, x + 64, 114, Some(w - 64));
        if v.uid_rotation {
            p.text(t(Str::EmuRotations), 9, false, DIM, x, 126, Some(60));
            p.text(&v.rotations.to_string(), 9, false, PRIMARY, x + 64, 126, None);
        } else {
            p.text(t(Str::EmuDuration), 9, false, DIM, x, 126, Some(60));
            p.text(&format!("{:02}:{:02}", v.seconds / 60, v.seconds % 60), 9, false, TEXT, x + 64, 126, None);
        }
    }
    if v.banner.is_none() {
        p.rect(INSET, FOOT_TOP, INNER_W, FOOT_H, BAR);
        p.rect(INSET, FOOT_TOP, INNER_W, 1, BORDER);
        p.text(t(Str::EmuFooterNote), 9, false, DIM, INSET + 6, FOOT_TOP + 3, Some(216));
        let (bx, by) = (SCREEN_W - INSET - 74, FOOT_TOP + 3);
        p.canvas.bordered(bx, by, 68, 13, SURFACE, 2, CREAM);
        p.text_centered(t(Str::EmuStopButton), 9, false, TEXT, bx + 2, by + 2, 64, 9);
    } else {
        p.footer(&[], v.banner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_kinds() {
        assert_eq!(classify_error("cannot open /dev/spidev0.2: No such file or directory"), ErrorKind::Overlay);
        assert_eq!(classify_error("cannot open /dev/spidev0.2: Permission denied"), ErrorKind::Overlay); // spidev wins, as in C++
        assert_eq!(classify_error("cannot request GPIO26: Permission denied"), ErrorKind::Permissions);
        assert_eq!(classify_error("ST25R3916 init failed at: oscillator"), ErrorKind::Generic);
    }
}
