// Rust port of LibISDB/MediaParsers/MPEGVideoParser.cpp + MPEG2VideoParser.cpp

// MPEGVideoParser.cpp:137-163
/// EBSP から emulation_prevention_three_byte (0x000003) を除去して RBSP を得る。
/// 不正なバイト列があれば None を返す。
pub fn ebsp_to_rbsp(data: &mut Vec<u8>) -> Option<usize> {
    let src_len = data.len();
    let mut dst = 0usize;
    let mut count = 0i32;

    let mut i = 0usize;
    while i < src_len {
        let byte = data[i];
        if count == 2 {
            if byte < 0x03 {
                return None;
            }
            if byte == 0x03 {
                if i < src_len - 1 && data[i + 1] > 0x03 {
                    return None;
                }
                if i == src_len - 1 {
                    break;
                }
                // C++: i++ then fall-through → write pData[i+1], reset count
                i += 1;
                count = 0;
                // fall through: write data[i] below
                let next = data[i];
                data[dst] = next;
                dst += 1;
                if next == 0x00 {
                    count += 1;
                }
                i += 1;
                continue;
            }
        }
        data[dst] = byte;
        dst += 1;
        if byte == 0x00 {
            count += 1;
        } else {
            count = 0;
        }
        i += 1;
    }
    data.truncate(dst);
    Some(dst)
}

// MPEG2VideoParser.hpp:79-88
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SequenceExtension {
    pub is_valid: bool,
    pub profile_and_level: u8,
    pub progressive: bool,
    pub chroma_format: u8,
    pub low_delay: bool,
    pub frame_rate_ext_n: u8,
    pub frame_rate_ext_d: u8,
}

