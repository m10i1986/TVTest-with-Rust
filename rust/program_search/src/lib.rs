// TVTest の ProgramSearch.cpp の純粋ロジックを Rust へ移植したもの。
//
// 移植対象(純粋ロジックのみ、GUI/CSettings/LibISDB/CRegExp は対象外):
//   - EventSearchServiceList : CEventSearchServiceList (ProgramSearch.h:42)
//     - get_service_key      : GetServiceKey(静的メソッド)
//     - encode_service_key   : EncodeServiceKey:158
//     - decode_service_key   : DecodeServiceKey:174
//     - to_string            : ToString:99
//     - from_str             : FromString:126
//   - EventSearchSettings    : CEventSearchSettings (ProgramSearch.h:89)
//     - to_string            : ToString:212
//     - from_str             : FromString:293
//     - parse_time           : ParseTime:386
//   - EventSearchSettingsList: CEventSearchSettingsList (ProgramSearch.h:162)
//     - Load/Save は CSettings 依存のため対象外
//
// ServiceKey は ULONGLONG(u64): NetworkID(32-47) | TSID(16-31) | ServiceID(0-15)。
// EncodeServiceKey は Base64 変種(6bit×8)エンコード。

use std::collections::BTreeSet;
use tvtest_string_utility as su;
use tvtest_util as util;

/// サービスキー(48bit: NID<<32|TSID<<16|SID)。原実装 ServiceKey (ProgramSearch.h:45)。
pub type ServiceKey = u64;

pub fn get_service_key(nid: u16, tsid: u16, sid: u16) -> ServiceKey {
    ((nid as u64) << 32) | ((tsid as u64) << 16) | (sid as u64)
}
pub fn service_key_nid(key: ServiceKey) -> u16 { (key >> 32) as u16 }
pub fn service_key_tsid(key: ServiceKey) -> u16 { ((key >> 16) & 0xFFFF) as u16 }
pub fn service_key_sid(key: ServiceKey) -> u16 { (key & 0xFFFF) as u16 }

const ENCODE_CHARS: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// サービスキーを Base64 変種で文字列へエンコード。原実装 EncodeServiceKey:158。
fn encode_service_key(key: ServiceKey) -> Vec<u16> {
    let mut buf: Vec<u16> = Vec::with_capacity(8);
    for i in 0..8 {
        let ch = ((key >> (42 - i * 6)) & 0x3F) as usize;
        if ch != 0 || !buf.is_empty() {
            buf.push(ENCODE_CHARS[ch] as u16);
        }
    }
    buf
}

/// Base64 変種文字列をサービスキーへデコード。原実装 DecodeServiceKey:174。
fn decode_service_key(text: &[u16]) -> Option<ServiceKey> {
    let mut key: ServiceKey = 0;
    for &c in text {
        let v: u64 = if c >= b'A' as u16 && c <= b'Z' as u16 {
            (c - b'A' as u16) as u64
        } else if c >= b'a' as u16 && c <= b'z' as u16 {
            (c - b'a' as u16 + 26) as u64
        } else if c >= b'0' as u16 && c <= b'9' as u16 {
            (c - b'0' as u16 + 52) as u64
        } else if c == b'+' as u16 {
            62
        } else if c == b'/' as u16 {
            63
        } else {
            return None;
        };
        key = (key << 6) | v;
    }
    Some(key)
}

/// イベント検索サービスリスト。原実装 CEventSearchServiceList (ProgramSearch.h:42)。
#[derive(Debug, Clone, Default)]
pub struct EventSearchServiceList {
    services: BTreeSet<ServiceKey>,
}

impl EventSearchServiceList {
    pub fn new() -> Self {
        EventSearchServiceList { services: BTreeSet::new() }
    }

    pub fn clear(&mut self) { self.services.clear(); }
    pub fn is_empty(&self) -> bool { self.services.is_empty() }
    pub fn get_service_count(&self) -> usize { self.services.len() }

