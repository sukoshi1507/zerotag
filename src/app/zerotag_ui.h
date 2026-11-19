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
#include "emulation_session.h"
#include "lvgl.h"
#include "st25r3916.h"
#include "theme.h"

#include <cstdint>
#include <functional>
#include <map>
#include <memory>
#include <string>
#include <utility>

namespace app {

class ZeroTagUi {
public:
    ZeroTagUi(AssetManager& assets, bool dark_mode, std::function<void()> request_quit,
              std::function<void(const std::string&)> persist_language = {});
    ~ZeroTagUi();

    ZeroTagUi(const ZeroTagUi&) = delete;
    ZeroTagUi& operator=(const ZeroTagUi&) = delete;

    void start();

    // Called by the platform key router. Returns true when the key was used.
    bool on_key(uint32_t key, bool long_pressed);
    void on_key_release(uint32_t key);

private:
    enum class Screen { Library, Catalogue, Detail, Emulating, Setup };

    // ---- data ----
    void load_data();
    void rescan_library();
    std::vector<const amiibo::Item*> visible_items() const;
    std::vector<const amiibo::Entry*> visible_entries() const;
    static std::string item_name(const amiibo::Item& item);
    static std::string base_name(const std::string& path);
    static std::string home_dir();

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

    bool handle_text_key(uint32_t key, std::string& query);
    void move_selection(int delta, size_t count, int& selected, int& top, int rows);

    // ---- rendering ----
    void render();
    void render_bar(const std::string& title, const std::string& subtitle);
    void render_footer(const std::vector<std::pair<std::string, std::string>>& hints);
    void render_library();
    void render_catalogue();
    void render_detail();
    void render_emulating();
    void render_setup();

    lv_obj_t* box(lv_obj_t* parent, int x, int y, int w, int h, lv_color_t fill, int border_w = 0,
                  lv_color_t border = lv_color_black(), int radius = 0);
    lv_obj_t* text(lv_obj_t* parent, const std::string& value, const lv_font_t* font, lv_color_t color, int x,
                   int y, int w = 0, bool wrap = false);
    lv_obj_t* thumb(lv_obj_t* parent, const amiibo::Entry* entry, int x, int y, int h);
    void search_field(lv_obj_t* parent, const std::string& query, const std::string& placeholder, bool focused);
    const lv_font_t* font(int px, bool bold = false);

    static void tick_cb(lv_timer_t* timer);

    AssetManager& assets_;
    bool dark_;
    std::function<void()> request_quit_;
    std::function<void(const std::string&)> persist_language_;
    view::ThemePalette pal_{};
    lv_color_t dim_{};
    lv_obj_t* root_{nullptr};
    lv_timer_t* timer_{nullptr};
    std::map<int, const lv_font_t*> fonts_;

    // paths
    std::string db_path_;
    std::string images_dir_;
    std::string library_dir_;
    std::string keys_path_;

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

    // ui state
    Screen screen_{Screen::Library};
    Screen screen_before_setup_{Screen::Library};
    std::string lib_query_;
    std::string cat_query_;
    int lib_sel_{0};
    int lib_top_{0};
    int cat_sel_{0};
    int cat_top_{0};
    std::string current_path_; // the item shown in Detail / emulated
    std::string banner_;
    bool banner_error_{false};
    uint32_t banner_until_{0};
    int pending_confirm_{0}; // 1: reset, 2: delete (second press confirms)
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
};

} // namespace app
