//! TVTest の `CCommandManager`(src/Command.cpp / Command.h)の移植。
//!
//! コマンドの登録、ID とテキスト識別子の相互変換、状態フラグ(無効/チェック)の
//! 管理、列挙(`CCommandLister`)を担う。コマンド実行ハンドラはコールバック、
//! テキストのカスタマイズ(`CCommandCustomizer`)と状態変化通知(`CEventListener`)
//! はトレイトで抽象化する。
//!
//! 文字列は原実装の `wchar_t`(UTF-16)に合わせて `[u16]`/`Vec<u16>` で扱う。
//!
//! # 原実装からの主な差異(いずれも挙動は等価)
//! - `CM_COMMAND_FIRST`/`CM_COMMAND_LAST` はリソース ID マクロのため、ID 範囲を
//!   [`CommandManager::new`] の引数として受け取る。
//! - テキストマップのキー比較は原実装の `StringUtility::IsEqualNoCase`(Win32 の
//!   `towlower` ベース)に相当する大文字小文字無視だが、コマンド ID テキストは
//!   ASCII 識別子のため ASCII 範囲の畳み込みで等価。
//! - `GetCommandText`/`GetCommandShortText` のリソース文字列フォールバック
//!   (`LoadString`)は Win32 依存のため対象外(該当時は `None`)。
//! - イベントリスナはポインタ同一性ではなく、登録時に返すハンドルで除去する。

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

/// コマンドの状態フラグ(Command.h `CommandState`)。ビット和で組み合わせる。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct CommandState(pub u8);

impl CommandState {
    pub const NONE: Self = Self(0x00);
    pub const DISABLED: Self = Self(0x01);
    pub const CHECKED: Self = Self(0x02);

    /// 指定フラグをすべて含むか。
    pub const fn contains(self, flag: Self) -> bool {
        (self.0 & flag.0) == flag.0
    }
}

impl std::ops::BitOr for CommandState {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitAnd for CommandState {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

impl std::ops::Not for CommandState {
    type Output = Self;
    fn not(self) -> Self {
        Self(!self.0)
    }
}

/// コマンド実行時のフラグ(Command.h `InvokeFlag`)。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct InvokeFlag(pub u32);

impl InvokeFlag {
    pub const NONE: Self = Self(0x0000);
    pub const MOUSE: Self = Self(0x0001);
}

/// コマンド実行ハンドラへ渡されるパラメータ(Command.h `InvokeParameters`)。
#[derive(Clone, Copy, Debug)]
pub struct InvokeParameters {
    pub id: i32,
    pub flags: InvokeFlag,
}

/// コマンド実行ハンドラ(Command.h `CommandHandler`)。
pub type CommandHandler = Box<dyn Fn(&mut InvokeParameters) -> bool>;

/// コマンドテキストのカスタマイザ(Command.h `CCommandCustomizer`)。
pub trait CommandCustomizer {
    /// 指定コマンドをこのカスタマイザが扱うか。
    fn is_command_valid(&self, command: i32) -> bool;
    /// 指定コマンドのテキストを返す。扱わない/未定義なら `None`。
    fn get_command_text(&self, _command: i32) -> Option<Vec<u16>> {
        None
    }
}

/// コマンド状態の変化通知リスナ(Command.h `CEventListener`)。
///
/// 通知中にマネージャを書き換える必要はないため `&self` とし、実装側で状態を
/// 持つ場合は内部可変性(`RefCell` 等)を用いる。
pub trait CommandEventListener {
    /// コマンドの状態が変化した(Command.cpp 260)。
    fn on_command_state_changed(&self, _id: i32, _old_state: CommandState, _new_state: CommandState) {}
    /// ラジオ状態のチェックが変化した(Command.cpp 291)。
    fn on_command_radio_checked_state_changed(&self, _first_id: i32, _last_id: i32, _checked_id: i32) {}
}

/// [`CommandManager::add_event_listener`] が返すハンドル。除去に用いる。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ListenerHandle(usize);

/// 大文字小文字を無視するテキストマップのキー(ASCII 畳み込み)。
#[derive(Clone, Debug)]
struct NoCaseKey(Vec<u16>);

/// ASCII 大文字を小文字へ畳み込む(原実装の `towlower` を ASCII 範囲で代替)。
fn ascii_fold(c: u16) -> u16 {
    if (0x41..=0x5A).contains(&c) {
        c + 0x20
    } else {
        c
    }
}

impl PartialEq for NoCaseKey {
    fn eq(&self, other: &Self) -> bool {
        self.0.len() == other.0.len()
            && self
                .0
                .iter()
                .zip(other.0.iter())
                .all(|(&a, &b)| ascii_fold(a) == ascii_fold(b))
    }
}

impl Eq for NoCaseKey {}

impl Hash for NoCaseKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        for &c in &self.0 {
            state.write_u16(ascii_fold(c));
        }
    }
}

