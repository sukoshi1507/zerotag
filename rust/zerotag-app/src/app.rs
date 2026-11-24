// SPDX-License-Identifier: MIT
//! The application: state, key handling and rendering of the screens (port of the logic of zerotag_ui.cpp).
//! Everything the hardware does goes through the `Hardware` trait, so it is tested with a mock.

use crate::hw::{Hardware, Image};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use zerotag_core::amiibo::{
    generate, read_image, rotate_uid, write_image, Database, Entry, Facet, Item, Keys, Library, LibraryState,
};
use zerotag_core::audio::{self, Sound};
use zerotag_core::cap_setup::Status as CapStatus;
use zerotag_core::config::{save_setting, ApplicationConfig};
use zerotag_core::device_status::read_device_status;
use zerotag_core::input::{Key, Routed};
use zerotag_ui::i18n::{format, tr};
use zerotag_ui::screens::cap_setup::{self, CapSetupView};
use zerotag_ui::screens::catalogue::{self, CatalogueRow, CatalogueView};
use zerotag_ui::screens::detail::{self, Confirm, DetailView};
use zerotag_ui::screens::emulation::{self, EmulationView};
use zerotag_ui::screens::facets::{self, FacetRow, FacetsView};
use zerotag_ui::screens::library::{self, LibraryRow, LibraryView, Signature, Tab};
use zerotag_ui::screens::settings::{self, toggle_value, SettingsRow, SettingsView, Status};
use zerotag_ui::thumbs::Thumbnails;
use zerotag_ui::widgets::{clip_chars, Painter};
use zerotag_ui::{Canvas, Fonts, Lang, Str};

const ROWS: usize = 4;
const BANNER: Duration = Duration::from_millis(4000);
const AUTOSAVE_QUIET: Duration = Duration::from_millis(1500);
const FACETS: [Facet; 3] = [Facet::AmiiboSeries, Facet::Game, Facet::Type];
const SETTINGS_ROWS: usize = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Library,
    Catalogue,
    Facets,
    Detail,
    Emulating,
    Settings,
    CapSetup,
}

/// Where everything lives.
#[derive(Debug, Clone)]
pub struct Paths {
    /// Contains `amiibo/amiibo_db.json` and `amiibo/images/`.
    pub assets: PathBuf,
    pub library_dir: PathBuf,
    pub keys: PathBuf,
    pub state: PathBuf,
    pub online: PathBuf,
    /// User configuration file (settings are saved here).
    pub config: PathBuf,
    /// Extra thumbnail folder (~/.zerotag/images).
    pub user_images: PathBuf,
}

impl Paths {
    /// Layout of the C++ app: ~/amiibo, ~/.zerotag/..., ~/.config/zerotag/zerotag.conf. `ZEROTAG_AMIIBO_DIR` and
    /// `ZEROTAG_KEYS` override the folder and the key file.
    pub fn for_home(home: &Path, assets: PathBuf) -> Self {
        let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        Self {
            assets,
            library_dir: env("ZEROTAG_AMIIBO_DIR").unwrap_or_else(|| home.join("amiibo")),
            keys: env("ZEROTAG_KEYS").unwrap_or_else(|| home.join(".zerotag/key_retail.bin")),
            state: home.join(".zerotag/library_state.txt"),
            online: home.join(".zerotag/amiibo_online.json"),
            config: std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .unwrap_or_else(|| home.join(".config"))
                .join("zerotag/zerotag.conf"),
            user_images: home.join(".zerotag/images"),
        }
    }
}

fn printable(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, ' ' | '-' | '.' | '\'')
}

/// Moves the selection by `delta` inside `count` rows and scrolls the window of `rows` rows to keep it visible.
pub fn move_selection(delta: i32, count: usize, sel: &mut usize, top: &mut usize, rows: usize) {
    if count == 0 {
        *sel = 0;
        *top = 0;
        return;
    }
    *sel = (*sel as i32 + delta).clamp(0, count as i32 - 1) as usize;
    if *sel < *top {
        *top = *sel;
    }
    if *sel >= *top + rows {
        *top = *sel + 1 - rows;
    }
}

/// Edits a search field: Backspace, or a printable character (capitals are folded). True when the key was used.
fn handle_text_key(key: Key, query: &mut String) -> bool {
    match key {
        Key::Backspace => {
            query.pop();
            true
        }
        Key::Char(c) => {
            let c = c.to_ascii_lowercase();
            if printable(c) && query.len() < 24 {
                query.push(c);
                true
            } else {
                false
            }
        }
        _ => false,
    }
}

pub struct App<H: Hardware> {
    hw: H,
    paths: Paths,
    pub lang: Lang,
    uid_rotation: bool,
    sound: bool,
    online: bool,

    db: Database,
    db_error: Option<String>,
    keys: Option<Arc<Keys>>,
    library: Library,
    library_ok: bool,
    state: LibraryState,

    fonts: Fonts,
    thumbs: Thumbnails,

    screen: Screen,
    before_settings: Screen,
    lib_query: String,
    lib_sel: usize,
    lib_top: usize,
    lib_tab: Tab,
    cat_query: String,
    cat_sel: usize,
    cat_top: usize,
    filter: Option<(Facet, String)>,
    facet_stage: u8,
    facet_sel: usize,
    facet_top: usize,
    facet_browse: Facet,
    current: Option<PathBuf>,
    confirm: Confirm,
    settings_sel: usize,
    settings_top: usize,
    cap_status: String,
    cap_ok: bool,

    emu_error: Option<String>,
    emu_started: Instant,
    last_stats_key: u64,

    cap_password: String,
    cap_message: Option<String>,
    cap_busy: bool,
    after_cap: Screen,

    banner: Option<(String, bool, Instant)>,
    battery: Option<(i32, bool)>,
    battery_read: Option<Instant>,
    esc_down: bool,
    esc_handled: bool,
    pub want_quit: bool,
    pub want_screenshot: bool,
    dirty: bool,
}

impl<H: Hardware> App<H> {
    pub fn new(hw: H, paths: Paths, config: &ApplicationConfig, now: Instant) -> Result<Self, String> {
        let lang = Lang::from_code(&config.language, Lang::Fr);
        let fonts = Fonts::load(lang)?;
        let thumbs = Thumbnails::new(vec![paths.assets.join("amiibo/images"), paths.user_images.clone()]);
        let mut app = Self {
            hw,
            lang,
            uid_rotation: config.uid_rotation,
            sound: config.sound,
            online: config.online,
            db: Database::default(),
            db_error: None,
            keys: None,
            library: Library::default(),
            library_ok: false,
            state: LibraryState::default(),
            fonts,
            thumbs,
            screen: Screen::Library,
            before_settings: Screen::Library,
            lib_query: String::new(),
            lib_sel: 0,
            lib_top: 0,
            lib_tab: Tab::All,
            cat_query: String::new(),
            cat_sel: 0,
            cat_top: 0,
            filter: None,
            facet_stage: 0,
            facet_sel: 0,
            facet_top: 0,
            facet_browse: Facet::AmiiboSeries,
            current: None,
            confirm: Confirm::None,
            settings_sel: 0,
            settings_top: 0,
            cap_status: String::new(),
            cap_ok: false,
            emu_error: None,
            emu_started: now,
            last_stats_key: 0,
            cap_password: String::new(),
            cap_message: None,
            cap_busy: false,
            after_cap: Screen::Library,
            banner: None,
            battery: None,
            battery_read: None,
            esc_down: false,
            esc_handled: false,
            want_quit: false,
            want_screenshot: false,
            dirty: true,
            paths,
        };
        audio::set_enabled(app.sound);
        app.load_data();
        if !app.hw.spi_available() {
            app.after_cap = Screen::Library;
            app.screen = Screen::CapSetup;
        }
        Ok(app)
    }

