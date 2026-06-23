// TVTest の ProgramSearch.cpp の CEventSearcher(イベント照合エンジン)を Rust へ移植したもの。
//
// 移植対象(純粋ロジックのみ):
//   - EventSearcher : CEventSearcher (ProgramSearch.h:186)
//     - begin_search  : BeginSearch:565   (設定コピー + 正規表現コンパイル)
//     - match_event   : Match:585         (サービス/ジャンル/曜日/時間帯/長さ/CA/映像 + キーワード)
//     - match_keyword : MatchKeyword:712  (`-`除外 / `"..."`句 / `|`OR 演算子)
//     - match_regexp  : MatchRegExp:780
//     - find_keyword  : FindKeyword:682 / FindExtendedText:699
//
// 依存:
//   - EventSearchSettings / EventSearchServiceList : program_search クレート
//   - EventInfo                                    : libisdb_event_info クレート
//   - get_video_type / VideoType                   : epg_util クレート
//   - RegExp / PatternFlag                         : regexp クレート
//
// Win32 依存の FindNLSString(LOCALE_USER_DEFAULT + NORM_IGNORECASE/NORM_IGNOREWIDTH)は
// ignore_case/ignore_width を考慮した部分文字列検索で近似する。
//   - ignore_width : 全角→半角変換(regexp::string_to_half_width。RegExp の IgnoreWidth と同一処理)
//   - ignore_case  : Unicode の小文字化(str::to_lowercase。原実装の NLS 照合の近似)
// MatchKeyword/FindExtendedText では検索の成否(>=0)のみ参照するため、一致位置は不要。

use libisdb_event_info::{EventInfo, ExtendedTextInfo};
use tvtest_epg_util::{get_video_type, VideoType};
use tvtest_program_search::{CaType, EventSearchSettings, VideoFilterType};
use tvtest_regexp::{string_to_half_width, PatternFlag, RegExp};

/// イベント照合エンジン。原実装 CEventSearcher (ProgramSearch.h:186)。
#[derive(Default)]
pub struct EventSearcher {
    settings: EventSearchSettings,
    regexp: RegExp,
}

impl EventSearcher {
    pub fn new() -> Self {
        Self::default()
    }

    /// 検索を開始する。設定をコピーし、正規表現モードならパターンをコンパイルする。
    /// 原実装 CEventSearcher::BeginSearch:565。
    ///
    /// 正規表現のコンパイルに失敗した場合のみ `false` を返す。
    pub fn begin_search(&mut self, settings: &EventSearchSettings) -> bool {
        self.settings = settings.clone();

        if settings.reg_exp && !settings.keyword.is_empty() {
            let mut flags = PatternFlag::OPTIMIZE;
            if settings.ignore_case {
                flags |= PatternFlag::IGNORE_CASE;
            }
            if settings.ignore_width {
                flags |= PatternFlag::IGNORE_WIDTH;
            }
            let pattern = String::from_utf16_lossy(&settings.keyword);
            if !self.regexp.set_pattern(&pattern, flags) {
                return false;
            }
        }

        true
    }

    /// 現在の検索設定を返す。原実装 CEventSearcher::GetSearchSettings (ProgramSearch.h:195)。
    pub fn search_settings(&self) -> &EventSearchSettings {
        &self.settings
    }

