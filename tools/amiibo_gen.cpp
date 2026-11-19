/*
 * SPDX-License-Identifier: MIT
 *
 * usage: amiibo_gen <key_retail.bin> <16-hex-char id> <out.bin>
 * Writes a blank signed 540-byte NTAG215 amiibo image (random UID). Treat the output like a tag dump:
 * do not leave it lying around.
 */

#include "amiibo_image.h"

#include <cstdio>

int main(int argc, char** argv) {
    if (argc < 4) {
        std::fprintf(stderr, "usage: %s <key_retail.bin> <id> <out.bin>\n", argv[0]);
        return 2;
    }
    uint32_t head = 0, tail = 0;
    if (!amiibo::parse_id(argv[2], head, tail)) {
        std::fprintf(stderr, "bad id\n");
        return 2;
    }
    amiibo::Keys keys;
    std::string error;
    amiibo::TagImage image;
    if (!keys.load(argv[1], error) || !amiibo::generate(keys, head, tail, image, error)) {
        std::fprintf(stderr, "%s\n", error.c_str());
        return 1;
    }
    std::FILE* f = std::fopen(argv[3], "wb");
    if (!f || std::fwrite(image.data(), 1, image.size(), f) != image.size()) {
        std::fprintf(stderr, "cannot write %s\n", argv[3]);
        return 1;
    }
    std::fclose(f);
    std::printf("wrote %zu bytes, id=%08X%08X\n", image.size(), head, tail);
    return 0;
}
