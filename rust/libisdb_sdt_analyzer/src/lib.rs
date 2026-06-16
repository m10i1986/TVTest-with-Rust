// Rust port of LibISDB/Filters/AnalyzerFilter.cpp の SDT→サービス情報構築ロジック。
// AnalyzerFilter.cpp:UpdateSDTServiceList(2104) / UpdateSDTStreamMap(2142)
// + AnalyzerFilter.hpp:SDTServiceInfo(110) / SDTStreamInfo(122)
//
// SDT(Service Description Table)をパースして、TS 配下の各サービスの情報
// (running_status・free_ca_mode・事業者名/サービス名・サービスタイプ・ロゴID)を
// 構築する純粋ロジックを切り出す。入力はパース済みの libisdb_ts_tables::SDTTable、
// 出力は SDTServiceInfo のリスト / SDTStreamInfo。
// PID マップ・SDTTableSet 管理・フィルタグラフ管理は対象外。
//
// 移植対象:
//   - 各サービスの service_id / running_status / free_ca_mode
//   - ServiceDescriptor(0x48)           → provider_name / service_name / service_type
//   - LogoTransmissionDescriptor(0xCF)  → logo_id
//
// 文字列は libisdb_arib_string::decode_to_string で ARIB デコードする。

use libisdb_ts_tables::SDTTable;
use libisdb_descriptor::{ServiceDescriptor, LogoTransmissionDescriptor};
use libisdb_arib_string::{decode_to_string, DecodeFlags};

/// 無効なサービスタイプ。LibISDBConsts.hpp:144。
pub const SERVICE_TYPE_INVALID: u8 = 0xFF;
/// 無効なロゴ ID。LogoTransmissionDescriptor::LOGO_ID_INVALID。
pub const LOGO_ID_INVALID: u16 = 0xFFFF;

/// ARIB 文字列(生バイト列)を UTF-8 にデコードする。空や失敗時は空文字列。
fn decode_aribstr(src: &[u8]) -> String {
    if src.is_empty() {
        return String::new();
    }
    decode_to_string(src, DecodeFlags::default()).unwrap_or_default()
}

/// SDT のサービス情報。AnalyzerFilter.hpp:SDTServiceInfo 相当。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SDTServiceInfo {
    pub service_id: u16,
    pub running_status: u8,
    pub free_ca_mode: bool,
    pub provider_name: String,
    pub service_name: String,
    pub service_type: u8,
    pub logo_id: u16,
}

/// SDT の TS 情報。AnalyzerFilter.hpp:SDTStreamInfo 相当。
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct SDTStreamInfo {
    pub transport_stream_id: u16,
    pub original_network_id: u16,
    pub service_list: Vec<SDTServiceInfo>,
}

/// SDTTable からサービス情報リストを構築する。UpdateSDTServiceList 相当。
pub fn build_sdt_service_list(sdt: &SDTTable) -> Vec<SDTServiceInfo> {
    let mut list = Vec::with_capacity(sdt.get_service_count());

    for i in 0..sdt.get_service_count() {
        let item = match sdt.get_service(i) {
            Some(s) => s,
            None => continue,
        };

        let mut service = SDTServiceInfo {
            service_id: item.service_id,
            running_status: item.running_status,
            free_ca_mode: item.free_ca_mode,
            provider_name: String::new(),
            service_name: String::new(),
            service_type: SERVICE_TYPE_INVALID,
            logo_id: LOGO_ID_INVALID,
        };

        // ServiceDescriptor(0x48): 事業者名 / サービス名 / サービスタイプ
        for desc in item.descriptors.iter() {
            if let Some(sd) = ServiceDescriptor::from_descriptor(desc) {
                service.provider_name = decode_aribstr(&sd.provider_name);
                service.service_name = decode_aribstr(&sd.service_name);
                service.service_type = sd.service_type;
                break;
            }
        }

        // LogoTransmissionDescriptor(0xCF): ロゴ ID
        for desc in item.descriptors.iter() {
            if let Some(ld) = LogoTransmissionDescriptor::from_descriptor(desc) {
                service.logo_id = ld.logo_id;
                break;
            }
        }

        list.push(service);
    }

    list
}

