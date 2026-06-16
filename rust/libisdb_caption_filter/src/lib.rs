// Rust port of LibISDB/Filters/CaptionFilter.cpp + CaptionFilter.hpp
//
// 字幕フィルタ。SingleIOFilter として、PAT/PMT を追跡して字幕 ES を見つけ、
// 対象サービス/コンポーネントの字幕を解析してハンドラへ通知する。データは
// そのまま下流へ流す(パススルー)。
//
// 原実装との対応:
//   - C++ の SingleIOFilter 継承 → FilterBase + FilterSink(ProcessData 後にパススルー出力)
//   - C++ の PIDMapManager + CaptionStream(PIDMapTarget) → 自己参照コールバックを避け、
//     本フィルタが PATTable + PMT テーブル群 + 字幕 ES ごとの (PesParser + CaptionParser) を保持
//   - C++ の CaptionParser::StorePacket は TS→PES 組立を内部で行うが、Rust 版 CaptionParser は
//     PES ペイロード入力のため、本フィルタが PesParser で TS→PES 組立してから parse_pes_payload
//   - C++ の Handler(OnLanguageUpdate/OnCaption)+ DRCSMap → 単一の CaptionFilterHandler trait
//     (Rust CaptionParser は DRCS も CaptionHandler 経由で通知するため統合)

use std::collections::HashMap;

use libisdb_arib_string::FormatInfo;
use libisdb_caption_parser::{CaptionHandler, CaptionParser, DrcsBitmap, LanguageInfo};
use libisdb_descriptor::StreamIdDescriptor;
use libisdb_filter_base::{DataStream, FilterBase, FilterSink, OutputSlot};
use libisdb_pes_packet::{PesPacket, PesParser};
use libisdb_ts_info::{is_1seg_pmt_pid, PID_INVALID, PID_PAT, STREAM_TYPE_CAPTION};
use libisdb_ts_packet::{TsPacket, TS_PACKET_SIZE};
use libisdb_ts_tables::{PATTable, PMTTable};

/// 無効なサービス ID。LibISDBConsts.hpp:39。
pub const SERVICE_ID_INVALID: u16 = 0x0000;
/// 全コンポーネント対象を表すコンポーネントタグ。
pub const COMPONENT_TAG_ALL: u8 = 0xFF;

/// 字幕フィルタが上位へ通知するハンドラ。CaptionFilter.hpp:47 Handler。
///
/// 原実装の Handler(OnLanguageUpdate/OnCaption)と DRCSMap を統合している
/// (Rust の CaptionParser は DRCS も CaptionHandler 経由で通知するため)。
pub trait CaptionFilterHandler {
    /// 言語情報が更新された。OnLanguageUpdate。
    fn on_language_update(&mut self, _languages: &[LanguageInfo]) {}
    /// 字幕本文が得られた。OnCaption。
    fn on_caption(&mut self, _language: u8, _text: &str, _format_list: &[FormatInfo]) {}
    /// DRCS(外字)が得られた。
    fn on_drcs(&mut self, _character_code: u16, _bitmap: &DrcsBitmap) {}
}

/// 字幕 ES 情報。CaptionFilter.hpp:98 CaptionESInfo。
#[derive(Clone, Copy, Debug)]
struct CaptionEsInfo {
    pid: u16,
    component_tag: u8,
}

/// サービス情報。CaptionFilter.hpp:103 ServiceInfo。
#[derive(Clone, Debug, Default)]
struct ServiceInfo {
    service_id: u16,
    caption_es_list: Vec<CaptionEsInfo>,
}

/// 字幕 ES ごとの解析状態(C++ の CaptionStream に相当)。
struct CaptionEsState {
    pes_parser: PesParser,
    caption_parser: CaptionParser,
}

/// CaptionParser から記録した字幕イベント(借用衝突回避のため一旦バッファする)。
enum CaptionEvent {
    LanguageUpdate(Vec<LanguageInfo>),
    Caption {
        language: u8,
        text: String,
        formats: Vec<FormatInfo>,
    },
    Drcs {
        character_code: u16,
        bitmap: DrcsBitmap,
    },
}

