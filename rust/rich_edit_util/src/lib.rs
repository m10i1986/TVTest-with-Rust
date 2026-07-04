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

//! TVTest のリッチエディットユーティリティ (`src/RichEditUtil.cpp` / `src/RichEditUtil.h`) の
//! 純粋部分 (Win32 API 非依存の文字列処理) を Rust に移植したクレート。
//!
//! 文字列は原実装の `wchar_t` に合わせて `&[u16]` (UTF-16 コード単位) で扱う。
//!
//! 移植対象:
//!   - [`URL_CHARS`]            : `m_pszURLChars` (RichEditUtil.cpp:31-35)
//!   - [`URL_FULL_WIDTH_CHARS`] : `m_pszURLFullWidthChars` (RichEditUtil.cpp:36-40)
//!   - [`CharRange`]            : `CHARRANGE` 相当 (RichEditUtil.h:35 の `CharRangeList` 要素)
//!   - [`search_next_url`]      : `SearchNextURL` (RichEditUtil.cpp:341-387)
//!   - [`find_urls`]            : `DetectURL` の URL 走査部分 (RichEditUtil.cpp:296-329)
//!   - [`url_to_half_width`]    : `DetectURL` の `ToHalfWidth` 処理 (RichEditUtil.cpp:305-321)
//!   - [`link_hit_test`]        : `LinkHitTest` の純粋部分 (RichEditUtil.cpp:409-423)
//!   - [`build_open_url`]       : `OpenLink` の URL 整形部分 (RichEditUtil.cpp:426-452)
//!
//! 対象外 (Win32 API / ウィンドウ操作依存のため):
//!   - RichEdit ウィンドウ操作全般: `LoadRichEditLib` / `UnloadRichEditLib` /
//!     `AppendText` / `CopyAllText` / `SelectAll` / `IsSelected` / `GetSelectedText` /
//!     `GetMaxLineWidth` / `DisableAutoFont`、および `DetectURL` の `SendMessage` 部分
//!     (`EM_GETLINE` による行単位のバッファ処理、`EM_EXSETSEL` / `EM_SETCHARFORMAT` /
//!     `EM_REPLACESEL` によるリンク書式設定・置換)。
//!   - `LogFontToCharFormat` / `LogFontToCharFormat2` / `CharFormatToCharFormat2`
//!     (HDC / `GetSysColor` 依存)。
//!   - `CRichEditLinkHandler` (マウス / カーソル処理)、`OpenLink` の `ShellExecute` 呼び出し。
//!
//! ## 原実装との対応に関する注記
//!
//! - 原実装の `SearchNextURL` はプレフィックス照合に
//!   `CompareString(LOCALE_USER_DEFAULT, NORM_IGNOREWIDTH)` を使用する。
//!   `NORM_IGNOREWIDTH` は全角半角を同一視するため、本クレートでは
//!   [`URL_FULL_WIDTH_CHARS`] → [`URL_CHARS`] の対応で各文字を半角化してから
//!   ASCII 比較する方式で照合する (プレフィックス文字は全て URL 文字集合内なので等価)。
//! - 原実装の `OpenLink` は `LCMapString(LCMAP_HALFWIDTH)` で全角→半角変換を行うが、
//!   [`build_open_url`] は URL 文字テーブルの全角→半角化で近似する
//!   (URL 検出済み文字列が入力である前提。テーブル外の文字はそのまま維持される)。

#![forbid(unsafe_code)]

// ---------------------------------------------------------------------------
// 定数・型定義
// ---------------------------------------------------------------------------

/// URL 文字テーブルの要素数 (半角・全角共通)。
const URL_CHARS_LEN: usize = 85;

