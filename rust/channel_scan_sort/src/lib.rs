//! TVTest のチャンネルスキャン結果リストビューのソート比較
//! (`src/ChannelScan.cpp` / `src/ChannelScan.h` の
//! `CChannelScan::CChannelListSort::CompareFunc`)を移植したクレート。
//!
//! 移植対象:
//! - 無名 enum の列種別(`COLUMN_NAME` 等、`ChannelScan.h:62-70`) → [`Column`]。
//! - 既定の列(`m_Column = COLUMN_CHANNELINDEX`、`ChannelScan.h:84`) → [`DEFAULT_COLUMN`]。
//! - `CompareFunc` 本体(`ChannelScan.cpp:172-214`)の比較ロジック → [`compare_channels`]。
//! - `COLUMN_NAME` の `::lstrcmpi` + `::lstrcmp` フォールバック
//!   (`ChannelScan.cpp:181-184`) → [`compare_channel_name`]。
//!
//! 対象外(Win32 / ListView / ダイアログ依存):
//! - `CChannelListSort::Sort` の `ListView_SortItems` 呼び出し(`ChannelScan.cpp:217-220`)。
//! - `CChannelListSort::UpdateChannelList` の ListView 操作(アイテムの並べ替え反映等)。
//! - `CScanSettingsDialog` 等のダイアログ全般。
//!
//! `COLUMN_SERVICEID`(`ChannelScan.cpp:194-199`)は原実装が
//! `GetAppClass().NetworkDefinition.GetNetworkTypeOrder(...)` を直接呼び出しているのに
//! 合わせ、本クレートも `tvtest_network_definition` に依存し
//! [`tvtest_network_definition::NetworkDefinition::get_network_type_order`] を
//! 内部で呼び出す設計とした(呼び出し側が事前計算した値を渡す設計にはしていない)。

#![forbid(unsafe_code)]

use std::cmp::Ordering;

use tvtest_channel_list::ChannelInfo;
use tvtest_network_definition::NetworkDefinition;

/// チャンネルスキャン結果リストビューの列種別。
/// 原実装の無名 enum(`ChannelScan.h:62-70`)。宣言順に 0 から連番。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Column {
    /// `COLUMN_NAME` = 0。
    Name = 0,
    /// `COLUMN_SERVICETYPE` = 1。
    ServiceType = 1,
    /// `COLUMN_CHANNELNAME` = 2。
    ChannelName = 2,
    /// `COLUMN_SERVICEID` = 3。
    ServiceId = 3,
    /// `COLUMN_REMOTECONTROLKEYID` = 4。
    RemoteControlKeyId = 4,
    /// `COLUMN_CHANNELINDEX` = 5。
    ChannelIndex = 5,
}

/// 既定の列。原実装 `m_Column = COLUMN_CHANNELINDEX`(`ChannelScan.h:84`)。
pub const DEFAULT_COLUMN: Column = Column::ChannelIndex;

/// チャンネル名の比較。原実装 `CompareFunc` の `COLUMN_NAME` ケース
/// (`ChannelScan.cpp:181-184`)。
///
/// `::lstrcmpi`(大小無視)で比較し、等しければ `::lstrcmp`(大小区別)でフォールバックする。
/// ASCII 範囲のみを大小無視で畳み込む自前実装(`tvtest_status_options` の
/// `is_equal_no_case_u16` と同様の方針)で近似する。非 ASCII 文字はそのままの値で比較する。
#[must_use]
pub fn compare_channel_name(name1: &str, name2: &str) -> Ordering {
    let ci = compare_ascii_case_insensitive(name1, name2);
    if ci == Ordering::Equal {
        name1.cmp(name2)
    } else {
        ci
    }
}

