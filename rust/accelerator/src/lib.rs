//! TVTest の `CAccelerator`(src/Accelerator.cpp / Accelerator.h)の純粋ロジック移植。
//!
//! キーボードショートカット(アクセラレータ)に関する以下を移植する:
//! - 仮想キーコード → 表示テキストの対応表([`ACCEL_KEY_LIST`] / [`get_key_name`])
//! - 修飾キー + キーの表示文字列整形([`format_accel_text`])
//! - 設定項目パラメータ(キー/修飾子/グローバル/アプリコマンド)の符号化・復号
//! - ホットキー / アプリコマンドの照合([`translate_hotkey`] / [`translate_app_command`])
//!
//! 文字列は原実装の `wchar_t`(UTF-16)に合わせて `Vec<u16>` で返す。
//!
//! # 対象外(Win32 / リソース依存)
//! `HACCEL` 生成、設定ダイアログ(`CBasicDialog`)、`CListView`、`CRawInput`、
//! メニュー反映(`SetMenuAccel`)、既定キー割当テーブル(`m_DefaultAccelList`、
//! `CM_*` リソース ID に依存)、`LoadSettings`/`SaveSettings`。

/// 修飾キーフラグ(Win32 ホットキー修飾子 `MOD_*`)。
pub const MOD_ALT: u8 = 0x01;
pub const MOD_CONTROL: u8 = 0x02;
pub const MOD_SHIFT: u8 = 0x04;

/// アクセラレータのキー名テーブルで用いる仮想キーコード(Win32 `VK_*`)。
pub mod vk {
    pub const BACK: u16 = 0x08;
    pub const TAB: u16 = 0x09;
    pub const CLEAR: u16 = 0x0C;
    pub const RETURN: u16 = 0x0D;
    pub const PAUSE: u16 = 0x13;
    pub const ESCAPE: u16 = 0x1B;
    pub const SPACE: u16 = 0x20;
    pub const PRIOR: u16 = 0x21;
    pub const NEXT: u16 = 0x22;
    pub const END: u16 = 0x23;
    pub const HOME: u16 = 0x24;
    pub const LEFT: u16 = 0x25;
    pub const UP: u16 = 0x26;
    pub const RIGHT: u16 = 0x27;
    pub const DOWN: u16 = 0x28;
    pub const SELECT: u16 = 0x29;
    pub const PRINT: u16 = 0x2A;
    pub const EXECUTE: u16 = 0x2B;
    pub const INSERT: u16 = 0x2D;
    pub const DELETE: u16 = 0x2E;
    pub const HELP: u16 = 0x2F;
    pub const OEM_1: u16 = 0xBA;
    pub const OEM_PLUS: u16 = 0xBB;
    pub const OEM_COMMA: u16 = 0xBC;
    pub const OEM_MINUS: u16 = 0xBD;
    pub const OEM_PERIOD: u16 = 0xBE;
    pub const OEM_2: u16 = 0xBF;
    pub const OEM_3: u16 = 0xC0;
    pub const OEM_4: u16 = 0xDB;
    pub const OEM_5: u16 = 0xDC;
    pub const OEM_6: u16 = 0xDD;
    pub const OEM_7: u16 = 0xDE;
    pub const OEM_102: u16 = 0xE2;
    pub const NUMPAD0: u16 = 0x60;
    pub const NUMPAD1: u16 = 0x61;
    pub const NUMPAD2: u16 = 0x62;
    pub const NUMPAD3: u16 = 0x63;
    pub const NUMPAD4: u16 = 0x64;
    pub const NUMPAD5: u16 = 0x65;
    pub const NUMPAD6: u16 = 0x66;
    pub const NUMPAD7: u16 = 0x67;
    pub const NUMPAD8: u16 = 0x68;
    pub const NUMPAD9: u16 = 0x69;
    pub const MULTIPLY: u16 = 0x6A;
    pub const ADD: u16 = 0x6B;
    pub const SUBTRACT: u16 = 0x6D;
    pub const DECIMAL: u16 = 0x6E;
    pub const DIVIDE: u16 = 0x6F;
    pub const F1: u16 = 0x70;
    pub const F2: u16 = 0x71;
    pub const F3: u16 = 0x72;
    pub const F4: u16 = 0x73;
    pub const F5: u16 = 0x74;
    pub const F6: u16 = 0x75;
    pub const F7: u16 = 0x76;
    pub const F8: u16 = 0x77;
    pub const F9: u16 = 0x78;
    pub const F10: u16 = 0x79;
    pub const F11: u16 = 0x7A;
    pub const F12: u16 = 0x7B;
}

