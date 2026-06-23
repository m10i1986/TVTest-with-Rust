//! TVTest のタイトルバー(src/TitleBar.cpp / TitleBar.h)の固定レイアウト幾何の移植。
//!
//! タイトルバーは「左側のラベル領域」と「右端に並ぶ 4 つのウィンドウボタン
//! (最小化 / 最大化 / 全画面 / 閉じる)」からなる。その矩形計算をウィンドウ/描画から
//! 切り離した純粋なモデルとして表現する:
//! - [`TitleBarStyle`](余白・アイコン/ボタンの寸法)
//! - [`TitleBar`](高さ算出・項目矩形・ヒットテスト・アイコン領域判定)
//!
//! # 対象外(Win32 / 描画依存)
//! 描画(`Draw`)、テーマ(`TitleBarTheme`)、フォント計測(`CalcFontHeight`)、
//! ツールチップ、ウィンドウ管理(`CCustomWindow`)、マウス処理、スタイルの DPI スケーリング。
//!
//! クライアント矩形・テーマ枠幅([`BorderWidths`])・フォント高さは、原実装が
//! `GetClientRect` / テーマ / フォントから得る値をデータとして注入する。

/// ラベル項目(TitleBar.h 104-112 の enum)。
pub const ITEM_LABEL: i32 = 0;
/// 最小化ボタン。
pub const ITEM_MINIMIZE: i32 = 1;
/// 最大化ボタン。
pub const ITEM_MAXIMIZE: i32 = 2;
/// 全画面ボタン。
pub const ITEM_FULLSCREEN: i32 = 3;
/// 閉じるボタン。
pub const ITEM_CLOSE: i32 = 4;
/// 最初のボタン項目(= 最小化)。
pub const ITEM_BUTTON_FIRST: i32 = ITEM_MINIMIZE;
/// 最後の項目(= 閉じる)。
pub const ITEM_LAST: i32 = ITEM_CLOSE;
/// ウィンドウボタンの数(TitleBar.cpp 36 `NUM_BUTTONS`)。
pub const NUM_BUTTONS: i32 = 4;

/// 上下左右の余白(`Style::Margins` 相当)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Margins {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Margins {
    /// 各辺を指定して生成する。
    pub const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    /// 全辺同じ値で生成する。
    pub const fn all(value: i32) -> Self {
        Self::new(value, value, value, value)
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

/// 寸法(`Style::Size` 相当)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size {
    pub width: i32,
    pub height: i32,
}

impl Size {
    /// 幅と高さから生成する。
    pub const fn new(width: i32, height: i32) -> Self {
        Self { width, height }
    }
}

/// テーマ枠の各辺の幅。
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

/// タイトルバーの寸法スタイル(TitleBar.h 114-128 `TitleBarStyle`)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TitleBarStyle {
    pub padding: Margins,
    pub label_margin: Margins,
    pub icon_size: Size,
    pub icon_margin: Margins,
    pub button_icon_size: Size,
    pub button_padding: Margins,
}

impl Default for TitleBarStyle {
    fn default() -> Self {
        // TitleBar.h 116-122 の既定値。
        Self {
            padding: Margins::all(0),
            label_margin: Margins::new(4, 2, 4, 2),
            icon_size: Size::new(16, 16),
            icon_margin: Margins::new(4, 0, 0, 0),
            button_icon_size: Size::new(12, 12),
            button_padding: Margins::all(4),
        }
    }
}

/// タイトルバーのレイアウト(TitleBar.h 36-166 `CTitleBar` の幾何部)。
#[derive(Clone, Debug, Default)]
pub struct TitleBar {
    style: TitleBarStyle,
    border: BorderWidths,
    client_rect: Rect,
    font_height: i32,
}

impl TitleBar {
    /// タイトルバーを生成する。
    pub fn new() -> Self {
        Self::default()
    }

    /// 寸法スタイルを設定する。
    pub fn set_style(&mut self, style: TitleBarStyle) {
        self.style = style;
    }

