// Rust port of LibISDB/Filters/TSPacketParserFilter.cpp + TSPacketParserFilter.hpp
//
// TSパケット解析フィルタ。SingleIOFilter として、入力バイト列から TS パケットを
// 同期・解析し、パケット統計を取りつつ下流へ出力する。ワンセグ PAT 生成にも対応。
//
// 原実装との対応:
//   - C++ の SingleIOFilter 継承 → libisdb_filter_base の FilterBase + FilterSink を実装
//   - C++ の OutputData(DataStream*) → OutputSlot::send
//   - C++ の DataStreamSequence<TSPacket> m_PacketSequence → Vec<[u8; 188]>
//   - C++ の OneSegPATGenerator::StorePacket(内部 PSI パース) →
//       本フィルタが NITTable + [PMTTable; 8] を保持し PSI パースを行い、
//       notify ベースの OneSegPatGenerator を駆動する(原実装の責務分割を本フィルタで再結合)
//
// 重要な挙動(原実装由来):
//   - PMT カウント: C++ の PIDMapManager::StorePacket / PSITableBase::StorePacket は
//     常に true を返すため、ワンセグ PMT カウントはセクションのバージョン変化ではなく
//     「受信パケットごと」に増える。本移植も store_packet の戻り値に依存せず
//     毎パケットで notify_pmt_section を呼ぶ。

use libisdb_filter_base::{
    DataStream, FilterBase, FilterSink, OutputSlot, SingleDataStream, VecDataStream,
    TYPE_ID_TS_PACKET,
};
use libisdb_ts_packet::{ParseResult, TsPacket, NUM_PID, PID_NULL, TS_PACKET_SIZE};
use libisdb_ts_info::{ONESEG_PMT_PID_FIRST, PID_NIT, PID_PAT};
use libisdb_ts_tables::{NITTable, PMTTable};
use libisdb_descriptor::PartialReceptionDescriptor;
use libisdb_one_seg_pat_generator::{
    is_1seg_pmt_pid, OneSegPatGenerator, ONESEG_PMT_PID_COUNT, TRANSPORT_STREAM_ID_INVALID,
};

/// 最大 PID 値。LibISDBConsts.hpp:64。
const PID_MAX: usize = 0x1FFF;
/// TS パケット最大サイズ(FEC 込み)。LibISDBConsts.hpp:35。
const TS_PACKET_SIZE_MAX: usize = 204;
/// 同期バイト。
const SYNC_BYTE: u8 = 0x47;

/// 指定したバイト列の TS パケット PID を取り出す。
#[inline]
fn pid_of(data: &[u8]) -> u16 {
    ((data[1] as u16 & 0x1F) << 8) | data[2] as u16
}

// ---------------------------------------------------------------------------
// PacketCountInfo (TSPacketParserFilter.hpp:45)
// ---------------------------------------------------------------------------

/// パケット数情報。TSPacketParserFilter.hpp:45。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PacketCountInfo {
    pub input: u64,
    pub output: u64,
    pub format_error: u64,
    pub transport_error: u64,
    pub continuity_error: u64,
    pub scrambled: u64,
}

impl PacketCountInfo {
    /// TSPacketParserFilter.hpp:69
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

impl std::ops::AddAssign for PacketCountInfo {
    /// TSPacketParserFilter.hpp:53 operator+=
    fn add_assign(&mut self, rhs: Self) {
        self.input += rhs.input;
        self.output += rhs.output;
        self.format_error += rhs.format_error;
        self.transport_error += rhs.transport_error;
        self.continuity_error += rhs.continuity_error;
        self.scrambled += rhs.scrambled;
    }
}

impl std::ops::Add for PacketCountInfo {
    type Output = PacketCountInfo;
    /// TSPacketParserFilter.hpp:64 operator+
    fn add(mut self, rhs: Self) -> Self {
        self += rhs;
        self
    }
}

// ---------------------------------------------------------------------------
// TSPacketParserFilter
// ---------------------------------------------------------------------------

/// TSパケット解析フィルタ。TSPacketParserFilter.hpp:41。
pub struct TsPacketParserFilter {
    /// 同期中の TS パケット蓄積バッファ (m_Packet)。最大 TS_PACKET_SIZE バイト。
    packet: Vec<u8>,
    /// 出力用パケットシーケンス (m_PacketSequence)。各要素は 188 バイト。
    packet_sequence: Vec<[u8; TS_PACKET_SIZE]>,
    /// 同期外れカウント (m_OutOfSyncCount)。
    out_of_sync_count: usize,

    output_sequence: bool,
    max_sequence_packet_count: usize,
    output_null_packet: bool,
    output_error_packet: bool,

