//! TVTest のサイドバー設定(`src/SideBarOptions.cpp` / `SideBarOptions.h`)の純粋ロジックを
//! 移植したクレート。
//!
//! 移植対象:
//! - 不透明度の範囲定数(`OPACITY_MIN`/`OPACITY_MAX`、SideBarOptions.h:39-40)。
//! - 表示位置 `PlaceType`(SideBarOptions.h:42-48)。
//! - アイテムリストの区切りを表すコマンド ID(`ITEM_SEPARATOR`、SideBarOptions.h:72)。
//! - サイドバーに配置可能な全アイテム `ItemList`(SideBarOptions.cpp:42-101)。
//! - 既定表示アイテム一覧 `DefaultItemList`(SideBarOptions.cpp:103-123、`TVTEST_FOR_1SEG`
//!   非定義時の通常ビルド版のみ)。
//! - `ReadSettings` のうち純粋なロジック部分(SideBarOptions.cpp:147-197):
//!   - `PopupOpacity` 読み込み時の `std::clamp(Value, OPACITY_MIN, OPACITY_MAX)`。
//!   - `Place` 読み込み時の `CheckEnumRange` 相当の範囲チェック。
//!   - `ItemCount` 読み込み時の上限クランプ(200 以上なら 200 に切り詰め)。
//! - `IsAvailableItem`(SideBarOptions.cpp:896-904)の線形検索。
//!
//! 対象外(Win32 / CSettings / UI 依存):
//! - `DlgProc`(ダイアログプロシージャ)。
//! - `ReadSettings`/`WriteSettings` の `CSettings` I/O 本体。
//! - `CreateImage`/`CreateIconImageList` 等のアイコン画像生成。
//! - `UpdateListViewIcons`/`SetItemList` 等の UI 更新。
//! - `RegisterCommand`。
//! - `ApplySideBarOptions`/`ApplyItemList`。
//! - `OnDarkModeChanged`。

#![forbid(unsafe_code)]

/// ポップアップ不透明度の最小値(SideBarOptions.h:39)。
pub const OPACITY_MIN: i32 = 20;
/// ポップアップ不透明度の最大値(SideBarOptions.h:40)。
pub const OPACITY_MAX: i32 = 100;

/// アイテムリスト中の区切りを表すコマンド ID(SideBarOptions.h:72)。
pub const ITEM_SEPARATOR: i32 = 0;

/// `ReadSettings` の `ItemCount` に対する上限クランプ値
/// (SideBarOptions.cpp:161-163 のコメント「はまるのを防ぐために、200を上限にしておく」)。
pub const ITEM_COUNT_LIMIT: i32 = 200;

// サイドバーアイテムのコマンド ID(resource.h)。

// ズームコマンド(rust/zoom クレートの CM_ZOOM_FIRST/CM_CUSTOMZOOM_FIRST と同値)。
const CM_ZOOM_FIRST: i32 = 100;
const CM_ZOOM_20: i32 = CM_ZOOM_FIRST;
const CM_ZOOM_25: i32 = CM_ZOOM_FIRST + 1;
const CM_ZOOM_33: i32 = CM_ZOOM_FIRST + 2;
const CM_ZOOM_50: i32 = CM_ZOOM_FIRST + 3;
const CM_ZOOM_66: i32 = CM_ZOOM_FIRST + 4;
const CM_ZOOM_75: i32 = CM_ZOOM_FIRST + 5;
const CM_ZOOM_100: i32 = CM_ZOOM_FIRST + 6;
const CM_ZOOM_150: i32 = CM_ZOOM_FIRST + 7;
const CM_ZOOM_200: i32 = CM_ZOOM_FIRST + 8;
const CM_ZOOM_250: i32 = CM_ZOOM_FIRST + 9;
const CM_ZOOM_300: i32 = CM_ZOOM_FIRST + 10;
const CM_CUSTOMZOOM_FIRST: i32 = 19000;

