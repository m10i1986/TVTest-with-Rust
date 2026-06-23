//! TVTest のレイアウト(src/Layout.cpp / Layout.h)のスプリッタ幾何ロジック移植。
//!
//! `CSplitter`(2 ペインのスプリッタ)の幾何計算を、ウィンドウツリーから切り離した
//! 純粋なモデルとして表現する:
//! - [`Rect`] / [`Size`](Win32 RECT/SIZE 相当)
//! - [`StyleFlag`](水平/垂直・固定)
//! - [`PaneContainer`](ペインの子が公開する ID / 可視状態 / 最小サイズ)
//! - [`Splitter`](バー位置クランプ・ペイン矩形計算・最小サイズ集計・スワップ等)
//!
//! 原実装の各幾何メソッドは子コンテナへ `pContainer->GetMinSize()` /
//! `GetVisible()` / `GetID()` を問い合わせる。本移植ではそれらをペインの
//! データ([`PaneContainer`])として保持し、純粋に計算する。`Adjust` は子へ位置を
//! 設定する副作用の代わりに、算出したレイアウト結果([`LayoutAdjust`])を返す。
//!
//! # 対象外(Win32 / ウィンドウツリー依存)
//! ウィンドウ管理(`CContainer`/`CWindowContainer`/`CLayoutBase`)、子ウィンドウへの
//! 位置反映、マウスキャプチャ・カーソル設定(`SetCapture`/`SetCursor`/`PtInRect`)、
//! DPI スケーリング(`ApplyStyle`)、描画。

use bitflags::bitflags;

/// 矩形(Win32 `RECT` 相当)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    /// 4 辺から矩形を生成する。
    pub const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    /// 幅(`right - left`)。
    pub const fn width(&self) -> i32 {
        self.right - self.left
    }

    /// 高さ(`bottom - top`)。
    pub const fn height(&self) -> i32 {
        self.bottom - self.top
    }
}

/// サイズ(Win32 `SIZE` 相当)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Size {
    pub cx: i32,
    pub cy: i32,
}

impl Size {
    /// 幅と高さからサイズを生成する。
    pub const fn new(cx: i32, cy: i32) -> Self {
        Self { cx, cy }
    }
}

bitflags! {
    /// スプリッタの様式(Layout.h 95-101 `CSplitter::StyleFlag`)。
    ///
    /// `Horz`/`None` は値 0(空)。`Vert` で垂直分割、`Fixed` で固定分割。
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
    pub struct StyleFlag: u32 {
        /// 垂直分割(上下に並べる)。
        const VERT = 0x0001;
        /// 固定分割(分割バーを表示せずペイン 2 を固定サイズにする)。
        const FIXED = 0x0002;
    }
}

/// ペインの子コンテナが公開する情報(`CContainer` の問い合わせ結果)。
///
/// 原実装の `pContainer->GetID()` / `GetVisible()` / `GetMinSize()` に対応する。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaneContainer {
    /// コンテナ ID(`GetID`)。
    pub id: i32,
    /// 可視状態(`GetVisible`)。
    pub visible: bool,
    /// 最小サイズ(`GetMinSize`)。
    pub min_size: Size,
}

impl PaneContainer {
    /// コンテナ情報を生成する。
    pub const fn new(id: i32, visible: bool, min_size: Size) -> Self {
        Self {
            id,
            visible,
            min_size,
        }
    }
}

/// スプリッタの 1 ペイン(Layout.h 104-108 `CSplitter::PaneInfo`)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PaneInfo {
    container: Option<PaneContainer>,
    fixed_size: i32,
}

impl Default for PaneInfo {
    fn default() -> Self {
        // FixedSize の既定値は -1(Layout.h 107)。
        Self {
            container: None,
            fixed_size: -1,
        }
    }
}

/// `Adjust` が算出するレイアウト結果(子へ設定すべき位置)。
///
/// 原実装の `Adjust`(Layout.cpp 451-488)が子コンテナへ `SetPosition` する代わりに、
/// どのペインをどの矩形に配置すべきかを返す。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutAdjust {
    /// 可視のペインが無い(配置なし)。
    Empty,
    /// ペイン 0 のみ可視。指定矩形へ配置する。
    Pane0(Rect),
    /// ペイン 1 のみ可視。指定矩形へ配置する。
    Pane1(Rect),
    /// 両ペイン可視。それぞれの矩形へ配置する。
    Both { pane0: Rect, pane1: Rect },
}

