// Rust port of LibISDB/Filters/TSPacketCounterFilter.cpp + TSPacketCounterFilter.hpp
//
// TSパケットカウンタフィルタ。SingleIOFilter として、入力 TS パケットを数えつつ
// 下流へそのまま渡す(パススルー)。PAT/PMT を追跡してサービスの ES PID を把握し、
// 対象サービスの映像/音声 PID のビットレートとスクランブルパケット数を計測する。
//
// 原実装との対応:
//   - C++ の SingleIOFilter 継承 → FilterBase + FilterSink を実装(ProcessData 後にパススルー出力)
//   - C++ の PIDMapManager + PSITable コールバック(OnPATSection/OnPMTSection/ESPIDMapTarget)
//     → 自己参照コールバックを避け、本フィルタが PATTable + PMT テーブル群 + ES PID 集合を
//       直接保持し、PID でルーティングして同等の挙動を実現
//       (libisdb_pmt_analyzer 等と同じく、PSI コールバック構造を平坦化する方式)
//   - C++ の std::atomic カウンタ → 本フィルタは単一スレッド所有のため通常の u64
//
// 重要な挙動(原実装由来):
//   - スクランブル計数は2系統で排他: 対象サービス未設定時は ProcessData が全パケットを数え、
//     対象サービス設定時はその ES PID のみ(ESPIDMapTarget 相当)が数える。
//   - OnPATSection/OnPMTSection はセクション更新(バージョン変化)時にのみ発火する
//     (CreateWithHandler のハンドラ意味論)。store_packet の戻り値 true で判定。

use std::collections::{HashMap, HashSet};

use libisdb_bitrate::BitRateCalculator;
use libisdb_filter_base::{DataStream, FilterBase, FilterSink, OutputSlot};
use libisdb_ts_info::{PID_INVALID, PID_PAT};
use libisdb_ts_packet::{TsPacket, TS_PACKET_SIZE};
use libisdb_ts_tables::{PATTable, PMTTable};

/// 無効なサービス ID。LibISDBConsts.hpp:39。
pub const SERVICE_ID_INVALID: u16 = 0x0000;

/// 注入されたクロック関数の保持型。
type ClockFn = Box<dyn Fn() -> u64>;

/// サービス情報。TSPacketCounterFilter.hpp:78。
///
/// 原実装は PMTPID も保持するが、本移植では PMT テーブルを PMT PID をキーにした
/// HashMap で管理しており、サービスとの対応は不要なため省略している
/// (PMT 更新時のサービス特定は PMT 内の program_number で行う)。
#[derive(Debug, Clone, Default)]
struct ServiceInfo {
    service_id: u16,
    es_pid_list: Vec<u16>,
}

/// TSパケットカウンタフィルタ。TSPacketCounterFilter.hpp:43。
pub struct TsPacketCounterFilter {
    // PSI 追跡(C++ の PIDMapManager + PSITable 群に相当)
    pat_table: PATTable,
    pmt_tables: HashMap<u16, PMTTable>, // PMT PID → PMT テーブル
    service_list: Vec<ServiceInfo>,
    /// 現在 ES として監視中の PID 集合(対象サービスの ES、ESPIDMapTarget マップ相当)
    mapped_es_pids: HashSet<u16>,
    target_service_id: u16,

    input_packet_count: u64,
    scrambled_packet_count: u64,

    video_pid: u16,
    audio_pid: u16,
    video_bitrate: BitRateCalculator<ClockFn>,
    audio_bitrate: BitRateCalculator<ClockFn>,

    // 出力スロット(SingleIOFilter のパススルー出力)
    output: OutputSlot,
}

