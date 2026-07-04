//! `CImage`(`Graphics.cpp:97-385` / `Graphics.h:85-115`)の移植。

use std::ffi::c_void;
use std::ptr::{null_mut, write_bytes};

use windows::core::PCWSTR;
use windows::Win32::Graphics::Gdi::{GetObjectW, BITMAP, BITMAPINFO, HBITMAP, HPALETTE};
use windows::Win32::Graphics::GdiPlus::{
    GdipBitmapLockBits, GdipBitmapUnlockBits, GdipCloneBitmapAreaI, GdipCreateBitmapFromFile,
    GdipCreateBitmapFromGdiDib, GdipCreateBitmapFromHBITMAP, GdipCreateBitmapFromScan0,
    GdipCreateHBITMAPFromBitmap, GdipDisposeImage, GdipGetImageHeight, GdipGetImagePixelFormat,
    GdipGetImageWidth, BitmapData, GpBitmap, ImageLockModeWrite, Ok as GpOk, PixelFormatAlpha,
    PixelFormatCanonical, PixelFormatGDI, PixelFormatIndexed, Rect, Status,
};

use crate::types::make_argb;

// gdipluspixelformats.h の PixelFormat 定数。windows-rs にはフラグ部
// (PixelFormatIndexed 等)しか無いため、周知の定義どおりに組み立てる。
/// `PixelFormat1bppIndexed`(gdipluspixelformats.h)。
pub const PIXEL_FORMAT_1BPP_INDEXED: i32 =
    (1 | (1 << 8) | PixelFormatIndexed | PixelFormatGDI) as i32;
/// `PixelFormat4bppIndexed`(gdipluspixelformats.h)。
pub const PIXEL_FORMAT_4BPP_INDEXED: i32 =
    (2 | (4 << 8) | PixelFormatIndexed | PixelFormatGDI) as i32;
/// `PixelFormat8bppIndexed`(gdipluspixelformats.h)。
pub const PIXEL_FORMAT_8BPP_INDEXED: i32 =
    (3 | (8 << 8) | PixelFormatIndexed | PixelFormatGDI) as i32;
/// `PixelFormat24bppRGB`(gdipluspixelformats.h)。
pub const PIXEL_FORMAT_24BPP_RGB: i32 = (8 | (24 << 8) | PixelFormatGDI) as i32;
/// `PixelFormat32bppARGB`(gdipluspixelformats.h)。
pub const PIXEL_FORMAT_32BPP_ARGB: i32 =
    (10 | (32 << 8) | PixelFormatAlpha | PixelFormatGDI | PixelFormatCanonical) as i32;

/// `&[u16]` の文字列(最初の NUL まで)を NUL 終端の `Vec<u16>` にする。
pub(crate) fn to_null_terminated(s: &[u16]) -> Vec<u16> {
    let len = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    let mut v = Vec::with_capacity(len + 1);
    v.extend_from_slice(&s[..len]);
    v.push(0);
    v
}

/// `CImage`(`Graphics.h:85-115`)。`Gdiplus::Bitmap`(`GpBitmap`)の RAII
/// ラッパー。
///
/// ムーブ代入(`Graphics.cpp:121-127`)は Rust のムーブそのもので表現される。
/// コピー代入(`Graphics.cpp:103-118`)は [`Clone`] 実装が対応する。
///
/// 対象外: `LoadFromResource` 2 種(`Graphics.cpp:143-182`)は HINSTANCE
/// リソース依存のため移植しない。
#[derive(Debug)]
pub struct Image {
    bitmap: *mut GpBitmap,
}

impl Image {
    /// `CImage()`(`Graphics.h:88`)。未生成状態で生成する。
    #[must_use]
    pub const fn new() -> Self {
        Self { bitmap: null_mut() }
    }

    /// `CCanvas` 等が内部の `GpBitmap` にアクセスするためのポインタ取得
    /// (`Graphics.h:114` の `friend class CCanvas` 相当)。
    pub(crate) const fn as_bitmap_ptr(&self) -> *mut GpBitmap {
        self.bitmap
    }

    /// `VerifyConstruct`(`Graphics.cpp:376-385`)相当。flat API は
    /// `GpStatus` を返すため、`Ok` 以外なら生成物を解放して未生成とする。
    fn set_verified(&mut self, status: Status, bitmap: *mut GpBitmap) -> bool {
        if status == GpOk && !bitmap.is_null() {
            self.bitmap = bitmap;
            true
        } else {
            if !bitmap.is_null() {
                unsafe { GdipDisposeImage(bitmap.cast()) };
            }
            false
        }
    }

    /// `Free`(`Graphics.cpp:130-133`)。画像を解放する。
    pub fn free(&mut self) {
        if !self.bitmap.is_null() {
            unsafe { GdipDisposeImage(self.bitmap.cast()) };
            self.bitmap = null_mut();
        }
    }

