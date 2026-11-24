// SPDX-License-Identifier: MIT
//! CardputerZero keyboard (tca8418, kernel driver with 3 layers: base, Sym, Fn; Shift is a normal modifier).
//! Port of src/platform/linux_input.cpp without LVGL: evdev key codes to keys, a non-blocking reader, and the
//! press / long-press / release detection.
//!
//! The Sym symbols arrive as keypad / Japanese key codes (the official table is
//! /usr/share/keymaps/tca8418_keypad_m5stack_keymap.map). APPLaunch keeps /dev/input/event3 open: other readers get
//! no events while it runs, so key tests have to happen inside the app.

use std::fs::{self, File};
use std::io::{ErrorKind, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    Esc,
    Left,
    Right,
    Up,
    Down,
    Enter,
    Backspace,
    Delete,
    Tab,
    PrintScreen,
    Help,
    PageUp,
    PageDown,
    F1,
    F2,
    F3,
    F4,
    /// A printable character (letters are capitals while Shift is held).
    Char(char),
}

impl Key {
    pub fn name(self) -> String {
        match self {
            Key::Esc => "Esc".into(),
            Key::Left => "Left".into(),
            Key::Right => "Right".into(),
            Key::PrintScreen => "PrintScreen".into(),
            Key::Help => "Help".into(),
            Key::Up => "Up".into(),
            Key::Down => "Down".into(),
            Key::Enter => "Enter".into(),
            Key::Char(c) if (' '..='~').contains(&c) => c.to_string(),
            other => format!("{other:?}"),
        }
    }
}

// Linux key codes (linux/input-event-codes.h).
mod code {
    pub const ESC: u16 = 1;
    pub const K1: u16 = 2;
    pub const K9: u16 = 10;
    pub const K0: u16 = 11;
    pub const BACKSPACE: u16 = 14;
    pub const TAB: u16 = 15;
    pub const Q: u16 = 16;
    pub const P: u16 = 25;
    pub const LEFTBRACE: u16 = 26;
    pub const RIGHTBRACE: u16 = 27;
    pub const ENTER: u16 = 28;
    pub const A: u16 = 30;
    pub const L: u16 = 38;
    pub const SEMICOLON: u16 = 39;
    pub const APOSTROPHE: u16 = 40;
    pub const GRAVE: u16 = 41;
    pub const LEFTSHIFT: u16 = 42;
    pub const BACKSLASH: u16 = 43;
    pub const Z: u16 = 44;
    pub const M: u16 = 50;
    pub const COMMA: u16 = 51;
    pub const DOT: u16 = 52;
    pub const SLASH: u16 = 53;
    pub const RIGHTSHIFT: u16 = 54;
    pub const KPASTERISK: u16 = 55;
    pub const SPACE: u16 = 57;
    pub const F1: u16 = 59;
    pub const F2: u16 = 60;
    pub const F3: u16 = 61;
    pub const F4: u16 = 62;
    pub const NUMLOCK: u16 = 69;
    pub const SCROLLLOCK: u16 = 70;
    pub const KP7: u16 = 71;
    pub const KP8: u16 = 72;
    pub const KP9: u16 = 73;
    pub const KPMINUS: u16 = 74;
    pub const KP4: u16 = 75;
    pub const KP5: u16 = 76;
    pub const KP6: u16 = 77;
    pub const KP1: u16 = 79;
    pub const KP2: u16 = 80;
    pub const KP3: u16 = 81;
    pub const KP0: u16 = 82;
    pub const KPDOT: u16 = 83;
    pub const ZENKAKUHANKAKU: u16 = 85;
    pub const K102ND: u16 = 86;
    pub const RO: u16 = 89;
    pub const KATAKANA: u16 = 90;
    pub const HIRAGANA: u16 = 91;
    pub const HENKAN: u16 = 92;
    pub const KATAKANAHIRAGANA: u16 = 93;
    pub const MUHENKAN: u16 = 94;
    pub const KPENTER: u16 = 96;
    pub const SYSRQ: u16 = 99;
    pub const UP: u16 = 103;
    pub const PAGEUP: u16 = 104;
    pub const LEFT: u16 = 105;
    pub const RIGHT: u16 = 106;
    pub const DOWN: u16 = 108;
    pub const PAGEDOWN: u16 = 109;
    pub const DELETE: u16 = 111;
    pub const HELP: u16 = 138;
}

