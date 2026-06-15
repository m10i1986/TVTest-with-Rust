// Rust port of LibISDB/TS/StreamSelector.cpp + StreamSelector.hpp
// StreamSelector.cpp:39, StreamSelector.hpp:43
//
// 特定サービス/ストリーム種別の TS パケットのみを通過させ、必要なら PAT を
// 対象サービスのみに絞って再生成するフィルタ。
//
// 原実装は PIDMapManager + PSITableBase コールバックで PAT/PMT/CAT をパースして
// PMTPIDList / EMMPIDList を構築するが、そのパース部分は libisdb_ts_tables /
// libisdb_psi_table の責務である。本クレートでは StreamSelector 固有の純粋ロジック
//   - StreamTypeTable          : StreamFlag → ストリーム種別許可テーブル (StreamSelector.cpp:372)
//   - make_target_pid_table    : MakeTargetPIDTable (StreamSelector.cpp:141)
//   - get_service_index_by_id  : GetServiceIndexByID (StreamSelector.cpp:172)
//   - input_packet 判定        : InputPacket の通過/破棄判定 (StreamSelector.cpp:76)
//   - make_pat                 : MakePAT バイト列再生成 (StreamSelector.cpp:287)
// を移植する。PAT/PMT/CAT のパース結果(PMTPIDInfo リスト・EMM PID リスト・対象 PMT PID)は
// 呼び出し側が set_pmt_pid_list / set_emm_pid_list / set_target_pmt_pid で通知する。

use libisdb_ts_info::{
    PID_INVALID, PID_MAX, PID_PAT,
    STREAM_TYPE_MPEG1_VIDEO, STREAM_TYPE_MPEG2_VIDEO, STREAM_TYPE_MPEG1_AUDIO,
    STREAM_TYPE_MPEG2_AUDIO, STREAM_TYPE_AAC, STREAM_TYPE_MPEG4_VISUAL,
    STREAM_TYPE_MPEG4_AUDIO, STREAM_TYPE_H264, STREAM_TYPE_H265, STREAM_TYPE_AC3,
    STREAM_TYPE_DTS, STREAM_TYPE_TRUEHD, STREAM_TYPE_DOLBY_DIGITAL_PLUS,
    STREAM_TYPE_CAPTION, STREAM_TYPE_DATA_CARROUSEL,
};
use libisdb_crc::crc32_mpeg2;
use libisdb_utilities::{load16_be, load32_be, store32_be};

/// 無効なサービス ID (LibISDBConsts.hpp:39 SERVICE_ID_INVALID)
pub const SERVICE_ID_INVALID: u16 = 0x0000;

const TS_PACKET_SIZE: usize = 188;
const PID_TABLE_SIZE: usize = PID_MAX as usize + 1;

// StreamFlag ビット (StreamSelector.hpp:46)
pub mod stream_flag {
    pub const NONE: u32               = 0x0000_0000;
    pub const MPEG1_VIDEO: u32        = 0x0000_0001;
    pub const MPEG2_VIDEO: u32        = 0x0000_0002;
    pub const MPEG1_AUDIO: u32        = 0x0000_0004;
    pub const MPEG2_AUDIO: u32        = 0x0000_0008;
    pub const AAC: u32                = 0x0000_0010;
    pub const MPEG4_VISUAL: u32       = 0x0000_0020;
    pub const MPEG4_AUDIO: u32        = 0x0000_0040;
    pub const H264: u32               = 0x0000_0080;
    pub const H265: u32               = 0x0000_0100;
    pub const AC3: u32                = 0x0000_0200;
    pub const DTS: u32                = 0x0000_0400;
    pub const TRUEHD: u32             = 0x0000_0800;
    pub const DOLBY_DIGITAL_PLUS: u32 = 0x0000_1000;
    pub const CAPTION: u32            = 0x0000_2000;
    pub const DATA_CARROUSEL: u32     = 0x0000_4000;
    pub const AUDIO: u32 =
        MPEG1_AUDIO | MPEG2_AUDIO | AAC | MPEG4_AUDIO | AC3 | DTS | TRUEHD | DOLBY_DIGITAL_PLUS;
    pub const VIDEO: u32 = MPEG1_VIDEO | MPEG2_VIDEO | MPEG4_VISUAL | H264 | H265;
    pub const ALL: u32 = 0xFFFF_FFFF;
}

