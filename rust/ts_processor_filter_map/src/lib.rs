//! TVTest の TS プロセッサ(録画補正フィルタ)チューナー別マッピング
//! (`src/TSProcessorManager.cpp` / `src/TSProcessorManager.h` の `CTSProcessorSettings` /
//! `TunerFilterInfo`)の純粋ロジックを移植したクレート。
//!
//! 移植対象:
//! - `FilterInfo`(`TSProcessorManager.h:38-45`)、`TunerFilterInfo`
//!   (`TSProcessorManager.h:47-64`、`NID_INVALID`/`TSID_INVALID`/`SID_INVALID` =
//!   `0xFFFF` を `None` として表現)。
//! - `GetTunerFilterInfo`(`TSProcessorManager.cpp:696-720`。チューナー名の
//!   `PathFindFileName`+`PathMatchSpec` 照合と NID/TSID/SID 条件の AND 判定)。
//! - `IsTunerFilterMapEnabled`(`TSProcessorManager.cpp:723-738`)。
//! - `OnTunerChange` の純粋部分(`TSProcessorManager.cpp:435-479`。新旧チューナーの
//!   `TunerFilterInfo` 解決結果から「フィルタを閉じるべきか」「モジュールを
//!   アンロードすべきか」を判定する部分。実際の `CloseFilter`/`UnloadModule` 呼び出しは
//!   対象外)。
//!
//! 対象外(Win32 / CSettings / COM 依存):
//! - `ReadSettings` / `WriteSettings` の `CSettings` I/O 本体。
//! - `CTSProcessor` / `CCoreEngine` によるフィルタの生成・破棄・GUID 取得・COM 操作
//!   (`OpenFilter`/`CloseFilter`/`UnloadModule`/`OnTunerOpened` 等)。
//! - `PathMatchSpec`/`PathFindFileName`/`IsEqualFileName` は `tvtest_driver_manager`
//!   クレートの実装を再利用する。

#![forbid(unsafe_code)]

use tvtest_driver_manager::{is_equal_file_name, path_find_file_name, path_match_spec};

/// `FilterInfo`(`TSProcessorManager.h:38-45`)。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilterInfo {
    pub module: String,
    pub device: String,
    pub filter: String,
}

/// `TunerFilterInfo`(`TSProcessorManager.h:47-64`)。
///
/// 原実装の `NetworkID`/`TransportStreamID`/`ServiceID` は `WORD` で `0xFFFF` を
/// 「無効値(条件を課さない)」として使うが、本クレートでは `Option<u16>` で表現する
/// (`None` が `IsXxxEnabled() == false` に相当)。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TunerFilterInfo {
    pub filter: FilterInfo,
    pub enable: bool,
    pub enable_processing: bool,
    pub tuner: String,
    pub network_id: Option<u16>,
    pub transport_stream_id: Option<u16>,
    pub service_id: Option<u16>,
}

impl TunerFilterInfo {
    #[must_use]
    pub fn new(tuner: impl Into<String>) -> Self {
        Self {
            filter: FilterInfo::default(),
            enable: true,
            enable_processing: true,
            tuner: tuner.into(),
            network_id: None,
            transport_stream_id: None,
            service_id: None,
        }
    }
}

