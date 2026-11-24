/*
 * SPDX-License-Identifier: MIT
 */

#include "emulation_session.h"

#include <algorithm>

namespace platform {

EmulationSession::~EmulationSession() {
    request_stop();
    if (worker_.joinable()) {
        worker_.join();
    }
}

bool EmulationSession::start(St25r3916& chip, const Image& image, int seconds, std::string& error,
                             RotateUid rotate_uid) {
    if (running_.load() || worker_.joinable()) {
        error = "an emulation is already running";
        return false;
    }
    if (!chip.is_open()) {
        error = "chip not open";
        return false;
    }
    initial_ = image;
    image_ = image;
    {
        std::lock_guard<std::mutex> lock(snapshot_mutex_);
        snapshot_ = image;
        dirty_ = false;
    }
    result_ = Result{};
    stop_ = false;
    stats_.reads = 0;
    stats_.writes = 0;
    stats_.rotations = 0;
    stats_.field = false;
    stats_.selected = false;
    running_ = true;
    worker_ = std::thread([this, &chip, seconds, rotate_uid = std::move(rotate_uid)]() {
        std::string err;
        St25r3916::EmulationOptions options;
        options.stop = [this]() { return stop_.load(); };
        options.stats = &stats_;
        options.rotate_uid = rotate_uid;
        options.on_change = [this](const uint8_t* data) {
            std::lock_guard<std::mutex> lock(snapshot_mutex_);
            std::copy(data, data + kImageSize, snapshot_.begin());
            dirty_ = true;
            last_change_ = std::chrono::steady_clock::now();
        };
        const bool ok = chip.emulate_ntag215(image_.data(), seconds, options, err);
        result_.ok = ok;
        result_.error = err;
        result_.image = image_;
        result_.changed = (image_ != initial_);
        running_ = false;
    });
    return true;
}

bool EmulationSession::poll_autosave(Image& out, uint32_t quiet_ms) {
    std::lock_guard<std::mutex> lock(snapshot_mutex_);
    if (!dirty_ || std::chrono::steady_clock::now() - last_change_ < std::chrono::milliseconds(quiet_ms)) {
        return false;
    }
    out = snapshot_;
    dirty_ = false;
    return true;
}

EmulationSession::Image EmulationSession::current_image() const {
    std::lock_guard<std::mutex> lock(snapshot_mutex_);
    return snapshot_;
}

void EmulationSession::request_stop() {
    stop_ = true;
}

EmulationSession::Result EmulationSession::finish() {
    if (worker_.joinable()) {
        worker_.join();
    }
    return result_;
}

} // namespace platform
