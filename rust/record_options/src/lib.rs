//! TVTest の録画設定(`src/RecordOptions.cpp` / `RecordOptions.h`)の純粋ロジックを
//! 移植したクレート。
//!
//! 移植対象:
//! - 各種バッファサイズの制限定数(`MEGA_BYTES`、`WRITE_BUFFER_SIZE_MIN/MAX`、
//!   `TIMESHIFT_BUFFER_SIZE_MIN/MAX`、`MAX_PENDING_SIZE_MIN/MAX`、RecordOptions.cpp:35-47)。
//! - ステータスバー録画コマンドの選択肢一覧(`StatusBarCommandList`、RecordOptions.cpp:50-55)
//!   および既定値(コンストラクタ、RecordOptions.cpp:60-63)。
//! - `ReadSettings` のうち以下の純粋ロジック(RecordOptions.cpp:81-149):
//!   - `RecordBufferSize` 読み込み時のクランプ(126-127行)。
//!   - `TimeShiftRecBufferSize` 読み込み時の MB→バイト変換とクランプ(130-131行)。
//!   - `RecMaxPendingSize` 読み込み時のクランプ(133-134行)。
//!   - `StatusBarRecordCommand` 読み込み時の ID 解決ロジック(137-146行)。
//! - `WriteSettings` のうち以下(RecordOptions.cpp:152-177):
//!   - `TimeShiftRecBufferSize` 書き込み時のバイト→MB変換(167行)。
//!   - `StatusBarRecordCommand` 書き込み時の空文字列判定(171-175行)。
//! - `GetLowFreeSpaceThresholdBytes`(RecordOptions.h:83-86)と既定値
//!   (`m_LowFreeSpaceThreshold` の既定値 `2048`、RecordOptions.h:42)。
//! - `GetFilePath` のファイルパス長チェック(RecordOptions.cpp:195-203)のうち、
//!   `PathCombine` を除く長さ判定部分。
//!
//! 対象外(Win32 / CSettings / DlgProc / RecordManager 依存):
//! - `DlgProc`(ダイアログ)。
//! - `ReadSettings` / `WriteSettings` の `CSettings` I/O 本体。
//! - `ConfirmChannelChange` / `ConfirmServiceChange` / `ConfirmStop` /
//!   `ConfirmStatusBarStop` / `ConfirmExit` の `MessageBox` 表示。
//! - `GetFilePath` / `GenerateFilePath` の実際のパス結合処理(`PathCombine` 等)。
//! - `Apply`(`CRecordManager::SetRecordingSettings` 連携)。
//! - `EnableTimeShiftRecording` の `Apply` 呼び出し。

#![forbid(unsafe_code)]

/// 1 メガバイトのバイト数(RecordOptions.cpp:35)。
pub const MEGA_BYTES: u32 = 1024 * 1024;

/// 書き出しバッファサイズの下限(バイト単位、RecordOptions.cpp:38)。
pub const WRITE_BUFFER_SIZE_MIN: u32 = 1024;

/// 書き出しバッファサイズの上限(バイト単位、RecordOptions.cpp:39)。
pub const WRITE_BUFFER_SIZE_MAX: u32 = 32 * MEGA_BYTES;

/// さかのぼり録画バッファサイズの下限(バイト単位、RecordOptions.cpp:42)。
pub const TIMESHIFT_BUFFER_SIZE_MIN: u32 = MEGA_BYTES;

/// さかのぼり録画バッファサイズの上限(バイト単位、RecordOptions.cpp:43)。
pub const TIMESHIFT_BUFFER_SIZE_MAX: u32 = 1024 * MEGA_BYTES;

/// 書き出し待ちバッファの下限(バイト単位、RecordOptions.cpp:46)。
pub const MAX_PENDING_SIZE_MIN: u32 = 32 * MEGA_BYTES;

/// 書き出し待ちバッファの上限(バイト単位、RecordOptions.cpp:47)。
pub const MAX_PENDING_SIZE_MAX: u32 = 1024 * MEGA_BYTES;

/// ステータスバーからの録画のコマンドの選択肢一覧(RecordOptions.cpp:50-55)。
///
/// `CM_RECORD_START` = 151、`CM_RECORDOPTION` = 154、`CM_TIMESHIFTRECORDING` = 158
/// (resource.h)、末尾の `0` は「何もしない」を表す。
pub const STATUS_BAR_COMMAND_LIST: [i32; 4] = [151, 154, 158, 0];

