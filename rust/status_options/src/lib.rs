//! TVTest のステータスバー設定(`src/StatusOptions.cpp` / `src/StatusOptions.h`)の純粋ロジックを
//! 移植したクレート。
//!
//! 移植対象:
//! - `STATUS_ITEM_*` の項目 ID 定数、`STATUS_ITEM_FIRST` / `STATUS_ITEM_LAST`
//!   (`src/StatusItems.h:34-51`)。
//! - コンストラクタの `DefaultItemList`(`StatusOptions.cpp:61-79`)。本クレートは
//!   `IS_HD == true` の通常ビルド(非 `TVTEST_FOR_1SEG`)版のみを対象とする。
//! - `ReadSettings` の `Item{}_ID` 文字列パースのうち、数値/テキスト ID の判定・範囲チェック・
//!   `m_AvailItemList` からの既定可視性引き継ぎ・大小無視での重複排除・`m_AvailItemList` に
//!   存在するが `ItemList` に未出現の項目を可視false で追加する処理
//!   (`StatusOptions.cpp:119-169`)。
//! - `PopupOpacity` のクランプ(`StatusOptions.cpp:186-188`、`OPACITY_MIN`/`OPACITY_MAX` は
//!   `StatusOptions.h:38-39`)。
//!
//! 対象外(Win32 / CSettings / UI 依存):
//! - `DlgProc` とダイアログ全般(アイテムリストのドラッグ&ドロップ、リサイズ等)。
//! - `ReadSettings` / `WriteSettings` の `CSettings` I/O 本体(フォント設定・DPI・MultiRow 等)。
//! - `CStatusView` / `CItemListSubclass` / DPI 処理・フォントの `StyleUtil` 連携。
//! - `ApplyOptions` / `ApplyItemList` / `ApplyItemWidth` 等、実際の UI への適用処理。

#![forbid(unsafe_code)]

/// `STATUS_ITEM_CHANNEL`(`StatusItems.h:35`)。
pub const STATUS_ITEM_CHANNEL: i32 = 0;
/// `STATUS_ITEM_VIDEOSIZE`(`StatusItems.h:36`)。
pub const STATUS_ITEM_VIDEOSIZE: i32 = 1;
/// `STATUS_ITEM_VOLUME`(`StatusItems.h:37`)。
pub const STATUS_ITEM_VOLUME: i32 = 2;
/// `STATUS_ITEM_AUDIOCHANNEL`(`StatusItems.h:38`)。
pub const STATUS_ITEM_AUDIOCHANNEL: i32 = 3;
/// `STATUS_ITEM_RECORD`(`StatusItems.h:39`)。
pub const STATUS_ITEM_RECORD: i32 = 4;
/// `STATUS_ITEM_CAPTURE`(`StatusItems.h:40`)。
pub const STATUS_ITEM_CAPTURE: i32 = 5;
/// `STATUS_ITEM_ERROR`(`StatusItems.h:41`)。
pub const STATUS_ITEM_ERROR: i32 = 6;
/// `STATUS_ITEM_SIGNALLEVEL`(`StatusItems.h:42`)。
pub const STATUS_ITEM_SIGNALLEVEL: i32 = 7;
/// `STATUS_ITEM_CLOCK`(`StatusItems.h:43`)。
pub const STATUS_ITEM_CLOCK: i32 = 8;
/// `STATUS_ITEM_PROGRAMINFO`(`StatusItems.h:44`)。
pub const STATUS_ITEM_PROGRAMINFO: i32 = 9;
/// `STATUS_ITEM_BUFFERING`(`StatusItems.h:45`)。
pub const STATUS_ITEM_BUFFERING: i32 = 10;
/// `STATUS_ITEM_TUNER`(`StatusItems.h:46`)。
pub const STATUS_ITEM_TUNER: i32 = 11;
/// `STATUS_ITEM_MEDIABITRATE`(`StatusItems.h:47`)。
pub const STATUS_ITEM_MEDIABITRATE: i32 = 12;
/// `STATUS_ITEM_FAVORITES`(`StatusItems.h:48`)。
pub const STATUS_ITEM_FAVORITES: i32 = 13;