/// SDTTable から TS 単位のストリーム情報を構築する。UpdateSDTStreamMap 相当
/// (1 SDTTable → 1 SDTStreamInfo)。
pub fn build_sdt_stream_info(sdt: &SDTTable) -> SDTStreamInfo {
    SDTStreamInfo {
        transport_stream_id: sdt.get_transport_stream_id(),
        original_network_id: sdt.get_original_network_id(),
        service_list: build_sdt_service_list(sdt),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libisdb_ts_packet::{TsPacket, TS_PACKET_SIZE};

    const PID_SDT: u16 = 0x0011;

    fn crc32_mpeg2_calc(data: &[u8]) -> u32 {
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

    /// ディスクリプタ(tag,payload)を tag/len/payload バイト列に変換。
    fn make_desc(tag: u8, payload: &[u8]) -> Vec<u8> {
        let mut v = vec![tag, payload.len() as u8];
        v.extend_from_slice(payload);
        v
    }

    /// ServiceDescriptor(0x48) を組み立てる。provider/service は ARIB 生バイト。
    fn build_service_desc(service_type: u8, provider: &[u8], service: &[u8]) -> Vec<u8> {
        let mut p = Vec::new();
        p.push(service_type);
        p.push(provider.len() as u8);
        p.extend_from_slice(provider);
        p.push(service.len() as u8);
        p.extend_from_slice(service);
        make_desc(0x48, &p)
    }

    /// LogoTransmissionDescriptor(0xCF) type=1 を組み立てる。logo_id を格納。
    fn build_logo_desc(logo_id: u16) -> Vec<u8> {
        // type(1) + logo_id(2,上位7bit予約) + logo_version(2) + download_data_id(2)
        let mut p = Vec::new();
        p.push(0x01); // logo_transmission_type=1
        p.extend_from_slice(&(logo_id & 0x01FF).to_be_bytes());
        p.extend_from_slice(&0x0001u16.to_be_bytes()); // logo_version
        p.extend_from_slice(&0x0100u16.to_be_bytes()); // download_data_id
        make_desc(0xCF, &p)
    }

    /// SDT セクションを 1 TS 分組み立てて SDTTable に store する。
    /// services: (service_id, running_status, free_ca_mode, descriptor block bytes)
    fn parse_sdt(
        transport_stream_id: u16,
        original_network_id: u16,
        services: &[(u16, u8, bool, Vec<u8>)],
    ) -> SDTTable {
        // payload data 部 (get_payload_data が返す領域)
        // [0..2]=original_network_id, [2]=reserved
        let mut body = Vec::new();
        body.extend_from_slice(&original_network_id.to_be_bytes());
        body.push(0xFF); // reserved_future_use

        for (sid, rs, fcm, db) in services {
            body.extend_from_slice(&sid.to_be_bytes());
            body.push(0xFC | 0x01); // reserved(3) + EIT flags 等 (適当, present_following=1)
            // running_status(3) + free_ca_mode(1) + descriptors_loop_length(12)
            let dl = db.len();
            let b = ((*rs & 0x07) << 5)
                | (if *fcm { 0x10 } else { 0x00 })
                | (((dl >> 8) as u8) & 0x0F);
            body.push(b);
            body.push((dl & 0xFF) as u8);
            body.extend_from_slice(db);
        }

        // section header (table_id = 0x42 SDT actual, table_id_extension = transport_stream_id)
        let table_id = SDTTable::TABLE_ID_ACTUAL;
        let section_length = 5 + body.len() + 4;
        let mut sec = Vec::new();
        sec.push(table_id);
        sec.push(0xB0 | ((section_length >> 8) as u8));
        sec.push((section_length & 0xFF) as u8);
        sec.extend_from_slice(&transport_stream_id.to_be_bytes()); // table_id_extension
        sec.push(0xC1); // reserved+version+current_next
        sec.push(0x00); // section_number
        sec.push(0x00); // last_section_number
        sec.extend_from_slice(&body);
        let crc = crc32_mpeg2_calc(&sec);
        sec.extend_from_slice(&crc.to_be_bytes());

        // TS パケットに載せる (PID = 0x0011 SDT)
        let mut data = [0xFFu8; TS_PACKET_SIZE];
        data[0] = 0x47;
        data[1] = 0x40 | ((PID_SDT >> 8) as u8 & 0x1F);
        data[2] = (PID_SDT & 0xFF) as u8;
        data[3] = 0x10;
        data[4] = 0x00; // pointer_field
        data[5..5 + sec.len()].copy_from_slice(&sec);

        let mut pkt = TsPacket::new(&data);
        pkt.parse_packet(None);

        let mut sdt = SDTTable::new(SDTTable::TABLE_ID_ACTUAL);
        assert!(sdt.store_packet(&pkt), "SDT store_packet failed");
        sdt
    }

    #[test]
    fn test_build_sdt_service_list_basic() {
        // service_desc: type=0x01, provider="Ａ"(G0英数), service="Ｂ"
        let provider = vec![0x1B, 0x28, 0x4A, 0x41];
        let service = vec![0x1B, 0x28, 0x4A, 0x42];
        let svc_desc = build_service_desc(0x01, &provider, &service);
        let logo_desc = build_logo_desc(0x0005);

        let mut block = Vec::new();
        block.extend_from_slice(&svc_desc);
        block.extend_from_slice(&logo_desc);

        let sdt = parse_sdt(0x0040, 0x0004, &[(0x0064, 0x04, false, block)]);
        let list = build_sdt_service_list(&sdt);

        assert_eq!(list.len(), 1);
        let s = &list[0];
        assert_eq!(s.service_id, 0x0064);
        assert_eq!(s.running_status, 0x04);
        assert!(!s.free_ca_mode);
        assert_eq!(s.provider_name, "Ａ");
        assert_eq!(s.service_name, "Ｂ");
        assert_eq!(s.service_type, 0x01);
        assert_eq!(s.logo_id, 0x0005);
    }

    #[test]
    fn test_build_sdt_service_list_no_descriptors() {
        // ディスクリプタ無し: 既定値(空名/SERVICE_TYPE_INVALID/LOGO_ID_INVALID)
        let sdt = parse_sdt(0x0041, 0x0007, &[(0x00C8, 0x02, true, Vec::new())]);
        let list = build_sdt_service_list(&sdt);

        assert_eq!(list.len(), 1);
        let s = &list[0];
        assert_eq!(s.service_id, 0x00C8);
        assert_eq!(s.running_status, 0x02);
        assert!(s.free_ca_mode);
        assert!(s.provider_name.is_empty());
        assert!(s.service_name.is_empty());
        assert_eq!(s.service_type, SERVICE_TYPE_INVALID);
        assert_eq!(s.logo_id, LOGO_ID_INVALID);
    }

    #[test]
    fn test_build_sdt_service_list_multiple() {
        let svc1 = build_service_desc(0x01, &[0x1B, 0x28, 0x4A, 0x41], &[]);
        let svc2 = build_service_desc(0x02, &[0x1B, 0x28, 0x4A, 0x42], &[]);

        let sdt = parse_sdt(
            0x0050,
            0x0010,
            &[(0x0001, 0x04, false, svc1), (0x0002, 0x04, false, svc2)],
        );
        let list = build_sdt_service_list(&sdt);

        assert_eq!(list.len(), 2);
        assert_eq!(list[0].service_id, 0x0001);
        assert_eq!(list[0].provider_name, "Ａ");
        assert_eq!(list[0].service_type, 0x01);
        assert_eq!(list[1].service_id, 0x0002);
        assert_eq!(list[1].provider_name, "Ｂ");
        assert_eq!(list[1].service_type, 0x02);
    }

    #[test]
    fn test_build_sdt_stream_info() {
        let svc_desc = build_service_desc(0x01, &[], &[0x1B, 0x28, 0x4A, 0x41]);
        let sdt = parse_sdt(0x1234, 0x5678, &[(0x0064, 0x04, false, svc_desc)]);
        let info = build_sdt_stream_info(&sdt);

        assert_eq!(info.transport_stream_id, 0x1234);
        assert_eq!(info.original_network_id, 0x5678);
        assert_eq!(info.service_list.len(), 1);
        assert_eq!(info.service_list[0].service_id, 0x0064);
        assert_eq!(info.service_list[0].service_name, "Ａ");
    }
}