/// UTF-8 文字列リテラルを UTF-16 コード単位の配列へ変換する const fn。
///
/// BMP 内の文字 (UTF-8 で 1〜3 バイト) のみ対応。文字数が `N` と一致しない場合や
/// BMP 外の文字が含まれる場合はコンパイル時に panic する。
const fn utf8_to_utf16_units<const N: usize>(s: &str) -> [u16; N] {
    let bytes = s.as_bytes();
    let mut out = [0u16; N];
    let mut i = 0;
    let mut n = 0;
    while i < bytes.len() {
        let b0 = bytes[i] as u32;
        let (cp, adv) = if b0 < 0x80 {
            (b0, 1)
        } else if b0 < 0xE0 {
            (((b0 & 0x1F) << 6) | (bytes[i + 1] as u32 & 0x3F), 2)
        } else {
            assert!(b0 < 0xF0, "BMP 外の文字 (サロゲートペア) は非対応");
            (
                ((b0 & 0x0F) << 12) | ((bytes[i + 1] as u32 & 0x3F) << 6) | (bytes[i + 2] as u32 & 0x3F),
                3,
            )
        };
        out[n] = cp as u16;
        n += 1;
        i += adv;
    }
    assert!(n == N, "文字数が配列長と一致しない");
    out
}

/// URL に使用可能な半角文字のテーブル。
///
/// 原実装: `CRichEditUtil::m_pszURLChars` (RichEditUtil.cpp:31-35)。
/// [`URL_FULL_WIDTH_CHARS`] とインデックスで 1 対 1 に対応する。
pub const URL_CHARS: [u16; URL_CHARS_LEN] = utf8_to_utf16_units(concat!(
    "0123456789",
    "ABCDEFGHIJKLMNOPQRSTUVWXYZ",
    "abcdefghijklmnopqrstuvwxyz",
    "!#$%&'()*+,-./:;=?@[]_~",
));

/// URL に使用可能な全角文字のテーブル。
///
/// 原実装: `CRichEditUtil::m_pszURLFullWidthChars` (RichEditUtil.cpp:36-40)。
/// [`URL_CHARS`] とインデックスで 1 対 1 に対応する。
/// 半角アポストロフィ `'` に対応する全角は `’` (U+2019) である点に注意
/// (U+FF07 ではない。原実装のテーブルの通り)。
pub const URL_FULL_WIDTH_CHARS: [u16; URL_CHARS_LEN] = utf8_to_utf16_units(concat!(
    "０１２３４５６７８９",
    "ＡＢＣＤＥＦＧＨＩＪＫＬＭＮＯＰＱＲＳＴＵＶＷＸＹＺ",
    "ａｂｃｄｅｆｇｈｉｊｋｌｍｎｏｐｑｒｓｔｕｖｗｘｙｚ",
    "！＃＄％＆’（）＊＋，－．／：；＝？＠［］＿～",
));

/// 半角開き括弧 `(`。
const HALF_OPEN_PAREN: u16 = b'(' as u16;
/// 半角閉じ括弧 `)`。
const HALF_CLOSE_PAREN: u16 = b')' as u16;
/// 全角開き括弧 `（` (U+FF08)。
const FULL_OPEN_PAREN: u16 = 0xFF08;
/// 全角閉じ括弧 `）` (U+FF09)。
const FULL_CLOSE_PAREN: u16 = 0xFF09;

/// Win32 `CHARRANGE` に対応する構造体 (RichEditUtil.h:35 の `CharRangeList` 要素)。
///
/// `cp_min` は範囲の開始位置 (含む)、`cp_max` は終了位置 (含まない)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CharRange {
    /// 範囲の開始位置 (含む)。`CHARRANGE::cpMin` 相当。
    pub cp_min: i32,
    /// 範囲の終了位置 (含まない)。`CHARRANGE::cpMax` 相当。
    pub cp_max: i32,
}

/// URL プレフィックスのリスト (RichEditUtil.cpp:343-350 の `URLPrefixList`)。
const URL_PREFIXES: [&[u8]; 3] = [b"http://", b"https://", b"www."];

// ---------------------------------------------------------------------------
// 内部ヘルパ
// ---------------------------------------------------------------------------

/// 全角 URL 文字を対応する半角 URL 文字へ変換する。テーブルに無い文字は `None`。
///
/// 原実装の `::StrChr(m_pszURLFullWidthChars, c)` +
/// `m_pszURLChars[pFound - m_pszURLFullWidthChars]` (RichEditUtil.cpp:308-311) 相当。
fn full_width_to_half_width(c: u16) -> Option<u16> {
    URL_FULL_WIDTH_CHARS
        .iter()
        .position(|&f| f == c)
        .map(|i| URL_CHARS[i])
}