/// 字幕イベントを一旦バッファに記録する CaptionHandler。
#[derive(Default)]
struct RecordingHandler {
    events: Vec<CaptionEvent>,
}

impl CaptionHandler for RecordingHandler {
    fn on_language_update(&mut self, languages: &[LanguageInfo]) {
        self.events
            .push(CaptionEvent::LanguageUpdate(languages.to_vec()));
    }
    fn on_caption(&mut self, language: u8, text: &str, format_list: &[FormatInfo]) {
        self.events.push(CaptionEvent::Caption {
            language,
            text: text.to_string(),
            formats: format_list.to_vec(),
        });
    }
    fn on_drcs(&mut self, character_code: u16, bitmap: &DrcsBitmap) {
        self.events.push(CaptionEvent::Drcs {
            character_code,
            bitmap: bitmap.clone(),
        });
    }
}

/// 何もしない CaptionHandler(非対象 ES の解析用)。
struct NullHandler;
impl CaptionHandler for NullHandler {}

/// 字幕フィルタ。CaptionFilter.hpp:42。
pub struct CaptionFilter {
    service_list: Vec<ServiceInfo>,
    follow_active_service: bool,
    target_service_id: u16,
    target_component_tag: u8,
    target_es_pid: u16,

    pat_table: PATTable,
    pmt_tables: HashMap<u16, PMTTable>,
    caption_streams: HashMap<u16, CaptionEsState>,

    handler: Option<Box<dyn CaptionFilterHandler>>,

    output: OutputSlot,
}

impl Default for CaptionFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl CaptionFilter {
    /// CaptionFilter::CaptionFilter (cpp:82)
    pub fn new() -> Self {
        Self {
            service_list: Vec::new(),
            follow_active_service: true, // 原実装の既定値は true
            target_service_id: SERVICE_ID_INVALID,
            target_component_tag: COMPONENT_TAG_ALL,
            target_es_pid: PID_INVALID,

            pat_table: PATTable::new(),
            pmt_tables: HashMap::new(),
            caption_streams: HashMap::new(),

            handler: None,

            output: OutputSlot::new(),
        }
    }

    // ── 出力スロット接続 ───────────────────────────────────────

    pub fn connect_output(&mut self, sink: Box<dyn FilterSink>) {
        self.output.connect(sink);
    }
    pub fn disconnect_output(&mut self) {
        self.output.disconnect();
    }
    pub fn is_output_connected(&self) -> bool {
        self.output.is_connected()
    }

    // ── 設定/取得 (CaptionFilter.cpp:126-) ──────────────────────

    /// CaptionFilter::SetTargetStream (cpp:126)
    pub fn set_target_stream(&mut self, service_id: u16, component_tag: u8) -> bool {
        // 旧対象の解除は本移植では target_es_pid を更新するだけで足りる
        // (どの ES が対象かは解析時に target_es_pid と比較して判定するため)
        self.target_es_pid = PID_INVALID;

        if let Some(index) = self.get_service_index_by_id(service_id) {
            let mut target = PID_INVALID;
            {
                let caption_list = &self.service_list[index].caption_es_list;
                if component_tag == COMPONENT_TAG_ALL {
                    if let Some(first) = caption_list.first() {
                        target = first.pid;
                    }
                } else {
                    for e in caption_list {
                        if e.component_tag == component_tag {
                            target = e.pid;
                            break;
                        }
                    }
                }
            }
            // 字幕ストリームが存在する場合のみ対象に設定
            if target != PID_INVALID && self.caption_streams.contains_key(&target) {
                self.target_es_pid = target;
            }
        }

        self.target_service_id = service_id;
        self.target_component_tag = component_tag;
        true
    }

    pub fn get_target_service_id(&self) -> u16 {
        self.target_service_id
    }
    pub fn get_target_component_tag(&self) -> u8 {
        self.target_component_tag
    }

    /// CaptionFilter::SetFollowActiveService (cpp:177)
    pub fn set_follow_active_service(&mut self, follow: bool) {
        self.follow_active_service = follow;
    }
    pub fn get_follow_active_service(&self) -> bool {
        self.follow_active_service
    }

