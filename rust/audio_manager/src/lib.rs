//! TVTest の `CAudioManager`(src/AudioManager.cpp / AudioManager.h)のモデル層移植。
//!
//! 音声ストリームの管理のうち、Win32/エンジンに依存しない以下の純粋ロジックを移植する:
//! - 音声 ID の符号化/復号([`make_id`] / [`id_to_component_tag`] / [`id_to_stream_index`])
//! - PMT(コンポーネント一覧)と EIT(イベント音声)の統合([`AudioManager::make_audio_list`])
//! - 既定音声の選択(言語優先・デュアルモノ考慮、[`AudioManager::default_audio`])
//! - EIT 音声情報のデュアルモノ主/副/両展開([`build_event_audio_list`])
//! - サービス/イベント更新時の選択状態の保存・復元
//!   ([`AudioManager::on_service_changed`] / [`AudioManager::on_event_changed`])
//!
//! 文字列は原実装の `wchar_t`(UTF-16)に合わせて `Vec<u16>` で扱う。
//!
//! # 対象外(Win32 / エンジン依存)
//! `CCoreEngine`/`AnalyzerFilter` からの音声ストリーム情報取得、ロック(`MutexLock`)。
//! [`AudioManager::on_service_changed`] / [`on_event_changed`] は取得済みデータを引数で受け取る。

use std::collections::HashMap;

/// 音声 ID 型(AudioManager.h 35 `IDType`)。
pub type IdType = u16;

/// 無効な音声 ID(AudioManager.h 37 `ID_INVALID`)。
pub const ID_INVALID: IdType = 0xFFFF;

/// 無効なコンポーネントタグ(LibISDBConsts.hpp 42)。
pub const COMPONENT_TAG_INVALID: u8 = 0xFF;
/// 無効なコンポーネントタイプ(LibISDBConsts.hpp 43)。
pub const COMPONENT_TYPE_INVALID: u8 = 0xFF;
/// 無効な TS ID(LibISDBConsts.hpp 37)。
pub const TRANSPORT_STREAM_ID_INVALID: u16 = 0x0000;
/// 無効なサービス ID(LibISDBConsts.hpp 39)。
pub const SERVICE_ID_INVALID: u16 = 0x0000;
/// 無効なイベント ID(LibISDBConsts.hpp 41)。
pub const EVENT_ID_INVALID: u16 = 0x0000;

/// デュアルモノのモード(AudioManager.h 39-45 `DualMonoMode`)。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DualMonoMode {
    #[default]
    Invalid,
    Main,
    Sub,
    Both,
}

/// インデックスとコンポーネントタグから音声 ID を生成する(AudioManager.h 74-79 `MakeID`)。
///
/// コンポーネントタグが有効ならそれをそのまま ID とし、無効なら下位 8bit を
/// `COMPONENT_TAG_INVALID`、上位 8bit に符号付きインデックス(INT8)を格納する。
pub const fn make_id(index: i32, component_tag: u8) -> IdType {
    if component_tag != COMPONENT_TAG_INVALID {
        component_tag as IdType
    } else {
        (((index as i8) as u8 as u16) << 8) | (COMPONENT_TAG_INVALID as u16)
    }
}

/// 音声 ID からコンポーネントタグを取り出す(AudioManager.h 80 `IDToComponentTag`)。
pub const fn id_to_component_tag(id: IdType) -> u8 {
    (id & 0xFF) as u8
}

/// 音声 ID からストリームインデックス(符号付き)を取り出す(AudioManager.h 81 `IDToStreamIndex`)。
pub const fn id_to_stream_index(id: IdType) -> i32 {
    (((id >> 8) as u8) as i8) as i32
}

/// TS ID とサービス ID から選択保持マップのキーを生成する(AudioManager.h 116-118 `ServiceMapKey`)。
pub const fn service_map_key(transport_stream_id: u16, service_id: u16) -> u32 {
    ((transport_stream_id as u32) << 16) | (service_id as u32)
}

