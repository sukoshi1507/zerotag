/*
 * SPDX-License-Identifier: MIT
 */

#pragma once

#include <array>
#include <cstdint>
#include <string>

namespace amiibo {

constexpr size_t kNtag215Size = 540; // 135 pages of 4 bytes
using TagImage = std::array<uint8_t, kNtag215Size>;

// Master keys (key_retail.bin: 160 bytes) supplied by the user. They are never embedded in the project.
class Keys {
public:
    Keys();
    ~Keys();
    Keys(const Keys&) = delete;
    Keys& operator=(const Keys&) = delete;

    bool load(const std::string& path, std::string& error);
    bool loaded() const;

private:
    friend bool generate(const Keys&, uint32_t, uint32_t, TagImage&, std::string&);
    friend bool verify(const Keys&, const TagImage&, uint32_t&, uint32_t&);
    struct Impl;
    Impl* impl_;
};

// Parses the 16 hex characters of an AmiiboAPI `id` (head + tail, e.g. "01010000000e0002").
bool parse_id(const std::string& id, uint32_t& head, uint32_t& tail);

// Builds a blank, signed NTAG215 image for the character identified by head/tail, with a random UID.
bool generate(const Keys& keys, uint32_t head, uint32_t tail, TagImage& out, std::string& error);

// Checks the signatures of an image and extracts its head/tail. False if the image does not verify.
bool verify(const Keys& keys, const TagImage& image, uint32_t& head, uint32_t& tail);

} // namespace amiibo