    packet_count: PacketCountInfo,
    total_packet_count: PacketCountInfo,
    pid_packet_count: Vec<PacketCountInfo>,
    pid_total_packet_count: Vec<PacketCountInfo>,
    continuity_counter: Box<[u8; NUM_PID]>,
    input_bytes: u64,
    total_input_bytes: u64,

    // ワンセグ PAT 生成
    pat_generator: OneSegPatGenerator,
    generate_1seg_pat: bool,
    nit_table: NITTable,
    pmt_tables: [PMTTable; ONESEG_PMT_PID_COUNT],

    // 出力スロット (SingleOutputFilter::m_OutputFilter 相当)
    output: OutputSlot,
}

impl Default for TsPacketParserFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl TsPacketParserFilter {
    /// TSPacketParserFilter::TSPacketParserFilter (TSPacketParserFilter.cpp:36)
    pub fn new() -> Self {
        Self {
            packet: Vec::with_capacity(TS_PACKET_SIZE),
            packet_sequence: Vec::new(),
            out_of_sync_count: 0,

            output_sequence: true,
            max_sequence_packet_count: 64,
            output_null_packet: false,
            output_error_packet: false,

            packet_count: PacketCountInfo::default(),
            total_packet_count: PacketCountInfo::default(),
            pid_packet_count: vec![PacketCountInfo::default(); PID_MAX + 1],
            pid_total_packet_count: vec![PacketCountInfo::default(); PID_MAX + 1],
            continuity_counter: Box::new([0x10u8; NUM_PID]),
            input_bytes: 0,
            total_input_bytes: 0,

            pat_generator: OneSegPatGenerator::new(),
            generate_1seg_pat: true,
            nit_table: NITTable::new(),
            pmt_tables: std::array::from_fn(|_| PMTTable::new()),

            output: OutputSlot::new(),
        }
    }

    // ── 出力スロット接続 ───────────────────────────────────────

    /// 下流フィルタを接続する。
    pub fn connect_output(&mut self, sink: Box<dyn FilterSink>) {
        self.output.connect(sink);
    }

    /// 下流フィルタを切断する。
    pub fn disconnect_output(&mut self) {
        self.output.disconnect();
    }

    /// 下流が接続されているか。
    pub fn is_output_connected(&self) -> bool {
        self.output.is_connected()
    }

    // ── 設定 (TSPacketParserFilter.cpp:109-) ────────────────────

    /// TSPacketParserFilter::SetOutputSequence (cpp:109)
    pub fn set_output_sequence(&mut self, enable: bool) {
        self.output_sequence = enable;
    }
    pub fn get_output_sequence(&self) -> bool {
        self.output_sequence
    }

    /// TSPacketParserFilter::SetMaxSequencePacketCount (cpp:117)
    /// Count < 1 は失敗。
    pub fn set_max_sequence_packet_count(&mut self, count: usize) -> bool {
        if count < 1 {
            return false;
        }
        self.max_sequence_packet_count = count;
        true
    }
    pub fn get_max_sequence_packet_count(&self) -> usize {
        self.max_sequence_packet_count
    }

    /// TSPacketParserFilter::SetOutputNullPacket (cpp:130)
    pub fn set_output_null_packet(&mut self, enable: bool) {
        self.output_null_packet = enable;
    }
    pub fn get_output_null_packet(&self) -> bool {
        self.output_null_packet
    }

    /// TSPacketParserFilter::SetOutputErrorPacket (cpp:138)
    pub fn set_output_error_packet(&mut self, enable: bool) {
        self.output_error_packet = enable;
    }
    pub fn get_output_error_packet(&self) -> bool {
        self.output_error_packet
    }

    /// TSPacketParserFilter::SetGenerate1SegPAT (cpp:211)
    pub fn set_generate_1seg_pat(&mut self, enable: bool) {
        self.generate_1seg_pat = enable;
    }
    pub fn get_generate_1seg_pat(&self) -> bool {
        self.generate_1seg_pat
    }

    /// TSPacketParserFilter::SetTransportStreamID (cpp:219)
    pub fn set_transport_stream_id(&mut self, transport_stream_id: u16) -> bool {
        self.pat_generator.set_transport_stream_id(transport_stream_id)
    }

    // ── 統計取得 (TSPacketParserFilter.cpp:146-) ────────────────

    /// TSPacketParserFilter::GetPacketCount (cpp:146)
    pub fn get_packet_count(&self) -> PacketCountInfo {
        self.packet_count
    }

    /// TSPacketParserFilter::GetPacketCount(PID) (cpp:154)
    pub fn get_packet_count_pid(&self, pid: u16) -> PacketCountInfo {
        if pid as usize > PID_MAX {
            return PacketCountInfo::default();
        }
        self.pid_packet_count[pid as usize]
    }

