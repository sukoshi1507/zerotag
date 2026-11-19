// SPDX-License-Identifier: MIT
// Plays every ZeroTag sound, then 8 s of the chiptune loop. Usage: audio_test [music_seconds]
#include "audio.h"

#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <thread>

int main(int argc, char** argv) {
    using namespace std::chrono_literals;
    const int music = argc > 1 ? std::atoi(argv[1]) : 8;
    const struct { const char* name; platform::Sound sound; } all[] = {
        {"blip", platform::Sound::Blip}, {"start", platform::Sound::Start},
        {"error", platform::Sound::Error}, {"success", platform::Sound::Success}};
    for (const auto& s : all) {
        std::printf("%s\n", s.name);
        platform::audio::play(s.sound);
        std::this_thread::sleep_for(1500ms);
    }
    if (music > 0) {
        std::printf("music %d s\n", music);
        platform::audio::start_music();
        std::this_thread::sleep_for(std::chrono::seconds(music));
        platform::audio::stop_music();
        std::this_thread::sleep_for(500ms);
    }
    return 0;
}
