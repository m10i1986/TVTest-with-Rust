//! TVTest のメニュー設定(`src/MenuOptions.cpp` / `MenuOptions.h`)の純粋ロジックを移植したクレート。
//!
//! 移植対象は `CMenuOptions` のうちプラットフォーム非依存な部分:
//! - メインメニュー項目の既定テーブル(`m_DefaultMenuItemList`)と追加項目範囲(`m_AdditionalItemList`)
//! - 項目 ID ↔ コマンド ID の相互変換(`IDToCommand` / `CommandToID`)とサブメニュー位置解決
//!   (`GetSubMenuPosByCommand`)
//! - 表示項目リストの抽出(`GetMenuItemList`)
//! - 設定読み込み時の項目リスト構築(`ReadSettings`)・既定一致判定/直列化(`WriteSettings`)
//! - ダイアログ初期化時の既定/追加項目マージ(`DlgProc` WM_INITDIALOG)とダイアログ結果反映(PSN_APPLY)
//!
//! ダイアログ(`DlgProc`)・ListView・`LoadString`(項目テキスト)・`CSettings` I/O は対象外。
//! `CCommandManager`(コマンドテキスト→コマンド ID 変換 `ParseIDText`、コマンド有効判定
//! `IsCommandValid`)はクロージャで注入する。`MenuInfo` の `TextID` は `LoadString` 専用で
//! 純粋ロジックに不要なため省略し、テーブルは `id`/`command` のみ保持する。

/// 区切り項目を表す ID(MenuOptions.h:83)。
pub const MENU_ID_SEPARATOR: i32 = -1;
/// 無効/未解決の ID(MenuOptions.h:84)。
pub const MENU_ID_INVALID: i32 = -2;

/// コマンド ID の下限(resource.h:154)。サブメニュー位置(`SUBMENU_*`)はこれ未満。
pub const CM_COMMAND_FIRST: i32 = 100;

/// 設定の項目状態フラグ「表示」(MenuOptions.cpp:39)。
pub const ITEM_STATE_VISIBLE: i32 = 0x0001;

/// `MaxChannelMenuRows` の既定値(MenuOptions.h:89)。
pub const DEFAULT_MAX_CHANNEL_MENU_ROWS: i32 = 24;
/// `MaxChannelMenuEventInfo` の既定値(MenuOptions.h:90)。
pub const DEFAULT_MAX_CHANNEL_MENU_EVENT_INFO: i32 = 30;

// 追加項目(プラグイン)範囲(resource.h:442-443, 490-491)。
/// プラグインコマンド範囲の先頭。
pub const CM_PLUGIN_FIRST: i32 = 3000;
/// プラグインコマンド範囲の末尾。
pub const CM_PLUGIN_LAST: i32 = 3999;
/// プラグイン拡張コマンド範囲の先頭。
pub const CM_PLUGINCOMMAND_FIRST: i32 = 15000;
/// プラグイン拡張コマンド範囲の末尾。
pub const CM_PLUGINCOMMAND_LAST: i32 = 15999;