    /// CaptionFilter::SetActiveServiceID (cpp:110)
    pub fn set_active_service_id(&mut self, service_id: u16) {
        if self.follow_active_service {
            self.set_target_stream(service_id, COMPONENT_TAG_ALL);
        }
    }

    /// CaptionFilter::SetCaptionHandler (cpp:185)
    pub fn set_caption_handler(&mut self, handler: Option<Box<dyn CaptionFilterHandler>>) {
        self.handler = handler;
    }
    pub fn has_caption_handler(&self) -> bool {
        self.handler.is_some()
    }

    /// CaptionFilter::GetLanguageCount (cpp:205)
    pub fn get_language_count(&self) -> usize {
        self.current_caption_parser()
            .map_or(0, |p| p.language_count())
    }

    /// CaptionFilter::GetLanguageCode (cpp:217)
    pub fn get_language_code(&self, language_tag: u8) -> u32 {
        self.current_caption_parser()
            .map_or(0, |p| p.language_code_by_tag(language_tag))
    }

    // ── 内部処理 ───────────────────────────────────────────────

    /// CaptionFilter::GetServiceIndexByID (cpp:245)
    fn get_service_index_by_id(&self, service_id: u16) -> Option<usize> {
        self.service_list
            .iter()
            .position(|s| s.service_id == service_id)
    }

    /// CaptionFilter::GetCurrentCaptionParser (cpp:255)
    fn current_caption_parser(&self) -> Option<&CaptionParser> {
        if self.target_es_pid != PID_INVALID {
            self.caption_streams
                .get(&self.target_es_pid)
                .map(|s| &s.caption_parser)
        } else {
            None
        }
    }

    /// CaptionFilter::ProcessData (cpp:117) の1パケット処理
    fn process_packet(&mut self, arr: &[u8; TS_PACKET_SIZE]) {
        let mut pkt = TsPacket::new(arr);
        pkt.parse_packet(None);
        let pid = pkt.get_pid();

        if pid == PID_PAT {
            if self.pat_table.store_packet(&pkt) {
                self.on_pat_updated();
            }
        } else if self.pmt_tables.contains_key(&pid) {
            if self.pmt_tables.get_mut(&pid).unwrap().store_packet(&pkt) {
                self.on_pmt_updated(pid);
            }
        } else if self.caption_streams.contains_key(&pid) {
            self.feed_caption(pid, &pkt);
        }
    }

    /// 字幕 ES パケットを解析する(C++ の CaptionStream::StorePacket 相当)。
    fn feed_caption(&mut self, pid: u16, pkt: &TsPacket) {
        let is_target = pid == self.target_es_pid;
        let mut events: Vec<CaptionEvent> = Vec::new();

        {
            let state = self.caption_streams.get_mut(&pid).unwrap();
            // PesParser と CaptionParser を分割借用
            let CaptionEsState {
                pes_parser,
                caption_parser,
            } = state;

            if is_target {
                let mut rec = RecordingHandler::default();
                pes_parser.store_packet(pkt, &mut |pes: &PesPacket| {
                    if let Some(payload) = pes.get_payload_data() {
                        caption_parser.parse_pes_payload(payload, &mut rec);
                    }
                });
                events = rec.events;
            } else {
                let mut null = NullHandler;
                pes_parser.store_packet(pkt, &mut |pes: &PesPacket| {
                    if let Some(payload) = pes.get_payload_data() {
                        caption_parser.parse_pes_payload(payload, &mut null);
                    }
                });
            }
        }

        if !events.is_empty() {
            self.forward_events(events);
        }
    }

    fn forward_events(&mut self, events: Vec<CaptionEvent>) {
        if let Some(h) = self.handler.as_mut() {
            for ev in events {
                match ev {
                    CaptionEvent::LanguageUpdate(langs) => h.on_language_update(&langs),
                    CaptionEvent::Caption {
                        language,
                        text,
                        formats,
                    } => h.on_caption(language, &text, &formats),
                    CaptionEvent::Drcs {
                        character_code,
                        bitmap,
                    } => h.on_drcs(character_code, &bitmap),
                }
            }
        }
    }