/// The Sym layer: code -> character.
const SYM_KEYS: [(u16, char); 32] = [
    (code::LEFTBRACE, '!'),
    (code::RIGHTBRACE, '@'),
    (code::SEMICOLON, '#'),
    (code::APOSTROPHE, '$'),
    (code::GRAVE, '%'),
    (code::BACKSLASH, '^'),
    (code::COMMA, '&'),
    (code::DOT, '*'),
    (code::SLASH, '('),
    (code::MUHENKAN, ')'),
    (code::KPASTERISK, '~'),
    (code::NUMLOCK, '`'),
    (code::SCROLLLOCK, '_'),
    (code::KP7, '-'),
    (code::KP8, '+'),
    (code::KP9, '='),
    (code::KPMINUS, '['),
    (code::KP4, ']'),
    (code::KP5, '{'),
    (code::KP6, '}'),
    (code::KP1, ';'),
    (code::KP2, ':'),
    (code::KP3, '\''),
    (code::KP0, '"'),
    (code::KPDOT, '<'),
    (code::ZENKAKUHANKAKU, '>'),
    (code::K102ND, '\\'),
    (code::RO, '|'),
    (code::KATAKANA, ','),
    (code::HIRAGANA, '.'),
    (code::HENKAN, '/'),
    (code::KATAKANAHIRAGANA, '?'),
];

/// Maps an evdev key code to a key. `None` for codes the application ignores (and for the shift keys).
pub fn map_evdev_key(c: u16, shift: bool) -> Option<Key> {
    let special = match c {
        code::ESC => Some(Key::Esc),
        code::LEFT => Some(Key::Left),
        code::RIGHT => Some(Key::Right),
        code::SYSRQ => Some(Key::PrintScreen),
        code::HELP => Some(Key::Help),
        code::UP => Some(Key::Up),
        code::DOWN => Some(Key::Down),
        code::ENTER | code::KPENTER => Some(Key::Enter),
        code::BACKSPACE => Some(Key::Backspace),
        code::DELETE => Some(Key::Delete),
        code::TAB => Some(Key::Tab),
        code::SPACE => Some(Key::Char(' ')),
        code::PAGEUP => Some(Key::PageUp),
        code::PAGEDOWN => Some(Key::PageDown),
        code::F1 => Some(Key::F1),
        code::F2 => Some(Key::F2),
        code::F3 => Some(Key::F3),
        code::F4 => Some(Key::F4),
        _ => None,
    };
    if special.is_some() {
        return special;
    }
    if let Some(&(_, ch)) = SYM_KEYS.iter().find(|(k, _)| *k == c) {
        return Some(Key::Char(ch));
    }
    // Letters (Linux key codes are not contiguous: three keyboard rows) and digits. Shift gives capitals only.
    const ROW_Q: &[u8] = b"qwertyuiop";
    const ROW_A: &[u8] = b"asdfghjkl";
    const ROW_Z: &[u8] = b"zxcvbnm";
    let letter = |b: u8| Some(Key::Char(if shift { b.to_ascii_uppercase() } else { b } as char));
    match c {
        code::Q..=code::P => letter(ROW_Q[(c - code::Q) as usize]),
        code::A..=code::L => letter(ROW_A[(c - code::A) as usize]),
        code::Z..=code::M => letter(ROW_Z[(c - code::Z) as usize]),
        code::K1..=code::K9 => Some(Key::Char((b'1' + (c - code::K1) as u8) as char)),
        code::K0 => Some(Key::Char('0')),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub key: Key,
    pub pressed: bool,
}

/// A keyboard event device, read without blocking.
pub struct Keypad {
    file: File,
    shift: bool,
    buf: Vec<u8>,
}

const EV_KEY: u16 = 1;
const INPUT_EVENT_SIZE: usize = 24; // struct input_event on 64-bit: timeval (16) + type + code + value

impl Keypad {
    /// Opens `path` (O_NONBLOCK) if it looks like the keyboard (has the navigation keys).
    pub fn open(path: impl AsRef<Path>) -> Option<Self> {
        let path = path.as_ref();
        if !has_nav_keys(path) {
            return None;
        }
        let file = fs::OpenOptions::new().read(true).custom_flags(0o4000 /* O_NONBLOCK */).open(path).ok()?;
        Some(Self { file, shift: false, buf: Vec::new() })
    }

    /// Opens every keyboard device found in /dev/input (normally one).
    pub fn discover() -> Vec<Self> {
        let Ok(rd) = fs::read_dir("/dev/input") else { return Vec::new() };
        let mut paths: Vec<PathBuf> =
            rd.flatten().map(|e| e.path()).filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("event"))).collect();
        paths.sort();
        paths.iter().filter_map(Keypad::open).collect()
    }

    /// Returns the key events available now (auto-repeat and ignored keys are dropped).
    pub fn poll(&mut self) -> Vec<KeyEvent> {
        let mut chunk = [0u8; 24 * 16];
        loop {
            match self.file.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => self.buf.extend_from_slice(&chunk[..n]),
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(_) => break, // WouldBlock: nothing more to read
            }
        }
        let mut out = Vec::new();
        let whole = self.buf.len() / INPUT_EVENT_SIZE * INPUT_EVENT_SIZE;
        for ev in self.buf[..whole].chunks_exact(INPUT_EVENT_SIZE) {
            let kind = u16::from_ne_bytes([ev[16], ev[17]]);
            let c = u16::from_ne_bytes([ev[18], ev[19]]);
            let value = i32::from_ne_bytes([ev[20], ev[21], ev[22], ev[23]]);
            if let Some(e) = decode(kind, c, value, &mut self.shift) {
                out.push(e);
            }
        }
        self.buf.drain(..whole);
        out
    }
}