// アスペクト比コマンド。
const CM_ASPECTRATIO_FIRST: i32 = 121;
const CM_ASPECTRATIO_DEFAULT: i32 = CM_ASPECTRATIO_FIRST;
const CM_ASPECTRATIO_16X9: i32 = CM_ASPECTRATIO_FIRST + 1;
const CM_ASPECTRATIO_LETTERBOX: i32 = CM_ASPECTRATIO_FIRST + 2;
const CM_ASPECTRATIO_WINDOWBOX: i32 = CM_ASPECTRATIO_FIRST + 3;
const CM_ASPECTRATIO_PILLARBOX: i32 = CM_ASPECTRATIO_FIRST + 4;
const CM_ASPECTRATIO_4X3: i32 = CM_ASPECTRATIO_FIRST + 5;

// 表示・操作系コマンド。
const CM_FULLSCREEN: i32 = 137;
const CM_ALWAYSONTOP: i32 = 138;
const CM_DISABLEVIEWER: i32 = 161;
const CM_CAPTURE: i32 = 164;
const CM_SAVEIMAGE: i32 = 163;
const CM_COPYIMAGE: i32 = 162;
const CM_CAPTUREPREVIEW: i32 = 165;
const CM_RESET: i32 = 200;
const CM_RESETVIEWER: i32 = 201;
const CM_PANEL: i32 = 203;
const CM_PROGRAMGUIDE: i32 = 204;
const CM_STATUSBAR: i32 = 206;
const CM_VIDEODECODERPROPERTY: i32 = 215;
const CM_OPTIONS: i32 = 214;
const CM_STREAMINFO: i32 = 220;
const CM_SPDIF_TOGGLE: i32 = 149;
const CM_HOMEDISPLAY: i32 = 224;
const CM_CHANNELDISPLAY: i32 = 225;
const CM_1SEGMODE: i32 = 236;

// チャンネル番号コマンド。
const CM_CHANNELNO_FIRST: i32 = 12000;
const CM_CHANNELNO_1: i32 = CM_CHANNELNO_FIRST;
const CM_CHANNELNO_2: i32 = CM_CHANNELNO_FIRST + 1;
const CM_CHANNELNO_3: i32 = CM_CHANNELNO_FIRST + 2;
const CM_CHANNELNO_4: i32 = CM_CHANNELNO_FIRST + 3;
const CM_CHANNELNO_5: i32 = CM_CHANNELNO_FIRST + 4;
const CM_CHANNELNO_6: i32 = CM_CHANNELNO_FIRST + 5;
const CM_CHANNELNO_7: i32 = CM_CHANNELNO_FIRST + 6;
const CM_CHANNELNO_8: i32 = CM_CHANNELNO_FIRST + 7;
const CM_CHANNELNO_9: i32 = CM_CHANNELNO_FIRST + 8;
const CM_CHANNELNO_10: i32 = CM_CHANNELNO_FIRST + 9;
const CM_CHANNELNO_11: i32 = CM_CHANNELNO_FIRST + 10;
const CM_CHANNELNO_12: i32 = CM_CHANNELNO_FIRST + 11;

/// アイコンインデックスの先頭値(無名 namespace 内の `ZOOM_ICON_FIRST`、SideBarOptions.cpp:37)。
const ZOOM_ICON_FIRST: i32 = 37;

/// サイドバーの表示位置(SideBarOptions.h:42-48 の `PlaceType`)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaceType {
    /// 左側。
    Left = 0,
    /// 右側。
    Right = 1,
    /// 上側。
    Top = 2,
    /// 下側。
    Bottom = 3,
}

/// `PlaceType` の末尾値(`TVTEST_ENUM_CLASS_TRAILER` 相当、`CheckEnumRange` の範囲上限に使う)。
pub const PLACE_TYPE_TRAILER: i32 = 4;

