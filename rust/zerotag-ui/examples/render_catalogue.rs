// SPDX-License-Identifier: MIT
// Renders the catalogue screen to a PNG with the bundled catalogue: render_catalogue <db.json> <query> <lang> <out.png>
use zerotag_core::amiibo::Database;
use zerotag_ui::screens::catalogue::{render, CatalogueRow, CatalogueView};
use zerotag_ui::widgets::Painter;
use zerotag_ui::{Canvas, Fonts, Lang, SCREEN_H, SCREEN_W};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let db = Database::load(&a[1]).expect("catalogue");
    let query = a[2].trim_end_matches('_');
    let lang = Lang::from_code(&a[3], Lang::Fr);
    let hits = db.search(query);
    let rows: Vec<CatalogueRow> = hits
        .iter()
        .map(|e| CatalogueRow {
            name: e.name.clone(),
            amiibo_series: e.amiibo_series.clone(),
            owned: e.name == "Bokoblin",
            ..Default::default()
        })
        .collect();
    let selected = rows.iter().position(|r| r.name == "Bokoblin").unwrap_or(0);
    let top = selected.saturating_sub(1);
    let fonts = Fonts::load(lang).expect("Noto CJK");
    let mut canvas = Canvas::new(SCREEN_W, SCREEN_H, 0);
    let view = CatalogueView {
        lang,
        rows: &rows,
        total: db.entries().len(),
        selected,
        top,
        query,
        filter_chip: None,
        keys_ok: true,
        banner: None,
        battery: Some((83, false)),
    };
    render(&mut Painter { canvas: &mut canvas, fonts: &fonts }, &view);
    zerotag_core::screenshot::write_png(std::path::Path::new(&a[4]), &canvas.to_rgba(), SCREEN_W as u32, SCREEN_H as u32)
        .expect("png");
}