/// 2 ペインのスプリッタ(Layout.h 91-150 `CSplitter`)。
#[derive(Clone, Debug)]
pub struct Splitter {
    id: i32,
    panes: [PaneInfo; 2],
    style: StyleFlag,
    adjust_pane: i32,
    bar_pos: i32,
    bar_width: i32,
    position: Rect,
}

impl Splitter {
    /// スプリッタを生成する(Layout.cpp 139-142。バー幅の既定値は 4)。
    pub fn new(id: i32) -> Self {
        Self {
            id,
            panes: [PaneInfo::default(), PaneInfo::default()],
            style: StyleFlag::empty(),
            adjust_pane: 0,
            bar_pos: 0,
            bar_width: 4,
            position: Rect::default(),
        }
    }

    /// コンテナ ID を返す。
    pub fn id(&self) -> i32 {
        self.id
    }

    /// 現在の様式を返す(`GetStyle`)。
    pub fn style(&self) -> StyleFlag {
        self.style
    }

    /// バー位置を返す(`GetBarPos`)。
    pub fn bar_pos(&self) -> i32 {
        self.bar_pos
    }

    /// バー幅を返す(`GetBarWidth`)。
    pub fn bar_width(&self) -> i32 {
        self.bar_width
    }

    /// バー幅を設定する(原実装は `ApplyStyle` で DPI スケーリングして設定)。
    pub fn set_bar_width(&mut self, width: i32) {
        self.bar_width = width;
    }

    /// 現在の領域を返す(`GetPosition`)。
    pub fn position(&self) -> Rect {
        self.position
    }

    fn is_vert(&self) -> bool {
        self.style.contains(StyleFlag::VERT)
    }

    fn is_fixed(&self) -> bool {
        self.style.contains(StyleFlag::FIXED)
    }

    /// 指定ペインの子コンテナを設定する(`ReplacePane`/`SetPane` のモデル相当)。
    ///
    /// ウィンドウの登録/解除は対象外で、スロットのコンテナ情報のみ差し替える。
    /// 範囲外の `index` は `false`。
    pub fn set_pane(&mut self, index: usize, container: Option<PaneContainer>) -> bool {
        if index > 1 {
            return false;
        }
        self.panes[index].container = container;
        true
    }

    /// 指定ペインの子コンテナを返す(`GetPane`)。
    pub fn get_pane(&self, index: usize) -> Option<&PaneContainer> {
        if index > 1 {
            return None;
        }
        self.panes[index].container.as_ref()
    }

    /// ID から子コンテナを返す(`GetPaneByID`)。
    pub fn get_pane_by_id(&self, id: i32) -> Option<&PaneContainer> {
        let index = self.id_to_index(id)?;
        self.panes[index].container.as_ref()
    }

    /// ID からペイン索引を返す(Layout.cpp 405-412 `IDToIndex`。無ければ `None`)。
    pub fn id_to_index(&self, id: i32) -> Option<usize> {
        (0..2).find(|&i| self.panes[i].container.is_some_and(|c| c.id == id))
    }

    /// 子コンテナ数を返す(Layout.cpp 208-217 `NumChildContainers`)。
    pub fn num_child_containers(&self) -> i32 {
        self.panes.iter().filter(|p| p.container.is_some()).count() as i32
    }

    /// `Index` 番目(非 null のみ数える)の子コンテナを返す
    /// (Layout.cpp 220-232 `GetChildContainer`)。
    pub fn get_child_container(&self, index: i32) -> Option<&PaneContainer> {
        let mut j = 0;
        for pane in &self.panes {
            if let Some(c) = &pane.container {
                if index == j {
                    return Some(c);
                }
                j += 1;
            }
        }
        None
    }

    /// 調整対象ペインの ID を設定する(`SetAdjustPane`)。
    pub fn set_adjust_pane(&mut self, id: i32) {
        self.adjust_pane = id;
    }

    /// バー位置を設定する(`SetBarPos`)。
    pub fn set_bar_pos(&mut self, pos: i32) {
        self.bar_pos = pos;
    }

