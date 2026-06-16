// Rust port of LibISDB/Filters/AnalyzerFilter.cpp の NIT→ネットワーク情報構築ロジック。
// AnalyzerFilter.cpp:OnNITSection + AnalyzerFilter.hpp:NetworkStreamInfo 他
//
// NIT(Network Information Table)をパースして、ネットワーク配下の各 TS の情報
// (サービスリスト・伝送系情報・部分受信)を構築する純粋ロジックを切り出す。
// 入力はパース済みの libisdb_ts_tables::NITTable、出力は NetworkStreamInfo のリスト。
// PID マップ・フィルタグラフ管理は対象外。
//
// 移植対象:
//   - 各 TS の transport_stream_id / original_network_id
//   - ServiceListDescriptor(0x41)              → service_list
//   - TerrestrialDeliverySystemDescriptor(0xFA) → terrestrial (地上波伝送情報)
//   - SatelliteDeliverySystemDescriptor(0x43)   → satellite (衛星伝送情報)
//   - CableDeliverySystemDescriptor(0x44)       → cable (有線伝送情報)
//   - PartialReceptionDescriptor(0xFB)         → partial_reception_service_list (部分受信)

use libisdb_ts_tables::NITTable;
use libisdb_descriptor::{
    ServiceListDescriptor, TerrestrialDeliverySystemDescriptor, PartialReceptionDescriptor,
    SatelliteDeliverySystemDescriptor, CableDeliverySystemDescriptor,
};

/// NIT のサービスリストエントリ。ServiceListDescriptor::ServiceInfo 相当。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct NetworkServiceInfo {
    pub service_id: u16,
    pub service_type: u8,
}

/// 地上波伝送系情報。AnalyzerFilter.hpp:TerrestrialDeliverySystemInfo。
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct TerrestrialDeliverySystemInfo {
    pub area_code: u16,
    pub guard_interval: u8,
    pub transmission_mode: u8,
    pub frequency: Vec<u16>,
}

/// 衛星伝送系情報。AnalyzerFilter.hpp:SatelliteDeliverySystemInfo。
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct SatelliteDeliverySystemInfo {
    pub frequency: u32,
    pub orbital_position: u16,
    pub west_east_flag: bool,
    pub polarization: u8,
    pub modulation: u8,
    pub symbol_rate: u32,
    pub fec_inner: u8,
}

/// 有線伝送系情報。AnalyzerFilter.hpp:CableDeliverySystemInfo。
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct CableDeliverySystemInfo {
    pub frequency: u32,
    pub frame_type: u8,
    pub fec_outer: u8,
    pub modulation: u8,
    pub symbol_rate: u32,
    pub fec_inner: u8,
}

/// NIT 配下の TS 情報。AnalyzerFilter.hpp:NetworkStreamInfo。
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct NetworkStreamInfo {
    pub transport_stream_id: u16,
    pub original_network_id: u16,
    pub service_list: Vec<NetworkServiceInfo>,
    /// 地上波伝送系情報(あれば)
    pub terrestrial: Option<TerrestrialDeliverySystemInfo>,
    /// 衛星伝送系情報(あれば)
    pub satellite: Option<SatelliteDeliverySystemInfo>,
    /// 有線伝送系情報(あれば)
    pub cable: Option<CableDeliverySystemInfo>,
    /// 部分受信対象のサービス ID リスト(あれば)
    pub partial_reception_service_list: Vec<u16>,
}

