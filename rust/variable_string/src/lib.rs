// TVTest の VariableString.cpp / VariableString.h の純粋ロジックを Rust へ移植したもの。
//
// 原実装は LibISDB::DateTime や AppMain(グローバル変数管理)、CPopupMenu などの
// GUI/COM に密結合しているが、本クレートではプラットフォーム非依存で挙動を
// 厳密に検証できる以下の中核ロジックのみを移植する:
//   - FormatVariableString    : `%keyword%` 置換と `sep-*`(区切り)挿入アルゴリズム
//   - get_time_string         : 日時キーワード → 文字列(CVariableStringMap::GetTimeString)
//   - is_date_time_parameter  : 日時パラメータ判定(CVariableStringMap::IsDateTimeParameter)
//   - get_event_title         : 番組名から [字] 等のマークを除去(CEventVariableStringMap::GetEventTitle)
//   - get_event_mark          : 番組名から [字] 等のマークを抽出(CEventVariableStringMap::GetEventMark)
//   - normalize_file_name     : ファイル名禁止文字 → 全角(CEventVariableStringMap::NormalizeString)
//
// 文字列は原実装の wchar_t(UTF-16)に合わせ Vec<u16> / &[u16] ベースで扱う。

use tvtest_string_utility as su;

/// 日時を表す構造体。原実装の `LibISDB::DateTime` のうち、
/// get_time_string が参照するフィールドのみを保持する。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DateTime {
    pub year: i32,
    pub month: i32,
    pub day: i32,
    pub day_of_week: i32,
    pub hour: i32,
    pub minute: i32,
    pub second: i32,
}

/// 大小無視の ASCII 比較(原実装の `::lstrcmpiW` の ASCII 近似)。
/// キーワードはすべて ASCII 英数とハイフンのため、これで原実装と一致する。
fn eq_ignore_ascii_case_u16(a: &[u16], b: &str) -> bool {
    let bb: Vec<u16> = b.encode_utf16().collect();
    if a.len() != bb.len() {
        return false;
    }
    a.iter().zip(bb.iter()).all(|(&x, &y)| {
        let lx = if (b'A' as u16..=b'Z' as u16).contains(&x) { x + 32 } else { x };
        let ly = if (b'A' as u16..=b'Z' as u16).contains(&y) { y + 32 } else { y };
        lx == ly
    })
}

/// 先頭 n 文字を大小無視 ASCII 比較(原実装の `::StrCmpNI(keyword, prefix, n) == 0`)。
/// 一致すれば true。a の長さが n に満たない場合は不一致。
/// 原実装 GetLocalString の `start-`/`end-`/`tot-` プレフィックス判定で用いる。
/// 当該箇所(イベント情報依存)は未移植のため現時点では未使用だが、対応保持のため残す。
#[allow(dead_code)]
fn starts_with_ignore_ascii_case_u16(a: &[u16], prefix: &str) -> bool {
    let pp: Vec<u16> = prefix.encode_utf16().collect();
    if a.len() < pp.len() {
        return false;
    }
    eq_ignore_ascii_case_u16(&a[..pp.len()], prefix)
}

/// 日時パラメータかどうかを判定する。
/// 原実装 VariableString.cpp:299 CVariableStringMap::IsDateTimeParameter。
pub fn is_date_time_parameter(keyword: &[u16]) -> bool {
    const PARAMETER_LIST: [&str; 15] = [
        "date", "year", "year2", "month", "month2", "day", "day2", "time", "hour", "hour2",
        "minute", "minute2", "second", "second2", "day-of-week",
    ];
    PARAMETER_LIST
        .iter()
        .any(|&e| eq_ignore_ascii_case_u16(keyword, e))
}

