// SPDX-License-Identifier: MIT
//! Translations (French, English, Japanese). The table is generated from tools/i18n_strings.tsv.

use crate::i18n_strings::{Str, TABLE};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Lang {
    #[default]
    Fr,
    En,
    Ja,
    Zh,
}

pub const LANG_COUNT: usize = 4;

impl Lang {
    pub fn next(self) -> Lang {
        match self {
            Lang::Fr => Lang::En,
            Lang::En => Lang::Ja,
            Lang::Ja => Lang::Zh,
            Lang::Zh => Lang::Fr,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Lang::Fr => "fr",
            Lang::En => "en",
            Lang::Ja => "ja",
            Lang::Zh => "zh",
        }
    }

    pub fn native_name(self) -> &'static str {
        match self {
            Lang::Fr => "Français",
            Lang::En => "English",
            Lang::Ja => "日本語",
            Lang::Zh => "简体中文",
        }
    }

    /// Which face of the Noto Sans CJK collection draws this language (Han glyphs differ: JP = 0, SC = 2).
    pub fn cjk_face(self) -> u32 {
        match self {
            Lang::Zh => 2,
            _ => 0,
        }
    }

    /// Unknown codes give `fallback`; "jp" is accepted for Japanese, "zh-cn" and friends for Simplified Chinese.
    pub fn from_code(code: &str, fallback: Lang) -> Lang {
        match code {
            "fr" => Lang::Fr,
            "en" => Lang::En,
            "ja" | "jp" => Lang::Ja,
            "zh" | "zh-cn" | "zh_cn" | "zh-hans" => Lang::Zh,
            _ => fallback,
        }
    }
}

pub fn tr(lang: Lang, s: Str) -> &'static str {
    TABLE[s as usize][lang as usize]
}

/// Replaces the printf-style placeholders (`%s`, `%d`, `%zu`, `%u`) of the string in order.
pub fn format(lang: Lang, s: Str, args: &[&dyn std::fmt::Display]) -> String {
    let fmt = tr(lang, s);
    let mut out = String::new();
    let mut args = args.iter();
    let mut rest = fmt;
    while let Some(pos) = rest.find('%') {
        out.push_str(&rest[..pos]);
        let tail = &rest[pos + 1..];
        let len = ["zu", "s", "d", "u"].iter().find(|p| tail.starts_with(**p)).map(|p| p.len());
        match len {
            Some(n) => {
                if let Some(a) = args.next() {
                    out.push_str(&a.to_string());
                }
                rest = &tail[n..];
            }
            None if tail.starts_with('%') => {
                out.push('%');
                rest = &tail[1..];
            }
            None => {
                out.push('%');
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n_strings::COUNT;

    #[test]
    fn table_is_complete_and_placeholders_agree_across_languages() {
        assert_eq!(TABLE.len(), COUNT);
        for (i, row) in TABLE.iter().enumerate() {
            assert!(row.iter().all(|s| !s.is_empty()), "empty translation in row {i}");
            let count = |s: &str| s.matches('%').count();
            assert!(row.iter().all(|s| count(s) == count(row[0])), "placeholders differ: {row:?}");
        }
    }

    #[test]
    fn lookups_and_format() {
        assert_eq!(tr(Lang::En, Str::CatalogueTitle), "Catalogue");
        assert_eq!(tr(Lang::Ja, Str::LibraryTitle), "ライブラリ");
        assert_eq!(tr(Lang::Fr, Str::LibraryTitle), "Bibliothèque");
        assert_eq!(format(Lang::En, Str::FacetChip, &[&"Type", &"Card"]), "Type: Card");
        assert_eq!(format(Lang::Fr, Str::FacetValuesCount, &[&12usize]), "12 valeurs");
        assert_eq!(format(Lang::Ja, Str::FacetValuesCount, &[&3usize]), "3 件の値");
        assert_eq!(tr(Lang::Zh, Str::CatalogueTitle), "图鉴");
        assert_eq!(format(Lang::Zh, Str::FacetChip, &[&"类型", &"卡片"]), "类型：卡片");
        assert_eq!(format(Lang::Zh, Str::ScanProgress, &[&5, &135]), "5 / 135 页");
    }

    #[test]
    fn language_cycle_and_codes() {
        assert_eq!(Lang::Fr.next().next().next().next(), Lang::Fr);
        assert_eq!(Lang::Ja.next(), Lang::Zh);
        assert_eq!(Lang::from_code("jp", Lang::Fr), Lang::Ja);
        assert_eq!(Lang::from_code("zh-CN".to_ascii_lowercase().as_str(), Lang::Fr), Lang::Zh);
        assert_eq!((Lang::Ja.cjk_face(), Lang::Zh.cjk_face(), Lang::Fr.cjk_face()), (0, 2, 0));
        assert_eq!(Lang::from_code("xx", Lang::En), Lang::En);
        assert_eq!(Lang::Ja.code(), "ja");
    }
}