    // ---- data

    fn load_data(&mut self) {
        let db_path = self.paths.assets.join("amiibo/amiibo_db.json");
        match Database::load(&db_path) {
            Ok(mut db) => {
                // The list downloaded by the optional online update adds the figures the bundled catalogue does not know.
                if self.paths.online.is_file() {
                    if let Err(e) = db.merge(&self.paths.online) {
                        eprintln!("online catalogue: {e}");
                    }
                }
                self.db = db;
            }
            Err(e) => self.db_error = Some(e),
        }
        self.reload_keys();
        match LibraryState::load(&self.paths.state) {
            Ok(s) => self.state = s,
            Err(e) => eprintln!("library state: {e}"),
        }
        self.rescan();
    }

    fn reload_keys(&mut self) {
        self.keys = Keys::load(&self.paths.keys).ok().map(Arc::new);
    }

    fn rescan(&mut self) {
        match Library::scan(&self.paths.library_dir, &self.db, self.keys.as_deref()) {
            Ok(lib) => {
                self.library = lib;
                self.library_ok = true;
            }
            Err(e) => {
                eprintln!("library: {e}");
                self.library = Library::new(&self.paths.library_dir); // `create` makes the folder on first use
                self.library_ok = false;
            }
        }
    }

    fn base_name(path: &Path) -> String {
        path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    }

    fn item_name(item: &Item) -> String {
        item.entry.as_ref().map_or_else(|| format!("Inconnu {:08x}{:08x}", item.head, item.tail), |e| e.name.clone())
    }

    fn is_favorite(&self, item: &Item) -> bool {
        self.state.is_favorite(&Self::base_name(&item.path))
    }

    /// Indices into `library.items()` of the backups the library screen lists.
    fn visible_items(&self) -> Vec<usize> {
        let mut items: Vec<usize> = if self.lib_query.is_empty() {
            (0..self.library.items().len()).collect()
        } else {
            self.library.search_indices(&self.db, &self.lib_query)
        };
        let all = self.library.items();
        match self.lib_tab {
            Tab::Recent => {
                let mut recent = Vec::new();
                for name in &self.state.recent {
                    if let Some(&i) = items.iter().find(|&&i| Self::base_name(&all[i].path) == *name) {
                        recent.push(i);
                    }
                }
                recent
            }
            Tab::Favorites => {
                items.retain(|&i| self.is_favorite(&all[i]));
                items
            }
            // "All" puts the favourites first (scan order kept)
            Tab::All => {
                let (mut fav, rest): (Vec<_>, Vec<_>) = items.into_iter().partition(|&i| self.is_favorite(&all[i]));
                fav.extend(rest);
                fav
            }
        }
    }

    fn visible_entries(&self) -> Vec<usize> {
        match &self.filter {
            Some((facet, value)) => self.db.search_in_indices(*facet, value, &self.cat_query),
            None => self.db.search_indices(&self.cat_query),
        }
    }

    fn current_item(&self) -> Option<&Item> {
        let path = self.current.as_ref()?;
        self.library.items().iter().find(|i| &i.path == path)
    }

    // ---- feedback

    fn say(&mut self, msg: impl Into<String>, error: bool) {
        self.banner = Some((msg.into(), error, Instant::now() + BANNER));
        self.dirty = true;
    }

