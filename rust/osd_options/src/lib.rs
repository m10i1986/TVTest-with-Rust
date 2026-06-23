//! TVTest の OSD 設定(`src/OSDOptions.cpp` / `OSDOptions.h`)の純粋ロジックを
//! 移植したクレート。
//!
//! 移植対象:
//! - OSD 種別(`OSDType`)とそのビットフラグ(`OSD_FLAG`、OSDOptions.cpp:42)。
//! - チャンネル変更時の表示種別(`ChannelChangeType`)と範囲検証(`CheckEnumRange`)。
//! - 設定値 `EnabledOSD` / `EnabledOSDMask` のマスクマージ(`ReadSettings`、OSDOptions.cpp:124-135)。
//!   項目が増えたときに新項目へ既定値を反映するための互換ロジック。
//! - OSD 有効判定(`IsOSDEnabled`、OSDOptions.cpp:210-213)。
//! - レイヤードウィンドウ判定(`GetLayeredWindow`、OSDOptions.cpp:196-199)。
//! - フェード時間の秒⇔ミリ秒変換(`DlgProc`、OSDOptions.cpp:226 / 327)。
//!
//! 対象外(Win32 / DirectShow / CSettings / Style 依存):
//! - `COSDOptions::DlgProc`(ダイアログ)・`ChooseFontNoSize`(フォント選択)。
//! - `ReadSettings` / `WriteSettings` の `CSettings` I/O 本体(本クレートは値の変換のみ提供)。
//! - `CAeroGlass` による DWM コンポジション状態の取得(`m_fCompositionEnabled` は呼び出し側が注入)。
//! - フォント(`LOGFONT` / `Style::Font`)・色・不透明度などの単純な値の読み書き。

#![forbid(unsafe_code)]

/// OSD 種別(OSDOptions.h:43-50 の `OSDType`)。`OSD_FLAG` のビット位置を兼ねる。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OsdType {
    /// チャンネル。
    Channel = 0,
    /// 音量。
    Volume = 1,
    /// 音声。
    Audio = 2,
    /// 録画。
    Recording = 3,
    /// チャンネル番号入力なし。
    ChannelNoInput = 4,
}

impl OsdType {
    /// OSD 種別のビットフラグ(`OSD_FLAG(type) = 1U << type`、OSDOptions.cpp:42)。
    #[must_use]
    pub const fn flag(self) -> u32 {
        1u32 << (self as u32)
    }
}

/// `TVTEST_ENUM_CLASS_TRAILER`(`OSDType` の終端番兵)の値。OSDOptions.h:49。
///
/// `OSDType` の最後の要素 `ChannelNoInput` の次の値で、現在の種別数に等しい。
pub const OSD_TYPE_TRAILER: u32 = 5;

/// 既定で有効な OSD(コンストラクタ、OSDOptions.cpp:87-89)。
///
/// `OSD_FLAG(Channel) | OSD_FLAG(Volume) | OSD_FLAG(ChannelNoInput)` = `0x13`。
pub const DEFAULT_ENABLED_OSD: u32 =
    OsdType::Channel.flag() | OsdType::Volume.flag() | OsdType::ChannelNoInput.flag();

/// `WriteSettings` が保存する現行マスク(OSDOptions.cpp:170)。
///
/// `OSD_FLAG(TVTEST_ENUM_CLASS_TRAILER) - 1`。全種別ぶんの下位ビットが立つ(`0x1F`)。
pub const WRITE_ENABLED_OSD_MASK: u32 = (1u32 << OSD_TYPE_TRAILER) - 1;

/// マスクが未保存(または 0)のときに使う既定マスク(OSDOptions.cpp:130)。
///
/// `OSD_FLAG(OSDType::ChannelNoInput) - 1` = `0x0F`。`ChannelNoInput` を追加する前の
/// 旧バージョンが保存した `EnabledOSD` を想定した、互換用の旧マスク。
pub const LEGACY_ENABLED_OSD_MASK: u32 = OsdType::ChannelNoInput.flag() - 1;

/// 保存された `EnabledOSD` を現在の既定値へマージする(`ReadSettings`、OSDOptions.cpp:124-135)。
///
/// - `saved_enabled`: 設定 `EnabledOSD` から読み込んだ値。
/// - `saved_mask`: 設定 `EnabledOSDMask` から読み込んだ値。未保存なら `None`、`Some(0)` も
///   未保存と同じ扱い(OSDOptions.cpp:128-130)。
/// - `current`: マージ前の現在値(通常は [`DEFAULT_ENABLED_OSD`])。
///
/// マスク外にビットが立っていなければ「マスク内は保存値・マスク外は現在値」を採り、新しく
/// 追加された項目(マスク外)には現在の既定値を反映する。マスク外にビットが立っていれば
/// 保存値をそのまま採用する。
#[must_use]
pub fn merge_enabled_osd(saved_enabled: u32, saved_mask: Option<u32>, current: u32) -> u32 {
    let mask = match saved_mask {
        Some(m) if m != 0 => m,
        _ => LEGACY_ENABLED_OSD_MASK,
    };
    if saved_enabled & !mask == 0 {
        (saved_enabled & mask) | (current & !mask)
    } else {
        saved_enabled
    }
}