impl PlaceType {
    /// 設定値(整数)からの復元。`CheckEnumRange` 相当で範囲外は `None`。
    pub fn from_int(value: i32) -> Option<PlaceType> {
        match value {
            0 => Some(PlaceType::Left),
            1 => Some(PlaceType::Right),
            2 => Some(PlaceType::Top),
            3 => Some(PlaceType::Bottom),
            _ => None,
        }
    }

    /// 設定値(整数)への変換。
    pub fn to_int(self) -> i32 {
        self as i32
    }
}

/// サイドバーに配置可能な 1 アイテム(`CSideBar::SideBarItem` 相当)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SideBarItem {
    /// コマンド ID。
    pub command: i32,
    /// アイコンインデックス。
    pub icon: i32,
}

const fn item(command: i32, icon: i32) -> SideBarItem {
    SideBarItem { command, icon }
}

/// サイドバーに配置可能な全アイテム(SideBarOptions.cpp:42-101 の `ItemList`)。
///
/// 原実装の配列要素数は 58(SideBarOptions.cpp:43-100 の各行が 1 要素)。
pub static ITEM_LIST: [SideBarItem; 58] = [
    item(CM_ZOOM_20, ZOOM_ICON_FIRST),
    item(CM_ZOOM_25, ZOOM_ICON_FIRST + 1),
    item(CM_ZOOM_33, ZOOM_ICON_FIRST + 2),
    item(CM_ZOOM_50, ZOOM_ICON_FIRST + 3),
    item(CM_ZOOM_66, ZOOM_ICON_FIRST + 4),
    item(CM_ZOOM_75, ZOOM_ICON_FIRST + 5),
    item(CM_ZOOM_100, ZOOM_ICON_FIRST + 6),
    item(CM_ZOOM_150, ZOOM_ICON_FIRST + 7),
    item(CM_ZOOM_200, ZOOM_ICON_FIRST + 8),
    item(CM_ZOOM_250, ZOOM_ICON_FIRST + 9),
    item(CM_ZOOM_300, ZOOM_ICON_FIRST + 10),
    item(CM_CUSTOMZOOM_FIRST, ZOOM_ICON_FIRST + 11),
    item(CM_CUSTOMZOOM_FIRST + 1, ZOOM_ICON_FIRST + 12),
    item(CM_CUSTOMZOOM_FIRST + 2, ZOOM_ICON_FIRST + 13),
    item(CM_CUSTOMZOOM_FIRST + 3, ZOOM_ICON_FIRST + 14),
    item(CM_CUSTOMZOOM_FIRST + 4, ZOOM_ICON_FIRST + 15),
    item(CM_CUSTOMZOOM_FIRST + 5, ZOOM_ICON_FIRST + 16),
    item(CM_CUSTOMZOOM_FIRST + 6, ZOOM_ICON_FIRST + 17),
    item(CM_CUSTOMZOOM_FIRST + 7, ZOOM_ICON_FIRST + 18),
    item(CM_CUSTOMZOOM_FIRST + 8, ZOOM_ICON_FIRST + 19),
    item(CM_CUSTOMZOOM_FIRST + 9, ZOOM_ICON_FIRST + 20),
    item(CM_ASPECTRATIO_DEFAULT, 0),
    item(CM_ASPECTRATIO_16X9, 1),
    item(CM_ASPECTRATIO_LETTERBOX, 2),
    item(CM_ASPECTRATIO_WINDOWBOX, 3),
    item(CM_ASPECTRATIO_PILLARBOX, 4),
    item(CM_ASPECTRATIO_4X3, 5),
    item(CM_FULLSCREEN, 6),
    item(CM_ALWAYSONTOP, 7),
    item(CM_DISABLEVIEWER, 8),
    item(CM_CAPTURE, 9),
    item(CM_SAVEIMAGE, 10),
    item(CM_COPYIMAGE, 11),
    item(CM_CAPTUREPREVIEW, 12),
    item(CM_RESET, 13),
    item(CM_RESETVIEWER, 14),
    item(CM_PANEL, 15),
    item(CM_PROGRAMGUIDE, 16),
    item(CM_STATUSBAR, 17),
    item(CM_VIDEODECODERPROPERTY, 18),
    item(CM_OPTIONS, 19),
    item(CM_STREAMINFO, 20),
    item(CM_SPDIF_TOGGLE, 21),
    item(CM_HOMEDISPLAY, 22),
    item(CM_CHANNELDISPLAY, 23),
    item(CM_1SEGMODE, 24),
    item(CM_CHANNELNO_1, 25),
    item(CM_CHANNELNO_2, 26),
    item(CM_CHANNELNO_3, 27),
    item(CM_CHANNELNO_4, 28),
    item(CM_CHANNELNO_5, 29),
    item(CM_CHANNELNO_6, 30),
    item(CM_CHANNELNO_7, 31),
    item(CM_CHANNELNO_8, 32),
    item(CM_CHANNELNO_9, 33),
    item(CM_CHANNELNO_10, 34),
    item(CM_CHANNELNO_11, 35),
    item(CM_CHANNELNO_12, 36),
];