/// 日時キーワードを文字列へ変換する。一致しないキーワードなら None。
/// 原実装 VariableString.cpp:259 CVariableStringMap::GetTimeString。
pub fn get_time_string(keyword: &[u16], time: &DateTime) -> Option<Vec<u16>> {
    let s: String = if eq_ignore_ascii_case_u16(keyword, "date") {
        format!("{}{:02}{:02}", time.year, time.month, time.day)
    } else if eq_ignore_ascii_case_u16(keyword, "time") {
        format!("{:02}{:02}{:02}", time.hour, time.minute, time.second)
    } else if eq_ignore_ascii_case_u16(keyword, "year") {
        format!("{}", time.year)
    } else if eq_ignore_ascii_case_u16(keyword, "year2") {
        format!("{:02}", time.year % 100)
    } else if eq_ignore_ascii_case_u16(keyword, "month") {
        format!("{}", time.month)
    } else if eq_ignore_ascii_case_u16(keyword, "month2") {
        format!("{:02}", time.month)
    } else if eq_ignore_ascii_case_u16(keyword, "day") {
        format!("{}", time.day)
    } else if eq_ignore_ascii_case_u16(keyword, "day2") {
        format!("{:02}", time.day)
    } else if eq_ignore_ascii_case_u16(keyword, "hour") {
        format!("{}", time.hour)
    } else if eq_ignore_ascii_case_u16(keyword, "hour2") {
        format!("{:02}", time.hour)
    } else if eq_ignore_ascii_case_u16(keyword, "minute") {
        format!("{}", time.minute)
    } else if eq_ignore_ascii_case_u16(keyword, "minute2") {
        format!("{:02}", time.minute)
    } else if eq_ignore_ascii_case_u16(keyword, "second") {
        format!("{}", time.second)
    } else if eq_ignore_ascii_case_u16(keyword, "second2") {
        format!("{:02}", time.second)
    } else if eq_ignore_ascii_case_u16(keyword, "day-of-week") {
        tvtest_util::get_day_of_week_text(time.day_of_week).to_string()
    } else {
        return None;
    };
    Some(su::to_u16(&s))
}

const MAX_MARK_LENGTH: usize = 3;

/// 番組名から `[字]` のようなマークを除去してタイトルを得る。
/// 原実装 VariableString.cpp:673 CEventVariableStringMap::GetEventTitle。
pub fn get_event_title(event_name: &[u16]) -> Vec<u16> {
    let left_bracket = u16::from(b'[');
    let right_bracket = u16::from(b']');
    let mut title: Vec<u16> = Vec::new();

    let mut next = 0usize;
    while next < event_name.len() {
        let left = find_from(event_name, left_bracket, next);
        let Some(left) = left else {
            title.extend_from_slice(&event_name[next..]);
            break;
        };

        let right = find_from(event_name, right_bracket, left + 1);
        let Some(right) = right else {
            title.extend_from_slice(&event_name[next..]);
            break;
        };

        if left > next {
            title.extend_from_slice(&event_name[next..left]);
        }

        // マーク長(括弧含む)が MAX_MARK_LENGTH + 2 を超えるものはマークではなく
        // 本文の一部とみなして残す。
        if right - left + 1 > MAX_MARK_LENGTH + 2 {
            title.extend_from_slice(&event_name[left..=right]);
        }

        next = right + 1;
    }

    // 前後の半角スペースを除去(原実装 StringUtility::Trim(*pTitle, L" "))。
    su::trim(&mut title, &su::to_u16(" "));
    title
}

/// 番組名から `[字]` のようなマークを抽出する。
/// 原実装 VariableString.cpp:707 CEventVariableStringMap::GetEventMark。
pub fn get_event_mark(event_name: &[u16]) -> Vec<u16> {
    let left_bracket = u16::from(b'[');
    let right_bracket = u16::from(b']');
    let mut marks: Vec<u16> = Vec::new();

    let mut next = 0usize;
    while next < event_name.len() {
        let Some(left) = find_from(event_name, left_bracket, next) else {
            break;
        };
        let Some(right) = find_from(event_name, right_bracket, left + 1) else {
            break;
        };

        let length = right - left + 1;
        if length > 2 && length <= MAX_MARK_LENGTH + 2 {
            marks.extend_from_slice(&event_name[left..=right]);
        }

        next = right + 1;
    }

    marks
}

/// `haystack` の `from` 以降から最初に `needle` が現れる位置(原実装の find)。
fn find_from(haystack: &[u16], needle: u16, from: usize) -> Option<usize> {
    if from > haystack.len() {
        return None;
    }
    haystack[from..]
        .iter()
        .position(|&c| c == needle)
        .map(|p| p + from)
}

