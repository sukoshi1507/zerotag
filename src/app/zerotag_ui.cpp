/*
 * SPDX-License-Identifier: MIT
 */

#include "zerotag_ui.h"

#include "audio.h"
#include "cap_setup.h"
#include "device_status.h"
#include "i18n.h"
#include "linux_input.h"
#include "online_update.h"
#include "logger.h"
#include "screenshot_service.h"

#include <algorithm>
#include <cctype>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <ctime>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <thread>
#include <vector>

#include <pwd.h>
#include <unistd.h>

namespace fs = std::filesystem;
using app::i18n::Str;
using app::i18n::tr;

namespace app {
namespace {

constexpr int kScreenW = 320;
constexpr int kScreenH = 170;
constexpr int kInset = 4;                     // width of the pixel frame
constexpr int kBarH = 18;
constexpr int kFootH = 18;
constexpr int kInnerW = kScreenW - 2 * kInset; // 312
constexpr int kContentTop = kInset + kBarH;    // 22
constexpr int kFootTop = kScreenH - kInset - kFootH; // 148
constexpr int kListW = 190;
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

// Catalogue facets, in the order of the Facets screen.
constexpr amiibo::Facet kFacetChoices[] = {amiibo::Facet::AmiiboSeries, amiibo::Facet::Game, amiibo::Facet::Type};
constexpr size_t kFacetChoiceCount = sizeof(kFacetChoices) / sizeof(kFacetChoices[0]);

const char* facet_label(amiibo::Facet facet) {
    switch (facet) {
    case amiibo::Facet::AmiiboSeries: return tr(Str::FacetSeries);
    case amiibo::Facet::Game: return tr(Str::FacetGame);
    case amiibo::Facet::Type: return tr(Str::FacetType);
    }
    return "";
}

size_t facet_index(amiibo::Facet facet) {
    for (size_t i = 0; i < kFacetChoiceCount; ++i) {
        if (kFacetChoices[i] == facet) return i;
    }
    return 0;
}

// ---- scan helpers
constexpr int kNtagPages = 135;          // 540 bytes
constexpr size_t kModelInfoOffset = 0x54; // head and tail, big endian (pages 21-22)

uint32_t scan_be32(const uint8_t* p) {
    return (static_cast<uint32_t>(p[0]) << 24) | (static_cast<uint32_t>(p[1]) << 16) |
           (static_cast<uint32_t>(p[2]) << 8) | p[3];
}

std::string scan_uid_text(const amiibo::TagImage& image) {
    char b[32];
    std::snprintf(b, sizeof(b), "%02X:%02X:%02X:%02X:%02X:%02X:%02X", image[0], image[1], image[2], image[4],
                  image[5], image[6], image[7]);
    return b;
}

// File name of a new backup: the character when it is in the catalogue, else the UID. Same convention as
// amiibo::Library::create (name_UIDsuffix); the suffix avoids two copies of one character clashing.
std::string scan_stem(const amiibo::Entry* entry, const amiibo::TagImage& image) {
    char uid[16];
    std::snprintf(uid, sizeof(uid), "%02X%02X%02X%02X%02X%02X%02X", image[0], image[1], image[2], image[4], image[5],
                  image[6], image[7]);
    if (!entry) {
        return uid;
    }
    std::string stem;
    for (const char c : entry->name) {
        stem += std::isalnum(static_cast<unsigned char>(c)) != 0 ? c : '_';
    }
    if (stem.empty()) {
        stem = entry->id;
    }
    if (stem.size() > 48) {
        stem.resize(48);
    }
    char suffix[16];
    std::snprintf(suffix, sizeof(suffix), "_%02X%02X%02X%02X", image[4], image[5], image[6], image[7]);
    return stem + suffix;
}

// Reads the 135 pages of an NTAG215 (GET_VERSION first). Runs on the scan worker thread.
bool scan_read_tag(platform::St25r3916& chip, std::atomic<int>& progress, std::atomic<ScanIssue>& issue,
                   std::atomic<int>& issue_page, amiibo::TagImage& image) {
    uint8_t version[8] = {};
    if (!chip.type2_get_version(version)) {
        issue = ScanIssue::Interrupted;
        issue_page = 0;
        return false;
    }
    // NTAG family (version[2] = 0x04), storage 0x11 = NTAG215
    if (version[2] != 0x04 || version[6] != 0x11) {
        issue = ScanIssue::NotNtag215;
        return false;
    }
    // Blocks of 4 pages: 0, 4, ..., 128 cover pages 0-131; the last block starts at 131 so that it ends on page 134.
    uint8_t block[16] = {};
    std::vector<int> starts;
    for (int page = 0; page <= 128; page += 4) {
        starts.push_back(page);
    }
    starts.push_back(kNtagPages - 4);
    for (const int page : starts) {
        if (!chip.type2_read(static_cast<uint8_t>(page), block)) {
            issue = ScanIssue::Interrupted;
            issue_page = page;
            return false;
        }
        std::memcpy(image.data() + page * 4, block, sizeof(block));
        progress = std::min(page + 4, kNtagPages);
    }
    return true;
}

} // namespace

// ---------------------------------------------------------------------------
// Construction and data
// ---------------------------------------------------------------------------

ZeroTagUi::ZeroTagUi(AssetManager& assets, bool dark_mode, std::function<void()> request_quit,
                     std::function<void(const std::string&, const std::string&)> persist_setting)
    : assets_(assets),
      dark_(dark_mode),
      request_quit_(std::move(request_quit)),
      persist_setting_(std::move(persist_setting)) {
    (void)dark_; // the Famicom look is dark only
    pal_.background = lv_color_hex(0x101014);
    pal_.surface = lv_color_hex(0x1a1518);
    pal_.bar = lv_color_hex(0x0b0b10);
    pal_.button = lv_color_hex(0x0b0b10);
    pal_.border = lv_color_hex(0x4a3a3c);
    pal_.text = lv_color_hex(0xf1ebd8);
    pal_.cream = lv_color_hex(0xece6d3);
    pal_.dark_red = lv_color_hex(0x6b2a30);
    pal_.accent = lv_color_hex(0xc8202f);
    pal_.primary = lv_color_hex(0xf3c93a);
    pal_.warning = lv_color_hex(0xf3c93a);
    pal_.error = lv_color_hex(0xe88080);
    pal_.info = lv_color_hex(0xece6d3);
    dim_ = lv_color_hex(0xa79f8e);
}

ZeroTagUi::~ZeroTagUi() {
    stop_scan();
    stop_online_update();
    manual_leave();
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
    online_path_ = home_dir() + "/.zerotag/amiibo_online.json";
    load_catalogue();

    keys_ = std::make_unique<amiibo::Keys>();
    keys_ok_ = keys_->load(keys_path_, keys_error_);
    if (!keys_ok_) {
        LOG_WARN("keys: {}", keys_error_);
    }
    state_path_ = home_dir() + "/.zerotag/library_state.txt";
    std::string state_error;
    if (!amiibo::load_library_state(state_path_, state_, state_error)) {
        LOG_WARN("library state: {}", state_error);
    }
    rescan_library();
}

void ZeroTagUi::load_catalogue() {
    db_ok_ = db_.load(db_path_, db_error_);
    if (!db_ok_) {
        LOG_WARN("catalogue: {}", db_error_);
        return;
    }
    // The list downloaded by the optional online update adds the figures the bundled catalogue does not know.
    std::error_code ec;
    if (fs::is_regular_file(online_path_, ec)) {
        std::string merge_error;
        const size_t added = db_.merge(online_path_, merge_error);
        if (!merge_error.empty()) {
            LOG_WARN("online catalogue: {}", merge_error);
        } else {
            LOG_INFO("online catalogue: {} new amiibo", added);
        }
    }
}

void ZeroTagUi::rescan_library() {
    library_error_.clear();
    library_ok_ = library_.scan(library_dir_, db_, keys_ok_ ? keys_.get() : nullptr, library_error_);
    if (!library_ok_) {
        LOG_WARN("library: {}", library_error_);
    }
}

std::vector<const amiibo::Item*> ZeroTagUi::visible_items() const {
    std::vector<const amiibo::Item*> items;
    if (!lib_query_.empty()) {
        items = library_.search(lib_query_);
    } else {
        for (const auto& item : library_.items()) {
            items.push_back(&item);
        }
    }

    // Favourites and recent list: the tab filters the search result; "All" puts favourites first (scan order kept).
    if (lib_tab_ == LibTab::Recent) {
        std::vector<const amiibo::Item*> recent;
        for (const auto& name : state_.recent) {
            for (const auto* item : items) {
                if (base_name(item->path) == name) {
                    recent.push_back(item);
                    break;
                }
            }
        }
        return recent;
    }
    if (lib_tab_ == LibTab::Favorites) {
        items.erase(std::remove_if(items.begin(), items.end(),
                                   [this](const amiibo::Item* item) {
                                       return !state_.is_favorite(base_name(item->path));
                                   }),
                    items.end());
    } else {
        std::stable_partition(items.begin(), items.end(), [this](const amiibo::Item* item) {
            return state_.is_favorite(base_name(item->path));
        });
    }
    return items;
}

std::vector<const amiibo::Entry*> ZeroTagUi::visible_entries() const {
    if (filter_on_) {
        return db_.search_in(filter_facet_, filter_value_, cat_query_);
    }
    return db_.search(cat_query_);
}

// ---------------------------------------------------------------------------
// Favourites and recent list (persisted in ~/.zerotag/library_state.txt)
// ---------------------------------------------------------------------------

int ZeroTagUi::find_visible_index(const std::string& path) const {
    const auto items = visible_items();
    for (size_t i = 0; i < items.size(); ++i) {
        if (items[i]->path == path) {
            return static_cast<int>(i);
        }
    }
    return -1;
}

bool ZeroTagUi::persist_library_state(std::string& error) {
    if (amiibo::save_library_state(state_path_, state_, error)) {
        return true;
    }
    LOG_WARN("library state: {}", error);
    return false;
}

void ZeroTagUi::toggle_favorite(const amiibo::Item& item) {
    const std::string name = base_name(item.path);
    const bool now_favorite = state_.toggle_favorite(name);
    std::string error;
    if (!persist_library_state(error)) {
        state_.toggle_favorite(name); // keep the screen and the file in step: undo
        say(i18n::format(Str::BannerFailed, error.c_str()), true);
        return;
    }
    if (now_favorite) {
        platform::audio::play(platform::Sound::Blip);
    }
    say(i18n::format(now_favorite ? Str::BannerFavAdded : Str::BannerFavRemoved, name.c_str()));
}

void ZeroTagUi::record_recent(const std::string& path) {
    state_.mark_emulated(base_name(path));
    std::string error;
    persist_library_state(error); // a failed write only loses the recent order: no banner
}

void ZeroTagUi::cycle_library_tab() {
    lib_tab_ = static_cast<LibTab>((static_cast<int>(lib_tab_) + 1) % static_cast<int>(LibTab::Count));
    lib_sel_ = 0;
    lib_top_ = 0;
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
    if (!platform::spi_available()) {
        screen_after_cap_ = Screen::Library;
        screen_ = Screen::CapSetup;
    }
    render();
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

void ZeroTagUi::go(Screen screen) {
    screen_ = screen;
    pending_confirm_ = 0;
    if (screen == Screen::Scan) {
        start_scan_if_idle();
    }
    if (screen == Screen::Manual) {
        manual_enter();
    } else {
        manual_leave();
    }
    render();
}

void ZeroTagUi::say(const std::string& value, bool error) {
    banner_ = value;
    banner_error_ = error;
    banner_until_ = lv_tick_get() + kBannerMs;
}

void ZeroTagUi::create_blank(const amiibo::Entry& entry) {
    if (!keys_ok_) {
        say(tr(Str::KeysMissingF1), true);
        return;
    }
    std::string path, error;
    if (!library_.create(*keys_, entry, path, error)) {
        say(i18n::format(Str::BannerFailed, error.c_str()), true);
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
    say(i18n::format(Str::BannerCreated, base_name(path).c_str()));
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
    if (!ec) {
        state_.forget(base_name(path));
        std::string state_error;
        persist_library_state(state_error);
    }
    rescan_library();
    lib_sel_ = std::max(0, std::min(lib_sel_, static_cast<int>(visible_items().size()) - 1));
    say(ec ? std::string(tr(Str::BannerCantDelete)) : i18n::format(Str::BannerDeleted, base_name(path).c_str()),
        static_cast<bool>(ec));
    go(Screen::Library);
}

void ZeroTagUi::reset_item(const amiibo::Item& item) {
    if (!keys_ok_) {
        say(tr(Str::KeysMissingF1), true);
        return;
    }
    amiibo::TagImage image;
    std::string error;
    if (!amiibo::generate(*keys_, item.head, item.tail, image, error) ||
        !amiibo::Library::write_image(item.path, image, error)) {
        say(i18n::format(Str::BannerFailed, error.c_str()), true);
        return;
    }
    rescan_library();
    say(tr(Str::BannerResetDone));
    render();
}

void ZeroTagUi::start_emulation(const amiibo::Item& item) {
    if (!platform::spi_available()) {
        open_cap_setup(Screen::Detail);
        return;
    }
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
    platform::EmulationSession::RotateUid rotate;
    if (uid_rotation_ && keys_ok_) {
        rotate = [this](uint8_t* data) {
            amiibo::TagImage copy;
            std::copy(data, data + copy.size(), copy.begin());
            std::string e;
            if (!amiibo::rotate_uid(*keys_, copy, e)) {
                return false;
            }
            std::copy(copy.begin(), copy.end(), data);
            return true;
        };
    }
    if (!session_->start(*chip_, image, 0, error, rotate)) {
        emu_error_ = error;
        session_.reset();
        chip_.reset();
    } else {
        record_recent(item.path);
        platform::audio::play(platform::Sound::Start);
    }
    if (!emu_error_.empty()) {
        platform::audio::play(platform::Sound::Error);
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
                say(i18n::format(Str::BannerSaved, base_name(current_path_).c_str()));
            } else {
                say(i18n::format(Str::BannerSaveFailed, error.c_str()), true);
            }
            rescan_library();
        }
        session_.reset();
    }
    chip_.reset(); // powers the Cap off
}

void ZeroTagUi::cycle_language(int direction) {
    i18n::Lang next = i18n::language();
    for (int i = 0; i < (direction >= 0 ? 1 : i18n::kLangCount - 1); ++i) {
        next = i18n::next_language(next);
    }
    i18n::set_language(next);
    if (persist_setting_) {
        persist_setting_("language", i18n::code(next));
    }
    say(i18n::format(Str::BannerLanguage, i18n::native_name(next)));
    render();
}

void ZeroTagUi::open_cap_setup(Screen after) {
    cap_password_.clear();
    cap_message_.clear();
    cap_busy_ = false;
    screen_after_cap_ = after;
    go(Screen::CapSetup);
}

void ZeroTagUi::submit_cap_setup() {
    if (cap_busy_) {
        return;
    }
    cap_busy_ = true;
    cap_message_.clear();
    render();
    lv_refr_now(nullptr); // sudo can take a couple of seconds
    const auto result = platform::run_cap_setup(cap_password_); // wipes the password
    cap_busy_ = false;
    switch (result.status) {
    case platform::CapSetupResult::Status::Ok:
        if (platform::spi_available()) {
            check_hardware();
            say(tr(Str::CapDone));
            go(screen_after_cap_);
            return;
        }
        cap_message_ = i18n::format(Str::CapFailed, "no SPI node");
        break;
    case platform::CapSetupResult::Status::PasswordRefused:
        cap_message_ = tr(Str::CapBadPassword);
        break;
    default:
        cap_message_ = i18n::format(Str::CapFailed, result.message.c_str());
        break;
    }
    render();
}

void ZeroTagUi::set_sound(bool enabled) {
    sound_ = enabled;
    platform::audio::set_enabled(enabled);
}

void ZeroTagUi::toggle_sound() {
    set_sound(!sound_);
    if (persist_setting_) {
        persist_setting_("sound", sound_ ? "yes" : "no");
    }
    platform::audio::play(platform::Sound::Blip); // silent when it was just switched off
    render();
}

void ZeroTagUi::toggle_rotation() {
    if (!uid_rotation_ && !keys_ok_) {
        say(tr(Str::KeysMissingF1), true); // the rotation re-signs the image: it needs the keys
        render();
        return;
    }
    uid_rotation_ = !uid_rotation_;
    if (persist_setting_) {
        persist_setting_("uid_rotation", uid_rotation_ ? "yes" : "no");
    }
    say(tr(uid_rotation_ ? Str::BannerRotationOn : Str::BannerRotationOff));
    render();
}

void ZeroTagUi::check_hardware() {
    cap_ok_ = false;
    cap_status_.clear();
    std::string error;
    std::error_code ec;
    if (!fs::exists(platform::St25r3916Config{}.spi_device, ec)) {
        cap_status_ = tr(Str::ErrSpi); // the overlay is not loaded: nothing to open yet
        return;
    }
    platform::St25r3916 chip;
    if (chip.open(error)) {
        cap_ok_ = true;
        cap_status_ = i18n::format(Str::SetupCapDetected, chip.revision());
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

    if (online_phase_.load() == OnlinePhase::Done || online_phase_.load() == OnlinePhase::Failed) {
        finish_online_update();
        dirty = false; // finish_online_update() has rendered
    } else if (online_phase_.load() == OnlinePhase::Running && screen_ == Screen::Settings) {
        dirty = true; // the "downloading" dots
    }

    if (screen_ == Screen::Scan) {
        // Scan: the worker runs on its own thread; the UI joins it and saves the backup when it is done.
        if (scan_phase_.load() == ScanPhase::Done) {
            finish_scan();
        }
        if (scan_phase_.load() == ScanPhase::Failed && !scan_error_sounded_) {
            scan_error_sounded_ = true;
            platform::audio::play(platform::Sound::Error);
        }
        dirty = true;
    }

    if (session_) {
        if (!session_->running()) {
            // The worker ended on its own (setup failure or timeout).
            const auto result = session_->finish();
            if (!result.ok) {
                emu_error_ = result.error.empty() ? std::string(tr(Str::EmuInterrupted)) : result.error;
            }
            session_.reset();
            chip_.reset();
            dirty = true;
        } else {
            const auto& st = session_->stats();
            const uint32_t key = (st.reads.load() << 8) ^ (st.writes.load() << 4) ^ (st.field.load() ? 2u : 0u) ^
                                 (st.selected.load() ? 1u : 0u) ^ ((lv_tick_elaps(emu_started_) / 1000u) << 20) ^
                                 (st.rotations.load() << 14);
            if (key != last_stats_key_) {
                last_stats_key_ = key;
                dirty = true;
            }
            // Autosave once the reader is done with a burst of writes or after a UID rotation.
            platform::EmulationSession::Image snapshot;
            if (session_->poll_autosave(snapshot, 1500)) {
                std::string error;
                if (amiibo::Library::write_image(current_path_, snapshot, error)) {
                    rescan_library();
                    say(tr(Str::BannerAutosaved));
                } else {
                    say(i18n::format(Str::BannerSaveFailed, error.c_str()), true);
                }
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
// Online update of the catalogue (optional, off by default)
// ---------------------------------------------------------------------------

void ZeroTagUi::toggle_online() {
    online_ = !online_;
    if (persist_setting_) {
        persist_setting_("online", online_ ? "yes" : "no");
    }
    platform::audio::play(platform::Sound::Blip);
    render();
}

bool ZeroTagUi::is_upcoming(const amiibo::Entry& entry) {
    if (entry.release.empty()) {
        return false;
    }
    char today[16];
    const std::time_t now = std::time(nullptr);
    std::tm local{};
    localtime_r(&now, &local);
    std::strftime(today, sizeof(today), "%Y-%m-%d", &local);
    return entry.release > today;
}

void ZeroTagUi::start_online_update() {
    if (!online_) {
        say(tr(Str::UpdateNeedsOn), true);
        return;
    }
    if (online_thread_.joinable() || online_phase_.load() == OnlinePhase::Running) {
        return;
    }
    online_cancel_ = false;
    online_phase_ = OnlinePhase::Running;
    online_thread_ = std::thread([this] {
        std::error_code ec;
        fs::create_directories(fs::path(online_path_).parent_path(), ec);
        std::string error;
        const bool ok = platform::download_amiibo_list(online_path_, online_cancel_, error);
        {
            std::lock_guard<std::mutex> lock(online_mutex_);
            online_error_ = error;
        }
        online_phase_ = ok ? OnlinePhase::Done : OnlinePhase::Failed;
    });
    render();
}

void ZeroTagUi::stop_online_update() {
    online_cancel_ = true;
    if (online_thread_.joinable()) {
        online_thread_.join();
    }
}

void ZeroTagUi::finish_online_update() {
    if (online_thread_.joinable()) {
        online_thread_.join();
    }
    if (online_phase_.load() == OnlinePhase::Done) {
        const size_t before = db_.entries().size();
        reload_catalogue();
        online_added_ = db_.entries().size() > before ? db_.entries().size() - before : 0;
        say(online_added_ > 0 ? i18n::format(Str::UpdateDone, static_cast<int>(online_added_)) : std::string(tr(Str::UpdateUpToDate)),
            false);
        platform::audio::play(platform::Sound::Blip);
    } else {
        std::string error;
        {
            std::lock_guard<std::mutex> lock(online_mutex_);
            error = online_error_;
        }
        say(i18n::format(Str::UpdateFailed, clip(error, 40).c_str()), true);
        platform::audio::play(platform::Sound::Error);
    }
    online_phase_ = OnlinePhase::Idle;
    render();
}

void ZeroTagUi::reload_catalogue() {
    scan_entry_ = nullptr;
    cat_sel_ = 0;
    cat_top_ = 0;
    load_catalogue();
    rescan_library();
}

// ---------------------------------------------------------------------------
// Scan (NFC-A reader on the Cap: a real NTAG215 becomes a backup in the library)
// ---------------------------------------------------------------------------

void ZeroTagUi::start_scan_if_idle() {
    // A finished scan stays on screen until ESC; a worker still running is not started twice.
    if (scan_thread_.joinable() || scan_phase_.load() == ScanPhase::Saved) {
        return;
    }
    start_scan();
}

void ZeroTagUi::start_scan() {
    scan_stop_ = false;
    scan_error_sounded_ = false;
    scan_phase_ = ScanPhase::Opening;
    scan_issue_ = ScanIssue::None;
    scan_issue_page_ = 0;
    scan_progress_ = 0;
    scan_entry_ = nullptr;
    scan_path_.clear();
    scan_uid_.clear();
    {
        std::lock_guard<std::mutex> lock(scan_mutex_);
        scan_error_.clear();
    }
    scan_thread_ = std::thread([this] { scan_worker(); });
}

void ZeroTagUi::stop_scan() {
    scan_stop_ = true;
    if (scan_thread_.joinable()) {
        scan_thread_.join();
    }
}

void ZeroTagUi::scan_worker() {
    platform::St25r3916 chip; // powers the Cap on; powered off again when this function returns
    std::string error;
    if (!chip.open(error) || !chip.init_nfc_a(error)) { // open() powers the chip; init_nfc_a() makes it a reader
        {
            std::lock_guard<std::mutex> lock(scan_mutex_);
            scan_error_ = error;
        }
        scan_phase_ = ScanPhase::Failed;
        return;
    }
    while (!scan_stop_) {
        scan_phase_ = ScanPhase::Waiting;
        platform::St25r3916::Tag tag;
        std::string why;
        if (!chip.detect_tag(tag, why)) { // REQA/WUPA: nothing in the field yet
            std::this_thread::sleep_for(std::chrono::milliseconds(120));
            continue;
        }
        scan_phase_ = ScanPhase::Reading;
        scan_progress_ = 0;
        scan_issue_ = ScanIssue::None;
        amiibo::TagImage image{};
        if (scan_read_tag(chip, scan_progress_, scan_issue_, scan_issue_page_, image)) {
            {
                std::lock_guard<std::mutex> lock(scan_mutex_);
                scan_image_ = image;
            }
            scan_phase_ = ScanPhase::Done;
            return;
        }
        // Tag removed or unreadable: back to waiting, the message stays on screen until the next attempt.
        std::this_thread::sleep_for(std::chrono::milliseconds(200));
    }
}

void ZeroTagUi::finish_scan() {
    if (scan_thread_.joinable()) {
        scan_thread_.join(); // the worker has just set Done: it is leaving its loop
    }
    amiibo::TagImage image{};
    {
        std::lock_guard<std::mutex> lock(scan_mutex_);
        image = scan_image_;
    }
    const amiibo::Entry* entry = db_.find(scan_be32(&image[kModelInfoOffset]),
                                          scan_be32(&image[kModelInfoOffset + 4]));
    std::error_code ec;
    fs::create_directories(library_dir_, ec);
    const std::string stem = scan_stem(entry, image);
    std::string path;
    for (int n = 1;; ++n) {
        const std::string name = n == 1 ? stem : stem + "_" + std::to_string(n);
        path = (fs::path(library_dir_) / (name + ".bin")).string();
        if (!fs::exists(path, ec)) {
            break;
        }
    }
    std::string error;
    if (!amiibo::Library::write_image(path, image, error)) {
        {
            std::lock_guard<std::mutex> lock(scan_mutex_);
            scan_error_ = error;
        }
        scan_phase_ = ScanPhase::Failed;
        platform::audio::play(platform::Sound::Error);
        say(i18n::format(Str::BannerSaveFailed, error.c_str()), true);
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
    scan_entry_ = entry;
    scan_path_ = path;
    scan_uid_ = scan_uid_text(image);
    scan_phase_ = ScanPhase::Saved;
    platform::audio::play(platform::Sound::Success); // tu-du-tuuu
    say(i18n::format(Str::BannerScanned, base_name(path).c_str()));
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
    if (key >= 'A' && key <= 'Z') {
        key = key - 'A' + 'a'; // filters ignore case
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

// ---- catalogue facets: keys and state ----

void ZeroTagUi::apply_facet_filter(amiibo::Facet facet, const std::string& value) {
    filter_facet_ = facet;
    filter_value_ = value;
    filter_on_ = true;
    cat_sel_ = 0;
    cat_top_ = 0;
    go(Screen::Catalogue);
}

void ZeroTagUi::facets_back() {
    if (facet_stage_ == 1) {
        facet_stage_ = 0;
        facet_sel_ = static_cast<int>(facet_index(facet_browse_));
        facet_top_ = 0;
        render();
    } else {
        go(Screen::Catalogue);
    }
}

bool ZeroTagUi::handle_facets_key(uint32_t key) {
    const std::vector<amiibo::FacetCount> values =
        facet_stage_ == 1 ? db_.facet_counts(facet_browse_) : std::vector<amiibo::FacetCount>{};
    const size_t count = facet_stage_ == 0 ? kFacetChoiceCount : values.size();
    if (key == LV_KEY_UP) {
        move_selection(-1, count, facet_sel_, facet_top_, kRows);
    } else if (key == LV_KEY_DOWN) {
        move_selection(1, count, facet_sel_, facet_top_, kRows);
    } else if (key == platform::kKeyPageUp) {
        move_selection(-kRows, count, facet_sel_, facet_top_, kRows);
    } else if (key == platform::kKeyPageDown) {
        move_selection(kRows, count, facet_sel_, facet_top_, kRows);
    } else if (key == LV_KEY_ENTER) {
        if (facet_stage_ == 0) {
            if (facet_sel_ < static_cast<int>(kFacetChoiceCount)) {
                facet_browse_ = kFacetChoices[facet_sel_];
                facet_stage_ = 1;
                facet_sel_ = 0;
                facet_top_ = 0;
            }
        } else if (facet_sel_ < static_cast<int>(values.size())) {
            apply_facet_filter(facet_browse_, values[static_cast<size_t>(facet_sel_)].value);
            return true;
        }
    } else {
        return false;
    }
    render();
    return true;
}

bool ZeroTagUi::on_key(uint32_t key, bool long_pressed) {
    if (key == platform::kKeyPrintScreen) {
        if (!long_pressed) {
            const auto result = platform::screenshot::capture_active_screen();
            say(result.success ? i18n::format(Str::BannerCapture, base_name(result.path).c_str())
                           : std::string(tr(Str::BannerCaptureFailed)),
            !result.success);
            render();
        }
        return true;
    }
    if (key == platform::kKeyHelp && screen_ == Screen::CapSetup) {
        return true;
    }
    if (key == platform::kKeyHelp) {
        if (!long_pressed) {
            if (screen_ == Screen::Help) {
                go(screen_before_help_);
            } else {
                screen_before_help_ = screen_;
                go(Screen::Help);
            }
        }
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
    if (screen_ == Screen::CapSetup) {
        if (key == LV_KEY_ENTER) {
            submit_cap_setup();
        } else if (key == LV_KEY_BACKSPACE) {
            if (!cap_password_.empty()) {
                cap_password_.pop_back();
            }
            render();
        } else if (key >= 32 && key <= 126 && cap_password_.size() < 64 && !cap_busy_) {
            cap_password_.push_back(static_cast<char>(key));
            render();
        }
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
        } else if (key == platform::kKeyF4) {
            // Scan a real amiibo: without the Cap, first go through the setup screen (it comes back to the scan).
            if (!platform::spi_available()) {
                open_cap_setup(Screen::Scan);
            } else {
                go(Screen::Scan);
            }
            return true;
        } else if (key == platform::kKeyF1) {
            screen_before_settings_ = Screen::Library;
            check_hardware();
            go(Screen::Settings);
            return true;
        } else if (key == platform::kKeyF2) {
            if (lib_sel_ < static_cast<int>(items.size())) {
                const std::string path = items[static_cast<size_t>(lib_sel_)]->path;
                toggle_favorite(*items[static_cast<size_t>(lib_sel_)]);
                // The favourites move to the top: keep the cursor on the same backup.
                const int index = find_visible_index(path);
                if (index >= 0) {
                    lib_sel_ = index;
                }
                move_selection(0, visible_items().size(), lib_sel_, lib_top_, kRows);
            }
        } else if (key == platform::kKeyF3) {
            cycle_library_tab();
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
            screen_before_settings_ = Screen::Catalogue;
            check_hardware();
            go(Screen::Settings);
            return true;
        } else if (key == platform::kKeyF2) {
            facet_stage_ = 0;
            facet_sel_ = static_cast<int>(facet_index(facet_browse_));
            facet_top_ = 0;
            go(Screen::Facets);
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
                say(tr(Str::BannerConfirmReset));
                render();
            }
        } else if (key == 'u') {
            toggle_rotation();
        } else if (key == platform::kKeyF2) {
            toggle_favorite(*item);
            render();
        } else if (key == LV_KEY_DEL) {
            if (pending_confirm_ == 2) {
                pending_confirm_ = 0;
                delete_item(*item);
            } else {
                pending_confirm_ = 2;
                say(tr(Str::BannerConfirmDelete));
                render();
            }
        } else {
            return false;
        }
        return true;
    }
    case Screen::Facets:
        return handle_facets_key(key);
    case Screen::Emulating:
    case Screen::Scan: // keys ignored here: ESC leaves (on release)
        return true;
    case Screen::Settings:
        if (key == LV_KEY_UP) {
            settings_sel_ = std::max(0, settings_sel_ - 1);
        } else if (key == LV_KEY_DOWN) {
            settings_sel_ = std::min(9, settings_sel_ + 1);
        } else if (settings_sel_ == 0 && key == LV_KEY_LEFT) {
            cycle_language(-1);
            return true;
        } else if (settings_sel_ == 1 && key == LV_KEY_LEFT) {
            toggle_rotation();
            return true;
        } else if (settings_sel_ == 2 && key == LV_KEY_LEFT) {
            toggle_sound();
            return true;
        } else if (settings_sel_ == 3 && key == LV_KEY_LEFT) {
            toggle_online();
            return true;
        } else if (key == LV_KEY_RIGHT || key == LV_KEY_ENTER) {
            switch (settings_sel_) {
            case 0:
                cycle_language(1);
                return true;
            case 1:
                toggle_rotation();
                return true;
            case 2:
                toggle_sound();
                return true;
            case 3:
                toggle_online();
                return true;
            case 4:
                start_online_update();
                return true;
            case 5:
                if (!platform::spi_available()) {
                    open_cap_setup(Screen::Settings);
                    return true;
                }
                check_hardware();
                break;
            case 6:
                keys_ok_ = keys_->load(keys_path_, keys_error_);
                rescan_library();
                break;
            case 7:
                rescan_library();
                break;
            case 8:
                screen_before_help_ = Screen::Settings;
                go(Screen::Help);
                return true;
            default:
                manual_page_ = 0;
                go(Screen::Manual);
                return true;
            }
        } else {
            return false;
        }
        if (settings_sel_ < settings_top_) {
            settings_top_ = settings_sel_;
        }
        if (settings_sel_ >= settings_top_ + 4) {
            settings_top_ = settings_sel_ - 3;
        }
        render();
        return true;
    case Screen::Manual:
        if (key == LV_KEY_LEFT || key == LV_KEY_RIGHT) {
            manual_page_ = (manual_page_ + (key == LV_KEY_RIGHT ? 1 : 4)) % 5;
            platform::audio::play(platform::Sound::Blip);
            render();
        }
        return true;
    case Screen::Help:
    case Screen::CapSetup:
        return true;
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
        } else if (filter_on_) {
            filter_on_ = false;
            cat_sel_ = 0;
            cat_top_ = 0;
            render();
        } else {
            go(Screen::Library);
        }
        break;
    case Screen::Facets:
        facets_back();
        break;
    case Screen::Detail:
        go(Screen::Library);
        break;
    case Screen::Emulating:
        stop_emulation();
        go(Screen::Detail);
        break;
    case Screen::Settings:
        go(screen_before_settings_);
        break;
    case Screen::Help:
        go(screen_before_help_);
        break;
    case Screen::Manual:
        go(Screen::Settings);
        break;
    case Screen::Scan:
        stop_scan();
        scan_phase_ = ScanPhase::Opening; // the next scan starts from scratch
        go(Screen::Library);
        break;
    case Screen::CapSetup:
        if (!cap_busy_) {
            cap_password_.assign(cap_password_.size(), '\0');
            cap_password_.clear();
            go(screen_after_cap_);
        }
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
    // Thumbnails are not shipped (artwork of the amiibo): look in the installed assets, then in the user's folder.
    std::string path = amiibo::Database::image_path(images_dir_, *entry);
    std::error_code ec;
    if (!fs::exists(path, ec)) {
        path = amiibo::Database::image_path(home_dir() + "/.zerotag/images", *entry);
        if (!fs::exists(path, ec)) {
            return holder;
        }
    }
    lv_obj_t* img = lv_image_create(holder);
    lv_image_set_src(img, ("A:" + path).c_str());
    lv_image_set_pivot(img, 0, 0);
    lv_image_set_scale(img, static_cast<uint32_t>(256 * h / 88));
    lv_obj_set_pos(img, 0, 0);
    return holder;
}

// ---------------------------------------------------------------------------
// Famicom-style building blocks
// ---------------------------------------------------------------------------

void ZeroTagUi::triforce(int x, int y) {
    // 9x4 pixel triforce drawn with 2x2 boxes.
    static const char* const rows[4] = {"....Y....", "...YYY...", "..Y...Y..", ".YYY.YYY."};
    for (int r = 0; r < 4; ++r) {
        for (int c = 0; c < 9; ++c) {
            if (rows[r][c] == 'Y') {
                box(root_, x + c * 2, y + r * 2, 2, 2, pal_.primary, 0);
            }
        }
    }
}

void ZeroTagUi::cursor(int x, int y) {
    // 8x7 pointing-right triangle, NES menu style.
    box(root_, x, y, 2, 7, pal_.accent, 0);
    box(root_, x + 2, y + 1, 2, 5, pal_.accent, 0);
    box(root_, x + 4, y + 2, 2, 3, pal_.accent, 0);
    box(root_, x + 6, y + 3, 2, 1, pal_.accent, 0);
}

void ZeroTagUi::draw_frame() {
    // cream outline (1 px), red band (2 px), dark line (1 px); the screen corners are notched.
    const lv_color_t cream = pal_.cream;
    box(root_, 0, 0, kScreenW, 1, cream, 0);
    box(root_, 0, kScreenH - 1, kScreenW, 1, cream, 0);
    box(root_, 0, 0, 1, kScreenH, cream, 0);
    box(root_, kScreenW - 1, 0, 1, kScreenH, cream, 0);
    box(root_, 1, 1, kScreenW - 2, 2, pal_.accent, 0);
    box(root_, 1, kScreenH - 3, kScreenW - 2, 2, pal_.accent, 0);
    box(root_, 1, 1, 2, kScreenH - 2, pal_.accent, 0);
    box(root_, kScreenW - 3, 1, 2, kScreenH - 2, pal_.accent, 0);
    box(root_, 3, 3, kScreenW - 6, 1, pal_.background, 0);
    box(root_, 3, kScreenH - 4, kScreenW - 6, 1, pal_.background, 0);
    box(root_, 3, 3, 1, kScreenH - 6, pal_.background, 0);
    box(root_, kScreenW - 4, 3, 1, kScreenH - 6, pal_.background, 0);
    // notched corners and gold studs
    box(root_, 0, 0, 2, 2, pal_.background, 0);
    box(root_, kScreenW - 2, 0, 2, 2, pal_.background, 0);
    box(root_, 0, kScreenH - 2, 2, 2, pal_.background, 0);
    box(root_, kScreenW - 2, kScreenH - 2, 2, 2, pal_.background, 0);
    box(root_, kInset, kInset, 2, 2, pal_.primary, 0);
    box(root_, kScreenW - kInset - 2, kInset, 2, 2, pal_.primary, 0);
    box(root_, kInset, kScreenH - kInset - 2, 2, 2, pal_.primary, 0);
    box(root_, kScreenW - kInset - 2, kScreenH - kInset - 2, 2, 2, pal_.primary, 0);
}

void ZeroTagUi::button(int x, int y, int w, const std::string& label, lv_color_t fill, lv_color_t border,
                       lv_color_t color, bool bold) {
    lv_obj_t* b = box(root_, x, y, w, 20, fill, 2, border, 0);
    lv_obj_t* l = text(b, label, font(10, bold), color, 0, 0);
    lv_obj_align(l, LV_ALIGN_CENTER, 0, 0);
}

void ZeroTagUi::search_field(lv_obj_t* parent, const std::string& query, const std::string& placeholder,
                             bool focused, int width) {
    lv_obj_t* f = box(parent, 10, 26, width, 16, pal_.button, 2, focused ? pal_.accent : pal_.cream, 0);
    if (query.empty()) {
        text(f, placeholder, font(10), dim_, 3, 0, width - 12);
    } else {
        text(f, query + "_", font(10), pal_.text, 3, 0, width - 12);
    }
}

void ZeroTagUi::pixel_star(int x, int y, lv_color_t color) {
    // 7x7 five-point star, one pixel per cell.
    static const char* const rows[7] = {"...X...", "...X...", "XXXXXXX", ".XXXXX.", "..XXX..", ".XX.XX.", "XX...XX"};
    for (int r = 0; r < 7; ++r) {
        for (int c = 0; c < 7; ++c) {
            if (rows[r][c] == 'X') {
                box(root_, x + c, y + r, 1, 1, color, 0);
            }
        }
    }
}

void ZeroTagUi::list_row(int y, bool selected, const amiibo::Entry* entry, const std::string& name,
                         const std::string& sub, const std::string& badge, bool favorite) {
    if (selected) {
        box(root_, kInset + 1, y, kListW - kInset - 3, kRowH - 2, lv_color_hex(0x2a1417), 0);
        cursor(kInset + 4, y + 8);
    }
    thumb(root_, entry, 20, y + 2, 20);
    const int tw = kListW - 36 - (badge.empty() ? (favorite ? 20 : 8) : 50);
    text(root_, name, font(11, selected), pal_.text, 36, y + 2, tw);
    text(root_, sub, font(9), dim_, 36, y + 14, kListW - 44);
    if (!badge.empty()) {
        text(root_, badge, font(9), pal_.primary, kListW - 50, y + 4, 44);
    }
    if (favorite) {
        pixel_star(kListW - 16, y + 4, pal_.primary);
    }
}

void ZeroTagUi::scrollbar(size_t count, int first) {
    if (count <= static_cast<size_t>(kRows)) {
        return;
    }
    const int track = kRows * kRowH;
    const int bar_h = std::max(6, track * kRows / static_cast<int>(count));
    const int bar_y = kListTop + (track - bar_h) * first / std::max(1, static_cast<int>(count) - kRows);
    box(root_, kListW - 6, bar_y, 2, bar_h, pal_.border, 0);
}

void ZeroTagUi::render_bar(const std::string& title, const std::string& subtitle) {
    box(root_, kInset, kInset, kInnerW, kBarH, pal_.surface, 0);
    box(root_, kInset, kInset + kBarH - 1, kInnerW, 1, pal_.accent, 0);
    triforce(kInset + 6, kInset + 5);
    lv_obj_t* t = text(root_, title, font(12, true), pal_.text, kInset + 30, kInset + 2);
    lv_obj_update_layout(t);
    const int sx = kInset + 30 + lv_obj_get_width(t) + 6;
    text(root_, subtitle, font(10), dim_, sx, kInset + 3, kScreenW - sx - 50);

    const auto status = platform::read_device_status();
    if (status.battery_present && status.battery_percent >= 0) {
        char b[16];
        std::snprintf(b, sizeof(b), "%d%%%s", status.battery_percent, status.battery_charging ? "+" : "");
        lv_obj_t* l = text(root_, b, font(10, true), pal_.primary, 0, 0);
        lv_obj_align(l, LV_ALIGN_TOP_RIGHT, -(kInset + 6), kInset + 3);
    }
}

void ZeroTagUi::render_footer(const std::vector<std::pair<std::string, std::string>>& hints) {
    box(root_, kInset, kFootTop, kInnerW, kFootH, pal_.bar, 0);
    box(root_, kInset, kFootTop, kInnerW, 1, pal_.border, 0);
    if (!banner_.empty()) {
        text(root_, banner_, font(10), banner_error_ ? pal_.error : pal_.primary, kInset + 6, kFootTop + 3,
             kInnerW - 12);
        return;
    }
    int x = kInset + 6;
    for (const auto& hint : hints) {
        lv_obj_t* probe = text(root_, hint.first, font(9), pal_.text, 0, 0);
        lv_obj_update_layout(probe);
        const int kw = lv_obj_get_width(probe) + 8;
        lv_obj_delete(probe);
        lv_obj_t* cap = box(root_, x, kFootTop + 3, kw, 13, pal_.surface, 2, pal_.cream, 0);
        lv_obj_t* kl = text(cap, hint.first, font(9), pal_.text, 0, 0);
        lv_obj_align(kl, LV_ALIGN_CENTER, 0, 0);
        lv_obj_t* label = text(root_, hint.second, font(9), dim_, x + kw + 3, kFootTop + 3);
        lv_obj_update_layout(label);
        x += kw + 3 + lv_obj_get_width(label) + 7;
    }
}

// ---------------------------------------------------------------------------
// Screens
// ---------------------------------------------------------------------------

void ZeroTagUi::render() {
    if (!root_) {
        return;
    }
    manual_stars_.clear(); // the objects below are deleted with the screen
    manual_bars_.clear();
    manual_chars_.clear();
    manual_scroll_first_ = -1;
    lv_obj_clean(root_);
    switch (screen_) {
    case Screen::Library:
        render_library();
        break;
    case Screen::Catalogue:
        render_catalogue();
        break;
    case Screen::Facets:
        render_facets();
        break;
    case Screen::Detail:
        render_detail();
        break;
    case Screen::Emulating:
        render_emulating();
        break;
    case Screen::Settings:
        render_settings();
        break;
    case Screen::Help:
        render_help();
        break;
    case Screen::CapSetup:
        render_cap_setup();
        break;
    case Screen::Scan:
        render_scan();
        break;
    case Screen::Manual:
        render_manual();
        break;
    }
    draw_frame();
}

void ZeroTagUi::render_library() {
    const auto items = visible_items();
    render_bar("ZEROTAG", std::string(tr(Str::LibraryTitle)) + " - " + std::to_string(library_.items().size()));
    search_field(root_, lib_query_, tr(Str::FilterPlaceholder), !lib_query_.empty(), 108);
    // Tab chip (F3): Tous / Favoris / Récents
    const bool tab_active = lib_tab_ != LibTab::All;
    lv_obj_t* tab_chip = box(root_, 122, 26, 60, 16, pal_.button, 2, tab_active ? pal_.primary : pal_.cream, 0);
    const char* tab_label = lib_tab_ == LibTab::Favorites ? tr(Str::TabFavorites)
                            : lib_tab_ == LibTab::Recent  ? tr(Str::TabRecent)
                                                          : tr(Str::TabAll);
    lv_obj_t* tab_text = text(tab_chip, tab_label, font(9, tab_active), tab_active ? pal_.primary : pal_.text, 0, 0,
                              54);
    lv_obj_align(tab_text, LV_ALIGN_CENTER, 0, 0);
    box(root_, kListW, kContentTop + 2, 2, kFootTop - kContentTop - 2, pal_.border, 0);

    if (items.empty()) {
        std::string line1 = library_.items().empty() ? tr(Str::LibraryEmpty) : tr(Str::NoResult);
        std::string line2 = !keys_ok_ ? tr(Str::KeysMissingF1) : tr(Str::LibraryHowTo);
        if (!library_.items().empty() && lib_query_.empty() && tab_active) {
            line1 = tr(lib_tab_ == LibTab::Favorites ? Str::EmptyFavorites : Str::EmptyRecent);
            line2.clear();
        }
        text(root_, line1, font(10), pal_.text, 12, 64, kListW - 24, true);
        text(root_, line2, font(9), dim_, 12, 86, kListW - 24, true);
    }

    const int first = std::min(lib_top_, std::max(0, static_cast<int>(items.size()) - 1));
    for (int row = 0; row < kRows; ++row) {
        const int idx = first + row;
        if (idx >= static_cast<int>(items.size())) {
            break;
        }
        const amiibo::Item& item = *items[static_cast<size_t>(idx)];
        list_row(kListTop + row * kRowH, idx == lib_sel_, item.entry, item_name(item),
                 item.entry ? item.entry->amiibo_series : item.uid, "",
                 state_.is_favorite(base_name(item.path)));
    }
    scrollbar(items.size(), first);

    if (lib_sel_ < static_cast<int>(items.size())) {
        const amiibo::Item& item = *items[static_cast<size_t>(lib_sel_)];
        const int px = kListW + 8;
        const bool favorite = state_.is_favorite(base_name(item.path));
        box(root_, px, 26, 36, 62, pal_.surface, 2, pal_.cream, 0);
        thumb(root_, item.entry, px + 2 + 2, 30, 54);
        text(root_, item_name(item), font(11, true), pal_.text, px + 42, 26, kScreenW - px - 42 - 8 - (favorite ? 10 : 0),
             true);
        if (favorite) {
            pixel_star(kScreenW - kInset - 9, 27, pal_.primary);
        }
        const int tw = kScreenW - px - 8;
        if (item.entry) {
            text(root_, item.entry->amiibo_series, font(9), dim_, px, 92, tw);
            text(root_, item.entry->game_series, font(9), dim_, px, 104, tw);
        }
        text(root_, item.uid, font(9), pal_.text, px, 118, tw);
        const char* sig = !item.signature_checked ? tr(Str::SigUnchecked)
                                                  : (item.signature_ok ? tr(Str::SigValid) : tr(Str::SigInvalid));
        text(root_, sig, font(9), !item.signature_checked ? dim_ : (item.signature_ok ? pal_.primary : pal_.error),
             px, 131, tw);
    }
    render_footer({{tr(Str::KeyUpDown), tr(Str::HintChoose)},
                   {tr(Str::KeyEnter), tr(Str::HintOpen)},
                   {tr(Str::KeyTab), tr(Str::HintCatalogue)},
                   {"F1", tr(Str::HintSettings)}});
}

void ZeroTagUi::render_catalogue() {
    const auto entries = visible_entries();
    // Count first: the subtitle is cut at the right when the facet value is long.
    std::string subtitle = std::to_string(entries.size()) + " / " + std::to_string(db_.entries().size());
    if (filter_on_) {
        subtitle += "  " + i18n::format(Str::FacetChip, facet_label(filter_facet_), filter_value_.c_str());
    }
    render_bar(tr(Str::CatalogueTitle), subtitle);
    search_field(root_, cat_query_, tr(Str::SearchPlaceholder), true);
    box(root_, kListW, kContentTop + 2, 2, kFootTop - kContentTop - 2, pal_.border, 0);

    if (!db_ok_) {
        text(root_, tr(Str::CatalogueUnreadable), font(10), pal_.error, 12, 64, kListW - 24);
        text(root_, clip(db_error_, 60), font(9), dim_, 12, 80, kListW - 24, true);
    } else if (entries.empty()) {
        text(root_, tr(Str::NoResult), font(10), pal_.text, 12, 64, kListW - 24);
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
        list_row(kListTop + row * kRowH, idx == cat_sel_, e, e->name, e->amiibo_series,
                 owned(e) ? tr(Str::Owned) : (is_upcoming(*e) ? tr(Str::BadgeSoon) : (e->online ? tr(Str::BadgeNew) : "")));
    }
    scrollbar(entries.size(), first);

    if (cat_sel_ < static_cast<int>(entries.size())) {
        const amiibo::Entry* e = entries[static_cast<size_t>(cat_sel_)];
        const int px = kListW + 8;
        box(root_, px, 26, 36, 62, pal_.surface, 2, pal_.cream, 0);
        thumb(root_, e, px + 2 + 2, 30, 54);
        text(root_, e->name, font(11, true), pal_.text, px + 42, 26, kScreenW - px - 42 - 8, true);
        const int tw = kScreenW - px - 8;
        text(root_, e->amiibo_series, font(9), dim_, px, 92, tw);
        lv_obj_t* note = box(root_, px, 106, tw, 40, pal_.button, 2, pal_.border, 0);
        const int nw = tw - 10;
        if (!keys_ok_) {
            text(note, tr(Str::NoteKeysTitle), font(9, true), pal_.warning, 3, 1, nw);
            text(note, tr(Str::NoteKeysBody), font(9), dim_, 3, 13, nw, true);
        } else if (owned(e)) {
            text(note, tr(Str::NoteOwnedTitle), font(9, true), pal_.primary, 3, 1, nw);
            text(note, tr(Str::NoteOwnedBody), font(9), dim_, 3, 13, nw, true);
        } else {
            text(note, tr(Str::NoteNewTitle), font(9, true), pal_.warning, 3, 1, nw);
            text(note, tr(Str::NoteNewBody), font(9), dim_, 3, 13, nw, true);
        }
    }
    render_footer({{tr(Str::KeyUpDown), tr(Str::HintChoose)},
                   {tr(Str::KeyEnter), tr(Str::HintCreate)},
                   {tr(Str::KeyTab), tr(Str::HintLibrary)},
                   {"F2", tr(Str::HintFilter)}});
}

// ---- catalogue facets: screen ----

void ZeroTagUi::facet_row(int y, bool selected, const std::string& title, const std::string& right) {
    if (selected) {
        box(root_, kInset + 1, y, kInnerW - 10, 28, lv_color_hex(0x2a1417), 0);
        cursor(kInset + 4, y + 10);
    }
    text(root_, title, font(11, selected), pal_.text, 24, y + 8, right.empty() ? kInnerW - 40 : 190);
    if (!right.empty()) {
        lv_obj_t* v = text(root_, right, font(11, true), pal_.primary, 0, 0);
        lv_obj_align(v, LV_ALIGN_TOP_RIGHT, -(kInset + 8), y + 8);
    }
}

void ZeroTagUi::render_facets() {
    if (facet_stage_ == 0) {
        render_bar(tr(Str::FacetsTitle), "");
    } else {
        render_bar(tr(Str::FacetsTitle), facet_label(facet_browse_));
    }
    // Same geometry as the settings rows: 4 rows of 30 px, a thin bar on the right edge when the list is longer.
    constexpr int rh = 30;
    auto edge_bar = [&](size_t count, int first) {
        if (count <= static_cast<size_t>(kRows)) {
            return;
        }
        const int track = kRows * rh;
        const int bar_h = track * kRows / static_cast<int>(count);
        const int bar_y = kContentTop + 3 + (track - bar_h) * first / static_cast<int>(count - kRows);
        box(root_, kScreenW - kInset - 4, bar_y, 2, bar_h, pal_.border, 0);
    };

    if (facet_stage_ == 0) {
        for (int slot = 0; slot < kRows && slot < static_cast<int>(kFacetChoiceCount); ++slot) {
            const int i = facet_top_ + slot;
            if (i >= static_cast<int>(kFacetChoiceCount)) {
                break;
            }
            const amiibo::Facet facet = kFacetChoices[i];
            const auto values = db_.facet_counts(facet);
            facet_row(kContentTop + 3 + slot * rh, i == facet_sel_, facet_label(facet),
                      i18n::format(Str::FacetValuesCount, values.size()));
        }
        render_footer({{tr(Str::KeyUpDown), tr(Str::HintChoose)},
                       {tr(Str::KeyEnter), tr(Str::HintOpen)},
                       {tr(Str::KeyEsc), tr(Str::HintBack)}});
        return;
    }

    const auto values = db_.facet_counts(facet_browse_);
    if (values.empty()) {
        text(root_, tr(Str::NoResult), font(10), pal_.text, 12, kContentTop + 12, kScreenW - 24);
    }
    for (int slot = 0; slot < kRows; ++slot) {
        const int i = facet_top_ + slot;
        if (i >= static_cast<int>(values.size())) {
            break;
        }
        const auto& fc = values[static_cast<size_t>(i)];
        facet_row(kContentTop + 3 + slot * rh, i == facet_sel_, fc.value, std::to_string(fc.count));
    }
    edge_bar(values.size(), facet_top_);
    render_footer({{tr(Str::KeyUpDown), tr(Str::HintChoose)},
                   {tr(Str::KeyEnter), tr(Str::HintFilter)},
                   {tr(Str::KeyEsc), tr(Str::HintBack)}});
}

void ZeroTagUi::render_detail() {
    const amiibo::Item* item = nullptr;
    for (const auto& it : library_.items()) {
        if (it.path == current_path_) {
            item = &it;
        }
    }
    if (!item) {
        render_bar("ZEROTAG", tr(Str::NotFound));
        render_footer({{tr(Str::KeyEsc), tr(Str::HintBack)}});
        return;
    }
    render_bar(item_name(*item), item->entry ? item->entry->amiibo_series : "");

    box(root_, 12, 28, 84, 116, pal_.surface, 2, pal_.cream, 0);
    thumb(root_, item->entry, 12 + (84 - 55) / 2, 38, 96);

    const int x = 106;
    const int w = kScreenW - x - 10;
    const bool favorite = state_.is_favorite(base_name(item->path));
    text(root_, item_name(*item), font(14, true), pal_.text, x, 25, w - (favorite ? 14 : 0));
    if (favorite) {
        pixel_star(x + w - 7, 27, pal_.primary);
    }
    const std::string sub =
        item->entry ? item->entry->game_series + " - " + item->entry->type : std::string(tr(Str::UnknownCharacter));
    text(root_, sub, font(9), dim_, x, 44, w);

    const char* sig = !item->signature_checked ? tr(Str::SigUnchecked)
                                               : (item->signature_ok ? tr(Str::SigValid) : tr(Str::SigInvalid));
    const lv_color_t sig_color = !item->signature_checked ? dim_ : (item->signature_ok ? pal_.primary : pal_.error);
    lv_obj_t* chip1 = box(root_, x, 59, 106, 15, pal_.button, 2, sig_color, 0);
    lv_obj_t* c1 = text(chip1, sig, font(9), sig_color, 0, 0, 98);
    lv_obj_align(c1, LV_ALIGN_CENTER, 0, 0);
    lv_obj_t* chip2 = box(root_, x + 110, 59, 94, 15, pal_.button, 2, uid_rotation_ ? pal_.primary : pal_.cream, 0);
    lv_obj_t* c2 = text(chip2,
                        std::string(tr(Str::ChipRotation)) + " " + tr(uid_rotation_ ? Str::ValueOn : Str::ValueOff),
                        font(9), uid_rotation_ ? pal_.primary : pal_.text, 0, 0, 86);
    lv_obj_align(c2, LV_ALIGN_CENTER, 0, 0);

    char id[24];
    std::snprintf(id, sizeof(id), "%08x %08x", item->head, item->tail);
    const struct {
        const char* k;
        std::string v;
    } rows[] = {{"UID", item->uid}, {"Id", id}, {tr(Str::LabelFile), base_name(item->path)}};
    int y = 80;
    for (const auto& r : rows) {
        text(root_, r.k, font(9), dim_, x, y, 36);
        text(root_, clip(r.v, 30), font(9), pal_.text, x + 38, y, w - 38);
        y += 13;
    }

    button(x, 123, 108, tr(Str::ButtonEmulate), pal_.accent, pal_.cream, pal_.text, true);
    button(x + 112, 123, 54, pending_confirm_ == 1 ? tr(Str::ButtonResetAsk) : tr(Str::ButtonReset), pal_.surface,
           pending_confirm_ == 1 ? pal_.warning : pal_.cream, pal_.text, false);
    button(x + 170, 123, 40, pending_confirm_ == 2 ? tr(Str::ButtonDeleteAsk) : tr(Str::ButtonDelete), pal_.surface,
           pending_confirm_ == 2 ? pal_.error : lv_color_hex(0xa8606a), pal_.error, false);

    render_footer({{tr(Str::KeyEnter), tr(Str::HintEmulate)},
                   {"R", tr(Str::HintNewUid)},
                   {"U", tr(Str::HintRotation)},
                   {tr(Str::KeyDel), tr(Str::HintDelete)}});
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
    const lv_color_t accent = !emu_error_.empty() ? pal_.error : (selected ? pal_.primary : pal_.cream);

    render_bar(tr(Str::EmulationTitle), item ? item_name(*item) : "");

    // rings and thumbnail
    const int cx = 68, cy = 86;
    box(root_, cx - 56, cy - 56, 112, 112, pal_.background, 2, selected ? pal_.accent : pal_.dark_red, 56);
    box(root_, cx - 44, cy - 44, 88, 88, pal_.background, 2, field ? pal_.accent : pal_.dark_red, 44);
    box(root_, cx - 28, cy - 28, 56, 56, pal_.surface, 2, pal_.cream, 28);
    if (item) {
        thumb(root_, item->entry, cx - 14, cy - 22, 46);
    }

    const int x = 138;
    const int w = kScreenW - x - 10;
    if (!emu_error_.empty()) {
        text(root_, tr(Str::EmuFailed), font(12, true), pal_.error, x, 28, w, true);
        text(root_, clip(emu_error_, 90), font(9), pal_.text, x, 48, w, true);
        const bool perms = emu_error_.find("Permission") != std::string::npos ||
                           emu_error_.find("denied") != std::string::npos ||
                           emu_error_.find("open") != std::string::npos || emu_error_.find("GPIO") != std::string::npos;
        const bool overlay = emu_error_.find("spidev") != std::string::npos ||
                             emu_error_.find("No such file") != std::string::npos;
        text(root_, overlay ? tr(Str::ErrSpi) : (perms ? tr(Str::ErrPerms) : tr(Str::ErrGeneric)), font(9), dim_, x,
             88, w, true);
    } else {
        text(root_, selected ? tr(Str::EmuConnected) : (field ? tr(Str::EmuDetected) : tr(Str::EmuWaiting)),
             font(12, true), accent, x, 28, w, true);
        text(root_, selected ? tr(Str::EmuConnHelp) : tr(Str::EmuWaitHelp), font(9), dim_, x, 46, w, true);
        const uint32_t reads = running ? session_->stats().reads.load() : 0;
        const uint32_t writes = running ? session_->stats().writes.load() : 0;
        const uint32_t rotations = running ? session_->stats().rotations.load() : 0;
        const uint32_t secs = lv_tick_elaps(emu_started_) / 1000u;
        char b[48];
        text(root_, tr(Str::EmuReads), font(9), dim_, x, 90, 60);
        std::snprintf(b, sizeof(b), "%u", reads);
        text(root_, b, font(9), pal_.text, x + 64, 90);
        text(root_, tr(Str::EmuWrites), font(9), dim_, x, 102, 60);
        const std::string writes_text = writes ? i18n::format(Str::EmuWritesPending, writes) : std::to_string(writes);
        text(root_, writes_text, font(9), writes ? pal_.warning : pal_.text, x + 64, 102, w - 64);
        std::string uid_text;
        if (running) {
            const auto img = session_->current_image();
            char u[32];
            std::snprintf(u, sizeof(u), "%02X:%02X:%02X:%02X:%02X:%02X:%02X", img[0], img[1], img[2], img[4], img[5],
                          img[6], img[7]);
            uid_text = u;
        }
        text(root_, "UID", font(9), dim_, x, 114, 60);
        text(root_, uid_text, font(9), pal_.text, x + 64, 114, w - 64);
        if (uid_rotation_) {
            text(root_, tr(Str::EmuRotations), font(9), dim_, x, 126, 60);
            text(root_, std::to_string(rotations), font(9), pal_.primary, x + 64, 126);
        } else {
            text(root_, tr(Str::EmuDuration), font(9), dim_, x, 126, 60);
            std::snprintf(b, sizeof(b), "%02u:%02u", secs / 60, secs % 60);
            text(root_, b, font(9), pal_.text, x + 64, 126);
        }
    }
    if (banner_.empty()) {
        box(root_, kInset, kFootTop, kInnerW, kFootH, pal_.bar, 0);
        box(root_, kInset, kFootTop, kInnerW, 1, pal_.border, 0);
        text(root_, tr(Str::EmuFooterNote), font(9), dim_, kInset + 6, kFootTop + 3, 216);
        lv_obj_t* cap = box(root_, kScreenW - kInset - 74, kFootTop + 3, 68, 13, pal_.surface, 2, pal_.cream, 0);
        lv_obj_t* l = text(cap, tr(Str::EmuStopButton), font(9), pal_.text, 0, 0);
        lv_obj_align(l, LV_ALIGN_CENTER, 0, 0);
    } else {
        render_footer({});
    }
}

void ZeroTagUi::render_settings() {
    render_bar("ZEROTAG", tr(Str::SetupTitle));
    auto short_path = [&](const std::string& p) {
        const std::string home = home_dir();
        return (!home.empty() && p.rfind(home, 0) == 0) ? "~" + p.substr(home.size()) : p;
    };
    struct Row {
        std::string title;
        std::string detail;
        std::string value; // right side (language, rotation)
        int status;        // -1 none, 0 ok, 1 warning, 2 error
    };
    std::vector<Row> rows;
    rows.push_back({tr(Str::SettingsLanguage), "", std::string("< ") + i18n::native_name(i18n::language()) + " >", -1});
    rows.push_back({tr(Str::SettingsRotation), tr(Str::HelpRotation),
                    std::string("< ") + tr(uid_rotation_ ? Str::ValueOn : Str::ValueOff) + " >", -1});
    rows.push_back({tr(Str::SettingsSound), "", std::string("< ") + tr(sound_ ? Str::ValueOn : Str::ValueOff) + " >", -1});
    rows.push_back({tr(Str::SettingsOnline), tr(Str::OnlineHint),
                    std::string("< ") + tr(online_ ? Str::ValueOn : Str::ValueOff) + " >", -1});
    {
        std::string detail = tr(Str::UpdateHint);
        std::error_code ec;
        if (online_phase_.load() == OnlinePhase::Running) {
            detail = tr(Str::UpdateRunning);
        } else if (!online_) {
            detail = tr(Str::UpdateNeedsOn);
        } else if (fs::is_regular_file(online_path_, ec)) {
            std::size_t fresh = 0;
            for (const auto& e : db_.entries()) fresh += e.online ? 1 : 0;
            detail = i18n::format(Str::UpdateKnown, static_cast<int>(fresh));
        }
        rows.push_back({tr(Str::SettingsUpdate), detail, "", -1});
    }
    rows.push_back({tr(Str::SetupCap), cap_status_.empty() ? std::string(tr(Str::SetupNotChecked)) : clip(cap_status_, 70),
                    "", cap_ok_ ? 0 : 2});
    rows.push_back({keys_ok_ ? tr(Str::SetupKeysOk) : tr(Str::NoteKeysTitle),
                    keys_ok_ ? short_path(keys_path_) : i18n::format(Str::SetupKeysHelp, short_path(keys_path_).c_str()),
                    "", keys_ok_ ? 0 : 1});
    rows.push_back({tr(Str::SetupFolder),
                    library_ok_ ? i18n::format(Str::SetupFolderInfo, short_path(library_dir_).c_str(),
                                               library_.items().size(), db_.entries().size())
                                : i18n::format(Str::SetupFolderAbsent, short_path(library_dir_).c_str()),
                    "", library_ok_ && db_ok_ ? 0 : 1});
    rows.push_back({tr(Str::HelpTitle), tr(Str::HelpMenuHint), "", -1});
    rows.push_back({tr(Str::SettingsManual), tr(Str::ManualMenuHint), "", -1});

    const int rh = 30;
    for (int slot = 0; slot < 4; ++slot) {
        const int i = settings_top_ + slot;
        if (i >= static_cast<int>(rows.size())) {
            break;
        }
        const Row& row = rows[static_cast<size_t>(i)];
        const int y = kContentTop + 3 + slot * rh;
        const bool sel = i == settings_sel_;
        if (sel) {
            box(root_, kInset + 1, y, kInnerW - 2, rh - 2, lv_color_hex(0x2a1417), 0);
            cursor(kInset + 4, y + 10);
        }
        text(root_, row.title, font(11, sel), pal_.text, 24, y + 1, 150);
        if (!row.detail.empty()) {
            text(root_, row.detail, font(9), dim_, 24, y + 15, kInnerW - 52);
        }
        if (!row.value.empty()) {
            lv_obj_t* v = text(root_, row.value, font(11, true), pal_.primary, 0, 0);
            lv_obj_align(v, LV_ALIGN_TOP_RIGHT, -(kInset + 8), y + 7);
        }
        if (row.status >= 0) {
            const lv_color_t c = row.status == 0 ? pal_.primary : (row.status == 1 ? pal_.warning : pal_.error);
            box(root_, kScreenW - kInset - 16, y + 10, 8, 8, c, 0);
        }
    }
    if (rows.size() > 4) {
        const int track = 4 * rh;
        const int bar_h = track * 4 / static_cast<int>(rows.size());
        const int bar_y = kContentTop + 3 + (track - bar_h) * settings_top_ / static_cast<int>(rows.size() - 4);
        box(root_, kScreenW - kInset - 4, bar_y, 2, bar_h, pal_.border, 0);
    }
    render_footer({{tr(Str::KeyUpDown), tr(Str::HintChoose)},
                   {tr(Str::KeyEnter), tr(Str::HintChange)},
                   {tr(Str::KeyEsc), tr(Str::HintBack)}});
}

void ZeroTagUi::render_help() {
    render_bar("ZEROTAG", tr(Str::HelpTitle));
    std::vector<std::pair<std::string, std::string>> lines;
    switch (screen_before_help_) {
    case Screen::Catalogue:
        lines = {{tr(Str::KeyUpDown), tr(Str::HelpChoose)}, {"a-z", tr(Str::HelpTypeSearch)},
                 {tr(Str::KeyEnter), tr(Str::HelpCreate)}, {tr(Str::KeyTab), tr(Str::HelpToLibrary)},
                 {"F2", tr(Str::HelpFilter)}, {tr(Str::KeyEsc), tr(Str::HelpClearBack)}};
        break;
    case Screen::Facets:
        lines = {{tr(Str::KeyUpDown), tr(Str::HelpChoose)}, {tr(Str::KeyEnter), tr(Str::HelpFacetPick)},
                 {tr(Str::KeyEsc), tr(Str::HelpBack)}};
        break;
    case Screen::Detail:
        lines = {{tr(Str::KeyEnter), tr(Str::HelpEmulate)}, {"R", tr(Str::HelpNewUid)}, {"U", tr(Str::HelpRotation)},
                 {tr(Str::KeyDel), tr(Str::HelpDelete)}, {tr(Str::KeyEsc), tr(Str::HelpBack)},
                 {"F2", tr(Str::HintFavorite)}};
        break;
    case Screen::Emulating:
        lines = {{tr(Str::KeyEsc), tr(Str::HelpStopSave)}, {"", tr(Str::HelpAutosave)}};
        break;
    case Screen::Settings:
        lines = {{tr(Str::KeyUpDown), tr(Str::HelpChoose)}, {tr(Str::KeyEnter), tr(Str::HelpChange)},
                 {"< >", tr(Str::HelpChange)}, {tr(Str::KeyEsc), tr(Str::HelpBack)}};
        break;
    default:
        lines = {{tr(Str::KeyUpDown), tr(Str::HelpChoose)}, {tr(Str::KeyEnter), tr(Str::HelpOpen)},
                 {"a-z", tr(Str::HelpTypeFilter)}, {tr(Str::KeyTab), tr(Str::HelpToCatalogue)},
                 {"F1", tr(Str::HelpSettings)}, {"F4", tr(Str::HelpScan)},
                 {tr(Str::KeyEsc), tr(Str::HelpHoldQuit)}, {"F2", tr(Str::HelpFavTabs)}};
        break;
    }
    int y = kContentTop + 6;
    for (const auto& line : lines) {
        if (!line.first.empty()) {
            lv_obj_t* probe = text(root_, line.first, font(9), pal_.text, 0, 0);
            lv_obj_update_layout(probe);
            const int kw = lv_obj_get_width(probe) + 8;
            lv_obj_delete(probe);
            lv_obj_t* cap = box(root_, 12, y, kw, 13, pal_.surface, 2, pal_.cream, 0);
            lv_obj_t* kl = text(cap, line.first, font(9), pal_.text, 0, 0);
            lv_obj_align(kl, LV_ALIGN_CENTER, 0, 0);
        }
        text(root_, line.second, font(10), line.first.empty() ? dim_ : pal_.text, 78, y, kScreenW - 78 - 12);
        y += 17;
    }
    render_footer({{tr(Str::KeyEsc), tr(Str::HintBack)}});
}

// ---------------------------------------------------------------------------
// Manual: key reference pages over a demoscene backdrop (raster bars, starfield, sine scroller, chiptune)
// ---------------------------------------------------------------------------

namespace {

std::vector<std::string> utf8_chars(const std::string& text) {
    std::vector<std::string> out;
    for (size_t i = 0; i < text.size();) {
        const unsigned char c = static_cast<unsigned char>(text[i]);
        const size_t len = c < 0x80 ? 1 : (c >> 5) == 0x6 ? 2 : (c >> 4) == 0xE ? 3 : (c >> 3) == 0x1E ? 4 : 1;
        out.push_back(text.substr(i, len));
        i += len;
    }
    return out;
}

constexpr int kManualPages = 5;
constexpr int kScrollerTop = 130;
constexpr int kScrollSlots = 36;
constexpr int kScrollAdvance = 9;

} // namespace

void ZeroTagUi::anim_cb(lv_timer_t* timer) {
    static_cast<ZeroTagUi*>(lv_timer_get_user_data(timer))->manual_animate();
}

void ZeroTagUi::manual_enter() {
    anim_t0_ = lv_tick_get();
    if (!anim_timer_) {
        anim_timer_ = lv_timer_create(anim_cb, 50, this);
    }
    platform::audio::start_music(); // no-op when sound is off
}

void ZeroTagUi::manual_leave() {
    if (anim_timer_) {
        lv_timer_delete(anim_timer_);
        anim_timer_ = nullptr;
        platform::audio::stop_music();
    }
}

void ZeroTagUi::render_manual() {
    static const Str titles[kManualPages] = {Str::ManLibrary, Str::ManCatalogue, Str::ManEmulation, Str::ManScan,
                                             Str::ManCredits};
    render_bar("ZEROTAG", std::string(tr(titles[manual_page_])) + "  " + std::to_string(manual_page_ + 1) + "/" +
                              std::to_string(kManualPages));

    // Copper bars under the title bar: a dark / bright / dark stack that slides up and down.
    struct Palette { uint32_t edge, mid; };
    static const Palette bar_colors[3] = {{0x6b2a30, 0xe8503f}, {0x7a5a10, 0xf3c93a}, {0x6e675a, 0xf1ebd8}};
    for (int i = 0; i < 3; ++i) {
        ManualBar bar{};
        bar.phase = i * 2.1;
        bar.obj[0] = box(root_, kInset + 1, 0, kInnerW - 2, 1, lv_color_hex(bar_colors[i].edge), 0);
        bar.obj[1] = box(root_, kInset + 1, 0, kInnerW - 2, 2, lv_color_hex(bar_colors[i].mid), 0);
        bar.obj[2] = box(root_, kInset + 1, 0, kInnerW - 2, 1, lv_color_hex(bar_colors[i].edge), 0);
        manual_bars_.push_back(bar);
    }

    // Page content.
    using Line = std::pair<std::string, std::string>;
    std::vector<Line> lines;
    std::vector<std::string> paragraphs;
    switch (manual_page_) {
    case 0:
        lines = {{tr(Str::KeyUpDown), tr(Str::HelpChoose)}, {tr(Str::KeyEnter), tr(Str::HelpOpen)},
                 {"a-z", tr(Str::HelpTypeFilter)}, {tr(Str::KeyTab), tr(Str::HelpToCatalogue)},
                 {"F2 F3", tr(Str::HelpFavTabs)}, {"F4", tr(Str::HelpScan)}, {"F1", tr(Str::HelpSettings)}};
        break;
    case 1:
        lines = {{tr(Str::KeyUpDown), tr(Str::HelpChoose)}, {"a-z", tr(Str::HelpTypeSearch)},
                 {tr(Str::KeyEnter), tr(Str::HelpCreate)}, {"F2", tr(Str::HelpFilter)},
                 {tr(Str::KeyTab), tr(Str::HelpToLibrary)}, {tr(Str::KeyEsc), tr(Str::HelpClearBack)}};
        break;
    case 2:
        lines = {{tr(Str::KeyEnter), tr(Str::HelpEmulate)}, {"R", tr(Str::HelpNewUid)}, {"U", tr(Str::HelpRotation)},
                 {"F2", tr(Str::HintFavorite)}, {tr(Str::KeyDel), tr(Str::HelpDelete)},
                 {tr(Str::KeyEsc), tr(Str::HelpStopSave)}, {"", tr(Str::HelpAutosave)}};
        break;
    case 3:
        paragraphs = {tr(Str::ManScanText1), tr(Str::ManScanText2), tr(Str::ManScanText3)};
        break;
    default:
        paragraphs = {tr(Str::ManCredits1), tr(Str::ManCredits2), tr(Str::ManCredits3)};
        break;
    }
    int y = 36;
    for (const auto& line : lines) {
        if (!line.first.empty()) {
            lv_obj_t* probe = text(root_, line.first, font(9), pal_.text, 0, 0);
            lv_obj_update_layout(probe);
            const int kw = lv_obj_get_width(probe) + 8;
            lv_obj_delete(probe);
            lv_obj_t* cap = box(root_, 12, y, kw, 12, pal_.surface, 2, pal_.cream, 0);
            lv_obj_t* kl = text(cap, line.first, font(9), pal_.text, 0, 0);
            lv_obj_align(kl, LV_ALIGN_CENTER, 0, 0);
        }
        text(root_, line.second, font(9), line.first.empty() ? dim_ : pal_.text, 78, y, kScreenW - 78 - 12);
        y += 13;
    }
    int py = 40;
    for (size_t i = 0; i < paragraphs.size(); ++i) {
        text(root_, paragraphs[i], font(10, i == 0), i == 0 ? pal_.primary : pal_.text, 12, py, kScreenW - 24, true);
        py += 28;
    }

    // Sine scroller band.
    box(root_, kInset, kScrollerTop, kInnerW, kFootTop - kScrollerTop, pal_.bar, 0);
    box(root_, kInset, kScrollerTop, kInnerW, 1, pal_.accent, 0);
    // Starfield inside the scroller band only: a small dirty area keeps the animation cheap.
    static const lv_color_t star_colors[3] = {lv_color_hex(0x5a4a4a), lv_color_hex(0x8a7466), lv_color_hex(0xf1ebd8)};
    for (int i = 0; i < 14; ++i) {
        const int speed = 1 + i % 3;
        lv_obj_t* star = box(root_, 0, 0, speed == 3 ? 2 : 1, speed == 3 ? 2 : 1, star_colors[speed - 1], 0);
        manual_stars_.push_back({star, (i * 97) % 304, kScrollerTop + 3 + (i * 5) % 13, speed});
    }
    manual_scroll_ = utf8_chars(tr(Str::ManualScroller));
    manual_char_color_.assign(kScrollSlots, -1);
    for (int i = 0; i < kScrollSlots; ++i) {
        manual_chars_.push_back(text(root_, " ", font(11, true), pal_.text, 0, kScrollerTop + 3));
    }
    render_footer({{"< >", tr(Str::ManualPage)}, {tr(Str::KeyEsc), tr(Str::HintBack)}});
    manual_animate();
}

void ZeroTagUi::manual_animate() {
    if (screen_ != Screen::Manual || manual_chars_.empty()) {
        return;
    }
    const double s = lv_tick_elaps(anim_t0_) / 1000.0;

    for (const auto& star : manual_stars_) {
        const int span = 304;
        const int x = (((star.x0 - static_cast<int>(s * star.speed * 14.0)) % span) + span) % span + 8;
        lv_obj_set_pos(star.obj, x, star.y);
    }
    for (const auto& bar : manual_bars_) {
        const int y = 26 + static_cast<int>(3.0 + 3.0 * std::sin(s * 2.0 + bar.phase));
        lv_obj_set_pos(bar.obj[0], kInset + 1, y);
        lv_obj_set_pos(bar.obj[1], kInset + 1, y + 1);
        lv_obj_set_pos(bar.obj[2], kInset + 1, y + 3);
    }

    static const uint32_t cycle[4] = {0xf1ebd8, 0xf3c93a, 0xe8503f, 0xf08a2a};
    const int pos = static_cast<int>(s * 55.0);
    const int first = pos / kScrollAdvance;
    const int frac = pos % kScrollAdvance;
    const int n = static_cast<int>(manual_scroll_.size());
    const bool retext = first != manual_scroll_first_;
    manual_scroll_first_ = first;
    for (int i = 0; i < kScrollSlots; ++i) {
        lv_obj_t* c = manual_chars_[static_cast<size_t>(i)];
        if (retext && n > 0) {
            lv_label_set_text(c, manual_scroll_[static_cast<size_t>((first + i) % n)].c_str());
        }
        lv_obj_set_pos(c, kInset + i * kScrollAdvance - frac, kScrollerTop + 3 + static_cast<int>(2.0 * std::sin(s * 4.0 + i * 0.45)));
        const int color_index = (first + i + static_cast<int>(s * 4.0)) % 4;
        if (manual_char_color_[static_cast<size_t>(i)] != color_index) {
            manual_char_color_[static_cast<size_t>(i)] = color_index;
            lv_obj_set_style_text_color(c, lv_color_hex(cycle[color_index]), 0);
        }
    }
}

void ZeroTagUi::render_cap_setup() {
    render_bar("ZEROTAG", tr(Str::CapSetupTitle));
    text(root_, tr(Str::CapSetupTitle), font(12, true), pal_.warning, 14, kContentTop + 8, kScreenW - 28);
    text(root_, tr(Str::CapSetupText), font(10), pal_.text, 14, kContentTop + 28, kScreenW - 28, true);

    text(root_, tr(Str::CapPassword), font(9), dim_, 14, 90, 80);
    lv_obj_t* field = box(root_, 14, 102, kScreenW - 28, 18, pal_.button, 2, cap_busy_ ? pal_.border : pal_.accent, 0);
    // One asterisk per character: the password itself is never drawn.
    text(field, std::string(cap_password_.size(), '*') + (cap_busy_ ? "" : "_"), font(12, true), pal_.text, 4, 0,
         kScreenW - 44);
    if (cap_busy_) {
        text(root_, tr(Str::CapBusy), font(10), pal_.primary, 14, 124, kScreenW - 28);
    } else if (!cap_message_.empty()) {
        text(root_, cap_message_, font(10), pal_.error, 14, 124, kScreenW - 28, true);
    }
    render_footer({{tr(Str::KeyEnter), tr(Str::HintValidate)}, {tr(Str::KeyEsc), tr(Str::HintSkip)}});
}

void ZeroTagUi::render_scan() {
    const ScanPhase phase = scan_phase_.load();
    const bool reading = phase == ScanPhase::Reading || phase == ScanPhase::Done;
    const bool saved = phase == ScanPhase::Saved;
    render_bar(tr(Str::ScanTitle), "NTAG215");

    // Rings as on the emulation screen: red while the figure is read, gold once it is saved.
    const int cx = 68, cy = 86;
    box(root_, cx - 56, cy - 56, 112, 112, pal_.background, 2, reading ? pal_.accent : pal_.dark_red, 56);
    const lv_color_t inner_ring = saved ? pal_.primary : (reading ? pal_.accent : pal_.dark_red);
    box(root_, cx - 44, cy - 44, 88, 88, pal_.background, 2, inner_ring, 44);
    box(root_, cx - 28, cy - 28, 56, 56, pal_.surface, 2, pal_.cream, 28);
    if (saved && scan_entry_) {
        thumb(root_, scan_entry_, cx - 14, cy - 22, 46);
    } else {
        triforce(cx - 9, cy - 4);
    }

    const int x = 138;
    const int w = kScreenW - x - 10;
    switch (phase) {
    case ScanPhase::Opening:
        text(root_, tr(Str::ScanOpening), font(12, true), pal_.text, x, 28, w, true);
        break;
    case ScanPhase::Waiting: {
        text(root_, tr(Str::ScanWaiting), font(12, true), pal_.text, x, 28, w, true);
        text(root_, tr(Str::ScanWaitHelp), font(9), dim_, x, 46, w, true);
        const ScanIssue issue = scan_issue_.load();
        if (issue == ScanIssue::NotNtag215) {
            text(root_, tr(Str::ScanNotNtag), font(9), pal_.error, x, 92, w, true);
        } else if (issue == ScanIssue::Interrupted) {
            text(root_, i18n::format(Str::ScanReadFailed, scan_issue_page_.load()), font(9), pal_.error, x, 92, w,
                 true);
        }
        break;
    }
    case ScanPhase::Reading:
    case ScanPhase::Done: {
        const int progress = scan_progress_.load();
        text(root_, tr(Str::ScanReading), font(12, true), pal_.warning, x, 28, w, true);
        box(root_, x, 60, w, 8, pal_.button, 2, pal_.border, 0);
        const int inner = w - 4;
        if (progress > 0) {
            box(root_, x + 2, 62, inner * progress / kNtagPages, 4, pal_.primary, 0);
        }
        text(root_, i18n::format(Str::ScanProgress, progress, kNtagPages), font(9), pal_.text, x, 74, w);
        text(root_, tr(Str::ScanKeepHelp), font(9), dim_, x, 90, w, true);
        break;
    }
    case ScanPhase::Saved:
        text(root_, tr(Str::ScanSaved), font(12, true), pal_.primary, x, 28, w, true);
        text(root_, scan_entry_ ? scan_entry_->name : std::string(tr(Str::UnknownCharacter)), font(11, true),
             pal_.text, x, 48, w);
        text(root_, base_name(scan_path_), font(9), dim_, x, 66, w);
        text(root_, "UID " + scan_uid_, font(9), pal_.text, x, 80, w);
        break;
    case ScanPhase::Failed: {
        std::string message;
        {
            std::lock_guard<std::mutex> lock(scan_mutex_);
            message = scan_error_;
        }
        text(root_, tr(Str::ScanFailed), font(12, true), pal_.error, x, 28, w, true);
        text(root_, clip(message, 90), font(9), pal_.text, x, 48, w, true);
        const bool overlay = message.find("spidev") != std::string::npos ||
                             message.find("No such file") != std::string::npos;
        const bool perms = message.find("Permission") != std::string::npos ||
                           message.find("denied") != std::string::npos || message.find("GPIO") != std::string::npos;
        text(root_, overlay ? tr(Str::ErrSpi) : (perms ? tr(Str::ErrPerms) : tr(Str::ErrGeneric)), font(9), dim_, x,
             92, w, true);
        break;
    }
    }
    render_footer({{tr(Str::KeyEsc), tr(Str::HintBack)}});
}

} // namespace app