    /// 最小サイズを集計する(Layout.cpp 176-205 `GetMinSize`)。
    pub fn get_min_size(&self) -> Size {
        let pane0 = self.visible_container(0);
        let pane1 = self.visible_container(1);

        if let Some(c0) = pane0 {
            let mut size = c0.min_size;
            if let Some(c1) = pane1 {
                let sz = c1.min_size;
                if !self.is_vert() {
                    size.cx += sz.cx;
                    if size.cy < sz.cy {
                        size.cy = sz.cy;
                    }
                    if !self.is_fixed() {
                        size.cx += self.bar_width;
                    }
                } else {
                    size.cy += sz.cy;
                    if size.cx < sz.cx {
                        size.cx = sz.cx;
                    }
                    if !self.is_fixed() {
                        size.cy += self.bar_width;
                    }
                }
            }
            size
        } else if let Some(c1) = pane1 {
            c1.min_size
        } else {
            Size::new(0, 0)
        }
    }

    /// 領域を設定する(Layout.cpp 152-173 `SetPosition`)。
    ///
    /// 調整対象がペイン 0 のとき、リサイズ差分に応じてバー位置を追従させる。
    /// 原実装はこの後 `Adjust` を呼んで子へ反映するが、本移植では位置と
    /// バー位置の更新のみ行い、配置結果は [`Splitter::adjust`] で取得する。
    pub fn set_position(&mut self, pos: Rect) {
        let pane0_visible = self.panes[0].container.is_some_and(|c| c.visible);
        let pane1_present = self.panes[1].container.is_some();
        let pane0_id = self.panes[0].container.map(|c| c.id);

        if pane0_visible && pane1_present && pane0_id == Some(self.adjust_pane) {
            if !self.is_fixed() || self.panes[1].fixed_size < 0 {
                if !self.is_vert() {
                    self.bar_pos += pos.width() - self.position.width();
                } else {
                    self.bar_pos += pos.height() - self.position.height();
                }
            } else if !self.is_vert() {
                self.bar_pos = pos.width() - self.panes[1].fixed_size;
            } else {
                self.bar_pos = pos.height() - self.panes[1].fixed_size;
            }
            if self.bar_pos < 0 {
                self.bar_pos = 0;
            }
        }
        self.position = pos;
    }

    /// 2 ペインを入れ替える(Layout.cpp 339-351 `SwapPane`)。
    pub fn swap_pane(&mut self) {
        self.panes.swap(0, 1);
        if !self.is_vert() {
            self.bar_pos = self.position.width() - (self.bar_pos + self.bar_width);
        } else {
            self.bar_pos = self.position.height() - (self.bar_pos + self.bar_width);
        }
        if self.bar_pos < 0 {
            self.bar_pos = 0;
        }
    }

    /// 指定 ID のペインサイズを設定する(Layout.cpp 354-384 `SetPaneSize`)。
    pub fn set_pane_size(&mut self, id: i32, size: i32) -> bool {
        let Some(index) = self.id_to_index(id) else {
            return false;
        };
        if !self.is_vert() {
            if index == 0 {
                self.bar_pos = size;
            } else {
                self.bar_pos = self.position.width() - size;
                if !self.is_fixed() {
                    self.bar_pos -= self.bar_width;
                }
                if self.bar_pos < 0 {
                    self.bar_pos = 0;
                }
            }
        } else if index == 0 {
            self.bar_pos = size;
        } else {
            self.bar_pos = self.position.height() - size;
            if !self.is_fixed() {
                self.bar_pos -= self.bar_width;
            }
            if self.bar_pos < 0 {
                self.bar_pos = 0;
            }
        }
        self.panes[index].fixed_size = size;
        true
    }

    /// 指定 ID のペインサイズを返す(Layout.cpp 387-402 `GetPaneSize`)。
    pub fn get_pane_size(&self, id: i32) -> i32 {
        let Some(index) = self.id_to_index(id) else {
            return 0;
        };
        if index == 0 {
            return self.bar_pos;
        }
        let mut size = if !self.is_vert() {
            self.position.width()
        } else {
            self.position.height()
        };
        size -= self.bar_pos + self.bar_width;
        size.max(0)
    }

    /// 様式を設定する(Layout.cpp 415-434 `SetStyle`)。
    ///
    /// 水平/垂直が切り替わり、かつ固定様式でペイン 1 が固定サイズを持つ場合、
    /// バー位置を新しい向きに合わせ直す。原実装の `fAdjust` による子への即時反映は
    /// 対象外(配置は [`Splitter::adjust`] で取得する)。
    pub fn set_style(&mut self, style: StyleFlag) {
        if self.style == style {
            return;
        }
        if (self.style & StyleFlag::VERT) != (style & StyleFlag::VERT)
            && style.contains(StyleFlag::FIXED)
            && self.panes[1].container.is_some()
            && self.panes[1].fixed_size >= 0
        {
            if style.contains(StyleFlag::VERT) {
                self.bar_pos = self.position.height() - self.panes[1].fixed_size;
            } else {
                self.bar_pos = self.position.width() - self.panes[1].fixed_size;
            }
        }
        self.style = style;
    }

