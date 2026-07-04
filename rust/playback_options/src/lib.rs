//! TVTest の再生設定(`src/PlaybackOptions.cpp` / `PlaybackOptions.h`)の純粋ロジックを
//! 移植したクレート。
//!
//! 移植対象:
//! - パケットバッファ長の上限クランプ(`ReadSettings`、PlaybackOptions.cpp:86-87)。
//! - パケットバッファプール割合の範囲クランプ(`ReadSettings`、PlaybackOptions.cpp:88-89)。
//! - ストリームスレッド優先度の範囲クランプ(`ReadSettings`、PlaybackOptions.cpp:90-91)。
//! - 起動時のミュート復元判定(`IsMuteOnStartUp`、PlaybackOptions.h:49)。
//! - 起動時の1セグモード復元判定(`Is1SegModeOnStartup`、PlaybackOptions.h:52)。
//! - 各種定数(`MAX_PACKET_BUFFER_LENGTH`、`THREAD_PRIORITY_*`、`UPDATE_*` 更新フラグ、
//!   PlaybackOptions.h:63-71)。
//!
//! 対象外(Win32 / DirectShow / CSettings / CoreEngine 依存):
//! - `CPlaybackOptions::DlgProc`(ダイアログ)。
//! - `ReadSettings` / `WriteSettings` の `CSettings` I/O 本体(本クレートは値の変換のみ提供)。
//! - `Apply` における `CCoreEngine` / `LibISDB::ViewerFilter` / `LibISDB::BonDriverSourceFilter`
//!   との連携(`SetAdjustAudioStreamTime` / `EnablePTSSync` / `SetPacketBufferLength` /
//!   `SetPacketBufferPool` / `SetStreamingThreadPriority` / `SetAdjust1SegVideoSample` など)。

#![forbid(unsafe_code)]

/// パケットバッファ長の最大値(PlaybackOptions.h:71)。
pub const MAX_PACKET_BUFFER_LENGTH: u32 = 0x0010_0000;

/// スレッド優先度「通常」(Win32 の `THREAD_PRIORITY_NORMAL` に相当、PlaybackOptions.h:87)。
pub const THREAD_PRIORITY_NORMAL: i32 = 0;

/// スレッド優先度「最高」(Win32 の `THREAD_PRIORITY_HIGHEST` に相当、PlaybackOptions.h:91)。
pub const THREAD_PRIORITY_HIGHEST: i32 = 2;

/// 音声ストリーム時刻補正の更新フラグ(PlaybackOptions.h:64)。
pub const UPDATE_ADJUSTAUDIOSTREAMTIME: u32 = 0x0000_0001;

/// PTS 同期の更新フラグ(PlaybackOptions.h:65)。
pub const UPDATE_PTSSYNC: u32 = 0x0000_0002;

/// パケットバッファリングの更新フラグ(PlaybackOptions.h:66)。
pub const UPDATE_PACKETBUFFERING: u32 = 0x0000_0004;

/// ストリームスレッド優先度の更新フラグ(PlaybackOptions.h:67)。
pub const UPDATE_STREAMTHREADPRIORITY: u32 = 0x0000_0008;

/// フレームレート調整の更新フラグ(PlaybackOptions.h:68)。
pub const UPDATE_ADJUSTFRAMERATE: u32 = 0x0000_0010;

/// パケットバッファ長を `MAX_PACKET_BUFFER_LENGTH` 以下にクランプする。
///
/// 原実装(PlaybackOptions.cpp:87):
/// `m_PacketBufferLength = std::min(BufferLength, (unsigned int)MAX_PACKET_BUFFER_LENGTH);`
#[must_use]
pub const fn clamp_packet_buffer_length(value: u32) -> u32 {
    if value < MAX_PACKET_BUFFER_LENGTH {
        value
    } else {
        MAX_PACKET_BUFFER_LENGTH
    }
}

/// パケットバッファプール割合を `0..=100` の範囲にクランプする。
///
/// 原実装(PlaybackOptions.cpp:89):
/// `m_PacketBufferPoolPercentage = std::clamp(m_PacketBufferPoolPercentage, 0, 100);`
#[must_use]
pub fn clamp_packet_buffer_pool_percentage(value: i32) -> i32 {
    value.clamp(0, 100)
}

/// ストリームスレッド優先度を `THREAD_PRIORITY_NORMAL..=THREAD_PRIORITY_HIGHEST` の
/// 範囲にクランプする。
///
/// 原実装(PlaybackOptions.cpp:91):
/// `m_StreamThreadPriority = std::clamp(m_StreamThreadPriority, THREAD_PRIORITY_NORMAL, THREAD_PRIORITY_HIGHEST);`
#[must_use]
pub fn clamp_stream_thread_priority(value: i32) -> i32 {
    value.clamp(THREAD_PRIORITY_NORMAL, THREAD_PRIORITY_HIGHEST)
}

