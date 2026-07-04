//! TVTest の `TaskbarSharedProperties.cpp` / `TaskbarSharedProperties.h` の純粋部分を移植したもの。
//!
//! タスクバー連携プロセスと共有メモリ経由でやり取りする「最近視聴したチャンネル」情報の
//! ヘッダー検証と、`ChannelInfo`(原実装の `CTunerChannelInfo` 相当)⇔ 固定レイアウトの
//! `RecentChannelInfo` の相互変換を扱う。
//!
//! `CSharedMemory` によるメモリのマップ・ロック・タイムアウト待機自体はプラットフォーム
//! 依存のため対象外。呼び出し側が共有メモリのバイト列を本クレートの構造体として解釈する
//! ことを想定する。

/// 共有メモリに保持できる最近視聴チャンネルの最大数。原実装 `MAX_RECENT_CHANNELS`。
pub const MAX_RECENT_CHANNELS: u32 = 20;

/// チャンネル名の最大文字数(NUL終端含む)。原実装 `MAX_CHANNEL_NAME`(ChannelList.h:33)。
pub const MAX_CHANNEL_NAME: usize = 64;

/// チューナー名(ファイルパス)の最大文字数(NUL終端含む)。原実装 `MAX_PATH`。
pub const MAX_PATH_LEN: usize = 260;

/// 共有メモリ先頭のヘッダー。原実装 `SharedInfoHeader`(TaskbarSharedProperties.h:45-53)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SharedInfoHeader {
    pub size: u32,
    pub version: u32,
    pub max_recent_channels: u32,
    pub recent_channel_count: u32,
}

impl SharedInfoHeader {
    /// 原実装 `SharedInfoHeader::VERSION_CURRENT`。
    pub const VERSION_CURRENT: u32 = 0;

    /// 新規作成時のヘッダーを構築する。原実装 `Open` の `!fExists` 分岐(TaskbarSharedProperties.cpp:59-62)。
    pub fn new_for_create(header_size: u32, recent_channel_count: u32) -> Self {
        Self {
            size: header_size,
            version: Self::VERSION_CURRENT,
            max_recent_channels: MAX_RECENT_CHANNELS,
            recent_channel_count,
        }
    }

    /// 既存共有メモリのヘッダー検証。原実装 `ValidateHeader`(TaskbarSharedProperties.cpp:174-179)。
    pub fn validate(&self, header_size: u32) -> bool {
        self.size == header_size && self.version == Self::VERSION_CURRENT
    }
}

/// チャンネル同定・表示に必要な情報。原実装 `CTunerChannelInfo`(ChannelList.h:36-90)相当。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelInfo {
    pub space: i32,
    pub channel_index: i32,
    pub channel_no: i32,
    pub physical_channel: i32,
    pub network_id: u16,
    pub transport_stream_id: u16,
    pub service_id: u16,
    pub service_type: u8,
    pub name: Vec<u16>,
    pub tuner_name: Vec<u16>,
}

/// 共有メモリ上の固定レイアウトチャンネル情報。原実装 `RecentChannelInfo`(TaskbarSharedProperties.h:55-68)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentChannelInfo {
    pub space: i32,
    pub channel_index: i32,
    pub channel_no: i32,
    pub physical_channel: i32,
    pub network_id: u16,
    pub transport_stream_id: u16,
    pub service_id: u16,
    pub service_type: u8,
    /// 固定長 `MAX_CHANNEL_NAME` (NUL終端含む)へ切り詰められたチャンネル名。
    pub channel_name: [u16; MAX_CHANNEL_NAME],
    /// 固定長 `MAX_PATH_LEN` (NUL終端含む)へ切り詰められたチューナー名。
    pub tuner_name: [u16; MAX_PATH_LEN],
}

/// 固定長 UTF-16 バッファへ切り詰めてコピーする。原実装 `StringCopy`(NUL終端含めてバッファ長を
/// 超えない。`wcsncpy_s(..., _TRUNCATE)` 相当: 収まらない場合は末尾を切り詰めて必ずNUL終端する)。
fn copy_truncated<const N: usize>(src: &[u16]) -> [u16; N] {
    let mut buf = [0u16; N];
    let copy_len = src.len().min(N - 1);
    buf[..copy_len].copy_from_slice(&src[..copy_len]);
    buf
}

/// NUL終端(またはバッファ末尾)までの文字列部分を取り出す。
fn trim_nul(buf: &[u16]) -> &[u16] {
    match buf.iter().position(|&c| c == 0) {
        Some(pos) => &buf[..pos],
        None => buf,
    }
}