/// テキスト先頭が ASCII プレフィックスと全角半角無視で一致するか判定する。
///
/// 原実装の `CompareString(LOCALE_USER_DEFAULT, NORM_IGNOREWIDTH, ...)`
/// (RichEditUtil.cpp:358-361) の近似。プレフィックス文字は全て URL 文字集合に
/// 含まれるため、テキスト側を URL 文字テーブルで半角化してから比較すれば等価になる。
fn matches_prefix_ignore_width(text: &[u16], prefix: &[u8]) -> bool {
    debug_assert!(text.len() >= prefix.len());
    prefix.iter().zip(text).all(|(&pc, &tc)| {
        let half = full_width_to_half_width(tc).unwrap_or(tc);
        half == u16::from(pc)
    })
}

// ---------------------------------------------------------------------------
// 公開 API
// ---------------------------------------------------------------------------

/// テキストから次の URL を検索する。
///
/// 原実装: `CRichEditUtil::SearchNextURL` (RichEditUtil.cpp:341-387)。
///
/// 戻り値は `(URL 開始オフセット, URL 長)` (いずれも UTF-16 コード単位)。
/// 見つからない場合は `None`。
///
/// 原実装の挙動を厳密に再現する:
/// - 走査は `for (int i = 0; i < TextLength - 4; i++)`。テキスト長が 5 未満の
///   場合は走査ゼロ回で `None` (int の引き算で上限が 0 以下になるため)。
/// - プレフィックス (`http://` / `https://` / `www.`) の一致条件は
///   `i + プレフィックス長 < テキスト長` (テキスト末尾ちょうどで終わるプレフィックス
///   は一致扱いにならない)。全角形 (例: `ｈｔｔｐ：／／`) も一致する。
/// - URL 本体は「文字が < U+0080 なら [`URL_CHARS`] に含まれる限り、それ以外は
///   [`URL_FULL_WIDTH_CHARS`] に含まれる限り」伸長する (RichEditUtil.cpp:362-371)。
/// - 末尾処理 (RichEditUtil.cpp:372-378): URL 直前の文字が `(` または `（` で
///   最終文字が `)` または `）` の場合、または最終文字自体が `(` または `（` の
///   場合、URL 長を 1 縮める。
pub fn search_next_url(text: &[u16]) -> Option<(usize, usize)> {
    let text_length = text.len();

    // 原実装 (:355): for (int i = 0; i < TextLength - 4; i++)
    // TextLength < 5 のときは走査ゼロ回 (saturating_sub で int の負値上限を再現)。
    for i in 0..text_length.saturating_sub(4) {
        for prefix in URL_PREFIXES {
            let mut url_length = prefix.len();
            if i + url_length < text_length && matches_prefix_ignore_width(&text[i..], prefix) {
                // URL 本体の伸長 (:362-371)
                while i + url_length < text_length {
                    let c = text[i + url_length];
                    let in_table = if c < 0x0080 {
                        URL_CHARS.contains(&c)
                    } else {
                        URL_FULL_WIDTH_CHARS.contains(&c)
                    };
                    if !in_table {
                        break;
                    }
                    url_length += 1;
                }
                // 括弧終端の除去 (:372-378)
                let last_char = text[i + url_length - 1];
                if (i > 0
                    && (text[i - 1] == HALF_OPEN_PAREN || text[i - 1] == FULL_OPEN_PAREN)
                    && (last_char == HALF_CLOSE_PAREN || last_char == FULL_CLOSE_PAREN))
                    || last_char == HALF_OPEN_PAREN
                    || last_char == FULL_OPEN_PAREN
                {
                    url_length -= 1;
                }
                return Some((i, url_length));
            }
        }
    }

    None
}