    /// マウスドラッグでバーを移動する(Layout.cpp 259-281 の OnMouseMove キャプチャ部)。
    ///
    /// 両ペインの最小サイズで挟んでクランプし、バー位置が変化したら `true`。
    /// 両ペインが可視で存在しなければ `false`。
    pub fn drag_bar(&mut self, x: i32, y: i32) -> bool {
        let (Some(c0), Some(c1)) = (self.visible_container(0), self.visible_container(1)) else {
            return false;
        };
        let min1 = c0.min_size;
        let min2 = c1.min_size;
        let mut bar_pos;
        if !self.is_vert() {
            bar_pos = x - self.position.left;
            if self.position.width() - bar_pos - self.bar_width < min2.cx {
                bar_pos = self.position.width() - self.bar_width - min2.cx;
            }
            if bar_pos < min1.cx {
                bar_pos = min1.cx;
            }
        } else {
            bar_pos = y - self.position.top;
            if self.position.height() - bar_pos - self.bar_width < min2.cy {
                bar_pos = self.position.height() - self.bar_width - min2.cy;
            }
            if bar_pos < min1.cy {
                bar_pos = min1.cy;
            }
        }
        if self.bar_pos != bar_pos {
            self.bar_pos = bar_pos;
            true
        } else {
            false
        }
    }

    /// 分割バーの矩形を返す(Layout.cpp 491-549 `GetBarRect`)。
    ///
    /// 両ペインが可視で存在するときのみ `Some`。`m_BarPos` は変更しない(局所計算)。
    pub fn get_bar_rect(&self) -> Option<Rect> {
        let c0 = self.visible_container(0)?;
        let c1 = self.visible_container(1)?;
        let min1 = c0.min_size;
        let min2 = c1.min_size;

        let mut rc = self.position;

        if !self.is_vert() {
            let width = if !self.is_fixed() || self.panes[1].fixed_size < 0 {
                let mut w = self.position.width() - self.bar_pos;
                if !self.is_fixed() {
                    w -= self.bar_width;
                }
                if w < min2.cx {
                    w = min2.cx;
                }
                w
            } else {
                self.panes[1].fixed_size
            };
            let mut bar_pos = self.position.width() - width;
            if !self.is_fixed() {
                bar_pos -= self.bar_width;
            }
            if bar_pos < min1.cx {
                bar_pos = min1.cx;
            }
            rc.left = self.position.left + bar_pos;
            rc.right = rc.left;
            if !self.is_fixed() {
                rc.right += self.bar_width;
            }
        } else {
            let height = if !self.is_fixed() || self.panes[1].fixed_size < 0 {
                let mut h = self.position.height() - self.bar_pos;
                if !self.is_fixed() {
                    h -= self.bar_width;
                }
                if h < min2.cy {
                    h = min2.cy;
                }
                h
            } else {
                self.panes[1].fixed_size
            };
            let mut bar_pos = self.position.height() - height;
            if !self.is_fixed() {
                bar_pos -= self.bar_width;
            }
            if bar_pos < min1.cy {
                bar_pos = min1.cy;
            }
            rc.top = self.position.top + bar_pos;
            rc.bottom = rc.top;
            if !self.is_fixed() {
                rc.bottom += self.bar_width;
            }
        }
        Some(rc)
    }

    /// ペイン配置を算出する(Layout.cpp 451-488 `Adjust`)。
    ///
    /// 原実装が子へ `SetPosition` する副作用の代わりに、配置すべき矩形を返す。
    /// レイアウトロック(`m_pBase->IsLayoutLocked`)の判定は呼び出し側の責務。
    pub fn adjust(&self) -> LayoutAdjust {
        let pane0_visible = self.panes[0].container.is_some_and(|c| c.visible);
        let pane1_visible = self.panes[1].container.is_some_and(|c| c.visible);

        if !pane0_visible || !pane1_visible {
            if pane0_visible {
                return LayoutAdjust::Pane0(self.position);
            }
            if pane1_visible {
                return LayoutAdjust::Pane1(self.position);
            }
            return LayoutAdjust::Empty;
        }

        // 両ペイン可視。
        let rc_bar = self
            .get_bar_rect()
            .expect("両ペイン可視なら GetBarRect は Some");
        let min_size = self.panes[1].container.expect("可視判定済み").min_size;

        if !self.is_vert() {
            let mut rc0 = self.position;
            rc0.right = rc_bar.left;
            let mut rc1 = self.position;
            rc1.left = rc_bar.right;
            rc1.right = self.position.right;
            if rc1.width() < min_size.cx {
                rc1.right = rc1.left + min_size.cx;
            }
            LayoutAdjust::Both {
                pane0: rc0,
                pane1: rc1,
            }
        } else {
            let mut rc0 = self.position;
            rc0.bottom = rc_bar.top;
            let mut rc1 = self.position;
            rc1.top = rc_bar.bottom;
            rc1.bottom = self.position.bottom;
            if rc1.height() < min_size.cy {
                rc1.bottom = rc1.top + min_size.cy;
            }
            LayoutAdjust::Both {
                pane0: rc0,
                pane1: rc1,
            }
        }
    }