    /// TSPacketParserFilter::GetTotalPacketCount (cpp:165)
    pub fn get_total_packet_count(&self) -> PacketCountInfo {
        self.total_packet_count + self.packet_count
    }

    /// TSPacketParserFilter::GetTotalPacketCount(PID) (cpp:173)
    pub fn get_total_packet_count_pid(&self, pid: u16) -> PacketCountInfo {
        if pid as usize > PID_MAX {
            return PacketCountInfo::default();
        }
        self.pid_total_packet_count[pid as usize] + self.pid_packet_count[pid as usize]
    }

    /// TSPacketParserFilter::ResetErrorPacketCount (cpp:184)
    pub fn reset_error_packet_count(&mut self) {
        self.packet_count.format_error = 0;
        self.packet_count.transport_error = 0;
        self.packet_count.continuity_error = 0;
        self.packet_count.scrambled = 0;
    }

    /// TSPacketParserFilter::GetInputBytes (cpp:195)
    pub fn get_input_bytes(&self) -> u64 {
        self.input_bytes
    }

    /// TSPacketParserFilter::GetTotalInputBytes (cpp:203)
    pub fn get_total_input_bytes(&self) -> u64 {
        self.total_input_bytes + self.input_bytes
    }

    // ── 内部処理 ───────────────────────────────────────────────

    /// TSPacketParserFilter::SyncPacket (cpp:227)
    fn sync_packet(&mut self, data: &[u8]) {
        self.input_bytes += data.len() as u64;

        let size = data.len();
        let mut cur_pos = 0usize;

        while cur_pos < size {
            let cur_size = self.packet.len();

            if cur_size == 0 {
                // 同期バイト待ち中 (do-while)
                loop {
                    let b = data[cur_pos];
                    cur_pos += 1;
                    if b == SYNC_BYTE {
                        // 同期バイト発見
                        self.packet.push(SYNC_BYTE);
                        break;
                    }
                    self.out_of_sync_count += 1;
                    if cur_pos >= size {
                        break;
                    }
                }
            } else {
                let mut cur_size = cur_size;
                if cur_size < TS_PACKET_SIZE {
                    // データ待ち中
                    let remain = (TS_PACKET_SIZE - cur_size).min(size - cur_pos);
                    self.packet.extend_from_slice(&data[cur_pos..cur_pos + remain]);
                    cur_pos += remain;
                    cur_size += remain;
                }

                if cur_size == TS_PACKET_SIZE {
                    // パケットサイズ分データが揃った
                    let mut arr = [0u8; TS_PACKET_SIZE];
                    arr.copy_from_slice(&self.packet);
                    let mut pkt = TsPacket::new(&arr);
                    let result = pkt.parse_packet(Some(&mut self.continuity_counter));

                    if self.out_of_sync_count > (TS_PACKET_SIZE_MAX - TS_PACKET_SIZE)
                        && matches!(
                            result,
                            ParseResult::FormatError | ParseResult::TransportError
                        )
                    {
                        // 同期バイトが他にもある場合、そこから再同期する
                        let mut resync = false;
                        for i in 1..TS_PACKET_SIZE {
                            if self.packet[i] == SYNC_BYTE {
                                self.packet.drain(0..i); // TrimHead(i)
                                resync = true;
                                break;
                            }
                        }
                        if resync {
                            continue;
                        }
                    }

                    self.process_packet(result, &pkt, &arr);

                    self.out_of_sync_count = 0;
                }
            }
        }
    }

    /// TSPacketParserFilter::ProcessPacket (cpp:286)
    fn process_packet(&mut self, result: ParseResult, pkt: &TsPacket, raw: &[u8; TS_PACKET_SIZE]) {
        self.packet_count.input += 1;

        let pid = pkt.get_pid();
        let pid_idx = pid as usize;
        let mut output = false;

        match result {
            ParseResult::ContinuityError => {
                self.packet_count.continuity_error += 1;
                self.pid_packet_count[pid_idx].continuity_error += 1;
                // [[fallthrough]] → OK
                self.process_valid_packet(pkt, pid_idx, &mut output);
            }
            ParseResult::Ok => {
                self.process_valid_packet(pkt, pid_idx, &mut output);
            }
            ParseResult::FormatError => {
                self.packet_count.format_error += 1;
                if self.output_error_packet {
                    output = true;
                }
            }
            ParseResult::TransportError => {
                self.packet_count.transport_error += 1;
                if self.output_error_packet {
                    output = true;
                }
            }
        }

        if output {
            self.output_packet(*raw);
        }

        // m_Packet.ClearSize()
        self.packet.clear();
    }