/// FromStreamFlags のストリーム種別対応表 (StreamSelector.cpp:386)
const STREAM_TYPE_LIST: [u8; 15] = [
    STREAM_TYPE_MPEG1_VIDEO,
    STREAM_TYPE_MPEG2_VIDEO,
    STREAM_TYPE_MPEG1_AUDIO,
    STREAM_TYPE_MPEG2_AUDIO,
    STREAM_TYPE_AAC,
    STREAM_TYPE_MPEG4_VISUAL,
    STREAM_TYPE_MPEG4_AUDIO,
    STREAM_TYPE_H264,
    STREAM_TYPE_H265,
    STREAM_TYPE_AC3,
    STREAM_TYPE_DTS,
    STREAM_TYPE_TRUEHD,
    STREAM_TYPE_DOLBY_DIGITAL_PLUS,
    STREAM_TYPE_CAPTION,
    STREAM_TYPE_DATA_CARROUSEL,
];

/// ストリーム種別の許可テーブル。StreamSelector::StreamTypeTable (StreamSelector.hpp:69)。
/// 原実装の std::bitset<256> を [bool; 256] で表現する。
#[derive(Clone)]
pub struct StreamTypeTable {
    bits: [bool; 256],
}

impl Default for StreamTypeTable {
    fn default() -> Self {
        // 既定構築は全 true (StreamSelector.cpp:372 → Set())
        Self { bits: [true; 256] }
    }
}

impl StreamTypeTable {
    /// 全種別を許可した状態で生成 (StreamSelector.cpp:372)
    pub fn new() -> Self {
        Self::default()
    }

    /// StreamFlag から生成 (StreamSelector.cpp:378)
    pub fn from_stream_flags(flags: u32) -> Self {
        let mut t = Self::new();
        t.set_from_stream_flags(flags);
        t
    }

    /// 指定種別が許可されているか (operator[], StreamSelector.hpp:75)
    pub fn get(&self, stream_type: u8) -> bool {
        self.bits[stream_type as usize]
    }

    pub fn set_all(&mut self) {
        self.bits = [true; 256];
    }

    pub fn reset_all(&mut self) {
        self.bits = [false; 256];
    }

    pub fn set(&mut self, stream_type: u8, value: bool) {
        self.bits[stream_type as usize] = value;
    }

    /// FromStreamFlags (StreamSelector.cpp:384)
    pub fn set_from_stream_flags(&mut self, flags: u32) {
        self.set_all();
        for (i, &st) in STREAM_TYPE_LIST.iter().enumerate() {
            if flags & (1u32 << i) == 0 {
                self.bits[st as usize] = false;
            }
        }
    }
}

/// ES 情報。StreamSelector::ESInfo (StreamSelector.hpp:110)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EsInfo {
    pub pid: u16,
    pub stream_type: u8,
}

/// PMT PID 情報。StreamSelector::PMTPIDInfo (StreamSelector.hpp:115)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PmtPidInfo {
    pub service_id: u16,
    pub pmt_pid: u16,
    pub pcr_pid: u16,
    pub ecm_pid_list: Vec<u16>,
    pub es_list: Vec<EsInfo>,
}

impl PmtPidInfo {
    pub fn new(service_id: u16, pmt_pid: u16) -> Self {
        Self {
            service_id,
            pmt_pid,
            pcr_pid: PID_INVALID,
            ecm_pid_list: Vec::new(),
            es_list: Vec::new(),
        }
    }
}

/// PAT 再生成のためのバージョン管理状態 (StreamSelector.hpp:136-139)
#[derive(Clone, Copy, Debug, Default)]
struct PatRewriteState {
    last_tsid: u16,
    last_pmt_pid: u16,
    last_version: u8,
    version: u8,
}

/// ストリーム選択器。StreamSelector クラス本体。
pub struct StreamSelector {
    target_service_id: u16,
    target_stream_type_enabled: bool,
    target_stream_type: StreamTypeTable,
    generate_pat: bool,

    pmt_pid_list: Vec<PmtPidInfo>,
    emm_pid_list: Vec<u16>,
    target_pid_table: Box<[bool; PID_TABLE_SIZE]>,

    target_pmt_pid: u16,
    pat_state: PatRewriteState,
}

