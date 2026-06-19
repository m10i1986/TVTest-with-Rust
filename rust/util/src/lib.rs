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

//! TVTest `Util` のうち、プラットフォーム非依存なヘルパ関数を Rust に移植したもの。
//!
//! 原実装 (`src/Util.cpp`)。GUI / クリップボード / レジストリ / OS バージョン判定など
//! Win32 API 依存の関数は対象外とし、純粋な計算(16進・色空間・時刻・暦)のみ移植する。
//!
//! `SYSTEMTIME` / `RECT` は Win32 の単純な値型なので、本クレートに同等の構造体を定義して
//! 等価な計算を再現する。

// ---------------------------------------------------------------------------
// 16 進数
// ---------------------------------------------------------------------------

/// 16進1文字を数値化。不正文字は 0。原実装 `HexCharToInt` (Util.cpp:33)。
pub fn hex_char_to_int(code: u16) -> i32 {
    match code {
        c if (b'0' as u16..=b'9' as u16).contains(&c) => (c - b'0' as u16) as i32,
        c if (b'A' as u16..=b'F' as u16).contains(&c) => (c - b'A' as u16) as i32 + 10,
        c if (b'a' as u16..=b'f' as u16).contains(&c) => (c - b'a' as u16) as i32 + 10,
        _ => 0,
    }
}

/// 最大 `length` 桁の16進文字列を数値化し、`(値, 消費した桁数)` を返す。
/// 原実装 `HexStringToUInt` (Util.cpp:45)。`ppszEnd` は消費桁数で表現する。
pub fn hex_string_to_uint(s: &[u16], length: usize) -> (u32, usize) {
    let mut value: u32 = 0;
    let mut i = 0;
    while i < length && i < s.len() {
        let code = s[i];
        let v = match code {
            c if (b'0' as u16..=b'9' as u16).contains(&c) => (c - b'0' as u16) as u32,
            c if (b'A' as u16..=b'F' as u16).contains(&c) => (c - b'A' as u16) as u32 + 10,
            c if (b'a' as u16..=b'f' as u16).contains(&c) => (c - b'a' as u16) as u32 + 10,
            _ => break,
        };
        value = (value << 4) | v;
        i += 1;
    }
    (value, i)
}

// ---------------------------------------------------------------------------
// 矩形
// ---------------------------------------------------------------------------

/// Win32 `RECT` 相当。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// 2つの矩形が交差するか。原実装 `IsRectIntersect` (Util.cpp:68)。
pub fn is_rect_intersect(r1: &Rect, r2: &Rect) -> bool {
    r1.left < r2.right && r1.right > r2.left && r1.top < r2.bottom && r1.bottom > r2.top
}

// ---------------------------------------------------------------------------
// 音量・色
// ---------------------------------------------------------------------------

/// レベル(0..=100)をデシベルへ変換。原実装 `LevelToDeciBel` (Util.cpp:75)。
pub fn level_to_decibel(level: i32) -> f32 {
    if level <= 0 {
        -100.0
    } else if level >= 100 {
        0.0
    } else {
        (20.0 * (level as f64 / 100.0).log10()) as f32
    }
}

/// COLORREF(0x00BBGGRR)から R/G/B を取り出すヘルパ。
fn get_r(c: u32) -> u32 {
    c & 0xFF
}
fn get_g(c: u32) -> u32 {
    (c >> 8) & 0xFF
}
fn get_b(c: u32) -> u32 {
    (c >> 16) & 0xFF
}
/// R/G/B から COLORREF を作る(Win32 `RGB` マクロ相当。下位8bitのみ使用)。
fn make_rgb(r: u32, g: u32, b: u32) -> u32 {
    (r & 0xFF) | ((g & 0xFF) << 8) | ((b & 0xFF) << 16)
}

/// 2色を `ratio`(0..=255)で混色。原実装 `MixColor` (Util.cpp:89)。
pub fn mix_color(color1: u32, color2: u32, ratio: u8) -> u32 {
    let ratio = ratio as u32;
    make_rgb(
        (get_r(color1) * ratio + get_r(color2) * (255 - ratio)) / 255,
        (get_g(color1) * ratio + get_g(color2) * (255 - ratio)) / 255,
        (get_b(color1) * ratio + get_b(color2) * (255 - ratio)) / 255,
    )
}

