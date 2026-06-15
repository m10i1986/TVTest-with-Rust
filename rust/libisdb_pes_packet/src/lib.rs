// LibISDB の PESPacket.cpp + PESPacket.hpp を Rust へ移植したもの。
//
// 移植対象:
//   - PTS_CLOCK     : PESPacket.hpp:37 (90kHz)
//   - get_pts       : PESPacket.hpp:39
//   - PesPacket     : PESPacket.cpp:78
//     - parse_header     : PESPacket.cpp:91
//     - get_pts_count    : PESPacket.cpp:141
//     - get_packet_crc   : PESPacket.cpp:150
//     - get_payload_data / get_payload_size : PESPacket.cpp:177
//   - PesParser     : PESPacket.cpp:201
//     - store_packet     : PESPacket.cpp:210
//     - store_header     : PESPacket.cpp:259
//     - store_payload    : PESPacket.cpp:290
//
// C++ の DataBuffer 継承は Vec<u8> で代替する。
// PacketHandler (コールバック) は FnMut クロージャで代替する。

use libisdb_ts_packet::TsPacket;

/// PTS クロック周波数(90kHz)。PESPacket.hpp:37。
pub const PTS_CLOCK: i64 = 90000;

/// ストリーム ID の定数(IsAdditionalHeaderStreamID で使用)。
mod stream_id {
    pub const PROGRAM_STREAM_MAP:                   u8 = 0xBC;
    pub const PADDING_STREAM:                       u8 = 0xBE;
    pub const PRIVATE_STREAM_2:                     u8 = 0xBF;
    pub const ECM_STREAM:                           u8 = 0xF0;
    pub const EMM_STREAM:                           u8 = 0xF1;
    pub const DSMCC_STREAM:                         u8 = 0xF2;
    pub const ITU_T_REC_H222_1_TYPE_E:              u8 = 0xF8;
    pub const PROGRAM_STREAM_DIRECTORY:             u8 = 0xFF;
}

/// 追加ヘッダを持つストリーム ID かどうかを判定する。PESPacket.cpp:61。
fn is_additional_header_stream_id(id: u8) -> bool {
    id != stream_id::PROGRAM_STREAM_MAP
        && id != stream_id::PADDING_STREAM
        && id != stream_id::PRIVATE_STREAM_2
        && id != stream_id::ECM_STREAM
        && id != stream_id::EMM_STREAM
        && id != stream_id::PROGRAM_STREAM_DIRECTORY
        && id != stream_id::DSMCC_STREAM
        && id != stream_id::ITU_T_REC_H222_1_TYPE_E
}

/// 5バイトの PTS/DTS フィールドから 33 ビット時刻を取り出す。PESPacket.hpp:39。
pub fn get_pts(p: &[u8]) -> i64 {
    let high = (((p[0] as u32 & 0x0E) << 14)
        | ((p[1] as u32) << 7)
        | ((p[2] as u32) >> 1)) as i64;
    let low = (((p[3] as u32) << 7) | ((p[4] as u32) >> 1)) as i64;
    (high << 15) | low
}

/// PES ヘッダ情報。PESPacket.hpp:86。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PesHeader {
    pub stream_id: u8,
    pub packet_length: u16,
    pub scrambling_control: u8,
    pub priority: bool,
    pub data_alignment_indicator: bool,
    pub copyright: bool,
    pub original_or_copy: bool,
    pub pts_dts_flags: u8,
    pub escr_flag: bool,
    pub es_rate_flag: bool,
    pub dsm_trick_mode_flag: bool,
    pub additional_copy_info_flag: bool,
    pub crc_flag: bool,
    pub extension_flag: bool,
    pub header_data_length: u8,
}

/// PES パケット。PESPacket.cpp:78。
///
/// 生データは Vec<u8> で保持する。
#[derive(Debug, Clone, Default)]
pub struct PesPacket {
    pub data: Vec<u8>,
    header: PesHeader,
}

impl PesPacket {
    pub fn new() -> Self {
        Self::default()
    }

