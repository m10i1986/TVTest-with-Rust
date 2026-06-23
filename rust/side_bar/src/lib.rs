//! TVTest のサイドバー(src/SideBar.cpp / SideBar.h)の項目モデルとボタン幾何の移植。
//!
//! コマンドに対応するボタンを縦(または横)一列に並べたサイドバーを、ウィンドウ/描画から
//! 切り離した純粋なモデルとして表現する:
//! - [`ItemState`](無効 / チェック / ホット)
//! - [`SideBarItem`](コマンド / アイコン / 状態。コマンド 0 = 区切り)
//! - [`SideBar`](項目リスト・状態管理・バー幅/ボタン矩形/ヒットテスト)
//!
//! # 対象外(Win32 / 描画依存)
//! アイコン描画(`Draw`/`DrawIcon`)、テーマ(`SideBarTheme`)、ツールチップ、
//! ウィンドウ管理(`CCustomWindow`)、マウス処理、スタイルの DPI スケーリング。
//!
//! テーマ枠の幅は [`BorderWidths`] として、項目の寸法(アイコンサイズ・パディング・
//! 区切り幅)はフィールドとして与え、幾何メソッドが純粋に矩形を算出する。

use bitflags::bitflags;

/// 区切り項目を表すコマンド値(SideBar.h 43 `ITEM_SEPARATOR`)。
pub const ITEM_SEPARATOR: i32 = 0;

/// アイコンの既定幅(SideBar.h 170 `ICON_WIDTH`)。
pub const ICON_WIDTH: i32 = 16;
/// アイコンの既定高さ(SideBar.h 171 `ICON_HEIGHT`)。
pub const ICON_HEIGHT: i32 = 16;

bitflags! {
    /// 項目の状態(SideBar.h 45-51 `CSideBar::ItemState`)。
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
    pub struct ItemState: u32 {
        /// 無効(操作不可)。
        const DISABLED = 0x0001;
        /// チェック済み。
        const CHECKED = 0x0002;
        /// ホット(マウスが乗っている)。
        const HOT = 0x0004;
    }
}

/// サイドバーの項目(SideBar.h 53-62 `CSideBar::SideBarItem`)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SideBarItem {
    pub command: i32,
    pub icon: i32,
    pub state: ItemState,
}

impl SideBarItem {
    /// 項目を生成する。
    pub fn new(command: i32, icon: i32, state: ItemState) -> Self {
        Self {
            command,
            icon,
            state,
        }
    }

    /// 無効か(SideBar.h 59 `IsDisabled`)。
    pub fn is_disabled(&self) -> bool {
        self.state.contains(ItemState::DISABLED)
    }

    /// 有効か(SideBar.h 60 `IsEnabled`)。
    pub fn is_enabled(&self) -> bool {
        !self.is_disabled()
    }

    /// チェック済みか(SideBar.h 61 `IsChecked`)。
    pub fn is_checked(&self) -> bool {
        self.state.contains(ItemState::CHECKED)
    }

    /// 区切りか(コマンドが `ITEM_SEPARATOR`)。
    pub fn is_separator(&self) -> bool {
        self.command == ITEM_SEPARATOR
    }
}

/// 上下左右の余白(`Style::Margins` 相当)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Margins {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Margins {
    /// 全辺同じ値で生成する(`Style::Margins{3}` 相当)。
    pub const fn all(value: i32) -> Self {
        Self {
            left: value,
            top: value,
            right: value,
            bottom: value,
        }
    }

    /// 左右の合計(`Horz()`)。
    pub const fn horz(&self) -> i32 {
        self.left + self.right
    }

    /// 上下の合計(`Vert()`)。
    pub const fn vert(&self) -> i32 {
        self.top + self.bottom
    }
}

/// テーマ枠の各辺の幅(`Theme::GetBorderWidths` の結果)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct BorderWidths {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// 矩形(Win32 `RECT` 相当)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// サイドバー(SideBar.h 38-187 `CSideBar` の項目/幾何部)。
#[derive(Clone, Debug)]
pub struct SideBar {
    items: Vec<SideBarItem>,
    icon_width: i32,
    icon_height: i32,
    item_padding: Margins,
    separator_width: i32,
    border: BorderWidths,
    vertical: bool,
}