/// 登録済みコマンドの情報(Command.h `CommandInfo`)。
struct CommandInfo {
    first_id: i32,
    last_id: i32,
    last_listing_id: i32,
    id_text: Vec<u16>,
    text: Vec<u16>,
    short_text: Vec<u16>,
    handler: Option<CommandHandler>,
}

/// `Text` の末尾に `Number + 1` の十進表記を連結する(Command.cpp 350-357 `GetNumberedText`)。
fn get_numbered_text(text: &[u16], number: i32) -> Vec<u16> {
    let mut v = text.to_vec();
    for ch in (number + 1).to_string().chars() {
        v.push(ch as u16);
    }
    v
}

/// `CCommandManager` の移植。
pub struct CommandManager {
    // ID 範囲(原実装の CM_COMMAND_FIRST / CM_COMMAND_LAST に対応)。
    first_id: i32,
    last_id: i32,
    command_list: Vec<CommandInfo>,
    // FirstID -> command_list の添字。範囲検索(IDToIndex)のため昇順マップ。
    command_id_map: BTreeMap<i32, usize>,
    // ID テキスト(大文字小文字無視) -> ID。
    command_text_map: HashMap<NoCaseKey, i32>,
    // ID ごとの状態(添字 = ID - first_id)。
    command_state_list: Vec<CommandState>,
    customizers: Vec<Box<dyn CommandCustomizer>>,
    event_listeners: Vec<Option<Box<dyn CommandEventListener>>>,
}

impl CommandManager {
    /// 指定 ID 範囲のマネージャを生成する(Command.cpp 33-39)。
    ///
    /// `first_id`/`last_id` は原実装の `CM_COMMAND_FIRST`/`CM_COMMAND_LAST`。
    /// 状態リストは範囲分を [`CommandState::NONE`] で初期化する。
    pub fn new(first_id: i32, last_id: i32) -> Self {
        let count = if last_id >= first_id {
            (last_id - first_id + 1) as usize
        } else {
            0
        };
        Self {
            first_id,
            last_id,
            command_list: Vec::new(),
            command_id_map: BTreeMap::new(),
            command_text_map: HashMap::new(),
            command_state_list: vec![CommandState::NONE; count],
            customizers: Vec::new(),
            event_listeners: Vec::new(),
        }
    }