/// NITTable から各 TS のネットワーク情報リストを構築する。OnNITSection 相当。
pub fn build_network_stream_list(nit: &NITTable) -> Vec<NetworkStreamInfo> {
    let mut list = Vec::with_capacity(nit.get_ts_count());

    for i in 0..nit.get_ts_count() {
        let ts = match nit.get_ts_info(i) {
            Some(t) => t,
            None => continue,
        };

        let mut info = NetworkStreamInfo {
            transport_stream_id: ts.transport_stream_id,
            original_network_id: ts.original_network_id,
            ..Default::default()
        };

        // ServiceListDescriptor(0x41): サービスリスト
        for desc in ts.descriptors.iter() {
            if let Some(sld) = ServiceListDescriptor::from_descriptor(desc) {
                for s in &sld.service_list {
                    info.service_list.push(NetworkServiceInfo {
                        service_id: s.service_id,
                        service_type: s.service_type,
                    });
                }
            }
        }

        // TerrestrialDeliverySystemDescriptor(0xFA): 地上波伝送系
        for desc in ts.descriptors.iter() {
            if let Some(td) = TerrestrialDeliverySystemDescriptor::from_descriptor(desc) {
                info.terrestrial = Some(TerrestrialDeliverySystemInfo {
                    area_code: td.area_code,
                    guard_interval: td.guard_interval,
                    transmission_mode: td.transmission_mode,
                    frequency: td.frequency.clone(),
                });
                break;
            }
        }

        // SatelliteDeliverySystemDescriptor(0x43): 衛星伝送系
        for desc in ts.descriptors.iter() {
            if let Some(sd) = SatelliteDeliverySystemDescriptor::from_descriptor(desc) {
                info.satellite = Some(SatelliteDeliverySystemInfo {
                    frequency: sd.frequency,
                    orbital_position: sd.orbital_position,
                    west_east_flag: sd.west_east_flag,
                    polarization: sd.polarization,
                    modulation: sd.modulation,
                    symbol_rate: sd.symbol_rate,
                    fec_inner: sd.fec_inner,
                });
                break;
            }
        }

        // CableDeliverySystemDescriptor(0x44): 有線伝送系
        for desc in ts.descriptors.iter() {
            if let Some(cd) = CableDeliverySystemDescriptor::from_descriptor(desc) {
                info.cable = Some(CableDeliverySystemInfo {
                    frequency: cd.frequency,
                    frame_type: cd.frame_type,
                    fec_outer: cd.fec_outer,
                    modulation: cd.modulation,
                    symbol_rate: cd.symbol_rate,
                    fec_inner: cd.fec_inner,
                });
                break;
            }
        }

        // PartialReceptionDescriptor(0xFB): 部分受信
        for desc in ts.descriptors.iter() {
            if let Some(prd) = PartialReceptionDescriptor::from_descriptor(desc) {
                info.partial_reception_service_list = prd.service_list.clone();
                break;
            }
        }

        list.push(info);
    }

    list
}

/// NIT の network_id を返す。
pub fn network_id(nit: &NITTable) -> u16 {
    nit.get_network_id()
}

#[cfg(test)]
mod tests {
    use super::*;
    use libisdb_ts_tables::NITTable;
    use libisdb_ts_packet::{TsPacket, TS_PACKET_SIZE};

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

    /// ServiceListDescriptor(0x41) を組み立てる。services: (service_id, service_type)。
    fn build_service_list_desc(services: &[(u16, u8)]) -> Vec<u8> {
        let mut p = Vec::new();
        for &(sid, st) in services {
            p.extend_from_slice(&sid.to_be_bytes());
            p.push(st);
        }
        let mut d = vec![0x41, p.len() as u8];
        d.extend_from_slice(&p);
        d
    }

    /// TerrestrialDeliverySystemDescriptor(0xFA) を組み立てる。
    fn build_terrestrial_desc(area_code: u16, guard: u8, mode: u8, freqs: &[u16]) -> Vec<u8> {
        let mut p = Vec::new();
        // area_code(12bit) + guard_interval(2bit) + transmission_mode(2bit) + reserved
        let w = ((area_code & 0x0FFF) << 4) | ((guard as u16 & 0x03) << 2) | (mode as u16 & 0x03);
        p.extend_from_slice(&w.to_be_bytes());
        for &f in freqs {
            p.extend_from_slice(&f.to_be_bytes());
        }
        let mut d = vec![0xFA, p.len() as u8];
        d.extend_from_slice(&p);
        d
    }

    /// PartialReceptionDescriptor(0xFB) を組み立てる。
    fn build_partial_reception_desc(service_ids: &[u16]) -> Vec<u8> {
        let mut p = Vec::new();
        for &sid in service_ids {
            p.extend_from_slice(&sid.to_be_bytes());
        }
        let mut d = vec![0xFB, p.len() as u8];
        d.extend_from_slice(&p);
        d
    }