/// ASCII 範囲のみを大小無視で畳み込んで比較する。非 ASCII 文字はそのままの値で比較する。
fn compare_ascii_case_insensitive(a: &str, b: &str) -> Ordering {
    let mut ia = a.chars();
    let mut ib = b.chars();
    loop {
        match (ia.next(), ib.next()) {
            (Some(ca), Some(cb)) => {
                let la = to_ascii_lower_char(ca);
                let lb = to_ascii_lower_char(cb);
                match la.cmp(&lb) {
                    Ordering::Equal => continue,
                    other => return other,
                }
            }
            (Some(_), None) => return Ordering::Greater,
            (None, Some(_)) => return Ordering::Less,
            (None, None) => return Ordering::Equal,
        }
    }
}

fn to_ascii_lower_char(c: char) -> char {
    if c.is_ascii_uppercase() {
        c.to_ascii_lowercase()
    } else {
        c
    }
}

/// 2 つのチャンネル情報を比較する。原実装 `CompareFunc`(`ChannelScan.cpp:172-214`)。
///
/// `column` の値に応じて比較し、`descending` が `true` なら結果を反転する
/// (`ChannelScan.cpp:213` の `pThis->m_fDescending ? -Cmp : Cmp` 相当)。
///
/// `COLUMN_SERVICEID` の比較では、内部で
/// [`NetworkDefinition::get_network_type_order`] を呼び出す
/// (`ChannelScan.cpp:195-196` 相当)。
#[must_use]
pub fn compare_channels(
    ch1: &ChannelInfo,
    ch2: &ChannelInfo,
    column: Column,
    network_definition: &NetworkDefinition,
    descending: bool,
) -> Ordering {
    let cmp = match column {
        Column::Name => compare_channel_name(&ch1.name, &ch2.name),
        Column::ServiceType => cmp_i32(i32::from(ch1.service_type), i32::from(ch2.service_type)),
        Column::ChannelName | Column::ChannelIndex => {
            cmp_i32(ch1.channel_index, ch2.channel_index)
        }
        Column::ServiceId => {
            let order = network_definition.get_network_type_order(ch1.network_id, ch2.network_id);
            if order == 0 {
                cmp_i32(i32::from(ch1.service_id), i32::from(ch2.service_id))
            } else {
                cmp_i32(order, 0)
            }
        }
        Column::RemoteControlKeyId => cmp_i32(ch1.channel_no, ch2.channel_no),
    };

    if descending {
        cmp.reverse()
    } else {
        cmp
    }
}