/// 音声情報(AudioManager.h 47-60 `AudioInfo`)。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct AudioInfo {
    pub id: IdType,
    pub component_tag: u8,
    pub component_type: u8,
    pub dual_mono: DualMonoMode,
    pub multi_lingual: bool,
    pub language: u32,
    pub language2: u32,
    pub text: Vec<u16>,
}

impl AudioInfo {
    /// デュアルモノか(AudioManager.h 59 `IsDualMono`、ComponentType == 0x02)。
    pub fn is_dual_mono(&self) -> bool {
        self.component_type == 0x02
    }
}

/// 音声選択情報(AudioManager.h 64-70 `AudioSelectInfo`)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AudioSelectInfo {
    pub id: IdType,
    pub dual_mono: DualMonoMode,
}

impl Default for AudioSelectInfo {
    fn default() -> Self {
        // AudioManager.h 66-67 のメンバ既定値。
        Self {
            id: ID_INVALID,
            dual_mono: DualMonoMode::Invalid,
        }
    }
}

impl AudioSelectInfo {
    /// 初期状態へ戻す(AudioManager.h 69 `Reset`)。
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// 言語優先設定の 1 項目(CAudioOptions::AudioLanguageInfo 相当)。
#[derive(Clone, Copy, Debug)]
pub struct AudioLanguagePriority {
    pub language: u32,
    /// 副音声優先(fSub)。
    pub sub: bool,
}

/// EIT(イベント音声記述子)の 1 項目(LibISDB::AnalyzerFilter::EventAudioInfo 相当)。
#[derive(Clone, Debug)]
pub struct EventAudioInfo {
    pub component_type: u8,
    pub component_tag: u8,
    pub es_multi_lingual_flag: bool,
    pub language_code: u32,
    pub language_code2: u32,
    pub text: Vec<u16>,
}

const CR: u16 = 0x0D; // '\r'
const LF: u16 = 0x0A; // '\n'
const PLUS: u16 = 0x2B; // '+'

/// EIT の音声情報リストから音声リストを構築する(AudioManager.cpp 319-375 の変換部)。
///
/// デュアルモノ項目は 主(Main)/副(Sub)/両(Both)の 3 件に展開する。テキストは `\r`
/// (任意で続く `\n`)で主/副に分割し、両は `主+副` を連結する。
pub fn build_event_audio_list(entries: &[EventAudioInfo]) -> Vec<AudioInfo> {
    let mut list = Vec::new();

    for ea in entries {
        let mut audio1 = AudioInfo {
            component_type: ea.component_type,
            component_tag: ea.component_tag,
            ..Default::default()
        };
        let is_dual_mono = audio1.is_dual_mono();
        if is_dual_mono {
            audio1.dual_mono = DualMonoMode::Main;
            audio1.multi_lingual =
                ea.es_multi_lingual_flag && ea.language_code != ea.language_code2;
        } else {
            audio1.dual_mono = DualMonoMode::Invalid;
            audio1.multi_lingual = false;
        }
        audio1.language = ea.language_code;
        audio1.language2 = 0;

        let delimiter = ea.text.iter().position(|&c| c == CR);
        audio1.text = match delimiter {
            None => ea.text.clone(),
            Some(pos) => ea.text[..pos].to_vec(),
        };

        list.push(audio1.clone());

        if is_dual_mono {
            // 副音声テキストは区切り(+続く \n)以降。
            let sub_text = match delimiter {
                Some(mut pos) => {
                    pos += 1;
                    if pos < ea.text.len() && ea.text[pos] == LF {
                        pos += 1;
                    }
                    ea.text[pos..].to_vec()
                }
                None => Vec::new(),
            };

            let audio2 = AudioInfo {
                id: 0,
                component_type: ea.component_type,
                component_tag: ea.component_tag,
                dual_mono: DualMonoMode::Sub,
                multi_lingual: audio1.multi_lingual,
                language: if ea.es_multi_lingual_flag {
                    ea.language_code2
                } else {
                    ea.language_code
                },
                language2: 0,
                text: sub_text,
            };
            list.push(audio2.clone());

            // 両音声は Audio1 を加工。
            audio1.dual_mono = DualMonoMode::Both;
            audio1.language2 = audio2.language;
            if delimiter.is_some() {
                audio1.text.push(PLUS);
                audio1.text.extend_from_slice(&audio2.text);
            } else {
                audio1.text.clear();
            }
            list.push(audio1);
        }
    }

    list
}

/// サービスごとの選択保持(AudioManager.h 100-104 `ServiceAudioSelectInfo`)。
#[derive(Clone, Copy, Debug)]
struct ServiceAudioSelectInfo {
    selected_audio: AudioSelectInfo,
    event_id: u16,
}

/// `CAudioManager` のモデル層。
#[derive(Debug, Default)]
pub struct AudioManager {
    audio_component_list: Vec<IdType>,
    event_audio_list: Vec<AudioInfo>,
    audio_list: Vec<AudioInfo>,
    selected_audio: AudioSelectInfo,
    cur_transport_stream_id: u16,
    cur_service_id: u16,
    cur_event_id: u16,
    service_audio_select_map: HashMap<u32, ServiceAudioSelectInfo>,
}

impl AudioManager {
    /// 空のマネージャを生成する。
    pub fn new() -> Self {
        Self::default()
    }