/// C の `static_cast<BYTE>` 相当(下位8bitへの切り捨て)。
fn to_byte(v: f64) -> u32 {
    // C++ の float→整数変換はゼロ方向への切り捨て、BYTE キャストは下位8bit。
    (v as i64 as u32) & 0xFF
}

/// HSV から RGB(COLORREF)へ変換。原実装 `HSVToRGB` (Util.cpp:98)。
///
/// Hue/Saturation/Value は 0.0..=1.0。
pub fn hsv_to_rgb(hue: f64, saturation: f64, value: f64) -> u32 {
    let (r, g, b);
    if saturation == 0.0 {
        r = value;
        g = value;
        b = value;
    } else {
        let mut h = hue * 6.0;
        if h >= 6.0 {
            h -= 6.0;
        }
        let s = saturation;
        let v = value;
        let f = h - (h as i32) as f64;
        let p = v * (1.0 - s);
        let q = v * (1.0 - s * f);
        let t = v * (1.0 - s * (1.0 - f));
        let (rr, gg, bb) = match h as i32 {
            0 => (v, t, p),
            1 => (q, v, p),
            2 => (p, v, t),
            3 => (p, q, v),
            4 => (t, p, v),
            5 => (v, p, q),
            // 原実装の switch は default を持たないが、h<6.0 が保証されるため到達しない。
            _ => (v, t, p),
        };
        r = rr;
        g = gg;
        b = bb;
    }
    make_rgb(
        to_byte(r * 255.0 + 0.5),
        to_byte(g * 255.0 + 0.5),
        to_byte(b * 255.0 + 0.5),
    )
}

/// RGB から HSV へ変換し `(hue, saturation, value)` を返す。原実装 `RGBToHSV` (Util.cpp:130)。
pub fn rgb_to_hsv(red: u8, green: u8, blue: u8) -> (f64, f64, f64) {
    let r = red as f64 / 255.0;
    let g = green as f64 / 255.0;
    let b = blue as f64 / 255.0;

    // 原実装の Max/Min 選択ロジックをそのまま再現。
    let (max, min) = if r > g {
        (r.max(b), g.min(b))
    } else {
        (g.max(b), r.min(b))
    };

    let v = max;
    let (h, s);
    if max > min {
        s = (max - min) / max;
        let delta = max - min;
        let mut hh = if r == max {
            (g - b) / delta
        } else if g == max {
            2.0 + (b - r) / delta
        } else {
            4.0 + (r - g) / delta
        };
        hh /= 6.0;
        if hh < 0.0 {
            hh += 1.0;
        } else if hh >= 1.0 {
            hh -= 1.0;
        }
        h = hh;
    } else {
        s = 0.0;
        h = 0.0;
    }

    (h, s, v)
}

// ---------------------------------------------------------------------------
// 日時(SYSTEMTIME)
// ---------------------------------------------------------------------------

/// Win32 `SYSTEMTIME` 相当。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SystemTime {
    pub year: u16,
    pub month: u16,
    pub day_of_week: u16,
    pub day: u16,
    pub hour: u16,
    pub minute: u16,
    pub second: u16,
    pub milliseconds: u16,
}

/// FILETIME の 1 ミリ秒(100ns 単位)。原実装 `FILETIME_MILLISECOND` (Util.h:56)。
pub const FILETIME_MILLISECOND: i64 = 10_000;

/// 2つの日時を比較。原実装 `CompareSystemTime` (Util.cpp:199)。
///
/// 原実装はビットパック比較で、`day_of_week` を含めない点に注意。
pub fn compare_system_time(t1: &SystemTime, t2: &SystemTime) -> i32 {
    let mut date1 =
        ((t1.year as u32) << 16) | ((t1.month as u32) << 8) | (t1.day as u32);
    let mut date2 =
        ((t2.year as u32) << 16) | ((t2.month as u32) << 8) | (t2.day as u32);
    if date1 == date2 {
        date1 = ((t1.hour as u32) << 24)
            | ((t1.minute as u32) << 16)
            | ((t1.second as u32) << 10)
            | (t1.milliseconds as u32);
        date2 = ((t2.hour as u32) << 24)
            | ((t2.minute as u32) << 16)
            | ((t2.second as u32) << 10)
            | (t2.milliseconds as u32);
    }
    if date1 < date2 {
        -1
    } else if date1 > date2 {
        1
    } else {
        0
    }
}