impl TsPacketCounterFilter {
    /// TSPacketCounterFilter::TSPacketCounterFilter (cpp:37)
    ///
    /// `clock_fn` はビットレート計測用のクロック(ミリ秒精度、CLOCKS_PER_SEC=1000)。
    /// C++ の TickClock(GetTickCount64)を外部注入に置き換えた(libisdb_bitrate の方針)。
    pub fn new<C: Fn() -> u64 + Clone + 'static>(clock_fn: C) -> Self {
        let cv = clock_fn.clone();
        let ca = clock_fn;
        let mut s = Self {
            pat_table: PATTable::new(),
            pmt_tables: HashMap::new(),
            service_list: Vec::new(),
            mapped_es_pids: HashSet::new(),
            target_service_id: SERVICE_ID_INVALID,

            input_packet_count: 0,
            scrambled_packet_count: 0,

            video_pid: PID_INVALID,
            audio_pid: PID_INVALID,
            video_bitrate: BitRateCalculator::new(Box::new(cv) as ClockFn),
            audio_bitrate: BitRateCalculator::new(Box::new(ca) as ClockFn),

            output: OutputSlot::new(),
        };
        // C++ コンストラクタは Reset() を呼ぶ
        s.reset_impl();
        s
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

    // ── 設定 (TSPacketCounterFilter.cpp:66-) ────────────────────

    /// TSPacketCounterFilter::SetActiveServiceID (cpp:66)
    pub fn set_active_service_id(&mut self, service_id: u16) {
        if self.target_service_id == service_id {
            return;
        }
        self.target_service_id = service_id;

        // 対象外サービスの ES を解除
        for i in 0..self.service_list.len() {
            if self.service_list[i].service_id != service_id {
                self.unmap_service_es(i);
            }
        }
        // 対象サービスの ES をマップ
        for i in 0..self.service_list.len() {
            if self.service_list[i].service_id == service_id {
                self.map_service_es(i);
            }
        }
    }

    /// TSPacketCounterFilter::SetActiveVideoPID (cpp:85)。ServiceChanged は未使用。
    pub fn set_active_video_pid(&mut self, pid: u16, _service_changed: bool) {
        self.set_video_pid(pid);
    }

    /// TSPacketCounterFilter::SetActiveAudioPID (cpp:91)。ServiceChanged は未使用。
    pub fn set_active_audio_pid(&mut self, pid: u16, _service_changed: bool) {
        self.set_audio_pid(pid);
    }

    /// TSPacketCounterFilter::SetVideoPID (cpp:140)
    pub fn set_video_pid(&mut self, pid: u16) {
        self.video_pid = pid;
    }

    /// TSPacketCounterFilter::SetAudioPID (cpp:148)
    pub fn set_audio_pid(&mut self, pid: u16) {
        self.audio_pid = pid;
    }

    // ── 取得 (TSPacketCounterFilter.cpp:116-) ───────────────────

    /// TSPacketCounterFilter::GetInputPacketCount (cpp:116)
    pub fn get_input_packet_count(&self) -> u64 {
        self.input_packet_count
    }
    /// TSPacketCounterFilter::ResetInputPacketCount (cpp:122)
    pub fn reset_input_packet_count(&mut self) {
        self.input_packet_count = 0;
    }
    /// TSPacketCounterFilter::GetScrambledPacketCount (cpp:128)
    pub fn get_scrambled_packet_count(&self) -> u64 {
        self.scrambled_packet_count
    }
    /// TSPacketCounterFilter::ResetScrambledPacketCount (cpp:134)
    pub fn reset_scrambled_packet_count(&mut self) {
        self.scrambled_packet_count = 0;
    }
    /// TSPacketCounterFilter::GetVideoBitRate (cpp:156)
    pub fn get_video_bit_rate(&self) -> u64 {
        self.video_bitrate.get_bit_rate()
    }
    /// TSPacketCounterFilter::GetAudioBitRate (cpp:164)
    pub fn get_audio_bit_rate(&self) -> u64 {
        self.audio_bitrate.get_bit_rate()
    }

    // ── 内部処理 ───────────────────────────────────────────────

    fn reset_impl(&mut self) {
        // m_PIDMapManager.UnmapAllTargets() + PID_PAT 再マップ相当
        self.pat_table.reset();
        self.pmt_tables.clear();
        self.mapped_es_pids.clear();

        self.service_list.clear();
        self.target_service_id = SERVICE_ID_INVALID;

        self.input_packet_count = 0;
        self.scrambled_packet_count = 0;

        self.video_pid = PID_INVALID;
        self.audio_pid = PID_INVALID;
        self.video_bitrate.initialize();
        self.audio_bitrate.initialize();
    }

