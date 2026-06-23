//! TVTest の表示設定(`src/ViewOptions.cpp` / `ViewOptions.h`)の純粋ロジックを
//! 移植したクレート。
//!
//! 移植対象:
//! - パン&スキャン時のウィンドウ調整モード(`AdjustWindowMode`)と範囲検証(`CheckEnumRange`)。
//! - 旧設定 `PanScanNoResizeWindow`(bool)から `AdjustWindowMode` への互換変換
//!   (`ReadSettings`、ViewOptions.cpp:97-105)。
//! - タイトル文字列書式のプリセット(`TitleTextFormatPresets`、ViewOptions.cpp:36-49)。
//! - 旧仕様のタイトル書式を現行へ変換する `TitleFormatMakeCompatible`(ViewOptions.cpp:54-60)。
//!
//! 対象外(Win32 / CSettings / Style 依存):
//! - `CViewOptions::DlgProc`(ダイアログ)・フォント選択・ロゴ画像のファイル選択。
//! - `ReadSettings` / `WriteSettings` の `CSettings` I/O 本体・`Apply`(`ViewerFilter` 連携)。
//! - フォント(`Style::Font`)・各種 bool フラグの単純な値の読み書き。

#![forbid(unsafe_code)]

/// アプリ名(`APP_NAME`、TVTest.h:28-33)。タイトル書式や既定ロゴ名に使う。
pub const APP_NAME: &str = "TVTest";

/// パン&スキャン時のウィンドウ調整モード(ViewOptions.h:43-48 の `AdjustWindowMode`)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdjustWindowMode {
    /// サイズを変えない。
    None = 0,
    /// 幅と高さを変える(ウィンドウに合わせる)。
    Fit = 1,
    /// 幅のみ変える。
    Width = 2,
}

/// `AdjustWindowMode` の終端番兵(`TVTEST_ENUM_CLASS_TRAILER`)の値。ViewOptions.h:47。
pub const ADJUST_WINDOW_MODE_TRAILER: i32 = 3;

/// 既定のウィンドウ調整モード(`m_PanScanAdjustWindowMode` の初期値、ViewOptions.h:93)。
pub const ADJUST_WINDOW_MODE_DEFAULT: AdjustWindowMode = AdjustWindowMode::Width;

impl AdjustWindowMode {
    /// 整数値を `AdjustWindowMode` へ変換する(`CheckEnumRange`、ViewOptions.cpp:97-99)。
    ///
    /// 有効範囲は `0 <= value < TVTEST_ENUM_CLASS_TRAILER`(= 0..=2)。範囲外は `None`。
    #[must_use]
    pub fn from_int(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::None),
            1 => Some(Self::Fit),
            2 => Some(Self::Width),
            _ => Option::None,
        }
    }

    /// 整数値へ変換する。
    #[must_use]
    pub const fn to_int(self) -> i32 {
        self as i32
    }
}

/// 旧設定 `PanScanNoResizeWindow`(bool)から現行モードへ変換する(ViewOptions.cpp:104)。
///
/// `f ? AdjustWindowMode::Width : AdjustWindowMode::Fit`。`PanScanAdjustWindow`(整数)が
/// 無い旧バージョン設定からの読み込み時にのみ使われる互換変換。高さを変えない(=幅のみ)が
/// `Width`、変える(=ウィンドウに合わせる)が `Fit` に対応する。
#[must_use]
pub fn adjust_window_mode_from_legacy(no_resize_window: bool) -> AdjustWindowMode {
    if no_resize_window {
        AdjustWindowMode::Width
    } else {
        AdjustWindowMode::Fit
    }
}

/// タイトル文字列書式のプリセット 1 件(ViewOptions.cpp:36-49 の `TitleTextFormatPresets`)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TitleTextFormatPreset {
    /// メニューに表示する説明文。
    pub descript: &'static str,
    /// 実際に設定されるタイトル書式文字列。
    pub format: &'static str,
}

/// タイトル文字列書式のプリセット一覧(ViewOptions.cpp:36-49)。
pub const TITLE_TEXT_FORMAT_PRESETS: [TitleTextFormatPreset; 2] = [
    TitleTextFormatPreset {
        descript: "サービス名 / 番組時間 番組名 - TVTest",
        format:
            "%rec-circle% %service-name% %sep-slash% %event-time% %event-name% %sep-hyphen% TVTest",
    },
    TitleTextFormatPreset {
        descript: "サービス名 / 番組名 - TVTest",
        format: "%rec-circle% %service-name% %sep-slash% %event-name% %sep-hyphen% TVTest",
    },
];

/// 既定のタイトル書式(コンストラクタ、ViewOptions.cpp:64。`TitleTextFormatPresets[0].format`)。
pub const DEFAULT_TITLE_TEXT_FORMAT: &str = TITLE_TEXT_FORMAT_PRESETS[0].format;

/// 既定のロゴファイル名(`m_LogoFileName` の初期値、ViewOptions.h:106。`APP_NAME "_Logo.bmp"`)。
pub const DEFAULT_LOGO_FILE_NAME: &str = "TVTest_Logo.bmp";

/// 旧仕様のタイトル書式を現行仕様へ変換する(`TitleFormatMakeCompatible`、ViewOptions.cpp:54-60)。
///
/// `%event-sep%` を含む場合のみ、`%event-sep%` → `%sep-slash%`、`- TVTest` → `%sep-hyphen% TVTest`
/// を全置換する。`%event-sep%` を含まなければ(たとえ `- TVTest` を含んでいても)何もしない。
/// 原実装に合わせて UTF-16(`Vec<u16>`)を直接書き換える。
pub fn title_format_make_compatible(text: &mut Vec<u16>) {
    let event_sep = utf16("%event-sep%");
    if contains(text, &event_sep) {
        tvtest_string_utility::replace(text, &event_sep, &utf16("%sep-slash%"));
        tvtest_string_utility::replace(text, &utf16("- TVTest"), &utf16("%sep-hyphen% TVTest"));
    }
}