    /// ProcessPacket の OK / ContinuityError 共通処理 (cpp:298-321)
    fn process_valid_packet(&mut self, pkt: &TsPacket, pid_idx: usize, output: &mut bool) {
        self.pid_packet_count[pid_idx].input += 1;

        if pkt.is_scrambled() {
            self.packet_count.scrambled += 1;
            self.pid_packet_count[pid_idx].scrambled += 1;
        }

        // ワンセグ PAT 生成 (cpp:314)
        // StorePacket 相当は常に呼ぶ(状態更新のため)。GetPATPacket 相当のみ
        // generate_1seg_pat で制御する。
        if self.store_packet_pat(pkt) && self.generate_1seg_pat {
            let pmt_list: Vec<Option<u16>> = self
                .pmt_tables
                .iter()
                .map(|t| {
                    let id = t.get_program_number_id();
                    if id != 0 {
                        Some(id)
                    } else {
                        None
                    }
                })
                .collect();
            if let Some(pat) = self.pat_generator.generate_pat_packet(&pmt_list) {
                self.output_packet(pat);
            }
        }

        if self.output_null_packet || pkt.get_pid() != PID_NULL {
            *output = true;
        }
    }

    /// OneSegPATGenerator::StorePacket (OneSegPATGenerator.cpp:63) 相当。
    /// PAT を生成すべきタイミングなら true を返す。
    ///
    /// 原実装では OneSegPATGenerator が内部に PIDMapManager + NIT/PMT テーブルを
    /// 保持して PSI パースするが、本移植ではそれを本フィルタが保持し、
    /// notify ベースの OneSegPatGenerator を駆動する。
    fn store_packet_pat(&mut self, pkt: &TsPacket) -> bool {
        let pid = pkt.get_pid();

        if pid == PID_PAT {
            self.pat_generator.notify_pat_received();
            return false;
        }

        if pid == PID_NIT {
            // NIT パース。セクション更新時に TSID を抽出して通知 (OnNITSection 相当)。
            if self.nit_table.store_packet(pkt) {
                let tsid = Self::extract_oneseg_tsid(&self.nit_table);
                self.pat_generator.notify_nit_transport_stream_id(tsid);
            }
            return false;
        }

        if is_1seg_pmt_pid(pid) {
            let index = (pid - ONESEG_PMT_PID_FIRST) as usize;
            // PMT をパース(program_number を最新化)。戻り値は使わない:
            // C++ の PIDMapManager::StorePacket は常に true を返すため、
            // カウントは毎パケットで行う(notify_pmt_section)。
            self.pmt_tables[index].store_packet(pkt);
            return self.pat_generator.notify_pmt_section(pid);
        }

        false
    }

    /// OnNITSection (OneSegPATGenerator.cpp:186) のうち、先頭 TS の
    /// 部分受信記述子からワンセグ TSID を取り出す部分。
    /// 部分受信記述子が存在しサービスがある場合のみ有効な TSID を返す。
    fn extract_oneseg_tsid(nit: &NITTable) -> u16 {
        if let Some(item) = nit.get_ts_info(0) {
            if let Some(desc) = item
                .descriptors
                .get_descriptor_by_tag(PartialReceptionDescriptor::TAG)
            {
                if let Some(prd) = PartialReceptionDescriptor::from_descriptor(desc) {
                    if !prd.service_list.is_empty() {
                        return item.transport_stream_id;
                    }
                }
            }
        }
        TRANSPORT_STREAM_ID_INVALID
    }

    /// TSPacketParserFilter::OutputPacket (cpp:346)
    fn output_packet(&mut self, packet: [u8; TS_PACKET_SIZE]) {
        let pid = pid_of(&packet);
        let pid_idx = pid as usize;

        self.packet_count.output += 1;
        self.pid_packet_count[pid_idx].output += 1;

        if self.output_sequence {
            let need_flush = self.packet_sequence.len() >= self.max_sequence_packet_count
                || (!self.packet_sequence.is_empty()
                    && pid_of(&self.packet_sequence[0]) != pid);
            if need_flush {
                self.flush_sequence();
            }
            self.packet_sequence.push(packet);
        } else {
            // OutputData(&Packet)
            let mut stream = SingleDataStream::ts_packet(&packet);
            self.output.send(&mut stream);
        }
    }

    /// 蓄積したパケットシーケンスを下流へ出力する (OutputData(m_PacketSequence))。
    fn flush_sequence(&mut self) {
        if self.packet_sequence.is_empty() {
            return;
        }
        let items: Vec<Vec<u8>> = self.packet_sequence.drain(..).map(|p| p.to_vec()).collect();
        let mut stream = VecDataStream::new(TYPE_ID_TS_PACKET, items);
        self.output.send(&mut stream);
    }
}

// ---------------------------------------------------------------------------
// FilterBase 実装 (TSPacketParserFilter.cpp:52, 77)
// ---------------------------------------------------------------------------

impl FilterBase for TsPacketParserFilter {
    fn input_count(&self) -> usize {
        1
    }
    fn output_count(&self) -> usize {
        1
    }