    fn t(&self, s: Str) -> &'static str {
        tr(self.lang, s)
    }

    fn go(&mut self, screen: Screen) {
        self.screen = screen;
        self.confirm = Confirm::None;
        self.dirty = true;
    }

    fn persist(&self, key: &str, value: &str) {
        if let Err(e) = save_setting(&self.paths.config, key, value) {
            eprintln!("failed to save {key} to {}: {e}", self.paths.config.display());
        }
    }

    // ---- actions

    fn toggle_favorite(&mut self, idx: usize) {
        let name = Self::base_name(&self.library.items()[idx].path);
        let now_fav = self.state.toggle_favorite(&name);
        if let Err(e) = self.state.save(&self.paths.state) {
            self.state.toggle_favorite(&name); // keep the screen and the file in step: undo
            self.say(format(self.lang, Str::BannerFailed, &[&e]), true);
            return;
        }
        if now_fav {
            audio::play(Sound::Blip);
        }
        let msg = format(self.lang, if now_fav { Str::BannerFavAdded } else { Str::BannerFavRemoved }, &[&name]);
        self.say(msg, false);
    }

    fn create_blank(&mut self, entry_idx: usize) {
        let Some(keys) = self.keys.clone() else {
            self.say(self.t(Str::KeysMissingF1), true);
            return;
        };
        let entry: Entry = self.db.entries()[entry_idx].clone();
        let path = match self.library.create(&keys, &entry) {
            Ok(p) => p,
            Err(e) => {
                self.say(format(self.lang, Str::BannerFailed, &[&e]), true);
                return;
            }
        };
        self.rescan();
        self.lib_query.clear();
        self.lib_sel = 0;
        self.lib_top = 0;
        if let Some(pos) = self.visible_items().iter().position(|&i| self.library.items()[i].path == path) {
            self.lib_sel = pos;
            self.lib_top = (pos + 1).saturating_sub(ROWS);
        }
        let msg = format(self.lang, Str::BannerCreated, &[&Self::base_name(&path)]);
        self.say(msg, false);
        self.go(Screen::Library);
    }

    fn delete_item(&mut self, path: PathBuf) {
        // Overwrite before removing: a backup is a tag dump.
        let _ = std::fs::write(&path, [0u8; 540]);
        let removed = std::fs::remove_file(&path).is_ok();
        if removed {
            self.state.forget(&Self::base_name(&path));
            let _ = self.state.save(&self.paths.state);
        }
        self.rescan();
        let count = self.visible_items().len();
        self.lib_sel = self.lib_sel.min(count.saturating_sub(1));
        let msg = if removed {
            format(self.lang, Str::BannerDeleted, &[&Self::base_name(&path)])
        } else {
            self.t(Str::BannerCantDelete).to_owned()
        };
        self.say(msg, !removed);
        self.go(Screen::Library);
    }

    fn reset_item(&mut self, path: PathBuf, head: u32, tail: u32) {
        let Some(keys) = self.keys.clone() else {
            self.say(self.t(Str::KeysMissingF1), true);
            return;
        };
        let done = generate(&keys, head, tail).map_err(|e| e.to_string()).and_then(|img| write_image(&path, &img));
        match done {
            Ok(()) => {
                self.rescan();
                self.say(self.t(Str::BannerResetDone), false);
            }
            Err(e) => self.say(format(self.lang, Str::BannerFailed, &[&e]), true),
        }
        self.dirty = true;
    }

    fn start_emulation(&mut self, path: PathBuf) {
        if !self.hw.spi_available() {
            self.open_cap_setup(Screen::Detail);
            return;
        }
        self.emu_error = None;
        self.last_stats_key = 0;
        self.emu_started = Instant::now();
        self.current = Some(path.clone());
        self.screen = Screen::Emulating;
        self.dirty = true;
        let image = match read_image(&path) {
            Ok(i) => i,
            Err(e) => {
                self.emu_error = Some(e);
                return;
            }
        };
        let rotate: Option<zerotag_core::nfc::RotateUid> = match (self.uid_rotation, self.keys.clone()) {
            (true, Some(keys)) => Some(Box::new(move |data: &mut Image| {
                let mut copy = *data;
                if rotate_uid(&keys, &mut copy).is_err() {
                    return false;
                }
                *data = copy;
                true
            })),
            _ => None,
        };
        match self.hw.start_emulation(&image, rotate) {
            Ok(()) => {
                self.state.mark_emulated(&Self::base_name(&path));
                let _ = self.state.save(&self.paths.state); // a failed write only loses the recent order
                audio::play(Sound::Start);
            }
            Err(e) => {
                self.emu_error = Some(e);
                audio::play(Sound::Error);
            }
        }
    }

    fn stop_emulation(&mut self) {
        let Some(result) = self.hw.stop_emulation() else { return };
        if let Some(e) = result.error {
            self.emu_error = Some(e);
        } else if result.changed {
            if let Some(path) = self.current.clone() {
                match write_image(&path, &result.image) {
                    Ok(()) => self.say(format(self.lang, Str::BannerSaved, &[&Self::base_name(&path)]), false),
                    Err(e) => self.say(format(self.lang, Str::BannerSaveFailed, &[&e]), true),
                }
                self.rescan();
            }
        }
    }

    fn set_language(&mut self, lang: Lang) {
        if !self.fonts.serves(lang) {
            match Fonts::load(lang) {
                Ok(f) => self.fonts = f,
                Err(e) => {
                    self.say(e, true);
                    return;
                }
            }
        }
        self.lang = lang;
        self.persist("language", lang.code());
        self.say(format(lang, Str::BannerLanguage, &[&lang.native_name()]), false);
    }

    fn cycle_language(&mut self, direction: i32) {
        let mut next = self.lang;
        for _ in 0..if direction >= 0 { 1 } else { 3 } {
            next = next.next();
        }
        self.set_language(next);
    }

    fn toggle_sound(&mut self) {
        self.sound = !self.sound;
        audio::set_enabled(self.sound);
        self.persist("sound", if self.sound { "yes" } else { "no" });
        audio::play(Sound::Blip); // silent when it was just switched off
        self.dirty = true;
    }

    fn toggle_rotation(&mut self) {
        if !self.uid_rotation && self.keys.is_none() {
            self.say(self.t(Str::KeysMissingF1), true); // the rotation re-signs the image: it needs the keys
            return;
        }
        self.uid_rotation = !self.uid_rotation;
        self.persist("uid_rotation", if self.uid_rotation { "yes" } else { "no" });
        self.say(self.t(if self.uid_rotation { Str::BannerRotationOn } else { Str::BannerRotationOff }), false);
    }

    fn toggle_online(&mut self) {
        self.online = !self.online;
        self.persist("online", if self.online { "yes" } else { "no" });
        audio::play(Sound::Blip);
        self.dirty = true;
    }

    fn check_hardware(&mut self) {
        self.cap_ok = false;
        if !self.hw.spi_available() {
            self.cap_status = self.t(Str::ErrSpi).to_owned(); // the overlay is not loaded: nothing to open yet
            return;
        }
        match self.hw.probe() {
            Ok(rev) => {
                self.cap_ok = true;
                self.cap_status = format(self.lang, Str::SetupCapDetected, &[&rev]);
            }
            Err(e) => self.cap_status = e,
        }
    }

    fn open_cap_setup(&mut self, after: Screen) {
        self.cap_password.clear();
        self.cap_message = None;
        self.cap_busy = false;
        self.after_cap = after;
        self.go(Screen::CapSetup);
    }

    fn submit_cap_setup(&mut self) {
        if self.cap_busy {
            return;
        }
        self.cap_message = None;
        let result = self.hw.run_cap_setup(&mut self.cap_password); // wipes the password
        match result.status {
            CapStatus::Ok if self.hw.spi_available() => {
                self.check_hardware();
                self.say(self.t(Str::CapDone), false);
                self.go(self.after_cap);
                return;
            }
            CapStatus::Ok => self.cap_message = Some(format(self.lang, Str::CapFailed, &[&"no SPI node"])),
            CapStatus::PasswordRefused => self.cap_message = Some(self.t(Str::CapBadPassword).to_owned()),
            CapStatus::Failed => self.cap_message = Some(format(self.lang, Str::CapFailed, &[&result.message])),
        }
        self.dirty = true;
    }

    // ---- keys

    pub fn on_routed(&mut self, r: Routed) {
        match r {
            Routed::Down(k) => self.on_key(k, false),
            Routed::LongPress(k) => self.on_key(k, true),
            Routed::Up(k) => self.on_key_release(k),
        }
    }

    fn on_key(&mut self, key: Key, long: bool) {
        match key {
            Key::PrintScreen => {
                if !long {
                    self.want_screenshot = true;
                }
                return;
            }
            Key::Help => return, // the help screen is not ported yet
            Key::Esc => {
                if !long {
                    self.esc_down = true;
                    self.esc_handled = false;
                } else if self.esc_down && !self.esc_handled && self.screen == Screen::Library && self.lib_query.is_empty() {
                    self.esc_handled = true;
                    self.want_quit = true;
                }
                return;
            }
            _ if long => return,
            _ => {}
        }
        self.dirty = true;
        match self.screen {
            Screen::CapSetup => self.key_cap_setup(key),
            Screen::Library => self.key_library(key),
            Screen::Catalogue => self.key_catalogue(key),
            Screen::Detail => self.key_detail(key),
            Screen::Facets => self.key_facets(key),
            Screen::Settings => self.key_settings(key),
            Screen::Emulating => {} // ESC leaves (on release)
        }
    }

    fn key_cap_setup(&mut self, key: Key) {
        match key {
            Key::Enter => self.submit_cap_setup(),
            Key::Backspace => {
                self.cap_password.pop();
            }
            Key::Char(c) if (' '..='~').contains(&c) && self.cap_password.len() < 64 && !self.cap_busy => {
                self.cap_password.push(c)
            }
            _ => {}
        }
    }

    fn page_or_step(key: Key) -> Option<i32> {
        match key {
            Key::Up => Some(-1),
            Key::Down => Some(1),
            Key::PageUp => Some(-(ROWS as i32)),
            Key::PageDown => Some(ROWS as i32),
            _ => None,
        }
    }

    fn key_library(&mut self, key: Key) {
        let items = self.visible_items();
        if let Some(d) = Self::page_or_step(key) {
            move_selection(d, items.len(), &mut self.lib_sel, &mut self.lib_top, ROWS);
            return;
        }
        match key {
            Key::Enter => {
                if let Some(&i) = items.get(self.lib_sel) {
                    self.current = Some(self.library.items()[i].path.clone());
                    self.go(Screen::Detail);
                }
            }
            Key::Tab => self.go(Screen::Catalogue),
            Key::F1 => {
                self.before_settings = Screen::Library;
                self.check_hardware();
                self.go(Screen::Settings);
            }
            Key::F2 => {
                if let Some(&i) = items.get(self.lib_sel) {
                    let path = self.library.items()[i].path.clone();
                    self.toggle_favorite(i);
                    // The favourites move to the top: keep the cursor on the same backup.
                    let now = self.visible_items();
                    if let Some(p) = now.iter().position(|&j| self.library.items()[j].path == path) {
                        self.lib_sel = p;
                    }
                    move_selection(0, now.len(), &mut self.lib_sel, &mut self.lib_top, ROWS);
                }
            }
            Key::F3 => {
                self.lib_tab = match self.lib_tab {
                    Tab::All => Tab::Favorites,
                    Tab::Favorites => Tab::Recent,
                    Tab::Recent => Tab::All,
                };
                self.lib_sel = 0;
                self.lib_top = 0;
            }
            Key::F4 => self.say("Scan: pas encore porté en Rust", true),
            k => {
                if handle_text_key(k, &mut self.lib_query) {
                    self.lib_sel = 0;
                    self.lib_top = 0;
                }
            }
        }
    }

    fn key_catalogue(&mut self, key: Key) {
        let entries = self.visible_entries();
        if let Some(d) = Self::page_or_step(key) {
            move_selection(d, entries.len(), &mut self.cat_sel, &mut self.cat_top, ROWS);
            return;
        }
        match key {
            Key::Enter => {
                if let Some(&e) = entries.get(self.cat_sel) {
                    self.create_blank(e);
                }
            }
            Key::Tab => self.go(Screen::Library),
            Key::F1 => {
                self.before_settings = Screen::Catalogue;
                self.check_hardware();
                self.go(Screen::Settings);
            }
            Key::F2 => {
                self.facet_stage = 0;
                self.facet_sel = FACETS.iter().position(|&f| f == self.facet_browse).unwrap_or(0);
                self.facet_top = 0;
                self.go(Screen::Facets);
            }
            k => {
                if handle_text_key(k, &mut self.cat_query) {
                    self.cat_sel = 0;
                    self.cat_top = 0;
                }
            }
        }
    }

    fn key_detail(&mut self, key: Key) {
        let Some((path, head, tail, idx)) = self.current_item().map(|i| (i.path.clone(), i.head, i.tail, i.clone())) .map(|(p, h, t, i)| {
            let idx = self.library.items().iter().position(|x| x.path == i.path).unwrap_or(0);
            (p, h, t, idx)
        }) else {
            self.go(Screen::Library);
            return;
        };
        match key {
            Key::Enter => self.start_emulation(path),
            Key::Char('r' | 'R') => {
                if self.confirm == Confirm::Reset {
                    self.confirm = Confirm::None;
                    self.reset_item(path, head, tail);
                } else {
                    self.confirm = Confirm::Reset;
                    self.say(self.t(Str::BannerConfirmReset), false);
                }
            }
            Key::Char('u' | 'U') => self.toggle_rotation(),
            Key::F2 => self.toggle_favorite(idx),
            Key::Delete => {
                if self.confirm == Confirm::Delete {
                    self.confirm = Confirm::None;
                    self.delete_item(path);
                } else {
                    self.confirm = Confirm::Delete;
                    self.say(self.t(Str::BannerConfirmDelete), false);
                }
            }
            _ => {}
        }
    }

    fn facet_values(&self) -> Vec<(String, usize)> {
        self.db.facet_counts(self.facet_browse).into_iter().map(|f| (f.value, f.count)).collect()
    }

    fn key_facets(&mut self, key: Key) {
        let values = if self.facet_stage == 1 { self.facet_values() } else { Vec::new() };
        let count = if self.facet_stage == 0 { FACETS.len() } else { values.len() };
        if let Some(d) = Self::page_or_step(key) {
            move_selection(d, count, &mut self.facet_sel, &mut self.facet_top, ROWS);
            return;
        }
        if key == Key::Enter {
            if self.facet_stage == 0 {
                if let Some(&f) = FACETS.get(self.facet_sel) {
                    self.facet_browse = f;
                    self.facet_stage = 1;
                    self.facet_sel = 0;
                    self.facet_top = 0;
                }
            } else if let Some((value, _)) = values.get(self.facet_sel) {
                self.filter = Some((self.facet_browse, value.clone()));
                self.cat_sel = 0;
                self.cat_top = 0;
                self.go(Screen::Catalogue);
            }
        }
    }

    fn facets_back(&mut self) {
        if self.facet_stage == 1 {
            self.facet_stage = 0;
            self.facet_sel = FACETS.iter().position(|&f| f == self.facet_browse).unwrap_or(0);
            self.facet_top = 0;
            self.dirty = true;
        } else {
            self.go(Screen::Catalogue);
        }
    }

    fn key_settings(&mut self, key: Key) {
        match key {
            Key::Up => self.settings_sel = self.settings_sel.saturating_sub(1),
            Key::Down => self.settings_sel = (self.settings_sel + 1).min(SETTINGS_ROWS - 1),
            Key::Left if self.settings_sel <= 3 => match self.settings_sel {
                0 => self.cycle_language(-1),
                1 => self.toggle_rotation(),
                2 => self.toggle_sound(),
                _ => self.toggle_online(),
            },
            Key::Right | Key::Enter => match self.settings_sel {
                0 => self.cycle_language(1),
                1 => self.toggle_rotation(),
                2 => self.toggle_sound(),
                3 => self.toggle_online(),
                4 => {
                    if !self.hw.spi_available() {
                        self.open_cap_setup(Screen::Settings);
                        return;
                    }
                    self.check_hardware();
                }
                5 => {
                    self.reload_keys();
                    self.rescan();
                }
                _ => self.rescan(),
            },
            _ => return,
        }
        if self.settings_sel < self.settings_top {
            self.settings_top = self.settings_sel;
        }
        if self.settings_sel >= self.settings_top + 4 {
            self.settings_top = self.settings_sel - 3;
        }
    }

    fn on_key_release(&mut self, key: Key) {
        if key != Key::Esc {
            return;
        }
        let act = self.esc_down && !self.esc_handled;
        self.esc_down = false;
        if !act {
            return;
        }
        self.dirty = true;
        match self.screen {
            Screen::Library => {
                if !self.lib_query.is_empty() {
                    self.lib_query.clear();
                    self.lib_sel = 0;
                    self.lib_top = 0;
                }
            }
            Screen::Catalogue => {
                if !self.cat_query.is_empty() {
                    self.cat_query.clear();
                } else if self.filter.is_some() {
                    self.filter = None;
                } else {
                    self.go(Screen::Library);
                    return;
                }
                self.cat_sel = 0;
                self.cat_top = 0;
            }
            Screen::Facets => self.facets_back(),
            Screen::Detail => self.go(Screen::Library),
            Screen::Emulating => {
                self.stop_emulation();
                self.go(Screen::Detail);
            }
            Screen::Settings => self.go(self.before_settings),
            Screen::CapSetup => {
                if !self.cap_busy {
                    self.cap_password.clear();
                    self.go(self.after_cap);
                }
            }
        }
    }

    // ---- time

    /// Called regularly: banner expiry, emulation progress and autosave, battery.
    pub fn tick(&mut self, now: Instant) {
        if self.banner.as_ref().is_some_and(|b| now >= b.2) {
            self.banner = None;
            self.dirty = true;
        }
        if self.battery_read.is_none_or(|t| now.duration_since(t) > Duration::from_secs(10)) {
            self.battery_read = Some(now);
            let st = read_device_status();
            let b = (st.battery_present && st.battery_percent >= 0).then_some((st.battery_percent, st.battery_charging));
            if b != self.battery {
                self.battery = b;
                self.dirty = true;
            }
        }
        if self.screen != Screen::Emulating && !self.hw.emulation_running() {
            return;
        }
        if !self.hw.emulation_running() {
            // The worker ended on its own (setup failure or timeout).
            if let Some(result) = self.hw.stop_emulation() {
                if let Some(e) = result.error {
                    self.emu_error = Some(if e.is_empty() { self.t(Str::EmuInterrupted).into() } else { e });
                }
            }
            self.dirty = true;
            return;
        }
        let st = self.hw.stats();
        let key = ((st.reads as u64) << 8)
            ^ ((st.writes as u64) << 4)
            ^ if st.field { 2 } else { 0 }
            ^ if st.selected { 1 } else { 0 }
            ^ (now.duration_since(self.emu_started).as_secs() << 40)
            ^ ((st.rotations as u64) << 14);
        if key != self.last_stats_key {
            self.last_stats_key = key;
            self.dirty = true;
        }
        // Autosave once the reader is done with a burst of writes or after a UID rotation.
        if let Some(snapshot) = self.hw.poll_autosave(AUTOSAVE_QUIET) {
            if let Some(path) = self.current.clone() {
                match write_image(&path, &snapshot) {
                    Ok(()) => {
                        self.rescan();
                        self.say(self.t(Str::BannerAutosaved), false);
                    }
                    Err(e) => self.say(format(self.lang, Str::BannerSaveFailed, &[&e]), true),
                }
            }
            self.dirty = true;
        }
    }

    /// Starts on another screen (for `--screen`, to look at a screen without the keyboard).
    pub fn debug_screen(&mut self, name: Option<&str>) {
        let screen = match name {
            Some("library") => Screen::Library,
            Some("catalogue") => Screen::Catalogue,
            Some("settings") => {
                self.check_hardware();
                Screen::Settings
            }
            Some("detail") => {
                self.current = self.library.items().first().map(|i| i.path.clone());
                Screen::Detail
            }
            Some("capsetup") => Screen::CapSetup,
            _ => return,
        };
        self.go(screen);
    }

    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    pub fn screenshot_result(&mut self, result: Result<PathBuf, String>) {
        match result {
            Ok(p) => self.say(format(self.lang, Str::BannerCapture, &[&Self::base_name(&p)]), false),
            Err(_) => self.say(self.t(Str::BannerCaptureFailed), true),
        }
    }

    // ---- rendering

    fn banner_ref(&self) -> Option<(&str, bool)> {
        self.banner.as_ref().map(|(m, e, _)| (m.as_str(), *e))
    }

    fn is_upcoming(entry: &Entry) -> bool {
        !entry.release.is_empty() && entry.release.as_str() > chrono::Local::now().format("%Y-%m-%d").to_string().as_str()
    }

    fn signature(item: &Item) -> Signature {
        match (item.signature_checked, item.signature_ok) {
            (false, _) => Signature::Unchecked,
            (true, true) => Signature::Valid,
            (true, false) => Signature::Invalid,
        }
    }

    fn facet_label(&self, f: Facet) -> &'static str {
        self.t(match f {
            Facet::AmiiboSeries => Str::FacetSeries,
            Facet::Game => Str::FacetGame,
            Facet::Type => Str::FacetType,
        })
    }

    fn short_path(&self, p: &Path) -> String {
        let home = std::env::var("HOME").unwrap_or_default();
        match p.to_string_lossy().strip_prefix(&home) {
            Some(rest) if !home.is_empty() => format!("~{rest}"),
            _ => p.to_string_lossy().into_owned(),
        }
    }

    pub fn render(&self, canvas: &mut Canvas) {
        let mut p = Painter { canvas, fonts: &self.fonts, thumbs: Some(&self.thumbs) };
        let (lang, battery, banner) = (self.lang, self.battery, self.banner_ref());
        match self.screen {
            Screen::Library => {
                let all = self.library.items();
                let rows: Vec<LibraryRow> = self
                    .visible_items()
                    .into_iter()
                    .map(|i| {
                        let it = &all[i];
                        LibraryRow {
                            id: it.entry.as_ref().map_or_else(String::new, |e| e.id.clone()),
                            name: Self::item_name(it),
                            sub: it.entry.as_ref().map_or_else(|| it.uid.clone(), |e| e.amiibo_series.clone()),
                            favorite: self.is_favorite(it),
                            amiibo_series: it.entry.as_ref().map_or_else(String::new, |e| e.amiibo_series.clone()),
                            game_series: it.entry.as_ref().map_or_else(String::new, |e| e.game_series.clone()),
                            uid: it.uid.clone(),
                            signature: Self::signature(it),
                            known: it.entry.is_some(),
                        }
                    })
                    .collect();
                let top = self.lib_top.min(rows.len().saturating_sub(1));
                library::render(
                    &mut p,
                    &LibraryView {
                        lang,
                        rows: &rows,
                        total: all.len(),
                        selected: self.lib_sel,
                        top,
                        query: &self.lib_query,
                        tab: self.lib_tab,
                        keys_ok: self.keys.is_some(),
                        banner,
                        battery,
                    },
                );
            }
            Screen::Catalogue => {
                let owned: HashSet<&str> =
                    self.library.items().iter().filter_map(|i| i.entry.as_ref().map(|e| e.id.as_str())).collect();
                let rows: Vec<CatalogueRow> = self
                    .visible_entries()
                    .into_iter()
                    .map(|i| {
                        let e = &self.db.entries()[i];
                        CatalogueRow {
                            id: e.id.clone(),
                            name: e.name.clone(),
                            amiibo_series: e.amiibo_series.clone(),
                            owned: owned.contains(e.id.as_str()),
                            upcoming: Self::is_upcoming(e),
                            online: e.online,
                        }
                    })
                    .collect();
                let chip = self.filter.as_ref().map(|(f, v)| format(lang, Str::FacetChip, &[&self.facet_label(*f), v]));
                catalogue::render(
                    &mut p,
                    &CatalogueView {
                        lang,
                        rows: &rows,
                        total: self.db.entries().len(),
                        selected: self.cat_sel,
                        top: self.cat_top.min(rows.len().saturating_sub(1)),
                        query: &self.cat_query,
                        filter_chip: chip,
                        keys_ok: self.keys.is_some(),
                        banner,
                        battery,
                    },
                );
            }
            Screen::Facets => {
                let (rows, browsing): (Vec<FacetRow>, Option<&str>) = if self.facet_stage == 0 {
                    let rows = FACETS
                        .iter()
                        .map(|&f| FacetRow {
                            title: self.facet_label(f).into(),
                            right: format(lang, Str::FacetValuesCount, &[&self.db.facet_counts(f).len()]),
                        })
                        .collect();
                    (rows, None)
                } else {
                    let rows = self.facet_values().into_iter().map(|(v, n)| FacetRow { title: v, right: n.to_string() }).collect();
                    (rows, Some(self.facet_label(self.facet_browse)))
                };
                facets::render(
                    &mut p,
                    &FacetsView { lang, browsing, rows: &rows, selected: self.facet_sel, top: self.facet_top, banner, battery },
                );
            }
            Screen::Detail => match self.current_item() {
                None => detail::render(
                    &mut p,
                    &DetailView {
                        lang,
                        found: false,
                        entry_id: "",
                        name: "",
                        amiibo_series: "",
                        game_and_type: None,
                        favorite: false,
                        signature: Signature::Unchecked,
                        uid_rotation: false,
                        uid: "",
                        head: 0,
                        tail: 0,
                        file: "",
                        confirm: Confirm::None,
                        banner,
                        battery,
                    },
                ),
                Some(it) => {
                    let name = Self::item_name(it);
                    let file = Self::base_name(&it.path);
                    detail::render(
                        &mut p,
                        &DetailView {
                            lang,
                            found: true,
                            entry_id: it.entry.as_ref().map_or("", |e| e.id.as_str()),
                            name: &name,
                            amiibo_series: it.entry.as_ref().map_or("", |e| e.amiibo_series.as_str()),
                            game_and_type: it.entry.as_ref().map(|e| format!("{} - {}", e.game_series, e.type_)),
                            favorite: self.is_favorite(it),
                            signature: Self::signature(it),
                            uid_rotation: self.uid_rotation,
                            uid: &it.uid,
                            head: it.head,
                            tail: it.tail,
                            file: &file,
                            confirm: self.confirm,
                            banner,
                            battery,
                        },
                    );
                }
            },
            Screen::Emulating => {
                let st = self.hw.stats();
                let running = self.hw.emulation_running();
                let (name, id) = self
                    .current_item()
                    .map(|i| (Self::item_name(i), i.entry.as_ref().map_or_else(String::new, |e| e.id.clone())))
                    .unwrap_or_default();
                let uid = if running {
                    self.hw.current_image().map_or_else(String::new, |i| {
                        [i[0], i[1], i[2], i[4], i[5], i[6], i[7]].iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":")
                    })
                } else {
                    String::new()
                };
                emulation::render(
                    &mut p,
                    &EmulationView {
                        lang,
                        item_name: &name,
                        entry_id: &id,
                        running,
                        field: running && st.field,
                        selected: running && st.selected,
                        error: self.emu_error.as_deref(),
                        reads: st.reads,
                        writes: st.writes,
                        rotations: st.rotations,
                        seconds: Instant::now().duration_since(self.emu_started).as_secs() as u32,
                        uid: &uid,
                        uid_rotation: self.uid_rotation,
                        banner,
                        battery,
                    },
                );
            }
            Screen::Settings => {
                let rows = self.settings_rows();
                settings::render(
                    &mut p,
                    &SettingsView { lang, rows: &rows, selected: self.settings_sel, top: self.settings_top, banner, battery },
                );
            }
            Screen::CapSetup => cap_setup::render(
                &mut p,
                &CapSetupView {
                    lang,
                    password_len: self.cap_password.chars().count(),
                    busy: self.cap_busy,
                    message: self.cap_message.as_deref(),
                    battery,
                },
            ),
        }
    }

    fn settings_rows(&self) -> Vec<SettingsRow> {
        let t = |s: Str| self.t(s).to_owned();
        let keys_ok = self.keys.is_some();
        let keys_path = self.short_path(&self.paths.keys);
        let folder = self.short_path(&self.paths.library_dir);
        vec![
            SettingsRow { title: t(Str::SettingsLanguage), value: format!("< {} >", self.lang.native_name()), ..Default::default() },
            SettingsRow {
                title: t(Str::SettingsRotation),
                detail: t(Str::HelpRotation),
                value: toggle_value(self.lang, self.uid_rotation),
                ..Default::default()
            },
            SettingsRow { title: t(Str::SettingsSound), value: toggle_value(self.lang, self.sound), ..Default::default() },
            SettingsRow {
                title: t(Str::SettingsOnline),
                detail: t(Str::OnlineHint),
                value: toggle_value(self.lang, self.online),
                ..Default::default()
            },
            SettingsRow {
                title: t(Str::SetupCap),
                detail: if self.cap_status.is_empty() { t(Str::SetupNotChecked) } else { clip_chars(&self.cap_status, 70) },
                status: Some(if self.cap_ok { Status::Ok } else { Status::Error }),
                ..Default::default()
            },
            SettingsRow {
                title: t(if keys_ok { Str::SetupKeysOk } else { Str::NoteKeysTitle }),
                detail: if keys_ok { keys_path } else { format(self.lang, Str::SetupKeysHelp, &[&keys_path]) },
                status: Some(if keys_ok { Status::Ok } else { Status::Warning }),
                ..Default::default()
            },
            SettingsRow {
                title: t(Str::SetupFolder),
                detail: if self.library_ok {
                    format(self.lang, Str::SetupFolderInfo, &[&folder, &self.library.items().len(), &self.db.entries().len()])
                } else {
                    format(self.lang, Str::SetupFolderAbsent, &[&folder])
                },
                status: Some(if self.library_ok && self.db_error.is_none() { Status::Ok } else { Status::Warning }),
                ..Default::default()
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hw::LiveStats;
    use std::cell::RefCell;
    use std::rc::Rc;
    use zerotag_core::cap_setup::CapSetupResult;
    use zerotag_core::nfc::{RotateUid, SessionResult};

    #[derive(Default)]
    struct Shared {
        spi: bool,
        /// A session exists (it must be stopped to get the chip back).
        open: bool,
        /// Its worker is alive.
        running: bool,
        started: Vec<Image>,
        start_error: Option<String>,
        stop_result: Option<SessionResult>,
        autosave: Option<Image>,
        setup_calls: Vec<String>,
        setup_ok: bool,
        stats: LiveStats,
    }

    #[derive(Clone, Default)]
    struct Mock(Rc<RefCell<Shared>>);

    impl Hardware for Mock {
        fn spi_available(&self) -> bool {
            self.0.borrow().spi
        }
        fn probe(&mut self) -> Result<u8, String> {
            Ok(2)
        }
        fn start_emulation(&mut self, image: &Image, _rotate: Option<RotateUid>) -> Result<(), String> {
            let mut s = self.0.borrow_mut();
            if let Some(e) = s.start_error.clone() {
                return Err(e);
            }
            s.started.push(*image);
            s.open = true;
            s.running = true;
            Ok(())
        }
        fn emulation_running(&self) -> bool {
            self.0.borrow().running
        }
        fn stats(&self) -> LiveStats {
            self.0.borrow().stats
        }
        fn current_image(&self) -> Option<Image> {
            self.0.borrow().started.last().copied()
        }
        fn poll_autosave(&mut self, _q: Duration) -> Option<Image> {
            self.0.borrow_mut().autosave.take()
        }
        fn stop_emulation(&mut self) -> Option<SessionResult> {
            let mut s = self.0.borrow_mut();
            s.running = false;
            let was = std::mem::replace(&mut s.open, false);
            was.then(|| s.stop_result.take().unwrap_or(SessionResult { error: None, changed: false, image: [0; 540] }))
        }
        fn run_cap_setup(&mut self, password: &mut String) -> CapSetupResult {
            let mut s = self.0.borrow_mut();
            s.setup_calls.push(password.clone());
            password.clear();
            if s.setup_ok {
                s.spi = true;
                CapSetupResult { status: CapStatus::Ok, message: "ready".into() }
            } else {
                CapSetupResult { status: CapStatus::PasswordRefused, message: "Sorry".into() }
            }
        }
    }

    struct Env {
        _dir: tempfile::TempDir,
        mock: Mock,
        app: App<Mock>,
        lib: PathBuf,
    }

    fn fake_keys_file(path: &Path) {
        let mut b = [0u8; 160];
        for (i, x) in b.iter_mut().enumerate() {
            *x = (i as u8).wrapping_mul(37).wrapping_add(11);
        }
        for set in [0usize, 80] {
            b[set + 16..set + 30].copy_from_slice(b"unfixed infos\0");
            b[set + 30] = 0;
            b[set + 31] = 14;
        }
        std::fs::write(path, b).unwrap();
    }

    fn env(spi: bool, with_keys: bool) -> Option<Env> {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_path_buf();
        let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        let mut paths = Paths::for_home(&home, assets);
        paths.library_dir = home.join("amiibo");
        paths.keys = home.join(".zerotag/key_retail.bin");
        paths.state = home.join(".zerotag/library_state.txt");
        paths.config = home.join(".config/zerotag/zerotag.conf");
        std::fs::create_dir_all(home.join(".zerotag")).unwrap();
        if with_keys {
            fake_keys_file(&paths.keys);
        }
        let lib = paths.library_dir.clone();
        let mock = Mock::default();
        mock.0.borrow_mut().spi = spi;
        let config = ApplicationConfig { sound: false, ..Default::default() };
        let app = App::new(mock.clone(), paths, &config, Instant::now()).ok()?; // None: no Noto font on this machine
        Some(Env { _dir: dir, mock, app, lib })
    }

    fn press(app: &mut App<Mock>, key: Key) {
        app.on_routed(Routed::Down(key));
        app.on_routed(Routed::Up(key));
    }

    fn type_text(app: &mut App<Mock>, text: &str) {
        for c in text.chars() {
            press(app, Key::Char(c));
        }
    }

    #[test]
    fn selection_scrolls_and_clamps() {
        let (mut sel, mut top) = (0usize, 0usize);
        move_selection(1, 10, &mut sel, &mut top, 4);
        assert_eq!((sel, top), (1, 0));
        move_selection(4, 10, &mut sel, &mut top, 4);
        assert_eq!((sel, top), (5, 2));
        move_selection(100, 10, &mut sel, &mut top, 4);
        assert_eq!((sel, top), (9, 6));
        move_selection(-100, 10, &mut sel, &mut top, 4);
        assert_eq!((sel, top), (0, 0));
        move_selection(1, 0, &mut sel, &mut top, 4);
        assert_eq!((sel, top), (0, 0));
    }

    #[test]
    fn text_keys_fold_case_and_limit_length() {
        let mut q = String::new();
        assert!(handle_text_key(Key::Char('Z'), &mut q));
        assert!(!handle_text_key(Key::Char('é'), &mut q));
        assert!(!handle_text_key(Key::Enter, &mut q));
        for _ in 0..40 {
            handle_text_key(Key::Char('a'), &mut q);
        }
        assert_eq!(q.len(), 24);
        assert!(handle_text_key(Key::Backspace, &mut q));
        assert_eq!(q.len(), 23);
    }

    #[test]
    fn starts_on_the_cap_setup_screen_without_the_spi_device_and_a_wrong_password_is_refused_then_accepted() {
        let Some(mut e) = env(false, true) else { return };
        assert_eq!(e.app.screen, Screen::CapSetup);
        type_text(&mut e.app, "wrong");
        press(&mut e.app, Key::Enter);
        assert_eq!(e.mock.0.borrow().setup_calls, ["wrong"]);
        assert_eq!(e.app.screen, Screen::CapSetup);
        assert_eq!(e.app.cap_message.as_deref(), Some(tr(Lang::Fr, Str::CapBadPassword)));
        assert!(e.app.cap_password.is_empty()); // wiped

        e.mock.0.borrow_mut().setup_ok = true;
        type_text(&mut e.app, "secret");
        press(&mut e.app, Key::Enter);
        assert_eq!(e.app.screen, Screen::Library);
        assert!(e.app.cap_ok);
        assert!(e.app.banner.as_ref().is_some_and(|b| b.0 == tr(Lang::Fr, Str::CapDone)));
    }

    #[test]
    fn esc_skips_the_cap_setup_and_a_long_esc_quits_only_from_an_empty_library() {
        let Some(mut e) = env(false, true) else { return };
        press(&mut e.app, Key::Esc);
        assert_eq!(e.app.screen, Screen::Library);
        // typing a filter: long ESC must not quit
        type_text(&mut e.app, "zel");
        e.app.on_routed(Routed::Down(Key::Esc));
        e.app.on_routed(Routed::LongPress(Key::Esc));
        assert!(!e.app.want_quit);
        e.app.on_routed(Routed::Up(Key::Esc));
        // ... a short ESC clears the filter instead
        assert!(e.app.lib_query.is_empty());
        e.app.on_routed(Routed::Down(Key::Esc));
        e.app.on_routed(Routed::LongPress(Key::Esc));
        assert!(e.app.want_quit);
    }

    #[test]
    fn catalogue_search_create_blank_then_library_detail_favorite_and_delete() {
        let Some(mut e) = env(true, true) else { return };
        assert_eq!(e.app.screen, Screen::Library);
        press(&mut e.app, Key::Tab);
        assert_eq!(e.app.screen, Screen::Catalogue);
        type_text(&mut e.app, "bokoblin");
        assert_eq!(e.app.visible_entries().len(), 1);
        press(&mut e.app, Key::Enter); // creates a signed blank backup and goes to the library
        assert_eq!(e.app.screen, Screen::Library);
        assert_eq!(e.app.library.items().len(), 1);
        let item = e.app.library.items()[0].clone();
        assert!(item.signature_checked && item.signature_ok);
        assert!(item.path.starts_with(&e.lib) && Self_base(&item.path).starts_with("Bokoblin"));

        press(&mut e.app, Key::F2); // favourite
        assert!(e.app.state.is_favorite(&Self_base(&item.path)));
        assert!(std::fs::read_to_string(&e.app.paths.state).unwrap().contains("favorite=Bokoblin"));
        press(&mut e.app, Key::F3); // tabs: favourites -> recent -> all
        assert_eq!((e.app.lib_tab, e.app.visible_items().len()), (Tab::Favorites, 1));
        press(&mut e.app, Key::F3);
        assert_eq!((e.app.lib_tab, e.app.visible_items().len()), (Tab::Recent, 0));
        press(&mut e.app, Key::F3);

        press(&mut e.app, Key::Enter);
        assert_eq!(e.app.screen, Screen::Detail);
        press(&mut e.app, Key::Delete); // first press only asks for confirmation
        assert_eq!((e.app.screen, e.app.confirm), (Screen::Detail, Confirm::Delete));
        assert!(item.path.exists());
        press(&mut e.app, Key::Delete);
        assert_eq!(e.app.screen, Screen::Library);
        assert!(!item.path.exists());
        assert!(e.app.library.items().is_empty());
        assert!(!e.app.state.is_favorite(&Self_base(&item.path))); // forgotten
    }

    #[allow(non_snake_case)]
    fn Self_base(p: &Path) -> String {
        App::<Mock>::base_name(p)
    }

    #[test]
    fn creating_without_keys_is_refused_with_a_banner() {
        let Some(mut e) = env(true, false) else { return };
        press(&mut e.app, Key::Tab);
        type_text(&mut e.app, "link");
        press(&mut e.app, Key::Enter);
        assert_eq!(e.app.screen, Screen::Catalogue);
        assert!(e.app.banner.as_ref().is_some_and(|b| b.1 && b.0 == tr(Lang::Fr, Str::KeysMissingF1)));
        assert!(e.app.library.items().is_empty());
    }

    fn with_one_backup() -> Option<Env> {
        let mut e = env(true, true)?;
        press(&mut e.app, Key::Tab);
        type_text(&mut e.app, "bokoblin");
        press(&mut e.app, Key::Enter);
        press(&mut e.app, Key::Enter); // open the detail screen
        assert_eq!(e.app.screen, Screen::Detail);
        Some(e)
    }

    #[test]
    fn emulation_saves_what_the_reader_wrote_when_stopped() {
        let Some(mut e) = with_one_backup() else { return };
        let path = e.app.current.clone().unwrap();
        let original = std::fs::read(&path).unwrap();
        press(&mut e.app, Key::Enter);
        assert_eq!(e.app.screen, Screen::Emulating);
        assert_eq!(e.mock.0.borrow().started.len(), 1);
        assert_eq!(e.mock.0.borrow().started[0][..], original[..]);
        assert!(e.app.state.recent.contains(&Self_base(&path)));

        let mut changed = [0u8; 540];
        changed.copy_from_slice(&original);
        changed[0x100] ^= 0xFF;
        e.mock.0.borrow_mut().stop_result = Some(SessionResult { error: None, changed: true, image: changed });
        press(&mut e.app, Key::Esc);
        assert_eq!(e.app.screen, Screen::Detail);
        assert_eq!(std::fs::read(&path).unwrap(), changed);
        assert!(!e.mock.0.borrow().running);
        assert!(e.app.banner.as_ref().is_some_and(|b| b.0.contains(&Self_base(&path))));
    }

    #[test]
    fn emulation_autosave_and_failure_to_start() {
        let Some(mut e) = with_one_backup() else { return };
        let path = e.app.current.clone().unwrap();
        press(&mut e.app, Key::Enter);
        let mut snap = [7u8; 540];
        snap[..8].copy_from_slice(&std::fs::read(&path).unwrap()[..8]);
        e.mock.0.borrow_mut().autosave = Some(snap);
        e.app.tick(Instant::now());
        assert_eq!(std::fs::read(&path).unwrap(), snap);
        assert!(e.app.banner.as_ref().is_some_and(|b| b.0 == tr(Lang::Fr, Str::BannerAutosaved)));
        press(&mut e.app, Key::Esc);

        e.mock.0.borrow_mut().start_error = Some("cannot open /dev/spidev0.2: No such file or directory".into());
        press(&mut e.app, Key::Enter);
        assert_eq!(e.app.screen, Screen::Emulating);
        assert!(e.app.emu_error.as_deref().is_some_and(|m| m.contains("spidev")));
    }

    #[test]
    fn worker_ending_by_itself_shows_the_error_and_releases_the_chip() {
        let Some(mut e) = with_one_backup() else { return };
        press(&mut e.app, Key::Enter);
        {
            let mut s = e.mock.0.borrow_mut();
            s.running = false; // the worker died on its own (setup failure)
            s.stop_result =
                Some(SessionResult { error: Some("ST25R3916 init failed at: oscillator".into()), changed: false, image: [0; 540] });
        }
        e.app.tick(Instant::now());
        assert_eq!(e.app.emu_error.as_deref(), Some("ST25R3916 init failed at: oscillator"));
        assert!(!e.mock.0.borrow().open); // the session was joined and the chip dropped
        assert!(e.app.take_dirty());
    }

    #[test]
    fn settings_rows_toggle_and_persist_and_language_cycles() {
        let Some(mut e) = env(true, true) else { return };
        press(&mut e.app, Key::F1);
        assert_eq!(e.app.screen, Screen::Settings);
        assert!(e.app.cap_ok && e.app.cap_status.contains('2')); // probe: revision 2
        press(&mut e.app, Key::Enter); // language: fr -> en
        assert_eq!(e.app.lang, Lang::En);
        press(&mut e.app, Key::Down);
        press(&mut e.app, Key::Enter); // rotation on (keys are present)
        assert!(e.app.uid_rotation);
        press(&mut e.app, Key::Down);
        press(&mut e.app, Key::Left); // sound toggles with Left too (it was off in this test: now on)
        let conf = std::fs::read_to_string(&e.app.paths.config).unwrap();
        assert!(conf.contains("language=en") && conf.contains("uid_rotation=yes") && conf.contains("sound=yes"), "{conf}");
        for _ in 0..3 {
            press(&mut e.app, Key::Up);
        }
        press(&mut e.app, Key::Left); // language backwards: en -> fr
        assert_eq!(e.app.lang, Lang::Fr);
        press(&mut e.app, Key::Esc);
        assert_eq!(e.app.screen, Screen::Library);
        let mut c = ApplicationConfig::default();
        c.load_into(&e.app.paths.config).unwrap();
        assert_eq!((c.language.as_str(), c.uid_rotation), ("fr", true));
    }

    #[test]
    fn rotation_needs_the_keys() {
        let Some(mut e) = env(true, false) else { return };
        press(&mut e.app, Key::F1);
        press(&mut e.app, Key::Down);
        press(&mut e.app, Key::Enter);
        assert!(!e.app.uid_rotation);
        assert!(e.app.banner.as_ref().is_some_and(|b| b.1));
    }

    #[test]
    fn every_screen_renders_without_panicking_in_every_language() {
        let Some(mut e) = with_one_backup() else { return };
        let mut canvas = Canvas::new(320, 170, 0);
        for lang in [Lang::Fr, Lang::En, Lang::Ja, Lang::Zh] {
            e.app.set_language(lang);
            for screen in [Screen::Library, Screen::Catalogue, Screen::Facets, Screen::Detail, Screen::Emulating, Screen::Settings, Screen::CapSetup] {
                e.app.screen = screen;
                e.app.render(&mut canvas);
            }
        }
    }

    #[test]
    fn facet_filter_flow() {
        let Some(mut e) = env(true, true) else { return };
        press(&mut e.app, Key::Tab);
        let all = e.app.visible_entries().len();
        press(&mut e.app, Key::F2);
        assert_eq!(e.app.screen, Screen::Facets);
        press(&mut e.app, Key::Down);
        press(&mut e.app, Key::Down); // Type
        press(&mut e.app, Key::Enter);
        assert_eq!(e.app.facet_stage, 1);
        press(&mut e.app, Key::Enter); // first value
        assert_eq!(e.app.screen, Screen::Catalogue);
        assert!(e.app.filter.is_some() && e.app.visible_entries().len() < all);
        press(&mut e.app, Key::Esc); // clears the filter first
        assert!(e.app.filter.is_none());
        assert_eq!(e.app.visible_entries().len(), all);
        press(&mut e.app, Key::Esc);
        assert_eq!(e.app.screen, Screen::Library);
    }
}
