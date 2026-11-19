/*
 * SPDX-License-Identifier: MIT
 */

#include "cap_setup.h"

#include "st25r3916.h"

#include <algorithm>
#include <cerrno>
#include <chrono>
#include <csignal>
#include <cstdlib>
#include <cstring>
#include <filesystem>

#if defined(__linux__)
#include <fcntl.h>
#include <poll.h>
#include <sys/wait.h>
#include <unistd.h>
#endif

namespace platform {
namespace {

constexpr const char* kHelper = "/usr/lib/zerotag/cap-setup";
constexpr int kTimeoutMs = 20000;

void wipe(std::string& s) {
#if defined(__linux__)
    if (!s.empty()) {
        explicit_bzero(&s[0], s.size());
    }
#endif
    s.clear();
}

std::string last_line(const std::string& text) {
    std::string t = text;
    while (!t.empty() && (t.back() == '\n' || t.back() == '\r' || t.back() == ' ')) {
        t.pop_back();
    }
    const auto pos = t.find_last_of('\n');
    return pos == std::string::npos ? t : t.substr(pos + 1);
}

bool contains(const std::string& text, const char* needle) {
    return text.find(needle) != std::string::npos;
}

} // namespace

bool spi_available() {
    std::error_code ec;
    return std::filesystem::exists(St25r3916Config{}.spi_device, ec);
}

CapSetupResult run_cap_setup(std::string& password) {
    CapSetupResult result;
#if !defined(__linux__)
    wipe(password);
    result.message = "unsupported platform";
    return result;
#else
    int to_child[2] = {-1, -1};
    int from_child[2] = {-1, -1};
    if (pipe2(to_child, O_CLOEXEC) != 0 || pipe2(from_child, O_CLOEXEC) != 0) {
        wipe(password);
        result.message = std::strerror(errno);
        return result;
    }

    std::signal(SIGPIPE, SIG_IGN); // the child may exit before reading the password

    const pid_t pid = fork();
    if (pid < 0) {
        wipe(password);
        result.message = std::strerror(errno);
        close(to_child[0]);
        close(to_child[1]);
        close(from_child[0]);
        close(from_child[1]);
        return result;
    }
    if (pid == 0) {
        dup2(to_child[0], STDIN_FILENO);
        dup2(from_child[1], STDOUT_FILENO);
        dup2(from_child[1], STDERR_FILENO);
        setenv("LC_ALL", "C", 1); // stable messages to recognise
        execlp("sudo", "sudo", "-S", "-p", "", kHelper, static_cast<char*>(nullptr));
        _exit(127);
    }

    close(to_child[0]);
    close(from_child[1]);

    // Feed the password (one line), then close the pipe.
    std::string line = password;
    line.push_back('\n');
    wipe(password);
    const char* data = line.data();
    size_t left = line.size();
    while (left > 0) {
        const ssize_t n = write(to_child[1], data, left);
        if (n <= 0) {
            break;
        }
        data += n;
        left -= static_cast<size_t>(n);
    }
    wipe(line);
    close(to_child[1]);

    // Collect what the helper prints, with a timeout.
    std::string output;
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::milliseconds(kTimeoutMs);
    bool timed_out = false;
    for (;;) {
        const auto left_ms = std::chrono::duration_cast<std::chrono::milliseconds>(deadline - std::chrono::steady_clock::now()).count();
        if (left_ms <= 0) {
            timed_out = true;
            break;
        }
        pollfd pfd{from_child[0], POLLIN, 0};
        const int rc = poll(&pfd, 1, static_cast<int>(std::min<long long>(left_ms, 500)));
        if (rc > 0) {
            char buf[256];
            const ssize_t n = read(from_child[0], buf, sizeof(buf));
            if (n <= 0) {
                break; // EOF: the child closed its output
            }
            if (output.size() < 2048) {
                output.append(buf, static_cast<size_t>(n));
            }
        }
    }
    close(from_child[0]);

    int status = 0;
    if (timed_out) {
        kill(pid, SIGKILL);
        waitpid(pid, &status, 0);
        result.message = "timeout";
        return result;
    }
    waitpid(pid, &status, 0);
    const int code = WIFEXITED(status) ? WEXITSTATUS(status) : -1;

    if (code == 0) {
        result.status = CapSetupResult::Status::Ok;
        result.message = last_line(output);
        return result;
    }
    // sudo prints these when the password is wrong or one is needed but cannot be given.
    if (contains(output, "incorrect password") || contains(output, "Sorry") ||
        contains(output, "a password is required") || contains(output, "no password was provided") ||
        contains(output, "is not in the sudoers") || contains(output, "may not run sudo")) {
        result.status = CapSetupResult::Status::PasswordRefused;
    } else {
        result.status = CapSetupResult::Status::Failed;
    }
    result.message = code == 127 ? "sudo not found" : last_line(output);
    return result;
#endif
}

} // namespace platform
