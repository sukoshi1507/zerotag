/*
 * SPDX-License-Identifier: MIT
 */

#pragma once

#include <cstdint>
#include <string>
#include <vector>

namespace amiibo {

// One entry of assets/amiibo/amiibo_db.json (data from AmiiboAPI).
struct Entry {
    std::string id; // 16 hex characters: head (8) + tail (8)
    std::string name;
    std::string character;
    std::string game_series;
    std::string amiibo_series;
    std::string type;
    uint32_t head{0};
    uint32_t tail{0};
};

class Database {
public:
    // Loads the catalogue. Entries with an invalid id are skipped.
    bool load(const std::string& path, std::string& error);

    const std::vector<Entry>& entries() const { return entries_; }
    const Entry* find(uint32_t head, uint32_t tail) const;

    // Case-insensitive substring search over name, character, game series and amiibo series.
    // Every whitespace-separated word of `query` has to match. An empty query returns everything.
    std::vector<const Entry*> search(const std::string& query) const;

    // `<images_dir>/<id>.png`
    static std::string image_path(const std::string& images_dir, const Entry& entry);

private:
    std::vector<Entry> entries_;
};

} // namespace amiibo