/// 既定表示アイテム一覧(SideBarOptions.cpp:103-123 の `DefaultItemList`、
/// `TVTEST_FOR_1SEG` 非定義時の通常ビルド版)。`0` は `ITEM_SEPARATOR`(区切り)。
pub static DEFAULT_ITEM_LIST: [i32; 13] = [
    CM_ZOOM_25,
    CM_ZOOM_33,
    CM_ZOOM_50,
    CM_ZOOM_100,
    ITEM_SEPARATOR,
    CM_FULLSCREEN,
    CM_ALWAYSONTOP,
    CM_DISABLEVIEWER,
    ITEM_SEPARATOR,
    CM_PANEL,
    CM_PROGRAMGUIDE,
    CM_CHANNELDISPLAY,
    CM_OPTIONS,
];

/// `ReadSettings` の `PopupOpacity` に対するクランプ(SideBarOptions.cpp:154-155 の
/// `std::clamp(Value, OPACITY_MIN, OPACITY_MAX)`)。
#[must_use]
pub fn clamp_popup_opacity(value: i32) -> i32 {
    value.clamp(OPACITY_MIN, OPACITY_MAX)
}

/// `ReadSettings` の `ItemCount` に対する上限クランプ(SideBarOptions.cpp:160-163)。
///
/// `NumItems > 0` であることは呼び出し側で確認済みの前提で、上限のみをクランプする
/// (`value` が `ITEM_COUNT_LIMIT` 以上なら `ITEM_COUNT_LIMIT` を返し、それ以外はそのまま返す)。
#[must_use]
pub fn clamp_item_count(value: i32) -> i32 {
    if value >= ITEM_COUNT_LIMIT {
        ITEM_COUNT_LIMIT
    } else {
        value
    }
}