    pub fn add(&mut self, key: ServiceKey) { self.services.insert(key); }
    pub fn add_ids(&mut self, nid: u16, tsid: u16, sid: u16) {
        self.services.insert(get_service_key(nid, tsid, sid));
    }

    pub fn is_exists(&self, key: ServiceKey) -> bool { self.services.contains(&key) }
    pub fn is_exists_ids(&self, nid: u16, tsid: u16, sid: u16) -> bool {
        self.services.contains(&get_service_key(nid, tsid, sid))
    }

    pub fn combine(&mut self, other: &EventSearchServiceList) {
        for &k in &other.services {
            self.services.insert(k);
        }
    }

    /// 原実装 ToString:99。
    /// サービスキーを':'区切りのBase64変種文字列に直列化。
    /// 前のキーと同じNIDなら NID部を省略、さらに同じNID==TSIDなら NID=TSID部も省略。
    pub fn to_string_u16(&self) -> Vec<u16> {
        let colon = b':' as u16;
        let mut out: Vec<u16> = Vec::new();
        let mut prev_key: ServiceKey = 0;

        for &key in &self.services {
            let mut encode_key = key;
            if prev_key != 0 && (prev_key >> 16) == (key >> 16) {
                encode_key &= 0xFFFF;
            } else if service_key_nid(key) == service_key_tsid(key) {
                encode_key &= 0xFFFF_FFFF;
            }
            out.extend(encode_service_key(encode_key));
            out.push(colon);
            prev_key = key;
        }
        out
    }

    /// 原実装 FromString:126。
    pub fn from_str_u16(&mut self, text: &[u16]) -> bool {
        let colon = b':' as u16;
        self.services.clear();
        let mut pos = 0;
        let mut prev_key: ServiceKey = 0;

        while pos < text.len() {
            let end = text[pos..]
                .iter()
                .position(|&c| c == colon)
                .map(|p| p + pos)
                .unwrap_or(text.len());
            let token = &text[pos..end];
            if !token.is_empty() {
                let mut key = match decode_service_key(token) {
                    Some(k) => k,
                    None => break,
                };
                if key <= 0xFFFF {
                    key |= prev_key & 0xFFFF_FFFF_0000;
                } else if service_key_nid(key) == 0 {
                    key |= (service_key_tsid(key) as u64) << 32;
                }
                self.services.insert(key);
                prev_key = key;
            }
            if end < text.len() && text[end] == colon {
                pos = end + 1;
            } else {
                break;
            }
        }
        true
    }
}

/// CA フィルタ種別。原実装 CEventSearchSettings::CAType (ProgramSearch.h:100)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CaType {
    #[default]
    Free = 0,
    Chargeable = 1,
}

impl CaType {
    fn from_u32(v: u32) -> Self {
        match v {
            1 => CaType::Chargeable,
            _ => CaType::Free,
        }
    }
}

/// 映像フィルタ種別。原実装 CEventSearchSettings::VideoType (ProgramSearch.h:105)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VideoFilterType {
    #[default]
    Hd = 0,
    Sd = 1,
}

impl VideoFilterType {
    fn from_u32(v: u32) -> Self {
        match v {
            1 => VideoFilterType::Sd,
            _ => VideoFilterType::Hd,
        }
    }
}

/// 時刻情報。原実装 CEventSearchSettings::TimeInfo (ProgramSearch.h:95)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TimeInfo {
    pub hour: i32,
    pub minute: i32,
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    struct ConditionFlag: u32 {
        const REG_EXP       = 0x00000001;
        const IGNORE_CASE   = 0x00000002;
        const IGNORE_WIDTH  = 0x00000004;
        const GENRE         = 0x00000008;
        const DAY_OF_WEEK   = 0x00000010;
        const TIME          = 0x00000020;
        const DURATION      = 0x00000040;
        const CA            = 0x00000080;
        const VIDEO         = 0x00000100;
        const SERVICE_LIST  = 0x00000200;
        const DISABLED      = 0x00000400;
        const EVENT_NAME    = 0x00000800;
        const EVENT_TEXT    = 0x00001000;
    }
}

