//! TVTest のタスクバー設定(`src/TaskbarOptions.cpp` / `TaskbarOptions.h`)の純粋ロジックを
//! 移植したクレート。
//!
//! 移植対象:
//! - 既定のタスク一覧(`m_DefaultTaskList`、TaskbarOptions.cpp:33-37)。
//! - `ReadSettings` のタスク一覧文字列パース(TaskbarOptions.cpp:61-82)。空文字列はセパレータ
//!   (`0`)、非空文字列はコマンド ID テキストとして解決する。ID 解決は呼び出し側の
//!   `tvtest_command::CommandManager::parse_id_text` に委譲する。
//! - ジャンプリスト有効判定(`IsJumpListEnabled`、TaskbarOptions.cpp:116-121)。
//!
//! 対象外(Win32 / CSettings / TaskbarManager 依存):
//! - `ReadSettings` / `WriteSettings` の `CSettings` I/O 本体。
//! - `SetEnableJumpList` の `TaskbarManager.ReinitializeJumpList` 呼び出し。
//! - ジャンプリスト自体の構築(`ICustomDestinationList` 等)。

#![forbid(unsafe_code)]

use tvtest_command::CommandManager;

/// タスク一覧のセパレータを表すコマンド ID(`0`、TaskbarOptions.cpp:75)。
pub const TASK_SEPARATOR: i32 = 0;

/// 既定のタスク一覧(`m_DefaultTaskList`、TaskbarOptions.cpp:33-37)。
///
/// `CM_FULLSCREEN` = 137、`CM_DISABLEVIEWER` = 161、`CM_PROGRAMGUIDE` = 204
/// (resource.h)。
pub const DEFAULT_TASK_LIST: [i32; 3] = [137, 161, 204];

/// `ReadSettings` の `TaskN` 設定値からタスク一覧を構築する(TaskbarOptions.cpp:61-82)。
///
/// - `entries`: `Task0`, `Task1`, ... の順で読み込めた文字列。原実装は `Settings.Read` が
///   失敗した時点(欠番)でループを打ち切るため、呼び出し側は連番が途切れた時点までの
///   スライスを渡すこと。
/// - 各エントリは前後の空白/タブを `Trim` した後、空文字列ならセパレータ(`0`)を追加する。
/// - 非空文字列は `manager.parse_id_text` で ID を解決し、`0`(未解決)以外のみ追加する
///   (TaskbarOptions.cpp:77-79 の `if (ID != 0)`)。
#[must_use]
pub fn parse_task_list(entries: &[Vec<u16>], manager: &CommandManager) -> Vec<i32> {
    let mut task_list = Vec::new();
    let spaces: [u16; 2] = [u16::from(b' '), u16::from(b'\t')];

    for entry in entries {
        let mut command = entry.clone();
        tvtest_string_utility::trim(&mut command, &spaces);

        if command.is_empty() {
            task_list.push(TASK_SEPARATOR);
        } else {
            let id = manager.parse_id_text(&command);
            if id != 0 {
                task_list.push(id);
            }
        }
    }

    task_list
}

/// ジャンプリストが有効か(`IsJumpListEnabled`、TaskbarOptions.cpp:116-121)。
///
/// `fEnableJumpList && ((fShowTasks && !TaskList.empty()) || (fShowRecentChannels && MaxRecentChannels > 0))`。
#[must_use]
pub fn is_jump_list_enabled(
    enable_jump_list: bool,
    show_tasks: bool,
    task_list_is_empty: bool,
    show_recent_channels: bool,
    max_recent_channels: i32,
) -> bool {
    enable_jump_list
        && ((show_tasks && !task_list_is_empty) || (show_recent_channels && max_recent_channels > 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_task_list_values() {
        // CM_FULLSCREEN / CM_DISABLEVIEWER / CM_PROGRAMGUIDE(resource.h)。
        assert_eq!(DEFAULT_TASK_LIST, [137, 161, 204]);
    }

    #[test]
    fn jump_list_enabled_requires_enable_flag() {
        assert!(!is_jump_list_enabled(false, true, false, true, 10));
    }

    #[test]
    fn jump_list_enabled_by_tasks() {
        assert!(is_jump_list_enabled(true, true, false, false, 0));
        // タスク一覧が空なら無効。
        assert!(!is_jump_list_enabled(true, true, true, false, 0));
        // 表示自体が無効なら無効。
        assert!(!is_jump_list_enabled(true, false, false, false, 0));
    }

    #[test]
    fn jump_list_enabled_by_recent_channels() {
        assert!(is_jump_list_enabled(true, false, true, true, 1));
        // 上限が 0 以下なら無効。
        assert!(!is_jump_list_enabled(true, false, true, true, 0));
        assert!(!is_jump_list_enabled(true, false, true, true, -1));
        // 表示自体が無効なら無効。
        assert!(!is_jump_list_enabled(true, false, true, false, 10));
    }

    #[test]
    fn jump_list_enabled_either_condition() {
        // 両方満たしても true(OR 条件)。
        assert!(is_jump_list_enabled(true, true, false, true, 5));
    }

    fn utf16(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn parse_task_list_empty_entry_is_separator() {
        let manager = CommandManager::new(1, 1);
        let entries = vec![utf16(""), utf16("  \t ")];
        let result = parse_task_list(&entries, &manager);
        assert_eq!(result, vec![TASK_SEPARATOR, TASK_SEPARATOR]);
    }

    #[test]
    fn parse_task_list_unresolved_command_is_skipped() {
        // ID テキストが未登録なら parse_id_text は 0 を返し、エントリは追加されない。
        let manager = CommandManager::new(1, 1);
        let entries = vec![utf16("UnknownCommand")];
        let result = parse_task_list(&entries, &manager);
        assert!(result.is_empty());
    }

    #[test]
    fn parse_task_list_mixed_entries() {
        let manager = CommandManager::new(1, 1);
        // 未登録コマンドは無視、空文字列はセパレータとして残る。
        let entries = vec![utf16("Foo"), utf16(""), utf16("Bar")];
        let result = parse_task_list(&entries, &manager);
        assert_eq!(result, vec![TASK_SEPARATOR]);
    }
}
