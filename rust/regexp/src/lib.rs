// TVTest の RegExp.cpp を Rust へ移植したもの。
//
// 移植対象:
//   - char_to_half_width     : CharToHalfWidth:44
//   - string_to_half_width   : StringToHalfWidth:57
//   - map_pattern_string     : CRegExpEngine::MapPatternString:89
//   - PatternFlag            : CRegExp::PatternFlag (RegExp.h:43)
//   - TextRange              : CRegExp::TextRange (RegExp.h:49)
//   - RegExp                 : CRegExp (RegExp.h:54) — ECMAScript エンジン相当
//
// C++ の CRegExp は bregonig.dll/VBScript/std::regex から実行時選択するが、
// Rust 版は `regex` クレート(ECMAScript 相当)のみを使用する。
// IgnoreWidth フラグによる全角→半角正規化を含む。

use bitflags::bitflags;
use regex::{Regex, RegexBuilder};

bitflags! {
    /// 原実装 CRegExp::PatternFlag (RegExp.h:43)。
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct PatternFlag: u32 {
        const IGNORE_CASE  = 0x0001;
        const IGNORE_WIDTH = 0x0002;
        const OPTIMIZE     = 0x0004;
    }
}

/// 検索一致範囲。原実装 CRegExp::TextRange (RegExp.h:49)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextRange {
    pub start: usize,
    pub length: usize,
}

/// 全角 ASCII 記号・英数字・スペースを半角に変換する。
/// 変換した場合 true。原実装 CharToHalfWidth:44。
fn char_to_half_width(c: char) -> Option<char> {
    if c >= '！' && c <= '～' {
        // U+FF01..U+FF5E → U+0021..U+007E
        let half = (c as u32 - '！' as u32 + '!' as u32) as u8 as char;
        Some(half)
    } else if c == '\u{3000}' {
        // 全角スペース → 半角スペース
        Some(' ')
    } else {
        None
    }
}

/// 文字列内の全角 ASCII を半角に変換する。原実装 StringToHalfWidth:57。
/// 原実装と同様に ASCII 範囲のみ(変換前後で長さが変わらない文字のみ)を対象とする。
pub fn string_to_half_width(text: &str) -> String {
    text.chars()
        .map(|c| char_to_half_width(c).unwrap_or(c))
        .collect()
}

/// 正規表現パターンに IgnoreWidth 変換を施す。
/// バックスラッシュエスケープされた文字はスキップ。
/// 変換後の記号文字はエスケープを挿入。原実装 MapPatternString:89。
fn map_pattern_string(pattern: &str, flags: PatternFlag) -> String {
    if !flags.contains(PatternFlag::IGNORE_WIDTH) {
        return pattern.to_string();
    }
    let chars: Vec<char> = pattern.chars().collect();
    let mut result = String::with_capacity(pattern.len() + 16);
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' {
            result.push('\\');
            i += 1;
            if i < chars.len() {
                result.push(chars[i]);
                i += 1;
            }
            continue;
        }
        if let Some(half) = char_to_half_width(chars[i]) {
            // 記号文字はエスケープが必要
            let needs_escape = matches!(half,
                '!'..='/' | ':'..='?' | '['..='^' | '{'..='}');
            if needs_escape {
                result.push('\\');
            }
            result.push(half);
        } else {
            result.push(chars[i]);
        }
        i += 1;
    }
    result
}

/// ECMAScript 正規表現ラッパー。原実装 CRegExp (RegExp.h:54)。
///
/// `set_pattern` でコンパイル、`match_text` で検索。
/// IgnoreWidth フラグ時はパターンおよび検索対象を全角→半角変換してからマッチング。
pub struct RegExp {
    compiled: Option<Regex>,
    flags: PatternFlag,
}

impl Default for RegExp {
    fn default() -> Self {
        Self::new()
    }
}

impl RegExp {
    pub fn new() -> Self {
        RegExp { compiled: None, flags: PatternFlag::empty() }
    }

    /// パターンをコンパイルする。原実装 CRegExp::SetPattern:619。
    pub fn set_pattern(&mut self, pattern: &str, flags: PatternFlag) -> bool {
        if pattern.is_empty() {
            return false;
        }
        self.flags = flags;
        let mapped = map_pattern_string(pattern, flags);
        let result = RegexBuilder::new(&mapped)
            .case_insensitive(flags.contains(PatternFlag::IGNORE_CASE))
            .build();
        match result {
            Ok(re) => {
                self.compiled = Some(re);
                true
            }
            Err(_) => false,
        }
    }

    /// パターンをクリアする。原実装 CRegExpEngine::ClearPattern:76。
    pub fn clear_pattern(&mut self) {
        self.compiled = None;
        self.flags = PatternFlag::empty();
    }

    /// 初期化済みか(パターンが設定されているか)。原実装 CRegExp::IsInitialized:613。
    pub fn is_initialized(&self) -> bool {
        self.compiled.is_some()
    }

