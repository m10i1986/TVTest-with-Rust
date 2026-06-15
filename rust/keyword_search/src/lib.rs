// TVTest の KeywordSearch.cpp / KeywordSearch.h の純粋ロジックを Rust へ移植したもの。
//
// 検索エンジンの URL テンプレート(例: "https://example.com/?q={keyword:utf-8}")を
// 与えられたキーワードで展開する処理が中核。原実装は以下に密結合している:
//   - CSettings           : 検索エンジン一覧の読み込み(Load)
//   - WideCharToMultiByte / mlang.dll : コードページ変換(EncodeURL)
//   - StringUtility::ToHalfWidthNoKatakana : 半角化(未移植)
//   - ShellExecute / AppendMenu : ブラウザ起動・メニュー生成
//
// 本クレートでは:
//   - parse_search_engine_entry : Load のエントリ判定(`xxx.Name` → `xxx.URL` 対応)
//   - percent_encode_bytes      : EncodeURL のバイト列 → パーセントエンコード部(空白→'+'、
//                                 英字はそのまま、他は %XX)
//   - build_search_url          : Search の URL テンプレート展開(`{keyword:...}` 解決)
//                                 を KeywordEncoder trait で抽象化して移植
// を提供する。コードページ変換・半角化・ブラウザ起動は呼び出し側に委ねる。
//
// 文字列は原実装の wchar_t(UTF-16)に合わせ &[u16] / Vec<u16> ベースで扱う。

use tvtest_string_utility as su;

/// 検索エンジン情報。原実装 KeywordSearch.h CKeywordSearch::SearchEngineInfo。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchEngineInfo {
    pub name: Vec<u16>,
    pub url: Vec<u16>,
}

/// キーワードのエンコード先コードページ種別。
/// 原実装 Search の ParameterList(`{keyword:...}` の `...` 部分)に対応する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeywordCharset {
    Utf8,
    ShiftJis,
    EucJp,
    Iso2022Jp,
}

impl KeywordCharset {
    /// `{keyword:...}` のパラメータ名(大小無視)から charset を判定する。
    /// 原実装 Search:115-123 の ParameterList。
    pub fn from_param(param: &[u16]) -> Option<Self> {
        const LIST: [(&str, KeywordCharset); 4] = [
            ("keyword:utf-8", KeywordCharset::Utf8),
            ("keyword:shift_jis", KeywordCharset::ShiftJis),
            ("keyword:euc-jp", KeywordCharset::EucJp),
            ("keyword:iso-2022-jp", KeywordCharset::Iso2022Jp),
        ];
        for (name, charset) in LIST.iter() {
            if is_equal_no_case(param, name) {
                return Some(*charset);
            }
        }
        None
    }
}

/// 大小無視の文字列比較(原実装 StringUtility::IsEqualNoCase の ASCII 近似)。
/// パラメータ名はすべて ASCII のため、これで原実装と一致する。
fn is_equal_no_case(a: &[u16], b: &str) -> bool {
    let bb: Vec<u16> = b.encode_utf16().collect();
    if a.len() != bb.len() {
        return false;
    }
    a.iter().zip(bb.iter()).all(|(&x, &y)| towlower_ascii(x) == towlower_ascii(y))
}

fn towlower_ascii(c: u16) -> u16 {
    if (u16::from(b'A')..=u16::from(b'Z')).contains(&c) {
        c + 32
    } else {
        c
    }
}

/// バイト列をパーセントエンコードして dst(UTF-16)へ追記する。
/// 原実装 EncodeURL:235-246。
/// 0x20(空白) → '+'、英字(A-Z/a-z)はそのまま、それ以外は `%XX`(大文字16進)。
pub fn percent_encode_bytes(bytes: &[u8], dst: &mut Vec<u16>) {
    for &b in bytes {
        if b == 0x20 {
            dst.push(u16::from(b'+'));
        } else if (0x41..=0x5A).contains(&b) || (0x61..=0x7A).contains(&b) {
            dst.push(b as u16);
        } else {
            // "%XX"(2桁大文字16進)。
            let s = format!("%{:02X}", b);
            dst.extend(s.encode_utf16());
        }
    }
}