/// `GetTunerFilterInfo`(`TSProcessorManager.cpp:696-720`)。
///
/// `tuner` が空なら `None`。`map` を先頭から線形検索し、以下をすべて満たす最初の要素を返す:
/// - `entry.tuner` が空、またはファイル名部分 (`path_find_file_name`) が
///   `path_match_spec` で一致、または `tuner` 自体が `path_match_spec` で一致
///   (フルパス指定パターン対応、`tuner != ファイル名部分` のときのみ判定)。
/// - `network_id`/`transport_stream_id`/`service_id` それぞれ、`entry` 側が `None`
///   (条件なし)か、値が一致すること。
#[must_use]
pub fn get_tuner_filter_info<'a>(
    map: &'a [TunerFilterInfo],
    tuner: &str,
    network_id: Option<u16>,
    transport_stream_id: Option<u16>,
    service_id: Option<u16>,
) -> Option<&'a TunerFilterInfo> {
    if tuner.is_empty() {
        return None;
    }

    let name = path_find_file_name(tuner);

    map.iter().find(|e| {
        let tuner_matches = e.tuner.is_empty()
            || path_match_spec(name, &e.tuner)
            || (tuner != name && path_match_spec(tuner, &e.tuner));

        tuner_matches
            && e.network_id.is_none_or(|id| Some(id) == network_id)
            && e.transport_stream_id.is_none_or(|id| Some(id) == transport_stream_id)
            && e.service_id.is_none_or(|id| Some(id) == service_id)
    })
}

/// `IsTunerFilterMapEnabled`(`TSProcessorManager.cpp:723-738`)。
///
/// マップが空なら `false`。有効(`enable == true`)かつ、チューナー名か
/// NID/TSID/SID のいずれかの条件を持つ要素が 1 つでもあれば `true`。
#[must_use]
pub fn is_tuner_filter_map_enabled(map: &[TunerFilterInfo]) -> bool {
    if map.is_empty() {
        return false;
    }

    map.iter().any(|e| {
        e.enable
            && (!e.tuner.is_empty()
                || e.network_id.is_some()
                || e.transport_stream_id.is_some()
                || e.service_id.is_some())
    })
}

/// チューナー切り替え時に取るべきアクション(`OnTunerChange` の判定結果、
/// `TSProcessorManager.cpp:457-478`)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TunerChangeAction {
    /// 新チューナー側でこの TS プロセッサの処理を無効化する
    /// (`fEnableProcessing == false` のため `CloseFilter` のみ実行し、以降の判定は行わない)。
    Disable,
    /// フィルタとモジュールをそのまま維持する。
    Keep,
    /// フィルタを閉じる(`Device`/`Filter` が大小無視で不一致)。
    CloseFilter,
    /// モジュールをアンロードする(`Module` が `IsEqualFileName` で不一致)。
    UnloadModule,
    /// フィルタを閉じ、かつモジュールもアンロードする(両方不一致)。
    CloseFilterAndUnloadModule,
}