    /// 音声数を返す(AudioManager.cpp 34-39 `GetAudioCount`)。
    pub fn audio_count(&self) -> i32 {
        self.audio_list.len() as i32
    }

    /// 指定インデックスの音声情報を返す(AudioManager.cpp 42-52 `GetAudioInfo`)。
    pub fn audio_info(&self, index: i32) -> Option<&AudioInfo> {
        if index < 0 {
            return None;
        }
        self.audio_list.get(index as usize)
    }

    /// 音声リスト全体を返す(AudioManager.cpp 55-65 `GetAudioList`)。
    pub fn audio_list(&self) -> &[AudioInfo] {
        &self.audio_list
    }

    /// ID で音声情報のインデックスを検索する(AudioManager.cpp 68-78 `FindAudioInfoByID`)。
    /// 見つからなければ -1。
    pub fn find_audio_info_by_id(&self, id: IdType) -> i32 {
        self.audio_list
            .iter()
            .position(|a| a.id == id)
            .map_or(-1, |i| i as i32)
    }

    /// 既定の音声を求める(AudioManager.cpp 81-128 `GetDefaultAudio`)。
    ///
    /// 言語優先が有効なら優先リスト順に一致する音声を探し、無ければ先頭の音声を選ぶ。
    /// デュアルモノの先頭音声では現在の選択モード(無ければ Main)を引き継ぐ。
    /// 音声リストが空なら `None`(原実装の -1)。それ以外は (インデックス, 選択情報)。
    pub fn default_audio(
        &self,
        enable_language_priority: bool,
        priority_list: &[AudioLanguagePriority],
    ) -> Option<(usize, AudioSelectInfo)> {
        if self.audio_list.is_empty() {
            return None;
        }

        if enable_language_priority && !priority_list.is_empty() {
            for priority in priority_list {
                for (idx, audio) in self.audio_list.iter().enumerate() {
                    if audio.language == priority.language
                        && audio.dual_mono != DualMonoMode::Both
                        && (!priority.sub || idx != 0 || audio.dual_mono == DualMonoMode::Sub)
                    {
                        return Some((
                            idx,
                            AudioSelectInfo {
                                id: audio.id,
                                dual_mono: audio.dual_mono,
                            },
                        ));
                    }
                }
            }
        }

        let info = &self.audio_list[0];
        let dual_mono = if info.is_dual_mono() {
            if self.selected_audio.dual_mono != DualMonoMode::Invalid {
                self.selected_audio.dual_mono
            } else {
                DualMonoMode::Main
            }
        } else {
            DualMonoMode::Invalid
        };
        Some((0, AudioSelectInfo { id: info.id, dual_mono }))
    }