impl Default for StreamSelector {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamSelector {
    /// StreamSelector::StreamSelector (StreamSelector.cpp:39)
    pub fn new() -> Self {
        let mut s = Self {
            target_service_id: SERVICE_ID_INVALID,
            target_stream_type_enabled: false,
            target_stream_type: StreamTypeTable::new(),
            generate_pat: true,
            pmt_pid_list: Vec::new(),
            emm_pid_list: Vec::new(),
            target_pid_table: Box::new([false; PID_TABLE_SIZE]),
            target_pmt_pid: PID_INVALID,
            pat_state: PatRewriteState {
                last_tsid: 0, // TRANSPORT_STREAM_ID_INVALID
                last_pmt_pid: PID_INVALID,
                last_version: 0,
                version: 0,
            },
        };
        s.reset();
        s
    }

    /// StreamSelector::Reset (StreamSelector.cpp:55)
    /// PID マップ再構築(NIT/PMT/CAT)は呼び出し側の責務。
    pub fn reset(&mut self) {
        self.pmt_pid_list.clear();
        self.emm_pid_list.clear();
        self.target_pid_table.fill(false);
        self.target_pmt_pid = PID_INVALID;
        self.pat_state = PatRewriteState {
            last_tsid: 0,
            last_pmt_pid: PID_INVALID,
            last_version: 0,
            version: 0,
        };
    }

    pub fn target_service_id(&self) -> u16 {
        self.target_service_id
    }
    pub fn target_stream_type(&self) -> &StreamTypeTable {
        &self.target_stream_type
    }
    pub fn generate_pat(&self) -> bool {
        self.generate_pat
    }
    /// SetGeneratePAT (StreamSelector.cpp:135)
    pub fn set_generate_pat(&mut self, generate: bool) {
        self.generate_pat = generate;
    }
    pub fn target_pmt_pid(&self) -> u16 {
        self.target_pmt_pid
    }

    /// SetTarget(ServiceID, StreamTypeTable*) (StreamSelector.cpp:101)
    /// `stream_type` が None の場合は全ストリーム種別を通過。
    pub fn set_target(&mut self, service_id: u16, stream_type: Option<StreamTypeTable>) {
        self.target_service_id = service_id;
        match stream_type {
            Some(t) => {
                self.target_stream_type_enabled = true;
                self.target_stream_type = t;
            }
            None => {
                self.target_stream_type_enabled = false;
            }
        }

        self.target_pmt_pid = PID_INVALID;
        if service_id != SERVICE_ID_INVALID {
            if let Some(idx) = self.get_service_index_by_id(service_id) {
                self.target_pmt_pid = self.pmt_pid_list[idx].pmt_pid;
            }
        }

        self.make_target_pid_table();
    }

    /// SetTarget(ServiceID, StreamFlag) (StreamSelector.cpp:124)
    pub fn set_target_flags(&mut self, service_id: u16, stream_flags: u32) {
        if stream_flags == stream_flag::ALL {
            self.set_target(service_id, None);
        } else {
            self.set_target(service_id, Some(StreamTypeTable::from_stream_flags(stream_flags)));
        }
    }

    /// パース済みの PMT PID 情報リストを設定する(OnPATSection/OnPMTSection 結果)。
    pub fn set_pmt_pid_list(&mut self, list: Vec<PmtPidInfo>) {
        self.pmt_pid_list = list;
        // 対象 PMT PID を再計算
        self.target_pmt_pid = PID_INVALID;
        if self.target_service_id != SERVICE_ID_INVALID {
            if let Some(idx) = self.get_service_index_by_id(self.target_service_id) {
                self.target_pmt_pid = self.pmt_pid_list[idx].pmt_pid;
            }
        }
        self.make_target_pid_table();
    }

    /// パース済みの EMM PID リストを設定する(OnCATSection 結果)。
    pub fn set_emm_pid_list(&mut self, list: Vec<u16>) {
        self.emm_pid_list = list;
        self.make_target_pid_table();
    }

    pub fn pmt_pid_list(&self) -> &[PmtPidInfo] {
        &self.pmt_pid_list
    }

    /// GetServiceIndexByID (StreamSelector.cpp:172)
    /// 原実装は末尾から検索する。
    pub fn get_service_index_by_id(&self, service_id: u16) -> Option<usize> {
        self.pmt_pid_list
            .iter()
            .enumerate()
            .rev()
            .find(|(_, e)| e.service_id == service_id)
            .map(|(i, _)| i)
    }

    /// 指定 PID が通過対象かどうか(MakeTargetPIDTable の結果参照)。
    pub fn is_target_pid(&self, pid: u16) -> bool {
        if pid as usize >= PID_TABLE_SIZE {
            return false;
        }
        self.target_pid_table[pid as usize]
    }

