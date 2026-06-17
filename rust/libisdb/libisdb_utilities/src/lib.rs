// LibISDB の Utilities.hpp + Utilities.cpp を Rust へ移植したもの。
//
// 移植対象:
//   - byte_swap16/32/64 : ByteSwap16/32/64 (バイトスワップ)
//   - rotate_left32/right32 : RotateLeft32/RotateRight32 (ローテート)
//   - load16_be/load24_be/load32_be : Load16/Load24/Load32 (ビッグエンディアン読み出し)
//   - store16_be/store24_be/store32_be : Store16/Store24/Store32
//   - make_bcd : MakeBCD:163 (値 → BCD バイト)
//   - get_bcd_byte : GetBCD(uint8_t):168 (BCD バイト → 値)
//   - get_bcd : GetBCD(uint8_t*,size_t):35 (複数ニブル BCD → 値)
//   - round_off/round_up/round_down : 四捨五入・切り上げ・切り捨て

// --- バイトスワップ ---

/// 16 ビットバイトスワップ。Utilities.hpp:56。
#[inline]
pub const fn byte_swap16(v: u16) -> u16 {
    v.swap_bytes()
}

/// 32 ビットバイトスワップ。Utilities.hpp:60。
#[inline]
pub const fn byte_swap32(v: u32) -> u32 {
    v.swap_bytes()
}

/// 64 ビットバイトスワップ。Utilities.hpp:68。
#[inline]
pub const fn byte_swap64(v: u64) -> u64 {
    v.swap_bytes()
}

// --- ローテート ---

/// 32 ビット左ローテート。Utilities.hpp:84。
#[inline]
pub const fn rotate_left32(v: u32, shift: u32) -> u32 {
    v.rotate_left(shift)
}

/// 32 ビット右ローテート。Utilities.hpp:89。
#[inline]
pub const fn rotate_right32(v: u32, shift: u32) -> u32 {
    v.rotate_right(shift)
}

// --- ビッグエンディアン読み出し ---

/// 2 バイトをビッグエンディアンで読む。Utilities.hpp:96。
#[inline]
pub fn load16_be(p: &[u8]) -> u16 {
    u16::from_be_bytes(p[..2].try_into().unwrap())
}

/// 3 バイトをビッグエンディアンで読む。Utilities.hpp:106。
#[inline]
pub fn load24_be(p: &[u8]) -> u32 {
    ((p[0] as u32) << 16) | ((p[1] as u32) << 8) | (p[2] as u32)
}

/// 4 バイトをビッグエンディアンで読む。Utilities.hpp:114。
#[inline]
pub fn load32_be(p: &[u8]) -> u32 {
    u32::from_be_bytes(p[..4].try_into().unwrap())
}

// --- ビッグエンディアン書き込み ---

/// 2 バイトをビッグエンディアンで書く。Utilities.hpp:127。
#[inline]
pub fn store16_be(p: &mut [u8], v: u16) {
    p[..2].copy_from_slice(&v.to_be_bytes());
}

/// 3 バイトをビッグエンディアンで書く。Utilities.hpp:138。
#[inline]
pub fn store24_be(p: &mut [u8], v: u32) {
    p[0] = ((v >> 16) & 0xFF) as u8;
    p[1] = ((v >>  8) & 0xFF) as u8;
    p[2] = (v        & 0xFF) as u8;
}

/// 4 バイトをビッグエンディアンで書く。Utilities.hpp:146。
#[inline]
pub fn store32_be(p: &mut [u8], v: u32) {
    p[..4].copy_from_slice(&v.to_be_bytes());
}

// --- BCD ---

/// 値を BCD バイト (1 バイト = 2 ニブル) に変換する。Utilities.hpp:163。
#[inline]
pub const fn make_bcd(value: u8) -> u8 {
    ((value / 10) << 4) | (value % 10)
}

/// BCD バイトを値に変換する(1 バイト)。Utilities.hpp:168。
#[inline]
pub const fn get_bcd_byte(value: u8) -> u8 {
    ((value >> 4) * 10) + (value & 0x0F)
}

/// 複数ニブルの BCD データを u32 値に変換する。Utilities.cpp:35。
/// `nibble_length` はニブル(半バイト)の個数。
pub fn get_bcd(data: &[u8], nibble_length: usize) -> u32 {
    let mut result: u32 = 0;
    let bytes = nibble_length / 2;

    for i in 0..bytes {
        result *= 100;
        result += get_bcd_byte(data[i]) as u32;
    }

    if nibble_length & 1 != 0 {
        result *= 10;
        result += (data[nibble_length / 2] >> 4) as u32;
    }

    result
}

// --- 丸め ---

/// 四捨五入する。Utilities.hpp:159。
#[inline]
pub const fn round_off(v: u32, r: u32) -> u32 {
    (v + (r / 2)) / r * r
}

/// 切り上げる。Utilities.hpp:160。
#[inline]
pub const fn round_up(v: u32, r: u32) -> u32 {
    (v + (r - 1)) / r * r
}

