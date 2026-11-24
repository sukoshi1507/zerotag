/*
 * SPDX-License-Identifier: MIT
 */

#include "amiibo_library.h"

#include <algorithm>
#include <cctype>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <fstream>

#include <sys/stat.h>

namespace fs = std::filesystem;

namespace amiibo {
namespace {

constexpr size_t kModelInfo = 0x54; // pages 21-22: head (4 bytes) then tail (4 bytes), big endian

uint32_t be32(const uint8_t* p) {
    return (static_cast<uint32_t>(p[0]) << 24) | (static_cast<uint32_t>(p[1]) << 16) |
           (static_cast<uint32_t>(p[2]) << 8) | p[3];
}

std::string uid_text(const TagImage& image) {
    char buf[32];
    std::snprintf(buf, sizeof(buf), "%02X:%02X:%02X:%02X:%02X:%02X:%02X", image[0], image[1], image[2], image[4],
                  image[5], image[6], image[7]);
    return buf;
}

std::string lower(std::string s) {
    std::transform(s.begin(), s.end(), s.begin(), [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
    return s;
}

std::string file_stem_for(const Entry& entry, const TagImage& image) {
    std::string stem;
    for (const char c : entry.name) {
        const unsigned char u = static_cast<unsigned char>(c);
        stem += (std::isalnum(u) != 0) ? c : '_';
    }
    if (stem.empty()) {
        stem = entry.id;
    }
    if (stem.size() > 48) {
        stem.resize(48);
    }
    char suffix[16];
    std::snprintf(suffix, sizeof(suffix), "_%02X%02X%02X%02X", image[4], image[5], image[6], image[7]);
    return stem + suffix;
}

} // namespace

bool Library::read_image(const std::string& path, TagImage& image, std::string& error) {
    std::ifstream f(path, std::ios::binary);
    if (!f) {
        error = "cannot open " + path;
        return false;
    }
    f.read(reinterpret_cast<char*>(image.data()), static_cast<std::streamsize>(image.size()));
    if (static_cast<size_t>(f.gcount()) != image.size() || f.peek() != std::ifstream::traits_type::eof()) {
        error = path + " is not a 540-byte image";
        return false;
    }
    return true;
}

bool Library::write_image(const std::string& path, const TagImage& image, std::string& error) {
    const std::string tmp = path + ".tmp";
    {
        std::ofstream f(tmp, std::ios::binary | std::ios::trunc);
        if (!f) {
            error = "cannot write " + tmp;
            return false;
        }
        f.write(reinterpret_cast<const char*>(image.data()), static_cast<std::streamsize>(image.size()));
        f.flush();
        if (!f) {
            error = "write failed for " + tmp;
            fs::remove(tmp);
            return false;
        }
    }
    ::chmod(tmp.c_str(), 0600);
    std::error_code ec;
    fs::rename(tmp, path, ec);
    if (ec) {
        error = "cannot replace " + path + ": " + ec.message();
        fs::remove(tmp, ec);
        return false;
    }
    return true;
}

bool Library::scan(const std::string& dir, const Database& db, const Keys* keys, std::string& error) {
    items_.clear();
    dir_ = dir;
    db_ = &db;
    std::error_code ec;
    if (!fs::is_directory(dir, ec)) {
        error = "not a directory: " + dir;
        return false;
    }
    for (const auto& de : fs::directory_iterator(dir, ec)) {
        if (!de.is_regular_file(ec) || de.path().extension() != ".bin" || de.file_size(ec) != kNtag215Size) {
            continue;
        }
        TagImage image;
        std::string ignored;
        if (!read_image(de.path().string(), image, ignored)) {
            continue;
        }
        Item item;
        item.path = de.path().string();
        item.head = be32(&image[kModelInfo]);
        item.tail = be32(&image[kModelInfo + 4]);
        item.uid = uid_text(image);
        if (keys != nullptr && keys->loaded()) {
            uint32_t h = 0, t = 0;
            item.signature_checked = true;
            item.signature_ok = verify(*keys, image, h, t);
            if (item.signature_ok) {
                item.head = h;
                item.tail = t;
            }
        }
        item.entry = db.find(item.head, item.tail);
        items_.push_back(std::move(item));
    }
    if (ec) {
        error = "cannot read " + dir + ": " + ec.message();
        return false;
    }
    std::sort(items_.begin(), items_.end(), [](const Item& a, const Item& b) {
        const std::string na = a.entry ? lower(a.entry->name) : "~";
        const std::string nb = b.entry ? lower(b.entry->name) : "~";
        return na != nb ? na < nb : a.path < b.path;
    });
    return true;
}

std::vector<const Item*> Library::search(const std::string& query) const {
    std::vector<const Item*> result;
    if (db_ == nullptr) {
        return result;
    }
    const auto matches = db_->search(query);
    for (const auto& item : items_) {
        if (item.entry != nullptr && std::find(matches.begin(), matches.end(), item.entry) != matches.end()) {
            result.push_back(&item);
        }
    }
    return result;
}

bool Library::create(const Keys& keys, const Entry& entry, std::string& path, std::string& error) {
    if (dir_.empty()) {
        error = "library folder not set (call scan first)";
        return false;
    }
    TagImage image;
    if (!generate(keys, entry.head, entry.tail, image, error)) {
        return false;
    }
    std::error_code ec;
    fs::create_directories(dir_, ec);
    const std::string stem = file_stem_for(entry, image);
    path = (fs::path(dir_) / (stem + ".bin")).string();
    if (fs::exists(path, ec)) {
        error = path + " already exists";
        return false;
    }
    if (!write_image(path, image, error)) {
        return false;
    }
    Item item;
    item.path = path;
    item.head = entry.head;
    item.tail = entry.tail;
    item.entry = &entry;
    item.uid = uid_text(image);
    item.signature_checked = true;
    item.signature_ok = true;
    items_.push_back(std::move(item));
    return true;
}

} // namespace amiibo