    /// ID から選択情報を求める(AudioManager.cpp 131-156 `GetAudioSelectInfoByID`)。
    pub fn audio_select_info_by_id(&self, id: IdType) -> Option<AudioSelectInfo> {
        let index = self.find_audio_info_by_id(id);
        if index < 0 {
            return None;
        }
        let info = &self.audio_list[index as usize];
        let dual_mono = if info.is_dual_mono() {
            if self.selected_audio.dual_mono != DualMonoMode::Invalid {
                self.selected_audio.dual_mono
            } else {
                DualMonoMode::Main
            }
        } else {
            DualMonoMode::Invalid
        };
        Some(AudioSelectInfo { id, dual_mono })
    }

    /// 選択音声を設定する(AudioManager.cpp 159-168 `SetSelectedAudio`)。`None` で初期化。
    pub fn set_selected_audio(&mut self, select_info: Option<AudioSelectInfo>) {
        self.selected_audio = select_info.unwrap_or_default();
    }

    /// 現在の選択音声を返す(AudioManager.cpp 171-178 `GetSelectedAudio` の出力部)。
    pub fn selected_audio(&self) -> AudioSelectInfo {
        self.selected_audio
    }

    /// 選択音声があるか(`GetSelectedAudio` の戻り値、ID != ID_INVALID)。
    pub fn has_selected_audio(&self) -> bool {
        self.selected_audio.id != ID_INVALID
    }

    /// 選択音声(ID とデュアルモノモード一致)のインデックスを返す
    /// (AudioManager.cpp 181-195 `FindSelectedAudio`)。
    pub fn find_selected_audio(&self) -> i32 {
        if self.selected_audio.id == ID_INVALID {
            return -1;
        }
        self.audio_list
            .iter()
            .position(|a| a.id == self.selected_audio.id && a.dual_mono == self.selected_audio.dual_mono)
            .map_or(-1, |i| i as i32)
    }

    /// 選択 ID を設定する(AudioManager.cpp 198-203 `SetSelectedID`)。
    pub fn set_selected_id(&mut self, id: IdType) {
        self.selected_audio.id = id;
    }

    /// 選択 ID を返す(AudioManager.cpp 206-211 `GetSelectedID`)。
    pub fn selected_id(&self) -> IdType {
        self.selected_audio.id
    }

    /// 選択デュアルモノモードを設定する(AudioManager.cpp 214-224 `SetSelectedDualMonoMode`)。
    ///
    /// 原実装は `CheckEnumRange` で範囲検証するが、Rust の enum は常に有効なため true を返す。
    pub fn set_selected_dual_mono_mode(&mut self, mode: DualMonoMode) -> bool {
        self.selected_audio.dual_mono = mode;
        true
    }

    /// 選択デュアルモノモードを返す(AudioManager.cpp 227-232 `GetSelectedDualMonoMode`)。
    pub fn selected_dual_mono_mode(&self) -> DualMonoMode {
        self.selected_audio.dual_mono
    }

