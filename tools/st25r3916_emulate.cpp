/*
 * SPDX-License-Identifier: MIT
 *
 * Hardware test: emulates an NTAG215 with the ST25R3916 through an EmulationSession (the same path the
 * application uses). Needs root and the SPI overlay.
 *
 *   st25r_emulate --blank [seconds]               synthetic blank NTAG215 (random UID, NDEF "hello"), no keys
 *   st25r_emulate <image.bin> [seconds]           540-byte NTAG215 image (e.g. an amiibo image)
 *   st25r_emulate --save <image.bin> [seconds]    same, and writes the image back if the reader changed it
 *
 * Prints the live counters once per second.
 */

#include "emulation_session.h"

#include <chrono>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <random>
#include <string>
#include <thread>

using platform::EmulationSession;

static bool load(const char* path, EmulationSession::Image& image) {
    std::FILE* f = std::fopen(path, "rb");
    const bool ok = f && std::fread(image.data(), 1, image.size(), f) == image.size();
    if (f) {
        std::fclose(f);
    }
    return ok;
}

// Same write-back rule as the library: temporary file, then rename, mode 0600.
static bool save(const char* path, const EmulationSession::Image& image) {
    const std::string tmp = std::string(path) + ".tmp";
    std::FILE* f = std::fopen(tmp.c_str(), "wb");
    if (!f) {
        return false;
    }
    const bool ok = std::fwrite(image.data(), 1, image.size(), f) == image.size() && std::fflush(f) == 0;
    std::fclose(f);
    return ok && std::rename(tmp.c_str(), path) == 0;
}

int main(int argc, char** argv) {
    int arg = 1;
    const bool save_back = argc > arg && std::strcmp(argv[arg], "--save") == 0;
    if (save_back) {
        ++arg;
    }
    if (argc <= arg) {
        std::fprintf(stderr, "usage: %s [--save] --blank|<image.bin> [seconds]\n", argv[0]);
        return 1;
    }
    const char* source = argv[arg++];
    const int seconds = argc > arg ? std::atoi(argv[arg]) : 30;

    EmulationSession::Image image{};
    const bool blank = std::strcmp(source, "--blank") == 0;
    if (blank) {
        std::random_device rd;
        uint8_t uid[7] = {0x04};
        for (int i = 1; i < 7; ++i) {
            uid[i] = static_cast<uint8_t>(rd());
        }
        image[0] = uid[0]; image[1] = uid[1]; image[2] = uid[2];
        image[3] = 0x88 ^ uid[0] ^ uid[1] ^ uid[2];
        image[4] = uid[3]; image[5] = uid[4]; image[6] = uid[5]; image[7] = uid[6];
        image[8] = uid[3] ^ uid[4] ^ uid[5] ^ uid[6];
        image[9] = 0x48;
        const uint8_t cc[4] = {0xE1, 0x10, 0x3E, 0x00}; // NTAG215 capability container
        std::memcpy(&image[12], cc, 4);
        // NDEF TLV with one text record "hello" (en), so a phone has something to display.
        const uint8_t ndef[] = {0x03, 0x0C, 0xD1, 0x01, 0x08, 0x54, 0x02, 'e', 'n', 'h', 'e', 'l', 'l', 'o', 0xFE};
        std::memcpy(&image[16], ndef, sizeof(ndef));
        image[536] = 0x80; image[537] = 0x80; // PACK
    } else if (!load(source, image)) {
        std::fprintf(stderr, "cannot read a 540-byte image from %s\n", source);
        return 1;
    }
    if (blank && save_back) {
        std::fprintf(stderr, "--save needs an image file\n");
        return 1;
    }

    platform::St25r3916 chip;
    std::string error;
    if (!chip.open(error)) {
        std::fprintf(stderr, "open failed: %s\n", error.c_str());
        return 2;
    }

    EmulationSession session;
    if (!session.start(chip, image, seconds, error)) {
        std::fprintf(stderr, "start failed: %s\n", error.c_str());
        return 3;
    }
    std::printf("emulating for %d s\n", seconds);
    unsigned last_reads = ~0u, last_writes = ~0u;
    bool last_field = false, last_selected = false;
    while (session.running()) {
        const auto& st = session.stats();
        const unsigned reads = st.reads, writes = st.writes;
        const bool field = st.field, selected = st.selected;
        if (reads != last_reads || writes != last_writes || field != last_field || selected != last_selected) {
            std::printf("field=%d selected=%d reads=%u writes=%u\n", field, selected, reads, writes);
            std::fflush(stdout);
            last_reads = reads;
            last_writes = writes;
            last_field = field;
            last_selected = selected;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(200));
    }
    const auto result = session.finish();
    if (!result.ok) {
        std::fprintf(stderr, "emulation failed: %s\n", result.error.c_str());
        return 4;
    }
    std::printf("done, tag %s\n", result.changed ? "was written by the reader" : "unchanged");
    if (save_back && result.changed) {
        std::printf(save(source, result.image) ? "image saved back\n" : "could not save the image\n");
    }
    return 0;
}