    /// SatelliteDeliverySystemDescriptor(0x43) を組み立てる。
    fn build_satellite_desc() -> Vec<u8> {
        // frequency BCD 8桁=12345678, orbital BCD 4桁=1100,
        // p[6]=0xA1(we=1,pol=01,mod=00001), symbol_rate BCD 7桁=0234560, fec=0x07
        let p: [u8; 11] = [
            0x12, 0x34, 0x56, 0x78,
            0x11, 0x00,
            0xA1,
            0x02, 0x34, 0x56, 0x07,
        ];
        let mut d = vec![0x43, p.len() as u8];
        d.extend_from_slice(&p);
        d
    }

    /// CableDeliverySystemDescriptor(0x44) を組み立てる。
    fn build_cable_desc() -> Vec<u8> {
        // frequency BCD 8桁=12345678, p[5]=0x52(frame_type=0101,fec_outer=0010),
        // modulation=0x07, symbol_rate BCD 7桁=0234560, fec_inner=0x07
        let p: [u8; 11] = [
            0x12, 0x34, 0x56, 0x78,
            0x00,
            0x52,
            0x07,
            0x02, 0x34, 0x56, 0x07,
        ];
        let mut d = vec![0x44, p.len() as u8];
        d.extend_from_slice(&p);
        d
    }

    /// NIT セクションを 1 TS 分組み立てて NITTable に store する。
    fn make_nit_table(network_id: u16, ts_id: u16, onid: u16, ts_descs: &[u8]) -> NITTable {
        // network_descriptors_length(2) = 0
        let mut body = Vec::new();
        body.extend_from_slice(&[0xF0, 0x00]); // reserved + network_descriptors_length=0
        // transport_stream_loop_length(2)
        // TS ループ: ts_id(2) + onid(2) + reserved+ts_descs_len(2) + descs
        let mut ts_loop = Vec::new();
        ts_loop.extend_from_slice(&ts_id.to_be_bytes());
        ts_loop.extend_from_slice(&onid.to_be_bytes());
        let dll = ts_descs.len();
        ts_loop.push(0xF0 | ((dll >> 8) as u8 & 0x0F));
        ts_loop.push((dll & 0xFF) as u8);
        ts_loop.extend_from_slice(ts_descs);
        body.push(0xF0 | ((ts_loop.len() >> 8) as u8 & 0x0F));
        body.push((ts_loop.len() & 0xFF) as u8);
        body.extend_from_slice(&ts_loop);

        // section header (table_id = 0x40 NIT actual)
        let table_id = 0x40u8;
        let section_length = 5 + body.len() + 4;
        let mut sec = Vec::new();
        sec.push(table_id);
        sec.push(0xB0 | ((section_length >> 8) as u8));
        sec.push((section_length & 0xFF) as u8);
        sec.extend_from_slice(&network_id.to_be_bytes()); // table_id_extension = network_id
        sec.push(0xC1);
        sec.push(0x00);
        sec.push(0x00);
        sec.extend_from_slice(&body);
        let crc = crc32_mpeg2_calc(&sec);
        sec.extend_from_slice(&crc.to_be_bytes());

        // TS パケットに載せる (PID = 0x0010 NIT)
        let pid = 0x0010u16;
        let mut data = [0xFFu8; TS_PACKET_SIZE];
        data[0] = 0x47;
        data[1] = 0x40 | ((pid >> 8) as u8 & 0x1F);
        data[2] = (pid & 0xFF) as u8;
        data[3] = 0x10;
        data[4] = 0x00;
        data[5..5 + sec.len()].copy_from_slice(&sec);

        let mut pkt = TsPacket::new(&data);
        pkt.parse_packet(None);

        let mut table = NITTable::new();
        table.store_packet(&pkt);
        table
    }

    #[test]
    fn test_build_network_stream_service_list() {
        let descs = build_service_list_desc(&[(0x0400, 0x01), (0x0401, 0x02)]);
        let nit = make_nit_table(0x0004, 0x7FE0, 0x0004, &descs);
        assert_eq!(network_id(&nit), 0x0004);
        assert_eq!(nit.get_ts_count(), 1);

        let list = build_network_stream_list(&nit);
        assert_eq!(list.len(), 1);
        let ts = &list[0];
        assert_eq!(ts.transport_stream_id, 0x7FE0);
        assert_eq!(ts.original_network_id, 0x0004);
        assert_eq!(ts.service_list.len(), 2);
        assert_eq!(ts.service_list[0].service_id, 0x0400);
        assert_eq!(ts.service_list[0].service_type, 0x01);
        assert_eq!(ts.service_list[1].service_id, 0x0401);
    }

