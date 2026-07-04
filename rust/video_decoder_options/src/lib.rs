//! TVTest のビデオデコーダ設定(`src/VideoDecoderOptions.cpp` / `VideoDecoderOptions.h`)の
//! 純粋ロジックを移植したクレート。
//!
//! 移植対象:
//! - `CSettings::GetEntries` で取得したプロパティ値文字列(`Entry.Value`)を VARIANT
//!   (`VT_BOOL` / `VT_INT`)相当の値へ変換するロジック(`ReadSettings`、
//!   VideoDecoderOptions.cpp:63-82)。
//!
//! 対象外(Win32 / DirectShow / LibISDB / CSettings 依存):
//! - ダイアログ(本ファイルに `DlgProc` は存在しないが、設定ダイアログからの呼び出し経路)。
//! - `ReadSettings` / `WriteSettings` の `CSettings` I/O 本体(本クレートは値の変換のみ提供)。
//! - `ApplyVideoDecoderSettings`(`LibISDB::ViewerFilter` との連携、VideoDecoderOptions.cpp:122-132)。
//! - `LibISDB::DirectShow::KnownDecoderManager::VideoDecoderSettings` 本体(`m_VideoDecoderSettings`)。

#![forbid(unsafe_code)]

/// プロパティ値の変換結果(VideoDecoderOptions.cpp:70-81 の `VARIANT`(`VT_BOOL` / `VT_INT`)相当)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropertyValue {
    /// `VT_BOOL`(`Property.Value.boolVal`)。
    Bool(bool),
    /// `VT_INT`(`Property.Value.intVal`)。
    Int(i32),
}

/// プロパティ値文字列を `PropertyValue` へ変換する(`ReadSettings`、VideoDecoderOptions.cpp:63-82)。
///
/// - 空文字列は `None`(原実装では `continue` してプロパティ自体を追加しない、cpp:63-64)。
/// - 大小無視で `"yes"` または `"true"` なら `Bool(true)`(`::lstrcmpi`、cpp:70-73)。
/// - 大小無視で `"no"` または `"false"` なら `Bool(false)`(cpp:74-77)。
/// - それ以外は `::StrToInt` 相当(先頭空白・符号・10進数字列のみを読み取り、数字が
///   続かなければ 0)でパースした `Int` を返す(cpp:78-81)。
#[must_use]
pub fn parse_property_value(value: &str) -> Option<PropertyValue> {
    if value.is_empty() {
        return None;
    }

    if value.eq_ignore_ascii_case("yes") || value.eq_ignore_ascii_case("true") {
        Some(PropertyValue::Bool(true))
    } else if value.eq_ignore_ascii_case("no") || value.eq_ignore_ascii_case("false") {
        Some(PropertyValue::Bool(false))
    } else {
        Some(PropertyValue::Int(str_to_int(value)))
    }
}

/// `::StrToInt` 相当のパース(内部的に `StrToIntEx` を CALLBACK 無しで呼ぶ実装で、
/// 事実上 `wcstol(str, null, 10)` に近い)。
///
/// 先頭の空白をスキップし、任意の符号(`+`/`-`)の後に続く 10進数字列のみを読み取る。
/// 数字が 1 つも続かなければ 0 を返す。オーバーフロー時は `i32` の範囲へ飽和させる
/// (`wcstol` は `LONG_MAX` / `LONG_MIN` に飽和し `errno` を設定するのみで、値としては
/// 飽和値が返る挙動に合わせる)。
fn str_to_int(s: &str) -> i32 {
    let trimmed = s.trim_start();
    let bytes = trimmed.as_bytes();
    let mut idx = 0;

    let negative = if idx < bytes.len() && (bytes[idx] == b'+' || bytes[idx] == b'-') {
        let neg = bytes[idx] == b'-';
        idx += 1;
        neg
    } else {
        false
    };

    let digits_start = idx;
    while idx < bytes.len() && bytes[idx].is_ascii_digit() {
        idx += 1;
    }

    if idx == digits_start {
        return 0;
    }

    let digits = &trimmed[digits_start..idx];
    let mut value: i64 = 0;
    for b in digits.as_bytes() {
        value = value * 10 + i64::from(b - b'0');
        if negative {
            value = value.min(-(i64::from(i32::MIN)));
        } else {
            value = value.min(i64::from(i32::MAX));
        }
    }

    if negative {
        -value as i32
    } else {
        value as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yes_lowercase_is_true() {
        assert_eq!(parse_property_value("yes"), Some(PropertyValue::Bool(true)));
    }

    #[test]
    fn yes_uppercase_is_case_insensitive() {
        assert_eq!(parse_property_value("YES"), Some(PropertyValue::Bool(true)));
    }

    #[test]
    fn true_is_true() {
        assert_eq!(parse_property_value("true"), Some(PropertyValue::Bool(true)));
    }

    #[test]
    fn no_is_false() {
        assert_eq!(parse_property_value("no"), Some(PropertyValue::Bool(false)));
    }

    #[test]
    fn false_is_false() {
        assert_eq!(parse_property_value("false"), Some(PropertyValue::Bool(false)));
    }

    #[test]
    fn empty_string_is_none() {
        assert_eq!(parse_property_value(""), None);
    }

    #[test]
    fn numeric_string_is_int() {
        assert_eq!(parse_property_value("123"), Some(PropertyValue::Int(123)));
    }

    #[test]
    fn non_numeric_string_is_int_zero() {
        assert_eq!(parse_property_value("abc"), Some(PropertyValue::Int(0)));
    }

    #[test]
    fn negative_numeric_string_is_int() {
        assert_eq!(parse_property_value("-5"), Some(PropertyValue::Int(-5)));
    }

    #[test]
    fn mixed_case_true_false_is_case_insensitive() {
        assert_eq!(parse_property_value("True"), Some(PropertyValue::Bool(true)));
        assert_eq!(parse_property_value("NO"), Some(PropertyValue::Bool(false)));
        assert_eq!(parse_property_value("False"), Some(PropertyValue::Bool(false)));
    }

    #[test]
    fn leading_whitespace_and_sign_are_handled() {
        assert_eq!(parse_property_value("  42"), Some(PropertyValue::Int(42)));
        assert_eq!(parse_property_value("+42"), Some(PropertyValue::Int(42)));
        assert_eq!(parse_property_value("  -42"), Some(PropertyValue::Int(-42)));
    }

    #[test]
    fn digits_followed_by_non_digits_stop_at_first_non_digit() {
        assert_eq!(parse_property_value("123abc"), Some(PropertyValue::Int(123)));
    }

    #[test]
    fn sign_without_digits_is_zero() {
        assert_eq!(parse_property_value("-"), Some(PropertyValue::Int(0)));
        assert_eq!(parse_property_value("+"), Some(PropertyValue::Int(0)));
    }
}