    /// TSPacketParserFilter::Reset (cpp:52)
    fn reset(&mut self) {
        self.total_packet_count += self.packet_count;
        self.packet_count.reset();

        for i in 0..self.pid_packet_count.len() {
            let cur = self.pid_packet_count[i];
            self.pid_total_packet_count[i] += cur;
            self.pid_packet_count[i].reset();
        }

        self.total_input_bytes += self.input_bytes;
        self.input_bytes = 0;

        self.continuity_counter.fill(0x10);

        self.packet.clear();
        self.packet_sequence.clear();
        self.out_of_sync_count = 0;

        // m_PATGenerator.Reset() — PSI テーブルも再構築する
        self.pat_generator.reset();
        self.nit_table.reset();
        for t in self.pmt_tables.iter_mut() {
            t.reset();
        }
    }

    /// TSPacketParserFilter::StartStreaming (cpp:77)
    fn start_streaming(&mut self) -> bool {
        // FilterBase::StartStreaming() はデフォルト true
        if self.output_sequence {
            // m_PacketSequence.Allocate(m_MaxSequencePacketCount)
            self.packet_sequence.reserve(self.max_sequence_packet_count);
        }
        true
    }
}

// ---------------------------------------------------------------------------
// FilterSink 実装 (TSPacketParserFilter.cpp:91 ReceiveData)
// ---------------------------------------------------------------------------

impl FilterSink for TsPacketParserFilter {
    /// TSPacketParserFilter::ReceiveData (cpp:91)
    fn receive_data(&mut self, stream: &mut dyn DataStream) -> bool {
        loop {
            self.sync_packet(stream.data());
            if !stream.next() {
                break;
            }
        }

        // 残りのシーケンスを出力 (cpp:100)
        self.flush_sequence();

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
    use std::cell::RefCell;
    use std::rc::Rc;

    /// 受信したパケットを記録するシンク。
    struct Recorder {
        packets: Rc<RefCell<Vec<Vec<u8>>>>,
    }
    impl FilterSink for Recorder {
        fn receive_data(&mut self, stream: &mut dyn DataStream) -> bool {
            self.packets.borrow_mut().push(stream.data().to_vec());
            while stream.next() {
                self.packets.borrow_mut().push(stream.data().to_vec());
            }
            true
        }
    }

    fn make_recorder() -> (Rc<RefCell<Vec<Vec<u8>>>>, Box<dyn FilterSink>) {
        let store = Rc::new(RefCell::new(Vec::new()));
        let sink = Box::new(Recorder { packets: store.clone() });
        (store, sink)
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

    /// 最小構成の PMT パケット(ES 無し)を構築する。program_number を指定。
    fn make_pmt_packet(pid: u16, cc: u8, program_number: u16) -> [u8; TS_PACKET_SIZE] {
        let mut section = vec![
            0x02, // table_id
            0x00, // section_syntax/length (後で設定)
            0x00,
            (program_number >> 8) as u8,
            (program_number & 0xFF) as u8,
            0xC1, // reserved+version+current_next
            0x00, // section_number
            0x00, // last_section_number
            0xE0, // reserved + PCR_PID(high)
            0x00, // PCR_PID(low)
            0xF0, // reserved + program_info_length(high)
            0x00, // program_info_length(low)
        ];
        // section_length = (length フィールド以降のバイト数) = (len - 3) + CRC(4)
        let section_length = (section.len() - 3 + 4) as u16;
        section[1] = 0xB0 | ((section_length >> 8) as u8);
        section[2] = (section_length & 0xFF) as u8;
        let crc = crc32_mpeg2(&section, 0xFFFF_FFFF);
        section.extend_from_slice(&crc.to_be_bytes());

        let mut payload = vec![0x00u8]; // pointer_field
        payload.extend_from_slice(&section);
        make_ts_packet(pid, cc, true, &payload)
    }

    fn feed(filter: &mut TsPacketParserFilter, data: &[u8]) {
        let mut stream = SingleDataStream::new(TYPE_ID_TS_PACKET, data);
        filter.receive_data(&mut stream);
    }

    // ── PacketCountInfo ─────────────────────────────────────────

    #[test]
    fn test_packet_count_add() {
        let mut a = PacketCountInfo {
            input: 1,
            output: 2,
            format_error: 3,
            transport_error: 4,
            continuity_error: 5,
            scrambled: 6,
        };
        let b = a;
        a += b;
        assert_eq!(a.input, 2);
        assert_eq!(a.scrambled, 12);

        let c = PacketCountInfo { input: 10, ..Default::default() }
            + PacketCountInfo { input: 5, output: 1, ..Default::default() };
        assert_eq!(c.input, 15);
        assert_eq!(c.output, 1);
    }

    // ── 基本のパススルー ────────────────────────────────────────

    #[test]
    fn test_basic_passthrough_sequence() {
        let (store, sink) = make_recorder();
        let mut filter = TsPacketParserFilter::new();
        filter.connect_output(sink);
        filter.start_streaming();

        // PID 0x0100 の有効パケットを3つ(連続性カウンタ連番)
        let mut buf = Vec::new();
        for cc in 0..3u8 {
            buf.extend_from_slice(&make_ts_packet(0x0100, cc, true, b"payload"));
        }
        feed(&mut filter, &buf);

        // output_sequence=true なので、同一 PID は1シーケンスにまとまる
        let recorded = store.borrow();
        assert_eq!(recorded.len(), 3); // 3パケットが1シーケンスとして渡る

        let stats = filter.get_packet_count();
        assert_eq!(stats.input, 3);
        assert_eq!(stats.output, 3);
        assert_eq!(stats.continuity_error, 0);
        assert_eq!(filter.get_input_bytes(), (TS_PACKET_SIZE * 3) as u64);
    }

    #[test]
    fn test_single_output_mode() {
        let (store, sink) = make_recorder();
        let mut filter = TsPacketParserFilter::new();
        filter.set_output_sequence(false);
        filter.connect_output(sink);

        let mut buf = Vec::new();
        for cc in 0..2u8 {
            buf.extend_from_slice(&make_ts_packet(0x0100, cc, true, b"x"));
        }
        feed(&mut filter, &buf);

        // 各パケットが個別に出力される
        let recorded = store.borrow();
        assert_eq!(recorded.len(), 2);
        assert_eq!(recorded[0].len(), TS_PACKET_SIZE);
        assert_eq!(pid_of(&recorded[0]), 0x0100);
    }

    // ── 同期外れと再同期 ────────────────────────────────────────

    #[test]
    fn test_sync_with_garbage_prefix() {
        let (store, sink) = make_recorder();
        let mut filter = TsPacketParserFilter::new();
        filter.set_output_sequence(false);
        filter.connect_output(sink);

        let mut buf = vec![0x00, 0x11, 0x22, 0x33]; // ゴミ4バイト
        buf.extend_from_slice(&make_ts_packet(0x0100, 0, true, b"data"));
        feed(&mut filter, &buf);

        let recorded = store.borrow();
        assert_eq!(recorded.len(), 1);
        assert_eq!(pid_of(&recorded[0]), 0x0100);
        assert_eq!(filter.get_packet_count().input, 1);
    }

    // ── パケットの分割受信 ──────────────────────────────────────

    #[test]
    fn test_packet_split_across_buffers() {
        let (store, sink) = make_recorder();
        let mut filter = TsPacketParserFilter::new();
        filter.set_output_sequence(false);
        filter.connect_output(sink);

        let pkt = make_ts_packet(0x0100, 0, true, b"split");
        // 前半100バイトと後半88バイトを別々のバッファとして DataStream に流す
        let first = pkt[..100].to_vec();
        let second = pkt[100..].to_vec();
        let items: Vec<Vec<u8>> = vec![first, second];
        let mut stream = VecDataStream::new(TYPE_ID_TS_PACKET, items);
        filter.receive_data(&mut stream);

        let recorded = store.borrow();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0], pkt.to_vec());
    }