/// 仮想キーコード → 表示テキストの対応表(Accelerator.cpp 48-148 `AccelKeyList`)。
pub const ACCEL_KEY_LIST: &[(u16, &str)] = &[
    (vk::BACK, "BS"),
    (vk::TAB, "Tab"),
    (vk::CLEAR, "Clear"),
    (vk::RETURN, "Enter"),
    (vk::PAUSE, "Pause"),
    (vk::ESCAPE, "Esc"),
    (vk::SPACE, "Space"),
    (vk::PRIOR, "PgUp"),
    (vk::NEXT, "PgDown"),
    (vk::END, "End"),
    (vk::HOME, "Home"),
    (vk::LEFT, "←"),
    (vk::UP, "↑"),
    (vk::RIGHT, "→"),
    (vk::DOWN, "↓"),
    (vk::SELECT, "Select"),
    (vk::PRINT, "Print"),
    (vk::EXECUTE, "Execute"),
    (vk::INSERT, "Ins"),
    (vk::DELETE, "Del"),
    (vk::HELP, "Help"),
    ('0' as u16, "0"),
    ('1' as u16, "1"),
    ('2' as u16, "2"),
    ('3' as u16, "3"),
    ('4' as u16, "4"),
    ('5' as u16, "5"),
    ('6' as u16, "6"),
    ('7' as u16, "7"),
    ('8' as u16, "8"),
    ('9' as u16, "9"),
    ('A' as u16, "A"),
    ('B' as u16, "B"),
    ('C' as u16, "C"),
    ('D' as u16, "D"),
    ('E' as u16, "E"),
    ('F' as u16, "F"),
    ('G' as u16, "G"),
    ('H' as u16, "H"),
    ('I' as u16, "I"),
    ('J' as u16, "J"),
    ('K' as u16, "K"),
    ('L' as u16, "L"),
    ('M' as u16, "M"),
    ('N' as u16, "N"),
    ('O' as u16, "O"),
    ('P' as u16, "P"),
    ('Q' as u16, "Q"),
    ('R' as u16, "R"),
    ('S' as u16, "S"),
    ('T' as u16, "T"),
    ('U' as u16, "U"),
    ('V' as u16, "V"),
    ('W' as u16, "W"),
    ('X' as u16, "X"),
    ('Y' as u16, "Y"),
    ('Z' as u16, "Z"),
    (vk::OEM_MINUS, "-"),
    (vk::OEM_7, "^"),
    (vk::OEM_5, "\\"),
    (vk::OEM_3, "@"),
    (vk::OEM_4, "["),
    (vk::OEM_PLUS, ";"),
    (vk::OEM_1, ":"),
    (vk::OEM_6, "]"),
    (vk::OEM_COMMA, ","),
    (vk::OEM_PERIOD, "."),
    (vk::OEM_2, "/"),
    (vk::OEM_102, "＼"),
    (vk::NUMPAD0, "Num0"),
    (vk::NUMPAD1, "Num1"),
    (vk::NUMPAD2, "Num2"),
    (vk::NUMPAD3, "Num3"),
    (vk::NUMPAD4, "Num4"),
    (vk::NUMPAD5, "Num5"),
    (vk::NUMPAD6, "Num6"),
    (vk::NUMPAD7, "Num7"),
    (vk::NUMPAD8, "Num8"),
    (vk::NUMPAD9, "Num9"),
    (vk::MULTIPLY, "Num*"),
    (vk::ADD, "Num+"),
    (vk::SUBTRACT, "Num-"),
    (vk::DECIMAL, "Num."),
    (vk::DIVIDE, "Num/"),
    (vk::F1, "F1"),
    (vk::F2, "F2"),
    (vk::F3, "F3"),
    (vk::F4, "F4"),
    (vk::F5, "F5"),
    (vk::F6, "F6"),
    (vk::F7, "F7"),
    (vk::F8, "F8"),
    (vk::F9, "F9"),
    (vk::F10, "F10"),
    (vk::F11, "F11"),
    (vk::F12, "F12"),
];