    /// イベントが検索条件に一致するか判定する。原実装 CEventSearcher::Match:585。
    pub fn match_event(&self, event: &EventInfo) -> bool {
        let s = &self.settings;

        // サービスリスト
        if s.service_list_enabled
            && !s.service_list.is_exists_ids(
                event.network_id,
                event.transport_stream_id,
                event.service_id,
            )
        {
            return false;
        }

        // ジャンル
        if s.genre {
            let mut matched = false;
            for nibble in &event.content_nibble.nibble_list {
                let level1 = nibble.content_nibble_level1;
                if level1 != 0xE {
                    if level1 > 15 {
                        return false;
                    }
                    let l1 = level1 as usize;
                    if (s.genre1 as u32 & (1u32 << level1)) != 0 && s.genre2[l1] == 0 {
                        matched = true;
                    } else {
                        let level2 = nibble.content_nibble_level2;
                        if (s.genre2[l1] as u32 & (1u32 << level2)) != 0 {
                            matched = true;
                        }
                    }
                    break;
                }
            }
            if !matched {
                return false;
            }
        }

        // 曜日
        if s.day_of_week
            && (s.day_of_week_flags & (1u32 << (event.start_time.day_of_week as u32))) == 0
        {
            return false;
        }

        // 時間帯(開始時刻が範囲外なら除外。日跨ぎ範囲に対応)
        if s.time {
            let range_start = (s.start_time.hour * 60 + s.start_time.minute) % (24 * 60);
            let range_end = (s.end_time.hour * 60 + s.end_time.minute) % (24 * 60);
            let event_start = event.start_time.hour * 60 + event.start_time.minute;
            let event_end = event_start + (event.duration / 60) as i32;

            if range_start <= range_end {
                if event_end <= range_start || event_start > range_end {
                    return false;
                }
            } else if event_end <= range_start && event_start > range_end {
                return false;
            }
        }

        // 番組の長さ
        if s.duration {
            if event.duration < s.duration_shortest {
                return false;
            }
            if s.duration_longest > 0 && event.duration > s.duration_longest {
                return false;
            }
        }

        // CA(無料/有料)
        if s.ca {
            match s.ca_type {
                CaType::Free => {
                    if event.free_ca_mode {
                        return false;
                    }
                }
                CaType::Chargeable => {
                    if !event.free_ca_mode {
                        return false;
                    }
                }
            }
        }

        // 映像種別(HD/SD)
        if s.video && !event.video_list.is_empty() {
            let vt = get_video_type(event.video_list[0].component_type);
            match s.video_type {
                VideoFilterType::Hd => {
                    if vt != VideoType::Hd {
                        return false;
                    }
                }
                VideoFilterType::Sd => {
                    if vt != VideoType::Sd {
                        return false;
                    }
                }
            }
        }

        // キーワード
        if s.keyword.is_empty() {
            return true;
        }

        if s.reg_exp {
            return self.match_regexp(event);
        }

        let keyword = String::from_utf16_lossy(&s.keyword);
        self.match_keyword(event, &keyword)
    }

    /// キーワードで照合する。原実装 CEventSearcher::MatchKeyword:712。
    ///
    /// 構文:
    ///   - 空白区切りの語は AND(全て一致が必要)
    ///   - 先頭 `-` を付けた語は除外(一致したら不一致)
    ///   - `"..."` で囲むと空白を含むフレーズ
    ///   - `|` で繋ぐと OR(いずれか一致でよい)
    ///
    /// 除外語のみで構成され、いずれにも一致しない場合は一致とみなす。
    fn match_keyword(&self, event: &EventInfo, keyword: &str) -> bool {
        let chars: Vec<char> = keyword.chars().collect();
        let len = chars.len();
        let mut p = 0usize;

        let mut f_match = false;
        let mut f_minus_only = true;
        let mut f_or = false;
        let mut f_prev_or = false;
        let mut f_or_match = false;
        let mut word_count = 0;

        while p < len {
            let mut f_minus = false;

            // 先頭の空白をスキップ
            while p < len && chars[p] == ' ' {
                p += 1;
            }
            // マイナス(除外)
            if p < len && chars[p] == '-' {
                f_minus = true;
                p += 1;
            }
            // 区切り文字(引用符 or 空白)
            let delimiter = if p < len && chars[p] == '"' {
                p += 1;
                '"'
            } else {
                ' '
            };
            // 単語抽出(区切り文字 / '|' / 末尾まで)
            let word_start = p;
            while p < len && chars[p] != delimiter && chars[p] != '|' {
                p += 1;
            }
            let word_len = p - word_start;
            let word: String = chars[word_start..p].iter().collect();
            // 区切り文字を消費
            if p < len && chars[p] == delimiter {
                p += 1;
            }
            // 後続の空白をスキップ
            while p < len && chars[p] == ' ' {
                p += 1;
            }
            // OR 演算子
            if p < len && chars[p] == '|' {
                if !f_or {
                    f_or = true;
                    f_or_match = false;
                }
                p += 1;
            } else {
                f_or = false;
            }

            if word_len > 0 {
                let s = &self.settings;
                let found = (s.event_name && self.find_keyword(&event.event_name, &word))
                    || (s.event_text && self.find_keyword(&event.event_text, &word))
                    || (s.event_text && self.find_extended_text(&event.extended_text, &word));
                if found {
                    if f_minus {
                        return false;
                    }
                    f_match = true;
                    if f_or {
                        f_or_match = true;
                    }
                } else if !f_minus && !f_or && (!f_prev_or || !f_or_match) {
                    return false;
                }
                if !f_minus {
                    f_minus_only = false;
                }
                word_count += 1;
            }
            f_prev_or = f_or;
        }

        if f_minus_only && word_count > 0 {
            return true;
        }
        f_match
    }

