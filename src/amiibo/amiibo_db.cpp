/*
 * SPDX-License-Identifier: MIT
 */

#include "amiibo_db.h"

#include "amiibo_image.h"

#include <algorithm>
#include <cctype>
#include <fstream>
#include <map>
#include <sstream>
#include <tuple>

namespace amiibo {
namespace {

// Minimal JSON reader for the catalogue: {"amiibo": [ {"key": "string", ...}, ... ], ...}.
// Objects inside the array are read as string maps (other value types are skipped); anything else
// at the top level is skipped.
class Reader {
public:
    explicit Reader(const std::string& text) : s_(text) {}

    bool parse(std::vector<std::map<std::string, std::string>>& out, std::string& error) {
        ws();
        if (!eat('{')) return fail(error, "top level is not an object");
        ws();
        if (peek() == '}') return true;
        for (;;) {
            std::string key;
            ws();
            if (!string(key)) return fail(error, "bad key");
            ws();
            if (!eat(':')) return fail(error, "missing ':'");
            ws();
            if (key == "amiibo" && peek() == '[') {
                if (!array_of_objects(out)) return fail(error, "bad amiibo array");
            } else if (!skip()) {
                return fail(error, "bad value");
            }
            ws();
            if (eat(',')) continue;
            if (eat('}')) return true;
            return fail(error, "bad object");
        }
    }

private:
    const std::string& s_;
    size_t i_{0};

    static bool fail(std::string& error, const char* what) {
        error = std::string("amiibo_db.json: ") + what;
        return false;
    }
    char peek() const { return i_ < s_.size() ? s_[i_] : '\0'; }
    bool eat(char c) {
        if (peek() == c) {
            ++i_;
            return true;
        }
        return false;
    }
    void ws() {
        while (i_ < s_.size() && std::isspace(static_cast<unsigned char>(s_[i_]))) ++i_;
    }

    static void utf8(std::string& out, uint32_t cp) {
        if (cp < 0x80) {
            out += static_cast<char>(cp);
        } else if (cp < 0x800) {
            out += static_cast<char>(0xC0 | (cp >> 6));
            out += static_cast<char>(0x80 | (cp & 0x3F));
        } else if (cp < 0x10000) {
            out += static_cast<char>(0xE0 | (cp >> 12));
            out += static_cast<char>(0x80 | ((cp >> 6) & 0x3F));
            out += static_cast<char>(0x80 | (cp & 0x3F));
        } else {
            out += static_cast<char>(0xF0 | (cp >> 18));
            out += static_cast<char>(0x80 | ((cp >> 12) & 0x3F));
            out += static_cast<char>(0x80 | ((cp >> 6) & 0x3F));
            out += static_cast<char>(0x80 | (cp & 0x3F));
        }
    }

    bool hex4(uint32_t& v) {
        if (i_ + 4 > s_.size()) return false;
        v = 0;
        for (int k = 0; k < 4; ++k) {
            const char c = s_[i_++];
            v <<= 4;
            if (c >= '0' && c <= '9') v |= static_cast<uint32_t>(c - '0');
            else if (c >= 'a' && c <= 'f') v |= static_cast<uint32_t>(c - 'a' + 10);
            else if (c >= 'A' && c <= 'F') v |= static_cast<uint32_t>(c - 'A' + 10);
            else return false;
        }
        return true;
    }

    bool string(std::string& out) {
        out.clear();
        if (!eat('"')) return false;
        while (i_ < s_.size()) {
            const char c = s_[i_++];
            if (c == '"') return true;
            if (c != '\\') {
                out += c;
                continue;
            }
            if (i_ >= s_.size()) return false;
            const char e = s_[i_++];
            switch (e) {
            case '"': out += '"'; break;
            case '\\': out += '\\'; break;
            case '/': out += '/'; break;
            case 'b': out += '\b'; break;
            case 'f': out += '\f'; break;
            case 'n': out += '\n'; break;
            case 'r': out += '\r'; break;
            case 't': out += '\t'; break;
            case 'u': {
                uint32_t cp = 0;
                if (!hex4(cp)) return false;
                if (cp >= 0xD800 && cp < 0xDC00 && i_ + 1 < s_.size() && s_[i_] == '\\' && s_[i_ + 1] == 'u') {
                    i_ += 2;
                    uint32_t lo = 0;
                    if (!hex4(lo) || lo < 0xDC00 || lo > 0xDFFF) return false;
                    cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                }
                utf8(out, cp);
                break;
            }
            default: return false;
            }
        }
        return false;
    }

