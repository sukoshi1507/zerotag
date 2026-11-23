// SPDX-License-Identifier: MIT
// usage: amiibo_rotate <key_retail.bin> <image.bin>   (rewrites the image in place with a new random UID)
use zerotag_core::amiibo::{rotate_uid, Keys, NTAG215_SIZE};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 3 {
        eprintln!("usage: {} <key_retail.bin> <image.bin>", a[0]);
        std::process::exit(2);
    }
    let keys = Keys::load(&a[1]).expect("keys");
    let mut img: [u8; NTAG215_SIZE] = std::fs::read(&a[2]).expect("read").try_into().expect("540 bytes");
    rotate_uid(&keys, &mut img).expect("rotate");
    std::fs::write(&a[2], img).expect("write");
}
