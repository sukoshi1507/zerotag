/*
 * SPDX-License-Identifier: MIT
 *
 * Host test of the catalogue facets (browsing by amiibo series, game, type) and of the search restricted to a
 * facet value. Needs only the catalogue, no keys.
 *   usage: amiibo_db_test <amiibo_db.json>
 */

#include "amiibo_db.h"

#include <algorithm>
#include <cctype>
#include <cstdio>
#include <string>

static int failures = 0;
static void check(bool ok, const char* what) {
    std::printf("%-70s %s\n", what, ok ? "ok" : "FAIL");
    failures += ok ? 0 : 1;
}

static std::string lowered(std::string s) {
    for (auto& c : s) c = static_cast<char>(std::tolower(static_cast<unsigned char>(c)));
    return s;
}

int main(int argc, char** argv) {
    if (argc < 2) {
        std::fprintf(stderr, "usage: %s <amiibo_db.json>\n", argv[0]);
        return 2;
    }
    amiibo::Database db;
    std::string error;
    check(db.load(argv[1], error), "catalogue loads");

    for (const amiibo::Facet facet : {amiibo::Facet::AmiiboSeries, amiibo::Facet::Game, amiibo::Facet::Type}) {
        const auto values = db.facet_counts(facet);
        size_t total = 0;
        bool sorted = true, unique = true, counts_ok = true, subset_ok = true;
        for (size_t i = 0; i < values.size(); ++i) {
            total += values[i].count;
            const auto all_of_value = db.search_in(facet, values[i].value, "");
            counts_ok &= values[i].count == all_of_value.size();
            if (i > 0) {
                const std::string a = lowered(values[i - 1].value), b = lowered(values[i].value);
                sorted &= a < b;
                unique &= a != b;
            }
            // A filtered result is a subset of the value's entries, and every hit matches the query.
            for (const amiibo::Entry* e : db.search_in(facet, values[i].value, "zelda")) {
                subset_ok &= std::find(all_of_value.begin(), all_of_value.end(), e) != all_of_value.end();
                subset_ok &= lowered(e->name + e->character + e->game_series + e->amiibo_series).find("zelda") !=
                             std::string::npos;
            }
        }
        check(!values.empty(), "facet has values");
        check(total == db.entries().size(), "facet counts add up to the catalogue");
        check(sorted && unique, "facet values are sorted and unique (case-insensitive)");
        check(counts_ok, "each count equals search_in with an empty query");
        check(subset_ok, "search_in with a query only returns matching entries of that value");
    }

    const auto games = db.facet_counts(amiibo::Facet::Game);
    const auto types = db.facet_counts(amiibo::Facet::Type);
    std::string zelda_game;
    for (const auto& g : games) {
        if (lowered(g.value).find("zelda") != std::string::npos) zelda_game = g.value;
    }
    check(!zelda_game.empty(), "a Zelda game value exists");
    check(db.search_in(amiibo::Facet::Game, lowered(zelda_game), "").size() ==
              db.search_in(amiibo::Facet::Game, zelda_game, "").size(),
          "facet value match is case-insensitive");
    check(db.search_in(amiibo::Facet::Game, "no such game", "").empty() &&
              db.search_in(amiibo::Facet::Game, "", "").empty(),
          "unknown or empty facet value matches nothing");
    bool has_figure = false, has_card = false;
    for (const auto& t : types) {
        has_figure |= t.value == "Figure";
        has_card |= t.value == "Card";
    }
    check(has_figure && has_card, "type facet has Figure and Card");

    const amiibo::Entry* zelda_smash = db.find(0x01010000, 0x000e0002);
    const auto smash = db.search_in(amiibo::Facet::AmiiboSeries, "Super Smash Bros.", "");
    const auto smash_zelda = db.search_in(amiibo::Facet::AmiiboSeries, "Super Smash Bros.", "zelda");
    check(!smash.empty() && !smash_zelda.empty() && smash_zelda.size() < smash.size(),
          "series filter combined with a text query narrows the list");
    check(zelda_smash && std::find(smash_zelda.begin(), smash_zelda.end(), zelda_smash) != smash_zelda.end(),
          "the Zelda (Smash) entry is in the Super Smash Bros. series filter");
    check(db.search("").size() == db.entries().size() && !db.search("zelda smash").empty(),
          "plain search unchanged");

    std::printf("%s\n", failures ? "FAILED" : "all passed");
    return failures ? 1 : 0;
}
