/*
 * SPDX-License-Identifier: MIT
 */

#pragma once

#include <cstddef>
#include <cstdint>
#include <functional>
#include <string>

namespace platform {

// ST25R3916 NFC front end on the M5Stack Cap CC1101, driven from Linux userspace.
//
// Topology verified on a CardputerZero (see README):
//  - SPI0, mode 1, kernel-managed CS on GPIO22. /dev/spidev0.2 only exists once the
//    `spi0-spidev2-gpio22-overlay` device-tree overlay is loaded (root, done by the launcher).
//  - The Cap is unpowered until GPIO26 (POWER_EN) is high and the
//    /sys/class/leds/ext_5v_out LED is on. Without power every register reads 0x00.
//  - IRQ line: rising edge, GPIO number still to be confirmed.
struct St25r3916Config {
    std::string spi_device{"/dev/spidev0.2"};
    std::string gpio_chip{"/dev/gpiochip0"};
    int power_gpio{26};
    std::string power_led{"/sys/class/leds/ext_5v_out/brightness"};
    int irq_gpio{-1};
    uint32_t spi_speed_hz{1000000};
};

class St25r3916 {
public:
    explicit St25r3916(St25r3916Config config = {});
    ~St25r3916();

    St25r3916(const St25r3916&) = delete;
    St25r3916& operator=(const St25r3916&) = delete;

    const St25r3916Config& config() const;

    // Powers the Cap, opens the SPI device and checks the IC identity register.
    bool open(std::string& error);
    void close();
    bool is_open() const;

    // Raw value of register 0x3F (IC identity): type in bits 7..3 (0x28 expected), revision in 2..0.
    uint8_t ic_identity() const;
    int revision() const;

    // Reads one register of space A. Returns false on a bus error.
    bool read_register(uint8_t address, uint8_t& value);

    // --- NFC-A reader mode (ISO 14443-3A) ---
    struct Tag {
        uint8_t uid[10]{};
        uint8_t uid_len{0};
        uint16_t atqa{0};
        uint8_t sak{0};
    };

    // Brings the chip out of reset, starts the oscillator and configures NFC-A at 106 kbit/s.
    bool init_nfc_a(std::string& error);
    void field_off();

    // NFC Forum Type 2 (NTAG) commands on a tag selected by detect_tag(). CRC is handled by the chip.
    bool type2_get_version(uint8_t out[8]);          // NTAG GET_VERSION (0x60)
    bool type2_read(uint8_t page, uint8_t out[16]);  // READ (0x30): 4 pages starting at `page`

    // REQA/WUPA + anticollision + select (cascade levels 1..3). Returns false when no tag answers.
    bool detect_tag(Tag& tag, std::string& error);

    // --- NFC-A card emulation (NTAG215 / Type 2 tag) ---
    // The chip answers REQA/WUPA/anticollision/select by itself from its passive-target memory; this
    // method serves the Type 2 commands (READ, FAST_READ, GET_VERSION, READ_SIG, PWD_AUTH, WRITE, HLTA)
    // from `image` (540 bytes, writes from the reader modify it in place). It runs for `seconds`
    // (<= 0: until `stop` returns true) and reports what happens through `log`.
    using EmulationLog = std::function<void(const std::string&)>;
    bool emulate_ntag215(uint8_t* image, int seconds, const EmulationLog& log, std::string& error,
                         const std::function<bool()>& stop = {});

private:
    St25r3916Config config_;
    int spi_fd_{-1};
    int power_line_fd_{-1};
    uint8_t identity_{0};
    bool led_on_{false};

    bool power_on(std::string& error);
    void power_off();

    bool write_regs(uint8_t reg, const uint8_t* data, size_t len, bool space_b = false);
    bool read_regs(uint8_t reg, uint8_t* data, size_t len, bool space_b = false);
    bool write_reg(uint8_t reg, uint8_t value, bool space_b = false);
    bool read_reg(uint8_t reg, uint8_t& value, bool space_b = false);
    bool modify_reg(uint8_t reg, uint8_t clear_mask, uint8_t set_mask, bool space_b = false);
    bool direct_command(uint8_t cmd);
    bool transfer(const uint8_t* tx, uint8_t* rx, size_t len);
    bool read_interrupts(uint32_t& value);
    bool clear_interrupts();
    uint32_t wait_interrupt(uint32_t mask, int timeout_ms);
    bool fifo_size(uint16_t& bytes);
    bool read_fifo(uint8_t* buf, size_t max, size_t& actual);
    bool write_fifo(const uint8_t* buf, size_t len);
    bool set_no_response_timer_ms(int ms);
    bool transmit(const uint8_t* frame, size_t len, bool with_crc, uint32_t wait_mask, int timeout_ms,
                  uint8_t* rx, size_t rx_max, size_t& rx_len, uint32_t& irq);
    bool request(bool wakeup, uint16_t& atqa);
    bool anticollision(uint8_t level, uint8_t out[5], std::string& error);
    bool select_level(uint8_t level, const uint8_t id[5], uint8_t& sak);
    bool enable_oscillator();

    // Emulation internals (st25r3916_emulation.cpp)
    bool write_mask_interrupts(uint32_t mask);
    bool read_mask_interrupts(uint32_t& mask);
    bool modify_mask_interrupts(uint32_t unmask, uint32_t mask);
    bool load_pt_memory_a(const uint8_t* data, size_t len);
    bool transmit_bits(const uint8_t* frame, size_t bits, bool with_crc);
};

} // namespace platform
