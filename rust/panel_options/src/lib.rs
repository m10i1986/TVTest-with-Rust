//! TVTest のパネル設定(`src/PanelOptions.cpp` / `src/PanelOptions.h`)の純粋ロジックを
//! 移植したクレート。
//!
//! 移植対象:
//! - パネル ID 定数 `PANEL_ID_*` / `NUM_PANELS` / `PANEL_ID_FIRST` / `PANEL_ID_LAST`
//!   (`PanelOptions.h:35-44`)。
//! - コンストラクタの既定アイテム一覧(`PanelOptions.cpp:41-60`)。
//! - `CompareID`(`PanelOptions.h:84-87`、大小無視比較)。
//! - `GetItemIDFromIDText`(`PanelOptions.cpp:587-595`)。
//! - `GetInitialTab`(`PanelOptions.cpp:222-248`、数値/テキスト ID 解決・
//!   `InitialTab` → `LastTab` の順にフォールバック)。
//! - `RegisterPanelItem`(`PanelOptions.cpp:251-274`、ID 重複チェック+登録)。
//! - `SetPanelItemVisibility` / `GetPanelItemVisibility`
//!   (`PanelOptions.cpp:277-310`)。
//! - `ApplyItemList` のタブ順序構築ロジック(`PanelOptions.cpp:313-342`、
//!   `SetTabVisible`/`SetTabOrder` の実呼び出しを除いた `TabOrder` の計算部分)。
//! - `ReadSettings` の `PanelTab{}_ID` / `PanelTab{}_Visible` パース
//!   (`PanelOptions.cpp:117-170`、数値/テキスト ID 判定・範囲チェック・重複排除)。
//!
//! 対象外(Win32 / CSettings / UI 依存):
//! - `DlgProc`(ダイアログプロシージャ、リストビュー操作)。
//! - `ReadSettings` / `WriteSettings` の `CSettings` I/O 本体。
//! - `InitializePanelForm` / `Create` 等、`CPanelForm` への実適用。
//! - `StyleUtil` によるフォント設定。
//! - `UpdateItemListControlsState`(リストビューの選択状態に応じたボタン活性/非活性)。

#![forbid(unsafe_code)]

/// `PANEL_ID_INFORMATION`(`PanelOptions.h:36`)。
pub const PANEL_ID_INFORMATION: i32 = 0;
/// `PANEL_ID_PROGRAMLIST`(`PanelOptions.h:37`)。
pub const PANEL_ID_PROGRAMLIST: i32 = 1;
/// `PANEL_ID_CHANNEL`(`PanelOptions.h:38`)。
pub const PANEL_ID_CHANNEL: i32 = 2;
/// `PANEL_ID_CONTROL`(`PanelOptions.h:39`)。
pub const PANEL_ID_CONTROL: i32 = 3;
/// `PANEL_ID_CAPTION`(`PanelOptions.h:40`)。
pub const PANEL_ID_CAPTION: i32 = 4;
/// `NUM_PANELS`(`PanelOptions.h:41`)。
pub const NUM_PANELS: i32 = 5;
/// `PANEL_ID_FIRST`(`PanelOptions.h:42`、`= PANEL_ID_INFORMATION`)。
pub const PANEL_ID_FIRST: i32 = PANEL_ID_INFORMATION;
/// `PANEL_ID_LAST`(`PanelOptions.h:43`、`= PANEL_ID_CAPTION`)。
pub const PANEL_ID_LAST: i32 = PANEL_ID_CAPTION;

/// パネル 1 件分の情報(`CPanelOptions::PanelItemInfo`、`PanelOptions.h:75-80`)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelItemInfo {
    pub id: String,
    pub title: String,
    pub visible: bool,
}

impl PanelItemInfo {
    #[must_use]
    pub fn new(id: impl Into<String>, title: impl Into<String>, visible: bool) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            visible,
        }
    }
}

/// コンストラクタの既定アイテム一覧(`PanelOptions.cpp:41-60`)。
/// `(ID, タイトル)` の組。
pub const DEFAULT_ITEM_LIST: [(&str, &str); 5] = [
    ("Information", "情報"),
    ("ProgramList", "番組表"),
    ("Channel", "チャンネル"),
    ("Control", "操作"),
    ("Caption", "字幕"),
];