/// 指定コマンド ID が配置可能アイテムに含まれるか(`IsAvailableItem`、SideBarOptions.cpp:896-904)。
#[must_use]
pub fn is_available_item(avail_item_list: &[SideBarItem], id: i32) -> bool {
    avail_item_list.iter().any(|e| e.command == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_list_len_is_58() {
        assert_eq!(ITEM_LIST.len(), 58);
    }

    #[test]
    fn item_list_first_entry() {
        assert_eq!(ITEM_LIST[0], item(CM_ZOOM_20, ZOOM_ICON_FIRST));
        assert_eq!(ITEM_LIST[0].command, 100);
        assert_eq!(ITEM_LIST[0].icon, 37);
    }

    #[test]
    fn item_list_last_entry() {
        assert_eq!(ITEM_LIST[57], item(CM_CHANNELNO_12, 36));
        assert_eq!(ITEM_LIST[57].command, 12011);
        assert_eq!(ITEM_LIST[57].icon, 36);
    }

    #[test]
    fn item_list_aspect_ratio_block() {
        assert_eq!(ITEM_LIST[21], item(CM_ASPECTRATIO_DEFAULT, 0));
        assert_eq!(ITEM_LIST[26], item(CM_ASPECTRATIO_4X3, 5));
    }

    #[test]
    fn item_list_custom_zoom_block() {
        assert_eq!(ITEM_LIST[11], item(CM_CUSTOMZOOM_FIRST, ZOOM_ICON_FIRST + 11));
        assert_eq!(ITEM_LIST[20], item(CM_CUSTOMZOOM_FIRST + 9, ZOOM_ICON_FIRST + 20));
    }

    #[test]
    fn default_item_list_contents() {
        assert_eq!(
            DEFAULT_ITEM_LIST,
            [
                CM_ZOOM_25,
                CM_ZOOM_33,
                CM_ZOOM_50,
                CM_ZOOM_100,
                0,
                CM_FULLSCREEN,
                CM_ALWAYSONTOP,
                CM_DISABLEVIEWER,
                0,
                CM_PANEL,
                CM_PROGRAMGUIDE,
                CM_CHANNELDISPLAY,
                CM_OPTIONS,
            ]
        );
    }

    #[test]
    fn default_item_list_len_is_13() {
        assert_eq!(DEFAULT_ITEM_LIST.len(), 13);
    }

    #[test]
    fn place_type_from_int_within_range() {
        assert_eq!(PlaceType::from_int(0), Some(PlaceType::Left));
        assert_eq!(PlaceType::from_int(1), Some(PlaceType::Right));
        assert_eq!(PlaceType::from_int(2), Some(PlaceType::Top));
        assert_eq!(PlaceType::from_int(3), Some(PlaceType::Bottom));
    }

    #[test]
    fn place_type_from_int_out_of_range() {
        assert_eq!(PlaceType::from_int(-1), None);
        assert_eq!(PlaceType::from_int(4), None);
        assert_eq!(PlaceType::from_int(PLACE_TYPE_TRAILER), None);
    }

    #[test]
    fn place_type_round_trip() {
        for value in 0..PLACE_TYPE_TRAILER {
            let place = PlaceType::from_int(value).unwrap();
            assert_eq!(place.to_int(), value);
        }
    }

    #[test]
    fn clamp_popup_opacity_boundaries() {
        assert_eq!(clamp_popup_opacity(0), OPACITY_MIN);
        assert_eq!(clamp_popup_opacity(OPACITY_MIN - 1), OPACITY_MIN);
        assert_eq!(clamp_popup_opacity(OPACITY_MIN), OPACITY_MIN);
        assert_eq!(clamp_popup_opacity(50), 50);
        assert_eq!(clamp_popup_opacity(OPACITY_MAX), OPACITY_MAX);
        assert_eq!(clamp_popup_opacity(OPACITY_MAX + 1), OPACITY_MAX);
        assert_eq!(clamp_popup_opacity(1000), OPACITY_MAX);
    }

    #[test]
    fn clamp_item_count_boundaries() {
        assert_eq!(clamp_item_count(1), 1);
        assert_eq!(clamp_item_count(199), 199);
        assert_eq!(clamp_item_count(200), 200);
        assert_eq!(clamp_item_count(201), 200);
        assert_eq!(clamp_item_count(1000), 200);
    }

    #[test]
    fn is_available_item_match_and_mismatch() {
        assert!(is_available_item(&ITEM_LIST, CM_ZOOM_20));
        assert!(is_available_item(&ITEM_LIST, CM_CHANNELNO_12));
        assert!(!is_available_item(&ITEM_LIST, 999_999));
        assert!(!is_available_item(&[], CM_ZOOM_20));
    }

    #[test]
    fn item_separator_value() {
        assert_eq!(ITEM_SEPARATOR, 0);
    }

    #[test]
    fn opacity_and_item_count_constants() {
        assert_eq!(OPACITY_MIN, 20);
        assert_eq!(OPACITY_MAX, 100);
        assert_eq!(ITEM_COUNT_LIMIT, 200);
        assert_eq!(PLACE_TYPE_TRAILER, 4);
    }
}