    // ── NULL パケットのフィルタリング ───────────────────────────

    #[test]
    fn test_null_packet_dropped_by_default() {
        let (store, sink) = make_recorder();
        let mut filter = TsPacketParserFilter::new();
        filter.set_output_sequence(false);
        filter.connect_output(sink);

        feed(&mut filter, &make_ts_packet(PID_NULL, 0, false, b""));

        // 既定では NULL パケットは出力されない
        assert_eq!(store.borrow().len(), 0);
        // ただし input カウントは増える
        assert_eq!(filter.get_packet_count().input, 1);
        assert_eq!(filter.get_packet_count().output, 0);
    }

    #[test]
    fn test_null_packet_output_when_enabled() {
        let (store, sink) = make_recorder();
        let mut filter = TsPacketParserFilter::new();
        filter.set_output_sequence(false);
        filter.set_output_null_packet(true);
        filter.connect_output(sink);

        feed(&mut filter, &make_ts_packet(PID_NULL, 0, false, b""));

        assert_eq!(store.borrow().len(), 1);
        assert_eq!(pid_of(&store.borrow()[0]), PID_NULL);
    }

    // ── エラーパケット ──────────────────────────────────────────

    #[test]
    fn test_format_error_dropped_by_default() {
        let (store, sink) = make_recorder();
        let mut filter = TsPacketParserFilter::new();
        filter.set_output_sequence(false);
        filter.connect_output(sink);

        // afc=0x00 は FormatError
        let mut pkt = make_ts_packet(0x0100, 0, true, b"x");
        pkt[3] = 0x00; // afc=00, cc=0 → FormatError
        feed(&mut filter, &pkt);

        assert_eq!(store.borrow().len(), 0);
        assert_eq!(filter.get_packet_count().format_error, 1);
    }