/// 設定エントリ(名前, 値)が検索エンジンの `Name` エントリなら、
/// 対応する URL キー名を返す。原実装 Load:55-68 のエントリ判定。
///
/// 条件: 名前に '.' があり(位置 > 0)、'.' 以降が "Name"(大小無視)であること。
/// 戻り値は URL を引くためのキー名 `<prefix>.URL`(例 "Engine1.Name" → "Engine1.URL")。
/// 該当しなければ None。
pub fn search_engine_url_key(entry_name: &[u16]) -> Option<Vec<u16>> {
    let dot = u16::from(b'.');
    let pos = entry_name.iter().position(|&c| c == dot)?;
    if pos == 0 {
        return None;
    }
    let suffix = &entry_name[pos + 1..];
    if !is_equal_no_case(suffix, "Name") {
        return None;
    }
    // "<prefix>." + "URL"。原実装は substr(0, Pos+1) に "URL" を連結。
    let mut key: Vec<u16> = entry_name[..=pos].to_vec(); // prefix + '.'
    key.extend(su::to_u16("URL"));
    Some(key)
}

/// キーワードを指定 charset でエンコードするためのトレイト。
/// 原実装 EncodeURL のコードページ変換・半角化を抽象化する。
///
/// 実装側は、キーワードを必要に応じて半角化・トリムし、charset に対応する
/// バイト列へ変換したうえで [`percent_encode_bytes`] で dst へ追記する責務を持つ。
/// 変換に失敗した場合は false を返す(原実装 EncodeURL の戻り値に対応)。
pub trait KeywordEncoder {
    fn encode(&self, charset: KeywordCharset, keyword: &[u16], dst: &mut Vec<u16>) -> bool;
}

/// 検索 URL を組み立てる。原実装 Search:91-151 の URL 展開部分(ShellExecute 直前まで)。
///
/// URL テンプレート中の `{...}` を走査し、既知の `{keyword:...}` であれば encoder で
/// 展開、`{` 外の文字はそのまま連結する。
///
/// `{` に対応する `}` が無い場合は走査を打ち切り(原実装は break)、未処理の残り
/// (= 直前の処理位置から末尾まで。`{` 以降も含む)をそのまま追記する。
///
/// 以下の場合に None(原実装の return false)を返す:
///   - 未知のパラメータが現れた場合は失敗
///   - encoder が失敗した場合は失敗
///   - 結果が空の場合は失敗
pub fn build_search_url(
    encoder: &dyn KeywordEncoder,
    url: &[u16],
    keyword: &[u16],
) -> Option<Vec<u16>> {
    let lbrace = u16::from(b'{');
    let rbrace = u16::from(b'}');
    let mut buffer: Vec<u16> = Vec::new();
    let mut pos = 0usize;

    while pos < url.len() {
        let begin = find_from(url, lbrace, pos);
        let Some(begin) = begin else { break };
        let end = find_from(url, rbrace, begin + 1);
        let Some(end) = end else { break };

        if begin > pos {
            buffer.extend_from_slice(&url[pos..begin]);
        }

        let param = &url[begin + 1..end];
        match KeywordCharset::from_param(param) {
            Some(charset) => {
                if !encoder.encode(charset, keyword, &mut buffer) {
                    return None;
                }
            }
            None => {
                // 未知パラメータ。原実装 Search:132-135 で return false。
                return None;
            }
        }

        pos = end + 1;
    }

    if pos < url.len() {
        buffer.extend_from_slice(&url[pos..]);
    }

    if buffer.is_empty() {
        return None;
    }

    Some(buffer)
}

