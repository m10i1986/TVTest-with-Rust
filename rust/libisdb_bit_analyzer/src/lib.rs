// Rust port of LibISDB/Filters/AnalyzerFilter.cpp の BIT→ネットワーク情報構築ロジック。
// AnalyzerFilter.cpp:GetBITList (1758 付近) + AnalyzerFilter.hpp:BITNetworkInfo 他
//
// BIT(Broadcaster Information Table)をパースして、ネットワーク配下の
// ブロードキャスタ情報(名称・サービスリスト・拡張ブロードキャスタ情報)と、
// ネットワーク単位の SI 伝送パラメータを構築する純粋ロジックを切り出す。
// 入力はパース済みの libisdb_ts_tables::BITTable、出力は BITNetworkInfo。
// PID マップ・BITMultiTable の完了判定・フィルタグラフ管理は対象外。
//
// 移植対象:
//   - 第1ループ(ネットワークループ):
//       SIParameterDescriptor(0xD7) → si_parameter_list
//   - 第2ループ(ブロードキャスタループ):
//       broadcaster_id
//       BroadcasterNameDescriptor(0xD8)     → broadcaster_name (ARIB デコード)
//       ServiceListDescriptor(0x41)         → service_list
//       ExtendedBroadcasterDescriptor(0xCE) → broadcaster_type / terrestrial
//
// 文字列は libisdb_arib_string::decode_to_string で ARIB デコードする。

use libisdb_ts_tables::BITTable;
use libisdb_descriptor::{
    BroadcasterNameDescriptor, ExtendedBroadcasterDescriptor, ServiceListDescriptor,
    SIParameterDescriptor, SIParameterTableEntry,
};
use libisdb_arib_string::{decode_to_string, DecodeFlags};

/// ブロードキャスタタイプの既定値。AnalyzerFilter.cpp:1796。
pub const BROADCASTER_TYPE_INVALID: u8 = 0xFF;

/// ARIB 文字列(生バイト列)を UTF-8 にデコードする。空や失敗時は空文字列。
fn decode_aribstr(src: &[u8]) -> String {
    if src.is_empty() {
        return String::new();
    }
    decode_to_string(src, DecodeFlags::default()).unwrap_or_default()
}

/// サービスリストエントリ。ServiceListDescriptor::ServiceInfo 相当。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct BITServiceInfo {
    pub service_id: u16,
    pub service_type: u8,
}

/// 地上デジタルテレビジョン放送ブロードキャスタの情報。
/// ExtendedBroadcasterDescriptor::TerrestrialBroadcasterInfo 相当。
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct TerrestrialBroadcasterInfo {
    pub terrestrial_broadcaster_id: u16,
    pub affiliation_id_list: Vec<u8>,
    pub broadcaster_id_list: Vec<BroadcasterIdEntry>,
}

/// broadcaster_id ループの 1 要素。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct BroadcasterIdEntry {
    pub original_network_id: u16,
    pub broadcaster_id: u8,
}

/// SI 伝送パラメータ情報。AnalyzerFilter.hpp:SIParameterInfo 相当。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SIParameterInfo {
    pub parameter_version: u8,
    pub update_time: libisdb_datetime::DateTime,
    pub table_list: Vec<SIParameterTableEntry>,
}

/// ブロードキャスタ情報。AnalyzerFilter.hpp:BITBroadcasterInfo 相当。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BITBroadcasterInfo {
    pub broadcaster_id: u8,
    /// broadcaster_type (拡張ブロードキャスタ記述子が無ければ 0xFF)
    pub broadcaster_type: u8,
    pub broadcaster_name: String,
    pub service_list: Vec<BITServiceInfo>,
    /// 地上ブロードキャスタ情報(broadcaster_type が地上/地上音声のときのみ Some)
    pub terrestrial: Option<TerrestrialBroadcasterInfo>,
}

/// BIT のネットワーク情報。AnalyzerFilter.hpp:BITNetworkInfo 相当。
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct BITNetworkInfo {
    pub original_network_id: u16,
    pub si_parameter_list: Vec<SIParameterInfo>,
    pub broadcaster_list: Vec<BITBroadcasterInfo>,
}