    /// テーマ枠の幅を設定する。
    pub fn set_border(&mut self, border: BorderWidths) {
        self.border = border;
    }

    /// クライアント矩形を設定する(原実装の `GetClientRect` 相当。通常 left=top=0)。
    pub fn set_client_rect(&mut self, rect: Rect) {
        self.client_rect = rect;
    }

    /// フォント高さを設定する(原実装の `CalcFontHeight` の結果)。
    pub fn set_font_height(&mut self, font_height: i32) {
        self.font_height = font_height;
    }

    /// ボタン 1 つの幅を返す(TitleBar.cpp 155-158 `GetButtonWidth`)。
    pub fn button_width(&self) -> i32 {
        self.style.button_icon_size.width + self.style.button_padding.horz()
    }

    /// ボタン 1 つの高さを返す(TitleBar.cpp 161-164 `GetButtonHeight`)。
    pub fn button_height(&self) -> i32 {
        self.style.button_icon_size.height + self.style.button_padding.vert()
    }

    /// タイトルバーの高さを算出する(TitleBar.cpp 142-152 `CalcHeight`)。
    pub fn calc_height(&self) -> i32 {
        let label_height = self.font_height + self.style.label_margin.vert();
        let icon_height = self.style.icon_size.height + self.style.icon_margin.vert();
        let button_height = self.button_height();
        let height = label_height.max(icon_height).max(button_height);
        height + self.style.padding.vert() + self.border.top + self.border.bottom
    }

    /// 項目の矩形を返す(TitleBar.cpp 550-579 `GetItemRect`)。
    ///
    /// `item` は [`ITEM_LABEL`]〜[`ITEM_LAST`]。範囲外は `None`。
    pub fn get_item_rect(&self, item: i32) -> Option<Rect> {
        if !(0..=ITEM_LAST).contains(&item) {
            return None;
        }

        // クライアント矩形から枠とパディングを差し引く。
        let mut rc = self.client_rect;
        rc.left += self.border.left + self.style.padding.left;
        rc.top += self.border.top + self.style.padding.top;
        rc.right -= self.border.right + self.style.padding.right;
        rc.bottom -= self.border.bottom + self.style.padding.bottom;

        let button_width = self.button_width();
        let mut button_pos = rc.right - NUM_BUTTONS * button_width;
        if button_pos < 0 {
            button_pos = 0;
        }

        if item == ITEM_LABEL {
            rc.right = button_pos;
            if rc.right < rc.left {
                rc.right = rc.left;
            }
        } else {
            let button_height = self.button_height();
            rc.left = button_pos + (item - 1) * button_width;
            rc.right = rc.left + button_width;
            rc.top += ((rc.bottom - rc.top) - button_height) / 2;
            rc.bottom = rc.top + button_height;
        }
        Some(rc)
    }

    /// 座標から項目を求める(TitleBar.cpp 595-607 `HitTest`)。
    ///
    /// 後ろの項目(ボタン)を優先して判定し、見つからなければ -1。
    /// 判定は Win32 `PtInRect` と同じく左上を含み右下を含まない。
    pub fn hit_test(&self, x: i32, y: i32) -> i32 {
        for item in (0..=ITEM_LAST).rev() {
            if let Some(rc) = self.get_item_rect(item) {
                if x >= rc.left && x < rc.right && y >= rc.top && y < rc.bottom {
                    return item;
                }
            }
        }
        -1
    }