/// 暦日時(SYSTEMTIME)を FILETIME(1601-01-01 起点・100ns 単位)へ変換。
/// Win32 `SystemTimeToFileTime` のうち、有効な暦日に対する変換を等価実装。
fn system_time_to_filetime(t: &SystemTime) -> i64 {
    // 1601-01-01 を 0 日とする通日を求める。
    let days = days_from_civil(t.year as i64, t.month as i64, t.day as i64)
        - days_from_civil(1601, 1, 1);
    let secs = days * 86_400
        + (t.hour as i64) * 3600
        + (t.minute as i64) * 60
        + (t.second as i64);
    secs * 10_000_000 + (t.milliseconds as i64) * FILETIME_MILLISECOND
}

/// FILETIME を暦日時(SYSTEMTIME)へ変換。Win32 `FileTimeToSystemTime` 相当。
fn filetime_to_system_time(ft: i64) -> SystemTime {
    let total_ms = ft / FILETIME_MILLISECOND;
    let milliseconds = (total_ms % 1000) as u16;
    let total_secs = total_ms.div_euclid(1000);
    let secs_of_day = total_secs.rem_euclid(86_400);
    let days = total_secs.div_euclid(86_400) + days_from_civil(1601, 1, 1);

    let (year, month, day) = civil_from_days(days);
    let hour = (secs_of_day / 3600) as u16;
    let minute = ((secs_of_day % 3600) / 60) as u16;
    let second = (secs_of_day % 60) as u16;
    let dow = day_of_week_from_days(days);

    SystemTime {
        year: year as u16,
        month: month as u16,
        day_of_week: dow as u16,
        day: day as u16,
        hour,
        minute,
        second,
        milliseconds,
    }
}

/// グレゴリオ暦の (年,月,日) → 1970-01-01 を 0 とする通日(Howard Hinnant のアルゴリズム)。
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// 通日(1970-01-01 = 0)→ グレゴリオ暦 (年,月,日)。
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// 通日(1970-01-01 = 0, 木曜)から曜日(0=日)を求める。
fn day_of_week_from_days(z: i64) -> i64 {
    // 1970-01-01 は木曜(=4)。
    (z.rem_euclid(7) + 4).rem_euclid(7)
}

/// 日時にミリ秒オフセットを加算。原実装 `OffsetSystemTime` (Util.cpp:227)。
pub fn offset_system_time(t: &mut SystemTime, offset_ms: i64) {
    let ft = system_time_to_filetime(t) + offset_ms * FILETIME_MILLISECOND;
    *t = filetime_to_system_time(ft);
}

/// 2つの日時の差をミリ秒で返す。原実装 `DiffSystemTime` (Util.cpp:238)。
pub fn diff_system_time(start: &SystemTime, end: &SystemTime) -> i64 {
    (system_time_to_filetime(end) - system_time_to_filetime(start)) / FILETIME_MILLISECOND
}

/// 時分秒ミリ秒を 0 に切り詰める。原実装 `SystemTimeTruncateDay` (Util.cpp:257)。
pub fn system_time_truncate_day(t: &mut SystemTime) {
    t.hour = 0;
    t.minute = 0;
    t.second = 0;
    t.milliseconds = 0;
}

/// 分秒ミリ秒を 0 に切り詰める。原実装 `SystemTimeTruncateHour` (Util.cpp:266)。
pub fn system_time_truncate_hour(t: &mut SystemTime) {
    t.minute = 0;
    t.second = 0;
    t.milliseconds = 0;
}

/// 秒ミリ秒を 0 に切り詰める。原実装 `SystemTimeTruncateMinuite` (Util.cpp:274)。
pub fn system_time_truncate_minute(t: &mut SystemTime) {
    t.second = 0;
    t.milliseconds = 0;
}

/// ミリ秒を 0 に切り詰める。原実装 `SystemTimeTruncateSecond` (Util.cpp:281)。
pub fn system_time_truncate_second(t: &mut SystemTime) {
    t.milliseconds = 0;
}

// ---------------------------------------------------------------------------
// 暦
// ---------------------------------------------------------------------------

/// 曜日を計算(0=日, 1=月, ..., 6=土)。原実装 `CalcDayOfWeek` (Util.cpp:340)。
pub fn calc_day_of_week(year: i32, month: i32, day: i32) -> i32 {
    let (mut y, mut m) = (year, month);
    if m <= 2 {
        y -= 1;
        m += 12;
    }
    (y * 365 + y / 4 - y / 100 + y / 400 + 306 * (m + 1) / 10 + day - 428).rem_euclid(7)
}