    /// `LoadFromFile`(`Graphics.cpp:136-140`)。ファイルから画像を読み込む。
    ///
    /// `file_name` は UTF-16 のファイルパス(NUL 終端は不要。含まれる場合は
    /// 最初の NUL までを使用)。
    pub fn load_from_file(&mut self, file_name: &[u16]) -> bool {
        self.free();
        let wide = to_null_terminated(file_name);
        let mut bitmap = null_mut();
        let status = unsafe { GdipCreateBitmapFromFile(PCWSTR(wide.as_ptr()), &mut bitmap) };
        self.set_verified(status, bitmap)
    }

    /// `Create`(`Graphics.cpp:185-204`)。指定サイズ・ビット数のメモリ画像を
    /// 生成し、全ピクセルをゼロクリアする。
    ///
    /// `bits_per_pixel` は 1/4/8/24/32 のみ有効。`width` / `height` が 0 以下、
    /// または未対応のビット数なら `false`。
    pub fn create(&mut self, width: i32, height: i32, bits_per_pixel: i32) -> bool {
        self.free();
        if width <= 0 || height <= 0 {
            return false;
        }
        let format = match bits_per_pixel {
            1 => PIXEL_FORMAT_1BPP_INDEXED,
            4 => PIXEL_FORMAT_4BPP_INDEXED,
            8 => PIXEL_FORMAT_8BPP_INDEXED,
            24 => PIXEL_FORMAT_24BPP_RGB,
            32 => PIXEL_FORMAT_32BPP_ARGB,
            _ => return false,
        };
        let mut bitmap = null_mut();
        let status =
            unsafe { GdipCreateBitmapFromScan0(width, height, 0, format, None, &mut bitmap) };
        if !self.set_verified(status, bitmap) {
            return false;
        }
        self.clear();
        true
    }

    /// `CreateFromBitmap`(`Graphics.cpp:207-255`)。GDI ビットマップから
    /// 画像を生成する。
    ///
    /// 32bpp の場合はアルファチャンネルを保持するため DIB 経由
    /// (`Graphics.cpp:238-247`)、それ以外は `GdipCreateBitmapFromHBITMAP`。
    /// パレットを使わない場合は `hpal` に `HPALETTE(std::ptr::null_mut())` を
    /// 渡す(原実装のデフォルト引数 `hpal = nullptr`、`Graphics.h:100`)。
    ///
    /// # Safety
    ///
    /// - `hbm`(および非 NULL の `hpal`)は有効な GDI ハンドルであること。
    /// - `hbm` と `hpal` は本 `Image` を破棄するまで有効でなければならない
    ///   (`Graphics.h:99`)。特に 32bpp の DIB セクションではピクセルデータが
    ///   コピーされずに参照され続ける。
    pub unsafe fn create_from_bitmap(&mut self, hbm: HBITMAP, hpal: HPALETTE) -> bool {
        self.free();

        let mut bm = BITMAP::default();
        let size = std::mem::size_of::<BITMAP>() as i32;
        if unsafe { GetObjectW(hbm.into(), size, Some(std::ptr::from_mut(&mut bm).cast())) }
            != size
        {
            return false;
        }

        // Bitmap::FromHBITMAP() はアルファチャンネルが無視される(Graphics.cpp:215)
        if bm.bmBitsPixel == 32 {
            let mut bmi = BITMAPINFO::default();
            // 原実装(Graphics.cpp:240)は biSize に BITMAPINFOHEADER ではなく
            // BITMAPINFO のサイズを設定している。挙動一致のため踏襲する
            // (32bpp 経路では biSize は参照されないため実害はない)。
            bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFO>() as u32;
            bmi.bmiHeader.biWidth = bm.bmWidth;
            bmi.bmiHeader.biHeight = bm.bmHeight;
            bmi.bmiHeader.biPlanes = 1;
            bmi.bmiHeader.biBitCount = 32;
            unsafe { self.create_from_dib(&bmi, bm.bmBits) }
        } else {
            let mut bitmap = null_mut();
            let status = unsafe { GdipCreateBitmapFromHBITMAP(hbm, hpal, &mut bitmap) };
            self.set_verified(status, bitmap)
        }
    }

