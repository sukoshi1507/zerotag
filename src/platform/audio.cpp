/*
 * SPDX-License-Identifier: MIT
 */

#include "audio.h"

#include <algorithm>
#include <atomic>
#include <cmath>
#include <cstdint>
#include <mutex>
#include <thread>
#include <vector>

#ifdef __linux__
#include <csignal>
#include <fcntl.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>
#endif

namespace platform {
namespace audio {
namespace {

constexpr int kRate = 22050;
constexpr double kPi = 3.14159265358979323846;

std::atomic<bool> g_enabled{true};

enum class Wave { Square50, Square25, Triangle, Noise };

double midi_hz(int note) {
    return 440.0 * std::pow(2.0, (note - 69) / 12.0);
}

// Adds one note to `pcm` starting at `start_ms`: linear attack of 3 ms, exponential-ish decay to silence.
void add_note(std::vector<float>& pcm, double start_ms, double length_ms, int note, Wave wave, float volume,
              double decay = 3.0) {
    const size_t first = static_cast<size_t>(start_ms * kRate / 1000.0);
    const size_t count = static_cast<size_t>(length_ms * kRate / 1000.0);
    if (pcm.size() < first + count) {
        pcm.resize(first + count, 0.0f);
    }
    const double hz = midi_hz(note);
    uint32_t lfsr = 0xACE1u;
    float noise_value = 0.0f;
    for (size_t i = 0; i < count; ++i) {
        const double t = static_cast<double>(i) / kRate;
        const double phase = std::fmod(t * hz, 1.0);
        float s = 0.0f;
        switch (wave) {
        case Wave::Square50: s = phase < 0.5 ? 1.0f : -1.0f; break;
        case Wave::Square25: s = phase < 0.25 ? 1.0f : -1.0f; break;
        case Wave::Triangle: s = static_cast<float>(phase < 0.5 ? 4.0 * phase - 1.0 : 3.0 - 4.0 * phase); break;
        case Wave::Noise:
            if (i % 6 == 0) { // sample-and-hold noise, a bit crunchy
                const uint32_t bit = ((lfsr >> 0) ^ (lfsr >> 2) ^ (lfsr >> 3) ^ (lfsr >> 5)) & 1u;
                lfsr = (lfsr >> 1) | (bit << 15);
                noise_value = (lfsr & 1u) ? 1.0f : -1.0f;
            }
            s = noise_value;
            break;
        }
        const double attack = std::min(1.0, t / 0.003);
        const double env = attack * std::exp(-decay * static_cast<double>(i) / static_cast<double>(count));
        pcm[first + i] += s * volume * static_cast<float>(env);
    }
}

std::vector<int16_t> to_pcm(const std::vector<float>& mix) {
    std::vector<int16_t> out(mix.size());
    for (size_t i = 0; i < mix.size(); ++i) {
        const float v = std::max(-1.0f, std::min(1.0f, mix[i]));
        out[i] = static_cast<int16_t>(v * 30000.0f);
    }
    return out;
}

std::vector<int16_t> make_sound(Sound sound) {
    std::vector<float> m;
    switch (sound) {
    case Sound::Success: // C5 G5 then a long C6 with a fifth under it: "tu-du-tuuu"
        add_note(m, 0, 95, 72, Wave::Square25, 0.30f, 2.0);
        add_note(m, 100, 95, 79, Wave::Square25, 0.30f, 2.0);
        add_note(m, 200, 520, 84, Wave::Square25, 0.30f, 2.6);
        add_note(m, 200, 520, 76, Wave::Triangle, 0.30f, 2.6);
        break;
    case Sound::Start: // rising blips
        add_note(m, 0, 70, 67, Wave::Square50, 0.25f, 1.5);
        add_note(m, 75, 70, 72, Wave::Square50, 0.25f, 1.5);
        add_note(m, 150, 140, 76, Wave::Square50, 0.25f, 2.5);
        break;
    case Sound::Error: // falling buzz
        add_note(m, 0, 120, 55, Wave::Square50, 0.28f, 1.0);
        add_note(m, 130, 260, 48, Wave::Square25, 0.28f, 2.0);
        break;
    case Sound::Blip:
        add_note(m, 0, 60, 88, Wave::Square25, 0.22f, 3.0);
        break;
    }
    return to_pcm(m);
}

// A 7 s loop in A minor: arpeggio, bass, hats and a small lead, tracker style (16th notes at 140 bpm).
std::vector<int16_t> make_music() {
    const double step = 60000.0 / 140.0 / 4.0; // ms per 16th
    // Chords (Am F C G): root note of the bass and the three notes of the arpeggio.
    const int bass[4] = {45, 41, 48, 43};
    const int chord[4][3] = {{57, 60, 64}, {53, 57, 60}, {60, 64, 67}, {55, 59, 62}};
    // Lead: two bars of melody, one value per 16th (0 = rest), repeated twice with a variation.
    const int lead[2][32] = {
        {76, 0, 0, 76, 0, 79, 0, 76, 74, 0, 0, 72, 0, 0, 0, 0, 72, 0, 0, 72, 0, 76, 0, 72, 71, 0, 0, 67, 0, 0, 0, 0},
        {81, 0, 0, 81, 0, 79, 0, 76, 77, 0, 0, 76, 0, 0, 72, 0, 76, 0, 0, 79, 0, 83, 0, 79, 74, 0, 71, 0, 67, 0, 0, 0},
    };
    std::vector<float> m;
    for (int round = 0; round < 2; ++round) {
        for (int s = 0; s < 32; ++s) {
            const double t = (round * 32 + s) * step;
            const int c = s / 8;
            add_note(m, t, step * 0.9, chord[c][s % 3] + 12, Wave::Square25, 0.10f, 2.2);        // arpeggio
            if (s % 4 == 0) {
                add_note(m, t, step * 3.6, bass[c], Wave::Triangle, 0.30f, 1.6);                   // bass
            }
            if (s % 2 == 1) {
                add_note(m, t, step * 0.5, 60, Wave::Noise, 0.07f, 4.0);                           // hat
            }
            if (s % 8 == 0) {
                add_note(m, t, step * 1.5, 40, Wave::Noise, 0.16f, 5.0);                           // kick-ish thump
            }
            if (lead[round][s] != 0) {
                add_note(m, t, step * 2.0, lead[round][s], Wave::Square50, 0.14f, 2.0);            // lead
            }
        }
    }
    return to_pcm(m);
}

#ifdef __linux__

std::mutex g_music_mutex;
pid_t g_music_pid = -1;
std::atomic<bool> g_music_stop{false};
bool g_music_running = false;

// Starts `aplay` reading raw PCM on its stdin. Returns the child pid and the write end of the pipe.
bool spawn_aplay(pid_t& pid, int& write_fd) {
    int fds[2];
    if (pipe2(fds, O_CLOEXEC) != 0) {
        return false;
    }
    pid = fork();
    if (pid < 0) {
        close(fds[0]);
        close(fds[1]);
        return false;
    }
    if (pid == 0) {
        dup2(fds[0], STDIN_FILENO);
        const int null_fd = open("/dev/null", O_WRONLY);
        if (null_fd >= 0) {
            dup2(null_fd, STDOUT_FILENO);
            dup2(null_fd, STDERR_FILENO);
        }
        execlp("aplay", "aplay", "-q", "-t", "raw", "-f", "S16_LE", "-r", "22050", "-c", "1", static_cast<char*>(nullptr));
        _exit(127);
    }
    close(fds[0]);
    write_fd = fds[1];
    return true;
}

bool write_all(int fd, const uint8_t* data, size_t len) {
    while (len > 0) {
        const ssize_t n = ::write(fd, data, len);
        if (n < 0) {
            return false; // EPIPE: aplay is gone or was stopped
        }
        data += n;
        len -= static_cast<size_t>(n);
    }
    return true;
}

void ignore_sigpipe_once() {
    static std::once_flag flag;
    std::call_once(flag, [] { std::signal(SIGPIPE, SIG_IGN); });
}

void play_once(std::vector<int16_t> pcm) {
    ignore_sigpipe_once();
    std::thread([pcm = std::move(pcm)] {
        pid_t pid = -1;
        int fd = -1;
        if (!spawn_aplay(pid, fd)) {
            return;
        }
        write_all(fd, reinterpret_cast<const uint8_t*>(pcm.data()), pcm.size() * sizeof(int16_t));
        close(fd);
        waitpid(pid, nullptr, 0);
    }).detach();
}

void music_loop(std::vector<int16_t> pcm) {
    while (!g_music_stop) {
        pid_t pid = -1;
        int fd = -1;
        if (!spawn_aplay(pid, fd)) {
            break;
        }
        {
            std::lock_guard<std::mutex> lock(g_music_mutex);
            g_music_pid = pid;
        }
        // The same buffer is written again and again into one aplay: the loop has no gap.
        while (!g_music_stop &&
               write_all(fd, reinterpret_cast<const uint8_t*>(pcm.data()), pcm.size() * sizeof(int16_t))) {
        }
        close(fd);
        {
            std::lock_guard<std::mutex> lock(g_music_mutex);
            if (g_music_pid == pid) {
                kill(pid, SIGTERM);
                g_music_pid = -1;
            }
        }
        waitpid(pid, nullptr, 0);
        if (!g_music_stop) {
            break; // aplay died by itself (no device): do not respawn it in a tight loop
        }
    }
    std::lock_guard<std::mutex> lock(g_music_mutex);
    g_music_running = false;
}

#endif // __linux__

} // namespace

void set_enabled(bool value) {
    g_enabled = value;
    if (!value) {
        stop_music();
    }
}

bool enabled() {
    return g_enabled;
}

void play(Sound sound) {
#ifdef __linux__
    if (!g_enabled) {
        return;
    }
    play_once(make_sound(sound));
#else
    (void)sound;
#endif
}

void start_music() {
#ifdef __linux__
    if (!g_enabled) {
        return;
    }
    ignore_sigpipe_once();
    std::lock_guard<std::mutex> lock(g_music_mutex);
    if (g_music_running) {
        return;
    }
    g_music_running = true;
    g_music_stop = false;
    std::thread([] { music_loop(make_music()); }).detach();
#endif
}

void stop_music() {
#ifdef __linux__
    g_music_stop = true;
    std::lock_guard<std::mutex> lock(g_music_mutex);
    if (g_music_pid > 0) {
        kill(g_music_pid, SIGTERM); // the writer then gets EPIPE and leaves its loop
    }
#endif
}

} // namespace audio
} // namespace platform
