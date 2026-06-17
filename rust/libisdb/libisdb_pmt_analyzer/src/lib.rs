// Rust port of LibISDB/Filters/AnalyzerFilter.cpp の PMT→ServiceInfo 構築ロジック。
// AnalyzerFilter.cpp:1967 (OnPMTSection) + AnalyzerFilter.hpp:73 (ESInfo/ServiceInfo)
//
// AnalyzerFilter は SingleIOFilter を継承する重いフィルタだが、その中核である
// 「PMT をストリーム種別ごとに振り分けて ServiceInfo を構築する」純粋ロジックを
// 独立クレートとして切り出す。入力はパース済みの libisdb_ts_tables::PMTTable、
// 出力は種別分けされた ServiceInfo。PID マップ・フィルタグラフ・SDT 連携は対象外。
//
// 移植対象:
//   - OnPMTSection の ES 振り分け + component_tag ソート (AnalyzerFilter.cpp:1991-2074)
//
// 階層伝送記述子(HierarchicalTransmissionDescriptor)は libisdb_descriptor 未移植のため、
// component_tag(StreamIdDescriptor 0x52) と quality_level/hierarchical_reference_pid
// (HierarchicalTransmissionDescriptor 0xC0) を取得する。

use libisdb_ts_info::{
    PID_INVALID, STREAM_TYPE_INVALID,
    STREAM_TYPE_MPEG1_VIDEO, STREAM_TYPE_MPEG2_VIDEO, STREAM_TYPE_MPEG4_VISUAL,
    STREAM_TYPE_H264, STREAM_TYPE_H265,
    STREAM_TYPE_MPEG1_AUDIO, STREAM_TYPE_MPEG2_AUDIO, STREAM_TYPE_AAC,
    STREAM_TYPE_MPEG4_AUDIO, STREAM_TYPE_AC3, STREAM_TYPE_DTS, STREAM_TYPE_TRUEHD,
    STREAM_TYPE_DOLBY_DIGITAL_PLUS, STREAM_TYPE_CAPTION, STREAM_TYPE_DATA_CARROUSEL,
};
use libisdb_ts_tables::{PMTTable, SDTTable, PATTable};
use libisdb_descriptor::{
    StreamIdDescriptor, HierarchicalTransmissionDescriptor,
    CaDescriptor, ServiceDescriptor, LogoTransmissionDescriptor,
};
use libisdb_arib_string::{decode_to_string, DecodeFlags};

/// 無効なコンポーネントタグ (LibISDBConsts.hpp:42)
pub const COMPONENT_TAG_INVALID: u8 = 0xFF;
/// 無効なサービス種別 (LibISDBConsts.hpp:144)
pub const SERVICE_TYPE_INVALID: u8 = 0xFF;
/// 無効なロゴ ID (Descriptors.hpp:874)
pub const LOGO_ID_INVALID: u16 = 0xFFFF;

/// ES 情報。AnalyzerFilter.hpp:73 ESInfo。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EsInfo {
    pub pid: u16,
    pub stream_type: u8,
    pub component_tag: u8,
    pub quality_level: u8,
    pub hierarchical_reference_pid: u16,
}

impl Default for EsInfo {
    fn default() -> Self {
        Self {
            pid: PID_INVALID,
            stream_type: STREAM_TYPE_INVALID,
            component_tag: COMPONENT_TAG_INVALID,
            quality_level: 0xFF,
            hierarchical_reference_pid: PID_INVALID,
        }
    }
}

/// ECM 情報。AnalyzerFilter.hpp:83 ECMInfo。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EcmInfo {
    pub ca_system_id: u16,
    pub pid: u16,
}

/// サービス情報。AnalyzerFilter.hpp:88 ServiceInfo。
/// PMT 由来フィールドは build_service_info で、SDT 由来フィールド
/// (running_status/free_ca_mode/provider_name/service_name/service_type/logo_id)は
/// apply_sdt_service_info で設定する。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ServiceInfo {
    pub is_pmt_acquired: bool,
    pub service_id: u16,
    pub pmt_pid: u16,
    pub es_list: Vec<EsInfo>,
    pub video_es_list: Vec<EsInfo>,
    pub audio_es_list: Vec<EsInfo>,
    pub caption_es_list: Vec<EsInfo>,
    pub data_carrousel_es_list: Vec<EsInfo>,
    pub other_es_list: Vec<EsInfo>,
    pub pcr_pid: u16,
    pub ecm_list: Vec<EcmInfo>,
    // SDT 由来 (apply_sdt_service_info で設定)
    pub running_status: u8,
    pub free_ca_mode: bool,
    pub provider_name: String,
    pub service_name: String,
    pub service_type: u8,
    pub logo_id: u16,
}

