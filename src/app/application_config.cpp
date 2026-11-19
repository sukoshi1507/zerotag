/*
 * SPDX-FileCopyrightText: 2026 M5Stack Technology CO LTD
 *
 * SPDX-License-Identifier: MIT
 */

#include "application_config.h"

#include <algorithm>
#include <cctype>
#include <filesystem>
#include <fstream>
#include <vector>

namespace app {
namespace {

std::string trim(std::string value) {
    const auto is_not_space = [](unsigned char character) {
        return !std::isspace(character);
    };

    value.erase(value.begin(), std::find_if(value.begin(), value.end(), is_not_space));
    value.erase(std::find_if(value.rbegin(), value.rend(), is_not_space).base(), value.end());
    return value;
}

std::string lowercase(std::string value) {
    std::transform(value.begin(), value.end(), value.begin(), [](unsigned char character) {
        return static_cast<char>(std::tolower(character));
    });
    return value;
}

bool parse_bool(const std::string& value, bool& result) {
    const auto normalized = lowercase(trim(value));
    if (normalized == "yes" || normalized == "true" || normalized == "1") {
        result = true;
        return true;
    }
    if (normalized == "no" || normalized == "false" || normalized == "0") {
        result = false;
        return true;
    }
    return false;
}

bool is_section(const std::string& line, const std::string& expected) {
    const auto normalized = trim(line);
    return normalized.size() >= 2 && normalized.front() == '[' && normalized.back() == ']' &&
           lowercase(trim(normalized.substr(1, normalized.size() - 2))) == expected;
}

bool is_key(const std::string& line, const std::string& expected) {
    const auto separator = line.find('=');
    return separator != std::string::npos &&
           lowercase(trim(line.substr(0, separator))) == expected;
}

} // namespace

bool load_application_config(const std::string& path,
                             ApplicationConfig& config,
                             std::string& error) {
    error.clear();
    std::ifstream input(path);
    if (!input) {
        error = "could not open file";
        return false;
    }

    std::string section;
    std::string line;
    std::size_t line_number = 0;
    while (std::getline(input, line)) {
        ++line_number;
        line = trim(line);
        if (line.empty() || line.front() == '#' || line.front() == ';') {
            continue;
        }

        if (line.front() == '[' && line.back() == ']') {
            section = lowercase(trim(line.substr(1, line.size() - 2)));
            continue;
        }

        const auto separator = line.find('=');
        if (separator == std::string::npos) {
            continue;
        }

        const auto key = lowercase(trim(line.substr(0, separator)));
        if (section == "application" && key == "uid_rotation") {
            bool v = false;
            if (parse_bool(line.substr(separator + 1), v)) {
                config.uid_rotation = v;
            }
        }
        if (section == "application" && key == "sound") {
            bool v = true;
            if (parse_bool(line.substr(separator + 1), v)) {
                config.sound = v;
            }
        }
        if (section == "application" && key == "language") {
            config.language = lowercase(trim(line.substr(separator + 1)));
        }
        if (section == "application" && key == "dark_mode") {
            if (!parse_bool(line.substr(separator + 1), config.dark_mode)) {
                error = "invalid dark_mode value at line " + std::to_string(line_number);
                return false;
            }
        }
    }

    return true;
}

bool save_application_config(const std::string& path,
                             const ApplicationConfig& config,
                             std::string& error) {
    error.clear();

    const std::filesystem::path config_path(path);
    const auto parent = config_path.parent_path();
    if (!parent.empty()) {
        std::error_code filesystem_error;
        std::filesystem::create_directories(parent, filesystem_error);
        if (filesystem_error) {
            error = "could not create parent directory: " + filesystem_error.message();
            return false;
        }
    }

    std::vector<std::string> lines;
    std::ifstream input(path);
    std::string line;
    while (std::getline(input, line)) {
        lines.push_back(line);
    }

    std::vector<std::string> updated;
    bool in_application_section = false;
    bool found_application_section = false;
    bool wrote_dark_mode = false;
    const std::string dark_mode_line = config.dark_mode ? "dark_mode=yes" : "dark_mode=no";

    for (const auto& current_line : lines) {
        const auto normalized = trim(current_line);
        const bool section_header = normalized.size() >= 2 && normalized.front() == '[' &&
                                    normalized.back() == ']';
        if (section_header) {
            if (in_application_section && !wrote_dark_mode) {
                updated.push_back(dark_mode_line);
                wrote_dark_mode = true;
            }
            in_application_section = is_section(current_line, "application");
            found_application_section = found_application_section || in_application_section;
        }

        if (in_application_section && is_key(current_line, "dark_mode")) {
            if (!wrote_dark_mode) {
                updated.push_back(dark_mode_line);
                wrote_dark_mode = true;
            }
            continue;
        }
        updated.push_back(current_line);
    }

    if (!wrote_dark_mode) {
        if (!updated.empty() && !updated.back().empty()) {
            updated.emplace_back();
        }
        if (!found_application_section) {
            updated.emplace_back("[application]");
        }
        updated.push_back(dark_mode_line);
    }

    std::ofstream output(path, std::ios::trunc);
    if (!output) {
        error = "could not open file for writing";
        return false;
    }
    for (const auto& updated_line : updated) {
        output << updated_line << '\n';
    }
    if (!output) {
        error = "failed while writing file";
        return false;
    }

    return true;
}

bool save_setting(const std::string& path, const std::string& key, const std::string& value, std::string& error) {
    error.clear();
    const std::filesystem::path config_path(path);
    if (!config_path.parent_path().empty()) {
        std::error_code ec;
        std::filesystem::create_directories(config_path.parent_path(), ec);
        if (ec) {
            error = "could not create parent directory: " + ec.message();
            return false;
        }
    }

    std::vector<std::string> lines;
    {
        std::ifstream input(path);
        std::string line;
        while (std::getline(input, line)) {
            lines.push_back(line);
        }
    }

    // Find the [application] section and its end.
    std::size_t begin = std::string::npos;
    std::size_t end = lines.size();
    for (std::size_t i = 0; i < lines.size(); ++i) {
        if (begin == std::string::npos) {
            if (is_section(lines[i], "application")) {
                begin = i + 1;
            }
        } else {
            const auto n = trim(lines[i]);
            if (n.size() >= 2 && n.front() == '[' && n.back() == ']') {
                end = i;
                break;
            }
        }
    }
    const std::string entry = key + "=" + value;
    if (begin == std::string::npos) {
        if (!lines.empty() && !lines.back().empty()) {
            lines.emplace_back();
        }
        lines.emplace_back("[application]");
        lines.push_back(entry);
    } else {
        bool replaced = false;
        for (std::size_t i = begin; i < end; ++i) {
            if (is_key(lines[i], key)) {
                lines[i] = entry;
                replaced = true;
                break;
            }
        }
        if (!replaced) {
            std::size_t pos = end;
            while (pos > begin && trim(lines[pos - 1]).empty()) {
                --pos;
            }
            lines.insert(lines.begin() + static_cast<std::ptrdiff_t>(pos), entry);
        }
    }

    std::ofstream output(path, std::ios::trunc);
    if (!output) {
        error = "could not open file for writing";
        return false;
    }
    for (const auto& l : lines) {
        output << l << '\n';
    }
    if (!output) {
        error = "failed while writing file";
        return false;
    }
    return true;
}

} // namespace app