    /// MakeTargetPIDTable (StreamSelector.cpp:141)
    pub fn make_target_pid_table(&mut self) {
        if self.pmt_pid_list.is_empty() {
            let v = self.target_service_id == SERVICE_ID_INVALID;
            self.target_pid_table.fill(v);
            return;
        }

        self.target_pid_table.fill(false);

        for pmt in &self.pmt_pid_list {
            if self.target_service_id == SERVICE_ID_INVALID
                || self.target_service_id == pmt.service_id
            {
                self.target_pid_table[pmt.pmt_pid as usize] = true;

                if pmt.pcr_pid != PID_INVALID {
                    self.target_pid_table[pmt.pcr_pid as usize] = true;
                }

                for &ecm in &pmt.ecm_pid_list {
                    self.target_pid_table[ecm as usize] = true;
                }

                for es in &pmt.es_list {
                    if !self.target_stream_type_enabled
                        || self.target_stream_type.get(es.stream_type)
                    {
                        self.target_pid_table[es.pid as usize] = true;
                    }
                }
            }
        }

        for &emm in &self.emm_pid_list {
            self.target_pid_table[emm as usize] = true;
        }
    }

    /// 入力パケットの処置を判定する。InputPacket (StreamSelector.cpp:76)。
    ///
    /// PID マップへの格納(PAT/PMT/CAT パース)は呼び出し側で先に行う前提。
    /// 戻り値:
    ///   - `PacketAction::Pass`      : そのまま通過
    ///   - `PacketAction::Drop`      : 破棄
    ///   - `PacketAction::RewritePat`: PAT を再生成して出力すべき(make_pat を呼ぶ)
    pub fn decide_packet(&self, pid: u16) -> PacketAction {
        if self.target_service_id == SERVICE_ID_INVALID && !self.target_stream_type_enabled {
            return PacketAction::Pass;
        }

        if pid < 0x0030 || self.is_target_pid(pid) {
            if pid == PID_PAT
                && self.generate_pat
                && self.target_pmt_pid != PID_INVALID
            {
                return PacketAction::RewritePat;
            }
            return PacketAction::Pass;
        }

        PacketAction::Drop
    }