// MPEG2VideoParser.hpp:90-102
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DisplayColor {
    pub color_primaries: u8,
    pub transfer_characteristics: u8,
    pub matrix_coefficients: u8,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DisplayExtension {
    pub is_valid: bool,
    pub video_format: u8,
    pub color_description: bool,
    pub color: DisplayColor,
    pub display_horizontal_size: u16,
    pub display_vertical_size: u16,
}

// MPEG2VideoParser.hpp:65-103
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SequenceHeader {
    pub horizontal_size: u16,
    pub vertical_size: u16,
    pub aspect_ratio_info: u8,
    pub frame_rate_code: u8,
    pub bit_rate: u32,
    pub marker_bit: bool,
    pub vbv_buffer_size: u32,
    pub constrained_parameters_flag: bool,
    pub load_intra_quantiser_matrix: bool,
    pub load_non_intra_quantiser_matrix: bool,
    pub ext_sequence: SequenceExtension,
    pub ext_display: DisplayExtension,
}

// MPEG2VideoParser.hpp:38-106 / cpp:43-155
#[derive(Debug, Clone, Default)]
pub struct Mpeg2Sequence {
    data: Vec<u8>,
    pub header: SequenceHeader,
}

impl Mpeg2Sequence {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_data(&self) -> &[u8] {
        &self.data
    }

    pub fn get_size(&self) -> usize {
        self.data.len()
    }

    pub fn clear_size(&mut self) {
        self.data.clear();
    }

    pub fn set_data(&mut self, bytes: &[u8]) {
        self.data.clear();
        self.data.extend_from_slice(bytes);
    }

    pub fn add_data(&mut self, bytes: &[u8]) -> usize {
        self.data.extend_from_slice(bytes);
        self.data.len()
    }

    pub fn trim_tail(&mut self, n: usize) {
        let len = self.data.len();
        if n <= len {
            self.data.truncate(len - n);
        } else {
            self.data.clear();
        }
    }

    // MPEG2VideoParser.cpp:43-147
    pub fn parse_header(&mut self) -> bool {
        let d = &self.data;
        let mut header_size: usize = 12;

        if d.len() < header_size {
            return false;
        }
        if d[0] != 0x00 || d[1] != 0x00 || d[2] != 0x01 || d[3] != 0xB3 {
            return false;
        }

        let mut h = SequenceHeader::default();

        h.horizontal_size = ((d[4] as u16) << 4) | ((d[5] as u16 & 0xF0) >> 4);
        h.vertical_size = ((d[5] as u16 & 0x0F) << 8) | d[6] as u16;
        h.aspect_ratio_info = (d[7] & 0xF0) >> 4;
        h.frame_rate_code = d[7] & 0x0F;
        h.bit_rate = ((d[8] as u32) << 10) | ((d[9] as u32) << 2) | ((d[10] as u32 & 0xC0) >> 6);
        h.marker_bit = (d[10] & 0x20) != 0;
        h.vbv_buffer_size = (((d[10] & 0x1F) as u32) << 5) | ((d[11] & 0xF8) as u32 >> 3);
        h.constrained_parameters_flag = (d[11] & 0x04) != 0;
        h.load_intra_quantiser_matrix = (d[11] & 0x02) != 0;
        h.load_non_intra_quantiser_matrix = (d[11] & 0x01) != 0;

        if h.load_intra_quantiser_matrix {
            header_size += 64;
            if d.len() < header_size {
                return false;
            }
        }
        if h.load_non_intra_quantiser_matrix {
            header_size += 64;
            if d.len() < header_size {
                return false;
            }
        }

        if h.horizontal_size == 0 || h.vertical_size == 0 {
            return false;
        }
        if h.aspect_ratio_info == 0 || h.aspect_ratio_info > 4 {
            return false;
        }
        if h.frame_rate_code == 0 || h.frame_rate_code > 8 {
            return false;
        }
        if !h.marker_bit {
            return false;
        }
        if h.constrained_parameters_flag {
            return false;
        }

        // 拡張ヘッダ検索 (MPEG2VideoParser.cpp:89-144)
        let scan_end = d.len().saturating_sub(1).min(header_size + 1024);
        let mut sync_state: u32 = 0xFFFF_FFFF;
        let mut i = header_size;
        while i < scan_end {
            sync_state = (sync_state << 8) | d[i] as u32;
            i += 1;
            if sync_state == 0x0000_01B5 {
                let ext_id = d[i] >> 4;
                match ext_id {
                    1 => {
                        // シーケンス拡張 48 ビット
                        if i + 6 > d.len() {
                            break;
                        }
                        h.ext_sequence.profile_and_level =
                            ((d[i] & 0x0F) << 4) | (d[i + 1] >> 4);
                        h.ext_sequence.progressive = (d[i + 1] & 0x08) != 0;
                        h.ext_sequence.chroma_format = (d[i + 1] & 0x06) >> 1;
                        let h_ext = (((d[i + 1] & 0x01) as u16) << 1)
                            | ((d[i + 2] & 0x80) as u16 >> 7);
                        h.horizontal_size |= h_ext << 12;
                        let v_ext = ((d[i + 2] & 0x60) as u16) >> 5;
                        h.vertical_size |= v_ext << 12;
                        h.bit_rate |= ((d[i + 2] & 0x1F) as u32) << 7
                            | ((d[i + 3] as u32) >> 1) << 18;
                        if (d[i + 3] & 0x01) == 0 {
                            break; // marker bit missing
                        }
                        h.vbv_buffer_size |= (d[i + 4] as u32) << 10;
                        h.ext_sequence.low_delay = (d[i + 5] & 0x80) != 0;
                        h.ext_sequence.frame_rate_ext_n = (d[i + 5] & 0x60) >> 5;
                        h.ext_sequence.frame_rate_ext_d = (d[i + 5] & 0x18) >> 3;
                        h.ext_sequence.is_valid = true;
                        i += 6;
                    }
                    2 => {
                        // ディスプレイ拡張 40 ビット (+24 ビット)
                        if i + 5 > d.len() {
                            break;
                        }
                        h.ext_display.video_format = (d[i] & 0x0E) >> 1;
                        h.ext_display.color_description = (d[i] & 0x01) != 0;
                        let mut j = i;
                        if h.ext_display.color_description {
                            if j + 5 + 3 > d.len() {
                                break;
                            }
                            h.ext_display.color.color_primaries = d[j + 1];
                            h.ext_display.color.transfer_characteristics = d[j + 2];
                            h.ext_display.color.matrix_coefficients = d[j + 3];
                            j += 3;
                        }
                        if (d[j + 2] & 0x02) == 0 {
                            break; // marker bit
                        }
                        if (d[j + 4] & 0x07) != 0 {
                            break; // marker bit
                        }
                        h.ext_display.display_horizontal_size =
                            ((d[j + 1] as u16) << 6) | ((d[j + 2] & 0xFC) as u16 >> 2);
                        h.ext_display.display_vertical_size =
                            (((d[j + 2] & 0x01) as u16) << 13)
                                | ((d[j + 3] as u16) << 5)
                                | ((d[j + 4] & 0xF8) as u16 >> 3);
                        h.ext_display.is_valid = true;
                        i = j + 5;
                    }
                    _ => {}
                }
            }
        }

        self.header = h;
        true
    }

    pub fn reset(&mut self) {
        self.data.clear();
        self.header = SequenceHeader::default();
    }

    // MPEG2VideoParser.cpp:158-182
    pub fn get_aspect_ratio(&self) -> Option<(u8, u8)> {
        match self.header.aspect_ratio_info {
            1 => Some((1, 1)),
            2 => Some((4, 3)),
            3 => Some((16, 9)),
            4 => Some((221, 100)),
            _ => None,
        }
    }

    // MPEG2VideoParser.cpp:185-207
    pub fn get_frame_rate(&self) -> Option<(u32, u32)> {
        const FRAME_RATE_LIST: [(u32, u32); 8] = [
            (24000, 1001),
            (24, 1),
            (25, 1),
            (30000, 1001),
            (30, 1),
            (50, 1),
            (60000, 1001),
            (60, 1),
        ];
        let code = self.header.frame_rate_code;
        if code == 0 || code > 8 {
            return None;
        }
        Some(FRAME_RATE_LIST[(code - 1) as usize])
    }
}

// MPEGVideoParser.cpp:36-131
#[derive(Debug)]
pub struct Mpeg2VideoParser {
    sync_state: u32,
    sequence: Mpeg2Sequence,
}

impl Mpeg2VideoParser {
    pub fn new() -> Self {
        Self {
            sync_state: 0xFFFF_FFFF,
            sequence: Mpeg2Sequence::new(),
        }
    }

    pub fn reset(&mut self) {
        self.sync_state = 0xFFFF_FFFF;
        self.sequence.reset();
    }

    // MPEG2VideoParser.cpp:218-221 + MPEGVideoParser.cpp:63-131
    pub fn store_es<F>(&mut self, data: &[u8], handler: &mut F) -> bool
    where
        F: FnMut(&Mpeg2Sequence),
    {
        const START_CODE: u32 = 0x0000_01B3;
        let mut found_sequence = false;
        let mut sync_state = self.sync_state;
        let size = data.len();
        let mut pos = 0usize;

        while pos < size {
            let remain = size - pos;
            let mut start = 0usize;

            while start < remain {
                sync_state = (sync_state << 8) | data[pos + start] as u32;
                start += 1;
                if sync_state == START_CODE {
                    break;
                }
            }

            if start < remain {
                // スタートコード発見
                if self.sequence.get_size() >= 4 {
                    if start > 4 {
                        let chunk = &data[pos..pos + start - 4];
                        self.sequence.add_data(chunk);
                    } else if start < 4 {
                        self.sequence.trim_tail(4 - start);
                    }

                    let mut seq = std::mem::take(&mut self.sequence);
                    if seq.parse_header() {
                        handler(&seq);
                    }
                    self.sequence = seq;
                    self.sequence.clear_size();
                    found_sequence = true;
                }

                let sc_bytes = [
                    (sync_state >> 24) as u8,
                    (sync_state >> 16) as u8,
                    (sync_state >> 8) as u8,
                    sync_state as u8,
                ];
                self.sequence.set_data(&sc_bytes);

                sync_state = 0xFFFF_FFFF;
                pos += start;
            } else {
                // スタートコード未発見: バッファに蓄積
                if self.sequence.get_size() >= 4 {
                    let added = self.sequence.add_data(&data[pos..]);
                    if added >= 0x100_0000 {
                        self.sequence.clear_size();
                    }
                }
                break;
            }
        }

        self.sync_state = sync_state;
        found_sequence
    }
}

impl Default for Mpeg2VideoParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- ebsp_to_rbsp ----

    #[test]
    fn test_ebsp_to_rbsp_no_emulation() {
        let mut data = vec![0x01u8, 0x02, 0x03, 0x04];
        let result = ebsp_to_rbsp(&mut data);
        assert_eq!(result, Some(4));
        assert_eq!(data, vec![0x01, 0x02, 0x03, 0x04]);
    }

    #[test]
    fn test_ebsp_to_rbsp_removes_emulation_byte() {
        // 0x00 0x00 0x03 0x00 → 0x00 0x00 0x00
        let mut data = vec![0x00u8, 0x00, 0x03, 0x00];
        let result = ebsp_to_rbsp(&mut data);
        assert_eq!(result, Some(3));
        assert_eq!(data, vec![0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_ebsp_to_rbsp_trailing_emulation_byte() {
        // 0x00 0x00 0x03 at end → 0x03 を削除して 0xAB 0x00 0x00
        let mut data = vec![0xABu8, 0x00, 0x00, 0x03];
        let result = ebsp_to_rbsp(&mut data);
        assert_eq!(result, Some(3));
        assert_eq!(data, vec![0xAB, 0x00, 0x00]);
    }

    #[test]
    fn test_ebsp_to_rbsp_invalid_byte_after_two_zeros() {
        // 0x00 0x00 0x01 は不正(0x01 < 0x03)
        let mut data = vec![0x00u8, 0x00, 0x01];
        let result = ebsp_to_rbsp(&mut data);
        assert_eq!(result, None);
    }

    #[test]
    fn test_ebsp_to_rbsp_invalid_byte_after_emulation() {
        // 0x00 0x00 0x03 0x04 → 0x04 > 0x03 は不正
        let mut data = vec![0x00u8, 0x00, 0x03, 0x04];
        let result = ebsp_to_rbsp(&mut data);
        assert_eq!(result, None);
    }

    #[test]
    fn test_ebsp_to_rbsp_multiple_emulations() {
        // [0x00,0x00,0x03,0x00,0x00,0x03,0x01]
        // 1st removal: 0x03 at i=2 → skip, write data[3]=0x00; 2nd: 0x03 at i=5 → skip, write data[6]=0x01
        // → [0x00,0x00,0x00,0x00,0x01]  size=5
        let mut data = vec![0x00u8, 0x00, 0x03, 0x00, 0x00, 0x03, 0x01];
        let result = ebsp_to_rbsp(&mut data);
        assert_eq!(result, Some(5));
        assert_eq!(data, vec![0x00, 0x00, 0x00, 0x00, 0x01]);
    }

    // ---- make_minimal_seq_header ----

    fn make_minimal_seq_header(aspect: u8, frame_rate: u8) -> Vec<u8> {
        // sequence_header: 0x000001B3, horizontal=1280, vertical=720
        // horizontal_size = (d[4]<<4) | (d[5]>>4) → d[4]=0x50, d[5]_upper=0x0 → 0x500=1280
        // vertical_size   = (d[5]&0x0F)<<8 | d[6] → d[5]_lower=0x2, d[6]=0xD0 → 0x2D0=720
        // marker bit = d[10] & 0x20
        let mut d = vec![0u8; 12];
        d[0] = 0x00; d[1] = 0x00; d[2] = 0x01; d[3] = 0xB3;
        d[4] = 0x50;
        d[5] = 0x02; // upper nibble 0 → horiz bits[3:0]=0, lower nibble 2 → vert bits[11:8]=2
        d[6] = 0xD0; // vert bits[7:0]
        d[7] = (aspect << 4) | (frame_rate & 0x0F);
        d[8] = 0x00; d[9] = 0x00;
        d[10] = 0x20; // marker bit set
        d[11] = 0x00; // constrained=0
        d
    }

    // ---- get_aspect_ratio ----

    #[test]
    fn test_get_aspect_ratio_square() {
        let mut seq = Mpeg2Sequence::new();
        seq.set_data(&make_minimal_seq_header(1, 4));
        assert!(seq.parse_header());
        assert_eq!(seq.get_aspect_ratio(), Some((1, 1)));
    }

    #[test]
    fn test_get_aspect_ratio_4_3() {
        let mut seq = Mpeg2Sequence::new();
        seq.set_data(&make_minimal_seq_header(2, 4));
        assert!(seq.parse_header());
        assert_eq!(seq.get_aspect_ratio(), Some((4, 3)));
    }

    #[test]
    fn test_get_aspect_ratio_16_9() {
        let mut seq = Mpeg2Sequence::new();
        seq.set_data(&make_minimal_seq_header(3, 4));
        assert!(seq.parse_header());
        assert_eq!(seq.get_aspect_ratio(), Some((16, 9)));
    }

    #[test]
    fn test_get_aspect_ratio_221_100() {
        let mut seq = Mpeg2Sequence::new();
        seq.set_data(&make_minimal_seq_header(4, 4));
        assert!(seq.parse_header());
        assert_eq!(seq.get_aspect_ratio(), Some((221, 100)));
    }

    #[test]
    fn test_get_aspect_ratio_invalid() {
        let mut seq = Mpeg2Sequence::new();
        seq.header.aspect_ratio_info = 0;
        assert_eq!(seq.get_aspect_ratio(), None);
    }

    // ---- get_frame_rate ----

    #[test]
    fn test_get_frame_rate_23_976() {
        let mut seq = Mpeg2Sequence::new();
        seq.set_data(&make_minimal_seq_header(3, 1));
        assert!(seq.parse_header());
        assert_eq!(seq.get_frame_rate(), Some((24000, 1001)));
    }

    #[test]
    fn test_get_frame_rate_25() {
        let mut seq = Mpeg2Sequence::new();
        seq.set_data(&make_minimal_seq_header(3, 3));
        assert!(seq.parse_header());
        assert_eq!(seq.get_frame_rate(), Some((25, 1)));
    }

    #[test]
    fn test_get_frame_rate_29_97() {
        let mut seq = Mpeg2Sequence::new();
        seq.set_data(&make_minimal_seq_header(3, 4));
        assert!(seq.parse_header());
        assert_eq!(seq.get_frame_rate(), Some((30000, 1001)));
    }

    #[test]
    fn test_get_frame_rate_59_94() {
        let mut seq = Mpeg2Sequence::new();
        seq.set_data(&make_minimal_seq_header(3, 7));
        assert!(seq.parse_header());
        assert_eq!(seq.get_frame_rate(), Some((60000, 1001)));
    }

    #[test]
    fn test_get_frame_rate_invalid_zero() {
        let mut seq = Mpeg2Sequence::new();
        seq.header.frame_rate_code = 0;
        assert_eq!(seq.get_frame_rate(), None);
    }

    #[test]
    fn test_get_frame_rate_invalid_nine() {
        let mut seq = Mpeg2Sequence::new();
        seq.header.frame_rate_code = 9;
        assert_eq!(seq.get_frame_rate(), None);
    }

    // ---- parse_header validation ----

    #[test]
    fn test_parse_header_too_short() {
        let mut seq = Mpeg2Sequence::new();
        seq.set_data(&[0x00, 0x00, 0x01, 0xB3, 0x50]);
        assert!(!seq.parse_header());
    }

    #[test]
    fn test_parse_header_wrong_start_code() {
        let mut seq = Mpeg2Sequence::new();
        let mut d = make_minimal_seq_header(3, 4);
        d[3] = 0xB0;
        seq.set_data(&d);
        assert!(!seq.parse_header());
    }

    #[test]
    fn test_parse_header_invalid_aspect_zero() {
        let mut seq = Mpeg2Sequence::new();
        seq.set_data(&make_minimal_seq_header(0, 4));
        assert!(!seq.parse_header());
    }

    #[test]
    fn test_parse_header_invalid_frame_rate_zero() {
        let mut seq = Mpeg2Sequence::new();
        seq.set_data(&make_minimal_seq_header(3, 0));
        assert!(!seq.parse_header());
    }

    #[test]
    fn test_parse_header_marker_bit_missing() {
        let mut seq = Mpeg2Sequence::new();
        let mut d = make_minimal_seq_header(3, 4);
        d[10] &= !0x20;
        seq.set_data(&d);
        assert!(!seq.parse_header());
    }

    #[test]
    fn test_parse_header_dimensions() {
        let mut seq = Mpeg2Sequence::new();
        seq.set_data(&make_minimal_seq_header(3, 4));
        assert!(seq.parse_header());
        assert_eq!(seq.header.horizontal_size, 1280);
        assert_eq!(seq.header.vertical_size, 720);
    }

    // ---- Mpeg2VideoParser::store_es ----
    // C++ の挙動: シーケンスは「次のスタートコード到着時」に出力される。
    // 1つだけの ES では蓄積され、2つ目が来たときに1つ目が出力される。

    #[test]
    fn test_store_es_single_not_flushed() {
        // 1回で1シーケンスだけ送ってもまだ出力されない(次のスタートコード待ち)
        let mut parser = Mpeg2VideoParser::new();
        let es = make_minimal_seq_header(3, 4);
        let mut called = 0u32;
        let found = parser.store_es(&es, &mut |_: &Mpeg2Sequence| called += 1);
        assert!(!found);
        assert_eq!(called, 0);
    }

    #[test]
    fn test_store_es_two_sequences_triggers_first() {
        // 2つ目のシーケンス開始で1つ目が出力される
        let mut parser = Mpeg2VideoParser::new();
        let mut es = make_minimal_seq_header(3, 4);
        es.extend_from_slice(&make_minimal_seq_header(2, 3));
        let mut called = 0u32;
        let mut last_aspect = 0u8;
        parser.store_es(&es, &mut |seq: &Mpeg2Sequence| {
            last_aspect = seq.header.aspect_ratio_info;
            called += 1;
        });
        // 最初のシーケンス(aspect=3)が出力される
        assert_eq!(called, 1);
        assert_eq!(last_aspect, 3);
    }

    #[test]
    fn test_store_es_three_sequences() {
        // 3つ連続→2つ出力
        let mut parser = Mpeg2VideoParser::new();
        let mut es = make_minimal_seq_header(3, 4);
        es.extend_from_slice(&make_minimal_seq_header(2, 3));
        es.extend_from_slice(&make_minimal_seq_header(1, 2));
        let mut called = 0u32;
        parser.store_es(&es, &mut |_: &Mpeg2Sequence| called += 1);
        assert_eq!(called, 2);
    }

    #[test]
    fn test_store_es_split_across_calls() {
        // 1つのシーケンスを2回に分けて送り、2つ目のシーケンスの開始で1つ目を出力
        let mut parser = Mpeg2VideoParser::new();
        let mut es = make_minimal_seq_header(3, 4);
        es.extend_from_slice(&make_minimal_seq_header(2, 3));
        let split = 6;
        let (first, second) = es.split_at(split);
        let mut called = 0u32;
        parser.store_es(first, &mut |_: &Mpeg2Sequence| called += 1);
        let found = parser.store_es(second, &mut |seq: &Mpeg2Sequence| {
            assert_eq!(seq.header.horizontal_size, 1280);
            called += 1;
        });
        assert!(found);
        assert_eq!(called, 1);
    }

    #[test]
    fn test_store_es_no_start_code() {
        let mut parser = Mpeg2VideoParser::new();
        let es = vec![0xFFu8, 0xFF, 0xFF, 0xFF];
        let mut called = 0u32;
        let found = parser.store_es(&es, &mut |_: &Mpeg2Sequence| called += 1);
        assert!(!found);
        assert_eq!(called, 0);
    }

    #[test]
    fn test_store_es_reset_clears_state() {
        // reset 後は蓄積が消え、改めて2つ送ると1つ出力
        let mut parser = Mpeg2VideoParser::new();
        let es = make_minimal_seq_header(3, 4);
        let mut called = 0u32;
        parser.store_es(&es, &mut |_: &Mpeg2Sequence| called += 1);
        assert_eq!(called, 0);
        parser.reset();
        let mut es2 = make_minimal_seq_header(3, 4);
        es2.extend_from_slice(&make_minimal_seq_header(2, 3));
        parser.store_es(&es2, &mut |_: &Mpeg2Sequence| called += 1);
        assert_eq!(called, 1);
    }
}