    /// TSPacketCounterFilter::ProcessData (cpp:97)
    fn process_data(&mut self, stream: &mut dyn DataStream) {
        if !stream.is_ts_packet() {
            return;
        }
        loop {
            let data = stream.data();
            if data.len() >= TS_PACKET_SIZE {
                let mut arr = [0u8; TS_PACKET_SIZE];
                arr.copy_from_slice(&data[..TS_PACKET_SIZE]);
                let mut pkt = TsPacket::new(&arr);
                pkt.parse_packet(None);

                self.input_packet_count += 1;

                self.store_packet(&pkt);

                if self.target_service_id == SERVICE_ID_INVALID && pkt.is_scrambled() {
                    self.scrambled_packet_count += 1;
                }
            }
            if !stream.next() {
                break;
            }
        }
    }

    /// PIDMapManager::StorePacket 相当。PID でテーブル/ES ハンドラへルーティングする。
    fn store_packet(&mut self, pkt: &TsPacket) {
        let pid = pkt.get_pid();

        if pid == PID_PAT {
            // セクション更新時に OnPATSection 相当
            if self.pat_table.store_packet(pkt) {
                self.on_pat_updated();
            }
        } else if self.pmt_tables.contains_key(&pid) {
            // セクション更新時に OnPMTSection 相当
            let updated = self.pmt_tables.get_mut(&pid).unwrap().store_packet(pkt);
            if updated {
                self.on_pmt_updated(pid);
            }
        } else if self.mapped_es_pids.contains(&pid) {
            self.es_store_packet(pkt);
        }
    }

    /// TSPacketCounterFilter::OnPATSection (cpp:199)
    fn on_pat_updated(&mut self) {
        // 既存の ES / PMT マップを解除
        self.mapped_es_pids.clear();
        self.pmt_tables.clear();

        let program_count = self.pat_table.get_program_count();
        self.service_list.clear();
        self.service_list.reserve(program_count);

        for i in 0..program_count {
            let service_id = self.pat_table.get_program_number(i);
            let pmt_pid = self.pat_table.get_pmt_pid(i);
            self.service_list.push(ServiceInfo {
                service_id,
                es_pid_list: Vec::new(),
            });
            // 新しい PMT テーブルをマップ(OnPMTSection は更新時に駆動)
            self.pmt_tables.insert(pmt_pid, PMTTable::new());
        }
    }

    /// TSPacketCounterFilter::OnPMTSection (cpp:228)
    fn on_pmt_updated(&mut self, pmt_pid: u16) {
        let service_id = match self.pmt_tables.get(&pmt_pid) {
            Some(t) => t.get_program_number_id(),
            None => return,
        };
        let service_index = match self.get_service_index_by_id(service_id) {
            Some(i) => i,
            None => return,
        };

        let new_es: Vec<u16> = {
            let pmt = &self.pmt_tables[&pmt_pid];
            (0..pmt.get_es_count()).map(|i| pmt.get_es_pid(i)).collect()
        };

        if new_es != self.service_list[service_index].es_pid_list {
            let is_target = service_id == self.target_service_id;
            if is_target {
                self.unmap_service_es(service_index);
            }
            self.service_list[service_index].es_pid_list = new_es;
            if is_target {
                self.map_service_es(service_index);
            }
        }
    }

    /// TSPacketCounterFilter::GetServiceIndexByID (cpp:172)。末尾から検索。
    fn get_service_index_by_id(&self, service_id: u16) -> Option<usize> {
        (0..self.service_list.len())
            .rev()
            .find(|&i| self.service_list[i].service_id == service_id)
    }

    /// TSPacketCounterFilter::MapServiceESs (cpp:185)
    fn map_service_es(&mut self, index: usize) {
        let pids = self.service_list[index].es_pid_list.clone();
        for pid in pids {
            self.mapped_es_pids.insert(pid);
        }
    }

    /// TSPacketCounterFilter::UnmapServiceESs (cpp:192)
    fn unmap_service_es(&mut self, index: usize) {
        let pids = self.service_list[index].es_pid_list.clone();
        for pid in pids {
            self.mapped_es_pids.remove(&pid);
        }
    }

