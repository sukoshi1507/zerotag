/*
 * SPDX-License-Identifier: MIT
 *
 * Optional online update of the catalogue: downloads the amiibo list of https://www.amiiboapi.org (the same
 * source as the bundled catalogue) so that newly announced figures show up. Off by default; nothing but the
 * public list is requested and nothing about the user or the device is sent.
 */

#pragma once

#include <atomic>
#include <string>

namespace platform {

// Downloads https://www.amiiboapi.org/api/amiibo/ to `dest` with curl (no shell involved; HTTPS only, 60 s
// limit). The file is written to `dest + ".part"`, checked to be a JSON object containing an "amiibo" array
// and renamed over `dest` only then, so a failed download never damages the previous copy. `cancel` is
// polled while waiting. Returns false with a short reason in `error`.
bool download_amiibo_list(const std::string& dest, std::atomic<bool>& cancel, std::string& error);

} // namespace platform
