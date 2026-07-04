//! TVTest `CProgramGuideFrameSettings`(`src/ProgramGuide.h:891-961` 定義、
//! `src/ProgramGuide.cpp:7358-7584` 実装)の純粋ロジックを移植したクレート。
//!
//! 移植対象:
//! - `TOOLBAR_NUM`(5、`CProgramGuideFrameBase::TOOLBAR_NUM` 由来)/
//!   `DATEBAR_MAXBUTTONCOUNT`/`DATEBAR_DEFAULTBUTTONCOUNT`(`ProgramGuide.h:895-897`)。
//! - `m_ToolbarInfoList`(`ProgramGuide.cpp:7358-7366`、5種のツールバー ID テキスト+表示名)。
//! - `TimeBarSettings`(`ProgramGuide.h:899-916`、`TimeType`/範囲定数/既定値)。
//! - `ParseIDText`(`ProgramGuide.cpp:7573-7584`、大小無視の ID テキスト解決)。
//! - `GetToolbarIDText`/`GetToolbarName`(`ProgramGuide.cpp:7486-7499`、範囲チェック付き参照)。
//! - `SetToolbarVisible`/`GetToolbarVisible`(`ProgramGuide.cpp:7502-7516`)。
//! - `SetToolbarOrderList`/`GetToolbarOrderList`(`ProgramGuide.cpp:7519-7552`、
//!   順序配列の検証(範囲/重複)と `Order` フィールドへの変換・逆変換)。
//! - `SetDateBarButtonCount`(`ProgramGuide.cpp:7555-7563`、範囲チェック)。
//! - `SetTimeBarSettings`(`ProgramGuide.cpp:7566-7570`、単純代入)。
//! - `ReadSettings` のツールバー順序パース(`ProgramGuide.cpp:7380-7431`、
//!   `Toolbar{i}_Name`/`Toolbar{i}_Status` の読み取り結果を受け取り、ver.0.9.0 互換
//!   (`Name` 未取得時はインデックスそのまま)・テキスト解決失敗時のスキップ・重複排除・
//!   不足分の昇順補完を行う)。
//!
//! 対象外(Win32 / CSettings / UI 依存):
//! - `DlgProc`(ダイアログプロシージャ、`CCheckListView` 操作)。
//! - `ReadSettings`/`WriteSettings` の `CSettings` I/O 本体(文字列/整数の実読み書き)。
//! - `CProgramGuideFrame`/`CProgramGuideDisplay` への実適用。

#![forbid(unsafe_code)]

/// `CProgramGuideFrameBase::TOOLBAR_NUM` 由来の `TOOLBAR_NUM`(`ProgramGuide.h:895`)。
pub const TOOLBAR_NUM: usize = 5;
/// `DATEBAR_MAXBUTTONCOUNT`(`ProgramGuide.h:896`)。
pub const DATEBAR_MAXBUTTONCOUNT: i32 = 8;
/// `DATEBAR_DEFAULTBUTTONCOUNT`(`ProgramGuide.h:897`)。
pub const DATEBAR_DEFAULTBUTTONCOUNT: i32 = 8;

/// `CProgramGuideFrameSettings::ToolbarInfo`(`ProgramGuide.h:937-941`)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolbarInfo {
    pub id_text: &'static str,
    pub name: &'static str,
}

/// `m_ToolbarInfoList`(`ProgramGuide.cpp:7358-7366`)。配列添字がツールバー ID。
pub const TOOLBAR_INFO_LIST: [ToolbarInfo; TOOLBAR_NUM] = [
    ToolbarInfo {
        id_text: "TunerMenu",
        name: "チューナーメニュー",
    },
    ToolbarInfo {
        id_text: "DateMenu",
        name: "日付メニュー",
    },
    ToolbarInfo {
        id_text: "Favorites",
        name: "番組表選択ボタン",
    },
    ToolbarInfo {
        id_text: "DateBar",
        name: "日付バー",
    },
    ToolbarInfo {
        id_text: "TimeBar",
        name: "時刻バー",
    },
];

/// `CProgramGuideFrameSettings::TimeBarSettings::TimeType`(`ProgramGuide.h:901-905`)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimeType {
    #[default]
    Interval,
    Custom,
}