    // Skips any JSON value.
    bool skip() {
        ws();
        const char c = peek();
        if (c == '"') {
            std::string tmp;
            return string(tmp);
        }
        if (c == '{' || c == '[') {
            const char close = c == '{' ? '}' : ']';
            ++i_;
            ws();
            if (eat(close)) return true;
            for (;;) {
                ws();
                if (c == '{') {
                    std::string k;
                    if (!string(k)) return false;
                    ws();
                    if (!eat(':')) return false;
                }
                if (!skip()) return false;
                ws();
                if (eat(',')) continue;
                return eat(close);
            }
        }
        const size_t start = i_;
        while (i_ < s_.size() && std::string(",}] \t\r\n").find(s_[i_]) == std::string::npos) ++i_;
        return i_ > start;
    }

    // Reads the regional release dates into object["release"] as the latest non-null date (ISO dates sort as text).
    bool release_object(std::map<std::string, std::string>& object) {
        if (!eat('{')) return false;
        ws();
        if (eat('}')) return true;
        for (;;) {
            std::string region;
            ws();
            if (!string(region)) return false;
            ws();
            if (!eat(':')) return false;
            ws();
            if (peek() == '"') {
                std::string date;
                if (!string(date)) return false;
                std::string& latest = object["release"];
                if (date > latest) latest = date;
            } else if (!skip()) {
                return false;
            }
            ws();
            if (eat(',')) continue;
            return eat('}');
        }
    }

