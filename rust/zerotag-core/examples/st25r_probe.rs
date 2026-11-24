// SPDX-License-Identifier: MIT
// Hardware probe: powers the Cap, reads the ST25R3916 identity, optionally polls for an NFC-A tag.
// Needs the SPI overlay (sudo /usr/lib/zerotag/cap-setup) and the spi + gpio groups.
//   st25r_probe              identity only
//   st25r_probe <seconds>    also poll for a tag for that long
//   st25r_probe <seconds> <file>   also write the 540-byte image of the tag to <file>
use std::time::{Duration, Instant};
use zerotag_core::nfc::{open_cap, CapConfig};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut chip = open_cap(&CapConfig::default()).unwrap_or_else(|e| {
        eprintln!("open failed: {e}");
        std::process::exit(1)
    });
    println!("ST25R3916 identity=0x{:02X} revision={}", chip.ic_identity(), chip.revision());
    let seconds: u64 = a.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    if seconds == 0 {
        return;
    }
    if let Err(e) = chip.init_nfc_a() {
        eprintln!("init failed: {e}");
        std::process::exit(2);
    }
    println!("NFC-A reader ready, polling for {seconds} s...");
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut last_error = String::new();
    while Instant::now() < deadline {
        match chip.detect_tag() {
            Ok(tag) => {
                let uid: Vec<String> = tag.uid.iter().map(|b| format!("{b:02X}")).collect();
                println!("TAG: ATQA=0x{:04X} SAK=0x{:02X} UID({})={}", tag.atqa, tag.sak, tag.uid.len(), uid.join(":"));
                match chip.type2_get_version() {
                    Some(v) => println!("GET_VERSION: {}", v.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")),
                    None => println!("GET_VERSION: no answer (not an NTAG?)"),
                }
                for first in [0u8, 3] {
                    match chip.type2_read(first) {
                        Some(p) => println!(
                            "READ pages {first}-{}: {}",
                            first + 3,
                            p.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")
                        ),
                        None => println!("READ page {first} failed"),
                    }
                }
                if let Some(path) = a.get(2) {
                    let Some(image) = chip.dump_ntag215() else {
                        eprintln!("dump: READ failed");
                        std::process::exit(4)
                    };
                    if std::fs::write(path, image).is_err() {
                        eprintln!("dump: cannot write {path}");
                        std::process::exit(5);
                    }
                    println!("dumped {} bytes to {path}", image.len());
                }
                return;
            }
            Err(e) => {
                if e != "no tag" && e != last_error {
                    println!("  ({e})");
                    last_error = e;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    println!("no tag detected");
    std::process::exit(3);
}