/// 曜日番号(0=日)から日本語1文字を返す。範囲外は "？"。
/// 原実装 `GetDayOfWeekText` (Util.cpp:350)。
pub fn get_day_of_week_text(day_of_week: i32) -> &'static str {
    const NAMES: [&str; 7] = ["日", "月", "火", "水", "木", "金", "土"];
    if (0..=6).contains(&day_of_week) {
        NAMES[day_of_week as usize]
    } else {
        "？"
    }
}

// ---------------------------------------------------------------------------
// DIB(デバイス独立ビットマップ)のサイズ計算
//
// 原実装は BITMAPINFOHEADER ポインタを取るが(Image.cpp / Image.h)、計算はヘッダの
// フィールド値だけで完結する整数演算のため、本クレートを Win32 非依存に保つよう
// 必要な値を引数で受ける純粋関数として移植する。
// ---------------------------------------------------------------------------

/// DIB の 1 行のバイト数(4 バイト境界に切り上げ)。原実装 `DIB_ROW_BYTES`(Image.h:40)。
///
/// `((width * bit_count + 31) / 32) * 4`。
pub fn dib_row_bytes(width: i32, bit_count: u16) -> usize {
    (((width as i64) * (bit_count as i64) + 31) / 32 * 4) as usize
}

/// DIB の情報部(ヘッダ + カラーテーブル/ビットフィールド)のサイズ。
/// 原実装 `CalcDIBInfoSize`(Image.cpp:39)。
///
/// `bi_size` は `biSize`、`compression` は `biCompression`。8bpp 以下はパレット
/// (`2^bit_count` 個の `RGBQUAD`)、`BI_BITFIELDS`(=3)は 3 つの `DWORD` を加える。
pub fn calc_dib_info_size(bi_size: u32, bit_count: u16, compression: u32) -> usize {
    /// `BI_BITFIELDS`
    const BI_BITFIELDS: u32 = 3;
    let mut size = bi_size as usize;
    if bit_count <= 8 {
        // (1 << bit_count) 個の RGBQUAD(各 4 バイト)。
        size += (1usize << bit_count) * 4;
    } else if compression == BI_BITFIELDS {
        // 3 つの DWORD(各 4 バイト)。
        size += 3 * 4;
    }
    size
}

/// DIB のビット(ピクセル)部のサイズ。原実装 `CalcDIBBitsSize`(Image.cpp:52)。
///
/// `1 行のバイト数 * |height|`。
pub fn calc_dib_bits_size(width: i32, bit_count: u16, height: i32) -> usize {
    dib_row_bytes(width, bit_count) * height.unsigned_abs() as usize
}