    /// 範囲付きコマンドを登録する(Command.cpp 42-91 `RegisterCommand`)。
    ///
    /// 範囲が不正なら false。`last_listing_id` が 0 なら列挙対象外。
    #[allow(clippy::too_many_arguments)]
    pub fn register_command(
        &mut self,
        first_id: i32,
        last_id: i32,
        last_listing_id: i32,
        id_text: &[u16],
        handler: Option<CommandHandler>,
        text: &[u16],
        short_text: &[u16],
        state: CommandState,
    ) -> bool {
        if (first_id > last_id)
            || (first_id < self.first_id || first_id > self.last_id)
            || (last_id < self.first_id || last_id > self.last_id)
            || (last_listing_id != 0 && (last_listing_id < first_id || last_listing_id > last_id))
        {
            return false;
        }

        let info = CommandInfo {
            first_id,
            last_id,
            last_listing_id,
            id_text: id_text.to_vec(),
            text: text.to_vec(),
            short_text: short_text.to_vec(),
            handler,
        };
        self.command_list.push(info);
        let index = self.command_list.len() - 1;

        // 識別子重複時は最初の登録を優先(std::map::emplace 相当)。
        self.command_id_map.entry(first_id).or_insert(index);

        if !id_text.is_empty() {
            if first_id == last_id {
                self.command_text_map
                    .entry(NoCaseKey(id_text.to_vec()))
                    .or_insert(first_id);
            } else if last_listing_id != 0 {
                for id in first_id..=last_listing_id {
                    let key = NoCaseKey(get_numbered_text(id_text, id - first_id));
                    self.command_text_map.entry(key).or_insert(id);
                }
            }
        }

        for id in first_id..=last_id {
            self.command_state_list[(id - self.first_id) as usize] = state;
        }

        true
    }

    /// 単一 ID のコマンドを登録する(Command.h 111-118 の簡易オーバーロード)。
    pub fn register_command_single(
        &mut self,
        id: i32,
        id_text: &[u16],
        handler: Option<CommandHandler>,
        text: &[u16],
        short_text: &[u16],
        state: CommandState,
    ) -> bool {
        self.register_command(id, id, id, id_text, handler, text, short_text, state)
    }

    /// コマンドを実行する(Command.cpp 94-116 `InvokeCommand`)。
    ///
    /// 無効な ID、またはハンドラ未設定なら false。
    pub fn invoke_command(&self, id: i32, flags: InvokeFlag) -> bool {
        let index = self.id_to_index(id);
        if index < 0 {
            return false;
        }
        let info = &self.command_list[index as usize];
        match &info.handler {
            Some(handler) => {
                let mut params = InvokeParameters { id, flags };
                handler(&mut params)
            }
            None => false,
        }
    }

    /// 有効なコマンド ID か(Command.cpp 119-122 `IsCommandValid`)。
    pub fn is_command_valid(&self, id: i32) -> bool {
        self.id_to_index(id) >= 0
    }

    /// コマンドの ID テキストを返す(Command.cpp 125-140 `GetCommandIDText`)。
    /// 未登録/テキスト無しなら空。
    pub fn get_command_id_text(&self, id: i32) -> Vec<u16> {
        let index = self.id_to_index(id);
        if index < 0 {
            return Vec::new();
        }
        let info = &self.command_list[index as usize];
        if info.id_text.is_empty() {
            return Vec::new();
        }
        if info.first_id == info.last_id {
            return info.id_text.clone();
        }
        get_numbered_text(&info.id_text, id - info.first_id)
    }

    /// コマンドのテキストを返す(Command.cpp 143-176 `GetCommandText`)。
    ///
    /// カスタマイザ→登録テキストの順に解決する。リソース文字列フォールバック
    /// (`LoadString`)は対象外で、該当時は `None`。
    pub fn get_command_text(&self, id: i32) -> Option<Vec<u16>> {
        self.resolve_text(id, |info| &info.text)
    }

    /// コマンドの短いテキストを返す(Command.cpp 179-212 `GetCommandShortText`)。
    pub fn get_command_short_text(&self, id: i32) -> Option<Vec<u16>> {
        self.resolve_text(id, |info| &info.short_text)
    }

    fn resolve_text(
        &self,
        id: i32,
        select: impl Fn(&CommandInfo) -> &Vec<u16>,
    ) -> Option<Vec<u16>> {
        let index = self.id_to_index(id);
        if index < 0 {
            return None;
        }
        let info = &self.command_list[index as usize];

        // 最初に該当したカスタマイザのみを参照する(Command.cpp 158-164)。
        for customizer in &self.customizers {
            if customizer.is_command_valid(id) {
                if let Some(text) = customizer.get_command_text(id) {
                    return Some(text);
                }
                break;
            }
        }

        let text = select(info);
        if !text.is_empty() {
            Some(text.clone())
        } else {
            // LoadString フォールバックは対象外。
            None
        }
    }