/// `STATUS_ITEM_FIRST`(`StatusItems.h:49`、`= STATUS_ITEM_CHANNEL`)。
pub const STATUS_ITEM_FIRST: i32 = STATUS_ITEM_CHANNEL;
/// `STATUS_ITEM_LAST`(`StatusItems.h:50`、`= STATUS_ITEM_FAVORITES`)。
pub const STATUS_ITEM_LAST: i32 = STATUS_ITEM_FAVORITES;

/// ポップアップ不透明度の最小値(`StatusOptions.h:38`)。
pub const OPACITY_MIN: i32 = 20;
/// ポップアップ不透明度の最大値(`StatusOptions.h:39`)。
pub const OPACITY_MAX: i32 = 100;

/// コンストラクタの `DefaultItemList`(`StatusOptions.cpp:61-79`)。
///
/// `IS_HD == true`(通常ビルド、非 `TVTEST_FOR_1SEG`)の値で固定している。要素は
/// `(ID, 既定の可視性)` で、`Width` の初期値は常に `-1`(このテーブルには含めず、
/// [`StatusItemInfo`] 生成時に別途 `-1` を設定する)。
pub const DEFAULT_ITEM_LIST: [(i32, bool); 14] = [
    (STATUS_ITEM_TUNER, true),
    (STATUS_ITEM_CHANNEL, true),
    (STATUS_ITEM_FAVORITES, false),
    (STATUS_ITEM_VIDEOSIZE, true),
    (STATUS_ITEM_VOLUME, true),
    (STATUS_ITEM_AUDIOCHANNEL, true),
    (STATUS_ITEM_RECORD, true),
    (STATUS_ITEM_CAPTURE, true),
    (STATUS_ITEM_ERROR, true),
    (STATUS_ITEM_SIGNALLEVEL, true),
    (STATUS_ITEM_CLOCK, false),
    (STATUS_ITEM_PROGRAMINFO, false),
    (STATUS_ITEM_BUFFERING, false),
    (STATUS_ITEM_MEDIABITRATE, false),
];

/// ステータス項目 1 件分の情報(`StatusOptions.h` の `StatusItemInfo`)。
///
/// `id` が `-1` のときのみ `id_text` が有効(未解決のテキスト ID)。それ以外では
/// `id_text` は空にしておく。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusItemInfo {
    pub id: i32,
    pub id_text: Vec<u16>,
    pub visible: bool,
    pub width: i32,
}

impl StatusItemInfo {
    /// 数値 ID が解決済みの項目を作る(`width` は常に `-1` から開始)。
    #[must_use]
    pub fn with_id(id: i32, visible: bool) -> Self {
        Self {
            id,
            id_text: Vec::new(),
            visible,
            width: -1,
        }
    }

    /// 未解決のテキスト ID を持つ項目を作る(`id` は `-1`、`width` は `-1` から開始)。
    #[must_use]
    pub fn with_id_text(id_text: Vec<u16>, visible: bool) -> Self {
        Self {
            id: -1,
            id_text,
            visible,
            width: -1,
        }
    }
}

/// ASCII 範囲のみを大小無視で比較する文字列等価判定(`LibISDB::StringEqualsI` 相当、
/// `StatusOptions.cpp:145`, `314` で使用)。
///
/// 非 ASCII 文字はそのまま(大小無視の畳み込みをせず)比較する。長さが異なれば直ちに
/// 不一致。
#[must_use]
pub fn is_equal_no_case_u16(a: &[u16], b: &[u16]) -> bool {
    if a.len() != b.len() {
        return false;
    }

    a.iter().zip(b.iter()).all(|(&ca, &cb)| {
        if ca == cb {
            return true;
        }
        to_ascii_lower_u16(ca) == to_ascii_lower_u16(cb)
    })
}

fn to_ascii_lower_u16(c: u16) -> u16 {
    if (u16::from(b'A')..=u16::from(b'Z')).contains(&c) {
        c + (u16::from(b'a') - u16::from(b'A'))
    } else {
        c
    }
}

