#![forbid(unsafe_code)]
//! TVTest の番組表パネル(`src/ProgramListPanel.cpp` / `src/ProgramListPanel.h`、
//! `CProgramListPanel`)のうち、ヘッダー/番組リストの矩形算出・ヒットテスト・
//! 行数ベースのアイテム位置計算・スクロール範囲計算を移植したもの。
//!
//! 移植範囲:
//! - `GetHeaderRect`(ProgramListPanel.cpp:476-481) / `GetChannelButtonRect`(:483-492) /
//!   `GetProgramListRect`(:495-501): クライアント矩形からヘッダー/チャンネルボタン/
//!   番組リストの矩形を算出する。
//! - `CalcChannelHeight`(:504-509): フォント高さ・ボタンアイコンサイズから
//!   ヘッダー(チャンネル欄)の高さを算出する。
//! - `ItemHitTest`(:699-717): ヘッダー領域内のヒットテスト
//!   ([`HeaderHitItem`] を返す)。
//! - `ProgramHitTest`(:720-740) / `GetItemRect`(:743-766): アイテムごとの
//!   (タイトル行数 + 本文行数)を積算し、座標⇔インデックスを変換する。
//! - `SetScrollPos`(:536-565) / `SetScrollBar`(:568-584) のうち、スクロール量の
//!   範囲クランプ計算部分(`si.nMax` の算出、:545-551 / :576-579)。
//!
//! 対象外(呼び出し側の責務):
//! - `CalcDimensions`(:512-533): `HDC` を用いたテキスト折返し行数の計測。本
//!   クレートは、算出済みの「アイテムごとのタイトル行数 + 本文行数」を
//!   [`ItemLines`] として受け取るのみ。
//! - `GetClientRect` / `PtInRect` 自体の呼び出し、`SetScrollInfo` /
//!   `ScrollWindowEx` によるウィンドウ操作、`Invalidate` による再描画。
//! - `Style::CStyleManager` / `CStyleScaling` によるスタイル値のDPI解決
//!   (`SetStyle` / `NormalizeStyle`)。本クレートは解決済みのピクセル値を
//!   [`ProgramListPanelStyle`] として受け取る。
//!
//! 矩形は Win32 の `RECT` に依存しないよう、本クレート独自の [`Rect`] を使う。

/// 矩形(`RECT` 相当)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    #[must_use]
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
}

/// 上下左右のマージン(`Style::Margins` 相当)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Margins {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Margins {
    #[must_use]
    pub fn horz(&self) -> i32 {
        self.left + self.right
    }

    #[must_use]
    pub fn vert(&self) -> i32 {
        self.top + self.bottom
    }
}

/// 矩形からマージンを減算する。原実装 `Style::Subtract`(Style.cpp:679-689)。
/// 減算した結果 `right < left` / `bottom < top` になる場合は `left` / `top`
/// にクランプする(原実装の挙動をそのまま踏襲)。
#[must_use]
pub fn subtract(rect: Rect, margins: Margins) -> Rect {
    let mut r = Rect {
        left: rect.left + margins.left,
        top: rect.top + margins.top,
        right: rect.right - margins.right,
        bottom: rect.bottom - margins.bottom,
    };
    if r.right < r.left {
        r.right = r.left;
    }
    if r.bottom < r.top {
        r.bottom = r.top;
    }
    r
}

/// 幅・高さ(`Style::Size` 相当)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Size {
    pub width: i32,
    pub height: i32,
}

/// `ProgramListPanelStyle`(ProgramListPanel.h:118-133)のうち、本クレートの
/// 計算に必要な値をまとめたもの。DPI解決済みのピクセル値を渡す。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProgramListPanelStyle {
    pub channel_padding: Margins,
    pub channel_name_margin: Margins,
    pub channel_button_icon_size: Size,
    pub channel_button_padding: Margins,
    pub channel_button_margin: i32,
    pub title_padding: Margins,
    pub line_spacing: i32,
}

/// `ItemHitTest` の戻り値(ProgramListPanel.h:141-144 の `enum` 相当)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderHitItem {
    /// チャンネル名部分(`ITEM_CHANNEL`)。
    Channel,
    /// チャンネル一覧ボタン(`ITEM_CHANNELLISTBUTTON`)。
    ChannelListButton,
}