    /// TSPacketCounterFilter::ESPIDMapTarget::StorePacket (cpp:266)
    fn es_store_packet(&mut self, pkt: &TsPacket) {
        if pkt.is_scrambled() {
            self.scrambled_packet_count += 1;
        }
        let pid = pkt.get_pid();
        if pid == self.video_pid {
            self.video_bitrate.update(pkt.get_payload_size() as usize);
        } else if pid == self.audio_pid {
            self.audio_bitrate.update(pkt.get_payload_size() as usize);
        }
    }
}

// ---------------------------------------------------------------------------
// FilterBase 実装
// ---------------------------------------------------------------------------

impl FilterBase for TsPacketCounterFilter {
    fn input_count(&self) -> usize {
        1
    }
    fn output_count(&self) -> usize {
        1
    }

    /// TSPacketCounterFilter::Reset (cpp:45)
    fn reset(&mut self) {
        self.reset_impl();
    }
}

// ---------------------------------------------------------------------------
// FilterSink 実装(SingleIOFilter: ProcessData → OutputData パススルー)
// ---------------------------------------------------------------------------

impl FilterSink for TsPacketCounterFilter {
    fn receive_data(&mut self, stream: &mut dyn DataStream) -> bool {
        // SingleIOFilter::ReceiveData = ProcessData(pData) → OutputData(pData)
        self.process_data(stream);
        // パススルー出力(OutputData は内部で rewind してから下流へ渡す)
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
    use libisdb_crc::crc32_mpeg2;
    use libisdb_filter_base::{SingleDataStream, VecDataStream, TYPE_ID_TS_PACKET};
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    /// 固定クロック(0 を返す)。
    fn zero_clock() -> impl Fn() -> u64 + Clone + 'static {
        || 0u64
    }

    /// 単純な TS パケット (payload-only, afc=0x01) を構築する。
    fn make_ts_packet(pid: u16, cc: u8, pusi: bool, payload: &[u8]) -> [u8; TS_PACKET_SIZE] {
        let mut p = [0xFFu8; TS_PACKET_SIZE];
        p[0] = 0x47;
        p[1] = ((pusi as u8) << 6) | ((pid >> 8) as u8 & 0x1F);
        p[2] = (pid & 0xFF) as u8;
        p[3] = 0x10 | (cc & 0x0F); // afc=01 (payload), scrambling=0
        let n = payload.len().min(184);
        p[4..4 + n].copy_from_slice(&payload[..n]);
        p
    }

    /// PAT パケット。programs: (program_number, pmt_pid) のリスト。
    fn make_pat_packet(cc: u8, tsid: u16, programs: &[(u16, u16)]) -> [u8; TS_PACKET_SIZE] {
        let mut section = vec![
            0x00, // table_id
            0x00, // length(後で設定)
            0x00,
            (tsid >> 8) as u8,
            (tsid & 0xFF) as u8,
            0xC1, // version+current_next
            0x00, // section_number
            0x00, // last_section_number
        ];
        for &(program_number, pmt_pid) in programs {
            section.push((program_number >> 8) as u8);
            section.push((program_number & 0xFF) as u8);
            section.push(0xE0 | ((pmt_pid >> 8) as u8 & 0x1F));
            section.push((pmt_pid & 0xFF) as u8);
        }
        let section_length = (section.len() - 3 + 4) as u16;
        section[1] = 0xB0 | ((section_length >> 8) as u8);
        section[2] = (section_length & 0xFF) as u8;
        let crc = crc32_mpeg2(&section, 0xFFFF_FFFF);
        section.extend_from_slice(&crc.to_be_bytes());

        let mut payload = vec![0x00u8]; // pointer_field
        payload.extend_from_slice(&section);
        make_ts_packet(PID_PAT, cc, true, &payload)
    }

