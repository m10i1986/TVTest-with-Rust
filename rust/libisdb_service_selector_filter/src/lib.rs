// Rust port of LibISDB/Filters/ServiceSelectorFilter.cpp + ServiceSelectorFilter.hpp
//
// サービス選択フィルタ。SingleIOFilter として、StreamSelector を使い対象サービス/
// ストリーム種別のパケットのみを下流へ通す。対象が PAT のときは PAT を再生成する。
//
// 原実装との対応:
//   - C++ の SingleIOFilter 継承 → FilterBase + FilterSink を実装
//   - C++ の StreamSelector::InputPacket(内部 PIDMapManager で PAT/PMT/CAT をパース)
//     → Rust の StreamSelector は PSI パースを呼び出し側責務に分離しているため、
//       本フィルタが PATTable + PMT テーブル群 + CATTable を保持して PmtPidInfo/EMM PID を
//       構築し set_pmt_pid_list/set_emm_pid_list で StreamSelector に渡す。
//       パケット処置は decide_packet(Pass/Drop/RewritePat) + make_pat で再現。

use std::collections::HashMap;

use libisdb_descriptor::CaDescriptor;
use libisdb_filter_base::{DataStream, FilterBase, FilterSink, OutputSlot, VecDataStream};
use libisdb_stream_selector::{
    stream_flag, EsInfo, PacketAction, PmtPidInfo, StreamSelector, SERVICE_ID_INVALID,
};
use libisdb_ts_info::{PID_CAT, PID_INVALID, PID_PAT};
use libisdb_ts_packet::{TsPacket, TS_PACKET_SIZE};
use libisdb_ts_tables::{CATTable, PATTable, PMTTable};

/// サービス選択フィルタ。ServiceSelectorFilter.hpp:39。
pub struct ServiceSelectorFilter {
    target_service_id: u16,
    target_stream: u32,
    follow_active_service: bool,

    stream_selector: StreamSelector,

    // PSI 追跡(C++ の StreamSelector 内部 PIDMapManager + テーブル群に相当)
    pat_table: PATTable,
    cat_table: CATTable,
    pmt_tables: HashMap<u16, PMTTable>, // PMT PID → PMT テーブル
    /// StreamSelector へ渡す PMT PID 情報の作業用コピー(真の source of truth)
    pmt_info_list: Vec<PmtPidInfo>,

    output: OutputSlot,
}