/// 仮想キーコードに対応する表示テキストを返す。未登録なら `None`
/// (Accelerator.cpp 287-292 のループに相当)。
pub fn get_key_name(key: u16) -> Option<&'static str> {
    ACCEL_KEY_LIST
        .iter()
        .find(|&&(code, _)| code == key)
        .map(|&(_, text)| text)
}

/// 修飾キー + キーの表示文字列を整形する(Accelerator.cpp 284-302 `FormatAccelText`)。
///
/// 形式は `[Shift+][Ctrl+][Alt+]<キー名>[ (G)]`。未登録キーのキー名は空。
pub fn format_accel_text(key: u16, modifiers: u8, global: bool) -> Vec<u16> {
    let mut s = String::new();
    if modifiers & MOD_SHIFT != 0 {
        s.push_str("Shift+");
    }
    if modifiers & MOD_CONTROL != 0 {
        s.push_str("Ctrl+");
    }
    if modifiers & MOD_ALT != 0 {
        s.push_str("Alt+");
    }
    if let Some(name) = get_key_name(key) {
        s.push_str(name);
    }
    if global {
        s.push_str(" (G)");
    }
    s.encode_utf16().collect()
}

/// 設定項目パラメータを符号化する(Accelerator.cpp 39-41 `MAKE_ACCEL_PARAM`)。
///
/// 配置は `key<<16 | mod<<8 | (global?0x80) | appcommand`。
pub fn make_accel_param(key: u16, modifiers: u8, global: bool, appcommand: u8) -> i64 {
    ((key as i64) << 16)
        | ((modifiers as i64) << 8)
        | (if global { 0x80 } else { 0x00 })
        | (appcommand as i64)
}

/// パラメータからキーコードを取り出す(Accelerator.cpp 42 `GET_ACCEL_KEY`)。
pub fn get_accel_key(param: i64) -> u16 {
    ((param >> 16) & 0xFFFF) as u16
}

/// パラメータから修飾子を取り出す(Accelerator.cpp 43 `GET_ACCEL_MOD`)。
pub fn get_accel_mod(param: i64) -> u8 {
    ((param >> 8) & 0xFF) as u8
}

/// パラメータからグローバルフラグを取り出す(Accelerator.cpp 44 `GET_ACCEL_GLOBAL`)。
pub fn get_accel_global(param: i64) -> bool {
    (param & 0x80) != 0
}

/// パラメータからアプリコマンドを取り出す(Accelerator.cpp 45 `GET_ACCEL_APPCOMMAND`)。
pub fn get_accel_appcommand(param: i64) -> u8 {
    (param & 0x7F) as u8
}

/// `lParam` からアプリコマンド値を取り出す(Win32 `GET_APPCOMMAND_LPARAM`)。
///
/// 上位ワードの最上位ニブル(`FAPPCOMMAND_MASK`=0xF000、デバイス種別)を除いた値。
pub fn get_appcommand_lparam(lparam: i64) -> u16 {
    (((lparam >> 16) & 0xFFFF) as u16) & !0xF000
}

/// メディアキーの種別(Accelerator.h 83-86 `MediaKeyType`)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MediaKeyType {
    AppCommand,
    RawInput,
}

/// キー割り当て情報(Accelerator.h 75-81 `KeyInfo`)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KeyInfo {
    pub command: u16,
    pub key_code: u16,
    pub modifiers: u8,
    pub global: bool,
}

/// アプリコマンド割り当て情報(Accelerator.h 93-98 `AppCommandInfo`)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AppCommandInfo {
    pub command: u16,
    pub type_: MediaKeyType,
    pub app_command: u16,
}

/// ホットキーの `wParam`(=`modifiers<<8 | key_code`)からコマンドを引く
/// (Accelerator.cpp 665-673 `TranslateHotKey`)。該当なしは -1。
pub fn translate_hotkey(key_list: &[KeyInfo], wparam: u32) -> i32 {
    for key in key_list {
        if (((key.modifiers as u32) << 8) | (key.key_code as u32)) == wparam {
            return key.command as i32;
        }
    }
    -1
}

