// Rust port of LibISDB/EPG/EventInfo.cpp + EventInfo.hpp
// EventInfo.cpp:37, EventInfo.hpp:39

use libisdb_datetime::DateTime;

// EventInfo.hpp:121
bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
    pub struct TypeFlag: u32 {
        const Basic     = 0x0001;
        const Extended  = 0x0002;
        const Present   = 0x0004;
        const Following = 0x0008;
        const Database  = 0x0010;
    }
}

// EventInfo.hpp:42
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ExtendedTextInfo {
    pub description: String,
    pub text: String,
}

// EventInfo.hpp:49
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct VideoInfo {
    pub stream_content: u8,
    pub component_type: u8,
    pub component_tag: u8,
    pub language_code: u32,
    pub text: String,
}

// EventInfo.hpp:59
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct AudioInfo {
    pub stream_content: u8,
    pub component_type: u8,
    pub component_tag: u8,
    pub simulcast_group_tag: u8,
    pub es_multi_lingual_flag: bool,
    pub main_component_flag: bool,
    pub quality_indicator: u8,
    pub sampling_rate: u8,
    pub language_code: u32,
    pub language_code2: u32,
    pub text: String,
}

// EventInfo.hpp:75 — NibbleInfo: (level1, level2) pair
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ContentNibble {
    pub content_nibble_level1: u8,
    pub content_nibble_level2: u8,
    pub user_nibble1: u8,
    pub user_nibble2: u8,
}

// EventInfo.hpp:75
#[derive(Clone, Debug, Default)]
pub struct ContentNibbleInfo {
    pub nibble_list: Vec<ContentNibble>,
}

impl PartialEq for ContentNibbleInfo {
    fn eq(&self, other: &Self) -> bool {
        self.nibble_list == other.nibble_list
    }
}
impl Eq for ContentNibbleInfo {}

