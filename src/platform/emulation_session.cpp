/*
 * SPDX-License-Identifier: MIT
 */

#include "emulation_session.h"

namespace platform {

EmulationSession::~EmulationSession() {
    request_stop();
    if (worker_.joinable()) {
        worker_.join();
    }
}

bool EmulationSession::start(St25r3916& chip, const Image& image, int seconds, std::string& error) {
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
    result_ = Result{};
    stop_ = false;
    stats_.reads = 0;
    stats_.writes = 0;
    stats_.field = false;
    stats_.selected = false;
    running_ = true;
    worker_ = std::thread([this, &chip, seconds]() {
        std::string err;
        const bool ok = chip.emulate_ntag215(
            image_.data(), seconds, {}, err, [this]() { return stop_.load(); }, &stats_);
        result_.ok = ok;
        result_.error = err;
        result_.image = image_;
        result_.changed = (image_ != initial_);
        running_ = false;
    });
    return true;
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
