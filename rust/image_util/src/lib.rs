/*
  TVTest
  Copyright(c) 2008-2020 DBCTRADO

  This program is free software; you can redistribute it and/or modify
  it under the terms of the GNU General Public License as published by
  the Free Software Foundation; either version 2 of the License, or
  (at your option) any later version.

  This program is distributed in the hope that it will be useful,
  but WITHOUT ANY WARRANTY; without even the implied warranty of
  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
  GNU General Public License for more details.

  You should have received a copy of the GNU General Public License
  along with this program; if not, write to the Free Software
  Foundation, Inc., 59 Temple Place, Suite 330, Boston, MA  02111-1307  USA
*/

//! TVTest の画像処理 (`src/Image.cpp` / `src/Image.h`) の純粋計算部分を Rust に移植したクレート。
//!
//! 移植対象:
//!   - [`dib_row_bytes`]        : `DIB_ROW_BYTES` (Image.h:42)
//!   - [`calc_dib_info_size`]   : `CalcDIBInfoSize` (Image.cpp:39)
//!   - [`calc_dib_bits_size`]   : `CalcDIBBitsSize` (Image.cpp:52)
//!   - [`calc_dib_size`]        : `CalcDIBSize` (Image.cpp:58)
//!   - [`crop_image`]           : `CropImage` (Image.cpp:64)
//!   - [`resize_image`]         : `ResizeImage` (Image.cpp:94)
//!
//! 対象外 (Win32 / DLL / ファイル I/O 依存のため):
//!   - `CImageCodec` (Image.cpp:194-311): `TVTest_Image.dll` の動的ロードと
//!     `SaveImage` / `LoadAribPngFromMemory` / `LoadAribPngFromFile` の呼び出し。
//!   - `GlobalAlloc` / `GlobalLock` によるメモリ管理 (`ResizeImage` 内)。
//!     Rust 版は `Vec<u8>` を保持する [`DibImage`] を返すことで代替する。
//!
//! `BITMAPINFOHEADER` / `RECT` は Win32 の単純な値型なので、windows クレートに依存せず
//! 本クレートに同等の構造体 ([`BitmapInfoHeader`] / [`Rect`]) を定義して計算を再現する。
//!
//! ## 原実装との差異 (安全性のための追加検証)
//!
//! 原実装は範囲チェックを行わず、クロップ範囲がはみ出す場合や
//! バッファ長が不足する場合は未定義動作(バッファ外アクセス)となる。
//! Rust 版ではこれらを事前検証して [`ImageError`] を返す。
//! 検証を通過した入力に対しては原実装とバイト単位で同一の結果を生成する
//! (原実装の癖もそのまま再現する。各関数のドキュメント参照)。

#![forbid(unsafe_code)]

// ---------------------------------------------------------------------------
// 定数・型定義
// ---------------------------------------------------------------------------

/// `BI_RGB` (無圧縮)。
pub const BI_RGB: u32 = 0;
/// `BI_BITFIELDS` (ビットフィールド指定)。
pub const BI_BITFIELDS: u32 = 3;

/// `sizeof(BITMAPINFOHEADER)` = 40。
pub const BITMAPINFOHEADER_SIZE: u32 = 40;
/// `sizeof(BITMAPV4HEADER)` = 108。
pub const BITMAPV4HEADER_SIZE: u32 = 108;
/// `sizeof(BITMAPV5HEADER)` = 124。
pub const BITMAPV5HEADER_SIZE: u32 = 124;

/// `sizeof(RGBQUAD)` = 4。
const RGBQUAD_SIZE: usize = 4;
/// `sizeof(DWORD)` = 4。
const DWORD_SIZE: usize = 4;

/// Win32 `BITMAPINFOHEADER` に対応する構造体。
///
/// `bi_height > 0` はボトムアップ DIB (バッファ先頭行 = 画像最下行)、
/// `bi_height < 0` はトップダウン DIB (バッファ先頭行 = 画像最上行) を表す。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BitmapInfoHeader {
    pub bi_size: u32,
    pub bi_width: i32,
    pub bi_height: i32,
    pub bi_planes: u16,
    pub bi_bit_count: u16,
    pub bi_compression: u32,
    pub bi_size_image: u32,
    pub bi_x_pels_per_meter: i32,
    pub bi_y_pels_per_meter: i32,
    pub bi_clr_used: u32,
    pub bi_clr_important: u32,
}

/// Win32 `RECT` に対応する構造体。座標系は画像上端原点 (top-down) の論理座標。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// [`resize_image`] の結果。原実装の `HGLOBAL` (BITMAPINFOHEADER + ピクセルデータの連続領域)
/// をヘッダとピクセル列に分けて保持する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DibImage {
    /// 出力 DIB のヘッダ (24bpp / BI_RGB / ボトムアップ)。
    pub header: BitmapInfoHeader,
    /// ピクセルデータ (行アライメント込み、ボトムアップ)。
    pub data: Vec<u8>,
}

/// 画像操作のエラー。
///
/// 原実装では `UnsupportedBitCount` に相当する場合のみ `nullptr` を返し、
/// 他は検証なし(未定義動作)だったが、Rust 版では安全のためエラーとして報告する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageError {
    /// ビット数が 24/32 以外 (原実装 ResizeImage (Image.cpp:98) が nullptr を返すケース)。
    UnsupportedBitCount,
    /// 出力サイズやクロップ範囲が不正 (原実装では未定義動作)。
    OutOfRange,
    /// 入力または出力バッファの長さが不足 (原実装では未定義動作)。
    BufferTooSmall,
}

