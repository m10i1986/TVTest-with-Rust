// TVTest の ChannelHistory.cpp / ChannelHistory.h の純粋ロジックを Rust へ移植したもの。
//
// チャンネルの「進む/戻る」履歴(CChannelHistory)と、最近見たチャンネルの
// LRU リスト(CRecentChannelList)を扱う。原実装は CChannelInfo / CTunerChannelInfo /
// CSettings / メニュー(HMENU)に依存するが、本クレートでは履歴ナビゲーションと
// 重複排除・LRU の純粋なリスト操作のみを移植する。
//
// CSettings I/O(ReadSettings/WriteSettings)とメニュー生成(SetMenu 等)は対象外。
// チャンネル同定に必要なフィールドだけを持つ軽量な TunerChannelInfo を定義する。
//
// チューナー名比較は原実装の IsEqualFileName(CompareStringOrdinal による大小無視)
// を ASCII 大小無視で近似する。

/// チャンネル同定に必要なフィールドを持つ軽量なチューナーチャンネル情報。
/// 原実装の CTunerChannelInfo のうち、履歴判定で参照されるものだけを保持する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunerChannelInfo {
    pub tuner_name: Vec<u16>,
    pub space: i32,
    pub channel_index: i32,
    pub network_id: u16,
    pub transport_stream_id: u16,
    pub service_id: u16,
    pub name: Vec<u16>,
}

/// チューナー名(ファイル名)の大小無視比較。原実装 IsEqualFileName の ASCII 近似。
fn is_equal_file_name(a: &[u16], b: &[u16]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b.iter()).all(|(&x, &y)| towlower_ascii(x) == towlower_ascii(y))
}

fn towlower_ascii(c: u16) -> u16 {
    if (u16::from(b'A')..=u16::from(b'Z')).contains(&c) {
        c + 32
    } else {
        c
    }
}

/// 進む/戻る用のチャンネル履歴。原実装 CChannelHistory。
#[derive(Debug, Clone)]
pub struct ChannelHistory {
    channel_list: Vec<TunerChannelInfo>,
    current_channel: i32,
    max_channel_history: i32,
}

impl ChannelHistory {
    /// 最大履歴数を指定して生成する。原実装の m_MaxChannelHistory に対応。
    pub fn new(max_channel_history: i32) -> Self {
        Self {
            channel_list: Vec::new(),
            current_channel: -1,
            max_channel_history,
        }
    }

    /// 履歴をクリアする。原実装 Clear:32。
    pub fn clear(&mut self) {
        self.channel_list.clear();
        self.current_channel = -1;
    }

    /// 現在のチャンネル位置(0 始まり。履歴なしは -1)。
    pub fn current_index(&self) -> i32 {
        self.current_channel
    }

    /// 履歴件数。
    pub fn len(&self) -> usize {
        self.channel_list.len()
    }

    /// 履歴が空か。
    pub fn is_empty(&self) -> bool {
        self.channel_list.is_empty()
    }

    /// 現在のチャンネルを設定する。原実装 SetCurrentChannel:39。
    ///
    /// 既に現在位置が同一チャンネル(チューナー名・channel_index・network_id・
    /// transport_stream_id・service_id が一致)なら何もせず true。
    /// それ以外は現在位置より後ろの履歴(進む側)を破棄して末尾に追加し、現在位置を進める。
    /// 履歴が最大数を超えたら先頭を捨てる。
    pub fn set_current_channel(&mut self, driver_name: &[u16], channel: &TunerChannelInfo) -> bool {
        if self.current_channel >= 0 {
            let cur = &self.channel_list[self.current_channel as usize];
            if is_equal_file_name(&cur.tuner_name, driver_name)
                && cur.channel_index == channel.channel_index
                && cur.network_id == channel.network_id
                && cur.transport_stream_id == channel.transport_stream_id
                && cur.service_id == channel.service_id
            {
                return true;
            }
        }

        // 現在位置より後ろ(進む側)の履歴を破棄する。
        while self.channel_list.len() as i32 - 1 > self.current_channel {
            self.channel_list.pop();
        }

        // driver_name を tuner_name として持つ複製を末尾へ追加(原実装の
        // CTunerChannelInfo(*pChannelInfo, pszDriverName) 構築に相当)。
        let mut info = channel.clone();
        info.tuner_name = driver_name.to_vec();
        self.channel_list.push(info);
        self.current_channel += 1;

        if self.channel_list.len() as i32 > self.max_channel_history {
            self.channel_list.remove(0);
            self.current_channel -= 1;
        }

        true
    }

    /// 一つ進む。進めなければ None。原実装 Forward:71。
    pub fn forward(&mut self) -> Option<&TunerChannelInfo> {
        if self.current_channel + 1 >= self.channel_list.len() as i32 {
            return None;
        }
        self.current_channel += 1;
        Some(&self.channel_list[self.current_channel as usize])
    }