impl Default for SideBar {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            // SideBarStyle の既定(SideBar.h 146-148)。
            icon_width: ICON_WIDTH,
            icon_height: ICON_HEIGHT,
            item_padding: Margins::all(3),
            separator_width: 8,
            border: BorderWidths::default(),
            // m_fVertical 既定 true(SideBar.h 161)。
            vertical: true,
        }
    }
}

impl SideBar {
    /// 空のサイドバーを生成する。
    pub fn new() -> Self {
        Self::default()
    }

    /// 縦並びかを設定する(`SetVertical`)。
    pub fn set_vertical(&mut self, vertical: bool) {
        self.vertical = vertical;
    }

    /// 縦並びか(`GetVertical`)。
    pub fn vertical(&self) -> bool {
        self.vertical
    }

    /// アイコンサイズを設定する。
    pub fn set_icon_size(&mut self, width: i32, height: i32) {
        self.icon_width = width;
        self.icon_height = height;
    }

    /// 項目パディングを設定する。
    pub fn set_item_padding(&mut self, padding: Margins) {
        self.item_padding = padding;
    }

    /// 区切り幅を設定する。
    pub fn set_separator_width(&mut self, width: i32) {
        self.separator_width = width;
    }

    /// テーマ枠の幅を設定する。
    pub fn set_border(&mut self, border: BorderWidths) {
        self.border = border;
    }

    /// 全項目を削除する(SideBar.cpp 142-147 `DeleteAllItems`)。
    pub fn delete_all_items(&mut self) {
        self.items.clear();
    }

    /// 項目を追加する(SideBar.cpp 149-175 `AddItem`/`AddItems`)。
    pub fn add_item(&mut self, item: SideBarItem) {
        self.items.push(item);
    }

    /// 複数項目を追加する(`AddItems`)。
    pub fn add_items(&mut self, items: &[SideBarItem]) {
        self.items.extend_from_slice(items);
    }

    /// 区切りを追加する(SideBar.cpp 178-187 `AddSeparator`。アイコン -1・状態なし)。
    pub fn add_separator(&mut self) {
        self.items.push(SideBarItem::new(ITEM_SEPARATOR, -1, ItemState::empty()));
    }

    /// 項目数を返す(`GetItemCount`)。
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// 全項目を返す。
    pub fn items(&self) -> &[SideBarItem] {
        &self.items
    }

    /// 索引のコマンドを返す(SideBar.cpp 196-202 `GetItemCommand`。範囲外は `None`)。
    pub fn get_item_command(&self, index: usize) -> Option<i32> {
        self.items.get(index).map(|item| item.command)
    }

    /// コマンドから索引を返す(SideBar.cpp 204-212 `CommandToIndex`。無ければ `None`)。
    pub fn command_to_index(&self, command: i32) -> Option<usize> {
        self.items.iter().position(|item| item.command == command)
    }

    /// コマンドで有効/無効を設定する(SideBar.cpp 214-228 `EnableItem`)。
    pub fn enable_item(&mut self, command: i32, enable: bool) -> bool {
        let Some(index) = self.command_to_index(command) else {
            return false;
        };
        self.enable_item_by_index(index, enable)
    }

    /// 索引で有効/無効を設定する(SideBar.cpp 230-242 `EnableItemByIndex`)。
    ///
    /// 状態が変わるときのみ `Disabled` ビットを反転する。
    pub fn enable_item_by_index(&mut self, index: usize, enable: bool) -> bool {
        let Some(item) = self.items.get_mut(index) else {
            return false;
        };
        if item.is_enabled() != enable {
            item.state ^= ItemState::DISABLED;
        }
        true
    }

    /// コマンドが有効か(SideBar.cpp 244-251 `IsItemEnabled`)。
    pub fn is_item_enabled(&self, command: i32) -> bool {
        match self.command_to_index(command) {
            Some(index) => self.items[index].is_enabled(),
            None => false,
        }
    }

    /// コマンドでチェック状態を設定する(SideBar.cpp 254-266 `CheckItem`)。
    pub fn check_item(&mut self, command: i32, check: bool) -> bool {
        let Some(index) = self.command_to_index(command) else {
            return false;
        };
        self.check_item_by_index(index, check)
    }

    /// 索引でチェック状態を設定する(SideBar.cpp 268-278 `CheckItemByIndex`)。
    pub fn check_item_by_index(&mut self, index: usize, check: bool) -> bool {
        let Some(item) = self.items.get_mut(index) else {
            return false;
        };
        if item.is_checked() != check {
            item.state ^= ItemState::CHECKED;
        }
        true
    }