impl TimeType {
    /// `CheckEnumRange`(`ProgramGuide.cpp:7439`)相当の範囲チェック付き変換。
    #[must_use]
    pub fn from_i32(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::Interval),
            1 => Some(Self::Custom),
            _ => None,
        }
    }

    /// `static_cast<int>(m_TimeBarSettings.Time)`(`ProgramGuide.cpp:7477`)。
    #[must_use]
    pub fn to_i32(self) -> i32 {
        self as i32
    }
}

/// `CProgramGuideFrameSettings::TimeBarSettings`(`ProgramGuide.h:899-916`)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeBarSettings {
    pub time: TimeType,
    pub interval: i32,
    pub custom_time: String,
    pub max_button_count: i32,
}

impl TimeBarSettings {
    pub const INTERVAL_MIN: i32 = 2;
    pub const INTERVAL_MAX: i32 = 12;
    pub const BUTTONCOUNT_MIN: i32 = 1;
    pub const BUTTONCOUNT_MAX: i32 = 20;
}

impl Default for TimeBarSettings {
    fn default() -> Self {
        Self {
            time: TimeType::Interval,
            interval: 4,
            custom_time: "0,3,6,9,12,15,18,21".to_string(),
            max_button_count: 10,
        }
    }
}

/// `CProgramGuideFrameSettings` のツールバー表示/順序状態(`m_ToolbarSettingsList`,
/// `ProgramGuide.h:943-947,953`)。
#[derive(Debug, Clone)]
pub struct ProgramGuideFrameSettings {
    visible: [bool; TOOLBAR_NUM],
    order: [usize; TOOLBAR_NUM],
    date_bar_button_count: i32,
    time_bar_settings: TimeBarSettings,
}

impl Default for ProgramGuideFrameSettings {
    /// コンストラクタ(`ProgramGuide.cpp:7369-7377`)= 全項目表示・恒等順序。
    fn default() -> Self {
        let mut order = [0usize; TOOLBAR_NUM];
        for (i, slot) in order.iter_mut().enumerate() {
            *slot = i;
        }
        Self {
            visible: [true; TOOLBAR_NUM],
            order,
            date_bar_button_count: DATEBAR_DEFAULTBUTTONCOUNT,
            time_bar_settings: TimeBarSettings::default(),
        }
    }
}

/// `ParseIDText`(`ProgramGuide.cpp:7573-7584`)。空文字列や未知の ID テキストは
/// `None`(原実装は `-1`)。`TOOLBAR_INFO_LIST` を大小無視で線形検索する。
#[must_use]
pub fn parse_id_text(id_text: &str) -> Option<usize> {
    if id_text.is_empty() {
        return None;
    }
    TOOLBAR_INFO_LIST
        .iter()
        .position(|info| info.id_text.eq_ignore_ascii_case(id_text))
}

