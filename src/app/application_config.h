/*
 * SPDX-FileCopyrightText: 2026 M5Stack Technology CO LTD
 *
 * SPDX-License-Identifier: MIT
 */

#pragma once

#include <string>

namespace app {

struct ApplicationConfig {
    bool dark_mode = true;
    std::string language = "fr"; // fr, en, ja
    bool uid_rotation = false;   // new UID after each read session
};

bool load_application_config(const std::string& path,
                             ApplicationConfig& config,
                             std::string& error);
// Writes (or updates) one key of the [application] section, keeping the rest of the file.
bool save_setting(const std::string& path, const std::string& key, const std::string& value, std::string& error);
bool save_application_config(const std::string& path,
                             const ApplicationConfig& config,
                             std::string& error);

} // namespace app
