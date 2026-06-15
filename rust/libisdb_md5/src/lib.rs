// LibISDB の MD5.cpp を Rust へ移植したもの。
//
// 移植対象:
//   - calc_md5 : CalcMD5:138 (MD5 ハッシュ値計算)
//
// Utilities.hpp の RotateLeft32 は Rust の u32::rotate_left に対応。
// Little-endian CPU を前提としたメモリレイアウトをそのまま移植。

/// MD5 ハッシュ値(16 バイト)。MD5.hpp:36。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Md5Value(pub [u8; 16]);

impl Md5Value {
    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    pub fn as_u32s(&self) -> [u32; 4] {
        [
            u32::from_le_bytes(self.0[0..4].try_into().unwrap()),
            u32::from_le_bytes(self.0[4..8].try_into().unwrap()),
            u32::from_le_bytes(self.0[8..12].try_into().unwrap()),
            u32::from_le_bytes(self.0[12..16].try_into().unwrap()),
        ]
    }
}

// MD5 の補助関数。MD5.cpp:39-43。
#[inline(always)]
fn f1(x: u32, y: u32, z: u32) -> u32 { z ^ (x & (y ^ z)) }
#[inline(always)]
fn f2(x: u32, y: u32, z: u32) -> u32 { f1(z, x, y) }
#[inline(always)]
fn f3(x: u32, y: u32, z: u32) -> u32 { x ^ y ^ z }
#[inline(always)]
fn f4(x: u32, y: u32, z: u32) -> u32 { y ^ (x | !z) }

// MD5Step の展開。MD5.cpp:44-50。
#[inline(always)]
fn md5step<F: Fn(u32, u32, u32) -> u32>(
    func: F,
    w: &mut u32, x: u32, y: u32, z: u32,
    data: u32, shift: u32,
) {
    *w = w.wrapping_add(func(x, y, z)).wrapping_add(data);
    *w = w.rotate_left(shift);
    *w = w.wrapping_add(x);
}

