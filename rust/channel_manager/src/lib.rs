//! TVTest の `CChannelManager`(src/ChannelManager.cpp / ChannelManager.h)の純粋ロジック移植。
//!
//! チャンネル送り(リモコン番号順 / インデックス順・無効チャンネルのスキップ・巡回)、
//! 現在位置の状態保持([`ChannelPosition`])、チャンネル指定値([`ChannelSpec`])を移植する。
//!
//! # 対象外(上位統合層 / Win32 / I-O 依存)
//! チューニング空間リストの解決(`GetChannelList`/`NumSpaces`)、チャンネルファイル読込
//! (`LoadChannelList`)、BonDriver 連携(`MakeDriverTuningSpaceList`)、`SetCurrentChannel`
//! のチューニング空間検証。これらは [`tvtest_channel_list`] 上に構築する上位層に属する。
//! 本クレートのナビゲーション関数は対象の [`ChannelList`] を引数で受け取る。

use tvtest_channel_list::ChannelList;

/// 無効な空間(ChannelManager.h 46 `SPACE_INVALID`)。
pub const SPACE_INVALID: i32 = -2;
/// 全空間(ChannelManager.h 47 `SPACE_ALL`)。
pub const SPACE_ALL: i32 = -1;

/// チャンネル送りの順序(ChannelManager.h 49-52 `UpDownOrder`)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UpDownOrder {
    /// リスト上のインデックス順。
    Index,
    /// リモコン番号(チャンネル番号)順。
    Id,
}

/// 指定リスト内で次/前の有効チャンネルのインデックスを返す
/// (ChannelManager.cpp 229-266 `GetNextChannel(CurChannel, Order, fNext)`)。
///
/// - `Id` 指定かつ現在チャンネルがリモコン番号を持つ場合はリモコン番号順で巡回。
/// - それ以外はインデックス順に巡回し、無効チャンネルを飛ばす。
/// - 見つからない/`cur_channel` が範囲外なら -1。
pub fn next_channel_in_list(
    list: &ChannelList,
    cur_channel: i32,
    order: UpDownOrder,
    next: bool,
) -> i32 {
    let n = list.num_channels() as i32;
    if cur_channel < 0 || cur_channel >= n {
        return -1;
    }

    let mut channel = cur_channel;
    let cur_no = list
        .get_channel(cur_channel as usize)
        .map_or(0, |c| c.channel_no);

    if order == UpDownOrder::Id && cur_no > 0 {
        let result = if next {
            list.get_next_channel(channel as usize, true)
        } else {
            list.get_prev_channel(channel as usize, true)
        };
        channel = result.map_or(-1, |i| i as i32);
    } else {
        let mut found = false;
        // 最大 NumChannels 回まで巡回して有効チャンネルを探す。
        for _ in 0..n {
            if next {
                channel += 1;
                if channel >= n {
                    channel = 0;
                }
            } else {
                channel -= 1;
                if channel < 0 {
                    channel = n - 1;
                }
            }
            if list
                .get_channel(channel as usize)
                .is_some_and(|c| c.enabled)
            {
                found = true;
                break;
            }
        }
        if !found {
            return -1;
        }
    }

    channel
}

/// チャンネルマネージャの現在位置状態(`CChannelManager` の位置関連メンバ)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelPosition {
    pub current_space: i32,
    pub current_channel: i32,
    pub current_service_id: i32,
    pub changing_channel: i32,
}

impl Default for ChannelPosition {
    fn default() -> Self {
        Self::new()
    }
}

impl ChannelPosition {
    /// 初期状態(ChannelManager.cpp 38-43 `Reset` の位置部分)。
    pub fn new() -> Self {
        Self {
            current_space: SPACE_INVALID,
            current_channel: -1,
            current_service_id: -1,
            changing_channel: -1,
        }
    }

    /// 位置状態を初期化する(`Reset` の位置部分)。
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// 現在のサービス ID を設定する(ChannelManager.cpp 195-199 `SetCurrentServiceID`)。
    pub fn set_current_service_id(&mut self, service_id: i32) -> bool {
        self.current_service_id = service_id;
        true
    }