/// `OnTunerChange` の 1 プロセッサ分の判定ロジック(`TSProcessorManager.cpp:453-478`)。
///
/// `old_tuner_info`/`new_tuner_info` は `get_tuner_filter_info` の呼び出し結果
/// (旧チューナー名/新チューナー名それぞれで検索したもの)を渡す。`default_filter` は
/// `CTSProcessorSettings::m_DefaultFilter`。
#[must_use]
pub fn decide_tuner_change_action(
    old_tuner_info: Option<&TunerFilterInfo>,
    new_tuner_info: Option<&TunerFilterInfo>,
    default_filter: &FilterInfo,
) -> TunerChangeAction {
    let new_filter = match new_tuner_info {
        Some(info) if info.enable => {
            if !info.enable_processing {
                return TunerChangeAction::Disable;
            }
            &info.filter
        }
        _ => default_filter,
    };

    let old_filter = match old_tuner_info {
        Some(info) if info.enable => {
            if !info.enable_processing {
                return TunerChangeAction::Keep;
            }
            &info.filter
        }
        _ => default_filter,
    };

    let close_filter = !old_filter.device.eq_ignore_ascii_case(&new_filter.device)
        || !old_filter.filter.eq_ignore_ascii_case(&new_filter.filter);
    let unload_module = !is_equal_file_name(&old_filter.module, &new_filter.module);

    match (close_filter, unload_module) {
        (true, true) => TunerChangeAction::CloseFilterAndUnloadModule,
        (true, false) => TunerChangeAction::CloseFilter,
        (false, true) => TunerChangeAction::UnloadModule,
        (false, false) => TunerChangeAction::Keep,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filter(module: &str, device: &str, filter: &str) -> FilterInfo {
        FilterInfo {
            module: module.to_string(),
            device: device.to_string(),
            filter: filter.to_string(),
        }
    }

    #[test]
    fn get_tuner_filter_info_empty_tuner_returns_none() {
        let map = vec![TunerFilterInfo::new("")];
        assert!(get_tuner_filter_info(&map, "", None, None, None).is_none());
    }

    #[test]
    fn get_tuner_filter_info_matches_by_wildcard() {
        let map = vec![TunerFilterInfo::new("BonDriver_*.dll")];
        let found = get_tuner_filter_info(&map, "C:\\BonDriver\\BonDriver_PT3.dll", None, None, None);
        assert!(found.is_some());
    }

    #[test]
    fn get_tuner_filter_info_no_match_returns_none() {
        let map = vec![TunerFilterInfo::new("BonDriver_Other.dll")];
        let found = get_tuner_filter_info(&map, "BonDriver_PT3.dll", None, None, None);
        assert!(found.is_none());
    }

    #[test]
    fn get_tuner_filter_info_empty_tuner_field_matches_any() {
        let map = vec![TunerFilterInfo::new("")];
        let found = get_tuner_filter_info(&map, "AnyTuner.dll", None, None, None);
        assert!(found.is_some());
    }

    #[test]
    fn get_tuner_filter_info_network_id_condition() {
        let mut entry = TunerFilterInfo::new("");
        entry.network_id = Some(4);
        let map = vec![entry];

        assert!(get_tuner_filter_info(&map, "Tuner.dll", Some(4), None, None).is_some());
        assert!(get_tuner_filter_info(&map, "Tuner.dll", Some(7), None, None).is_none());
        assert!(get_tuner_filter_info(&map, "Tuner.dll", None, None, None).is_none());
    }

    #[test]
    fn get_tuner_filter_info_all_conditions_must_match() {
        let mut entry = TunerFilterInfo::new("Tuner.dll");
        entry.network_id = Some(4);
        entry.transport_stream_id = Some(10);
        entry.service_id = Some(101);
        let map = vec![entry];

        assert!(get_tuner_filter_info(&map, "Tuner.dll", Some(4), Some(10), Some(101)).is_some());
        assert!(get_tuner_filter_info(&map, "Tuner.dll", Some(4), Some(10), Some(999)).is_none());
    }

    #[test]
    fn get_tuner_filter_info_returns_first_match() {
        let map = vec![TunerFilterInfo::new(""), TunerFilterInfo::new("Other.dll")];
        let found = get_tuner_filter_info(&map, "Tuner.dll", None, None, None).unwrap();
        assert_eq!(found.tuner, "");
    }

    #[test]
    fn is_tuner_filter_map_enabled_empty_map() {
        assert!(!is_tuner_filter_map_enabled(&[]));
    }

    #[test]
    fn is_tuner_filter_map_enabled_disabled_entry() {
        let mut entry = TunerFilterInfo::new("Tuner.dll");
        entry.enable = false;
        assert!(!is_tuner_filter_map_enabled(&[entry]));
    }

    #[test]
    fn is_tuner_filter_map_enabled_entry_with_no_condition() {
        // Tuner も NID/TSID/SID も無効 → 条件を課していないので enabled 扱いにならない
        let entry = TunerFilterInfo {
            tuner: String::new(),
            ..TunerFilterInfo::new("")
        };
        assert!(!is_tuner_filter_map_enabled(&[entry]));
    }

    #[test]
    fn is_tuner_filter_map_enabled_true_with_tuner_condition() {
        let entry = TunerFilterInfo::new("Tuner.dll");
        assert!(is_tuner_filter_map_enabled(&[entry]));
    }

    #[test]
    fn is_tuner_filter_map_enabled_true_with_nid_condition() {
        let mut entry = TunerFilterInfo::new("");
        entry.network_id = Some(4);
        assert!(is_tuner_filter_map_enabled(&[entry]));
    }

    #[test]
    fn decide_tuner_change_action_new_disabled_processing() {
        let mut new_info = TunerFilterInfo::new("New.dll");
        new_info.enable_processing = false;
        let default_filter = FilterInfo::default();
        let action = decide_tuner_change_action(None, Some(&new_info), &default_filter);
        assert_eq!(action, TunerChangeAction::Disable);
    }

    #[test]
    fn decide_tuner_change_action_old_disabled_processing_keeps() {
        let mut old_info = TunerFilterInfo::new("Old.dll");
        old_info.enable_processing = false;
        let default_filter = FilterInfo::default();
        let action = decide_tuner_change_action(Some(&old_info), None, &default_filter);
        assert_eq!(action, TunerChangeAction::Keep);
    }

    #[test]
    fn decide_tuner_change_action_same_filter_keeps() {
        let default_filter = filter("Mod.dll", "Device", "Filter");
        let action = decide_tuner_change_action(None, None, &default_filter);
        assert_eq!(action, TunerChangeAction::Keep);
    }

    #[test]
    fn decide_tuner_change_action_device_change_closes_filter() {
        let mut old_info = TunerFilterInfo::new("Old.dll");
        old_info.filter = filter("Mod.dll", "DeviceA", "FilterA");
        let mut new_info = TunerFilterInfo::new("New.dll");
        new_info.filter = filter("Mod.dll", "DeviceB", "FilterA");
        let default_filter = FilterInfo::default();

        let action = decide_tuner_change_action(Some(&old_info), Some(&new_info), &default_filter);
        assert_eq!(action, TunerChangeAction::CloseFilter);
    }

    #[test]
    fn decide_tuner_change_action_module_change_unloads() {
        let mut old_info = TunerFilterInfo::new("Old.dll");
        old_info.filter = filter("ModA.dll", "Device", "Filter");
        let mut new_info = TunerFilterInfo::new("New.dll");
        new_info.filter = filter("ModB.dll", "Device", "Filter");
        let default_filter = FilterInfo::default();

        let action = decide_tuner_change_action(Some(&old_info), Some(&new_info), &default_filter);
        assert_eq!(action, TunerChangeAction::UnloadModule);
    }

    #[test]
    fn decide_tuner_change_action_both_change() {
        let mut old_info = TunerFilterInfo::new("Old.dll");
        old_info.filter = filter("ModA.dll", "DeviceA", "Filter");
        let mut new_info = TunerFilterInfo::new("New.dll");
        new_info.filter = filter("ModB.dll", "DeviceB", "Filter");
        let default_filter = FilterInfo::default();

        let action = decide_tuner_change_action(Some(&old_info), Some(&new_info), &default_filter);
        assert_eq!(action, TunerChangeAction::CloseFilterAndUnloadModule);
    }

    #[test]
    fn decide_tuner_change_action_device_comparison_case_insensitive() {
        let mut old_info = TunerFilterInfo::new("Old.dll");
        old_info.filter = filter("Mod.dll", "DEVICE", "FILTER");
        let mut new_info = TunerFilterInfo::new("New.dll");
        new_info.filter = filter("Mod.dll", "device", "filter");
        let default_filter = FilterInfo::default();

        let action = decide_tuner_change_action(Some(&old_info), Some(&new_info), &default_filter);
        assert_eq!(action, TunerChangeAction::Keep);
    }

    #[test]
    fn decide_tuner_change_action_falls_back_to_default_filter() {
        // 新旧ともに TunerFilterInfo が見つからない(None)場合は default_filter 同士の比較になり Keep
        let default_filter = filter("Mod.dll", "Device", "Filter");
        let action = decide_tuner_change_action(None, None, &default_filter);
        assert_eq!(action, TunerChangeAction::Keep);
    }
}
