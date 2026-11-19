/*
 * SPDX-License-Identifier: MIT
 */

#include "i18n.h"

#include <cstdarg>
#include <cstdio>
#include <vector>

namespace app::i18n {
namespace {

const char* const kTable[][kLangCount] = {
#define X(name, fr, en, ja) {fr, en, ja},
#include "i18n_strings.def"
#undef X
};

Lang g_lang = Lang::Fr;

} // namespace

void set_language(Lang lang) {
    g_lang = lang;
}

Lang language() {
    return g_lang;
}

Lang next_language(Lang lang) {
    return static_cast<Lang>((static_cast<int>(lang) + 1) % kLangCount);
}

const char* code(Lang lang) {
    switch (lang) {
    case Lang::En: return "en";
    case Lang::Ja: return "ja";
    default: return "fr";
    }
}

const char* native_name(Lang lang) {
    switch (lang) {
    case Lang::En: return "English";
    case Lang::Ja: return "日本語";
    default: return "Français";
    }
}

Lang from_code(const std::string& value, Lang fallback) {
    if (value == "fr") return Lang::Fr;
    if (value == "en") return Lang::En;
    if (value == "ja" || value == "jp") return Lang::Ja;
    return fallback;
}

const char* tr(Str s) {
    return kTable[static_cast<int>(s)][static_cast<int>(g_lang)];
}

std::string format(Str s, ...) {
    const char* fmt = tr(s);
    va_list args;
    va_start(args, s);
    va_list copy;
    va_copy(copy, args);
    const int n = std::vsnprintf(nullptr, 0, fmt, copy);
    va_end(copy);
    std::string out;
    if (n > 0) {
        std::vector<char> buf(static_cast<size_t>(n) + 1);
        std::vsnprintf(buf.data(), buf.size(), fmt, args);
        out.assign(buf.data(), static_cast<size_t>(n));
    }
    va_end(args);
    return out;
}

} // namespace app::i18n