// 既定メニューテーブルで参照するコマンド ID(resource.h / Menu.h)。
// 直接コマンド(ID == Command)。
const CM_FULLSCREEN: i32 = 137;
const CM_ALWAYSONTOP: i32 = 138;
const CM_RECORD: i32 = 150;
const CM_RECORDOPTION: i32 = 154;
const CM_DISABLEVIEWER: i32 = 161;
const CM_COPYIMAGE: i32 = 162;
const CM_SAVEIMAGE: i32 = 163;
const CM_CAPTUREPREVIEW: i32 = 165;
const CM_PANEL: i32 = 203;
const CM_PROGRAMGUIDE: i32 = 204;
const CM_OPTIONS: i32 = 214;
const CM_STREAMINFO: i32 = 220;
const CM_CLOSE: i32 = 221;
const CM_1SEGMODE: i32 = 236;
// サブメニュー位置(ID。CMainMenu::SUBMENU_*、Menu.h:51-64)。
const SUBMENU_ZOOM: i32 = 0;
const SUBMENU_ASPECTRATIO: i32 = 1;
const SUBMENU_CHANNEL: i32 = 5;
const SUBMENU_SERVICE: i32 = 6;
const SUBMENU_SPACE: i32 = 7;
const SUBMENU_FAVORITES: i32 = 8;
const SUBMENU_CHANNELHISTORY: i32 = 9;
const SUBMENU_VOLUME: i32 = 12;
const SUBMENU_AUDIO: i32 = 13;
const SUBMENU_VIDEO: i32 = 14;
const SUBMENU_RESET: i32 = 24;
const SUBMENU_BAR: i32 = 28;
const SUBMENU_PLUGIN: i32 = 29;
const SUBMENU_FILTERPROPERTY: i32 = 31;
// サブメニューを開くコマンド(Command。resource.h:291-313)。
const CM_ZOOMMENU: i32 = 300;
const CM_ASPECTRATIOMENU: i32 = 301;
const CM_CHANNELMENU: i32 = 302;
const CM_SERVICEMENU: i32 = 303;
const CM_TUNINGSPACEMENU: i32 = 304;
const CM_FAVORITESMENU: i32 = 305;
const CM_RECENTCHANNELMENU: i32 = 306;
const CM_VOLUMEMENU: i32 = 307;
const CM_AUDIOMENU: i32 = 308;
const CM_VIDEOMENU: i32 = 309;
const CM_RESETMENU: i32 = 310;
const CM_BARMENU: i32 = 311;
const CM_PLUGINMENU: i32 = 312;
const CM_FILTERPROPERTYMENU: i32 = 313;

/// 既定メニュー項目 1 エントリ(MenuOptions.h:63-68 の `MenuInfo`、`TextID` は省略)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuInfo {
    /// 項目 ID(サブメニュー位置 `SUBMENU_*`・コマンド ID・`MENU_ID_SEPARATOR`)。
    pub id: i32,
    /// 対応コマンド ID(区切りは 0)。
    pub command: i32,
}

const fn mi(id: i32, command: i32) -> MenuInfo {
    MenuInfo { id, command }
}

const SEP: MenuInfo = mi(MENU_ID_SEPARATOR, 0);

/// 既定メニュー項目テーブル(MenuOptions.cpp:44-81)。配列順がメニューの既定並び順。
pub static DEFAULT_MENU_ITEM_LIST: [MenuInfo; 35] = [
    mi(SUBMENU_ZOOM, CM_ZOOMMENU),
    mi(SUBMENU_ASPECTRATIO, CM_ASPECTRATIOMENU),
    mi(CM_FULLSCREEN, CM_FULLSCREEN),
    mi(CM_ALWAYSONTOP, CM_ALWAYSONTOP),
    SEP,
    mi(SUBMENU_CHANNEL, CM_CHANNELMENU),
    mi(SUBMENU_SERVICE, CM_SERVICEMENU),
    mi(SUBMENU_SPACE, CM_TUNINGSPACEMENU),
    mi(SUBMENU_FAVORITES, CM_FAVORITESMENU),
    mi(SUBMENU_CHANNELHISTORY, CM_RECENTCHANNELMENU),
    mi(CM_1SEGMODE, CM_1SEGMODE),
    SEP,
    mi(SUBMENU_VOLUME, CM_VOLUMEMENU),
    mi(SUBMENU_AUDIO, CM_AUDIOMENU),
    mi(SUBMENU_VIDEO, CM_VIDEOMENU),
    SEP,
    mi(CM_RECORD, CM_RECORD),
    mi(CM_RECORDOPTION, CM_RECORDOPTION),
    SEP,
    mi(CM_COPYIMAGE, CM_COPYIMAGE),
    mi(CM_SAVEIMAGE, CM_SAVEIMAGE),
    mi(CM_CAPTUREPREVIEW, CM_CAPTUREPREVIEW),
    SEP,
    mi(CM_DISABLEVIEWER, CM_DISABLEVIEWER),
    mi(SUBMENU_RESET, CM_RESETMENU),
    SEP,
    mi(CM_PANEL, CM_PANEL),
    mi(CM_PROGRAMGUIDE, CM_PROGRAMGUIDE),
    mi(SUBMENU_BAR, CM_BARMENU),
    mi(SUBMENU_PLUGIN, CM_PLUGINMENU),
    mi(CM_OPTIONS, CM_OPTIONS),
    mi(SUBMENU_FILTERPROPERTY, CM_FILTERPROPERTYMENU),
    mi(CM_STREAMINFO, CM_STREAMINFO),
    SEP,
    mi(CM_CLOSE, CM_CLOSE),
];