    /// 変更中チャンネルを設定する(ChannelManager.cpp 202-206 `SetChangingChannel`)。
    pub fn set_changing_channel(&mut self, channel: i32) -> bool {
        self.changing_channel = channel;
        true
    }

    /// 現在(または変更中)チャンネルを基準に次/前の有効チャンネルを返す
    /// (ChannelManager.cpp 269-282 `GetNextChannel(Order, fNext)`)。
    ///
    /// 変更中チャンネルがあればそれを、無ければ現在チャンネルを基準にする。
    pub fn next_channel(&self, list: &ChannelList, order: UpDownOrder, next: bool) -> i32 {
        let channel = if self.changing_channel >= 0 {
            self.changing_channel
        } else {
            if self.current_channel < 0 {
                return -1;
            }
            self.current_channel
        };
        next_channel_in_list(list, channel, order, next)
    }
}

/// チャンネル指定値(ChannelManager.h 93-108 / .cpp 407-434 `CChannelSpec`)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelSpec {
    space: i32,
    channel: i32,
    service_id: i32,
}

impl Default for ChannelSpec {
    fn default() -> Self {
        Self::new()
    }
}

impl ChannelSpec {
    /// 初期状態(ChannelManager.h 95-97 のメンバ既定値)。
    pub fn new() -> Self {
        Self {
            space: SPACE_INVALID,
            channel: -1,
            service_id: -1,
        }
    }

    /// 現在位置を取り込む(ChannelManager.cpp 407-413 `Store`)。
    ///
    /// 原実装は `CChannelManager*` から現在値を取得するが、ここでは位置状態を受け取る。
    pub fn store_from(&mut self, position: &ChannelPosition) -> bool {
        self.space = position.current_space;
        self.channel = position.current_channel;
        self.service_id = position.current_service_id;
        true
    }

    /// 空間を設定する(ChannelManager.cpp 416-420 `SetSpace`)。
    pub fn set_space(&mut self, space: i32) -> bool {
        self.space = space;
        true
    }

    /// 空間を返す。
    pub fn space(&self) -> i32 {
        self.space
    }

    /// チャンネルを設定する(ChannelManager.cpp 423-427 `SetChannel`)。
    pub fn set_channel(&mut self, channel: i32) -> bool {
        self.channel = channel;
        true
    }

    /// チャンネルを返す。
    pub fn channel(&self) -> i32 {
        self.channel
    }

    /// サービス ID を設定する(ChannelManager.cpp 430-434 `SetServiceID`)。
    pub fn set_service_id(&mut self, service_id: i32) -> bool {
        self.service_id = service_id;
        true
    }

    /// サービス ID を返す。
    pub fn service_id(&self) -> i32 {
        self.service_id
    }