/// ファイル名に使用できない文字を全角へ置き換える。
/// 原実装 VariableString.cpp:436 CEventVariableStringMap::NormalizeString。
pub fn normalize_file_name(s: &mut [u16]) {
    // (半角, 全角) の対応表。原実装の CharMap と同一。
    const CHAR_MAP: [(char, char); 9] = [
        ('\\', '￥'),
        ('/', '／'),
        (':', '：'),
        ('*', '＊'),
        ('?', '？'),
        ('"', '”'),
        ('<', '＜'),
        ('>', '＞'),
        ('|', '｜'),
    ];
    for e in s.iter_mut() {
        for &(from, to) in CHAR_MAP.iter() {
            if *e == from as u16 {
                *e = to as u16;
                break;
            }
        }
    }
}

/// 変数マップ。原実装の CVariableStringMap の純粋インターフェース部分を抽象化する。
/// GUI/AppMain 依存(グローバル変数取得・メニュー生成)は呼び出し側に委ねる。
pub trait VariableStringMap {
    /// フォーマット開始。原実装 BeginFormat。失敗時は false。
    fn begin_format(&mut self) -> bool {
        true
    }
    /// フォーマット終了。原実装 EndFormat。
    fn end_format(&mut self) {}
    /// キーワードに対応する文字列を返す。見つからなければ false。
    /// 原実装 GetString(純粋仮想)。
    fn get_string(&mut self, keyword: &[u16], out: &mut Vec<u16>) -> bool;
    /// 文字列を正規化(置換)する。原実装 NormalizeString。既定では何もしない。
    fn normalize_string(&self, _s: &mut Vec<u16>) -> bool {
        false
    }
}

/// 区切り文字(原実装 FormatVariableString の sep-hyphen / sep-slash / sep-backslash)。
const SEP_HYPHEN: &[u16] = &[b'-' as u16];
const SEP_SLASH: &[u16] = &[b'/' as u16];
const SEP_BACKSLASH: &[u16] = &[b'\\' as u16];

/// 区切り挿入位置の情報。原実装 FormatVariableString 内の SeparatorInfo。
struct SeparatorInfo {
    pos: usize,
    separator: &'static [u16],
}

/// フォーマット文字列を変数展開する。
/// 原実装 VariableString.cpp:33 FormatVariableString。
///
/// 返り値は処理成否(原実装の bool)。展開結果は out に格納する。
pub fn format_variable_string(
    map: &mut dyn VariableStringMap,
    format: &[u16],
    out: &mut Vec<u16>,
) -> bool {
    out.clear();

    if !map.begin_format() {
        return false;
    }

    let percent = u16::from(b'%');

    let mut separator_list: Vec<SeparatorInfo> = Vec::new();
    let mut i = 0usize;

    while i < format.len() {
        if format[i] == percent {
            i += 1;
            if i < format.len() && format[i] == percent {
                out.push(percent);
                i += 1;
            } else {
                // キーワードを '%' または終端まで読み取る。
                let mut keyword: Vec<u16> = Vec::new();
                while i < format.len() && format[i] != percent {
                    keyword.push(format[i]);
                    i += 1;
                }
                if i < format.len() && format[i] == percent {
                    i += 1; // 閉じ '%' を消費。
                    if eq_ignore_ascii_case_u16(&keyword, "sep-hyphen") {
                        separator_list.push(SeparatorInfo { pos: out.len(), separator: SEP_HYPHEN });
                    } else if eq_ignore_ascii_case_u16(&keyword, "sep-slash") {
                        separator_list.push(SeparatorInfo { pos: out.len(), separator: SEP_SLASH });
                    } else if eq_ignore_ascii_case_u16(&keyword, "sep-backslash") {
                        separator_list.push(SeparatorInfo { pos: out.len(), separator: SEP_BACKSLASH });
                    } else {
                        let mut text: Vec<u16> = Vec::new();
                        if map.get_string(&keyword, &mut text) {
                            map.normalize_string(&mut text);
                            out.extend_from_slice(&text);
                        } else {
                            // キーワード未解決: `%keyword%` をそのまま出力(キーワードは正規化)。
                            out.push(percent);
                            map.normalize_string(&mut keyword);
                            out.extend_from_slice(&keyword);
                            out.push(percent);
                        }
                    }
                } else {
                    // 閉じ '%' が無い: `%keyword` をそのまま出力(キーワードは正規化)。
                    out.push(percent);
                    map.normalize_string(&mut keyword);
                    out.extend_from_slice(&keyword);
                }
            }
        } else {
            out.push(format[i]);
            i += 1;
        }
    }

    if !separator_list.is_empty() {
        insert_separators(out, &separator_list);
    }

    map.end_format();

    true
}


