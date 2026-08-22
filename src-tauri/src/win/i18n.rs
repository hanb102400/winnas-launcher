//! 系统语言检测（设计文档 4.14，FR-20）。
//!
//! 仅负责「检测操作系统当前 UI 语言」并映射到应用支持的 10 种 locale。
//! 只在首次启动（`setting.conf` 中 `language` 为空）时被 `config::init()` 调用一次，
//! 用户手动切换后不再被系统覆盖。所有 Win32 调用收敛在本模块。
//!
//! 优先级：`GetUserDefaultLocaleName`（BCP-47）→ `GetUserDefaultUILanguage`（LANGID）。
//! 实测简体中文系统上 `GetUserDefaultUILanguage` 可能因 MUI 语言包顺序误报繁体（0x0804），
//! 而 `GetUserDefaultLocaleName` 返回 `zh-CN` 与实际显示语言一致，故以其为准。

use windows::Win32::Globalization::{GetUserDefaultLocaleName, GetUserDefaultUILanguage};

/// 应用支持的 10 种 locale（与前端 `src/i18n/index.ts` 的 `LANGS` 保持一致）。
pub const SUPPORTED: [&str; 10] = [
    "zh-CN", "zh-TW", "en", "ja", "ko", "es", "ar", "fr", "de", "ru",
];

/// 检测操作系统当前 UI 语言，映射到应用支持的 locale。
/// 未命中任何支持语言时回退简体中文 `zh-CN`。
///
/// 策略：
/// 1. `GetUserDefaultLocaleName`（BCP-47）为准——与系统实际显示语言一致，能区分简繁（脚本/地区）；
/// 2. BCP-47 读取失败时，回退 `GetUserDefaultUILanguage`（LANGID）判定。
pub fn detect_os_language() -> String {
    if let Some(tag) = detect_os_language_bcp47() {
        return map_bcp47(&tag);
    }
    if let Some(code) = map_langid(unsafe { GetUserDefaultUILanguage() }) {
        return code;
    }
    "zh-CN".to_string()
}

/// LANGID → locale；无法判定（未知语言 / 中性中文）返回 `None`。
fn map_langid(langid: u16) -> Option<String> {
    const LANG_CHINESE: u16 = 0x04;
    const LANG_ARABIC: u16 = 0x01;
    const LANG_GERMAN: u16 = 0x07;
    const LANG_ENGLISH: u16 = 0x09;
    const LANG_SPANISH: u16 = 0x0a;
    const LANG_FRENCH: u16 = 0x0c;
    const LANG_JAPANESE: u16 = 0x11;
    const LANG_KOREAN: u16 = 0x12;
    const LANG_RUSSIAN: u16 = 0x19;
    // 中文 sublanguage（简化/繁体区分）
    const SUBLANG_CHINESE_SIMPLIFIED: u16 = 0x01;
    const SUBLANG_CHINESE_TRADITIONAL: u16 = 0x02;
    const SUBLANG_CHINESE_HONGKONG: u16 = 0x03;
    const SUBLANG_CHINESE_SINGAPORE: u16 = 0x04;
    const SUBLANG_CHINESE_MACAU: u16 = 0x05;

    let primary = langid & 0x3ff;
    let sublang = (langid >> 10) & 0x1f;
    match primary {
        LANG_CHINESE => match sublang {
            SUBLANG_CHINESE_SIMPLIFIED | SUBLANG_CHINESE_SINGAPORE => Some("zh-CN".to_string()),
            SUBLANG_CHINESE_TRADITIONAL | SUBLANG_CHINESE_HONGKONG | SUBLANG_CHINESE_MACAU => {
                Some("zh-TW".to_string())
            }
            // 中性 zh（sub 0x00，如 0x0004 / 0x7c04）→ 交给 BCP-47 判定
            _ => None,
        },
        LANG_ENGLISH => Some("en".to_string()),
        LANG_JAPANESE => Some("ja".to_string()),
        LANG_KOREAN => Some("ko".to_string()),
        LANG_SPANISH => Some("es".to_string()),
        LANG_ARABIC => Some("ar".to_string()),
        LANG_FRENCH => Some("fr".to_string()),
        LANG_GERMAN => Some("de".to_string()),
        LANG_RUSSIAN => Some("ru".to_string()),
        _ => None,
    }
}

/// 读取 BCP-47 系统默认 locale（如 `zh-Hans-CN`、`en-US`、`ja-JP`）。
fn detect_os_language_bcp47() -> Option<String> {
    // LOCALE_NAME_MAX_LENGTH = 85（含结尾 \0）
    let mut buf = [0u16; 85];
    let len = unsafe { GetUserDefaultLocaleName(&mut buf) };
    if len > 1 {
        let tag = String::from_utf16_lossy(&buf[..(len as usize - 1)]);
        if !tag.trim().is_empty() {
            return Some(tag);
        }
    }
    None
}

/// BCP-47 标签（`语言[–脚本][–地区]`）→ locale；未命中回退 `zh-CN`。
fn map_bcp47(tag: &str) -> String {
    let mut parts = tag.split(['-', '_']);
    let lang = parts.next().unwrap_or("").to_ascii_lowercase();
    let rest: Vec<&str> = parts.collect();
    match lang.as_str() {
        // 繁体判定：脚本 Hant 或地区 TW/HK/MO
        "zh" => {
            let script = rest.iter().find(|s| s.len() == 4);
            let region = rest.iter().find(|s| s.len() == 2);
            let is_traditional = script
                .map(|s| s.eq_ignore_ascii_case("hant"))
                .unwrap_or(false)
                || region
                    .map(|r| matches!(r.to_ascii_uppercase().as_str(), "TW" | "HK" | "MO"))
                    .unwrap_or(false);
            if is_traditional {
                "zh-TW".to_string()
            } else {
                "zh-CN".to_string()
            }
        }
        "en" => "en".to_string(),
        "ja" => "ja".to_string(),
        "ko" => "ko".to_string(),
        "es" => "es".to_string(),
        "ar" => "ar".to_string(),
        "fr" => "fr".to_string(),
        "de" => "de".to_string(),
        "ru" => "ru".to_string(),
        _ => "zh-CN".to_string(),
    }
}
