/*
 * SPDX-License-Identifier: MIT
 */

#include "zerotag_ui.h"

#include "device_status.h"
#include "linux_input.h"
#include "logger.h"
#include "screenshot_service.h"

#include <algorithm>
#include <cstdio>
#include <cstdlib>
#include <filesystem>
#include <fstream>

#include <pwd.h>
#include <unistd.h>

namespace fs = std::filesystem;

namespace app {
namespace {

constexpr int kScreenW = 320;
constexpr int kScreenH = 170;
constexpr int kBarH = 20;
constexpr int kFootH = 20;
constexpr int kListW = 188;
constexpr int kRowH = 26;
constexpr int kListTop = 44; // below the search field
constexpr int kRows = 4;
constexpr uint32_t kBannerMs = 4000;

std::string lower_hex(uint32_t v) {
    char b[16];
    std::snprintf(b, sizeof(b), "%08x", v);
    return b;
}

bool printable(uint32_t key) {
    return (key >= 'a' && key <= 'z') || (key >= '0' && key <= '9') || key == ' ' || key == '-' || key == '.' ||
           key == '\'';
}

std::string clip(const std::string& s, size_t n) {
    return s.size() <= n ? s : s.substr(0, n - 1) + "...";
}

} // namespace

// ---------------------------------------------------------------------------
// Construction and data
// ---------------------------------------------------------------------------

ZeroTagUi::ZeroTagUi(AssetManager& assets, bool dark_mode, std::function<void()> request_quit)
    : assets_(assets), dark_(dark_mode), request_quit_(std::move(request_quit)) {
    pal_ = view::palette(dark_);
    dim_ = lv_color_mix(pal_.text, pal_.background, 150);
}

ZeroTagUi::~ZeroTagUi() {
    if (timer_) {
        lv_timer_delete(timer_);
    }
    if (session_) {
        session_->request_stop();
        session_->finish();
    }
    session_.reset();
    chip_.reset();
}

std::string ZeroTagUi::home_dir() {
    if (const char* home = std::getenv("HOME"); home && home[0] != '\0') {
        return home;
    }
    if (const passwd* user = getpwuid(getuid()); user && user->pw_dir && user->pw_dir[0] != '\0') {
        return user->pw_dir;
    }
    return "/tmp";
}

std::string ZeroTagUi::base_name(const std::string& path) {
    return fs::path(path).filename().string();
}

std::string ZeroTagUi::item_name(const amiibo::Item& item) {
    if (item.entry) {
        return item.entry->name;
    }
    return "Inconnu " + lower_hex(item.head) + lower_hex(item.tail);
}

void ZeroTagUi::load_data() {
    const char* env_dir = std::getenv("ZEROTAG_AMIIBO_DIR");
    const char* env_keys = std::getenv("ZEROTAG_KEYS");
    library_dir_ = (env_dir && env_dir[0]) ? env_dir : home_dir() + "/amiibo";
    keys_path_ = (env_keys && env_keys[0]) ? env_keys : home_dir() + "/.zerotag/key_retail.bin";

    db_path_ = assets_.resolve("amiibo/amiibo_db.json").string();
    images_dir_ = assets_.resolve("amiibo/images").string();
    db_ok_ = db_.load(db_path_, db_error_);
    if (!db_ok_) {
        LOG_WARN("catalogue: {}", db_error_);
    }

    keys_ = std::make_unique<amiibo::Keys>();
    keys_ok_ = keys_->load(keys_path_, keys_error_);
    if (!keys_ok_) {
        LOG_WARN("keys: {}", keys_error_);
    }
    rescan_library();
}

void ZeroTagUi::rescan_library() {
    library_error_.clear();
    library_ok_ = library_.scan(library_dir_, db_, keys_ok_ ? keys_.get() : nullptr, library_error_);
    if (!library_ok_) {
        LOG_WARN("library: {}", library_error_);
    }
}

std::vector<const amiibo::Item*> ZeroTagUi::visible_items() const {
    if (!lib_query_.empty()) {
        return library_.search(lib_query_);
    }
    std::vector<const amiibo::Item*> all;
    for (const auto& item : library_.items()) {
        all.push_back(&item);
    }
    return all;
}

std::vector<const amiibo::Entry*> ZeroTagUi::visible_entries() const {
    return db_.search(cat_query_);
}

void ZeroTagUi::start() {
    load_data();
    root_ = lv_obj_create(lv_screen_active());
    lv_obj_remove_style_all(root_);
    lv_obj_set_size(root_, kScreenW, kScreenH);
    lv_obj_set_pos(root_, 0, 0);
    lv_obj_remove_flag(root_, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_set_style_bg_opa(root_, LV_OPA_COVER, 0);
    lv_obj_set_style_bg_color(root_, pal_.background, 0);
    timer_ = lv_timer_create(tick_cb, 250, this);
    render();
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

void ZeroTagUi::go(Screen screen) {
    screen_ = screen;
    pending_confirm_ = 0;
    render();
}

void ZeroTagUi::say(const std::string& value, bool error) {
    banner_ = value;
    banner_error_ = error;
    banner_until_ = lv_tick_get() + kBannerMs;
}

void ZeroTagUi::create_blank(const amiibo::Entry& entry) {
    if (!keys_ok_) {
        say("Cles introuvables : voir F1", true);
        return;
    }
    std::string path, error;
    if (!library_.create(*keys_, entry, path, error)) {
        say("Echec : " + error, true);
        return;
    }
    rescan_library();
    lib_query_.clear();
    lib_sel_ = 0;
    lib_top_ = 0;
    const auto items = visible_items();
    for (size_t i = 0; i < items.size(); ++i) {
        if (items[i]->path == path) {
            lib_sel_ = static_cast<int>(i);
            lib_top_ = std::max(0, lib_sel_ - kRows + 1);
        }
    }
    say("Cree : " + base_name(path));
    go(Screen::Library);
}

void ZeroTagUi::delete_item(const amiibo::Item& item) {
    const std::string path = item.path;
    {
        // Overwrite before removing: a backup is a tag dump.
        std::ofstream f(path, std::ios::binary | std::ios::trunc);
        const std::string zeros(amiibo::kNtag215Size, '\0');
        f.write(zeros.data(), static_cast<std::streamsize>(zeros.size()));
    }
    std::error_code ec;
    fs::remove(path, ec);
    rescan_library();
    lib_sel_ = std::max(0, std::min(lib_sel_, static_cast<int>(visible_items().size()) - 1));
    say(ec ? "Suppression impossible" : "Supprime : " + base_name(path), static_cast<bool>(ec));
    go(Screen::Library);
}

void ZeroTagUi::reset_item(const amiibo::Item& item) {
    if (!keys_ok_) {
        say("Cles introuvables : voir F1", true);
        return;
    }
    amiibo::TagImage image;
    std::string error;
    if (!amiibo::generate(*keys_, item.head, item.tail, image, error) ||
        !amiibo::Library::write_image(item.path, image, error)) {
        say("Echec : " + error, true);
        return;
    }
    rescan_library();
    say("Reinitialise avec un nouvel UID");
    render();
}

void ZeroTagUi::start_emulation(const amiibo::Item& item) {
    emu_error_.clear();
    last_stats_key_ = 0;
    emu_started_ = lv_tick_get();
    current_path_ = item.path;
    screen_ = Screen::Emulating;

    amiibo::TagImage image;
    std::string error;
    if (!amiibo::Library::read_image(item.path, image, error)) {
        emu_error_ = error;
        render();
        return;
    }
    chip_ = std::make_unique<platform::St25r3916>();
    if (!chip_->open(error)) {
        emu_error_ = error;
        chip_.reset();
        render();
        return;
    }
    session_ = std::make_unique<platform::EmulationSession>();
    if (!session_->start(*chip_, image, 0, error)) {
        emu_error_ = error;
        session_.reset();
        chip_.reset();
    }
    render();
}

void ZeroTagUi::stop_emulation() {
    if (session_) {
        session_->request_stop();
        const auto result = session_->finish();
        if (!result.ok && !result.error.empty()) {
            emu_error_ = result.error;
        } else if (result.changed) {
            std::string error;
            amiibo::TagImage image = result.image;
            if (amiibo::Library::write_image(current_path_, image, error)) {
                say("Ecritures enregistrees dans " + base_name(current_path_));
            } else {
                say("Enregistrement impossible : " + error, true);
            }
            rescan_library();
        }
        session_.reset();
    }
    chip_.reset(); // powers the Cap off
}

void ZeroTagUi::check_hardware() {
    cap_ok_ = false;
    cap_status_.clear();
    std::string error;
    platform::St25r3916 chip;
    if (chip.open(error)) {
        cap_ok_ = true;
        char b[96];
        std::snprintf(b, sizeof(b), "ST25R3916 detecte, revision %d", chip.revision());
        cap_status_ = b;
    } else {
        cap_status_ = error;
    }
}

void ZeroTagUi::tick() {
    const uint32_t now = lv_tick_get();
    bool dirty = false;

    if (banner_until_ != 0 && lv_tick_elaps(banner_until_) < 0x80000000u && now >= banner_until_) {
        banner_until_ = 0;
        banner_.clear();
        dirty = true;
    }

    if (screen_ == Screen::Emulating && session_) {
        if (!session_->running()) {
            // The worker ended on its own (setup failure or timeout).
            const auto result = session_->finish();
            if (!result.ok) {
                emu_error_ = result.error.empty() ? "emulation interrompue" : result.error;
            }
            session_.reset();
            chip_.reset();
            dirty = true;
        } else {
            const auto& st = session_->stats();
            const uint32_t key = (st.reads.load() << 8) ^ (st.writes.load() << 4) ^ (st.field.load() ? 2u : 0u) ^
                                 (st.selected.load() ? 1u : 0u) ^ ((lv_tick_elaps(emu_started_) / 1000u) << 20);
            if (key != last_stats_key_) {
                last_stats_key_ = key;
                dirty = true;
            }
        }
    }
    if (dirty) {
        render();
    }
}

void ZeroTagUi::tick_cb(lv_timer_t* timer) {
    auto* self = static_cast<ZeroTagUi*>(lv_timer_get_user_data(timer));
    if (self) {
        self->tick();
    }
}

// ---------------------------------------------------------------------------
// Keys
// ---------------------------------------------------------------------------

bool ZeroTagUi::handle_text_key(uint32_t key, std::string& query) {
    if (key == LV_KEY_BACKSPACE) {
        if (!query.empty()) {
            query.pop_back();
        }
        return true;
    }
    if (printable(key) && query.size() < 24) {
        query.push_back(static_cast<char>(key));
        return true;
    }
    return false;
}

void ZeroTagUi::move_selection(int delta, size_t count, int& selected, int& top, int rows) {
    if (count == 0) {
        selected = 0;
        top = 0;
        return;
    }
    selected = std::max(0, std::min(selected + delta, static_cast<int>(count) - 1));
    if (selected < top) {
        top = selected;
    }
    if (selected >= top + rows) {
        top = selected - rows + 1;
    }
}

bool ZeroTagUi::on_key(uint32_t key, bool long_pressed) {
    if (key == platform::kKeyPrintScreen) {
        if (!long_pressed) {
            const auto result = platform::screenshot::capture_active_screen();
            say(result.success ? "Capture : " + base_name(result.path) : "Capture impossible", !result.success);
            render();
        }
        return true;
    }
    if (key == platform::kKeyHelp) {
        return true;
    }
    if (key == LV_KEY_ESC) {
        if (!long_pressed) {
            esc_down_ = true;
            esc_handled_ = false;
        } else if (esc_down_ && !esc_handled_ && screen_ == Screen::Library && lib_query_.empty()) {
            esc_handled_ = true;
            request_quit_();
        }
        return true;
    }
    if (long_pressed) {
        return true;
    }

    const int page = kRows;
    switch (screen_) {
    case Screen::Library: {
        const auto items = visible_items();
        if (key == LV_KEY_UP) {
            move_selection(-1, items.size(), lib_sel_, lib_top_, kRows);
        } else if (key == LV_KEY_DOWN) {
            move_selection(1, items.size(), lib_sel_, lib_top_, kRows);
        } else if (key == platform::kKeyPageUp) {
            move_selection(-page, items.size(), lib_sel_, lib_top_, kRows);
        } else if (key == platform::kKeyPageDown) {
            move_selection(page, items.size(), lib_sel_, lib_top_, kRows);
        } else if (key == LV_KEY_ENTER) {
            if (lib_sel_ < static_cast<int>(items.size())) {
                current_path_ = items[static_cast<size_t>(lib_sel_)]->path;
                go(Screen::Detail);
                return true;
            }
        } else if (key == '\t') {
            go(Screen::Catalogue);
            return true;
        } else if (key == platform::kKeyF1) {
            screen_before_setup_ = Screen::Library;
            check_hardware();
            go(Screen::Setup);
            return true;
        } else if (handle_text_key(key, lib_query_)) {
            lib_sel_ = 0;
            lib_top_ = 0;
        } else {
            return false;
        }
        render();
        return true;
    }
    case Screen::Catalogue: {
        const auto entries = visible_entries();
        if (key == LV_KEY_UP) {
            move_selection(-1, entries.size(), cat_sel_, cat_top_, kRows);
        } else if (key == LV_KEY_DOWN) {
            move_selection(1, entries.size(), cat_sel_, cat_top_, kRows);
        } else if (key == platform::kKeyPageUp) {
            move_selection(-page, entries.size(), cat_sel_, cat_top_, kRows);
        } else if (key == platform::kKeyPageDown) {
            move_selection(page, entries.size(), cat_sel_, cat_top_, kRows);
        } else if (key == LV_KEY_ENTER) {
            if (cat_sel_ < static_cast<int>(entries.size())) {
                create_blank(*entries[static_cast<size_t>(cat_sel_)]);
            }
            return true;
        } else if (key == '\t') {
            go(Screen::Library);
            return true;
        } else if (key == platform::kKeyF1) {
            screen_before_setup_ = Screen::Catalogue;
            check_hardware();
            go(Screen::Setup);
            return true;
        } else if (handle_text_key(key, cat_query_)) {
            cat_sel_ = 0;
            cat_top_ = 0;
        } else {
            return false;
        }
        render();
        return true;
    }
    case Screen::Detail: {
        const amiibo::Item* item = nullptr;
        for (const auto& it : library_.items()) {
            if (it.path == current_path_) {
                item = &it;
            }
        }
        if (!item) {
            go(Screen::Library);
            return true;
        }
        if (key == LV_KEY_ENTER) {
            start_emulation(*item);
        } else if (key == 'r') {
            if (pending_confirm_ == 1) {
                pending_confirm_ = 0;
                reset_item(*item);
            } else {
                pending_confirm_ = 1;
                say("Appuie encore sur R : efface la sauvegarde de jeu");
                render();
            }
        } else if (key == LV_KEY_DEL) {
            if (pending_confirm_ == 2) {
                pending_confirm_ = 0;
                delete_item(*item);
            } else {
                pending_confirm_ = 2;
                say("Appuie encore sur Suppr : supprime le fichier");
                render();
            }
        } else {
            return false;
        }
        return true;
    }
    case Screen::Emulating:
        return true;
    case Screen::Setup:
        if (key == 'r') {
            keys_ok_ = keys_->load(keys_path_, keys_error_);
            rescan_library();
            check_hardware();
            render();
            return true;
        }
        return false;
    }
    return false;
}

void ZeroTagUi::on_key_release(uint32_t key) {
    if (key != LV_KEY_ESC) {
        return;
    }
    const bool act = esc_down_ && !esc_handled_;
    esc_down_ = false;
    if (!act) {
        return;
    }
    switch (screen_) {
    case Screen::Library:
        if (!lib_query_.empty()) {
            lib_query_.clear();
            lib_sel_ = 0;
            lib_top_ = 0;
            render();
        }
        break;
    case Screen::Catalogue:
        if (!cat_query_.empty()) {
            cat_query_.clear();
            cat_sel_ = 0;
            cat_top_ = 0;
            render();
        } else {
            go(Screen::Library);
        }
        break;
    case Screen::Detail:
        go(Screen::Library);
        break;
    case Screen::Emulating:
        stop_emulation();
        go(Screen::Detail);
        break;
    case Screen::Setup:
        go(screen_before_setup_);
        break;
    }
}

// ---------------------------------------------------------------------------
// Widgets
// ---------------------------------------------------------------------------

const lv_font_t* ZeroTagUi::font(int px, bool bold) {
    const int key = px * 2 + (bold ? 1 : 0);
    const auto it = fonts_.find(key);
    if (it != fonts_.end()) {
        return it->second;
    }
    const lv_font_t* f = assets_.load_standard_font(static_cast<uint32_t>(px),
                                                    bold ? StandardFontWeight::Bold : StandardFontWeight::Regular);
    if (!f) {
        f = LV_FONT_DEFAULT;
    }
    fonts_[key] = f;
    return f;
}

lv_obj_t* ZeroTagUi::box(lv_obj_t* parent, int x, int y, int w, int h, lv_color_t fill, int border_w,
                         lv_color_t border, int radius) {
    lv_obj_t* o = lv_obj_create(parent);
    lv_obj_remove_style_all(o);
    lv_obj_remove_flag(o, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_remove_flag(o, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_set_pos(o, x, y);
    lv_obj_set_size(o, w, h);
    lv_obj_set_style_bg_opa(o, LV_OPA_COVER, 0);
    lv_obj_set_style_bg_color(o, fill, 0);
    lv_obj_set_style_border_width(o, border_w, 0);
    lv_obj_set_style_border_color(o, border, 0);
    lv_obj_set_style_radius(o, radius, 0);
    return o;
}

lv_obj_t* ZeroTagUi::text(lv_obj_t* parent, const std::string& value, const lv_font_t* f, lv_color_t color, int x,
                          int y, int w, bool wrap) {
    lv_obj_t* l = lv_label_create(parent);
    lv_label_set_text(l, value.c_str());
    lv_obj_set_style_text_font(l, f, 0);
    lv_obj_set_style_text_color(l, color, 0);
    lv_obj_set_pos(l, x, y);
    if (w > 0) {
        lv_obj_set_width(l, w);
        if (wrap) {
            lv_label_set_long_mode(l, LV_LABEL_LONG_MODE_WRAP);
        } else {
            // One line: the height is fixed so that the long text is cut with "..." instead of wrapping.
            lv_obj_set_height(l, lv_font_get_line_height(f));
            lv_label_set_long_mode(l, LV_LABEL_LONG_MODE_DOTS);
        }
    }
    return l;
}

lv_obj_t* ZeroTagUi::thumb(lv_obj_t* parent, const amiibo::Entry* entry, int x, int y, int h) {
    const int w = std::max(8, h * 51 / 88);
    lv_obj_t* holder = box(parent, x, y, w, h, pal_.surface, 0, pal_.border, 2);
    if (!entry) {
        return holder;
    }
    const std::string path = amiibo::Database::image_path(images_dir_, *entry);
    std::error_code ec;
    if (!fs::exists(path, ec)) {
        return holder;
    }
    lv_obj_t* img = lv_image_create(holder);
    lv_image_set_src(img, ("A:" + path).c_str());
    lv_image_set_pivot(img, 0, 0);
    lv_image_set_scale(img, static_cast<uint32_t>(256 * h / 88));
    lv_obj_set_pos(img, 0, 0);
    return holder;
}

void ZeroTagUi::search_field(lv_obj_t* parent, const std::string& query, const std::string& placeholder,
                             bool focused) {
    lv_obj_t* f = box(parent, 6, 23, kListW - 12, 18, pal_.button, 1, focused ? pal_.primary : pal_.border, 4);
    if (query.empty()) {
        text(f, placeholder, font(10), dim_, 6, 2, kListW - 26);
    } else {
        text(f, query + "_", font(10), pal_.text, 6, 2, kListW - 26);
    }
}

void ZeroTagUi::render_bar(const std::string& title, const std::string& subtitle) {
    box(root_, 0, 0, kScreenW, kBarH, pal_.surface, 0);
    box(root_, 0, kBarH - 1, kScreenW, 1, pal_.border, 0);
    lv_obj_t* t = text(root_, title, font(12, true), pal_.primary, 6, 2);
    lv_obj_update_layout(t);
    text(root_, subtitle, font(10), dim_, 6 + lv_obj_get_width(t) + 6, 4, 150);

    const auto status = platform::read_device_status();
    if (status.battery_present && status.battery_percent >= 0) {
        char b[16];
        std::snprintf(b, sizeof(b), "%d%%%s", status.battery_percent, status.battery_charging ? "+" : "");
        lv_obj_t* l = text(root_, b, font(10), pal_.text, 0, 4);
        lv_obj_align(l, LV_ALIGN_TOP_RIGHT, -6, 4);
    }
}

void ZeroTagUi::render_footer(const std::vector<std::pair<std::string, std::string>>& hints) {
    const int y = kScreenH - kFootH;
    box(root_, 0, y, kScreenW, kFootH, pal_.bar, 0);
    box(root_, 0, y, kScreenW, 1, pal_.border, 0);
    if (!banner_.empty()) {
        text(root_, banner_, font(10), banner_error_ ? pal_.error : pal_.primary, 6, y + 4, kScreenW - 12);
        return;
    }
    int x = 6;
    for (const auto& hint : hints) {
        lv_obj_t* key = text(root_, hint.first, font(9), pal_.text, 0, 0);
        lv_obj_update_layout(key);
        const int kw = lv_obj_get_width(key) + 8;
        lv_obj_delete(key);
        lv_obj_t* cap = box(root_, x, y + 4, kw, 13, pal_.surface, 1, pal_.border, 3);
        lv_obj_t* kl = text(cap, hint.first, font(9), pal_.text, 0, 0);
        lv_obj_align(kl, LV_ALIGN_CENTER, 0, 0);
        lv_obj_t* label = text(root_, hint.second, font(9), dim_, x + kw + 3, y + 4);
        lv_obj_update_layout(label);
        x += kw + 3 + lv_obj_get_width(label) + 9;
    }
}

// ---------------------------------------------------------------------------
// Screens
// ---------------------------------------------------------------------------

void ZeroTagUi::render() {
    if (!root_) {
        return;
    }
    lv_obj_clean(root_);
    switch (screen_) {
    case Screen::Library:
        render_library();
        break;
    case Screen::Catalogue:
        render_catalogue();
        break;
    case Screen::Detail:
        render_detail();
        break;
    case Screen::Emulating:
        render_emulating();
        break;
    case Screen::Setup:
        render_setup();
        break;
    }
}

void ZeroTagUi::render_library() {
    const auto items = visible_items();
    render_bar("ZeroTag", "Bibliotheque - " + std::to_string(library_.items().size()));
    search_field(root_, lib_query_, "Taper pour filtrer...", !lib_query_.empty());
    box(root_, kListW, kBarH, 1, kScreenH - kBarH - kFootH, pal_.border, 0);

    if (items.empty()) {
        std::string line1 = library_.items().empty() ? "Aucun amiibo dans la bibliotheque." : "Aucun resultat.";
        std::string line2 = !keys_ok_ ? "Cles introuvables : F1" : "Tab : catalogue, Entree cree un amiibo.";
        text(root_, line1, font(10), pal_.text, 8, 62, kListW - 16);
        text(root_, line2, font(9), dim_, 8, 80, kListW - 16);
    }

    const int first = std::min(lib_top_, std::max(0, static_cast<int>(items.size()) - 1));
    for (int row = 0; row < kRows; ++row) {
        const int idx = first + row;
        if (idx >= static_cast<int>(items.size())) {
            break;
        }
        const amiibo::Item& item = *items[static_cast<size_t>(idx)];
        const bool sel = idx == lib_sel_;
        const int y = kListTop + row * kRowH;
        if (sel) {
            box(root_, 1, y, kListW - 1, kRowH - 2, lv_color_mix(pal_.primary, pal_.background, 45), 0, pal_.border, 3);
        }
        thumb(root_, item.entry, 6, y + 2, kRowH - 6);
        text(root_, item_name(item), font(11, sel), sel ? pal_.primary : pal_.text, 28, y + 2, kListW - 36);
        text(root_, item.entry ? item.entry->amiibo_series : item.uid, font(9), dim_, 28, y + 14, kListW - 36);
    }
    if (items.size() > static_cast<size_t>(kRows)) {
        const int track = kRows * kRowH;
        const int bar_h = std::max(6, track * kRows / static_cast<int>(items.size()));
        const int bar_y = kListTop + (track - bar_h) * first / std::max(1, static_cast<int>(items.size()) - kRows);
        box(root_, kListW - 4, bar_y, 2, bar_h, pal_.border, 0, pal_.border, 1);
    }

    // preview
    if (lib_sel_ < static_cast<int>(items.size())) {
        const amiibo::Item& item = *items[static_cast<size_t>(lib_sel_)];
        thumb(root_, item.entry, kListW + 8, 28, 70);
        const int tx = kListW + 8 + 70 * 51 / 88 + 8;
        const int tw = kScreenW - tx - 6;
        text(root_, item_name(item), font(12, true), pal_.text, tx, 26, tw);
        if (item.entry) {
            text(root_, item.entry->amiibo_series, font(9), dim_, tx, 44, tw);
            text(root_, item.entry->game_series, font(9), dim_, tx, 57, tw);
            text(root_, item.entry->type, font(9), dim_, tx, 70, tw);
        }
        text(root_, item.uid, font(9), pal_.text, kListW + 8, 104, kScreenW - kListW - 12);
        const char* sig = !item.signature_checked ? "Signature non verifiee"
                                                  : (item.signature_ok ? "Signature valide" : "Signature invalide");
        text(root_, sig, font(9), !item.signature_checked ? dim_ : (item.signature_ok ? pal_.primary : pal_.error),
             kListW + 8, 118, kScreenW - kListW - 12);
    }
    render_footer({{"Haut/Bas", "Choix"}, {"Entree", "Ouvrir"}, {"Tab", "Catalogue"}, {"F1", "Etat"}});
}

void ZeroTagUi::render_catalogue() {
    const auto entries = visible_entries();
    render_bar("Catalogue", std::to_string(entries.size()) + " / " + std::to_string(db_.entries().size()));
    search_field(root_, cat_query_, "Rechercher un personnage...", true);
    box(root_, kListW, kBarH, 1, kScreenH - kBarH - kFootH, pal_.border, 0);

    if (!db_ok_) {
        text(root_, "Catalogue illisible :", font(10), pal_.error, 8, 62, kListW - 16);
        text(root_, clip(db_error_, 60), font(9), dim_, 8, 78, kListW - 16);
    } else if (entries.empty()) {
        text(root_, "Aucun resultat.", font(10), pal_.text, 8, 62, kListW - 16);
    }

    auto owned = [&](const amiibo::Entry* e) {
        for (const auto& it : library_.items()) {
            if (it.entry == e) {
                return true;
            }
        }
        return false;
    };

    const int first = std::min(cat_top_, std::max(0, static_cast<int>(entries.size()) - 1));
    for (int row = 0; row < kRows; ++row) {
        const int idx = first + row;
        if (idx >= static_cast<int>(entries.size())) {
            break;
        }
        const amiibo::Entry* e = entries[static_cast<size_t>(idx)];
        const bool sel = idx == cat_sel_;
        const int y = kListTop + row * kRowH;
        if (sel) {
            box(root_, 1, y, kListW - 1, kRowH - 2, lv_color_mix(pal_.primary, pal_.background, 45), 0, pal_.border, 3);
        }
        thumb(root_, e, 6, y + 2, kRowH - 6);
        const int w = owned(e) ? kListW - 82 : kListW - 36;
        text(root_, e->name, font(11, sel), sel ? pal_.primary : pal_.text, 28, y + 2, w);
        text(root_, e->amiibo_series, font(9), dim_, 28, y + 14, kListW - 36);
        if (owned(e)) {
            text(root_, "possede", font(9), pal_.primary, kListW - 48, y + 4);
        }
    }
    if (entries.size() > static_cast<size_t>(kRows)) {
        const int track = kRows * kRowH;
        const int bar_h = std::max(6, track * kRows / static_cast<int>(entries.size()));
        const int bar_y = kListTop + (track - bar_h) * first / std::max(1, static_cast<int>(entries.size()) - kRows);
        box(root_, kListW - 4, bar_y, 2, bar_h, pal_.border, 0, pal_.border, 1);
    }

    if (cat_sel_ < static_cast<int>(entries.size())) {
        const amiibo::Entry* e = entries[static_cast<size_t>(cat_sel_)];
        thumb(root_, e, kListW + 8, 28, 70);
        const int tx = kListW + 8 + 70 * 51 / 88 + 8;
        const int tw = kScreenW - tx - 6;
        text(root_, e->name, font(12, true), pal_.text, tx, 26, tw);
        text(root_, e->amiibo_series, font(9), dim_, tx, 44, tw);
        text(root_, e->game_series, font(9), dim_, tx, 57, tw);
        text(root_, e->type, font(9), dim_, tx, 70, tw);
        lv_obj_t* note = box(root_, kListW + 6, 104, kScreenW - kListW - 12, 44, pal_.button, 1, pal_.border, 4);
        if (!keys_ok_) {
            text(note, "Cles introuvables", font(9, true), pal_.warning, 6, 4, kScreenW - kListW - 24);
            text(note, "Creation impossible : voir F1.", font(9), dim_, 6, 18, kScreenW - kListW - 24, true);
        } else if (owned(e)) {
            text(note, "Deja possede", font(9, true), pal_.primary, 6, 4, kScreenW - kListW - 24);
            text(note, "Entree en cree un autre exemplaire.", font(9), dim_, 6, 18, kScreenW - kListW - 24, true);
        } else {
            text(note, "Pas encore possede", font(9, true), pal_.warning, 6, 4, kScreenW - kListW - 24);
            text(note, "Entree cree une image vierge signee.", font(9), dim_, 6, 18, kScreenW - kListW - 24, true);
        }
    }
    render_footer({{"Haut/Bas", "Choix"}, {"Entree", "Creer"}, {"Tab", "Biblio."}, {"Echap", "Retour"}});
}

void ZeroTagUi::render_detail() {
    const amiibo::Item* item = nullptr;
    for (const auto& it : library_.items()) {
        if (it.path == current_path_) {
            item = &it;
        }
    }
    if (!item) {
        render_bar("ZeroTag", "Introuvable");
        render_footer({{"Echap", "Retour"}});
        return;
    }
    render_bar(item_name(*item), item->entry ? item->entry->amiibo_series : "");

    box(root_, 8, 26, 86, 120, pal_.button, 1, pal_.border, 6);
    thumb(root_, item->entry, 8 + (86 - 96 * 51 / 88) / 2, 38, 96);

    const int x = 104;
    const int w = kScreenW - x - 8;
    text(root_, item_name(*item), font(14, true), pal_.text, x, 24, w);
    std::string sub = item->entry ? item->entry->game_series + " - " + item->entry->type : "Personnage hors catalogue";
    text(root_, sub, font(9), dim_, x, 44, w);

    const char* sig = !item->signature_checked ? "Signature non verifiee"
                                               : (item->signature_ok ? "Signature valide" : "Signature invalide");
    const lv_color_t sig_color = !item->signature_checked ? dim_ : (item->signature_ok ? pal_.primary : pal_.error);
    lv_obj_t* chip1 = box(root_, x, 60, 112, 14, pal_.button, 1, sig_color, 7);
    lv_obj_t* c1 = text(chip1, sig, font(9), sig_color, 0, 0);
    lv_obj_align(c1, LV_ALIGN_CENTER, 0, 0);
    lv_obj_t* chip2 = box(root_, x + 118, 60, 56, 14, pal_.button, 1, pal_.border, 7);
    lv_obj_t* c2 = text(chip2, "NTAG215", font(9), pal_.text, 0, 0);
    lv_obj_align(c2, LV_ALIGN_CENTER, 0, 0);

    char id[24];
    std::snprintf(id, sizeof(id), "%08x %08x", item->head, item->tail);
    const struct {
        const char* k;
        std::string v;
    } rows[] = {{"UID", item->uid}, {"Id", id}, {"Fichier", base_name(item->path)}};
    int y = 80;
    for (const auto& r : rows) {
        text(root_, r.k, font(9), dim_, x, y);
        text(root_, clip(r.v, 28), font(9), pal_.text, x + 40, y, w - 40);
        y += 13;
    }

    lv_obj_t* emu = box(root_, x, 126, 104, 20, pal_.primary, 0, pal_.primary, 4);
    lv_obj_t* el = text(emu, "Entree  Emuler", font(10, true), pal_.background, 0, 0);
    lv_obj_align(el, LV_ALIGN_CENTER, 0, 0);
    lv_obj_t* reset = box(root_, x + 108, 126, 54, 20, pal_.button, 1,
                          pending_confirm_ == 1 ? pal_.warning : pal_.border, 4);
    lv_obj_t* rl = text(reset, pending_confirm_ == 1 ? "R ?" : "R Neuf", font(10), pal_.text, 0, 0);
    lv_obj_align(rl, LV_ALIGN_CENTER, 0, 0);
    lv_obj_t* del = box(root_, x + 166, 126, 38, 20, pal_.button, 1, pending_confirm_ == 2 ? pal_.error : pal_.border, 4);
    lv_obj_t* dl = text(del, pending_confirm_ == 2 ? "Supp ?" : "Supp", font(10), pal_.error, 0, 0);
    lv_obj_align(dl, LV_ALIGN_CENTER, 0, 0);

    render_footer({{"Entree", "Emuler"}, {"R", "Nouvel UID"}, {"Suppr", "Supprimer"}, {"Echap", "Retour"}});
}

void ZeroTagUi::render_emulating() {
    const amiibo::Item* item = nullptr;
    for (const auto& it : library_.items()) {
        if (it.path == current_path_) {
            item = &it;
        }
    }
    const bool running = session_ != nullptr;
    const bool field = running && session_->stats().field.load();
    const bool selected = running && session_->stats().selected.load();
    const lv_color_t accent = !emu_error_.empty() ? pal_.error : (selected ? pal_.primary : pal_.info);

    render_bar("Emulation", item ? item_name(*item) : "");

    // rings and thumbnail
    const int cx = 66, cy = 86;
    box(root_, cx - 46, cy - 46, 92, 92, pal_.background, 1, selected ? accent : pal_.border, 46);
    box(root_, cx - 37, cy - 37, 74, 74, pal_.background, 1, field ? accent : pal_.border, 37);
    box(root_, cx - 28, cy - 28, 56, 56, pal_.surface, selected ? 2 : 1, accent, 28);
    if (item) {
        thumb(root_, item->entry, cx - 16, cy - 22, 46);
    }

    const int x = 132;
    const int w = kScreenW - x - 8;
    if (!emu_error_.empty()) {
        text(root_, "Emulation impossible", font(13, true), pal_.error, x, 30, w);
        text(root_, clip(emu_error_, 90), font(9), pal_.text, x, 50, w, true);
        const bool perms = emu_error_.find("Permission") != std::string::npos ||
                           emu_error_.find("denied") != std::string::npos || emu_error_.find("open") != std::string::npos ||
                           emu_error_.find("GPIO") != std::string::npos;
        text(root_, perms ? "Droits insuffisants : le Cap demande root (voir F1 dans la bibliotheque)."
                          : "Verifie le Cap et relance (F1 : etat).",
             font(9), dim_, x, 88, w, true);
    } else {
        text(root_, selected ? "Lecteur connecte" : (field ? "Lecteur detecte..." : "En attente d'un lecteur..."),
             font(13, true), accent, x, 28, w);
        text(root_, selected ? "Garde le Cap en place jusqu'a la fin du chargement."
                             : "Pose le Cap contre la zone NFC de la console quand le jeu demande de scanner.",
             font(9), dim_, x, 48, w, true);
        const uint32_t reads = running ? session_->stats().reads.load() : 0;
        const uint32_t writes = running ? session_->stats().writes.load() : 0;
        const uint32_t secs = lv_tick_elaps(emu_started_) / 1000u;
        char b[48];
        text(root_, "Lectures", font(9), dim_, x, 92);
        std::snprintf(b, sizeof(b), "%u", reads);
        text(root_, b, font(9), pal_.text, x + 58, 92);
        text(root_, "Ecritures", font(9), dim_, x, 105);
        std::snprintf(b, sizeof(b), writes ? "%u - a enregistrer" : "%u", writes);
        text(root_, b, font(9), writes ? pal_.warning : pal_.text, x + 58, 105, w - 58);
        text(root_, "Duree", font(9), dim_, x, 118);
        std::snprintf(b, sizeof(b), "%02u:%02u", secs / 60, secs % 60);
        text(root_, b, font(9), pal_.text, x + 58, 118);
    }
    if (banner_.empty()) {
        text(root_, "Les ecritures du jeu sont enregistrees a l'arret.", font(9), dim_, 6, kScreenH - kFootH + 4, 230);
        box(root_, 0, kScreenH - kFootH, kScreenW, 1, pal_.border, 0);
        lv_obj_t* cap = box(root_, kScreenW - 62, kScreenH - kFootH + 4, 56, 13, pal_.surface, 1, pal_.border, 3);
        lv_obj_t* l = text(cap, "Echap Stop", font(9), pal_.text, 0, 0);
        lv_obj_align(l, LV_ALIGN_CENTER, 0, 0);
    } else {
        render_footer({});
    }
}

void ZeroTagUi::render_setup() {
    render_bar("ZeroTag", "Etat du systeme");
    struct Row {
        lv_color_t color;
        std::string title;
        std::string detail;
    };
    std::vector<Row> rows;
    rows.push_back({cap_ok_ ? pal_.primary : pal_.error, "Cap CC1101 - NFC",
                    cap_status_.empty() ? "Non verifie (R)" : clip(cap_status_, 90)});
    char b[128];
    std::snprintf(b, sizeof(b), "%s - %zu sauvegardes - %zu personnages au catalogue", library_dir_.c_str(),
                  library_.items().size(), db_.entries().size());
    rows.push_back({library_ok_ && db_ok_ ? pal_.primary : pal_.warning, "Dossier des amiibo",
                    library_ok_ ? b : "Dossier absent : " + library_dir_ + " (cree au premier amiibo)"});
    rows.push_back({keys_ok_ ? pal_.primary : pal_.warning, keys_ok_ ? "Cles trouvees" : "Cles introuvables",
                    keys_ok_ ? keys_path_
                             : "Copie key_retail.bin dans " + keys_path_ +
                                   ". Sans elle : emulation oui, creation et signatures non."});
    int y = 24;
    for (const auto& r : rows) {
        box(root_, 8, y + 3, 8, 8, r.color, 0, r.color, 4);
        text(root_, r.title, font(11, true), pal_.text, 24, y, kScreenW - 32);
        text(root_, r.detail, font(9), dim_, 24, y + 15, kScreenW - 32, true);
        y += 40;
        box(root_, 0, y - 6, kScreenW, 1, pal_.surface, 0);
    }
    render_footer({{"R", "Reverifier"}, {"Echap", "Retour"}});
}

} // namespace app