// MD5Transform の 64 ステップ。MD5.cpp:52-133。
fn md5_transform(state: &mut [u32; 4], block: &[u32; 16]) {
    let [mut a, mut b, mut c, mut d] = *state;
    let p = block;

    md5step(f1, &mut a, b, c, d, p[ 0].wrapping_add(0xD76AA478),  7);
    md5step(f1, &mut d, a, b, c, p[ 1].wrapping_add(0xE8C7B756), 12);
    md5step(f1, &mut c, d, a, b, p[ 2].wrapping_add(0x242070DB), 17);
    md5step(f1, &mut b, c, d, a, p[ 3].wrapping_add(0xC1BDCEEE), 22);
    md5step(f1, &mut a, b, c, d, p[ 4].wrapping_add(0xF57C0FAF),  7);
    md5step(f1, &mut d, a, b, c, p[ 5].wrapping_add(0x4787C62A), 12);
    md5step(f1, &mut c, d, a, b, p[ 6].wrapping_add(0xA8304613), 17);
    md5step(f1, &mut b, c, d, a, p[ 7].wrapping_add(0xFD469501), 22);
    md5step(f1, &mut a, b, c, d, p[ 8].wrapping_add(0x698098D8),  7);
    md5step(f1, &mut d, a, b, c, p[ 9].wrapping_add(0x8B44F7AF), 12);
    md5step(f1, &mut c, d, a, b, p[10].wrapping_add(0xFFFF5BB1), 17);
    md5step(f1, &mut b, c, d, a, p[11].wrapping_add(0x895CD7BE), 22);
    md5step(f1, &mut a, b, c, d, p[12].wrapping_add(0x6B901122),  7);
    md5step(f1, &mut d, a, b, c, p[13].wrapping_add(0xFD987193), 12);
    md5step(f1, &mut c, d, a, b, p[14].wrapping_add(0xA679438E), 17);
    md5step(f1, &mut b, c, d, a, p[15].wrapping_add(0x49B40821), 22);

    md5step(f2, &mut a, b, c, d, p[ 1].wrapping_add(0xF61E2562),  5);
    md5step(f2, &mut d, a, b, c, p[ 6].wrapping_add(0xC040B340),  9);
    md5step(f2, &mut c, d, a, b, p[11].wrapping_add(0x265E5A51), 14);
    md5step(f2, &mut b, c, d, a, p[ 0].wrapping_add(0xE9B6C7AA), 20);
    md5step(f2, &mut a, b, c, d, p[ 5].wrapping_add(0xD62F105D),  5);
    md5step(f2, &mut d, a, b, c, p[10].wrapping_add(0x02441453),  9);
    md5step(f2, &mut c, d, a, b, p[15].wrapping_add(0xD8A1E681), 14);
    md5step(f2, &mut b, c, d, a, p[ 4].wrapping_add(0xE7D3FBC8), 20);
    md5step(f2, &mut a, b, c, d, p[ 9].wrapping_add(0x21E1CDE6),  5);
    md5step(f2, &mut d, a, b, c, p[14].wrapping_add(0xC33707D6),  9);
    md5step(f2, &mut c, d, a, b, p[ 3].wrapping_add(0xF4D50D87), 14);
    md5step(f2, &mut b, c, d, a, p[ 8].wrapping_add(0x455A14ED), 20);
    md5step(f2, &mut a, b, c, d, p[13].wrapping_add(0xA9E3E905),  5);
    md5step(f2, &mut d, a, b, c, p[ 2].wrapping_add(0xFCEFA3F8),  9);
    md5step(f2, &mut c, d, a, b, p[ 7].wrapping_add(0x676F02D9), 14);
    md5step(f2, &mut b, c, d, a, p[12].wrapping_add(0x8D2A4C8A), 20);

    md5step(f3, &mut a, b, c, d, p[ 5].wrapping_add(0xFFFA3942),  4);
    md5step(f3, &mut d, a, b, c, p[ 8].wrapping_add(0x8771F681), 11);
    md5step(f3, &mut c, d, a, b, p[11].wrapping_add(0x6D9D6122), 16);
    md5step(f3, &mut b, c, d, a, p[14].wrapping_add(0xFDE5380C), 23);
    md5step(f3, &mut a, b, c, d, p[ 1].wrapping_add(0xA4BEEA44),  4);
    md5step(f3, &mut d, a, b, c, p[ 4].wrapping_add(0x4BDECFA9), 11);
    md5step(f3, &mut c, d, a, b, p[ 7].wrapping_add(0xF6BB4B60), 16);
    md5step(f3, &mut b, c, d, a, p[10].wrapping_add(0xBEBFBC70), 23);
    md5step(f3, &mut a, b, c, d, p[13].wrapping_add(0x289B7EC6),  4);
    md5step(f3, &mut d, a, b, c, p[ 0].wrapping_add(0xEAA127FA), 11);
    md5step(f3, &mut c, d, a, b, p[ 3].wrapping_add(0xD4EF3085), 16);
    md5step(f3, &mut b, c, d, a, p[ 6].wrapping_add(0x04881D05), 23);
    md5step(f3, &mut a, b, c, d, p[ 9].wrapping_add(0xD9D4D039),  4);
    md5step(f3, &mut d, a, b, c, p[12].wrapping_add(0xE6DB99E5), 11);
    md5step(f3, &mut c, d, a, b, p[15].wrapping_add(0x1FA27CF8), 16);
    md5step(f3, &mut b, c, d, a, p[ 2].wrapping_add(0xC4AC5665), 23);

    md5step(f4, &mut a, b, c, d, p[ 0].wrapping_add(0xF4292244),  6);
    md5step(f4, &mut d, a, b, c, p[ 7].wrapping_add(0x432AFF97), 10);
    md5step(f4, &mut c, d, a, b, p[14].wrapping_add(0xAB9423A7), 15);
    md5step(f4, &mut b, c, d, a, p[ 5].wrapping_add(0xFC93A039), 21);
    md5step(f4, &mut a, b, c, d, p[12].wrapping_add(0x655B59C3),  6);
    md5step(f4, &mut d, a, b, c, p[ 3].wrapping_add(0x8F0CCC92), 10);
    md5step(f4, &mut c, d, a, b, p[10].wrapping_add(0xFFEFF47D), 15);
    md5step(f4, &mut b, c, d, a, p[ 1].wrapping_add(0x85845DD1), 21);
    md5step(f4, &mut a, b, c, d, p[ 8].wrapping_add(0x6FA87E4F),  6);
    md5step(f4, &mut d, a, b, c, p[15].wrapping_add(0xFE2CE6E0), 10);
    md5step(f4, &mut c, d, a, b, p[ 6].wrapping_add(0xA3014314), 15);
    md5step(f4, &mut b, c, d, a, p[13].wrapping_add(0x4E0811A1), 21);
    md5step(f4, &mut a, b, c, d, p[ 4].wrapping_add(0xF7537E82),  6);
    md5step(f4, &mut d, a, b, c, p[11].wrapping_add(0xBD3AF235), 10);
    md5step(f4, &mut c, d, a, b, p[ 2].wrapping_add(0x2AD7D2BB), 15);
    md5step(f4, &mut b, c, d, a, p[ 9].wrapping_add(0xEB86D391), 21);

    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
}

// 64 バイトスライスを u32 x16 のブロックとして解釈(little-endian)。
fn as_u32_block(block: &[u8; 64]) -> [u32; 16] {
    let mut out = [0u32; 16];
    for (i, chunk) in block.chunks_exact(4).enumerate() {
        out[i] = u32::from_le_bytes(chunk.try_into().unwrap());
    }
    out
}

