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

bool Database::load(const std::string& path, std::string& error) {
    entries_.clear();
    std::ifstream f(path, std::ios::binary);
    if (!f) {
        error = "cannot open " + path;
        return false;
    }
    std::stringstream buffer;
    buffer << f.rdbuf();
    const std::string text = buffer.str();

    std::vector<std::map<std::string, std::string>> objects;
    if (!Reader(text).parse(objects, error)) {
        return false;
    }
    entries_.reserve(objects.size());
    for (const auto& o : objects) {
        Entry e;
        e.id = lower(field(o, "id"));
        if (!parse_id(e.id, e.head, e.tail)) {
            continue;
        }
        e.name = field(o, "name");
        e.character = field(o, "character");
        e.game_series = field(o, "gameSeries");
        e.amiibo_series = field(o, "amiiboSeries");
        e.type = field(o, "type");
        entries_.push_back(std::move(e));
    }
    if (entries_.empty()) {
        error = "no amiibo entry found in " + path;
        return false;
    }
    return true;
}

const Entry* Database::find(uint32_t head, uint32_t tail) const {
    for (const auto& e : entries_) {
        if (e.head == head && e.tail == tail) {
            return &e;
        }
    }
    return nullptr;
}

std::vector<const Entry*> Database::search(const std::string& query) const {
    std::vector<std::string> words;
    {
        std::stringstream ss(lower(query));
        std::string w;
        while (ss >> w) words.push_back(w);
    }
    std::vector<const Entry*> result;
    for (const auto& e : entries_) {
        const std::string haystack = lower(e.name + '\n' + e.character + '\n' + e.game_series + '\n' + e.amiibo_series);
        const bool all = std::all_of(words.begin(), words.end(), [&](const std::string& w) {
            return haystack.find(w) != std::string::npos;
        });
        if (all) result.push_back(&e);
    }
    return result;
}

std::string Database::image_path(const std::string& images_dir, const Entry& entry) {
    return images_dir + "/" + entry.id + ".png";
}

} // namespace amiibo
