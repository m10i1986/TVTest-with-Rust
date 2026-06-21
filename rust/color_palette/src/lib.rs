//! TVTest の `CColorPalette`(src/ColorPalette.cpp / ColorPalette.h)のモデル層移植。
//!
//! 16 列固定のグリッドに並ぶ最大 256 色のカラーパレットを表現する。元実装は
//! `CCustomWindow` を継承したウィンドウだが、ここでは Win32 に依存しない純粋な
//! ロジック(パレットの保持・選択/ホット項目の管理・色検索・グリッド幾何・
//! ヒットテスト)だけを移植する。
//!
//! 描画(`WM_PAINT`/`DrawSelRect`)、ツールチップ(`CTooltip`)、ウィンドウクラス
//! 登録(`Initialize`/`WndProc`)、通知送出(`SendNotify`)は対象外。

/// 無効な色を表す値(Win32 の `CLR_INVALID`)。
pub const CLR_INVALID: ColorRef = 0xFFFF_FFFF;

/// Win32 の `COLORREF`(0x00BBGGRR)に相当する色値。
pub type ColorRef = u32;

/// `RGB(r, g, b)` マクロ相当。`COLORREF`(0x00BBGGRR)を生成する。
pub const fn rgb(r: u8, g: u8, b: u8) -> ColorRef {
    (r as ColorRef) | ((g as ColorRef) << 8) | ((b as ColorRef) << 16)
}

/// `GetRValue` 相当。
pub const fn get_r_value(color: ColorRef) -> u8 {
    (color & 0xFF) as u8
}

/// `GetGValue` 相当。
pub const fn get_g_value(color: ColorRef) -> u8 {
    ((color >> 8) & 0xFF) as u8
}

/// `GetBValue` 相当。
pub const fn get_b_value(color: ColorRef) -> u8 {
    ((color >> 16) & 0xFF) as u8
}

/// Win32 の `RGBQUAD`(フィールド順 blue, green, red, reserved)に相当する 1 色分の格納形式。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RgbQuad {
    pub blue: u8,
    pub green: u8,
    pub red: u8,
    pub reserved: u8,
}

impl RgbQuad {
    /// `COLORREF` から `RGBQUAD` を生成する(reserved は 0)。
    pub const fn from_color(color: ColorRef) -> Self {
        Self {
            blue: get_b_value(color),
            green: get_g_value(color),
            red: get_r_value(color),
            reserved: 0,
        }
    }

    /// `RGB(rgbRed, rgbGreen, rgbBlue)` 相当の `COLORREF` を返す。
    pub const fn to_color(self) -> ColorRef {
        rgb(self.red, self.green, self.blue)
    }
}

/// Win32 の `RECT` に相当する矩形。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// パレットは横 16 列固定。
const COLUMNS: i32 = 16;

/// 1 項目の最小サイズ(ColorPalette.cpp WM_SIZE / ColorPalette.h の既定値 6)。
const MIN_ITEM_SIZE: i32 = 6;

/// `CColorPalette` のモデル層。
///
/// 描画・通知を伴わない状態と幾何計算のみを担う。元実装で `InvalidateRect` や
/// `SetToolTip`、`SendNotify` を呼んでいた箇所は副作用を持たないようにしてある。
#[derive(Debug, Clone)]
pub struct ColorPalette {
    // ColorPalette.h 36-45 のメンバに対応。
    palette: Vec<RgbQuad>,
    sel_color: i32,
    hot_color: i32,
    left: i32,
    top: i32,
    item_width: i32,
    item_height: i32,
    back_color: ColorRef,
}

impl Default for ColorPalette {
    fn default() -> Self {
        Self::new()
    }
}

impl ColorPalette {
    /// 空のパレットを生成する(ColorPalette.h 36-45 の既定値)。
    pub fn new() -> Self {
        Self {
            palette: Vec::new(),
            sel_color: -1,
            hot_color: -1,
            left: 0,
            top: 0,
            item_width: MIN_ITEM_SIZE,
            item_height: MIN_ITEM_SIZE,
            back_color: CLR_INVALID,
        }
    }

    /// 色数を返す(元実装の `m_NumColors`)。
    pub fn num_colors(&self) -> i32 {
        self.palette.len() as i32
    }

    /// 現在のパレットを取得する。
    ///
    /// 元実装 `GetPalette`(ColorPalette.cpp 80-86)はバッファへコピーし、未設定なら
    /// false を返す。ここではパレット未設定時は `None` を返す。
    pub fn get_palette(&self) -> Option<&[RgbQuad]> {
        if self.palette.is_empty() {
            None
        } else {
            Some(&self.palette)
        }
    }

