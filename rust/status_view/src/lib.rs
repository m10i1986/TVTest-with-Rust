//! TVTest のステータスバー(src/StatusView.cpp / StatusView.h)の項目レイアウト移植。
//!
//! ステータスバーの項目を横一列に並べ、幅が足りなければ複数行へ折り返すアルゴリズムを、
//! ウィンドウ/描画から切り離した純粋なモデルとして表現する:
//! - [`ItemStyle`](可変幅 / フル行 / 強制フル行)
//! - [`StatusItem`](ID / 幅 / 可視 + 算出結果の実幅・改行フラグ)
//! - [`StatusView`](行折り返し計算 `calc_layout` と ID 索引・全幅集計)
//!
//! # 対象外(Win32 / 描画依存)
//! 描画(`Draw`)、テーマ枠・パディングのピクセル変換、フォント計測(`CalcTextHeight`)、
//! 項目の縦位置・テーマ枠を含む矩形計算(`GetItemRectByIndex` の枠/高さ部)、
//! ウィンドウ管理(`CBasicWindow`)、マウス操作。
//!
//! 項目の幅(`GetWidth`)・可視(`GetVisible`)・スタイルはデータとして与え、
//! `calc_layout` が実幅(`m_ActualWidth`)と改行フラグ(`m_fBreak`)・行数を算出する。

use bitflags::bitflags;

bitflags! {
    /// 項目のスタイル(StatusView.h 44-50 `CStatusItem::StyleFlag`)。
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
    pub struct ItemStyle: u32 {
        /// 余白を埋めるよう幅が伸びる項目(`VariableWidth`)。
        const VARIABLE_WIDTH = 0x0001;
        /// 単独で 1 行を占める項目(`FullRow`)。
        const FULL_ROW = 0x0002;
        /// 強制的に単独で 1 行を占める項目(`ForceFullRow`)。
        const FORCE_FULL_ROW = 0x0004;
    }
}

/// ステータスバーの項目(StatusView.h 40-137 `CStatusItem` の幾何関連部)。
///
/// `width`/`visible`/`style` は入力。`actual_width`/`break_after` は
/// [`StatusView::calc_layout`] が算出する結果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatusItem {
    id: i32,
    width: i32,
    visible: bool,
    style: ItemStyle,
    actual_width: i32,
    break_after: bool,
}

impl StatusItem {
    /// 項目を生成する。
    pub fn new(id: i32, width: i32, visible: bool, style: ItemStyle) -> Self {
        Self {
            id,
            width,
            visible,
            style,
            // m_ActualWidth は既定 -1(StatusView.h 133)。calc_layout で確定する。
            actual_width: -1,
            break_after: false,
        }
    }

    /// ID を返す(`GetID`)。
    pub fn id(&self) -> i32 {
        self.id
    }

    /// 設定幅を返す(`GetWidth`)。
    pub fn width(&self) -> i32 {
        self.width
    }

    /// 設定幅を変更する。
    pub fn set_width(&mut self, width: i32) {
        self.width = width;
    }

    /// 可視か(`GetVisible`)。
    pub fn visible(&self) -> bool {
        self.visible
    }

    /// 可視状態を設定する。
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// スタイルを返す。
    pub fn style(&self) -> ItemStyle {
        self.style
    }

    /// 可変幅か(StatusView.h 95 `IsVariableWidth`)。
    pub fn is_variable_width(&self) -> bool {
        self.style.contains(ItemStyle::VARIABLE_WIDTH)
    }

    /// フル行か(StatusView.h 96 `IsFullRow`)。
    pub fn is_full_row(&self) -> bool {
        self.style.contains(ItemStyle::FULL_ROW)
    }

    /// 強制フル行か(StatusView.h 97 `IsForceFullRow`)。
    pub fn is_force_full_row(&self) -> bool {
        self.style.contains(ItemStyle::FORCE_FULL_ROW)
    }

    /// 算出された実幅を返す(`GetActualWidth`)。
    pub fn actual_width(&self) -> i32 {
        self.actual_width
    }

    /// この項目で行が終わる(直後で改行する)か(`m_fBreak`)。
    pub fn break_after(&self) -> bool {
        self.break_after
    }
}

/// ステータスバーの項目レイアウト(StatusView.h 的 `CStatusView` の幾何関連部)。
#[derive(Clone, Debug)]
pub struct StatusView {
    items: Vec<StatusItem>,
    multi_row: bool,
    max_rows: i32,
    item_padding_horz: i32,
    rows: i32,
}

impl Default for StatusView {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            // m_fMultiRow 既定 false / m_MaxRows 既定 2(StatusView.h 273-274)。
            multi_row: false,
            max_rows: 2,
            item_padding_horz: 0,
            rows: 1,
        }
    }
}

impl StatusView {
    /// 空のステータスバーを生成する。
    pub fn new() -> Self {
        Self::default()
    }

    /// 複数行表示の許可を設定する。
    pub fn set_multi_row(&mut self, multi_row: bool) {
        self.multi_row = multi_row;
    }

    /// 最大行数を設定する。
    pub fn set_max_rows(&mut self, max_rows: i32) {
        self.max_rows = max_rows;
    }

