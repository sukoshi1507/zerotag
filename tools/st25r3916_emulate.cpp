/*
 * SPDX-License-Identifier: MIT
 *
 * Hardware test: emulates an NTAG215 with the ST25R3916. Needs root and the SPI overlay.
 *
 *   st25r_emulate --blank [seconds]        synthetic blank NTAG215 (random UID, empty NDEF), no keys needed
 *   st25r_emulate <image.bin> [seconds]    540-byte NTAG215 image (e.g. an amiibo image)
 */

#include "st25r3916.h"

#include <array>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <random>
#include <string>

int main(int argc, char** argv) {
    if (argc < 2) {
        std::fprintf(stderr, "usage: %s --blank|<image.bin> [seconds]\n", argv[0]);
        return 1;
    }
    std::array<uint8_t, 540> image{};
    if (std::strcmp(argv[1], "--blank") == 0) {
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
    } else {
        std::FILE* f = std::fopen(argv[1], "rb");
        if (!f || std::fread(image.data(), 1, image.size(), f) != image.size()) {
            std::fprintf(stderr, "cannot read a 540-byte image from %s\n", argv[1]);
            return 1;
        }
        std::fclose(f);
    }
    const int seconds = argc > 2 ? std::atoi(argv[2]) : 30;

    platform::St25r3916 chip;
    std::string error;
    if (!chip.open(error)) {
        std::fprintf(stderr, "open failed: %s\n", error.c_str());
        return 2;
    }
    const bool ok = chip.emulate_ntag215(
        image.data(), seconds, [t0 = std::chrono::steady_clock::now()](const std::string& s) {
            const auto ms = std::chrono::duration_cast<std::chrono::milliseconds>(std::chrono::steady_clock::now() - t0).count();
            std::printf("[%6lld ms] %s\n", static_cast<long long>(ms), s.c_str());
            std::fflush(stdout);
        }, error);
    if (!ok) {
        std::fprintf(stderr, "emulation failed: %s\n", error.c_str());
        return 3;
    }
    return 0;
}
