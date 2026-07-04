//! TVTest のドライバ設定(`src/DriverOptions.cpp` / `DriverOptions.h`)の純粋ロジックを
//! 移植したクレート。
//!
//! 移植対象:
//! - `DRIVER_FLAG_*` フラグ定数(DriverOptions.cpp:36-44)。
//! - `CDriverSettings` 内の `enum`(初期チャンネル種別、DriverOptions.h:72-76)に相当する
//!   [`InitialChannelType`]。
//! - `CDriverSettings::SetInitialChannelType`(DriverOptions.cpp:112-118)相当の範囲検証。
//! - `BonDriverOptions` の既定値(DriverOptions.h:70-74)に相当する [`BonDriverFlags`]。
//! - `CDriverOptions::ReadSettings` の `Driver{i}_Options` / `Driver{i}_OptionsMask` の
//!   フラグデコード部分(DriverOptions.cpp:241-259)に相当する [`decode_driver_flags`]。
//! - `CDriverOptions::WriteSettings` のフラグエンコード部分(DriverOptions.cpp:309-323)に
//!   相当する [`encode_driver_flags`]。
//! - `Driver{i}_LastStatus` のデコード/エンコード(DriverOptions.cpp:279-280, 339)に相当する
//!   [`decode_last_all_channels`] / [`encode_last_all_channels`]。
//! - `CDriverOptions::GetInitialChannel`(DriverOptions.cpp:361-398)に相当する
//!   [`get_initial_channel`]。
//!
//! 対象外(Win32 / CSettings / 呼び出し側オブジェクト管理に依存):
//! - `CDriverOptions::DlgProc`(ダイアログ)。
//! - `ReadSettings` / `WriteSettings` の `CSettings` I/O 本体(本クレートは値の変換のみ提供)。
//! - `CDriverSettingList` / `CDriverSettings` 自体のオブジェクト管理(コピー・生成・破棄)。
//! - `CDriverSettingList::Find` が用いる `IsEqualFileName` によるファイル名検索。
//! - `CDriverManager` との連携(`Initialize`、`BonDriverOptions(LPCTSTR)` コンストラクタ等)。

#![forbid(unsafe_code)]

/// シグナルレベルを表示しない(DriverOptions.cpp:36)。
pub const DRIVER_FLAG_NOSIGNALLEVEL: u32 = 0x0000_0001;

/// チャンネル切り替え時にストリームをパージする(DriverOptions.cpp:37)。
pub const DRIVER_FLAG_PURGESTREAMONCHANNELCHANGE: u32 = 0x0000_0004;

/// 全チャンネルを対象とする(DriverOptions.cpp:38)。
pub const DRIVER_FLAG_ALLCHANNELS: u32 = 0x0000_0008;

/// チャンネル切り替えエラーカウントをリセットする(DriverOptions.cpp:39)。
pub const DRIVER_FLAG_RESETCHANNELCHANGEERRORCOUNT: u32 = 0x0000_0010;

/// 初期ストリームを無視しない(否定フラグ、DriverOptions.cpp:40)。
pub const DRIVER_FLAG_NOTIGNOREINITIALSTREAM: u32 = 0x0000_0020;

/// ストリームの同期再生を待ち合わせる(DriverOptions.cpp:41)。
pub const DRIVER_FLAG_PUMPSTREAMSYNCPLAYBACK: u32 = 0x0000_0040;

/// 全フラグビットのマスク(DriverOptions.cpp:43)。
pub const DRIVER_FLAG_MASK: u32 = 0x0000_007F;

/// `Driver{i}_OptionsMask` が読み込めない場合に使う既定マスク(DriverOptions.cpp:44)。
pub const DRIVER_FLAG_DEFAULTMASK: u32 = 0x0000_003F;

/// 初期チャンネルの種別(`CDriverSettings` 内の `enum`、DriverOptions.h:72-76)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitialChannelType {
    /// 初期チャンネル指定なし(前回のチャンネルには合わせない)。
    None = 0,
    /// 前回終了時のチャンネルを使う。
    Last = 1,
    /// カスタム指定したチャンネルを使う。
    Custom = 2,
}