impl Default for ServiceSelectorFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl ServiceSelectorFilter {
    /// ServiceSelectorFilter::ServiceSelectorFilter (cpp:39)
    pub fn new() -> Self {
        Self {
            target_service_id: SERVICE_ID_INVALID,
            target_stream: stream_flag::ALL,
            follow_active_service: false,

            stream_selector: StreamSelector::new(),

            pat_table: PATTable::new(),
            cat_table: CATTable::new(),
            pmt_tables: HashMap::new(),
            pmt_info_list: Vec::new(),

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

    // ── 設定/取得 (ServiceSelectorFilter.cpp:86-) ───────────────

    /// ServiceSelectorFilter::SetTargetServiceID (cpp:86)
    pub fn set_target_service_id(&mut self, service_id: u16, stream: u32) -> bool {
        if self.target_service_id != service_id || self.target_stream != stream {
            self.target_service_id = service_id;
            self.target_stream = stream;
            self.stream_selector.set_target_flags(service_id, stream);
        }
        true
    }

    pub fn get_target_service_id(&self) -> u16 {
        self.target_service_id
    }
    pub fn get_target_stream(&self) -> u32 {
        self.target_stream
    }

    /// ServiceSelectorFilter::SetFollowActiveService (cpp:107)
    pub fn set_follow_active_service(&mut self, follow: bool) {
        self.follow_active_service = follow;
    }
    pub fn get_follow_active_service(&self) -> bool {
        self.follow_active_service
    }

    /// ServiceSelectorFilter::SetActiveServiceID (cpp:55)
    pub fn set_active_service_id(&mut self, service_id: u16) {
        if self.follow_active_service {
            self.set_target_service_id(service_id, self.target_stream);
        }
    }

    // ── 内部処理 ───────────────────────────────────────────────

    /// StreamSelector::InputPacket (StreamSelector.cpp:76) 相当。
    /// 出力すべきパケットを返す(None なら破棄)。
    fn input_packet(&mut self, arr: &[u8; TS_PACKET_SIZE]) -> Option<[u8; TS_PACKET_SIZE]> {
        let mut pkt = TsPacket::new(arr);
        pkt.parse_packet(None);
        let pid = pkt.get_pid();

        // PAT/PMT/CAT を更新(m_PIDMapManager.StorePacket 相当)
        self.store_psi(&pkt, pid);

        match self.stream_selector.decide_packet(pid) {
            PacketAction::Pass => Some(*arr),
            PacketAction::Drop => None,
            PacketAction::RewritePat => {
                // MakePAT が失敗した場合は原実装どおり元パケットを通す
                match self.stream_selector.make_pat(arr) {
                    Some(pat) => Some(pat),
                    None => Some(*arr),
                }
            }
        }
    }

    fn store_psi(&mut self, pkt: &TsPacket, pid: u16) {
        if pid == PID_PAT {
            if self.pat_table.store_packet(pkt) {
                self.on_pat_updated();
            }
        } else if pid == PID_CAT {
            if self.cat_table.store_packet(pkt) {
                self.on_cat_updated();
            }
        } else if self.pmt_tables.contains_key(&pid) {
            if self.pmt_tables.get_mut(&pid).unwrap().store_packet(pkt) {
                self.on_pmt_updated(pid);
            }
        }
    }

    /// StreamSelector::OnPATSection (StreamSelector.cpp:185)
    fn on_pat_updated(&mut self) {
        let program_count = self.pat_table.get_program_count();
        // 既存サービスの ES/ECM/PCR 情報を保持するため旧リストを退避
        let old_list = std::mem::take(&mut self.pmt_info_list);
        let mut new_list: Vec<PmtPidInfo> = Vec::with_capacity(program_count);
        let mut new_tables: HashMap<u16, PMTTable> = HashMap::new();

        for i in 0..program_count {
            let service_id = self.pat_table.get_program_number(i);
            let pmt_pid = self.pat_table.get_pmt_pid(i);

            // 既存エントリ(同一 service_id、末尾優先)を引き継ぐ
            let mut info = match old_list.iter().rev().find(|e| e.service_id == service_id) {
                Some(existing) => existing.clone(),
                None => PmtPidInfo::new(service_id, pmt_pid),
            };
            info.service_id = service_id;
            info.pmt_pid = pmt_pid;
            new_list.push(info);

            new_tables.insert(pmt_pid, PMTTable::new());
        }

        self.pmt_info_list = new_list;
        self.pmt_tables = new_tables;
        self.stream_selector.set_pmt_pid_list(self.pmt_info_list.clone());
    }

    /// StreamSelector::OnPMTSection (StreamSelector.cpp:226)
    fn on_pmt_updated(&mut self, pmt_pid: u16) {
        let service_id = match self.pmt_tables.get(&pmt_pid) {
            Some(t) => t.get_program_number_id(),
            None => return,
        };
        let idx = match self.pmt_info_list.iter().rposition(|e| e.service_id == service_id) {
            Some(i) => i,
            None => return,
        };

        // PMT から PCR / ECM / ES を収集
        let (pcr_pid, ecm, es_list) = {
            let pmt = self.pmt_tables.get(&pmt_pid).unwrap();

            let pcr = pmt.get_pcr_pid();
            let pcr_pid = if pcr < 0x1FFF { pcr } else { PID_INVALID };

            let mut ecm = Vec::new();
            for desc in pmt.get_descriptor_block().iter() {
                if let Some(ca) = CaDescriptor::from_descriptor(desc) {
                    if ca.ca_pid < 0x1FFF {
                        ecm.push(ca.ca_pid);
                    }
                }
            }

            let es_list: Vec<EsInfo> = (0..pmt.get_es_count())
                .map(|i| EsInfo {
                    stream_type: pmt.get_stream_type(i),
                    pid: pmt.get_es_pid(i),
                })
                .collect();

            (pcr_pid, ecm, es_list)
        };

        {
            let info = &mut self.pmt_info_list[idx];
            info.pcr_pid = pcr_pid;
            info.ecm_pid_list = ecm;
            info.es_list = es_list;
        }
        self.stream_selector.set_pmt_pid_list(self.pmt_info_list.clone());
    }

    /// StreamSelector::OnCATSection (StreamSelector.cpp:267)
    fn on_cat_updated(&mut self) {
        let mut emm = Vec::new();
        for desc in self.cat_table.get_descriptor_block().iter() {
            if let Some(ca) = CaDescriptor::from_descriptor(desc) {
                if ca.ca_pid < 0x1FFF {
                    emm.push(ca.ca_pid);
                }
            }
        }
        self.stream_selector.set_emm_pid_list(emm);
    }
}

// ---------------------------------------------------------------------------
// FilterBase 実装
// ---------------------------------------------------------------------------

impl FilterBase for ServiceSelectorFilter {
    fn input_count(&self) -> usize {
        1
    }
    fn output_count(&self) -> usize {
        1
    }

    /// ServiceSelectorFilter::Reset (cpp:47)
    /// 原実装は m_StreamSelector.Reset() のみだが、本移植では PSI テーブルも保持しているため
    /// それらもリセットする(StreamSelector::Reset が内部テーブルを再構築するのと等価)。
    /// 対象サービス/ストリームは保持される(原実装の StreamSelector::Reset と同じ)。
    fn reset(&mut self) {
        self.stream_selector.reset();
        self.pat_table.reset();
        self.cat_table.reset();
        self.pmt_tables.clear();
        self.pmt_info_list.clear();
    }
}

// ---------------------------------------------------------------------------
// FilterSink 実装(SingleIOFilter)
// ---------------------------------------------------------------------------

impl FilterSink for ServiceSelectorFilter {
    /// ServiceSelectorFilter::ReceiveData (cpp:64)
    fn receive_data(&mut self, stream: &mut dyn DataStream) -> bool {
        if !stream.is_ts_packet() {
            // 非 TSPacket は出力しない(原実装も m_PacketSequence が空のまま)
            return true;
        }

        let type_id = stream.type_id();
        let mut out_seq: Vec<Vec<u8>> = Vec::new();

        loop {
            let data = stream.data();
            if data.len() >= TS_PACKET_SIZE {
                let mut arr = [0u8; TS_PACKET_SIZE];
                arr.copy_from_slice(&data[..TS_PACKET_SIZE]);
                if let Some(out) = self.input_packet(&arr) {
                    out_seq.push(out.to_vec());
                }
            }
            if !stream.next() {
                break;
            }
        }

        if !out_seq.is_empty() {
            let mut out_stream = VecDataStream::new(type_id, out_seq);
            self.output.send(&mut out_stream);
        }

        true
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use libisdb_crc::crc32_mpeg2;
    use libisdb_filter_base::{VecDataStream as VDS, TYPE_ID_TS_PACKET};
    use std::cell::RefCell;
    use std::rc::Rc;

    /// 指定したバイト列の TS パケット PID を取り出す。
    #[inline]
    fn pid_of(data: &[u8]) -> u16 {
        ((data[1] as u16 & 0x1F) << 8) | data[2] as u16
    }

    // ── テスト用 TS/PSI パケットビルダー ────────────────────────

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
            0x00,
            0x00,
            0x00,
            (tsid >> 8) as u8,
            (tsid & 0xFF) as u8,
            0xC1,
            0x00,
            0x00,
        ];
        for &(program_number, pmt_pid) in programs {
            section.push((program_number >> 8) as u8);
            section.push((program_number & 0xFF) as u8);
            section.push(0xE0 | ((pmt_pid >> 8) as u8 & 0x1F));
            section.push((pmt_pid & 0xFF) as u8);
        }
        finalize_section(&mut section);
        let mut payload = vec![0x00u8];
        payload.extend_from_slice(&section);
        make_ts_packet(PID_PAT, cc, true, &payload)
    }

    /// CA 記述子(tag=0x09)を構築する。
    fn ca_descriptor(ca_system_id: u16, ca_pid: u16) -> Vec<u8> {
        vec![
            0x09,
            0x04,
            (ca_system_id >> 8) as u8,
            (ca_system_id & 0xFF) as u8,
            0xE0 | ((ca_pid >> 8) as u8 & 0x1F),
            (ca_pid & 0xFF) as u8,
        ]
    }

    fn make_pmt_packet(
        pid: u16,
        cc: u8,
        program_number: u16,
        pcr_pid: u16,
        prog_desc: &[u8],
        es: &[(u8, u16)],
    ) -> [u8; TS_PACKET_SIZE] {
        let pil = prog_desc.len();
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
            0xF0 | ((pil >> 8) as u8 & 0x0F),
            (pil & 0xFF) as u8,
        ];
        section.extend_from_slice(prog_desc);
        for &(stream_type, es_pid) in es {
            section.push(stream_type);
            section.push(0xE0 | ((es_pid >> 8) as u8 & 0x1F));
            section.push((es_pid & 0xFF) as u8);
            section.push(0xF0);
            section.push(0x00);
        }
        finalize_section(&mut section);
        let mut payload = vec![0x00u8];
        payload.extend_from_slice(&section);
        make_ts_packet(pid, cc, true, &payload)
    }

