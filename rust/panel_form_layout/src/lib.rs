//! TVTest の `CPanelForm`(src/PanelForm.cpp, src/PanelForm.h)のうち、
//! タブの管理(ID/表示可否/表示順)・タブ幅算出・ヒットテストという
//! Win32 API 呼び出しを伴わない純粋ロジック部分を移植したクレート。
//!
//! 対象外: ウィンドウ生成・GDI描画(`Draw`)・フォント計測(`GetTextExtentPoint32`)・
//! テーマ/スタイル反映・ツールチップ(`CTooltip`)。フォント計測結果(タブ幅)は
//! 呼び出し側が測定して渡す設計とする。

/// タブ1件分の情報(原実装 `CPanelForm::CWindowInfo`、PanelForm.h:145-155)。
/// `CPage`(実ウィンドウ)自体は保持せず、ID・表示可否のみを扱う。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabEntry {
    pub id: i32,
    pub visible: bool,
}

/// `CPanelForm::TabInfo`(PanelForm.h:81-85)相当。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabInfo {
    pub id: i32,
    pub visible: bool,
}

/// タブの登録・表示順・表示可否を管理する(`CPanelForm`の `m_WindowList` / `m_TabOrder` 相当)。
#[derive(Debug, Clone, Default)]
pub struct PanelTabList {
    entries: Vec<TabEntry>,
    /// 表示順に並んだ `entries` のインデックス列(`m_TabOrder`、PanelForm.h:173)。
    order: Vec<usize>,
    cur_tab: Option<usize>,
    prev_active_id: Option<i32>,
}

impl PanelTabList {
    pub fn new() -> Self {
        Self::default()
    }

    /// タブを末尾に追加する(`CPanelForm::AddPage`のタブ管理部分、PanelForm.cpp参照)。
    /// 追加後は表示順末尾にも加わる。
    pub fn add_tab(&mut self, id: i32, visible: bool) {
        let index = self.entries.len();
        self.entries.push(TabEntry { id, visible });
        self.order.push(index);
    }

    pub fn num_tabs(&self) -> usize {
        self.entries.len()
    }

    /// `CPanelForm::IDToIndex`(PanelForm.cpp:203-210)。登録順インデックスを返す。
    pub fn id_to_index(&self, id: i32) -> Option<usize> {
        self.entries.iter().position(|e| e.id == id)
    }

    /// `CPanelForm::GetCurPageID`(PanelForm.cpp:213-218)。
    pub fn cur_page_id(&self) -> Option<i32> {
        self.cur_tab.map(|i| self.entries[i].id)
    }

    /// `CPanelForm::SetCurPageByID`のインデックス解決部分(PanelForm.cpp:221-228)。
    /// 実際の `SetCurTab`(ウィンドウ表示切替・再描画)は呼び出し側が担う。
    pub fn set_cur_page_by_id(&mut self, id: i32) -> bool {
        match self.id_to_index(id) {
            Some(index) => {
                self.cur_tab = Some(index);
                true
            }
            None => false,
        }
    }

    /// `CPanelForm::SetTabVisible`の表示可否切替と、非表示時のカレントタブ再選出ロジック
    /// (PanelForm.cpp:231-269)。実際の再描画・`OnVisibilityChanged`呼び出しは対象外。
    /// 戻り値は `(成功したか, 表示状態が変化したか, 新しいカレントタブindex)`。
    pub fn set_tab_visible(&mut self, id: i32, visible: bool) -> (bool, bool, Option<usize>) {
        let Some(index) = self.id_to_index(id) else {
            return (false, false, self.cur_tab);
        };

        if self.entries[index].visible == visible {
            return (true, false, self.cur_tab);
        }

        self.entries[index].visible = visible;

        if !visible && self.cur_tab == Some(index) {
            let mut new_cur = None;

            if let Some(prev_id) = self.prev_active_id {
                if let Some(i) = self.id_to_index(prev_id) {
                    if self.entries[i].visible {
                        new_cur = Some(i);
                    }
                }
            }

            if new_cur.is_none() {
                new_cur = self.entries.iter().position(|e| e.visible);
            }

            self.cur_tab = new_cur;
        }

        (true, true, self.cur_tab)
    }

