// LibISDB の DateTime.cpp + ARIBTime.cpp を Rust へ移植したもの。
//
// 移植対象(DateTime.cpp):
//   - is_leap_year      : IsLeapYear:36
//   - get_day_of_year   : GetDayOfYear:42
//   - get_day_of_week   : GetDayOfWeek:62 (Zellerの公式)
//   - DateTime 構造体   : DateTime (DateTime.hpp:38)
//     - new/reset/is_valid/compare/diff_seconds/diff_milliseconds
//     - offset_seconds (純粋計算のみ; FILETIME/timegm 依存は除外)
//     - get_linear_seconds/get_linear_milliseconds
//     - from_linear_seconds/from_linear_milliseconds
//     - truncate_to_* 系
//     - set_day_of_week
//
// 移植対象(ARIBTime.cpp):
//   - get_bcd / make_bcd          : Utilities.hpp inline 関数
//   - load16_be                   : Load16 big-endian ロード
//   - parse_bcd_time              : ParseBCDTime:116
//   - make_bcd_time               : MakeBCDTime:128
//   - bcd_time_to_second          : BCDTimeToSecond:142
//   - bcd_time_hm_to_minute       : BCDTimeHMToMinute:158
//   - parse_mjd_time              : ParseMJDTime:63
//   - make_mjd_time               : MakeMJDTime:83
//   - mjd_bcd_to_datetime         : MJDBCDTimeToDateTime:38
//   - mjd_to_datetime             : MJDTimeToDateTime:97
//   - datetime_to_mjd             : DateTimeToMJDTime:109
//
// OffsetMilliseconds/GetLinearMilliseconds 等の FILETIME/SYSTEMTIME 依存部分は
// get_linear_seconds を u64(Unix 時間 UTC 秒相当)で実装。

/// うるう年判定。DateTime.cpp:36。
pub fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0) && ((year % 100 != 0) || (year % 400 == 0))
}

/// 年内通算日(0-based)。DateTime.cpp:42。
pub fn get_day_of_year(year: i32, month: i32, day: i32) -> Option<i32> {
    if !(1..=12).contains(&month) {
        return None;
    }
    const MONTH_DAYS: [i32; 11] = [31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let mut yd = day - 1;
    if month >= 2 {
        yd += MONTH_DAYS[(month - 2) as usize];
        if month >= 3 && is_leap_year(year) {
            yd += 1;
        }
    }
    Some(yd)
}

/// 曜日(0=日曜 ... 6=土曜)。DateTime.cpp:62 Zeller の公式。
pub fn get_day_of_week(year: i32, month: i32, day: i32) -> i32 {
    let (y, m) = if month <= 2 { (year - 1, month + 12) } else { (year, month) };
    ((y * 365 + y / 4 - y / 100 + y / 400 + 306 * (m + 1) / 10 + day - 428) % 7 + 7) % 7
}

/// 日時構造体。DateTime.hpp:38。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DateTime {
    pub year: i32,
    pub month: i32,
    pub day: i32,
    pub day_of_week: i32,
    pub hour: i32,
    pub minute: i32,
    pub second: i32,
    pub millisecond: i32,
}

impl DateTime {
    pub fn new() -> Self {
        DateTime::default()
    }

    /// フィールドをゼロリセット。DateTime.cpp:194。
    pub fn reset(&mut self) {
        *self = DateTime::default();
    }

    /// フィールドが有効範囲内か。DateTime.cpp:200。
    pub fn is_valid(&self) -> bool {
        self.year >= 1
            && (1..=12).contains(&self.month)
            && (1..=31).contains(&self.day)
            && (0..=6).contains(&self.day_of_week)
            && (0..=23).contains(&self.hour)
            && (0..=59).contains(&self.minute)
            && (0..=60).contains(&self.second)
            && (0..=999).contains(&self.millisecond)
    }

    /// 比較。負: self < other, 0: 等, 正: self > other。DateTime.cpp:213。
    pub fn compare(&self, other: &DateTime) -> i32 {
        let d = self.diff_milliseconds(other);
        if d < 0 { -1 } else if d > 0 { 1 } else { 0 }
    }

    /// 秒差(self - other)。DateTime.cpp:221。
    pub fn diff_seconds(&self, other: &DateTime) -> i64 {
        self.get_linear_seconds() as i64 - other.get_linear_seconds() as i64
    }

    /// ミリ秒差(self - other)。DateTime.cpp:227。
    pub fn diff_milliseconds(&self, other: &DateTime) -> i64 {
        self.get_linear_milliseconds() as i64 - other.get_linear_milliseconds() as i64
    }

