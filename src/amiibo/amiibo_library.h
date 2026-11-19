/*
 * SPDX-License-Identifier: MIT
 */

#pragma once

#include "amiibo_db.h"
#include "amiibo_image.h"

#include <cstdint>
#include <string>
#include <vector>

namespace amiibo {

// A backup found in the library folder.
struct Item {
    std::string path;
    uint32_t head{0};
    uint32_t tail{0};
    const Entry* entry{nullptr}; // nullptr when the character is not in the catalogue
    bool signature_checked{false}; // keys were available
    bool signature_ok{false};      // meaningful only when signature_checked
    std::string uid;               // "04:AA:..." for display
};

// A folder of 540-byte NTAG215 backups. Characters are identified from the model info stored in clear
// in the image (pages 21-22), so no key is needed to list a collection; the keys, when given, are only
// used to check the signatures.
class Library {
public:
    // Scans `dir` (not recursive) for *.bin files of 540 bytes. `db` must outlive the library.
    bool scan(const std::string& dir, const Database& db, const Keys* keys, std::string& error);

    const std::vector<Item>& items() const { return items_; }
    const std::string& dir() const { return dir_; }

    // Case-insensitive search over the catalogue fields of the identified items.
    std::vector<const Item*> search(const std::string& query) const;

    // Generates a blank signed image for `entry`, stores it in the folder and returns its path.
    bool create(const Keys& keys, const Entry& entry, std::string& path, std::string& error);

    static bool read_image(const std::string& path, TagImage& image, std::string& error);
    // Atomic (temporary file + rename), mode 0600. Used to keep what a console wrote during emulation.
    static bool write_image(const std::string& path, const TagImage& image, std::string& error);

private:
    std::string dir_;
    const Database* db_{nullptr};
    std::vector<Item> items_;
};

} // namespace amiibo