    /// パレットを設定する(ColorPalette.cpp 89-103 `SetPalette`)。
    ///
    /// 色数が 1〜256 の範囲外なら false を返し、状態は変更しない。設定に成功すると
    /// 選択・ホットを解除する。元実装の `InvalidateRect`/`SetToolTip` は対象外。
    pub fn set_palette(&mut self, palette: &[RgbQuad]) -> bool {
        let num_colors = palette.len();
        if !(1..=256).contains(&num_colors) {
            return false;
        }
        self.palette = palette.to_vec();
        self.sel_color = -1;
        self.hot_color = -1;
        true
    }

    /// 指定インデックスの色を `COLORREF` で取得する(ColorPalette.cpp 106-111)。
    ///
    /// 範囲外なら `CLR_INVALID` を返す。
    pub fn get_color(&self, index: i32) -> ColorRef {
        if index < 0 || index >= self.num_colors() {
            return CLR_INVALID;
        }
        self.palette[index as usize].to_color()
    }

    /// 指定インデックスの色を設定する(ColorPalette.cpp 114-128 `SetColor`)。
    ///
    /// 範囲外なら false。元実装の `InvalidateRect` は対象外。
    pub fn set_color(&mut self, index: i32, color: ColorRef) -> bool {
        if index < 0 || index >= self.num_colors() {
            return false;
        }
        let item = &mut self.palette[index as usize];
        item.blue = get_b_value(color);
        item.green = get_g_value(color);
        item.red = get_r_value(color);
        true
    }

    /// 選択中インデックスを返す(ColorPalette.cpp 131-134 `GetSel`)。-1 は未選択。
    pub fn get_sel(&self) -> i32 {
        self.sel_color
    }

    /// 選択中インデックスを設定する(ColorPalette.cpp 137-148 `SetSel`)。
    ///
    /// パレット未設定なら false。範囲外の値は -1(未選択)に丸める。元実装の
    /// `DrawNewSelHighlight`(再描画)は対象外。
    pub fn set_sel(&mut self, sel: i32) -> bool {
        if self.palette.is_empty() {
            return false;
        }
        let sel = if sel < 0 || sel >= self.num_colors() {
            -1
        } else {
            sel
        };
        if sel != self.sel_color {
            self.sel_color = sel;
        }
        true
    }

    /// ホット(マウス直下)インデックスを返す(ColorPalette.cpp 151-154 `GetHot`)。
    pub fn get_hot(&self) -> i32 {
        self.hot_color
    }

    /// 指定色に一致する最初のインデックスを返す(ColorPalette.cpp 157-164 `FindColor`)。
    /// 見つからなければ -1。
    pub fn find_color(&self, color: ColorRef) -> i32 {
        for i in 0..self.num_colors() {
            if self.palette[i as usize].to_color() == color {
                return i;
            }
        }
        -1
    }

    /// 背景色を返す(ColorPalette.h 184 相当)。
    pub fn back_color(&self) -> ColorRef {
        self.back_color
    }

    /// 背景色を設定する(ColorPalette.cpp 173-180 `SetBackColor`)。
    ///
    /// 値が変化した場合に true(元実装ではこのとき `InvalidateRect` を呼ぶ)を返す。
    pub fn set_back_color(&mut self, color: ColorRef) -> bool {
        if self.back_color != color {
            self.back_color = color;
            true
        } else {
            false
        }
    }

    /// クライアント領域サイズに応じてグリッド配置を更新する(ColorPalette.cpp 257-268 `WM_SIZE`)。
    ///
    /// 1 項目サイズはクライアントサイズ/16 と最小 6 の大きい方、左上はグリッド全体を
    /// 中央寄せした位置になる。元実装の `SetToolTip` は対象外。
    pub fn set_layout(&mut self, client_width: i32, client_height: i32) {
        self.item_width = std::cmp::max(client_width / COLUMNS, MIN_ITEM_SIZE);
        self.item_height = std::cmp::max(client_height / COLUMNS, MIN_ITEM_SIZE);
        self.left = (client_width - self.item_width * COLUMNS) / 2;
        self.top = (client_height - self.item_height * COLUMNS) / 2;
    }

    /// 現在の 1 項目幅。
    pub fn item_width(&self) -> i32 {
        self.item_width
    }

    /// 現在の 1 項目高さ。
    pub fn item_height(&self) -> i32 {
        self.item_height
    }

