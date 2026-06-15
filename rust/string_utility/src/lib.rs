/*
  TVTest
  Copyright(c) 2008-2020 DBCTRADO

  This program is free software; you can redistribute it and/or modify
  it under the terms of the GNU General Public License as published by
  the Free Software Foundation; either version 2 of the License, or
  (at your option) any later version.

  This program is distributed in the hope that it will be useful,
  but WITHOUT ANY WARRANTY; without even the implied warranty of
  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
  GNU General Public License for more details.

  You should have received a copy of the GNU General Public License
  along with this program; if not, write to the Free Software
  Foundation, Inc., 59 Temple Place, Suite 330, Boston, MA  02111-1307  USA
*/

//! TVTest `StringUtility` のうち、プラットフォーム非依存な関数群を Rust に移植したもの。
//!
//! 原実装 (`src/StringUtility.cpp`) は `wchar_t`(Windows では UTF-16)を扱うため、
//! 本クレートも文字列を `&[u16]`(UTF-16 コードユニット列)として扱い、挙動を厳密に一致させる。
//!
//! Win32 API に依存する関数(`CompareNoCase`/`ToUpper`/`ToLower`/`ToHalfWidthNoKatakana`/`ToAnsi`)は
//! Rust 単体では等価検証できないため、本移植の対象外とする。

/// 空白文字の判定。原実装 `IsWhitespace` (StringUtility.cpp:78) と同一。
fn is_whitespace(c: u16) -> bool {
    c == u16::from(b' ') || c == u16::from(b'\r') || c == u16::from(b'\n') || c == u16::from(b'\t')
}

// ---------------------------------------------------------------------------
// 数値変換
// ---------------------------------------------------------------------------

/// C の `wcstoll(s, nullptr, 0)` 相当。原実装 `StringToInt64` (StringUtility.cpp:33)。
///
/// 基数 0 は接頭辞で自動判定する:
///   - `0x` / `0X` → 16 進
///   - 先頭 `0`     → 8 進
///   - それ以外     → 10 進
///
/// 解釈できる範囲まで読み、変換不能になった時点で停止する(C と同じ前方一致)。
pub fn string_to_int64(s: &[u16]) -> i64 {
    let (neg, digits) = take_sign(s);
    let (radix, rest) = detect_radix(digits);
    let mut value: i64 = 0;
    for &c in rest {
        match digit_value(c, radix) {
            Some(d) => {
                value = value.wrapping_mul(radix as i64).wrapping_add(d as i64);
            }
            None => break,
        }
    }
    if neg {
        value.wrapping_neg()
    } else {
        value
    }
}

/// C の `wcstoull(s, nullptr, 0)` 相当。原実装 `StringToUInt64` (StringUtility.cpp:39)。
pub fn string_to_uint64(s: &[u16]) -> u64 {
    let (neg, digits) = take_sign(s);
    let (radix, rest) = detect_radix(digits);
    let mut value: u64 = 0;
    for &c in rest {
        match digit_value(c, radix) {
            Some(d) => {
                value = value.wrapping_mul(radix as u64).wrapping_add(d as u64);
            }
            None => break,
        }
    }
    // C の strtoull は負号付きでも符号反転して返す。
    if neg {
        value.wrapping_neg()
    } else {
        value
    }
}

/// 先頭の空白をスキップし符号を取り出す。C の strtoll/strtoull の前処理に相当。
fn take_sign(s: &[u16]) -> (bool, &[u16]) {
    let mut i = 0;
    while i < s.len() && is_whitespace(s[i]) {
        i += 1;
    }
    let mut neg = false;
    if i < s.len() {
        if s[i] == u16::from(b'-') {
            neg = true;
            i += 1;
        } else if s[i] == u16::from(b'+') {
            i += 1;
        }
    }
    (neg, &s[i..])
}