    bool array_of_objects(std::vector<std::map<std::string, std::string>>& out) {
        if (!eat('[')) return false;
        ws();
        if (eat(']')) return true;
        for (;;) {
            ws();
            if (!eat('{')) return false;
            std::map<std::string, std::string> object;
            ws();
            if (!eat('}')) {
                for (;;) {
                    std::string key;
                    ws();
                    if (!string(key)) return false;
                    ws();
                    if (!eat(':')) return false;
                    ws();
                    if (peek() == '"') {
                        std::string value;
                        if (!string(value)) return false;
                        object[key] = value;
                    } else if (key == "release" && peek() == '{') {
                        // AmiiboAPI: "release": {"au": "2026-11-12" | null, "eu": ..., "jp": ..., "na": ...}
                        if (!release_object(object)) return false;
                    } else if (!skip()) {
                        return false;
                    }
                    ws();
                    if (eat(',')) continue;
                    if (eat('}')) break;
                    return false;
                }
            }
            out.push_back(std::move(object));
            ws();
            if (eat(',')) continue;
            return eat(']');
        }
    }
};

std::string lower(std::string s) {
    std::transform(s.begin(), s.end(), s.begin(), [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
    return s;
}

std::string field(const std::map<std::string, std::string>& m, const char* key) {
    const auto it = m.find(key);
    return it == m.end() ? std::string() : it->second;
}

} // namespace

namespace {

bool read_objects(const std::string& path, std::vector<std::map<std::string, std::string>>& objects,
                  std::string& error) {
    std::ifstream f(path, std::ios::binary);
    if (!f) {
        error = "cannot open " + path;
        return false;
    }
    std::stringstream buffer;
    buffer << f.rdbuf();
    const std::string text = buffer.str();
    return Reader(text).parse(objects, error);
}

// The bundled catalogue has "id"; the raw AmiiboAPI answer has "head" and "tail" (both "0x..." strings).
bool entry_from(const std::map<std::string, std::string>& o, Entry& e) {
    e = Entry{};
    e.id = lower(field(o, "id"));
    if (e.id.empty()) {
        std::string head = lower(field(o, "head"));
        std::string tail = lower(field(o, "tail"));
        if (head.rfind("0x", 0) == 0) head.erase(0, 2);
        if (tail.rfind("0x", 0) == 0) tail.erase(0, 2);
        e.id = head + tail;
    }
    if (!parse_id(e.id, e.head, e.tail)) {
        return false;
    }
    e.name = field(o, "name");
    e.character = field(o, "character");
    e.game_series = field(o, "gameSeries");
    e.amiibo_series = field(o, "amiiboSeries");
    e.type = field(o, "type");
    e.release = field(o, "release");
    return true;
}

} // namespace

bool Database::load(const std::string& path, std::string& error) {
    entries_.clear();
    std::vector<std::map<std::string, std::string>> objects;
    if (!read_objects(path, objects, error)) {
        return false;
    }
    entries_.reserve(objects.size());
    for (const auto& o : objects) {
        Entry e;
        if (entry_from(o, e)) {
            entries_.push_back(std::move(e));
        }
    }
    if (entries_.empty()) {
        error = "no amiibo entry found in " + path;
        return false;
    }
    return true;
}

size_t Database::merge(const std::string& path, std::string& error) {
    std::vector<std::map<std::string, std::string>> objects;
    if (!read_objects(path, objects, error)) {
        return 0;
    }
    std::map<std::string, size_t> index;
    for (size_t i = 0; i < entries_.size(); ++i) index[entries_[i].id] = i;
    size_t added = 0;
    for (const auto& o : objects) {
        Entry e;
        if (!entry_from(o, e)) continue;
        const auto it = index.find(e.id);
        if (it != index.end()) {
            // Known entry: keep the bundled text, learn the release date.
            if (entries_[it->second].release.empty()) entries_[it->second].release = e.release;
            continue;
        }
        e.online = true;
        index[e.id] = entries_.size();
        entries_.push_back(std::move(e));
        ++added;
    }
    std::sort(entries_.begin(), entries_.end(), [](const Entry& a, const Entry& b) {
        return std::tie(a.amiibo_series, a.name, a.id) < std::tie(b.amiibo_series, b.name, b.id);
    });
    return added;
}

const Entry* Database::find(uint32_t head, uint32_t tail) const {
    for (const auto& e : entries_) {
        if (e.head == head && e.tail == tail) {
            return &e;
        }
    }
    return nullptr;
}

namespace {

// Lower-case and drop the accents of Latin-1 letters: "Pokémon" matches "pokemon" and the other way round.
std::string fold(const std::string& in) {
    static const char kMap[] = "aaaaaaaceeeeiiiidnooooo/ouuuuypy"; // U+00E0..U+00FF
    std::string out;
    out.reserve(in.size());
    for (size_t i = 0; i < in.size(); ++i) {
        const unsigned char c = static_cast<unsigned char>(in[i]);
        if (c == 0xC3 && i + 1 < in.size()) {
            unsigned char d = static_cast<unsigned char>(in[i + 1]);
            if (d >= 0x80 && d <= 0x9F) d = static_cast<unsigned char>(d + 0x20); // upper case letters
            if (d >= 0xA0) {
                out += kMap[d - 0xA0];
                ++i;
                continue;
            }
        }
        out += static_cast<char>(std::tolower(c));
    }
    return out;
}

std::vector<std::string> words_of(const std::string& query) {
    std::vector<std::string> words;
    std::stringstream ss(fold(query));
    std::string w;
    while (ss >> w) words.push_back(w);
    return words;
}

bool matches(const Entry& e, const std::vector<std::string>& words) {
    const std::string haystack = fold(e.name + '\n' + e.character + '\n' + e.game_series + '\n' +
                                      e.amiibo_series + '\n' + e.id);
    return std::all_of(words.begin(), words.end(), [&](const std::string& w) {
        return haystack.find(w) != std::string::npos;
    });
}

const std::string& facet_value(const Entry& e, Facet facet) {
    switch (facet) {
    case Facet::AmiiboSeries: return e.amiibo_series;
    case Facet::Game: return e.game_series;
    case Facet::Type: return e.type;
    }
    return e.type;
}

} // namespace

std::vector<const Entry*> Database::search(const std::string& query) const {
    const auto words = words_of(query);
    std::vector<const Entry*> result;
    for (const auto& e : entries_) {
        if (matches(e, words)) result.push_back(&e);
    }
    return result;
}

std::vector<FacetCount> Database::facet_counts(Facet facet) const {
    // Keyed by the lower-cased value: "Mario" and "mario" are the same facet value; the first spelling is shown.
    std::map<std::string, FacetCount> by_key;
    for (const auto& e : entries_) {
        const std::string& value = facet_value(e, facet);
        if (value.empty()) continue;
        FacetCount& fc = by_key[lower(value)];
        if (fc.value.empty()) fc.value = value;
        ++fc.count;
    }
    std::vector<FacetCount> out;
    out.reserve(by_key.size());
    for (auto& kv : by_key) out.push_back(std::move(kv.second));
    return out;
}

std::vector<const Entry*> Database::search_in(Facet facet, const std::string& value, const std::string& query) const {
    const std::string wanted = lower(value);
    const auto words = words_of(query);
    std::vector<const Entry*> result;
    if (wanted.empty()) return result;
    for (const auto& e : entries_) {
        if (lower(facet_value(e, facet)) == wanted && matches(e, words)) result.push_back(&e);
    }
    return result;
}

std::string Database::image_path(const std::string& images_dir, const Entry& entry) {
    return images_dir + "/" + entry.id + ".png";
}

} // namespace amiibo
