/*
 * SPDX-License-Identifier: MIT
 */

#pragma once

#include <cstddef>
#include <string>
#include <vector>

namespace amiibo {

// Per-user state of the library: favourites and recently emulated backups. Backups are named by their file
// name (not their path), so the state survives a move of the library folder. The .bin files are never touched.
struct LibraryState {
    static constexpr size_t kRecentMax = 10;

    std::vector<std::string> favorites; // in the order they were marked
    std::vector<std::string> recent;    // most recent first, at most kRecentMax

    bool is_favorite(const std::string& name) const;
    // Returns true when `name` became a favourite, false when it was removed from them.
    bool toggle_favorite(const std::string& name);
    // Moves `name` to the top of the recent list.
    void mark_emulated(const std::string& name);
    // Drops `name` from both lists (the file was deleted).
    void forget(const std::string& name);
};

// A missing file is an empty state (not an error). Unknown or malformed lines are ignored.
bool load_library_state(const std::string& path, LibraryState& state, std::string& error);
// Atomic write (temporary file, fsync, rename), mode 0600, parent folder created with mode 0700 when missing.
bool save_library_state(const std::string& path, const LibraryState& state, std::string& error);

} // namespace amiibo