/// `Item{}_ID` 文字列が 10 進整数として全体パース可能かを判定する
/// (`std::_tcstol` + `*p == L'\0'` 相当、`StatusOptions.cpp:119-121`)。
///
/// 先頭の空白・`+`/`-` 符号は許容する。文字列の一部だけが数値として消費され、
/// 末尾に余分な文字が残る場合(例: `"5abc"`)は `None` を返す(=IDテキストとして扱う)。
/// 空文字列や空白のみの文字列も `None`。
#[must_use]
pub fn try_parse_status_item_id(text: &[u16]) -> Option<i32> {
    let s = String::from_utf16(text).ok()?;

    let trimmed = s.trim_start_matches(|c: char| c.is_whitespace());
    if trimmed.is_empty() {
        return None;
    }

    let (sign, digits_part) = match trimmed.strip_prefix('-') {
        Some(rest) => (-1i64, rest),
        None => match trimmed.strip_prefix('+') {
            Some(rest) => (1i64, rest),
            None => (1i64, trimmed),
        },
    };

    if digits_part.is_empty() || !digits_part.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }

    let magnitude: i64 = digits_part.parse().ok()?;
    let value = sign * magnitude;

    i32::try_from(value).ok()
}

/// `Item{}_ID` 文字列から ID を解決する(`StatusOptions.cpp:119-139` の数値/テキスト判定部分)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedItemId {
    /// 数値としてパースでき、`STATUS_ITEM_FIRST..=STATUS_ITEM_LAST` の範囲内だった場合。
    /// `avail_item_list` に同じ ID があれば、その既定可視性を引き継ぐ。
    Numeric { id: i32, visible: bool },
    /// 数値としてパースできなかった場合。ID テキストとして保持し、可視性は `false`。
    Text { id_text: Vec<u16>, visible: bool },
    /// 数値としてパースできたが `STATUS_ITEM_FIRST..=STATUS_ITEM_LAST` の範囲外だった場合。
    /// 原実装ではこのアイテム自体を無視する(`continue`、`StatusOptions.cpp:124-126`)。
    OutOfRange,
}

/// `id_text` から ID を解決し、`avail_item_list` から既定可視性を引き継ぐ
/// (`StatusOptions.cpp:119-151` のうち重複排除を除いた部分)。
///
/// - 数値としてパース可能(`try_parse_status_item_id`)かつ範囲内なら
///   [`ResolvedItemId::Numeric`]。`avail_item_list` に同じ ID があれば `visible` を
///   そこから引き継ぎ、無ければ `false`(呼び出し側の `Item_Visible` 設定で上書きされる
///   想定、`StatusOptions.cpp:134-139`)。
/// - 範囲外の数値なら [`ResolvedItemId::OutOfRange`](呼び出し側でこのエントリ自体を
///   スキップすべき)。
/// - 数値としてパースできないなら [`ResolvedItemId::Text`](`visible` は常に `false`、
///   `StatusOptions.cpp:150`)。
#[must_use]
pub fn resolve_item_id(id_text: &[u16], avail_item_list: &[(i32, bool)]) -> ResolvedItemId {
    match try_parse_status_item_id(id_text) {
        Some(id) => {
            if !(STATUS_ITEM_FIRST..=STATUS_ITEM_LAST).contains(&id) {
                return ResolvedItemId::OutOfRange;
            }
            let visible = avail_item_list
                .iter()
                .find(|&&(avail_id, _)| avail_id == id)
                .is_some_and(|&(_, v)| v);
            ResolvedItemId::Numeric { id, visible }
        }
        None => ResolvedItemId::Text {
            id_text: id_text.to_vec(),
            visible: false,
        },
    }
}

/// 数値 ID による重複排除(`StatusOptions.cpp:127-133`)。`item_list` に同じ `id` が
/// 既に存在するか判定する。
#[must_use]
pub fn contains_id(item_list: &[StatusItemInfo], id: i32) -> bool {
    item_list.iter().any(|item| item.id == id)
}

/// テキスト ID による大小無視の重複排除(`StatusOptions.cpp:143-149`)。`item_list` に
/// 大小無視で同じ `id_text` を持つ項目が既に存在するか判定する。
#[must_use]
pub fn contains_id_text(item_list: &[StatusItemInfo], id_text: &[u16]) -> bool {
    item_list
        .iter()
        .any(|item| is_equal_no_case_u16(&item.id_text, id_text))
}

/// `m_AvailItemList` のうち `item_list` に未出現の項目を、可視性 `false` として末尾に
/// 追加する(`StatusOptions.cpp:163-169` の `std::ranges::find` 相当)。
///
/// 順序は `avail_item_list` の順。
pub fn append_missing_avail_items(
    item_list: &mut Vec<StatusItemInfo>,
    avail_item_list: &[(i32, bool)],
) {
    for &(id, _) in avail_item_list {
        if !contains_id(item_list, id) {
            item_list.push(StatusItemInfo {
                id,
                id_text: Vec::new(),
                visible: false,
                width: -1,
            });
        }
    }
}