/// One raw event to a key event, updating the Shift state.
fn decode(kind: u16, c: u16, value: i32, shift: &mut bool) -> Option<KeyEvent> {
    if kind != EV_KEY {
        return None;
    }
    if c == code::LEFTSHIFT || c == code::RIGHTSHIFT {
        *shift = value != 0;
        return None;
    }
    let key = map_evdev_key(c, *shift)?;
    (value != 2).then_some(KeyEvent { key, pressed: value != 0 })
}

/// True when the event device has at least one of the navigation keys (read from sysfs, no ioctl needed).
fn has_nav_keys(dev: &Path) -> bool {
    let Some(name) = dev.file_name() else { return false };
    let caps = Path::new("/sys/class/input").join(name).join("device/capabilities/key");
    let Ok(text) = fs::read_to_string(caps) else { return false };
    key_bitmap_has_any(&text, &[1, 105, 106, 44, 46, 5, 6, 7, 8, 9, 99, 138])
}

/// `capabilities/key` is a list of hex words (unsigned long, 64 bits here), most significant word first.
fn key_bitmap_has_any(text: &str, codes: &[usize]) -> bool {
    let words: Vec<u64> = text.split_whitespace().filter_map(|w| u64::from_str_radix(w, 16).ok()).collect();
    codes.iter().any(|&c| {
        words.len().checked_sub(1 + c / 64).and_then(|i| words.get(i)).is_some_and(|w| w >> (c % 64) & 1 == 1)
    })
}

/// What the key router reports to the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Routed {
    /// A new key went down.
    Down(Key),
    /// The key has been held for the long-press delay (sent once per press).
    LongPress(Key),
    Up(Key),
}

pub const LONG_PRESS: Duration = Duration::from_millis(700);

/// Press / long-press / release detection (route_key_state in the C++).
#[derive(Debug, Default)]
pub struct KeyRouter {
    held: Option<(Key, Instant)>,
    long_sent: bool,
}

impl KeyRouter {
    pub fn on_event(&mut self, e: KeyEvent, now: Instant) -> Vec<Routed> {
        let mut out = Vec::new();
        let same = self.held.is_some_and(|(k, _)| k == e.key);
        if e.pressed && !same {
            if let Some((prev, _)) = self.held.take() {
                // another key went down while one is held: the old press is over
                out.push(Routed::Up(prev));
            }
            self.held = Some((e.key, now));
            self.long_sent = false;
            out.push(Routed::Down(e.key));
        } else if !e.pressed && same {
            self.held = None;
            out.push(Routed::Up(e.key));
        }
        out
    }

