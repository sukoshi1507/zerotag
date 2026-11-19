/*
 * SPDX-License-Identifier: MIT
 *
 * UI translations (French, English, Japanese). Strings live in i18n_strings.def.
 */

#pragma once

#include <string>

namespace app::i18n {

enum class Lang { Fr = 0, En = 1, Ja = 2 };
constexpr int kLangCount = 3;

enum class Str {
#define X(name, fr, en, ja) name,
#include "i18n_strings.def"
#undef X
    Count
};

void set_language(Lang lang);
Lang language();
Lang next_language(Lang lang);
const char* code(Lang lang);        // "fr", "en", "ja"
const char* native_name(Lang lang); // "Français", "English", "日本語"
Lang from_code(const std::string& code, Lang fallback = Lang::Fr);

const char* tr(Str s);
// printf-style: the translated string is the format.
std::string format(Str s, ...);

} // namespace app::i18n
