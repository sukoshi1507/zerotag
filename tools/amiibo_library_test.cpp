/*
 * SPDX-License-Identifier: MIT
 *
 * Host test of the catalogue loader and the backup library.
 *   usage: amiibo_library_test <amiibo_db.json> <key_retail.bin>
 * Works in a temporary folder (under $TMPDIR) whose files are overwritten before removal.
 * Keys and images are never printed.
 */

#include "amiibo_library.h"

#include <cstdio>
#include <cstdlib>
#include <filesystem>
#include <fstream>

#include <sys/stat.h>
#include <unistd.h>

namespace fs = std::filesystem;

static int failures = 0;
static void check(bool ok, const char* what) {
    std::printf("%-58s %s\n", what, ok ? "ok" : "FAIL");
    failures += ok ? 0 : 1;
}

static void wipe(const fs::path& dir) {
    std::error_code ec;
    for (const auto& de : fs::directory_iterator(dir, ec)) {
        if (de.is_regular_file(ec)) {
            std::ofstream f(de.path(), std::ios::binary | std::ios::trunc);
            const std::string zeros(540, '\0');
            f.write(zeros.data(), static_cast<std::streamsize>(zeros.size()));
        }
    }
    fs::remove_all(dir, ec);
}

int main(int argc, char** argv) {
    if (argc < 3) {
        std::fprintf(stderr, "usage: %s <amiibo_db.json> <key_retail.bin>\n", argv[0]);
        return 2;
    }
    std::string error;

    amiibo::Database db;
    check(db.load(argv[1], error), "catalogue loads");
    check(db.entries().size() == 954, "954 entries");
    const amiibo::Entry* zelda = db.find(0x01010000, 0x000e0002);
    check(zelda != nullptr && zelda->name == "Zelda" && zelda->amiibo_series == "Super Smash Bros.",
          "find Zelda (Smash) by head/tail");
    check(db.find(0xDEADBEEF, 0x12345678) == nullptr, "unknown id is not found");
    check(db.search("").size() == db.entries().size(), "empty search returns everything");
    check(!db.search("zelda smash").empty(), "search 'zelda smash'");
    check(!db.search("ZELDA").empty() && db.search("zzzznope").empty(), "search is case-insensitive, misses are empty");
    check(db.load("/nonexistent.json", error) == false, "missing catalogue is reported");
    check(db.load(argv[1], error), "catalogue reloads");
    zelda = db.find(0x01010000, 0x000e0002);

    amiibo::Keys keys;
    check(keys.load(argv[2], error), "keys load");

    const fs::path tmp = fs::temp_directory_path() / ("zerotag_lib_test_" + std::to_string(::getpid()));
    fs::create_directories(tmp);

    amiibo::Library lib;
    check(lib.scan(tmp.string(), db, &keys, error) && lib.items().empty(), "empty folder scans");
    std::string path;
    check(zelda && lib.create(keys, *zelda, path, error), "create a Zelda backup");
    check(lib.create(keys, *zelda, path, error), "second create gets another UID and file");

    amiibo::Library lib2;
    check(lib2.scan(tmp.string(), db, &keys, error) && !lib2.items().empty(), "rescan finds the backups");
    const amiibo::Item& item = lib2.items().front();
    check(item.entry == zelda && item.signature_checked && item.signature_ok, "identified as Zelda, signatures ok");

    amiibo::Library lib3;
    check(lib3.scan(tmp.string(), db, nullptr, error) && lib3.items().front().entry == zelda &&
              !lib3.items().front().signature_checked,
          "identified without keys");
    check(!lib2.search("zelda").empty() && lib2.search("mario").empty(), "library search");

    // Write-back keeps what the console wrote.
    amiibo::TagImage image;
    check(amiibo::Library::read_image(item.path, image, error), "read image");
    image[0x100] ^= 0x5A;
    check(amiibo::Library::write_image(item.path, image, error), "write image back");
    amiibo::TagImage again;
    check(amiibo::Library::read_image(item.path, again, error) && again == image, "written image reads back");
    struct stat st {};
    check(::stat(item.path.c_str(), &st) == 0 && (st.st_mode & 0777) == 0600, "mode is 0600");
    check(!fs::exists(item.path + ".tmp"), "no temporary file left");

    // A modified payload no longer verifies, but the character is still identified.
    amiibo::Library lib4;
    lib4.scan(tmp.string(), db, &keys, error);
    bool found_bad = false;
    for (const auto& it : lib4.items()) {
        found_bad |= (it.path == item.path && it.signature_checked && !it.signature_ok && it.entry == zelda);
    }
    check(found_bad, "tampered backup: signature fails, still identified");

    // Files of the wrong size are ignored.
    {
        std::ofstream f(tmp / "junk.bin", std::ios::binary);
        f << "not an amiibo";
    }
    amiibo::Library lib5;
    lib5.scan(tmp.string(), db, nullptr, error);
    check(lib5.items().size() == lib4.items().size(), "wrong-size .bin is ignored");

    wipe(tmp);
    std::printf("%s\n", failures ? "FAILED" : "all passed");
    return failures ? 1 : 0;
}