/// アイテム(番組)ごとの行数。`CProgramItemInfo::GetTitleLines` /
/// `GetTextLines`(ProgramListPanel.cpp:733, :756)に対応する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ItemLines {
    pub title_lines: i32,
    pub text_lines: i32,
}

impl ItemLines {
    fn total(&self) -> i32 {
        self.title_lines + self.text_lines
    }
}

/// クライアント領域の幅・高さ・ヘッダー高さ・フォント高さ・スタイル値・
/// スクロール位置を保持し、矩形算出/ヒットテスト/スクロール範囲計算を行う。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProgramListPanelLayout {
    /// クライアント領域の幅(`GetClientRect` の結果、`rc.right`)。
    pub client_width: i32,
    /// クライアント領域の高さ(`rc.bottom`)。
    pub client_height: i32,
    /// ヘッダー(チャンネル欄)の高さ(`m_ChannelHeight`)。
    pub channel_height: i32,
    /// フォントの高さ(`m_FontHeight`)。
    pub font_height: i32,
    pub style: ProgramListPanelStyle,
    /// 現在のスクロール位置(`m_ScrollPos`)。
    pub scroll_pos: i32,
}

impl ProgramListPanelLayout {
    /// ヘッダー(チャンネル欄)の矩形。原実装 `GetHeaderRect`(:476-481)。
    #[must_use]
    pub fn header_rect(&self) -> Rect {
        Rect {
            left: 0,
            top: 0,
            right: self.client_width,
            bottom: self.channel_height,
        }
    }

    /// チャンネル一覧ボタンの矩形。原実装 `GetChannelButtonRect`(:483-492)。
    #[must_use]
    pub fn channel_button_rect(&self) -> Rect {
        let rc = subtract(self.header_rect(), self.style.channel_padding);
        let width =
            self.style.channel_button_icon_size.width + self.style.channel_button_padding.horz();
        let height =
            self.style.channel_button_icon_size.height + self.style.channel_button_padding.vert();
        let top = rc.top + ((rc.bottom - rc.top) - height) / 2;
        Rect {
            left: rc.right - width,
            top,
            right: rc.right,
            bottom: top + height,
        }
    }

    /// 番組リスト部分の矩形。原実装 `GetProgramListRect`(:495-501)。
    #[must_use]
    pub fn program_list_rect(&self) -> Rect {
        let top = self.channel_height;
        let bottom = if self.client_height < top {
            top
        } else {
            self.client_height
        };
        Rect {
            left: 0,
            top,
            right: self.client_width,
            bottom,
        }
    }

    /// ヘッダー領域内のヒットテスト。原実装 `ItemHitTest`(:699-717)。
    ///
    /// ヘッダー領域外なら `None`。チャンネル一覧ボタンの矩形内なら
    /// `ChannelListButton`。ボタン矩形の左端から `channel_button_margin`
    /// だけ離れた位置より左側なら `Channel`。それ以外(ボタンとチャンネル名の
    /// 間の余白)は `None`(原実装で `HotItem` が更新されないケース)。
    #[must_use]
    pub fn item_hit_test(&self, x: i32, y: i32) -> Option<HeaderHitItem> {
        if !self.header_rect().contains(x, y) {
            return None;
        }
        let button_rect = self.channel_button_rect();
        if button_rect.contains(x, y) {
            Some(HeaderHitItem::ChannelListButton)
        } else if x < button_rect.left - self.style.channel_button_margin {
            Some(HeaderHitItem::Channel)
        } else {
            None
        }
    }

    /// アイテム 1 件分の高さ(タイトル行 + 本文行の合計から算出)。
    /// `(タイトル行数 + 本文行数) * (フォント高さ + 行間) + (タイトル余白の上下 - 行間)`
    /// (:732-734, :755-757 の式)。
    fn item_height(&self, lines: ItemLines) -> i32 {
        lines.total() * (self.font_height + self.style.line_spacing)
            + (self.style.title_padding.top + self.style.title_padding.bottom
                - self.style.line_spacing)
    }