/// 指定種別の OSD が有効か(`IsOSDEnabled`、OSDOptions.cpp:210-213)。
///
/// `m_fShowOSD && (m_EnabledOSD & OSD_FLAG(Type)) != 0`。
#[must_use]
pub fn is_osd_enabled(show_osd: bool, enabled_osd: u32, osd_type: OsdType) -> bool {
    show_osd && (enabled_osd & osd_type.flag()) != 0
}

/// レイヤードウィンドウを使うか(`GetLayeredWindow`、OSDOptions.cpp:196-199)。
///
/// `m_fLayeredWindow && m_fCompositionEnabled`。
#[must_use]
pub fn get_layered_window(layered_window: bool, composition_enabled: bool) -> bool {
    layered_window && composition_enabled
}

/// フェード時間(ミリ秒)をダイアログ表示用の秒へ変換する(OSDOptions.cpp:226)。
///
/// `m_FadeTime / 1000`(整数除算で端数切り捨て)。
#[must_use]
pub fn fade_time_to_display_seconds(fade_time_ms: i32) -> i32 {
    fade_time_ms / 1000
}

/// ダイアログの秒入力をフェード時間(ミリ秒)へ変換する(OSDOptions.cpp:327)。
///
/// `Seconds * 1000`。
#[must_use]
pub fn display_seconds_to_fade_time(seconds: i32) -> i32 {
    seconds * 1000
}

/// チャンネル変更時の OSD 表示種別(OSDOptions.h:36-41 の `ChannelChangeType`)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelChangeType {
    /// ロゴとテキスト。
    LogoAndText = 0,
    /// テキストのみ。
    TextOnly = 1,
    /// ロゴのみ。
    LogoOnly = 2,
}

/// `ChannelChangeType` の終端番兵(`TVTEST_ENUM_CLASS_TRAILER`)の値。OSDOptions.h:40。
pub const CHANNEL_CHANGE_TYPE_TRAILER: i32 = 3;

