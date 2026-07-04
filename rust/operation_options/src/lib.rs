//! TVTest の操作設定(`src/OperationOptions.cpp` / `OperationOptions.h`)の
//! 純粋ロジックを移植したクレート。
//!
//! 移植対象:
//! - `WHEEL_CHANNEL_DELAY_MIN`(OperationOptions.h:38)。
//! - 各コマンドの既定値(コンストラクタ、OperationOptions.cpp:34-43)。
//! - `ReadSettings` 内の「ver.0.9.0 より前との互換用」`WheelModeList` 配列引き
//!   (OperationOptions.cpp:64-96)。新形式の文字列コマンド("WheelCommand" 等)が
//!   読めなかった場合に、旧形式の整数モード("WheelMode" 等)を `WheelModeList` で
//!   コマンド ID へ変換する部分のみを純粋関数化したもの。
//! - `WheelChannelDelay` の下限クランプ(OperationOptions.cpp:101-105)。
//! - `IsWheelCommandReverse`(OperationOptions.cpp:180-189)。
//!
//! 対象外(Win32 / CSettings / WheelCommandManager 依存):
//! - `COperationOptions::DlgProc`(ダイアログ)。
//! - `ReadSettings` / `WriteSettings` の `CSettings` I/O 本体(本クレートは値の変換のみ提供)。
//! - `CWheelCommandManager::ParseCommand` / `GetCommandParsableName` による、新形式の
//!   文字列コマンド名の解決(既に `rust/wheel_command` クレートに実装済み)。
//! - `CCommandManager::ParseIDText` / `GetCommandIDText` の呼び出し(マウスクリック系
//!   コマンドの文字列解決、既に `rust/command` クレートに実装済み)。

#![forbid(unsafe_code)]

/// `WheelChannelDelay` の下限値(ミリ秒)。OperationOptions.h:38。
pub const WHEEL_CHANNEL_DELAY_MIN: i32 = 100;

/// ホイール操作で音量を変更するコマンド ID。resource.h の `CM_WHEEL_VOLUME`(19400)。
pub const CM_WHEEL_VOLUME: i32 = 19400;
/// ホイール操作でチャンネルを変更するコマンド ID。resource.h の `CM_WHEEL_CHANNEL`(19401)。
pub const CM_WHEEL_CHANNEL: i32 = 19401;
/// ホイール操作で音声を切り替えるコマンド ID。resource.h の `CM_WHEEL_AUDIO`(19402)。
pub const CM_WHEEL_AUDIO: i32 = 19402;
/// ホイール操作でズームを変更するコマンド ID。resource.h の `CM_WHEEL_ZOOM`(19403)。
pub const CM_WHEEL_ZOOM: i32 = 19403;
/// ホイール操作でアスペクト比を変更するコマンド ID。resource.h の `CM_WHEEL_ASPECTRATIO`(19404)。
pub const CM_WHEEL_ASPECTRATIO: i32 = 19404;

/// 全画面表示コマンド ID。resource.h の `CM_FULLSCREEN`(137)。
pub const CM_FULLSCREEN: i32 = 137;
/// メニュー表示コマンド ID。resource.h の `CM_MENU`(232)。
pub const CM_MENU: i32 = 232;

/// ver.0.9.0 より前との互換用の `WheelModeList`(OperationOptions.cpp:64-71)。
///
/// 旧形式の整数モード値("WheelMode" 等)を、そのインデックスでコマンド ID へ変換する
/// ためのテーブル。
pub const WHEEL_MODE_LIST: [i32; 6] = [
    0,
    CM_WHEEL_VOLUME,
    CM_WHEEL_CHANNEL,
    CM_WHEEL_AUDIO,
    CM_WHEEL_ZOOM,
    CM_WHEEL_ASPECTRATIO,
];