impl std::fmt::Display for ImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImageError::UnsupportedBitCount => write!(f, "unsupported bit count (must be 24 or 32)"),
            ImageError::OutOfRange => write!(f, "size or crop rectangle out of range"),
            ImageError::BufferTooSmall => write!(f, "source or destination buffer too small"),
        }
    }
}

impl std::error::Error for ImageError {}

// ---------------------------------------------------------------------------
// DIB サイズ計算
// ---------------------------------------------------------------------------

/// DIB 1 行のバイト数 (4 バイト境界にアライメント)。
/// 原実装 `DIB_ROW_BYTES` (Image.h:42): `((width * bpp + 31) >> 5) << 2`。
///
/// `width` または `bit_count` が 0 以下の場合は 0 を返す
/// (原実装は int の算術シフトで不定な値になるが、実用上呼ばれない領域のため安全側に倒す)。
pub const fn dib_row_bytes(width: i32, bit_count: u16) -> usize {
    let bits = width as i64 * bit_count as i64;
    if bits <= 0 {
        return 0;
    }
    (((bits + 31) >> 5) << 2) as usize
}

/// BITMAPINFOHEADER + パレット(またはビットフィールド)のサイズ。
/// 原実装 `CalcDIBInfoSize` (Image.cpp:39)。
///
/// - `bi_bit_count <= 8` のとき `(1 << bi_bit_count) * sizeof(RGBQUAD)` を加算する。
///   原実装は `bi_clr_used` を参照しない(常にフルパレットサイズ)ことに注意。
/// - それ以外で `bi_compression == BI_BITFIELDS` のとき `3 * sizeof(DWORD)` を加算する。
pub fn calc_dib_info_size(bmih: &BitmapInfoHeader) -> usize {
    // 原実装 TVTEST_ASSERT (Image.cpp:42) と同等のデバッグ検証。
    debug_assert!(
        bmih.bi_size == BITMAPINFOHEADER_SIZE
            || bmih.bi_size == BITMAPV4HEADER_SIZE
            || bmih.bi_size == BITMAPV5HEADER_SIZE
    );

    let mut size = bmih.bi_size as usize;
    if bmih.bi_bit_count <= 8 {
        size += (1usize << bmih.bi_bit_count) * RGBQUAD_SIZE;
    } else if bmih.bi_compression == BI_BITFIELDS {
        size += 3 * DWORD_SIZE;
    }
    size
}

/// DIB ピクセルデータ全体のバイト数 (行バイト数 × 高さの絶対値)。
/// 原実装 `CalcDIBBitsSize` (Image.cpp:52)。
pub fn calc_dib_bits_size(bmih: &BitmapInfoHeader) -> usize {
    dib_row_bytes(bmih.bi_width, bmih.bi_bit_count) * bmih.bi_height.unsigned_abs() as usize
}

/// DIB 全体 (ヘッダ + パレット + ピクセルデータ) のバイト数。
/// 原実装 `CalcDIBSize` (Image.cpp:58)。
pub fn calc_dib_size(bmih: &BitmapInfoHeader) -> usize {
    calc_dib_info_size(bmih) + calc_dib_bits_size(bmih)
}

// ---------------------------------------------------------------------------
// 切り出し (クロップ)
// ---------------------------------------------------------------------------

