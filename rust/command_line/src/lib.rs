// TVTest の CommandLine.cpp の純粋ロジックを Rust へ移植したもの。
//
// CommandLine.cpp の大部分は CArgsParser(CommandLineToArgvW 依存)・
// CCommandLineOptions::Parse(大量のフィールド設定)であり、GUI/Win32 に密結合する。
//
// 本クレートでは以下の純粋ロジックのみを移植する:
//   - parse_duration        : "?h?m?s" 形式の時間文字列 → 秒数(GetDurationValue)
//   - parse_ini_entry       : "[section]name=value" 形式 → IniEntry(GetIniEntry)
//   - parse_datetime        : "Y-M-DTh:m:s" 等の日時文字列 → SystemTime
//                             (GetValue(SYSTEMTIME*) の純粋部分。
//                              GetLocalTime 依存を「現在時刻を引数で受け取る」形に変換。
//                              OffsetSystemTime は tvtest_util::offset_system_time を使用)
//
// CommandLineToArgvW / ShellExecute 等の Win32 API は対象外。
//
// 文字列は原実装の wchar_t(UTF-16)に合わせ &[u16] / Vec<u16> ベースで扱う。

use tvtest_util::{self as util};

const SYSTEMTIME_SECOND: i64 = 1_000;
const SYSTEMTIME_MINUTE: i64 = 60 * SYSTEMTIME_SECOND;
const SYSTEMTIME_HOUR: i64 = 60 * SYSTEMTIME_MINUTE;

/// INI ファイルエントリ。原実装 CCommandLineOptions::IniEntry。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IniEntry {
    pub section: Vec<u16>,
    pub name: Vec<u16>,
    pub value: Vec<u16>,
}

/// "?h?m?s" 形式の時間文字列を秒数へ変換する。原実装 GetDurationValue:321。
///
/// 数字の後に 'h'/'H'(時)、'm'/'M'(分)、's'/'S'(秒)が続く形式。
/// 単位がない末尾の数字は秒扱い。負数も受け入れる。
/// 解析に失敗した場合(overflow 等)は None。
pub fn parse_duration(text: &[u16]) -> Option<i32> {
    if text.is_empty() {
        return None;
    }
    let mut duration_sec: i32 = 0;
    let mut current_num: i64 = 0;
    let mut has_digit = false;
    let mut negative = false;
    let mut i = 0;

    while i < text.len() {
        let c = text[i];
        if c == u16::from(b'-') {
            negative = true;
            i += 1;
        } else if c >= u16::from(b'0') && c <= u16::from(b'9') {
            current_num = current_num * 10 + (c - u16::from(b'0')) as i64;
            if current_num > i32::MAX as i64 {
                return None;
            }
            has_digit = true;
            i += 1;
        } else {
            let n = if negative { -current_num } else { current_num };
            match c {
                c if c == u16::from(b'h') || c == u16::from(b'H') => {
                    duration_sec = duration_sec.checked_add((n * 3600) as i32)?;
                }
                c if c == u16::from(b'm') || c == u16::from(b'M') => {
                    duration_sec = duration_sec.checked_add((n * 60) as i32)?;
                }
                c if c == u16::from(b's') || c == u16::from(b'S') => {
                    duration_sec = duration_sec.checked_add(n as i32)?;
                }
                _ => {}
            }
            current_num = 0;
            negative = false;
            has_digit = false;
            i += 1;
        }
    }
    // 末尾の数字(単位なし)は秒扱い。
    if has_digit {
        let n = if negative { -current_num } else { current_num };
        duration_sec = duration_sec.checked_add(n as i32)?;
    }

    Some(duration_sec)
}

/// "[section]name=value" 形式の文字列を IniEntry へパースする。原実装 GetIniEntry:361。
///
/// `[section]` 部分は省略可能。`=` が無いか name 部分が空なら None。
pub fn parse_ini_entry(text: &[u16]) -> Option<IniEntry> {
    let lbracket = u16::from(b'[');
    let rbracket = u16::from(b']');
    let equals = u16::from(b'=');

    let mut pos = 0usize;
    let mut section: Vec<u16> = Vec::new();

    if !text.is_empty() && text[0] == lbracket {
        pos += 1;
        let end = find_from(text, rbracket, pos)?;
        section = text[pos..end].to_vec();
        pos = end + 1;
    }

    let eq_pos = find_from(text, equals, pos)?;
    if eq_pos == pos {
        // name 部分が空。
        return None;
    }

    let name = text[pos..eq_pos].to_vec();
    let value = text[eq_pos + 1..].to_vec();

    Some(IniEntry { section, name, value })
}

fn find_from(haystack: &[u16], needle: u16, from: usize) -> Option<usize> {
    if from > haystack.len() {
        return None;
    }
    haystack[from..]
        .iter()
        .position(|&c| c == needle)
        .map(|p| p + from)
}