/// BITTable からネットワーク情報を構築する。GetBITList の 1 セクション分相当。
///
/// 元の GetBITList は BITMultiTable の全セクションを走査して List に push するが、
/// ここではパース済み BITTable 1 つを 1 つの BITNetworkInfo に変換する。
pub fn build_bit_network_info(bit: &BITTable) -> BITNetworkInfo {
    let mut info = BITNetworkInfo {
        original_network_id: bit.get_original_network_id(),
        ..Default::default()
    };

    // 第1ループ(ネットワークループ): SIParameterDescriptor(0xD7)
    for desc in bit.get_descriptor_block().iter() {
        if let Some(sip) = SIParameterDescriptor::from_descriptor(desc) {
            info.si_parameter_list.push(SIParameterInfo {
                parameter_version: sip.parameter_version,
                update_time: sip.update_time,
                table_list: sip.table_list,
            });
        }
    }

    // 第2ループ(ブロードキャスタループ)
    for i in 0..bit.get_broadcaster_count() {
        let bc = match bit.get_broadcaster(i) {
            Some(b) => b,
            None => continue,
        };

        let mut broadcaster = BITBroadcasterInfo {
            broadcaster_id: bc.broadcaster_id,
            broadcaster_type: BROADCASTER_TYPE_INVALID,
            broadcaster_name: String::new(),
            service_list: Vec::new(),
            terrestrial: None,
        };

        // BroadcasterNameDescriptor(0xD8)
        for desc in bc.descriptors.iter() {
            if let Some(bnd) = BroadcasterNameDescriptor::from_descriptor(desc) {
                // GetBroadcasterName は空でないときのみ採用するが、
                // デコード結果が空文字でもそのまま格納される(元実装の Decode と同様)。
                broadcaster.broadcaster_name = decode_aribstr(&bnd.broadcaster_name);
                break;
            }
        }

        // ServiceListDescriptor(0x41)
        for desc in bc.descriptors.iter() {
            if let Some(sld) = ServiceListDescriptor::from_descriptor(desc) {
                for s in &sld.service_list {
                    broadcaster.service_list.push(BITServiceInfo {
                        service_id: s.service_id,
                        service_type: s.service_type,
                    });
                }
                break;
            }
        }

        // ExtendedBroadcasterDescriptor(0xCE)
        for desc in bc.descriptors.iter() {
            if let Some(ebd) = ExtendedBroadcasterDescriptor::from_descriptor(desc) {
                broadcaster.broadcaster_type = ebd.broadcaster_type;
                broadcaster.terrestrial = ebd.terrestrial.map(|t| TerrestrialBroadcasterInfo {
                    terrestrial_broadcaster_id: t.terrestrial_broadcaster_id,
                    affiliation_id_list: t.affiliation_id_list,
                    broadcaster_id_list: t
                        .broadcaster_id_list
                        .into_iter()
                        .map(|e| BroadcasterIdEntry {
                            original_network_id: e.original_network_id,
                            broadcaster_id: e.broadcaster_id,
                        })
                        .collect(),
                });
                break;
            }
        }

        info.broadcaster_list.push(broadcaster);
    }

    info
}

#[cfg(test)]
mod tests {
    use super::*;
    use libisdb_ts_packet::{TsPacket, TS_PACKET_SIZE};

    const PID_BIT: u16 = 0x0024;

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

    /// BIT セクションを 1 TS 分組み立てて BITTable に store する。
    ///
    /// network_descriptors: 第1ループ用の DescriptorBlock 生バイト
    /// broadcasters: (broadcaster_id, descriptor block bytes) のリスト
    fn parse_bit(
        original_network_id: u16,
        network_descriptors: &[u8],
        broadcasters: &[(u8, Vec<u8>)],
    ) -> BITTable {
        // payload data 部 (get_payload_data が返す領域)
        // [0]=bvp(1)+reserved(3)+first_desc_len 上位4bit, [1]=下位8bit
        let mut body = Vec::new();
        let net_desc_len = network_descriptors.len();
        body.push(0x10 | ((net_desc_len >> 8) as u8 & 0x0F)); // bvp=1
        body.push((net_desc_len & 0xFF) as u8);
        body.extend_from_slice(network_descriptors);
        for (bid, db) in broadcasters {
            body.push(*bid);
            let dl = db.len();
            body.push(((dl >> 8) as u8) & 0x0F);
            body.push((dl & 0xFF) as u8);
            body.extend_from_slice(db);
        }

        // section header (table_id = 0xC4 BIT, table_id_extension = original_network_id)
        let section_length = 5 + body.len() + 4;
        let mut sec = Vec::new();
        sec.push(BITTable::TABLE_ID);
        sec.push(0xB0 | ((section_length >> 8) as u8));
        sec.push((section_length & 0xFF) as u8);
        sec.extend_from_slice(&original_network_id.to_be_bytes()); // table_id_extension
        sec.push(0xC1); // reserved+version+current_next
        sec.push(0x00); // section_number
        sec.push(0x00); // last_section_number
        sec.extend_from_slice(&body);
        let crc = crc32_mpeg2_calc(&sec);
        sec.extend_from_slice(&crc.to_be_bytes());

        // TS パケットに載せる (PID = 0x0024 BIT)
        let mut data = [0xFFu8; TS_PACKET_SIZE];
        data[0] = 0x47;
        data[1] = 0x40 | ((PID_BIT >> 8) as u8 & 0x1F);
        data[2] = (PID_BIT & 0xFF) as u8;
        data[3] = 0x10;
        data[4] = 0x00; // pointer_field
        data[5..5 + sec.len()].copy_from_slice(&sec);

        let mut pkt = TsPacket::new(&data);
        pkt.parse_packet(None);

        let mut bit = BITTable::new();
        assert!(bit.store_packet(&pkt), "BIT store_packet failed");
        bit
    }