    /// コマンド範囲をラジオ選択する(SideBar.cpp 280-287 `CheckRadioItem`)。
    ///
    /// `first..=last` のうち `check` のみチェックする。`first > last` なら `false`。
    pub fn check_radio_item(&mut self, first: i32, last: i32, check: i32) -> bool {
        if first > last {
            return false;
        }
        for command in first..=last {
            self.check_item(command, command == check);
        }
        true
    }

    /// コマンドがチェック済みか(SideBar.cpp 290-297 `IsItemChecked`)。
    pub fn is_item_checked(&self, command: i32) -> bool {
        match self.command_to_index(command) {
            Some(index) => self.items[index].is_checked(),
            None => false,
        }
    }

    /// バーの幅(縦並び時)/高さ(横並び時)を返す(SideBar.cpp 114-127 `GetBarWidth`)。
    pub fn bar_width(&self) -> i32 {
        if self.vertical {
            self.icon_width + self.item_padding.horz() + self.border.left + self.border.right
        } else {
            self.icon_height + self.item_padding.vert() + self.border.top + self.border.bottom
        }
    }

    /// 項目 1 つ分の幅(アイコン幅 + 左右パディング)。
    fn item_width(&self) -> i32 {
        self.icon_width + self.item_padding.horz()
    }

    /// 項目 1 つ分の高さ(アイコン高さ + 上下パディング)。
    fn item_height(&self) -> i32 {
        self.icon_height + self.item_padding.vert()
    }

    /// 索引の項目の矩形を返す(SideBar.cpp 591-622 `GetItemRect`。範囲外は `None`)。
    pub fn get_item_rect(&self, index: usize) -> Option<Rect> {
        if index >= self.items.len() {
            return None;
        }
        let item_width = self.item_width();
        let item_height = self.item_height();

        let mut offset = 0;
        for item in &self.items[..index] {
            if item.is_separator() {
                offset += self.separator_width;
            } else if self.vertical {
                offset += item_height;
            } else {
                offset += item_width;
            }
        }

        let is_sep = self.items[index].is_separator();
        let mut rect = Rect::default();
        if self.vertical {
            rect.left = self.border.left;
            rect.right = self.border.left + item_width;
            rect.top = self.border.top + offset;
            rect.bottom = rect.top + if is_sep { self.separator_width } else { item_height };
        } else {
            rect.top = self.border.top;
            rect.bottom = self.border.top + item_height;
            rect.left = self.border.left + offset;
            rect.right = rect.left + if is_sep { self.separator_width } else { item_width };
        }
        Some(rect)
    }