/// `i32` 同士の差分に基づく比較(原実装の `int` 引き算による比較を `Ordering` 化したもの)。
/// 引き算そのものは行わず `Ord::cmp` を用いるため、オーバーフロー/アンダーフローの
/// 懸念がない。
fn cmp_i32(a: i32, b: i32) -> Ordering {
    a.cmp(&b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(name: &str, channel_index: i32, channel_no: i32, network_id: u16, service_id: u16, service_type: u8) -> ChannelInfo {
        let mut c = ChannelInfo::new(0, channel_index, channel_no, name);
        c.network_id = network_id;
        c.service_id = service_id;
        c.service_type = service_type;
        c
    }

    // ---- compare_channel_name ----

    #[test]
    fn name_case_insensitive_equal_falls_back_to_case_sensitive_ascending() {
        // "abc" と "ABC" は大小無視で等しい → 大小区別でフォールバック。
        // 'a'(0x61) > 'A'(0x41) なので "abc" > "ABC"。
        assert_eq!(compare_channel_name("abc", "ABC"), Ordering::Greater);
        assert_eq!(compare_channel_name("ABC", "abc"), Ordering::Less);
    }

    #[test]
    fn name_case_insensitive_equal_same_string_is_equal() {
        assert_eq!(compare_channel_name("abc", "abc"), Ordering::Equal);
    }

    #[test]
    fn name_case_insensitive_different_uses_case_insensitive_order() {
        // "abc" vs "Bcd": 大小無視で 'a' < 'b' なので Less。
        assert_eq!(compare_channel_name("abc", "Bcd"), Ordering::Less);
        assert_eq!(compare_channel_name("Bcd", "abc"), Ordering::Greater);
    }

    // ---- compare_channels: Column::Name ----

    #[test]
    fn compare_channels_name_ascending() {
        let a = ch("AAA", 0, 0, 0, 0, 0);
        let b = ch("BBB", 0, 0, 0, 0, 0);
        let def = NetworkDefinition::new();
        assert_eq!(compare_channels(&a, &b, Column::Name, &def, false), Ordering::Less);
        assert_eq!(compare_channels(&b, &a, Column::Name, &def, false), Ordering::Greater);
        assert_eq!(compare_channels(&a, &a, Column::Name, &def, false), Ordering::Equal);
    }

    #[test]
    fn compare_channels_name_descending_reverses() {
        let a = ch("AAA", 0, 0, 0, 0, 0);
        let b = ch("BBB", 0, 0, 0, 0, 0);
        let def = NetworkDefinition::new();
        assert_eq!(compare_channels(&a, &b, Column::Name, &def, true), Ordering::Greater);
        assert_eq!(compare_channels(&b, &a, Column::Name, &def, true), Ordering::Less);
    }

    // ---- compare_channels: Column::ServiceType ----

    #[test]
    fn compare_channels_service_type_positive_negative_zero() {
        let def = NetworkDefinition::new();
        let a = ch("A", 0, 0, 0, 0, 5);
        let b = ch("B", 0, 0, 0, 0, 1);
        assert_eq!(compare_channels(&a, &b, Column::ServiceType, &def, false), Ordering::Greater);
        assert_eq!(compare_channels(&b, &a, Column::ServiceType, &def, false), Ordering::Less);
        assert_eq!(compare_channels(&a, &a, Column::ServiceType, &def, false), Ordering::Equal);
    }

    #[test]
    fn compare_channels_service_type_descending_reverses() {
        let def = NetworkDefinition::new();
        let a = ch("A", 0, 0, 0, 0, 5);
        let b = ch("B", 0, 0, 0, 0, 1);
        assert_eq!(compare_channels(&a, &b, Column::ServiceType, &def, true), Ordering::Less);
    }

    // ---- compare_channels: Column::ChannelName (=ChannelIndex と同一ロジック) ----

    #[test]
    fn compare_channels_channel_name_uses_channel_index() {
        let def = NetworkDefinition::new();
        let a = ch("A", 10, 0, 0, 0, 0);
        let b = ch("B", 3, 0, 0, 0, 0);
        assert_eq!(compare_channels(&a, &b, Column::ChannelName, &def, false), Ordering::Greater);
        assert_eq!(compare_channels(&b, &a, Column::ChannelName, &def, false), Ordering::Less);
        assert_eq!(compare_channels(&a, &a, Column::ChannelName, &def, false), Ordering::Equal);
    }

    // ---- compare_channels: Column::ChannelIndex ----

    #[test]
    fn compare_channels_channel_index_positive_negative_zero() {
        let def = NetworkDefinition::new();
        let a = ch("A", 10, 0, 0, 0, 0);
        let b = ch("B", 3, 0, 0, 0, 0);
        assert_eq!(compare_channels(&a, &b, Column::ChannelIndex, &def, false), Ordering::Greater);
        assert_eq!(compare_channels(&b, &a, Column::ChannelIndex, &def, false), Ordering::Less);
        assert_eq!(compare_channels(&a, &a, Column::ChannelIndex, &def, false), Ordering::Equal);
    }

    #[test]
    fn compare_channels_channel_index_descending_reverses() {
        let def = NetworkDefinition::new();
        let a = ch("A", 10, 0, 0, 0, 0);
        let b = ch("B", 3, 0, 0, 0, 0);
        assert_eq!(compare_channels(&a, &b, Column::ChannelIndex, &def, true), Ordering::Less);
    }

    // ---- compare_channels: Column::RemoteControlKeyId ----

    #[test]
    fn compare_channels_remote_control_key_id_positive_negative_zero() {
        let def = NetworkDefinition::new();
        let a = ch("A", 0, 12, 0, 0, 0);
        let b = ch("B", 0, 4, 0, 0, 0);
        assert_eq!(compare_channels(&a, &b, Column::RemoteControlKeyId, &def, false), Ordering::Greater);
        assert_eq!(compare_channels(&b, &a, Column::RemoteControlKeyId, &def, false), Ordering::Less);
        assert_eq!(compare_channels(&a, &a, Column::RemoteControlKeyId, &def, false), Ordering::Equal);
    }

    #[test]
    fn compare_channels_remote_control_key_id_descending_reverses() {
        let def = NetworkDefinition::new();
        let a = ch("A", 0, 12, 0, 0, 0);
        let b = ch("B", 0, 4, 0, 0, 0);
        assert_eq!(compare_channels(&a, &b, Column::RemoteControlKeyId, &def, true), Ordering::Less);
    }

    // ---- compare_channels: Column::ServiceId ----

    #[test]
    fn compare_channels_service_id_same_network_type_falls_back_to_service_id() {
        // どちらも未登録 NID(Terrestrial 扱い)で種別順序が同じ → ServiceID フォールバック。
        let def = NetworkDefinition::new();
        let a = ch("A", 0, 0, 0x7880, 200, 0);
        let b = ch("B", 0, 0, 0x7881, 100, 0);
        assert_eq!(compare_channels(&a, &b, Column::ServiceId, &def, false), Ordering::Greater);
        assert_eq!(compare_channels(&b, &a, Column::ServiceId, &def, false), Ordering::Less);
    }

    #[test]
    fn compare_channels_service_id_same_network_id_and_service_id_is_equal() {
        let def = NetworkDefinition::new();
        let a = ch("A", 0, 0, 4, 101, 0);
        let b = ch("B", 0, 0, 4, 101, 0);
        assert_eq!(compare_channels(&a, &b, Column::ServiceId, &def, false), Ordering::Equal);
    }

    #[test]
    fn compare_channels_service_id_network_type_order_takes_priority() {
        // NID4=BS, NID6=CS。ServiceID の大小に関わらずネットワーク種別順序が優先される。
        let def = NetworkDefinition::new();
        let bs = ch("BS", 0, 0, 4, 9999, 0);
        let cs = ch("CS", 0, 0, 6, 1, 0);
        // BS(2) - CS(3) = -1 → Less。
        assert_eq!(compare_channels(&bs, &cs, Column::ServiceId, &def, false), Ordering::Less);
        assert_eq!(compare_channels(&cs, &bs, Column::ServiceId, &def, false), Ordering::Greater);
    }

    #[test]
    fn compare_channels_service_id_descending_reverses() {
        let def = NetworkDefinition::new();
        let a = ch("A", 0, 0, 0x7880, 200, 0);
        let b = ch("B", 0, 0, 0x7881, 100, 0);
        assert_eq!(compare_channels(&a, &b, Column::ServiceId, &def, true), Ordering::Less);
    }

    // ---- DEFAULT_COLUMN ----

    #[test]
    fn default_column_is_channel_index() {
        assert_eq!(DEFAULT_COLUMN, Column::ChannelIndex);
    }

    // ---- Column の判別値(宣言順に 0 から連番であることの確認) ----

    #[test]
    fn column_discriminants_match_original_enum_order() {
        assert_eq!(Column::Name as i32, 0);
        assert_eq!(Column::ServiceType as i32, 1);
        assert_eq!(Column::ChannelName as i32, 2);
        assert_eq!(Column::ServiceId as i32, 3);
        assert_eq!(Column::RemoteControlKeyId as i32, 4);
        assert_eq!(Column::ChannelIndex as i32, 5);
    }
}