/// `haystack` の `from` 以降から最初に `needle` が現れる位置。
fn find_from(haystack: &[u16], needle: u16, from: usize) -> Option<usize> {
    if from > haystack.len() {
        return None;
    }
    haystack[from..]
        .iter()
        .position(|&c| c == needle)
        .map(|p| p + from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Vec<u16> {
        su::to_u16(s)
    }
    fn s(v: &[u16]) -> String {
        su::from_u16(v)
    }

    // キーワードを UTF-8 / Shift_JIS としてエンコードする簡易エンコーダ。
    // テスト用に Rust 標準の UTF-8 変換のみを使い(ASCII の範囲で原実装と一致)、
    // percent_encode_bytes を通す。
    struct Utf8Encoder;
    impl KeywordEncoder for Utf8Encoder {
        fn encode(&self, _charset: KeywordCharset, keyword: &[u16], dst: &mut Vec<u16>) -> bool {
            let kw = su::from_u16(keyword);
            let trimmed = kw.trim();
            if trimmed.is_empty() {
                return true;
            }
            percent_encode_bytes(trimmed.as_bytes(), dst);
            true
        }
    }

    #[test]
    fn test_percent_encode_bytes() {
        let mut out = Vec::new();
        percent_encode_bytes(b"abcXYZ", &mut out);
        assert_eq!(s(&out), "abcXYZ"); // 英字はそのまま。

        out.clear();
        percent_encode_bytes(b"a b", &mut out);
        assert_eq!(s(&out), "a+b"); // 空白 → '+'。

        out.clear();
        percent_encode_bytes(b"1+2=3", &mut out);
        // 数字・記号は %XX。'1'=0x31, '+'=0x2B, '2'=0x32, '='=0x3D, '3'=0x33。
        assert_eq!(s(&out), "%31%2B%32%3D%33");
    }

    #[test]
    fn test_percent_encode_bytes_digits() {
        // 数字は英字ではないため %XX。'1'=0x31 → "%31"。
        let mut out = Vec::new();
        percent_encode_bytes(b"12", &mut out);
        assert_eq!(s(&out), "%31%32");
    }

    #[test]
    fn test_keyword_charset_from_param() {
        assert_eq!(KeywordCharset::from_param(&w("keyword:utf-8")), Some(KeywordCharset::Utf8));
        assert_eq!(KeywordCharset::from_param(&w("KEYWORD:UTF-8")), Some(KeywordCharset::Utf8));
        assert_eq!(KeywordCharset::from_param(&w("keyword:shift_jis")), Some(KeywordCharset::ShiftJis));
        assert_eq!(KeywordCharset::from_param(&w("keyword:euc-jp")), Some(KeywordCharset::EucJp));
        assert_eq!(KeywordCharset::from_param(&w("keyword:iso-2022-jp")), Some(KeywordCharset::Iso2022Jp));
        assert_eq!(KeywordCharset::from_param(&w("unknown")), None);
    }

    #[test]
    fn test_search_engine_url_key() {
        // "Engine1.Name" → "Engine1.URL"。
        assert_eq!(s(&search_engine_url_key(&w("Engine1.Name")).unwrap()), "Engine1.URL");
        // 大小無視で "name" でも一致。
        assert_eq!(s(&search_engine_url_key(&w("E.name")).unwrap()), "E.URL");
        // '.' が無い → None。
        assert!(search_engine_url_key(&w("Name")).is_none());
        // '.' 以降が "Name" でない → None。
        assert!(search_engine_url_key(&w("Engine1.URL")).is_none());
        // 先頭が '.'(pos==0)→ None。
        assert!(search_engine_url_key(&w(".Name")).is_none());
    }

    #[test]
    fn test_build_search_url_basic() {
        let enc = Utf8Encoder;
        let url = build_search_url(
            &enc,
            &w("https://example.com/?q={keyword:utf-8}"),
            &w("rust"),
        )
        .unwrap();
        assert_eq!(s(&url), "https://example.com/?q=rust");
    }

    #[test]
    fn test_build_search_url_space_to_plus() {
        let enc = Utf8Encoder;
        let url = build_search_url(
            &enc,
            &w("https://example.com/?q={keyword:utf-8}&x=1"),
            &w("a b"),
        )
        .unwrap();
        assert_eq!(s(&url), "https://example.com/?q=a+b&x=1");
    }

    #[test]
    fn test_build_search_url_unknown_param_fails() {
        let enc = Utf8Encoder;
        // 未知パラメータ → None。
        assert!(build_search_url(&enc, &w("http://x/?q={unknown}"), &w("k")).is_none());
    }

    #[test]
    fn test_build_search_url_no_closing_brace_breaks() {
        let enc = Utf8Encoder;
        // '}' が無い場合、原実装はループを break し、Pos(='{' 前)から末尾までを
        // そのまま追記する。よって URL 全体がそのまま残る。
        let url = build_search_url(&enc, &w("http://x/?q={keyword:utf-8"), &w("k")).unwrap();
        assert_eq!(s(&url), "http://x/?q={keyword:utf-8");
    }

    #[test]
    fn test_build_search_url_no_placeholder() {
        let enc = Utf8Encoder;
        // プレースホルダ無しでも URL がそのまま残る(空でない)。
        let url = build_search_url(&enc, &w("http://x/fixed"), &w("k")).unwrap();
        assert_eq!(s(&url), "http://x/fixed");
    }

    #[test]
    fn test_build_search_url_empty_result_fails() {
        let enc = Utf8Encoder;
        // URL が空 → 結果も空 → None。
        assert!(build_search_url(&enc, &w(""), &w("k")).is_none());
        // 空キーワード(trim 後空)でプレースホルダのみ → 結果空 → None。
        assert!(build_search_url(&enc, &w("{keyword:utf-8}"), &w("   ")).is_none());
    }
}
