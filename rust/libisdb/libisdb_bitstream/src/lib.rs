// LibISDB の BitstreamReader.cpp を Rust へ移植したもの。
//
// 移植対象:
//   - BitstreamReader : BitstreamReader.cpp:36 (ビット列読み込みクラス)
//
// MSB ファースト(MPEG スタイル)のビットストリームリーダー。
// get_ue_v / get_se_v は Exp-Golomb 符号(H.264 VLC)。

/// ビット列読み込み器。BitstreamReader.hpp:35。
pub struct BitstreamReader<'a> {
    bits: &'a [u8],
    bit_size: usize,
    bit_pos: usize,
    is_overrun: bool,
}

impl<'a> BitstreamReader<'a> {
    /// スライスから新規作成。BitstreamReader.cpp:36。
    pub fn new(bits: &'a [u8]) -> Self {
        Self {
            bit_size: bits.len() * 8,
            bits,
            bit_pos: 0,
            is_overrun: false,
        }
    }

    /// 現在のビット位置を返す。BitstreamReader.hpp:40。
    pub fn get_pos(&self) -> usize {
        self.bit_pos
    }

    /// オーバーランしているかどうか。BitstreamReader.hpp:46。
    pub fn is_overrun(&self) -> bool {
        self.is_overrun
    }

    /// `bits` ビット分読み取る(MSB ファースト)。BitstreamReader.cpp:45。
    pub fn get_bits(&mut self, bits: usize) -> u32 {
        if self.bit_size.wrapping_sub(self.bit_pos) < bits {
            self.mark_overrun();
            return 0;
        }

        let mut p_idx = self.bit_pos >> 3;
        let mut shift = 7i32 - (self.bit_pos & 7) as i32;
        let mut value: u32 = 0;
        let mut remaining = bits;

        self.bit_pos += bits;

        while remaining > 0 {
            remaining -= 1;
            value <<= 1;
            value |= ((self.bits[p_idx] >> shift as u32) & 0x01) as u32;
            shift -= 1;
            if shift < 0 {
                shift = 7;
                p_idx += 1;
            }
        }

        value
    }

    /// 1 ビットを bool として読み取る。BitstreamReader.cpp:72。
    pub fn get_flag(&mut self) -> bool {
        self.get_bits(1) != 0
    }

    /// Exp-Golomb 符号 ue(v) を読み取る。BitstreamReader.cpp:78。
    pub fn get_ue_v(&mut self) -> Option<u32> {
        let (length, info) = self.get_vlc_symbol()?;
        Some((1u32 << (length / 2)) + info - 1)
    }

    /// Exp-Golomb 符号 se(v) を読み取る。BitstreamReader.cpp:89。
    pub fn get_se_v(&mut self) -> Option<i32> {
        let (length, info) = self.get_vlc_symbol()?;
        let n = (1u32 << (length / 2)).wrapping_add(info).wrapping_sub(1);
        let value = ((n + 1) >> 1) as i32;
        if (n & 0x01) != 0 {
            Some(value)
        } else {
            Some(-value)
        }
    }

    /// `bits` ビットをスキップする。BitstreamReader.cpp:102。
    pub fn skip(&mut self, bits: usize) -> bool {
        if self.bit_size.wrapping_sub(self.bit_pos) < bits {
            self.mark_overrun();
            return false;
        }
        self.bit_pos += bits;
        true
    }

    // Exp-Golomb VLC 符号を読み取る。BitstreamReader.cpp:115。
    // 戻り値は (bit_count, info)。オーバーランは None。
    fn get_vlc_symbol(&mut self) -> Option<(u32, u32)> {
        let mut p_idx = self.bit_pos >> 3;
        let end_idx = self.bit_size >> 3;
        let mut shift = 7i32 - (self.bit_pos & 7) as i32;
        let mut bit_count: usize = 1;
        let mut length: usize = 0;

        while (self.bits[p_idx] >> shift as u32) & 0x01 == 0 {
            length += 1;
            bit_count += 1;
            shift -= 1;
            if shift < 0 {
                shift = 7;
                p_idx += 1;
                if p_idx == end_idx {
                    self.mark_overrun();
                    return None;
                }
            }
        }

        bit_count += length;
        if self.bit_size.wrapping_sub(self.bit_pos) < bit_count {
            self.mark_overrun();
            return None;
        }

        let mut info: u32 = 0;
        let mut rem = length;
        while rem > 0 {
            rem -= 1;
            shift -= 1;
            if shift < 0 {
                shift = 7;
                p_idx += 1;
            }
            info <<= 1;
            info |= ((self.bits[p_idx] >> shift as u32) & 0x01) as u32;
        }

        self.bit_pos += bit_count;
        Some((bit_count as u32, info))
    }