/// 区切りを挿入する。原実装 FormatVariableString:94-112。
/// 前後にトークン(空白以外)が存在する場合のみ区切りを挿入する。
fn insert_separators(s: &mut Vec<u16>, separator_list: &[SeparatorInfo]) {
    let space = u16::from(b' ');
    let mut f_last = true;
    let n = separator_list.len() as isize;
    let mut idx = n - 1;
    while idx >= 0 {
        let i = idx as usize;
        let begin = if i > 0 { separator_list[i - 1].pos } else { 0 };
        let end = if i + 1 < separator_list.len() {
            separator_list[i + 1].pos
        } else {
            s.len()
        };
        let cur = separator_list[i].pos;

        // [begin, cur) に空白以外があれば fPrev、[cur, end) に空白以外があれば fNext。
        let f_prev = s[begin..cur].iter().any(|&c| c != space);
        let f_next = s[cur..end].iter().any(|&c| c != space);

        if (f_prev && f_next) || (f_prev && !f_last) {
            let sep = separator_list[i].separator;
            // pos の位置へ挿入。
            let pos = separator_list[i].pos;
            s.splice(pos..pos, sep.iter().copied());
        }
        if f_prev || f_next {
            f_last = false;
        }
        idx -= 1;
    }
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

    // get_string が固定マップを引くだけの単純な実装。
    struct MapStub {
        entries: Vec<(String, String)>,
    }
    impl VariableStringMap for MapStub {
        fn get_string(&mut self, keyword: &[u16], out: &mut Vec<u16>) -> bool {
            let k = su::from_u16(keyword).to_lowercase();
            for (key, val) in &self.entries {
                if key.to_lowercase() == k {
                    *out = w(val);
                    return true;
                }
            }
            false
        }
    }

    #[test]
    fn test_is_date_time_parameter() {
        assert!(is_date_time_parameter(&w("date")));
        assert!(is_date_time_parameter(&w("DATE")));
        assert!(is_date_time_parameter(&w("day-of-week")));
        assert!(!is_date_time_parameter(&w("foo")));
        assert!(!is_date_time_parameter(&w("date2")));
    }

    #[test]
    fn test_get_time_string() {
        let t = DateTime {
            year: 2026,
            month: 6,
            day: 5,
            day_of_week: 5, // 金
            hour: 9,
            minute: 7,
            second: 3,
        };
        assert_eq!(s(&get_time_string(&w("date"), &t).unwrap()), "20260605");
        assert_eq!(s(&get_time_string(&w("time"), &t).unwrap()), "090703");
        assert_eq!(s(&get_time_string(&w("year"), &t).unwrap()), "2026");
        assert_eq!(s(&get_time_string(&w("year2"), &t).unwrap()), "26");
        assert_eq!(s(&get_time_string(&w("month"), &t).unwrap()), "6");
        assert_eq!(s(&get_time_string(&w("month2"), &t).unwrap()), "06");
        assert_eq!(s(&get_time_string(&w("day"), &t).unwrap()), "5");
        assert_eq!(s(&get_time_string(&w("day2"), &t).unwrap()), "05");
        assert_eq!(s(&get_time_string(&w("hour2"), &t).unwrap()), "09");
        assert_eq!(s(&get_time_string(&w("day-of-week"), &t).unwrap()), "金");
        assert!(get_time_string(&w("foo"), &t).is_none());
    }

    #[test]
    fn test_get_event_title() {
        // [二][字] は除去、本文のみ残る。
        assert_eq!(s(&get_event_title(&w("[二][字]今日のニュース"))), "今日のニュース");
        // 長い [..] (MAX_MARK_LENGTH+2 = 5 を超える)は本文として残す。
        assert_eq!(s(&get_event_title(&w("[特別企画]番組"))), "[特別企画]番組");
        // 前後空白は除去。
        assert_eq!(s(&get_event_title(&w("[字] タイトル "))), "タイトル");
        // 括弧が閉じない場合は以降をそのまま。
        assert_eq!(s(&get_event_title(&w("番組[未閉じ"))), "番組[未閉じ");
    }

    #[test]
    fn test_get_event_mark() {
        // [二][字] を抽出。
        assert_eq!(s(&get_event_mark(&w("[二][字]今日のニュース"))), "[二][字]");
        // 長すぎる [..] はマークとみなさない。
        assert_eq!(s(&get_event_mark(&w("[特別企画]番組"))), "");
        // [] (長さ2)はマークではない(length > 2 が条件)。
        assert_eq!(s(&get_event_mark(&w("[]番組"))), "");
        // マーク無し。
        assert_eq!(s(&get_event_mark(&w("ただの番組"))), "");
    }

    #[test]
    fn test_normalize_file_name() {
        let mut v = w("a/b\\c:d*e?f\"g<h>i|j");
        normalize_file_name(&mut v);
        assert_eq!(s(&v), "a／b￥c：d＊e？f”g＜h＞i｜j");
    }

    #[test]
    fn test_format_plain_and_escape() {
        let mut map = MapStub { entries: vec![] };
        let mut out = Vec::new();
        assert!(format_variable_string(&mut map, &w("abc"), &mut out));
        assert_eq!(s(&out), "abc");

        // %% は % 1 文字に。
        out.clear();
        format_variable_string(&mut map, &w("100%% done"), &mut out);
        assert_eq!(s(&out), "100% done");
    }

    #[test]
    fn test_format_keyword_resolved_and_unresolved() {
        let mut map = MapStub {
            entries: vec![("event-name".into(), "ニュース".into())],
        };
        let mut out = Vec::new();
        format_variable_string(&mut map, &w("番組:%event-name%"), &mut out);
        assert_eq!(s(&out), "番組:ニュース");

        // 未解決キーワードは %keyword% のまま残る。
        out.clear();
        format_variable_string(&mut map, &w("x%unknown%y"), &mut out);
        assert_eq!(s(&out), "x%unknown%y");

        // 閉じ '%' が無い場合は %keyword をそのまま。
        out.clear();
        format_variable_string(&mut map, &w("x%unclosed"), &mut out);
        assert_eq!(s(&out), "x%unclosed");
    }

    #[test]
    fn test_format_separator_between_tokens() {
        let mut map = MapStub {
            entries: vec![("a".into(), "A".into()), ("b".into(), "B".into())],
        };
        // 前後にトークンがあれば区切りが入る。
        let mut out = Vec::new();
        format_variable_string(&mut map, &w("%a%%sep-hyphen%%b%"), &mut out);
        assert_eq!(s(&out), "A-B");
    }

    #[test]
    fn test_format_separator_skipped_when_no_token() {
        let mut map = MapStub {
            entries: vec![("a".into(), "A".into())],
        };
        // 後ろにトークンが無ければ区切りは入らない。
        let mut out = Vec::new();
        format_variable_string(&mut map, &w("%a%%sep-hyphen%"), &mut out);
        assert_eq!(s(&out), "A");

        // 前にトークンが無ければ区切りは入らない。
        out.clear();
        format_variable_string(&mut map, &w("%sep-hyphen%%a%"), &mut out);
        assert_eq!(s(&out), "A");
    }

    #[test]
    fn test_format_separator_empty_value_skips() {
        // 値が空(未解決でない空文字)の場合、前後トークンが無いと区切りは入らない。
        let mut map = MapStub {
            entries: vec![
                ("a".into(), "A".into()),
                ("empty".into(), "".into()),
                ("b".into(), "B".into()),
            ],
        };
        let mut out = Vec::new();
        // A - (空) のパターン: 後ろが空なので最初の sep は入らないが、
        // 連続する区切りの判定を確認する。
        format_variable_string(&mut map, &w("%a%%sep-hyphen%%empty%%sep-hyphen%%b%"), &mut out);
        // a と b の間に empty(空)。fLast 伝播により区切りが正しく処理される。
        assert_eq!(s(&out), "A-B");
    }
}
