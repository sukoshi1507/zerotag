/*
 * SPDX-License-Identifier: MIT
 */

#pragma once

#include "st25r3916.h"

#include <array>
#include <atomic>
#include <chrono>
#include <functional>
#include <mutex>
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
    // `rotate_uid`, when set, is called after each read session (see St25r3916::EmulationOptions).
    using RotateUid = std::function<bool(uint8_t* image)>;
    bool start(St25r3916& chip, const Image& image, int seconds, std::string& error, RotateUid rotate_uid = {});
    void request_stop();
    bool running() const { return running_.load(); }
    const St25r3916::EmulationStats& stats() const { return stats_; }

    // Autosave support: true once, with the current image, when the tag changed (a write or a UID rotation) and
    // nothing changed for `quiet_ms` (the reader is done with its burst of writes).
    bool poll_autosave(Image& out, uint32_t quiet_ms);
    // The image as the reader currently sees it (UID included).
    Image current_image() const;

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
    mutable std::mutex snapshot_mutex_;
    Image snapshot_{};
    bool dirty_{false};
    std::chrono::steady_clock::time_point last_change_{};
};

} // namespace platform