/// `m_WheelCommand` の既定値(コンストラクタ、OperationOptions.cpp:35)。
pub const DEFAULT_WHEEL_COMMAND: i32 = CM_WHEEL_VOLUME;
/// `m_WheelShiftCommand` の既定値(コンストラクタ、OperationOptions.cpp:36)。
pub const DEFAULT_WHEEL_SHIFT_COMMAND: i32 = CM_WHEEL_CHANNEL;
/// `m_WheelCtrlCommand` の既定値(コンストラクタ、OperationOptions.cpp:37)。
pub const DEFAULT_WHEEL_CTRL_COMMAND: i32 = CM_WHEEL_AUDIO;
/// `m_WheelTiltCommand` の既定値(コンストラクタ、OperationOptions.cpp:38)。
pub const DEFAULT_WHEEL_TILT_COMMAND: i32 = 0;
/// `m_LeftDoubleClickCommand` の既定値(コンストラクタ、OperationOptions.cpp:39)。
pub const DEFAULT_LEFT_DOUBLE_CLICK_COMMAND: i32 = CM_FULLSCREEN;
/// `m_RightClickCommand` の既定値(コンストラクタ、OperationOptions.cpp:40)。
pub const DEFAULT_RIGHT_CLICK_COMMAND: i32 = CM_MENU;
/// `m_MiddleClickCommand` の既定値(コンストラクタ、OperationOptions.cpp:41)。
pub const DEFAULT_MIDDLE_CLICK_COMMAND: i32 = 0;
/// `m_WheelChannelDelay` の既定値(OperationOptions.h:85)。
pub const DEFAULT_WHEEL_CHANNEL_DELAY: i32 = 1000;

/// 旧形式の整数モード値を `WheelModeList` によりコマンド ID へ変換する
/// (`ReadSettings`、OperationOptions.cpp:75-96 の
/// `Value >= 0 && Value < lengthof(WheelModeList)` の判定と配列引きに相当)。
///
/// 範囲外(`value < 0` または `value >= WHEEL_MODE_LIST.len()`)なら `None` を返す。
/// この場合、呼び出し側は値を変更せず既存のコマンドを維持する(原実装の `else if` が
/// 不成立のときに何もしないのと同じ)。
#[must_use]
pub fn wheel_command_from_legacy_mode(value: i32) -> Option<i32> {
    usize::try_from(value)
        .ok()
        .and_then(|index| WHEEL_MODE_LIST.get(index).copied())
}

/// `WheelChannelDelay` を下限クランプする(`ReadSettings`、OperationOptions.cpp:101-105)。
///
/// `value < WHEEL_CHANNEL_DELAY_MIN` なら `WHEEL_CHANNEL_DELAY_MIN` に引き上げる。
/// 上限のクランプは行わない。
#[must_use]
pub fn clamp_wheel_channel_delay(value: i32) -> i32 {
    value.max(WHEEL_CHANNEL_DELAY_MIN)
}