    /// 一つ戻る。戻れなければ None。原実装 Backward:79。
    pub fn backward(&mut self) -> Option<&TunerChannelInfo> {
        if self.current_channel < 1 {
            return None;
        }
        self.current_channel -= 1;
        Some(&self.channel_list[self.current_channel as usize])
    }

    /// 現在のチャンネル情報(履歴なしは None)。
    pub fn current(&self) -> Option<&TunerChannelInfo> {
        if self.current_channel < 0 {
            return None;
        }
        self.channel_list.get(self.current_channel as usize)
    }
}

/// 最近見たチャンネルの LRU リスト。原実装 CRecentChannelList。
#[derive(Debug, Clone)]
pub struct RecentChannelList {
    channel_list: Vec<TunerChannelInfo>,
    max_channel_history: i32,
}

impl RecentChannelList {
    /// 最大保持数を指定して生成する。
    pub fn new(max_channel_history: i32) -> Self {
        Self {
            channel_list: Vec::new(),
            max_channel_history,
        }
    }

    /// 件数。原実装 NumChannels:95。
    pub fn num_channels(&self) -> i32 {
        self.channel_list.len() as i32
    }

    /// クリアする。原実装 Clear:101。
    pub fn clear(&mut self) {
        self.channel_list.clear();
    }

    /// インデックス指定で取得(範囲外は None)。原実装 GetChannelInfo:107。
    pub fn get_channel_info(&self, index: i32) -> Option<&TunerChannelInfo> {
        if index < 0 || index >= self.num_channels() {
            return None;
        }
        Some(&self.channel_list[index as usize])
    }

