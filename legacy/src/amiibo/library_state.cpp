/*
 * SPDX-License-Identifier: MIT
 */

#include "library_state.h"

#include <algorithm>
#include <cerrno>
#include <cstring>
#include <filesystem>
#include <fstream>

#include <fcntl.h>
#include <sys/stat.h>
#include <unistd.h>

namespace fs = std::filesystem;

namespace amiibo {
namespace {

constexpr size_t kMaxLines = 2000; // the file is tiny: anything bigger is not ours

bool valid_name(const std::string& name) {
    return !name.empty() && name.size() < 256 && name.find_first_of("\r\n") == std::string::npos;
}

void remove_name(std::vector<std::string>& list, const std::string& name) {
    list.erase(std::remove(list.begin(), list.end(), name), list.end());
}

bool write_all(int fd, const std::string& data) {
    size_t done = 0;
    while (done < data.size()) {
        const ssize_t n = ::write(fd, data.data() + done, data.size() - done);
        if (n < 0) {
            if (errno == EINTR) {
                continue;
            }
            return false;
        }
        done += static_cast<size_t>(n);
    }
    return true;
}

} // namespace

bool LibraryState::is_favorite(const std::string& name) const {
    return std::find(favorites.begin(), favorites.end(), name) != favorites.end();
}

bool LibraryState::toggle_favorite(const std::string& name) {
    if (!valid_name(name)) {
        return false;
    }
    if (is_favorite(name)) {
        remove_name(favorites, name);
        return false;
    }
    favorites.push_back(name);
    return true;
}

void LibraryState::mark_emulated(const std::string& name) {
    if (!valid_name(name)) {
        return;
    }
    remove_name(recent, name);
    recent.insert(recent.begin(), name);
    if (recent.size() > kRecentMax) {
        recent.resize(kRecentMax);
    }
}

void LibraryState::forget(const std::string& name) {
    remove_name(favorites, name);
    remove_name(recent, name);
}

bool load_library_state(const std::string& path, LibraryState& state, std::string& error) {
    error.clear();
    state = LibraryState{};
    std::error_code ec;
    if (!fs::exists(path, ec)) {
        return true;
    }
    std::ifstream input(path);
    if (!input) {
        error = "could not open " + path;
        return false;
    }

    std::string line;
    size_t lines = 0;
    while (std::getline(input, line) && ++lines <= kMaxLines) {
        const auto separator = line.find('=');
        if (separator == std::string::npos) {
            continue;
        }
        const std::string key = line.substr(0, separator);
        const std::string value = line.substr(separator + 1);
        if (!valid_name(value)) {
            continue;
        }
        if (key == "favorite") {
            if (!state.is_favorite(value)) {
                state.favorites.push_back(value);
            }
        } else if (key == "recent") {
            if (std::find(state.recent.begin(), state.recent.end(), value) == state.recent.end() &&
                state.recent.size() < LibraryState::kRecentMax) {
                state.recent.push_back(value);
            }
        }
    }
    return true;
}

bool save_library_state(const std::string& path, const LibraryState& state, std::string& error) {
    error.clear();
    const fs::path target(path);
    const fs::path parent = target.parent_path();
    if (!parent.empty()) {
        std::error_code ec;
        if (!fs::is_directory(parent, ec)) {
            fs::create_directories(parent, ec);
            if (ec) {
                error = "could not create " + parent.string() + ": " + ec.message();
                return false;
            }
            ::chmod(parent.c_str(), 0700);
        }
    }

    std::string content = "# ZeroTag library state (favourites, recent). Safe to delete.\n";
    for (const auto& name : state.favorites) {
        if (valid_name(name)) {
            content += "favorite=" + name + "\n";
        }
    }
    for (const auto& name : state.recent) {
        if (valid_name(name)) {
            content += "recent=" + name + "\n";
        }
    }

    const std::string tmp = path + ".tmp";
    const int fd = ::open(tmp.c_str(), O_WRONLY | O_CREAT | O_TRUNC, 0600);
    if (fd < 0) {
        error = "could not open " + tmp + ": " + std::strerror(errno);
        return false;
    }
    bool ok = write_all(fd, content) && ::fsync(fd) == 0;
    if (::close(fd) != 0) {
        ok = false;
    }
    if (!ok) {
        error = "write failed for " + tmp;
        ::unlink(tmp.c_str());
        return false;
    }
    if (::rename(tmp.c_str(), path.c_str()) != 0) {
        error = "could not replace " + path + ": " + std::strerror(errno);
        ::unlink(tmp.c_str());
        return false;
    }
    return true;
}

} // namespace amiibo