    /// 座標から番組アイテムのインデックスを解決する。原実装
    /// `ProgramHitTest`(:720-740)。番組リスト領域外、またどのアイテムにも
    /// 属さない座標(全アイテムの下)なら `None`。
    #[must_use]
    pub fn program_hit_test(&self, x: i32, y: i32, items: &[ItemLines]) -> Option<usize> {
        let rc = self.program_list_rect();
        if !rc.contains(x, y) {
            return None;
        }
        let mut top = rc.top - self.scroll_pos;
        for (i, lines) in items.iter().enumerate() {
            let bottom = top + self.item_height(*lines);
            let item_rc = Rect {
                left: rc.left,
                top,
                right: rc.right,
                bottom,
            };
            if item_rc.contains(x, y) {
                return Some(i);
            }
            top = bottom;
        }
        None
    }

    /// 指定インデックスの番組アイテムの矩形。原実装 `GetItemRect`(:743-766)。
    /// `item` が範囲外(`items.len()` 以上)なら `None`。
    #[must_use]
    pub fn item_rect(&self, item: usize, items: &[ItemLines]) -> Option<Rect> {
        if item >= items.len() {
            return None;
        }
        let rc = self.program_list_rect();
        let mut top = rc.top - self.scroll_pos;
        for (i, lines) in items.iter().enumerate() {
            let bottom = top + self.item_height(*lines);
            if i == item {
                return Some(Rect {
                    left: rc.left,
                    top,
                    right: rc.right,
                    bottom,
                });
            }
            top = bottom;
        }
        None
    }

    /// スクロール可能な最大量(`si.nMax`)を算出する。原実装
    /// `SetScrollBar`(:576-579)。全アイテムの行数が 0 未満なら `0`。
    #[must_use]
    pub fn scroll_range(&self, total_lines: i32, item_count: i32) -> i32 {
        if total_lines < 1 {
            0
        } else {
            total_lines * (self.font_height + self.style.line_spacing)
                + item_count
                    * (self.style.title_padding.top + self.style.title_padding.bottom
                        - self.style.line_spacing)
        }
    }

    /// 新しいスクロール位置を範囲内にクランプする。原実装
    /// `SetScrollPos`(:542-552)。`page` は表示可能な高さ(`program_list_rect`
    /// の高さ)。
    #[must_use]
    pub fn clamp_scroll_pos(&self, pos: i32, total_lines: i32, item_count: i32) -> i32 {
        if pos < 0 {
            return 0;
        }
        let rc = self.program_list_rect();
        let page = rc.bottom - rc.top;
        let max = (self.scroll_range(total_lines, item_count) - page).max(0);
        pos.min(max)
    }
}