    /// PMT と EIT を統合して音声リストを作成する(AudioManager.cpp 399-439 `MakeAudioList`)。
    pub fn make_audio_list(&mut self) {
        let mut result: Vec<AudioInfo> = Vec::new();

        for &id in &self.audio_component_list {
            let component_tag = id_to_component_tag(id);
            let mut found = false;

            if component_tag != COMPONENT_TAG_INVALID {
                for j in 0..self.event_audio_list.len() {
                    let info = &self.event_audio_list[j];
                    if info.component_tag == component_tag {
                        let mut a = info.clone();
                        a.id = id;
                        let dual_mono = info.is_dual_mono();
                        result.push(a);
                        // デュアルモノは続く副/両の 2 件も取り込む(build_event_audio_list が
                        // 三つ組を保証するが、念のため境界を確認)。
                        if dual_mono && j + 2 < self.event_audio_list.len() {
                            let mut a1 = self.event_audio_list[j + 1].clone();
                            a1.id = id;
                            result.push(a1);
                            let mut a2 = self.event_audio_list[j + 2].clone();
                            a2.id = id;
                            result.push(a2);
                        }
                        found = true;
                        break;
                    }
                }
            }

            if !found {
                result.push(AudioInfo {
                    id,
                    component_tag,
                    component_type: COMPONENT_TYPE_INVALID,
                    dual_mono: DualMonoMode::Invalid,
                    multi_lingual: false,
                    language: 0,
                    language2: 0,
                    text: Vec::new(),
                });
            }
        }

        self.audio_list = result;
    }

    /// コンポーネント一覧を更新し音声リストを再構築する(テスト/上位層向けの補助)。
    pub fn set_component_list(&mut self, component_list: Vec<IdType>) {
        self.audio_component_list = component_list;
        self.make_audio_list();
    }

    /// EIT 音声リストを更新し音声リストを再構築する(テスト/上位層向けの補助)。
    pub fn set_event_audio_list(&mut self, event_audio_list: Vec<AudioInfo>) {
        self.event_audio_list = event_audio_list;
        self.make_audio_list();
    }

    /// サービス更新時の選択状態処理(AudioManager.cpp 256-302 `OnServiceUpdated` の純粋部)。
    ///
    /// 取得済みのコンポーネント一覧と TS ID/サービス ID を受け取り、サービス変更時は
    /// 旧サービスの選択を保存し新サービスの選択を復元する。コンポーネント一覧に変化が
    /// あれば音声リストを再構築する。何か処理したら true。
    pub fn on_service_changed(
        &mut self,
        component_list: Vec<IdType>,
        transport_stream_id: u16,
        service_id: u16,
    ) -> bool {
        let service_changed = transport_stream_id != self.cur_transport_stream_id
            || service_id != self.cur_service_id;

        if self.audio_component_list == component_list {
            if !service_changed {
                return false;
            }
        } else {
            self.audio_component_list = component_list;
            self.make_audio_list();
        }

        if service_changed {
            if self.cur_transport_stream_id != 0
                && self.cur_service_id != 0
                && self.selected_audio.id != ID_INVALID
            {
                let info = ServiceAudioSelectInfo {
                    selected_audio: self.selected_audio,
                    event_id: self.cur_event_id,
                };
                self.service_audio_select_map.insert(
                    service_map_key(self.cur_transport_stream_id, self.cur_service_id),
                    info,
                );
            }

            self.cur_transport_stream_id = transport_stream_id;
            self.cur_service_id = service_id;
            self.cur_event_id = EVENT_ID_INVALID;

            self.set_selected_audio(None);

            if transport_stream_id != 0 && service_id != 0 {
                if let Some(info) = self
                    .service_audio_select_map
                    .get(&service_map_key(transport_stream_id, service_id))
                {
                    self.selected_audio = info.selected_audio;
                    self.cur_event_id = info.event_id;
                }
            }
        } else {
            // 選択されていた ID のストリームが無くなったらリセット。
            if self.selected_audio.id != ID_INVALID
                && !self.audio_component_list.contains(&self.selected_audio.id)
            {
                self.set_selected_audio(None);
            }
        }

        true
    }