/// イベント検索設定。原実装 CEventSearchSettings (ProgramSearch.h:89)。
#[derive(Debug, Clone)]
pub struct EventSearchSettings {
    pub disabled: bool,
    pub name: Vec<u16>,
    pub keyword: Vec<u16>,
    pub reg_exp: bool,
    pub ignore_case: bool,
    pub ignore_width: bool,
    pub event_name: bool,
    pub event_text: bool,
    pub genre: bool,
    pub genre1: u16,
    pub genre2: [u16; 16],
    pub day_of_week: bool,
    pub day_of_week_flags: u32,
    pub time: bool,
    pub start_time: TimeInfo,
    pub end_time: TimeInfo,
    pub duration: bool,
    pub duration_shortest: u32,
    pub duration_longest: u32,
    pub ca: bool,
    pub ca_type: CaType,
    pub video: bool,
    pub video_type: VideoFilterType,
    pub service_list_enabled: bool,
    pub service_list: EventSearchServiceList,
}

impl Default for EventSearchSettings {
    fn default() -> Self {
        EventSearchSettings {
            disabled: false,
            name: Vec::new(),
            keyword: Vec::new(),
            reg_exp: false,
            ignore_case: true,
            ignore_width: true,
            event_name: true,
            event_text: true,
            genre: false,
            genre1: 0,
            genre2: [0; 16],
            day_of_week: false,
            day_of_week_flags: 0,
            time: false,
            start_time: TimeInfo { hour: 0, minute: 0 },
            end_time: TimeInfo { hour: 23, minute: 59 },
            duration: false,
            duration_shortest: 10 * 60,
            duration_longest: 0,
            ca: false,
            ca_type: CaType::Free,
            video: false,
            video_type: VideoFilterType::Hd,
            service_list_enabled: false,
            service_list: EventSearchServiceList::new(),
        }
    }
}

impl EventSearchSettings {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn clear(&mut self) {
        *self = Default::default();
    }

    /// 原実装 ToString:212。カンマ区切りの UTF-16 文字列にシリアライズ。
    pub fn to_string_u16(&self) -> Vec<u16> {
        let comma = w(",");
        let colon = w(":");

        let mut out: Vec<u16> = Vec::new();
        // Name, Keyword: StringUtility::Encode でエスケープ。
        out.extend(su::encode(&self.name, &su::default_encode_chars()));
        out.extend_from_slice(&comma);
        out.extend(su::encode(&self.keyword, &su::default_encode_chars()));

        let mut flags = ConditionFlag::empty();
        if self.disabled { flags |= ConditionFlag::DISABLED; }
        if self.reg_exp { flags |= ConditionFlag::REG_EXP; }
        if self.ignore_case { flags |= ConditionFlag::IGNORE_CASE; }
        if self.ignore_width { flags |= ConditionFlag::IGNORE_WIDTH; }
        if self.event_name { flags |= ConditionFlag::EVENT_NAME; }
        if self.event_text { flags |= ConditionFlag::EVENT_TEXT; }
        if self.genre { flags |= ConditionFlag::GENRE; }
        if self.day_of_week { flags |= ConditionFlag::DAY_OF_WEEK; }
        if self.time { flags |= ConditionFlag::TIME; }
        if self.duration { flags |= ConditionFlag::DURATION; }
        if self.ca { flags |= ConditionFlag::CA; }
        if self.video { flags |= ConditionFlag::VIDEO; }
        if self.service_list_enabled { flags |= ConditionFlag::SERVICE_LIST; }

        // Genre2 (16×4 hex digits、全0なら空)。
        let genre2_str = if self.genre2.iter().any(|&v| v != 0) {
            self.genre2.iter().map(|&v| format!("{:04x}", v)).collect::<String>()
        } else {
            String::new()
        };

        let meta = format!(
            ",{},{},{},{},{}:{:02},{}:{:02},{},{},{},{}",
            flags.bits(),
            self.genre1,
            genre2_str,
            self.day_of_week_flags,
            self.start_time.hour, self.start_time.minute,
            self.end_time.hour, self.end_time.minute,
            self.duration_shortest,
            self.duration_longest,
            self.ca_type as u32,
            self.video_type as u32,
        );
        out.extend(meta.encode_utf16());

        if !self.service_list.is_empty() {
            out.extend_from_slice(&comma);
            out.extend(self.service_list.to_string_u16());
        }
        let _ = colon;
        out
    }