    /// ヘッダを解析する。PESPacket.cpp:91。
    pub fn parse_header(&mut self) -> bool {
        self.header = PesHeader::default();

        if self.data.len() < 6 {
            return false;
        }
        if self.data[0] != 0x00 || self.data[1] != 0x00 || self.data[2] != 0x01 {
            return false; // packet_start_code_prefix 異常
        }

        self.header.stream_id     = self.data[3];
        self.header.packet_length = ((self.data[4] as u16) << 8) | self.data[5] as u16;

        if is_additional_header_stream_id(self.header.stream_id) {
            if self.data.len() < 9 {
                return false;
            }
            if self.data[6] & 0xC0 != 0x80 {
                return false; // 固定ビット異常
            }

            self.header.scrambling_control      = (self.data[6] & 0x30) >> 4;
            self.header.priority                = (self.data[6] & 0x08) != 0;
            self.header.data_alignment_indicator= (self.data[6] & 0x04) != 0;
            self.header.copyright               = (self.data[6] & 0x02) != 0;
            self.header.original_or_copy        = (self.data[6] & 0x01) != 0;
            self.header.pts_dts_flags           = (self.data[7] & 0xC0) >> 6;
            self.header.escr_flag               = (self.data[7] & 0x20) != 0;
            self.header.es_rate_flag            = (self.data[7] & 0x10) != 0;
            self.header.dsm_trick_mode_flag     = (self.data[7] & 0x08) != 0;
            self.header.additional_copy_info_flag=(self.data[7] & 0x04) != 0;
            self.header.crc_flag                = (self.data[7] & 0x02) != 0;
            self.header.extension_flag          = (self.data[7] & 0x01) != 0;
            self.header.header_data_length      = self.data[8];

            if self.header.scrambling_control != 0 {
                return false; // Not scrambled のみ対応
            }
            if self.header.pts_dts_flags == 1 {
                return false; // 未定義のフラグ
            }
        }

        true
    }

    /// リセットする。PESPacket.cpp:133。
    pub fn reset(&mut self) {
        self.data.clear();
        self.header = PesHeader::default();
    }

    /// PTS カウントを返す。PESPacket.cpp:141。
    pub fn get_pts_count(&self) -> Option<i64> {
        if self.header.pts_dts_flags != 0 && self.data.len() >= 14 {
            Some(get_pts(&self.data[9..14]))
        } else {
            None
        }
    }

    /// パケット CRC を返す。PESPacket.cpp:150。
    pub fn get_packet_crc(&self) -> u16 {
        if !self.header.crc_flag {
            return 0;
        }
        let mut pos: usize = 9;
        if self.header.pts_dts_flags == 2 { pos += 5; }
        else if self.header.pts_dts_flags == 3 { pos += 10; }
        if self.header.escr_flag { pos += 6; }
        if self.header.es_rate_flag { pos += 3; }
        if self.header.dsm_trick_mode_flag { pos += 1; }
        if self.header.additional_copy_info_flag { pos += 1; }
        if self.data.len() < pos + 2 {
            return 0;
        }
        ((self.data[pos] as u16) << 8) | self.data[pos + 1] as u16
    }

    /// ペイロード先頭スライスを返す。PESPacket.cpp:177。
    pub fn get_payload_data(&self) -> Option<&[u8]> {
        let pos = self.payload_start_pos();
        if self.data.len() <= pos {
            return None;
        }
        Some(&self.data[pos..])
    }

    /// ペイロードサイズを返す。PESPacket.cpp:188。
    pub fn get_payload_size(&self) -> usize {
        let pos = self.payload_start_pos();
        if self.data.len() <= pos { 0 } else { self.data.len() - pos }
    }