/// 基数 0 の接頭辞判定。
fn detect_radix(s: &[u16]) -> (u32, &[u16]) {
    if s.first() == Some(&u16::from(b'0')) {
        if s.len() >= 2 && (s[1] == u16::from(b'x') || s[1] == u16::from(b'X')) {
            return (16, &s[2..]);
        }
        return (8, &s[1..]);
    }
    (10, s)
}

/// 1 文字を指定基数の数値に変換。範囲外は `None`。
fn digit_value(c: u16, radix: u32) -> Option<u32> {
    let d = match c {
        c if (u16::from(b'0')..=u16::from(b'9')).contains(&c) => (c - u16::from(b'0')) as u32,
        c if (u16::from(b'a')..=u16::from(b'z')).contains(&c) => (c - u16::from(b'a')) as u32 + 10,
        c if (u16::from(b'A')..=u16::from(b'Z')).contains(&c) => (c - u16::from(b'A')) as u32 + 10,
        _ => return None,
    };
    if d < radix {
        Some(d)
    } else {
        None
    }
}

/// 符号付き整数で構成される文字列かを判定。原実装 `StringIsDigit` (StringUtility.cpp:57)。
///
/// 先頭の `+`/`-` を 1 つだけ許容し、以降は ASCII 数字のみ。空文字列は false。
pub fn string_is_digit(s: &[u16]) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut i = 0;
    if s[i] == u16::from(b'-') || s[i] == u16::from(b'+') {
        i += 1;
    }
    if i >= s.len() {
        // 符号のみは原実装でも数字判定に進み、終端 '\0' で false になる。
        return false;
    }
    for &c in &s[i..] {
        if !(u16::from(b'0')..=u16::from(b'9')).contains(&c) {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// 空白処理
// ---------------------------------------------------------------------------

/// 末尾の空白を取り除き、取り除いた文字数を返す。原実装 `RemoveTrailingWhitespace` (StringUtility.cpp:84)。
pub fn remove_trailing_whitespace(s: &mut Vec<u16>) -> usize {
    let mut last_non_ws: Option<usize> = None;
    for (i, &c) in s.iter().enumerate() {
        if !is_whitespace(c) {
            last_non_ws = Some(i);
        }
    }
    match last_non_ws {
        // 全て非空白、もしくは末尾に空白が無い場合は変更なし。
        Some(idx) if idx + 1 == s.len() => 0,
        Some(idx) => {
            let removed = s.len() - (idx + 1);
            s.truncate(idx + 1);
            removed
        }
        None => {
            // 空文字列、または全て空白。原実装は先頭の空白から切り詰める。
            let removed = s.len();
            s.clear();
            removed
        }
    }
}

/// 先頭の空白をスキップした位置(オフセット)を返す。原実装 `SkipLeadingWhitespace` (StringUtility.cpp:106)。
pub fn skip_leading_whitespace(s: &[u16]) -> usize {
    let mut i = 0;
    while i < s.len() && is_whitespace(s[i]) {
        i += 1;
    }
    i
}

// ---------------------------------------------------------------------------
// StringUtility 名前空間
// ---------------------------------------------------------------------------

/// 前後から指定文字集合を取り除く。原実装 `StringUtility::Trim` (StringUtility.cpp:200)。
///
/// 変更があれば true。空文字列・空 spaces のときは何もせず false。
pub fn trim(s: &mut Vec<u16>, spaces: &[u16]) -> bool {
    if s.is_empty() || spaces.is_empty() {
        return false;
    }
    let in_spaces = |c: u16| spaces.contains(&c);
    let first = s.iter().position(|&c| !in_spaces(c));
    match first {
        None => {
            // 全て spaces。
            s.clear();
            true
        }
        Some(first) => {
            let last = s.iter().rposition(|&c| !in_spaces(c)).unwrap();
            let len = last - first + 1;
            if len == s.len() {
                return false;
            }
            *s = s[first..=last].to_vec();
            true
        }
    }
}

/// 末尾から指定文字集合を取り除く。原実装 `StringUtility::TrimEnd` (StringUtility.cpp:219)。
pub fn trim_end(s: &mut Vec<u16>, spaces: &[u16]) -> bool {
    if spaces.is_empty() {
        return false;
    }
    let in_spaces = |c: u16| spaces.contains(&c);
    let len = match s.iter().rposition(|&c| !in_spaces(c)) {
        Some(idx) => idx + 1,
        None => 0,
    };
    if s.len() == len {
        return false;
    }
    s.truncate(len);
    true
}

/// 部分文字列 `from` を `to` に全置換。原実装 `StringUtility::Replace`(文字列版) (StringUtility.cpp:237)。
///
/// `from` が空のときは何もせず false。空文字列に対する無限ループは起こさない。
pub fn replace(s: &mut Vec<u16>, from: &[u16], to: &[u16]) -> bool {
    if from.is_empty() {
        return false;
    }
    let mut result: Vec<u16> = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if i + from.len() <= s.len() && &s[i..i + from.len()] == from {
            result.extend_from_slice(to);
            i += from.len();
        } else {
            result.push(s[i]);
            i += 1;
        }
    }
    *s = result;
    true
}

/// 1 文字を別の 1 文字に全置換。原実装 `StringUtility::Replace`(文字版) (StringUtility.cpp:259)。常に true。
pub fn replace_char(s: &mut [u16], from: u16, to: u16) -> bool {
    for c in s.iter_mut() {
        if *c == from {
            *c = to;
        }
    }
    true
}

/// 区切り文字列で分割。原実装 `StringUtility::Split` (StringUtility.cpp:360)。
///
/// `delimiter` が空のときは空 Vec を返す(原実装は false を返し pList を空にする)。
/// 末尾の区切り後も 1 要素(空でも)を必ず追加する点に注意。
pub fn split(src: &[u16], delimiter: &[u16]) -> Vec<Vec<u16>> {
    let mut list = Vec::new();
    if delimiter.is_empty() {
        return list;
    }
    let mut next = 0;
    let mut i = 0;
    while i + delimiter.len() <= src.len() {
        if &src[i..i + delimiter.len()] == delimiter {
            list.push(src[next..i].to_vec());
            i += delimiter.len();
            next = i;
        } else {
            i += 1;
        }
    }
    list.push(src[next..].to_vec());
    list
}

/// 区切り文字列で結合。原実装 `StringUtility::Combine` (StringUtility.cpp:382)。
pub fn combine(list: &[Vec<u16>], delimiter: &[u16]) -> Vec<u16> {
    let mut dst = Vec::new();
    for (i, item) in list.iter().enumerate() {
        if i > 0 {
            dst.extend_from_slice(delimiter);
        }
        dst.extend_from_slice(item);
    }
    dst
}

/// 制御文字・`%`・指定文字を `%XXXX`(大文字 4 桁 16 進)へエンコード。
/// 原実装 `StringUtility::Encode` (StringUtility.cpp:403)。
///
/// 既定のエンコード対象文字は原実装と同じ `\ " ' , /`。
pub fn encode(src: &[u16], encode_chars: &[u16]) -> Vec<u16> {
    let mut dst = Vec::with_capacity(src.len());
    for &c in src {
        let do_encode = c <= 0x19 || c == u16::from(b'%') || encode_chars.contains(&c);
        if do_encode {
            // "%{:04X}" 相当。
            for byte in format!("%{:04X}", c).bytes() {
                dst.push(u16::from(byte));
            }
        } else {
            dst.push(c);
        }
    }
    dst
}

/// 既定のエンコード対象文字 `\ " ' , /`(原実装の既定引数)。
pub fn default_encode_chars() -> Vec<u16> {
    "\\\"',/".encode_utf16().collect()
}

/// `%XXXX` 形式をデコード。原実装 `StringUtility::Decode` (StringUtility.cpp:440)。
///
/// `%` の直後を最大 4 桁の 16 進として読む(`HexStringToUInt(p, 4, &p)` 相当)。
pub fn decode(src: &[u16]) -> Vec<u16> {
    let mut dst = Vec::with_capacity(src.len());
    let mut i = 0;
    while i < src.len() {
        if src[i] == u16::from(b'%') {
            i += 1;
            let (code, consumed) = hex_string_to_uint(&src[i..], 4);
            dst.push(code as u16);
            i += consumed;
        } else {
            dst.push(src[i]);
            i += 1;
        }
    }
    dst
}

/// 最大 `max_len` 桁の 16 進文字列を数値化し、消費した桁数を返す。
/// TVTest の `HexStringToUInt`(Util.cpp)に相当する範囲を移植。
fn hex_string_to_uint(s: &[u16], max_len: usize) -> (u32, usize) {
    let mut value: u32 = 0;
    let mut consumed = 0;
    while consumed < max_len && consumed < s.len() {
        match digit_value(s[consumed], 16) {
            Some(d) => {
                value = value.wrapping_mul(16).wrapping_add(d);
                consumed += 1;
            }
            None => break,
        }
    }
    (value, consumed)
}

// ---------------------------------------------------------------------------
// FNV ハッシュ
// ---------------------------------------------------------------------------

const FNV_PRIME_32: u32 = 16_777_619;
const FNV_OFFSET_BASIS_32: u32 = 2_166_136_261;
const FNV_PRIME_64: u64 = 1_099_511_628_211;
const FNV_OFFSET_BASIS_64: u64 = 14_695_981_039_346_656_037;

/// 原実装 `Hash32` (StringUtility.cpp:500)。FNV 系だが乗算→XOR の順序が原実装どおり。
pub fn hash32(s: &[u16]) -> u32 {
    let mut hash = FNV_OFFSET_BASIS_32;
    for &c in s {
        hash = FNV_PRIME_32.wrapping_mul(hash) ^ u32::from(c);
    }
    hash
}

/// 原実装 `Hash64` (StringUtility.cpp:505)。
pub fn hash64(s: &[u16]) -> u64 {
    let mut hash = FNV_OFFSET_BASIS_64;
    for &c in s {
        hash = FNV_PRIME_64.wrapping_mul(hash) ^ u64::from(c);
    }
    hash
}

/// ASCII 範囲のみ小文字化する `towlower` 相当の近似。
///
/// 原実装は CRT の `std::towlower` を用いるためロケール依存だが、
/// 本移植では検証可能な ASCII 範囲のみを扱う(非 ASCII はそのまま)。
fn towlower_ascii(c: u16) -> u16 {
    if (u16::from(b'A')..=u16::from(b'Z')).contains(&c) {
        c + 32
    } else {
        c
    }
}

/// 原実装 `HashNoCase32` (StringUtility.cpp:510)。`towlower` 適用後にハッシュ。
pub fn hash_nocase32(s: &[u16]) -> u32 {
    let mut hash = FNV_OFFSET_BASIS_32;
    for &c in s {
        hash = FNV_PRIME_32.wrapping_mul(hash) ^ u32::from(towlower_ascii(c));
    }
    hash
}

/// 原実装 `HashNoCase64` (StringUtility.cpp:515)。
pub fn hash_nocase64(s: &[u16]) -> u64 {
    let mut hash = FNV_OFFSET_BASIS_64;
    for &c in s {
        hash = FNV_PRIME_64.wrapping_mul(hash) ^ u64::from(towlower_ascii(c));
    }
    hash
}

// ---------------------------------------------------------------------------
// テスト用ヘルパ
// ---------------------------------------------------------------------------

/// `&str` を UTF-16 コードユニット列に変換するヘルパ(テスト・呼び出し側の利便用)。
pub fn to_u16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// UTF-16 コードユニット列を `String` に変換するヘルパ。
pub fn from_u16(s: &[u16]) -> String {
    String::from_utf16_lossy(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Vec<u16> {
        to_u16(s)
    }

    #[test]
    fn test_string_to_int64_decimal() {
        assert_eq!(string_to_int64(&u("123")), 123);
        assert_eq!(string_to_int64(&u("-456")), -456);
        assert_eq!(string_to_int64(&u("+789")), 789);
        assert_eq!(string_to_int64(&u("  42")), 42); // 先頭空白スキップ
        assert_eq!(string_to_int64(&u("12abc")), 12); // 前方一致
        assert_eq!(string_to_int64(&u("abc")), 0);
        assert_eq!(string_to_int64(&u("")), 0);
    }

    #[test]
    fn test_string_to_int64_radix() {
        assert_eq!(string_to_int64(&u("0x1F")), 0x1F);
        assert_eq!(string_to_int64(&u("0X10")), 16);
        assert_eq!(string_to_int64(&u("010")), 8); // 8 進
        assert_eq!(string_to_int64(&u("0")), 0);
        assert_eq!(string_to_int64(&u("0x")), 0); // 接頭辞のみ
    }

    #[test]
    fn test_string_to_uint64() {
        assert_eq!(string_to_uint64(&u("18446744073709551615")), u64::MAX);
        assert_eq!(string_to_uint64(&u("0xFF")), 255);
        // 負号は符号反転(C の strtoull と同じ)
        assert_eq!(string_to_uint64(&u("-1")), u64::MAX);
    }

    #[test]
    fn test_string_is_digit() {
        assert!(string_is_digit(&u("123")));
        assert!(string_is_digit(&u("-123")));
        assert!(string_is_digit(&u("+9")));
        assert!(!string_is_digit(&u("")));
        assert!(!string_is_digit(&u("12a")));
        assert!(!string_is_digit(&u("-"))); // 符号のみは false
        assert!(!string_is_digit(&u("0x10")));
    }

    #[test]
    fn test_remove_trailing_whitespace() {
        let mut s = u("hello   ");
        assert_eq!(remove_trailing_whitespace(&mut s), 3);
        assert_eq!(from_u16(&s), "hello");

        let mut s = u("hello");
        assert_eq!(remove_trailing_whitespace(&mut s), 0);
        assert_eq!(from_u16(&s), "hello");

        let mut s = u("   ");
        assert_eq!(remove_trailing_whitespace(&mut s), 3);
        assert_eq!(from_u16(&s), "");

        let mut s = u("a b  \t\r\n");
        assert_eq!(remove_trailing_whitespace(&mut s), 5); // "  \t\r\n" = 5 文字
        assert_eq!(from_u16(&s), "a b");
    }

    #[test]
    fn test_skip_leading_whitespace() {
        assert_eq!(skip_leading_whitespace(&u("   abc")), 3);
        assert_eq!(skip_leading_whitespace(&u("abc")), 0);
        assert_eq!(skip_leading_whitespace(&u("\t\r\n x")), 4);
        assert_eq!(skip_leading_whitespace(&u("   ")), 3);
    }

    #[test]
    fn test_trim() {
        let mut s = u("  hello  ");
        assert!(trim(&mut s, &u(" \t")));
        assert_eq!(from_u16(&s), "hello");

        let mut s = u("hello");
        assert!(!trim(&mut s, &u(" \t"))); // 変更なし

        let mut s = u("    ");
        assert!(trim(&mut s, &u(" \t")));
        assert_eq!(from_u16(&s), "");

        let mut s = u("");
        assert!(!trim(&mut s, &u(" \t")));
    }

    #[test]
    fn test_trim_end() {
        let mut s = u("hello   ");
        assert!(trim_end(&mut s, &u(" ")));
        assert_eq!(from_u16(&s), "hello");

        let mut s = u("hello");
        assert!(!trim_end(&mut s, &u(" ")));

        let mut s = u("   ");
        assert!(trim_end(&mut s, &u(" ")));
        assert_eq!(from_u16(&s), "");
    }

    #[test]
    fn test_replace() {
        let mut s = u("a-b-c");
        assert!(replace(&mut s, &u("-"), &u("_")));
        assert_eq!(from_u16(&s), "a_b_c");

        let mut s = u("aaa");
        assert!(replace(&mut s, &u("a"), &u("bb")));
        assert_eq!(from_u16(&s), "bbbbbb");

        let mut s = u("hello world");
        assert!(replace(&mut s, &u("o"), &u(""))); // 削除
        assert_eq!(from_u16(&s), "hell wrld");

        let mut s = u("abc");
        assert!(!replace(&mut s, &u(""), &u("x"))); // from 空は false
        assert_eq!(from_u16(&s), "abc");
    }

    #[test]
    fn test_replace_char() {
        let mut s = u("a.b.c");
        assert!(replace_char(&mut s, u16::from(b'.'), u16::from(b'/')));
        assert_eq!(from_u16(&s), "a/b/c");
    }

    #[test]
    fn test_split() {
        let r = split(&u("a,b,c"), &u(","));
        assert_eq!(r.len(), 3);
        assert_eq!(from_u16(&r[0]), "a");
        assert_eq!(from_u16(&r[2]), "c");

        // 末尾区切りの後にも空要素が入る
        let r = split(&u("a,"), &u(","));
        assert_eq!(r.len(), 2);
        assert_eq!(from_u16(&r[1]), "");

        // 区切りが無い
        let r = split(&u("abc"), &u(","));
        assert_eq!(r.len(), 1);
        assert_eq!(from_u16(&r[0]), "abc");

        // 区切りが空は空 Vec
        let r = split(&u("abc"), &u(""));
        assert!(r.is_empty());

        // 複数文字区切り
        let r = split(&u("a::b::c"), &u("::"));
        assert_eq!(r.len(), 3);
        assert_eq!(from_u16(&r[1]), "b");
    }

    #[test]
    fn test_combine() {
        let list = vec![u("a"), u("b"), u("c")];
        assert_eq!(from_u16(&combine(&list, &u(","))), "a,b,c");
        assert_eq!(from_u16(&combine(&[], &u(","))), "");
        assert_eq!(from_u16(&combine(&[u("x")], &u(","))), "x");
    }

    #[test]
    fn test_split_combine_roundtrip() {
        let original = u("alpha,beta,gamma");
        let parts = split(&original, &u(","));
        assert_eq!(combine(&parts, &u(",")), original);
    }

    #[test]
    fn test_encode_decode() {
        let chars = default_encode_chars();

        // バックスラッシュはエンコード対象
        let encoded = encode(&u("a\\b"), &chars);
        assert_eq!(from_u16(&encoded), "a%005Cb");

        // % 自身もエンコード
        let encoded = encode(&u("100%"), &chars);
        assert_eq!(from_u16(&encoded), "100%0025");

        // 通常文字はそのまま
        let encoded = encode(&u("hello"), &chars);
        assert_eq!(from_u16(&encoded), "hello");

        // ラウンドトリップ
        let original = u("path\\to,file");
        let encoded = encode(&original, &chars);
        let decoded = decode(&encoded);
        assert_eq!(decoded, original);
    }

    #[test]
    fn test_decode_partial_hex() {
        // % の後が 4 桁未満でも読めるだけ読む
        assert_eq!(from_u16(&decode(&u("%41"))), "A"); // 0x41 = 'A'
        assert_eq!(from_u16(&decode(&u("%0041"))), "A");
    }

    #[test]
    fn test_hash_known_values() {
        // FNV のオフセット基底: 空文字列はオフセット基底そのもの
        assert_eq!(hash32(&u("")), FNV_OFFSET_BASIS_32);
        assert_eq!(hash64(&u("")), FNV_OFFSET_BASIS_64);

        // 決定性(同入力→同出力)
        assert_eq!(hash32(&u("TVTest")), hash32(&u("TVTest")));
        assert_ne!(hash32(&u("a")), hash32(&u("b")));
    }

    #[test]
    fn test_hash_nocase() {
        // 大小無視で一致
        assert_eq!(hash_nocase32(&u("ABC")), hash_nocase32(&u("abc")));
        assert_eq!(hash_nocase64(&u("Hello")), hash_nocase64(&u("hello")));
        // 大小区別ありとは異なる
        assert_ne!(hash32(&u("ABC")), hash32(&u("abc")));
    }
}
