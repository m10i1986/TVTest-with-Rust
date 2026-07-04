#![forbid(unsafe_code)]
//! TVTest のチャンネル選択パネル(`src/ChannelDisplay.cpp` / `src/ChannelDisplay.h`、
//! `CChannelDisplay`)のうち、チューナー一覧・チャンネル一覧の等間隔グリッド
//! レイアウト計算部分の Rust 移植。
//!
//! 移植範囲:
//! - `GetTunerItemRect`(ChannelDisplay.cpp:531-537)/
//!   `GetChannelItemRect`(:540-546): インデックスからアイテム矩形を算出する。
//! - `TunerItemHitTest`(:569-578)/`ChannelItemHitTest`(:581-593): 座標から
//!   アイテムのインデックスを解決する(範囲外は `-1`)。
//!
//! 対象外(呼び出し側の責務):
//! - ウィンドウ生成・スクロールバー制御・`InvalidateRect`(`UpdateTunerItem` /
//!   `UpdateChannelItem`、:549-566)。
//! - `CTuner` / `CTuningSpaceInfo` の実体管理。`ChannelItemHitTest` が参照する
//!   「現在選択中チューナーのチャンネル数」(`GetTuningSpaceInfo(m_CurTuner)->NumChannels()`、
//!   :587-589)は呼び出し側が `total_channels` として渡す。
//! - フォント計測・DPI 計算によるアイテム幅/高さの決定(`Layout`)。本クレートは
//!   算出済みの幅・高さ・左上座標・スクロール位置・可視件数を受け取るのみ。
//!
//! 矩形は Win32 の `RECT` に依存しないよう、本クレート独自の [`ItemRect`] を使う。

/// アイテムの矩形(`RECT` 相当)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ItemRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// 等間隔グリッドで並ぶ一覧(チューナー一覧・チャンネル一覧共通)のレイアウト。
///
/// `CChannelDisplay` のメンバ変数のうち、本クレートが扱う計算に必要な部分
/// (`m_TunerItemLeft`/`m_TunerItemTop`/`m_TunerItemWidth`/`m_TunerItemHeight`/
/// `m_TunerScrollPos`/`m_VisibleTunerItems` の組、およびチャンネル側の対応する組、
/// ChannelDisplay.h:163-176)を一般化したもの。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GridLayout {
    /// アイテム列左端の x 座標(`m_TunerItemLeft` / `m_ChannelItemLeft`)。
    pub left: i32,
    /// スクロール位置 0 のときのアイテム列上端の y 座標
    /// (`m_TunerItemTop` / `m_ChannelItemTop`)。
    pub top: i32,
    /// アイテム 1 個の幅(`m_TunerItemWidth` / `m_ChannelItemWidth`)。
    pub item_width: i32,
    /// アイテム 1 個の高さ(`m_TunerItemHeight` / `m_ChannelItemHeight`)。
    pub item_height: i32,
    /// 先頭に表示されているアイテムのインデックス
    /// (`m_TunerScrollPos` / `m_ChannelScrollPos`)。
    pub scroll_pos: i32,
    /// 画面に表示できるアイテム数(`m_VisibleTunerItems` / `m_VisibleChannelItems`)。
    pub visible_items: i32,
}

impl GridLayout {
    /// アイテム矩形を算出する。原実装 `GetTunerItemRect`(ChannelDisplay.cpp:531-537)/
    /// `GetChannelItemRect`(:540-546)。
    ///
    /// `top = layout.top + (index - scroll_pos) * item_height`。範囲外の `index`
    /// (負値やスクロール位置より大きく離れた値)を渡しても、原実装同様に
    /// 矩形は計算されるだけで検証はしない。
    #[must_use]
    pub fn item_rect(&self, index: i32) -> ItemRect {
        let top = self.top + (index - self.scroll_pos) * self.item_height;
        ItemRect {
            left: self.left,
            top,
            right: self.left + self.item_width,
            bottom: top + self.item_height,
        }
    }

    /// 座標からアイテム列内かどうかを判定する。`TunerItemHitTest`(:571-572)/
    /// `ChannelItemHitTest`(:584-585)の座標範囲チェック部分。
    fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.left
            && x < self.left + self.item_width
            && y >= self.top
            && y < self.top + self.visible_items * self.item_height
    }

    /// 座標からインデックスを逆算する(範囲チェックなし)。`(y - top) / item_height +
    /// scroll_pos`(:573 / :586)。
    fn index_at(&self, y: i32) -> i32 {
        (y - self.top) / self.item_height + self.scroll_pos
    }
}

/// チューナー一覧のヒットテスト。原実装 `TunerItemHitTest`(ChannelDisplay.cpp:569-578)。
///
/// 座標がアイテム列の範囲外、またはインデックスが `total_tuning_spaces`
/// (`m_TotalTuningSpaces`)以上なら `-1`。
#[must_use]
pub fn tuner_item_hit_test(layout: &GridLayout, x: i32, y: i32, total_tuning_spaces: i32) -> i32 {
    if layout.contains(x, y) {
        let index = layout.index_at(y);
        if index < total_tuning_spaces {
            return index;
        }
    }
    -1
}

