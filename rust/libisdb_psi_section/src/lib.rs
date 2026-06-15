// LibISDB の PSISection.cpp + PSISection.hpp を Rust へ移植したもの。
//
// 移植対象:
//   - PsiSection    : PSISection.cpp:38  (PSI セクションの保持と解析)
//     - parse_header  : PSISection.cpp:71  (ヘッダ解析)
//     - get_payload_data / get_payload_size : PSISection.cpp:131/142
//     - 各種 getter
//   - PsiSectionParser : PSISection.cpp:181 (TS パケットから PSI セクションを組み立て)
//     - store_packet   : PSISection.cpp:195
//     - store_header / store_payload : PSISection.cpp:277/307
//
// C++ の DataBuffer 継承は Vec<u8> で代替する。
// PSISectionHandler (コールバック) は Rust の FnMut で代替する。

use libisdb_crc::crc32_mpeg2;
use libisdb_ts_packet::TsPacket;

/// PSI ヘッダ。PSISection.hpp:64。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PsiHeader {
    pub table_id: u8,
    pub section_syntax_indicator: bool,
    pub private_indicator: bool,
    pub section_length: u16,
    pub table_id_extension: u16,
    pub version_number: u8,
    pub current_next_indicator: bool,
    pub section_number: u8,
    pub last_section_number: u8,
}

/// PSI セクション。PSISection.cpp:38。
#[derive(Debug, Clone, Default)]
pub struct PsiSection {
    /// 受信した生データ (tag + length + payload)。
    pub data: Vec<u8>,
    header: PsiHeader,
}

impl PartialEq for PsiSection {
    fn eq(&self, other: &Self) -> bool {
        self.data == other.data
    }
}

impl Eq for PsiSection {}

impl PsiSection {
    pub fn new() -> Self {
        Self::default()
    }

    /// ヘッダを解析する。PSISection.cpp:71。
    ///
    /// `is_extended=true` のとき 8 バイトヘッダ(拡張セクション)を期待する。
    pub fn parse_header(&mut self, is_extended: bool, ignore_section_number: bool) -> bool {
        let header_size = if is_extended { 8 } else { 3 };
        if self.data.len() < header_size {
            return false;
        }

        let d = &self.data;
        self.header.table_id                = d[0];
        self.header.section_syntax_indicator= (d[1] & 0x80) != 0;
        self.header.private_indicator       = (d[1] & 0x40) != 0;
        self.header.section_length          = (((d[1] & 0x0F) as u16) << 8) | d[2] as u16;

        if self.header.section_syntax_indicator && is_extended {
            self.header.table_id_extension     = ((d[3] as u16) << 8) | d[4] as u16;
            self.header.version_number         = (d[5] & 0x3E) >> 1;
            self.header.current_next_indicator = (d[5] & 0x01) != 0;
            self.header.section_number         = d[6];
            self.header.last_section_number    = d[7];
        }

        if self.header.table_id == 0xFF {
            return false;
        }
        if (d[1] & 0x30) != 0x30 {
            return false;
        }
        if self.header.section_length > 4093 {
            return false;
        }
        if self.header.section_syntax_indicator != is_extended {
            return false;
        }

        if self.header.section_syntax_indicator {
            if (d[5] & 0xC0) != 0xC0 {
                return false;
            }
            if !ignore_section_number
                && self.header.section_number > self.header.last_section_number
            {
                return false;
            }
            if self.header.section_length < 9 {
                return false;
            }
        }

        true
    }

    /// リセットする。PSISection.cpp:124。
    pub fn reset(&mut self) {
        self.data.clear();
        self.header = PsiHeader::default();
    }

    /// ペイロード先頭スライスを返す。PSISection.cpp:131。
    pub fn get_payload_data(&self) -> Option<&[u8]> {
        let header_size = if self.header.section_syntax_indicator { 8 } else { 3 };
        if self.data.len() < header_size {
            return None;
        }
        Some(&self.data[header_size..])
    }

    /// ペイロードサイズを返す。PSISection.cpp:142。
    pub fn get_payload_size(&self) -> u16 {
        let header_size = if self.header.section_syntax_indicator { 8usize } else { 3 };
        if self.data.len() <= header_size {
            return 0;
        }
        let total = 3 + self.header.section_length as usize;
        let data_len = if self.data.len() < total { self.data.len() } else { total };
        if data_len <= header_size {
            return 0;
        }
        if self.header.section_syntax_indicator {
            if self.header.section_length < 9 {
                return 0;
            }
            self.header.section_length - 9
        } else {
            self.header.section_length
        }
    }