impl InitialChannelType {
    /// `i32` から変換する。
    ///
    /// 原実装(`CDriverSettings::SetInitialChannelType`、DriverOptions.cpp:112-118):
    /// ```cpp
    /// if (Type < INITIALCHANNEL_NONE || Type > INITIALCHANNEL_CUSTOM)
    ///     return false;
    /// m_InitialChannelType = Type;
    /// return true;
    /// ```
    /// 範囲外(`0..=2` の外)であれば `None`(Rust の `Option::None`)を返す。
    #[must_use]
    pub const fn from_int(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::None),
            1 => Some(Self::Last),
            2 => Some(Self::Custom),
            _ => None,
        }
    }

    /// `i32` へ変換する。
    #[must_use]
    pub const fn to_int(self) -> i32 {
        self as i32
    }
}

/// `BonDriverOptions` のフラグ部分(DriverOptions.h:68-80)に相当する構造体。
///
/// `FirstChannelSetDelay` / `MinChannelChangeInterval`(DWORD の遅延時間設定)は
/// フラグデコード/エンコードと無関係な単純な値渡しのため、本クレートの対象外とする。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BonDriverFlags {
    /// シグナルレベルを表示しない(既定値 `false`、DriverOptions.h:70)。
    pub no_signal_level: bool,
    /// 初期ストリームを無視する(既定値 `true`、DriverOptions.h:71)。
    pub ignore_initial_stream: bool,
    /// チャンネル切り替え時にストリームをパージする(既定値 `true`、DriverOptions.h:72)。
    pub purge_stream_on_channel_change: bool,
    /// チャンネル切り替えエラーカウントをリセットする(既定値 `true`、DriverOptions.h:73)。
    pub reset_channel_change_error_count: bool,
    /// ストリームの同期再生を待ち合わせる(既定値 `false`、DriverOptions.h:74)。
    pub pump_stream_sync_playback: bool,
}

impl Default for BonDriverFlags {
    /// 原実装 `BonDriverOptions() = default;`(DriverOptions.h:70-78)の既定値に一致する。
    fn default() -> Self {
        Self {
            no_signal_level: false,
            ignore_initial_stream: true,
            purge_stream_on_channel_change: true,
            reset_channel_change_error_count: true,
            pump_stream_sync_playback: false,
        }
    }
}