/// 追加項目(プラグイン)範囲(MenuOptions.cpp:83-86 の `m_AdditionalItemList`)。
pub static ADDITIONAL_ITEM_LIST: [(i32, i32); 2] = [
    (CM_PLUGIN_FIRST, CM_PLUGIN_LAST),
    (CM_PLUGINCOMMAND_FIRST, CM_PLUGINCOMMAND_LAST),
];

/// 項目 ID → コマンド ID(MenuOptions.cpp:234-241 の `IDToCommand`)。
/// テーブルに無ければ ID をそのまま返す。
pub fn id_to_command(id: i32) -> i32 {
    DEFAULT_MENU_ITEM_LIST
        .iter()
        .find(|e| e.id == id)
        .map(|e| e.command)
        .unwrap_or(id)
}

/// コマンド ID → 項目 ID(MenuOptions.cpp:244-251 の `CommandToID`)。
/// テーブルに無ければコマンド ID をそのまま返す。
pub fn command_to_id(command: i32) -> i32 {
    DEFAULT_MENU_ITEM_LIST
        .iter()
        .find(|e| e.command == command)
        .map(|e| e.id)
        .unwrap_or(command)
}

/// コマンドに対応するサブメニュー位置(MenuOptions.cpp:221-231 の `GetSubMenuPosByCommand`)。
///
/// コマンドがテーブルにあり、その ID が `CM_COMMAND_FIRST` 未満(= サブメニュー位置)なら ID を、
/// それ以外(通常コマンド)や未登録は -1 を返す。
pub fn sub_menu_pos_by_command(command: i32) -> i32 {
    for item in DEFAULT_MENU_ITEM_LIST.iter() {
        if item.command == command {
            if item.id < CM_COMMAND_FIRST {
                return item.id;
            }
            break;
        }
    }
    -1
}

/// 文字列(コマンドテキスト)から項目 ID を解決(MenuOptions.cpp:254-264 の `GetIDFromString`)。
///
/// 空文字列は区切り(`MENU_ID_SEPARATOR`)。それ以外は `parsed_command`(`ParseIDText` の結果)が
/// 正なら `command_to_id`、0 以下なら `MENU_ID_INVALID`。コマンドテキストの解決は呼び出し側の責務。
pub fn id_from_string(name: &str, parsed_command: i32) -> i32 {
    if name.is_empty() {
        return MENU_ID_SEPARATOR;
    }
    if parsed_command > 0 {
        command_to_id(parsed_command)
    } else {
        MENU_ID_INVALID
    }
}

/// 実行時メニュー項目(MenuOptions.h:70-75 の `MenuItemInfo`)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuItemInfo {
    /// 設定由来の項目名(コマンドテキスト。ID 未解決時の手掛かり)。
    pub name: String,
    /// 項目 ID(未解決は `MENU_ID_INVALID`)。
    pub id: i32,
    /// 表示するか。
    pub visible: bool,
}

/// `WriteSettings` で 1 項目を直列化した結果(MenuOptions.cpp:175-182)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuWriteId {
    /// ID 未解決 → 項目名をそのまま書き出す。
    Name(String),
    /// 区切り → 空文字列を書き出す。
    Separator,
    /// 通常項目 → このコマンド ID をテキスト化して書き出す(`GetCommandIDText`)。
    Command(i32),
}