// EventInfo.hpp:92 — event group
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct EventGroupItem {
    pub service_id: u16,
    pub event_id: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct EventGroupInfo {
    pub group_type: u8,
    pub event_list: Vec<EventGroupItem>,
}

// EventInfo.hpp:99
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct CommonEventInfo {
    pub service_id: u16,
    pub event_id: u16,
}

// EventInfo.hpp:106
#[derive(Clone, Debug, Default)]
pub struct SeriesInfo {
    pub series_id: u16,
    pub repeat_label: u8,
    pub program_pattern: u8,
    pub expire_date: DateTime,
    pub episode_number: u16,
    pub last_episode_number: u16,
    pub series_name: String,
}

// EventInfo.hpp:39 — main EventInfo struct
#[derive(Clone, Debug, Default)]
pub struct EventInfo {
    pub network_id: u16,
    pub transport_stream_id: u16,
    pub service_id: u16,
    pub event_id: u16,
    pub start_time: DateTime,
    pub duration: u32,
    pub running_status: u8,
    pub free_ca_mode: bool,
    pub event_name: String,
    pub event_text: String,
    pub extended_text: Vec<ExtendedTextInfo>,
    pub video_list: Vec<VideoInfo>,
    pub audio_list: Vec<AudioInfo>,
    pub content_nibble: ContentNibbleInfo,
    pub event_group_list: Vec<EventGroupInfo>,
    pub is_common_event: bool,
    pub common_event: CommonEventInfo,
    pub type_flag: TypeFlag,
    pub updated_time: u64,
    pub source_id: u32,
}

impl PartialEq for EventInfo {
    fn eq(&self, other: &Self) -> bool {
        self.is_equal(other)
            && self.type_flag == other.type_flag
            && self.updated_time == other.updated_time
            && self.source_id == other.source_id
    }
}
impl Eq for EventInfo {}

impl EventInfo {
    pub fn new() -> Self { Self::default() }

    // EventInfo.cpp:46
    pub fn is_equal(&self, other: &Self) -> bool {
        self.network_id == other.network_id
            && self.transport_stream_id == other.transport_stream_id
            && self.service_id == other.service_id
            && self.event_id == other.event_id
            && self.start_time.is_valid() == other.start_time.is_valid()
            && (!self.start_time.is_valid() || (self.start_time == other.start_time))
            && self.duration == other.duration
            && self.running_status == other.running_status
            && self.free_ca_mode == other.free_ca_mode
            && self.event_name == other.event_name
            && self.event_text == other.event_text
            && self.extended_text == other.extended_text
            && self.video_list == other.video_list
            && self.audio_list == other.audio_list
            && self.content_nibble == other.content_nibble
            && self.event_group_list == other.event_group_list
            && self.is_common_event == other.is_common_event
            && (!self.is_common_event || (self.common_event == other.common_event))
    }

    // EventInfo.cpp:69
    pub fn has_basic(&self) -> bool { self.type_flag.contains(TypeFlag::Basic) }
    pub fn has_extended(&self) -> bool { self.type_flag.contains(TypeFlag::Extended) }
    pub fn is_present(&self) -> bool { self.type_flag.contains(TypeFlag::Present) }
    pub fn is_following(&self) -> bool { self.type_flag.contains(TypeFlag::Following) }
    pub fn is_present_following(&self) -> bool {
        self.type_flag.intersects(TypeFlag::Present | TypeFlag::Following)
    }
    pub fn is_database(&self) -> bool { self.type_flag.contains(TypeFlag::Database) }

    // EventInfo.cpp:105
    pub fn get_start_time(&self) -> Option<&DateTime> {
        if self.start_time.is_valid() { Some(&self.start_time) } else { None }
    }

    // EventInfo.cpp:116
    pub fn get_end_time(&self) -> Option<DateTime> {
        if self.start_time.is_valid() {
            self.start_time.offset_seconds(self.duration as i64)
        } else {
            None
        }
    }

    // EventInfo.cpp:133 — EPG time (UTC+9) → UTC
    pub fn get_start_time_utc(&self) -> Option<DateTime> {
        if self.start_time.is_valid() {
            epg_time_to_utc_time(&self.start_time)
        } else {
            None
        }
    }

    // EventInfo.cpp:149 — end time in UTC
    pub fn get_end_time_utc(&self) -> Option<DateTime> {
        if self.start_time.is_valid() {
            let offset = -9i64 * 3600 + self.duration as i64;
            self.start_time.offset_seconds(offset)
        } else {
            None
        }
    }

    // EventInfo.cpp:198 — concatenate extended text items with newlines
    pub fn get_concatenated_extended_text(&self) -> String {
        let mut result = String::new();
        let len = self.extended_text.len();
        for (i, item) in self.extended_text.iter().enumerate() {
            if !item.description.is_empty() {
                result.push_str(&item.description);
                result.push('\n');
            }
            if !item.text.is_empty() {
                result.push_str(&item.text);
                if i + 1 < len {
                    result.push('\n');
                }
            }
        }
        result
    }

    // EventInfo.cpp:244
    pub fn get_main_audio_index(&self) -> Option<usize> {
        self.audio_list.iter().position(|a| a.main_component_flag)
    }

    // EventInfo.cpp:254
    pub fn get_main_audio_info(&self) -> Option<&AudioInfo> {
        if self.audio_list.is_empty() {
            return None;
        }
        if let Some(idx) = self.get_main_audio_index() {
            return Some(&self.audio_list[idx]);
        }
        Some(&self.audio_list[0])
    }
}

// EventInfo.cpp:270 — EPG time (UTC+9) → UTC (-9h offset)
pub fn epg_time_to_utc_time(epg_time: &DateTime) -> Option<DateTime> {
    epg_time.offset_seconds(-9 * 3600)
}

// EventInfo.cpp:287 — UTC → EPG time (UTC+9) (+9h offset)
pub fn utc_time_to_epg_time(utc_time: &DateTime) -> Option<DateTime> {
    utc_time.offset_seconds(9 * 3600)
}

#[cfg(test)]
mod tests {
    use super::*;
    use libisdb_datetime::{DateTime, get_day_of_week};

    fn make_datetime(year: i32, month: i32, day: i32, h: i32, m: i32, s: i32) -> DateTime {
        let dow = get_day_of_week(year, month, day);
        DateTime { year, month, day, day_of_week: dow, hour: h, minute: m, second: s, millisecond: 0 }
    }

    #[test]
    fn test_type_flag_basic() {
        let flags = TypeFlag::Basic;
        assert!(flags.contains(TypeFlag::Basic));
        assert!(!flags.contains(TypeFlag::Extended));
    }

    #[test]
    fn test_type_flag_combined() {
        let flags = TypeFlag::Present | TypeFlag::Following;
        assert!(flags.intersects(TypeFlag::Present | TypeFlag::Following));
        assert!(!flags.contains(TypeFlag::Basic));
    }

    #[test]
    fn test_event_info_default() {
        let e = EventInfo::new();
        assert!(!e.start_time.is_valid());
        assert_eq!(e.duration, 0);
        assert!(e.event_name.is_empty());
    }

    #[test]
    fn test_has_basic() {
        let mut e = EventInfo::new();
        e.type_flag = TypeFlag::Basic;
        assert!(e.has_basic());
        assert!(!e.has_extended());
    }

    #[test]
    fn test_is_present_following() {
        let mut e = EventInfo::new();
        e.type_flag = TypeFlag::Present;
        assert!(e.is_present_following());
        e.type_flag = TypeFlag::Following;
        assert!(e.is_present_following());
        e.type_flag = TypeFlag::Database;
        assert!(!e.is_present_following());
    }

    #[test]
    fn test_get_start_time_none_when_invalid() {
        let e = EventInfo::new();
        assert!(e.get_start_time().is_none());
    }

    #[test]
    fn test_get_start_time_valid() {
        let mut e = EventInfo::new();
        e.start_time = make_datetime(2024, 4, 1, 12, 0, 0);
        assert!(e.get_start_time().is_some());
    }

    #[test]
    fn test_get_end_time_basic() {
        let mut e = EventInfo::new();
        e.start_time = make_datetime(2024, 4, 1, 12, 0, 0);
        e.duration = 3600; // 1 hour
        let end = e.get_end_time().unwrap();
        assert_eq!(end.hour, 13);
        assert_eq!(end.minute, 0);
    }

    #[test]
    fn test_get_end_time_none_when_invalid() {
        let e = EventInfo::new();
        assert!(e.get_end_time().is_none());
    }

    #[test]
    fn test_epg_time_to_utc_time() {
        // EPG time = UTC+9: 2024-04-01 12:00 JST → 2024-04-01 03:00 UTC
        let epg = make_datetime(2024, 4, 1, 12, 0, 0);
        let utc = epg_time_to_utc_time(&epg).unwrap();
        assert_eq!(utc.hour, 3);
        assert_eq!(utc.day, 1);
    }

    #[test]
    fn test_epg_time_to_utc_time_day_boundary() {
        // 2024-04-01 08:00 JST → 2024-03-31 23:00 UTC
        let epg = make_datetime(2024, 4, 1, 8, 0, 0);
        let utc = epg_time_to_utc_time(&epg).unwrap();
        assert_eq!(utc.day, 31);
        assert_eq!(utc.month, 3);
        assert_eq!(utc.hour, 23);
    }

    #[test]
    fn test_utc_time_to_epg_time() {
        // UTC 2024-04-01 03:00 → EPG 2024-04-01 12:00
        let utc = make_datetime(2024, 4, 1, 3, 0, 0);
        let epg = utc_time_to_epg_time(&utc).unwrap();
        assert_eq!(epg.hour, 12);
        assert_eq!(epg.day, 1);
    }

    #[test]
    fn test_utc_to_epg_day_wrap() {
        // UTC 2024-03-31 23:00 → EPG 2024-04-01 08:00
        let utc = make_datetime(2024, 3, 31, 23, 0, 0);
        let epg = utc_time_to_epg_time(&utc).unwrap();
        assert_eq!(epg.month, 4);
        assert_eq!(epg.day, 1);
        assert_eq!(epg.hour, 8);
    }

    #[test]
    fn test_get_start_time_utc() {
        let mut e = EventInfo::new();
        e.start_time = make_datetime(2024, 4, 1, 12, 0, 0);
        let utc = e.get_start_time_utc().unwrap();
        assert_eq!(utc.hour, 3);
    }

    #[test]
    fn test_get_end_time_utc() {
        let mut e = EventInfo::new();
        e.start_time = make_datetime(2024, 4, 1, 12, 0, 0);
        e.duration = 7200; // 2 hours
        // EPG 12:00 + 2h = 14:00 JST → 05:00 UTC
        let end_utc = e.get_end_time_utc().unwrap();
        assert_eq!(end_utc.hour, 5);
    }

    #[test]
    fn test_concatenated_extended_text_empty() {
        let e = EventInfo::new();
        assert_eq!(e.get_concatenated_extended_text(), "");
    }

    #[test]
    fn test_concatenated_extended_text() {
        let mut e = EventInfo::new();
        e.extended_text = vec![
            ExtendedTextInfo { description: "タイトル".into(), text: "本文1".into() },
            ExtendedTextInfo { description: String::new(), text: "本文2".into() },
        ];
        let text = e.get_concatenated_extended_text();
        assert!(text.contains("タイトル\n"));
        assert!(text.contains("本文1\n"));
        assert!(text.contains("本文2"));
    }

    #[test]
    fn test_main_audio_index_first_main() {
        let mut e = EventInfo::new();
        e.audio_list = vec![
            AudioInfo { main_component_flag: false, ..Default::default() },
            AudioInfo { main_component_flag: true, ..Default::default() },
        ];
        assert_eq!(e.get_main_audio_index(), Some(1));
    }

    #[test]
    fn test_main_audio_info_fallback() {
        let mut e = EventInfo::new();
        e.audio_list = vec![
            AudioInfo { main_component_flag: false, component_tag: 7, ..Default::default() },
        ];
        let info = e.get_main_audio_info().unwrap();
        assert_eq!(info.component_tag, 7);
    }

    #[test]
    fn test_main_audio_info_none_when_empty() {
        let e = EventInfo::new();
        assert!(e.get_main_audio_info().is_none());
    }

    #[test]
    fn test_is_equal() {
        let e1 = EventInfo {
            network_id: 1,
            event_id: 100,
            ..Default::default()
        };
        let e2 = e1.clone();
        assert!(e1.is_equal(&e2));
    }

    #[test]
    fn test_is_equal_diff_service() {
        let e1 = EventInfo { service_id: 1, ..Default::default() };
        let e2 = EventInfo { service_id: 2, ..Default::default() };
        assert!(!e1.is_equal(&e2));
    }

    #[test]
    fn test_event_info_eq() {
        let e1 = EventInfo {
            network_id: 10,
            type_flag: TypeFlag::Basic,
            updated_time: 12345,
            ..Default::default()
        };
        let e2 = e1.clone();
        assert_eq!(e1, e2);
    }

    #[test]
    fn test_epg_time_roundtrip() {
        let epg = make_datetime(2024, 6, 15, 20, 30, 0);
        let utc = epg_time_to_utc_time(&epg).unwrap();
        let back = utc_time_to_epg_time(&utc).unwrap();
        assert_eq!(back.year, epg.year);
        assert_eq!(back.month, epg.month);
        assert_eq!(back.day, epg.day);
        assert_eq!(back.hour, epg.hour);
        assert_eq!(back.minute, epg.minute);
    }
}