/// コンストラクタ相当: 既定アイテム一覧から `avail_item_list` / `item_list` を作る
/// (`PanelOptions.cpp:52-60`。両リストとも同一内容で初期化される)。
#[must_use]
pub fn default_item_list() -> Vec<PanelItemInfo> {
    DEFAULT_ITEM_LIST
        .iter()
        .map(|&(id, title)| PanelItemInfo::new(id, title, true))
        .collect()
}

/// `CompareID`(`PanelOptions.h:84-87`、`StringUtility::IsEqualNoCase` 相当)。
/// ASCII 範囲のみ大小無視で比較する近似実装。
#[must_use]
pub fn compare_id(id1: &str, id2: &str) -> bool {
    id1.eq_ignore_ascii_case(id2)
}

/// `GetItemIDFromIDText`(`PanelOptions.cpp:587-595`)。
/// `avail_item_list` を先頭から線形検索し、一致するインデックスを返す。
#[must_use]
pub fn get_item_id_from_id_text(avail_item_list: &[PanelItemInfo], id_text: &str) -> Option<i32> {
    avail_item_list
        .iter()
        .position(|item| compare_id(&item.id, id_text))
        .map(|i| i as i32)
}

/// `InitialTab` / `LastTab` 文字列を数値 ID かテキスト ID かに応じて解決する共通処理
/// (`GetInitialTab` の各ブロック、`PanelOptions.cpp:226-245` の `std::_tcstol` 判定部分)。
///
/// 数値として完全にパースできればその値を範囲チェックした上で返し、そうでなければ
/// `GetItemIDFromIDText` によるテキスト解決を試みる。
fn resolve_tab_id(avail_item_list: &[PanelItemInfo], text: &str) -> Option<i32> {
    if let Ok(num) = text.parse::<i32>() {
        if num >= 0 && (num as usize) < avail_item_list.len() {
            return Some(num);
        }
        return None;
    }
    get_item_id_from_id_text(avail_item_list, text)
}

/// `GetInitialTab`(`PanelOptions.cpp:222-248`)。
/// `initial_tab` が空でなければそれを解決し、失敗すれば `last_tab` にフォールバックする。
/// どちらも解決できなければ `None`(原実装は `-1`)。
#[must_use]
pub fn get_initial_tab(
    avail_item_list: &[PanelItemInfo],
    initial_tab: &str,
    last_tab: &str,
) -> Option<i32> {
    if !initial_tab.is_empty() {
        if let Some(id) = resolve_tab_id(avail_item_list, initial_tab) {
            return Some(id);
        }
    }
    if !last_tab.is_empty() {
        if let Some(id) = resolve_tab_id(avail_item_list, last_tab) {
            return Some(id);
        }
    }
    None
}

/// `RegisterPanelItem`(`PanelOptions.cpp:251-274`)。
/// `id` / `title` が空、または `id` が既に `avail_item_list` に存在する場合は登録せず
/// `None` を返す(原実装は `-1`)。成功時は `avail_item_list` に追加した上で、
/// `item_list` に同一 ID が無ければそちらにも追加し、新しい `avail_item_list` 上の
/// インデックスを返す。
pub fn register_panel_item(
    avail_item_list: &mut Vec<PanelItemInfo>,
    item_list: &mut Vec<PanelItemInfo>,
    id: &str,
    title: &str,
) -> Option<i32> {
    if id.is_empty() || title.is_empty() {
        return None;
    }
    if get_item_id_from_id_text(avail_item_list, id).is_some() {
        return None;
    }

    let item = PanelItemInfo::new(id, title, true);
    avail_item_list.push(item.clone());

    if !item_list.iter().any(|existing| compare_id(&existing.id, id)) {
        item_list.push(item);
    }

    Some(avail_item_list.len() as i32 - 1)
}