    fn mark_overrun(&mut self) {
        self.bit_pos = self.bit_size;
        self.is_overrun = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_bits_zero() {
        let data = [0x00u8];
        let mut r = BitstreamReader::new(&data);
        assert_eq!(r.get_bits(0), 0);
        assert_eq!(r.get_pos(), 0);
        assert!(!r.is_overrun());
    }

    #[test]
    fn test_get_bits_single_byte() {
        let data = [0xA5u8]; // 1010_0101
        let mut r = BitstreamReader::new(&data);
        assert_eq!(r.get_bits(4), 0b1010); // 上位 4 ビット
        assert_eq!(r.get_bits(4), 0b0101); // 下位 4 ビット
        assert!(!r.is_overrun());
    }

    #[test]
    fn test_get_bits_cross_byte() {
        // 0xFF 0x00 → bit列 1111_1111_0000_0000
        let data = [0xFFu8, 0x00u8];
        let mut r = BitstreamReader::new(&data);
        // 4 ビット読む → 0b1111
        assert_eq!(r.get_bits(4), 0b1111);
        // 次の 8 ビット(byte 境界跨ぎ) → 0b1111_0000
        assert_eq!(r.get_bits(8), 0b11110000);
    }

    #[test]
    fn test_get_flag() {
        let data = [0b10100000u8];
        let mut r = BitstreamReader::new(&data);
        assert!(r.get_flag());  // bit 7 = 1
        assert!(!r.get_flag()); // bit 6 = 0
        assert!(r.get_flag());  // bit 5 = 1
    }

    #[test]
    fn test_overrun() {
        let data = [0xFFu8];
        let mut r = BitstreamReader::new(&data);
        r.get_bits(8); // ぴったり消費
        assert!(!r.is_overrun());
        r.get_bits(1); // オーバーラン
        assert!(r.is_overrun());
        assert_eq!(r.get_pos(), 8); // bit_pos = bit_size
    }

    #[test]
    fn test_skip() {
        let data = [0xA5u8]; // 1010_0101
        let mut r = BitstreamReader::new(&data);
        assert!(r.skip(4));
        assert_eq!(r.get_pos(), 4);
        assert_eq!(r.get_bits(4), 0b0101);
    }

    #[test]
    fn test_skip_overrun() {
        let data = [0xFFu8];
        let mut r = BitstreamReader::new(&data);
        assert!(!r.skip(9)); // 9 ビット > 8 ビット
        assert!(r.is_overrun());
    }

    #[test]
    fn test_get_ue_v_zero() {
        // ue(0) = bit "1" → code_num 0
        let data = [0b10000000u8];
        let mut r = BitstreamReader::new(&data);
        assert_eq!(r.get_ue_v(), Some(0));
    }

    #[test]
    fn test_get_ue_v_one() {
        // ue(1) = bits "010" → code_num 1
        let data = [0b01000000u8];
        let mut r = BitstreamReader::new(&data);
        assert_eq!(r.get_ue_v(), Some(1));
    }

    #[test]
    fn test_get_ue_v_two() {
        // ue(2) = bits "011" → code_num 2
        let data = [0b01100000u8];
        let mut r = BitstreamReader::new(&data);
        assert_eq!(r.get_ue_v(), Some(2));
    }

    #[test]
    fn test_get_ue_v_three() {
        // ue(3) = bits "00100" → code_num 3
        let data = [0b00100000u8];
        let mut r = BitstreamReader::new(&data);
        assert_eq!(r.get_ue_v(), Some(3));
    }

    #[test]
    fn test_get_se_v_zero() {
        // se(0): ue code_num=0, n=0 → value = (0+1)>>1 = 0, bit=0 → -0 = 0
        let data = [0b10000000u8];
        let mut r = BitstreamReader::new(&data);
        assert_eq!(r.get_se_v(), Some(0));
    }

    #[test]
    fn test_get_se_v_pos1() {
        // se(1): ue code_num=1, n=1-1+1=1 → value=(1+1)>>1=1, bit=1 → 1
        let data = [0b01000000u8]; // "010"
        let mut r = BitstreamReader::new(&data);
        assert_eq!(r.get_se_v(), Some(1));
    }

    #[test]
    fn test_get_se_v_neg1() {
        // se(-1): "011" code_num=2, n=2 → value=1, bit=0 → -1
        let data = [0b01100000u8];
        let mut r = BitstreamReader::new(&data);
        assert_eq!(r.get_se_v(), Some(-1));
    }

    #[test]
    fn test_get_bits_all_ones() {
        let data = [0xFF, 0xFF];
        let mut r = BitstreamReader::new(&data);
        assert_eq!(r.get_bits(16), 0xFFFF);
        assert!(!r.is_overrun());
    }

    #[test]
    fn test_sequential_reads() {
        let data = [0b11001010u8, 0b00110101u8];
        let mut r = BitstreamReader::new(&data);
        assert_eq!(r.get_bits(2), 0b11);
        assert_eq!(r.get_bits(2), 0b00);
        assert_eq!(r.get_bits(4), 0b1010);
        assert_eq!(r.get_bits(4), 0b0011);
        assert_eq!(r.get_bits(4), 0b0101);
        assert!(!r.is_overrun());
    }
}
