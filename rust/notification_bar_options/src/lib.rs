//! TVTest の通知バー設定(`src/NotificationBarOptions.cpp` / `src/NotificationBarOptions.h`)の
//! 純粋ロジックを移植したクレート。
//!
//! 移植対象:
//! - 通知フラグ定数 `NOTIFY_EVENTNAME` / `NOTIFY_TSPROCESSORERROR`
//!   (`NotificationBarOptions.h:37-38`)。
//! - コンストラクタの既定値: 有効フラグ `true`・表示時間 `3000`(ミリ秒)・
//!   既定通知フラグ `NOTIFY_EVENTNAME | NOTIFY_TSPROCESSORERROR`
//!   (`NotificationBarOptions.h:63-65`)。
//! - `EnableNotify`(`NotificationBarOptions.cpp:102-108`)・`IsNotifyEnabled`
//!   (`NotificationBarOptions.cpp:96-99`)。
//!
//! 対象外(Win32 / CSettings / UI 依存):
//! - `DlgProc`(ダイアログプロシージャ、フォント選択)。
//! - `ReadSettings` / `WriteSettings` の `CSettings` I/O 本体。
//! - `Style::CStyleManager::AssignFontSizeFromLogFont` 等のフォント既定値算出。
//! - `Create`(`CreateDialogWindow`)。

#![forbid(unsafe_code)]

/// `NOTIFY_EVENTNAME`(`NotificationBarOptions.h:37`)。
pub const NOTIFY_EVENTNAME: u32 = 0x0000_0001;
/// `NOTIFY_TSPROCESSORERROR`(`NotificationBarOptions.h:38`)。
pub const NOTIFY_TSPROCESSORERROR: u32 = 0x0000_0002;

/// コンストラクタの既定通知バー表示時間(ミリ秒、`NotificationBarOptions.h:64`)。
pub const DEFAULT_NOTIFICATION_BAR_DURATION: i32 = 3000;

/// コンストラクタの既定通知フラグ(`NotificationBarOptions.h:65`)。
pub const DEFAULT_NOTIFICATION_BAR_FLAGS: u32 = NOTIFY_EVENTNAME | NOTIFY_TSPROCESSORERROR;

/// 通知バーの状態(`CNotificationBarOptions` の純粋部分)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotificationBarState {
    pub enabled: bool,
    pub duration: i32,
    pub flags: u32,
}

impl Default for NotificationBarState {
    fn default() -> Self {
        Self {
            enabled: true,
            duration: DEFAULT_NOTIFICATION_BAR_DURATION,
            flags: DEFAULT_NOTIFICATION_BAR_FLAGS,
        }
    }
}

impl NotificationBarState {
    /// `EnableNotify`(`NotificationBarOptions.cpp:102-108`)。
    pub fn enable_notify(&mut self, notify_type: u32, enabled: bool) {
        if enabled {
            self.flags |= notify_type;
        } else {
            self.flags &= !notify_type;
        }
    }

    /// `IsNotifyEnabled`(`NotificationBarOptions.cpp:96-99`)。
    /// 通知バー自体が無効なら、個別フラグに関わらず `false`。
    #[must_use]
    pub fn is_notify_enabled(&self, notify_type: u32) -> bool {
        self.enabled && (self.flags & notify_type) != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state() {
        let state = NotificationBarState::default();
        assert!(state.enabled);
        assert_eq!(state.duration, 3000);
        assert_eq!(state.flags, NOTIFY_EVENTNAME | NOTIFY_TSPROCESSORERROR);
    }

    #[test]
    fn default_notify_enabled() {
        let state = NotificationBarState::default();
        assert!(state.is_notify_enabled(NOTIFY_EVENTNAME));
        assert!(state.is_notify_enabled(NOTIFY_TSPROCESSORERROR));
    }

    #[test]
    fn enable_notify_sets_flag() {
        let mut state = NotificationBarState {
            enabled: true,
            duration: 3000,
            flags: 0,
        };
        state.enable_notify(NOTIFY_EVENTNAME, true);
        assert_eq!(state.flags, NOTIFY_EVENTNAME);
        assert!(state.is_notify_enabled(NOTIFY_EVENTNAME));
        assert!(!state.is_notify_enabled(NOTIFY_TSPROCESSORERROR));
    }

    #[test]
    fn enable_notify_clears_flag() {
        let mut state = NotificationBarState::default();
        state.enable_notify(NOTIFY_EVENTNAME, false);
        assert_eq!(state.flags, NOTIFY_TSPROCESSORERROR);
        assert!(!state.is_notify_enabled(NOTIFY_EVENTNAME));
        assert!(state.is_notify_enabled(NOTIFY_TSPROCESSORERROR));
    }

    #[test]
    fn enable_notify_idempotent() {
        let mut state = NotificationBarState::default();
        state.enable_notify(NOTIFY_EVENTNAME, true);
        assert_eq!(state.flags, DEFAULT_NOTIFICATION_BAR_FLAGS);
        state.enable_notify(NOTIFY_TSPROCESSORERROR, false);
        state.enable_notify(NOTIFY_TSPROCESSORERROR, false);
        assert_eq!(state.flags, NOTIFY_EVENTNAME);
    }

    #[test]
    fn is_notify_enabled_false_when_bar_disabled() {
        let state = NotificationBarState {
            enabled: false,
            duration: 3000,
            flags: NOTIFY_EVENTNAME | NOTIFY_TSPROCESSORERROR,
        };
        assert!(!state.is_notify_enabled(NOTIFY_EVENTNAME));
        assert!(!state.is_notify_enabled(NOTIFY_TSPROCESSORERROR));
    }

    #[test]
    fn is_notify_enabled_false_when_flag_not_set() {
        let state = NotificationBarState {
            enabled: true,
            duration: 3000,
            flags: 0,
        };
        assert!(!state.is_notify_enabled(NOTIFY_EVENTNAME));
    }

    #[test]
    fn flag_values() {
        assert_eq!(NOTIFY_EVENTNAME, 1);
        assert_eq!(NOTIFY_TSPROCESSORERROR, 2);
    }

    #[test]
    fn enable_notify_multiple_flags_independent() {
        let mut state = NotificationBarState {
            enabled: true,
            duration: 3000,
            flags: 0,
        };
        state.enable_notify(NOTIFY_EVENTNAME, true);
        state.enable_notify(NOTIFY_TSPROCESSORERROR, true);
        assert_eq!(state.flags, NOTIFY_EVENTNAME | NOTIFY_TSPROCESSORERROR);
        state.enable_notify(NOTIFY_EVENTNAME, false);
        assert_eq!(state.flags, NOTIFY_TSPROCESSORERROR);
    }
}
