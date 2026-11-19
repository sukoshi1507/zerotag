/*
 * SPDX-License-Identifier: MIT
 */

#pragma once

#include "st25r3916.h"

#include <array>
#include <atomic>
#include <cstdint>
#include <string>
#include <thread>

namespace platform {

// Runs the NTAG215 emulation on a worker thread so a UI can show live counters, and hands back the
// final image so the caller can store what the reader wrote.
class EmulationSession {
public:
    static constexpr size_t kImageSize = 540;
    using Image = std::array<uint8_t, kImageSize>;

    struct Result {
        bool ok{false};
        std::string error;
        bool changed{false}; // the reader wrote to the tag
        Image image{};       // final content (equal to the initial one when !changed)
    };

    EmulationSession() = default;
    ~EmulationSession();
    EmulationSession(const EmulationSession&) = delete;
    EmulationSession& operator=(const EmulationSession&) = delete;

    // `chip` must be open and stay alive until finish() returns; nothing else may use it meanwhile.
    // seconds <= 0: run until request_stop().
    bool start(St25r3916& chip, const Image& image, int seconds, std::string& error);
    void request_stop();
    bool running() const { return running_.load(); }
    const St25r3916::EmulationStats& stats() const { return stats_; }

    // Waits for the worker to end (after request_stop() or the timeout) and returns the outcome.
    Result finish();

private:
    St25r3916::EmulationStats stats_;
    std::atomic<bool> stop_{false};
    std::atomic<bool> running_{false};
    std::thread worker_;
    Image initial_{};
    Image image_{};
    Result result_;
};

} // namespace platform