    /// UTC 秒(1970-01-01 00:00:00 基点の線形秒)。DateTime.cpp:311 の非Windows 版相当。
    pub fn get_linear_seconds(&self) -> u64 {
        // 負の年や 1970 以前は未対応(EPG 用途では 2000 年代以降のみ使用)
        if self.year < 1970 {
            return 0;
        }
        // 年月日 → JDN (Julian Day Number)
        let y = self.year as i64;
        let m = self.month as i64;
        let d = self.day as i64;
        let (y2, m2) = if m <= 2 { (y - 1, m + 12) } else { (y, m) };
        let a = y2 / 100;
        let b = 2 - a + a / 4;
        let jdn = (365.25 * (y2 + 4716) as f64) as i64
            + (30.6001 * (m2 + 1) as f64) as i64
            + d + b - 1524;
        // 1970-01-01 の JDN = 2440588
        let days_since_epoch = jdn - 2440588;
        if days_since_epoch < 0 {
            return 0;
        }
        days_since_epoch as u64 * 86400
            + self.hour as u64 * 3600
            + self.minute as u64 * 60
            + self.second as u64
    }

    /// UTC ミリ秒。DateTime.cpp:331 の非Windows 版相当。
    pub fn get_linear_milliseconds(&self) -> u64 {
        self.get_linear_seconds() * 1000 + self.millisecond as u64
    }

    /// UTC 秒から DateTime を復元。DateTime.cpp:352 の非Windows 版相当。
    pub fn from_linear_seconds(seconds: u64) -> Self {
        // 1970-01-01 からの日数と時刻を計算
        let s_of_day = (seconds % 86400) as i32;
        let total_days = seconds / 86400;
        // JDN に変換して年月日を求める (1970-01-01 のJDN = 2440588)
        let g = total_days + 2440588;
        let (year, month, day) = days_to_ymd(g);
        let hour   = s_of_day / 3600;
        let minute = (s_of_day % 3600) / 60;
        let second = s_of_day % 60;
        let dow = get_day_of_week(year, month, day);
        DateTime { year, month, day, day_of_week: dow, hour, minute, second, millisecond: 0 }
    }

    /// UTC ミリ秒から DateTime を復元。DateTime.cpp:374 の非Windows 版相当。
    pub fn from_linear_milliseconds(ms: u64) -> Self {
        let mut dt = Self::from_linear_seconds(ms / 1000);
        dt.millisecond = (ms % 1000) as i32;
        dt
    }

    /// 秒オフセット。DateTime.cpp:239 の非Windows 版相当。
    pub fn offset_seconds(&self, seconds: i64) -> Option<DateTime> {
        let lin = self.get_linear_seconds() as i64 + seconds;
        if lin < 0 {
            return None;
        }
        Some(DateTime::from_linear_seconds(lin as u64))
    }

    /// ミリ秒オフセット。DateTime.cpp:266 の非Windows 版相当。
    pub fn offset_milliseconds(&self, ms: i64) -> Option<DateTime> {
        let lin = self.get_linear_milliseconds() as i64 + ms;
        if lin < 0 {
            return None;
        }
        Some(DateTime::from_linear_milliseconds(lin as u64))
    }

    /// 曜日を計算してセットする。DateTime.cpp:495。
    pub fn set_day_of_week(&mut self) {
        if self.is_valid() {
            self.day_of_week = get_day_of_week(self.year, self.month, self.day);
        }
    }

    pub fn truncate_to_seconds(&mut self) { self.millisecond = 0; }
    pub fn truncate_to_minutes(&mut self) { self.second = 0; self.millisecond = 0; }
    pub fn truncate_to_hours(&mut self)   { self.minute = 0; self.second = 0; self.millisecond = 0; }
    pub fn truncate_to_days(&mut self)    { self.hour = 0; self.minute = 0; self.second = 0; self.millisecond = 0; }
}

impl PartialOrd for DateTime {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DateTime {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.diff_milliseconds(other).cmp(&0)
    }
}

/// グレゴリオ暦通算日 → (year, month, day)
fn days_to_ymd(g: u64) -> (i32, i32, i32) {
    // アルゴリズム: http://www.tondering.dk/claus/cal/julperiod.php#formula
    let g = g as i64;
    let a = g + 32044;
    let b = (4 * a + 3) / 146097;
    let c = a - (146097 * b) / 4;
    let d = (4 * c + 3) / 1461;
    let e = c - (1461 * d) / 4;
    let m = (5 * e + 2) / 153;
    let day   = (e - (153 * m + 2) / 5 + 1) as i32;
    let month = (m + 3 - 12 * (m / 10)) as i32;
    let year  = (100 * b + d - 4800 + m / 10) as i32;
    (year, month, day)
}

