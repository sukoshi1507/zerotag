// SPDX-License-Identifier: MIT
// Renders the library and the detail screens with sample data: render_screens <lang> <out_dir> <amiibo_db.json>
// (the thumbnails are read from <amiibo_db dir>/images when present)
use zerotag_core::amiibo::Database;
use zerotag_ui::screens::detail::{self, Confirm, DetailView};
use zerotag_ui::screens::emulation::{self, EmulationView};
use zerotag_ui::screens::facets::{self, FacetRow, FacetsView};
use zerotag_ui::screens::settings::{self, toggle_value, SettingsRow, SettingsView, Status};
use zerotag_ui::screens::scan::{self, ScanIssueView, ScanPhaseView, ScanView};
use zerotag_ui::screens::library::{self, LibraryRow, LibraryView, Signature, Tab};
use zerotag_ui::thumbs::Thumbnails;
use zerotag_ui::widgets::Painter;
use zerotag_ui::{Canvas, Fonts, Lang, SCREEN_H, SCREEN_W};

fn save(c: &Canvas, path: &str) {
    zerotag_core::screenshot::write_png(std::path::Path::new(path), &c.to_rgba(), SCREEN_W as u32, SCREEN_H as u32)
        .expect("png");
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let lang = Lang::from_code(&a[1], Lang::Fr);
    let db = Database::load(&a[3]).expect("catalogue");
    let fonts = Fonts::load(lang).expect("Noto CJK");
    let thumbs = Thumbnails::new(vec![std::path::Path::new(&a[3]).parent().unwrap().join("images")]);
    let pick = |name: &str, series: &str| {
        let by_name = db.entries().iter().filter(|e| e.name.starts_with(name));
        by_name.clone().find(|e| e.amiibo_series == series).or_else(|| by_name.clone().next()).unwrap_or_else(|| panic!("{name}"))
    };
    let samples = [
        (pick("Link", "Super Smash Bros."), true, "04:E1:7B:16:71:2F:06", Signature::Valid),
        (pick("Zelda", "Super Smash Bros."), false, "04:4E:38:79:78:30:12", Signature::Valid),
        (pick("Bokoblin", "Legend Of Zelda"), false, "04:CE:58:EE:2E:1D:0B", Signature::Unchecked),
        (pick("Ganondorf", "Legend Of Zelda"), false, "04:11:22:33:44:55:66", Signature::Invalid),
        (pick("Daruk", "Legend Of Zelda"), false, "04:AA:BB:CC:DD:EE:FF", Signature::Valid),
    ];
    let rows: Vec<LibraryRow> = samples
        .iter()
        .map(|(e, fav, uid, sig)| LibraryRow {
            id: e.id.clone(),
            name: e.name.clone(),
            sub: e.amiibo_series.clone(),
            favorite: *fav,
            amiibo_series: e.amiibo_series.clone(),
            game_series: e.game_series.clone(),
            uid: (*uid).into(),
            signature: *sig,
            known: true,
        })
        .collect();
    let mut canvas = Canvas::new(SCREEN_W, SCREEN_H, 0);
    let view = LibraryView {
        lang,
        rows: &rows,
        total: rows.len(),
        selected: 0,
        top: 0,
        query: "",
        tab: Tab::All,
        keys_ok: true,
        banner: None,
        battery: Some((83, false)),
    };
    library::render(&mut Painter { canvas: &mut canvas, fonts: &fonts, thumbs: Some(&thumbs) }, &view);
    save(&canvas, &format!("{}/library_{}.png", a[2], a[1]));

    let link = samples[0].0;
    let view = DetailView {
        lang,
        found: true,
        entry_id: &link.id,
        name: &link.name,
        amiibo_series: &link.amiibo_series,
        game_and_type: Some(format!("{} - {}", link.game_series, link.type_)),
        favorite: true,
        signature: Signature::Valid,
        uid_rotation: true,
        uid: "04:E1:7B:16:71:2F:06",
        head: link.head,
        tail: link.tail,
        file: "Link_167F2F06.bin",
        confirm: Confirm::Reset,
        banner: None,
        battery: Some((83, true)),
    };
    detail::render(&mut Painter { canvas: &mut canvas, fonts: &fonts, thumbs: Some(&thumbs) }, &view);
    save(&canvas, &format!("{}/detail_{}.png", a[2], a[1]));

    let view = EmulationView {
        lang,
        item_name: &link.name,
        entry_id: &link.id,
        running: true,
        field: true,
        selected: true,
        error: None,
        reads: 143,
        writes: 2,
        rotations: 0,
        seconds: 75,
        uid: "04:E1:7B:16:71:2F:06",
        uid_rotation: false,
        banner: None,
        battery: Some((83, false)),
    };
    emulation::render(&mut Painter { canvas: &mut canvas, fonts: &fonts, thumbs: Some(&thumbs) }, &view);
    save(&canvas, &format!("{}/emulation_{}.png", a[2], a[1]));

    let rows = vec![
        SettingsRow { title: "Langue".into(), value: format!("< {} >", lang.native_name()), ..Default::default() },
        SettingsRow { title: "Rotation d'UID".into(), detail: "Nouvel UID après chaque lecture".into(), value: toggle_value(lang, false), ..Default::default() },
        SettingsRow { title: "Cap CC1101 – NFC".into(), detail: "ST25R3916 détecté, révision 2".into(), status: Some(Status::Ok), ..Default::default() },
        SettingsRow { title: "Clés trouvées".into(), detail: "~/.zerotag/key_retail.bin".into(), status: Some(Status::Ok), ..Default::default() },
        SettingsRow { title: "Dossier des amiibo".into(), detail: "~/amiibo – 6 sauvegardes – 954 personnages".into(), status: Some(Status::Warning), ..Default::default() },
    ];
    let view = SettingsView { lang, rows: &rows, selected: 1, top: 0, banner: None, battery: Some((83, false)) };
    settings::render(&mut Painter { canvas: &mut canvas, fonts: &fonts, thumbs: None }, &view);
    save(&canvas, &format!("{}/settings_{}.png", a[2], a[1]));

    let rows = vec![
        FacetRow { title: "Série amiibo".into(), right: "77 valeurs".into() },
        FacetRow { title: "Jeu".into(), right: "41 valeurs".into() },
        FacetRow { title: "Type".into(), right: "5 valeurs".into() },
    ];
    let view = FacetsView { lang, browsing: None, rows: &rows, selected: 0, top: 0, banner: None, battery: Some((83, false)) };
    facets::render(&mut Painter { canvas: &mut canvas, fonts: &fonts, thumbs: None }, &view);
    save(&canvas, &format!("{}/facets_{}.png", a[2], a[1]));

    let phases = [
        ("waiting", ScanPhaseView::Waiting { issue: Some(ScanIssueView::Interrupted { page: 52 }) }),
        ("reading", ScanPhaseView::Reading { progress: 70 }),
        (
            "saved",
            ScanPhaseView::Saved {
                name: Some(link.name.clone()),
                entry_id: link.id.clone(),
                file: "Link_167F2F06.bin".into(),
                uid: "04:E1:7B:16:71:2F:06".into(),
            },
        ),
    ];
    for (name, phase) in &phases {
        let view = ScanView { lang, phase, banner: None, battery: Some((83, false)) };
        scan::render(&mut Painter { canvas: &mut canvas, fonts: &fonts, thumbs: Some(&thumbs) }, &view);
        save(&canvas, &format!("{}/scan_{name}_{}.png", a[2], a[1]));
    }
}
