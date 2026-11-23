// SPDX-License-Identifier: MIT
// usage: amiibo_gen <key_retail.bin> <16-hex-char id> <out.bin>
// Writes a blank signed 540-byte amiibo image. Treat it like a tag dump: do not leave it lying around.
use zerotag_core::amiibo::{generate, parse_id, Keys};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 4 {
        eprintln!("usage: {} <key_retail.bin> <id> <out.bin>", a[0]);
        std::process::exit(2);
    }
    let (head, tail) = parse_id(&a[2]).unwrap_or_else(|| {
        eprintln!("bad id");
        std::process::exit(2)
    });
    let keys = Keys::load(&a[1]).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1)
    });
    let img = generate(&keys, head, tail).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1)
    });
    std::fs::write(&a[3], img).expect("write");
}