/// `m_StatusBarRecordCommand` の既定値(コンストラクタ、RecordOptions.cpp:60-63)。
/// `CM_RECORD_START` の値。
pub const DEFAULT_STATUS_BAR_RECORD_COMMAND: i32 = 151;

/// `m_LowFreeSpaceThreshold` の既定値(MB 単位、RecordOptions.h:42)。
pub const DEFAULT_LOW_FREE_SPACE_THRESHOLD_MB: u32 = 2048;

/// 書き出しバッファサイズを `WRITE_BUFFER_SIZE_MIN..=WRITE_BUFFER_SIZE_MAX` の範囲に
/// クランプする。
///
/// 原実装(RecordOptions.cpp:126-127):
/// `m_Settings.m_WriteCacheSize = std::clamp(Value, WRITE_BUFFER_SIZE_MIN, WRITE_BUFFER_SIZE_MAX);`
#[must_use]
pub fn clamp_write_buffer_size(value: u32) -> u32 {
    value.clamp(WRITE_BUFFER_SIZE_MIN, WRITE_BUFFER_SIZE_MAX)
}

/// さかのぼり録画バッファサイズ(MB 単位で読み込んだ値)をバイト単位へ変換した上で
/// `TIMESHIFT_BUFFER_SIZE_MIN..=TIMESHIFT_BUFFER_SIZE_MAX` の範囲にクランプする。
///
/// `value_mb * MEGA_BYTES` は `u32` 同士では桁あふれし得るため、`u64` で中間計算してから
/// 最終的に `u32` へ収める。
///
/// 原実装(RecordOptions.cpp:130-131):
/// `m_Settings.m_TimeShiftBufferSize = std::clamp(Value * MEGA_BYTES, TIMESHIFT_BUFFER_SIZE_MIN, TIMESHIFT_BUFFER_SIZE_MAX);`
#[must_use]
pub fn clamp_timeshift_buffer_size_mb(value_mb: u32) -> u32 {
    let bytes = u64::from(value_mb) * u64::from(MEGA_BYTES);
    let clamped = bytes.clamp(
        u64::from(TIMESHIFT_BUFFER_SIZE_MIN),
        u64::from(TIMESHIFT_BUFFER_SIZE_MAX),
    );
    // TIMESHIFT_BUFFER_SIZE_MAX は u32 の範囲内であり、clamp によって必ず範囲内に
    // 収まるため、u32 へのキャストは安全。
    clamped as u32
}

/// バイト単位の値をメガバイト単位へ変換する(整数除算)。
///
/// 原実装(RecordOptions.cpp:167):
/// `static_cast<unsigned int>(m_Settings.m_TimeShiftBufferSize / MEGA_BYTES)`
#[must_use]
pub fn bytes_to_megabytes(value_bytes: u32) -> u32 {
    value_bytes / MEGA_BYTES
}

/// 書き出し待ちバッファサイズを `MAX_PENDING_SIZE_MIN..=MAX_PENDING_SIZE_MAX` の範囲に
/// クランプする。
///
/// 原実装(RecordOptions.cpp:133-134):
/// `m_Settings.m_MaxPendingSize = std::clamp(Value, MAX_PENDING_SIZE_MIN, MAX_PENDING_SIZE_MAX);`
#[must_use]
pub fn clamp_max_pending_size(value: u32) -> u32 {
    value.clamp(MAX_PENDING_SIZE_MIN, MAX_PENDING_SIZE_MAX)
}

/// 低空き容量しきい値(MB 単位)をバイト単位へ変換する。
///
/// 原実装(`GetLowFreeSpaceThresholdBytes`、RecordOptions.h:83-86):
/// `return static_cast<ULONGLONG>(m_LowFreeSpaceThreshold) * (1024 * 1024);`
#[must_use]
pub fn get_low_free_space_threshold_bytes(threshold_mb: u32) -> u64 {
    u64::from(threshold_mb) * 1024 * 1024
}

/// `StatusBarRecordCommand` 読み込みロジック相当(RecordOptions.cpp:137-146)。
///
/// - `command_text` が `None`(文字列の読み込み自体に失敗)なら、既存値 `current` を
///   そのまま返す。
/// - `Some(空スライス)` なら `m_StatusBarRecordCommand = 0` に相当し、`0` を返す。
/// - `Some(非空スライス)` なら `manager.parse_id_text` で ID を解決し、`0` 以外なら
///   その値を、`0`(未解決)なら `current` を維持して返す。
#[must_use]
pub fn resolve_status_bar_record_command(
    command_text: Option<&[u16]>,
    current: i32,
    manager: &tvtest_command::CommandManager,
) -> i32 {
    match command_text {
        None => current,
        Some([]) => 0,
        Some(text) => {
            let command = manager.parse_id_text(text);
            if command != 0 {
                command
            } else {
                current
            }
        }
    }
}