    /// ID テキストから ID を解決する(Command.cpp 215-238 `ParseIDText`)。
    /// 見つからなければ 0。
    pub fn parse_id_text(&self, text: &[u16]) -> i32 {
        if text.is_empty() {
            return 0;
        }
        self.command_text_map
            .get(&NoCaseKey(text.to_vec()))
            .copied()
            .unwrap_or(0)
    }

    /// コマンドの状態を設定する(Command.cpp 241-244 `SetCommandState`)。
    pub fn set_command_state(&mut self, id: i32, state: CommandState) -> bool {
        self.set_command_state_masked(id, !CommandState::NONE, state)
    }

    /// マスク指定でコマンドの状態を設定する(Command.cpp 247-264 `SetCommandState`)。
    ///
    /// 状態が変化した場合のみリスナへ通知する。
    pub fn set_command_state_masked(
        &mut self,
        id: i32,
        mask: CommandState,
        state: CommandState,
    ) -> bool {
        if id < self.first_id || id > self.last_id {
            return false;
        }
        let index = (id - self.first_id) as usize;
        let old_state = self.command_state_list[index];
        let new_state = (old_state & !mask) | (state & mask);
        if old_state != new_state {
            self.command_state_list[index] = new_state;
            for listener in self.event_listeners.iter().flatten() {
                listener.on_command_state_changed(id, old_state, new_state);
            }
        }
        true
    }

    /// コマンドの状態を取得する(Command.cpp 267-273 `GetCommandState`)。
    pub fn get_command_state(&self, id: i32) -> CommandState {
        if id < self.first_id || id > self.last_id {
            return CommandState::NONE;
        }
        self.command_state_list[(id - self.first_id) as usize]
    }

    /// ラジオ的なチェック状態を設定する(Command.cpp 276-294 `SetCommandRadioCheckedState`)。
    ///
    /// `first_id`..=`last_id` のうち `checked_id` のみ [`CommandState::CHECKED`] を立てる。
    pub fn set_command_radio_checked_state(
        &mut self,
        first_id: i32,
        last_id: i32,
        checked_id: i32,
    ) -> bool {
        if (first_id > last_id)
            || (first_id < self.first_id || first_id > self.last_id)
            || (last_id < self.first_id || last_id > self.last_id)
        {
            return false;
        }

        for id in first_id..=last_id {
            let index = (id - self.first_id) as usize;
            if id == checked_id {
                self.command_state_list[index] = self.command_state_list[index] | CommandState::CHECKED;
            } else {
                self.command_state_list[index] = self.command_state_list[index] & !CommandState::CHECKED;
            }
        }

        for listener in self.event_listeners.iter().flatten() {
            listener.on_command_radio_checked_state_changed(first_id, last_id, checked_id);
        }

        true
    }

    /// テキストカスタマイザを追加する(Command.cpp 297-303 `AddCommandCustomizer`)。
    pub fn add_command_customizer(&mut self, customizer: Box<dyn CommandCustomizer>) {
        self.customizers.push(customizer);
    }

    /// イベントリスナを追加し、除去用のハンドルを返す(Command.cpp 306-318 `AddEventListener`)。
    pub fn add_event_listener(&mut self, listener: Box<dyn CommandEventListener>) -> ListenerHandle {
        self.event_listeners.push(Some(listener));
        ListenerHandle(self.event_listeners.len() - 1)
    }