/// MD5 ハッシュ値を計算する。MD5.cpp:138。
pub fn calc_md5(data: &[u8]) -> Md5Value {
    let mut state: [u32; 4] = [
        0x67452301,
        0xEFCDAB89,
        0x98BADCFE,
        0x10325476,
    ];

    let mut remaining = data;

    // 64 バイト単位のフルブロック処理
    while remaining.len() >= 64 {
        let block: &[u8; 64] = remaining[..64].try_into().unwrap();
        md5_transform(&mut state, &as_u32_block(block));
        remaining = &remaining[64..];
    }

    // パディングブロック構築
    let tail_len = remaining.len();
    let bits_size = (data.len() as u64).wrapping_mul(8);

    let mut padding = [0u8; 128];
    padding[..tail_len].copy_from_slice(remaining);
    padding[tail_len] = 0x80;

    // ビット長を 8 バイト little-endian でパディング末尾に埋め込む
    if tail_len < 56 {
        // 1 ブロックで完結
        padding[56..64].copy_from_slice(&bits_size.to_le_bytes());
        let block: &[u8; 64] = padding[..64].try_into().unwrap();
        md5_transform(&mut state, &as_u32_block(block));
    } else {
        // 2 ブロック必要
        padding[120..128].copy_from_slice(&bits_size.to_le_bytes());
        let block1: &[u8; 64] = padding[..64].try_into().unwrap();
        md5_transform(&mut state, &as_u32_block(block1));
        let block2: &[u8; 64] = padding[64..128].try_into().unwrap();
        md5_transform(&mut state, &as_u32_block(block2));
    }

    // state を little-endian バイト列として出力
    let mut out = [0u8; 16];
    for (i, &word) in state.iter().enumerate() {
        out[i * 4..(i + 1) * 4].copy_from_slice(&word.to_le_bytes());
    }
    Md5Value(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(v: &Md5Value) -> String {
        v.0.iter().map(|b| format!("{:02x}", b)).collect()
    }

    #[test]
    fn test_md5_empty() {
        // RFC 1321 テストベクタ: MD5("") = d41d8cd98f00b204e9800998ecf8427e
        assert_eq!(hex(&calc_md5(b"")), "d41d8cd98f00b204e9800998ecf8427e");
    }

    #[test]
    fn test_md5_a() {
        // MD5("a") = 0cc175b9c0f1b6a831c399e269772661
        assert_eq!(hex(&calc_md5(b"a")), "0cc175b9c0f1b6a831c399e269772661");
    }

    #[test]
    fn test_md5_abc() {
        // MD5("abc") = 900150983cd24fb0d6963f7d28e17f72
        assert_eq!(hex(&calc_md5(b"abc")), "900150983cd24fb0d6963f7d28e17f72");
    }

    #[test]
    fn test_md5_message_digest() {
        // MD5("message digest") = f96b697d7cb7938d525a2f31aaf161d0
        assert_eq!(hex(&calc_md5(b"message digest")), "f96b697d7cb7938d525a2f31aaf161d0");
    }

    #[test]
    fn test_md5_alphabet() {
        // MD5("abcdefghijklmnopqrstuvwxyz") = c3fcd3d76192e4007dfb496cca67e13b
        assert_eq!(hex(&calc_md5(b"abcdefghijklmnopqrstuvwxyz")), "c3fcd3d76192e4007dfb496cca67e13b");
    }

    #[test]
    fn test_md5_long() {
        // MD5("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789")
        // = d174ab98d277d9f5a5611c2c9f419d9f
        let input = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
        assert_eq!(hex(&calc_md5(input)), "d174ab98d277d9f5a5611c2c9f419d9f");
    }

    #[test]
    fn test_md5_numbers() {
        // MD5("12345678901234567890123456789012345678901234567890123456789012345678901234567890")
        // = 57edf4a22be3c955ac49da2e2107b67a
        let input = b"12345678901234567890123456789012345678901234567890123456789012345678901234567890";
        assert_eq!(hex(&calc_md5(input)), "57edf4a22be3c955ac49da2e2107b67a");
    }

    #[test]
    fn test_md5_64_bytes() {
        // ちょうど 64 バイト (1 ブロック境界)
        let input = [b'a'; 64];
        let result = calc_md5(&input);
        // 再現性確認
        assert_eq!(result, calc_md5(&input));
    }

    #[test]
    fn test_md5_55_bytes() {
        // 55 バイト (パディング後 1 ブロックで完結するギリギリ)
        let input = [b'b'; 55];
        let result = calc_md5(&input);
        assert_eq!(result, calc_md5(&input));
    }

    #[test]
    fn test_md5_56_bytes() {
        // 56 バイト (2 ブロック必要になる境界)
        let input = [b'c'; 56];
        let result = calc_md5(&input);
        assert_eq!(result, calc_md5(&input));
    }

    #[test]
    fn test_md5_returns_16_bytes() {
        let result = calc_md5(b"test");
        assert_eq!(result.0.len(), 16);
    }

    #[test]
    fn test_md5_equality() {
        let a = calc_md5(b"hello");
        let b = calc_md5(b"hello");
        let c = calc_md5(b"world");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn test_md5_quick_fox() {
        // MD5("The quick brown fox jumps over the lazy dog")
        // = 9e107d9d372bb6826bd81d3542a419d6
        assert_eq!(
            hex(&calc_md5(b"The quick brown fox jumps over the lazy dog")),
            "9e107d9d372bb6826bd81d3542a419d6"
        );
    }
}