impl ChannelChangeType {
    /// 整数値を `ChannelChangeType` へ変換する(`CheckEnumRange`、OSDOptions.cpp:136-138)。
    ///
    /// 有効範囲は `0 <= value < TVTEST_ENUM_CLASS_TRAILER`(= 0..=2)。範囲外は `None`。
    #[must_use]
    pub fn from_int(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::LogoAndText),
            1 => Some(Self::TextOnly),
            2 => Some(Self::LogoOnly),
            _ => None,
        }
    }

    /// 整数値へ変換する。
    #[must_use]
    pub const fn to_int(self) -> i32 {
        self as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osd_flag_values() {
        // OSD_FLAG(type) = 1 << type。
        assert_eq!(OsdType::Channel.flag(), 0x01);
        assert_eq!(OsdType::Volume.flag(), 0x02);
        assert_eq!(OsdType::Audio.flag(), 0x04);
        assert_eq!(OsdType::Recording.flag(), 0x08);
        assert_eq!(OsdType::ChannelNoInput.flag(), 0x10);
        // 終端番兵のフラグは 0x20。
        assert_eq!(1u32 << OSD_TYPE_TRAILER, 0x20);
    }

    #[test]
    fn default_and_mask_constants() {
        // 既定: Channel | Volume | ChannelNoInput = 0x13。
        assert_eq!(DEFAULT_ENABLED_OSD, 0x13);
        // 現行マスク = 0x1F(全 5 種別)。
        assert_eq!(WRITE_ENABLED_OSD_MASK, 0x1F);
        // 旧マスク = 0x0F(ChannelNoInput 追加前)。
        assert_eq!(LEGACY_ENABLED_OSD_MASK, 0x0F);
    }

    #[test]
    fn merge_legacy_config_applies_default_for_new_item() {
        // 旧バージョン(ChannelNoInput 追加前)が Channel のみ保存し、マスクは未保存。
        // 旧マスク 0x0F の外(ChannelNoInput=0x10)には現在の既定値が反映される。
        let merged = merge_enabled_osd(0x01, None, DEFAULT_ENABLED_OSD);
        // (0x01 & 0x0F) | (0x13 & ~0x0F) = 0x01 | 0x10 = 0x11。
        assert_eq!(merged, 0x11);
    }

    #[test]
    fn merge_mask_zero_treated_as_unset() {
        // マスク 0 は未保存と同じ扱い(旧マスク 0x0F を使う)。
        let merged = merge_enabled_osd(0x02, Some(0), DEFAULT_ENABLED_OSD);
        // (0x02 & 0x0F) | (0x13 & ~0x0F) = 0x02 | 0x10 = 0x12。
        assert_eq!(merged, 0x12);
    }

    #[test]
    fn merge_with_current_mask_keeps_saved() {
        // 現行マスク 0x1F が保存されている場合、全種別が保存値で確定する。
        let merged = merge_enabled_osd(0x13, Some(WRITE_ENABLED_OSD_MASK), DEFAULT_ENABLED_OSD);
        // (0x13 & 0x1F) | (0x13 & ~0x1F) = 0x13。
        assert_eq!(merged, 0x13);

        // すべて無効で保存しても、マスク内は保存値が優先され既定値で上書きされない。
        let merged = merge_enabled_osd(0x00, Some(WRITE_ENABLED_OSD_MASK), DEFAULT_ENABLED_OSD);
        assert_eq!(merged, 0x00);
    }

    #[test]
    fn merge_bits_outside_mask_uses_saved_as_is() {
        // マスク外にビットが立っていれば保存値をそのまま採用する。
        let merged = merge_enabled_osd(0x100, Some(WRITE_ENABLED_OSD_MASK), DEFAULT_ENABLED_OSD);
        assert_eq!(merged, 0x100);
        // 旧マスクでもマスク外(0x10 含む)に立っていればそのまま。
        let merged = merge_enabled_osd(0x13, None, DEFAULT_ENABLED_OSD);
        assert_eq!(merged, 0x13);
    }

    #[test]
    fn is_osd_enabled_basic() {
        // 表示オフなら常に無効。
        assert!(!is_osd_enabled(
            false,
            DEFAULT_ENABLED_OSD,
            OsdType::Channel
        ));
        // 既定で有効な種別。
        assert!(is_osd_enabled(true, DEFAULT_ENABLED_OSD, OsdType::Channel));
        assert!(is_osd_enabled(true, DEFAULT_ENABLED_OSD, OsdType::Volume));
        assert!(is_osd_enabled(
            true,
            DEFAULT_ENABLED_OSD,
            OsdType::ChannelNoInput
        ));
        // 既定で無効な種別。
        assert!(!is_osd_enabled(true, DEFAULT_ENABLED_OSD, OsdType::Audio));
        assert!(!is_osd_enabled(
            true,
            DEFAULT_ENABLED_OSD,
            OsdType::Recording
        ));
    }

    #[test]
    fn get_layered_window_basic() {
        assert!(get_layered_window(true, true));
        assert!(!get_layered_window(true, false));
        assert!(!get_layered_window(false, true));
        assert!(!get_layered_window(false, false));
    }

    #[test]
    fn fade_time_conversions() {
        // ms → 秒は端数切り捨て。
        assert_eq!(fade_time_to_display_seconds(3000), 3);
        assert_eq!(fade_time_to_display_seconds(3999), 3);
        // 秒 → ms。
        assert_eq!(display_seconds_to_fade_time(3), 3000);
        // 既定 3000ms は 3 秒で往復一致。
        assert_eq!(
            display_seconds_to_fade_time(fade_time_to_display_seconds(3000)),
            3000
        );
    }

    #[test]
    fn channel_change_type_from_int() {
        assert_eq!(
            ChannelChangeType::from_int(0),
            Some(ChannelChangeType::LogoAndText)
        );
        assert_eq!(
            ChannelChangeType::from_int(1),
            Some(ChannelChangeType::TextOnly)
        );
        assert_eq!(
            ChannelChangeType::from_int(2),
            Some(ChannelChangeType::LogoOnly)
        );
        // 範囲外(終端番兵以上・負値)は None。
        assert_eq!(
            ChannelChangeType::from_int(CHANNEL_CHANGE_TYPE_TRAILER),
            None
        );
        assert_eq!(ChannelChangeType::from_int(3), None);
        assert_eq!(ChannelChangeType::from_int(-1), None);
    }

    #[test]
    fn channel_change_type_round_trip() {
        for ty in [
            ChannelChangeType::LogoAndText,
            ChannelChangeType::TextOnly,
            ChannelChangeType::LogoOnly,
        ] {
            assert_eq!(ChannelChangeType::from_int(ty.to_int()), Some(ty));
        }
    }
}
