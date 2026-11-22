/*
 * SPDX-License-Identifier: MIT
 */

#include "online_update.h"

#include <cerrno>
#include <chrono>
#include <cstdio>
#include <cstring>
#include <fstream>
#include <sstream>
#include <thread>

#include <fcntl.h>
#include <signal.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <unistd.h>

namespace platform {
namespace {

constexpr const char* kUrl = "https://www.amiiboapi.org/api/amiibo/";
constexpr int kTimeoutSeconds = 60;

// Last line of the file the child wrote its errors to, for the message shown to the user.
std::string last_line(const std::string& path) {
    std::ifstream f(path);
    std::string line, last;
    while (std::getline(f, line)) {
        if (!line.empty()) last = line;
    }
    return last.size() > 80 ? last.substr(0, 80) : last;
}

bool looks_like_amiibo_list(const std::string& path) {
    std::ifstream f(path, std::ios::binary);
    std::stringstream ss;
    ss << f.rdbuf();
    const std::string text = ss.str();
    const size_t first = text.find_first_not_of(" \t\r\n");
    return first != std::string::npos && text[first] == '{' && text.find("\"amiibo\"") != std::string::npos &&
           text.find("\"head\"") != std::string::npos;
}

} // namespace

bool download_amiibo_list(const std::string& dest, std::atomic<bool>& cancel, std::string& error) {
    const std::string part = dest + ".part";
    const std::string errfile = dest + ".err";
    ::unlink(part.c_str());

    const pid_t pid = fork();
    if (pid < 0) {
        error = std::string("fork: ") + std::strerror(errno);
        return false;
    }
    if (pid == 0) {
        const int err = ::open(errfile.c_str(), O_WRONLY | O_CREAT | O_TRUNC, 0600);
        const int null = ::open("/dev/null", O_RDWR);
        if (null >= 0) {
            ::dup2(null, STDIN_FILENO);
            ::dup2(null, STDOUT_FILENO);
        }
        if (err >= 0) ::dup2(err, STDERR_FILENO);
        const std::string limit = std::to_string(kTimeoutSeconds);
        ::execlp("curl", "curl", "--fail", "--silent", "--show-error", "--location", "--proto", "=https",
                 "--max-time", limit.c_str(), "--max-filesize", "8000000", "--user-agent", "zerotag-catalogue-update",
                 "--output", part.c_str(), kUrl, static_cast<char*>(nullptr));
        _exit(127); // curl is not installed
    }

    int status = 0;
    const auto start = std::chrono::steady_clock::now();
    for (;;) {
        const pid_t done = waitpid(pid, &status, WNOHANG);
        if (done == pid) break;
        if (done < 0) {
            error = std::string("waitpid: ") + std::strerror(errno);
            return false;
        }
        const bool late = std::chrono::steady_clock::now() - start > std::chrono::seconds(kTimeoutSeconds + 10);
        if (cancel.load() || late) {
            ::kill(pid, SIGTERM);
            waitpid(pid, &status, 0);
            ::unlink(part.c_str());
            ::unlink(errfile.c_str());
            error = cancel.load() ? "cancelled" : "timeout";
            return false;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(100));
    }

    bool ok = WIFEXITED(status) && WEXITSTATUS(status) == 0;
    if (!ok) {
        if (WIFEXITED(status) && WEXITSTATUS(status) == 127) {
            error = "curl is not installed (sudo apt install curl)";
        } else {
            const std::string why = last_line(errfile);
            error = why.empty() ? "download failed" : why;
        }
    } else if (!looks_like_amiibo_list(part)) {
        error = "unexpected answer from the server";
        ok = false;
    } else if (::rename(part.c_str(), dest.c_str()) != 0) {
        error = std::string("rename: ") + std::strerror(errno);
        ok = false;
    }
    ::unlink(errfile.c_str());
    if (!ok) ::unlink(part.c_str());
    return ok;
}

} // namespace platform