    #[test]
    fn test_error_packet_output_when_enabled() {
        let (store, sink) = make_recorder();
        let mut filter = TsPacketParserFilter::new();
        filter.set_output_sequence(false);
        filter.set_output_error_packet(true);
        filter.connect_output(sink);

        let mut pkt = make_ts_packet(0x0100, 0, true, b"x");
        pkt[3] = 0x00; // FormatError
        feed(&mut filter, &pkt);

        assert_eq!(store.borrow().len(), 1);
        assert_eq!(filter.get_packet_count().format_error, 1);
    }

    // ── シーケンスのフラッシュ ──────────────────────────────────

    #[test]
    fn test_sequence_flush_on_pid_change() {
        let (store, sink) = make_recorder();
        let mut filter = TsPacketParserFilter::new();
        filter.connect_output(sink);

        let mut buf = Vec::new();
        buf.extend_from_slice(&make_ts_packet(0x0100, 0, true, b"a"));
        buf.extend_from_slice(&make_ts_packet(0x0100, 1, true, b"b"));
        buf.extend_from_slice(&make_ts_packet(0x0200, 0, true, b"c")); // PID 変化
        feed(&mut filter, &buf);

        // 最初の2つ(PID 0x0100)が1シーケンス、3つ目(PID 0x0200)が別シーケンス
        let recorded = store.borrow();
        assert_eq!(recorded.len(), 3);
        assert_eq!(pid_of(&recorded[0]), 0x0100);
        assert_eq!(pid_of(&recorded[1]), 0x0100);
        assert_eq!(pid_of(&recorded[2]), 0x0200);
    }

    #[test]
    fn test_sequence_flush_on_max_count() {
        let (store, sink) = make_recorder();
        let mut filter = TsPacketParserFilter::new();
        assert!(filter.set_max_sequence_packet_count(2));
        filter.connect_output(sink);

        let mut buf = Vec::new();
        for cc in 0..5u8 {
            buf.extend_from_slice(&make_ts_packet(0x0100, cc, true, b"x"));
        }
        feed(&mut filter, &buf);

        // max=2 なので 2+2+1 の 3シーケンスで計5パケット
        let recorded = store.borrow();
        assert_eq!(recorded.len(), 5);
        assert_eq!(filter.get_packet_count().output, 5);
    }

    #[test]
    fn test_set_max_sequence_packet_count_guard() {
        let mut filter = TsPacketParserFilter::new();
        assert!(!filter.set_max_sequence_packet_count(0)); // < 1 は失敗
        assert_eq!(filter.get_max_sequence_packet_count(), 64); // 変わらない
        assert!(filter.set_max_sequence_packet_count(10));
        assert_eq!(filter.get_max_sequence_packet_count(), 10);
    }

    // ── 連続性エラー ────────────────────────────────────────────

    #[test]
    fn test_continuity_error_detection() {
        let mut filter = TsPacketParserFilter::new();
        filter.set_output_sequence(false);

        let mut buf = Vec::new();
        buf.extend_from_slice(&make_ts_packet(0x0100, 0, true, b"x"));
        buf.extend_from_slice(&make_ts_packet(0x0100, 1, true, b"x")); // 連番 OK
        buf.extend_from_slice(&make_ts_packet(0x0100, 5, true, b"x")); // 飛び → 連続性エラー
        feed(&mut filter, &buf);

        assert_eq!(filter.get_packet_count().input, 3);
        assert_eq!(filter.get_packet_count().continuity_error, 1);
        assert_eq!(filter.get_packet_count_pid(0x0100).continuity_error, 1);
    }

    // ── 統計の PID 別・総計と Reset ─────────────────────────────

    #[test]
    fn test_reset_accumulates_totals() {
        let mut filter = TsPacketParserFilter::new();
        filter.set_output_sequence(false);

        feed(&mut filter, &make_ts_packet(0x0100, 0, true, b"x"));
        assert_eq!(filter.get_packet_count().input, 1);

        filter.reset();

        // current はクリア、total に積算される
        assert_eq!(filter.get_packet_count().input, 0);
        assert_eq!(filter.get_total_packet_count().input, 1);
        assert_eq!(filter.get_total_input_bytes(), TS_PACKET_SIZE as u64);
        assert_eq!(filter.get_input_bytes(), 0);

        // さらに受信
        feed(&mut filter, &make_ts_packet(0x0100, 0, true, b"x"));
        assert_eq!(filter.get_total_packet_count().input, 2); // total + current
    }