/// 起動時にミュート状態を復元すべきかどうかを判定する。
///
/// 原実装(PlaybackOptions.h:49):
/// `bool IsMuteOnStartUp() const { return m_fRestoreMute && m_fMute; }`
#[must_use]
pub const fn is_mute_on_startup(restore_mute: bool, mute: bool) -> bool {
    restore_mute && mute
}

/// 起動時に1セグモードを復元すべきかどうかを判定する。
///
/// 原実装(PlaybackOptions.h:52):
/// `bool Is1SegModeOnStartup() const { return m_fRestore1SegMode && m_f1SegMode; }`
#[must_use]
pub const fn is_1seg_mode_on_startup(restore_1seg_mode: bool, seg1_mode: bool) -> bool {
    restore_1seg_mode && seg1_mode
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_packet_buffer_length_within_range() {
        assert_eq!(clamp_packet_buffer_length(40_000), 40_000);
    }

    #[test]
    fn clamp_packet_buffer_length_at_upper_bound() {
        assert_eq!(
            clamp_packet_buffer_length(MAX_PACKET_BUFFER_LENGTH),
            MAX_PACKET_BUFFER_LENGTH
        );
    }

    #[test]
    fn clamp_packet_buffer_length_above_upper_bound() {
        assert_eq!(
            clamp_packet_buffer_length(MAX_PACKET_BUFFER_LENGTH + 1),
            MAX_PACKET_BUFFER_LENGTH
        );
    }

    #[test]
    fn clamp_packet_buffer_length_zero() {
        assert_eq!(clamp_packet_buffer_length(0), 0);
    }

    #[test]
    fn clamp_pool_percentage_lower_bound() {
        assert_eq!(clamp_packet_buffer_pool_percentage(0), 0);
    }

    #[test]
    fn clamp_pool_percentage_upper_bound() {
        assert_eq!(clamp_packet_buffer_pool_percentage(100), 100);
    }

    #[test]
    fn clamp_pool_percentage_within_range() {
        assert_eq!(clamp_packet_buffer_pool_percentage(50), 50);
    }

    #[test]
    fn clamp_pool_percentage_below_lower_bound() {
        assert_eq!(clamp_packet_buffer_pool_percentage(-10), 0);
    }

    #[test]
    fn clamp_pool_percentage_above_upper_bound() {
        assert_eq!(clamp_packet_buffer_pool_percentage(150), 100);
    }

    #[test]
    fn clamp_stream_thread_priority_lower_bound() {
        assert_eq!(
            clamp_stream_thread_priority(THREAD_PRIORITY_NORMAL),
            THREAD_PRIORITY_NORMAL
        );
    }

    #[test]
    fn clamp_stream_thread_priority_upper_bound() {
        assert_eq!(
            clamp_stream_thread_priority(THREAD_PRIORITY_HIGHEST),
            THREAD_PRIORITY_HIGHEST
        );
    }

    #[test]
    fn clamp_stream_thread_priority_within_range() {
        assert_eq!(clamp_stream_thread_priority(1), 1);
    }

    #[test]
    fn clamp_stream_thread_priority_below_lower_bound() {
        assert_eq!(
            clamp_stream_thread_priority(-5),
            THREAD_PRIORITY_NORMAL
        );
    }

    #[test]
    fn clamp_stream_thread_priority_above_upper_bound() {
        assert_eq!(
            clamp_stream_thread_priority(10),
            THREAD_PRIORITY_HIGHEST
        );
    }

    #[test]
    fn is_mute_on_startup_both_true() {
        assert!(is_mute_on_startup(true, true));
    }

    #[test]
    fn is_mute_on_startup_restore_false_mute_true() {
        assert!(!is_mute_on_startup(false, true));
    }

    #[test]
    fn is_mute_on_startup_restore_true_mute_false() {
        assert!(!is_mute_on_startup(true, false));
    }

    #[test]
    fn is_mute_on_startup_both_false() {
        assert!(!is_mute_on_startup(false, false));
    }

    #[test]
    fn is_1seg_mode_on_startup_both_true() {
        assert!(is_1seg_mode_on_startup(true, true));
    }

    #[test]
    fn is_1seg_mode_on_startup_restore_false_mode_true() {
        assert!(!is_1seg_mode_on_startup(false, true));
    }

    #[test]
    fn is_1seg_mode_on_startup_restore_true_mode_false() {
        assert!(!is_1seg_mode_on_startup(true, false));
    }

    #[test]
    fn is_1seg_mode_on_startup_both_false() {
        assert!(!is_1seg_mode_on_startup(false, false));
    }

    #[test]
    fn update_flags_are_distinct_bits() {
        let flags = [
            UPDATE_ADJUSTAUDIOSTREAMTIME,
            UPDATE_PTSSYNC,
            UPDATE_PACKETBUFFERING,
            UPDATE_STREAMTHREADPRIORITY,
            UPDATE_ADJUSTFRAMERATE,
        ];
        let combined = flags.iter().fold(0u32, |acc, f| acc | f);
        let sum: u32 = flags.iter().sum();
        assert_eq!(combined, sum, "flags must not overlap");
        assert_eq!(combined, 0x1F);
    }
}
