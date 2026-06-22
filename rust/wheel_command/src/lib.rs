//! TVTest の `CWheelCommandManager`(src/WheelCommand.cpp / WheelCommand.h)の移植。
//!
//! マウスホイールに割り当て可能なコマンドの固定テーブル(コマンド ID と設定ファイル
//! 用の識別子テキスト)を保持し、ID と識別子テキストの相互変換を行う。
//!
//! 文字列は原実装の `wchar_t`(UTF-16)に合わせて `[u16]`/`Vec<u16>` で扱う。
//!
//! # 原実装からの差異(挙動は等価)
//! - `GetCommandText`(`LoadString` によるリソース文字列取得)は Win32 依存のため対象外。
//! - 識別子テキストの比較は `StringUtility::IsEqualNoCase`(Win32 `towlower` ベース)に
//!   相当する大文字小文字無視だが、識別子は ASCII のため ASCII 畳み込みで等価。

/// ホイールコマンドの ID(resource.h 527-532)。
pub const CM_WHEEL_VOLUME: i32 = 19400;
pub const CM_WHEEL_CHANNEL: i32 = 19401;
pub const CM_WHEEL_AUDIO: i32 = 19402;
pub const CM_WHEEL_ZOOM: i32 = 19403;
pub const CM_WHEEL_ASPECTRATIO: i32 = 19404;
pub const CM_WHEEL_AUDIODELAY: i32 = 19405;

/// 識別子テキストの最大長(WheelCommand.h 34)。
pub const MAX_COMMAND_PARSABLE_NAME: usize = 32;
/// コマンドテキストの最大長(WheelCommand.h 35)。
pub const MAX_COMMAND_TEXT: usize = 64;

/// ホイールコマンドの固定テーブル(WheelCommand.cpp 35-45)。
const COMMAND_LIST: &[(i32, &str)] = &[
    (CM_WHEEL_VOLUME, "WheelVolume"),
    (CM_WHEEL_CHANNEL, "WheelChannel"),
    (CM_WHEEL_AUDIO, "WheelAudio"),
    (CM_WHEEL_ZOOM, "WheelZoom"),
    (CM_WHEEL_ASPECTRATIO, "WheelAspectRatio"),
    (CM_WHEEL_AUDIODELAY, "WheelAudioDelay"),
];

/// ASCII 大文字を小文字へ畳み込む(原実装の `towlower` を ASCII 範囲で代替)。
fn ascii_fold(c: u16) -> u16 {
    if (0x41..=0x5A).contains(&c) {
        c + 0x20
    } else {
        c
    }
}

/// `[u16]` と `str` を大文字小文字無視(ASCII)で比較する。
fn eq_no_case(a: &[u16], b: &str) -> bool {
    let mut bi = b.encode_utf16();
    let mut ai = a.iter().copied();
    loop {
        match (ai.next(), bi.next()) {
            (Some(x), Some(y)) => {
                if ascii_fold(x) != ascii_fold(y) {
                    return false;
                }
            }
            (None, None) => return true,
            _ => return false, // 長さ不一致
        }
    }
}

/// `CWheelCommandManager` の移植。
pub struct WheelCommandManager {
    command_list: &'static [(i32, &'static str)],
}

impl Default for WheelCommandManager {
    fn default() -> Self {
        Self::new()
    }
}

impl WheelCommandManager {
    /// 固定テーブルでマネージャを生成する(WheelCommand.cpp 33-52)。
    pub fn new() -> Self {
        Self {
            command_list: COMMAND_LIST,
        }
    }

    /// コマンド数を返す(WheelCommand.cpp 55-58 `GetCommandCount`)。
    pub fn command_count(&self) -> i32 {
        self.command_list.len() as i32
    }

    /// インデックスからコマンド ID を返す(WheelCommand.cpp 61-67 `GetCommandID`)。
    /// 範囲外は 0。
    pub fn command_id(&self, index: i32) -> i32 {
        if index < 0 || index as usize >= self.command_list.len() {
            return 0;
        }
        self.command_list[index as usize].0
    }

    /// コマンド ID から設定ファイル用の識別子テキストを返す
    /// (WheelCommand.cpp 70-88 `GetCommandParsableName`)。
    ///
    /// - `ID <= 0`: 空文字列(原実装は長さ 0 を返す)。
    /// - 見つかった: 識別子テキスト。
    /// - 見つからない: `None`(原実装は -1 を返す)。
    pub fn command_parsable_name(&self, id: i32) -> Option<Vec<u16>> {
        if id <= 0 {
            return Some(Vec::new());
        }
        for &(cmd_id, text) in self.command_list {
            if cmd_id == id {
                return Some(text.encode_utf16().collect());
            }
        }
        None
    }

    /// 識別子テキストからコマンド ID を解決する(WheelCommand.cpp 100-114 `ParseCommand`)。
    /// 空文字列・未知の識別子は 0。
    pub fn parse_command(&self, command: &[u16]) -> i32 {
        if command.is_empty() {
            return 0;
        }
        for &(cmd_id, text) in self.command_list {
            if eq_no_case(command, text) {
                return cmd_id;
            }
        }
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn command_count_and_ids() {
        let m = WheelCommandManager::new();
        assert_eq!(m.command_count(), 6);
        assert_eq!(m.command_id(0), CM_WHEEL_VOLUME);
        assert_eq!(m.command_id(5), CM_WHEEL_AUDIODELAY);
        // 範囲外
        assert_eq!(m.command_id(-1), 0);
        assert_eq!(m.command_id(6), 0);
    }

    #[test]
    fn parsable_name_lookup() {
        let m = WheelCommandManager::new();
        assert_eq!(m.command_parsable_name(CM_WHEEL_ZOOM), Some(w("WheelZoom")));
        assert_eq!(
            m.command_parsable_name(CM_WHEEL_ASPECTRATIO),
            Some(w("WheelAspectRatio"))
        );
        // ID <= 0 は空文字列
        assert_eq!(m.command_parsable_name(0), Some(Vec::new()));
        assert_eq!(m.command_parsable_name(-5), Some(Vec::new()));
        // 未知の ID は None
        assert_eq!(m.command_parsable_name(99999), None);
    }

    #[test]
    fn parse_command_case_insensitive() {
        let m = WheelCommandManager::new();
        assert_eq!(m.parse_command(&w("WheelVolume")), CM_WHEEL_VOLUME);
        assert_eq!(m.parse_command(&w("wheelvolume")), CM_WHEEL_VOLUME);
        assert_eq!(m.parse_command(&w("WHEELAUDIODELAY")), CM_WHEEL_AUDIODELAY);
        // 空・未知
        assert_eq!(m.parse_command(&[]), 0);
        assert_eq!(m.parse_command(&w("Unknown")), 0);
        // 前方一致だが長さ不一致は不可
        assert_eq!(m.parse_command(&w("Wheel")), 0);
        assert_eq!(m.parse_command(&w("WheelVolumeX")), 0);
    }

    #[test]
    fn name_and_id_roundtrip() {
        let m = WheelCommandManager::new();
        for i in 0..m.command_count() {
            let id = m.command_id(i);
            let name = m.command_parsable_name(id).unwrap();
            assert_eq!(m.parse_command(&name), id);
        }
    }
}
