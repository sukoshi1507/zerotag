/*
 * SPDX-License-Identifier: MIT
 *
 * ZeroTag user interface: library of amiibo backups, catalogue, detail, emulation and status screens
 * (320x170, keyboard driven). Written directly on LVGL: each screen is rebuilt from the state on change.
 */

#pragma once

#include "amiibo_db.h"
#include "amiibo_library.h"
#include "asset_manager.h"
#include "library_state.h"
#include "emulation_session.h"
#include "lvgl.h"
#include "st25r3916.h"

#include <atomic>
#include <cstdint>
#include <functional>
#include <map>
#include <memory>
#include <mutex>
#include <string>
#include <thread>
#include <utility>
#include <vector>

namespace app {

// Famicom-inspired palette: cream, Famicom red, gold on a dark warm background.
struct UiPalette {
    lv_color_t background, surface, bar, button, border; // surfaces and separators
    lv_color_t text, cream, dark_red;
    lv_color_t accent;  // Famicom red: cursor, primary button, frame
    lv_color_t primary; // gold: positive state, highlights
    lv_color_t warning, error, info;
};

// Phases of the scan screen (worker thread -> UI thread) and the last problem met while reading.
enum class ScanPhase { Opening, Waiting, Reading, Done, Saved, Failed };
enum class ScanIssue { None, NotNtag215, Interrupted };

class ZeroTagUi {
public:
    ZeroTagUi(AssetManager& assets, bool dark_mode, std::function<void()> request_quit,
              std::function<void(const std::string&, const std::string&)> persist_setting = {});
    ~ZeroTagUi();

    ZeroTagUi(const ZeroTagUi&) = delete;
    ZeroTagUi& operator=(const ZeroTagUi&) = delete;

    void start();
    void set_uid_rotation(bool enabled) { uid_rotation_ = enabled; }
    void set_sound(bool enabled);

    // Called by the platform key router. Returns true when the key was used.
    bool on_key(uint32_t key, bool long_pressed);
    void on_key_release(uint32_t key);

private:
    enum class Screen { Library, Catalogue, Facets, Detail, Emulating, Settings, Help, CapSetup, Scan, Manual };
    // Library tabs (F3 cycles): all backups (favourites first), favourites only, recently emulated.
    enum class LibTab { All = 0, Favorites = 1, Recent = 2, Count = 3 };

    // ---- data ----
    void load_data();
    void rescan_library();
    std::vector<const amiibo::Item*> visible_items() const;
    std::vector<const amiibo::Entry*> visible_entries() const;
    static std::string item_name(const amiibo::Item& item);
    // ---- catalogue facets (browse by series / game / type) ----
    void apply_facet_filter(amiibo::Facet facet, const std::string& value);
    bool handle_facets_key(uint32_t key);
    void facets_back();
    static std::string base_name(const std::string& path);
    static std::string home_dir();
    // favourites and recent list (F2 / emulation start)
    void toggle_favorite(const amiibo::Item& item);
    void record_recent(const std::string& path);
    bool persist_library_state(std::string& error);
    void cycle_library_tab();
    int find_visible_index(const std::string& path) const;

    // ---- actions ----
    void go(Screen screen);
    void say(const std::string& text, bool error = false);
    void create_blank(const amiibo::Entry& entry);
    void delete_item(const amiibo::Item& item);
    void reset_item(const amiibo::Item& item);
    void start_emulation(const amiibo::Item& item);
    void stop_emulation();
    void tick();
    void check_hardware();
    void cycle_language(int direction);
    void toggle_rotation();
    void toggle_sound();
    void open_cap_setup(Screen after);
    void submit_cap_setup();

    // ---- scan: reads a real NTAG215 into the library (F2 from the library)
    void start_scan_if_idle();
    void start_scan();
    void stop_scan();
    void scan_worker();
    void finish_scan(); // UI thread: joins the worker, writes the backup, rescans the library

    bool handle_text_key(uint32_t key, std::string& query);
    void move_selection(int delta, size_t count, int& selected, int& top, int rows);

    // ---- rendering ----
    void render();
    void render_bar(const std::string& title, const std::string& subtitle);
    void render_footer(const std::vector<std::pair<std::string, std::string>>& hints);
    void render_library();
    void render_catalogue();
    void render_facets(); // ---- catalogue facets ----
    void facet_row(int y, bool selected, const std::string& title, const std::string& right);
    void render_detail();
    void render_emulating();
    void render_settings();
    void render_help();
    void render_cap_setup();
    void render_scan();
    void render_manual();