/// チャンネル一覧のヒットテスト。原実装 `ChannelItemHitTest`(ChannelDisplay.cpp:581-593)。
///
/// `cur_tuner_selected` が偽(`m_CurTuner < 0`、:583)、座標がアイテム列の範囲外、
/// またはインデックスが `total_channels`(現在選択中チューナーの
/// `NumChannels()`、:587-589)以上なら `-1`。
#[must_use]
pub fn channel_item_hit_test(
    layout: &GridLayout,
    x: i32,
    y: i32,
    cur_tuner_selected: bool,
    total_channels: i32,
) -> i32 {
    if cur_tuner_selected && layout.contains(x, y) {
        let index = layout.index_at(y);
        if index < total_channels {
            return index;
        }
    }
    -1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> GridLayout {
        GridLayout {
            left: 10,
            top: 20,
            item_width: 100,
            item_height: 16,
            scroll_pos: 0,
            visible_items: 5,
        }
    }

    // ----- item_rect(ChannelDisplay.cpp:531-546) -----

    #[test]
    fn item_rect_first_index() {
        let l = layout();
        assert_eq!(
            l.item_rect(0),
            ItemRect {
                left: 10,
                top: 20,
                right: 110,
                bottom: 36,
            }
        );
    }

    #[test]
    fn item_rect_advances_by_item_height() {
        let l = layout();
        assert_eq!(
            l.item_rect(3),
            ItemRect {
                left: 10,
                top: 20 + 3 * 16,
                right: 110,
                bottom: 20 + 3 * 16 + 16,
            }
        );
    }

    #[test]
    fn item_rect_accounts_for_scroll_pos() {
        let l = GridLayout {
            scroll_pos: 2,
            ..layout()
        };
        // Index 2 がスクロール後の先頭(top のまま)
        assert_eq!(l.item_rect(2).top, 20);
        assert_eq!(l.item_rect(5).top, 20 + 3 * 16);
    }

    #[test]
    fn item_rect_negative_relative_index_moves_above_top() {
        // 原実装は範囲チェックをしないため、スクロール位置より手前の index は
        // top より上の矩形になる
        let l = GridLayout {
            scroll_pos: 5,
            ..layout()
        };
        assert_eq!(l.item_rect(3).top, 20 - 2 * 16);
    }

    // ----- tuner_item_hit_test(ChannelDisplay.cpp:569-578) -----

    #[test]
    fn tuner_hit_test_inside_first_item() {
        let l = layout();
        assert_eq!(tuner_item_hit_test(&l, 10, 20, 10), 0);
        assert_eq!(tuner_item_hit_test(&l, 109, 35, 10), 0);
    }

    #[test]
    fn tuner_hit_test_second_row() {
        let l = layout();
        assert_eq!(tuner_item_hit_test(&l, 50, 36, 10), 1);
    }

    #[test]
    fn tuner_hit_test_outside_left_edge() {
        let l = layout();
        assert_eq!(tuner_item_hit_test(&l, 9, 20, 10), -1);
    }

    #[test]
    fn tuner_hit_test_outside_right_edge() {
        let l = layout();
        assert_eq!(tuner_item_hit_test(&l, 110, 20, 10), -1);
    }

    #[test]
    fn tuner_hit_test_above_top() {
        let l = layout();
        assert_eq!(tuner_item_hit_test(&l, 50, 19, 10), -1);
    }

    #[test]
    fn tuner_hit_test_below_visible_area() {
        let l = layout();
        // visible_items = 5 → 5*16 = 80、top + 80 = 100 以降は範囲外
        assert_eq!(tuner_item_hit_test(&l, 50, 100, 10), -1);
        assert_eq!(tuner_item_hit_test(&l, 50, 99, 10), 4);
    }

    #[test]
    fn tuner_hit_test_beyond_total_tuning_spaces() {
        let l = layout();
        // index が算出されても total_tuning_spaces 以上なら -1(:574-575)
        assert_eq!(tuner_item_hit_test(&l, 50, 36, 1), -1);
        assert_eq!(tuner_item_hit_test(&l, 50, 20, 1), 0);
    }

    #[test]
    fn tuner_hit_test_with_scroll_pos() {
        let l = GridLayout {
            scroll_pos: 3,
            ..layout()
        };
        assert_eq!(tuner_item_hit_test(&l, 50, 20, 100), 3);
        assert_eq!(tuner_item_hit_test(&l, 50, 36, 100), 4);
    }

    // ----- channel_item_hit_test(ChannelDisplay.cpp:581-593) -----

    #[test]
    fn channel_hit_test_requires_tuner_selected() {
        let l = layout();
        assert_eq!(channel_item_hit_test(&l, 50, 20, false, 10), -1);
        assert_eq!(channel_item_hit_test(&l, 50, 20, true, 10), 0);
    }

    #[test]
    fn channel_hit_test_beyond_total_channels() {
        let l = layout();
        assert_eq!(channel_item_hit_test(&l, 50, 36, true, 1), -1);
        assert_eq!(channel_item_hit_test(&l, 50, 20, true, 1), 0);
    }

    #[test]
    fn channel_hit_test_outside_bounds() {
        let l = layout();
        assert_eq!(channel_item_hit_test(&l, 9, 20, true, 10), -1);
        assert_eq!(channel_item_hit_test(&l, 50, 100, true, 10), -1);
    }

    #[test]
    fn channel_hit_test_roundtrips_with_item_rect() {
        // item_rect で得た矩形の左上座標が、同じ index を返すことを確認
        let l = GridLayout {
            scroll_pos: 2,
            ..layout()
        };
        for index in 2..7 {
            let rc = l.item_rect(index);
            assert_eq!(channel_item_hit_test(&l, rc.left, rc.top, true, 100), index);
        }
    }
}
