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
    std::string release; // latest regional release date, "YYYY-MM-DD" ("" when unknown); only from the online list
    bool online{false};  // not in the bundled catalogue: learnt from the online update
    uint32_t head{0};
    uint32_t tail{0};
};

// Fields the catalogue can be browsed by.
enum class Facet : uint8_t {
    AmiiboSeries, // amiibo_series, e.g. "Super Smash Bros."
    Game,         // game_series, e.g. "The Legend of Zelda"
    Type,         // type: Figure, Card, Band, Block, Yarn
};

// One distinct value of a facet and the number of entries that have it.
struct FacetCount {
    std::string value;
    size_t count{0};
};

class Database {
public:
    // Loads the catalogue. Entries with an invalid id are skipped.
    bool load(const std::string& path, std::string& error);

    // Adds the entries of an AmiiboAPI answer (raw /api/amiibo/ JSON, or the same format as load()) that are not
    // in the catalogue yet, flagged `online`, and fills in release dates. Returns how many entries were added.
    size_t merge(const std::string& path, std::string& error);

    const std::vector<Entry>& entries() const { return entries_; }
    const Entry* find(uint32_t head, uint32_t tail) const;

    // Case- and accent-insensitive substring search over name, character, game series, amiibo series and the hex id.
    // Every whitespace-separated word of `query` has to match. An empty query returns everything.
    std::vector<const Entry*> search(const std::string& query) const;

    // Distinct non-empty values of `facet`, sorted case-insensitively, with their entry counts.
    std::vector<FacetCount> facet_counts(Facet facet) const;

    // Entries whose `facet` equals `value` (case-insensitive, exact) and that match `query` as search() does.
    std::vector<const Entry*> search_in(Facet facet, const std::string& value, const std::string& query) const;

    // `<images_dir>/<id>.png`
    static std::string image_path(const std::string& images_dir, const Entry& entry);

private:
    std::vector<Entry> entries_;
};

} // namespace amiibo
