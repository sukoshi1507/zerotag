// SPDX-License-Identifier: MIT
// Times verify / rotate_uid / generate on an image read from a file (nothing is written, nothing is printed but times).
// usage: bench_rotate <key_retail.bin> <image.bin>
use std::time::Instant;
use zerotag_core::amiibo::{generate, rotate_uid, verify, Keys};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let keys = Keys::load(&a[1]).expect("keys");
    let img: [u8; 540] = std::fs::read(&a[2]).expect("image").try_into().expect("540 bytes");
    for round in 0..3 {
        let t = Instant::now();
        let ok = verify(&keys, &img).is_some();
        let v = t.elapsed();
        let mut copy = img;
        let t = Instant::now();
        let r = rotate_uid(&keys, &mut copy).is_ok();
        let rot = t.elapsed();
        let t = Instant::now();
        let g = generate(&keys, 0x01010000, 0x000e0002).is_ok();
        println!("round {round}: verify {v:?} (ok={ok}), rotate_uid {rot:?} (ok={r}), generate {:?} (ok={g})", t.elapsed());
    }
}