    /// PMT パケット。es: (stream_type, es_pid) のリスト。
    fn make_pmt_packet(
        pid: u16,
        cc: u8,
        program_number: u16,
        pcr_pid: u16,
        es: &[(u8, u16)],
    ) -> [u8; TS_PACKET_SIZE] {
        let mut section = vec![
            0x02, // table_id
            0x00, // length(後で設定)
            0x00,
            (program_number >> 8) as u8,
            (program_number & 0xFF) as u8,
            0xC1, // version+current_next
            0x00, // section_number
            0x00, // last_section_number
            0xE0 | ((pcr_pid >> 8) as u8 & 0x1F),
            (pcr_pid & 0xFF) as u8,
            0xF0, // program_info_length(high)
            0x00, // program_info_length(low) = 0
        ];
        for &(stream_type, es_pid) in es {
            section.push(stream_type);
            section.push(0xE0 | ((es_pid >> 8) as u8 & 0x1F));
            section.push((es_pid & 0xFF) as u8);
            section.push(0xF0); // ES_info_length(high)
            section.push(0x00); // ES_info_length(low) = 0
        }
        let section_length = (section.len() - 3 + 4) as u16;
        section[1] = 0xB0 | ((section_length >> 8) as u8);
        section[2] = (section_length & 0xFF) as u8;
        let crc = crc32_mpeg2(&section, 0xFFFF_FFFF);
        section.extend_from_slice(&crc.to_be_bytes());

        let mut payload = vec![0x00u8];
        payload.extend_from_slice(&section);
        make_ts_packet(pid, cc, true, &payload)
    }

    /// 連結バッファを 188 バイト単位の TS パケット列に分割して供給する。
    /// 上流(TSPacketParserFilter)は1パケット1要素のシーケンスを出力するため、
    /// それを VecDataStream で再現する。
    fn feed(filter: &mut TsPacketCounterFilter, data: &[u8]) {
        let items: Vec<Vec<u8>> = data.chunks(TS_PACKET_SIZE).map(|c| c.to_vec()).collect();
        let mut stream = VecDataStream::new(TYPE_ID_TS_PACKET, items);
        filter.receive_data(&mut stream);
    }

    // ── 入力パケット数 ──────────────────────────────────────────

    #[test]
    fn test_input_packet_count() {
        let mut filter = TsPacketCounterFilter::new(zero_clock());
        let mut buf = Vec::new();
        for cc in 0..4u8 {
            buf.extend_from_slice(&make_ts_packet(0x0100, cc, true, b"x"));
        }
        feed(&mut filter, &buf);
        assert_eq!(filter.get_input_packet_count(), 4);

        filter.reset_input_packet_count();
        assert_eq!(filter.get_input_packet_count(), 0);
    }

    #[test]
    fn test_non_ts_stream_not_counted() {
        let mut filter = TsPacketCounterFilter::new(zero_clock());
        // TYPE_ID_DATA_BUFFER (= 非 TSPacket) はカウントされない
        let data = vec![0u8; TS_PACKET_SIZE];
        let mut stream = SingleDataStream::new(0, &data); // TYPE_ID_DATA_BUFFER
        filter.receive_data(&mut stream);
        assert_eq!(filter.get_input_packet_count(), 0);
    }

    // ── スクランブル計数 ────────────────────────────────────────

    #[test]
    fn test_scrambled_count_no_target() {
        let mut filter = TsPacketCounterFilter::new(zero_clock());
        // 対象サービス未設定 → 全スクランブルパケットを数える
        let mut s = make_ts_packet(0x0100, 0, true, b"x");
        s[3] |= 0x80; // transport_scrambling_control = 0b10 (scrambled)
        feed(&mut filter, &s);

        // 非スクランブルは数えない
        feed(&mut filter, &make_ts_packet(0x0100, 1, true, b"x"));

        assert_eq!(filter.get_scrambled_packet_count(), 1);

        filter.reset_scrambled_packet_count();
        assert_eq!(filter.get_scrambled_packet_count(), 0);
    }

    // ── PAT/PMT 追跡 + ES ビットレート ─────────────────────────