    /// `CPanelForm::GetTabVisible`(PanelForm.cpp:272-279)。
    pub fn tab_visible(&self, id: i32) -> bool {
        self.id_to_index(id)
            .is_some_and(|i| self.entries[i].visible)
    }

    /// `CPanelForm::SetTabOrder`(PanelForm.cpp:282-308)。
    /// `order` に含まれる各IDが1件も欠けず解決できた場合のみ表示順を置き換える。
    /// 原実装は件数の一致(全タブを含むか)は検証しない点を忠実に再現している。
    pub fn set_tab_order(&mut self, order: &[i32]) -> bool {
        let mut new_order = Vec::with_capacity(order.len());

        for &id in order {
            match self.id_to_index(id) {
                Some(index) => new_order.push(index),
                None => return false,
            }
        }

        self.order = new_order;
        true
    }

    /// `CPanelForm::GetTabInfo`(PanelForm.cpp:311-319)。`index` は表示順インデックス。
    pub fn tab_info(&self, index: usize) -> Option<TabInfo> {
        let entry_index = *self.order.get(index)?;
        let entry = &self.entries[entry_index];
        Some(TabInfo {
            id: entry.id,
            visible: entry.visible,
        })
    }

    /// `CPanelForm::GetTabID`(PanelForm.cpp:322-327)。`index` は表示順インデックス。
    pub fn tab_id(&self, index: usize) -> Option<i32> {
        let entry_index = *self.order.get(index)?;
        Some(self.entries[entry_index].id)
    }

    /// 表示順に並んだ、現在表示可能なタブのIDを列挙する(`HitTest`/`Draw`が辿る順序)。
    pub fn visible_tab_ids_in_order(&self) -> Vec<i32> {
        self.order
            .iter()
            .map(|&i| &self.entries[i])
            .filter(|e| e.visible)
            .map(|e| e.id)
            .collect()
    }

    /// 表示中のタブ数(`CPanelForm::GetRealTabWidth`の `NumVisibleTabs`、PanelForm.cpp:606-610)。
    pub fn num_visible_tabs(&self) -> usize {
        self.entries.iter().filter(|e| e.visible).count()
    }

    /// 前回アクティブだったページIDを記録する(`m_PrevActivePageID`)。
    pub fn set_prev_active_id(&mut self, id: Option<i32>) {
        self.prev_active_id = id;
    }
}

/// タブスタイル(原実装 `CPanelForm::TabStyle`、PanelForm.h:96-101)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabStyle {
    #[default]
    TextOnly,
    IconOnly,
    IconAndText,
}

/// タブ幅算出に必要なスタイル寸法(物理ピクセル、DPI換算後)。
/// 原実装 `CPanelForm::PanelFormStyle`(PanelForm.h:157-170)の幅計算に使う値のみ抽出。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabSizeStyle {
    pub tab_padding_horz: i32,
    pub tab_label_margin_horz: i32,
    pub tab_icon_size_width: i32,
    pub tab_icon_margin_horz: i32,
    pub tab_icon_label_margin: i32,
}

/// `CPanelForm::CalcTabSize`(PanelForm.cpp:567-600)のうち、フォント計測結果
/// (`max_label_width` = 表示中タブの中で最大のラベル幅)を受け取って
/// タブ幅を算出する部分。`GetTextExtentPoint32`によるラベル幅測定自体は対象外。
pub fn calc_tab_width(style: &TabSizeStyle, tab_style: TabStyle, max_label_width: i32) -> i32 {
    let mut width = style.tab_padding_horz;

    if tab_style != TabStyle::IconOnly {
        width += max_label_width + style.tab_label_margin_horz;
    }

    if tab_style != TabStyle::TextOnly {
        width += style.tab_icon_size_width + style.tab_icon_margin_horz;
        if tab_style == TabStyle::IconAndText {
            width += style.tab_icon_label_margin;
        }
    }

    width
}