    /// チャンネルを追加する。原実装 Add:115。
    ///
    /// 既存に「チューナー名・space・channel_index・service_id が一致」する項目があれば:
    ///   - それが先頭で network_id も一致するなら、何もせず true(既に最新)。
    ///   - そうでなければその項目を削除して、先頭へ挿入し直す(LRU 更新)。
    /// 最大数を超えたら末尾を捨てる。
    pub fn add(&mut self, driver_name: &[u16], channel: &TunerChannelInfo) -> bool {
        if let Some(pos) = self.channel_list.iter().position(|e| {
            is_equal_file_name(&e.tuner_name, driver_name)
                && e.space == channel.space
                && e.channel_index == channel.channel_index
                && e.service_id == channel.service_id
        }) {
            // 先頭でかつ network_id も一致 → 既に最新なので何もしない。
            if pos == 0 && self.channel_list[0].network_id == channel.network_id {
                return true;
            }
            self.channel_list.remove(pos);
        }

        let mut info = channel.clone();
        info.tuner_name = driver_name.to_vec();
        self.channel_list.insert(0, info);

        if self.channel_list.len() as i32 > self.max_channel_history {
            self.channel_list.pop();
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn ch(tuner: &str, space: i32, index: i32, nid: u16, sid: u16) -> TunerChannelInfo {
        TunerChannelInfo {
            tuner_name: w(tuner),
            space,
            channel_index: index,
            network_id: nid,
            transport_stream_id: 0,
            service_id: sid,
            name: w("ch"),
        }
    }

    #[test]
    fn test_history_basic_navigation() {
        let mut h = ChannelHistory::new(10);
        assert!(h.current().is_none());
        assert_eq!(h.current_index(), -1);

        let c1 = ch("A.dll", 0, 1, 4, 101);
        let c2 = ch("A.dll", 0, 2, 4, 102);
        let c3 = ch("A.dll", 0, 3, 4, 103);

        h.set_current_channel(&w("A.dll"), &c1);
        h.set_current_channel(&w("A.dll"), &c2);
        h.set_current_channel(&w("A.dll"), &c3);
        assert_eq!(h.len(), 3);
        assert_eq!(h.current_index(), 2);
        assert_eq!(h.current().unwrap().channel_index, 3);

        // 戻る。
        assert_eq!(h.backward().unwrap().channel_index, 2);
        assert_eq!(h.backward().unwrap().channel_index, 1);
        // これ以上戻れない。
        assert!(h.backward().is_none());
        assert_eq!(h.current_index(), 0);

        // 進む。
        assert_eq!(h.forward().unwrap().channel_index, 2);
        assert_eq!(h.forward().unwrap().channel_index, 3);
        assert!(h.forward().is_none());
    }

    #[test]
    fn test_history_same_channel_noop() {
        let mut h = ChannelHistory::new(10);
        let c1 = ch("A.dll", 0, 1, 4, 101);
        h.set_current_channel(&w("A.dll"), &c1);
        // 同一チャンネルを再設定 → 履歴は増えない。
        h.set_current_channel(&w("A.dll"), &c1);
        assert_eq!(h.len(), 1);
        assert_eq!(h.current_index(), 0);
    }

    #[test]
    fn test_history_branch_discards_forward() {
        let mut h = ChannelHistory::new(10);
        let c1 = ch("A.dll", 0, 1, 4, 101);
        let c2 = ch("A.dll", 0, 2, 4, 102);
        let c3 = ch("A.dll", 0, 3, 4, 103);
        h.set_current_channel(&w("A.dll"), &c1);
        h.set_current_channel(&w("A.dll"), &c2);
        h.set_current_channel(&w("A.dll"), &c3);
        // 2 つ戻る → current=0。
        h.backward();
        h.backward();
        assert_eq!(h.current_index(), 0);
        // 新規設定 → 進む側(c2, c3)が破棄され c4 が追加される。
        let c4 = ch("A.dll", 0, 4, 4, 104);
        h.set_current_channel(&w("A.dll"), &c4);
        assert_eq!(h.len(), 2); // c1, c4
        assert_eq!(h.current_index(), 1);
        assert_eq!(h.current().unwrap().channel_index, 4);
        assert!(h.forward().is_none());
    }

    #[test]
    fn test_history_max_overflow_drops_front() {
        let mut h = ChannelHistory::new(2);
        h.set_current_channel(&w("A.dll"), &ch("A.dll", 0, 1, 4, 101));
        h.set_current_channel(&w("A.dll"), &ch("A.dll", 0, 2, 4, 102));
        h.set_current_channel(&w("A.dll"), &ch("A.dll", 0, 3, 4, 103));
        // 最大 2。先頭(c1)が捨てられ c2, c3 が残る。current は調整される。
        assert_eq!(h.len(), 2);
        assert_eq!(h.current_index(), 1);
        assert_eq!(h.current().unwrap().channel_index, 3);
        // 戻ると c2。
        assert_eq!(h.backward().unwrap().channel_index, 2);
        assert!(h.backward().is_none());
    }

    #[test]
    fn test_recent_add_moves_to_front() {
        let mut r = RecentChannelList::new(10);
        r.add(&w("A.dll"), &ch("A.dll", 0, 1, 4, 101));
        r.add(&w("A.dll"), &ch("A.dll", 0, 2, 4, 102));
        r.add(&w("A.dll"), &ch("A.dll", 0, 3, 4, 103));
        // 先頭が最新(c3)。
        assert_eq!(r.num_channels(), 3);
        assert_eq!(r.get_channel_info(0).unwrap().channel_index, 3);
        assert_eq!(r.get_channel_info(2).unwrap().channel_index, 1);

        // 既存の c1 を再追加 → 先頭へ移動。
        r.add(&w("A.dll"), &ch("A.dll", 0, 1, 4, 101));
        assert_eq!(r.num_channels(), 3); // 件数は増えない。
        assert_eq!(r.get_channel_info(0).unwrap().channel_index, 1);
        assert_eq!(r.get_channel_info(1).unwrap().channel_index, 3);
    }

    #[test]
    fn test_recent_add_same_at_front_noop() {
        let mut r = RecentChannelList::new(10);
        r.add(&w("A.dll"), &ch("A.dll", 0, 1, 4, 101));
        // 先頭と完全一致(network_id も)→ 何もしない。
        r.add(&w("A.dll"), &ch("A.dll", 0, 1, 4, 101));
        assert_eq!(r.num_channels(), 1);
    }

    #[test]
    fn test_recent_add_front_same_but_nid_differs_reinserts() {
        let mut r = RecentChannelList::new(10);
        r.add(&w("A.dll"), &ch("A.dll", 0, 1, 4, 101));
        // 先頭と space/index/sid 一致だが network_id 違い → 削除して先頭へ挿入し直し。
        // 件数は 1 のまま、network_id が更新される。
        r.add(&w("A.dll"), &ch("A.dll", 0, 1, 5, 101));
        assert_eq!(r.num_channels(), 1);
        assert_eq!(r.get_channel_info(0).unwrap().network_id, 5);
    }

    #[test]
    fn test_recent_max_overflow_drops_back() {
        let mut r = RecentChannelList::new(2);
        r.add(&w("A.dll"), &ch("A.dll", 0, 1, 4, 101));
        r.add(&w("A.dll"), &ch("A.dll", 0, 2, 4, 102));
        r.add(&w("A.dll"), &ch("A.dll", 0, 3, 4, 103));
        // 最大 2。末尾(最古=c1)が捨てられ c3, c2 が残る。
        assert_eq!(r.num_channels(), 2);
        assert_eq!(r.get_channel_info(0).unwrap().channel_index, 3);
        assert_eq!(r.get_channel_info(1).unwrap().channel_index, 2);
    }

    #[test]
    fn test_recent_get_out_of_range() {
        let r = RecentChannelList::new(10);
        assert!(r.get_channel_info(0).is_none());
        assert!(r.get_channel_info(-1).is_none());
    }
}