/// `SetPanelItemVisibility`(`PanelOptions.cpp:277-295`)。
/// `id` が範囲外なら `false`。範囲内なら `avail_item_list` を更新し、対応する
/// `item_list` の項目があればそちらも更新する(`Panel.Form.SetTabVisible` の実呼び出しは
/// 呼び出し側の責務)。
pub fn set_panel_item_visibility(
    avail_item_list: &mut [PanelItemInfo],
    item_list: &mut [PanelItemInfo],
    id: i32,
    visible: bool,
) -> bool {
    if id < 0 || (id as usize) >= avail_item_list.len() {
        return false;
    }

    avail_item_list[id as usize].visible = visible;
    let id_text = avail_item_list[id as usize].id.clone();

    for item in item_list.iter_mut() {
        if compare_id(&item.id, &id_text) {
            item.visible = visible;
            break;
        }
    }

    true
}

/// `GetPanelItemVisibility`(`PanelOptions.cpp:298-310`)。
/// `id` が範囲外なら `false`。`item_list` に対応項目があればその可視性、
/// 無ければ `avail_item_list` の既定可視性を返す。
#[must_use]
pub fn get_panel_item_visibility(
    avail_item_list: &[PanelItemInfo],
    item_list: &[PanelItemInfo],
    id: i32,
) -> bool {
    if id < 0 || (id as usize) >= avail_item_list.len() {
        return false;
    }

    let id_text = &avail_item_list[id as usize].id;
    for item in item_list {
        if compare_id(&item.id, id_text) {
            return item.visible;
        }
    }

    avail_item_list[id as usize].visible
}

/// `ApplyItemList` のタブ順序構築ロジック(`PanelOptions.cpp:313-342`)。
/// `item_list` の順にテキスト ID を解決してタブ順序へ積み、`avail_item_list` の件数に
/// 満たなければ未出現の ID を昇順に補う。戻り値は `(タブID, 可視性)` の列
/// (`SetTabVisible`/`SetTabOrder` の実呼び出しは呼び出し側の責務)。
#[must_use]
pub fn build_tab_order(
    avail_item_list: &[PanelItemInfo],
    item_list: &[PanelItemInfo],
) -> Vec<(i32, bool)> {
    let mut tab_order: Vec<(i32, bool)> = Vec::with_capacity(avail_item_list.len());

    for item in item_list {
        if let Some(id) = get_item_id_from_id_text(avail_item_list, &item.id) {
            tab_order.push((id, item.visible));
        }
    }

    if tab_order.len() < avail_item_list.len() {
        for i in 0..avail_item_list.len() as i32 {
            if !tab_order.iter().any(|&(id, _)| id == i) {
                tab_order.push((i, avail_item_list[i as usize].visible));
            }
        }
    }

    tab_order
}