/// `title_format_make_compatible` の `&str` 版(ユーティリティ)。
#[must_use]
pub fn title_format_make_compatible_str(text: &str) -> String {
    let mut buf: Vec<u16> = text.encode_utf16().collect();
    title_format_make_compatible(&mut buf);
    String::from_utf16_lossy(&buf)
}

/// `&str` を UTF-16 へ変換する補助。
fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// `haystack` が部分列 `needle` を含むか(原実装 `String::find(...) != npos` 相当)。
fn contains(haystack: &[u16], needle: &[u16]) -> bool {
    if needle.is_empty() {
        return true;
    }
    if needle.len() > haystack.len() {
        return false;
    }
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adjust_window_mode_from_int() {
        assert_eq!(AdjustWindowMode::from_int(0), Some(AdjustWindowMode::None));
        assert_eq!(AdjustWindowMode::from_int(1), Some(AdjustWindowMode::Fit));
        assert_eq!(AdjustWindowMode::from_int(2), Some(AdjustWindowMode::Width));
        // 範囲外(終端番兵以上・負値)は None。
        assert_eq!(AdjustWindowMode::from_int(ADJUST_WINDOW_MODE_TRAILER), None);
        assert_eq!(AdjustWindowMode::from_int(3), None);
        assert_eq!(AdjustWindowMode::from_int(-1), None);
    }

    #[test]
    fn adjust_window_mode_round_trip() {
        for mode in [
            AdjustWindowMode::None,
            AdjustWindowMode::Fit,
            AdjustWindowMode::Width,
        ] {
            assert_eq!(AdjustWindowMode::from_int(mode.to_int()), Some(mode));
        }
    }

    #[test]
    fn adjust_window_mode_default_is_width() {
        assert_eq!(ADJUST_WINDOW_MODE_DEFAULT, AdjustWindowMode::Width);
        assert_eq!(ADJUST_WINDOW_MODE_DEFAULT.to_int(), 2);
    }

    #[test]
    fn legacy_no_resize_window_mapping() {
        // 高さを変えない(幅のみ)→ Width、変える → Fit。
        assert_eq!(
            adjust_window_mode_from_legacy(true),
            AdjustWindowMode::Width
        );
        assert_eq!(adjust_window_mode_from_legacy(false), AdjustWindowMode::Fit);
    }

    #[test]
    fn title_presets_and_defaults() {
        assert_eq!(TITLE_TEXT_FORMAT_PRESETS.len(), 2);
        // 既定書式はプリセット 0 の書式。
        assert_eq!(
            DEFAULT_TITLE_TEXT_FORMAT,
            TITLE_TEXT_FORMAT_PRESETS[0].format
        );
        assert!(DEFAULT_TITLE_TEXT_FORMAT.contains("%event-time%"));
        // プリセット 1 は番組時間を含まない。
        assert!(!TITLE_TEXT_FORMAT_PRESETS[1].format.contains("%event-time%"));
        // 説明・書式とも APP_NAME で終わる。
        for preset in &TITLE_TEXT_FORMAT_PRESETS {
            assert!(preset.descript.ends_with(APP_NAME));
            assert!(preset.format.ends_with(APP_NAME));
        }
        assert_eq!(DEFAULT_LOGO_FILE_NAME, "TVTest_Logo.bmp");
    }

    #[test]
    fn make_compatible_replaces_when_event_sep_present() {
        // %event-sep% を含むと両方の置換が行われる。
        let input = "%service-name% %event-sep% %event-name% - TVTest";
        let expected = "%service-name% %sep-slash% %event-name% %sep-hyphen% TVTest";
        assert_eq!(title_format_make_compatible_str(input), expected);
    }

    #[test]
    fn make_compatible_no_op_without_event_sep() {
        // %event-sep% が無ければ、- TVTest を含んでいても変換しない。
        let input = "%service-name% %event-name% - TVTest";
        assert_eq!(title_format_make_compatible_str(input), input);
    }

    #[test]
    fn make_compatible_replaces_all_occurrences() {
        // 全置換(複数箇所)。
        let input = "%event-sep%a%event-sep% - TVTest - TVTest";
        let expected = "%sep-slash%a%sep-slash% %sep-hyphen% TVTest %sep-hyphen% TVTest";
        assert_eq!(title_format_make_compatible_str(input), expected);
    }

    #[test]
    fn make_compatible_vec_in_place() {
        let mut buf: Vec<u16> = "%event-sep% - TVTest".encode_utf16().collect();
        title_format_make_compatible(&mut buf);
        assert_eq!(
            String::from_utf16_lossy(&buf),
            "%sep-slash% %sep-hyphen% TVTest"
        );
    }

    #[test]
    fn contains_helper() {
        let hay: Vec<u16> = "abcdef".encode_utf16().collect();
        assert!(contains(&hay, &"cd".encode_utf16().collect::<Vec<_>>()));
        assert!(!contains(&hay, &"xy".encode_utf16().collect::<Vec<_>>()));
        // 空 needle は含むとみなす(find("") == 0)。
        assert!(contains(&hay, &[]));
        // needle が長すぎる場合は含まない。
        assert!(!contains(
            &hay,
            &"abcdefg".encode_utf16().collect::<Vec<_>>()
        ));
    }
}