    /// 座標がアイコン領域内か(TitleBar.cpp 610-618 `PtInIcon`)。
    ///
    /// 原実装は X 座標のみで判定する(Y は見ない)。
    pub fn pt_in_icon(&self, x: i32) -> bool {
        let icon_left = self.border.left + self.style.padding.left + self.style.icon_margin.left;
        x >= icon_left && x < icon_left + self.style.icon_size.width
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 幅 400・高さ 30、枠/パディング 0 の既定タイトルバー。
    fn make() -> TitleBar {
        let mut t = TitleBar::new();
        t.set_client_rect(Rect {
            left: 0,
            top: 0,
            right: 400,
            bottom: 30,
        });
        t
    }

    #[test]
    fn button_dimensions() {
        let t = make();
        // 12 + (4+4) = 20。
        assert_eq!(t.button_width(), 20);
        assert_eq!(t.button_height(), 20);
    }

    #[test]
    fn calc_height_takes_max_plus_border() {
        let mut t = make();
        t.set_font_height(14);
        t.set_border(BorderWidths {
            left: 0,
            top: 1,
            right: 0,
            bottom: 1,
        });
        // label=14+4=18, icon=16, button=20 → max 20。+padding.vert(0)+border(1+1)=22。
        assert_eq!(t.calc_height(), 22);
    }

    #[test]
    fn label_rect_fills_left_of_buttons() {
        let t = make();
        // ButtonPos = 400 - 4*20 = 320。ラベルは 0..320。
        assert_eq!(
            t.get_item_rect(ITEM_LABEL).unwrap(),
            Rect { left: 0, top: 0, right: 320, bottom: 30 }
        );
    }

    #[test]
    fn button_rects_are_right_aligned_and_centered() {
        let t = make();
        // ボタンは縦中央(top=(30-20)/2=5, bottom=25)、幅 20 ずつ右へ。
        assert_eq!(
            t.get_item_rect(ITEM_MINIMIZE).unwrap(),
            Rect { left: 320, top: 5, right: 340, bottom: 25 }
        );
        assert_eq!(
            t.get_item_rect(ITEM_MAXIMIZE).unwrap(),
            Rect { left: 340, top: 5, right: 360, bottom: 25 }
        );
        assert_eq!(
            t.get_item_rect(ITEM_FULLSCREEN).unwrap(),
            Rect { left: 360, top: 5, right: 380, bottom: 25 }
        );
        assert_eq!(
            t.get_item_rect(ITEM_CLOSE).unwrap(),
            Rect { left: 380, top: 5, right: 400, bottom: 25 }
        );
    }

    #[test]
    fn item_rect_out_of_range() {
        let t = make();
        assert!(t.get_item_rect(-1).is_none());
        assert!(t.get_item_rect(ITEM_LAST + 1).is_none());
    }

    #[test]
    fn button_pos_clamped_when_too_narrow() {
        let mut t = TitleBar::new();
        // 幅 50 < 4*20=80 → ButtonPos が負になり 0 にクランプ。
        t.set_client_rect(Rect { left: 0, top: 0, right: 50, bottom: 30 });
        // ラベルは右端が ButtonPos(0)、左(0)未満なので left に丸め → 幅 0。
        assert_eq!(
            t.get_item_rect(ITEM_LABEL).unwrap(),
            Rect { left: 0, top: 0, right: 0, bottom: 30 }
        );
        // 最小化ボタンは 0..20。
        assert_eq!(
            t.get_item_rect(ITEM_MINIMIZE).unwrap(),
            Rect { left: 0, top: 5, right: 20, bottom: 25 }
        );
    }

    #[test]
    fn hit_test_prefers_buttons() {
        let t = make();
        // 閉じるボタン領域。
        assert_eq!(t.hit_test(390, 10), ITEM_CLOSE);
        // ラベル領域。
        assert_eq!(t.hit_test(100, 10), ITEM_LABEL);
        // ボタンの縦範囲外かつラベル右外 → どこにも当たらず -1。
        assert_eq!(t.hit_test(350, 28), -1);
    }

    #[test]
    fn pt_in_icon_checks_x_only() {
        let t = make();
        // IconLeft = 0 + 0 + 4 = 4、幅 16 → [4, 20)。
        assert!(t.pt_in_icon(10));
        assert!(!t.pt_in_icon(2));
        assert!(!t.pt_in_icon(20));
        // Y は無関係(X だけで判定)。
        assert!(t.pt_in_icon(4));
    }
}
