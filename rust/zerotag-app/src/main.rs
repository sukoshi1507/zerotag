// SPDX-License-Identifier: MIT
//! ZeroTag for the CardputerZero (Rust port).
//!
//!   zerotag [--assets DIR] [--fb auto] [--png OUT.png] [--screen library|catalogue|settings]
//!
//! `--png` renders the first frame to a file and exits (no display, no keyboard): used to check the screens.

mod app;
mod fb;
mod hw;

use app::{App, Paths, Screen};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use zerotag_core::config::ApplicationConfig;
use zerotag_core::input::{KeyRouter, Keypad};
use zerotag_ui::{Canvas, SCREEN_H, SCREEN_W};

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn find_assets(args: &[String]) -> PathBuf {
    if let Some(a) = arg_value(args, "--assets") {
        return PathBuf::from(a);
    }
    if let Some(a) = std::env::var_os("ZEROTAG_ASSETS") {
        return PathBuf::from(a);
    }
    for c in ["/usr/share/zerotag", "assets"] {
        if std::path::Path::new(c).join("amiibo/amiibo_db.json").exists() {
            return PathBuf::from(c);
        }
    }
    PathBuf::from("/usr/share/zerotag")
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| "/tmp".into()));
    let paths = Paths::for_home(&home, find_assets(&args));

    // The user's settings, else the packaged defaults (/etc/zerotag.conf).
    let mut config = ApplicationConfig::default();
    for file in [PathBuf::from("/etc/zerotag.conf"), paths.config.clone()] {
        let _ = config.load_into(&file);
    }

    let now = Instant::now();
    let mut app = match App::new(hw::RealHardware::default(), paths, &config, now) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("zerotag: {e}");
            std::process::exit(1);
        }
    };
    app.debug_screen(arg_value(&args, "--screen").as_deref());
    let emulate = arg_value(&args, "--emulate");
    let max_seconds: u64 = arg_value(&args, "--seconds").and_then(|s| s.parse().ok()).unwrap_or(0);

    let mut canvas = Canvas::new(SCREEN_W, SCREEN_H, 0);
    if let Some(out) = arg_value(&args, "--png") {
        app.render(&mut canvas);
        let r = zerotag_core::screenshot::write_png(std::path::Path::new(&out), &canvas.to_rgba(), SCREEN_W as u32, SCREEN_H as u32);
        if let Err(e) = r {
            eprintln!("zerotag: {e}");
            std::process::exit(1);
        }
        return;
    }

    let framebuffer = match fb::Framebuffer::open(SCREEN_W as usize, SCREEN_H as usize) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("zerotag: {e}");
            std::process::exit(1);
        }
    };
    let mut keypads = Keypad::discover();
    if keypads.is_empty() {
        eprintln!("zerotag: no keyboard found in /dev/input (the screen still works)");
    }
    let debug = std::env::var_os("ZEROTAG_DEBUG").is_some();
    if debug {
        eprintln!("zerotag: {} keyboard device(s)", keypads.len());
    }
    let mut router = KeyRouter::default();
    if let Some(f) = &emulate {
        app.debug_emulate(std::path::Path::new(f));
    }
    let started = Instant::now();
    let mut last_stats = None;
    let scan_test = arg_value(&args, "--screen").as_deref() == Some("scan");
    let mut last_scan_line = String::new();
    while !app.want_quit && (max_seconds == 0 || started.elapsed().as_secs() < max_seconds) {
        let now = Instant::now();
        for kp in &mut keypads {
            for ev in kp.poll() {
                for r in router.on_event(ev, now) {
                    if debug {
                        eprintln!("zerotag: {r:?}");
                    }
                    app.on_routed(r);
                }
            }
        }
        if let Some(r) = router.tick(now) {
            app.on_routed(r);
        }
        app.tick(now);
        if scan_test {
            let line = app.scan_status_line();
            if line != last_scan_line {
                eprintln!("scan: {line}");
                last_scan_line = line;
            }
        }
        if emulate.is_some() {
            let st = app.emulation_stats();
            if last_stats != Some(st) {
                eprintln!("field={} selected={} reads={} writes={}", st.field as u8, st.selected as u8, st.reads, st.writes);
                last_stats = Some(st);
            }
        }
        if app.take_dirty() {
            if !app.can_flush(now) {
                app.mark_dirty(); // a reader is talking to the tag: draw once it has gone
            } else {
                app.render(&mut canvas);
                if let Err(e) = framebuffer.present(&canvas) {
                    eprintln!("zerotag: framebuffer: {e}");
                }
                if std::mem::take(&mut app.want_screenshot) {
                    let shot = zerotag_core::screenshot::save(&canvas.to_rgba(), SCREEN_W as u32, SCREEN_H as u32);
                    app.screenshot_result(shot);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = Screen::Library; // the quit screen is the launcher's: nothing to draw
}