/// DIB 全体(情報部 + ビット部)のサイズ。原実装 `CalcDIBSize`(Image.cpp:58)。
pub fn calc_dib_size(bi_size: u32, width: i32, bit_count: u16, height: i32, compression: u32) -> usize {
    calc_dib_info_size(bi_size, bit_count, compression) + calc_dib_bits_size(width, bit_count, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn test_hex_char_to_int() {
        assert_eq!(hex_char_to_int(b'0' as u16), 0);
        assert_eq!(hex_char_to_int(b'9' as u16), 9);
        assert_eq!(hex_char_to_int(b'A' as u16), 10);
        assert_eq!(hex_char_to_int(b'f' as u16), 15);
        assert_eq!(hex_char_to_int(b'g' as u16), 0); // 不正
    }

    #[test]
    fn test_hex_string_to_uint() {
        assert_eq!(hex_string_to_uint(&u("FF"), 4), (255, 2));
        assert_eq!(hex_string_to_uint(&u("1A2B"), 4), (0x1A2B, 4));
        // 不正文字で停止
        assert_eq!(hex_string_to_uint(&u("12XY"), 4), (0x12, 2));
        // 長さ制限
        assert_eq!(hex_string_to_uint(&u("FFFFF"), 2), (0xFF, 2));
    }

    #[test]
    fn test_is_rect_intersect() {
        let a = Rect { left: 0, top: 0, right: 10, bottom: 10 };
        let b = Rect { left: 5, top: 5, right: 15, bottom: 15 };
        let c = Rect { left: 20, top: 20, right: 30, bottom: 30 };
        assert!(is_rect_intersect(&a, &b));
        assert!(!is_rect_intersect(&a, &c));
    }

    #[test]
    fn test_level_to_decibel() {
        assert_eq!(level_to_decibel(0), -100.0);
        assert_eq!(level_to_decibel(-5), -100.0);
        assert_eq!(level_to_decibel(100), 0.0);
        assert_eq!(level_to_decibel(150), 0.0);
        // 50% ≒ -6.02 dB
        let db = level_to_decibel(50);
        assert!((db - (-6.0206)).abs() < 0.01);
    }

    #[test]
    fn test_mix_color() {
        let red = make_rgb(255, 0, 0);
        let blue = make_rgb(0, 0, 255);
        // ratio=255 で完全に color1
        assert_eq!(mix_color(red, blue, 255), red);
        // ratio=0 で完全に color2
        assert_eq!(mix_color(red, blue, 0), blue);
        // 中間
        let mid = mix_color(red, blue, 128);
        assert_eq!(get_r(mid), (255 * 128) / 255);
        assert_eq!(get_b(mid), (255 * 127) / 255);
    }

    #[test]
    fn test_hsv_rgb_roundtrip() {
        // 純色赤: H=0, S=1, V=1 → RGB(255,0,0)
        let c = hsv_to_rgb(0.0, 1.0, 1.0);
        assert_eq!(get_r(c), 255);
        assert_eq!(get_g(c), 0);
        assert_eq!(get_b(c), 0);

        // 無彩色(グレー)
        let c = hsv_to_rgb(0.0, 0.0, 0.5);
        assert_eq!(get_r(c), 128);
        assert_eq!(get_g(c), 128);
        assert_eq!(get_b(c), 128);

        // RGB→HSV→RGB のラウンドトリップ
        let (h, s, v) = rgb_to_hsv(120, 200, 80);
        let c = hsv_to_rgb(h, s, v);
        assert_eq!(get_r(c), 120);
        assert_eq!(get_g(c), 200);
        assert_eq!(get_b(c), 80);
    }

    #[test]
    fn test_compare_system_time() {
        let a = SystemTime { year: 2024, month: 1, day: 1, ..Default::default() };
        let b = SystemTime { year: 2024, month: 1, day: 2, ..Default::default() };
        assert_eq!(compare_system_time(&a, &b), -1);
        assert_eq!(compare_system_time(&b, &a), 1);
        assert_eq!(compare_system_time(&a, &a), 0);

        // 同日内の時刻比較
        let c = SystemTime { year: 2024, month: 1, day: 1, hour: 10, ..Default::default() };
        let d = SystemTime { year: 2024, month: 1, day: 1, hour: 11, ..Default::default() };
        assert_eq!(compare_system_time(&c, &d), -1);
    }

    #[test]
    fn test_filetime_roundtrip() {
        let t = SystemTime {
            year: 2024, month: 6, day: 15,
            hour: 13, minute: 30, second: 45, milliseconds: 123,
            day_of_week: 0,
        };
        let ft = system_time_to_filetime(&t);
        let back = filetime_to_system_time(ft);
        assert_eq!(back.year, 2024);
        assert_eq!(back.month, 6);
        assert_eq!(back.day, 15);
        assert_eq!(back.hour, 13);
        assert_eq!(back.minute, 30);
        assert_eq!(back.second, 45);
        assert_eq!(back.milliseconds, 123);
    }

    #[test]
    fn test_offset_and_diff_system_time() {
        let mut t = SystemTime {
            year: 2024, month: 1, day: 1,
            hour: 0, minute: 0, second: 0, milliseconds: 0,
            day_of_week: 0,
        };
        let original = t;
        // 1日 = 86,400,000 ms 進める
        offset_system_time(&mut t, 86_400_000);
        assert_eq!(t.day, 2);
        assert_eq!(diff_system_time(&original, &t), 86_400_000);

        // 月跨ぎ
        let mut t = SystemTime { year: 2024, month: 1, day: 31, ..Default::default() };
        offset_system_time(&mut t, 86_400_000);
        assert_eq!(t.month, 2);
        assert_eq!(t.day, 1);
    }

    #[test]
    fn test_system_time_truncate() {
        let base = SystemTime {
            year: 2024, month: 6, day: 15,
            hour: 13, minute: 30, second: 45, milliseconds: 123,
            day_of_week: 0,
        };
        let mut t = base;
        system_time_truncate_day(&mut t);
        assert_eq!((t.hour, t.minute, t.second, t.milliseconds), (0, 0, 0, 0));

        let mut t = base;
        system_time_truncate_hour(&mut t);
        assert_eq!((t.hour, t.minute, t.second, t.milliseconds), (13, 0, 0, 0));

        let mut t = base;
        system_time_truncate_minute(&mut t);
        assert_eq!((t.hour, t.minute, t.second, t.milliseconds), (13, 30, 0, 0));

        let mut t = base;
        system_time_truncate_second(&mut t);
        assert_eq!((t.hour, t.minute, t.second, t.milliseconds), (13, 30, 45, 0));
    }

    #[test]
    fn test_calc_day_of_week() {
        // 2024-06-15 は土曜(=6)
        assert_eq!(calc_day_of_week(2024, 6, 15), 6);
        // 2000-01-01 は土曜(=6)
        assert_eq!(calc_day_of_week(2000, 1, 1), 6);
        // 1970-01-01 は木曜(=4)
        assert_eq!(calc_day_of_week(1970, 1, 1), 4);
    }

    #[test]
    fn test_day_of_week_consistency() {
        // filetime 由来の曜日と calc_day_of_week が一致すること
        let t = SystemTime { year: 2024, month: 6, day: 15, ..Default::default() };
        let ft = system_time_to_filetime(&t);
        let back = filetime_to_system_time(ft);
        assert_eq!(back.day_of_week as i32, calc_day_of_week(2024, 6, 15));
    }

    #[test]
    fn test_get_day_of_week_text() {
        assert_eq!(get_day_of_week_text(0), "日");
        assert_eq!(get_day_of_week_text(6), "土");
        assert_eq!(get_day_of_week_text(7), "？");
        assert_eq!(get_day_of_week_text(-1), "？");
    }

    // ----- DIB サイズ計算 -----

    #[test]
    fn test_dib_row_bytes() {
        // 32bpp は width*4(既に 4 バイト境界)。
        assert_eq!(dib_row_bytes(10, 32), 40);
        // 24bpp は 4 バイト境界に切り上げ。width=10 → 30 → 32。
        assert_eq!(dib_row_bytes(10, 24), 32);
        // 1bpp width=1 → 1bit → 4 バイトに切り上げ。
        assert_eq!(dib_row_bytes(1, 1), 4);
        // 8bpp width=5 → 5 バイト → 8。
        assert_eq!(dib_row_bytes(5, 8), 8);
    }

    #[test]
    fn test_calc_dib_info_size() {
        const BITMAPINFOHEADER_SIZE: u32 = 40;
        // 32bpp(BI_RGB=0): カラーテーブルなし。
        assert_eq!(
            calc_dib_info_size(BITMAPINFOHEADER_SIZE, 32, 0),
            40
        );
        // 8bpp: 256 色 * 4 バイト = 1024 を加算。
        assert_eq!(
            calc_dib_info_size(BITMAPINFOHEADER_SIZE, 8, 0),
            40 + 256 * 4
        );
        // 1bpp: 2 色 * 4 = 8。
        assert_eq!(calc_dib_info_size(BITMAPINFOHEADER_SIZE, 1, 0), 40 + 8);
        // 16bpp + BI_BITFIELDS(3): 3 * DWORD = 12 を加算。
        assert_eq!(calc_dib_info_size(BITMAPINFOHEADER_SIZE, 16, 3), 40 + 12);
        // 16bpp + BI_RGB: 加算なし。
        assert_eq!(calc_dib_info_size(BITMAPINFOHEADER_SIZE, 16, 0), 40);
    }

    #[test]
    fn test_calc_dib_bits_size() {
        // 32bpp 10x8 = 40 * 8 = 320。
        assert_eq!(calc_dib_bits_size(10, 32, 8), 320);
        // 高さが負(トップダウン)でも絶対値。
        assert_eq!(calc_dib_bits_size(10, 32, -8), 320);
        // 24bpp 10 行 4 = 32 * 4 = 128。
        assert_eq!(calc_dib_bits_size(10, 24, 4), 128);
    }

    #[test]
    fn test_calc_dib_size() {
        const H: u32 = 40;
        // 32bpp 10x8: 情報 40 + ビット 320 = 360。
        assert_eq!(calc_dib_size(H, 10, 32, 8, 0), 360);
        // 8bpp 10x8: 情報(40+1024) + ビット(dib_row_bytes(10,8)=12 * 8 = 96) = 1160。
        assert_eq!(calc_dib_size(H, 10, 8, 8, 0), (40 + 1024) + 12 * 8);
    }
}
