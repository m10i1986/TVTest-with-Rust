//! TVTest のリモコンキー(Raw Input HID)処理(`src/RawInput.cpp` / `src/RawInput.h`)の
//! 純粋ロジックを移植したクレート。
//!
//! 移植対象:
//! - HID データ値の定数群 `RAWINPUT_*`(`RawInput.cpp:31-47`)。
//! - `KeyList`(HID データ値⇔表示テキストのテーブル、`RawInput.cpp:49-68`)。
//! - `NumKeyTypes`(`RawInput.cpp:128-131`)。
//! - `GetKeyText`(`RawInput.cpp:134-139`)。
//! - `GetKeyData`(`RawInput.cpp:142-147`)。
//! - `KeyDataToIndex`(`RawInput.cpp:150-157`)。
//!
//! 対象外(Win32 Raw Input API 依存):
//! - `Initialize`(`RegisterRawInputDevices`)。
//! - `OnInput`(`GetRawInputData` による `WM_INPUT` メッセージ解析、イベントハンドラ通知)。
//! - `SetEventHandler`。

#![forbid(unsafe_code)]

/// HID データ値の定数群(`RawInput.cpp:31-47`)。
pub const RAWINPUT_DETAILS: i32 = 0x0209;
pub const RAWINPUT_GUIDE: i32 = 0x008D;
pub const RAWINPUT_TVJUMP: i32 = 0x0025;
pub const RAWINPUT_STANDBY: i32 = 0x0082;
pub const RAWINPUT_OEM1: i32 = 0x0080;
pub const RAWINPUT_OEM2: i32 = 0x0081;
pub const RAWINPUT_MYTV: i32 = 0x0046;
pub const RAWINPUT_MYVIDEOS: i32 = 0x004A;
pub const RAWINPUT_MYPICTURES: i32 = 0x0049;
pub const RAWINPUT_MYMUSIC: i32 = 0x0047;
pub const RAWINPUT_RECORDEDTV: i32 = 0x0048;
pub const RAWINPUT_DVDANGLE: i32 = 0x004B;
pub const RAWINPUT_DVDAUDIO: i32 = 0x004C;
pub const RAWINPUT_DVDMENU: i32 = 0x0024;
pub const RAWINPUT_DVDSUBTITLE: i32 = 0x004D;

/// `KeyList` の 1 エントリ(`RawInput.cpp:49-52` の無名構造体)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawInputKey {
    pub raw_data: i32,
    pub text: &'static str,
}

const fn key(raw_data: i32, text: &'static str) -> RawInputKey {
    RawInputKey { raw_data, text }
}

/// `KeyList`(`RawInput.cpp:49-68`)。要素数は原実装どおり 15。
pub static KEY_LIST: [RawInputKey; 15] = [
    key(RAWINPUT_DETAILS, "Details"),
    key(RAWINPUT_GUIDE, "Guide"),
    key(RAWINPUT_TVJUMP, "TV Jump"),
    key(RAWINPUT_STANDBY, "Standby"),
    key(RAWINPUT_OEM1, "OEM1"),
    key(RAWINPUT_OEM2, "OEM2"),
    key(RAWINPUT_MYTV, "My TV"),
    key(RAWINPUT_MYVIDEOS, "My Videos"),
    key(RAWINPUT_MYPICTURES, "My Pictures"),
    key(RAWINPUT_MYMUSIC, "My Music"),
    key(RAWINPUT_RECORDEDTV, "Recorded TV"),
    key(RAWINPUT_DVDANGLE, "DVD Angle"),
    key(RAWINPUT_DVDAUDIO, "DVD Audio"),
    key(RAWINPUT_DVDMENU, "DVD Menu"),
    key(RAWINPUT_DVDSUBTITLE, "DVD Subtitle"),
];

/// `NumKeyTypes`(`RawInput.cpp:128-131`)。
#[must_use]
pub fn num_key_types() -> usize {
    KEY_LIST.len()
}

/// `GetKeyText`(`RawInput.cpp:134-139`)。範囲外は `None`(原実装は `nullptr`)。
#[must_use]
pub fn get_key_text(key_index: i32) -> Option<&'static str> {
    if key_index < 0 || (key_index as usize) >= KEY_LIST.len() {
        return None;
    }
    Some(KEY_LIST[key_index as usize].text)
}

/// `GetKeyData`(`RawInput.cpp:142-147`)。範囲外は `0`。
#[must_use]
pub fn get_key_data(key_index: i32) -> i32 {
    if key_index < 0 || (key_index as usize) >= KEY_LIST.len() {
        return 0;
    }
    KEY_LIST[key_index as usize].raw_data
}

/// `KeyDataToIndex`(`RawInput.cpp:150-157`)。見つからなければ `-1`。
#[must_use]
pub fn key_data_to_index(data: i32) -> i32 {
    match KEY_LIST.iter().position(|k| k.raw_data == data) {
        Some(i) => i as i32,
        None => -1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_list_len_is_15() {
        assert_eq!(KEY_LIST.len(), 15);
    }

    #[test]
    fn num_key_types_matches_list_len() {
        assert_eq!(num_key_types(), 15);
    }

    #[test]
    fn key_list_first_and_last_entries() {
        assert_eq!(KEY_LIST[0], key(RAWINPUT_DETAILS, "Details"));
        assert_eq!(KEY_LIST[14], key(RAWINPUT_DVDSUBTITLE, "DVD Subtitle"));
    }

    #[test]
    fn get_key_text_valid_index() {
        assert_eq!(get_key_text(0), Some("Details"));
        assert_eq!(get_key_text(6), Some("My TV"));
        assert_eq!(get_key_text(14), Some("DVD Subtitle"));
    }

    #[test]
    fn get_key_text_out_of_range() {
        assert_eq!(get_key_text(-1), None);
        assert_eq!(get_key_text(15), None);
        assert_eq!(get_key_text(1000), None);
    }

    #[test]
    fn get_key_data_valid_index() {
        assert_eq!(get_key_data(0), RAWINPUT_DETAILS);
        assert_eq!(get_key_data(14), RAWINPUT_DVDSUBTITLE);
    }

    #[test]
    fn get_key_data_out_of_range() {
        assert_eq!(get_key_data(-1), 0);
        assert_eq!(get_key_data(15), 0);
    }

    #[test]
    fn key_data_to_index_found() {
        assert_eq!(key_data_to_index(RAWINPUT_DETAILS), 0);
        assert_eq!(key_data_to_index(RAWINPUT_MYTV), 6);
        assert_eq!(key_data_to_index(RAWINPUT_DVDSUBTITLE), 14);
    }

    #[test]
    fn key_data_to_index_not_found() {
        assert_eq!(key_data_to_index(0x9999), -1);
        assert_eq!(key_data_to_index(0), -1);
    }

    #[test]
    fn round_trip_get_key_data_and_index() {
        for i in 0..KEY_LIST.len() as i32 {
            let data = get_key_data(i);
            assert_eq!(key_data_to_index(data), i);
        }
    }

    #[test]
    fn rawinput_constant_values() {
        assert_eq!(RAWINPUT_DETAILS, 0x0209);
        assert_eq!(RAWINPUT_GUIDE, 0x008D);
        assert_eq!(RAWINPUT_DVDMENU, 0x0024);
    }
}