    /// MakePAT (StreamSelector.cpp:287)
    /// 元の PAT パケット(188 バイト)から対象サービスの PMT のみを残した PAT を生成する。
    /// 成功時は新しい 188 バイトパケットを返す。
    pub fn make_pat(&mut self, src: &[u8; TS_PACKET_SIZE]) -> Option<[u8; TS_PACKET_SIZE]> {
        // ペイロード先頭位置の算出 (TSPacket の payload_start_pos 相当)
        let afc = (src[3] >> 4) & 0x03;
        let payload_unit_start = (src[1] & 0x40) != 0;
        let mut header_size: usize = match afc {
            1 => 4,
            3 => {
                let af_len = src[4] as usize;
                let pos = af_len + 5;
                if pos < TS_PACKET_SIZE {
                    pos
                } else {
                    return None;
                }
            }
            _ => return None,
        };

        if !payload_unit_start {
            return None;
        }

        // pointer_field
        let unit_start_pos = src[header_size] as usize + 1;
        let payload_off = header_size + unit_start_pos;
        header_size += unit_start_pos;
        if header_size >= TS_PACKET_SIZE {
            return None;
        }

        let mut dst = [0u8; TS_PACKET_SIZE];
        // ヘッダ部コピー
        dst[..header_size].copy_from_slice(&src[..header_size]);
        // 残りを 0xFF で埋める
        for b in dst.iter_mut().take(TS_PACKET_SIZE).skip(header_size) {
            *b = 0xFF;
        }

        // payload_off は PAT セクション先頭(payloadData)
        let payload = &src[payload_off..];
        // table_id
        if payload[0] != 0 {
            return None;
        }

        let section_length = (((payload[1] & 0x0F) as usize) << 8) | payload[2] as usize;
        if section_length > TS_PACKET_SIZE - header_size - 3 - 4 {
            return None;
        }

        // CRC 検証
        let crc = load32_be(&payload[3 + section_length - 4..]);
        if crc32_mpeg2(&payload[..3 + section_length - 4], 0xFFFF_FFFF) != crc {
            return None;
        }

        let tsid = load16_be(&payload[3..]);
        let version = (payload[5] & 0x3E) >> 1;
        if tsid != self.pat_state.last_tsid {
            self.pat_state.version = 0;
        } else if self.target_pmt_pid != self.pat_state.last_pmt_pid
            || version != self.pat_state.last_version
        {
            self.pat_state.version = (self.pat_state.version + 1) & 0x1F;
        }
        self.pat_state.last_tsid = tsid;
        self.pat_state.last_pmt_pid = self.target_pmt_pid;
        self.pat_state.last_version = version;

        // program loop。dst の書き込み位置 = header_size + 8
        let program_data_off = payload_off + 8;
        let mut pos = 0usize;
        let mut new_program_list_size = 0usize;
        let mut has_pmt_pid = false;
        // SectionLength - (5 + 4): program loop の長さ
        let loop_len = section_length - (5 + 4);
        while pos < loop_len {
            let pid = load16_be(&src[program_data_off + pos + 2..]) & 0x1FFF;
            if pid == 0x0010 || pid == self.target_pmt_pid {
                let dst_pos = header_size + 8 + new_program_list_size;
                dst[dst_pos..dst_pos + 4]
                    .copy_from_slice(&src[program_data_off + pos..program_data_off + pos + 4]);
                new_program_list_size += 4;
                if pid == self.target_pmt_pid {
                    has_pmt_pid = true;
                }
            }
            pos += 4;
        }
        if !has_pmt_pid {
            return None;
        }

        // PAT セクションヘッダ書き換え。dst のセクション先頭 = header_size
        let sec = header_size;
        let new_section_len = new_program_list_size + (5 + 4);
        dst[sec] = 0;
        dst[sec + 1] = (payload[1] & 0xF0) | ((new_section_len >> 8) as u8);
        dst[sec + 2] = (new_section_len & 0xFF) as u8;
        dst[sec + 3] = (tsid >> 8) as u8;
        dst[sec + 4] = (tsid & 0xFF) as u8;
        dst[sec + 5] = (payload[5] & 0xC1) | (self.pat_state.version << 1);
        dst[sec + 6] = payload[6];
        dst[sec + 7] = payload[7];
        let crc_pos = sec + 8 + new_program_list_size;
        let new_crc = crc32_mpeg2(&dst[sec..sec + 8 + new_program_list_size], 0xFFFF_FFFF);
        store32_be(&mut dst[crc_pos..crc_pos + 4], new_crc);

        Some(dst)
    }
}

/// 入力パケットの処置。InputPacket (StreamSelector.cpp:76) の戻り値に相当。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PacketAction {
    /// そのまま通過
    Pass,
    /// 破棄
    Drop,
    /// PAT を再生成して出力(make_pat を呼ぶ)
    RewritePat,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── StreamTypeTable ─────────────────────────────────────

    #[test]
    fn test_stream_type_table_default_all_true() {
        let t = StreamTypeTable::new();
        assert!(t.get(STREAM_TYPE_H264));
        assert!(t.get(STREAM_TYPE_AAC));
        assert!(t.get(0));
        assert!(t.get(255));
    }

    #[test]
    fn test_stream_type_table_from_flags_video_only() {
        let t = StreamTypeTable::from_stream_flags(stream_flag::VIDEO);
        // 映像は通過
        assert!(t.get(STREAM_TYPE_MPEG2_VIDEO));
        assert!(t.get(STREAM_TYPE_H264));
        assert!(t.get(STREAM_TYPE_H265));
        // 音声・字幕は遮断
        assert!(!t.get(STREAM_TYPE_AAC));
        assert!(!t.get(STREAM_TYPE_AC3));
        assert!(!t.get(STREAM_TYPE_CAPTION));
        // リストに無い種別(例: 0x00)は true のまま(Set() 後に個別 reset するため)
        assert!(t.get(0x00));
    }

    #[test]
    fn test_stream_type_table_from_flags_audio_only() {
        let t = StreamTypeTable::from_stream_flags(stream_flag::AUDIO);
        assert!(t.get(STREAM_TYPE_AAC));
        assert!(t.get(STREAM_TYPE_AC3));
        assert!(!t.get(STREAM_TYPE_H264));
        assert!(!t.get(STREAM_TYPE_MPEG2_VIDEO));
    }