    pub fn get_table_id(&self) -> u8 { self.header.table_id }
    pub fn is_extended_section(&self) -> bool { self.header.section_syntax_indicator }
    pub fn get_private_indicator(&self) -> bool { self.header.private_indicator }
    pub fn get_section_length(&self) -> u16 { self.header.section_length }
    pub fn get_table_id_extension(&self) -> u16 { self.header.table_id_extension }
    pub fn get_version_number(&self) -> u8 { self.header.version_number }
    pub fn get_current_next_indicator(&self) -> bool { self.header.current_next_indicator }
    pub fn get_section_number(&self) -> u8 { self.header.section_number }
    pub fn get_last_section_number(&self) -> u8 { self.header.last_section_number }
    pub fn get_size(&self) -> usize { self.data.len() }

    /// 生データを追記する。
    pub fn add_data(&mut self, data: &[u8]) {
        self.data.extend_from_slice(data);
    }
}

/// PSI セクションパーサー。PSISection.cpp:181。
///
/// `FnMut` コールバックで完成したセクションを受け取る。
pub struct PsiSectionParser {
    is_extended: bool,
    ignore_section_number: bool,

    section: PsiSection,
    is_payload_storing: bool,
    store_size: u16,
    crc_error_count: u64,
}

impl PsiSectionParser {
    /// 新規作成。PSISection.cpp:181。
    pub fn new(is_extended: bool, ignore_section_number: bool) -> Self {
        Self {
            is_extended,
            ignore_section_number,
            section: PsiSection::new(),
            is_payload_storing: false,
            store_size: 0,
            crc_error_count: 0,
        }
    }

    /// リセットする。PSISection.cpp:256。
    pub fn reset(&mut self) {
        self.is_payload_storing = false;
        self.store_size = 0;
        self.crc_error_count = 0;
        self.section.reset();
    }

    /// CRC エラー数を返す。
    pub fn get_crc_error_count(&self) -> u64 {
        self.crc_error_count
    }

    /// TS パケットを処理する。PSISection.cpp:195。
    ///
    /// セクションが完成するたびに `handler` を呼び出す。
    pub fn store_packet<F>(&mut self, pkt: &TsPacket, handler: &mut F)
    where
        F: FnMut(&PsiSection),
    {
        let payload = match pkt.get_payload_data() {
            Some(p) => p,
            None => return,
        };
        let payload_size = pkt.get_payload_size() as usize;
        if payload_size == 0 {
            return;
        }
        let payload = &payload[..payload_size];

        let mut pos: usize;
        let mut size: usize;

        if pkt.get_payload_unit_start_indicator() {
            let unit_start_pos = payload[0] as usize + 1;
            if unit_start_pos >= payload_size {
                return;
            }

            if unit_start_pos > 1 {
                pos = 1;
                size = unit_start_pos - pos;
                if self.is_payload_storing {
                    self.store_payload(&payload[pos..], &mut size, handler);
                } else if self.section.get_size() > 0 {
                    if self.store_header(&payload[pos..], &mut size) {
                        pos += size;
                        size = unit_start_pos - pos;
                        self.store_payload(&payload[pos..], &mut size, handler);
                    }
                }
            }

            self.section.reset();
            self.is_payload_storing = false;

            pos = unit_start_pos;
            while pos < payload_size {
                size = payload_size - pos;
                if !self.is_payload_storing {
                    if !self.store_header(&payload[pos..], &mut size) {
                        break;
                    }
                    pos += size;
                    size = payload_size - pos;
                }
                self.store_payload(&payload[pos..], &mut size, handler);
                pos += size;
                if pos >= payload_size || payload[pos] == 0xFF {
                    break;
                }
            }
        } else {
            pos = 0;
            size = payload_size;
            if !self.is_payload_storing {
                if self.section.get_size() == 0 {
                    return;
                }
                if !self.store_header(&payload[pos..], &mut size) {
                    return;
                }
                pos += size;
                size = payload_size - pos;
            }
            self.store_payload(&payload[pos..], &mut size, handler);
        }
    }

    /// ヘッダをストアする。PSISection.cpp:277。
    fn store_header(&mut self, data: &[u8], remain: &mut usize) -> bool {
        if self.is_payload_storing {
            *remain = 0;
            return false;
        }

        let header_size: usize = if self.is_extended { 8 } else { 3 };
        let header_remain = header_size.saturating_sub(self.section.get_size());

        if header_remain > *remain {
            self.section.add_data(&data[..*remain]);
            return false;
        }

        self.section.add_data(&data[..header_remain]);
        *remain = header_remain;

        if self.section.parse_header(self.is_extended, self.ignore_section_number) {
            self.store_size = 3 + self.section.get_section_length();
            self.is_payload_storing = true;
            true
        } else {
            self.section.reset();
            false
        }
    }