/// `Driver{i}_Options` / `Driver{i}_OptionsMask` からフラグをデコードする。
///
/// `mask` で指定されたビットに対応するフラグのみ `value` の内容で上書きし、
/// それ以外のフラグは `current` の値をそのまま維持する。
///
/// 原実装(`CDriverOptions::ReadSettings`、DriverOptions.cpp:241-259):
/// ```cpp
/// if ((Mask & DRIVER_FLAG_NOSIGNALLEVEL) != 0)
///     pSettings->SetNoSignalLevel((Value & DRIVER_FLAG_NOSIGNALLEVEL) != 0);
/// if ((Mask & DRIVER_FLAG_PURGESTREAMONCHANNELCHANGE) != 0)
///     pSettings->SetPurgeStreamOnChannelChange((Value & DRIVER_FLAG_PURGESTREAMONCHANNELCHANGE) != 0);
/// if ((Mask & DRIVER_FLAG_ALLCHANNELS) != 0)
///     pSettings->SetAllChannels((Value & DRIVER_FLAG_ALLCHANNELS) != 0);
/// if ((Mask & DRIVER_FLAG_RESETCHANNELCHANGEERRORCOUNT) != 0)
///     pSettings->SetResetChannelChangeErrorCount((Value & DRIVER_FLAG_RESETCHANNELCHANGEERRORCOUNT) != 0);
/// if ((Mask & DRIVER_FLAG_NOTIGNOREINITIALSTREAM) != 0)
///     pSettings->SetIgnoreInitialStream((Value & DRIVER_FLAG_NOTIGNOREINITIALSTREAM) == 0);
/// if ((Mask & DRIVER_FLAG_PUMPSTREAMSYNCPLAYBACK) != 0)
///     pSettings->SetPumpStreamSyncPlayback((Value & DRIVER_FLAG_PUMPSTREAMSYNCPLAYBACK) != 0);
/// ```
///
/// `DRIVER_FLAG_ALLCHANNELS` は `BonDriverOptions` ではなく `CDriverSettings::m_fAllChannels`
/// (`GetAllChannels`/`SetAllChannels`)に対応するフラグだが、原実装と同じデコード処理を
/// 適用できるよう、本関数の戻り値には含めず [`decode_all_channels_flag`] で別途扱う。
#[must_use]
pub fn decode_driver_flags(current: BonDriverFlags, value: u32, mask: u32) -> BonDriverFlags {
    let mut result = current;

    if (mask & DRIVER_FLAG_NOSIGNALLEVEL) != 0 {
        result.no_signal_level = (value & DRIVER_FLAG_NOSIGNALLEVEL) != 0;
    }
    if (mask & DRIVER_FLAG_PURGESTREAMONCHANNELCHANGE) != 0 {
        result.purge_stream_on_channel_change = (value & DRIVER_FLAG_PURGESTREAMONCHANNELCHANGE) != 0;
    }
    if (mask & DRIVER_FLAG_RESETCHANNELCHANGEERRORCOUNT) != 0 {
        result.reset_channel_change_error_count =
            (value & DRIVER_FLAG_RESETCHANNELCHANGEERRORCOUNT) != 0;
    }
    if (mask & DRIVER_FLAG_NOTIGNOREINITIALSTREAM) != 0 {
        // 否定フラグ: ビットが立っていれば「初期ストリームを無視しない」
        // = ignore_initial_stream = false。
        result.ignore_initial_stream = (value & DRIVER_FLAG_NOTIGNOREINITIALSTREAM) == 0;
    }
    if (mask & DRIVER_FLAG_PUMPSTREAMSYNCPLAYBACK) != 0 {
        result.pump_stream_sync_playback = (value & DRIVER_FLAG_PUMPSTREAMSYNCPLAYBACK) != 0;
    }

    result
}

/// `DRIVER_FLAG_ALLCHANNELS` ビットをデコードする(`CDriverSettings::m_fAllChannels` 相当)。
///
/// `mask` に `DRIVER_FLAG_ALLCHANNELS` が含まれない場合は `current` を維持する。
///
/// 原実装(`CDriverOptions::ReadSettings`、DriverOptions.cpp:251-252):
/// ```cpp
/// if ((Mask & DRIVER_FLAG_ALLCHANNELS) != 0)
///     pSettings->SetAllChannels((Value & DRIVER_FLAG_ALLCHANNELS) != 0);
/// ```
#[must_use]
pub const fn decode_all_channels_flag(current: bool, value: u32, mask: u32) -> bool {
    if (mask & DRIVER_FLAG_ALLCHANNELS) != 0 {
        (value & DRIVER_FLAG_ALLCHANNELS) != 0
    } else {
        current
    }
}