    /// CaptionFilter::OnPATSection (cpp:268)
    fn on_pat_updated(&mut self) {
        // 現 PMT / 字幕 ES のマップを解除
        self.pmt_tables.clear();
        self.caption_streams.clear();
        self.target_es_pid = PID_INVALID;

        let program_count = self.pat_table.get_program_count();
        let mut new_list: Vec<ServiceInfo> = Vec::with_capacity(program_count);

        for i in 0..program_count {
            let service_id = self.pat_table.get_program_number(i);
            let pmt_pid = self.pat_table.get_pmt_pid(i);
            new_list.push(ServiceInfo {
                service_id,
                caption_es_list: Vec::new(),
            });
            self.pmt_tables.insert(pmt_pid, PMTTable::new());
        }

        self.service_list = new_list;
    }

    /// CaptionFilter::OnPMTSection (cpp:297)
    fn on_pmt_updated(&mut self, pmt_pid: u16) {
        let service_id = match self.pmt_tables.get(&pmt_pid) {
            Some(t) => t.get_program_number_id(),
            None => return,
        };
        let service_index = match self.get_service_index_by_id(service_id) {
            Some(i) => i,
            None => return,
        };

        // 字幕 ES を収集
        let caption_es: Vec<CaptionEsInfo> = {
            let pmt = self.pmt_tables.get(&pmt_pid).unwrap();
            pmt.get_es_list()
                .iter()
                .filter(|item| item.stream_type == STREAM_TYPE_CAPTION)
                .map(|item| {
                    let component_tag = item
                        .descriptors
                        .get_descriptor_by_tag(StreamIdDescriptor::TAG)
                        .and_then(StreamIdDescriptor::from_descriptor)
                        .map(|d| d.component_tag)
                        .unwrap_or(COMPONENT_TAG_ALL);
                    CaptionEsInfo {
                        pid: item.es_pid,
                        component_tag,
                    }
                })
                .collect()
        };

        // 字幕 ES の CaptionStream を再作成
        let one_seg = is_1seg_pmt_pid(pmt_pid);
        // 旧字幕ストリーム(このサービスの旧 ES)を除去
        for old in &self.service_list[service_index].caption_es_list {
            self.caption_streams.remove(&old.pid);
        }
        for es in &caption_es {
            self.caption_streams.insert(
                es.pid,
                CaptionEsState {
                    pes_parser: PesParser::new(),
                    caption_parser: CaptionParser::new(one_seg),
                },
            );
        }
        self.service_list[service_index].caption_es_list = caption_es;

        // 対象ストリームを再設定
        let (svc, tag) = (self.target_service_id, self.target_component_tag);
        self.set_target_stream(svc, tag);
    }
}

// ---------------------------------------------------------------------------
// FilterBase 実装
// ---------------------------------------------------------------------------

impl FilterBase for CaptionFilter {
    fn input_count(&self) -> usize {
        1
    }
    fn output_count(&self) -> usize {
        1
    }

    /// CaptionFilter::Reset (cpp:95)
    fn reset(&mut self) {
        self.service_list.clear();
        self.target_es_pid = PID_INVALID;
        self.pat_table.reset();
        self.pmt_tables.clear();
        self.caption_streams.clear();
    }
}

// ---------------------------------------------------------------------------
// FilterSink 実装(SingleIOFilter: ProcessData → OutputData パススルー)
// ---------------------------------------------------------------------------