    /// 原実装 FromString:293。カンマ区切りの UTF-16 文字列からデシリアライズ。
    pub fn from_str_u16(&mut self, text: &[u16]) -> bool {
        let delim: Vec<u16> = ",".encode_utf16().collect();
        let fields = su::split(text, &delim);

        for (i, field) in fields.iter().enumerate() {
            match i {
                0 => self.name = su::decode(field),
                1 => self.keyword = su::decode(field),
                2 => {
                    let s = su::from_u16(field);
                    let v = u32::from_str_radix(s.trim(), 10).unwrap_or(0);
                    let flags = ConditionFlag::from_bits_truncate(v);
                    self.disabled = flags.contains(ConditionFlag::DISABLED);
                    self.reg_exp = flags.contains(ConditionFlag::REG_EXP);
                    self.ignore_case = flags.contains(ConditionFlag::IGNORE_CASE);
                    self.ignore_width = flags.contains(ConditionFlag::IGNORE_WIDTH);
                    self.event_name = flags.contains(ConditionFlag::EVENT_NAME);
                    self.event_text = flags.contains(ConditionFlag::EVENT_TEXT);
                    self.genre = flags.contains(ConditionFlag::GENRE);
                    self.day_of_week = flags.contains(ConditionFlag::DAY_OF_WEEK);
                    self.time = flags.contains(ConditionFlag::TIME);
                    self.duration = flags.contains(ConditionFlag::DURATION);
                    self.ca = flags.contains(ConditionFlag::CA);
                    self.video = flags.contains(ConditionFlag::VIDEO);
                    self.service_list_enabled = flags.contains(ConditionFlag::SERVICE_LIST);
                }
                3 => {
                    let s = su::from_u16(field);
                    self.genre1 = u32::from_str_radix(s.trim(), 10).unwrap_or(0) as u16;
                }
                4 => {
                    // 16×4 hex digits。
                    if field.len() >= 16 * 4 {
                        for j in 0..16 {
                            let slice = &field[j * 4..(j + 1) * 4];
                            let (v, _) = util::hex_string_to_uint(slice, 4);
                            self.genre2[j] = v as u16;
                        }
                    } else {
                        self.genre2 = [0; 16];
                    }
                }
                5 => {
                    let s = su::from_u16(field);
                    self.day_of_week_flags = u32::from_str_radix(s.trim(), 10).unwrap_or(0);
                }
                6 => self.start_time = parse_time(field),
                7 => self.end_time = parse_time(field),
                8 => {
                    let s = su::from_u16(field);
                    self.duration_shortest = u32::from_str_radix(s.trim(), 10).unwrap_or(0);
                }
                9 => {
                    let s = su::from_u16(field);
                    self.duration_longest = u32::from_str_radix(s.trim(), 10).unwrap_or(0);
                }
                10 => {
                    let s = su::from_u16(field);
                    self.ca_type = CaType::from_u32(u32::from_str_radix(s.trim(), 10).unwrap_or(0));
                }
                11 => {
                    let s = su::from_u16(field);
                    self.video_type = VideoFilterType::from_u32(u32::from_str_radix(s.trim(), 10).unwrap_or(0));
                }
                12 => {
                    self.service_list.from_str_u16(field);
                }
                _ => {}
            }
        }
        true
    }
}