    /// 正規表現で照合する。原実装 CEventSearcher::MatchRegExp:780。
    fn match_regexp(&self, event: &EventInfo) -> bool {
        let s = &self.settings;

        if s.event_name
            && !event.event_name.is_empty()
            && self.regexp.match_text(&event.event_name).is_some()
        {
            return true;
        }

        if s.event_text
            && !event.event_text.is_empty()
            && self.regexp.match_text(&event.event_text).is_some()
        {
            return true;
        }

        if s.event_text && !event.extended_text.is_empty() {
            for e in &event.extended_text {
                if !e.description.is_empty() && self.regexp.match_text(&e.description).is_some() {
                    return true;
                }
                if !e.text.is_empty() && self.regexp.match_text(&e.text).is_some() {
                    return true;
                }
            }
        }

        false
    }

    /// 拡張テキスト(説明 + 本文)からキーワードを探す。原実装 CEventSearcher::FindExtendedText:699。
    fn find_extended_text(&self, extended_text: &[ExtendedTextInfo], keyword: &str) -> bool {
        for e in extended_text {
            if self.find_keyword(&e.description, keyword) {
                return true;
            }
            if self.find_keyword(&e.text, keyword) {
                return true;
            }
        }
        false
    }

    /// テキスト内にキーワードが含まれるかを判定する。
    /// 原実装 CEventSearcher::FindKeyword:682(Win32 FindNLSString)の純粋近似。
    fn find_keyword(&self, text: &str, keyword: &str) -> bool {
        if text.is_empty() {
            return false;
        }
        let hay = self.fold(text);
        let needle = self.fold(keyword);
        hay.contains(&needle)
    }

    /// ignore_width / ignore_case に応じて文字列を正規化する。
    fn fold(&self, text: &str) -> String {
        let s = if self.settings.ignore_width {
            string_to_half_width(text)
        } else {
            text.to_string()
        };
        if self.settings.ignore_case {
            s.to_lowercase()
        } else {
            s
        }
    }
}