impl ProgramGuideFrameSettings {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `GetToolbarIDText`(`ProgramGuide.cpp:7486-7491`)。
    #[must_use]
    pub fn toolbar_id_text(toolbar: i32) -> Option<&'static str> {
        if toolbar < 0 || toolbar as usize >= TOOLBAR_NUM {
            return None;
        }
        Some(TOOLBAR_INFO_LIST[toolbar as usize].id_text)
    }

    /// `GetToolbarName`(`ProgramGuide.cpp:7494-7499`)。
    #[must_use]
    pub fn toolbar_name(toolbar: i32) -> Option<&'static str> {
        if toolbar < 0 || toolbar as usize >= TOOLBAR_NUM {
            return None;
        }
        Some(TOOLBAR_INFO_LIST[toolbar as usize].name)
    }

    /// `SetToolbarVisible`(`ProgramGuide.cpp:7502-7508`)。
    pub fn set_toolbar_visible(&mut self, toolbar: i32, visible: bool) -> bool {
        if toolbar < 0 || toolbar as usize >= TOOLBAR_NUM {
            return false;
        }
        self.visible[toolbar as usize] = visible;
        true
    }

    /// `GetToolbarVisible`(`ProgramGuide.cpp:7511-7516`)。範囲外は `false`。
    #[must_use]
    pub fn toolbar_visible(&self, toolbar: i32) -> bool {
        if toolbar < 0 || toolbar as usize >= TOOLBAR_NUM {
            return false;
        }
        self.visible[toolbar as usize]
    }

    /// `SetToolbarOrderList`(`ProgramGuide.cpp:7519-7540`)。`order` は「表示順序に並んだ
    /// ツールバー ID の列」。範囲外の ID や重複があれば何も変更せず `false` を返す。
    pub fn set_toolbar_order_list(&mut self, order: &[usize; TOOLBAR_NUM]) -> bool {
        for (i, &id) in order.iter().enumerate() {
            if id >= TOOLBAR_NUM {
                return false;
            }
            if order[i + 1..].contains(&id) {
                return false;
            }
        }

        for (i, &id) in order.iter().enumerate() {
            self.order[id] = i;
        }

        true
    }

    /// `GetToolbarOrderList`(`ProgramGuide.cpp:7543-7552`)。`m_ToolbarSettingsList[i].Order`
    /// (=表示位置)から「表示順序に並んだツールバー ID の列」へ逆変換する。
    #[must_use]
    pub fn toolbar_order_list(&self) -> [usize; TOOLBAR_NUM] {
        let mut result = [0usize; TOOLBAR_NUM];
        for (id, &pos) in self.order.iter().enumerate() {
            result[pos] = id;
        }
        result
    }

    /// `SetDateBarButtonCount`(`ProgramGuide.cpp:7555-7563`)。
    pub fn set_date_bar_button_count(&mut self, count: i32) -> bool {
        if !(1..=DATEBAR_MAXBUTTONCOUNT).contains(&count) {
            return false;
        }
        self.date_bar_button_count = count;
        true
    }

    #[must_use]
    pub fn date_bar_button_count(&self) -> i32 {
        self.date_bar_button_count
    }

    /// `SetTimeBarSettings`(`ProgramGuide.cpp:7566-7570`、常に成功)。
    pub fn set_time_bar_settings(&mut self, settings: TimeBarSettings) {
        self.time_bar_settings = settings;
    }

    #[must_use]
    pub fn time_bar_settings(&self) -> &TimeBarSettings {
        &self.time_bar_settings
    }
}

/// `ReadSettings` で読んだ `Toolbar{i}_Name`(取得できなければ `None`)の1エントリ。
#[derive(Debug, Clone, Copy)]
pub struct ToolbarNameEntry<'a> {
    pub name: Option<&'a str>,
}