    /// イベント更新時の処理(AudioManager.cpp 378-395 `OnEventUpdated` の純粋部)。
    ///
    /// 取得済みの EIT 音声リスト([`build_event_audio_list`] の結果)とイベント ID を受け取り、
    /// イベント変更時は選択をリセット、リストに変化があれば音声リストを再構築する。
    /// 何か変化したら true。
    pub fn on_event_changed(&mut self, event_audio_list: Vec<AudioInfo>, event_id: u16) -> bool {
        let mut changed = false;

        if self.cur_event_id != event_id {
            if self.cur_event_id != EVENT_ID_INVALID {
                self.set_selected_audio(None);
                changed = true;
            }
            self.cur_event_id = event_id;
        }

        if self.event_audio_list != event_audio_list {
            self.event_audio_list = event_audio_list;
            self.make_audio_list();
            changed = true;
        }

        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn id_encoding_with_tag() {
        // 有効タグはそのまま ID
        let id = make_id(0, 0x10);
        assert_eq!(id, 0x0010);
        assert_eq!(id_to_component_tag(id), 0x10);
    }

    #[test]
    fn id_encoding_invalid_tag_uses_index() {
        // 無効タグ -> 上位にインデックス、下位に 0xFF
        let id = make_id(3, COMPONENT_TAG_INVALID);
        assert_eq!(id, 0x03FF);
        assert_eq!(id_to_component_tag(id), COMPONENT_TAG_INVALID);
        assert_eq!(id_to_stream_index(id), 3);
        // 負のインデックスも符号付きで往復
        let idn = make_id(-1, COMPONENT_TAG_INVALID);
        assert_eq!(idn, 0xFFFF);
        assert_eq!(id_to_stream_index(idn), -1);
    }

    #[test]
    fn service_map_key_layout() {
        assert_eq!(service_map_key(0x0001, 0x0002), 0x0001_0002);
    }

    #[test]
    fn is_dual_mono_check() {
        let a = AudioInfo { component_type: 0x02, ..Default::default() };
        assert!(a.is_dual_mono());
        let b = AudioInfo { component_type: 0x01, ..Default::default() };
        assert!(!b.is_dual_mono());
    }

    #[test]
    fn build_event_audio_non_dual_mono() {
        let entries = [EventAudioInfo {
            component_type: 0x01, // ステレオ
            component_tag: 0x10,
            es_multi_lingual_flag: false,
            language_code: 0x6A_70_6E, // "jpn"
            language_code2: 0,
            text: w("ステレオ"),
        }];
        let list = build_event_audio_list(&entries);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].dual_mono, DualMonoMode::Invalid);
        assert!(!list[0].multi_lingual);
        assert_eq!(list[0].language, 0x6A_70_6E);
        assert_eq!(list[0].text, w("ステレオ"));
    }

    #[test]
    fn build_event_audio_dual_mono_expands_to_three() {
        let entries = [EventAudioInfo {
            component_type: 0x02, // デュアルモノ
            component_tag: 0x11,
            es_multi_lingual_flag: true,
            language_code: 0x6A_70_6E,  // jpn
            language_code2: 0x65_6E_67, // eng
            text: w("main\rsub"),
        }];
        let list = build_event_audio_list(&entries);
        assert_eq!(list.len(), 3);
        // Main
        assert_eq!(list[0].dual_mono, DualMonoMode::Main);
        assert!(list[0].multi_lingual);
        assert_eq!(list[0].language, 0x6A_70_6E);
        assert_eq!(list[0].text, w("main"));
        // Sub
        assert_eq!(list[1].dual_mono, DualMonoMode::Sub);
        assert_eq!(list[1].language, 0x65_6E_67); // 多言語なので language_code2
        assert_eq!(list[1].text, w("sub"));
        // Both
        assert_eq!(list[2].dual_mono, DualMonoMode::Both);
        assert_eq!(list[2].language, 0x6A_70_6E);
        assert_eq!(list[2].language2, 0x65_6E_67);
        assert_eq!(list[2].text, w("main+sub"));
    }