/// ヘッダー(チャンネル欄)の高さを算出する。原実装
/// `CalcChannelHeight`(ProgramListPanel.cpp:504-509)。
#[must_use]
pub fn calc_channel_height(font_height: i32, style: &ProgramListPanelStyle) -> i32 {
    let label_height = font_height + style.channel_name_margin.vert();
    let button_height = style.channel_button_icon_size.height + style.channel_button_padding.vert();
    label_height.max(button_height) + style.channel_padding.vert()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style() -> ProgramListPanelStyle {
        ProgramListPanelStyle {
            channel_padding: Margins {
                left: 3,
                top: 3,
                right: 3,
                bottom: 3,
            },
            channel_name_margin: Margins {
                left: 0,
                top: 2,
                right: 0,
                bottom: 2,
            },
            channel_button_icon_size: Size {
                width: 12,
                height: 12,
            },
            channel_button_padding: Margins {
                left: 2,
                top: 2,
                right: 2,
                bottom: 2,
            },
            channel_button_margin: 12,
            title_padding: Margins {
                left: 2,
                top: 2,
                right: 2,
                bottom: 2,
            },
            line_spacing: 1,
        }
    }

    fn layout() -> ProgramListPanelLayout {
        let s = style();
        let channel_height = calc_channel_height(16, &s);
        ProgramListPanelLayout {
            client_width: 300,
            client_height: 400,
            channel_height,
            font_height: 16,
            style: s,
            scroll_pos: 0,
        }
    }

    // ----- subtract(Style.cpp:679-689) -----

    #[test]
    fn subtract_reduces_rect_by_margins() {
        let r = Rect {
            left: 0,
            top: 0,
            right: 100,
            bottom: 50,
        };
        let m = Margins {
            left: 3,
            top: 3,
            right: 3,
            bottom: 3,
        };
        assert_eq!(
            subtract(r, m),
            Rect {
                left: 3,
                top: 3,
                right: 97,
                bottom: 47,
            }
        );
    }

    #[test]
    fn subtract_clamps_when_margins_exceed_size() {
        let r = Rect {
            left: 0,
            top: 0,
            right: 4,
            bottom: 4,
        };
        let m = Margins {
            left: 10,
            top: 10,
            right: 10,
            bottom: 10,
        };
        let result = subtract(r, m);
        assert_eq!(result.right, result.left);
        assert_eq!(result.bottom, result.top);
    }

    // ----- calc_channel_height(ProgramListPanel.cpp:504-509) -----

    #[test]
    fn calc_channel_height_uses_label_height_when_larger() {
        let s = style();
        // label_height = 16 + 4 = 20, button_height = 12 + 4 = 16
        assert_eq!(calc_channel_height(16, &s), 20 + 6);
    }

    #[test]
    fn calc_channel_height_uses_button_height_when_larger() {
        let mut s = style();
        s.channel_button_icon_size.height = 40;
        // label_height = 16 + 4 = 20, button_height = 40 + 4 = 44
        assert_eq!(calc_channel_height(16, &s), 44 + 6);
    }

    // ----- header_rect / channel_button_rect / program_list_rect -----

    #[test]
    fn header_rect_spans_client_width() {
        let l = layout();
        let rc = l.header_rect();
        assert_eq!(rc.left, 0);
        assert_eq!(rc.right, 300);
        assert_eq!(rc.top, 0);
        assert_eq!(rc.bottom, l.channel_height);
    }

    #[test]
    fn program_list_rect_starts_below_header() {
        let l = layout();
        let rc = l.program_list_rect();
        assert_eq!(rc.top, l.channel_height);
        assert_eq!(rc.bottom, 400);
    }

    #[test]
    fn program_list_rect_clamps_when_client_height_below_header() {
        let mut l = layout();
        l.client_height = 5;
        let rc = l.program_list_rect();
        assert_eq!(rc.bottom, rc.top);
    }

    #[test]
    fn channel_button_rect_is_right_aligned_within_padding() {
        let l = layout();
        let header = subtract(l.header_rect(), l.style.channel_padding);
        let rc = l.channel_button_rect();
        assert_eq!(rc.right, header.right);
        assert_eq!(rc.right - rc.left, 12 + 4);
    }

    #[test]
    fn channel_button_rect_is_vertically_centered() {
        let l = layout();
        let header = subtract(l.header_rect(), l.style.channel_padding);
        let rc = l.channel_button_rect();
        let expected_top = header.top + ((header.bottom - header.top) - (rc.bottom - rc.top)) / 2;
        assert_eq!(rc.top, expected_top);
    }

    // ----- item_hit_test(ProgramListPanel.cpp:699-717) -----

    #[test]
    fn item_hit_test_outside_header_returns_none() {
        let l = layout();
        assert_eq!(l.item_hit_test(10, l.channel_height + 5), None);
    }

    #[test]
    fn item_hit_test_on_button_returns_channel_list_button() {
        let l = layout();
        let rc = l.channel_button_rect();
        assert_eq!(
            l.item_hit_test(rc.left, rc.top),
            Some(HeaderHitItem::ChannelListButton)
        );
    }

    #[test]
    fn item_hit_test_left_of_button_margin_returns_channel() {
        let l = layout();
        let rc = l.channel_button_rect();
        let x = rc.left - l.style.channel_button_margin - 1;
        assert_eq!(l.item_hit_test(x, rc.top), Some(HeaderHitItem::Channel));
    }

    #[test]
    fn item_hit_test_in_margin_gap_returns_none() {
        let l = layout();
        let rc = l.channel_button_rect();
        // ボタン矩形の左端から channel_button_margin 未満の位置(ボタンでもチャンネル名でもない)
        let x = rc.left - 1;
        assert_eq!(l.item_hit_test(x, rc.top), None);
    }

    // ----- program_hit_test / item_rect(:720-766) -----

    fn items() -> Vec<ItemLines> {
        vec![
            ItemLines {
                title_lines: 1,
                text_lines: 2,
            },
            ItemLines {
                title_lines: 1,
                text_lines: 1,
            },
            ItemLines {
                title_lines: 2,
                text_lines: 3,
            },
        ]
    }

    #[test]
    fn item_rect_stacks_by_line_height() {
        let l = layout();
        let its = items();
        let r0 = l.item_rect(0, &its).unwrap();
        let r1 = l.item_rect(1, &its).unwrap();
        assert_eq!(r0.bottom, r1.top);
        assert_eq!(r0.top, l.program_list_rect().top);
    }

    #[test]
    fn item_rect_out_of_range_returns_none() {
        let l = layout();
        let its = items();
        assert_eq!(l.item_rect(its.len(), &its), None);
    }

    #[test]
    fn program_hit_test_matches_item_rect_roundtrip() {
        let l = layout();
        let its = items();
        for i in 0..its.len() {
            let rc = l.item_rect(i, &its).unwrap();
            assert_eq!(l.program_hit_test(rc.left, rc.top, &its), Some(i));
        }
    }

    #[test]
    fn program_hit_test_outside_list_rect_returns_none() {
        let l = layout();
        let its = items();
        assert_eq!(l.program_hit_test(10, 0, &its), None);
    }

    #[test]
    fn program_hit_test_below_all_items_returns_none() {
        let l = layout();
        let its = items();
        let last = l.item_rect(its.len() - 1, &its).unwrap();
        assert_eq!(l.program_hit_test(10, last.bottom + 1000, &its), None);
    }

    #[test]
    fn program_hit_test_and_item_rect_account_for_scroll_pos() {
        let mut l = layout();
        let its = items();
        let unscrolled = l.item_rect(1, &its).unwrap();
        l.scroll_pos = 10;
        let scrolled = l.item_rect(1, &its).unwrap();
        assert_eq!(scrolled.top, unscrolled.top - 10);
    }

    // ----- scroll_range / clamp_scroll_pos(:536-584) -----

    #[test]
    fn scroll_range_is_zero_when_no_lines() {
        let l = layout();
        assert_eq!(l.scroll_range(0, 0), 0);
    }

    #[test]
    fn scroll_range_matches_set_scroll_bar_formula() {
        let l = layout();
        let total_lines = 10;
        let item_count = 3;
        let expected = total_lines * (l.font_height + l.style.line_spacing)
            + item_count
                * (l.style.title_padding.top + l.style.title_padding.bottom - l.style.line_spacing);
        assert_eq!(l.scroll_range(total_lines, item_count), expected);
    }

    #[test]
    fn clamp_scroll_pos_rejects_negative() {
        let l = layout();
        assert_eq!(l.clamp_scroll_pos(-5, 100, 3), 0);
    }

    #[test]
    fn clamp_scroll_pos_caps_at_max() {
        let l = layout();
        let max = (l.scroll_range(10, 3)
            - (l.program_list_rect().bottom - l.program_list_rect().top))
            .max(0);
        assert_eq!(l.clamp_scroll_pos(max + 1000, 10, 3), max);
    }

    #[test]
    fn clamp_scroll_pos_allows_within_range() {
        let l = layout();
        let max = (l.scroll_range(100, 3)
            - (l.program_list_rect().bottom - l.program_list_rect().top))
            .max(0);
        if max > 0 {
            assert_eq!(l.clamp_scroll_pos(max - 1, 100, 3), max - 1);
        }
    }

    #[test]
    fn clamp_scroll_pos_is_zero_when_content_fits_page() {
        let l = layout();
        // 行数が小さく、page より短い場合は max が 0 になりクランプされる
        assert_eq!(l.clamp_scroll_pos(50, 1, 1), 0);
    }
}
