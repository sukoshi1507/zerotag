/*
 * SPDX-License-Identifier: MIT
 */

#include "amiibo_image.h"

#include <cstring>
#include <random>

extern "C" {
#include "nfc3d/amiibo.h"
}

namespace amiibo {
namespace {

// Offsets in amiitool's "internal" (decrypted) layout, which spans 0x208 bytes.
constexpr size_t kPlainSize = NFC3D_AMIIBO_SIZE;
constexpr size_t kPlainStaticLock = 0x000;  // BCC1, internal 0x48, lock bytes 0F E0, capability container
constexpr size_t kPlainMagic = 0x028;       // 0xA5, then the write counter
constexpr size_t kPlainUid = 0x1D4;         // UID0-2, BCC0, UID3-6
constexpr size_t kPlainModel = 0x1DC;       // head (4 bytes) + tail (4 bytes)
constexpr size_t kPlainSalt = 0x1E8;        // 32 random bytes feeding the key derivation

// Pages 130..134 of the NTAG215 (not covered by the signatures).
constexpr size_t kTailDynamicLock = 0x208;
constexpr size_t kTailPassword = 0x214;

void put_be32(uint8_t* dst, uint32_t value) {
    dst[0] = static_cast<uint8_t>(value >> 24);
    dst[1] = static_cast<uint8_t>(value >> 16);
    dst[2] = static_cast<uint8_t>(value >> 8);
    dst[3] = static_cast<uint8_t>(value);
}

uint32_t get_be32(const uint8_t* src) {
    return (static_cast<uint32_t>(src[0]) << 24) | (static_cast<uint32_t>(src[1]) << 16) |
           (static_cast<uint32_t>(src[2]) << 8) | src[3];
}

// NTAG215 access password derived from the 7-byte UID.
void derive_password(const uint8_t uid[7], uint8_t out[4]) {
    out[0] = static_cast<uint8_t>(0xAA ^ uid[1] ^ uid[3]);
    out[1] = static_cast<uint8_t>(0x55 ^ uid[2] ^ uid[4]);
    out[2] = static_cast<uint8_t>(0xAA ^ uid[3] ^ uid[5]);
    out[3] = static_cast<uint8_t>(0x55 ^ uid[4] ^ uid[6]);
}

} // namespace

struct Keys::Impl {
    nfc3d_amiibo_keys keys{};
    bool loaded{false};
};

Keys::Keys() : impl_(new Impl) {}

Keys::~Keys() {
    if (impl_) {
        std::memset(&impl_->keys, 0, sizeof(impl_->keys));
        delete impl_;
    }
}

bool Keys::loaded() const {
    return impl_->loaded;
}

bool Keys::load(const std::string& path, std::string& error) {
    impl_->loaded = false;
    if (!nfc3d_amiibo_load_keys(&impl_->keys, path.c_str())) {
        error = "cannot read a valid key file: " + path;
        return false;
    }
    impl_->loaded = true;
    return true;
}

bool parse_id(const std::string& id, uint32_t& head, uint32_t& tail) {
    if (id.size() != 16) {
        return false;
    }
    uint64_t value = 0;
    for (char c : id) {
        int digit;
        if (c >= '0' && c <= '9') {
            digit = c - '0';
        } else if (c >= 'a' && c <= 'f') {
            digit = c - 'a' + 10;
        } else if (c >= 'A' && c <= 'F') {
            digit = c - 'A' + 10;
        } else {
            return false;
        }
        value = (value << 4) | static_cast<uint64_t>(digit);
    }
    head = static_cast<uint32_t>(value >> 32);
    tail = static_cast<uint32_t>(value);
    return true;
}

bool generate(const Keys& keys, uint32_t head, uint32_t tail, TagImage& out, std::string& error) {
    if (!keys.loaded()) {
        error = "keys not loaded";
        return false;
    }

    std::random_device rng;
    uint8_t uid[7];
    uid[0] = 0x04; // NXP manufacturer byte
    for (int i = 1; i < 7; ++i) {
        uid[i] = static_cast<uint8_t>(rng());
    }
    const uint8_t bcc0 = static_cast<uint8_t>(0x88 ^ uid[0] ^ uid[1] ^ uid[2]);
    const uint8_t bcc1 = static_cast<uint8_t>(uid[3] ^ uid[4] ^ uid[5] ^ uid[6]);

    uint8_t plain[kPlainSize] = {};
    plain[kPlainStaticLock + 0] = bcc1;
    plain[kPlainStaticLock + 1] = 0x48;
    plain[kPlainStaticLock + 2] = 0x0F;
    plain[kPlainStaticLock + 3] = 0xE0;
    plain[kPlainStaticLock + 4] = 0xF1;
    plain[kPlainStaticLock + 5] = 0x10;
    plain[kPlainStaticLock + 6] = 0xFF;
    plain[kPlainStaticLock + 7] = 0xEE;
    plain[kPlainMagic] = 0xA5;

    plain[kPlainUid + 0] = uid[0];
    plain[kPlainUid + 1] = uid[1];
    plain[kPlainUid + 2] = uid[2];
    plain[kPlainUid + 3] = bcc0;
    std::memcpy(plain + kPlainUid + 4, uid + 3, 4);

    put_be32(plain + kPlainModel, head);
    put_be32(plain + kPlainModel + 4, tail);
    for (size_t i = 0; i < 32; ++i) {
        plain[kPlainSalt + i] = static_cast<uint8_t>(rng());
    }

    out.fill(0);
    nfc3d_amiibo_pack(&keys.impl_->keys, plain, out.data());

    static const uint8_t dynamic_lock[12] = {0x01, 0x00, 0x0F, 0xBD, 0x00, 0x00, 0x00, 0x04, 0x5F, 0x00, 0x00, 0x00};
    std::memcpy(out.data() + kTailDynamicLock, dynamic_lock, sizeof(dynamic_lock));
    derive_password(uid, out.data() + kTailPassword);
    out[kTailPassword + 4] = 0x80; // PACK
    out[kTailPassword + 5] = 0x80;

    std::memset(plain, 0, sizeof(plain));
    return true;
}

bool verify(const Keys& keys, const TagImage& image, uint32_t& head, uint32_t& tail) {
    if (!keys.loaded()) {
        return false;
    }
    uint8_t plain[kPlainSize] = {};
    const bool ok = nfc3d_amiibo_unpack(&keys.impl_->keys, image.data(), plain);
    if (ok) {
        head = get_be32(plain + kPlainModel);
        tail = get_be32(plain + kPlainModel + 4);
    }
    std::memset(plain, 0, sizeof(plain));
    return ok;
}

} // namespace amiibo
