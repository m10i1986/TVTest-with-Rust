//! TVTest のリモコンキー(Raw Input HID)処理(`src/RawInput.cpp` / `src/RawInput.h`)のうち
//! Win32 Raw Input API 本体を windows-rs で移植したクレート。
//!
//! 移植対象:
//! - `Initialize`(`RawInput.cpp:73-86`、`RegisterRawInputDevices`)。
//! - `OnInput`(`RawInput.cpp:89-119`、`GetRawInputData` による `WM_INPUT` メッセージ解析)。
//!
//! データ値⇔表示テキストのテーブル・`KeyDataToIndex` 等の純粋ロジックは
//! [`tvtest_raw_input`] クレートに分離済み(`tvtest_winutil`/`tvtest_util` の分割方針を踏襲)。
//!
//! `RAWINPUT` の HID データ部(`RAWHID::bRawData`)は可変長のフレキシブル配列メンバーで
//! あり、windows-rs の `RAWINPUT` 構造体をそのまま使うと安全にアクセスできない
//! (`bRawData: [u8; 1]` は実際のデータの一部しか表さない)。そのため本クレートでは
//! `GetRawInputData` が書き込んだ生バイト列を `&[u8]` として扱い、`RAWINPUTHEADER`
//! (`dwType`)と `RAWHID`(`dwSizeHid`/`dwCount`)のオフセットから直接読み出す。

#![cfg(windows)]

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::{
    GetRawInputData, RegisterRawInputDevices, HRAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER,
    RIDEV_INPUTSINK, RID_INPUT, RIM_TYPEHID,
};

pub use tvtest_raw_input as raw_input;

/// `RAWINPUTHEADER` のサイズ(`GetRawInputData` の `cbSizeHeader` 引数、`RawInput.cpp:96`)。
const RAWINPUTHEADER_SIZE: u32 = std::mem::size_of::<RAWINPUTHEADER>() as u32;

/// `Initialize`(`RawInput.cpp:73-86`)。
///
/// TVTest が使う 2 つのデバイス(メディアセンター USBIR レシーバ相当の
/// `usUsagePage=0xFFBC, usUsage=0x88` と、コンシューマコントロール
/// `usUsagePage=0x0C, usUsage=0x01`)を `RIDEV_INPUTSINK` で登録する。
pub fn initialize(hwnd: HWND) -> bool {
    let devices = [
        RAWINPUTDEVICE {
            usUsagePage: 0xFFBC,
            usUsage: 0x88,
            dwFlags: RIDEV_INPUTSINK,
            hwndTarget: hwnd,
        },
        RAWINPUTDEVICE {
            usUsagePage: 0x0C,
            usUsage: 0x01,
            dwFlags: RIDEV_INPUTSINK,
            hwndTarget: hwnd,
        },
    ];

    unsafe { RegisterRawInputDevices(&devices, std::mem::size_of::<RAWINPUTDEVICE>() as u32) }
        .is_ok()
}

/// `WM_INPUT` を受信したときに `GetRawInputData` で取得した生データの解析結果
/// (`OnInput` の分岐、`RawInput.cpp:103-115`)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawInputEvent {
    /// `KeyList` に登録済みのキーが押された(`KeyDataToIndex` で解決できたインデックス)。
    Key(i32),
    /// HID データではあるが `KeyList` に無い、またはヘッダ/サイズ条件を満たさない生データ。
    Unknown(Vec<u8>),
    /// HID タイプ以外(マウス/キーボード)、または `dwCount`/`dwSizeHid` 条件を満たさない。
    None,
}

/// `OnInput`(`RawInput.cpp:89-119`)。
///
/// `hraw_input` は `WM_INPUT` の `lParam` を `HRAWINPUT` として渡す。原実装は
/// イベントハンドラへ直接通知するが、本関数は解析結果を [`RawInputEvent`] として返し、
/// 通知は呼び出し側の責務とする。
pub fn on_input(hraw_input: HRAWINPUT) -> RawInputEvent {
    let mut size: u32 = 0;

    unsafe {
        GetRawInputData(hraw_input, RID_INPUT, None, &mut size, RAWINPUTHEADER_SIZE);
    }
    if size == 0 {
        return RawInputEvent::None;
    }

    let mut buffer = vec![0u8; size as usize];
    let written = unsafe {
        GetRawInputData(
            hraw_input,
            RID_INPUT,
            Some(buffer.as_mut_ptr().cast()),
            &mut size,
            RAWINPUTHEADER_SIZE,
        )
    };
    if written != size {
        return RawInputEvent::None;
    }

    parse_raw_input_buffer(&buffer)
}