/// CMenuOptions のモデル層。チャンネルメニュー行数設定とメニュー項目リストを保持する。
#[derive(Clone, Debug)]
pub struct MenuOptions {
    max_channel_menu_rows: i32,
    max_channel_menu_event_info: i32,
    menu_item_list: Vec<MenuItemInfo>,
}

impl Default for MenuOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl MenuOptions {
    /// 既定値で初期化(項目リストは空 = 既定メニューを使う状態)。
    pub fn new() -> Self {
        Self {
            max_channel_menu_rows: DEFAULT_MAX_CHANNEL_MENU_ROWS,
            max_channel_menu_event_info: DEFAULT_MAX_CHANNEL_MENU_EVENT_INFO,
            menu_item_list: Vec::new(),
        }
    }

    /// チャンネルメニューの最大行数。
    pub fn max_channel_menu_rows(&self) -> i32 {
        self.max_channel_menu_rows
    }

    /// チャンネルメニューの最大行数を設定(MenuOptions.cpp:105。`> 0` のみ反映)。反映で `true`。
    pub fn set_max_channel_menu_rows(&mut self, rows: i32) -> bool {
        if rows > 0 {
            self.max_channel_menu_rows = rows;
            true
        } else {
            false
        }
    }

    /// チャンネルメニューに表示する番組情報の最大数。
    pub fn max_channel_menu_event_info(&self) -> i32 {
        self.max_channel_menu_event_info
    }

    /// チャンネルメニューに表示する番組情報の最大数を設定(MenuOptions.cpp:107。検証なし)。
    pub fn set_max_channel_menu_event_info(&mut self, value: i32) {
        self.max_channel_menu_event_info = value;
    }

    /// 現在のメニュー項目リスト(空 = 既定)。
    pub fn menu_item_list(&self) -> &[MenuItemInfo] {
        &self.menu_item_list
    }

    /// 設定から読み込んだ項目リストを構築(MenuOptions.cpp:109-132 の `ReadSettings` 項目部分)。
    ///
    /// `entries` は `(項目名, 状態値)` の並び。状態値が `Some` なら表示フラグは
    /// `state & ITEM_STATE_VISIBLE != 0`、`None`(設定欠如)なら既定で表示。各項目の ID は
    /// 未解決(`MENU_ID_INVALID`)として保持し、後段で解決する。`entries` が空なら何もしない
    /// (`ItemCount > 0` 不成立に相当)。
    pub fn load_items(&mut self, entries: &[(String, Option<i32>)]) {
        if entries.is_empty() {
            return;
        }
        self.menu_item_list.clear();
        self.menu_item_list.reserve(entries.len());
        for (name, state) in entries {
            let visible = match state {
                Some(value) => (value & ITEM_STATE_VISIBLE) != 0,
                None => true,
            };
            self.menu_item_list.push(MenuItemInfo {
                name: name.clone(),
                id: MENU_ID_INVALID,
                visible,
            });
        }
    }

    /// 表示すべきメニュー項目 ID の並びを取得(MenuOptions.cpp:193-218 の `GetMenuItemList`)。
    ///
    /// 項目リストが空なら既定テーブルの全 ID を返す。そうでなければ可視項目のみを対象に、
    /// ID が未解決なら `resolve`(`ParseIDText` 相当: 名前→コマンド ID、無ければ 0)で解決して
    /// キャッシュし、解決できた(`!= MENU_ID_INVALID`)ものだけを返す。
    pub fn get_menu_item_list<F: Fn(&str) -> i32>(&mut self, resolve: F) -> Vec<i32> {
        if self.menu_item_list.is_empty() {
            return DEFAULT_MENU_ITEM_LIST.iter().map(|e| e.id).collect();
        }
        let mut list = Vec::new();
        for item in &mut self.menu_item_list {
            if item.visible {
                if item.id == MENU_ID_INVALID {
                    item.id = id_from_string(&item.name, resolve(&item.name));
                }
                if item.id != MENU_ID_INVALID {
                    list.push(item.id);
                }
            }
        }
        list
    }