#[cfg(test)]
// テストでは EventSearchSettings の任意フィールドのみを設定するため Default + 代入を用いる。
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use libisdb_event_info::{ContentNibble, VideoInfo};
    use libisdb_datetime::DateTime;

    fn u16s(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    // キーワード以外の全条件を無効にした素のイベント。
    fn event_with_name(name: &str, text: &str) -> EventInfo {
        let mut e = EventInfo::default();
        e.event_name = name.to_string();
        e.event_text = text.to_string();
        e
    }

    fn searcher(settings: EventSearchSettings) -> EventSearcher {
        let mut s = EventSearcher::new();
        assert!(s.begin_search(&settings));
        s
    }

    #[test]
    fn test_no_condition_matches_all() {
        // 何の条件も指定しなければ全イベントが一致。
        let s = searcher(EventSearchSettings::default());
        assert!(s.match_event(&event_with_name("テスト番組", "本文")));
    }

    #[test]
    fn test_simple_keyword_hit_and_miss() {
        let mut st = EventSearchSettings::default();
        st.keyword = u16s("ニュース");
        let s = searcher(st);
        assert!(s.match_event(&event_with_name("朝のニュース", "")));
        assert!(!s.match_event(&event_with_name("天気予報", "")));
    }

    #[test]
    fn test_keyword_searches_event_text() {
        let mut st = EventSearchSettings::default();
        st.keyword = u16s("特集");
        let s = searcher(st);
        // 名前には無いが本文にある → 一致(event_text 既定 true)
        assert!(s.match_event(&event_with_name("番組", "今夜は特集です")));
    }

    #[test]
    fn test_keyword_and_condition() {
        // 空白区切りは AND
        let mut st = EventSearchSettings::default();
        st.keyword = u16s("ニュース スポーツ");
        let s = searcher(st);
        assert!(s.match_event(&event_with_name("ニュースとスポーツ", "")));
        assert!(!s.match_event(&event_with_name("ニュース速報", "")));
    }

    #[test]
    fn test_keyword_minus_exclusion() {
        // -語 は除外
        let mut st = EventSearchSettings::default();
        st.keyword = u16s("ニュース -再放送");
        let s = searcher(st);
        assert!(s.match_event(&event_with_name("夜のニュース", "")));
        assert!(!s.match_event(&event_with_name("ニュース 再放送", "")));
    }

    #[test]
    fn test_keyword_minus_only_matches_when_absent() {
        // 除外語のみ:一致しなければ true
        let mut st = EventSearchSettings::default();
        st.keyword = u16s("-再放送");
        let s = searcher(st);
        assert!(s.match_event(&event_with_name("新番組", "")));
        assert!(!s.match_event(&event_with_name("再放送です", "")));
    }

    #[test]
    fn test_keyword_quoted_phrase() {
        // "..." は空白を含むフレーズ
        let mut st = EventSearchSettings::default();
        st.keyword = u16s("\"プロ 野球\"");
        let s = searcher(st);
        assert!(s.match_event(&event_with_name("プロ 野球 中継", "")));
        // 連続でないと不一致
        assert!(!s.match_event(&event_with_name("プロのアマ野球", "")));
    }

    #[test]
    fn test_keyword_or_operator() {
        // a|b はいずれか一致
        let mut st = EventSearchSettings::default();
        st.keyword = u16s("野球|サッカー");
        let s = searcher(st);
        assert!(s.match_event(&event_with_name("プロ野球", "")));
        assert!(s.match_event(&event_with_name("サッカー中継", "")));
        assert!(!s.match_event(&event_with_name("テニス", "")));
    }

    #[test]
    fn test_ignore_case_and_width() {
        // 既定で ignore_case/ignore_width が有効
        let mut st = EventSearchSettings::default();
        st.keyword = u16s("abc");
        let s = searcher(st);
        // 全角大文字 → 半角小文字に正規化されて一致
        assert!(s.match_event(&event_with_name("ＡＢＣニュース", "")));

        // ignore_case を切ると不一致
        let mut st2 = EventSearchSettings::default();
        st2.keyword = u16s("abc");
        st2.ignore_case = false;
        st2.ignore_width = false;
        let s2 = searcher(st2);
        assert!(!s2.match_event(&event_with_name("ABCニュース", "")));
        assert!(s2.match_event(&event_with_name("abcニュース", "")));
    }

    #[test]
    fn test_event_name_only_flag() {
        // event_text を切ると本文はヒットしない
        let mut st = EventSearchSettings::default();
        st.keyword = u16s("特集");
        st.event_text = false;
        let s = searcher(st);
        assert!(!s.match_event(&event_with_name("番組", "今夜は特集です")));
        assert!(s.match_event(&event_with_name("特集番組", "")));
    }

    #[test]
    fn test_extended_text_match() {
        let mut st = EventSearchSettings::default();
        st.keyword = u16s("出演");
        let s = searcher(st);
        let mut e = event_with_name("ドラマ", "");
        e.extended_text.push(ExtendedTextInfo {
            description: "出演者".to_string(),
            text: "山田太郎".to_string(),
        });
        assert!(s.match_event(&e));
    }

    #[test]
    fn test_service_list_filter() {
        let mut st = EventSearchSettings::default();
        st.service_list_enabled = true;
        st.service_list.add_ids(0x7FE0, 0x0400, 0x0401);
        let s = searcher(st);

        let mut e = event_with_name("番組", "");
        e.network_id = 0x7FE0;
        e.transport_stream_id = 0x0400;
        e.service_id = 0x0401;
        assert!(s.match_event(&e));

        e.service_id = 0x0402;
        assert!(!s.match_event(&e));
    }

    #[test]
    fn test_genre_filter() {
        // ジャンル: level1=0x0(ニュース/報道), level2=0x1
        let mut st = EventSearchSettings::default();
        st.genre = true;
        st.genre1 = 1 << 0x0; // level1 = 0 を許可
        st.genre2[0] = 1 << 0x1; // level2 = 1 のみ許可
        let s = searcher(st);

        let mut e = event_with_name("ニュース", "");
        e.content_nibble.nibble_list.push(ContentNibble {
            content_nibble_level1: 0x0,
            content_nibble_level2: 0x1,
            ..Default::default()
        });
        assert!(s.match_event(&e));

        // level2 が違うと不一致
        let mut e2 = event_with_name("ニュース", "");
        e2.content_nibble.nibble_list.push(ContentNibble {
            content_nibble_level1: 0x0,
            content_nibble_level2: 0x2,
            ..Default::default()
        });
        assert!(!s.match_event(&e2));
    }

    #[test]
    fn test_genre_filter_level1_any_level2() {
        // genre2[level1]==0 のとき level1 一致だけで通す
        let mut st = EventSearchSettings::default();
        st.genre = true;
        st.genre1 = 1 << 0x7; // level1 = 7(音楽) を許可、level2 は不問
        let s = searcher(st);

        let mut e = event_with_name("音楽番組", "");
        e.content_nibble.nibble_list.push(ContentNibble {
            content_nibble_level1: 0x7,
            content_nibble_level2: 0x5,
            ..Default::default()
        });
        assert!(s.match_event(&e));
    }

    #[test]
    fn test_day_of_week_filter() {
        let mut st = EventSearchSettings::default();
        st.day_of_week = true;
        st.day_of_week_flags = 1 << 1; // 月曜のみ(0=日,1=月,...)
        let s = searcher(st);

        let mut e = event_with_name("番組", "");
        e.start_time = DateTime { day_of_week: 1, ..Default::default() };
        assert!(s.match_event(&e));

        e.start_time = DateTime { day_of_week: 2, ..Default::default() };
        assert!(!s.match_event(&e));
    }

    #[test]
    fn test_time_range_normal() {
        // 19:00-21:00 の番組帯
        let mut st = EventSearchSettings::default();
        st.time = true;
        st.start_time = tvtest_program_search::TimeInfo { hour: 19, minute: 0 };
        st.end_time = tvtest_program_search::TimeInfo { hour: 21, minute: 0 };
        let s = searcher(st);

        let mut e = event_with_name("番組", "");
        e.start_time = DateTime { hour: 20, minute: 0, ..Default::default() };
        e.duration = 30 * 60;
        assert!(s.match_event(&e));

        // 22:00 開始は範囲外
        e.start_time = DateTime { hour: 22, minute: 0, ..Default::default() };
        assert!(!s.match_event(&e));
    }

    #[test]
    fn test_time_range_wraparound() {
        // 23:00-翌5:00 の深夜帯(range_start > range_end)
        let mut st = EventSearchSettings::default();
        st.time = true;
        st.start_time = tvtest_program_search::TimeInfo { hour: 23, minute: 0 };
        st.end_time = tvtest_program_search::TimeInfo { hour: 5, minute: 0 };
        let s = searcher(st);

        let mut e = event_with_name("深夜番組", "");
        e.start_time = DateTime { hour: 2, minute: 0, ..Default::default() };
        e.duration = 60 * 60;
        assert!(s.match_event(&e));

        // 昼12:00 は範囲外
        e.start_time = DateTime { hour: 12, minute: 0, ..Default::default() };
        assert!(!s.match_event(&e));
    }

    #[test]
    fn test_duration_filter() {
        // 30分以上60分以下
        let mut st = EventSearchSettings::default();
        st.duration = true;
        st.duration_shortest = 30 * 60;
        st.duration_longest = 60 * 60;
        let s = searcher(st);

        let mut e = event_with_name("番組", "");
        e.duration = 45 * 60;
        assert!(s.match_event(&e));

        e.duration = 15 * 60;
        assert!(!s.match_event(&e));

        e.duration = 90 * 60;
        assert!(!s.match_event(&e));
    }

    #[test]
    fn test_ca_filter() {
        // 無料のみ
        let mut st = EventSearchSettings::default();
        st.ca = true;
        st.ca_type = CaType::Free;
        let s = searcher(st);

        let mut e = event_with_name("番組", "");
        e.free_ca_mode = false; // 無料
        assert!(s.match_event(&e));
        e.free_ca_mode = true; // 有料
        assert!(!s.match_event(&e));
    }

    #[test]
    fn test_video_type_filter() {
        // HD のみ
        let mut st = EventSearchSettings::default();
        st.video = true;
        st.video_type = VideoFilterType::Hd;
        let s = searcher(st);

        let mut e = event_with_name("番組", "");
        // component_type 0xB1 → HD(上位 0xB, 下位 0x1)
        e.video_list.push(VideoInfo { component_type: 0xB1, ..Default::default() });
        assert!(s.match_event(&e));

        // 0xA1 → SD(上位 0xA)
        let mut e2 = event_with_name("番組", "");
        e2.video_list.push(VideoInfo { component_type: 0xA1, ..Default::default() });
        assert!(!s.match_event(&e2));
    }

    #[test]
    fn test_regexp_match() {
        let mut st = EventSearchSettings::default();
        st.reg_exp = true;
        st.keyword = u16s("第[0-9]+話");
        let s = searcher(st);
        assert!(s.match_event(&event_with_name("連続ドラマ 第12話", "")));
        assert!(!s.match_event(&event_with_name("連続ドラマ 最終回", "")));
    }

    #[test]
    fn test_combined_conditions() {
        // キーワード + 長さ + 無料 の AND
        let mut st = EventSearchSettings::default();
        st.keyword = u16s("ニュース");
        st.duration = true;
        st.duration_shortest = 20 * 60;
        st.ca = true;
        st.ca_type = CaType::Free;
        let s = searcher(st);

        let mut e = event_with_name("夜のニュース", "");
        e.duration = 30 * 60;
        e.free_ca_mode = false;
        assert!(s.match_event(&e));

        // 長さ不足で除外
        e.duration = 10 * 60;
        assert!(!s.match_event(&e));
    }
}