/// `CPanelForm::GetRealTabWidth`(PanelForm.cpp:603-624)。
/// `fit_tab_width` が有効かつ全タブ幅がクライアント幅を超える場合、
/// クライアント幅に収まるよう縮小した幅を返す(最小幅は下回らない)。
pub fn real_tab_width(
    tab_width: i32,
    num_visible_tabs: i32,
    client_width: i32,
    fit_tab_width: bool,
    tab_style: TabStyle,
    tab_padding_horz: i32,
    tab_icon_size_width: i32,
) -> i32 {
    if fit_tab_width && num_visible_tabs > 0 && num_visible_tabs * tab_width > client_width {
        let width = client_width / num_visible_tabs;
        let min_width = tab_padding_horz
            + if tab_style != TabStyle::TextOnly {
                tab_icon_size_width
            } else {
                16
            };
        return width.max(min_width);
    }
    tab_width
}

/// `CPanelForm::HitTest`(PanelForm.cpp:627-645)。
/// `visible_ids_in_order` は表示順に並んだ表示中タブのID列(`visible_tab_ids_in_order`の結果)。
/// `tab_height` の範囲外、またはどのタブにも当たらない場合は `None`。
pub fn hit_test(
    x: i32,
    y: i32,
    tab_height: i32,
    real_tab_width: i32,
    visible_ids_in_order: &[i32],
) -> Option<i32> {
    if y < 0 || y >= tab_height {
        return None;
    }
    if real_tab_width <= 0 || x < 0 {
        return None;
    }

    let tab_index = x / real_tab_width;
    if tab_index < 0 {
        return None;
    }

    visible_ids_in_order.get(tab_index as usize).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_tab_appends_and_orders() {
        let mut tabs = PanelTabList::new();
        tabs.add_tab(1, true);
        tabs.add_tab(2, true);
        tabs.add_tab(3, false);

        assert_eq!(tabs.num_tabs(), 3);
        assert_eq!(tabs.id_to_index(2), Some(1));
        assert_eq!(tabs.id_to_index(99), None);
        assert_eq!(tabs.visible_tab_ids_in_order(), vec![1, 2]);
        assert_eq!(tabs.num_visible_tabs(), 2);
    }

    #[test]
    fn set_cur_page_by_id_resolves_index() {
        let mut tabs = PanelTabList::new();
        tabs.add_tab(10, true);
        tabs.add_tab(20, true);

        assert!(tabs.set_cur_page_by_id(20));
        assert_eq!(tabs.cur_page_id(), Some(20));
        assert!(!tabs.set_cur_page_by_id(999));
        // 失敗時は直前のカレントタブを維持する。
        assert_eq!(tabs.cur_page_id(), Some(20));
    }

    #[test]
    fn set_tab_visible_no_change_when_same_state() {
        let mut tabs = PanelTabList::new();
        tabs.add_tab(1, true);

        let (ok, changed, _) = tabs.set_tab_visible(1, true);
        assert!(ok);
        assert!(!changed);
    }

    #[test]
    fn set_tab_visible_unknown_id_fails() {
        let mut tabs = PanelTabList::new();
        tabs.add_tab(1, true);

        let (ok, changed, cur) = tabs.set_tab_visible(999, false);
        assert!(!ok);
        assert!(!changed);
        assert_eq!(cur, None);
    }

    #[test]
    fn set_tab_visible_hiding_current_falls_back_to_prev_active() {
        let mut tabs = PanelTabList::new();
        tabs.add_tab(1, true);
        tabs.add_tab(2, true);
        tabs.add_tab(3, true);
        tabs.set_cur_page_by_id(2);
        tabs.set_prev_active_id(Some(3));

        let (ok, changed, cur) = tabs.set_tab_visible(2, false);
        assert!(ok);
        assert!(changed);
        // prev_active_id(3)が表示中なのでそちらへフォールバックする。
        assert_eq!(cur, Some(2));
        assert_eq!(tabs.cur_page_id(), Some(3));
    }

    #[test]
    fn set_tab_visible_hiding_current_falls_back_to_first_visible_when_prev_hidden() {
        let mut tabs = PanelTabList::new();
        tabs.add_tab(1, true);
        tabs.add_tab(2, true);
        tabs.add_tab(3, true);
        tabs.set_cur_page_by_id(2);
        tabs.set_prev_active_id(Some(3));
        // prev_active(3)も非表示にしておく。
        tabs.set_tab_visible(3, false);

        let (ok, changed, cur) = tabs.set_tab_visible(2, false);
        assert!(ok);
        assert!(changed);
        // 先頭から探して最初に見つかる表示中タブ(ID=1)にフォールバックする。
        assert_eq!(cur, Some(0));
        assert_eq!(tabs.cur_page_id(), Some(1));
    }

    #[test]
    fn set_tab_visible_hiding_non_current_does_not_change_cur_tab() {
        let mut tabs = PanelTabList::new();
        tabs.add_tab(1, true);
        tabs.add_tab(2, true);
        tabs.set_cur_page_by_id(1);

        let (ok, changed, cur) = tabs.set_tab_visible(2, false);
        assert!(ok);
        assert!(changed);
        assert_eq!(cur, Some(0));
        assert_eq!(tabs.cur_page_id(), Some(1));
    }

    #[test]
    fn tab_visible_reports_state() {
        let mut tabs = PanelTabList::new();
        tabs.add_tab(1, true);
        tabs.add_tab(2, false);

        assert!(tabs.tab_visible(1));
        assert!(!tabs.tab_visible(2));
        assert!(!tabs.tab_visible(999));
    }

    #[test]
    fn set_tab_order_reorders_by_id() {
        let mut tabs = PanelTabList::new();
        tabs.add_tab(1, true);
        tabs.add_tab(2, true);
        tabs.add_tab(3, true);

        assert!(tabs.set_tab_order(&[3, 1, 2]));
        assert_eq!(tabs.tab_id(0), Some(3));
        assert_eq!(tabs.tab_id(1), Some(1));
        assert_eq!(tabs.tab_id(2), Some(2));
    }

    #[test]
    fn set_tab_order_fails_on_unknown_id_and_keeps_old_order() {
        let mut tabs = PanelTabList::new();
        tabs.add_tab(1, true);
        tabs.add_tab(2, true);

        assert!(!tabs.set_tab_order(&[1, 999]));
        // 失敗時は元の順序を維持する(原実装通り、途中まで積んだ一時ベクタは破棄される)。
        assert_eq!(tabs.tab_id(0), Some(1));
        assert_eq!(tabs.tab_id(1), Some(2));
    }

    #[test]
    fn set_tab_order_allows_partial_subset() {
        // 原実装(SetTabOrder)は渡された件数分だけを解決するのみで、
        // 全タブを含むことの検証は行わない。
        let mut tabs = PanelTabList::new();
        tabs.add_tab(1, true);
        tabs.add_tab(2, true);
        tabs.add_tab(3, true);

        assert!(tabs.set_tab_order(&[2]));
        assert_eq!(
            tabs.tab_info(0),
            Some(TabInfo {
                id: 2,
                visible: true
            })
        );
        assert_eq!(tabs.tab_info(1), None);
    }

    #[test]
    fn tab_info_and_tab_id_use_display_order() {
        let mut tabs = PanelTabList::new();
        tabs.add_tab(1, true);
        tabs.add_tab(2, false);
        tabs.set_tab_order(&[2, 1]);

        assert_eq!(
            tabs.tab_info(0),
            Some(TabInfo {
                id: 2,
                visible: false
            })
        );
        assert_eq!(tabs.tab_id(1), Some(1));
        assert_eq!(tabs.tab_info(99), None);
        assert_eq!(tabs.tab_id(99), None);
    }

    #[test]
    fn calc_tab_width_text_only() {
        let style = TabSizeStyle {
            tab_padding_horz: 6,
            tab_label_margin_horz: 4,
            tab_icon_size_width: 16,
            tab_icon_margin_horz: 2,
            tab_icon_label_margin: 4,
        };
        assert_eq!(calc_tab_width(&style, TabStyle::TextOnly, 50), 6 + 50 + 4);
    }

    #[test]
    fn calc_tab_width_icon_only() {
        let style = TabSizeStyle {
            tab_padding_horz: 6,
            tab_label_margin_horz: 4,
            tab_icon_size_width: 16,
            tab_icon_margin_horz: 2,
            tab_icon_label_margin: 4,
        };
        assert_eq!(calc_tab_width(&style, TabStyle::IconOnly, 50), 6 + 16 + 2);
    }

    #[test]
    fn calc_tab_width_icon_and_text() {
        let style = TabSizeStyle {
            tab_padding_horz: 6,
            tab_label_margin_horz: 4,
            tab_icon_size_width: 16,
            tab_icon_margin_horz: 2,
            tab_icon_label_margin: 4,
        };
        assert_eq!(
            calc_tab_width(&style, TabStyle::IconAndText, 50),
            6 + 50 + 4 + 16 + 2 + 4
        );
    }

    #[test]
    fn real_tab_width_no_shrink_when_fits() {
        let w = real_tab_width(80, 3, 300, true, TabStyle::TextOnly, 6, 16);
        assert_eq!(w, 80);
    }

    #[test]
    fn real_tab_width_shrinks_to_fit_client_width() {
        // 3タブ*80 = 240 > client_width(200) なので縮小する。
        let w = real_tab_width(80, 3, 200, true, TabStyle::TextOnly, 6, 16);
        assert_eq!(w, 200 / 3);
    }

    #[test]
    fn real_tab_width_respects_min_width_text_only() {
        // 極端に狭いクライアント幅でも最小幅(パディング+16)は下回らない。
        let w = real_tab_width(80, 10, 10, true, TabStyle::TextOnly, 6, 16);
        assert_eq!(w, 6 + 16);
    }

    #[test]
    fn real_tab_width_respects_min_width_with_icon() {
        let w = real_tab_width(80, 10, 10, true, TabStyle::IconAndText, 6, 20);
        assert_eq!(w, 6 + 20);
    }

    #[test]
    fn real_tab_width_disabled_fit_returns_tab_width() {
        let w = real_tab_width(80, 3, 100, false, TabStyle::TextOnly, 6, 16);
        assert_eq!(w, 80);
    }

    #[test]
    fn real_tab_width_zero_visible_tabs_returns_tab_width() {
        let w = real_tab_width(80, 0, 100, true, TabStyle::TextOnly, 6, 16);
        assert_eq!(w, 80);
    }

    #[test]
    fn hit_test_outside_tab_height_returns_none() {
        assert_eq!(hit_test(10, -1, 24, 80, &[1, 2]), None);
        assert_eq!(hit_test(10, 24, 24, 80, &[1, 2]), None);
    }

    #[test]
    fn hit_test_negative_x_returns_none() {
        assert_eq!(hit_test(-1, 10, 24, 80, &[1, 2]), None);
    }

    #[test]
    fn hit_test_finds_correct_tab() {
        let ids = [10, 20, 30];
        assert_eq!(hit_test(0, 5, 24, 80, &ids), Some(10));
        assert_eq!(hit_test(79, 5, 24, 80, &ids), Some(10));
        assert_eq!(hit_test(80, 5, 24, 80, &ids), Some(20));
        assert_eq!(hit_test(160, 5, 24, 80, &ids), Some(30));
    }

    #[test]
    fn hit_test_beyond_last_tab_returns_none() {
        let ids = [10, 20];
        assert_eq!(hit_test(200, 5, 24, 80, &ids), None);
    }

    #[test]
    fn hit_test_zero_tab_width_returns_none() {
        assert_eq!(hit_test(0, 5, 24, 0, &[1]), None);
    }
}
