/*
 * SPDX-License-Identifier: MIT
 *
 * usage: amiibo_verify <key_retail.bin> <tag_image.bin>
 * Checks the signatures of a 540-byte NTAG215 amiibo image. Prints only the verdict and the id.
 */

#include "amiibo_image.h"

#include <cstdio>

int main(int argc, char** argv) {
    if (argc < 3) {
        std::fprintf(stderr, "usage: %s <key_retail.bin> <tag_image.bin>\n", argv[0]);
        return 2;
    }
    amiibo::Keys keys;
    std::string error;
    if (!keys.load(argv[1], error)) {
        std::fprintf(stderr, "%s\n", error.c_str());
        return 1;
    }
    amiibo::TagImage image;
    std::FILE* f = std::fopen(argv[2], "rb");
    if (!f || std::fread(image.data(), 1, image.size(), f) != image.size()) {
        std::fprintf(stderr, "cannot read %zu bytes from %s\n", image.size(), argv[2]);
        return 1;
    }
    std::fclose(f);

    uint32_t head = 0, tail = 0;
    if (!amiibo::verify(keys, image, head, tail)) {
        std::printf("signatures: INVALID (wrong keys, or not an amiibo image)\n");
        return 3;
    }
    std::printf("signatures: valid\nid=%08X%08X\n", head, tail);
    return 0;
}