    #[test]
    fn test_build_bit_network_info_basic() {
        // ブロードキャスタ1件: name(0xD8) + service_list(0x41) + extended(0xCE 地上)
        // name = "Ａ" を得るため G0=英数(ESC 0x1B 0x28 0x4A)後に 'A'
        let name_payload = vec![0x1B, 0x28, 0x4A, 0x41];
        let name_desc = make_desc(0xD8, &name_payload);

        // service_list: service_id=0x0064, service_type=0x01
        let sl_payload = vec![0x00, 0x64, 0x01];
        let sl_desc = make_desc(0x41, &sl_payload);

        // extended broadcaster: type=1(地上), onid=0x1234, aff=1, bc=1
        // payload: 0x10, 0x12,0x34, 0x11, [aff:0x0A], [bc: 0x7E,0x87,0x05]
        let eb_payload = vec![0x10, 0x12, 0x34, 0x11, 0x0A, 0x7E, 0x87, 0x05];
        let eb_desc = make_desc(0xCE, &eb_payload);

        let mut block = Vec::new();
        block.extend_from_slice(&name_desc);
        block.extend_from_slice(&sl_desc);
        block.extend_from_slice(&eb_desc);

        // ネットワークループ: SIParameter(0xD7) NIT cycle
        // payload: version=0x05, update_time MJD=0xC8AB, NIT(0x40) len=1 BCD=0x12
        let si_payload = vec![0x05, 0xC8, 0xAB, 0x40, 0x01, 0x12];
        let si_desc = make_desc(0xD7, &si_payload);

        let bit = parse_bit(0x0004, &si_desc, &[(0x01, block)]);
        let info = build_bit_network_info(&bit);

        assert_eq!(info.original_network_id, 0x0004);

        // SIParameter
        assert_eq!(info.si_parameter_list.len(), 1);
        assert_eq!(info.si_parameter_list[0].parameter_version, 0x05);
        assert_eq!(info.si_parameter_list[0].table_list.len(), 1);

        // Broadcaster
        assert_eq!(info.broadcaster_list.len(), 1);
        let bc = &info.broadcaster_list[0];
        assert_eq!(bc.broadcaster_id, 0x01);
        assert_eq!(bc.broadcaster_name, "Ａ");
        assert_eq!(bc.service_list.len(), 1);
        assert_eq!(bc.service_list[0].service_id, 0x0064);
        assert_eq!(bc.service_list[0].service_type, 0x01);
        assert_eq!(bc.broadcaster_type, 1);
        let t = bc.terrestrial.as_ref().expect("terrestrial");
        assert_eq!(t.terrestrial_broadcaster_id, 0x1234);
        assert_eq!(t.affiliation_id_list, vec![0x0A]);
        assert_eq!(t.broadcaster_id_list.len(), 1);
        assert_eq!(t.broadcaster_id_list[0].original_network_id, 0x7E87);
        assert_eq!(t.broadcaster_id_list[0].broadcaster_id, 0x05);
    }

    #[test]
    fn test_build_bit_network_info_no_extended() {
        // 拡張ブロードキャスタ記述子が無い場合 broadcaster_type は 0xFF、terrestrial は None
        let sl_payload = vec![0x00, 0x64, 0x01];
        let sl_desc = make_desc(0x41, &sl_payload);

        let bit = parse_bit(0x0007, &[], &[(0x02, sl_desc)]);
        let info = build_bit_network_info(&bit);

        assert!(info.si_parameter_list.is_empty());
        assert_eq!(info.broadcaster_list.len(), 1);
        let bc = &info.broadcaster_list[0];
        assert_eq!(bc.broadcaster_id, 0x02);
        assert_eq!(bc.broadcaster_type, BROADCASTER_TYPE_INVALID);
        assert!(bc.terrestrial.is_none());
        assert!(bc.broadcaster_name.is_empty());
        assert_eq!(bc.service_list.len(), 1);
    }

    #[test]
    fn test_build_bit_network_info_multiple_broadcasters() {
        let bc1 = make_desc(0xD8, &[0x1B, 0x28, 0x4A, 0x41]);
        let bc2 = make_desc(0xD8, &[0x1B, 0x28, 0x4A, 0x42]);

        let bit = parse_bit(0x0010, &[], &[(0x01, bc1), (0x02, bc2)]);
        let info = build_bit_network_info(&bit);

        assert_eq!(info.broadcaster_list.len(), 2);
        assert_eq!(info.broadcaster_list[0].broadcaster_id, 0x01);
        assert_eq!(info.broadcaster_list[0].broadcaster_name, "Ａ");
        assert_eq!(info.broadcaster_list[1].broadcaster_id, 0x02);
        assert_eq!(info.broadcaster_list[1].broadcaster_name, "Ｂ");
    }
}
