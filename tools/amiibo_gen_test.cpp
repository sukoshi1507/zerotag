/*
 * SPDX-License-Identifier: MIT
 *
 * Host test of the amiibo image generator.  usage: amiibo_gen_test <key_retail.bin> [id]
 * Generates an image, verifies its signatures with the same keys and checks the NTAG215 structure.
 * Keys and full images are never printed.
 */

#include "amiibo_image.h"

#include <cstdio>

int main(int argc, char** argv) {
    if (argc < 2) {
        std::fprintf(stderr, "usage: %s <key_retail.bin> [16-hex-char id]\n", argv[0]);
        return 2;
    }
    const std::string id = argc > 2 ? argv[2] : "01010000000e0002";
    uint32_t head = 0, tail = 0;
    if (!amiibo::parse_id(id, head, tail)) {
        std::fprintf(stderr, "bad id\n");
        return 2;
    }

    amiibo::Keys keys;
    std::string error;
    if (!keys.load(argv[1], error)) {
        std::fprintf(stderr, "%s\n", error.c_str());
        return 1;
    }

    amiibo::TagImage image;
    if (!amiibo::generate(keys, head, tail, image, error)) {
        std::fprintf(stderr, "generate failed: %s\n", error.c_str());
        return 1;
    }

    int failures = 0;
    auto check = [&](bool ok, const char* what) {
        std::printf("%-46s %s\n", what, ok ? "ok" : "FAIL");
        failures += ok ? 0 : 1;
    };

    uint32_t h = 0, t = 0;
    check(amiibo::verify(keys, image, h, t), "signatures verify with the same keys");
    check(h == head && t == tail, "head/tail recovered after decrypt");
    check(image[0] == 0x04, "UID0 is the NXP byte");
    check(image[3] == static_cast<uint8_t>(0x88 ^ image[0] ^ image[1] ^ image[2]), "BCC0 correct");
    check(image[8] == static_cast<uint8_t>(image[4] ^ image[5] ^ image[6] ^ image[7]), "BCC1 correct");
    check(image[9] == 0x48 && image[10] == 0x0F && image[11] == 0xE0, "static lock bytes");
    check(image[12] == 0xF1 && image[13] == 0x10 && image[14] == 0xFF && image[15] == 0xEE, "capability container");
    check(image[0x10] == 0xA5, "0xA5 marker on page 4");
    check(image[0x54] == ((head >> 24) & 0xFF) && image[0x57] == (head & 0xFF), "model info on pages 21-22");
    check(image[0x208] == 0x01 && image[0x20B] == 0xBD, "dynamic lock bytes");
    check(image[0x218] == 0x80 && image[0x219] == 0x80, "PACK");

    // A tampered image must not verify.
    amiibo::TagImage bad = image;
    bad[0x100] ^= 0x01;
    check(!amiibo::verify(keys, bad, h, t), "tampered image is rejected");

    std::printf("UID: %02X:%02X:%02X:%02X:%02X:%02X:%02X  id=%08X%08X\n", image[0], image[1], image[2], image[4],
                image[5], image[6], image[7], head, tail);
    return failures ? 1 : 0;
}