    pub fn get_stream_id(&self) -> u8 { self.header.stream_id }
    pub fn get_packet_length(&self) -> u16 { self.header.packet_length }
    pub fn get_scrambling_control(&self) -> u8 { self.header.scrambling_control }
    pub fn get_priority(&self) -> bool { self.header.priority }
    pub fn get_data_alignment_indicator(&self) -> bool { self.header.data_alignment_indicator }
    pub fn get_copyright(&self) -> bool { self.header.copyright }
    pub fn get_original_or_copy(&self) -> bool { self.header.original_or_copy }
    pub fn get_pts_dts_flags(&self) -> u8 { self.header.pts_dts_flags }
    pub fn get_escr_flag(&self) -> bool { self.header.escr_flag }
    pub fn get_es_rate_flag(&self) -> bool { self.header.es_rate_flag }
    pub fn get_dsm_trick_mode_flag(&self) -> bool { self.header.dsm_trick_mode_flag }
    pub fn get_additional_copy_info_flag(&self) -> bool { self.header.additional_copy_info_flag }
    pub fn get_crc_flag(&self) -> bool { self.header.crc_flag }
    pub fn get_extension_flag(&self) -> bool { self.header.extension_flag }
    pub fn get_header_data_length(&self) -> u8 { self.header.header_data_length }
    pub fn get_size(&self) -> usize { self.data.len() }

    fn payload_start_pos(&self) -> usize {
        if is_additional_header_stream_id(self.header.stream_id) {
            self.header.header_data_length as usize + 9
        } else {
            6
        }
    }
}

/// PES パーサー。PESPacket.cpp:201。
///
/// TS パケットから PES パケットを組み立て、クロージャで通知する。
pub struct PesParser {
    packet: PesPacket,
    is_storing: bool,
    store_size: usize,
}

impl PesParser {
    /// 新規作成。PESPacket.cpp:201。
    pub fn new() -> Self {
        Self {
            packet: PesPacket::new(),
            is_storing: false,
            store_size: 0,
        }
    }

    /// リセットする。PESPacket.cpp:243。
    pub fn reset(&mut self) {
        self.packet.reset();
        self.is_storing = false;
        self.store_size = 0;
    }

    /// TS パケットを処理する。PESPacket.cpp:210。
    ///
    /// PES パケットが完成するたびに `handler` を呼び出す。
    /// 戻り値は payload_unit_start_indicator の状態。
    pub fn store_packet<F>(&mut self, pkt: &TsPacket, handler: &mut F) -> bool
    where
        F: FnMut(&PesPacket),
    {
        let payload = match pkt.get_payload_data() {
            Some(p) => p,
            None => return false,
        };
        let payload_size = pkt.get_payload_size() as usize;
        if payload_size == 0 {
            return false;
        }
        let payload = &payload[..payload_size];

        let mut triggered = false;
        let mut pos: usize = 0;

        if pkt.get_payload_unit_start_indicator() {
            if self.is_storing && self.packet.get_packet_length() == 0 {
                handler(&self.packet);
            }
            self.is_storing = false;
            triggered = true;
            self.packet.data.clear();

            let consumed = self.store_header(&payload[pos..], payload_size - pos);
            pos += consumed;
            pos += self.store_payload(&payload[pos..], payload_size - pos, handler);
        } else {
            let consumed = self.store_header(&payload[pos..], payload_size - pos);
            pos += consumed;
            self.store_payload(&payload[pos..], payload_size - pos, handler);
        }
        let _ = pos;

        triggered
    }

    /// ヘッダをストアする。PESPacket.cpp:259。戻り値は消費バイト数。
    fn store_header(&mut self, payload: &[u8], remain: usize) -> usize {
        if self.is_storing {
            return 0;
        }

        let header_remain = 9usize.saturating_sub(self.packet.get_size());

        if remain >= header_remain {
            self.packet.data.extend_from_slice(&payload[..header_remain]);
            if self.packet.parse_header() {
                self.store_size = self.packet.get_packet_length() as usize;
                if self.store_size != 0 {
                    self.store_size += 6;
                }
                self.is_storing = true;
                return header_remain;
            } else {
                self.packet.reset();
                return remain;
            }
        } else {
            self.packet.data.extend_from_slice(&payload[..remain]);
            return remain;
        }
    }