    /// 現在のリストが既定と一致し設定に保存不要か(MenuOptions.cpp:145-167 の `fDefault` 判定)。
    ///
    /// 空リストは保存対象外(`true`)。非空時は、先頭 `DEFAULT_MENU_ITEM_LIST.len()` 項目の ID が
    /// 既定と一致しかつ全て表示で、かつ既定より後ろの追加項目が全て非表示なら `true`。
    /// `true` のとき呼び出し側は項目を保存しない。
    pub fn is_default_for_write(&self) -> bool {
        if self.menu_item_list.is_empty() {
            return true;
        }
        if self.menu_item_list.len() < DEFAULT_MENU_ITEM_LIST.len() {
            return false;
        }
        for (i, def) in DEFAULT_MENU_ITEM_LIST.iter().enumerate() {
            if self.menu_item_list[i].id != def.id || !self.menu_item_list[i].visible {
                return false;
            }
        }
        for item in &self.menu_item_list[DEFAULT_MENU_ITEM_LIST.len()..] {
            if item.visible {
                return false;
            }
        }
        true
    }

    /// 項目リストを設定保存用に直列化(MenuOptions.cpp:171-185 の `WriteSettings` 項目部分)。
    ///
    /// 各項目について `(書き出し ID 表現, 状態値)` を返す。状態値は `表示なら ITEM_STATE_VISIBLE,
    /// 非表示なら 0`。ID 表現は未解決→`Name`、区切り→`Separator`、通常→`Command(id_to_command(id))`。
    pub fn serialize_items(&self) -> Vec<(MenuWriteId, i32)> {
        self.menu_item_list
            .iter()
            .map(|item| {
                let id_repr = if item.id == MENU_ID_INVALID {
                    MenuWriteId::Name(item.name.clone())
                } else if item.id == MENU_ID_SEPARATOR {
                    MenuWriteId::Separator
                } else {
                    MenuWriteId::Command(id_to_command(item.id))
                };
                let state = if item.visible { ITEM_STATE_VISIBLE } else { 0 };
                (id_repr, state)
            })
            .collect()
    }

    /// ダイアログ初期化時の既定/追加項目マージ(MenuOptions.cpp:291-328 の WM_INITDIALOG)。
    ///
    /// 空リストなら既定テーブルから全項目(表示)を構築。非空なら未解決 ID を `resolve` で解決し、
    /// 既定テーブルに在るが現リストに無い ID を非表示で追加、続いて追加項目範囲の各 ID を
    /// `is_valid`(`IsCommandValid` 相当)が真の間だけ非表示で追加する(範囲先頭から最初の無効で打ち切り)。
    /// 既定/追加の重複 ID(区切りなど)は最初の 1 件だけ追加される(`std::ranges::find` の挙動)。
    pub fn merge_for_dialog<R, V>(&mut self, resolve: R, is_valid: V)
    where
        R: Fn(&str) -> i32,
        V: Fn(i32) -> bool,
    {
        if self.menu_item_list.is_empty() {
            self.menu_item_list = DEFAULT_MENU_ITEM_LIST
                .iter()
                .map(|e| MenuItemInfo {
                    name: String::new(),
                    id: e.id,
                    visible: true,
                })
                .collect();
            return;
        }

        for item in &mut self.menu_item_list {
            if item.id == MENU_ID_INVALID {
                item.id = id_from_string(&item.name, resolve(&item.name));
            }
        }

        for def in DEFAULT_MENU_ITEM_LIST.iter() {
            if !self.menu_item_list.iter().any(|item| item.id == def.id) {
                self.menu_item_list.push(MenuItemInfo {
                    name: String::new(),
                    id: def.id,
                    visible: false,
                });
            }
        }

        for &(first, last) in ADDITIONAL_ITEM_LIST.iter() {
            for id in first..=last {
                if !is_valid(id) {
                    break;
                }
                if !self.menu_item_list.iter().any(|item| item.id == id) {
                    self.menu_item_list.push(MenuItemInfo {
                        name: String::new(),
                        id,
                        visible: false,
                    });
                }
            }
        }
    }

