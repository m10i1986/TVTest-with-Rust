// TVTest の FeaturedEvents.cpp の純粋ロジックを Rust へ移植したもの。
//
// 移植対象:
//   - FeaturedEventsSettings : CFeaturedEventsSettings (FeaturedEvents.h:36) のデータ構造
//     (ReadSettings/WriteSettings の CSettings I/O は対象外。保持値と既定値・SortType のみ)
//   - FeaturedEventsMatcher  : CFeaturedEventsMatcher (FeaturedEvents.h:92)
//     - begin_matching : BeginMatching (FeaturedEvents.cpp:184)
//     - end_matching   : EndMatching   (FeaturedEvents.cpp:204)
//     - is_match       : IsMatch       (FeaturedEvents.cpp:211)
//
// CFeaturedEventsSearcher(EPGDatabase を列挙する Update)と GUI(CFeaturedEventsDialog)、
// CSettings I/O は対象外。照合エンジン本体は event_searcher クレートの EventSearcher を利用する。
//
// 依存:
//   - EventSearcher                                : event_searcher クレート
//   - EventSearchServiceList / EventSearchSettingsList : program_search クレート
//   - EventInfo                                    : libisdb_event_info クレート

use libisdb_event_info::EventInfo;
use tvtest_event_searcher::EventSearcher;
use tvtest_program_search::{EventSearchServiceList, EventSearchSettingsList};

/// 注目番組の並び順。原実装 CFeaturedEventsSettings::SortType (FeaturedEvents.h:40)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortType {
    #[default]
    Time = 0,
    Service = 1,
}

impl SortType {
    /// 整数値から変換する。原実装 CheckEnumRange による範囲チェック(0..=1)に対応。
    pub fn from_int(v: i32) -> Option<SortType> {
        match v {
            0 => Some(SortType::Time),
            1 => Some(SortType::Service),
            _ => None,
        }
    }

    pub fn to_int(self) -> i32 {
        self as i32
    }
}

/// 注目番組の設定。原実装 CFeaturedEventsSettings (FeaturedEvents.h:36)。
///
/// ReadSettings/WriteSettings は CSettings 依存のため対象外。ここでは保持データと既定値、
/// および照合に必要な default_service_list / search_settings_list を持つ。
#[derive(Debug, Clone)]
pub struct FeaturedEventsSettings {
    pub default_service_list: EventSearchServiceList,
    pub search_settings_list: EventSearchSettingsList,
    pub sort_type: SortType,
    pub period_seconds: i32,
    pub show_event_text: bool,
    pub event_text_lines: i32,
}

impl FeaturedEventsSettings {
    /// 番組内容テキストの最大行数。原実装 MAX_EVENT_TEXT_LINES (FeaturedEvents.h:46)。
    pub const MAX_EVENT_TEXT_LINES: i32 = 10;

    pub fn new() -> Self {
        Self::default()
    }
}

impl Default for FeaturedEventsSettings {
    fn default() -> Self {
        // 既定値は CFeaturedEventsSettings のメンバ初期化子(FeaturedEvents.h:71-74)に準拠。
        Self {
            default_service_list: EventSearchServiceList::new(),
            search_settings_list: EventSearchSettingsList::new(),
            sort_type: SortType::Time,
            period_seconds: 24 * 60 * 60,
            show_event_text: true,
            event_text_lines: 2,
        }
    }
}

/// 注目番組の照合器。原実装 CFeaturedEventsMatcher (FeaturedEvents.h:92)。
///
/// 有効な検索設定ごとに EventSearcher を構築し、いずれかに一致すれば「注目番組」とみなす。
#[derive(Default)]
pub struct FeaturedEventsMatcher {
    default_service_list: EventSearchServiceList,
    searcher_list: Vec<EventSearcher>,
}

impl FeaturedEventsMatcher {
    pub fn new() -> Self {
        Self::default()
    }