/// フラグ群から `Driver{i}_Options` の値(`Flags`)をエンコードする。
///
/// `all_channels` は `BonDriverFlags` に含まれない(`CDriverSettings::m_fAllChannels` に
/// 対応する)ため、別引数として受け取る。
///
/// 原実装(`CDriverOptions::WriteSettings`、DriverOptions.cpp:309-323):
/// ```cpp
/// int Flags = 0;
/// if (pSettings->GetNoSignalLevel())
///     Flags |= DRIVER_FLAG_NOSIGNALLEVEL;
/// if (pSettings->GetPurgeStreamOnChannelChange())
///     Flags |= DRIVER_FLAG_PURGESTREAMONCHANNELCHANGE;
/// if (pSettings->GetAllChannels())
///     Flags |= DRIVER_FLAG_ALLCHANNELS;
/// if (pSettings->GetResetChannelChangeErrorCount())
///     Flags |= DRIVER_FLAG_RESETCHANNELCHANGEERRORCOUNT;
/// if (!pSettings->GetIgnoreInitialStream())
///     Flags |= DRIVER_FLAG_NOTIGNOREINITIALSTREAM;
/// if (pSettings->GetPumpStreamSyncPlayback())
///     Flags |= DRIVER_FLAG_PUMPSTREAMSYNCPLAYBACK;
/// ```
#[must_use]
pub const fn encode_driver_flags(flags: &BonDriverFlags, all_channels: bool) -> u32 {
    let mut result = 0u32;

    if flags.no_signal_level {
        result |= DRIVER_FLAG_NOSIGNALLEVEL;
    }
    if flags.purge_stream_on_channel_change {
        result |= DRIVER_FLAG_PURGESTREAMONCHANNELCHANGE;
    }
    if all_channels {
        result |= DRIVER_FLAG_ALLCHANNELS;
    }
    if flags.reset_channel_change_error_count {
        result |= DRIVER_FLAG_RESETCHANNELCHANGEERRORCOUNT;
    }
    if !flags.ignore_initial_stream {
        // 否定フラグ: 無視しない(ignore_initial_stream = false)ならビットを立てる。
        result |= DRIVER_FLAG_NOTIGNOREINITIALSTREAM;
    }
    if flags.pump_stream_sync_playback {
        result |= DRIVER_FLAG_PUMPSTREAMSYNCPLAYBACK;
    }

    result
}

/// `Driver{i}_LastStatus` から `fLastAllChannels` をデコードする。
///
/// 原実装(DriverOptions.cpp:279-280):
/// `pSettings->m_fLastAllChannels = (Value & 1) != 0;`
#[must_use]
pub const fn decode_last_all_channels(value: u32) -> bool {
    (value & 1) != 0
}

/// `fLastAllChannels` から `Driver{i}_LastStatus` の値をエンコードする。
///
/// 原実装(DriverOptions.cpp:339):
/// `Settings.Write(szName, pSettings->m_fLastAllChannels ? 0x01U : 0x00U);`
#[must_use]
pub const fn encode_last_all_channels(value: bool) -> u32 {
    if value {
        0x01
    } else {
        0x00
    }
}

/// `CDriverOptions::ChannelInfo`(DriverOptions.h:59-66)に相当する構造体。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelInfo {
    pub space: i32,
    pub channel: i32,
    pub service_id: i32,
    pub transport_stream_id: i32,
    pub all_channels: bool,
}

/// 前回終了時のチャンネル情報(`CDriverSettings::m_Last*` フィールド、DriverOptions.h:62-66)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LastChannelInfo {
    pub space: i32,
    pub channel: i32,
    pub service_id: i32,
    pub transport_stream_id: i32,
    pub all_channels: bool,
}

/// カスタム指定された初期チャンネル情報(`CDriverSettings::m_Initial*` フィールド、
/// DriverOptions.h:55-58, 85)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CustomChannelInfo {
    pub space: i32,
    pub channel: i32,
    pub service_id: i32,
    pub all_channels: bool,
}

