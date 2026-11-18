/*
 * SPDX-License-Identifier: MIT
 *
 * Hardware probe: powers the Cap, reads the ST25R3916 identity, optionally polls for an NFC-A tag.
 * Needs root and the spi0-spidev2-gpio22-overlay overlay loaded.
 *
 *   st25r3916_probe            identity only
 *   st25r3916_probe <seconds>  also poll for a tag for that long
 *   st25r3916_probe <seconds> <file>  also write the 540-byte NTAG215 image of the tag to <file>
 */

#include "st25r3916.h"

#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <vector>
#include <thread>

int main(int argc, char** argv) {
    platform::St25r3916 chip;
    std::string error;
    if (!chip.open(error)) {
        std::fprintf(stderr, "open failed: %s\n", error.c_str());
        return 1;
    }
    std::printf("ST25R3916 identity=0x%02X revision=%d\n", chip.ic_identity(), chip.revision());

    const int seconds = argc > 1 ? std::atoi(argv[1]) : 0;
    if (seconds <= 0) {
        return 0;
    }

    if (!chip.init_nfc_a(error)) {
        std::fprintf(stderr, "init failed: %s\n", error.c_str());
        return 2;
    }
    std::printf("NFC-A reader ready, polling for %d s...\n", seconds);

    const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(seconds);
    std::string last_error;
    while (std::chrono::steady_clock::now() < deadline) {
        platform::St25r3916::Tag tag;
        if (chip.detect_tag(tag, error)) {
            std::printf("TAG: ATQA=0x%04X SAK=0x%02X UID(%u)=", tag.atqa, tag.sak, tag.uid_len);
            for (unsigned i = 0; i < tag.uid_len; ++i) {
                std::printf("%02X%s", tag.uid[i], i + 1 < tag.uid_len ? ":" : "\n");
            }
            uint8_t version[8];
            if (chip.type2_get_version(version)) {
                std::printf("GET_VERSION:");
                for (uint8_t b : version) {
                    std::printf(" %02X", b);
                }
                std::printf("\n");
            } else {
                std::printf("GET_VERSION: no answer (not an NTAG?)\n");
            }
            uint8_t page[16];
            for (uint8_t first : {0, 3}) {
                if (chip.type2_read(first, page)) {
                    std::printf("READ pages %u-%u:", first, first + 3);
                    for (uint8_t b : page) {
                        std::printf(" %02X", b);
                    }
                    std::printf("\n");
                } else {
                    std::printf("READ page %u failed\n", first);
                }
            }
            if (argc > 2) {
                std::vector<uint8_t> image;
                for (uint8_t first = 0; first <= 128; first = static_cast<uint8_t>(first + 4)) {
                    if (!chip.type2_read(first, page)) {
                        std::fprintf(stderr, "dump: READ page %u failed\n", first);
                        return 4;
                    }
                    image.insert(image.end(), page, page + 16);
                }
                image.resize(540); // pages 130-134 (locks, password) are not signed and not needed
                std::FILE* f = std::fopen(argv[2], "wb");
                if (!f || std::fwrite(image.data(), 1, image.size(), f) != image.size()) {
                    std::fprintf(stderr, "dump: cannot write %s\n", argv[2]);
                    return 5;
                }
                std::fclose(f);
                std::printf("dumped %zu bytes to %s\n", image.size(), argv[2]);
            }
            return 0;
        }
        if (error != "no tag" && error != last_error) {
            std::printf("  (%s)\n", error.c_str());
            last_error = error;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(100));
    }
    std::printf("no tag detected\n");
    return 3;
}