    #[test]
    fn test_stream_type_table_from_flags_none() {
        let t = StreamTypeTable::from_stream_flags(stream_flag::NONE);
        // 全 STREAM_TYPE_LIST が reset される
        assert!(!t.get(STREAM_TYPE_MPEG2_VIDEO));
        assert!(!t.get(STREAM_TYPE_AAC));
        assert!(!t.get(STREAM_TYPE_CAPTION));
    }

    // ─── make_target_pid_table / get_service_index_by_id ──────

    fn sample_pmt_list() -> Vec<PmtPidInfo> {
        vec![
            PmtPidInfo {
                service_id: 0x0400,
                pmt_pid: 0x1FC8,
                pcr_pid: 0x0100,
                ecm_pid_list: vec![0x0200],
                es_list: vec![
                    EsInfo { pid: 0x0101, stream_type: STREAM_TYPE_H264 },
                    EsInfo { pid: 0x0102, stream_type: STREAM_TYPE_AAC },
                    EsInfo { pid: 0x0103, stream_type: STREAM_TYPE_CAPTION },
                ],
            },
            PmtPidInfo {
                service_id: 0x0401,
                pmt_pid: 0x1FC9,
                pcr_pid: 0x0110,
                ecm_pid_list: vec![],
                es_list: vec![
                    EsInfo { pid: 0x0111, stream_type: STREAM_TYPE_H264 },
                ],
            },
        ]
    }

    #[test]
    fn test_get_service_index_by_id() {
        let mut s = StreamSelector::new();
        s.set_pmt_pid_list(sample_pmt_list());
        assert_eq!(s.get_service_index_by_id(0x0400), Some(0));
        assert_eq!(s.get_service_index_by_id(0x0401), Some(1));
        assert_eq!(s.get_service_index_by_id(0x9999), None);
    }

    #[test]
    fn test_pid_table_all_services_when_no_target() {
        let mut s = StreamSelector::new();
        s.set_pmt_pid_list(sample_pmt_list());
        // target 未設定(SERVICE_ID_INVALID) → 全サービスの PID が通過
        assert!(s.is_target_pid(0x1FC8));
        assert!(s.is_target_pid(0x0101));
        assert!(s.is_target_pid(0x0111)); // 別サービスの ES も通過
    }

    #[test]
    fn test_pid_table_single_service() {
        let mut s = StreamSelector::new();
        s.set_pmt_pid_list(sample_pmt_list());
        s.set_target(0x0400, None);
        // 対象サービスの PID
        assert!(s.is_target_pid(0x1FC8)); // PMT
        assert!(s.is_target_pid(0x0100)); // PCR
        assert!(s.is_target_pid(0x0200)); // ECM
        assert!(s.is_target_pid(0x0101)); // ES H264
        assert!(s.is_target_pid(0x0102)); // ES AAC
        assert!(s.is_target_pid(0x0103)); // ES Caption
        // 別サービスの PID は遮断
        assert!(!s.is_target_pid(0x1FC9));
        assert!(!s.is_target_pid(0x0111));
    }

    #[test]
    fn test_pid_table_stream_type_filter() {
        let mut s = StreamSelector::new();
        s.set_pmt_pid_list(sample_pmt_list());
        // 映像のみ
        s.set_target_flags(0x0400, stream_flag::VIDEO);
        assert!(s.is_target_pid(0x0101)); // H264 通過
        assert!(!s.is_target_pid(0x0102)); // AAC 遮断
        assert!(!s.is_target_pid(0x0103)); // Caption 遮断
        // PMT/PCR/ECM は種別に関係なく通過
        assert!(s.is_target_pid(0x1FC8));
        assert!(s.is_target_pid(0x0100));
        assert!(s.is_target_pid(0x0200));
    }

    #[test]
    fn test_emm_pid_added() {
        let mut s = StreamSelector::new();
        s.set_pmt_pid_list(sample_pmt_list());
        s.set_target(0x0400, None);
        s.set_emm_pid_list(vec![0x0085]);
        assert!(s.is_target_pid(0x0085));
    }

    #[test]
    fn test_empty_pmt_list_no_target_passes_all() {
        let mut s = StreamSelector::new();
        // new()/reset() 直後は make_target_pid_table 未実行のため全 false。
        // 原実装も MakeTargetPIDTable が呼ばれる(SetTarget/PAT 更新)まで全 false。
        assert!(!s.is_target_pid(0x0123));
        // PMT リスト空のまま make_target_pid_table が呼ばれると、
        // target 未設定なら全 PID 通過になる(MakeTargetPIDTable:144)。
        s.make_target_pid_table();
        assert!(s.is_target_pid(0x0123));
    }