    #[test]
    fn build_event_audio_dual_mono_no_delimiter_clears_both() {
        let entries = [EventAudioInfo {
            component_type: 0x02,
            component_tag: 0x11,
            es_multi_lingual_flag: false,
            language_code: 0x6A_70_6E,
            language_code2: 0,
            text: w("mono"),
        }];
        let list = build_event_audio_list(&entries);
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].text, w("mono")); // Main は全体
        assert_eq!(list[1].text, w("")); // Sub は空
        assert_eq!(list[2].text, w("")); // Both は区切り無しでクリア
        assert!(!list[0].multi_lingual); // 多言語フラグ無し
    }

    #[test]
    fn make_audio_list_merges_pmt_and_eit() {
        let mut m = AudioManager::new();
        // EIT: タグ 0x10 のデュアルモノ -> 3 件
        let events = build_event_audio_list(&[EventAudioInfo {
            component_type: 0x02,
            component_tag: 0x10,
            es_multi_lingual_flag: false,
            language_code: 0x6A_70_6E,
            language_code2: 0,
            text: w("m\rs"),
        }]);
        m.set_event_audio_list(events);
        // PMT: タグ 0x10(EIT 一致, 3 件展開) と 無効タグ(一致せず 1 件)
        m.set_component_list(vec![make_id(0, 0x10), make_id(1, COMPONENT_TAG_INVALID)]);
        let list = m.audio_list();
        assert_eq!(list.len(), 4);
        // 先頭 3 件は ID 0x10 でデュアルモノ展開
        assert_eq!(list[0].id, 0x10);
        assert_eq!(list[0].dual_mono, DualMonoMode::Main);
        assert_eq!(list[1].dual_mono, DualMonoMode::Sub);
        assert_eq!(list[2].dual_mono, DualMonoMode::Both);
        // 4 件目は EIT 不一致の無効コンポーネント
        assert_eq!(list[3].id, make_id(1, COMPONENT_TAG_INVALID));
        assert_eq!(list[3].component_type, COMPONENT_TYPE_INVALID);
        assert_eq!(list[3].dual_mono, DualMonoMode::Invalid);
    }

    #[test]
    fn default_audio_empty_is_none() {
        let m = AudioManager::new();
        assert!(m.default_audio(false, &[]).is_none());
    }

    #[test]
    fn default_audio_fallback_front() {
        let mut m = AudioManager::new();
        m.set_event_audio_list(Vec::new());
        m.set_component_list(vec![make_id(0, 0x10), make_id(1, 0x11)]);
        // 言語優先無効 -> 先頭
        let (idx, sel) = m.default_audio(false, &[]).unwrap();
        assert_eq!(idx, 0);
        assert_eq!(sel.id, 0x10);
        assert_eq!(sel.dual_mono, DualMonoMode::Invalid); // 非デュアルモノ
    }

    #[test]
    fn default_audio_language_priority() {
        let mut m = AudioManager::new();
        let events = build_event_audio_list(&[
            EventAudioInfo {
                component_type: 0x01,
                component_tag: 0x10,
                es_multi_lingual_flag: false,
                language_code: 0x6A_70_6E, // jpn
                language_code2: 0,
                text: w("日本語"),
            },
            EventAudioInfo {
                component_type: 0x01,
                component_tag: 0x11,
                es_multi_lingual_flag: false,
                language_code: 0x65_6E_67, // eng
                language_code2: 0,
                text: w("英語"),
            },
        ]);
        m.set_event_audio_list(events);
        m.set_component_list(vec![make_id(0, 0x10), make_id(1, 0x11)]);
        // eng を優先 -> インデックス 1
        let priority = [AudioLanguagePriority { language: 0x65_6E_67, sub: false }];
        let (idx, sel) = m.default_audio(true, &priority).unwrap();
        assert_eq!(idx, 1);
        assert_eq!(sel.id, 0x11);
    }

    #[test]
    fn selection_get_set_and_find() {
        let mut m = AudioManager::new();
        m.set_event_audio_list(Vec::new());
        m.set_component_list(vec![make_id(0, 0x10), make_id(1, 0x11)]);

        assert!(!m.has_selected_audio());
        assert_eq!(m.find_selected_audio(), -1);

        m.set_selected_audio(Some(AudioSelectInfo {
            id: 0x11,
            dual_mono: DualMonoMode::Invalid,
        }));
        assert!(m.has_selected_audio());
        assert_eq!(m.selected_id(), 0x11);
        assert_eq!(m.find_selected_audio(), 1);

        // モード設定
        assert!(m.set_selected_dual_mono_mode(DualMonoMode::Sub));
        assert_eq!(m.selected_dual_mono_mode(), DualMonoMode::Sub);

        // リセット
        m.set_selected_audio(None);
        assert!(!m.has_selected_audio());
        assert_eq!(m.selected_id(), ID_INVALID);
    }

    #[test]
    fn audio_select_info_by_id_dual_mono() {
        let mut m = AudioManager::new();
        let events = build_event_audio_list(&[EventAudioInfo {
            component_type: 0x02,
            component_tag: 0x10,
            es_multi_lingual_flag: false,
            language_code: 0x6A_70_6E,
            language_code2: 0,
            text: w("m\rs"),
        }]);
        m.set_event_audio_list(events);
        m.set_component_list(vec![make_id(0, 0x10)]);
        // デュアルモノ ID -> 選択モード未設定なら Main
        let sel = m.audio_select_info_by_id(0x10).unwrap();
        assert_eq!(sel.dual_mono, DualMonoMode::Main);
        // 未知 ID
        assert!(m.audio_select_info_by_id(0x99).is_none());
    }

    #[test]
    fn on_service_changed_saves_and_restores_selection() {
        let mut m = AudioManager::new();
        // サービス(1,2)へ。コンポーネント [0x10]
        assert!(m.on_service_changed(vec![0x10], 1, 2));
        m.set_selected_id(0x10);

        // サービス(3,4)へ移動 -> (1,2) の選択を保存・選択リセット
        assert!(m.on_service_changed(vec![0x10], 3, 4));
        assert_eq!(m.selected_id(), ID_INVALID);

        // サービス(1,2)へ戻る -> 保存していた選択(0x10)を復元
        assert!(m.on_service_changed(vec![0x10], 1, 2));
        assert_eq!(m.selected_id(), 0x10);
    }

    #[test]
    fn on_service_changed_same_service_resets_lost_stream() {
        let mut m = AudioManager::new();
        m.on_service_changed(vec![0x10], 1, 2);
        m.set_selected_id(0x10);
        // 同一サービスでコンポーネントが 0x20 に変化 -> 0x10 が消えたのでリセット
        assert!(m.on_service_changed(vec![0x20], 1, 2));
        assert_eq!(m.selected_id(), ID_INVALID);
    }

    #[test]
    fn on_service_changed_no_change_returns_false() {
        let mut m = AudioManager::new();
        m.on_service_changed(vec![0x10], 1, 2);
        // 同一コンポーネント・同一サービス -> 変化なし
        assert!(!m.on_service_changed(vec![0x10], 1, 2));
    }

    #[test]
    fn on_event_changed_rebuilds_on_list_change() {
        let mut m = AudioManager::new();
        m.set_component_list(vec![make_id(0, 0x10)]);
        let events = build_event_audio_list(&[EventAudioInfo {
            component_type: 0x01,
            component_tag: 0x10,
            es_multi_lingual_flag: false,
            language_code: 0x6A_70_6E,
            language_code2: 0,
            text: w("音声"),
        }]);
        // 初回イベント
        assert!(m.on_event_changed(events.clone(), 0x1000));
        assert_eq!(m.audio_list()[0].text, w("音声"));
        // 同じリスト・同じイベント -> 変化なし
        assert!(!m.on_event_changed(events, 0x1000));
    }
}