    /// ダイアログ結果を反映(MenuOptions.cpp:476-486 の PSN_APPLY)。
    ///
    /// `(項目 ID, 表示)` の並びで項目リストを再構築する(名前は空)。
    pub fn set_from_dialog(&mut self, items: &[(i32, bool)]) {
        self.menu_item_list = items
            .iter()
            .map(|&(id, visible)| MenuItemInfo {
                name: String::new(),
                id,
                visible,
            })
            .collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_to_command_table_and_fallback() {
        assert_eq!(id_to_command(SUBMENU_ZOOM), CM_ZOOMMENU); // 0 → 300
        assert_eq!(id_to_command(CM_FULLSCREEN), CM_FULLSCREEN); // 137 → 137
        assert_eq!(id_to_command(SUBMENU_FILTERPROPERTY), CM_FILTERPROPERTYMENU); // 31 → 313
        assert_eq!(id_to_command(9999), 9999); // 未登録はそのまま
    }

    #[test]
    fn command_to_id_table_and_fallback() {
        assert_eq!(command_to_id(CM_ZOOMMENU), SUBMENU_ZOOM); // 300 → 0
        assert_eq!(command_to_id(CM_FULLSCREEN), CM_FULLSCREEN); // 137 → 137
        assert_eq!(command_to_id(CM_FILTERPROPERTYMENU), SUBMENU_FILTERPROPERTY); // 313 → 31
        assert_eq!(command_to_id(9999), 9999);
    }

    #[test]
    fn sub_menu_pos_only_for_submenu_ids() {
        // サブメニューコマンド → 位置(ID < 100)。
        assert_eq!(sub_menu_pos_by_command(CM_ZOOMMENU), SUBMENU_ZOOM);
        assert_eq!(
            sub_menu_pos_by_command(CM_FILTERPROPERTYMENU),
            SUBMENU_FILTERPROPERTY
        );
        // 通常コマンド(ID >= 100)→ -1。
        assert_eq!(sub_menu_pos_by_command(CM_FULLSCREEN), -1);
        // 未登録 → -1。
        assert_eq!(sub_menu_pos_by_command(9999), -1);
    }

    #[test]
    fn id_from_string_rules() {
        assert_eq!(id_from_string("", 0), MENU_ID_SEPARATOR);
        assert_eq!(id_from_string("", 12345), MENU_ID_SEPARATOR); // 空が最優先
        assert_eq!(id_from_string("ZoomMenu", CM_ZOOMMENU), SUBMENU_ZOOM); // 300 → 0
        assert_eq!(id_from_string("FullScreen", CM_FULLSCREEN), CM_FULLSCREEN);
        assert_eq!(id_from_string("Bad", 0), MENU_ID_INVALID); // 解決不可
        assert_eq!(id_from_string("Bad", -1), MENU_ID_INVALID);
    }

    #[test]
    fn new_defaults() {
        let opt = MenuOptions::new();
        assert_eq!(opt.max_channel_menu_rows(), 24);
        assert_eq!(opt.max_channel_menu_event_info(), 30);
        assert!(opt.menu_item_list().is_empty());
    }

    #[test]
    fn max_rows_validation() {
        let mut opt = MenuOptions::new();
        assert!(!opt.set_max_channel_menu_rows(0));
        assert!(!opt.set_max_channel_menu_rows(-5));
        assert_eq!(opt.max_channel_menu_rows(), 24);
        assert!(opt.set_max_channel_menu_rows(40));
        assert_eq!(opt.max_channel_menu_rows(), 40);
        opt.set_max_channel_menu_event_info(0); // 検証なし
        assert_eq!(opt.max_channel_menu_event_info(), 0);
    }

    #[test]
    fn get_menu_item_list_empty_returns_default_ids() {
        let mut opt = MenuOptions::new();
        let list = opt.get_menu_item_list(|_| 0);
        assert_eq!(list.len(), DEFAULT_MENU_ITEM_LIST.len());
        assert_eq!(list[0], SUBMENU_ZOOM);
        assert!(list.contains(&MENU_ID_SEPARATOR)); // 区切りも含む
        assert_eq!(*list.last().unwrap(), CM_CLOSE);
    }

    // テスト用のコマンドテキスト→コマンド ID 解決(ParseIDText 相当)。
    fn resolver(name: &str) -> i32 {
        match name {
            "FullScreen" => CM_FULLSCREEN,
            "ZoomMenu" => CM_ZOOMMENU,
            _ => 0,
        }
    }

    #[test]
    fn load_and_get_menu_item_list_resolves_and_filters() {
        let mut opt = MenuOptions::new();
        opt.load_items(&[
            ("FullScreen".to_string(), Some(ITEM_STATE_VISIBLE)),
            ("ZoomMenu".to_string(), None), // 状態欠如 → 既定で表示
            ("Bad".to_string(), Some(ITEM_STATE_VISIBLE)), // 解決不可 → INVALID で除外
            ("".to_string(), Some(0)),      // 空=区切り だが非表示
        ]);
        // ロード直後は全 ID 未解決。
        assert!(opt.menu_item_list().iter().all(|i| i.id == MENU_ID_INVALID));
        assert!(opt.menu_item_list()[1].visible); // 状態欠如は表示

        let list = opt.get_menu_item_list(resolver);
        // 表示かつ解決できたもの: FullScreen(137), ZoomMenu→0。Bad は INVALID、空区切りは非表示。
        assert_eq!(list, vec![CM_FULLSCREEN, SUBMENU_ZOOM]);
        // 解決結果がキャッシュされる。
        assert_eq!(opt.menu_item_list()[0].id, CM_FULLSCREEN);
        assert_eq!(opt.menu_item_list()[1].id, SUBMENU_ZOOM);
        assert_eq!(opt.menu_item_list()[2].id, MENU_ID_INVALID);
    }

    #[test]
    fn load_items_empty_is_noop() {
        let mut opt = MenuOptions::new();
        opt.set_from_dialog(&[(CM_FULLSCREEN, true)]);
        opt.load_items(&[]);
        assert_eq!(opt.menu_item_list().len(), 1);
    }

    #[test]
    fn is_default_for_write_cases() {
        let mut opt = MenuOptions::new();
        // 空 → 保存不要。
        assert!(opt.is_default_for_write());

        // 既定そのまま(全表示)→ 保存不要。
        let default_items: Vec<(i32, bool)> = DEFAULT_MENU_ITEM_LIST
            .iter()
            .map(|e| (e.id, true))
            .collect();
        opt.set_from_dialog(&default_items);
        assert!(opt.is_default_for_write());

        // 1 項目を非表示 → 既定でない。
        let mut modified = default_items.clone();
        modified[0].1 = false;
        opt.set_from_dialog(&modified);
        assert!(!opt.is_default_for_write());

        // 末尾に表示項目を追加 → 既定でない。
        let mut extended = default_items.clone();
        extended.push((CM_PLUGIN_FIRST, true));
        opt.set_from_dialog(&extended);
        assert!(!opt.is_default_for_write());

        // 末尾に非表示項目を追加 → 既定のまま。
        let mut extended_hidden = default_items.clone();
        extended_hidden.push((CM_PLUGIN_FIRST, false));
        opt.set_from_dialog(&extended_hidden);
        assert!(opt.is_default_for_write());

        // 既定より短い → 既定でない。
        opt.set_from_dialog(&default_items[..default_items.len() - 1]);
        assert!(!opt.is_default_for_write());
    }

    #[test]
    fn serialize_items_variants() {
        let mut opt = MenuOptions::new();
        // 名前のみ(未解決)・区切り・通常項目。
        opt.menu_item_list = vec![
            MenuItemInfo {
                name: "Unresolved".to_string(),
                id: MENU_ID_INVALID,
                visible: true,
            },
            MenuItemInfo {
                name: String::new(),
                id: MENU_ID_SEPARATOR,
                visible: true,
            },
            MenuItemInfo {
                name: String::new(),
                id: SUBMENU_ZOOM,
                visible: false,
            },
        ];
        let serialized = opt.serialize_items();
        assert_eq!(
            serialized[0],
            (
                MenuWriteId::Name("Unresolved".to_string()),
                ITEM_STATE_VISIBLE
            )
        );
        assert_eq!(serialized[1], (MenuWriteId::Separator, ITEM_STATE_VISIBLE));
        // SUBMENU_ZOOM(0) → コマンド CM_ZOOMMENU(300)、非表示で状態 0。
        assert_eq!(serialized[2], (MenuWriteId::Command(CM_ZOOMMENU), 0));
    }

    #[test]
    fn merge_for_dialog_empty_fills_default() {
        let mut opt = MenuOptions::new();
        opt.merge_for_dialog(resolver, |_| false);
        assert_eq!(opt.menu_item_list().len(), DEFAULT_MENU_ITEM_LIST.len());
        assert!(opt.menu_item_list().iter().all(|i| i.visible));
        assert_eq!(opt.menu_item_list()[0].id, SUBMENU_ZOOM);
    }

    #[test]
    fn merge_for_dialog_appends_missing_default_once() {
        let mut opt = MenuOptions::new();
        opt.load_items(&[("FullScreen".to_string(), Some(ITEM_STATE_VISIBLE))]);
        // プラグインは無効 → 追加なし。
        opt.merge_for_dialog(resolver, |_| false);
        // 既定の一意 ID 数(区切りは 1 件に集約)= 29。
        assert_eq!(opt.menu_item_list().len(), 29);
        // 先頭(ロード項目)は表示、残りは非表示。
        assert!(opt.menu_item_list()[0].visible);
        assert_eq!(opt.menu_item_list()[0].id, CM_FULLSCREEN);
        assert!(opt.menu_item_list()[1..].iter().all(|i| !i.visible));
        // 区切り(-1)はちょうど 1 件。
        assert_eq!(
            opt.menu_item_list()
                .iter()
                .filter(|i| i.id == MENU_ID_SEPARATOR)
                .count(),
            1
        );
    }

    #[test]
    fn merge_for_dialog_appends_valid_additional_items() {
        let mut opt = MenuOptions::new();
        opt.load_items(&[("FullScreen".to_string(), Some(ITEM_STATE_VISIBLE))]);
        // 最初のプラグイン範囲の先頭 3 個だけ有効。
        opt.merge_for_dialog(resolver, |id| {
            (CM_PLUGIN_FIRST..CM_PLUGIN_FIRST + 3).contains(&id)
        });
        // 既定 29 + プラグイン 3 = 32。
        assert_eq!(opt.menu_item_list().len(), 32);
        assert!(opt.menu_item_list().iter().any(|i| i.id == CM_PLUGIN_FIRST));
        assert!(opt
            .menu_item_list()
            .iter()
            .any(|i| i.id == CM_PLUGIN_FIRST + 2));
        assert!(!opt
            .menu_item_list()
            .iter()
            .any(|i| i.id == CM_PLUGIN_FIRST + 3));
    }

    #[test]
    fn set_from_dialog_rebuilds_list() {
        let mut opt = MenuOptions::new();
        opt.set_from_dialog(&[(SUBMENU_ZOOM, true), (CM_FULLSCREEN, false)]);
        assert_eq!(opt.menu_item_list().len(), 2);
        assert_eq!(opt.menu_item_list()[0].id, SUBMENU_ZOOM);
        assert!(opt.menu_item_list()[0].visible);
        assert!(!opt.menu_item_list()[1].visible);
        assert!(opt.menu_item_list().iter().all(|i| i.name.is_empty()));
    }
}