    /// ペイロードをストアする。PSISection.cpp:307。
    fn store_payload<F>(&mut self, data: &[u8], remain: &mut usize, handler: &mut F)
    where
        F: FnMut(&PsiSection),
    {
        if !self.is_payload_storing {
            *remain = 0;
            return;
        }

        let store_remain = (self.store_size as usize).saturating_sub(self.section.get_size());

        if store_remain > *remain {
            self.section.add_data(&data[..*remain]);
            return;
        }

        self.section.add_data(&data[..store_remain]);

        let crc_ok = crc32_mpeg2(&self.section.data, 0xFFFFFFFF) == 0;
        if crc_ok {
            handler(&self.section);
        } else {
            self.crc_error_count = self.crc_error_count.saturating_add(1);
        }

        self.section.reset();
        self.is_payload_storing = false;
        *remain = store_remain;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libisdb_ts_packet::{TsPacket, TS_PACKET_SIZE};

    fn make_ts_packet(pid: u16, pusi: bool, cc: u8, payload: &[u8]) -> TsPacket {
        let mut data = [0u8; TS_PACKET_SIZE];
        data[0] = 0x47;
        let pusi_bit: u8 = if pusi { 0x40 } else { 0 };
        data[1] = pusi_bit | ((pid >> 8) as u8 & 0x1F);
        data[2] = pid as u8;
        data[3] = (0x01u8 << 4) | (cc & 0x0F); // payload only
        // payload header: pointer_field = 0x00 (PUSI時), ペイロード本体
        let dst_start = 4;
        let copy_len = payload.len().min(TS_PACKET_SIZE - dst_start);
        data[dst_start..dst_start + copy_len].copy_from_slice(&payload[..copy_len]);
        let mut pkt = TsPacket::new(&data);
        pkt.parse_packet(None);
        pkt
    }

    fn build_pat_section() -> Vec<u8> {
        // PAT: table_id=0x00, SSI=1, PNI=0, section_length=13, table_id_ext=0x0001,
        //      version=0, cni=1, section_number=0, last_section_number=0
        // payload: program_number=0x0001, PID=0x0100
        // 合計 3+13=16 バイト (CRC 含む)
        let mut sec = vec![
            0x00u8,       // table_id=PAT
            0xB0, 0x0D,   // SSI=1, reserved, section_length=13
            0x00, 0x01,   // table_id_extension=0x0001
            0xC1,         // reserved, version=0, cni=1
            0x00,         // section_number=0
            0x00,         // last_section_number=0
            0x00, 0x01,   // program_number=0x0001
            0xE1, 0x00,   // reserved, PMT PID=0x0100
        ];
        // CRC32 MPEG-2 を付加
        let crc = crc32_mpeg2(&sec, 0xFFFFFFFF);
        sec.push(((crc >> 24) & 0xFF) as u8);
        sec.push(((crc >> 16) & 0xFF) as u8);
        sec.push(((crc >>  8) & 0xFF) as u8);
        sec.push((crc & 0xFF) as u8);
        sec
    }

    #[test]
    fn test_psi_header_parse_extended() {
        let mut sec = PsiSection::new();
        sec.data = build_pat_section();
        assert!(sec.parse_header(true, false));
        assert_eq!(sec.get_table_id(), 0x00);
        assert!(sec.is_extended_section());
        assert_eq!(sec.get_section_length(), 13);
        assert_eq!(sec.get_table_id_extension(), 0x0001);
        assert_eq!(sec.get_version_number(), 0);
        assert!(sec.get_current_next_indicator());
        assert_eq!(sec.get_section_number(), 0);
        assert_eq!(sec.get_last_section_number(), 0);
    }

    #[test]
    fn test_psi_header_invalid_table_id() {
        let mut sec = PsiSection::new();
        sec.data = vec![0xFF, 0xB0, 0x09, 0x00, 0x01, 0xC1, 0x00, 0x00];
        assert!(!sec.parse_header(true, false));
    }

    #[test]
    fn test_psi_header_invalid_reserved_bits() {
        let mut sec = PsiSection::new();
        // バイト[1] の reserved bits (0x30) が 0 になっている
        sec.data = vec![0x00, 0x80, 0x09, 0x00, 0x01, 0xC1, 0x00, 0x00];
        assert!(!sec.parse_header(true, false));
    }

    #[test]
    fn test_psi_header_section_number_check() {
        let mut sec = PsiSection::new();
        // section_number(0x02) > last_section_number(0x01) → invalid
        sec.data = vec![0x00, 0xB0, 0x09, 0x00, 0x01, 0xC1, 0x02, 0x01];
        assert!(!sec.parse_header(true, false));
    }

    #[test]
    fn test_psi_header_section_number_ignore() {
        let mut sec = PsiSection::new();
        sec.data = vec![0x00, 0xB0, 0x09, 0x00, 0x01, 0xC1, 0x02, 0x01];
        // ignore_section_number=true → OK
        assert!(sec.parse_header(true, true));
    }

    #[test]
    fn test_psi_payload_data() {
        let mut sec = PsiSection::new();
        sec.data = build_pat_section();
        sec.parse_header(true, false);
        let payload = sec.get_payload_data().unwrap();
        // 拡張セクション: 8バイトヘッダを除いたペイロード
        // PAT: program_number=0x0001, PID=0x0100, CRC=4バイト → 8バイト
        assert_eq!(payload.len(), 16 - 8); // 8 bytes
        assert_eq!(payload[0], 0x00);
        assert_eq!(payload[1], 0x01);
    }

    #[test]
    fn test_psi_payload_size() {
        let mut sec = PsiSection::new();
        sec.data = build_pat_section();
        sec.parse_header(true, false);
        // section_length=13, payload_size = 13 - 9 = 4 (CRC 含む)
        assert_eq!(sec.get_payload_size(), 4);
    }

    #[test]
    fn test_psi_reset() {
        let mut sec = PsiSection::new();
        sec.data = build_pat_section();
        sec.parse_header(true, false);
        sec.reset();
        assert_eq!(sec.get_size(), 0);
        assert_eq!(sec.get_table_id(), 0);
    }

    #[test]
    fn test_crc32_mpeg2_residue() {
        // CRC を付加したデータ全体に CRC を計算すると 0 になる
        let data = build_pat_section();
        assert_eq!(crc32_mpeg2(&data, 0xFFFFFFFF), 0);
    }

    #[test]
    fn test_parser_single_packet() {
        // PAT セクション(16バイト)を 1 パケット内に収めてパース
        let pat = build_pat_section();
        let mut payload = vec![0x00u8]; // pointer_field = 0
        payload.extend_from_slice(&pat);

        let pkt = make_ts_packet(0x0000, true, 0, &payload);

        let mut parser = PsiSectionParser::new(true, false);
        let mut received = Vec::new();
        parser.store_packet(&pkt, &mut |sec| {
            received.push(sec.data.clone());
        });

        assert_eq!(received.len(), 1);
        assert_eq!(received[0], pat);
        assert_eq!(parser.get_crc_error_count(), 0);
    }

    #[test]
    fn test_parser_bad_crc() {
        let mut pat = build_pat_section();
        // CRC を破壊する
        let last = pat.len() - 1;
        pat[last] ^= 0xFF;

        let mut payload = vec![0x00u8];
        payload.extend_from_slice(&pat);

        let pkt = make_ts_packet(0x0000, true, 0, &payload);

        let mut parser = PsiSectionParser::new(true, false);
        let mut received = Vec::new();
        parser.store_packet(&pkt, &mut |sec| {
            received.push(sec.data.clone());
        });

        assert_eq!(received.len(), 0);
        assert_eq!(parser.get_crc_error_count(), 1);
    }

    #[test]
    fn test_parser_reset() {
        let mut parser = PsiSectionParser::new(true, false);
        let pat = build_pat_section();
        let mut payload = vec![0x00u8];
        payload.extend_from_slice(&pat);
        let pkt = make_ts_packet(0x0000, true, 0, &payload);
        parser.store_packet(&pkt, &mut |_| {});
        parser.reset();
        assert_eq!(parser.get_crc_error_count(), 0);
        assert_eq!(parser.section.get_size(), 0);
    }

    #[test]
    fn test_parser_non_extended() {
        // section_syntax_indicator=0 (標準セクション)
        // table_id=0x72, reserved=0x30, section_length=0x001 → 1 バイトペイロード
        let mut sec = vec![
            0x72u8,       // table_id (DSM-CC)
            0x30, 0x05,   // SSI=0, reserved, section_length=5
            0x01, 0x02, 0x03, 0x04, 0x05, // payload
        ];
        // CRC なし(非拡張セクション)のダミー - ここではCRC残余=0が必要
        let crc = crc32_mpeg2(&sec, 0xFFFFFFFF);
        sec.push(((crc >> 24) & 0xFF) as u8);
        sec.push(((crc >> 16) & 0xFF) as u8);
        sec.push(((crc >>  8) & 0xFF) as u8);
        sec.push((crc & 0xFF) as u8);

        let mut psi = PsiSection::new();
        psi.data = sec;
        // is_extended=false で section_syntax_indicator=0 → valid
        let ok = psi.parse_header(false, false);
        assert!(ok, "non-extended section parse failed");
        assert!(!psi.is_extended_section());
        assert_eq!(psi.get_section_length(), 5);
    }
}
