/*
 * SPDX-License-Identifier: MIT
 *
 * Runs the root helper that loads the SPI overlay of the Cap (/usr/lib/zerotag/cap-setup) through sudo, with
 * a password typed in the application.
 */

#pragma once

#include <string>

namespace platform {

// True when the SPI device of the Cap exists (the overlay is loaded).
bool spi_available();

struct CapSetupResult {
    enum class Status { Ok, PasswordRefused, Failed };
    Status status{Status::Failed};
    std::string message; // last line printed by the helper (or the reason), never contains the password
};

// Runs `sudo -S -p "" /usr/lib/zerotag/cap-setup`, feeding `password` on its standard input (an empty string
// works when sudo needs none). No shell is involved and the password never appears in an argument, an
// environment variable or a log. It is overwritten in memory and cleared before returning. Blocks for a few
// seconds at most.
CapSetupResult run_cap_setup(std::string& password);

} // namespace platform
