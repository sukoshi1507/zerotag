/*
 * SPDX-License-Identifier: MIT
 */

#pragma once

namespace platform {

// Short sound effects and a looping chiptune, synthesised in code (square / triangle / noise voices) and played
// through `aplay` (raw S16 mono on stdin): no audio library and no sound file. Everything is a no-op when sound
// is disabled, when aplay is missing and on the desktop simulator.
enum class Sound {
    Success, // "tu-du-tuuu": an amiibo was read
    Start,   // emulation started
    Error,   // something failed
    Blip,    // small UI confirmation (favourite...)
};

namespace audio {

void set_enabled(bool enabled);
bool enabled();

void play(Sound sound);     // fire and forget, returns immediately
void start_music();         // chiptune loop for the manual (does nothing if already playing)
void stop_music();

} // namespace audio
} // namespace platform