    /// `CreateFromDIB`(`Graphics.cpp:258-318`)。DIB から画像を生成する。
    ///
    /// 32bpp の場合はアルファチャンネルを保持するため `GdipCreateBitmapFromScan0`
    /// (ボトムアップ DIB は末尾行ポインタ + 負ストライド)、それ以外は
    /// `GdipCreateBitmapFromGdiDib`。`bits` が NULL なら `false`。
    ///
    /// # Safety
    ///
    /// `bits` の画像データは本 `Image` を破棄するまで有効でなければならない
    /// (`Graphics.h:101-102`)。GDI+ はピクセルデータをコピーせず参照し続ける。
    pub unsafe fn create_from_dib(&mut self, bmi: &BITMAPINFO, bits: *mut c_void) -> bool {
        self.free();

        if bits.is_null() {
            return false;
        }

        // Bitmap::FromBITMAPINFO() はアルファチャンネルが無視される(Graphics.cpp:265)
        if bmi.bmiHeader.biBitCount == 32 {
            let width = bmi.bmiHeader.biWidth;
            let height = bmi.bmiHeader.biHeight.abs();

            let mut p = bits.cast::<u8>();
            let mut stride = width * 4;
            if bmi.bmiHeader.biHeight > 0 {
                p = unsafe { p.offset(((height - 1) * stride) as isize) };
                stride = -stride;
            }

            let mut bitmap = null_mut();
            let status = unsafe {
                GdipCreateBitmapFromScan0(
                    width,
                    height,
                    stride,
                    PIXEL_FORMAT_32BPP_ARGB,
                    Some(p.cast_const()),
                    &mut bitmap,
                )
            };
            self.set_verified(status, bitmap)
        } else {
            let mut bitmap = null_mut();
            let status = unsafe { GdipCreateBitmapFromGdiDib(bmi, bits, &mut bitmap) };
            self.set_verified(status, bitmap)
        }
    }

    /// `IsCreated`(`Graphics.cpp:321-324`)。画像が生成されているか取得する。
    #[must_use]
    pub fn is_created(&self) -> bool {
        !self.bitmap.is_null()
    }

    /// `GetWidth`(`Graphics.cpp:327-332`)。幅を取得する。未生成なら 0。
    #[must_use]
    pub fn get_width(&self) -> i32 {
        if self.bitmap.is_null() {
            return 0;
        }
        let mut width = 0u32;
        let _ = unsafe { GdipGetImageWidth(self.bitmap.cast(), &mut width) };
        width as i32
    }

    /// `GetHeight`(`Graphics.cpp:335-340`)。高さを取得する。未生成なら 0。
    #[must_use]
    pub fn get_height(&self) -> i32 {
        if self.bitmap.is_null() {
            return 0;
        }
        let mut height = 0u32;
        let _ = unsafe { GdipGetImageHeight(self.bitmap.cast(), &mut height) };
        height as i32
    }

    /// `Clear`(`Graphics.cpp:343-360`)。全ピクセルをゼロクリアする
    /// (32bppARGB では完全透明になる)。
    pub fn clear(&mut self) {
        if self.bitmap.is_null() {
            return;
        }
        let rc = Rect {
            X: 0,
            Y: 0,
            Width: self.get_width(),
            Height: self.get_height(),
        };
        let mut format = 0i32;
        let _ = unsafe { GdipGetImagePixelFormat(self.bitmap.cast(), &mut format) };
        let mut data = BitmapData::default();
        if unsafe {
            GdipBitmapLockBits(
                self.bitmap,
                &rc,
                ImageLockModeWrite.0 as u32,
                format,
                &mut data,
            )
        } == GpOk
        {
            let mut bits = data.Scan0.cast::<u8>();
            for _ in 0..data.Height {
                unsafe {
                    write_bytes(bits, 0, data.Stride.unsigned_abs() as usize);
                    bits = bits.offset(data.Stride as isize);
                }
            }
            let _ = unsafe { GdipBitmapUnlockBits(self.bitmap, &mut data) };
        }
    }

    /// `CreateBitmap`(`Graphics.cpp:363-373`)。GDI ビットマップ(HBITMAP)を
    /// 生成する。背景色は `Color(0, 0, 0, 0)`(完全透明)。
    ///
    /// 未生成または変換失敗時は NULL ハンドルを返す(原実装の `nullptr` 戻りと
    /// 同じ)。返されたハンドルの解放(`DeleteObject`)は呼び出し側の責任。
    #[must_use]
    pub fn create_hbitmap(&self) -> HBITMAP {
        if self.bitmap.is_null() {
            return HBITMAP(null_mut());
        }
        let mut hbm = HBITMAP(null_mut());
        if unsafe { GdipCreateHBITMAPFromBitmap(self.bitmap, &mut hbm, make_argb(0, 0, 0, 0)) }
            != GpOk
        {
            return HBITMAP(null_mut());
        }
        hbm
    }
}

impl Clone for Image {
    /// コピー代入(`Graphics.cpp:97-118`)。`Bitmap::Clone(0, 0, w, h, format)`
    /// = `GdipCloneBitmapAreaI` で同一サイズ・同一 `PixelFormat` の複製を作る。
    /// 複製元が未生成、または複製に失敗した場合は未生成の `Image` になる。
    fn clone(&self) -> Self {
        let mut image = Self::new();
        if !self.bitmap.is_null() {
            let mut format = 0i32;
            let _ = unsafe { GdipGetImagePixelFormat(self.bitmap.cast(), &mut format) };
            let mut bitmap = null_mut();
            let status = unsafe {
                GdipCloneBitmapAreaI(
                    0,
                    0,
                    self.get_width(),
                    self.get_height(),
                    format,
                    self.bitmap,
                    &mut bitmap,
                )
            };
            image.set_verified(status, bitmap);
        }
        image
    }
}

impl Default for Image {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Image {
    fn drop(&mut self) {
        self.free();
    }
}