/// ストリーム種別がビデオか。AnalyzerFilter.cpp:2014。
fn is_video_stream_type(stream_type: u8) -> bool {
    matches!(
        stream_type,
        STREAM_TYPE_MPEG1_VIDEO
            | STREAM_TYPE_MPEG2_VIDEO
            | STREAM_TYPE_MPEG4_VISUAL
            | STREAM_TYPE_H264
            | STREAM_TYPE_H265
    )
}

/// ストリーム種別がオーディオか。AnalyzerFilter.cpp:2022。
fn is_audio_stream_type(stream_type: u8) -> bool {
    matches!(
        stream_type,
        STREAM_TYPE_MPEG1_AUDIO
            | STREAM_TYPE_MPEG2_AUDIO
            | STREAM_TYPE_AAC
            | STREAM_TYPE_MPEG4_AUDIO
            | STREAM_TYPE_AC3
            | STREAM_TYPE_DTS
            | STREAM_TYPE_TRUEHD
            | STREAM_TYPE_DOLBY_DIGITAL_PLUS
    )
}

/// PAT から PMT 未取得状態のサービスリストを構築する。OnPATSection (AnalyzerFilter.cpp:1855)。
///
/// PAT の各プログラム(program_number != 0、NIT を除く)について、service_id と pmt_pid を
/// 設定した PMT 未取得状態の ServiceInfo を生成する。PMT 受信後に build_service_info で
/// 内容を埋め、SDT 受信後に apply_sdt_service_info で名前等を補完する流れになる。
pub fn build_service_list_from_pat(pat: &PATTable) -> Vec<ServiceInfo> {
    let count = pat.get_program_count();
    let mut list = Vec::with_capacity(count);
    for i in 0..count {
        list.push(ServiceInfo {
            is_pmt_acquired: false,
            service_id: pat.get_program_number(i),
            pmt_pid: pat.get_pmt_pid(i),
            pcr_pid: PID_INVALID,
            running_status: 0xFF,
            free_ca_mode: false,
            service_type: SERVICE_TYPE_INVALID,
            logo_id: LOGO_ID_INVALID,
            ..Default::default()
        });
    }
    list
}

/// パース済み PMTTable から ServiceInfo を構築する。OnPMTSection (AnalyzerFilter.cpp:1967)。
///
/// `service_id` / `pmt_pid` は呼び出し側(PAT 由来)が指定する。
/// PMT の program_number と service_id が食い違う場合でも PMT の内容で構築する
/// (原実装は PAT の program_number でサービスを引くため、ここでは引数を採用)。
pub fn build_service_info(pmt: &PMTTable, service_id: u16, pmt_pid: u16) -> ServiceInfo {
    let mut info = ServiceInfo {
        is_pmt_acquired: false,
        service_id,
        pmt_pid,
        pcr_pid: PID_INVALID,
        ..Default::default()
    };

    // ES を種別ごとに振り分け (AnalyzerFilter.cpp:1991)
    for item in pmt.get_es_list() {
        let mut es = EsInfo {
            pid: item.es_pid,
            stream_type: item.stream_type,
            ..Default::default()
        };

        // component_tag(StreamIdDescriptor) を取得 (AnalyzerFilter.cpp:1999)
        for desc in item.descriptors.iter() {
            if let Some(sid) = StreamIdDescriptor::from_descriptor(desc) {
                es.component_tag = sid.component_tag;
            }
        }

        // 階層伝送情報(HierarchicalTransmissionDescriptor 0xC0) を取得 (AnalyzerFilter.cpp:2003)
        for desc in item.descriptors.iter() {
            if let Some(htd) = HierarchicalTransmissionDescriptor::from_descriptor(desc) {
                es.quality_level = htd.quality_level;
                es.hierarchical_reference_pid = htd.reference_pid;
            }
        }

        info.es_list.push(es);

        if is_video_stream_type(es.stream_type) {
            info.video_es_list.push(es);
        } else if is_audio_stream_type(es.stream_type) {
            info.audio_es_list.push(es);
        } else if es.stream_type == STREAM_TYPE_CAPTION {
            info.caption_es_list.push(es);
        } else if es.stream_type == STREAM_TYPE_DATA_CARROUSEL {
            info.data_carrousel_es_list.push(es);
        } else {
            info.other_es_list.push(es);
        }
    }

    // component_tag 順に安定ソート (AnalyzerFilter.cpp:2052 InsertionSort)
    // Rust の sort_by_key は安定ソートなので InsertionSort と同じ順序になる。
    info.video_es_list.sort_by_key(|e| e.component_tag);
    info.audio_es_list.sort_by_key(|e| e.component_tag);
    info.caption_es_list.sort_by_key(|e| e.component_tag);
    info.data_carrousel_es_list.sort_by_key(|e| e.component_tag);

    // PCR PID (AnalyzerFilter.cpp:2057)
    let pcr_pid = pmt.get_pcr_pid();
    if pcr_pid < 0x1FFF {
        info.pcr_pid = pcr_pid;
    }

    // ECM (AnalyzerFilter.cpp:2064): PMT 記述子ブロックの全 CADescriptor を列挙
    for desc in pmt.get_descriptor_block().iter() {
        if let Some(ca) = CaDescriptor::from_descriptor(desc) {
            info.ecm_list.push(EcmInfo {
                ca_system_id: ca.ca_system_id,
                pid: ca.ca_pid,
            });
        }
    }

    info.is_pmt_acquired = true;
    info
}