/// `ReadSettings` の `PanelTab{}_ID` / `PanelTab{}_Visible` パース
/// (`PanelOptions.cpp:117-170`)。各エントリは `(id_text, visible)` で、`id_text` は
/// 設定ファイルから読んだ生の文字列(数値でもテキストでもよい)、`visible` は
/// 対応する `PanelTab{}_Visible` の読み取り結果(`None` は読み取り失敗=既定 `true`)。
///
/// 戻り値は新しい `item_list`(数値 ID は `avail_item_list` のテキスト ID に解決済み)。
/// `id_text` が空、範囲外の数値、または大小無視で既出の ID は読み飛ばす。
#[must_use]
pub fn load_item_list(
    avail_item_list: &[PanelItemInfo],
    entries: &[(&str, Option<bool>)],
) -> Vec<PanelItemInfo> {
    let mut item_list: Vec<PanelItemInfo> = Vec::new();

    for &(id_text, visible) in entries {
        if id_text.is_empty() {
            continue;
        }

        let resolved_id = if let Ok(num) = id_text.parse::<i32>() {
            if !(PANEL_ID_FIRST..=PANEL_ID_LAST).contains(&num) {
                continue;
            }
            avail_item_list[num as usize].id.clone()
        } else {
            id_text.to_string()
        };

        if item_list.iter().any(|item| compare_id(&item.id, &resolved_id)) {
            continue;
        }

        item_list.push(PanelItemInfo::new(resolved_id, "", visible.unwrap_or(true)));
    }

    item_list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_id_constants() {
        assert_eq!(PANEL_ID_FIRST, 0);
        assert_eq!(PANEL_ID_LAST, 4);
        assert_eq!(NUM_PANELS, 5);
    }

    #[test]
    fn default_item_list_contents() {
        let list = default_item_list();
        assert_eq!(list.len(), 5);
        assert_eq!(list[0], PanelItemInfo::new("Information", "情報", true));
        assert_eq!(list[4], PanelItemInfo::new("Caption", "字幕", true));
        assert!(list.iter().all(|i| i.visible));
    }

    #[test]
    fn compare_id_case_insensitive() {
        assert!(compare_id("Channel", "channel"));
        assert!(compare_id("CHANNEL", "Channel"));
        assert!(!compare_id("Channel", "Control"));
    }

    #[test]
    fn get_item_id_from_id_text_found_and_not_found() {
        let list = default_item_list();
        assert_eq!(get_item_id_from_id_text(&list, "channel"), Some(2));
        assert_eq!(get_item_id_from_id_text(&list, "Caption"), Some(4));
        assert_eq!(get_item_id_from_id_text(&list, "Unknown"), None);
    }

    #[test]
    fn get_initial_tab_numeric_initial() {
        let list = default_item_list();
        assert_eq!(get_initial_tab(&list, "2", ""), Some(2));
    }

    #[test]
    fn get_initial_tab_text_initial() {
        let list = default_item_list();
        assert_eq!(get_initial_tab(&list, "Caption", ""), Some(4));
    }

    #[test]
    fn get_initial_tab_numeric_out_of_range_falls_back() {
        let list = default_item_list();
        assert_eq!(get_initial_tab(&list, "99", "Control"), Some(3));
    }

    #[test]
    fn get_initial_tab_falls_back_to_last_tab() {
        let list = default_item_list();
        assert_eq!(get_initial_tab(&list, "", "Channel"), Some(2));
        assert_eq!(get_initial_tab(&list, "Unknown", "Channel"), Some(2));
    }

    #[test]
    fn get_initial_tab_none_when_both_unresolved() {
        let list = default_item_list();
        assert_eq!(get_initial_tab(&list, "", ""), None);
        assert_eq!(get_initial_tab(&list, "Unknown", "AlsoUnknown"), None);
    }

    #[test]
    fn register_panel_item_success() {
        let mut avail = default_item_list();
        let mut items = default_item_list();
        let id = register_panel_item(&mut avail, &mut items, "Plugin1", "プラグイン1");
        assert_eq!(id, Some(5));
        assert_eq!(avail.len(), 6);
        assert_eq!(items.len(), 6);
        assert_eq!(items[5], PanelItemInfo::new("Plugin1", "プラグイン1", true));
    }

    #[test]
    fn register_panel_item_rejects_empty() {
        let mut avail = default_item_list();
        let mut items = default_item_list();
        assert_eq!(register_panel_item(&mut avail, &mut items, "", "Title"), None);
        assert_eq!(register_panel_item(&mut avail, &mut items, "ID", ""), None);
        assert_eq!(avail.len(), 5);
    }

    #[test]
    fn register_panel_item_rejects_duplicate() {
        let mut avail = default_item_list();
        let mut items = default_item_list();
        assert_eq!(
            register_panel_item(&mut avail, &mut items, "channel", "重複"),
            None
        );
        assert_eq!(avail.len(), 5);
    }

    #[test]
    fn register_panel_item_does_not_duplicate_in_item_list() {
        let mut avail = default_item_list();
        let mut items: Vec<PanelItemInfo> = Vec::new();
        items.push(PanelItemInfo::new("Plugin1", "プラグイン1", false));
        let id = register_panel_item(&mut avail, &mut items, "Plugin1", "別名");
        // avail には無いので登録できる
        assert_eq!(id, Some(5));
        // item_list には既に Plugin1 があるので追加されない
        assert_eq!(items.len(), 1);
        assert!(!items[0].visible);
    }

    #[test]
    fn set_panel_item_visibility_updates_both_lists() {
        let mut avail = default_item_list();
        let mut items = default_item_list();
        assert!(set_panel_item_visibility(&mut avail, &mut items, 2, false));
        assert!(!avail[2].visible);
        assert!(!items[2].visible);
    }

    #[test]
    fn set_panel_item_visibility_out_of_range() {
        let mut avail = default_item_list();
        let mut items = default_item_list();
        assert!(!set_panel_item_visibility(&mut avail, &mut items, -1, false));
        assert!(!set_panel_item_visibility(&mut avail, &mut items, 5, false));
    }

    #[test]
    fn get_panel_item_visibility_from_item_list() {
        let avail = default_item_list();
        let mut items = default_item_list();
        items[2].visible = false;
        assert!(!get_panel_item_visibility(&avail, &items, 2));
    }

    #[test]
    fn get_panel_item_visibility_falls_back_to_avail() {
        let avail = default_item_list();
        let items: Vec<PanelItemInfo> = Vec::new();
        assert!(get_panel_item_visibility(&avail, &items, 0));
    }

    #[test]
    fn get_panel_item_visibility_out_of_range() {
        let avail = default_item_list();
        let items = default_item_list();
        assert!(!get_panel_item_visibility(&avail, &items, -1));
        assert!(!get_panel_item_visibility(&avail, &items, 5));
    }

    #[test]
    fn build_tab_order_identity_when_item_list_matches() {
        let avail = default_item_list();
        let items = default_item_list();
        let order = build_tab_order(&avail, &items);
        assert_eq!(
            order,
            vec![(0, true), (1, true), (2, true), (3, true), (4, true)]
        );
    }

    #[test]
    fn build_tab_order_reordered() {
        let avail = default_item_list();
        let mut items = default_item_list();
        items.swap(0, 4);
        let order = build_tab_order(&avail, &items);
        assert_eq!(order[0].0, 4);
        assert_eq!(order[4].0, 0);
    }

    #[test]
    fn build_tab_order_fills_missing_items() {
        let avail = default_item_list();
        // item_list には Channel のみ
        let items = vec![PanelItemInfo::new("Channel", "チャンネル", false)];
        let order = build_tab_order(&avail, &items);
        assert_eq!(order.len(), 5);
        assert_eq!(order[0], (2, false));
        // 残りは avail の既定可視性を伴って ID 昇順に補われる
        assert_eq!(order[1], (0, true));
        assert_eq!(order[2], (1, true));
        assert_eq!(order[3], (3, true));
        assert_eq!(order[4], (4, true));
    }

    #[test]
    fn build_tab_order_skips_unresolvable_ids() {
        let avail = default_item_list();
        let items = vec![PanelItemInfo::new("UnknownPlugin", "", true)];
        let order = build_tab_order(&avail, &items);
        // 未解決の ID は無視され、全既定アイテムが昇順で補われる
        assert_eq!(order.len(), 5);
        assert_eq!(order[0], (0, true));
    }

    #[test]
    fn load_item_list_numeric_id_resolves_to_text() {
        let avail = default_item_list();
        let entries = [("2", Some(false))];
        let items = load_item_list(&avail, &entries);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "Channel");
        assert!(!items[0].visible);
    }

    #[test]
    fn load_item_list_text_id_kept_as_is() {
        let avail = default_item_list();
        let entries = [("PluginX", Some(true))];
        let items = load_item_list(&avail, &entries);
        assert_eq!(items[0].id, "PluginX");
    }

    #[test]
    fn load_item_list_missing_visible_defaults_true() {
        let avail = default_item_list();
        let entries = [("Channel", None)];
        let items = load_item_list(&avail, &entries);
        assert!(items[0].visible);
    }

    #[test]
    fn load_item_list_skips_empty_and_out_of_range_and_duplicates() {
        let avail = default_item_list();
        let entries = [
            ("", Some(true)),
            ("99", Some(true)),
            ("-1", Some(true)),
            ("channel", Some(true)),
            ("Channel", Some(false)),
        ];
        let items = load_item_list(&avail, &entries);
        assert_eq!(items.len(), 1);
        // "channel"(テキストID)が先に登録され、後続の "Channel" は大小無視の重複として読み飛ばされる
        assert_eq!(items[0].id, "channel");
        assert!(items[0].visible);
    }
}