    /// 指定ペインが「存在し可視」なら子コンテナを返す内部ヘルパ。
    fn visible_container(&self, index: usize) -> Option<PaneContainer> {
        self.panes[index].container.filter(|c| c.visible)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 横並び(非固定)スプリッタを 2 ペインで構築する。
    fn make_horz(width: i32, height: i32, bar_pos: i32) -> Splitter {
        let mut s = Splitter::new(100);
        s.set_pane(0, Some(PaneContainer::new(1, true, Size::new(50, 30))));
        s.set_pane(1, Some(PaneContainer::new(2, true, Size::new(40, 20))));
        s.set_position(Rect::new(0, 0, width, height));
        s.set_bar_pos(bar_pos);
        s
    }

    #[test]
    fn min_size_horz_non_fixed() {
        let s = make_horz(400, 300, 100);
        // 横並び: cx = 50 + 40 + bar(4) = 94、cy = max(30,20) = 30。
        assert_eq!(s.get_min_size(), Size::new(94, 30));
    }

    #[test]
    fn min_size_vert_non_fixed() {
        let mut s = make_horz(400, 300, 100);
        s.set_style(StyleFlag::VERT);
        // 縦並び: cy = 30 + 20 + bar(4) = 54、cx = max(50,40) = 50。
        assert_eq!(s.get_min_size(), Size::new(50, 54));
    }

    #[test]
    fn min_size_only_pane1_visible() {
        let mut s = make_horz(400, 300, 100);
        s.set_pane(0, Some(PaneContainer::new(1, false, Size::new(50, 30))));
        assert_eq!(s.get_min_size(), Size::new(40, 20));
    }

    #[test]
    fn min_size_none_visible() {
        let mut s = make_horz(400, 300, 100);
        s.set_pane(0, None);
        s.set_pane(1, None);
        assert_eq!(s.get_min_size(), Size::new(0, 0));
    }

    #[test]
    fn bar_rect_horz_non_fixed() {
        let s = make_horz(400, 300, 100);
        // width = 400-100 = 300, !fixed -> -4 = 296, >= min2(40) のまま。
        // bar_pos = 400-296 = 104, -4 = 100, >= min1(50) のまま。
        // rc.left = 0+100 = 100, rc.right = 100+4 = 104。
        let rc = s.get_bar_rect().unwrap();
        assert_eq!(rc, Rect::new(100, 0, 104, 300));
    }

    #[test]
    fn adjust_both_horz() {
        let s = make_horz(400, 300, 100);
        match s.adjust() {
            LayoutAdjust::Both { pane0, pane1 } => {
                // バー矩形 left=100,right=104。
                assert_eq!(pane0, Rect::new(0, 0, 100, 300));
                assert_eq!(pane1, Rect::new(104, 0, 400, 300));
            }
            other => panic!("Both を期待: {other:?}"),
        }
    }

    #[test]
    fn adjust_only_pane0() {
        let mut s = make_horz(400, 300, 100);
        s.set_pane(1, Some(PaneContainer::new(2, false, Size::new(40, 20))));
        assert_eq!(s.adjust(), LayoutAdjust::Pane0(Rect::new(0, 0, 400, 300)));
    }

    #[test]
    fn adjust_empty() {
        let mut s = make_horz(400, 300, 100);
        s.set_pane(0, Some(PaneContainer::new(1, false, Size::new(50, 30))));
        s.set_pane(1, Some(PaneContainer::new(2, false, Size::new(40, 20))));
        assert_eq!(s.adjust(), LayoutAdjust::Empty);
    }

    #[test]
    fn drag_bar_clamps_to_min_sizes() {
        let mut s = make_horz(400, 300, 100);
        // 左へ寄せすぎ: x=10 → bar_pos=10 だが min1.cx=50 でクランプ。
        assert!(s.drag_bar(10, 150));
        assert_eq!(s.bar_pos(), 50);
        // 右へ寄せすぎ: x=395 → 400-bar_pos-4 < min2.cx(40) で
        // bar_pos = 400-4-40 = 356 にクランプ。
        assert!(s.drag_bar(395, 150));
        assert_eq!(s.bar_pos(), 356);
        // 同じ位置なら false。
        assert!(!s.drag_bar(395, 150));
    }

    #[test]
    fn drag_bar_requires_both_visible() {
        let mut s = make_horz(400, 300, 100);
        s.set_pane(1, Some(PaneContainer::new(2, false, Size::new(40, 20))));
        assert!(!s.drag_bar(200, 150));
    }

    #[test]
    fn swap_pane_mirrors_bar_pos() {
        let mut s = make_horz(400, 300, 100);
        s.swap_pane();
        // bar_pos = 400 - (100 + 4) = 296。
        assert_eq!(s.bar_pos(), 296);
        // ペインも入れ替わる。
        assert_eq!(s.get_pane(0).unwrap().id, 2);
        assert_eq!(s.get_pane(1).unwrap().id, 1);
    }

    #[test]
    fn set_and_get_pane_size_pane0() {
        let mut s = make_horz(400, 300, 100);
        assert!(s.set_pane_size(1, 120)); // ペイン 0(id=1)
        assert_eq!(s.bar_pos(), 120);
        assert_eq!(s.get_pane_size(1), 120);
    }

    #[test]
    fn set_and_get_pane_size_pane1() {
        let mut s = make_horz(400, 300, 100);
        assert!(s.set_pane_size(2, 120)); // ペイン 1(id=2)
        // bar_pos = 400 - 120 - 4 = 276。
        assert_eq!(s.bar_pos(), 276);
        // get_pane_size(2) = 400 - (276+4) = 120。
        assert_eq!(s.get_pane_size(2), 120);
    }

    #[test]
    fn set_pane_size_unknown_id() {
        let mut s = make_horz(400, 300, 100);
        assert!(!s.set_pane_size(999, 120));
        assert_eq!(s.get_pane_size(999), 0);
    }

    #[test]
    fn id_to_index_and_lookup() {
        let s = make_horz(400, 300, 100);
        assert_eq!(s.id_to_index(1), Some(0));
        assert_eq!(s.id_to_index(2), Some(1));
        assert_eq!(s.id_to_index(3), None);
        assert_eq!(s.get_pane_by_id(2).unwrap().min_size, Size::new(40, 20));
    }

    #[test]
    fn num_and_get_child_container() {
        let mut s = Splitter::new(0);
        assert_eq!(s.num_child_containers(), 0);
        s.set_pane(1, Some(PaneContainer::new(2, true, Size::new(40, 20))));
        assert_eq!(s.num_child_containers(), 1);
        // 非 null は index 1 のみ → 0 番目に詰めて返る。
        assert_eq!(s.get_child_container(0).unwrap().id, 2);
        assert!(s.get_child_container(1).is_none());
    }

    #[test]
    fn set_style_orientation_change_with_fixed() {
        let mut s = make_horz(400, 300, 100);
        s.set_pane_size(2, 120); // pane1 の fixed_size = 120、bar_pos=276
        // 水平→垂直 + 固定。fixed_size>=0 なのでバー位置を高さ基準に。
        s.set_style(StyleFlag::VERT | StyleFlag::FIXED);
        // bar_pos = height(300) - 120 = 180。
        assert_eq!(s.bar_pos(), 180);
    }

    #[test]
    fn set_position_tracks_adjust_pane() {
        let mut s = make_horz(400, 300, 100);
        s.set_adjust_pane(1); // ペイン 0 の ID
        // 幅を 400 → 500 に拡大。差分 +100 がバー位置に加算。
        s.set_position(Rect::new(0, 0, 500, 300));
        assert_eq!(s.bar_pos(), 200);
    }

    #[test]
    fn set_position_no_track_when_not_adjust_pane() {
        let mut s = make_horz(400, 300, 100);
        // adjust_pane の既定は 0、ペイン 0 の ID は 1 なので追従しない。
        s.set_position(Rect::new(0, 0, 500, 300));
        assert_eq!(s.bar_pos(), 100);
    }
}