    /// 項目の左右パディング合計(`ItemPadding.Horz()`)を設定する。
    pub fn set_item_padding_horz(&mut self, padding: i32) {
        self.item_padding_horz = padding;
    }

    /// 項目を追加する。
    pub fn add_item(&mut self, item: StatusItem) {
        self.items.push(item);
    }

    /// 全項目を消去する。
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// 項目数を返す。
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// 索引で項目を返す。
    pub fn get_item(&self, index: usize) -> Option<&StatusItem> {
        self.items.get(index)
    }

    /// 全項目を返す。
    pub fn items(&self) -> &[StatusItem] {
        &self.items
    }

    /// 算出された行数を返す(`GetRows` 相当)。
    pub fn rows(&self) -> i32 {
        self.rows
    }

    /// ID から索引を返す(StatusView.cpp 363-371 `IDToIndex`)。
    pub fn id_to_index(&self, id: i32) -> Option<usize> {
        self.items.iter().position(|item| item.id == id)
    }

    /// 索引から ID を返す(StatusView.cpp 373-378 `IndexToID`)。
    pub fn index_to_id(&self, index: usize) -> Option<i32> {
        self.items.get(index).map(|item| item.id)
    }

    /// 可視項目の合計幅 + 左右の枠幅を返す(StatusView.cpp 754-765 `GetIntegralWidth`)。
    pub fn integral_width(&self, border_left: i32, border_right: i32) -> i32 {
        let mut width = 0;
        for item in &self.items {
            if item.visible {
                width += item.width + self.item_padding_horz;
            }
        }
        width + border_left + border_right
    }

    /// 行折り返しを計算し、各項目の実幅・改行フラグと行数を確定する
    /// (StatusView.cpp 1205-1244 `CalcLayout` + 1281-1318 `CalcRows`)。
    ///
    /// `max_row_width` は枠を除いたクライアント幅。戻り値は行数。
    pub fn calc_layout(&mut self, max_row_width: i32) -> i32 {
        let padding = self.item_padding_horz;

        // 改行フラグを解除し、実幅を設定幅に戻す(CalcLayout 1209-1215)。
        for item in &mut self.items {
            item.break_after = false;
            item.actual_width = item.width;
        }

        // 可視項目のみを対象に索引列を作る。
        let visible: Vec<usize> = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.visible)
            .map(|(i, _)| i)
            .collect();

        self.rows = self.calc_rows(&visible, max_row_width);

        // 可変幅項目を行の余白だけ広げる(CalcLayout 1226-1243)。
        let mut variable_item: Option<usize> = None;
        let mut row_width = 0;
        for (k, &idx) in visible.iter().enumerate() {
            if self.items[idx].is_variable_width() {
                variable_item = Some(idx);
            }
            row_width += self.items[idx].actual_width + padding;
            if self.items[idx].break_after || k + 1 == visible.len() {
                if let Some(vi) = variable_item {
                    let add = max_row_width - row_width;
                    if add > 0 {
                        self.items[vi].actual_width += add;
                    }
                }
                variable_item = None;
                row_width = 0;
            }
        }