/// SDT のサービス情報を ServiceInfo に適用する。GetSDTServiceInfo (AnalyzerFilter.cpp:1932)。
///
/// `sdt` 内で `service_id` に一致するサービスがあれば、その running_status /
/// free_ca_mode / provider_name / service_name / service_type / logo_id を設定する。
/// 一致しない場合は何もせず false を返す。provider_name / service_name は
/// ServiceDescriptor の ARIB 文字列を decode_to_string でデコードする。
pub fn apply_sdt_service_info(info: &mut ServiceInfo, sdt: &SDTTable, service_id: u16) -> bool {
    let index = match sdt.get_service_index_by_id(service_id) {
        Some(i) => i,
        None => return false,
    };
    let item = match sdt.get_service(index) {
        Some(s) => s,
        None => return false,
    };

    info.running_status = item.running_status;
    info.free_ca_mode = item.free_ca_mode;
    info.provider_name.clear();
    info.service_name.clear();
    info.service_type = SERVICE_TYPE_INVALID;
    info.logo_id = LOGO_ID_INVALID;

    for desc in item.descriptors.iter() {
        if let Some(sd) = ServiceDescriptor::from_descriptor(desc) {
            // ARIB 文字列を UTF-8 にデコード (AnalyzerFilter.cpp:1952)
            if !sd.provider_name.is_empty() {
                if let Some(s) = decode_to_string(&sd.provider_name, DecodeFlags::default()) {
                    info.provider_name = s;
                }
            }
            if !sd.service_name.is_empty() {
                if let Some(s) = decode_to_string(&sd.service_name, DecodeFlags::default()) {
                    info.service_name = s;
                }
            }
            info.service_type = sd.service_type;
        }
        if let Some(ld) = LogoTransmissionDescriptor::from_descriptor(desc) {
            info.logo_id = ld.logo_id;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use libisdb_ts_tables::PMTTable;
    use libisdb_ts_packet::{TsPacket, TS_PACKET_SIZE};

    // ── PMT セクションを TS パケットに載せるテストヘルパ ──

    /// PMT セクションを構築する(CRC 付き)。
    /// es: (stream_type, pid, component_tag option) のリスト。
    /// pmt_descriptors: PMT 第1ループの記述子の生バイト列。
    fn build_pmt_section(
        program_number: u16,
        pcr_pid: u16,
        pmt_descriptors: &[u8],
        es: &[(u8, u16, Option<u8>)],
    ) -> Vec<u8> {
        let mut body = Vec::new();
        // PCR_PID
        body.extend_from_slice(&((0xE000 | (pcr_pid & 0x1FFF)).to_be_bytes()));
        // program_info_length + descriptors
        body.extend_from_slice(&((0xF000 | (pmt_descriptors.len() as u16 & 0x0FFF)).to_be_bytes()));
        body.extend_from_slice(pmt_descriptors);
        // ES loop
        for &(stream_type, pid, ctag) in es {
            body.push(stream_type);
            body.extend_from_slice(&((0xE000 | (pid & 0x1FFF)).to_be_bytes()));
            // ES_info: StreamIdDescriptor(0x52, len=1, component_tag) を付与
            let es_desc: Vec<u8> = match ctag {
                Some(tag) => vec![0x52, 0x01, tag],
                None => vec![],
            };
            body.extend_from_slice(&((0xF000 | (es_desc.len() as u16 & 0x0FFF)).to_be_bytes()));
            body.extend_from_slice(&es_desc);
        }

        // section header
        let table_id = 0x02u8;
        // section_length = 後続(table_id_ext..CRC) のバイト数
        // = 2(program_number) +1(version/cni)+1(sec_no)+1(last_sec_no) + body + 4(CRC)
        let section_length = 5 + body.len() + 4;
        let mut sec = Vec::new();
        sec.push(table_id);
        sec.push(0xB0 | ((section_length >> 8) as u8)); // section_syntax_indicator=1
        sec.push((section_length & 0xFF) as u8);
        sec.extend_from_slice(&program_number.to_be_bytes());
        sec.push(0xC1); // version=0, current_next=1
        sec.push(0x00); // section_number
        sec.push(0x00); // last_section_number
        sec.extend_from_slice(&body);
        let crc = libisdb_crc_calc(&sec);
        sec.extend_from_slice(&crc.to_be_bytes());
        sec
    }

    /// ES ごとに生記述子バイト列を渡せる PMT セクション構築ヘルパ。
    /// es: (stream_type, pid, es_descriptors_raw)。
    fn build_pmt_section_raw_es(
        program_number: u16,
        pcr_pid: u16,
        es: &[(u8, u16, Vec<u8>)],
    ) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&((0xE000 | (pcr_pid & 0x1FFF)).to_be_bytes()));
        body.extend_from_slice(&0xF000u16.to_be_bytes()); // program_info_length=0
        for (stream_type, pid, es_desc) in es {
            body.push(*stream_type);
            body.extend_from_slice(&((0xE000 | (pid & 0x1FFF)).to_be_bytes()));
            body.extend_from_slice(&((0xF000 | (es_desc.len() as u16 & 0x0FFF)).to_be_bytes()));
            body.extend_from_slice(es_desc);
        }
        let table_id = 0x02u8;
        let section_length = 5 + body.len() + 4;
        let mut sec = Vec::new();
        sec.push(table_id);
        sec.push(0xB0 | ((section_length >> 8) as u8));
        sec.push((section_length & 0xFF) as u8);
        sec.extend_from_slice(&program_number.to_be_bytes());
        sec.push(0xC1);
        sec.push(0x00);
        sec.push(0x00);
        sec.extend_from_slice(&body);
        let crc = libisdb_crc_calc(&sec);
        sec.extend_from_slice(&crc.to_be_bytes());
        sec
    }

    // テスト内では CRC を libisdb_ts_tables 経由で検証するため、CRC32-MPEG2 を直接計算する。
    fn libisdb_crc_calc(data: &[u8]) -> u32 {
        // CRC32-MPEG2 (init=0xFFFFFFFF, no final xor) — Tables.cpp と同じ
        const POLY: u32 = 0x04C1_1DB7;
        let mut crc: u32 = 0xFFFF_FFFF;
        for &b in data {
            crc ^= (b as u32) << 24;
            for _ in 0..8 {
                if crc & 0x8000_0000 != 0 {
                    crc = (crc << 1) ^ POLY;
                } else {
                    crc <<= 1;
                }
            }
        }
        crc
    }

    /// PMT セクションを 1 つの TS パケットに載せて PMTTable に store する。
    fn make_pmt_table(pmt_pid: u16, section: &[u8]) -> PMTTable {
        let mut data = [0xFFu8; TS_PACKET_SIZE];
        data[0] = 0x47;
        data[1] = 0x40 | ((pmt_pid >> 8) as u8 & 0x1F); // payload_unit_start + PID high
        data[2] = (pmt_pid & 0xFF) as u8;
        data[3] = 0x10; // adaptation=01, CC=0
        data[4] = 0x00; // pointer_field
        data[5..5 + section.len()].copy_from_slice(section);

        let mut pkt = TsPacket::new(&data);
        pkt.parse_packet(None);

        let mut table = PMTTable::new();
        table.store_packet(&pkt);
        table
    }

    /// SDT セクションを構築する(サービス1件、CRC 付き)。
    /// service_descriptors: そのサービスの記述子の生バイト列。
    fn build_sdt_section(
        transport_stream_id: u16,
        original_network_id: u16,
        service_id: u16,
        running_status: u8,
        free_ca_mode: bool,
        service_descriptors: &[u8],
    ) -> Vec<u8> {
        let mut body = Vec::new();
        // original_network_id + reserved_future_use(1 byte)
        body.extend_from_slice(&original_network_id.to_be_bytes());
        body.push(0xFF); // reserved_future_use

        // service loop (1 件)
        body.extend_from_slice(&service_id.to_be_bytes());
        body.push(0x00); // reserved + EIT flags(全0)
        // running_status(3bit) + free_CA_mode(1bit) + descriptors_loop_length(12bit)
        let dll = service_descriptors.len();
        let b = ((running_status & 0x07) << 5)
            | (if free_ca_mode { 0x10 } else { 0x00 })
            | ((dll >> 8) as u8 & 0x0F);
        body.push(b);
        body.push((dll & 0xFF) as u8);
        body.extend_from_slice(service_descriptors);

        // section header (table_id = 0x42 actual)
        let table_id = 0x42u8;
        let section_length = 5 + body.len() + 4;
        let mut sec = Vec::new();
        sec.push(table_id);
        sec.push(0xB0 | ((section_length >> 8) as u8)); // section_syntax_indicator=1, reserved
        sec.push((section_length & 0xFF) as u8);
        sec.extend_from_slice(&transport_stream_id.to_be_bytes());
        sec.push(0xC1); // version=0, current_next=1
        sec.push(0x00); // section_number
        sec.push(0x00); // last_section_number
        sec.extend_from_slice(&body);
        let crc = libisdb_crc_calc(&sec);
        sec.extend_from_slice(&crc.to_be_bytes());
        sec
    }

    fn make_sdt_table(section: &[u8]) -> SDTTable {
        // SDT PID = 0x0011
        let pid = 0x0011u16;
        let mut data = [0xFFu8; TS_PACKET_SIZE];
        data[0] = 0x47;
        data[1] = 0x40 | ((pid >> 8) as u8 & 0x1F);
        data[2] = (pid & 0xFF) as u8;
        data[3] = 0x10;
        data[4] = 0x00;
        data[5..5 + section.len()].copy_from_slice(section);

        let mut pkt = TsPacket::new(&data);
        pkt.parse_packet(None);

        let mut table = SDTTable::new(SDTTable::TABLE_ID_ACTUAL);
        table.store_packet(&pkt);
        table
    }

    /// PAT セクションを構築する(CRC 付き)。
    /// programs: (program_number, pid) のリスト。program_number=0 は NIT。
    fn build_pat_section(transport_stream_id: u16, programs: &[(u16, u16)]) -> Vec<u8> {
        let mut body = Vec::new();
        for &(prog, pid) in programs {
            body.extend_from_slice(&prog.to_be_bytes());
            body.extend_from_slice(&((0xE000 | (pid & 0x1FFF)).to_be_bytes()));
        }

        let table_id = 0x00u8;
        let section_length = 5 + body.len() + 4;
        let mut sec = Vec::new();
        sec.push(table_id);
        sec.push(0xB0 | ((section_length >> 8) as u8));
        sec.push((section_length & 0xFF) as u8);
        sec.extend_from_slice(&transport_stream_id.to_be_bytes());
        sec.push(0xC1); // version=0, current_next=1
        sec.push(0x00); // section_number
        sec.push(0x00); // last_section_number
        sec.extend_from_slice(&body);
        let crc = libisdb_crc_calc(&sec);
        sec.extend_from_slice(&crc.to_be_bytes());
        sec
    }

    fn make_pat_table(section: &[u8]) -> libisdb_ts_tables::PATTable {
        let pid = 0x0000u16; // PAT PID
        let mut data = [0xFFu8; TS_PACKET_SIZE];
        data[0] = 0x47;
        data[1] = 0x40 | ((pid >> 8) as u8 & 0x1F);
        data[2] = (pid & 0xFF) as u8;
        data[3] = 0x10;
        data[4] = 0x00;
        data[5..5 + section.len()].copy_from_slice(section);

        let mut pkt = TsPacket::new(&data);
        pkt.parse_packet(None);

        let mut table = libisdb_ts_tables::PATTable::new();
        table.store_packet(&pkt);
        table
    }

    #[test]
    fn test_build_service_info_categorizes_es() {
        // H264映像 + AAC音声 + 字幕 + データ放送 + 不明
        let es = [
            (STREAM_TYPE_H264, 0x0100, Some(0x00)),
            (STREAM_TYPE_AAC, 0x0110, Some(0x10)),
            (STREAM_TYPE_CAPTION, 0x0130, Some(0x30)),
            (STREAM_TYPE_DATA_CARROUSEL, 0x0140, Some(0x40)),
            (0x05, 0x0150, None), // 不明(private section)
        ];
        let section = build_pmt_section(0x0400, 0x0100, &[], &es);
        let table = make_pmt_table(0x1FC8, &section);
        assert_eq!(table.get_es_count(), 5);

        let info = build_service_info(&table, 0x0400, 0x1FC8);
        assert!(info.is_pmt_acquired);
        assert_eq!(info.service_id, 0x0400);
        assert_eq!(info.pmt_pid, 0x1FC8);
        assert_eq!(info.pcr_pid, 0x0100);
        assert_eq!(info.es_list.len(), 5);
        assert_eq!(info.video_es_list.len(), 1);
        assert_eq!(info.audio_es_list.len(), 1);
        assert_eq!(info.caption_es_list.len(), 1);
        assert_eq!(info.data_carrousel_es_list.len(), 1);
        assert_eq!(info.other_es_list.len(), 1);
        assert_eq!(info.video_es_list[0].pid, 0x0100);
        assert_eq!(info.video_es_list[0].component_tag, 0x00);
        assert_eq!(info.audio_es_list[0].stream_type, STREAM_TYPE_AAC);
    }

    #[test]
    fn test_build_service_info_sorts_by_component_tag() {
        // 同種別(音声)で component_tag が逆順 → ソート後は昇順
        let es = [
            (STREAM_TYPE_AAC, 0x0112, Some(0x12)),
            (STREAM_TYPE_AAC, 0x0110, Some(0x10)),
            (STREAM_TYPE_AAC, 0x0111, Some(0x11)),
        ];
        let section = build_pmt_section(0x0400, 0x0100, &[], &es);
        let table = make_pmt_table(0x1FC8, &section);
        let info = build_service_info(&table, 0x0400, 0x1FC8);
        assert_eq!(info.audio_es_list.len(), 3);
        assert_eq!(info.audio_es_list[0].component_tag, 0x10);
        assert_eq!(info.audio_es_list[1].component_tag, 0x11);
        assert_eq!(info.audio_es_list[2].component_tag, 0x12);
        // es_list は PMT の出現順を保持
        assert_eq!(info.es_list[0].component_tag, 0x12);
    }

    #[test]
    fn test_build_service_info_no_component_tag() {
        // component_tag が無い ES は COMPONENT_TAG_INVALID
        let es = [(STREAM_TYPE_H264, 0x0100, None)];
        let section = build_pmt_section(0x0400, 0x0100, &[], &es);
        let table = make_pmt_table(0x1FC8, &section);
        let info = build_service_info(&table, 0x0400, 0x1FC8);
        assert_eq!(info.video_es_list[0].component_tag, COMPONENT_TAG_INVALID);
    }

    #[test]
    fn test_build_service_info_hierarchical_transmission() {
        // ES に StreamIdDescriptor(0x52) と HierarchicalTransmissionDescriptor(0xC0) を付与。
        // 0xC0: quality_level=1, reference_PID=0x0123
        let es_desc = vec![
            0x52, 0x01, 0x05,             // StreamIdDescriptor component_tag=0x05
            0xC0, 0x03, 0x01, 0xE1, 0x23, // Hierarchical: quality=1, ref_pid=0x0123
        ];
        let section = build_pmt_section_raw_es(
            0x0400, 0x0100, &[(STREAM_TYPE_H264, 0x0100, es_desc)],
        );
        let table = make_pmt_table(0x1FC8, &section);
        let info = build_service_info(&table, 0x0400, 0x1FC8);
        assert_eq!(info.es_list.len(), 1);
        let es = &info.es_list[0];
        assert_eq!(es.component_tag, 0x05);
        assert_eq!(es.quality_level, 1);
        assert_eq!(es.hierarchical_reference_pid, 0x0123);
    }

    #[test]
    fn test_build_service_info_no_hierarchical_defaults() {
        // 階層伝送記述子が無ければ quality_level/hierarchical_reference_pid は既定値
        let es = [(STREAM_TYPE_H264, 0x0100, None)];
        let section = build_pmt_section(0x0400, 0x0100, &[], &es);
        let table = make_pmt_table(0x1FC8, &section);
        let info = build_service_info(&table, 0x0400, 0x1FC8);
        assert_eq!(info.es_list[0].quality_level, 0xFF);
        assert_eq!(info.es_list[0].hierarchical_reference_pid, PID_INVALID);
    }

    #[test]
    fn test_build_service_info_ecm() {
        // PMT 第1ループに CADescriptor(0x09): CASystemID=0x0005, CAPID=0x0901
        let ca_desc = [
            0x09u8, 0x04, // tag, length
            0x00, 0x05, // CA_system_id
            0xE9, 0x01, // CA_PID (0xE000 | 0x0901)
        ];
        let es = [(STREAM_TYPE_H264, 0x0100, Some(0x00))];
        let section = build_pmt_section(0x0400, 0x0100, &ca_desc, &es);
        let table = make_pmt_table(0x1FC8, &section);
        let info = build_service_info(&table, 0x0400, 0x1FC8);
        assert_eq!(info.ecm_list.len(), 1);
        assert_eq!(info.ecm_list[0].ca_system_id, 0x0005);
        assert_eq!(info.ecm_list[0].pid, 0x0901);
    }

    #[test]
    fn test_build_service_info_empty_pmt() {
        // ES が無い PMT
        let section = build_pmt_section(0x0400, 0x0100, &[], &[]);
        let table = make_pmt_table(0x1FC8, &section);
        let info = build_service_info(&table, 0x0400, 0x1FC8);
        assert!(info.is_pmt_acquired);
        assert!(info.es_list.is_empty());
        assert_eq!(info.pcr_pid, 0x0100);
        assert!(info.ecm_list.is_empty());
    }

    #[test]
    fn test_is_video_audio_stream_type() {
        assert!(is_video_stream_type(STREAM_TYPE_H264));
        assert!(is_video_stream_type(STREAM_TYPE_MPEG2_VIDEO));
        assert!(!is_video_stream_type(STREAM_TYPE_AAC));
        assert!(is_audio_stream_type(STREAM_TYPE_AAC));
        assert!(is_audio_stream_type(STREAM_TYPE_AC3));
        assert!(!is_audio_stream_type(STREAM_TYPE_H264));
    }

    // ─── apply_sdt_service_info ──────────────────────────────────

    #[test]
    fn test_apply_sdt_service_info() {
        // ServiceDescriptor(0x48): service_type=0x01,
        //   provider_name="ABC"(ASCII相当のARIB英数字), service_name="XYZ"
        // ARIB の英数字は G0=Kanji 初期のため、ここでは provider/service 名の
        // バイト列をそのまま渡し、decode が何らかの文字列を返すことを確認する。
        // 簡単のため空でない名前(0x20 スペース)を使う。
        let mut svc_desc = Vec::new();
        // ARIB 初期状態は G0=Kanji(2バイト)。確実にデコードできるよう
        // ESC で G0=Alphanumeric に切り替えてから ASCII 1 文字を置く。
        let provider: &[u8] = &[0x1B, 0x28, 0x4A, 0x41]; // ESC G0=Alphanumeric, 'A'→'Ａ'
        let service: &[u8] = &[0x1B, 0x28, 0x4A, 0x42]; // 'B'→'Ｂ'
        svc_desc.push(0x48); // tag
        svc_desc.push((3 + provider.len() + service.len()) as u8); // length
        svc_desc.push(0x01); // service_type
        svc_desc.push(provider.len() as u8);
        svc_desc.extend_from_slice(provider);
        svc_desc.push(service.len() as u8);
        svc_desc.extend_from_slice(service);

        let section = build_sdt_section(0x7FE0, 0x0004, 0x0400, 4, false, &svc_desc);
        let sdt = make_sdt_table(&section);
        assert_eq!(sdt.get_service_count(), 1);

        let mut info = ServiceInfo {
            service_id: 0x0400,
            ..Default::default()
        };
        assert!(apply_sdt_service_info(&mut info, &sdt, 0x0400));
        assert_eq!(info.running_status, 4);
        assert!(!info.free_ca_mode);
        assert_eq!(info.service_type, 0x01);
        // 名前は何らかのデコード結果(空でない)
        assert!(!info.provider_name.is_empty());
        assert!(!info.service_name.is_empty());
    }

    #[test]
    fn test_apply_sdt_service_info_logo_id() {
        // LogoTransmissionDescriptor(0xCF): type=1, logo_id=5
        let mut svc_desc = Vec::new();
        // ServiceDescriptor は無し、Logo のみ
        svc_desc.push(0xCF); // tag
        svc_desc.push(0x07); // length
        svc_desc.push(0x01); // logo_transmission_type
        svc_desc.extend_from_slice(&[0x00, 0x05]); // logo_id(下位9bit)=5
        svc_desc.extend_from_slice(&[0x00, 0x01]); // logo_version
        svc_desc.extend_from_slice(&[0x01, 0x00]); // download_data_id

        let section = build_sdt_section(0x7FE0, 0x0004, 0x0400, 4, true, &svc_desc);
        let sdt = make_sdt_table(&section);

        let mut info = ServiceInfo {
            service_id: 0x0400,
            ..Default::default()
        };
        assert!(apply_sdt_service_info(&mut info, &sdt, 0x0400));
        assert!(info.free_ca_mode);
        assert_eq!(info.logo_id, 5);
        // ServiceDescriptor が無いので service_type は INVALID
        assert_eq!(info.service_type, SERVICE_TYPE_INVALID);
    }

    #[test]
    fn test_apply_sdt_service_info_not_found() {
        let section = build_sdt_section(0x7FE0, 0x0004, 0x0400, 4, false, &[]);
        let sdt = make_sdt_table(&section);
        let mut info = ServiceInfo {
            service_id: 0x9999,
            ..Default::default()
        };
        // 存在しないサービス ID
        assert!(!apply_sdt_service_info(&mut info, &sdt, 0x9999));
    }

    // ─── build_service_list_from_pat ─────────────────────────────

    #[test]
    fn test_build_service_list_from_pat() {
        // NIT(program 0) + 2 サービス
        let section = build_pat_section(
            0x7FE0,
            &[(0x0000, 0x0010), (0x0400, 0x1FC8), (0x0401, 0x1FC9)],
        );
        let pat = make_pat_table(&section);
        // PATTable は program_number=0(NIT)を除外するので 2 件
        assert_eq!(pat.get_program_count(), 2);

        let list = build_service_list_from_pat(&pat);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].service_id, 0x0400);
        assert_eq!(list[0].pmt_pid, 0x1FC8);
        assert!(!list[0].is_pmt_acquired);
        assert_eq!(list[0].service_type, SERVICE_TYPE_INVALID);
        assert_eq!(list[0].logo_id, LOGO_ID_INVALID);
        assert_eq!(list[0].pcr_pid, PID_INVALID);
        assert_eq!(list[1].service_id, 0x0401);
        assert_eq!(list[1].pmt_pid, 0x1FC9);
    }

    #[test]
    fn test_build_service_list_from_pat_empty() {
        // NIT のみ
        let section = build_pat_section(0x7FE0, &[(0x0000, 0x0010)]);
        let pat = make_pat_table(&section);
        let list = build_service_list_from_pat(&pat);
        assert!(list.is_empty());
    }

    #[test]
    fn test_full_pat_pmt_sdt_pipeline() {
        // 実際の解析パイプライン: PAT → PMT → SDT
        // 1. PAT から service_id / pmt_pid を取得
        let pat_section = build_pat_section(0x7FE0, &[(0x0000, 0x0010), (0x0400, 0x1FC8)]);
        let pat = make_pat_table(&pat_section);
        let mut list = build_service_list_from_pat(&pat);
        assert_eq!(list.len(), 1);
        let service_id = list[0].service_id;
        let pmt_pid = list[0].pmt_pid;
        assert_eq!(service_id, 0x0400);

        // 2. PMT を受信して ES を埋める
        let es = [
            (STREAM_TYPE_H264, 0x0100, Some(0x00)),
            (STREAM_TYPE_AAC, 0x0110, Some(0x10)),
            (STREAM_TYPE_CAPTION, 0x0130, Some(0x30)),
        ];
        let pmt_section = build_pmt_section(service_id, 0x0100, &[], &es);
        let pmt = make_pmt_table(pmt_pid, &pmt_section);
        list[0] = build_service_info(&pmt, service_id, pmt_pid);
        assert!(list[0].is_pmt_acquired);
        assert_eq!(list[0].video_es_list.len(), 1);
        assert_eq!(list[0].audio_es_list.len(), 1);
        assert_eq!(list[0].caption_es_list.len(), 1);
        assert_eq!(list[0].pcr_pid, 0x0100);

        // 3. SDT を受信してサービス名等を補完
        let provider: &[u8] = &[0x1B, 0x28, 0x4A, 0x41];
        let mut svc_desc = Vec::new();
        svc_desc.push(0x48);
        svc_desc.push((3 + provider.len()) as u8);
        svc_desc.push(0x01); // service_type
        svc_desc.push(provider.len() as u8);
        svc_desc.extend_from_slice(provider);
        svc_desc.push(0x00); // service name length 0
        let sdt_section = build_sdt_section(0x7FE0, 0x0004, service_id, 4, false, &svc_desc);
        let sdt = make_sdt_table(&sdt_section);
        assert!(apply_sdt_service_info(&mut list[0], &sdt, service_id));

        // 全ステージの情報が揃う
        assert_eq!(list[0].service_id, 0x0400);
        assert_eq!(list[0].service_type, 0x01);
        assert_eq!(list[0].running_status, 4);
        assert_eq!(list[0].video_es_list[0].pid, 0x0100);
        assert_eq!(list[0].caption_es_list[0].pid, 0x0130);
    }

    #[test]
    fn test_pmt_then_sdt_integration() {
        // PMT で ServiceInfo を構築 → SDT でサービス名等を補完する統合シナリオ
        let es = [
            (STREAM_TYPE_H264, 0x0100, Some(0x00)),
            (STREAM_TYPE_AAC, 0x0110, Some(0x10)),
        ];
        let pmt_section = build_pmt_section(0x0400, 0x0100, &[], &es);
        let pmt = make_pmt_table(0x1FC8, &pmt_section);
        let mut info = build_service_info(&pmt, 0x0400, 0x1FC8);
        assert!(info.is_pmt_acquired);
        assert_eq!(info.video_es_list.len(), 1);

        // ServiceDescriptor: service_type=0x01, provider="A"(escape+ASCII), service名=空
        let provider: &[u8] = &[0x1B, 0x28, 0x4A, 0x41];
        let mut svc_desc = Vec::new();
        svc_desc.push(0x48); // tag
        svc_desc.push((3 + provider.len()) as u8); // length
        svc_desc.push(0x01); // service_type
        svc_desc.push(provider.len() as u8);
        svc_desc.extend_from_slice(provider);
        svc_desc.push(0x00); // service name length 0
        let sdt_section = build_sdt_section(0x7FE0, 0x0004, 0x0400, 4, false, &svc_desc);
        let sdt = make_sdt_table(&sdt_section);
        assert!(apply_sdt_service_info(&mut info, &sdt, 0x0400));

        // PMT 由来 + SDT 由来の両方が揃う
        assert_eq!(info.service_type, 0x01);
        assert_eq!(info.video_es_list[0].pid, 0x0100);
        assert_eq!(info.running_status, 4);
    }
}
