/*
 * SPDX-License-Identifier: MIT
 *
 * NFC-A card emulation (NTAG215) on the ST25R3916.
 *
 * The register sequence and the passive-target state machine (off / idle / ready / active / halt)
 * follow M5Stack's M5Unit-NFC `emulation_layer_a_ST25R3916.cpp` (MIT, (c) 2025 M5Stack Technology
 * CO LTD). The chip answers REQA/WUPA/anticollision/SELECT on its own from the passive-target memory;
 * the Type 2 command set is served here, in software, from a 540-byte tag image.
 */

#include "st25r3916.h"
#include "st25r3916_regs.h"

#include <algorithm>
#include <chrono>
#include <cstdio>
#include <cstring>
#include <vector>

namespace platform {

using namespace st25r;

namespace {

constexpr uint32_t kModeIrq = kIrqWuA | kIrqWuAx | kIrqRxePta;
constexpr uint32_t kDefaultIrq = kIrqNfct | kIrqRxs | kIrqEon | kIrqEof | kIrqCrc | kIrqErr1 | kIrqErr2 | kIrqPar;

constexpr uint8_t kModeBitrateDetection = kTarg | (0x09u << 3); // target, NFC-A, bit rate detection
constexpr uint8_t kModeListenNfcA = kTarg | (0x01u << 3);       // target, NFC-A

constexpr size_t kPages = 135;
constexpr size_t kPasswordPage = 133;
constexpr uint8_t kAck = 0x0A;
constexpr uint8_t kNak = 0x00;

enum class State { Off, Idle, Ready, Active, Halt };

const char* state_name(State s) {
    switch (s) {
    case State::Off: return "OFF";
    case State::Idle: return "IDLE";
    case State::Ready: return "READY";
    case State::Active: return "ACTIVE";
    case State::Halt: return "HALT";
    }
    return "?";
}

struct Reply {
    enum Kind { None, Data, Ack, Nak, Halt } kind{None};
    std::vector<uint8_t> data;
};

std::string hex(const uint8_t* d, size_t n) {
    std::string s;
    char buf[4];
    for (size_t i = 0; i < n; ++i) {
        std::snprintf(buf, sizeof(buf), "%02X", d[i]);
        s += buf;
        if (i + 1 < n) {
            s += ' ';
        }
    }
    return s;
}

// NTAG215 command set. `rx` has the CRC already stripped.
Reply handle_type2(uint8_t* image, const uint8_t* rx, size_t n, const St25r3916::EmulationLog& log) {
    Reply r;
    auto page_ptr = [&](size_t page) { return image + page * 4; };
    switch (rx[0]) {
    case 0x30: // READ: 4 pages, wraps around at the end of the memory
        if (n == 2) {
            if (rx[1] >= kPages) {
                r.kind = Reply::Nak;
                break;
            }
            for (size_t i = 0; i < 16; ++i) {
                r.data.push_back(image[((rx[1] * 4u) + i) % (kPages * 4)]);
            }
            r.kind = Reply::Data;
        }
        break;
    case 0x3A: // FAST_READ
        if (n == 3) {
            const size_t first = rx[1], last = rx[2];
            if (first > last || last >= kPages) {
                r.kind = Reply::Nak;
                break;
            }
            r.data.assign(page_ptr(first), page_ptr(last) + 4);
            r.kind = Reply::Data;
        }
        break;
    case 0x60: // GET_VERSION: NTAG215
        if (n == 1) {
            r.data = {0x00, 0x04, 0x04, 0x02, 0x01, 0x00, 0x11, 0x03};
            r.kind = Reply::Data;
        }
        break;
    case 0x3C: // READ_SIG: the originality signature is not reproduced
        if (n == 2) {
            r.data.assign(32, 0x00);
            r.kind = Reply::Data;
        }
        break;
    case 0x1B: // PWD_AUTH: always accepted, the PACK stored in the image is returned
        if (n == 5) {
            if (std::memcmp(rx + 1, page_ptr(kPasswordPage), 4) != 0) {
                log("  PWD_AUTH password differs from the image (accepted anyway)");
            }
            r.data.assign(page_ptr(kPasswordPage + 1), page_ptr(kPasswordPage + 1) + 2);
            r.kind = Reply::Data;
        }
        break;
    case 0xA2: // WRITE one page
        if (n == 6) {
            if (rx[1] < 2 || rx[1] >= kPages) {
                r.kind = Reply::Nak;
                break;
            }
            std::memcpy(page_ptr(rx[1]), rx + 2, 4);
            r.kind = Reply::Ack;
        }
        break;
    case 0x50: // HLTA
        if (n == 2 && rx[1] == 0x00) {
            r.kind = Reply::Halt;
        }
        break;
    default:
        break;
    }
    return r;
}

} // namespace

bool St25r3916::write_mask_interrupts(uint32_t mask) {
    const uint8_t data[4] = {static_cast<uint8_t>(mask >> 24), static_cast<uint8_t>(mask >> 16),
                             static_cast<uint8_t>(mask >> 8), static_cast<uint8_t>(mask)};
    return write_regs(kRegMaskMainInterrupt, data, 4);
}

bool St25r3916::read_mask_interrupts(uint32_t& mask) {
    uint8_t data[4] = {};
    if (!read_regs(kRegMaskMainInterrupt, data, 4)) {
        return false;
    }
    mask = (static_cast<uint32_t>(data[0]) << 24) | (static_cast<uint32_t>(data[1]) << 16) |
           (static_cast<uint32_t>(data[2]) << 8) | data[3];
    return true;
}

// `unmask` bits are enabled (mask bit cleared), `mask` bits are disabled.
bool St25r3916::modify_mask_interrupts(uint32_t unmask, uint32_t mask) {
    uint32_t current = 0;
    if (!read_mask_interrupts(current)) {
        return false;
    }
    const uint32_t next = (current & ~unmask) | mask;
    return next == current || write_mask_interrupts(next);
}

bool St25r3916::load_pt_memory_a(const uint8_t* data, size_t len) {
    uint8_t tx[16];
    if (len == 0 || len > 15) {
        return false;
    }
    tx[0] = kOpLoadPtMemoryA;
    std::memcpy(tx + 1, data, len);
    return transfer(tx, nullptr, len + 1);
}

// Transmits `bits` bits (the last byte partially when bits % 8 != 0), with or without CRC.
bool St25r3916::transmit_bits(const uint8_t* frame, size_t bits, bool with_crc) {
    if (!direct_command(kCmdClearFifo) || !write_fifo(frame, (bits + 7) / 8)) {
        return false;
    }
    const uint8_t ntx[2] = {static_cast<uint8_t>(bits >> 8), static_cast<uint8_t>(bits & 0xFF)};
    return write_regs(kRegNumberOfTransmittedBytes1, ntx, 2) &&
           direct_command(with_crc ? kCmdTransmitWithCrc : kCmdTransmitWithoutCrc);
}

bool St25r3916::emulate_ntag215(uint8_t* image, int seconds, const EmulationLog& log_fn, std::string& error,
                                const std::function<bool()>& stop) {
    auto log = [&](const std::string& s) {
        if (log_fn) {
            log_fn(s);
        }
    };
    auto fail = [&](const char* step) {
        error = std::string("emulation setup failed at: ") + step;
        return false;
    };
    if (!image) {
        error = "no image";
        return false;
    }

    // Reader-mode init brings the chip into a known state (oscillator, regulators, receiver); the
    // emulation then reconfigures it as a passive target.
    if (!init_nfc_a(error)) {
        return false;
    }
    field_off();

    // 7-byte UID from pages 0-2 (the BCC bytes are not part of it), ATQA 0x0044, SAK 0x00.
    uint8_t pt[15] = {};
    pt[0] = image[0];
    pt[1] = image[1];
    pt[2] = image[2];
    pt[3] = image[4];
    pt[4] = image[5];
    pt[5] = image[6];
    pt[6] = image[7];
    pt[10] = 0x44; // ATQA LSB
    pt[11] = 0x00;
    pt[12] = 0x04; // SAK level 1: cascade bit set (UID incomplete)
    pt[13] = 0x00; // SAK level 2
    pt[14] = 0x00; // SAK level 3
    log("UID " + hex(pt, 7));

    if (!modify_reg(kRegAuxiliaryDefinition, 0x30, 0x10) /* 7-byte UID */ || !load_pt_memory_a(pt, sizeof(pt))) {
        return fail("passive target memory");
    }
    write_reg(kRegModeDefinition, kModeBitrateDetection);                // target, NFC-A, bit rate detection
    // Automatic answers for NFC-A only.
    modify_reg(kRegNfcip1PassiveTarget, 0x0B, 0x0C);
    modify_reg(kRegTimerEmvControl, 0xE0, 0x08);                        // MRT step 512/fc
    write_reg(kRegMaskReceiverTimer, 4);                                // ~100 us
    modify_reg(kRegIso14443aSettings, 0xE0, 0x00);                      // parity on, no NFCIP-1
    write_reg(kRegAntennaTuningControl1, 0x00);
    write_reg(kRegAntennaTuningControl2, 0xFF);
    write_reg(kRegOvershootProtection1, 0x00, true);
    write_reg(kRegOvershootProtection2, 0x00, true);
    write_reg(kRegUndershootProtection1, 0x00, true);
    write_reg(kRegUndershootProtection2, 0x00, true);
    direct_command(kCmdUnmaskReceiveData);

    // ----- state machine -----
    uint32_t stored = 0;
    uint8_t bitrate = 0; // 0: 106, 1: 212, 2: 424
    bool bitrate_valid = false;
    bool data_flag = false;
    bool wakeup = false;
    State state = State::Off;

    auto poll = [&]() {
        uint32_t v = 0;
        if (read_interrupts(v)) {
            stored |= v;
        }
    };
    auto take = [&](uint32_t bits) {
        const uint32_t hit = stored & bits;
        stored &= ~hit;
        return hit;
    };
    auto field_present = [&]() {
        uint8_t v = 0;
        return read_reg(kRegAuxiliaryDisplay, v) && (v & kExtFieldDetected);
    };
    auto read_bitrate = [&]() {
        uint8_t v = 0;
        read_reg(kRegBitrateDetectionDisplay, v);
        bitrate = std::min<uint8_t>((v >> 4) & 0x03, 2);
        bitrate_valid = true;
    };

    auto enter = [&](State next) -> State {
        switch (next) {
        case State::Off:
            data_flag = false;
            bitrate_valid = false;
            wakeup = false;
            direct_command(kCmdStopAllActivities);
            modify_reg(kRegOperationControl, 0, kOpControlRxEn);
            modify_reg(kRegNfcip1PassiveTarget, kNfcipPtAutoA, 0);
            direct_command(kCmdGoToSense);
            modify_reg(kRegIso14443aSettings, 0x20, 0);
            write_mask_interrupts(0xFFFFFFFF);
            clear_interrupts();
            modify_mask_interrupts(kIrqOsc | kDefaultIrq | kModeIrq, 0);
            stored = 0;
            modify_reg(kRegAuxiliaryDefinition, 0, kNoCrcRx);
            modify_reg(kRegModeDefinition, 0x78, kModeBitrateDetection & 0x78);
            if (field_present()) {
                return State::Idle;
            }
            // The oscillator and the receiver stay on: phones poll with field bursts of a few tens of
            // ms, shorter than the oscillator start-up time, so the chip must be able to answer at once.
            modify_reg(kRegOperationControl, kOpControlTxEn, kOpControlEn | kOpControlRxEn);
            return State::Off;
        case State::Idle: {
            data_flag = false;
            uint8_t op = 0, aux = 0;
            if (read_reg(kRegOperationControl, op) && !(op & kOpControlEn)) {
                modify_reg(kRegOperationControl, 0, kOpControlEn | kOpControlRxEn);
                if (read_reg(kRegAuxiliaryDisplay, aux) && !(aux & kOscOk)) {
                    if (!(wait_interrupt(kIrqOsc, 1000) & kIrqOsc)) {
                        log("oscillator did not start");
                        return State::Off;
                    }
                }
            } else {
                take(kIrqOsc);
            }
            modify_reg(kRegAuxiliaryDefinition, 0, kNoCrcRx);
            if (state == State::Active && !wakeup) {
                modify_reg(kRegNfcip1PassiveTarget, kNfcipPtAutoA, 0);
                direct_command(kCmdGoToSense);
            }
            direct_command(kCmdClearFifo);
            direct_command(kCmdUnmaskReceiveData);
            wakeup = false;
            return State::Idle;
        }
        case State::Ready:
            data_flag = false;
            if (take(kIrqEof)) {
                return State::Off;
            }
            modify_reg(kRegAuxiliaryDefinition, kNoCrcRx, 0);
            write_reg(kRegBitrateDefinition, static_cast<uint8_t>((bitrate << 4) | bitrate));
            modify_reg(kRegOperationControl, kOpControlWu, 0);
            write_reg(kRegModeDefinition, kModeListenNfcA);
            return State::Ready;
        case State::Active:
            data_flag = false;
            modify_reg(kRegNfcip1PassiveTarget, 0, kNfcipPtAutoA); // the software answers from now on
            take(kIrqPar | kIrqCrc | kIrqErr2 | kIrqErr1);
            modify_mask_interrupts(kIrqRxe, 0);
            return State::Active;
        case State::Halt:
            data_flag = false;
            modify_reg(kRegNfcip1PassiveTarget, kNfcipPtAutoA, 0);
            direct_command(kCmdGoToSleep);
            modify_reg(kRegModeDefinition, 0xFB, kModeBitrateDetection & 0xFB);
            modify_reg(kRegIso14443aSettings, 0x20, 0);
            direct_command(kCmdUnmaskReceiveData);
            modify_mask_interrupts(kIrqNfct | kIrqRxs | kIrqCrc | kIrqErr1 | kIrqErr2 | kIrqPar | kIrqEon | kIrqEof |
                                       kModeIrq,
                                   0);
            if (!field_present()) {
                return State::Off;
            }
            return State::Halt;
        }
        return State::Off;
    };

    auto change = [&](State next) {
        const State actual = enter(next);
        if (actual != state) {
            log(std::string("state ") + state_name(state) + " -> " + state_name(actual));
        }
        state = actual;
    };

    auto send_reply = [&](const Reply& r) {
        switch (r.kind) {
        case Reply::Data:
            transmit_bits(r.data.data(), r.data.size() * 8, true);
            break;
        case Reply::Ack: {
            const uint8_t v = kAck;
            transmit_bits(&v, 4, false);
            break;
        }
        case Reply::Nak: {
            const uint8_t v = kNak;
            transmit_bits(&v, 4, false);
            break;
        }
        default:
            break;
        }
    };

    const auto start = std::chrono::steady_clock::now();
    const auto deadline = start + std::chrono::seconds(seconds > 0 ? seconds : 24 * 3600);
    change(State::Off);
    log("emulation running, present a reader");

    unsigned frames = 0;
    while (std::chrono::steady_clock::now() < deadline && !(stop && stop())) {
        poll();
        switch (state) {
        case State::Off:
            if (take(kIrqEon)) {
                change(State::Idle);
            }
            break;

        case State::Idle: {
            const uint32_t irq = take(kIrqNfct | kIrqRxe | kIrqEof | kIrqRxePta);
            if (!irq) {
                break;
            }
            if (irq & kIrqNfct) {
                read_bitrate();
            }
            if ((irq & kIrqEof) && !data_flag) {
                change(State::Off);
                break;
            }
            if ((irq & kIrqRxe) && bitrate_valid) {
                data_flag = true;
                direct_command(kCmdClearFifo);
                direct_command(kCmdUnmaskReceiveData);
                break;
            }
            if ((irq & kIrqRxePta) && bitrate_valid && bitrate == 0) {
                uint8_t pta = 0;
                if (read_reg(kRegPassiveTargetDisplay, pta) && (pta & 0x0F) > kPtaStateIdle) {
                    change(State::Ready);
                }
            }
            break;
        }

        case State::Ready: {
            const uint32_t irq = take(kIrqEof | (wakeup ? kIrqWuAx : kIrqWuA));
            if (!irq) {
                break;
            }
            if (irq & kIrqEof) {
                change(State::Off);
            } else {
                change(State::Active);
            }
            break;
        }

        case State::Active: {
            const uint32_t irq = take(kIrqEof | kIrqRxe);
            if (!irq) {
                break;
            }
            if (irq & kIrqEof) {
                change(State::Off);
                break;
            }
            const uint32_t err = take(kIrqPar | kIrqCrc | kIrqErr2 | kIrqErr1);
            uint8_t rx[64] = {};
            size_t n = 0;
            uint16_t pending = 0;
            fifo_size(pending);
            if (pending > 0) {
                read_fifo(rx, sizeof(rx), n);
            }
            if (err || n <= 2) {
                log("  rx error irq=" + std::to_string(err >> 8) + " len=" + std::to_string(n));
                direct_command(kCmdClearFifo);
                direct_command(kCmdUnmaskReceiveData);
                change(wakeup ? State::Halt : State::Idle);
                break;
            }
            n -= 2; // CRC_A
            ++frames;
            const Reply reply = handle_type2(image, rx, n, log_fn);
            send_reply(reply);
            log("  rx " + hex(rx, n) +
                (reply.kind == Reply::Data  ? "  -> " + std::to_string(reply.data.size()) + " bytes"
                 : reply.kind == Reply::Ack ? "  -> ACK"
                 : reply.kind == Reply::Nak ? "  -> NAK"
                 : reply.kind == Reply::Halt ? "  -> HLTA"
                                             : "  -> (no answer)"));
            if (reply.kind == Reply::Halt) {
                modify_mask_interrupts(0, kIrqRxe);
                change(State::Halt);
            }
            break;
        }

        case State::Halt: {
            const uint32_t irq = take(kIrqNfct | kIrqRxe | kIrqEof | kIrqRxePta);
            if (!irq) {
                break;
            }
            if (irq & kIrqNfct) {
                read_bitrate();
            }
            if (irq & kIrqEof) {
                change(State::Off);
                break;
            }
            if ((irq & kIrqRxe) && bitrate_valid) {
                direct_command(kCmdClearFifo);
                direct_command(kCmdUnmaskReceiveData);
                break;
            }
            if ((irq & kIrqRxePta) && bitrate_valid && bitrate == 0) {
                uint8_t pta = 0;
                if (read_reg(kRegPassiveTargetDisplay, pta) && (pta & 0x0F) > kPtaStateHalt) {
                    wakeup = true;
                    change(State::Ready);
                }
            }
            break;
        }
        }
    }

    log("emulation finished, " + std::to_string(frames) + " command frame(s) served");

    // Leave the chip in a quiet state.
    direct_command(kCmdStopAllActivities);
    write_reg(kRegModeDefinition, 0x00);
    modify_reg(kRegNfcip1PassiveTarget, 0, 0x0D);
    return true;
}

} // namespace platform