        self.rows
    }

    /// 行数を数えつつ改行フラグ・フル行項目の実幅を設定する
    /// (StatusView.cpp 1281-1318 `CalcRows`、副作用あり版)。
    fn calc_rows(&mut self, visible: &[usize], max_row_width: i32) -> i32 {
        let padding = self.item_padding_horz;
        let max_regular_rows = if self.multi_row { self.max_rows } else { 1 };
        let mut rows = 1;
        let mut row_width = 0;
        let n = visible.len();

        for i in 0..n {
            let idx = visible[i];

            // フル行項目: 条件を満たせば単独行にする。
            if self.items[idx].is_full_row()
                && (self.items[idx].is_force_full_row()
                    || (rows < max_regular_rows
                        && (i == 0 || i + 1 == n || rows + 1 < max_regular_rows)))
            {
                if i > 0 {
                    self.items[visible[i - 1]].break_after = true;
                }
                self.items[idx].actual_width = max_row_width - padding;
                self.items[idx].break_after = true;
                rows += 1;
                if i + 1 < n && rows < max_regular_rows {
                    rows += 1;
                }
                row_width = 0;
                continue;
            }

            let item_width = self.items[idx].width + padding;
            if rows < max_regular_rows && row_width > 0 && row_width + item_width > max_row_width {
                if i > 0 {
                    self.items[visible[i - 1]].break_after = true;
                }
                rows += 1;
                row_width = item_width;
            } else {
                row_width += item_width;
            }
        }

        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: i32, width: i32) -> StatusItem {
        StatusItem::new(id, width, true, ItemStyle::empty())
    }

    #[test]
    fn single_row_fits() {
        let mut v = StatusView::new();
        v.add_item(item(1, 30));
        v.add_item(item(2, 30));
        v.add_item(item(3, 30));
        // 単一行モード(multi_row=false)では折り返さない。
        assert_eq!(v.calc_layout(100), 1);
        assert!(v.items().iter().all(|it| !it.break_after()));
        // 実幅は設定幅のまま。
        assert_eq!(v.get_item(0).unwrap().actual_width(), 30);
    }

    #[test]
    fn single_row_no_wrap_even_when_overflow() {
        let mut v = StatusView::new();
        for i in 0..5 {
            v.add_item(item(i, 40));
        }
        // multi_row=false なので幅超過でも 1 行のまま。
        assert_eq!(v.calc_layout(100), 1);
        assert!(v.items().iter().all(|it| !it.break_after()));
    }

    #[test]
    fn multi_row_wraps() {
        let mut v = StatusView::new();
        v.set_multi_row(true);
        v.set_max_rows(3);
        for i in 0..5 {
            v.add_item(item(i, 40));
        }
        // 幅 40×5、行幅 100、最大 3 行。i1 と i3 の直後で改行し 3 行。
        assert_eq!(v.calc_layout(100), 3);
        assert!(v.get_item(1).unwrap().break_after());
        assert!(v.get_item(3).unwrap().break_after());
        assert!(!v.get_item(0).unwrap().break_after());
        assert!(!v.get_item(2).unwrap().break_after());
        assert!(!v.get_item(4).unwrap().break_after());
    }

    #[test]
    fn variable_width_expands_to_fill_row() {
        let mut v = StatusView::new();
        v.add_item(item(1, 30));
        v.add_item(StatusItem::new(2, 20, true, ItemStyle::VARIABLE_WIDTH));
        v.add_item(item(3, 30));
        // 行幅 100、合計 80 → 可変項目が 20 広がって 40 になる。
        v.calc_layout(100);
        assert_eq!(v.get_item(1).unwrap().actual_width(), 40);
        // 固定項目は不変。
        assert_eq!(v.get_item(0).unwrap().actual_width(), 30);
        assert_eq!(v.get_item(2).unwrap().actual_width(), 30);
    }

    #[test]
    fn variable_width_no_expand_when_full() {
        let mut v = StatusView::new();
        v.add_item(item(1, 50));
        v.add_item(StatusItem::new(2, 50, true, ItemStyle::VARIABLE_WIDTH));
        // 合計 100 = 行幅 → 余白なしで広がらない。
        v.calc_layout(100);
        assert_eq!(v.get_item(1).unwrap().actual_width(), 50);
    }

    #[test]
    fn force_full_row_occupies_a_row() {
        let mut v = StatusView::new();
        v.set_multi_row(true);
        v.set_max_rows(3);
        v.add_item(item(1, 30));
        // ForceFullRow は IsFullRow ガードの内側で効くため FULL_ROW も併せ持つ。
        v.add_item(StatusItem::new(
            2,
            50,
            true,
            ItemStyle::FULL_ROW | ItemStyle::FORCE_FULL_ROW,
        ));
        v.add_item(item(3, 30));
        v.calc_layout(100);
        // フル行項目の前で改行、フル行項目自身も改行、実幅は行幅。
        assert!(v.get_item(0).unwrap().break_after());
        assert!(v.get_item(1).unwrap().break_after());
        assert_eq!(v.get_item(1).unwrap().actual_width(), 100);
    }

    #[test]
    fn invisible_items_are_skipped() {
        let mut v = StatusView::new();
        v.set_multi_row(true);
        v.set_max_rows(3);
        v.add_item(item(1, 40));
        v.add_item(StatusItem::new(2, 40, false, ItemStyle::empty())); // 非可視
        v.add_item(item(3, 40));
        // 可視は 2 項目(40+40=80<=100)なので 1 行。
        assert_eq!(v.calc_layout(100), 1);
        assert!(v.items().iter().all(|it| !it.break_after()));
    }

    #[test]
    fn padding_counts_toward_width() {
        let mut v = StatusView::new();
        v.set_multi_row(true);
        v.set_max_rows(2);
        v.set_item_padding_horz(10);
        v.add_item(item(1, 40)); // 実質 50
        v.add_item(item(2, 40)); // 実質 50 → 計 100 = 行幅、まだ収まる
        v.add_item(item(3, 40)); // 100+50>100 → 改行
        assert_eq!(v.calc_layout(100), 2);
        assert!(v.get_item(1).unwrap().break_after());
    }

    #[test]
    fn id_index_lookup() {
        let mut v = StatusView::new();
        v.add_item(item(10, 30));
        v.add_item(item(20, 30));
        assert_eq!(v.id_to_index(20), Some(1));
        assert_eq!(v.id_to_index(99), None);
        assert_eq!(v.index_to_id(0), Some(10));
        assert_eq!(v.index_to_id(5), None);
    }

    #[test]
    fn integral_width_sums_visible_plus_border() {
        let mut v = StatusView::new();
        v.set_item_padding_horz(4);
        v.add_item(item(1, 30)); // 34
        v.add_item(StatusItem::new(2, 30, false, ItemStyle::empty())); // 非可視 → 除外
        v.add_item(item(3, 20)); // 24
        // 34 + 24 + 枠(2 + 3) = 63。
        assert_eq!(v.integral_width(2, 3), 63);
    }
}