    #[test]
    fn test_empty_pmt_list_with_target_blocks_all() {
        let mut s = StreamSelector::new();
        s.set_target(0x0400, None);
        // PMT リスト空 & target 設定 → 全 PID 遮断
        assert!(!s.is_target_pid(0x0123));
    }

    // ─── decide_packet ───────────────────────────────────────

    #[test]
    fn test_decide_packet_pass_through_when_no_filter() {
        let s = StreamSelector::new();
        // target 未設定 & stream type 無効 → 全通過
        assert_eq!(s.decide_packet(0x0123), PacketAction::Pass);
        assert_eq!(s.decide_packet(PID_PAT), PacketAction::Pass);
    }

    #[test]
    fn test_decide_packet_low_pid_always_pass() {
        let mut s = StreamSelector::new();
        s.set_pmt_pid_list(sample_pmt_list());
        s.set_target(0x0400, None);
        // PID < 0x0030 は常に通過(ただし PAT は条件次第で書換)
        assert_eq!(s.decide_packet(0x0011), PacketAction::Pass); // SDT
    }

    #[test]
    fn test_decide_packet_drop_other_service() {
        let mut s = StreamSelector::new();
        s.set_pmt_pid_list(sample_pmt_list());
        s.set_target(0x0400, None);
        // 別サービスの ES は破棄
        assert_eq!(s.decide_packet(0x0111), PacketAction::Drop);
    }

    #[test]
    fn test_decide_packet_pat_rewrite() {
        let mut s = StreamSelector::new();
        s.set_pmt_pid_list(sample_pmt_list());
        s.set_target(0x0400, None);
        // target_pmt_pid が設定済 & generate_pat=true → PAT 書換
        assert_eq!(s.decide_packet(PID_PAT), PacketAction::RewritePat);
    }

    #[test]
    fn test_decide_packet_pat_pass_when_generate_off() {
        let mut s = StreamSelector::new();
        s.set_pmt_pid_list(sample_pmt_list());
        s.set_target(0x0400, None);
        s.set_generate_pat(false);
        // generate_pat=false → そのまま通過
        assert_eq!(s.decide_packet(PID_PAT), PacketAction::Pass);
    }

    // ─── make_pat ────────────────────────────────────────────

    /// PAT パケット(188 バイト)を構築する。programs: (program_number, pid) のリスト。
    fn build_pat_packet(tsid: u16, version: u8, programs: &[(u16, u16)]) -> [u8; TS_PACKET_SIZE] {
        let mut pkt = [0xFFu8; TS_PACKET_SIZE];
        // TS header: sync, payload_unit_start, PID=0
        pkt[0] = 0x47;
        pkt[1] = 0x40; // payload_unit_start_indicator + PID high(0)
        pkt[2] = 0x00; // PID low
        pkt[3] = 0x10; // adaptation_field_control=01, CC=0
        pkt[4] = 0x00; // pointer_field=0

        // PAT セクション(payload 先頭 = 5)
        let sec = 5;
        let section_length = 5 + programs.len() * 4 + 4; // tsid(2)+ver(1)+sec_no(1)+last(1) + programs + CRC(4)
        pkt[sec] = 0x00; // table_id
        pkt[sec + 1] = 0xB0 | ((section_length >> 8) as u8); // section_syntax_indicator + length high
        pkt[sec + 2] = (section_length & 0xFF) as u8;
        pkt[sec + 3] = (tsid >> 8) as u8;
        pkt[sec + 4] = (tsid & 0xFF) as u8;
        pkt[sec + 5] = 0xC1 | (version << 1); // reserved + version + current_next
        pkt[sec + 6] = 0x00; // section_number
        pkt[sec + 7] = 0x00; // last_section_number

        let mut p = sec + 8;
        for &(prog, pid) in programs {
            pkt[p] = (prog >> 8) as u8;
            pkt[p + 1] = (prog & 0xFF) as u8;
            pkt[p + 2] = 0xE0 | ((pid >> 8) as u8);
            pkt[p + 3] = (pid & 0xFF) as u8;
            p += 4;
        }

        // CRC32-MPEG2 over section from table_id to before CRC
        let crc = crc32_mpeg2(&pkt[sec..p], 0xFFFF_FFFF);
        store32_be(&mut pkt[p..p + 4], crc);

        pkt
    }