    fn make_cat_packet(cc: u8, descriptors: &[u8]) -> [u8; TS_PACKET_SIZE] {
        let mut section = vec![0x01, 0x00, 0x00, 0xFF, 0xFF, 0xC1, 0x00, 0x00];
        section.extend_from_slice(descriptors);
        finalize_section(&mut section);
        let mut payload = vec![0x00u8];
        payload.extend_from_slice(&section);
        make_ts_packet(PID_CAT, cc, true, &payload)
    }

    // ── 出力レコーダ ────────────────────────────────────────────

    fn recorder() -> (Rc<RefCell<Vec<Vec<u8>>>>, Box<dyn FilterSink>) {
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

    /// 1パケット1要素で供給する。
    fn feed(filter: &mut ServiceSelectorFilter, packets: &[[u8; TS_PACKET_SIZE]]) {
        let items: Vec<Vec<u8>> = packets.iter().map(|p| p.to_vec()).collect();
        let mut stream = VDS::new(TYPE_ID_TS_PACKET, items);
        filter.receive_data(&mut stream);
    }

    fn output_pids(store: &Rc<RefCell<Vec<Vec<u8>>>>) -> Vec<u16> {
        store.borrow().iter().map(|p| pid_of(p)).collect()
    }

    // ── テスト ──────────────────────────────────────────────────

    #[test]
    fn test_passthrough_when_no_target() {
        let (store, sink) = recorder();
        let mut filter = ServiceSelectorFilter::new();
        filter.connect_output(sink);

        // 対象未設定(SERVICE_ID_INVALID, ALL)→ 全パケット通過
        let pkts = [
            make_ts_packet(0x0100, 0, true, b"a"),
            make_ts_packet(0x0200, 0, true, b"b"),
        ];
        feed(&mut filter, &pkts);

        assert_eq!(output_pids(&store), vec![0x0100, 0x0200]);
    }

    #[test]
    fn test_select_target_service() {
        let (store, sink) = recorder();
        let mut filter = ServiceSelectorFilter::new();
        filter.connect_output(sink);

        let svc_a = 0x0030u16;
        let svc_b = 0x0031u16;
        let pmt_a = 0x0100u16;
        let pmt_b = 0x0200u16;
        let video_a = 0x0111u16;
        let video_b = 0x0211u16;

        // PSI 投入
        feed(
            &mut filter,
            &[make_pat_packet(0, 0x1234, &[(svc_a, pmt_a), (svc_b, pmt_b)])],
        );
        feed(
            &mut filter,
            &[make_pmt_packet(pmt_a, 0, svc_a, video_a, &[], &[(0x02, video_a)])],
        );
        feed(
            &mut filter,
            &[make_pmt_packet(pmt_b, 0, svc_b, video_b, &[], &[(0x02, video_b)])],
        );

        // サービス A を選択(全ストリーム)
        assert!(filter.set_target_service_id(svc_a, stream_flag::ALL));

        // 各種パケットを投入
        store.borrow_mut().clear();
        feed(
            &mut filter,
            &[
                make_ts_packet(video_a, 1, true, b"va"), // 対象 ES → 通過
                make_ts_packet(video_b, 1, true, b"vb"), // 対象外 ES → 破棄
                make_ts_packet(0x0010, 0, false, b"nit"), // PID < 0x30 → 通過
            ],
        );

        let pids = output_pids(&store);
        assert!(pids.contains(&video_a), "対象サービスの映像は通過");
        assert!(!pids.contains(&video_b), "対象外サービスの映像は破棄");
        assert!(pids.contains(&0x0010), "PID<0x30 は常時通過");
    }

    #[test]
    fn test_stream_type_filtering() {
        let (store, sink) = recorder();
        let mut filter = ServiceSelectorFilter::new();
        filter.connect_output(sink);

        let svc = 0x0030u16;
        let pmt = 0x0100u16;
        let video = 0x0111u16;
        let audio = 0x0112u16;

        feed(&mut filter, &[make_pat_packet(0, 0x1234, &[(svc, pmt)])]);
        // 映像(stream_type 0x02 = MPEG2_VIDEO) + 音声(0x0F = AAC)
        feed(
            &mut filter,
            &[make_pmt_packet(pmt, 0, svc, video, &[], &[(0x02, video), (0x0F, audio)])],
        );

        // 映像のみ選択
        filter.set_target_service_id(svc, stream_flag::VIDEO);

        store.borrow_mut().clear();
        feed(
            &mut filter,
            &[
                make_ts_packet(video, 1, true, b"v"),
                make_ts_packet(audio, 1, true, b"a"),
            ],
        );

        let pids = output_pids(&store);
        assert!(pids.contains(&video), "映像は通過");
        assert!(!pids.contains(&audio), "音声は破棄(VIDEO のみ選択)");
    }

    #[test]
    fn test_ecm_emm_pass() {
        let (store, sink) = recorder();
        let mut filter = ServiceSelectorFilter::new();
        filter.connect_output(sink);

        let svc = 0x0030u16;
        let pmt = 0x0100u16;
        let video = 0x0111u16;
        let ecm_pid = 0x0123u16;
        let emm_pid = 0x0085u16;

        feed(&mut filter, &[make_pat_packet(0, 0x1234, &[(svc, pmt)])]);
        // PMT にプログラム ECM(CA 記述子)を付与
        feed(
            &mut filter,
            &[make_pmt_packet(
                pmt,
                0,
                svc,
                video,
                &ca_descriptor(0x0005, ecm_pid),
                &[(0x02, video)],
            )],
        );
        // CAT に EMM(CA 記述子)
        feed(&mut filter, &[make_cat_packet(0, &ca_descriptor(0x0005, emm_pid))]);

        filter.set_target_service_id(svc, stream_flag::ALL);

        store.borrow_mut().clear();
        feed(
            &mut filter,
            &[
                make_ts_packet(ecm_pid, 0, true, b"ecm"),
                make_ts_packet(emm_pid, 0, true, b"emm"),
                make_ts_packet(0x0500, 0, true, b"other"), // 対象外 → 破棄
            ],
        );

        let pids = output_pids(&store);
        assert!(pids.contains(&ecm_pid), "対象サービスの ECM は通過");
        assert!(pids.contains(&emm_pid), "EMM は通過");
        assert!(!pids.contains(&0x0500), "無関係 PID は破棄");
    }

    #[test]
    fn test_pat_rewrite() {
        let (store, sink) = recorder();
        let mut filter = ServiceSelectorFilter::new();
        filter.connect_output(sink);

        let svc_a = 0x0030u16;
        let svc_b = 0x0031u16;
        let pmt_a = 0x0100u16;
        let pmt_b = 0x0200u16;

        let pat = make_pat_packet(0, 0x1234, &[(svc_a, pmt_a), (svc_b, pmt_b)]);
        feed(&mut filter, &[pat]);
        feed(
            &mut filter,
            &[make_pmt_packet(pmt_a, 0, svc_a, 0x0111, &[], &[(0x02, 0x0111)])],
        );

        // サービス A を選択 → PAT 再生成が有効
        filter.set_target_service_id(svc_a, stream_flag::ALL);

        store.borrow_mut().clear();
        feed(&mut filter, &[pat]); // 再び PAT を投入 → 再生成される

        let recorded = store.borrow();
        assert_eq!(recorded.len(), 1);
        let out_pat = &recorded[0];
        assert_eq!(pid_of(out_pat), PID_PAT);

        // 再生成 PAT のプログラムループには対象 PMT(pmt_a)のみ + NIT(0x0010)が残る
        // payload: [4]=pointer_field(0), [5]=table_id, [6,7]=length, [8,9]=tsid,
        //          [10]=version, [11]=secnum, [12]=lastsec, [13..]=programs
        let sec = 5usize;
        let section_length = (((out_pat[sec + 1] & 0x0F) as usize) << 8) | out_pat[sec + 2] as usize;
        // program loop の長さ = section_length - (5 + 4)
        let loop_len = section_length - 9;
        let mut found_pmt_a = false;
        let mut found_pmt_b = false;
        let prog_off = sec + 8;
        let mut pos = 0;
        while pos < loop_len {
            let p = ((out_pat[prog_off + pos + 2] as u16 & 0x1F) << 8)
                | out_pat[prog_off + pos + 3] as u16;
            if p == pmt_a {
                found_pmt_a = true;
            }
            if p == pmt_b {
                found_pmt_b = true;
            }
            pos += 4;
        }
        assert!(found_pmt_a, "対象 PMT は残る");
        assert!(!found_pmt_b, "対象外 PMT は除去される");
    }

    #[test]
    fn test_follow_active_service() {
        let mut filter = ServiceSelectorFilter::new();
        filter.set_follow_active_service(true);

        // FollowActiveService 有効時は SetActiveServiceID で対象が変わる
        filter.set_active_service_id(0x0030);
        assert_eq!(filter.get_target_service_id(), 0x0030);

        // 無効時は変わらない
        filter.set_follow_active_service(false);
        filter.set_active_service_id(0x0031);
        assert_eq!(filter.get_target_service_id(), 0x0030);
    }

    #[test]
    fn test_set_target_change_detection() {
        let mut filter = ServiceSelectorFilter::new();
        assert_eq!(filter.get_target_service_id(), SERVICE_ID_INVALID);
        assert_eq!(filter.get_target_stream(), stream_flag::ALL);

        filter.set_target_service_id(0x0030, stream_flag::VIDEO);
        assert_eq!(filter.get_target_service_id(), 0x0030);
        assert_eq!(filter.get_target_stream(), stream_flag::VIDEO);
    }

    #[test]
    fn test_reset_keeps_target_clears_psi() {
        let (store, sink) = recorder();
        let mut filter = ServiceSelectorFilter::new();
        filter.connect_output(sink);

        let svc = 0x0030u16;
        let pmt = 0x0100u16;
        let video = 0x0111u16;
        feed(&mut filter, &[make_pat_packet(0, 0x1234, &[(svc, pmt)])]);
        feed(&mut filter, &[make_pmt_packet(pmt, 0, svc, video, &[], &[(0x02, video)])]);
        filter.set_target_service_id(svc, stream_flag::ALL);

        filter.reset();

        // 対象サービスは保持される
        assert_eq!(filter.get_target_service_id(), svc);

        // PSI はクリアされたので、PAT 再投入前は対象 ES の通過テーブルが無い
        // (video パケットは破棄される)
        store.borrow_mut().clear();
        feed(&mut filter, &[make_ts_packet(video, 5, true, b"v")]);
        assert!(output_pids(&store).is_empty(), "reset 後 PSI 未取得なら対象 ES は破棄");
    }

    #[test]
    fn test_non_ts_stream_no_output() {
        let (store, sink) = recorder();
        let mut filter = ServiceSelectorFilter::new();
        filter.connect_output(sink);

        let data = vec![0u8; TS_PACKET_SIZE];
        let mut stream = VDS::new(0, vec![data]); // TYPE_ID_DATA_BUFFER
        filter.receive_data(&mut stream);

        assert!(store.borrow().is_empty());
    }

    #[test]
    fn test_filter_base_ports() {
        let filter = ServiceSelectorFilter::new();
        assert_eq!(filter.input_count(), 1);
        assert_eq!(filter.output_count(), 1);
    }
}