/// `ReadSettings` のツールバー順序パース(`ProgramGuide.cpp:7380-7431`)。
///
/// `entries` は設定ファイル上の `Toolbar0_Name`〜`Toolbar{TOOLBAR_NUM-1}_Name` の
/// 読み取り結果を先頭から並べたもの。各エントリについて:
/// - `name` が `Some` なら `parse_id_text` で ID を解決し、失敗(未知のテキスト)なら
///   そのエントリを読み飛ばす(`ProgramGuide.cpp:7391-7393`)。
/// - `name` が `None`(ver.0.9.0 より前の設定ファイルとの互換)なら、そのエントリの
///   インデックスをそのまま ID として使う(`ProgramGuide.cpp:7396`)。
/// - 既に順序リストに同じ ID があれば読み飛ばす(`ProgramGuide.cpp:7399-7405`)。
///
/// 戻り値は `SetToolbarOrderList` にそのまま渡せる順序配列。読み取れたエントリが
/// `TOOLBAR_NUM` に満たない場合は、未出現の ID を昇順で末尾に補う
/// (`ProgramGuide.cpp:7417-7429`)。
#[must_use]
pub fn parse_toolbar_order(entries: &[ToolbarNameEntry; TOOLBAR_NUM]) -> [usize; TOOLBAR_NUM] {
    let mut order_list: Vec<usize> = Vec::with_capacity(TOOLBAR_NUM);

    for (i, entry) in entries.iter().enumerate() {
        let id = match entry.name {
            Some(name) => match parse_id_text(name) {
                Some(id) => id,
                None => continue,
            },
            None => i,
        };

        if order_list.contains(&id) {
            continue;
        }

        order_list.push(id);
    }

    if order_list.len() < TOOLBAR_NUM {
        for i in 0..TOOLBAR_NUM {
            if !order_list.contains(&i) {
                order_list.push(i);
            }
        }
    }

    order_list
        .try_into()
        .expect("order_list always has TOOLBAR_NUM entries")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_match_header() {
        assert_eq!(TOOLBAR_NUM, 5);
        assert_eq!(DATEBAR_MAXBUTTONCOUNT, 8);
        assert_eq!(DATEBAR_DEFAULTBUTTONCOUNT, 8);
    }

    #[test]
    fn toolbar_info_list_contents() {
        assert_eq!(TOOLBAR_INFO_LIST[0].id_text, "TunerMenu");
        assert_eq!(TOOLBAR_INFO_LIST[4].id_text, "TimeBar");
        assert_eq!(TOOLBAR_INFO_LIST[2].name, "番組表選択ボタン");
    }

    #[test]
    fn parse_id_text_case_insensitive() {
        assert_eq!(parse_id_text("datebar"), Some(3));
        assert_eq!(parse_id_text("TIMEBAR"), Some(4));
    }

    #[test]
    fn parse_id_text_empty_or_unknown_is_none() {
        assert_eq!(parse_id_text(""), None);
        assert_eq!(parse_id_text("Unknown"), None);
    }

    #[test]
    fn toolbar_id_text_and_name_range_check() {
        assert_eq!(
            ProgramGuideFrameSettings::toolbar_id_text(0),
            Some("TunerMenu")
        );
        assert_eq!(ProgramGuideFrameSettings::toolbar_name(4), Some("時刻バー"));
        assert_eq!(ProgramGuideFrameSettings::toolbar_id_text(-1), None);
        assert_eq!(ProgramGuideFrameSettings::toolbar_id_text(5), None);
    }

    #[test]
    fn default_settings_all_visible_identity_order() {
        let settings = ProgramGuideFrameSettings::new();
        for i in 0..TOOLBAR_NUM as i32 {
            assert!(settings.toolbar_visible(i));
        }
        assert_eq!(settings.toolbar_order_list(), [0, 1, 2, 3, 4]);
        assert_eq!(settings.date_bar_button_count(), DATEBAR_DEFAULTBUTTONCOUNT);
    }

    #[test]
    fn set_and_get_toolbar_visible() {
        let mut settings = ProgramGuideFrameSettings::new();
        assert!(settings.set_toolbar_visible(2, false));
        assert!(!settings.toolbar_visible(2));
        assert!(settings.toolbar_visible(0));
    }

    #[test]
    fn set_toolbar_visible_out_of_range() {
        let mut settings = ProgramGuideFrameSettings::new();
        assert!(!settings.set_toolbar_visible(-1, false));
        assert!(!settings.set_toolbar_visible(5, false));
    }

    #[test]
    fn set_and_get_toolbar_order_list_roundtrip() {
        let mut settings = ProgramGuideFrameSettings::new();
        let order = [4, 3, 2, 1, 0];
        assert!(settings.set_toolbar_order_list(&order));
        assert_eq!(settings.toolbar_order_list(), order);
    }

    #[test]
    fn set_toolbar_order_list_rejects_out_of_range() {
        let mut settings = ProgramGuideFrameSettings::new();
        let original = settings.toolbar_order_list();
        assert!(!settings.set_toolbar_order_list(&[0, 1, 2, 3, 5]));
        // 失敗時は変更されない
        assert_eq!(settings.toolbar_order_list(), original);
    }

    #[test]
    fn set_toolbar_order_list_rejects_duplicate() {
        let mut settings = ProgramGuideFrameSettings::new();
        let original = settings.toolbar_order_list();
        assert!(!settings.set_toolbar_order_list(&[0, 1, 1, 3, 4]));
        assert_eq!(settings.toolbar_order_list(), original);
    }

    #[test]
    fn set_date_bar_button_count_range() {
        let mut settings = ProgramGuideFrameSettings::new();
        assert!(settings.set_date_bar_button_count(1));
        assert_eq!(settings.date_bar_button_count(), 1);
        assert!(settings.set_date_bar_button_count(DATEBAR_MAXBUTTONCOUNT));
        assert!(!settings.set_date_bar_button_count(0));
        assert!(!settings.set_date_bar_button_count(DATEBAR_MAXBUTTONCOUNT + 1));
    }

    #[test]
    fn set_time_bar_settings_assigns() {
        let mut settings = ProgramGuideFrameSettings::new();
        let custom = TimeBarSettings {
            time: TimeType::Custom,
            interval: 6,
            custom_time: "1,2,3".to_string(),
            max_button_count: 12,
        };
        settings.set_time_bar_settings(custom.clone());
        assert_eq!(*settings.time_bar_settings(), custom);
    }

    #[test]
    fn time_type_from_i32_range_check() {
        assert_eq!(TimeType::from_i32(0), Some(TimeType::Interval));
        assert_eq!(TimeType::from_i32(1), Some(TimeType::Custom));
        assert_eq!(TimeType::from_i32(2), None);
        assert_eq!(TimeType::from_i32(-1), None);
    }

    #[test]
    fn time_type_to_i32_matches_static_cast() {
        assert_eq!(TimeType::Interval.to_i32(), 0);
        assert_eq!(TimeType::Custom.to_i32(), 1);
    }

    #[test]
    fn time_bar_settings_default_matches_header() {
        let settings = TimeBarSettings::default();
        assert_eq!(settings.time, TimeType::Interval);
        assert_eq!(settings.interval, 4);
        assert_eq!(settings.custom_time, "0,3,6,9,12,15,18,21");
        assert_eq!(settings.max_button_count, 10);
    }

    fn entry(name: Option<&str>) -> ToolbarNameEntry<'_> {
        ToolbarNameEntry { name }
    }

    #[test]
    fn parse_toolbar_order_identity_when_names_match_index() {
        let entries = [
            entry(Some("TunerMenu")),
            entry(Some("DateMenu")),
            entry(Some("Favorites")),
            entry(Some("DateBar")),
            entry(Some("TimeBar")),
        ];
        assert_eq!(parse_toolbar_order(&entries), [0, 1, 2, 3, 4]);
    }

    #[test]
    fn parse_toolbar_order_reordered() {
        let entries = [
            entry(Some("TimeBar")),
            entry(Some("DateBar")),
            entry(Some("Favorites")),
            entry(Some("DateMenu")),
            entry(Some("TunerMenu")),
        ];
        assert_eq!(parse_toolbar_order(&entries), [4, 3, 2, 1, 0]);
    }

    #[test]
    fn parse_toolbar_order_legacy_compat_uses_index() {
        // ver.0.9.0 より前の設定ファイルとの互換: Name 未取得(None)ならインデックスそのまま
        let entries = [
            entry(None),
            entry(None),
            entry(None),
            entry(None),
            entry(None),
        ];
        assert_eq!(parse_toolbar_order(&entries), [0, 1, 2, 3, 4]);
    }

    #[test]
    fn parse_toolbar_order_skips_unknown_name_and_fills_missing() {
        let entries = [
            entry(Some("UnknownToolbar")),
            entry(Some("TimeBar")),
            entry(None),
            entry(None),
            entry(None),
        ];
        // 1件目は未知のテキストで読み飛ばし、2件目は TimeBar(=4)。
        // 3〜5件目はインデックス2,3,4だが4は既出のため読み飛ばされ、
        // 不足分(0,1,3)が昇順で補われる。
        let order = parse_toolbar_order(&entries);
        assert_eq!(order[0], 4);
        // 残り4件は 0,1,2,3 が過不足なく含まれる
        let mut rest = order[1..].to_vec();
        rest.sort_unstable();
        assert_eq!(rest, vec![0, 1, 2, 3]);
    }

    #[test]
    fn parse_toolbar_order_skips_duplicate_ids() {
        let entries = [
            entry(Some("TunerMenu")),
            entry(Some("tunermenu")), // 大小無視で重複
            entry(Some("DateMenu")),
            entry(None),
            entry(None),
        ];
        let order = parse_toolbar_order(&entries);
        // 重複は1回だけカウントされ、残りは補完される
        assert_eq!(order[0], 0);
        assert_eq!(order[1], 1);
        let mut rest = order[2..].to_vec();
        rest.sort_unstable();
        assert_eq!(rest, vec![2, 3, 4]);
    }

    #[test]
    fn parse_toolbar_order_all_unknown_falls_back_to_identity() {
        let entries = [
            entry(Some("A")),
            entry(Some("B")),
            entry(Some("C")),
            entry(Some("D")),
            entry(Some("E")),
        ];
        assert_eq!(parse_toolbar_order(&entries), [0, 1, 2, 3, 4]);
    }
}