/// `GetRawInputData` が書き込んだ生バイト列から `RawInputEvent` を組み立てる
/// (`RawInput.cpp:103-115` の純粋パース部分、テスト容易性のため公開)。
///
/// バッファレイアウト: `RAWINPUTHEADER`(`dwType` を含む)に続けて、
/// `RIM_TYPEHID` の場合は `dwSizeHid: u32`・`dwCount: u32`・`bRawData: [u8; ...]`
/// (`RAWHID`、`RawInput.h`)が並ぶ。
#[must_use]
pub fn parse_raw_input_buffer(buffer: &[u8]) -> RawInputEvent {
    const HEADER_SIZE: usize = RAWINPUTHEADER_SIZE as usize;
    const HID_PREFIX_SIZE: usize = 8; // dwSizeHid: u32 + dwCount: u32

    if buffer.len() < HEADER_SIZE + 4 {
        return RawInputEvent::None;
    }

    let dw_type = u32::from_ne_bytes(buffer[0..4].try_into().unwrap());
    if dw_type != RIM_TYPEHID.0 {
        return RawInputEvent::None;
    }

    if buffer.len() < HEADER_SIZE + HID_PREFIX_SIZE {
        return RawInputEvent::None;
    }

    let hid = &buffer[HEADER_SIZE..];
    let dw_size_hid = u32::from_ne_bytes(hid[0..4].try_into().unwrap());
    let dw_count = u32::from_ne_bytes(hid[4..8].try_into().unwrap());

    if dw_count < 1 || dw_size_hid < 3 {
        return RawInputEvent::None;
    }

    let raw_data = &hid[HID_PREFIX_SIZE..];
    if raw_data.len() < 3 {
        return RawInputEvent::None;
    }

    let data_value = raw_data[1] as i32 | ((raw_data[2] as i32) << 8);
    let index = raw_input::key_data_to_index(data_value);

    if index >= 0 {
        RawInputEvent::Key(index)
    } else {
        let total_size = (dw_count as usize) * (dw_size_hid as usize);
        let available = total_size.min(raw_data.len());
        RawInputEvent::Unknown(raw_data[..available].to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_hid_buffer(dw_type: u32, dw_size_hid: u32, dw_count: u32, raw_data: &[u8]) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&dw_type.to_ne_bytes());
        buf.resize(RAWINPUTHEADER_SIZE as usize, 0);
        buf.extend_from_slice(&dw_size_hid.to_ne_bytes());
        buf.extend_from_slice(&dw_count.to_ne_bytes());
        buf.extend_from_slice(raw_data);
        buf
    }

    #[test]
    fn parse_empty_buffer_is_none() {
        assert_eq!(parse_raw_input_buffer(&[]), RawInputEvent::None);
    }

    #[test]
    fn parse_non_hid_type_is_none() {
        let buf = build_hid_buffer(RIM_TYPEHID.0 + 1, 4, 1, &[0, 1, 2, 3]);
        assert_eq!(parse_raw_input_buffer(&buf), RawInputEvent::None);
    }

    #[test]
    fn parse_known_key_resolves_index() {
        // RAWINPUT_GUIDE = 0x008D -> KeyList のインデックス
        let data_value = raw_input::RAWINPUT_GUIDE;
        let raw_data = [0u8, (data_value & 0xFF) as u8, ((data_value >> 8) & 0xFF) as u8];
        let buf = build_hid_buffer(RIM_TYPEHID.0, 3, 1, &raw_data);
        let expected_index = raw_input::key_data_to_index(data_value);
        assert_eq!(parse_raw_input_buffer(&buf), RawInputEvent::Key(expected_index));
    }

    #[test]
    fn parse_unknown_key_returns_raw_data() {
        let raw_data = [0u8, 0xFFu8, 0xFFu8];
        let buf = build_hid_buffer(RIM_TYPEHID.0, 3, 1, &raw_data);
        match parse_raw_input_buffer(&buf) {
            RawInputEvent::Unknown(data) => assert_eq!(data, raw_data.to_vec()),
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    #[test]
    fn parse_dw_count_zero_is_none() {
        let raw_data = [0u8, 1, 2];
        let buf = build_hid_buffer(RIM_TYPEHID.0, 3, 0, &raw_data);
        assert_eq!(parse_raw_input_buffer(&buf), RawInputEvent::None);
    }

    #[test]
    fn parse_dw_size_hid_too_small_is_none() {
        let raw_data = [0u8, 1];
        let buf = build_hid_buffer(RIM_TYPEHID.0, 2, 1, &raw_data);
        assert_eq!(parse_raw_input_buffer(&buf), RawInputEvent::None);
    }

    #[test]
    fn parse_buffer_too_short_for_header_is_none() {
        let buf = vec![0u8; 2];
        assert_eq!(parse_raw_input_buffer(&buf), RawInputEvent::None);
    }

    #[test]
    fn parse_buffer_too_short_for_hid_prefix_is_none() {
        let mut buf = vec![0u8; RAWINPUTHEADER_SIZE as usize];
        buf[0..4].copy_from_slice(&(RIM_TYPEHID.0).to_ne_bytes());
        buf.extend_from_slice(&[0u8; 2]); // dwSizeHid/dwCount 分に満たない
        assert_eq!(parse_raw_input_buffer(&buf), RawInputEvent::None);
    }

    #[test]
    fn parse_all_key_list_entries_round_trip() {
        for &k in raw_input::KEY_LIST.iter() {
            let raw_data = [0u8, (k.raw_data & 0xFF) as u8, ((k.raw_data >> 8) & 0xFF) as u8];
            let buf = build_hid_buffer(RIM_TYPEHID.0, 3, 1, &raw_data);
            let expected_index = raw_input::key_data_to_index(k.raw_data);
            assert_eq!(parse_raw_input_buffer(&buf), RawInputEvent::Key(expected_index));
        }
    }

    #[test]
    fn initialize_registers_devices_without_window() {
        // ウィンドウなし(HWND::default)でも RegisterRawInputDevices 自体は成功しうる。
        // 実行環境依存だが、少なくともパニックしないことを確認する。
        let _ = initialize(HWND::default());
    }
}