/// 原実装 ParseTime:386。"H:MM" 形式 → TimeInfo。
fn parse_time(s: &[u16]) -> TimeInfo {
    let colon = b':' as u16;
    let cp = s.iter().position(|&c| c == colon);
    let (h_slice, m_slice) = if let Some(p) = cp {
        (&s[..p], &s[p + 1..])
    } else {
        (s, &s[s.len()..])
    };
    let hour: i32 = su::from_u16(h_slice).trim().parse().unwrap_or(0);
    let minute: i32 = su::from_u16(m_slice).trim().parse().unwrap_or(0);
    TimeInfo { hour, minute }
}

/// イベント検索設定リスト。原実装 CEventSearchSettingsList (ProgramSearch.h:162)。
/// Load/Save(CSettings 依存)は対象外。
#[derive(Debug, Clone, Default)]
pub struct EventSearchSettingsList {
    pub settings: Vec<EventSearchSettings>,
}

impl EventSearchSettingsList {
    pub fn new() -> Self { Default::default() }

    pub fn clear(&mut self) { self.settings.clear(); }
    pub fn get_count(&self) -> usize { self.settings.len() }
    pub fn get_enabled_count(&self) -> usize {
        self.settings.iter().filter(|s| !s.disabled).count()
    }
    pub fn get(&self, index: usize) -> Option<&EventSearchSettings> { self.settings.get(index) }
    pub fn get_mut(&mut self, index: usize) -> Option<&mut EventSearchSettings> { self.settings.get_mut(index) }

    /// 原実装 FindByName:501。lstrcmpi → ASCII 大小無視比較で近似。
    pub fn find_by_name(&self, name: &[u16]) -> Option<usize> {
        let lower: Vec<u16> = name.iter().map(|&c| ascii_to_lower(c)).collect();
        self.settings.iter().position(|s| {
            let sl: Vec<u16> = s.name.iter().map(|&c| ascii_to_lower(c)).collect();
            sl == lower
        })
    }

    pub fn get_by_name(&self, name: &[u16]) -> Option<&EventSearchSettings> {
        self.find_by_name(name).map(|i| &self.settings[i])
    }

    pub fn add(&mut self, s: EventSearchSettings) { self.settings.push(s); }

    pub fn erase(&mut self, index: usize) -> bool {
        if index >= self.settings.len() { return false; }
        self.settings.remove(index);
        true
    }
}