    /// テキスト内を検索する。原実装 CRegExp::Match:635。
    ///
    /// `text` の先頭 `length` 文字(char 単位ではなくバイトスライス)を対象にする。
    /// C++ 版は wchar_t 単位、Rust 版は &str スライスで受け取る。
    pub fn match_text(&self, text: &str) -> Option<TextRange> {
        let re = self.compiled.as_ref()?;
        let target = if self.flags.contains(PatternFlag::IGNORE_WIDTH) {
            std::borrow::Cow::Owned(string_to_half_width(text))
        } else {
            std::borrow::Cow::Borrowed(text)
        };
        let m = re.find(&target)?;
        // バイトオフセットを文字(char)オフセットに変換
        let start = target[..m.start()].chars().count();
        let length = target[m.start()..m.end()].chars().count();
        Some(TextRange { start, length })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_char_to_half_width_ascii_symbols() {
        // ！(U+FF01) → !
        assert_eq!(char_to_half_width('！'), Some('!'));
        // ～(U+FF5E) → ~
        assert_eq!(char_to_half_width('～'), Some('~'));
        // 全角スペース → 半角スペース
        assert_eq!(char_to_half_width('\u{3000}'), Some(' '));
        // 変換なし
        assert_eq!(char_to_half_width('A'), None);
        assert_eq!(char_to_half_width('あ'), None);
    }

    #[test]
    fn test_char_to_half_width_digits_letters() {
        assert_eq!(char_to_half_width('Ａ'), Some('A'));
        assert_eq!(char_to_half_width('Ｚ'), Some('Z'));
        assert_eq!(char_to_half_width('ａ'), Some('a'));
        assert_eq!(char_to_half_width('ｚ'), Some('z'));
        assert_eq!(char_to_half_width('０'), Some('0'));
        assert_eq!(char_to_half_width('９'), Some('9'));
    }

    #[test]
    fn test_string_to_half_width() {
        let result = string_to_half_width("ＡＢＣ　１２３");
        assert_eq!(result, "ABC 123");
    }

    #[test]
    fn test_string_to_half_width_mixed() {
        let result = string_to_half_width("あいうABC");
        assert_eq!(result, "あいうABC");
    }

    #[test]
    fn test_set_pattern_empty_fails() {
        let mut re = RegExp::new();
        assert!(!re.set_pattern("", PatternFlag::empty()));
    }

    #[test]
    fn test_basic_match() {
        let mut re = RegExp::new();
        assert!(re.set_pattern("hello", PatternFlag::empty()));
        let r = re.match_text("say hello world").unwrap();
        assert_eq!(r.start, 4);
        assert_eq!(r.length, 5);
    }

    #[test]
    fn test_no_match() {
        let mut re = RegExp::new();
        assert!(re.set_pattern("xyz", PatternFlag::empty()));
        assert!(re.match_text("hello world").is_none());
    }

    #[test]
    fn test_ignore_case() {
        let mut re = RegExp::new();
        assert!(re.set_pattern("Hello", PatternFlag::IGNORE_CASE));
        assert!(re.match_text("say HELLO world").is_some());
    }

    #[test]
    fn test_ignore_width_pattern_and_target() {
        let mut re = RegExp::new();
        // 全角パターン "ＡＢＣ" で半角テキスト "ABC" を検索
        assert!(re.set_pattern("ＡＢＣ", PatternFlag::IGNORE_WIDTH));
        let r = re.match_text("ABC");
        assert!(r.is_some());
    }

    #[test]
    fn test_ignore_width_target_normalization() {
        let mut re = RegExp::new();
        // 半角パターンで全角テキストを検索
        assert!(re.set_pattern("ABC", PatternFlag::IGNORE_WIDTH));
        let r = re.match_text("ＡＢＣ");
        assert!(r.is_some());
    }

    #[test]
    fn test_regex_match_range() {
        let mut re = RegExp::new();
        assert!(re.set_pattern(r"\d+", PatternFlag::empty()));
        let r = re.match_text("abc 123 def").unwrap();
        assert_eq!(r.start, 4);
        assert_eq!(r.length, 3);
    }

    #[test]
    fn test_clear_pattern() {
        let mut re = RegExp::new();
        assert!(re.set_pattern("hello", PatternFlag::empty()));
        assert!(re.is_initialized());
        re.clear_pattern();
        assert!(!re.is_initialized());
        assert!(re.match_text("hello").is_none());
    }

    #[test]
    fn test_invalid_pattern_fails() {
        let mut re = RegExp::new();
        // 不正な正規表現
        assert!(!re.set_pattern("[invalid", PatternFlag::empty()));
        assert!(!re.is_initialized());
    }

    #[test]
    fn test_multibyte_char_offset() {
        let mut re = RegExp::new();
        // 日本語文字列内の位置が文字数ベースで正しいか
        assert!(re.set_pattern("世界", PatternFlag::empty()));
        let r = re.match_text("こんにちは世界").unwrap();
        assert_eq!(r.start, 5);
        assert_eq!(r.length, 2);
    }

    #[test]
    fn test_map_pattern_string_escape() {
        // IgnoreWidth で全角記号がエスケープされること
        let mapped = map_pattern_string("（.*）", PatternFlag::IGNORE_WIDTH);
        // （ → \( 、） → \)
        assert!(mapped.contains("\\("));
        assert!(mapped.contains("\\)"));
    }
}