    #[test]
    fn test_make_pat_filters_to_target_pmt() {
        let mut s = StreamSelector::new();
        s.set_pmt_pid_list(sample_pmt_list());
        s.set_target(0x0400, None); // target_pmt_pid = 0x1FC8

        // 元 PAT: NIT(prog 0 → PID 0x0010) + 2 サービス
        let src = build_pat_packet(
            0x7FE0,
            5,
            &[(0x0000, 0x0010), (0x0400, 0x1FC8), (0x0401, 0x1FC9)],
        );
        let dst = s.make_pat(&src).expect("should make PAT");

        // 生成された PAT を検証: NIT + 対象サービスのみ
        let sec = 5;
        let section_length = (((dst[sec + 1] & 0x0F) as usize) << 8) | dst[sec + 2] as usize;
        // program loop = section_length - (5 + 4) = NIT(4) + PMT(4) = 8
        assert_eq!(section_length - 9, 8);
        // TSID 保持
        assert_eq!(load16_be(&dst[sec + 3..]), 0x7FE0);
        // program 0: NIT
        assert_eq!(load16_be(&dst[sec + 8..]), 0x0000);
        assert_eq!(load16_be(&dst[sec + 10..]) & 0x1FFF, 0x0010);
        // program 1: 対象 PMT
        assert_eq!(load16_be(&dst[sec + 12..]), 0x0400);
        assert_eq!(load16_be(&dst[sec + 14..]) & 0x1FFF, 0x1FC8);

        // CRC 自己検証
        let total = 3 + section_length;
        let check = crc32_mpeg2(&dst[sec..sec + total], 0xFFFF_FFFF);
        assert_eq!(check, 0);
    }

    #[test]
    fn test_make_pat_fails_without_target_pmt() {
        let mut s = StreamSelector::new();
        s.set_pmt_pid_list(sample_pmt_list());
        s.set_target(0x0401, None); // target_pmt_pid = 0x1FC9

        // 元 PAT に 0x1FC9 が含まれない
        let src = build_pat_packet(0x7FE0, 0, &[(0x0000, 0x0010), (0x0400, 0x1FC8)]);
        assert!(s.make_pat(&src).is_none());
    }

    #[test]
    fn test_make_pat_version_increments_on_change() {
        let mut s = StreamSelector::new();
        s.set_pmt_pid_list(sample_pmt_list());
        s.set_target(0x0400, None);

        let src_v5 = build_pat_packet(0x7FE0, 5, &[(0x0000, 0x0010), (0x0400, 0x1FC8)]);
        let dst1 = s.make_pat(&src_v5).unwrap();
        // 初回(last_tsid=0 → tsid=0x7FE0 で異なる) → version=0
        let sec = 5;
        let v1 = (dst1[sec + 5] & 0x3E) >> 1;
        assert_eq!(v1, 0);

        // 同じ TSID、元バージョン変化(5→6) → version インクリメント
        let src_v6 = build_pat_packet(0x7FE0, 6, &[(0x0000, 0x0010), (0x0400, 0x1FC8)]);
        let dst2 = s.make_pat(&src_v6).unwrap();
        let v2 = (dst2[sec + 5] & 0x3E) >> 1;
        assert_eq!(v2, 1);
    }

    #[test]
    fn test_make_pat_generated_packet_parses() {
        let mut s = StreamSelector::new();
        s.set_pmt_pid_list(sample_pmt_list());
        s.set_target(0x0400, None);
        let src = build_pat_packet(0x7FE0, 0, &[(0x0000, 0x0010), (0x0400, 0x1FC8)]);
        let dst = s.make_pat(&src).unwrap();

        let mut pkt = libisdb_ts_packet::TsPacket::new(&dst);
        let result = pkt.parse_packet(None);
        assert!(matches!(result, libisdb_ts_packet::ParseResult::Ok));
        assert_eq!(pkt.get_pid(), PID_PAT);
    }

    #[test]
    fn test_make_pat_rejects_bad_crc() {
        let mut s = StreamSelector::new();
        s.set_pmt_pid_list(sample_pmt_list());
        s.set_target(0x0400, None);
        let mut src = build_pat_packet(0x7FE0, 0, &[(0x0000, 0x0010), (0x0400, 0x1FC8)]);
        // CRC を破壊(セクション末尾付近)
        src[20] ^= 0xFF;
        assert!(s.make_pat(&src).is_none());
    }
}
