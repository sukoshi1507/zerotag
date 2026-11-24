// SPDX-License-Identifier: MIT
// Hardware test: emulates an NTAG215 with the ST25R3916.
//   st25r_emulate --blank [seconds]             synthetic blank NTAG215 (random UID, NDEF "hello"), no keys
//   st25r_emulate <image.bin> [seconds]         540-byte NTAG215 image (e.g. an amiibo image)
//   st25r_emulate --save <image.bin> [seconds]  same, and writes the image back if the reader changed it
// Prints the live counters.
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::Arc;
use std::time::Duration;
use zerotag_core::nfc::{open_cap, CapConfig, EmulationOptions, EmulationStats};

fn blank() -> [u8; 540] {
    let mut im = [0u8; 540];
    let mut uid = [0x04u8; 7];
    getrandom::getrandom(&mut uid[1..]).expect("random");
    im[..3].copy_from_slice(&uid[..3]);
    im[3] = 0x88 ^ uid[0] ^ uid[1] ^ uid[2];
    im[4..8].copy_from_slice(&uid[3..]);
    im[8] = uid[3] ^ uid[4] ^ uid[5] ^ uid[6];
    im[9] = 0x48;
    im[12..16].copy_from_slice(&[0xE1, 0x10, 0x3E, 0x00]); // NTAG215 capability container
    // NDEF TLV with one text record "hello" (en), so a phone has something to display.
    let ndef = [0x03, 0x0C, 0xD1, 0x01, 0x08, 0x54, 0x02, b'e', b'n', b'h', b'e', b'l', b'l', b'o', 0xFE];
    im[16..16 + ndef.len()].copy_from_slice(&ndef);
    im[536] = 0x80;
    im[537] = 0x80; // PACK
    im
}

fn main() {
    let mut a: Vec<String> = std::env::args().skip(1).collect();
    let save_back = a.first().is_some_and(|s| s == "--save");
    if save_back {
        a.remove(0);
    }
    let Some(source) = a.first().cloned() else {
        eprintln!("usage: st25r_emulate [--save] --blank|<image.bin> [seconds]");
        std::process::exit(1)
    };
    let seconds: u64 = a.get(1).and_then(|s| s.parse().ok()).unwrap_or(30);
    let is_blank = source == "--blank";
    if is_blank && save_back {
        eprintln!("--save needs an image file");
        std::process::exit(1);
    }
    let mut image: [u8; 540] = if is_blank {
        blank()
    } else {
        std::fs::read(&source).ok().and_then(|d| d.try_into().ok()).unwrap_or_else(|| {
            eprintln!("cannot read a 540-byte image from {source}");
            std::process::exit(1)
        })
    };
    let original = image;

    let mut chip = open_cap(&CapConfig::default()).unwrap_or_else(|e| {
        eprintln!("open failed: {e}");
        std::process::exit(2)
    });
    println!("emulating for {seconds} s");
    let stats = Arc::new(EmulationStats::default());
    let done = Arc::new(AtomicBool::new(false));
    let printer = {
        let (stats, done) = (stats.clone(), done.clone());
        std::thread::spawn(move || {
            let mut last = (u32::MAX, u32::MAX, false, false);
            while !done.load(Relaxed) {
                let now = (stats.reads.load(Relaxed), stats.writes.load(Relaxed), stats.field.load(Relaxed), stats.selected.load(Relaxed));
                if now != last {
                    println!("field={} selected={} reads={} writes={}", now.2 as u8, now.3 as u8, now.0, now.1);
                    last = now;
                }
                std::thread::sleep(Duration::from_millis(200));
            }
        })
    };
    let options = EmulationOptions { stats: Some(stats), ..Default::default() };
    let result = chip.emulate_ntag215(&mut image, seconds, options);
    done.store(true, Relaxed);
    printer.join().ok();
    if let Err(e) = result {
        eprintln!("emulation failed: {e}");
        std::process::exit(4);
    }
    let changed = image != original;
    println!("done, tag {}", if changed { "was written by the reader" } else { "unchanged" });
    if save_back && changed {
        // Same write-back rule as the library: temporary file, then rename.
        let tmp = format!("{source}.tmp");
        let ok = std::fs::write(&tmp, image).and_then(|_| std::fs::rename(&tmp, &source)).is_ok();
        println!("{}", if ok { "image saved back" } else { "could not save the image" });
    }
}