/// 指定コマンドがホイール反転設定の対象かを判定する
/// (`IsWheelCommandReverse`、OperationOptions.cpp:180-189)。
///
/// `CM_WHEEL_VOLUME` なら `wheel_volume_reverse`、`CM_WHEEL_CHANNEL` なら
/// `wheel_channel_reverse`、それ以外は常に `false`。
#[must_use]
pub fn is_wheel_command_reverse(
    command: i32,
    wheel_volume_reverse: bool,
    wheel_channel_reverse: bool,
) -> bool {
    match command {
        CM_WHEEL_VOLUME => wheel_volume_reverse,
        CM_WHEEL_CHANNEL => wheel_channel_reverse,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_mode_list_contents() {
        assert_eq!(
            WHEEL_MODE_LIST,
            [
                0,
                CM_WHEEL_VOLUME,
                CM_WHEEL_CHANNEL,
                CM_WHEEL_AUDIO,
                CM_WHEEL_ZOOM,
                CM_WHEEL_ASPECTRATIO,
            ]
        );
    }

    #[test]
    fn legacy_mode_index_0_is_none_command() {
        // WheelMode=0 は「何もしない」(コマンド ID 0)。
        assert_eq!(wheel_command_from_legacy_mode(0), Some(0));
    }

    #[test]
    fn legacy_mode_index_1_is_volume() {
        assert_eq!(wheel_command_from_legacy_mode(1), Some(CM_WHEEL_VOLUME));
    }

    #[test]
    fn legacy_mode_index_2_is_channel() {
        assert_eq!(wheel_command_from_legacy_mode(2), Some(CM_WHEEL_CHANNEL));
    }

    #[test]
    fn legacy_mode_index_3_is_audio() {
        assert_eq!(wheel_command_from_legacy_mode(3), Some(CM_WHEEL_AUDIO));
    }

    #[test]
    fn legacy_mode_index_4_is_zoom() {
        assert_eq!(wheel_command_from_legacy_mode(4), Some(CM_WHEEL_ZOOM));
    }

    #[test]
    fn legacy_mode_index_5_is_aspect_ratio() {
        assert_eq!(
            wheel_command_from_legacy_mode(5),
            Some(CM_WHEEL_ASPECTRATIO)
        );
    }

    #[test]
    fn legacy_mode_out_of_range_is_none() {
        // 負の値は範囲外。
        assert_eq!(wheel_command_from_legacy_mode(-1), None);
        // WheelModeList の長さ(6)以上も範囲外。
        assert_eq!(wheel_command_from_legacy_mode(6), None);
    }

    #[test]
    fn clamp_wheel_channel_delay_boundaries() {
        // 下限未満は下限に引き上げ。
        assert_eq!(clamp_wheel_channel_delay(99), 100);
        // 下限ちょうどはそのまま。
        assert_eq!(clamp_wheel_channel_delay(100), 100);
        // 下限超過はそのまま(上限クランプなし)。
        assert_eq!(clamp_wheel_channel_delay(101), 101);
    }

    #[test]
    fn clamp_wheel_channel_delay_no_upper_bound() {
        assert_eq!(clamp_wheel_channel_delay(1_000_000), 1_000_000);
    }

    #[test]
    fn is_wheel_command_reverse_volume() {
        assert!(is_wheel_command_reverse(CM_WHEEL_VOLUME, true, false));
        assert!(!is_wheel_command_reverse(CM_WHEEL_VOLUME, false, true));
    }

    #[test]
    fn is_wheel_command_reverse_channel() {
        assert!(is_wheel_command_reverse(CM_WHEEL_CHANNEL, false, true));
        assert!(!is_wheel_command_reverse(CM_WHEEL_CHANNEL, true, false));
    }

    #[test]
    fn is_wheel_command_reverse_other_command_always_false() {
        assert!(!is_wheel_command_reverse(CM_WHEEL_AUDIO, true, true));
        assert!(!is_wheel_command_reverse(0, true, true));
        assert!(!is_wheel_command_reverse(CM_FULLSCREEN, true, true));
    }

    #[test]
    fn default_constants_match_constructor() {
        assert_eq!(DEFAULT_WHEEL_COMMAND, CM_WHEEL_VOLUME);
        assert_eq!(DEFAULT_WHEEL_SHIFT_COMMAND, CM_WHEEL_CHANNEL);
        assert_eq!(DEFAULT_WHEEL_CTRL_COMMAND, CM_WHEEL_AUDIO);
        assert_eq!(DEFAULT_WHEEL_TILT_COMMAND, 0);
        assert_eq!(DEFAULT_LEFT_DOUBLE_CLICK_COMMAND, CM_FULLSCREEN);
        assert_eq!(DEFAULT_RIGHT_CLICK_COMMAND, CM_MENU);
        assert_eq!(DEFAULT_MIDDLE_CLICK_COMMAND, 0);
        assert_eq!(DEFAULT_WHEEL_CHANNEL_DELAY, 1000);
    }
}