// ─── ARIBTime ────────────────────────────────────────────────

/// BCD 1バイトを 10 進に変換。Utilities.hpp GetBCD:inline。
pub fn get_bcd(v: u8) -> u8 {
    (v >> 4) * 10 + (v & 0x0F)
}

/// 10進値を BCD 1バイトに変換。Utilities.hpp MakeBCD:inline。
pub fn make_bcd(v: u32) -> u8 {
    (((v / 10) << 4) | (v % 10)) as u8
}

/// ビッグエンディアン 2バイトロード。Utilities.hpp Load16:inline。
pub fn load16_be(data: &[u8]) -> u16 {
    ((data[0] as u16) << 8) | (data[1] as u16)
}

/// BCD 3バイトを時/分/秒に変換。ARIBTime.cpp:116。
pub fn parse_bcd_time(bcd: &[u8]) -> (i32, i32, i32) {
    let h = get_bcd(bcd[0]) as i32;
    let m = get_bcd(bcd[1]) as i32;
    let s = get_bcd(bcd[2]) as i32;
    (h, m, s)
}

/// 時/分/秒を BCD 3バイトに変換。ARIBTime.cpp:128。
pub fn make_bcd_time(hour: i32, minute: i32, second: i32) -> [u8; 3] {
    [make_bcd(hour as u32), make_bcd(minute as u32), make_bcd(second as u32)]
}

/// BCD 3バイトを秒数に変換。ARIBTime.cpp:142。
/// 全ビット FF の場合は未定義として 0 を返す。
pub fn bcd_time_to_second(bcd: &[u8]) -> u32 {
    if bcd[0] == 0xFF && bcd[1] == 0xFF && bcd[2] == 0xFF {
        return 0;
    }
    get_bcd(bcd[0]) as u32 * 3600
        + get_bcd(bcd[1]) as u32 * 60
        + get_bcd(bcd[2]) as u32
}

/// BCD 時分(2バイト)を分数に変換。ARIBTime.cpp:158。
pub fn bcd_time_hm_to_minute(bcd: u16) -> u16 {
    ((bcd >> 12) * 10 + ((bcd >> 8) & 0x0F)) * 60
        + ((bcd >> 4) & 0x0F) * 10
        + (bcd & 0x0F)
}

/// MJD を年/月/日/曜日に変換。ARIBTime.cpp:63。
pub fn parse_mjd_time(mjd: u16) -> (i32, i32, i32, i32) {
    let mjd = mjd as f64;
    let yd = ((mjd - 15078.2) / 365.25) as i32;
    let md = ((mjd - 14956.1 - (yd as f64 * 365.25) as i32 as f64) / 30.6001) as i32;
    let k = if md == 14 || md == 15 { 1 } else { 0 };
    let day   = mjd as i32 - 14956 - (yd as f64 * 365.25) as i32 - (md as f64 * 30.6001) as i32;
    let year  = yd + k + 1900;
    let month = md - 1 - k * 12;
    let dow   = (mjd as i32 + 3) % 7;
    (year, month, day, dow)
}

/// 年/月/日を MJD に変換。ARIBTime.cpp:83。
pub fn make_mjd_time(year: i32, month: i32, day: i32) -> u16 {
    let (y, m) = if month <= 2 { (year - 1, month + 12) } else { (year, month) };
    ((y as f64 * 365.25) as i32 + y / 400 - y / 100
        + (((m - 2) as f64) * 30.59) as i32
        + day - 678912) as u16
}

/// MJD+BCD 5バイトを DateTime に変換。ARIBTime.cpp:38。
/// 全ビット FF は未定義として None を返す。
pub fn mjd_bcd_to_datetime(data: &[u8]) -> Option<DateTime> {
    if data.len() < 5 {
        return None;
    }
    if data[0] == 0xFF && data[1] == 0xFF && data[2] == 0xFF && data[3] == 0xFF && data[4] == 0xFF {
        return None;
    }
    let mjd = load16_be(&data[0..2]);
    let (year, month, day, day_of_week) = parse_mjd_time(mjd);
    let (hour, minute, second) = parse_bcd_time(&data[2..5]);
    Some(DateTime { year, month, day, day_of_week, hour, minute, second, millisecond: 0 })
}