/// テキスト全体から URL を列挙する。
///
/// 原実装: `CRichEditUtil::DetectURL` の URL 走査部分 (RichEditUtil.cpp:296-329)。
/// [`search_next_url`] を繰り返し呼び、見つかるたびに次の走査開始位置を URL 終端へ
/// 進める (原実装の `q += Length; Length = TotalLength - (q - szText);` 相当)。
///
/// 戻り値の [`CharRange`] は `text` 先頭を基準とするオフセット。
/// 原実装の RichEdit 行単位バッファ処理 (`EM_GETLINE`) は対象外なので、
/// 入力はテキスト全体とする (原実装の `LineIndex` 加算は行わない)。
pub fn find_urls(text: &[u16]) -> Vec<CharRange> {
    let mut ranges = Vec::new();
    let mut scan_start = 0usize;

    while let Some((offset, length)) = search_next_url(&text[scan_start..]) {
        let cp_min = (scan_start + offset) as i32;
        let cp_max = cp_min + length as i32;
        ranges.push(CharRange { cp_min, cp_max });
        scan_start += offset + length;
    }

    ranges
}

/// URL 内の全角 URL 文字を対応する半角文字へ置換する。
///
/// 原実装: `CRichEditUtil::DetectURL` の `ToHalfWidth` 処理 (RichEditUtil.cpp:305-321)。
///
/// [`URL_FULL_WIDTH_CHARS`] に含まれる文字を対応する [`URL_CHARS`] の文字へ置換する。
/// 1 文字も置換が発生しなければ `None` を返す
/// (原実装では `pszURL == nullptr` のままで `EM_REPLACESEL` による置換をスキップする)。
pub fn url_to_half_width(url: &[u16]) -> Option<Vec<u16>> {
    let mut converted: Option<Vec<u16>> = None;

    for (j, &c) in url.iter().enumerate() {
        if let Some(half) = full_width_to_half_width(c) {
            converted.get_or_insert_with(|| url.to_vec())[j] = half;
        }
    }

    converted
}

/// 文字位置がどのリンク範囲に含まれるかを判定する。
///
/// 原実装: `CRichEditUtil::LinkHitTest` の純粋部分 (RichEditUtil.cpp:409-423)。
/// 原実装の `EM_CHARFROMPOS` (座標→文字位置変換) は対象外で、変換済みの
/// 文字位置 `char_index` を受け取る。
///
/// `cp_min <= char_index` かつ `cp_max > char_index` を満たす最初の要素番号を返す。
/// 該当なし・空リストの場合は `None` (原実装の `-1` 相当)。
pub fn link_hit_test(char_index: i32, link_list: &[CharRange]) -> Option<usize> {
    link_list
        .iter()
        .position(|range| range.cp_min <= char_index && range.cp_max > char_index)
}

/// リンクテキストから開くべき URL を整形する。
///
/// 原実装: `CRichEditUtil::OpenLink` の URL 整形部分 (RichEditUtil.cpp:426-452)。
/// `EM_GETTEXTRANGE` によるテキスト取得と `ShellExecute` は対象外で、
/// 取得済みのリンクテキスト `text` を受け取る。
///
/// - 長さが 256 以上の場合は `None` (原実装 :428 の
///   `Range.cpMax - Range.cpMin >= 256` で `false` を返すケース)。
/// - 空の場合は `None` (原実装 :437 の `Length <= 0` で `false` を返すケース)。
/// - `LCMapString(LCMAP_HALFWIDTH)` (:441-443) は URL 文字テーブルの全角→半角化で
///   近似する (URL 検出済み文字列が入力である前提。テーブル外の文字は維持される)。
/// - 半角化後の先頭 4 文字が `www.` なら `http://` を前置する (:445-448 の
///   `StrCmpN` は大文字小文字を区別するため、`WWW.` は前置対象外)。
pub fn build_open_url(text: &[u16]) -> Option<Vec<u16>> {
    if text.len() >= 256 {
        return None;
    }
    if text.is_empty() {
        return None;
    }

    let mut url: Vec<u16> = text
        .iter()
        .map(|&c| full_width_to_half_width(c).unwrap_or(c))
        .collect();

    const HTTP_PREFIX: [u16; 7] = [
        b'h' as u16,
        b't' as u16,
        b't' as u16,
        b'p' as u16,
        b':' as u16,
        b'/' as u16,
        b'/' as u16,
    ];
    const WWW_DOT: [u16; 4] = [b'w' as u16, b'w' as u16, b'w' as u16, b'.' as u16];

    if url.len() >= WWW_DOT.len() && url[..WWW_DOT.len()] == WWW_DOT {
        let mut with_scheme = Vec::with_capacity(HTTP_PREFIX.len() + url.len());
        with_scheme.extend_from_slice(&HTTP_PREFIX);
        with_scheme.append(&mut url);
        url = with_scheme;
    }

    Some(url)
}