    #[test]
    fn test_pid_packet_count_out_of_range() {
        let filter = TsPacketParserFilter::new();
        // PID > PID_MAX はデフォルト値
        let c = filter.get_packet_count_pid(0xFFFF);
        assert_eq!(c, PacketCountInfo::default());
        let t = filter.get_total_packet_count_pid(0xFFFF);
        assert_eq!(t, PacketCountInfo::default());
    }

    #[test]
    fn test_reset_error_packet_count() {
        let mut filter = TsPacketParserFilter::new();
        filter.set_output_error_packet(true);

        let mut pkt = make_ts_packet(0x0100, 0, true, b"x");
        pkt[3] = 0x00; // FormatError
        feed(&mut filter, &pkt);
        assert_eq!(filter.get_packet_count().format_error, 1);

        filter.reset_error_packet_count();
        assert_eq!(filter.get_packet_count().format_error, 0);
    }

    // ── ワンセグ PAT 生成 ──────────────────────────────────────

    #[test]
    fn test_1seg_pat_generation() {
        let (store, sink) = make_recorder();
        let mut filter = TsPacketParserFilter::new();
        filter.set_output_sequence(false);
        // TSID を事前設定(NIT を待たずに PAT 生成可能にする)
        assert!(filter.set_transport_stream_id(0x1234));
        filter.connect_output(sink);

        // ワンセグ PMT を 5回受信 → 5回目で PAT 生成
        let service_id = 0x0030u16;
        for cc in 0..5u8 {
            feed(
                &mut filter,
                &make_pmt_packet(ONESEG_PMT_PID_FIRST, cc, service_id),
            );
        }

        let recorded = store.borrow();
        // PAT パケット(PID 0)が含まれているはず
        let pat_packets: Vec<&Vec<u8>> =
            recorded.iter().filter(|p| pid_of(p) == PID_PAT).collect();
        assert_eq!(pat_packets.len(), 1, "PAT は1つだけ生成される");

        // PAT の中身: table_id=0x00、payload に service_id と PMT PID が入る
        let pat = pat_packets[0];
        assert_eq!(pat.len(), TS_PACKET_SIZE);
        assert_eq!(pat[5], 0x00); // table_id (pointer_field=pat[4]=0x00 の次)
        // TSID
        assert_eq!(((pat[8] as u16) << 8) | pat[9] as u16, 0x1234);
    }

    #[test]
    fn test_1seg_pat_not_generated_when_disabled() {
        let (store, sink) = make_recorder();
        let mut filter = TsPacketParserFilter::new();
        filter.set_output_sequence(false);
        filter.set_generate_1seg_pat(false); // PAT 生成オフ
        filter.set_transport_stream_id(0x1234);
        filter.connect_output(sink);

        for cc in 0..6u8 {
            feed(
                &mut filter,
                &make_pmt_packet(ONESEG_PMT_PID_FIRST, cc, 0x0030),
            );
        }

        let recorded = store.borrow();
        let pat_count = recorded.iter().filter(|p| pid_of(p) == PID_PAT).count();
        assert_eq!(pat_count, 0); // 生成されない
    }

    #[test]
    fn test_1seg_pat_not_generated_when_pat_present() {
        let (store, sink) = make_recorder();
        let mut filter = TsPacketParserFilter::new();
        filter.set_output_sequence(false);
        filter.set_transport_stream_id(0x1234);
        filter.connect_output(sink);

        // 先に PAT を受信 → has_pat=true → ワンセグ PAT は生成されない
        feed(&mut filter, &make_ts_packet(PID_PAT, 0, true, b"\x00\x00"));

        for cc in 0..6u8 {
            feed(
                &mut filter,
                &make_pmt_packet(ONESEG_PMT_PID_FIRST, cc, 0x0030),
            );
        }

        let recorded = store.borrow();
        // 生成された PAT は無い(受信した PAT 自体は output される=1つ)
        // 受信 PAT は PID 0 だが、これは入力パケットの通過。生成 PAT が増えていないことを確認するため
        // PID 0 のパケットは「受信した1つ」だけのはず。
        let pid0_count = recorded.iter().filter(|p| pid_of(p) == PID_PAT).count();
        assert_eq!(pid0_count, 1); // 受信 PAT の通過のみ、生成 PAT は無い
    }

    // ── FilterBase デフォルト ───────────────────────────────────

    #[test]
    fn test_filter_base_ports() {
        let mut filter = TsPacketParserFilter::new();
        assert_eq!(filter.input_count(), 1);
        assert_eq!(filter.output_count(), 1);
        assert!(filter.start_streaming());
        assert!(filter.stop_streaming());
        assert!(filter.initialize());
    }
}