    lv_obj_t* box(lv_obj_t* parent, int x, int y, int w, int h, lv_color_t fill, int border_w = 0,
                  lv_color_t border = lv_color_black(), int radius = 0);
    lv_obj_t* text(lv_obj_t* parent, const std::string& value, const lv_font_t* font, lv_color_t color, int x,
                   int y, int w = 0, bool wrap = false);
    lv_obj_t* thumb(lv_obj_t* parent, const amiibo::Entry* entry, int x, int y, int h);
    void search_field(lv_obj_t* parent, const std::string& query, const std::string& placeholder, bool focused,
                      int width = 172);
    void list_row(int y, bool selected, const amiibo::Entry* entry, const std::string& name, const std::string& sub,
                  const std::string& badge, bool favorite = false);
    void pixel_star(int x, int y, lv_color_t color);
    void scrollbar(size_t count, int first);
    void triforce(int x, int y);
    void cursor(int x, int y);
    void draw_frame();
    void button(int x, int y, int w, const std::string& label, lv_color_t fill, lv_color_t border, lv_color_t color,
                bool bold);
    const lv_font_t* font(int px, bool bold = false);

    static void tick_cb(lv_timer_t* timer);

    AssetManager& assets_;
    bool dark_;
    std::function<void()> request_quit_;
    std::function<void(const std::string&, const std::string&)> persist_setting_;
    UiPalette pal_{};
    lv_color_t dim_{};
    lv_obj_t* root_{nullptr};
    lv_timer_t* timer_{nullptr};
    bool sound_{true};

    // ---- manual (demoscene screen): the animated objects are rebuilt with the page, moved by anim_timer_
    void manual_enter();
    void manual_leave();
    void manual_animate();
    static void anim_cb(lv_timer_t* timer);
    struct ManualStar { lv_obj_t* obj; int x0; int y; int speed; };
    struct ManualBar { lv_obj_t* obj[3]; double phase; };
    lv_timer_t* anim_timer_{nullptr};
    uint32_t anim_t0_{0};
    int manual_page_{0};
    std::vector<ManualStar> manual_stars_;
    std::vector<ManualBar> manual_bars_;
    std::vector<lv_obj_t*> manual_chars_;
    std::vector<int> manual_char_color_;
    std::vector<std::string> manual_scroll_;
    int manual_scroll_first_{-1};
    std::map<int, const lv_font_t*> fonts_;

    // paths
    std::string db_path_;
    std::string images_dir_;
    std::string library_dir_;
    std::string keys_path_;
    std::string state_path_; // favourites and recent list, ~/.zerotag/library_state.txt

    // data
    amiibo::Database db_;
    bool db_ok_{false};
    std::string db_error_;
    amiibo::Library library_;
    bool library_ok_{false};
    std::string library_error_;
    std::unique_ptr<amiibo::Keys> keys_;
    bool keys_ok_{false};
    std::string keys_error_;
    amiibo::LibraryState state_;
    LibTab lib_tab_{LibTab::All};

    // ui state
    Screen screen_{Screen::Library};
    Screen screen_before_settings_{Screen::Library};
    std::string lib_query_;
    std::string cat_query_;
    int lib_sel_{0};
    int lib_top_{0};
    int cat_sel_{0};
    int cat_top_{0};
    // catalogue facets: the Facets screen browses a facet (stage 0 picks it, stage 1 picks a value) and a
    // chosen value restricts the catalogue, combined with cat_query_
    int facet_stage_{0};
    amiibo::Facet facet_browse_{amiibo::Facet::AmiiboSeries};
    int facet_sel_{0};
    int facet_top_{0};
    bool filter_on_{false};
    amiibo::Facet filter_facet_{amiibo::Facet::AmiiboSeries};
    std::string filter_value_;
    std::string current_path_; // the item shown in Detail / emulated
    std::string banner_;
    bool banner_error_{false};
    uint32_t banner_until_{0};
    int pending_confirm_{0}; // 1: reset, 2: delete (second press confirms)
    // Cap setup (sudo password, never stored: wiped after use)
    std::string cap_password_;
    std::string cap_message_;
    bool cap_busy_{false};
    Screen screen_after_cap_{Screen::Library};
    int settings_sel_{0};
    int settings_top_{0};
    Screen screen_before_help_{Screen::Library};
    bool uid_rotation_{false};
    std::string cap_status_;
    bool cap_ok_{false};

    // keys
    bool esc_down_{false};
    bool esc_handled_{false};

    // emulation
    std::unique_ptr<platform::St25r3916> chip_;
    std::unique_ptr<platform::EmulationSession> session_;
    std::string emu_error_;
    uint32_t emu_started_{0};
    uint32_t last_stats_key_{0};

    // scan: the worker thread only touches the atomics and scan_image_/scan_error_ (under scan_mutex_)
    std::thread scan_thread_;
    std::mutex scan_mutex_;
    std::atomic<bool> scan_stop_{false};
    bool scan_error_sounded_{false};
    std::atomic<ScanPhase> scan_phase_{ScanPhase::Opening};
    std::atomic<ScanIssue> scan_issue_{ScanIssue::None};
    std::atomic<int> scan_issue_page_{0};
    std::atomic<int> scan_progress_{0};
    std::string scan_error_;        // why the chip could not be opened or the backup not written
    amiibo::TagImage scan_image_{}; // the image read by the worker, valid once the phase is Done
    const amiibo::Entry* scan_entry_{nullptr};
    std::string scan_path_;
    std::string scan_uid_;
};

} // namespace app