/// `WriteSettings` の `StatusBarRecordCommand` 書き込み時の分岐相当
/// (RecordOptions.cpp:171-175)。
///
/// `command` が `0` なら空文字列を書き込むべきことを表す `true` を返す。
#[must_use]
pub fn should_write_empty_status_bar_command(command: i32) -> bool {
    command == 0
}

/// 保存先フォルダとファイル名を結合したパスの長さが `max_length` 未満に収まるかを
/// 判定する(`GetFilePath` の長さチェック相当、RecordOptions.cpp:195-203)。
///
/// 原実装:
/// `if (m_SaveFolder.length() + 1 + m_FileName.length() >= static_cast<size_t>(MaxLength)) return false;`
#[must_use]
pub fn is_file_path_within_length(
    save_folder_len: usize,
    file_name_len: usize,
    max_length: usize,
) -> bool {
    save_folder_len + 1 + file_name_len < max_length
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn clamp_write_buffer_size_lower_bound() {
        assert_eq!(clamp_write_buffer_size(WRITE_BUFFER_SIZE_MIN), WRITE_BUFFER_SIZE_MIN);
    }

    #[test]
    fn clamp_write_buffer_size_below_lower_bound() {
        assert_eq!(clamp_write_buffer_size(0), WRITE_BUFFER_SIZE_MIN);
    }

    #[test]
    fn clamp_write_buffer_size_upper_bound() {
        assert_eq!(clamp_write_buffer_size(WRITE_BUFFER_SIZE_MAX), WRITE_BUFFER_SIZE_MAX);
    }

    #[test]
    fn clamp_write_buffer_size_above_upper_bound() {
        assert_eq!(
            clamp_write_buffer_size(WRITE_BUFFER_SIZE_MAX + 1),
            WRITE_BUFFER_SIZE_MAX
        );
    }

    #[test]
    fn clamp_write_buffer_size_within_range() {
        assert_eq!(clamp_write_buffer_size(65536), 65536);
    }

    #[test]
    fn clamp_timeshift_buffer_size_mb_lower_bound() {
        assert_eq!(clamp_timeshift_buffer_size_mb(1), TIMESHIFT_BUFFER_SIZE_MIN);
    }

    #[test]
    fn clamp_timeshift_buffer_size_mb_below_lower_bound() {
        // 0 MB -> 0 バイトはクランプ後 TIMESHIFT_BUFFER_SIZE_MIN。
        assert_eq!(clamp_timeshift_buffer_size_mb(0), TIMESHIFT_BUFFER_SIZE_MIN);
    }

    #[test]
    fn clamp_timeshift_buffer_size_mb_upper_bound() {
        assert_eq!(clamp_timeshift_buffer_size_mb(1024), TIMESHIFT_BUFFER_SIZE_MAX);
    }

    #[test]
    fn clamp_timeshift_buffer_size_mb_above_upper_bound_no_overflow() {
        // u32::MAX MB を渡しても u64 中間計算によりオーバーフローせず上限にクランプされる。
        assert_eq!(
            clamp_timeshift_buffer_size_mb(u32::MAX),
            TIMESHIFT_BUFFER_SIZE_MAX
        );
    }

    #[test]
    fn clamp_timeshift_buffer_size_mb_within_range() {
        assert_eq!(clamp_timeshift_buffer_size_mb(100), 100 * MEGA_BYTES);
    }

    #[test]
    fn bytes_to_megabytes_exact() {
        assert_eq!(bytes_to_megabytes(100 * MEGA_BYTES), 100);
    }

    #[test]
    fn bytes_to_megabytes_truncates() {
        assert_eq!(bytes_to_megabytes(MEGA_BYTES + 1), 1);
    }

    #[test]
    fn bytes_to_megabytes_zero() {
        assert_eq!(bytes_to_megabytes(0), 0);
    }

    #[test]
    fn clamp_max_pending_size_lower_bound() {
        assert_eq!(clamp_max_pending_size(MAX_PENDING_SIZE_MIN), MAX_PENDING_SIZE_MIN);
    }

    #[test]
    fn clamp_max_pending_size_below_lower_bound() {
        assert_eq!(clamp_max_pending_size(0), MAX_PENDING_SIZE_MIN);
    }

    #[test]
    fn clamp_max_pending_size_upper_bound() {
        assert_eq!(clamp_max_pending_size(MAX_PENDING_SIZE_MAX), MAX_PENDING_SIZE_MAX);
    }

    #[test]
    fn clamp_max_pending_size_above_upper_bound() {
        assert_eq!(
            clamp_max_pending_size(MAX_PENDING_SIZE_MAX + 1),
            MAX_PENDING_SIZE_MAX
        );
    }

    #[test]
    fn clamp_max_pending_size_within_range() {
        assert_eq!(clamp_max_pending_size(64 * MEGA_BYTES), 64 * MEGA_BYTES);
    }

    #[test]
    fn get_low_free_space_threshold_bytes_default_value() {
        assert_eq!(
            get_low_free_space_threshold_bytes(DEFAULT_LOW_FREE_SPACE_THRESHOLD_MB),
            2048u64 * 1024 * 1024
        );
    }

    #[test]
    fn get_low_free_space_threshold_bytes_zero() {
        assert_eq!(get_low_free_space_threshold_bytes(0), 0);
    }

    #[test]
    fn get_low_free_space_threshold_bytes_large_value_no_overflow() {
        // u32::MAX MB でも u64 計算のためオーバーフローしない。
        assert_eq!(
            get_low_free_space_threshold_bytes(u32::MAX),
            u64::from(u32::MAX) * 1024 * 1024
        );
    }

    #[test]
    fn resolve_status_bar_record_command_read_failure_keeps_current() {
        let manager = tvtest_command::CommandManager::new(100, 200);
        assert_eq!(resolve_status_bar_record_command(None, 151, &manager), 151);
    }

    #[test]
    fn resolve_status_bar_record_command_empty_text_sets_zero() {
        let manager = tvtest_command::CommandManager::new(100, 200);
        assert_eq!(
            resolve_status_bar_record_command(Some(&[]), 151, &manager),
            0
        );
    }

    #[test]
    fn resolve_status_bar_record_command_resolves_known_text() {
        let mut manager = tvtest_command::CommandManager::new(100, 200);
        manager.register_command_single(
            154,
            &utf16("RecordOption"),
            None,
            &[],
            &[],
            tvtest_command::CommandState::NONE,
        );
        let text = utf16("RecordOption");
        assert_eq!(
            resolve_status_bar_record_command(Some(&text), 151, &manager),
            154
        );
    }

    #[test]
    fn resolve_status_bar_record_command_unresolved_text_keeps_current() {
        let manager = tvtest_command::CommandManager::new(100, 200);
        let text = utf16("UnknownCommand");
        assert_eq!(
            resolve_status_bar_record_command(Some(&text), 151, &manager),
            151
        );
    }

    #[test]
    fn should_write_empty_status_bar_command_zero() {
        assert!(should_write_empty_status_bar_command(0));
    }

    #[test]
    fn should_write_empty_status_bar_command_nonzero() {
        assert!(!should_write_empty_status_bar_command(151));
    }

    #[test]
    fn is_file_path_within_length_within_range() {
        assert!(is_file_path_within_length(10, 10, 260));
    }

    #[test]
    fn is_file_path_within_length_at_boundary_is_false() {
        // folder(10) + 1 + file(9) = 20 == max_length(20) -> false(">= MaxLength")。
        assert!(!is_file_path_within_length(10, 9, 20));
    }

    #[test]
    fn is_file_path_within_length_just_below_boundary_is_true() {
        // folder(10) + 1 + file(8) = 19 < max_length(20) -> true。
        assert!(is_file_path_within_length(10, 8, 20));
    }

    #[test]
    fn is_file_path_within_length_exceeds_max() {
        assert!(!is_file_path_within_length(200, 200, 260));
    }

    #[test]
    fn status_bar_command_list_values() {
        // CM_RECORD_START / CM_RECORDOPTION / CM_TIMESHIFTRECORDING(resource.h)。
        assert_eq!(STATUS_BAR_COMMAND_LIST, [151, 154, 158, 0]);
    }

    #[test]
    fn default_status_bar_record_command_is_record_start() {
        assert_eq!(DEFAULT_STATUS_BAR_RECORD_COMMAND, STATUS_BAR_COMMAND_LIST[0]);
    }
}