impl FilterSink for CaptionFilter {
    /// CaptionFilter::ProcessData (cpp:117) → SingleIOFilter の OutputData パススルー
    fn receive_data(&mut self, stream: &mut dyn DataStream) -> bool {
        if stream.is_ts_packet() {
            loop {
                let data = stream.data();
                if data.len() >= TS_PACKET_SIZE {
                    let mut arr = [0u8; TS_PACKET_SIZE];
                    arr.copy_from_slice(&data[..TS_PACKET_SIZE]);
                    self.process_packet(&arr);
                }
                if !stream.next() {
                    break;
                }
            }
        }

        // パススルー出力(SingleIOFilter は ProcessData 後に必ず OutputData)
        self.output.send(stream);
        true
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use libisdb_crc::{crc16_ccitt, crc32_mpeg2};
    use libisdb_filter_base::{VecDataStream, TYPE_ID_TS_PACKET};
    use std::cell::RefCell;
    use std::rc::Rc;

    // ── TS / PSI / 字幕 PES ビルダー ────────────────────────────

    fn make_ts_packet(pid: u16, cc: u8, pusi: bool, payload: &[u8]) -> [u8; TS_PACKET_SIZE] {
        let mut p = [0xFFu8; TS_PACKET_SIZE];
        p[0] = 0x47;
        p[1] = ((pusi as u8) << 6) | ((pid >> 8) as u8 & 0x1F);
        p[2] = (pid & 0xFF) as u8;
        p[3] = 0x10 | (cc & 0x0F);
        let n = payload.len().min(184);
        p[4..4 + n].copy_from_slice(&payload[..n]);
        p
    }

    fn finalize_section(section: &mut Vec<u8>) {
        let section_length = (section.len() - 3 + 4) as u16;
        section[1] = 0xB0 | ((section_length >> 8) as u8);
        section[2] = (section_length & 0xFF) as u8;
        let crc = crc32_mpeg2(section, 0xFFFF_FFFF);
        section.extend_from_slice(&crc.to_be_bytes());
    }

    fn make_pat_packet(cc: u8, tsid: u16, programs: &[(u16, u16)]) -> [u8; TS_PACKET_SIZE] {
        let mut section = vec![
            0x00, 0x00, 0x00, (tsid >> 8) as u8, (tsid & 0xFF) as u8, 0xC1, 0x00, 0x00,
        ];
        for &(pn, pmt_pid) in programs {
            section.push((pn >> 8) as u8);
            section.push((pn & 0xFF) as u8);
            section.push(0xE0 | ((pmt_pid >> 8) as u8 & 0x1F));
            section.push((pmt_pid & 0xFF) as u8);
        }
        finalize_section(&mut section);
        let mut payload = vec![0x00u8];
        payload.extend_from_slice(&section);
        make_ts_packet(PID_PAT, cc, true, &payload)
    }

    fn stream_id_descriptor(component_tag: u8) -> Vec<u8> {
        vec![0x52, 0x01, component_tag]
    }

    /// es: (stream_type, es_pid, es_descriptors)
    fn make_pmt_packet(
        pid: u16,
        cc: u8,
        program_number: u16,
        pcr_pid: u16,
        es: &[(u8, u16, Vec<u8>)],
    ) -> [u8; TS_PACKET_SIZE] {
        let mut section = vec![
            0x02,
            0x00,
            0x00,
            (program_number >> 8) as u8,
            (program_number & 0xFF) as u8,
            0xC1,
            0x00,
            0x00,
            0xE0 | ((pcr_pid >> 8) as u8 & 0x1F),
            (pcr_pid & 0xFF) as u8,
            0xF0,
            0x00,
        ];
        for (stream_type, es_pid, desc) in es {
            section.push(*stream_type);
            section.push(0xE0 | ((*es_pid >> 8) as u8 & 0x1F));
            section.push((*es_pid & 0xFF) as u8);
            let dl = desc.len();
            section.push(0xF0 | ((dl >> 8) as u8 & 0x0F));
            section.push((dl & 0xFF) as u8);
            section.extend_from_slice(desc);
        }
        finalize_section(&mut section);
        let mut payload = vec![0x00u8];
        payload.extend_from_slice(&section);
        make_ts_packet(pid, cc, true, &payload)
    }

    /// 字幕 PES のペイロード(data_group)を組み立てる。
    fn build_caption_payload(data_group_id: u8, data_group_version: u8, body: &[u8]) -> Vec<u8> {
        let mut v = vec![0x80u8, 0xFF, 0x00]; // data_identifier / private_stream_id / header_length=0
        let mut dg = Vec::new();
        dg.push((data_group_id << 2) | (data_group_version & 0x03));
        dg.push(0x00);
        dg.push(0x00);
        dg.extend_from_slice(&(body.len() as u16).to_be_bytes());
        dg.extend_from_slice(body);
        let crc = crc16_ccitt(&dg, 0x0000);
        dg.extend_from_slice(&crc.to_be_bytes());
        v.extend_from_slice(&dg);
        v
    }

    /// 字幕管理データ本文(言語1件)。
    fn build_management_body() -> Vec<u8> {
        let mut b = Vec::new();
        b.push(0x00); // TMD=free
        b.push(0x01); // num_languages=1
        b.push((0u8 << 5) | 0x00); // language_tag=0, dmf=0
        b.push(0x00); // language_code[0] (jpn=0x6A706E)
        b.push(0x00);
        b.push(0x00);
        b.push(0x00); // format/tcs/rollup
        b.extend_from_slice(&[0x00, 0x00, 0x00]); // unit_loop_length=0
        b
    }

    /// 字幕 PES ペイロードを PES パケットに包む(private_stream_1 = 0xBD)。
    fn wrap_pes(caption_payload: &[u8]) -> Vec<u8> {
        let mut pes = vec![0x00u8, 0x00, 0x01, 0xBD];
        let packet_length = 3 + caption_payload.len(); // bytes[6,7,8] + payload
        pes.push((packet_length >> 8) as u8);
        pes.push((packet_length & 0xFF) as u8);
        pes.push(0x80); // '10' marker
        pes.push(0x00); // flags(no PTS)
        pes.push(0x00); // header_data_length=0
        pes.extend_from_slice(caption_payload);
        pes
    }

    fn make_caption_ts(pid: u16, cc: u8, caption_payload: &[u8]) -> [u8; TS_PACKET_SIZE] {
        let pes = wrap_pes(caption_payload);
        make_ts_packet(pid, cc, true, &pes)
    }

    // ── レコーダ系 ──────────────────────────────────────────────

    fn output_recorder() -> (Rc<RefCell<Vec<Vec<u8>>>>, Box<dyn FilterSink>) {
        let store = Rc::new(RefCell::new(Vec::new()));
        let st = store.clone();
        struct Rec(Rc<RefCell<Vec<Vec<u8>>>>);
        impl FilterSink for Rec {
            fn receive_data(&mut self, stream: &mut dyn DataStream) -> bool {
                self.0.borrow_mut().push(stream.data().to_vec());
                while stream.next() {
                    self.0.borrow_mut().push(stream.data().to_vec());
                }
                true
            }
        }
        (store, Box::new(Rec(st)))
    }

    #[derive(Default)]
    struct Counts {
        language_updates: u32,
        captions: Vec<(u8, String)>,
    }
    fn caption_handler() -> (Rc<RefCell<Counts>>, Box<dyn CaptionFilterHandler>) {
        let counts = Rc::new(RefCell::new(Counts::default()));
        let c = counts.clone();
        struct H(Rc<RefCell<Counts>>);
        impl CaptionFilterHandler for H {
            fn on_language_update(&mut self, _languages: &[LanguageInfo]) {
                self.0.borrow_mut().language_updates += 1;
            }
            fn on_caption(&mut self, language: u8, text: &str, _f: &[FormatInfo]) {
                self.0.borrow_mut().captions.push((language, text.to_string()));
            }
        }
        (counts, Box::new(H(c)))
    }

    fn feed(filter: &mut CaptionFilter, packets: &[[u8; TS_PACKET_SIZE]]) {
        let items: Vec<Vec<u8>> = packets.iter().map(|p| p.to_vec()).collect();
        let mut stream = VecDataStream::new(TYPE_ID_TS_PACKET, items);
        filter.receive_data(&mut stream);
    }

    // ── テスト ──────────────────────────────────────────────────

    #[test]
    fn test_passthrough() {
        let (out, sink) = output_recorder();
        let mut filter = CaptionFilter::new();
        filter.connect_output(sink);

        let pkts = [
            make_ts_packet(0x0100, 0, true, b"a"),
            make_ts_packet(0x0200, 0, true, b"b"),
        ];
        feed(&mut filter, &pkts);

        // 字幕フィルタは全データをパススルーする
        assert_eq!(out.borrow().len(), 2);
    }

    #[test]
    fn test_caption_es_detection_and_target() {
        let mut filter = CaptionFilter::new();
        let svc = 0x0030u16;
        let pmt = 0x0100u16;
        let caption_pid = 0x0140u16;

        feed(&mut filter, &[make_pat_packet(0, 0x1234, &[(svc, pmt)])]);
        feed(
            &mut filter,
            &[make_pmt_packet(
                pmt,
                0,
                svc,
                0x0111,
                &[
                    (0x02, 0x0111, vec![]),                          // 映像
                    (STREAM_TYPE_CAPTION, caption_pid, stream_id_descriptor(0x30)), // 字幕
                ],
            )],
        );

        // follow_active_service が既定 true なので、SetActiveServiceID で対象設定
        filter.set_active_service_id(svc);
        // 字幕言語はまだ来ていないので 0
        assert_eq!(filter.get_language_count(), 0);
    }

    #[test]
    fn test_full_caption_pipeline() {
        let (counts, handler) = caption_handler();
        let mut filter = CaptionFilter::new();
        filter.set_caption_handler(Some(handler));

        let svc = 0x0030u16;
        let pmt = 0x0100u16;
        let caption_pid = 0x0140u16;

        feed(&mut filter, &[make_pat_packet(0, 0x1234, &[(svc, pmt)])]);
        feed(
            &mut filter,
            &[make_pmt_packet(
                pmt,
                0,
                svc,
                0x0111,
                &[(STREAM_TYPE_CAPTION, caption_pid, stream_id_descriptor(0x30))],
            )],
        );

        // 対象サービスを選択(follow=true なので set_active でも可)
        filter.set_target_stream(svc, COMPONENT_TAG_ALL);
        assert_eq!(filter.get_target_service_id(), svc);

        // 字幕管理データ(言語更新)を投入
        let payload = build_caption_payload(0x00, 0x00, &build_management_body());
        feed(&mut filter, &[make_caption_ts(caption_pid, 0, &payload)]);

        // ハンドラに言語更新が通知される
        assert_eq!(counts.borrow().language_updates, 1);
        // 対象パーサーの言語数も 1
        assert_eq!(filter.get_language_count(), 1);
    }

    #[test]
    fn test_non_target_es_not_emitted() {
        let (counts, handler) = caption_handler();
        let mut filter = CaptionFilter::new();
        filter.set_follow_active_service(false); // 自動追従オフ
        filter.set_caption_handler(Some(handler));

        let svc = 0x0030u16;
        let pmt = 0x0100u16;
        let caption_pid = 0x0140u16;

        feed(&mut filter, &[make_pat_packet(0, 0x1234, &[(svc, pmt)])]);
        feed(
            &mut filter,
            &[make_pmt_packet(
                pmt,
                0,
                svc,
                0x0111,
                &[(STREAM_TYPE_CAPTION, caption_pid, stream_id_descriptor(0x30))],
            )],
        );
        // 対象未設定(target_es_pid = INVALID)

        let payload = build_caption_payload(0x00, 0x00, &build_management_body());
        feed(&mut filter, &[make_caption_ts(caption_pid, 0, &payload)]);

        // 対象でないのでハンドラには通知されない
        assert_eq!(counts.borrow().language_updates, 0);
    }

    #[test]
    fn test_target_by_component_tag() {
        let mut filter = CaptionFilter::new();
        filter.set_follow_active_service(false);

        let svc = 0x0030u16;
        let pmt = 0x0100u16;
        let cap1 = 0x0140u16;
        let cap2 = 0x0141u16;

        feed(&mut filter, &[make_pat_packet(0, 0x1234, &[(svc, pmt)])]);
        feed(
            &mut filter,
            &[make_pmt_packet(
                pmt,
                0,
                svc,
                0x0111,
                &[
                    (STREAM_TYPE_CAPTION, cap1, stream_id_descriptor(0x30)),
                    (STREAM_TYPE_CAPTION, cap2, stream_id_descriptor(0x38)),
                ],
            )],
        );

        // component_tag 0x38 を指定 → cap2 が対象
        assert!(filter.set_target_stream(svc, 0x38));
        // 対象 ES に字幕管理データを送ると言語が増える(cap2 のパーサー)
        let payload = build_caption_payload(0x00, 0x00, &build_management_body());
        feed(&mut filter, &[make_caption_ts(cap2, 0, &payload)]);
        assert_eq!(filter.get_language_count(), 1);

        // 全コンポーネント(0xFF)指定 → 先頭 cap1 が対象
        assert!(filter.set_target_stream(svc, COMPONENT_TAG_ALL));
        assert_eq!(filter.get_target_component_tag(), COMPONENT_TAG_ALL);
    }

    #[test]
    fn test_follow_active_service() {
        let mut filter = CaptionFilter::new();
        assert!(filter.get_follow_active_service()); // 既定 true

        let svc = 0x0030u16;
        let pmt = 0x0100u16;
        let caption_pid = 0x0140u16;
        feed(&mut filter, &[make_pat_packet(0, 0x1234, &[(svc, pmt)])]);
        feed(
            &mut filter,
            &[make_pmt_packet(
                pmt,
                0,
                svc,
                0x0111,
                &[(STREAM_TYPE_CAPTION, caption_pid, stream_id_descriptor(0x30))],
            )],
        );

        filter.set_active_service_id(svc);
        assert_eq!(filter.get_target_service_id(), svc);

        filter.set_follow_active_service(false);
        filter.set_active_service_id(0x0031);
        // 追従オフなので対象は変わらない
        assert_eq!(filter.get_target_service_id(), svc);
    }

    #[test]
    fn test_reset() {
        let mut filter = CaptionFilter::new();
        let svc = 0x0030u16;
        let pmt = 0x0100u16;
        let caption_pid = 0x0140u16;
        feed(&mut filter, &[make_pat_packet(0, 0x1234, &[(svc, pmt)])]);
        feed(
            &mut filter,
            &[make_pmt_packet(
                pmt,
                0,
                svc,
                0x0111,
                &[(STREAM_TYPE_CAPTION, caption_pid, stream_id_descriptor(0x30))],
            )],
        );
        filter.set_target_stream(svc, COMPONENT_TAG_ALL);

        filter.reset();

        // PSI クリア → 対象 ES も解除、言語数 0
        assert_eq!(filter.get_target_es_pid_for_test(), PID_INVALID);
        assert_eq!(filter.get_language_count(), 0);
        // 対象サービスは保持
        assert_eq!(filter.get_target_service_id(), svc);
    }

    #[test]
    fn test_no_handler_no_panic() {
        let mut filter = CaptionFilter::new();
        let svc = 0x0030u16;
        let pmt = 0x0100u16;
        let caption_pid = 0x0140u16;
        feed(&mut filter, &[make_pat_packet(0, 0x1234, &[(svc, pmt)])]);
        feed(
            &mut filter,
            &[make_pmt_packet(
                pmt,
                0,
                svc,
                0x0111,
                &[(STREAM_TYPE_CAPTION, caption_pid, stream_id_descriptor(0x30))],
            )],
        );
        filter.set_target_stream(svc, COMPONENT_TAG_ALL);

        // ハンドラ未設定でも字幕投入で panic しない(言語数は更新される)
        let payload = build_caption_payload(0x00, 0x00, &build_management_body());
        feed(&mut filter, &[make_caption_ts(caption_pid, 0, &payload)]);
        assert_eq!(filter.get_language_count(), 1);
    }

    #[test]
    fn test_filter_base_ports() {
        let filter = CaptionFilter::new();
        assert_eq!(filter.input_count(), 1);
        assert_eq!(filter.output_count(), 1);
    }

    // テスト用アクセサ
    impl CaptionFilter {
        fn get_target_es_pid_for_test(&self) -> u16 {
            self.target_es_pid
        }
    }
}