    /// 指定インデックスの項目矩形を返す(ColorPalette.cpp 183-191 `GetItemRect`)。
    pub fn item_rect(&self, index: i32) -> Rect {
        let x = self.left + index % COLUMNS * self.item_width;
        let y = self.top + index / COLUMNS * self.item_height;
        Rect {
            left: x,
            top: y,
            right: x + self.item_width,
            bottom: y + self.item_height,
        }
    }

    /// 座標 (x, y) に対応する項目インデックスを返す。グリッド外/色数外なら -1。
    ///
    /// 元実装の `WM_MOUSEMOVE`(ColorPalette.cpp 305-322)/`WM_LBUTTONDOWN`(324-343)
    /// 内のヒットテスト計算をそのまま移植したもの。
    pub fn hit_test(&self, x: i32, y: i32) -> i32 {
        let index =
            (y - self.top) / self.item_height * COLUMNS + (x - self.left) / self.item_width;
        if x < self.left
            || x >= self.left + self.item_width * COLUMNS
            || y < self.top
            || y >= self.top + self.item_height * COLUMNS
            || index >= self.num_colors()
        {
            return -1;
        }
        index
    }

    /// マウス移動に伴うホット項目の更新(ColorPalette.cpp 305-322 `WM_MOUSEMOVE`)。
    ///
    /// ホットが変化した場合に true(元実装ではこのとき `NOTIFY_HOTCHANGE` を送出)を返す。
    pub fn on_mouse_move(&mut self, x: i32, y: i32) -> bool {
        if self.palette.is_empty() {
            return false;
        }
        let hot = self.hit_test(x, y);
        if hot == self.hot_color {
            return false;
        }
        self.hot_color = hot;
        true
    }