/// 24bpp / 32bpp の DIB から矩形領域を 24bpp ボトムアップ DIB として切り出す。
/// 原実装 `CropImage` (Image.cpp:64)。
///
/// - `left` / `top` は画像上端原点の論理座標 (ソースがボトムアップでもトップダウンでも同じ)。
/// - 32bpp ソースの場合は B,G,R をコピーしアルファは捨てる (Image.cpp:80-87)。
/// - `dst_data` は `dib_row_bytes(width, 24) * height` バイト以上必要。
///   行末のパディングバイトには書き込まない (原実装と同じ)。
///
/// 原実装は範囲検証を行わない(はみ出しは未定義動作)が、Rust 版は
/// 範囲外なら [`ImageError::OutOfRange`]、バッファ不足なら
/// [`ImageError::BufferTooSmall`] を返す。
pub fn crop_image(
    src_header: &BitmapInfoHeader,
    src_data: &[u8],
    left: i32,
    top: i32,
    width: i32,
    height: i32,
    dst_data: &mut [u8],
) -> Result<(), ImageError> {
    if src_header.bi_bit_count != 24 && src_header.bi_bit_count != 32 {
        return Err(ImageError::UnsupportedBitCount);
    }
    let abs_height = src_header.bi_height.unsigned_abs() as i32;
    if width <= 0
        || height <= 0
        || left < 0
        || top < 0
        || left as i64 + width as i64 > src_header.bi_width as i64
        || top as i64 + height as i64 > abs_height as i64
    {
        return Err(ImageError::OutOfRange);
    }

    let src_row_bytes = dib_row_bytes(src_header.bi_width, src_header.bi_bit_count);
    let dst_row_bytes = dib_row_bytes(width, 24);
    if src_data.len() < src_row_bytes * abs_height as usize
        || dst_data.len() < dst_row_bytes * height as usize
    {
        return Err(ImageError::BufferTooSmall);
    }

    let bytes_per_pixel = (src_header.bi_bit_count / 8) as usize;
    let width = width as usize;

    for y in 0..height {
        // ソース行: ボトムアップならバッファ末尾側が画像上端 (Image.cpp:73-75)。
        let src_row = if src_header.bi_height > 0 {
            (src_header.bi_height - 1 - (top + y)) as usize
        } else {
            (top + y) as usize
        };
        let src_off = src_row * src_row_bytes + left as usize * bytes_per_pixel;
        // 出力はボトムアップ: 論理行 y はバッファ行 (height-1-y) (Image.cpp:70, 89)。
        let dst_off = (height - 1 - y) as usize * dst_row_bytes;

        if bytes_per_pixel == 3 {
            // 24bpp: 行を一括コピー (Image.cpp:78)。
            dst_data[dst_off..dst_off + width * 3]
                .copy_from_slice(&src_data[src_off..src_off + width * 3]);
        } else {
            // 32bpp: B,G,R をコピーしアルファを読み飛ばす (Image.cpp:80-87)。
            for x in 0..width {
                let s = src_off + x * 4;
                let d = dst_off + x * 3;
                dst_data[d..d + 3].copy_from_slice(&src_data[s..s + 3]);
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// リサイズ
// ---------------------------------------------------------------------------

/// 24bpp / 32bpp の DIB を 24bpp ボトムアップ DIB に変換しつつ拡縮する。
/// 原実装 `ResizeImage` (Image.cpp:94)。
///
/// - `src_rect` が `None` なら全体 (Image.cpp:126-130)。
/// - 出力サイズがソース矩形と同一なら [`crop_image`] による単純コピー (Image.cpp:131-135)。
/// - それ以外は 8bit 固定小数点の**バイリニア補間** (Image.cpp:137-186)。
///   中心合わせのマッピングで、端は `[0, (size-1)<<8]` にクランプされる。
///   整数比の縮小 (例: 2x2→1x1) では 2x2 近傍の重みが均等になり、単純平均と一致する。
///
/// 原実装の癖 (忠実に再現):
/// - ソース位置のオフセット調整は `SrcTop > 0` のときのみ行われる (Image.cpp:153-155)。
///   そのためボトムアップ画像で `top == 0` かつ矩形高さ < 画像高さの場合、
///   補間パスは画像**下端**の `SrcHeight` 行からサンプリングする。
/// - トップダウン (`bi_height < 0`) ソースの補間パスは出力が上下反転する
///   (等倍の `CropImage` パスは反転しない。原実装の潜在的な非一貫性)。
///
/// 戻り値の [`DibImage`] は 24bpp / `BI_RGB` / ボトムアップのヘッダとピクセル列
/// (Image.cpp:107-117 のヘッダ初期化と同一値)。
pub fn resize_image(
    src_header: &BitmapInfoHeader,
    src_data: &[u8],
    src_rect: Option<Rect>,
    width: i32,
    height: i32,
) -> Result<DibImage, ImageError> {
    // 原実装 Image.cpp:98-99: 24/32bpp 以外は nullptr。
    if src_header.bi_bit_count != 24 && src_header.bi_bit_count != 32 {
        return Err(ImageError::UnsupportedBitCount);
    }

    let abs_height = src_header.bi_height.unsigned_abs() as i32;
    // 原実装 Image.cpp:120-130。
    let (src_left, src_top, src_width, src_height) = match src_rect {
        Some(r) => (r.left, r.top, r.right - r.left, r.bottom - r.top),
        None => (0, 0, src_header.bi_width, abs_height),
    };

    if width <= 0
        || height <= 0
        || src_width <= 0
        || src_height <= 0
        || src_left < 0
        || src_top < 0
        || src_left as i64 + src_width as i64 > src_header.bi_width as i64
        || src_top as i64 + src_height as i64 > abs_height as i64
    {
        return Err(ImageError::OutOfRange);
    }

    let src_row_bytes = dib_row_bytes(src_header.bi_width, src_header.bi_bit_count);
    if src_data.len() < src_row_bytes * abs_height as usize {
        return Err(ImageError::BufferTooSmall);
    }

    // 出力ヘッダ初期化 (原実装 Image.cpp:107-117)。
    let dst_row_bytes = dib_row_bytes(width, 24);
    let header = BitmapInfoHeader {
        bi_size: BITMAPINFOHEADER_SIZE,
        bi_width: width,
        bi_height: height,
        bi_planes: 1,
        bi_bit_count: 24,
        bi_compression: BI_RGB,
        bi_size_image: 0,
        bi_x_pels_per_meter: 0,
        bi_y_pels_per_meter: 0,
        bi_clr_used: 0,
        bi_clr_important: 0,
    };
    let mut data = vec![0u8; dst_row_bytes * height as usize];

    // 等倍なら単純コピー (原実装 Image.cpp:131-135)。
    if src_width == width && src_height == height {
        crop_image(src_header, src_data, src_left, src_top, width, height, &mut data)?;
        return Ok(DibImage { header, data });
    }

    // バイリニア補間 (原実装 Image.cpp:137-186)。
    // マッピング計算は原実装の int と同値になる範囲で i64 を用いる
    // (巨大サイズでの int オーバーフロー = 原実装の未定義動作をパニックさせないため)。
    let src_planes = (src_header.bi_bit_count / 8) as usize;
    let src_x_center = (((src_width as i64) - 1) << 8) / 2;
    let src_y_center = (((src_height as i64) - 1) << 8) / 2;
    let dst_x_center = (((width as i64) - 1) << 8) / 2;
    let dst_y_center = (((height as i64) - 1) << 8) / 2;

    // 各出力 x に対応するソース x 位置 (8bit 固定小数点、原実装 Image.cpp:144-150)。
    let src_pos: Vec<i64> = (0..width as i64)
        .map(|x| {
            ((x << 8) - dst_x_center) * src_width as i64 / width as i64 + src_x_center
        })
        .map(|p| p.clamp(0, ((src_width as i64) - 1) << 8))
        .collect();

    // ソース先頭オフセット調整 (原実装 Image.cpp:153-155)。SrcTop > 0 のときのみ。
    let src_base = if src_top > 0 {
        (if src_header.bi_height > 0 {
            src_header.bi_height - (src_height + src_top)
        } else {
            src_top
        }) as usize
            * src_row_bytes
    } else {
        0
    };

    let mut q = 0usize; // 出力書き込み位置 (原実装 Image.cpp:156 の q)。
    let dst_pad_bytes = dst_row_bytes - width as usize * 3;

    for y in 0..height as i64 {
        let y1 = (((y << 8) - dst_y_center) * src_height as i64 / height as i64 + src_y_center)
            .clamp(0, ((src_height as i64) - 1) << 8);
        let dy2 = (y1 & 0xFF) as u32;
        let dy1 = 0x100 - dy2;
        let y_offset = if dy2 > 0 { src_row_bytes } else { 0 };
        let row_base = src_base + (y1 >> 8) as usize * src_row_bytes;

        for &x1 in &src_pos {
            let dx2 = (x1 & 0xFF) as u32;
            let dx1 = 0x100 - dx2;
            let p = row_base + ((x1 >> 8) as usize + src_left as usize) * src_planes;
            // 右端クランプ時 (dx2 == 0) は同一ピクセルを参照 (原実装 Image.cpp:172)。
            let p1 = p + ((dx2 as usize + 0xFF) >> 8) * src_planes;

            let mut b = (src_data[p] as u32 * dx1 + src_data[p1] as u32 * dx2) * dy1;
            let mut g = (src_data[p + 1] as u32 * dx1 + src_data[p1 + 1] as u32 * dx2) * dy1;
            let mut r = (src_data[p + 2] as u32 * dx1 + src_data[p1 + 2] as u32 * dx2) * dy1;
            let py = p + y_offset;
            let p1y = p1 + y_offset;
            b += (src_data[py] as u32 * dx1 + src_data[p1y] as u32 * dx2) * dy2;
            g += (src_data[py + 1] as u32 * dx1 + src_data[p1y + 1] as u32 * dx2) * dy2;
            r += (src_data[py + 2] as u32 * dx1 + src_data[p1y + 2] as u32 * dx2) * dy2;

            data[q] = (b >> 16) as u8;
            data[q + 1] = (g >> 16) as u8;
            data[q + 2] = (r >> 16) as u8;
            q += 3;
        }
        q += dst_pad_bytes;
    }

    Ok(DibImage { header, data })
}

// ---------------------------------------------------------------------------
// テスト
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn header(width: i32, height: i32, bit_count: u16) -> BitmapInfoHeader {
        BitmapInfoHeader {
            bi_size: BITMAPINFOHEADER_SIZE,
            bi_width: width,
            bi_height: height,
            bi_planes: 1,
            bi_bit_count: bit_count,
            bi_compression: BI_RGB,
            ..Default::default()
        }
    }

    /// 論理座標 (x, y: 上端原点) のピクセル値から DIB バッファを構築するヘルパ。
    /// `pixel(x, y)` は (B, G, R) を返す。32bpp の場合アルファは 0xEE 固定。
    fn build_dib(
        width: i32,
        height: i32,
        bit_count: u16,
        pixel: impl Fn(i32, i32) -> (u8, u8, u8),
    ) -> Vec<u8> {
        let abs_h = height.unsigned_abs() as i32;
        let row_bytes = dib_row_bytes(width, bit_count);
        let bpp = (bit_count / 8) as usize;
        let mut buf = vec![0u8; row_bytes * abs_h as usize];
        for y in 0..abs_h {
            let row = if height > 0 { (abs_h - 1 - y) as usize } else { y as usize };
            for x in 0..width {
                let (b, g, r) = pixel(x, y);
                let off = row * row_bytes + x as usize * bpp;
                buf[off] = b;
                buf[off + 1] = g;
                buf[off + 2] = r;
                if bpp == 4 {
                    buf[off + 3] = 0xEE;
                }
            }
        }
        buf
    }

    /// 24bpp ボトムアップ DIB バッファから論理座標 (x, y: 上端原点) のピクセルを取得。
    fn get_pixel24(data: &[u8], width: i32, height: i32, x: i32, y: i32) -> (u8, u8, u8) {
        let row_bytes = dib_row_bytes(width, 24);
        let off = (height - 1 - y) as usize * row_bytes + x as usize * 3;
        (data[off], data[off + 1], data[off + 2])
    }

    // -- dib_row_bytes (Image.h:42) ------------------------------------------

    #[test]
    fn row_bytes_1bpp() {
        assert_eq!(dib_row_bytes(1, 1), 4);
        assert_eq!(dib_row_bytes(32, 1), 4);
        assert_eq!(dib_row_bytes(33, 1), 8);
    }

    #[test]
    fn row_bytes_4bpp() {
        assert_eq!(dib_row_bytes(3, 4), 4);
        assert_eq!(dib_row_bytes(8, 4), 4);
        assert_eq!(dib_row_bytes(9, 4), 8);
    }

    #[test]
    fn row_bytes_8bpp() {
        assert_eq!(dib_row_bytes(4, 8), 4);
        assert_eq!(dib_row_bytes(5, 8), 8);
        assert_eq!(dib_row_bytes(7, 8), 8);
    }

    #[test]
    fn row_bytes_16bpp() {
        assert_eq!(dib_row_bytes(1, 16), 4);
        assert_eq!(dib_row_bytes(2, 16), 4);
        assert_eq!(dib_row_bytes(3, 16), 8);
    }

    #[test]
    fn row_bytes_24bpp_odd_width() {
        assert_eq!(dib_row_bytes(1, 24), 4);
        assert_eq!(dib_row_bytes(2, 24), 8);
        assert_eq!(dib_row_bytes(3, 24), 12); // ちょうど 9 バイト → 12
        assert_eq!(dib_row_bytes(5, 24), 16); // 15 バイト → 16
    }

    #[test]
    fn row_bytes_32bpp() {
        assert_eq!(dib_row_bytes(1, 32), 4);
        assert_eq!(dib_row_bytes(7, 32), 28); // 常に width*4 (パディング不要)
    }

    #[test]
    fn row_bytes_degenerate() {
        assert_eq!(dib_row_bytes(0, 24), 0);
        assert_eq!(dib_row_bytes(-1, 24), 0);
    }

    // -- calc_dib_info_size (Image.cpp:39) -----------------------------------

    #[test]
    fn info_size_palette_1bpp() {
        let h = header(10, 10, 1);
        assert_eq!(calc_dib_info_size(&h), 40 + 2 * 4);
    }

    #[test]
    fn info_size_palette_4bpp() {
        let h = header(10, 10, 4);
        assert_eq!(calc_dib_info_size(&h), 40 + 16 * 4);
    }

    #[test]
    fn info_size_palette_8bpp() {
        let h = header(10, 10, 8);
        assert_eq!(calc_dib_info_size(&h), 40 + 256 * 4);
    }

    #[test]
    fn info_size_ignores_clr_used() {
        // 原実装 (Image.cpp:44-45) は biClrUsed を参照せず常にフルパレットサイズ。
        let mut h = header(10, 10, 8);
        h.bi_clr_used = 16;
        assert_eq!(calc_dib_info_size(&h), 40 + 256 * 4);
    }

    #[test]
    fn info_size_16bpp_bitfields() {
        let mut h = header(10, 10, 16);
        h.bi_compression = BI_BITFIELDS;
        assert_eq!(calc_dib_info_size(&h), 40 + 3 * 4);
    }

    #[test]
    fn info_size_24bpp_no_palette() {
        let h = header(10, 10, 24);
        assert_eq!(calc_dib_info_size(&h), 40);
    }

    #[test]
    fn info_size_32bpp() {
        let h = header(10, 10, 32);
        assert_eq!(calc_dib_info_size(&h), 40);
        let mut h = h;
        h.bi_compression = BI_BITFIELDS;
        assert_eq!(calc_dib_info_size(&h), 40 + 12);
    }

    #[test]
    fn info_size_v4_v5_header() {
        let mut h = header(10, 10, 24);
        h.bi_size = BITMAPV4HEADER_SIZE;
        assert_eq!(calc_dib_info_size(&h), 108);
        h.bi_size = BITMAPV5HEADER_SIZE;
        assert_eq!(calc_dib_info_size(&h), 124);
    }

    // -- calc_dib_bits_size / calc_dib_size (Image.cpp:52, 58) ---------------

    #[test]
    fn bits_size_bottom_up() {
        // 3px * 24bpp = 9 バイト → 12 バイト/行、4 行。
        let h = header(3, 4, 24);
        assert_eq!(calc_dib_bits_size(&h), 12 * 4);
    }

    #[test]
    fn bits_size_top_down_uses_abs_height() {
        // 原実装 (Image.cpp:54) は std::abs(biHeight)。
        let h = header(3, -4, 24);
        assert_eq!(calc_dib_bits_size(&h), 12 * 4);
    }

    #[test]
    fn bits_size_odd_width_alignment() {
        let h = header(5, 2, 8); // 5 バイト → 8 バイト/行
        assert_eq!(calc_dib_bits_size(&h), 8 * 2);
    }

    #[test]
    fn dib_size_is_sum() {
        let h = header(5, 3, 8);
        assert_eq!(calc_dib_size(&h), calc_dib_info_size(&h) + calc_dib_bits_size(&h));
        assert_eq!(calc_dib_size(&h), (40 + 1024) + 8 * 3);
    }

    // -- crop_image (Image.cpp:64) -------------------------------------------

    /// テスト用: ピクセル (x, y) を一意な値 (B, G, R) = (x*16+y, x*16+y+1, x*16+y+2) にする。
    fn unique_pixel(x: i32, y: i32) -> (u8, u8, u8) {
        let v = (x * 16 + y) as u8;
        (v, v.wrapping_add(1), v.wrapping_add(2))
    }

    #[test]
    fn crop_24bpp_bottom_up() {
        let h = header(4, 4, 24);
        let src = build_dib(4, 4, 24, unique_pixel);
        let mut dst = vec![0u8; dib_row_bytes(2, 24) * 2];
        crop_image(&h, &src, 1, 1, 2, 2, &mut dst).unwrap();
        // 論理座標 (1,1)-(2,2) が切り出される。
        for y in 0..2 {
            for x in 0..2 {
                assert_eq!(
                    get_pixel24(&dst, 2, 2, x, y),
                    unique_pixel(x + 1, y + 1),
                    "at ({x}, {y})"
                );
            }
        }
    }

    #[test]
    fn crop_32bpp_drops_alpha() {
        let h = header(3, 3, 32);
        let src = build_dib(3, 3, 32, unique_pixel);
        let mut dst = vec![0u8; dib_row_bytes(3, 24) * 3];
        crop_image(&h, &src, 0, 0, 3, 3, &mut dst).unwrap();
        for y in 0..3 {
            for x in 0..3 {
                assert_eq!(get_pixel24(&dst, 3, 3, x, y), unique_pixel(x, y));
            }
        }
    }

    #[test]
    fn crop_top_down_source() {
        // トップダウンソース (bi_height < 0) でも論理座標で同じ結果になる (Image.cpp:74-75)。
        let h = header(4, -4, 24);
        let src = build_dib(4, -4, 24, unique_pixel);
        let mut dst = vec![0u8; dib_row_bytes(2, 24) * 2];
        crop_image(&h, &src, 2, 1, 2, 2, &mut dst).unwrap();
        for y in 0..2 {
            for x in 0..2 {
                assert_eq!(get_pixel24(&dst, 2, 2, x, y), unique_pixel(x + 2, y + 1));
            }
        }
    }

    #[test]
    fn crop_width_one() {
        let h = header(4, 4, 24);
        let src = build_dib(4, 4, 24, unique_pixel);
        let mut dst = vec![0u8; dib_row_bytes(1, 24) * 4];
        crop_image(&h, &src, 3, 0, 1, 4, &mut dst).unwrap();
        for y in 0..4 {
            assert_eq!(get_pixel24(&dst, 1, 4, 0, y), unique_pixel(3, y));
        }
    }

    #[test]
    fn crop_does_not_touch_padding() {
        // 幅 2 の出力行は 6 バイトデータ + 2 バイトパディング。パディングは書き込まれない。
        let h = header(4, 4, 24);
        let src = build_dib(4, 4, 24, unique_pixel);
        let mut dst = vec![0xAAu8; dib_row_bytes(2, 24) * 2];
        crop_image(&h, &src, 0, 0, 2, 2, &mut dst).unwrap();
        let row_bytes = dib_row_bytes(2, 24);
        for y in 0..2 {
            assert_eq!(dst[y * row_bytes + 6], 0xAA);
            assert_eq!(dst[y * row_bytes + 7], 0xAA);
        }
    }

    #[test]
    fn crop_out_of_range_is_error() {
        // 原実装では未定義動作(バッファ外読み取り)になるケース。Rust 版はエラー。
        let h = header(4, 4, 24);
        let src = build_dib(4, 4, 24, unique_pixel);
        let mut dst = vec![0u8; dib_row_bytes(2, 24) * 2];
        assert_eq!(crop_image(&h, &src, 3, 0, 2, 2, &mut dst), Err(ImageError::OutOfRange));
        assert_eq!(crop_image(&h, &src, 0, 3, 2, 2, &mut dst), Err(ImageError::OutOfRange));
        assert_eq!(crop_image(&h, &src, -1, 0, 2, 2, &mut dst), Err(ImageError::OutOfRange));
        assert_eq!(crop_image(&h, &src, 0, 0, 0, 2, &mut dst), Err(ImageError::OutOfRange));
    }

    #[test]
    fn crop_buffer_too_small_is_error() {
        let h = header(4, 4, 24);
        let src = build_dib(4, 4, 24, unique_pixel);
        let mut small = vec![0u8; 4];
        assert_eq!(crop_image(&h, &src, 0, 0, 2, 2, &mut small), Err(ImageError::BufferTooSmall));
        let short_src = vec![0u8; 8];
        let mut dst = vec![0u8; dib_row_bytes(2, 24) * 2];
        assert_eq!(
            crop_image(&h, &short_src, 0, 0, 2, 2, &mut dst),
            Err(ImageError::BufferTooSmall)
        );
    }

    #[test]
    fn crop_unsupported_bit_count() {
        let h = header(4, 4, 8);
        let src = vec![0u8; dib_row_bytes(4, 8) * 4];
        let mut dst = vec![0u8; dib_row_bytes(2, 24) * 2];
        assert_eq!(
            crop_image(&h, &src, 0, 0, 2, 2, &mut dst),
            Err(ImageError::UnsupportedBitCount)
        );
    }

    // -- resize_image (Image.cpp:94) -----------------------------------------

    #[test]
    fn resize_header_fields() {
        let h = header(2, 2, 24);
        let src = build_dib(2, 2, 24, |_, _| (0, 0, 0));
        let out = resize_image(&h, &src, None, 1, 1).unwrap();
        assert_eq!(
            out.header,
            BitmapInfoHeader {
                bi_size: BITMAPINFOHEADER_SIZE,
                bi_width: 1,
                bi_height: 1,
                bi_planes: 1,
                bi_bit_count: 24,
                bi_compression: BI_RGB,
                bi_size_image: 0,
                bi_x_pels_per_meter: 0,
                bi_y_pels_per_meter: 0,
                bi_clr_used: 0,
                bi_clr_important: 0,
            }
        );
        assert_eq!(out.data.len(), dib_row_bytes(1, 24));
    }

    #[test]
    fn resize_2x2_to_1x1_is_average() {
        // 2x2→1x1 では 4 ピクセルの均等平均 (床除算) になる。
        // B: (10+20+30+40)/4 = 25, G: (1+2+3+4)/4 = 2, R: (100+101+102+103)/4 = 101
        let h = header(2, 2, 24);
        let vals = [
            [(10u8, 1u8, 100u8), (20, 2, 101)], // y=0 (上)
            [(30, 3, 102), (40, 4, 103)],       // y=1 (下)
        ];
        let src = build_dib(2, 2, 24, |x, y| vals[y as usize][x as usize]);
        let out = resize_image(&h, &src, None, 1, 1).unwrap();
        assert_eq!(get_pixel24(&out.data, 1, 1, 0, 0), (25, 2, 101));
    }

    #[test]
    fn resize_2x2_to_1x1_floor_division() {
        // (0+0+0+1)*64*... = 端数は切り捨て ((b >> 16) 相当、Image.cpp:181)。
        let h = header(2, 2, 24);
        let src = build_dib(2, 2, 24, |x, y| if x == 1 && y == 1 { (1, 3, 255) } else { (0, 0, 0) });
        let out = resize_image(&h, &src, None, 1, 1).unwrap();
        // B: 1/4 → 0, G: 3/4 → 0, R: 255/4 = 63.75 → 63
        assert_eq!(get_pixel24(&out.data, 1, 1, 0, 0), (0, 0, 63));
    }

    #[test]
    fn resize_4x4_to_2x2_block_average() {
        // 4x4→2x2 は各 2x2 ブロックの均等平均になる (SrcPos = 128, 640 → dx2 = 128)。
        let h = header(4, 4, 24);
        let src = build_dib(4, 4, 24, |x, y| ((x * 4 + y * 40) as u8, 0, 0));
        let out = resize_image(&h, &src, None, 2, 2).unwrap();
        let avg = |xs: [i32; 2], ys: [i32; 2]| -> u8 {
            let mut sum = 0;
            for &y in &ys {
                for &x in &xs {
                    sum += x * 4 + y * 40;
                }
            }
            (sum / 4) as u8
        };
        assert_eq!(get_pixel24(&out.data, 2, 2, 0, 0).0, avg([0, 1], [0, 1]));
        assert_eq!(get_pixel24(&out.data, 2, 2, 1, 0).0, avg([2, 3], [0, 1]));
        assert_eq!(get_pixel24(&out.data, 2, 2, 0, 1).0, avg([0, 1], [2, 3]));
        assert_eq!(get_pixel24(&out.data, 2, 2, 1, 1).0, avg([2, 3], [2, 3]));
    }

    #[test]
    fn resize_32bpp_source() {
        // 32bpp ソースでも BGR のみ使用しアルファは無視される。
        let h = header(2, 2, 32);
        let src = build_dib(2, 2, 32, |x, y| ((x * 100 + y * 50) as u8, 8, 16));
        let out = resize_image(&h, &src, None, 1, 1).unwrap();
        // B: (0+100+50+150)/4 = 75
        assert_eq!(get_pixel24(&out.data, 1, 1, 0, 0), (75, 8, 16));
    }

    #[test]
    fn resize_same_size_goes_through_crop() {
        // 等倍は CropImage 経由の単純コピー (Image.cpp:131-135)。
        let h = header(4, 4, 24);
        let src = build_dib(4, 4, 24, unique_pixel);
        let out = resize_image(&h, &src, None, 4, 4).unwrap();
        for y in 0..4 {
            for x in 0..4 {
                assert_eq!(get_pixel24(&out.data, 4, 4, x, y), unique_pixel(x, y));
            }
        }
    }

    #[test]
    fn resize_rect_same_size_crops_subregion() {
        let h = header(4, 4, 24);
        let src = build_dib(4, 4, 24, unique_pixel);
        let rect = Rect { left: 1, top: 2, right: 3, bottom: 4 };
        let out = resize_image(&h, &src, Some(rect), 2, 2).unwrap();
        for y in 0..2 {
            for x in 0..2 {
                assert_eq!(get_pixel24(&out.data, 2, 2, x, y), unique_pixel(x + 1, y + 2));
            }
        }
    }

    #[test]
    fn resize_rect_scaled_bottom_right() {
        // 右下 2x2 (top > 0) を 1x1 に縮小 → その 4 ピクセルの平均。
        let h = header(4, 4, 24);
        let src = build_dib(4, 4, 24, |x, y| ((x * 10 + y * 40) as u8, 0, 0));
        let rect = Rect { left: 2, top: 2, right: 4, bottom: 4 };
        let out = resize_image(&h, &src, Some(rect), 1, 1).unwrap();
        // (2,2)=100, (3,2)=110, (2,3)=140, (3,3)=150 → 平均 125
        assert_eq!(get_pixel24(&out.data, 1, 1, 0, 0).0, 125);
    }

    #[test]
    fn resize_rect_top_zero_quirk_samples_bottom_rows() {
        // 原実装の癖 (Image.cpp:153): SrcTop == 0 だとオフセット調整されないため、
        // ボトムアップ画像では矩形 (top=0, 高さ 2) でも画像下端 2 行からサンプリングされる。
        let h = header(4, 4, 24);
        let src = build_dib(4, 4, 24, |x, y| ((x * 10 + y * 40) as u8, 0, 0));
        let rect = Rect { left: 0, top: 0, right: 2, bottom: 2 };
        let out = resize_image(&h, &src, Some(rect), 1, 1).unwrap();
        // 上端 2x2 の平均 (=25) ではなく、下端 2 行 (y=2,3) の左 2 列の平均:
        // (0,2)=80, (1,2)=90, (0,3)=120, (1,3)=130 → 105
        assert_eq!(get_pixel24(&out.data, 1, 1, 0, 0).0, 105);
    }

    #[test]
    fn resize_upscale_1x1_to_2x2() {
        // 1x1→2x2: SrcPos が全て 0 にクランプされ、全出力ピクセルが元の 1 ピクセルになる。
        let h = header(1, 1, 24);
        let src = build_dib(1, 1, 24, |_, _| (12, 34, 56));
        let out = resize_image(&h, &src, None, 2, 2).unwrap();
        for y in 0..2 {
            for x in 0..2 {
                assert_eq!(get_pixel24(&out.data, 2, 2, x, y), (12, 34, 56));
            }
        }
    }

    #[test]
    fn resize_width_one_output() {
        // 4x4→1x4 (高さ等倍): 各行で中央 2 列 (x=1,2) の均等平均になる。
        // SrcPos[0] = ((4-1)<<8)/2 = 384 → x1>>8 = 1, dx2 = 128。
        let h = header(4, 4, 24);
        let src = build_dib(4, 4, 24, |x, y| ((x * 20 + y) as u8, 0, 0));
        let out = resize_image(&h, &src, None, 1, 4).unwrap();
        for y in 0..4 {
            let expect = ((20 + y) + (40 + y)) / 2; // (x=1 + x=2) / 2
            assert_eq!(get_pixel24(&out.data, 1, 4, 0, y).0, expect as u8, "row {y}");
        }
    }

    #[test]
    fn resize_top_down_source_scaled_is_flipped() {
        // 原実装の癖: トップダウンソースの補間パスは出力が上下反転する
        // (等倍パスは CropImage が正しく反転処理するため一貫しない)。忠実に再現。
        let h = header(2, -4, 24);
        let src = build_dib(2, -4, 24, |_, y| ((y * 10) as u8, 0, 0));
        let out = resize_image(&h, &src, None, 1, 2).unwrap();
        // 出力の論理上段 = ソース下側 (y=2,3 の平均 = 25)、論理下段 = ソース上側 (y=0,1 の平均 = 5)。
        assert_eq!(get_pixel24(&out.data, 1, 2, 0, 0).0, 25);
        assert_eq!(get_pixel24(&out.data, 1, 2, 0, 1).0, 5);
    }

    #[test]
    fn resize_bottom_up_source_scaled_is_not_flipped() {
        // 対照: ボトムアップソースでは反転しない。
        let h = header(2, 4, 24);
        let src = build_dib(2, 4, 24, |_, y| ((y * 10) as u8, 0, 0));
        let out = resize_image(&h, &src, None, 1, 2).unwrap();
        assert_eq!(get_pixel24(&out.data, 1, 2, 0, 0).0, 5); // 上段 = ソース上側
        assert_eq!(get_pixel24(&out.data, 1, 2, 0, 1).0, 25); // 下段 = ソース下側
    }

    #[test]
    fn resize_output_padding_is_zero() {
        // 出力バッファは vec![0] で確保されるため行末パディングは 0。
        let h = header(4, 4, 24);
        let src = build_dib(4, 4, 24, |_, _| (255, 255, 255));
        let out = resize_image(&h, &src, None, 2, 2).unwrap();
        let row_bytes = dib_row_bytes(2, 24); // 8 バイト (6 データ + 2 パディング)
        for y in 0..2 {
            assert_eq!(out.data[y * row_bytes + 6], 0);
            assert_eq!(out.data[y * row_bytes + 7], 0);
        }
    }

    #[test]
    fn resize_unsupported_bit_count_is_error() {
        // 原実装 (Image.cpp:98-99) は nullptr を返す。
        let h = header(4, 4, 8);
        let src = vec![0u8; dib_row_bytes(4, 8) * 4];
        assert_eq!(resize_image(&h, &src, None, 2, 2), Err(ImageError::UnsupportedBitCount));
        let h16 = header(4, 4, 16);
        let src16 = vec![0u8; dib_row_bytes(4, 16) * 4];
        assert_eq!(resize_image(&h16, &src16, None, 2, 2), Err(ImageError::UnsupportedBitCount));
    }

    #[test]
    fn resize_invalid_args_are_errors() {
        let h = header(4, 4, 24);
        let src = build_dib(4, 4, 24, unique_pixel);
        // 出力サイズ 0 以下 (原実装ではゼロ除算等の未定義動作)。
        assert_eq!(resize_image(&h, &src, None, 0, 2), Err(ImageError::OutOfRange));
        assert_eq!(resize_image(&h, &src, None, 2, -1), Err(ImageError::OutOfRange));
        // 矩形がはみ出す (原実装では未定義動作)。
        let bad = Rect { left: 2, top: 0, right: 6, bottom: 4 };
        assert_eq!(resize_image(&h, &src, Some(bad), 2, 2), Err(ImageError::OutOfRange));
        // 空矩形。
        let empty = Rect { left: 1, top: 1, right: 1, bottom: 3 };
        assert_eq!(resize_image(&h, &src, Some(empty), 2, 2), Err(ImageError::OutOfRange));
        // ソースバッファ不足。
        let short_src = vec![0u8; 8];
        assert_eq!(resize_image(&h, &short_src, None, 2, 2), Err(ImageError::BufferTooSmall));
    }

    #[test]
    fn resize_error_display() {
        assert!(ImageError::UnsupportedBitCount.to_string().contains("bit count"));
        assert!(ImageError::OutOfRange.to_string().contains("range"));
        assert!(ImageError::BufferTooSmall.to_string().contains("buffer"));
    }
}