/// アプリコマンド値からコマンドを引く(Accelerator.cpp 676-686 `TranslateAppCommand`)。
/// 該当なしは 0。`MediaKeyType::AppCommand` の項目のみが対象。
pub fn translate_app_command(app_command_list: &[AppCommandInfo], command: u16) -> i32 {
    for info in app_command_list {
        if info.type_ == MediaKeyType::AppCommand && info.app_command == command {
            return info.command as i32;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn key_name_lookup() {
        assert_eq!(get_key_name(vk::F1), Some("F1"));
        assert_eq!(get_key_name('A' as u16), Some("A"));
        assert_eq!(get_key_name('0' as u16), Some("0"));
        assert_eq!(get_key_name(vk::LEFT), Some("←"));
        assert_eq!(get_key_name(vk::OEM_102), Some("＼"));
        assert_eq!(get_key_name(0x00FF), None);
    }

    #[test]
    fn format_text_modifiers_order() {
        // 修飾子は Shift, Ctrl, Alt の順
        assert_eq!(format_accel_text('A' as u16, MOD_CONTROL, false), w("Ctrl+A"));
        assert_eq!(
            format_accel_text('A' as u16, MOD_SHIFT | MOD_CONTROL | MOD_ALT, false),
            w("Shift+Ctrl+Alt+A")
        );
        assert_eq!(format_accel_text(vk::RETURN, MOD_ALT, false), w("Alt+Enter"));
    }

    #[test]
    fn format_text_global_and_unknown() {
        assert_eq!(format_accel_text(vk::F1, 0, true), w("F1 (G)"));
        // 未登録キーはキー名が空
        assert_eq!(format_accel_text(0x00FF, MOD_CONTROL, false), w("Ctrl+"));
        assert_eq!(format_accel_text(0x00FF, 0, false), w(""));
    }

    #[test]
    fn accel_param_roundtrip() {
        let param = make_accel_param(0x0041, MOD_SHIFT | MOD_CONTROL, true, 0x12);
        assert_eq!(get_accel_key(param), 0x0041);
        assert_eq!(get_accel_mod(param), MOD_SHIFT | MOD_CONTROL);
        assert!(get_accel_global(param));
        assert_eq!(get_accel_appcommand(param), 0x12);

        // グローバル無し / アプリコマンド最大(0x7F)
        let param2 = make_accel_param(vk::F12, MOD_ALT, false, 0x7F);
        assert_eq!(get_accel_key(param2), vk::F12);
        assert_eq!(get_accel_mod(param2), MOD_ALT);
        assert!(!get_accel_global(param2));
        assert_eq!(get_accel_appcommand(param2), 0x7F);
    }

    #[test]
    fn appcommand_lparam_extracts_command() {
        // 上位ワード 0x800C(デバイス=8, コマンド=0x0C)
        let lparam: i64 = 0x800C << 16;
        assert_eq!(get_appcommand_lparam(lparam), 0x000C);
    }

    #[test]
    fn translate_hotkey_lookup() {
        let list = [
            KeyInfo { command: 100, key_code: 'A' as u16, modifiers: MOD_CONTROL, global: false },
            KeyInfo { command: 101, key_code: vk::F5, modifiers: 0, global: false },
        ];
        // wParam = modifiers<<8 | key_code
        let wparam = ((MOD_CONTROL as u32) << 8) | ('A' as u32);
        assert_eq!(translate_hotkey(&list, wparam), 100);
        assert_eq!(translate_hotkey(&list, vk::F5 as u32), 101);
        // 該当なし
        assert_eq!(translate_hotkey(&list, 0x9999), -1);
    }

    #[test]
    fn translate_app_command_lookup() {
        let list = [
            AppCommandInfo { command: 200, type_: MediaKeyType::AppCommand, app_command: 5 },
            // RawInput 種別は対象外
            AppCommandInfo { command: 201, type_: MediaKeyType::RawInput, app_command: 6 },
        ];
        assert_eq!(translate_app_command(&list, 5), 200);
        assert_eq!(translate_app_command(&list, 6), 0); // RawInput は照合されない
        assert_eq!(translate_app_command(&list, 99), 0); // 未知
    }
}