    /// イベントリスナを除去する(Command.cpp 321-330 `RemoveEventListener`)。
    /// 既に除去済み/無効なハンドルなら false。
    pub fn remove_event_listener(&mut self, handle: ListenerHandle) -> bool {
        match self.event_listeners.get_mut(handle.0) {
            Some(slot @ Some(_)) => {
                *slot = None;
                true
            }
            _ => false,
        }
    }

    /// ID から `command_list` の添字を求める(Command.cpp 333-347 `IDToIndex`)。
    /// 該当なしは -1。
    fn id_to_index(&self, id: i32) -> i32 {
        // upper_bound(id) の直前 = id 以下で最大のキー。
        match self.command_id_map.range(..=id).next_back() {
            None => -1,
            Some((_, &index)) => {
                let info = &self.command_list[index];
                if id < info.first_id || id > info.last_id {
                    -1
                } else {
                    index as i32
                }
            }
        }
    }

    /// 列挙子を生成する(Command.h `CCommandLister`)。
    pub fn lister(&self) -> CommandLister<'_> {
        CommandLister {
            manager: self,
            index: 0,
            id: 0,
        }
    }
}

/// 列挙可能なコマンド ID を順に返す(Command.cpp 362-399 `CCommandLister`)。
pub struct CommandLister<'a> {
    manager: &'a CommandManager,
    index: usize,
    id: i32,
}