impl RecentChannelInfo {
    /// `ChannelInfo` から変換する。原実装 `TunerChannelInfoToRecentChannelInfo`
    /// (TaskbarSharedProperties.cpp:206-219)。
    pub fn from_channel_info(info: &ChannelInfo) -> Self {
        Self {
            space: info.space,
            channel_index: info.channel_index,
            channel_no: info.channel_no,
            physical_channel: info.physical_channel,
            network_id: info.network_id,
            transport_stream_id: info.transport_stream_id,
            service_id: info.service_id,
            service_type: info.service_type,
            channel_name: copy_truncated(&info.name),
            tuner_name: copy_truncated(&info.tuner_name),
        }
    }

    /// `ChannelInfo` へ変換する。原実装 `ReadRecentChannelList` の1要素分
    /// (TaskbarSharedProperties.cpp:182-203)。
    pub fn to_channel_info(&self) -> ChannelInfo {
        ChannelInfo {
            space: self.space,
            channel_index: self.channel_index,
            channel_no: self.channel_no,
            physical_channel: self.physical_channel,
            network_id: self.network_id,
            transport_stream_id: self.transport_stream_id,
            service_id: self.service_id,
            service_type: self.service_type,
            name: trim_nul(&self.channel_name).to_vec(),
            tuner_name: trim_nul(&self.tuner_name).to_vec(),
        }
    }
}

/// 新規作成時、`recent_channels` (末尾が最新)から共有メモリへ書き込む順序のリストを構築する。
/// 原実装 `Open` の `!fExists` 分岐(TaskbarSharedProperties.cpp:64-80): 入力の末尾(最新)から
/// 順に読み、`MAX_RECENT_CHANNELS` を超える分は古い方(先頭側)を切り捨てて、共有メモリ配列の
/// 先頭(index 0)には最新のものを置く。
pub fn build_initial_channel_list(recent_channels: &[ChannelInfo]) -> Vec<RecentChannelInfo> {
    let channel_count = (recent_channels.len() as u32).min(MAX_RECENT_CHANNELS) as usize;

    (0..channel_count)
        .map(|i| {
            // 原実装は GetChannelInfo(ChannelCount - 1 - i) で末尾から取得する。
            let src = &recent_channels[recent_channels.len() - 1 - i];
            RecentChannelInfo::from_channel_info(src)
        })
        .collect()
}

/// 共有メモリの `RecentChannelInfo` 配列を、原実装の並び順のまま `ChannelInfo` へ変換する。
/// 原実装 `ReadRecentChannelList`(TaskbarSharedProperties.cpp:182-203)。
pub fn read_recent_channel_list(stored: &[RecentChannelInfo]) -> Vec<ChannelInfo> {
    stored
        .iter()
        .map(RecentChannelInfo::to_channel_info)
        .collect()
}