// ---------------------------------------------------------------------------
// テスト
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    // ----- 文字テーブル -----

    #[test]
    fn test_url_char_tables_contents() {
        // 半角テーブルは原実装 (RichEditUtil.cpp:31-35) の文字列と一致する
        assert_eq!(
            URL_CHARS.to_vec(),
            u("0123456789\
               ABCDEFGHIJKLMNOPQRSTUVWXYZ\
               abcdefghijklmnopqrstuvwxyz\
               !#$%&'()*+,-./:;=?@[]_~")
        );

        // 全角テーブルは原実装 (RichEditUtil.cpp:36-40) のコードポイントと一致する
        let mut expected: Vec<u16> = Vec::new();
        expected.extend(0xFF10u16..=0xFF19); // ０-９
        expected.extend(0xFF21u16..=0xFF3A); // Ａ-Ｚ
        expected.extend(0xFF41u16..=0xFF5A); // ａ-ｚ
        expected.extend([
            0xFF01, // ！
            0xFF03, // ＃
            0xFF04, // ＄
            0xFF05, // ％
            0xFF06, // ＆
            0x2019, // ’ (U+FF07 ではない)
            0xFF08, // （
            0xFF09, // ）
            0xFF0A, // ＊
            0xFF0B, // ＋
            0xFF0C, // ，
            0xFF0D, // －
            0xFF0E, // ．
            0xFF0F, // ／
            0xFF1A, // ：
            0xFF1B, // ；
            0xFF1D, // ＝
            0xFF1F, // ？
            0xFF20, // ＠
            0xFF3B, // ［
            0xFF3D, // ］
            0xFF3F, // ＿
            0xFF5E, // ～
        ]);
        assert_eq!(URL_FULL_WIDTH_CHARS.to_vec(), expected);
    }

    #[test]
    fn test_full_width_to_half_width_mapping() {
        // インデックス 1 対 1 対応の代表例
        assert_eq!(full_width_to_half_width(0xFF10), Some(u16::from(b'0'))); // ０
        assert_eq!(full_width_to_half_width(0xFF3A), Some(u16::from(b'Z'))); // Ｚ
        assert_eq!(full_width_to_half_width(0xFF41), Some(u16::from(b'a'))); // ａ
        assert_eq!(full_width_to_half_width(0x2019), Some(u16::from(b'\''))); // ’ → '
        assert_eq!(full_width_to_half_width(0xFF5E), Some(u16::from(b'~'))); // ～ → ~
        // テーブル外
        assert_eq!(full_width_to_half_width(u16::from(b'a')), None); // 半角はテーブル外
        assert_eq!(full_width_to_half_width(0x3042), None); // あ
        assert_eq!(full_width_to_half_width(0xFF07), None); // ＇ (テーブルに無い)
    }

    // ----- search_next_url: 基本検出 -----

    #[test]
    fn test_search_http_middle() {
        // 中間位置の http:// URL。直後の空白で停止する
        let text = u("見て http://example.com を");
        assert_eq!(search_next_url(&text), Some((3, 18)));
    }

    #[test]
    fn test_search_https_whole_text() {
        // 先頭位置・テキスト全体が URL (パス・クエリ文字も URL 文字)
        let text = u("https://example.com/path?q=1");
        assert_eq!(search_next_url(&text), Some((0, 28)));
    }

    #[test]
    fn test_search_www_at_start() {
        let text = u("www.example.com");
        assert_eq!(search_next_url(&text), Some((0, 15)));
    }

    #[test]
    fn test_search_url_at_end() {
        // テキスト末尾で終わる URL
        let text = u("go http://a.bc");
        assert_eq!(search_next_url(&text), Some((3, 11)));
    }

    #[test]
    fn test_search_bare_prefix_followed_by_non_url_char() {
        // プレフィックスの直後に URL 文字が無くてもプレフィックス単体で検出される
        // (原実装の挙動)
        let text = u("aaa http:// bbb");
        assert_eq!(search_next_url(&text), Some((4, 7)));
    }

    #[test]
    fn test_search_prefix_at_text_end_not_matched() {
        // 一致条件は i + プレフィックス長 < テキスト長 (「<=」ではない) なので、
        // テキスト末尾ちょうどで終わるプレフィックスは検出されない
        assert_eq!(search_next_url(&u("see http://")), None);
        assert_eq!(search_next_url(&u("https://")), None);
        assert_eq!(search_next_url(&u("xwww.")), None);
    }

    #[test]
    fn test_search_text_too_short() {
        // TextLength < 5 は走査ゼロ回 (原実装の for (i = 0; i < TextLength - 4; i++))
        assert_eq!(search_next_url(&u("")), None);
        assert_eq!(search_next_url(&u("www.")), None);
        assert_eq!(search_next_url(&u("http")), None);
        assert_eq!(search_next_url(&u("a")), None);
    }

    #[test]
    fn test_search_minimum_match() {
        // 長さ 5 が検出可能な最小テキスト
        assert_eq!(search_next_url(&u("www.a")), Some((0, 5)));
        // 開始位置がずれた最小ケース
        assert_eq!(search_next_url(&u("xwww.a")), Some((1, 5)));
    }

    #[test]
    fn test_search_last_possible_position() {
        // 走査上限ぎりぎり (i = TextLength - 5) での検出
        let text = u("aaaaawww.b");
        assert_eq!(search_next_url(&text), Some((5, 5)));
    }

    #[test]
    fn test_search_case_sensitive_prefix() {
        // CompareString は NORM_IGNORECASE 無しなので大文字プレフィックスは不一致
        assert_eq!(search_next_url(&u("HTTP://example.com")), None);
        assert_eq!(search_next_url(&u("WWW.example.com")), None);
    }

    // ----- search_next_url: 全角 -----

    #[test]
    fn test_search_full_width_http() {
        // 全角の ｈｔｔｐ：／／ も NORM_IGNOREWIDTH 相当で一致する
        let text = u("ｈｔｔｐ：／／ｅｘａｍｐｌｅ．ｃｏｍ");
        assert_eq!(search_next_url(&text), Some((0, 18)));
    }

    #[test]
    fn test_search_full_width_www() {
        let text = u("ｗｗｗ．ｅｘａｍｐｌｅ．ｃｏｍ");
        assert_eq!(search_next_url(&text), Some((0, 15)));
    }

    #[test]
    fn test_search_mixed_width_prefix() {
        // 半角/全角混在のプレフィックス (httpｓ：// → https://)
        let text = u("httpｓ：//a.b");
        assert_eq!(search_next_url(&text), Some((0, 11)));
    }

    #[test]
    fn test_search_full_width_body_extension() {
        // 半角プレフィックス + 全角本体も伸長される
        let text = u("http://ｅｘａｍｐｌｅ．ｃｏｍ 続き");
        assert_eq!(search_next_url(&text), Some((0, 18)));
    }

    // ----- search_next_url: 括弧終端処理 -----

    #[test]
    fn test_search_paren_wrapped() {
        // 「(URL)」の形: ')' は URL 文字なので一旦含まれ、その後 1 縮められる
        let text = u("(http://example.com/)");
        assert_eq!(search_next_url(&text), Some((1, 19)));
    }

    #[test]
    fn test_search_full_width_paren_wrapped() {
        // 全角括弧「（URL）」も同様
        let text = u("（ｈｔｔｐ：／／ａ．ｂ）");
        assert_eq!(search_next_url(&text), Some((1, 10)));
    }

    #[test]
    fn test_search_mixed_paren_wrapped() {
        // 半角開き括弧 + 全角閉じ括弧の混在でも縮められる
        let text = u("(http://a.b）");
        assert_eq!(search_next_url(&text), Some((1, 10)));
    }

    #[test]
    fn test_search_trailing_close_paren_without_open() {
        // 直前が '(' でなければ末尾の ')' は URL に含まれたまま
        let text = u(")http://a.b)");
        assert_eq!(search_next_url(&text), Some((1, 11)));
    }

    #[test]
    fn test_search_trailing_open_paren() {
        // 末尾が '(' の場合は無条件で 1 縮める
        let text = u("http://example.com(");
        assert_eq!(search_next_url(&text), Some((0, 18)));
    }

    #[test]
    fn test_search_trailing_full_width_open_paren() {
        // 末尾が '（' の場合も同様
        let text = u("www.a（");
        assert_eq!(search_next_url(&text), Some((0, 5)));
    }

    // ----- search_next_url: URL 文字境界 -----

    #[test]
    fn test_search_stops_at_boundary_chars() {
        // 半角空白で停止
        assert_eq!(search_next_url(&u("http://a.b c")), Some((0, 10)));
        // 全角空白 (U+3000、テーブル外) で停止
        assert_eq!(search_next_url(&u("http://a.b　c")), Some((0, 10)));
        // 日本語 (テーブル外の非 ASCII) で停止
        assert_eq!(search_next_url(&u("http://例え")), Some((0, 7)));
        // '"' や '<' (テーブル外の ASCII) で停止
        assert_eq!(search_next_url(&u("http://a.b\"x")), Some((0, 10)));
        assert_eq!(search_next_url(&u("http://a.b<x")), Some((0, 10)));
    }

    #[test]
    fn test_search_no_url() {
        assert_eq!(search_next_url(&u("ただのテキストです")), None);
        assert_eq!(search_next_url(&u("no url here at all")), None);
    }

    // ----- find_urls -----

    #[test]
    fn test_find_urls_multiple() {
        let text = u("a http://x.jp と www.y.com 。");
        assert_eq!(
            find_urls(&text),
            vec![
                CharRange { cp_min: 2, cp_max: 13 },
                CharRange { cp_min: 16, cp_max: 25 },
            ]
        );
    }

    #[test]
    fn test_find_urls_paren_wrapped_pair() {
        // 括弧付き URL が複数あっても、走査再開位置が URL 終端になるため
        // 2 個目も正しく検出される
        let text = u("(www.a) (www.b)");
        assert_eq!(
            find_urls(&text),
            vec![
                CharRange { cp_min: 1, cp_max: 6 },
                CharRange { cp_min: 9, cp_max: 14 },
            ]
        );
    }

    #[test]
    fn test_find_urls_adjacent_merged() {
        // URL 直後に別の URL が連続する場合、'h' ':' '/' 等も URL 文字なので
        // 1 個の URL として貪欲に伸長される (原実装の挙動)
        let text = u("http://a.bhttp://c.d");
        assert_eq!(find_urls(&text), vec![CharRange { cp_min: 0, cp_max: 20 }]);
    }

    #[test]
    fn test_find_urls_none() {
        assert_eq!(find_urls(&u("")), vec![]);
        assert_eq!(find_urls(&u("URL の無いテキスト")), vec![]);
    }

    #[test]
    fn test_find_urls_full_width() {
        let text = u("詳細は ｈｔｔｐ：／／ａ．ｂ まで");
        assert_eq!(find_urls(&text), vec![CharRange { cp_min: 4, cp_max: 14 }]);
    }

    // ----- url_to_half_width -----

    #[test]
    fn test_url_to_half_width_no_conversion() {
        // 全て半角なら置換不発生 → None (原実装は pszURL == nullptr のまま)
        assert_eq!(url_to_half_width(&u("http://example.com")), None);
        assert_eq!(url_to_half_width(&u("")), None);
    }

    #[test]
    fn test_url_to_half_width_mixed() {
        assert_eq!(
            url_to_half_width(&u("ｈttp://ｅxample.com")),
            Some(u("http://example.com"))
        );
    }

    #[test]
    fn test_url_to_half_width_all_full_width() {
        assert_eq!(
            url_to_half_width(&u("ｗｗｗ．ｅｘａｍｐｌｅ．ｃｏｍ")),
            Some(u("www.example.com"))
        );
    }

    #[test]
    fn test_url_to_half_width_apostrophe() {
        // ’ (U+2019) → ' に変換される
        assert_eq!(url_to_half_width(&u("ａ’ｂ")), Some(u("a'b")));
    }

    #[test]
    fn test_url_to_half_width_keeps_non_table_chars() {
        // テーブル外の文字 (日本語等) はそのまま維持される
        assert_eq!(url_to_half_width(&u("ａあｂ")), Some(u("aあb")));
    }

    // ----- link_hit_test -----

    #[test]
    fn test_link_hit_test_boundaries() {
        let list = [
            CharRange { cp_min: 5, cp_max: 10 },
            CharRange { cp_min: 20, cp_max: 30 },
        ];
        assert_eq!(link_hit_test(4, &list), None);
        assert_eq!(link_hit_test(5, &list), Some(0)); // cp_min ちょうど (含む)
        assert_eq!(link_hit_test(9, &list), Some(0));
        assert_eq!(link_hit_test(10, &list), None); // cp_max ちょうど (含まない)
        assert_eq!(link_hit_test(20, &list), Some(1));
        assert_eq!(link_hit_test(29, &list), Some(1));
        assert_eq!(link_hit_test(30, &list), None);
    }

    #[test]
    fn test_link_hit_test_empty_list() {
        assert_eq!(link_hit_test(0, &[]), None);
    }

    #[test]
    fn test_link_hit_test_first_match_wins() {
        // 重複する範囲では最初の要素が返る (原実装は先頭から線形探索)
        let list = [
            CharRange { cp_min: 0, cp_max: 10 },
            CharRange { cp_min: 5, cp_max: 15 },
        ];
        assert_eq!(link_hit_test(7, &list), Some(0));
    }

    #[test]
    fn test_link_hit_test_negative_index() {
        let list = [CharRange { cp_min: 0, cp_max: 5 }];
        assert_eq!(link_hit_test(-1, &list), None);
    }

    // ----- build_open_url -----

    #[test]
    fn test_build_open_url_www_prefixed() {
        assert_eq!(
            build_open_url(&u("www.example.com")),
            Some(u("http://www.example.com"))
        );
    }

    #[test]
    fn test_build_open_url_http_unchanged() {
        assert_eq!(
            build_open_url(&u("http://example.com")),
            Some(u("http://example.com"))
        );
    }

    #[test]
    fn test_build_open_url_full_width_www() {
        // 全角 ｗｗｗ．は半角化後に www. と判定され http:// が前置される
        assert_eq!(
            build_open_url(&u("ｗｗｗ．ｅｘａｍｐｌｅ．ｃｏｍ")),
            Some(u("http://www.example.com"))
        );
    }

    #[test]
    fn test_build_open_url_full_width_http() {
        assert_eq!(build_open_url(&u("ｈｔｔｐ：／／ａ")), Some(u("http://a")));
    }

    #[test]
    fn test_build_open_url_www_case_sensitive() {
        // StrCmpN は大文字小文字を区別するため WWW. は前置されない
        assert_eq!(build_open_url(&u("WWW.example.com")), Some(u("WWW.example.com")));
    }

    #[test]
    fn test_build_open_url_short_text() {
        // 4 文字未満は www. 判定に届かずそのまま返る
        assert_eq!(build_open_url(&u("www")), Some(u("www")));
        assert_eq!(build_open_url(&u("a")), Some(u("a")));
    }

    #[test]
    fn test_build_open_url_empty() {
        // 原実装 :437 の Length <= 0 → false 相当
        assert_eq!(build_open_url(&u("")), None);
    }

    #[test]
    fn test_build_open_url_length_limit() {
        // 原実装 :428 の cpMax - cpMin >= 256 → false 相当
        let len255 = vec![u16::from(b'a'); 255];
        assert_eq!(build_open_url(&len255), Some(len255.clone()));

        let len256 = vec![u16::from(b'a'); 256];
        assert_eq!(build_open_url(&len256), None);
    }

    #[test]
    fn test_build_open_url_www_at_length_limit() {
        // 255 文字の www. URL は http:// 前置で 262 文字になる
        let mut text = u("www.");
        text.extend(std::iter::repeat_n(u16::from(b'a'), 251));
        assert_eq!(text.len(), 255);
        let result = build_open_url(&text).unwrap();
        assert_eq!(result.len(), 262);
        assert_eq!(&result[..11], &u("http://www.")[..]);
    }
}