/// MJD を DateTime に変換(時刻ゼロ)。ARIBTime.cpp:97。
pub fn mjd_to_datetime(mjd: u16) -> DateTime {
    let (year, month, day, day_of_week) = parse_mjd_time(mjd);
    DateTime { year, month, day, day_of_week, hour: 0, minute: 0, second: 0, millisecond: 0 }
}

/// DateTime を MJD に変換。ARIBTime.cpp:109。
pub fn datetime_to_mjd(dt: &DateTime) -> u16 {
    make_mjd_time(dt.year, dt.month, dt.day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_leap_year() {
        assert!(is_leap_year(2000));
        assert!(is_leap_year(2004));
        assert!(!is_leap_year(1900));
        assert!(!is_leap_year(2001));
    }

    #[test]
    fn test_get_day_of_year() {
        assert_eq!(get_day_of_year(2020, 1, 1), Some(0));
        assert_eq!(get_day_of_year(2020, 3, 1), Some(60)); // うるう年
        assert_eq!(get_day_of_year(2019, 3, 1), Some(59)); // 平年
        assert_eq!(get_day_of_year(2020, 12, 31), Some(365));
    }

    #[test]
    fn test_get_day_of_week() {
        // 2023-01-01 は日曜 = 0
        assert_eq!(get_day_of_week(2023, 1, 1), 0);
        // 2023-01-02 は月曜 = 1
        assert_eq!(get_day_of_week(2023, 1, 2), 1);
        // 2024-02-29 はうるう日、木曜 = 4
        assert_eq!(get_day_of_week(2024, 2, 29), 4);
    }

    #[test]
    fn test_datetime_is_valid() {
        let dt = DateTime {
            year: 2023, month: 6, day: 15, day_of_week: 4,
            hour: 12, minute: 30, second: 0, millisecond: 0,
        };
        assert!(dt.is_valid());
    }

    #[test]
    fn test_datetime_is_valid_leap_day() {
        let dt = DateTime {
            year: 2024, month: 2, day: 29, day_of_week: 4,
            hour: 0, minute: 0, second: 0, millisecond: 0,
        };
        assert!(dt.is_valid()); // is_valid は日の範囲のみチェック(1-31)
    }

    #[test]
    fn test_datetime_invalid_month() {
        let dt = DateTime { year: 2023, month: 13, day: 1, ..Default::default() };
        assert!(!dt.is_valid());
    }

    #[test]
    fn test_get_linear_seconds_epoch() {
        let dt = DateTime {
            year: 1970, month: 1, day: 1, day_of_week: 4,
            hour: 0, minute: 0, second: 0, millisecond: 0,
        };
        assert_eq!(dt.get_linear_seconds(), 0);
    }

    #[test]
    fn test_get_linear_seconds_roundtrip() {
        let dt = DateTime {
            year: 2023, month: 6, day: 15, day_of_week: 4,
            hour: 12, minute: 30, second: 45, millisecond: 0,
        };
        let secs = dt.get_linear_seconds();
        let restored = DateTime::from_linear_seconds(secs);
        assert_eq!(restored.year,   dt.year);
        assert_eq!(restored.month,  dt.month);
        assert_eq!(restored.day,    dt.day);
        assert_eq!(restored.hour,   dt.hour);
        assert_eq!(restored.minute, dt.minute);
        assert_eq!(restored.second, dt.second);
    }

    #[test]
    fn test_offset_seconds() {
        let dt = DateTime {
            year: 2023, month: 12, day: 31, day_of_week: 0,
            hour: 23, minute: 59, second: 59, millisecond: 0,
        };
        let next = dt.offset_seconds(1).unwrap();
        assert_eq!(next.year, 2024);
        assert_eq!(next.month, 1);
        assert_eq!(next.day, 1);
        assert_eq!(next.hour, 0);
        assert_eq!(next.minute, 0);
        assert_eq!(next.second, 0);
    }

    #[test]
    fn test_diff_seconds() {
        let dt1 = DateTime { year: 2023, month: 1, day: 1, day_of_week: 0, hour: 1, minute: 0, second: 0, millisecond: 0 };
        let dt2 = DateTime { year: 2023, month: 1, day: 1, day_of_week: 0, hour: 0, minute: 0, second: 0, millisecond: 0 };
        assert_eq!(dt1.diff_seconds(&dt2), 3600);
    }

    #[test]
    fn test_compare_order() {
        let dt1 = DateTime { year: 2023, month: 1, day: 1, day_of_week: 0, hour: 1, ..Default::default() };
        let dt2 = DateTime { year: 2023, month: 1, day: 1, day_of_week: 0, hour: 0, ..Default::default() };
        assert!(dt1 > dt2);
        assert!(dt2 < dt1);
    }

    #[test]
    fn test_truncate_to_days() {
        let mut dt = DateTime { year: 2023, month: 6, day: 15, day_of_week: 4, hour: 12, minute: 30, second: 45, millisecond: 500 };
        dt.truncate_to_days();
        assert_eq!((dt.hour, dt.minute, dt.second, dt.millisecond), (0, 0, 0, 0));
    }

    #[test]
    fn test_truncate_to_hours() {
        let mut dt = DateTime { hour: 12, minute: 30, second: 45, millisecond: 500, ..Default::default() };
        dt.truncate_to_hours();
        assert_eq!((dt.minute, dt.second, dt.millisecond), (0, 0, 0));
        assert_eq!(dt.hour, 12);
    }

    #[test]
    fn test_set_day_of_week() {
        let mut dt = DateTime { year: 2023, month: 1, day: 1, day_of_week: 0, ..Default::default() };
        dt.set_day_of_week();
        assert_eq!(dt.day_of_week, 0); // 2023-01-01 は日曜
    }

    // ─── ARIBTime テスト ───────────────────────────────────────

    #[test]
    fn test_get_bcd() {
        assert_eq!(get_bcd(0x23), 23);
        assert_eq!(get_bcd(0x59), 59);
        assert_eq!(get_bcd(0x00), 0);
    }

    #[test]
    fn test_make_bcd() {
        assert_eq!(make_bcd(23), 0x23);
        assert_eq!(make_bcd(59), 0x59);
        assert_eq!(make_bcd(0), 0x00);
    }

    #[test]
    fn test_bcd_time_roundtrip() {
        let bcd = make_bcd_time(12, 34, 56);
        let (h, m, s) = parse_bcd_time(&bcd);
        assert_eq!((h, m, s), (12, 34, 56));
    }

    #[test]
    fn test_bcd_time_to_second() {
        // 01:02:03 = 3600+120+3 = 3723
        assert_eq!(bcd_time_to_second(&[0x01, 0x02, 0x03]), 3723);
    }

    #[test]
    fn test_bcd_time_to_second_undefined() {
        assert_eq!(bcd_time_to_second(&[0xFF, 0xFF, 0xFF]), 0);
    }

    #[test]
    fn test_bcd_time_hm_to_minute() {
        // 01:30 = 0x0130 → 90分
        assert_eq!(bcd_time_hm_to_minute(0x0130), 90);
        // 00:00 = 0
        assert_eq!(bcd_time_hm_to_minute(0x0000), 0);
    }

    #[test]
    fn test_parse_mjd_time_known() {
        // MJD 51544 = 2000-01-01 (土曜=6)
        let (y, m, d, dow) = parse_mjd_time(51544);
        assert_eq!(y, 2000);
        assert_eq!(m, 1);
        assert_eq!(d, 1);
        assert_eq!(dow, 6); // (51544+3)%7 = 51547%7 = 1? → 検証は下で
    }

    #[test]
    fn test_make_mjd_roundtrip() {
        // 適当な日付で往復変換
        let mjd = make_mjd_time(2023, 6, 15);
        let (y, m, d, _) = parse_mjd_time(mjd);
        assert_eq!((y, m, d), (2023, 6, 15));
    }

    #[test]
    fn test_mjd_bcd_to_datetime() {
        // 2023-06-15 12:34:56 の MJD+BCD を組み立て
        let mjd = make_mjd_time(2023, 6, 15);
        let bcd = make_bcd_time(12, 34, 56);
        let data = [
            (mjd >> 8) as u8, (mjd & 0xFF) as u8,
            bcd[0], bcd[1], bcd[2],
        ];
        let dt = mjd_bcd_to_datetime(&data).unwrap();
        assert_eq!((dt.year, dt.month, dt.day), (2023, 6, 15));
        assert_eq!((dt.hour, dt.minute, dt.second), (12, 34, 56));
    }

    #[test]
    fn test_mjd_bcd_undefined_returns_none() {
        let data = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
        assert!(mjd_bcd_to_datetime(&data).is_none());
    }

    #[test]
    fn test_datetime_to_mjd_roundtrip() {
        let dt = DateTime { year: 2023, month: 6, day: 15, ..Default::default() };
        let mjd = datetime_to_mjd(&dt);
        let (y, m, d, _) = parse_mjd_time(mjd);
        assert_eq!((y, m, d), (2023, 6, 15));
    }
}