    #[test]
    fn test_pat_pmt_es_bitrate() {
        let time = Rc::new(Cell::new(0u64));
        let t = time.clone();
        let mut filter = TsPacketCounterFilter::new(move || t.get());

        let service_id = 0x0030u16;
        let pmt_pid = 0x0100u16;
        let video_pid = 0x0111u16;

        // PAT: program 0x0030 → PMT PID 0x0100
        feed(&mut filter, &make_pat_packet(0, 0x1234, &[(service_id, pmt_pid)]));
        // PMT: program 0x0030, PCR 0x0100, ES 映像 0x0111
        feed(
            &mut filter,
            &make_pmt_packet(pmt_pid, 0, service_id, pmt_pid, &[(0x02, video_pid)]),
        );

        // 対象サービスと映像 PID を設定 → ES 0x0111 がマップされる
        filter.set_active_service_id(service_id);
        filter.set_video_pid(video_pid);

        // time=0 で映像パケットを投入(まだビットレート更新は出ない)
        feed(&mut filter, &make_ts_packet(video_pid, 0, true, b"video-payload"));
        assert_eq!(filter.get_video_bit_rate(), 0);

        // 1秒進めてもう1パケット → ビットレート更新
        time.set(1000);
        feed(&mut filter, &make_ts_packet(video_pid, 1, true, b"video-payload"));

        assert!(filter.get_video_bit_rate() > 0, "映像ビットレートが計測される");
        // 音声 PID は未設定なので 0
        assert_eq!(filter.get_audio_bit_rate(), 0);
    }

    #[test]
    fn test_scrambled_count_with_target_only_es() {
        let mut filter = TsPacketCounterFilter::new(zero_clock());
        let service_id = 0x0030u16;
        let pmt_pid = 0x0100u16;
        let es_pid = 0x0111u16;

        feed(&mut filter, &make_pat_packet(0, 0x1234, &[(service_id, pmt_pid)]));
        feed(
            &mut filter,
            &make_pmt_packet(pmt_pid, 0, service_id, pmt_pid, &[(0x02, es_pid)]),
        );
        filter.set_active_service_id(service_id);

        // 対象サービスの ES PID でスクランブルパケット → 数える
        let mut s_es = make_ts_packet(es_pid, 0, true, b"x");
        s_es[3] |= 0x80;
        feed(&mut filter, &s_es);
        assert_eq!(filter.get_scrambled_packet_count(), 1);

        // 対象外 PID のスクランブルパケット → 対象設定済みなので数えない
        let mut s_other = make_ts_packet(0x0500, 0, true, b"x");
        s_other[3] |= 0x80;
        feed(&mut filter, &s_other);
        assert_eq!(filter.get_scrambled_packet_count(), 1); // 増えない
    }

    // ── パススルー ──────────────────────────────────────────────

    #[test]
    fn test_passthrough() {
        let store: Rc<RefCell<Vec<Vec<u8>>>> = Rc::new(RefCell::new(Vec::new()));
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

        let mut filter = TsPacketCounterFilter::new(zero_clock());
        filter.connect_output(Box::new(Rec(st)));

        let pkt = make_ts_packet(0x0100, 0, true, b"hello");
        feed(&mut filter, &pkt);

        let recorded = store.borrow();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0], pkt.to_vec());
        // カウントもされている
        assert_eq!(filter.get_input_packet_count(), 1);
    }

    // ── Reset ───────────────────────────────────────────────────

    #[test]
    fn test_reset_clears_state() {
        let mut filter = TsPacketCounterFilter::new(zero_clock());
        let mut s = make_ts_packet(0x0100, 0, true, b"x");
        s[3] |= 0x80;
        feed(&mut filter, &s);
        assert_eq!(filter.get_input_packet_count(), 1);
        assert_eq!(filter.get_scrambled_packet_count(), 1);

        filter.set_video_pid(0x0111);
        filter.reset();

        assert_eq!(filter.get_input_packet_count(), 0);
        assert_eq!(filter.get_scrambled_packet_count(), 0);
        assert_eq!(filter.get_video_bit_rate(), 0);
    }

    // ── FilterBase ──────────────────────────────────────────────

    #[test]
    fn test_filter_base_ports() {
        let filter = TsPacketCounterFilter::new(zero_clock());
        assert_eq!(filter.input_count(), 1);
        assert_eq!(filter.output_count(), 1);
    }

    #[test]
    fn test_active_pid_setters() {
        let mut filter = TsPacketCounterFilter::new(zero_clock());
        filter.set_active_video_pid(0x0111, true);
        filter.set_active_audio_pid(0x0112, false);
        // 設定が反映されること(ビットレートは未計測なので 0)
        assert_eq!(filter.get_video_bit_rate(), 0);
        assert_eq!(filter.get_audio_bit_rate(), 0);
    }
}