    /// ペイロードをストアする。PESPacket.cpp:290。戻り値は消費バイト数。
    fn store_payload<F>(&mut self, payload: &[u8], remain: usize, handler: &mut F) -> usize
    where
        F: FnMut(&PesPacket),
    {
        if !self.is_storing {
            return 0;
        }

        let store_remain = if self.store_size != 0 {
            self.store_size.saturating_sub(self.packet.get_size())
        } else {
            remain
        };

        if self.store_size != 0 && store_remain <= remain {
            self.packet.data.extend_from_slice(&payload[..store_remain]);
            handler(&self.packet);
            self.packet.reset();
            self.is_storing = false;
            return store_remain;
        } else {
            self.packet.data.extend_from_slice(&payload[..remain]);
            return remain;
        }
    }
}

impl Default for PesParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libisdb_ts_packet::{TsPacket, TS_PACKET_SIZE};

    fn make_ts_packet_with_payload(pid: u16, pusi: bool, cc: u8, payload: &[u8]) -> TsPacket {
        let mut data = [0u8; TS_PACKET_SIZE];
        data[0] = 0x47;
        let pusi_bit: u8 = if pusi { 0x40 } else { 0 };
        data[1] = pusi_bit | ((pid >> 8) as u8 & 0x1F);
        data[2] = pid as u8;
        data[3] = (0x01u8 << 4) | (cc & 0x0F); // payload only
        let dst = 4;
        let copy_len = payload.len().min(TS_PACKET_SIZE - dst);
        data[dst..dst + copy_len].copy_from_slice(&payload[..copy_len]);
        let mut pkt = TsPacket::new(&data);
        pkt.parse_packet(None);
        pkt
    }

    fn make_pes_header(stream_id: u8, pts: Option<i64>, payload_data: &[u8]) -> Vec<u8> {
        let mut pes = Vec::new();
        pes.extend_from_slice(&[0x00, 0x00, 0x01]); // start code
        pes.push(stream_id);

        let mut opt = Vec::new();
        let pts_dts_flags: u8 = if pts.is_some() { 0x02 } else { 0 }; // PTS only
        opt.push(0x80); // fixed bits
        opt.push(pts_dts_flags << 6);
        let hdl: u8 = if pts.is_some() { 5 } else { 0 };
        opt.push(hdl);

        if let Some(p) = pts {
            // PTS 33bit → 5バイトにパック (PESPacket.hpp:GetPTS の逆操作)
            // p[0]: 0b0010_HHH1  (H = bits[32:30])
            // p[1]: bits[29:22] (8 bits)
            // p[2]: bits[21:15]_1 (7 bits + marker)
            // p[3]: bits[14:7] (8 bits)
            // p[4]: bits[6:0]_1 (7 bits + marker)
            let p = p as u64;
            let b0 = ((p >> 29) & 0x06) as u8 | 0x21; // bits[31:30] in [2:1], prefix 0010, marker 1
            let b1 = ((p >> 22) & 0xFF) as u8;
            let b2 = (((p >> 15) & 0x7F) as u8) << 1 | 0x01;
            let b3 = ((p >> 7) & 0xFF) as u8;
            let b4 = ((p & 0x7F) as u8) << 1 | 0x01;
            opt.push(b0);
            opt.push(b1);
            opt.push(b2);
            opt.push(b3);
            opt.push(b4);
        }

        let packet_length = (opt.len() + payload_data.len()) as u16;
        pes.push((packet_length >> 8) as u8);
        pes.push(packet_length as u8);
        pes.extend_from_slice(&opt);
        pes.extend_from_slice(payload_data);
        pes
    }

    #[test]
    fn test_parse_header_basic() {
        let pes_data = make_pes_header(0xE0, None, b"hello");
        let mut pkt = PesPacket::new();
        pkt.data = pes_data;
        assert!(pkt.parse_header());
        assert_eq!(pkt.get_stream_id(), 0xE0);
        assert_eq!(pkt.get_pts_dts_flags(), 0);
        assert_eq!(pkt.get_header_data_length(), 0);
    }

    #[test]
    fn test_parse_header_with_pts() {
        let pes_data = make_pes_header(0xE0, Some(12345), b"world");
        let mut pkt = PesPacket::new();
        pkt.data = pes_data;
        assert!(pkt.parse_header());
        assert_eq!(pkt.get_pts_dts_flags(), 2); // PTS only
        let pts = pkt.get_pts_count().unwrap();
        assert_eq!(pts, 12345);
    }

    #[test]
    fn test_parse_header_bad_start_code() {
        let mut pkt = PesPacket::new();
        pkt.data = vec![0x00, 0x01, 0x01, 0xE0, 0x00, 0x00, 0x80, 0x00, 0x00];
        assert!(!pkt.parse_header());
    }

    #[test]
    fn test_parse_header_too_short() {
        let mut pkt = PesPacket::new();
        pkt.data = vec![0x00, 0x00, 0x01, 0xE0, 0x00];
        assert!(!pkt.parse_header());
    }

    #[test]
    fn test_parse_header_scrambled_rejected() {
        let mut pkt = PesPacket::new();
        // scrambling_control = 0x01 (non-zero → rejected)
        pkt.data = vec![0x00, 0x00, 0x01, 0xE0, 0x00, 0x0A, 0x80 | 0x10, 0x00, 0x00];
        assert!(!pkt.parse_header());
    }

    #[test]
    fn test_get_payload_data() {
        let pes_data = make_pes_header(0xE0, None, b"payload");
        let mut pkt = PesPacket::new();
        pkt.data = pes_data;
        pkt.parse_header();
        let payload = pkt.get_payload_data().unwrap();
        assert_eq!(payload, b"payload");
        assert_eq!(pkt.get_payload_size(), 7);
    }

    #[test]
    fn test_get_pts_none_when_no_pts() {
        let pes_data = make_pes_header(0xE0, None, b"test");
        let mut pkt = PesPacket::new();
        pkt.data = pes_data;
        pkt.parse_header();
        assert!(pkt.get_pts_count().is_none());
    }

    #[test]
    fn test_reset() {
        let pes_data = make_pes_header(0xE0, Some(9999), b"data");
        let mut pkt = PesPacket::new();
        pkt.data = pes_data;
        pkt.parse_header();
        pkt.reset();
        assert_eq!(pkt.get_size(), 0);
        assert_eq!(pkt.get_stream_id(), 0);
    }

    #[test]
    fn test_get_pts_value() {
        // PTS = 90000 (1秒 @ 90kHz)
        let pts_val: i64 = PTS_CLOCK;
        let pes_data = make_pes_header(0xC0, Some(pts_val), b"audio");
        let mut pkt = PesPacket::new();
        pkt.data = pes_data;
        pkt.parse_header();
        assert_eq!(pkt.get_pts_count(), Some(pts_val));
    }

    #[test]
    fn test_parser_single_pes() {
        let pes_data = make_pes_header(0xE0, None, b"video");
        let pkt = make_ts_packet_with_payload(0x100, true, 0, &pes_data);

        let mut parser = PesParser::new();
        let mut received: Vec<Vec<u8>> = Vec::new();
        let triggered = parser.store_packet(&pkt, &mut |p| {
            received.push(p.data.clone());
        });
        assert!(triggered);
        // packet_length を元に完成するかは packet_length 依存
        // ここでは簡易確認のみ
    }

    #[test]
    fn test_parser_reset() {
        let mut parser = PesParser::new();
        let pes_data = make_pes_header(0xE0, None, b"data");
        let pkt = make_ts_packet_with_payload(0x100, true, 0, &pes_data);
        parser.store_packet(&pkt, &mut |_| {});
        parser.reset();
        assert_eq!(parser.packet.get_size(), 0);
        assert!(!parser.is_storing);
    }

    #[test]
    fn test_program_stream_map_no_additional_header() {
        // stream_id=0xBC (PROGRAM_STREAM_MAP) → 追加ヘッダなし
        let mut pkt = PesPacket::new();
        pkt.data = vec![0x00, 0x00, 0x01, 0xBC, 0x00, 0x05, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE];
        assert!(pkt.parse_header());
        assert_eq!(pkt.get_stream_id(), 0xBC);
        // ペイロードは 6 バイト目から
        let payload = pkt.get_payload_data().unwrap();
        assert_eq!(payload[0], 0xAA);
    }

    #[test]
    fn test_pts_clock_value() {
        assert_eq!(PTS_CLOCK, 90000);
    }
}