    /// 有効な指定か(ChannelManager.h 107 `IsValid`)。
    pub fn is_valid(&self) -> bool {
        self.space > SPACE_INVALID && self.channel >= 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tvtest_channel_list::{ChannelInfo, ChannelList};

    /// (channel_no, enabled) の並びからチャンネルリストを作る。
    fn make_list(channels: &[(i32, bool)]) -> ChannelList {
        let mut list = ChannelList::new();
        for (i, &(no, enabled)) in channels.iter().enumerate() {
            let mut info = ChannelInfo::new(0, i as i32, no, "ch");
            info.enabled = enabled;
            list.add_channel(info);
        }
        list
    }

    #[test]
    fn index_order_skips_disabled_and_wraps() {
        // idx: 0=有効, 1=無効, 2=有効, 3=有効
        let list = make_list(&[(0, true), (0, false), (0, true), (0, true)]);
        // 0 から次へ -> 1(無効)を飛ばして 2
        assert_eq!(next_channel_in_list(&list, 0, UpDownOrder::Index, true), 2);
        // 0 から前へ -> -1 へ巡回し 3(有効)
        assert_eq!(next_channel_in_list(&list, 0, UpDownOrder::Index, false), 3);
        // 3 から次へ -> 0 へ巡回
        assert_eq!(next_channel_in_list(&list, 3, UpDownOrder::Index, true), 0);
    }

    #[test]
    fn index_order_invalid_and_empty() {
        let list = make_list(&[(0, true), (0, true)]);
        // 範囲外
        assert_eq!(next_channel_in_list(&list, -1, UpDownOrder::Index, true), -1);
        assert_eq!(next_channel_in_list(&list, 2, UpDownOrder::Index, true), -1);
        // 空リスト
        let empty = ChannelList::new();
        assert_eq!(next_channel_in_list(&empty, 0, UpDownOrder::Index, true), -1);
    }

    #[test]
    fn index_order_all_disabled() {
        let list = make_list(&[(0, false), (0, false), (0, false)]);
        // 自分自身も無効なので一周して見つからない -> -1
        assert_eq!(next_channel_in_list(&list, 0, UpDownOrder::Index, true), -1);
    }

    #[test]
    fn id_order_uses_remote_number() {
        // リモコン番号 1,2,3 を持つ有効チャンネル
        let list = make_list(&[(1, true), (2, true), (3, true)]);
        // idx0(no=1) から次へ -> no=2 の idx1
        assert_eq!(next_channel_in_list(&list, 0, UpDownOrder::Id, true), 1);
        // idx1(no=2) から前へ -> no=1 の idx0
        assert_eq!(next_channel_in_list(&list, 1, UpDownOrder::Id, false), 0);
        // idx2(no=3) から次へ -> 巡回して no=1 の idx0
        assert_eq!(next_channel_in_list(&list, 2, UpDownOrder::Id, true), 0);
    }

    #[test]
    fn id_order_falls_back_to_index_when_no_remote_number() {
        // リモコン番号が 0 のときはインデックス順にフォールバック
        let list = make_list(&[(0, true), (0, true), (0, true)]);
        assert_eq!(next_channel_in_list(&list, 0, UpDownOrder::Id, true), 1);
    }

    #[test]
    fn position_defaults_and_setters() {
        let mut pos = ChannelPosition::new();
        assert_eq!(pos.current_space, SPACE_INVALID);
        assert_eq!(pos.current_channel, -1);
        assert_eq!(pos.current_service_id, -1);
        assert_eq!(pos.changing_channel, -1);

        assert!(pos.set_current_service_id(101));
        assert_eq!(pos.current_service_id, 101);
        assert!(pos.set_changing_channel(2));
        assert_eq!(pos.changing_channel, 2);

        pos.reset();
        assert_eq!(pos.changing_channel, -1);
    }

    #[test]
    fn position_next_channel_prefers_changing() {
        let list = make_list(&[(0, true), (0, true), (0, true), (0, true)]);
        let mut pos = ChannelPosition::new();
        pos.current_channel = 0;
        // 変更中が無ければ現在チャンネル基準
        assert_eq!(pos.next_channel(&list, UpDownOrder::Index, true), 1);
        // 変更中があればそれを基準
        pos.set_changing_channel(2);
        assert_eq!(pos.next_channel(&list, UpDownOrder::Index, true), 3);
    }

    #[test]
    fn position_next_channel_no_current() {
        let list = make_list(&[(0, true), (0, true)]);
        let pos = ChannelPosition::new(); // current_channel = -1, changing = -1
        assert_eq!(pos.next_channel(&list, UpDownOrder::Index, true), -1);
    }

    #[test]
    fn channel_spec_defaults_and_validity() {
        let mut spec = ChannelSpec::new();
        assert_eq!(spec.space(), SPACE_INVALID);
        assert_eq!(spec.channel(), -1);
        assert_eq!(spec.service_id(), -1);
        assert!(!spec.is_valid());

        spec.set_space(0);
        spec.set_channel(3);
        spec.set_service_id(1024);
        assert!(spec.is_valid());
        assert_eq!(spec.space(), 0);
        assert_eq!(spec.channel(), 3);
        assert_eq!(spec.service_id(), 1024);

        // space は有効だが channel < 0 なら無効
        spec.set_channel(-1);
        assert!(!spec.is_valid());
    }

    #[test]
    fn channel_spec_store_from_position() {
        let mut pos = ChannelPosition::new();
        pos.current_space = 1;
        pos.current_channel = 5;
        pos.set_current_service_id(200);

        let mut spec = ChannelSpec::new();
        assert!(spec.store_from(&pos));
        assert_eq!(spec.space(), 1);
        assert_eq!(spec.channel(), 5);
        assert_eq!(spec.service_id(), 200);
        assert!(spec.is_valid());
    }
}