    /// 照合を開始する。原実装 CFeaturedEventsMatcher::BeginMatching (FeaturedEvents.cpp:184)。
    ///
    /// 既定サービスリストをコピーし、無効化されていない各検索設定について
    /// EventSearcher を生成して BeginSearch する。
    pub fn begin_matching(&mut self, settings: &FeaturedEventsSettings) -> bool {
        self.default_service_list = settings.default_service_list.clone();

        let list = &settings.search_settings_list;
        self.searcher_list.clear();
        self.searcher_list.reserve(list.get_enabled_count());

        for i in 0..list.get_count() {
            if let Some(s) = list.get(i) {
                if !s.disabled {
                    let mut searcher = EventSearcher::new();
                    searcher.begin_search(s);
                    self.searcher_list.push(searcher);
                }
            }
        }

        true
    }

    /// 照合を終了する。原実装 CFeaturedEventsMatcher::EndMatching (FeaturedEvents.cpp:204)。
    pub fn end_matching(&mut self) {
        self.default_service_list.clear();
        self.searcher_list.clear();
    }

    /// イベントが注目番組条件に一致するか。原実装 CFeaturedEventsMatcher::IsMatch (FeaturedEvents.cpp:211)。
    ///
    /// 各 EventSearcher について、サービスリスト指定が無い設定は
    /// 既定サービスリストに含まれるイベントのみを対象とする。
    pub fn is_match(&self, event: &EventInfo) -> bool {
        for searcher in &self.searcher_list {
            if !searcher.search_settings().service_list_enabled
                && !self.default_service_list.is_exists_ids(
                    event.network_id,
                    event.transport_stream_id,
                    event.service_id,
                )
            {
                continue;
            }
            if searcher.match_event(event) {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
// テストでは設定構造体の任意フィールドのみを設定するため Default + 代入を用いる。
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use tvtest_program_search::EventSearchSettings;

    fn u16s(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn event(name: &str, nid: u16, tsid: u16, sid: u16) -> EventInfo {
        let mut e = EventInfo::default();
        e.event_name = name.to_string();
        e.network_id = nid;
        e.transport_stream_id = tsid;
        e.service_id = sid;
        e
    }

    fn keyword_settings(name: &str, keyword: &str) -> EventSearchSettings {
        let mut s = EventSearchSettings::default();
        s.name = u16s(name);
        s.keyword = u16s(keyword);
        s
    }

    #[test]
    fn test_settings_defaults() {
        let s = FeaturedEventsSettings::new();
        assert_eq!(s.sort_type, SortType::Time);
        assert_eq!(s.period_seconds, 24 * 60 * 60);
        assert!(s.show_event_text);
        assert_eq!(s.event_text_lines, 2);
        assert_eq!(FeaturedEventsSettings::MAX_EVENT_TEXT_LINES, 10);
    }

    #[test]
    fn test_sort_type_from_int() {
        assert_eq!(SortType::from_int(0), Some(SortType::Time));
        assert_eq!(SortType::from_int(1), Some(SortType::Service));
        assert_eq!(SortType::from_int(2), None);
        assert_eq!(SortType::from_int(-1), None);
        assert_eq!(SortType::Service.to_int(), 1);
    }

    #[test]
    fn test_empty_matcher_matches_nothing() {
        // 検索設定が無ければ何も一致しない。
        let settings = FeaturedEventsSettings::new();
        let mut matcher = FeaturedEventsMatcher::new();
        assert!(matcher.begin_matching(&settings));
        assert!(!matcher.is_match(&event("番組", 1, 2, 3)));
    }

    #[test]
    fn test_match_requires_default_service_when_no_service_list() {
        // サービスリスト指定の無い設定は default_service_list に含まれるイベントのみ対象。
        let mut settings = FeaturedEventsSettings::new();
        settings.search_settings_list.add(keyword_settings("news", "ニュース"));
        settings.default_service_list.add_ids(0x7FE0, 0x0400, 0x0401);

        let mut matcher = FeaturedEventsMatcher::new();
        assert!(matcher.begin_matching(&settings));

        // default_service_list に含まれ、かつキーワード一致 → 一致
        assert!(matcher.is_match(&event("朝のニュース", 0x7FE0, 0x0400, 0x0401)));
        // キーワードは一致するが default_service_list 外 → 不一致
        assert!(!matcher.is_match(&event("朝のニュース", 0x7FE0, 0x0400, 0x0402)));
        // サービスは一致するがキーワード不一致 → 不一致
        assert!(!matcher.is_match(&event("天気予報", 0x7FE0, 0x0400, 0x0401)));
    }

    #[test]
    fn test_match_with_per_setting_service_list_ignores_default() {
        // 設定自身がサービスリストを持つ場合、default_service_list の制約を受けない。
        let mut s = keyword_settings("news", "ニュース");
        s.service_list_enabled = true;
        s.service_list.add_ids(0x0001, 0x0002, 0x0003);

        let mut settings = FeaturedEventsSettings::new();
        settings.search_settings_list.add(s);
        // default_service_list は空のまま

        let mut matcher = FeaturedEventsMatcher::new();
        assert!(matcher.begin_matching(&settings));

        // 設定のサービスリストに含まれ、キーワード一致 → 一致(default が空でも可)
        assert!(matcher.is_match(&event("ニュース速報", 0x0001, 0x0002, 0x0003)));
        // 設定のサービスリスト外 → EventSearcher 側で除外され不一致
        assert!(!matcher.is_match(&event("ニュース速報", 0x0001, 0x0002, 0x0004)));
    }

    #[test]
    fn test_disabled_settings_are_skipped() {
        // disabled の設定は EventSearcher が作られず照合に使われない。
        let mut disabled = keyword_settings("off", "ニュース");
        disabled.disabled = true;
        disabled.service_list_enabled = true;
        disabled.service_list.add_ids(1, 2, 3);

        let mut settings = FeaturedEventsSettings::new();
        settings.search_settings_list.add(disabled);

        let mut matcher = FeaturedEventsMatcher::new();
        assert!(matcher.begin_matching(&settings));
        // 唯一の設定が無効なので一致しない。
        assert!(!matcher.is_match(&event("ニュース", 1, 2, 3)));
    }

    #[test]
    fn test_multiple_settings_or() {
        // 複数設定はいずれか一致で OK。
        let mut a = keyword_settings("a", "野球");
        a.service_list_enabled = true;
        a.service_list.add_ids(1, 1, 1);
        let mut b = keyword_settings("b", "サッカー");
        b.service_list_enabled = true;
        b.service_list.add_ids(1, 1, 1);

        let mut settings = FeaturedEventsSettings::new();
        settings.search_settings_list.add(a);
        settings.search_settings_list.add(b);

        let mut matcher = FeaturedEventsMatcher::new();
        assert!(matcher.begin_matching(&settings));

        assert!(matcher.is_match(&event("プロ野球中継", 1, 1, 1)));
        assert!(matcher.is_match(&event("サッカー日本代表", 1, 1, 1)));
        assert!(!matcher.is_match(&event("テニス", 1, 1, 1)));
    }

    #[test]
    fn test_end_matching_clears() {
        let mut settings = FeaturedEventsSettings::new();
        let mut s = keyword_settings("a", "ニュース");
        s.service_list_enabled = true;
        s.service_list.add_ids(1, 1, 1);
        settings.search_settings_list.add(s);

        let mut matcher = FeaturedEventsMatcher::new();
        matcher.begin_matching(&settings);
        assert!(matcher.is_match(&event("ニュース", 1, 1, 1)));

        matcher.end_matching();
        // クリア後は一致しない。
        assert!(!matcher.is_match(&event("ニュース", 1, 1, 1)));
    }
}