    /// 左/右ボタン押下に伴う選択更新(ColorPalette.cpp 324-343 `WM_LBUTTONDOWN`/`WM_RBUTTONDOWN`)。
    ///
    /// グリッド外・色数外、または既に選択中と同じ項目なら false(選択変化なし)。
    /// 選択が変化した場合に true(元実装ではこのとき `NOTIFY_SELCHANGE` を送出)を返す。
    pub fn on_button_down(&mut self, x: i32, y: i32) -> bool {
        if self.palette.is_empty() {
            return false;
        }
        let sel = self.hit_test(x, y);
        if sel < 0 || sel == self.sel_color {
            return false;
        }
        self.sel_color = sel;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_palette(n: usize) -> Vec<RgbQuad> {
        (0..n)
            .map(|i| RgbQuad::from_color(rgb(i as u8, (i * 2) as u8, (i * 3) as u8)))
            .collect()
    }

    #[test]
    fn color_macros_roundtrip() {
        let c = rgb(0x12, 0x34, 0x56);
        assert_eq!(c, 0x0056_3412);
        assert_eq!(get_r_value(c), 0x12);
        assert_eq!(get_g_value(c), 0x34);
        assert_eq!(get_b_value(c), 0x56);
        assert_eq!(RgbQuad::from_color(c).to_color(), c);
    }

    #[test]
    fn new_defaults() {
        let p = ColorPalette::new();
        assert_eq!(p.num_colors(), 0);
        assert_eq!(p.get_sel(), -1);
        assert_eq!(p.get_hot(), -1);
        assert_eq!(p.item_width(), 6);
        assert_eq!(p.item_height(), 6);
        assert_eq!(p.back_color(), CLR_INVALID);
        assert!(p.get_palette().is_none());
    }

    #[test]
    fn set_palette_validates_range() {
        let mut p = ColorPalette::new();
        assert!(!p.set_palette(&[]));
        assert!(!p.set_palette(&make_palette(257)));
        assert!(p.set_palette(&make_palette(1)));
        assert_eq!(p.num_colors(), 1);
        assert!(p.set_palette(&make_palette(256)));
        assert_eq!(p.num_colors(), 256);
    }

    #[test]
    fn set_palette_resets_sel_and_hot() {
        let mut p = ColorPalette::new();
        p.set_palette(&make_palette(16));
        p.set_sel(3);
        p.on_mouse_move(0, 0); // ホットを動かす
        assert!(p.set_palette(&make_palette(16)));
        assert_eq!(p.get_sel(), -1);
        assert_eq!(p.get_hot(), -1);
    }

    #[test]
    fn get_set_color() {
        let mut p = ColorPalette::new();
        p.set_palette(&make_palette(16));
        assert_eq!(p.get_color(-1), CLR_INVALID);
        assert_eq!(p.get_color(16), CLR_INVALID);
        assert!(p.set_color(5, rgb(10, 20, 30)));
        assert_eq!(p.get_color(5), rgb(10, 20, 30));
        assert!(!p.set_color(16, rgb(0, 0, 0)));
    }

    #[test]
    fn find_color() {
        let mut p = ColorPalette::new();
        p.set_palette(&make_palette(16));
        p.set_color(7, rgb(99, 88, 77));
        assert_eq!(p.find_color(rgb(99, 88, 77)), 7);
        assert_eq!(p.find_color(rgb(1, 1, 1)), -1);
    }

    #[test]
    fn set_sel_clamps_and_requires_palette() {
        let mut p = ColorPalette::new();
        assert!(!p.set_sel(0)); // パレット未設定
        p.set_palette(&make_palette(16));
        assert!(p.set_sel(5));
        assert_eq!(p.get_sel(), 5);
        assert!(p.set_sel(99)); // 範囲外 -> -1
        assert_eq!(p.get_sel(), -1);
    }

    #[test]
    fn set_back_color_reports_change() {
        let mut p = ColorPalette::new();
        assert!(p.set_back_color(rgb(0, 0, 0)));
        assert_eq!(p.back_color(), rgb(0, 0, 0));
        assert!(!p.set_back_color(rgb(0, 0, 0))); // 同値なら変化なし
    }

    #[test]
    fn layout_geometry() {
        let mut p = ColorPalette::new();
        // 160x160 -> 項目 10x10、中央寄せで左上 (0,0)
        p.set_layout(160, 160);
        assert_eq!(p.item_width(), 10);
        assert_eq!(p.item_height(), 10);
        let r0 = p.item_rect(0);
        assert_eq!(r0, Rect { left: 0, top: 0, right: 10, bottom: 10 });
        // index 17 -> 行1 列1
        let r17 = p.item_rect(17);
        assert_eq!(r17, Rect { left: 10, top: 10, right: 20, bottom: 20 });
    }

    #[test]
    fn layout_centers_and_clamps_min_size() {
        let mut p = ColorPalette::new();
        // 170 幅 -> 項目幅 10、グリッド幅 160、左 (170-160)/2 = 5
        p.set_layout(170, 170);
        assert_eq!(p.item_width(), 10);
        assert_eq!(p.item_rect(0).left, 5);
        // 80 幅 -> 80/16=5 だが最小 6 にクランプ
        p.set_layout(80, 80);
        assert_eq!(p.item_width(), 6);
        assert_eq!(p.item_height(), 6);
    }

    #[test]
    fn hit_test_within_and_outside() {
        let mut p = ColorPalette::new();
        p.set_palette(&make_palette(256));
        p.set_layout(160, 160); // 10x10、左上 (0,0)
        assert_eq!(p.hit_test(5, 5), 0);
        assert_eq!(p.hit_test(15, 5), 1);
        assert_eq!(p.hit_test(5, 15), 16);
        assert_eq!(p.hit_test(159, 159), 255);
        assert_eq!(p.hit_test(160, 5), -1); // 右端外
        assert_eq!(p.hit_test(-1, 5), -1); // 左端外
        assert_eq!(p.hit_test(5, 160), -1); // 下端外
    }

    #[test]
    fn hit_test_respects_num_colors() {
        let mut p = ColorPalette::new();
        p.set_palette(&make_palette(16)); // 1 行分しか色がない
        p.set_layout(160, 160);
        assert_eq!(p.hit_test(5, 5), 0);
        assert_eq!(p.hit_test(5, 15), -1); // index 16 >= 色数 16
    }

    #[test]
    fn mouse_move_tracks_hot() {
        let mut p = ColorPalette::new();
        p.set_palette(&make_palette(256));
        p.set_layout(160, 160);
        assert!(p.on_mouse_move(5, 5)); // -1 -> 0
        assert_eq!(p.get_hot(), 0);
        assert!(!p.on_mouse_move(6, 6)); // 0 のまま変化なし
        assert!(p.on_mouse_move(200, 200)); // グリッド外 -> -1
        assert_eq!(p.get_hot(), -1);
    }

    #[test]
    fn button_down_changes_selection() {
        let mut p = ColorPalette::new();
        p.set_palette(&make_palette(256));
        p.set_layout(160, 160);
        assert!(p.on_button_down(5, 5)); // -1 -> 0
        assert_eq!(p.get_sel(), 0);
        assert!(!p.on_button_down(7, 7)); // 同じ項目 0 -> 変化なし
        assert!(p.on_button_down(15, 5)); // -> 1
        assert_eq!(p.get_sel(), 1);
        assert!(!p.on_button_down(200, 200)); // グリッド外 -> 変化なし
        assert_eq!(p.get_sel(), 1);
    }
}