/// `PopupOpacity` の範囲クランプ(`StatusOptions.cpp:186-188`、
/// `std::clamp(Value, OPACITY_MIN, OPACITY_MAX)` 相当)。
#[must_use]
pub fn clamp_popup_opacity(value: i32) -> i32 {
    value.clamp(OPACITY_MIN, OPACITY_MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn status_item_constants() {
        assert_eq!(STATUS_ITEM_CHANNEL, 0);
        assert_eq!(STATUS_ITEM_VIDEOSIZE, 1);
        assert_eq!(STATUS_ITEM_VOLUME, 2);
        assert_eq!(STATUS_ITEM_AUDIOCHANNEL, 3);
        assert_eq!(STATUS_ITEM_RECORD, 4);
        assert_eq!(STATUS_ITEM_CAPTURE, 5);
        assert_eq!(STATUS_ITEM_ERROR, 6);
        assert_eq!(STATUS_ITEM_SIGNALLEVEL, 7);
        assert_eq!(STATUS_ITEM_CLOCK, 8);
        assert_eq!(STATUS_ITEM_PROGRAMINFO, 9);
        assert_eq!(STATUS_ITEM_BUFFERING, 10);
        assert_eq!(STATUS_ITEM_TUNER, 11);
        assert_eq!(STATUS_ITEM_MEDIABITRATE, 12);
        assert_eq!(STATUS_ITEM_FAVORITES, 13);
        assert_eq!(STATUS_ITEM_FIRST, 0);
        assert_eq!(STATUS_ITEM_LAST, 13);
    }

    #[test]
    fn default_item_list_order_and_values() {
        // StatusOptions.cpp:65-78 の順序と IS_HD == true の可視性をそのまま再現する。
        assert_eq!(
            DEFAULT_ITEM_LIST,
            [
                (STATUS_ITEM_TUNER, true),
                (STATUS_ITEM_CHANNEL, true),
                (STATUS_ITEM_FAVORITES, false),
                (STATUS_ITEM_VIDEOSIZE, true),
                (STATUS_ITEM_VOLUME, true),
                (STATUS_ITEM_AUDIOCHANNEL, true),
                (STATUS_ITEM_RECORD, true),
                (STATUS_ITEM_CAPTURE, true),
                (STATUS_ITEM_ERROR, true),
                (STATUS_ITEM_SIGNALLEVEL, true),
                (STATUS_ITEM_CLOCK, false),
                (STATUS_ITEM_PROGRAMINFO, false),
                (STATUS_ITEM_BUFFERING, false),
                (STATUS_ITEM_MEDIABITRATE, false),
            ]
        );
        assert_eq!(DEFAULT_ITEM_LIST.len(), 14);
    }

    #[test]
    fn parse_simple_number() {
        assert_eq!(try_parse_status_item_id(&utf16("5")), Some(5));
        assert_eq!(try_parse_status_item_id(&utf16("0")), Some(0));
    }

    #[test]
    fn parse_with_leading_and_trailing_whitespace() {
        // 先頭の空白は許容するが、std::_tcstol は末尾の空白は消費しないため NULL 終端に
        // 到達せず数値扱いされない。
        assert_eq!(try_parse_status_item_id(&utf16("  5")), Some(5));
        assert_eq!(try_parse_status_item_id(&utf16("5  ")), None);
    }

    #[test]
    fn parse_with_sign() {
        assert_eq!(try_parse_status_item_id(&utf16("+5")), Some(5));
        assert_eq!(try_parse_status_item_id(&utf16("-1")), Some(-1));
    }

    #[test]
    fn parse_out_of_range_numeric() {
        // パース自体は成功するが範囲外(呼び出し側 resolve_item_id で判定)。
        assert_eq!(try_parse_status_item_id(&utf16("-1")), Some(-1));
        assert_eq!(try_parse_status_item_id(&utf16("99")), Some(99));
    }

    #[test]
    fn parse_non_numeric_text() {
        assert_eq!(try_parse_status_item_id(&utf16("Custom1")), None);
        assert_eq!(try_parse_status_item_id(&utf16("")), None);
        assert_eq!(try_parse_status_item_id(&utf16("   ")), None);
        assert_eq!(try_parse_status_item_id(&utf16("-")), None);
        assert_eq!(try_parse_status_item_id(&utf16("+")), None);
    }

    #[test]
    fn parse_partial_numeric_text_is_none() {
        // 末尾に余分な文字があるため全体としては数値扱いされない。
        assert_eq!(try_parse_status_item_id(&utf16("5abc")), None);
        assert_eq!(try_parse_status_item_id(&utf16("12.5")), None);
    }

    #[test]
    fn is_equal_no_case_matches_ignoring_ascii_case() {
        assert!(is_equal_no_case_u16(&utf16("Custom1"), &utf16("custom1")));
        assert!(is_equal_no_case_u16(&utf16("CUSTOM1"), &utf16("custom1")));
        assert!(is_equal_no_case_u16(&utf16(""), &utf16("")));
    }

    #[test]
    fn is_equal_no_case_detects_mismatch() {
        assert!(!is_equal_no_case_u16(&utf16("Custom1"), &utf16("Custom2")));
        assert!(!is_equal_no_case_u16(&utf16("Custom1"), &utf16("Custom10")));
        assert!(!is_equal_no_case_u16(&utf16("Custom1"), &utf16("")));
    }

    #[test]
    fn clamp_popup_opacity_within_range() {
        assert_eq!(clamp_popup_opacity(50), 50);
    }

    #[test]
    fn clamp_popup_opacity_boundaries() {
        assert_eq!(clamp_popup_opacity(OPACITY_MIN), OPACITY_MIN);
        assert_eq!(clamp_popup_opacity(OPACITY_MAX), OPACITY_MAX);
        assert_eq!(clamp_popup_opacity(OPACITY_MIN - 1), OPACITY_MIN);
        assert_eq!(clamp_popup_opacity(OPACITY_MAX + 1), OPACITY_MAX);
        assert_eq!(clamp_popup_opacity(-100), OPACITY_MIN);
        assert_eq!(clamp_popup_opacity(1000), OPACITY_MAX);
    }

    #[test]
    fn resolve_item_id_numeric_within_range_inherits_visibility() {
        let avail = [(STATUS_ITEM_CLOCK, false), (STATUS_ITEM_CHANNEL, true)];
        let resolved = resolve_item_id(&utf16("0"), &avail);
        assert_eq!(
            resolved,
            ResolvedItemId::Numeric {
                id: STATUS_ITEM_CHANNEL,
                visible: true,
            }
        );
    }

    #[test]
    fn resolve_item_id_numeric_not_in_avail_defaults_to_invisible() {
        let avail = [(STATUS_ITEM_CHANNEL, true)];
        // avail に無い ID(範囲内)は visible=false で解決される。
        let resolved = resolve_item_id(&utf16("8"), &avail);
        assert_eq!(
            resolved,
            ResolvedItemId::Numeric {
                id: STATUS_ITEM_CLOCK,
                visible: false,
            }
        );
    }

    #[test]
    fn resolve_item_id_out_of_range() {
        let avail = [(STATUS_ITEM_CHANNEL, true)];
        assert_eq!(resolve_item_id(&utf16("99"), &avail), ResolvedItemId::OutOfRange);
        assert_eq!(resolve_item_id(&utf16("-1"), &avail), ResolvedItemId::OutOfRange);
        assert_eq!(resolve_item_id(&utf16("14"), &avail), ResolvedItemId::OutOfRange);
    }

    #[test]
    fn resolve_item_id_text() {
        let avail = [(STATUS_ITEM_CHANNEL, true)];
        let resolved = resolve_item_id(&utf16("Custom1"), &avail);
        assert_eq!(
            resolved,
            ResolvedItemId::Text {
                id_text: utf16("Custom1"),
                visible: false,
            }
        );
    }

    #[test]
    fn contains_id_detects_duplicate() {
        let list = vec![StatusItemInfo::with_id(STATUS_ITEM_CHANNEL, true)];
        assert!(contains_id(&list, STATUS_ITEM_CHANNEL));
        assert!(!contains_id(&list, STATUS_ITEM_CLOCK));
    }

    #[test]
    fn contains_id_text_detects_duplicate_ignoring_case() {
        let list = vec![StatusItemInfo::with_id_text(utf16("Custom1"), false)];
        assert!(contains_id_text(&list, &utf16("custom1")));
        assert!(!contains_id_text(&list, &utf16("Custom2")));
    }

    #[test]
    fn append_missing_avail_items_adds_only_absent_ids() {
        let mut list = vec![StatusItemInfo::with_id(STATUS_ITEM_CHANNEL, true)];
        let avail = [
            (STATUS_ITEM_CHANNEL, true),
            (STATUS_ITEM_CLOCK, false),
            (STATUS_ITEM_VOLUME, true),
        ];

        append_missing_avail_items(&mut list, &avail);

        assert_eq!(list.len(), 3);
        assert_eq!(list[0].id, STATUS_ITEM_CHANNEL);
        assert!(list[0].visible);
        assert_eq!(list[1].id, STATUS_ITEM_CLOCK);
        assert!(!list[1].visible);
        assert_eq!(list[1].width, -1);
        assert_eq!(list[2].id, STATUS_ITEM_VOLUME);
        // avail 側は可視性 true だが、未出現分の追加は常に false(StatusOptions.cpp:166)。
        assert!(!list[2].visible);
    }

    #[test]
    fn append_missing_avail_items_preserves_avail_order() {
        let mut list: Vec<StatusItemInfo> = Vec::new();
        let avail = [
            (STATUS_ITEM_TUNER, true),
            (STATUS_ITEM_CHANNEL, true),
            (STATUS_ITEM_FAVORITES, false),
        ];

        append_missing_avail_items(&mut list, &avail);

        let ids: Vec<i32> = list.iter().map(|i| i.id).collect();
        assert_eq!(ids, vec![STATUS_ITEM_TUNER, STATUS_ITEM_CHANNEL, STATUS_ITEM_FAVORITES]);
        assert!(list.iter().all(|i| !i.visible));
    }

    #[test]
    fn status_item_info_constructors() {
        let numeric = StatusItemInfo::with_id(STATUS_ITEM_CHANNEL, true);
        assert_eq!(numeric.id, STATUS_ITEM_CHANNEL);
        assert!(numeric.id_text.is_empty());
        assert!(numeric.visible);
        assert_eq!(numeric.width, -1);

        let text = StatusItemInfo::with_id_text(utf16("Custom1"), false);
        assert_eq!(text.id, -1);
        assert_eq!(text.id_text, utf16("Custom1"));
        assert!(!text.visible);
        assert_eq!(text.width, -1);
    }

    #[test]
    fn full_read_settings_like_flow_dedup_and_append() {
        // ReadSettings の一連の流れを模した統合テスト:
        // "0"(=CHANNEL, avail 由来で visible=true) → 採用
        // "0" 重複 → 無視
        // "Custom1" → テキスト ID として採用
        // "custom1" 大小無視で重複 → 無視
        // "99" 範囲外 → 無視
        // 最後に avail の未出現分(TUNER, FAVORITES, ...)を可視false で追加。
        let avail: Vec<(i32, bool)> = DEFAULT_ITEM_LIST.to_vec();
        let entries = ["0", "0", "Custom1", "custom1", "99"];
        let mut item_list: Vec<StatusItemInfo> = Vec::new();

        for entry in entries {
            match resolve_item_id(&utf16(entry), &avail) {
                ResolvedItemId::Numeric { id, visible } => {
                    if contains_id(&item_list, id) {
                        continue;
                    }
                    item_list.push(StatusItemInfo::with_id(id, visible));
                }
                ResolvedItemId::Text { id_text, visible } => {
                    if contains_id_text(&item_list, &id_text) {
                        continue;
                    }
                    item_list.push(StatusItemInfo::with_id_text(id_text, visible));
                }
                ResolvedItemId::OutOfRange => {}
            }
        }

        assert_eq!(item_list.len(), 2);
        assert_eq!(item_list[0].id, STATUS_ITEM_CHANNEL);
        assert!(item_list[0].visible);
        assert_eq!(item_list[1].id, -1);
        assert_eq!(item_list[1].id_text, utf16("Custom1"));
        assert!(!item_list[1].visible);

        append_missing_avail_items(&mut item_list, &avail);

        // 2(既存) + 14(avail) - 1(CHANNEL は既出) = 15。
        assert_eq!(item_list.len(), 2 + DEFAULT_ITEM_LIST.len() - 1);
        assert!(contains_id(&item_list, STATUS_ITEM_TUNER));
        assert!(contains_id(&item_list, STATUS_ITEM_FAVORITES));
    }
}
