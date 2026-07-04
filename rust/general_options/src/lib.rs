//! TVTest の全般設定(`src/GeneralOptions.cpp` / `GeneralOptions.h`)の純粋ロジックを
//! 移植したクレート。
//!
//! 移植対象:
//! - 既定使用 BonDriver の種別(`DefaultDriverType`)と範囲検証(`CheckEnumRange`、
//!   GeneralOptions.cpp:72-74)。
//! - `DefaultDriverType` に応じて最初に使う BonDriver 名を決定する `GetFirstDriverName`
//!   (GeneralOptions.cpp:138-154)。
//! - 常駐化/ワンセグフォールバック更新用のビットフラグ定数(`UPDATE_RESIDENT` /
//!   `UPDATE_1SEGFALLBACK`、GeneralOptions.h:71-74)。
//!
//! 対象外(Win32 / CSettings / DriverManager / CoreEngine 依存):
//! - `CGeneralOptions::DlgProc`(ダイアログ)。
//! - `ReadSettings` / `WriteSettings` の `CSettings` I/O 本体。
//! - `Apply`(`DriverManager` / `CoreEngine`(`TSPacketParserFilter`)連携)。
//! - フォルダ選択時のファイルパス解決処理(`PathIsRelative` / `PathAppend` /
//!   `PathCanonicalize`)。

#![forbid(unsafe_code)]

/// 既定使用 BonDriver の種別(GeneralOptions.h:35-40 の `DefaultDriverType`)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefaultDriverType {
    /// 既定使用 BonDriver を指定しない。
    None = 0,
    /// 最後に使用した BonDriver を使う。
    Last = 1,
    /// 指定した BonDriver を使う。
    Custom = 2,
}

/// `DefaultDriverType` の終端番兵(`TVTEST_ENUM_CLASS_TRAILER`)の値。GeneralOptions.h:39。
pub const DEFAULT_DRIVER_TYPE_TRAILER: i32 = 3;

/// 既定の `DefaultDriverType`(`m_DefaultDriverType` の初期値、GeneralOptions.h:77)。
pub const DEFAULT_DRIVER_TYPE_DEFAULT: DefaultDriverType = DefaultDriverType::Last;

impl DefaultDriverType {
    /// 整数値を `DefaultDriverType` へ変換する(`CheckEnumRange`、GeneralOptions.cpp:72-74)。
    ///
    /// 有効範囲は `0 <= value < TVTEST_ENUM_CLASS_TRAILER`(= 0..=2)。範囲外は `None`。
    /// 原実装では範囲外の場合は既存の値を維持する(呼び出し側で対応する)。
    #[must_use]
    pub fn from_int(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::None),
            1 => Some(Self::Last),
            2 => Some(Self::Custom),
            _ => None,
        }
    }

    /// 整数値へ変換する。
    #[must_use]
    pub const fn to_int(self) -> i32 {
        self as i32
    }
}

/// 常駐化の設定が変更されたことを示す更新フラグ(`UPDATE_RESIDENT`、GeneralOptions.h:72)。
///
/// `Apply` 呼び出し時に `App.UICore.SetResident` を実行すべきかどうかの判定に使う。
pub const UPDATE_RESIDENT: u32 = 0x0000_0001;

/// ワンセグフォールバックの設定が変更されたことを示す更新フラグ
/// (`UPDATE_1SEGFALLBACK`、GeneralOptions.h:73)。
///
/// `Apply` 呼び出し時に `TSPacketParserFilter::SetGenerate1SegPAT` を実行すべきかどうかの
/// 判定に使う。
pub const UPDATE_1SEGFALLBACK: u32 = 0x0000_0002;

/// `DefaultDriverType` に応じて最初に使う BonDriver 名を決定する
/// (`GetFirstDriverName`、GeneralOptions.cpp:138-154)。
///
/// - `None` の場合は空文字列を返す。
/// - `Last` の場合は `last_bon_driver_name` を返す。
/// - `Custom` の場合は `default_bon_driver_name` を返す。
#[must_use]
pub fn get_first_driver_name(
    driver_type: DefaultDriverType,
    default_bon_driver_name: &str,
    last_bon_driver_name: &str,
) -> String {
    match driver_type {
        DefaultDriverType::None => String::new(),
        DefaultDriverType::Last => last_bon_driver_name.to_string(),
        DefaultDriverType::Custom => default_bon_driver_name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_driver_type_from_int_valid_values() {
        assert_eq!(DefaultDriverType::from_int(0), Some(DefaultDriverType::None));
        assert_eq!(DefaultDriverType::from_int(1), Some(DefaultDriverType::Last));
        assert_eq!(DefaultDriverType::from_int(2), Some(DefaultDriverType::Custom));
    }

    #[test]
    fn default_driver_type_from_int_out_of_range_trailer() {
        // 終端番兵ちょうどは範囲外。
        assert_eq!(DefaultDriverType::from_int(DEFAULT_DRIVER_TYPE_TRAILER), None);
        assert_eq!(DefaultDriverType::from_int(3), None);
    }

    #[test]
    fn default_driver_type_from_int_out_of_range_negative() {
        assert_eq!(DefaultDriverType::from_int(-1), None);
    }

    #[test]
    fn default_driver_type_round_trip() {
        for driver_type in [
            DefaultDriverType::None,
            DefaultDriverType::Last,
            DefaultDriverType::Custom,
        ] {
            assert_eq!(DefaultDriverType::from_int(driver_type.to_int()), Some(driver_type));
        }
    }

    #[test]
    fn default_driver_type_default_is_last() {
        assert_eq!(DEFAULT_DRIVER_TYPE_DEFAULT, DefaultDriverType::Last);
        assert_eq!(DEFAULT_DRIVER_TYPE_DEFAULT.to_int(), 1);
    }

    #[test]
    fn get_first_driver_name_none_returns_empty() {
        assert_eq!(
            get_first_driver_name(DefaultDriverType::None, "Custom.dll", "Last.dll"),
            ""
        );
    }

    #[test]
    fn get_first_driver_name_last_returns_last_driver() {
        assert_eq!(
            get_first_driver_name(DefaultDriverType::Last, "Custom.dll", "Last.dll"),
            "Last.dll"
        );
    }

    #[test]
    fn get_first_driver_name_custom_returns_default_driver() {
        assert_eq!(
            get_first_driver_name(DefaultDriverType::Custom, "Custom.dll", "Last.dll"),
            "Custom.dll"
        );
    }

    #[test]
    fn update_flags_are_distinct_bits() {
        assert_eq!(UPDATE_RESIDENT, 0x0000_0001);
        assert_eq!(UPDATE_1SEGFALLBACK, 0x0000_0002);
        assert_eq!(UPDATE_RESIDENT & UPDATE_1SEGFALLBACK, 0);
    }
}