    #[test]
    fn test_build_network_stream_terrestrial() {
        let mut descs = build_service_list_desc(&[(0x0400, 0x01)]);
        descs.extend_from_slice(&build_terrestrial_desc(0x017F, 0x02, 0x01, &[0x1234, 0x5678]));
        let nit = make_nit_table(0x0004, 0x7FE0, 0x0004, &descs);
        let list = build_network_stream_list(&nit);
        let ts = &list[0];
        let terr = ts.terrestrial.as_ref().expect("terrestrial info");
        assert_eq!(terr.area_code, 0x017F);
        assert_eq!(terr.guard_interval, 0x02);
        assert_eq!(terr.transmission_mode, 0x01);
        assert_eq!(terr.frequency, vec![0x1234, 0x5678]);
    }

    #[test]
    fn test_build_network_stream_partial_reception() {
        let mut descs = build_service_list_desc(&[(0x0400, 0x01)]);
        descs.extend_from_slice(&build_partial_reception_desc(&[0x0400]));
        let nit = make_nit_table(0x0004, 0x7FE0, 0x0004, &descs);
        let list = build_network_stream_list(&nit);
        let ts = &list[0];
        assert_eq!(ts.partial_reception_service_list, vec![0x0400]);
    }

    #[test]
    fn test_build_network_stream_satellite() {
        let mut descs = build_service_list_desc(&[(0x0400, 0x01)]);
        descs.extend_from_slice(&build_satellite_desc());
        let nit = make_nit_table(0x0004, 0x6020, 0x0004, &descs);
        let list = build_network_stream_list(&nit);
        let ts = &list[0];
        let sat = ts.satellite.as_ref().expect("satellite info");
        assert_eq!(sat.frequency, 12345678);
        assert_eq!(sat.orbital_position, 1100);
        assert!(sat.west_east_flag);
        assert_eq!(sat.polarization, 0b01);
        assert_eq!(sat.modulation, 0b00001);
        assert_eq!(sat.symbol_rate, 234560);
        assert_eq!(sat.fec_inner, 0x07);
        // 地上波は無し
        assert!(ts.terrestrial.is_none());
    }

    #[test]
    fn test_build_network_stream_cable() {
        let mut descs = build_service_list_desc(&[(0x0400, 0x01)]);
        descs.extend_from_slice(&build_cable_desc());
        let nit = make_nit_table(0x0004, 0x6020, 0x0004, &descs);
        let list = build_network_stream_list(&nit);
        let ts = &list[0];
        let cab = ts.cable.as_ref().expect("cable info");
        assert_eq!(cab.frequency, 12345678);
        assert_eq!(cab.frame_type, 0b0101);
        assert_eq!(cab.fec_outer, 0b0010);
        assert_eq!(cab.modulation, 0x07);
        assert_eq!(cab.symbol_rate, 234560);
        assert_eq!(cab.fec_inner, 0x07);
        // 地上波・衛星は無し
        assert!(ts.terrestrial.is_none());
        assert!(ts.satellite.is_none());
    }

    #[test]
    fn test_build_network_stream_no_descriptors() {
        let nit = make_nit_table(0x0004, 0x7FE0, 0x0004, &[]);
        let list = build_network_stream_list(&nit);
        assert_eq!(list.len(), 1);
        assert!(list[0].service_list.is_empty());
        assert!(list[0].terrestrial.is_none());
        assert!(list[0].partial_reception_service_list.is_empty());
    }

    #[test]
    fn test_build_network_stream_combined() {
        let mut descs = build_service_list_desc(&[(0x0400, 0x01), (0x0401, 0x02), (0x0402, 0xC0)]);
        descs.extend_from_slice(&build_terrestrial_desc(0x017F, 0x02, 0x01, &[0x1234]));
        descs.extend_from_slice(&build_partial_reception_desc(&[0x0400, 0x0401]));
        let nit = make_nit_table(0x0004, 0x7FE0, 0x0004, &descs);
        let list = build_network_stream_list(&nit);
        let ts = &list[0];
        assert_eq!(ts.service_list.len(), 3);
        assert!(ts.terrestrial.is_some());
        assert_eq!(ts.partial_reception_service_list, vec![0x0400, 0x0401]);
    }
}
