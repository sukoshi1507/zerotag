// SPDX-License-Identifier: MIT
// Draws a few Han characters at 36 px with the JP face and the SC face, for a visual check: face_check <out.png>
use zerotag_ui::{Canvas, Fonts, Lang};

fn main() {
    let out = std::env::args().nth(1).expect("out.png");
    let mut c = Canvas::new(320, 130, 0x101014);
    for (i, lang) in [Lang::Ja, Lang::Zh].into_iter().enumerate() {
        let f = Fonts::load(lang).expect("Noto CJK");
        f.draw(&mut c, "创建资料库选择筛选", 4, 4 + i as i32 * 60, 30, false, 0xf1ebd8);
    }
    zerotag_core::screenshot::write_png(std::path::Path::new(&out), &c.to_rgba(), 320, 130).expect("png");
}
