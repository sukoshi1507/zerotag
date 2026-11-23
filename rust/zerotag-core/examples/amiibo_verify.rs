// SPDX-License-Identifier: MIT
// usage: amiibo_verify <key_retail.bin> <tag_image.bin>   (prints only the verdict and the id)
use zerotag_core::amiibo::{verify, Keys, NTAG215_SIZE};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 3 {
        eprintln!("usage: {} <key_retail.bin> <tag_image.bin>", a[0]);
        std::process::exit(2);
    }
    let keys = Keys::load(&a[1]).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1)
    });
    let data = std::fs::read(&a[2]).unwrap_or_default();
    let img: [u8; NTAG215_SIZE] = data.try_into().unwrap_or_else(|_| {
        eprintln!("need {NTAG215_SIZE} bytes in {}", a[2]);
        std::process::exit(1)
    });
    match verify(&keys, &img) {
        Some((h, t)) => println!("signatures: valid\nid={h:08X}{t:08X}"),
        None => {
            println!("signatures: INVALID (wrong keys, or not an amiibo image)");
            std::process::exit(3)
        }
    }
}