/// 切り捨てる。Utilities.hpp:161。
#[inline]
pub const fn round_down(v: u32, r: u32) -> u32 {
    v / r * r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_byte_swap16() {
        assert_eq!(byte_swap16(0x1234), 0x3412);
        assert_eq!(byte_swap16(0x0100), 0x0001);
    }

    #[test]
    fn test_byte_swap32() {
        assert_eq!(byte_swap32(0x12345678), 0x78563412);
        assert_eq!(byte_swap32(0x01000000), 0x00000001);
    }

    #[test]
    fn test_byte_swap64() {
        assert_eq!(byte_swap64(0x0102030405060708), 0x0807060504030201);
    }

    #[test]
    fn test_rotate_left32() {
        assert_eq!(rotate_left32(0x80000000, 1), 0x00000001);
        assert_eq!(rotate_left32(0x00000001, 4), 0x00000010);
    }

    #[test]
    fn test_rotate_right32() {
        assert_eq!(rotate_right32(0x00000001, 1), 0x80000000);
        assert_eq!(rotate_right32(0x00000010, 4), 0x00000001);
    }

    #[test]
    fn test_load16_be() {
        let data = [0xAB, 0xCD];
        assert_eq!(load16_be(&data), 0xABCD);
    }

    #[test]
    fn test_load24_be() {
        let data = [0x01, 0x02, 0x03];
        assert_eq!(load24_be(&data), 0x010203);
    }

    #[test]
    fn test_load32_be() {
        let data = [0x12, 0x34, 0x56, 0x78];
        assert_eq!(load32_be(&data), 0x12345678);
    }

    #[test]
    fn test_store16_be() {
        let mut buf = [0u8; 2];
        store16_be(&mut buf, 0xABCD);
        assert_eq!(buf, [0xAB, 0xCD]);
    }

    #[test]
    fn test_store24_be() {
        let mut buf = [0u8; 3];
        store24_be(&mut buf, 0x010203);
        assert_eq!(buf, [0x01, 0x02, 0x03]);
    }

    #[test]
    fn test_store32_be() {
        let mut buf = [0u8; 4];
        store32_be(&mut buf, 0x12345678);
        assert_eq!(buf, [0x12, 0x34, 0x56, 0x78]);
    }

    #[test]
    fn test_load_store_roundtrip16() {
        let mut buf = [0u8; 2];
        store16_be(&mut buf, 0xDEAD);
        assert_eq!(load16_be(&buf), 0xDEAD);
    }

    #[test]
    fn test_load_store_roundtrip32() {
        let mut buf = [0u8; 4];
        store32_be(&mut buf, 0xDEADBEEF);
        assert_eq!(load32_be(&buf), 0xDEADBEEF);
    }

    #[test]
    fn test_make_bcd() {
        assert_eq!(make_bcd(0), 0x00);
        assert_eq!(make_bcd(9), 0x09);
        assert_eq!(make_bcd(10), 0x10);
        assert_eq!(make_bcd(59), 0x59);
        assert_eq!(make_bcd(99), 0x99);
    }

    #[test]
    fn test_get_bcd_byte() {
        assert_eq!(get_bcd_byte(0x00), 0);
        assert_eq!(get_bcd_byte(0x09), 9);
        assert_eq!(get_bcd_byte(0x10), 10);
        assert_eq!(get_bcd_byte(0x59), 59);
        assert_eq!(get_bcd_byte(0x99), 99);
    }

    #[test]
    fn test_make_get_bcd_roundtrip() {
        for v in 0u8..=99 {
            assert_eq!(get_bcd_byte(make_bcd(v)), v);
        }
    }

    #[test]
    fn test_get_bcd_multi_byte() {
        // 2 バイト(4 ニブル) = 1234
        let data = [0x12, 0x34];
        assert_eq!(get_bcd(&data, 4), 1234);
    }

    #[test]
    fn test_get_bcd_odd_nibbles() {
        // 3 ニブル: 0x12, 0x3? → 123
        let data = [0x12, 0x30];
        assert_eq!(get_bcd(&data, 3), 123);
    }

    #[test]
    fn test_get_bcd_single_nibble() {
        let data = [0x50];
        assert_eq!(get_bcd(&data, 1), 5);
    }

    #[test]
    fn test_round_off() {
        assert_eq!(round_off(14, 10), 10);
        assert_eq!(round_off(15, 10), 20);
        assert_eq!(round_off(25, 10), 30);
    }

    #[test]
    fn test_round_up() {
        assert_eq!(round_up(11, 10), 20);
        assert_eq!(round_up(10, 10), 10);
        assert_eq!(round_up(1, 10), 10);
    }

    #[test]
    fn test_round_down() {
        assert_eq!(round_down(19, 10), 10);
        assert_eq!(round_down(20, 10), 20);
        assert_eq!(round_down(9, 10), 0);
    }
}