/// 日時文字列を `util::SystemTime` へパースする。原実装 GetValue(SYSTEMTIME*):209。
///
/// `current` は原実装の `GetLocalTime(&CurTime)` 相当(現在時刻)を呼び出し側が渡す。
/// 時刻補完(省略部分の現在値補填・月/年ラップアラウンド)も原実装どおり実行する。
/// 解析に失敗した場合は None。
///
/// 受け入れる形式の例:
///   "Y/M/D-h:m:s"、"Y-M-DTh:m:s"、"M/D-h:m"、"h:m"、"h:m:s" など。
///   Y・M(月)・s は省略可能。
///   日付と時刻の区切りは '/'、'-'、'T'。時刻の区切りは ':'。
pub fn parse_datetime(text: &[u16], current: &util::SystemTime) -> Option<util::SystemTime> {
    let slash = u16::from(b'/');
    let hyphen = u16::from(b'-');
    let big_t = u16::from(b'T');
    let colon = u16::from(b':');

    let mut date_parts: [u16; 3] = [0; 3];
    let mut time_parts: [u16; 3] = [0; 3];
    let mut date_count = 0usize;
    let mut time_count = 0usize;
    let mut value: u32 = 0;
    let mut i = 0;

    while i <= text.len() {
        let c = if i < text.len() { text[i] } else { 0 };
        if c >= u16::from(b'0') && c <= u16::from(b'9') {
            value = value * 10 + (c - u16::from(b'0')) as u32;
            if value > 0xFFFF {
                return None;
            }
        } else {
            if c == slash || c == hyphen || c == big_t {
                if date_count >= 3 {
                    return None;
                }
                date_parts[date_count] = value as u16;
                date_count += 1;
            } else if c == colon || c == 0 {
                if time_count >= 3 {
                    return None;
                }
                time_parts[time_count] = value as u16;
                time_count += 1;
                if c == 0 {
                    break;
                }
            }
            value = 0;
        }
        i += 1;
    }

    // 条件チェック: 日付無しで時刻が 1 つ、または時刻が 1 つだけ(h のみ)はNG。
    if (date_count == 0 && time_count < 2) || time_count == 1 {
        return None;
    }

    // 日付部分の解釈。
    let mut time = util::SystemTime::default();
    let mut idx = 0;
    if date_count > 2 {
        time.year = date_parts[idx];
        idx += 1;
        if time.year < 100 {
            time.year += (current.year / 100) * 100;
        }
    }
    if date_count > 1 {
        time.month = date_parts[idx];
        idx += 1;
        if time.month < 1 || time.month > 12 {
            return None;
        }
    }
    if date_count > 0 {
        time.day = date_parts[idx];
        if time.day < 1 || time.day > 31 {
            return None;
        }
    }

    // 省略された日付フィールドを現在時刻で補完。
    if time.year == 0 {
        time.year = current.year;
        if time.month == 0 {
            time.month = current.month;
            if time.day == 0 {
                time.day = current.day;
            } else if time.day < current.day {
                time.month += 1;
                if time.month > 12 {
                    time.month = 1;
                    time.year += 1;
                }
            }
        } else if time.month < current.month {
            time.year += 1;
        }
    }

    // 時刻部分の解釈。
    let hour = time_parts[0];
    let minute = if time_count > 1 { time_parts[1] } else { 0 };
    let second = if time_count > 2 { time_parts[2] } else { 0 };

    if time_count > 1 && minute > 59 {
        return None;
    }
    if time_count > 2 && second > 59 {
        return None;
    }

    // 時刻のみ指定の場合、現在より前なら翌日扱い。
    let hour_adjusted = if date_count == 0 && (hour as u16) < current.hour {
        (hour as i64) + 24
    } else {
        hour as i64
    };

    // offset_system_time で日付に時分秒を加算。
    util::offset_system_time(
        &mut time,
        hour_adjusted * SYSTEMTIME_HOUR
            + (minute as i64) * SYSTEMTIME_MINUTE
            + (second as i64) * SYSTEMTIME_SECOND,
    );

    Some(time)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn cur() -> util::SystemTime {
        util::SystemTime {
            year: 2026,
            month: 6,
            day: 15,
            day_of_week: 1,
            hour: 12,
            minute: 30,
            second: 0,
            milliseconds: 0,
        }
    }

    // ---- parse_duration ----

    #[test]
    fn test_parse_duration_seconds_only() {
        assert_eq!(parse_duration(&w("120")), Some(120));
        assert_eq!(parse_duration(&w("0")), Some(0));
    }

    #[test]
    fn test_parse_duration_hms() {
        assert_eq!(parse_duration(&w("1h30m5s")), Some(3600 + 1800 + 5));
        assert_eq!(parse_duration(&w("2h")), Some(7200));
        assert_eq!(parse_duration(&w("45m")), Some(2700));
    }

    #[test]
    fn test_parse_duration_negative() {
        assert_eq!(parse_duration(&w("-45")), Some(-45));
        assert_eq!(parse_duration(&w("-1h30m")), Some(-3600 + 1800)); // -1h + 30m
    }

    #[test]
    fn test_parse_duration_mixed() {
        // "1h30m5s" と末尾数字(秒)の組み合わせ。
        assert_eq!(parse_duration(&w("1h30m5")), Some(3600 + 1800 + 5));
    }

    #[test]
    fn test_parse_duration_empty() {
        assert_eq!(parse_duration(&w("")), None);
    }

    // ---- parse_ini_entry ----

    #[test]
    fn test_parse_ini_entry_with_section() {
        let entry = parse_ini_entry(&w("[Settings]Key=Value")).unwrap();
        assert_eq!(String::from_utf16_lossy(&entry.section), "Settings");
        assert_eq!(String::from_utf16_lossy(&entry.name), "Key");
        assert_eq!(String::from_utf16_lossy(&entry.value), "Value");
    }

    #[test]
    fn test_parse_ini_entry_without_section() {
        let entry = parse_ini_entry(&w("Key=Value")).unwrap();
        assert!(entry.section.is_empty());
        assert_eq!(String::from_utf16_lossy(&entry.name), "Key");
        assert_eq!(String::from_utf16_lossy(&entry.value), "Value");
    }

    #[test]
    fn test_parse_ini_entry_empty_value() {
        let entry = parse_ini_entry(&w("[S]Key=")).unwrap();
        assert_eq!(String::from_utf16_lossy(&entry.section), "S");
        assert_eq!(String::from_utf16_lossy(&entry.name), "Key");
        assert!(entry.value.is_empty());
    }

    #[test]
    fn test_parse_ini_entry_no_equals() {
        assert!(parse_ini_entry(&w("NoEquals")).is_none());
    }

    #[test]
    fn test_parse_ini_entry_empty_name() {
        // '[S]=value' → name が空 → None。
        assert!(parse_ini_entry(&w("[S]=value")).is_none());
    }

    // ---- parse_datetime ----

    #[test]
    fn test_parse_datetime_full_ymd_hms() {
        let t = parse_datetime(&w("2026/1/5-9:07:03"), &cur()).unwrap();
        assert_eq!(t.year, 2026);
        assert_eq!(t.month, 1);
        assert_eq!(t.day, 5);
        assert_eq!(t.hour, 9);
        assert_eq!(t.minute, 7);
        assert_eq!(t.second, 3);
    }

    #[test]
    fn test_parse_datetime_iso_format() {
        let t = parse_datetime(&w("2026-06-15T14:00:00"), &cur()).unwrap();
        assert_eq!(t.year, 2026);
        assert_eq!(t.month, 6);
        assert_eq!(t.day, 15);
        assert_eq!(t.hour, 14);
        assert_eq!(t.minute, 0);
        assert_eq!(t.second, 0);
    }

    #[test]
    fn test_parse_datetime_time_only_future() {
        // 時刻のみ、現在(12:30)より後の 14:00 → 同日。
        let t = parse_datetime(&w("14:00"), &cur()).unwrap();
        assert_eq!(t.year, 2026);
        assert_eq!(t.month, 6);
        assert_eq!(t.day, 15);
        assert_eq!(t.hour, 14);
        assert_eq!(t.minute, 0);
    }

    #[test]
    fn test_parse_datetime_time_only_past_wraps_next_day() {
        // 時刻のみ、現在(12:30)より前の 10:00 → 翌日扱い(時に+24)。
        let t = parse_datetime(&w("10:00"), &cur()).unwrap();
        // 2026/6/15 の 0:00 に 10+24=34時間加算 → 2026/6/16 10:00。
        assert_eq!(t.hour, 10);
        assert_eq!(t.day, 16);
    }

    #[test]
    fn test_parse_datetime_date_only_past_day_wraps_month() {
        // 日付(日のみ)が現在(15日)より前 → 月をインクリメント。
        // /5 → 5日、15日より前 → 7月5日へ。
        let t = parse_datetime(&w("5-0:00"), &cur()).unwrap();
        assert_eq!(t.month, 7);
        assert_eq!(t.day, 5);
    }

    #[test]
    fn test_parse_datetime_invalid_hour_only() {
        // 時刻が 1 つだけ("10")は NG(DateCount==0 && TimeCount==1)。
        // ただし "/" "-" "T" ":" の区切りなしで数字のみは date として解釈試みる。
        // "10" のみ → date_count=0, time_count=1 → None。
        assert!(parse_datetime(&w("10"), &cur()).is_none());
    }

    #[test]
    fn test_parse_datetime_invalid_minute() {
        assert!(parse_datetime(&w("10:60"), &cur()).is_none());
    }
}