    /// Call regularly (every ~40 ms): reports a long press once when the delay has elapsed.
    pub fn tick(&mut self, now: Instant) -> Option<Routed> {
        let (key, since) = self.held?;
        if self.long_sent || now.duration_since(since) < LONG_PRESS {
            return None;
        }
        self.long_sent = true;
        Some(Routed::LongPress(key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(c: u16, shift: bool) -> Option<Key> {
        map_evdev_key(c, shift)
    }

    #[test]
    fn letters_digits_and_shift() {
        assert_eq!(ch(code::Q, false), Some(Key::Char('q')));
        assert_eq!(ch(code::P, true), Some(Key::Char('P')));
        assert_eq!(ch(code::A, false), Some(Key::Char('a')));
        assert_eq!(ch(code::L, true), Some(Key::Char('L')));
        assert_eq!(ch(code::Z, false), Some(Key::Char('z')));
        assert_eq!(ch(code::M, true), Some(Key::Char('M')));
        assert_eq!(ch(code::K1, true), Some(Key::Char('1'))); // shift gives capitals only
        assert_eq!(ch(code::K9, false), Some(Key::Char('9')));
        assert_eq!(ch(code::K0, false), Some(Key::Char('0')));
        assert_eq!(ch(code::SPACE, false), Some(Key::Char(' ')));
        assert_eq!(ch(29, false), None); // left ctrl: ignored
        assert_eq!(ch(code::LEFTSHIFT, false), None);
    }

    #[test]
    fn navigation_and_function_keys() {
        assert_eq!(ch(code::ESC, false), Some(Key::Esc));
        assert_eq!(ch(code::KPENTER, false), Some(Key::Enter));
        assert_eq!(ch(code::ENTER, false), Some(Key::Enter));
        assert_eq!(ch(code::PAGEDOWN, false), Some(Key::PageDown));
        assert_eq!(ch(code::F2, false), Some(Key::F2));
        assert_eq!(ch(code::HELP, false), Some(Key::Help));
        assert_eq!(ch(code::SYSRQ, false), Some(Key::PrintScreen));
    }

    #[test]
    fn sym_layer_matches_the_official_keymap_without_collisions() {
        let mut seen = std::collections::HashSet::new();
        for (c, expected) in SYM_KEYS {
            assert_eq!(ch(c, false), Some(Key::Char(expected)));
            assert_eq!(ch(c, true), Some(Key::Char(expected))); // Sym symbols ignore shift
            assert!(seen.insert(c), "duplicate code {c}");
        }
        let chars: std::collections::HashSet<_> = SYM_KEYS.iter().map(|(_, c)| *c).collect();
        assert_eq!(chars.len(), 32);
    }

    /// The numeric codes above must match the kernel header (skipped when it is not installed).
    #[test]
    fn key_codes_match_the_kernel_header() {
        let Ok(h) = fs::read_to_string("/usr/include/linux/input-event-codes.h") else { return };
        let value = |name: &str| -> u16 {
            h.lines()
                .find_map(|l| {
                    let mut w = l.split_whitespace();
                    (w.next() == Some("#define") && w.next() == Some(name)).then(|| w.next().unwrap().parse().unwrap())
                })
                .unwrap_or_else(|| panic!("{name} not found"))
        };
        for (name, v) in [
            ("KEY_ESC", code::ESC), ("KEY_1", code::K1), ("KEY_9", code::K9), ("KEY_0", code::K0),
            ("KEY_BACKSPACE", code::BACKSPACE), ("KEY_TAB", code::TAB), ("KEY_Q", code::Q), ("KEY_P", code::P),
            ("KEY_LEFTBRACE", code::LEFTBRACE), ("KEY_RIGHTBRACE", code::RIGHTBRACE), ("KEY_ENTER", code::ENTER),
            ("KEY_A", code::A), ("KEY_L", code::L), ("KEY_SEMICOLON", code::SEMICOLON),
            ("KEY_APOSTROPHE", code::APOSTROPHE), ("KEY_GRAVE", code::GRAVE), ("KEY_LEFTSHIFT", code::LEFTSHIFT),
            ("KEY_BACKSLASH", code::BACKSLASH), ("KEY_Z", code::Z), ("KEY_M", code::M), ("KEY_COMMA", code::COMMA),
            ("KEY_DOT", code::DOT), ("KEY_SLASH", code::SLASH), ("KEY_RIGHTSHIFT", code::RIGHTSHIFT),
            ("KEY_KPASTERISK", code::KPASTERISK), ("KEY_SPACE", code::SPACE), ("KEY_F1", code::F1),
            ("KEY_F2", code::F2), ("KEY_F3", code::F3), ("KEY_F4", code::F4), ("KEY_NUMLOCK", code::NUMLOCK),
            ("KEY_SCROLLLOCK", code::SCROLLLOCK), ("KEY_KP7", code::KP7), ("KEY_KP8", code::KP8),
            ("KEY_KP9", code::KP9), ("KEY_KPMINUS", code::KPMINUS), ("KEY_KP4", code::KP4), ("KEY_KP5", code::KP5),
            ("KEY_KP6", code::KP6), ("KEY_KP1", code::KP1), ("KEY_KP2", code::KP2), ("KEY_KP3", code::KP3),
            ("KEY_KP0", code::KP0), ("KEY_KPDOT", code::KPDOT), ("KEY_ZENKAKUHANKAKU", code::ZENKAKUHANKAKU),
            ("KEY_102ND", code::K102ND), ("KEY_RO", code::RO), ("KEY_KATAKANA", code::KATAKANA),
            ("KEY_HIRAGANA", code::HIRAGANA), ("KEY_HENKAN", code::HENKAN),
            ("KEY_KATAKANAHIRAGANA", code::KATAKANAHIRAGANA), ("KEY_MUHENKAN", code::MUHENKAN),
            ("KEY_KPENTER", code::KPENTER), ("KEY_SYSRQ", code::SYSRQ), ("KEY_UP", code::UP),
            ("KEY_PAGEUP", code::PAGEUP), ("KEY_LEFT", code::LEFT), ("KEY_RIGHT", code::RIGHT),
            ("KEY_DOWN", code::DOWN), ("KEY_PAGEDOWN", code::PAGEDOWN), ("KEY_DELETE", code::DELETE),
            ("KEY_HELP", code::HELP),
        ] {
            assert_eq!(value(name), v, "{name}");
        }
    }

    #[test]
    fn raw_events_decode_with_shift_state_and_repeat_filter() {
        let mut shift = false;
        assert_eq!(decode(4, 1, 1, &mut shift), None); // not EV_KEY
        assert_eq!(decode(EV_KEY, code::LEFTSHIFT, 1, &mut shift), None);
        assert!(shift);
        assert_eq!(decode(EV_KEY, code::A, 1, &mut shift), Some(KeyEvent { key: Key::Char('A'), pressed: true }));
        assert_eq!(decode(EV_KEY, code::A, 2, &mut shift), None); // auto-repeat
        assert_eq!(decode(EV_KEY, code::A, 0, &mut shift), Some(KeyEvent { key: Key::Char('A'), pressed: false }));
        decode(EV_KEY, code::RIGHTSHIFT, 0, &mut shift);
        assert!(!shift);
    }

    #[test]
    fn key_bitmap_parsing() {
        // KEY_ESC (bit 1) in the lowest word, KEY_HELP (138 = 2*64+10) in the third word from the end.
        assert!(key_bitmap_has_any("0 0 0 2", &[1]));
        assert!(!key_bitmap_has_any("0 0 0 2", &[105]));
        assert!(key_bitmap_has_any("400 0 0", &[138]));
        assert!(!key_bitmap_has_any("", &[1]));
        assert!(!key_bitmap_has_any("ffff", &[200])); // beyond the bitmap
    }

    #[test]
    fn router_down_long_up() {
        let t0 = Instant::now();
        let mut r = KeyRouter::default();
        let a = Key::Char('a');
        assert_eq!(r.on_event(KeyEvent { key: a, pressed: true }, t0), [Routed::Down(a)]);
        assert_eq!(r.tick(t0 + Duration::from_millis(100)), None);
        assert_eq!(r.tick(t0 + Duration::from_millis(800)), Some(Routed::LongPress(a)));
        assert_eq!(r.tick(t0 + Duration::from_millis(900)), None); // once per press
        assert_eq!(r.on_event(KeyEvent { key: a, pressed: false }, t0), [Routed::Up(a)]);
        assert_eq!(r.tick(t0 + Duration::from_secs(5)), None);
        // releasing a key that is not held is ignored; a second key replaces the first
        assert!(r.on_event(KeyEvent { key: a, pressed: false }, t0).is_empty());
        r.on_event(KeyEvent { key: a, pressed: true }, t0);
        let out = r.on_event(KeyEvent { key: Key::Esc, pressed: true }, t0);
        assert_eq!(out, [Routed::Up(a), Routed::Down(Key::Esc)]);
    }
}