impl CommandLister<'_> {
    /// 次の列挙対象コマンド ID を返す(Command.cpp 368-392 `Next`)。終端は 0。
    pub fn next_id(&mut self) -> i32 {
        let list = &self.manager.command_list;
        loop {
            if self.index >= list.len() {
                return 0;
            }
            // 列挙対象(LastListingID != 0)かつ ID テキストを持つ項目まで進める。
            while list[self.index].last_listing_id == 0 || list[self.index].id_text.is_empty() {
                self.index += 1;
                if self.index == list.len() {
                    return 0;
                }
            }
            let info = &list[self.index];
            if info.first_id > self.id {
                self.id = info.first_id;
                return self.id;
            } else if info.last_listing_id > self.id {
                self.id += 1;
                return self.id;
            } else {
                self.index += 1;
                self.id = 0;
                // 原実装の末尾再帰 return Next() に相当(ループ先頭へ)。
            }
        }
    }

    /// 列挙位置を先頭へ戻す(Command.cpp 395-399 `Reset`)。
    pub fn reset(&mut self) {
        self.index = 0;
        self.id = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    // 範囲(CM_COMMAND_FIRST/LAST 相当)。
    const FIRST: i32 = 100;
    const LAST: i32 = 200;

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn new_mgr() -> CommandManager {
        CommandManager::new(FIRST, LAST)
    }

    #[test]
    fn register_single_and_lookup() {
        let mut m = new_mgr();
        assert!(m.register_command_single(110, &w("Play"), None, &w("再生"), &[], CommandState::NONE));
        assert!(m.is_command_valid(110));
        assert!(!m.is_command_valid(111));
        assert_eq!(m.get_command_id_text(110), w("Play"));
        assert_eq!(m.parse_id_text(&w("Play")), 110);
        assert_eq!(m.parse_id_text(&w("play")), 110); // 大文字小文字無視
        assert_eq!(m.parse_id_text(&w("Unknown")), 0);
    }

    #[test]
    fn register_rejects_out_of_range() {
        let mut m = new_mgr();
        assert!(!m.register_command_single(99, &w("X"), None, &[], &[], CommandState::NONE));
        assert!(!m.register_command_single(201, &w("X"), None, &[], &[], CommandState::NONE));
        // first > last
        assert!(!m.register_command(150, 140, 0, &w("X"), None, &[], &[], CommandState::NONE));
        // last_listing が範囲外
        assert!(!m.register_command(110, 120, 130, &w("X"), None, &[], &[], CommandState::NONE));
    }

    #[test]
    fn ranged_command_numbered_text() {
        let mut m = new_mgr();
        // FirstID=110, LastID=120, LastListingID=115
        assert!(m.register_command(110, 120, 115, &w("Zoom"), None, &[], &[], CommandState::NONE));
        // 範囲内の各 ID が有効
        assert!(m.is_command_valid(110));
        assert!(m.is_command_valid(120));
        assert!(!m.is_command_valid(121));
        // ID テキストは番号付き(Number = id - FirstID, 表記は +1)
        assert_eq!(m.get_command_id_text(110), w("Zoom1"));
        assert_eq!(m.get_command_id_text(115), w("Zoom6"));
        // テキストマップは LastListingID までのみ
        assert_eq!(m.parse_id_text(&w("Zoom1")), 110);
        assert_eq!(m.parse_id_text(&w("Zoom6")), 115);
        assert_eq!(m.parse_id_text(&w("Zoom7")), 0); // 116 は列挙外
    }

    #[test]
    fn id_to_index_picks_correct_range() {
        let mut m = new_mgr();
        m.register_command_single(110, &w("A"), None, &[], &[], CommandState::NONE);
        m.register_command(130, 140, 140, &w("B"), None, &[], &[], CommandState::NONE);
        assert!(m.is_command_valid(110));
        assert!(!m.is_command_valid(120)); // 隙間
        assert!(m.is_command_valid(135));
        assert!(!m.is_command_valid(141));
    }

    #[test]
    fn invoke_command_calls_handler() {
        let mut m = new_mgr();
        let seen = Rc::new(RefCell::new(None));
        let seen2 = Rc::clone(&seen);
        let handler: CommandHandler = Box::new(move |p: &mut InvokeParameters| {
            *seen2.borrow_mut() = Some((p.id, p.flags));
            true
        });
        m.register_command_single(110, &w("Play"), Some(handler), &[], &[], CommandState::NONE);

        assert!(m.invoke_command(110, InvokeFlag::MOUSE));
        assert_eq!(*seen.borrow(), Some((110, InvokeFlag::MOUSE)));
        // ハンドラ無し / 無効 ID
        m.register_command_single(111, &w("NoOp"), None, &[], &[], CommandState::NONE);
        assert!(!m.invoke_command(111, InvokeFlag::NONE));
        assert!(!m.invoke_command(999, InvokeFlag::NONE));
    }

    #[test]
    fn command_state_set_get() {
        let mut m = new_mgr();
        m.register_command_single(110, &w("A"), None, &[], &[], CommandState::DISABLED);
        assert_eq!(m.get_command_state(110), CommandState::DISABLED);
        // マスク指定で CHECKED のみ操作
        assert!(m.set_command_state_masked(110, CommandState::CHECKED, CommandState::CHECKED));
        let s = m.get_command_state(110);
        assert!(s.contains(CommandState::DISABLED));
        assert!(s.contains(CommandState::CHECKED));
        // 全マスクで上書き
        assert!(m.set_command_state(110, CommandState::NONE));
        assert_eq!(m.get_command_state(110), CommandState::NONE);
        // 範囲外
        assert!(!m.set_command_state(999, CommandState::CHECKED));
        assert_eq!(m.get_command_state(999), CommandState::NONE);
    }

    struct RecordingListener {
        states: Rc<RefCell<Vec<(i32, CommandState, CommandState)>>>,
        radios: Rc<RefCell<Vec<(i32, i32, i32)>>>,
    }

    impl CommandEventListener for RecordingListener {
        fn on_command_state_changed(&self, id: i32, old_state: CommandState, new_state: CommandState) {
            self.states.borrow_mut().push((id, old_state, new_state));
        }
        fn on_command_radio_checked_state_changed(&self, first_id: i32, last_id: i32, checked_id: i32) {
            self.radios.borrow_mut().push((first_id, last_id, checked_id));
        }
    }

    #[test]
    fn event_listener_notifications() {
        let mut m = new_mgr();
        m.register_command_single(110, &w("A"), None, &[], &[], CommandState::NONE);
        let states = Rc::new(RefCell::new(Vec::new()));
        let radios = Rc::new(RefCell::new(Vec::new()));
        let handle = m.add_event_listener(Box::new(RecordingListener {
            states: Rc::clone(&states),
            radios: Rc::clone(&radios),
        }));

        m.set_command_state(110, CommandState::DISABLED);
        // 変化なしの場合は通知されない
        m.set_command_state(110, CommandState::DISABLED);
        assert_eq!(
            *states.borrow(),
            vec![(110, CommandState::NONE, CommandState::DISABLED)]
        );

        // 除去後は通知されない
        assert!(m.remove_event_listener(handle));
        assert!(!m.remove_event_listener(handle)); // 二重除去は false
        m.set_command_state(110, CommandState::NONE);
        assert_eq!(states.borrow().len(), 1);
    }

    #[test]
    fn radio_checked_state() {
        let mut m = new_mgr();
        for id in 110..=112 {
            m.register_command_single(id, &w("R"), None, &[], &[], CommandState::NONE);
        }
        let radios = Rc::new(RefCell::new(Vec::new()));
        m.add_event_listener(Box::new(RecordingListener {
            states: Rc::new(RefCell::new(Vec::new())),
            radios: Rc::clone(&radios),
        }));

        assert!(m.set_command_radio_checked_state(110, 112, 111));
        assert!(!m.get_command_state(110).contains(CommandState::CHECKED));
        assert!(m.get_command_state(111).contains(CommandState::CHECKED));
        assert!(!m.get_command_state(112).contains(CommandState::CHECKED));
        assert_eq!(*radios.borrow(), vec![(110, 112, 111)]);
    }

    struct PrefixCustomizer;
    impl CommandCustomizer for PrefixCustomizer {
        fn is_command_valid(&self, command: i32) -> bool {
            command == 110
        }
        fn get_command_text(&self, _command: i32) -> Option<Vec<u16>> {
            Some(w("カスタム"))
        }
    }

    #[test]
    fn command_text_resolution() {
        let mut m = new_mgr();
        m.register_command_single(110, &w("A"), None, &w("登録テキスト"), &w("短"), CommandState::NONE);
        m.register_command_single(111, &w("B"), None, &[], &[], CommandState::NONE);

        // 登録テキストが返る
        assert_eq!(m.get_command_text(110), Some(w("登録テキスト")));
        assert_eq!(m.get_command_short_text(110), Some(w("短")));
        // テキスト未登録 + リソースフォールバック対象外 -> None
        assert_eq!(m.get_command_text(111), None);
        // 無効 ID
        assert_eq!(m.get_command_text(999), None);

        // カスタマイザ優先
        m.add_command_customizer(Box::new(PrefixCustomizer));
        assert_eq!(m.get_command_text(110), Some(w("カスタム")));
        assert_eq!(m.get_command_text(111), None); // カスタマイザ対象外
    }

    #[test]
    fn lister_enumerates_listing_commands() {
        let mut m = new_mgr();
        // 単一コマンドも LastListingID=ID(≠0)かつ ID テキストありなので列挙対象。
        m.register_command_single(110, &w("Single"), None, &[], &[], CommandState::NONE);
        // 列挙対象の範囲コマンド(112..=114)
        m.register_command(112, 114, 114, &w("Multi"), None, &[], &[], CommandState::NONE);
        // ID テキスト無し -> 列挙されない
        m.register_command(120, 122, 122, &[], None, &[], &[], CommandState::NONE);
        // LastListingID=0 -> 列挙されない
        m.register_command(130, 132, 0, &w("NoList"), None, &[], &[], CommandState::NONE);

        let mut lister = m.lister();
        let mut ids = Vec::new();
        loop {
            let id = lister.next_id();
            if id == 0 {
                break;
            }
            ids.push(id);
        }
        assert_eq!(ids, vec![110, 112, 113, 114]);

        // Reset で再列挙できる
        lister.reset();
        assert_eq!(lister.next_id(), 110);
    }
}