/// `CRecentChannelList::Add` 相当の LRU 追加を模擬した「最近視聴リスト(末尾が最新)」に
/// 新規チャンネルを追加した後の共有メモリ書き込み順序を構築する。
/// 原実装 `AddRecentChannel`(TaskbarSharedProperties.cpp:126-155): 読み出した
/// `CRecentChannelList` へ追加後、`MAX_RECENT_CHANNELS` を超える分は切り捨て、末尾から
/// 順に書き込む(index 0 が最新)。
///
/// `updated_recent_channels` は呼び出し側で `CRecentChannelList::Add` 相当のロジック
/// (LRU 重複排除、`channel_history` クレート等)を適用した後の「末尾が最新」のリストを渡す。
pub fn build_updated_channel_list(
    updated_recent_channels: &[ChannelInfo],
) -> Vec<RecentChannelInfo> {
    build_initial_channel_list(updated_recent_channels)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn ch(index: i32, name: &str, tuner: &str) -> ChannelInfo {
        ChannelInfo {
            space: 0,
            channel_index: index,
            channel_no: index,
            physical_channel: 13 + index,
            network_id: 4,
            transport_stream_id: 5,
            service_id: 101,
            service_type: 1,
            name: w(name),
            tuner_name: w(tuner),
        }
    }

    #[test]
    fn header_validate_ok() {
        let header = SharedInfoHeader::new_for_create(16, 3);
        assert!(header.validate(16));
        assert_eq!(header.version, SharedInfoHeader::VERSION_CURRENT);
        assert_eq!(header.max_recent_channels, MAX_RECENT_CHANNELS);
        assert_eq!(header.recent_channel_count, 3);
    }

    #[test]
    fn header_validate_rejects_wrong_size() {
        let header = SharedInfoHeader::new_for_create(16, 0);
        assert!(!header.validate(20));
    }

    #[test]
    fn header_validate_rejects_wrong_version() {
        let mut header = SharedInfoHeader::new_for_create(16, 0);
        header.version = 1;
        assert!(!header.validate(16));
    }

    #[test]
    fn roundtrip_channel_info() {
        let info = ch(1, "NHK総合", "BonDriver_Sample.dll");
        let stored = RecentChannelInfo::from_channel_info(&info);
        let back = stored.to_channel_info();
        assert_eq!(back, info);
    }

    #[test]
    fn truncates_long_name_and_nul_terminates() {
        let long_name: String = "あ".repeat(MAX_CHANNEL_NAME + 10);
        let info = ch(1, &long_name, "tuner.dll");
        let stored = RecentChannelInfo::from_channel_info(&info);
        // NUL終端が必ずある(バッファ末尾より前)。
        assert!(stored.channel_name.contains(&0));
        // 切り詰め後の長さは MAX_CHANNEL_NAME - 1 以下。
        let back = stored.to_channel_info();
        assert!(back.name.len() < MAX_CHANNEL_NAME);
        assert_eq!(back.name, w(&long_name)[..MAX_CHANNEL_NAME - 1]);
    }

    #[test]
    fn truncates_long_tuner_name() {
        let long_tuner: String = "C:\\path\\".to_string() + &"x".repeat(MAX_PATH_LEN);
        let info = ch(1, "ch", &long_tuner);
        let stored = RecentChannelInfo::from_channel_info(&info);
        let back = stored.to_channel_info();
        assert!(back.tuner_name.len() < MAX_PATH_LEN);
    }

    #[test]
    fn empty_name_roundtrips_to_empty() {
        let info = ch(1, "", "");
        let stored = RecentChannelInfo::from_channel_info(&info);
        let back = stored.to_channel_info();
        assert!(back.name.is_empty());
        assert!(back.tuner_name.is_empty());
    }

    #[test]
    fn build_initial_channel_list_reverses_order_and_caps() {
        // 入力は「末尾が最新」の CRecentChannelList 相当(古い順)。
        let recent = vec![ch(1, "ch1", "t"), ch(2, "ch2", "t"), ch(3, "ch3", "t")];
        let built = build_initial_channel_list(&recent);
        assert_eq!(built.len(), 3);
        // index 0 が最新(ch3)。
        assert_eq!(built[0].channel_index, 3);
        assert_eq!(built[1].channel_index, 2);
        assert_eq!(built[2].channel_index, 1);
    }

    #[test]
    fn build_initial_channel_list_caps_at_max() {
        let recent: Vec<ChannelInfo> = (0..(MAX_RECENT_CHANNELS + 5))
            .map(|i| ch(i as i32, "ch", "t"))
            .collect();
        let built = build_initial_channel_list(&recent);
        assert_eq!(built.len(), MAX_RECENT_CHANNELS as usize);
        // 最新(末尾)から MAX_RECENT_CHANNELS 件が残る。index 0 が一番新しい。
        let last_index = recent.len() as i32 - 1;
        assert_eq!(built[0].channel_index, last_index);
        assert_eq!(
            built[MAX_RECENT_CHANNELS as usize - 1].channel_index,
            last_index - (MAX_RECENT_CHANNELS as i32 - 1)
        );
    }

    #[test]
    fn build_initial_channel_list_empty() {
        let built = build_initial_channel_list(&[]);
        assert!(built.is_empty());
    }

    #[test]
    fn read_recent_channel_list_preserves_order() {
        let recent = vec![ch(1, "ch1", "t"), ch(2, "ch2", "t")];
        let built = build_initial_channel_list(&recent);
        let read_back = read_recent_channel_list(&built);
        assert_eq!(read_back.len(), 2);
        assert_eq!(read_back[0].channel_index, 2);
        assert_eq!(read_back[1].channel_index, 1);
    }

    #[test]
    fn build_updated_channel_list_matches_initial_semantics() {
        let updated = vec![ch(1, "ch1", "t"), ch(2, "ch2", "t")];
        let built = build_updated_channel_list(&updated);
        assert_eq!(built[0].channel_index, 2);
        assert_eq!(built[1].channel_index, 1);
    }
}