    /// 座標から項目索引を求める(SideBar.cpp 636-646 `HitTest`。無ければ `None`)。
    ///
    /// 判定は Win32 `PtInRect` と同じく左上を含み右下を含まない。
    pub fn hit_test(&self, x: i32, y: i32) -> Option<usize> {
        (0..self.items.len()).find(|&i| {
            let rect = self.get_item_rect(i).expect("索引は範囲内");
            x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn btn(command: i32) -> SideBarItem {
        SideBarItem::new(command, command, ItemState::empty())
    }

    #[test]
    fn add_items_and_lookup() {
        let mut bar = SideBar::new();
        bar.add_item(btn(10));
        bar.add_separator();
        bar.add_item(btn(20));
        assert_eq!(bar.item_count(), 3);
        assert_eq!(bar.get_item_command(0), Some(10));
        assert_eq!(bar.get_item_command(1), Some(ITEM_SEPARATOR));
        assert_eq!(bar.get_item_command(2), Some(20));
        assert_eq!(bar.get_item_command(3), None);
        assert_eq!(bar.command_to_index(20), Some(2));
        assert_eq!(bar.command_to_index(99), None);
    }

    #[test]
    fn add_items_bulk() {
        let mut bar = SideBar::new();
        bar.add_items(&[btn(1), btn(2), btn(3)]);
        assert_eq!(bar.item_count(), 3);
        bar.delete_all_items();
        assert_eq!(bar.item_count(), 0);
    }

    #[test]
    fn enable_disable_toggles_once() {
        let mut bar = SideBar::new();
        bar.add_item(btn(10));
        assert!(bar.is_item_enabled(10));
        assert!(bar.enable_item(10, false));
        assert!(!bar.is_item_enabled(10));
        // 同じ値の再設定では何も変わらない(冪等)。
        assert!(bar.enable_item(10, false));
        assert!(!bar.is_item_enabled(10));
        assert!(bar.enable_item(10, true));
        assert!(bar.is_item_enabled(10));
        // 未知コマンドは false。
        assert!(!bar.enable_item(99, true));
        assert!(!bar.is_item_enabled(99));
    }

    #[test]
    fn check_uncheck() {
        let mut bar = SideBar::new();
        bar.add_item(btn(10));
        assert!(!bar.is_item_checked(10));
        assert!(bar.check_item(10, true));
        assert!(bar.is_item_checked(10));
        assert!(bar.check_item(10, false));
        assert!(!bar.is_item_checked(10));
    }

    #[test]
    fn check_radio_item_selects_one() {
        let mut bar = SideBar::new();
        for c in 10..=13 {
            bar.add_item(btn(c));
        }
        // 全部チェックしておく。
        for c in 10..=13 {
            bar.check_item(c, true);
        }
        assert!(bar.check_radio_item(10, 13, 12));
        assert!(!bar.is_item_checked(10));
        assert!(!bar.is_item_checked(11));
        assert!(bar.is_item_checked(12));
        assert!(!bar.is_item_checked(13));
        // first > last は false。
        assert!(!bar.check_radio_item(13, 10, 11));
    }

    #[test]
    fn enable_and_check_are_independent() {
        let mut bar = SideBar::new();
        bar.add_item(btn(10));
        bar.check_item(10, true);
        bar.enable_item(10, false);
        // 両方のビットが立っていても互いに干渉しない。
        assert!(bar.is_item_checked(10));
        assert!(!bar.is_item_enabled(10));
    }

    #[test]
    fn bar_width_vertical_and_horizontal() {
        let mut bar = SideBar::new(); // icon 16x16, padding all 3, border 0
        // 縦: 16 + (3+3) + 0 + 0 = 22。
        assert_eq!(bar.bar_width(), 22);
        bar.set_border(BorderWidths {
            left: 2,
            top: 1,
            right: 2,
            bottom: 1,
        });
        // 縦: 16 + 6 + 2 + 2 = 26。
        assert_eq!(bar.bar_width(), 26);
        bar.set_vertical(false);
        // 横: 16 + (3+3) + 1 + 1 = 24。
        assert_eq!(bar.bar_width(), 24);
    }

    #[test]
    fn item_rect_vertical_stacking() {
        let mut bar = SideBar::new(); // 縦, item_height = 16+6 = 22, sep = 8
        bar.add_item(btn(10));
        bar.add_separator();
        bar.add_item(btn(20));
        // 項目0: top 0..22。
        assert_eq!(
            bar.get_item_rect(0).unwrap(),
            Rect { left: 0, top: 0, right: 22, bottom: 22 }
        );
        // 区切り1: top 22..30(高さ 8)。
        assert_eq!(
            bar.get_item_rect(1).unwrap(),
            Rect { left: 0, top: 22, right: 22, bottom: 30 }
        );
        // 項目2: top 30..52。
        assert_eq!(
            bar.get_item_rect(2).unwrap(),
            Rect { left: 0, top: 30, right: 22, bottom: 52 }
        );
        assert!(bar.get_item_rect(3).is_none());
    }

    #[test]
    fn item_rect_horizontal() {
        let mut bar = SideBar::new();
        bar.set_vertical(false); // item_width = 22
        bar.add_item(btn(10));
        bar.add_item(btn(20));
        assert_eq!(
            bar.get_item_rect(1).unwrap(),
            Rect { left: 22, top: 0, right: 44, bottom: 22 }
        );
    }

    #[test]
    fn hit_test_vertical() {
        let mut bar = SideBar::new();
        bar.add_item(btn(10)); // 0..22
        bar.add_separator(); // 22..30
        bar.add_item(btn(20)); // 30..52
        assert_eq!(bar.hit_test(5, 10), Some(0));
        assert_eq!(bar.hit_test(5, 25), Some(1)); // 区切りもヒット
        assert_eq!(bar.hit_test(5, 40), Some(2));
        // 右端(x=22)は排他のため範囲外。
        assert_eq!(bar.hit_test(22, 10), None);
        // 下端より下も範囲外。
        assert_eq!(bar.hit_test(5, 100), None);
    }
}
