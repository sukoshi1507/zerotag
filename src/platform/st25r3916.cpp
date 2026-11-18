/*
 * SPDX-License-Identifier: MIT
 */

#include "st25r3916.h"
#include "st25r3916_regs.h"

#include <cerrno>
#include <cstdio>
#include <algorithm>
#include <chrono>
#include <cstring>
#include <fstream>
#include <thread>
#include <utility>

#if defined(__linux__)
#include <fcntl.h>
#include <linux/gpio.h>
#include <linux/spi/spidev.h>
#include <sys/ioctl.h>
#include <unistd.h>
#endif

namespace platform {
namespace {

using namespace st25r;
constexpr uint8_t kIdentityTypeMask = 0xF8;
constexpr uint8_t kIdentityTypeShifted = 0x28;

std::string errno_text(const std::string& what) {
    return what + ": " + std::strerror(errno);
}

} // namespace

St25r3916::St25r3916(St25r3916Config config) : config_(std::move(config)) {}

St25r3916::~St25r3916() {
    close();
}

const St25r3916Config& St25r3916::config() const {
    return config_;
}

bool St25r3916::is_open() const {
    return spi_fd_ >= 0;
}

uint8_t St25r3916::ic_identity() const {
    return identity_;
}

int St25r3916::revision() const {
    return identity_ & 0x07;
}

#if defined(__linux__)

bool St25r3916::power_on(std::string& error) {
    {
        std::ofstream led(config_.power_led);
        if (!led) {
            error = "cannot open " + config_.power_led;
            return false;
        }
        led << 1 << std::flush;
        if (!led) {
            error = "cannot write " + config_.power_led;
            return false;
        }
        led_on_ = true;
    }

    const int chip_fd = ::open(config_.gpio_chip.c_str(), O_RDONLY | O_CLOEXEC);
    if (chip_fd < 0) {
        error = errno_text("cannot open " + config_.gpio_chip);
        return false;
    }

    gpio_v2_line_request request{};
    request.offsets[0] = static_cast<uint32_t>(config_.power_gpio);
    request.num_lines = 1;
    std::strncpy(request.consumer, "pixljszero", sizeof(request.consumer) - 1);
    request.config.flags = GPIO_V2_LINE_FLAG_OUTPUT;
    request.config.num_attrs = 1;
    request.config.attrs[0].mask = 1;
    request.config.attrs[0].attr.id = GPIO_V2_LINE_ATTR_ID_OUTPUT_VALUES;
    request.config.attrs[0].attr.values = 1;

    const int rc = ::ioctl(chip_fd, GPIO_V2_GET_LINE_IOCTL, &request);
    const int saved_errno = errno;
    ::close(chip_fd);
    if (rc < 0) {
        errno = saved_errno;
        error = errno_text("cannot request GPIO" + std::to_string(config_.power_gpio));
        return false;
    }
    power_line_fd_ = request.fd;

    // Give the Cap supply time to settle before the first SPI access.
    std::this_thread::sleep_for(std::chrono::milliseconds(300));
    return true;
}

void St25r3916::power_off() {
    // Closing the line request releases GPIO26 (back to its default state).
    if (power_line_fd_ >= 0) {
        ::close(power_line_fd_);
        power_line_fd_ = -1;
    }
    if (led_on_) {
        std::ofstream led(config_.power_led);
        if (led) {
            led << 0 << std::flush;
        }
        led_on_ = false;
    }
}

bool St25r3916::transfer(const uint8_t* tx, uint8_t* rx, size_t len) {
    if (spi_fd_ < 0) {
        return false;
    }
    spi_ioc_transfer t{};
    t.tx_buf = reinterpret_cast<uintptr_t>(tx);
    t.rx_buf = reinterpret_cast<uintptr_t>(rx);
    t.len = static_cast<uint32_t>(len);
    t.speed_hz = config_.spi_speed_hz;
    t.bits_per_word = 8;
    return ::ioctl(spi_fd_, SPI_IOC_MESSAGE(1), &t) >= 0;
}

bool St25r3916::read_register(uint8_t address, uint8_t& value) {
    return read_reg(address, value);
}

bool St25r3916::open(std::string& error) {
    if (is_open()) {
        return true;
    }

    if (!power_on(error)) {
        power_off();
        return false;
    }

    spi_fd_ = ::open(config_.spi_device.c_str(), O_RDWR | O_CLOEXEC);
    if (spi_fd_ < 0) {
        error = errno_text("cannot open " + config_.spi_device) +
                " (is the spi0-spidev2-gpio22 overlay loaded?)";
        power_off();
        return false;
    }

    uint8_t mode = SPI_MODE_1;
    uint8_t bits = 8;
    uint32_t speed = config_.spi_speed_hz;
    if (::ioctl(spi_fd_, SPI_IOC_WR_MODE, &mode) < 0 ||
        ::ioctl(spi_fd_, SPI_IOC_WR_BITS_PER_WORD, &bits) < 0 ||
        ::ioctl(spi_fd_, SPI_IOC_WR_MAX_SPEED_HZ, &speed) < 0) {
        error = errno_text("cannot configure SPI");
        close();
        return false;
    }

    uint8_t identity = 0;
    if (!read_reg(kRegIcIdentity, identity)) {
        error = errno_text("SPI transfer failed");
        close();
        return false;
    }
    if ((identity & kIdentityTypeMask) != kIdentityTypeShifted) {
        char hex[8];
        std::snprintf(hex, sizeof(hex), "0x%02X", identity);
        error = std::string("unexpected IC identity ") + hex +
                " (expected type bits 0x28): is the Cap CC1101 installed and powered?";
        close();
        return false;
    }

    identity_ = identity;
    return true;
}

void St25r3916::close() {
    if (spi_fd_ >= 0) {
        field_off();
        ::close(spi_fd_);
        spi_fd_ = -1;
    }
    power_off();
    identity_ = 0;
}


// ---------------------------------------------------------------------------
// Register access. Frame layout (datasheet): [opcode|address] [data...]; space B is reached with a
// 0xFB prefix byte; a direct command is a single byte.
// ---------------------------------------------------------------------------

bool St25r3916::write_regs(uint8_t reg, const uint8_t* data, size_t len, bool space_b) {
    uint8_t tx[40];
    size_t n = 0;
    if (len + 2 > sizeof(tx)) {
        return false;
    }
    if (space_b) {
        tx[n++] = kPrefixSpaceB;
    }
    tx[n++] = static_cast<uint8_t>(kOpWriteRegister | (reg & 0x3F));
    for (size_t i = 0; i < len; ++i) {
        tx[n++] = data[i];
    }
    return transfer(tx, nullptr, n);
}

bool St25r3916::read_regs(uint8_t reg, uint8_t* data, size_t len, bool space_b) {
    uint8_t tx[40] = {};
    uint8_t rx[40] = {};
    size_t head = 0;
    if (len + 2 > sizeof(tx)) {
        return false;
    }
    if (space_b) {
        tx[head++] = kPrefixSpaceB;
    }
    tx[head++] = static_cast<uint8_t>(kOpReadRegister | (reg & 0x3F));
    if (!transfer(tx, rx, head + len)) {
        return false;
    }
    std::memcpy(data, rx + head, len);
    return true;
}

bool St25r3916::write_reg(uint8_t reg, uint8_t value, bool space_b) {
    return write_regs(reg, &value, 1, space_b);
}

bool St25r3916::read_reg(uint8_t reg, uint8_t& value, bool space_b) {
    return read_regs(reg, &value, 1, space_b);
}

bool St25r3916::modify_reg(uint8_t reg, uint8_t clear_mask, uint8_t set_mask, bool space_b) {
    uint8_t value = 0;
    if (!read_reg(reg, value, space_b)) {
        return false;
    }
    value = static_cast<uint8_t>((value & ~clear_mask) | set_mask);
    return write_reg(reg, value, space_b);
}

bool St25r3916::direct_command(uint8_t cmd) {
    return transfer(&cmd, nullptr, 1);
}

// The error interrupt register is cleared by reading the main one, so it is read first.
bool St25r3916::read_interrupts(uint32_t& value) {
    uint8_t error = 0;
    uint8_t main_timer[2] = {};
    uint8_t passive = 0;
    if (!read_reg(kRegErrorInterrupt, error) || !read_regs(kRegMainInterrupt, main_timer, 2) ||
        !read_reg(kRegPassiveTargetInterrupt, passive)) {
        return false;
    }
    value = (static_cast<uint32_t>(main_timer[0]) << 24) | (static_cast<uint32_t>(main_timer[1]) << 16) |
            (static_cast<uint32_t>(error) << 8) | passive;
    return true;
}

bool St25r3916::clear_interrupts() {
    uint32_t discard = 0;
    return read_interrupts(discard);
}

// Polls the interrupt registers (the IRQ pin is not used). Returns the accumulated bits matching `mask`,
// or the bits seen so far when the timeout elapses.
uint32_t St25r3916::wait_interrupt(uint32_t mask, int timeout_ms) {
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::milliseconds(timeout_ms);
    uint32_t seen = 0;
    do {
        uint32_t value = 0;
        if (read_interrupts(value)) {
            seen |= value;
            if (seen & mask) {
                return seen;
            }
        }
    } while (std::chrono::steady_clock::now() < deadline);
    return seen;
}

bool St25r3916::fifo_size(uint16_t& bytes) {
    uint8_t s[2] = {};
    if (!read_regs(kRegFifoStatus1, s, 2)) {
        return false;
    }
    bytes = static_cast<uint16_t>(s[0] | ((s[1] & 0xC0) << 2));
    return true;
}

bool St25r3916::read_fifo(uint8_t* buf, size_t max, size_t& actual) {
    actual = 0;
    uint16_t bytes = 0;
    if (!fifo_size(bytes) || bytes == 0) {
        return false;
    }
    const size_t n = std::min<size_t>(bytes, max);
    uint8_t tx[40] = {};
    uint8_t rx[40] = {};
    if (n + 1 > sizeof(tx)) {
        return false;
    }
    tx[0] = kOpReadFifo;
    if (!transfer(tx, rx, n + 1)) {
        return false;
    }
    std::memcpy(buf, rx + 1, n);
    actual = n;
    return true;
}

bool St25r3916::write_fifo(const uint8_t* buf, size_t len) {
    // Each load appends to the FIFO, so long frames are sent in chunks that fit the transfer buffer.
    constexpr size_t kChunk = 32;
    uint8_t tx[kChunk + 1];
    for (size_t offset = 0; offset < len; offset += kChunk) {
        const size_t n = std::min(kChunk, len - offset);
        tx[0] = kOpLoadFifo;
        std::memcpy(tx + 1, buf + offset, n);
        if (!transfer(tx, nullptr, n + 1)) {
            return false;
        }
    }
    return true;
}

// No-response timer, 64/fc steps (4.72 us), as selected by the default timer control value.
bool St25r3916::set_no_response_timer_ms(int ms) {
    const uint64_t steps = (static_cast<uint64_t>(ms) * 1000u * 13560000u + 64u * 1000000u - 1u) / (64u * 1000000u);
    const uint16_t nrt = static_cast<uint16_t>(std::min<uint64_t>(std::max<uint64_t>(steps, 1), 0xFFFF));
    const uint8_t data[2] = {static_cast<uint8_t>(nrt >> 8), static_cast<uint8_t>(nrt & 0xFF)};
    return write_regs(kRegNoResponseTimer1, data, 2);
}

// ---------------------------------------------------------------------------
// Initialisation, following the power-on sequence of M5Stack's M5Unit-NFC (MIT).
// ---------------------------------------------------------------------------

bool St25r3916::enable_oscillator() {
    if (!modify_reg(kRegOperationControl, 0, kOpControlEn)) {
        return false;
    }
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::milliseconds(50);
    do {
        uint8_t aux = 0;
        if (read_reg(kRegAuxiliaryDisplay, aux) && (aux & kOscOk)) {
            return true;
        }
    } while (std::chrono::steady_clock::now() < deadline);
    return false;
}

bool St25r3916::init_nfc_a(std::string& error) {
    if (!is_open()) {
        error = "chip not open";
        return false;
    }
    auto sleep_ms = [](int ms) { std::this_thread::sleep_for(std::chrono::milliseconds(ms)); };
    auto fail = [&](const char* step) {
        error = std::string("ST25R3916 init failed at: ") + step;
        return false;
    };

    direct_command(kCmdStopAllActivities);
    modify_reg(kRegOperationControl, kOpControlTxEn | kOpControlRxEn, 0);
    sleep_ms(2);

    if (!direct_command(kCmdSetDefault)) {
        return fail("set default");
    }
    sleep_ms(2);
    // Prevents the internal overheat protection from triggering (datasheet: FCh / 04h / 10h).
    const uint8_t protection[3] = {kPrefixTestAccess, 0x04, 0x10};
    if (!transfer(protection, nullptr, sizeof(protection))) {
        return fail("test access");
    }

    // I/O: SPI, MISO pull-downs, VDD 3.3 V (as M5Unit-NFC does by default).
    if (!write_reg(kRegIoConfiguration2, kMisoPd1 | kMisoPd2 | kIoDrvLvl | kSup3v) ||
        !write_reg(kRegTxDriver, 13 << 4) || !modify_reg(kRegIoConfiguration1, 0, 0x07)) {
        return fail("io configuration");
    }
    write_reg(kRegResistiveAmModulation, 0x80, true);
    modify_reg(kRegIoConfiguration2, 0, kAatEn);
    write_reg(kRegResistiveAmModulation, 0x00, true);
    write_reg(kRegFieldDetectorActivation, 0x13);
    write_reg(kRegFieldDetectorDeactivation, 0x02);
    modify_reg(kRegNfcip1PassiveTarget, 0xF0, 0x50);
    write_reg(kRegPassiveTargetModulation, 0x5F);
    write_reg(kRegEmdSuppression, 0x40, true);
    write_reg(kRegAntennaTuningControl1, 0x82);
    write_reg(kRegAntennaTuningControl2, 0x82);
    modify_reg(kRegOperationControl, 0, 0x03); // external field detector automatic
    direct_command(kCmdClearFifo);

    if (!enable_oscillator()) {
        return fail("oscillator");
    }
    direct_command(kCmdAdjustRegulators);
    sleep_ms(5);

    // NFC-A, 106 kbit/s in both directions.
    if (!write_reg(kRegModeDefinition, 0x08 | 0x01) || !write_reg(kRegBitrateDefinition, 0x00) ||
        !write_reg(kRegIso14443aSettings, 0x00)) {
        return fail("nfc-a mode");
    }
    modify_reg(kRegAuxiliaryDefinition, kDisCorr, 0);
    write_reg(kRegOvershootProtection1, 0x40, true);
    write_reg(kRegOvershootProtection2, 0x03, true);
    write_reg(kRegUndershootProtection1, 0x40, true);
    write_reg(kRegUndershootProtection2, 0x03, true);
    write_reg(kRegCorrelatorConfiguration1, 0x47, true);
    write_reg(kRegCorrelatorConfiguration2, 0x00, true);
    if (!write_reg(kRegReceiverConfiguration1, 0x08) || !write_reg(kRegReceiverConfiguration2, 0x2D) ||
        !write_reg(kRegReceiverConfiguration3, 0xD8) || !write_reg(kRegReceiverConfiguration4, 0x22)) {
        return fail("receiver");
    }
    direct_command(kCmdResetRxGain);

    // Switch the RF field on.
    direct_command(kCmdNfcInitialFieldOn);
    sleep_ms(5);
    if (!modify_reg(kRegOperationControl, 0, kOpControlTxEn | kOpControlRxEn)) {
        return fail("field on");
    }
    sleep_ms(10);
    return true;
}

void St25r3916::field_off() {
    if (is_open()) {
        direct_command(kCmdStopAllActivities);
        modify_reg(kRegOperationControl, kOpControlTxEn | kOpControlRxEn, 0);
    }
}

// ---------------------------------------------------------------------------
// ISO 14443-3A: REQA/WUPA, anticollision, select.
// ---------------------------------------------------------------------------

bool St25r3916::transmit(const uint8_t* frame, size_t len, bool with_crc, uint32_t wait_mask, int timeout_ms,
                         uint8_t* rx, size_t rx_max, size_t& rx_len, uint32_t& irq) {
    rx_len = 0;
    irq = 0;
    if (!clear_interrupts() || !direct_command(kCmdClearFifo) || !write_fifo(frame, len)) {
        return false;
    }
    const uint16_t bits = static_cast<uint16_t>(len << 3);
    const uint8_t ntx[2] = {static_cast<uint8_t>(bits >> 8), static_cast<uint8_t>(bits & 0xFF)};
    if (!write_regs(kRegNumberOfTransmittedBytes1, ntx, 2) ||
        !direct_command(with_crc ? kCmdTransmitWithCrc : kCmdTransmitWithoutCrc)) {
        return false;
    }
    irq = wait_interrupt(wait_mask, timeout_ms);
    if (!(irq & kIrqRxe)) {
        return false;
    }
    return read_fifo(rx, rx_max, rx_len);
}

bool St25r3916::request(bool wakeup, uint16_t& atqa) {
    atqa = 0;
    if (!set_no_response_timer_ms(10) || !write_reg(kRegIso14443aSettings, kAntcl) ||
        !modify_reg(kRegAuxiliaryDefinition, 0, kNoCrcRx) || !clear_interrupts() ||
        !direct_command(kCmdClearFifo) || !direct_command(wakeup ? kCmdTransmitWupa : kCmdTransmitReqa)) {
        return false;
    }
    const uint32_t irq = wait_interrupt(kIrqRxe, 15);
    uint8_t rx[2] = {};
    size_t n = 0;
    if (!(irq & kIrqRxe) || !read_fifo(rx, sizeof(rx), n) || n != 2) {
        return false;
    }
    atqa = static_cast<uint16_t>(rx[0] | (rx[1] << 8));
    return true;
}

bool St25r3916::anticollision(uint8_t level, uint8_t out[5], std::string& error) {
    if (!set_no_response_timer_ms(10) || !write_reg(kRegIso14443aSettings, kAntcl) ||
        !modify_reg(kRegAuxiliaryDefinition, kNoCrcRx, 0)) {
        error = "anticollision setup failed";
        return false;
    }
    const uint8_t frame[2] = {static_cast<uint8_t>(0x91 + level * 2), 0x20};
    size_t n = 0;
    uint32_t irq = 0;
    if (!transmit(frame, sizeof(frame), false, kIrqRxe | kIrqCol, 15, out, 5, n, irq)) {
        error = (irq & kIrqCol) ? "collision (several tags in the field)" : "no answer to anticollision";
        return false;
    }
    if (irq & kIrqCol) {
        error = "collision (several tags in the field)";
        return false;
    }
    if (n != 5 || (out[0] ^ out[1] ^ out[2] ^ out[3]) != out[4]) {
        error = "anticollision answer failed its check byte";
        return false;
    }
    return true;
}

bool St25r3916::select_level(uint8_t level, const uint8_t id[5], uint8_t& sak) {
    if (!write_reg(kRegIso14443aSettings, 0x00) || !modify_reg(kRegAuxiliaryDefinition, kNoCrcRx, 0)) {
        return false;
    }
    uint8_t frame[7] = {static_cast<uint8_t>(0x91 + level * 2), 0x70};
    std::memcpy(frame + 2, id, 5);
    uint8_t rx[4] = {};
    size_t n = 0;
    uint32_t irq = 0;
    if (!transmit(frame, sizeof(frame), true, kIrqRxe, 15, rx, sizeof(rx), n, irq) || n < 1) {
        return false;
    }
    sak = rx[0];
    return true;
}

bool St25r3916::detect_tag(Tag& tag, std::string& error) {
    tag = Tag{};
    // WUPA also wakes tags left in the HALT state.
    if (!request(true, tag.atqa)) {
        error = "no tag";
        return false;
    }
    for (uint8_t level = 1; level <= 3; ++level) {
        uint8_t id[5] = {};
        if (!anticollision(level, id, error)) {
            return false;
        }
        uint8_t sak = 0;
        if (!select_level(level, id, sak)) {
            error = "select failed";
            return false;
        }
        const bool cascade = id[0] == 0x88;
        const size_t take = cascade ? 3 : 4;
        std::memcpy(tag.uid + tag.uid_len, id + (cascade ? 1 : 0), take);
        tag.uid_len = static_cast<uint8_t>(tag.uid_len + take);
        tag.sak = sak;
        if (!(sak & 0x04)) { // cascade bit clear: UID complete
            return true;
        }
    }
    error = "UID longer than 10 bytes";
    return false;
}

bool St25r3916::type2_get_version(uint8_t out[8]) {
    const uint8_t cmd = 0x60;
    uint8_t rx[16] = {};
    size_t n = 0;
    uint32_t irq = 0;
    if (!write_reg(kRegIso14443aSettings, 0x00) || !modify_reg(kRegAuxiliaryDefinition, kNoCrcRx, 0) ||
        !set_no_response_timer_ms(10) || !transmit(&cmd, 1, true, kIrqRxe, 15, rx, sizeof(rx), n, irq) || n < 8) {
        return false;
    }
    std::memcpy(out, rx, 8);
    return true;
}

bool St25r3916::type2_read(uint8_t page, uint8_t out[16]) {
    const uint8_t cmd[2] = {0x30, page};
    uint8_t rx[24] = {};
    size_t n = 0;
    uint32_t irq = 0;
    if (!write_reg(kRegIso14443aSettings, 0x00) || !modify_reg(kRegAuxiliaryDefinition, kNoCrcRx, 0) ||
        !set_no_response_timer_ms(10) || !transmit(cmd, sizeof(cmd), true, kIrqRxe, 15, rx, sizeof(rx), n, irq) ||
        n < 16) {
        return false;
    }
    std::memcpy(out, rx, 16);
    return true;
}

#else // !__linux__ : desktop simulator builds have no NFC hardware

bool St25r3916::power_on(std::string&) {
    return false;
}

void St25r3916::power_off() {}

bool St25r3916::read_register(uint8_t, uint8_t&) {
    return false;
}

bool St25r3916::open(std::string& error) {
    error = "ST25R3916 is only available on Linux";
    return false;
}

bool St25r3916::init_nfc_a(std::string& error) {
    error = "ST25R3916 is only available on Linux";
    return false;
}

void St25r3916::field_off() {}

bool St25r3916::type2_get_version(uint8_t*) {
    return false;
}

bool St25r3916::type2_read(uint8_t, uint8_t*) {
    return false;
}

bool St25r3916::detect_tag(Tag&, std::string& error) {
    error = "ST25R3916 is only available on Linux";
    return false;
}

void St25r3916::close() {}

#endif

} // namespace platform
