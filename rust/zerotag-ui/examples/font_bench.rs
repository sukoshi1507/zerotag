// SPDX-License-Identifier: MIT
// Measures the font loading and a first draw (on the device, to size the start-up time).
use std::time::Instant;
use zerotag_ui::{Canvas, Fonts, Lang};

fn main() {
    for lang in [Lang::Fr, Lang::Zh] {
        let t = Instant::now();
        let f = Fonts::load(lang).expect("Noto CJK");
        let load = t.elapsed();
        let t = Instant::now();
        let mut c = Canvas::new(320, 170, 0);
        f.draw(&mut c, "Bibliothèque ライブラリ 资料库 The Legend of Zelda", 4, 4, 11, false, 0xFFFFFF);
        println!("{lang:?}: load {load:?}, first draw {:?}", t.elapsed());
    }
}