fn ascii_to_lower(c: u16) -> u16 {
    if c >= b'A' as u16 && c <= b'Z' as u16 { c + 32 } else { c }
}

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wu(s: &str) -> Vec<u16> { s.encode_utf16().collect() }
    fn su16(v: &[u16]) -> String { String::from_utf16_lossy(v).to_owned() }

    // ---- ServiceKey ----

    #[test]
    fn test_get_service_key_components() {
        let key = get_service_key(4, 101, 1024);
        assert_eq!(service_key_nid(key), 4);
        assert_eq!(service_key_tsid(key), 101);
        assert_eq!(service_key_sid(key), 1024);
    }

    #[test]
    fn test_encode_decode_service_key_roundtrip() {
        let key = get_service_key(4, 101, 1024);
        let encoded = encode_service_key(key);
        let decoded = decode_service_key(&encoded).unwrap();
        assert_eq!(decoded, key);
    }

    // ---- EventSearchServiceList ----

    #[test]
    fn test_service_list_add_exists() {
        let mut list = EventSearchServiceList::new();
        list.add_ids(4, 101, 1024);
        assert!(list.is_exists_ids(4, 101, 1024));
        assert!(!list.is_exists_ids(4, 101, 9999));
    }

    #[test]
    fn test_service_list_combine() {
        let mut a = EventSearchServiceList::new();
        let mut b = EventSearchServiceList::new();
        a.add_ids(1, 1, 1);
        b.add_ids(2, 2, 2);
        a.combine(&b);
        assert_eq!(a.get_service_count(), 2);
    }

    #[test]
    fn test_service_list_string_roundtrip() {
        let mut list = EventSearchServiceList::new();
        list.add_ids(4, 101, 1024);
        list.add_ids(4, 101, 2048);
        let s = list.to_string_u16();
        let mut list2 = EventSearchServiceList::new();
        list2.from_str_u16(&s);
        assert!(list2.is_exists_ids(4, 101, 1024));
        assert!(list2.is_exists_ids(4, 101, 2048));
    }

    // ---- EventSearchSettings ----

    #[test]
    fn test_search_settings_default() {
        let s = EventSearchSettings::new();
        assert!(!s.disabled);
        assert!(s.ignore_case);
        assert!(s.ignore_width);
        assert!(s.event_name);
        assert!(s.event_text);
        assert_eq!(s.end_time, TimeInfo { hour: 23, minute: 59 });
    }

    #[test]
    fn test_search_settings_to_from_string_basic() {
        let mut s = EventSearchSettings::new();
        s.name = wu("テスト検索");
        s.keyword = wu("NHK");
        s.ignore_case = true;
        s.event_name = true;

        let enc = s.to_string_u16();
        let mut s2 = EventSearchSettings::new();
        s2.from_str_u16(&enc);

        assert_eq!(su16(&s2.name), "テスト検索");
        assert_eq!(su16(&s2.keyword), "NHK");
        assert!(s2.ignore_case);
        assert!(s2.event_name);
    }

    #[test]
    fn test_search_settings_genre2_roundtrip() {
        let mut s = EventSearchSettings::new();
        s.genre = true;
        s.genre1 = 0x06;
        s.genre2[0] = 0x0001;
        s.genre2[1] = 0x0002;

        let enc = s.to_string_u16();
        let mut s2 = EventSearchSettings::new();
        s2.from_str_u16(&enc);

        assert!(s2.genre);
        assert_eq!(s2.genre1, 0x06);
        assert_eq!(s2.genre2[0], 0x0001);
        assert_eq!(s2.genre2[1], 0x0002);
    }

    #[test]
    fn test_search_settings_time_roundtrip() {
        let mut s = EventSearchSettings::new();
        s.time = true;
        s.start_time = TimeInfo { hour: 19, minute: 30 };
        s.end_time = TimeInfo { hour: 22, minute: 0 };

        let enc = s.to_string_u16();
        let mut s2 = EventSearchSettings::new();
        s2.from_str_u16(&enc);

        assert!(s2.time);
        assert_eq!(s2.start_time, TimeInfo { hour: 19, minute: 30 });
        assert_eq!(s2.end_time, TimeInfo { hour: 22, minute: 0 });
    }

    #[test]
    fn test_search_settings_service_list_roundtrip() {
        let mut s = EventSearchSettings::new();
        s.service_list_enabled = true;
        s.service_list.add_ids(4, 101, 1024);

        let enc = s.to_string_u16();
        let mut s2 = EventSearchSettings::new();
        s2.from_str_u16(&enc);

        assert!(s2.service_list_enabled);
        assert!(s2.service_list.is_exists_ids(4, 101, 1024));
    }

    // ---- EventSearchSettingsList ----

    #[test]
    fn test_settings_list_find_by_name() {
        let mut list = EventSearchSettingsList::new();
        let mut s = EventSearchSettings::new();
        s.name = wu("MySearch");
        list.add(s);

        assert!(list.find_by_name(&wu("mysearch")).is_some());
        assert!(list.find_by_name(&wu("MYSEARCH")).is_some());
        assert!(list.find_by_name(&wu("other")).is_none());
    }

    #[test]
    fn test_settings_list_erase() {
        let mut list = EventSearchSettingsList::new();
        list.add(EventSearchSettings::new());
        list.add(EventSearchSettings::new());
        assert!(list.erase(0));
        assert_eq!(list.get_count(), 1);
        assert!(!list.erase(5));
    }

    #[test]
    fn test_settings_list_enabled_count() {
        let mut list = EventSearchSettingsList::new();
        let mut s1 = EventSearchSettings::new();
        s1.disabled = true;
        list.add(s1);
        list.add(EventSearchSettings::new());
        assert_eq!(list.get_enabled_count(), 1);
    }
}