/// 初期チャンネル種別に応じて実際に使用するチャンネル情報を構築する。
///
/// 原実装(`CDriverOptions::GetInitialChannel`、DriverOptions.cpp:361-398)のうち、
/// `switch (pSettings->GetInitialChannelType())` 以降のチャンネル情報構築部分に相当する:
/// ```cpp
/// case CDriverSettings::INITIALCHANNEL_NONE:
///     pChannelInfo->Space = pSettings->m_LastSpace;
///     pChannelInfo->Channel = -1;
///     pChannelInfo->ServiceID = -1;
///     pChannelInfo->TransportStreamID = -1;
///     pChannelInfo->fAllChannels = pSettings->m_fLastAllChannels;
///     return true;
/// case CDriverSettings::INITIALCHANNEL_LAST:
///     pChannelInfo->Space = pSettings->m_LastSpace;
///     pChannelInfo->Channel = pSettings->m_LastChannel;
///     pChannelInfo->ServiceID = pSettings->m_LastServiceID;
///     pChannelInfo->TransportStreamID = pSettings->m_LastTransportStreamID;
///     pChannelInfo->fAllChannels = pSettings->m_fLastAllChannels;
///     return true;
/// case CDriverSettings::INITIALCHANNEL_CUSTOM:
///     pChannelInfo->Space = pSettings->GetInitialSpace();
///     pChannelInfo->Channel = pSettings->GetInitialChannel();
///     pChannelInfo->ServiceID = pSettings->GetInitialServiceID();
///     pChannelInfo->TransportStreamID = -1;
///     pChannelInfo->fAllChannels = pSettings->GetAllChannels();
///     return true;
/// ```
#[must_use]
pub const fn get_initial_channel(
    channel_type: InitialChannelType,
    last: &LastChannelInfo,
    custom: &CustomChannelInfo,
) -> ChannelInfo {
    match channel_type {
        InitialChannelType::None => ChannelInfo {
            space: last.space,
            channel: -1,
            service_id: -1,
            transport_stream_id: -1,
            all_channels: last.all_channels,
        },
        InitialChannelType::Last => ChannelInfo {
            space: last.space,
            channel: last.channel,
            service_id: last.service_id,
            transport_stream_id: last.transport_stream_id,
            all_channels: last.all_channels,
        },
        InitialChannelType::Custom => ChannelInfo {
            space: custom.space,
            channel: custom.channel,
            service_id: custom.service_id,
            transport_stream_id: -1,
            all_channels: custom.all_channels,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- InitialChannelType::from_int ---

    #[test]
    fn from_int_none() {
        assert_eq!(InitialChannelType::from_int(0), Some(InitialChannelType::None));
    }

    #[test]
    fn from_int_last() {
        assert_eq!(InitialChannelType::from_int(1), Some(InitialChannelType::Last));
    }

    #[test]
    fn from_int_custom() {
        assert_eq!(InitialChannelType::from_int(2), Some(InitialChannelType::Custom));
    }

    #[test]
    fn from_int_below_range() {
        assert_eq!(InitialChannelType::from_int(-1), None);
    }

    #[test]
    fn from_int_above_range() {
        assert_eq!(InitialChannelType::from_int(3), None);
    }

    #[test]
    fn to_int_round_trip() {
        for value in 0..=2 {
            let ty = InitialChannelType::from_int(value).unwrap();
            assert_eq!(ty.to_int(), value);
        }
    }

    // --- BonDriverFlags::default ---

    #[test]
    fn default_flags_match_cpp_defaults() {
        let flags = BonDriverFlags::default();
        assert!(!flags.no_signal_level);
        assert!(flags.ignore_initial_stream);
        assert!(flags.purge_stream_on_channel_change);
        assert!(flags.reset_channel_change_error_count);
        assert!(!flags.pump_stream_sync_playback);
    }

    // --- decode_driver_flags ---

    #[test]
    fn decode_no_signal_level_with_mask_set() {
        let current = BonDriverFlags::default();
        let decoded = decode_driver_flags(current, DRIVER_FLAG_NOSIGNALLEVEL, DRIVER_FLAG_NOSIGNALLEVEL);
        assert!(decoded.no_signal_level);
    }

    #[test]
    fn decode_no_signal_level_without_mask_keeps_current() {
        let current = BonDriverFlags {
            no_signal_level: true,
            ..BonDriverFlags::default()
        };
        // value にビットが立っていても mask に含まれなければ変更しない。
        let decoded = decode_driver_flags(current, DRIVER_FLAG_NOSIGNALLEVEL, 0);
        assert!(decoded.no_signal_level);

        let current2 = BonDriverFlags {
            no_signal_level: false,
            ..BonDriverFlags::default()
        };
        let decoded2 = decode_driver_flags(current2, DRIVER_FLAG_NOSIGNALLEVEL, 0);
        assert!(!decoded2.no_signal_level);
    }

    #[test]
    fn decode_purge_stream_with_mask_set_false() {
        let current = BonDriverFlags::default();
        // value にビットが立っていない(purge_stream_on_channel_change = false になる)。
        let decoded = decode_driver_flags(current, 0, DRIVER_FLAG_PURGESTREAMONCHANNELCHANGE);
        assert!(!decoded.purge_stream_on_channel_change);
    }

    #[test]
    fn decode_reset_channel_change_error_count_with_mask() {
        let current = BonDriverFlags::default();
        let decoded = decode_driver_flags(current, 0, DRIVER_FLAG_RESETCHANNELCHANGEERRORCOUNT);
        assert!(!decoded.reset_channel_change_error_count);
    }

    #[test]
    fn decode_pump_stream_sync_playback_with_mask() {
        let current = BonDriverFlags::default();
        let decoded = decode_driver_flags(
            current,
            DRIVER_FLAG_PUMPSTREAMSYNCPLAYBACK,
            DRIVER_FLAG_PUMPSTREAMSYNCPLAYBACK,
        );
        assert!(decoded.pump_stream_sync_playback);
    }

    #[test]
    fn decode_ignore_initial_stream_bit_set_means_false() {
        // NOTIGNOREINITIALSTREAM ビットが立っている => 初期ストリームを無視しない
        // => ignore_initial_stream = false (反転ロジック)。
        let current = BonDriverFlags::default();
        let decoded = decode_driver_flags(
            current,
            DRIVER_FLAG_NOTIGNOREINITIALSTREAM,
            DRIVER_FLAG_NOTIGNOREINITIALSTREAM,
        );
        assert!(!decoded.ignore_initial_stream);
    }

    #[test]
    fn decode_ignore_initial_stream_bit_clear_means_true() {
        // NOTIGNOREINITIALSTREAM ビットが立っていない => 初期ストリームを無視する
        // => ignore_initial_stream = true (反転ロジック)。
        let current = BonDriverFlags {
            ignore_initial_stream: false,
            ..BonDriverFlags::default()
        };
        let decoded = decode_driver_flags(current, 0, DRIVER_FLAG_NOTIGNOREINITIALSTREAM);
        assert!(decoded.ignore_initial_stream);
    }

    #[test]
    fn decode_ignore_initial_stream_without_mask_keeps_current() {
        let current = BonDriverFlags {
            ignore_initial_stream: false,
            ..BonDriverFlags::default()
        };
        // value にビットが立っていなくても mask に含まれなければ変更しない。
        let decoded = decode_driver_flags(current, 0, 0);
        assert!(!decoded.ignore_initial_stream);
    }

    #[test]
    fn decode_missing_mask_bits_leave_other_flags_untouched() {
        let current = BonDriverFlags {
            no_signal_level: true,
            pump_stream_sync_playback: true,
            ..BonDriverFlags::default()
        };
        // mask には NOSIGNALLEVEL しか含まれないため、他のフラグは維持される。
        let decoded = decode_driver_flags(current, 0, DRIVER_FLAG_NOSIGNALLEVEL);
        assert!(!decoded.no_signal_level);
        assert!(decoded.purge_stream_on_channel_change); // 既定値 true のまま
        assert!(decoded.reset_channel_change_error_count); // 既定値 true のまま
        assert!(decoded.ignore_initial_stream); // 既定値 true のまま
        assert!(decoded.pump_stream_sync_playback); // 変更されず true のまま
    }

    #[test]
    fn decode_default_mask_applies_all_but_pumpstreamsyncplayback_and_allchannels() {
        // DRIVER_FLAG_DEFAULTMASK (0x3F) には PUMPSTREAMSYNCPLAYBACK(0x40) は含まれない。
        let current = BonDriverFlags::default();
        let value = DRIVER_FLAG_PUMPSTREAMSYNCPLAYBACK; // マスク外のビット
        let decoded = decode_driver_flags(current, value, DRIVER_FLAG_DEFAULTMASK);
        // マスクに PUMPSTREAMSYNCPLAYBACK が含まれないので変化しない。
        assert!(!decoded.pump_stream_sync_playback);
    }

    // --- decode_all_channels_flag ---

    #[test]
    fn decode_all_channels_flag_with_mask_true() {
        assert!(decode_all_channels_flag(
            false,
            DRIVER_FLAG_ALLCHANNELS,
            DRIVER_FLAG_ALLCHANNELS
        ));
    }

    #[test]
    fn decode_all_channels_flag_with_mask_false() {
        assert!(!decode_all_channels_flag(
            true,
            0,
            DRIVER_FLAG_ALLCHANNELS
        ));
    }

    #[test]
    fn decode_all_channels_flag_without_mask_keeps_current() {
        assert!(decode_all_channels_flag(true, 0, 0));
        assert!(!decode_all_channels_flag(false, DRIVER_FLAG_ALLCHANNELS, 0));
    }

    // --- encode_driver_flags ---

    #[test]
    fn encode_default_flags() {
        let flags = BonDriverFlags::default();
        // 既定値: no_signal_level=false, purge=true, reset=true,
        // ignore_initial_stream=true(反転ビットは立たない), pump=false。
        let encoded = encode_driver_flags(&flags, false);
        assert_eq!(
            encoded,
            DRIVER_FLAG_PURGESTREAMONCHANNELCHANGE | DRIVER_FLAG_RESETCHANNELCHANGEERRORCOUNT
        );
    }

    #[test]
    fn encode_all_flags_set() {
        let flags = BonDriverFlags {
            no_signal_level: true,
            ignore_initial_stream: false, // -> NOTIGNOREINITIALSTREAM ビットが立つ
            purge_stream_on_channel_change: true,
            reset_channel_change_error_count: true,
            pump_stream_sync_playback: true,
        };
        let encoded = encode_driver_flags(&flags, true);
        // DRIVER_FLAG_MASK(0x7F) には未使用の 0x02 ビットが含まれるため、
        // 全フラグを立てた場合の値は DRIVER_FLAG_MASK から 0x02 を除いたものになる。
        assert_eq!(encoded, DRIVER_FLAG_MASK & !0x02);
    }

    #[test]
    fn encode_ignore_initial_stream_true_clears_bit() {
        let flags = BonDriverFlags {
            ignore_initial_stream: true,
            ..BonDriverFlags::default()
        };
        let encoded = encode_driver_flags(&flags, false);
        assert_eq!(encoded & DRIVER_FLAG_NOTIGNOREINITIALSTREAM, 0);
    }

    #[test]
    fn encode_ignore_initial_stream_false_sets_bit() {
        let flags = BonDriverFlags {
            ignore_initial_stream: false,
            ..BonDriverFlags::default()
        };
        let encoded = encode_driver_flags(&flags, false);
        assert_eq!(
            encoded & DRIVER_FLAG_NOTIGNOREINITIALSTREAM,
            DRIVER_FLAG_NOTIGNOREINITIALSTREAM
        );
    }

    // --- decode/encode 往復一貫性 ---

    #[test]
    fn decode_then_encode_round_trip_with_full_mask() {
        let original = BonDriverFlags {
            no_signal_level: true,
            ignore_initial_stream: false,
            purge_stream_on_channel_change: false,
            reset_channel_change_error_count: true,
            pump_stream_sync_playback: true,
        };
        let all_channels = true;
        let encoded = encode_driver_flags(&original, all_channels);

        // フルマスクでデコードすれば全ビットが反映され、元の値と一致する。
        let default_current = BonDriverFlags::default();
        let decoded = decode_driver_flags(default_current, encoded, DRIVER_FLAG_MASK);
        let decoded_all_channels = decode_all_channels_flag(false, encoded, DRIVER_FLAG_MASK);

        assert_eq!(decoded, original);
        assert_eq!(decoded_all_channels, all_channels);

        // 再エンコードしても同じ値に戻る。
        let re_encoded = encode_driver_flags(&decoded, decoded_all_channels);
        assert_eq!(re_encoded, encoded);
    }

    #[test]
    fn decode_then_encode_round_trip_with_default_mask() {
        let original = BonDriverFlags {
            no_signal_level: true,
            ignore_initial_stream: true,
            purge_stream_on_channel_change: false,
            reset_channel_change_error_count: false,
            pump_stream_sync_playback: false, // DEFAULTMASK 外なので既定値のまま扱う
        };
        let encoded = encode_driver_flags(&original, false);

        let default_current = BonDriverFlags::default();
        let decoded = decode_driver_flags(default_current, encoded, DRIVER_FLAG_DEFAULTMASK);

        // PUMPSTREAMSYNCPLAYBACK は DEFAULTMASK に含まれないため既定値(false)のまま。
        assert!(!decoded.pump_stream_sync_playback);
        assert_eq!(decoded.no_signal_level, original.no_signal_level);
        assert_eq!(decoded.ignore_initial_stream, original.ignore_initial_stream);
        assert_eq!(
            decoded.purge_stream_on_channel_change,
            original.purge_stream_on_channel_change
        );
        assert_eq!(
            decoded.reset_channel_change_error_count,
            original.reset_channel_change_error_count
        );
    }

    // --- decode/encode_last_all_channels ---

    #[test]
    fn decode_last_all_channels_bit_set() {
        assert!(decode_last_all_channels(1));
        assert!(decode_last_all_channels(0x01));
    }

    #[test]
    fn decode_last_all_channels_bit_clear() {
        assert!(!decode_last_all_channels(0));
    }

    #[test]
    fn decode_last_all_channels_ignores_other_bits() {
        // 下位ビットのみを見る((value & 1) != 0)。
        assert!(decode_last_all_channels(0b11));
        assert!(!decode_last_all_channels(0b10));
    }

    #[test]
    fn encode_last_all_channels_true() {
        assert_eq!(encode_last_all_channels(true), 0x01);
    }

    #[test]
    fn encode_last_all_channels_false() {
        assert_eq!(encode_last_all_channels(false), 0x00);
    }

    #[test]
    fn last_all_channels_round_trip() {
        assert!(decode_last_all_channels(encode_last_all_channels(true)));
        assert!(!decode_last_all_channels(encode_last_all_channels(false)));
    }

    // --- get_initial_channel ---

    fn sample_last() -> LastChannelInfo {
        LastChannelInfo {
            space: 1,
            channel: 2,
            service_id: 3,
            transport_stream_id: 4,
            all_channels: true,
        }
    }

    fn sample_custom() -> CustomChannelInfo {
        CustomChannelInfo {
            space: 10,
            channel: 20,
            service_id: 30,
            all_channels: false,
        }
    }

    #[test]
    fn get_initial_channel_none_uses_last_space_and_all_channels_only() {
        let last = sample_last();
        let custom = sample_custom();
        let info = get_initial_channel(InitialChannelType::None, &last, &custom);
        assert_eq!(
            info,
            ChannelInfo {
                space: last.space,
                channel: -1,
                service_id: -1,
                transport_stream_id: -1,
                all_channels: last.all_channels,
            }
        );
    }

    #[test]
    fn get_initial_channel_last_uses_all_last_fields() {
        let last = sample_last();
        let custom = sample_custom();
        let info = get_initial_channel(InitialChannelType::Last, &last, &custom);
        assert_eq!(
            info,
            ChannelInfo {
                space: last.space,
                channel: last.channel,
                service_id: last.service_id,
                transport_stream_id: last.transport_stream_id,
                all_channels: last.all_channels,
            }
        );
    }

    #[test]
    fn get_initial_channel_custom_uses_custom_fields_and_forces_tsid_negative_one() {
        let last = sample_last();
        let custom = sample_custom();
        let info = get_initial_channel(InitialChannelType::Custom, &last, &custom);
        assert_eq!(
            info,
            ChannelInfo {
                space: custom.space,
                channel: custom.channel,
                service_id: custom.service_id,
                transport_stream_id: -1,
                all_channels: custom.all_channels,
            }
        );
    }
}
