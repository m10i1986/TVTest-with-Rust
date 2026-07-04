//! TVTest の GDI+ グラフィックス基盤(`src/Graphics.cpp` / `src/Graphics.h`)を
//! windows-rs の GDI+ flat API(`Win32::Graphics::GdiPlus`)で移植したクレート。
//! `PseudoOSD` / `OSDManager` 移植の前提となる。
//!
//! 移植対象:
//! - [`GraphicsCore`](`CGraphicsCore`、`Graphics.cpp:64-92`)。
//!   `GdiplusStartup` / `GdiplusShutdown` の RAII。
//! - [`Color`](`CColor`、`Graphics.h:72-83`)、
//!   [`GradientDirection`](`Graphics.h:43-46`)、
//!   [`TextFlag`](`Graphics.h:48-70`)。
//! - [`Image`](`CImage`、`Graphics.cpp:97-385`)。`GpBitmap` の RAII。
//!   複製 / ファイル読み込み / メモリ画像生成 / DIB・HBITMAP からの生成 /
//!   クリア / HBITMAP 化。
//! - [`Brush`](`CBrush`、`Graphics.cpp:390-445`)。`GpSolidFill` の RAII。
//! - [`Font`](`CFont`、`Graphics.cpp:450-500`)。`LOGFONTW` からの
//!   `GpFont` 生成。
//! - [`Canvas`](`CCanvas`、`Graphics.cpp:505-929`)。`GpGraphics` の RAII。
//!   クリア / 合成モード / 画像描画 / 塗りつぶし / グラデーション /
//!   テキスト描画・計測 / 縁取りテキスト描画・計測 / フォントメトリクス。
//!
//! ## 対象外
//!
//! - `CImage::LoadFromResource` 2 種(`Graphics.cpp:143-182`)。HINSTANCE の
//!   リソース(および `IStream`)に依存するため移植しない。
//!
//! ## 原実装との差異
//!
//! - 原実装は Gdiplus C++ クラス API を使うが、windows-rs には GDI+ flat API
//!   しか無いため、各メソッドが内部で呼ぶ flat API に置き換えている。C++ の
//!   `GetLastStatus()` による `VerifyConstruct` は、flat API の `GpStatus`
//!   戻り値が `Ok` 以外なら生成物を解放して未生成とする処理で対応する。
//! - `CFont::Create` のフォントファミリ生成失敗時、SDK によっては C++ の
//!   `Gdiplus::Font` コンストラクタが GenericSansSerif へフォールバックするが、
//!   本移植ではフォールバックせず失敗とする([`Font::create`] の doc 参照)。
//! - `CImage::CreateFromDIB` / `CreateFromBitmap` は、ピクセルデータを GDI+ が
//!   コピーせず参照し続けるため(`Graphics.h:99-102` のコメント)、Rust では
//!   unsafe fn とし安全条件を doc に明記した。
//! - 文字列は `wchar_t` に合わせて `&[u16]`(UTF-16 コード単位)で受け取り、
//!   最初の NUL で切り詰める(`LPCTSTR` の意味論)。

#![cfg(windows)]

mod brush;
mod canvas;
mod font;
mod graphics_core;
mod image;
mod types;

pub use brush::Brush;
pub use canvas::Canvas;
pub use font::Font;
pub use graphics_core::GraphicsCore;
pub use image::{
    Image, PIXEL_FORMAT_1BPP_INDEXED, PIXEL_FORMAT_24BPP_RGB, PIXEL_FORMAT_32BPP_ARGB,
    PIXEL_FORMAT_4BPP_INDEXED, PIXEL_FORMAT_8BPP_INDEXED,
};
pub use types::{Color, GradientDirection, TextFlag};

#[cfg(test)]
mod tests {
    use super::*;

    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::null_mut;
    use std::sync::OnceLock;

    use windows::Win32::Foundation::{RECT, SIZE};
    use windows::Win32::Graphics::Gdi::{
        CreateDIBSection, DeleteObject, GetDC, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO,
        DIB_RGB_COLORS, HDC, HPALETTE, LOGFONTW,
    };
    use windows::Win32::Graphics::GdiPlus::{GdipBitmapGetPixel, Ok as GpOk};

    /// テスト全体で一度だけ GDI+ を初期化する(Shutdown はしない)。
    /// テストは並行実行されるため、`OnceLock` で初期化を直列化する。
    fn ensure_gdiplus() {
        static INIT: OnceLock<bool> = OnceLock::new();
        let ok = *INIT.get_or_init(|| {
            let mut core = GraphicsCore::new();
            let ok = core.initialize();
            // Drop の GdiplusShutdown を呼ばせず、プロセス終了まで維持する
            std::mem::forget(core);
            ok
        });
        assert!(ok, "GdiplusStartup failed");
    }

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left,
            top,
            right,
            bottom,
        }
    }

    fn logfont(face: &str, height: i32) -> LOGFONTW {
        let mut lf = LOGFONTW {
            lfHeight: height,
            lfWeight: 400, // FW_NORMAL
            ..Default::default()
        };
        for (i, c) in face.encode_utf16().enumerate() {
            lf.lfFaceName[i] = c;
        }
        lf
    }

    fn make_font() -> Font {
        let font = Font::from_logfont(&logfont("Arial", -16));
        assert!(font.is_created());
        font
    }

    /// `GdipBitmapGetPixel` で ARGB 値を読み出す。
    fn pixel(image: &Image, x: i32, y: i32) -> u32 {
        let mut color = 0u32;
        let status = unsafe { GdipBitmapGetPixel(image.as_bitmap_ptr(), x, y, &mut color) };
        assert_eq!(status, GpOk);
        color
    }

    fn any_nonzero_pixel(image: &Image) -> bool {
        for y in 0..image.get_height() {
            for x in 0..image.get_width() {
                if pixel(image, x, y) != 0 {
                    return true;
                }
            }
        }
        false
    }

    #[test]
    fn graphics_core_initialize_finalize() {
        // 他テストのために先にプロセス永続のトークンを確保しておく
        ensure_gdiplus();

        let mut core = GraphicsCore::new();
        assert!(!core.is_initialized());
        assert!(core.initialize());
        assert!(core.is_initialized());
        // 多重初期化は何もせず true(Graphics.cpp:70-83)
        assert!(core.initialize());
        core.finalize();
        assert!(!core.is_initialized());
        // 再初期化も可能
        assert!(core.initialize());
        // Drop で Finalize(Graphics.cpp:64-67)
    }

    #[test]
    fn color_conversions() {
        // 既定値は全成分 0(Graphics.h:75-78)
        assert_eq!(Color::default().to_argb(), 0);
        // GdiplusColor(CColor)(Graphics.cpp:42-45): ARGB 並び
        assert_eq!(Color::new(0x12, 0x34, 0x56, 0x78).to_argb(), 0x7812_3456);
        // デフォルト引数 a = 255(Graphics.h:81)
        assert_eq!(Color::from_rgb(1, 2, 3).to_argb(), 0xFF01_0203);
        // COLORREF(0x00BBGGRR)から(Graphics.h:82)
        let c = Color::from_colorref(0x00CC_8844);
        assert_eq!((c.red, c.green, c.blue, c.alpha), (0x44, 0x88, 0xCC, 255));
    }

    #[test]
    fn text_flag_bits() {
        // Graphics.h:48-70 の値と一致すること
        assert_eq!(TextFlag::NONE.bits(), 0x0000_0000);
        assert_eq!(TextFlag::FORMAT_LEFT.bits(), 0x0000_0000);
        assert_eq!(TextFlag::FORMAT_RIGHT.bits(), 0x0000_0001);
        assert_eq!(TextFlag::FORMAT_HORZ_CENTER.bits(), 0x0000_0002);
        assert_eq!(TextFlag::FORMAT_HORZ_ALIGN_MASK.bits(), 0x0000_0003);
        assert_eq!(TextFlag::FORMAT_TOP.bits(), 0x0000_0000);
        assert_eq!(TextFlag::FORMAT_BOTTOM.bits(), 0x0000_0004);
        assert_eq!(TextFlag::FORMAT_VERT_CENTER.bits(), 0x0000_0008);
        assert_eq!(TextFlag::FORMAT_VERT_ALIGN_MASK.bits(), 0x0000_000C);
        assert_eq!(TextFlag::FORMAT_NO_WRAP.bits(), 0x0000_0010);
        assert_eq!(TextFlag::FORMAT_NO_CLIP.bits(), 0x0000_0020);
        assert_eq!(TextFlag::FORMAT_END_ELLIPSIS.bits(), 0x0000_0040);
        assert_eq!(TextFlag::FORMAT_WORD_ELLIPSIS.bits(), 0x0000_0080);
        assert_eq!(TextFlag::FORMAT_TRIM_CHAR.bits(), 0x0000_0100);
        assert_eq!(TextFlag::FORMAT_CLIP_LAST_LINE.bits(), 0x0000_0200);
        assert_eq!(TextFlag::DRAW_ANTIALIAS.bits(), 0x0000_1000);
        assert_eq!(TextFlag::DRAW_NO_ANTIALIAS.bits(), 0x0000_2000);
        assert_eq!(TextFlag::DRAW_CLEAR_TYPE.bits(), 0x0000_4000);
        assert_eq!(TextFlag::DRAW_HINTING.bits(), 0x0000_8000);
        assert_eq!(TextFlag::DRAW_PATH.bits(), 0x0001_0000);
    }

    #[test]
    fn image_create_arguments() {
        ensure_gdiplus();
        let mut image = Image::new();
        assert!(!image.is_created());
        assert_eq!(image.get_width(), 0);
        assert_eq!(image.get_height(), 0);

        // 有効なビット数(Graphics.cpp:191-197)
        for bpp in [1, 4, 8, 24, 32] {
            assert!(image.create(4, 3, bpp), "bpp={bpp}");
            assert!(image.is_created());
            assert_eq!(image.get_width(), 4);
            assert_eq!(image.get_height(), 3);
        }
        // 無効なビット数(Graphics.cpp:197)
        for bpp in [0, 2, 16, 64] {
            assert!(!image.create(4, 3, bpp), "bpp={bpp}");
            assert!(!image.is_created());
        }
        // サイズが 0 以下(Graphics.cpp:188-189)
        assert!(!image.create(0, 3, 32));
        assert!(!image.create(4, 0, 32));
        assert!(!image.create(-1, 3, 32));
    }

    #[test]
    fn image_create_clears_pixels() {
        ensure_gdiplus();
        let mut image = Image::new();
        assert!(image.create(4, 4, 32));
        // 生成直後は全ピクセル 0(Graphics.cpp:202 の Clear())
        assert!(!any_nonzero_pixel(&image));

        {
            let mut canvas = Canvas::from_image(&mut image);
            let brush = Brush::from_rgba(255, 0, 0, 255);
            assert!(canvas.fill_rect(&brush, &rect(0, 0, 4, 4)));
        }
        assert_eq!(pixel(&image, 0, 0), 0xFFFF_0000);

        // Clear(Graphics.cpp:343-360)で全ゼロに戻る
        image.clear();
        assert!(!any_nonzero_pixel(&image));
    }

    #[test]
    fn image_clone_independence() {
        ensure_gdiplus();

        // 未生成の複製は未生成(Graphics.cpp:105-116)
        let empty = Image::new();
        assert!(!empty.clone().is_created());

        let mut original = Image::new();
        assert!(original.create(2, 2, 32));
        {
            let mut canvas = Canvas::from_image(&mut original);
            let brush = Brush::from_rgba(255, 0, 0, 255);
            assert!(canvas.fill_rect(&brush, &rect(0, 0, 2, 2)));
        }

        let copy = original.clone();
        assert!(copy.is_created());
        assert_eq!(copy.get_width(), 2);
        assert_eq!(copy.get_height(), 2);
        assert_eq!(pixel(&copy, 0, 0), 0xFFFF_0000);

        // 複製元を描き換えても複製には影響しない
        {
            let mut canvas = Canvas::from_image(&mut original);
            assert!(canvas.clear(0, 0, 255, 255));
        }
        assert_eq!(pixel(&original, 0, 0), 0xFF00_00FF);
        assert_eq!(pixel(&copy, 0, 0), 0xFFFF_0000);
    }

    #[test]
    fn image_load_from_file_missing() {
        ensure_gdiplus();
        let path = std::env::temp_dir().join("tvtest_graphics_no_such_file_12345.bmp");
        let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        let mut image = Image::new();
        assert!(!image.load_from_file(&wide));
        assert!(!image.is_created());
    }

    #[test]
    fn image_load_from_file_bmp() {
        ensure_gdiplus();

        // 2x2 24bpp のボトムアップ BMP を手組みする
        let mut bmp: Vec<u8> = Vec::new();
        // BITMAPFILEHEADER
        bmp.extend_from_slice(b"BM");
        bmp.extend_from_slice(&70u32.to_le_bytes()); // ファイルサイズ
        bmp.extend_from_slice(&0u32.to_le_bytes());
        bmp.extend_from_slice(&54u32.to_le_bytes()); // ピクセルデータオフセット
        // BITMAPINFOHEADER
        bmp.extend_from_slice(&40u32.to_le_bytes());
        bmp.extend_from_slice(&2i32.to_le_bytes()); // 幅
        bmp.extend_from_slice(&2i32.to_le_bytes()); // 高さ(ボトムアップ)
        bmp.extend_from_slice(&1u16.to_le_bytes());
        bmp.extend_from_slice(&24u16.to_le_bytes());
        bmp.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
        bmp.extend_from_slice(&16u32.to_le_bytes());
        bmp.extend_from_slice(&[0u8; 16]); // 解像度・色数
        // 下行: (0,1)=青、(1,1)=白(BGR + 2 バイトパディング)
        bmp.extend_from_slice(&[0xFF, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0x00, 0x00]);
        // 上行: (0,0)=赤、(1,0)=緑
        bmp.extend_from_slice(&[0x00, 0x00, 0xFF, 0x00, 0xFF, 0x00, 0x00, 0x00]);

        let path = std::env::temp_dir().join("tvtest_graphics_test_load.bmp");
        std::fs::write(&path, &bmp).unwrap();
        let wide: Vec<u16> = path.as_os_str().encode_wide().collect();

        let mut image = Image::new();
        let loaded = image.load_from_file(&wide);
        let _ = std::fs::remove_file(&path);
        assert!(loaded);
        assert_eq!(image.get_width(), 2);
        assert_eq!(image.get_height(), 2);
        assert_eq!(pixel(&image, 0, 0), 0xFFFF_0000); // 赤
        assert_eq!(pixel(&image, 1, 0), 0xFF00_FF00); // 緑
        assert_eq!(pixel(&image, 0, 1), 0xFF00_00FF); // 青
        assert_eq!(pixel(&image, 1, 1), 0xFFFF_FFFF); // 白
    }

    #[test]
    fn image_create_from_dib_32bpp_bottom_up() {
        ensure_gdiplus();
        // ボトムアップ(biHeight > 0): メモリ先頭行が最下行(Graphics.cpp:300-305)
        let bits: Vec<u32> = vec![0x1122_3344, 0x5566_7788, 0x99AA_BBCC, 0xDDEE_FF00];
        let mut bmi = BITMAPINFO::default();
        bmi.bmiHeader.biSize = 40;
        bmi.bmiHeader.biWidth = 2;
        bmi.bmiHeader.biHeight = 2;
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;

        let mut image = Image::new();
        assert!(unsafe { image.create_from_dib(&bmi, bits.as_ptr().cast_mut().cast()) });
        assert_eq!(image.get_width(), 2);
        assert_eq!(image.get_height(), 2);
        assert_eq!(pixel(&image, 0, 1), 0x1122_3344);
        assert_eq!(pixel(&image, 1, 1), 0x5566_7788);
        assert_eq!(pixel(&image, 0, 0), 0x99AA_BBCC);
        assert_eq!(pixel(&image, 1, 0), 0xDDEE_FF00);
        drop(image); // bits より先に破棄する(安全条件)
    }

    #[test]
    fn image_create_from_dib_32bpp_top_down() {
        ensure_gdiplus();
        // トップダウン(biHeight < 0): メモリ先頭行が最上行
        let bits: Vec<u32> = vec![0x1122_3344, 0x5566_7788, 0x99AA_BBCC, 0xDDEE_FF00];
        let mut bmi = BITMAPINFO::default();
        bmi.bmiHeader.biSize = 40;
        bmi.bmiHeader.biWidth = 2;
        bmi.bmiHeader.biHeight = -2;
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;

        let mut image = Image::new();
        assert!(unsafe { image.create_from_dib(&bmi, bits.as_ptr().cast_mut().cast()) });
        assert_eq!(pixel(&image, 0, 0), 0x1122_3344);
        assert_eq!(pixel(&image, 1, 1), 0xDDEE_FF00);
        drop(image);
    }

    #[test]
    fn image_create_from_dib_24bpp_and_null_bits() {
        ensure_gdiplus();
        // 24bpp は GdipCreateBitmapFromGdiDib 経路(Graphics.cpp:311-315)
        let bits: [u8; 16] = [
            0xFF, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0x00, 0x00, // 下行: 青、白
            0x00, 0x00, 0xFF, 0x00, 0xFF, 0x00, 0x00, 0x00, // 上行: 赤、緑
        ];
        let mut bmi = BITMAPINFO::default();
        bmi.bmiHeader.biSize = 40;
        bmi.bmiHeader.biWidth = 2;
        bmi.bmiHeader.biHeight = 2;
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 24;

        let mut image = Image::new();
        assert!(unsafe { image.create_from_dib(&bmi, bits.as_ptr().cast_mut().cast()) });
        assert_eq!(image.get_width(), 2);
        assert_eq!(image.get_height(), 2);
        assert_eq!(pixel(&image, 0, 0), 0xFFFF_0000);
        assert_eq!(pixel(&image, 1, 0), 0xFF00_FF00);
        assert_eq!(pixel(&image, 0, 1), 0xFF00_00FF);
        drop(image);

        // pBits が NULL なら false(Graphics.cpp:262-263)
        let mut image = Image::new();
        assert!(!unsafe { image.create_from_dib(&bmi, null_mut()) });
    }

    #[test]
    fn image_create_from_bitmap_32bpp_keeps_alpha() {
        ensure_gdiplus();
        let mut bmi = BITMAPINFO::default();
        bmi.bmiHeader.biSize = 40;
        bmi.bmiHeader.biWidth = 2;
        bmi.bmiHeader.biHeight = 2; // ボトムアップ DIB セクション
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;

        let mut bits_ptr: *mut c_void = null_mut();
        let hbm =
            unsafe { CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits_ptr, None, 0) }
                .unwrap();
        unsafe {
            let p = bits_ptr.cast::<u32>();
            p.write(0x8011_2233); // 下行左(半透明)
            p.add(1).write(0x8044_5566);
            p.add(2).write(0xFF77_8899); // 上行左
            p.add(3).write(0xFFAA_BBCC);
        }

        let mut image = Image::new();
        assert!(unsafe { image.create_from_bitmap(hbm, HPALETTE(null_mut())) });
        assert_eq!(image.get_width(), 2);
        assert_eq!(image.get_height(), 2);
        // 32bpp はアルファチャンネルが保持される(Graphics.cpp:215-247)
        assert_eq!(pixel(&image, 0, 0), 0xFF77_8899);
        assert_eq!(pixel(&image, 0, 1), 0x8011_2233);
        drop(image); // hbm(ピクセルデータ)より先に破棄する

        let _ = unsafe { DeleteObject(hbm.into()) };
    }

    #[test]
    fn image_create_from_bitmap_24bpp_and_invalid() {
        ensure_gdiplus();
        let mut bmi = BITMAPINFO::default();
        bmi.bmiHeader.biSize = 40;
        bmi.bmiHeader.biWidth = 3;
        bmi.bmiHeader.biHeight = 2;
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 24;

        let mut bits_ptr: *mut c_void = null_mut();
        let hbm =
            unsafe { CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits_ptr, None, 0) }
                .unwrap();

        // 非 32bpp は GdipCreateBitmapFromHBITMAP 経路(Graphics.cpp:248-252)
        let mut image = Image::new();
        assert!(unsafe { image.create_from_bitmap(hbm, HPALETTE(null_mut())) });
        assert_eq!(image.get_width(), 3);
        assert_eq!(image.get_height(), 2);
        drop(image);
        let _ = unsafe { DeleteObject(hbm.into()) };

        // 無効なハンドルは GetObject が失敗して false(Graphics.cpp:211-213)
        let mut image = Image::new();
        assert!(!unsafe {
            image.create_from_bitmap(
                windows::Win32::Graphics::Gdi::HBITMAP(null_mut()),
                HPALETTE(null_mut()),
            )
        });
    }

    #[test]
    fn image_create_hbitmap() {
        ensure_gdiplus();

        // 未生成なら NULL(Graphics.cpp:365-366)
        let empty = Image::new();
        assert!(empty.create_hbitmap().is_invalid());

        let mut image = Image::new();
        assert!(image.create(3, 2, 32));
        let hbm = image.create_hbitmap();
        assert!(!hbm.is_invalid());

        let mut bm = BITMAP::default();
        let size = std::mem::size_of::<BITMAP>() as i32;
        let got =
            unsafe { GetObjectW(hbm.into(), size, Some(std::ptr::from_mut(&mut bm).cast())) };
        assert_eq!(got, size);
        assert_eq!(bm.bmWidth, 3);
        assert_eq!(bm.bmHeight, 2);
        assert_eq!(bm.bmBitsPixel, 32);

        let _ = unsafe { DeleteObject(hbm.into()) };
    }

    #[test]
    fn brush_create_and_recreate() {
        ensure_gdiplus();
        let mut brush = Brush::new();
        assert!(!brush.is_created());

        assert!(brush.create_solid_brush(10, 20, 30, 255));
        assert!(brush.is_created());
        // 2 回目は GdipSetSolidFillColor 経路(Graphics.cpp:412-414)
        assert!(brush.create_solid_brush(0, 255, 0, 255));

        let mut image = Image::new();
        assert!(image.create(2, 2, 32));
        {
            let mut canvas = Canvas::from_image(&mut image);
            assert!(canvas.fill_rect(&brush, &rect(0, 0, 2, 2)));
        }
        assert_eq!(pixel(&image, 0, 0), 0xFF00_FF00);

        assert!(brush.create_solid_brush_color(Color::new(1, 2, 3, 4)));
        brush.free();
        assert!(!brush.is_created());
    }

    #[test]
    fn font_create_and_missing() {
        ensure_gdiplus();

        let mut font = Font::new();
        assert!(!font.is_created());
        assert!(font.create(&logfont("Arial", -16)));
        assert!(font.is_created());

        // Bold / Italic / Underline / Strikeout(Graphics.cpp:464-472)
        let mut lf = logfont("Arial", 20);
        lf.lfWeight = 700; // FW_BOLD
        lf.lfItalic = 1;
        lf.lfUnderline = 1;
        lf.lfStrikeOut = 1;
        assert!(font.create(&lf));

        // 存在しないフォント名では失敗(本移植の方針: フォールバックしない)
        assert!(!font.create(&logfont("NoSuchFontName12345", -16)));
        assert!(!font.is_created());

        // サイズ 0 は GdipCreateFont が失敗する
        assert!(!font.create(&logfont("Arial", 0)));
    }

    #[test]
    fn canvas_not_created_operations_fail() {
        ensure_gdiplus();

        // 未生成の CImage からは未生成のキャンバス(Graphics.cpp:516)
        let mut empty = Image::new();
        let mut canvas = Canvas::from_image(&mut empty);
        assert!(!canvas.is_created());
        assert!(!canvas.clear(0, 0, 0, 0));
        assert!(!canvas.set_composition(true));

        // NULL の HDC からも未生成(Graphics.cpp:507)
        let mut canvas = unsafe { Canvas::from_hdc(HDC(null_mut())) };
        assert!(!canvas.is_created());
        let font = make_font();
        let brush = Brush::from_rgba(255, 255, 255, 255);
        assert!(!canvas.fill_rect(&brush, &rect(0, 0, 1, 1)));
        assert!(!canvas.draw_text(&w("A"), &font, &rect(0, 0, 8, 8), &brush, TextFlag::NONE));
        assert_eq!(canvas.get_line_spacing(&font), 0.0);
        assert_eq!(canvas.get_font_ascent(&font), 0.0);
        assert_eq!(canvas.get_font_descent(&font), 0.0);
        let mut size = SIZE { cx: 10, cy: 10 };
        assert!(!canvas.get_text_size(&w("A"), &font, TextFlag::NONE, &mut size));
        assert_eq!((size.cx, size.cy), (0, 0));
    }

    #[test]
    fn canvas_from_hdc_screen() {
        ensure_gdiplus();
        let hdc = unsafe { GetDC(None) };
        assert!(!hdc.is_invalid());
        {
            let mut canvas = unsafe { Canvas::from_hdc(hdc) };
            assert!(canvas.is_created());
            let font = make_font();
            assert!(canvas.get_line_spacing(&font) > 0.0);
            let mut size = SIZE { cx: 0, cy: 0 };
            assert!(canvas.get_text_size(&w("A"), &font, TextFlag::FORMAT_NO_WRAP, &mut size));
            assert!(size.cx > 0 && size.cy > 0);
        }
        unsafe { ReleaseDC(None, hdc) };
    }

    #[test]
    fn canvas_clear_color() {
        ensure_gdiplus();
        let mut image = Image::new();
        assert!(image.create(2, 2, 32));
        {
            let mut canvas = Canvas::from_image(&mut image);
            assert!(canvas.clear(10, 20, 30, 255));
        }
        assert_eq!(pixel(&image, 0, 0), 0xFF0A_141E);
        assert_eq!(pixel(&image, 1, 1), 0xFF0A_141E);
    }

    #[test]
    fn canvas_set_composition_source_copy_overwrites_alpha() {
        ensure_gdiplus();
        let mut image = Image::new();
        assert!(image.create(2, 2, 32));
        {
            let mut canvas = Canvas::from_image(&mut image);
            assert!(canvas.clear(255, 0, 0, 255));
            // SourceCopy(Graphics.cpp:531-539)では合成せず上書きされ、
            // アルファもソースの値になる
            assert!(canvas.set_composition(false));
            let brush = Brush::from_rgba(0, 0, 255, 128);
            assert!(canvas.fill_rect(&brush, &rect(0, 0, 2, 2)));
        }
        assert_eq!(pixel(&image, 0, 0) >> 24, 0x80);

        // SourceOver では不透明の背景に合成されアルファは 255 のまま
        assert!(image.create(2, 2, 32));
        {
            let mut canvas = Canvas::from_image(&mut image);
            assert!(canvas.clear(255, 0, 0, 255));
            assert!(canvas.set_composition(true));
            let brush = Brush::from_rgba(0, 0, 255, 128);
            assert!(canvas.fill_rect(&brush, &rect(0, 0, 2, 2)));
        }
        assert_eq!(pixel(&image, 0, 0) >> 24, 0xFF);
    }

    #[test]
    fn canvas_fill_rect_partial() {
        ensure_gdiplus();
        let mut image = Image::new();
        assert!(image.create(8, 8, 32));
        {
            let mut canvas = Canvas::from_image(&mut image);
            let brush = Brush::from_rgba(0, 255, 0, 255);
            assert!(canvas.fill_rect(&brush, &rect(2, 2, 6, 6)));

            // 未生成のブラシでは false(Graphics.cpp:582-583)
            let empty = Brush::new();
            assert!(!canvas.fill_rect(&empty, &rect(0, 0, 8, 8)));
        }
        assert_eq!(pixel(&image, 0, 0), 0);
        assert_eq!(pixel(&image, 2, 2), 0xFF00_FF00);
        assert_eq!(pixel(&image, 5, 5), 0xFF00_FF00);
        assert_eq!(pixel(&image, 6, 6), 0);
    }

    #[test]
    fn canvas_draw_image() {
        ensure_gdiplus();
        let mut src = Image::new();
        assert!(src.create(4, 4, 32));
        {
            let mut canvas = Canvas::from_image(&mut src);
            assert!(canvas.clear(255, 0, 0, 255));
        }

        let mut dst = Image::new();
        assert!(dst.create(8, 8, 32));
        {
            let mut canvas = Canvas::from_image(&mut dst);
            assert!(canvas.draw_image(&src, 2, 2));
            // 未生成の画像では false(Graphics.cpp:544-546)
            assert!(!canvas.draw_image(&Image::new(), 0, 0));
        }
        assert_eq!(pixel(&dst, 0, 0), 0);
        assert_eq!(pixel(&dst, 2, 2), 0xFFFF_0000);
        assert_eq!(pixel(&dst, 5, 5), 0xFFFF_0000);
        assert_eq!(pixel(&dst, 7, 7), 0);
    }

    #[test]
    fn canvas_draw_image_rect_opacity() {
        ensure_gdiplus();
        let mut src = Image::new();
        assert!(src.create(2, 2, 32));
        {
            let mut canvas = Canvas::from_image(&mut src);
            assert!(canvas.clear(255, 0, 0, 255));
        }

        // 不透明度 1.0: そのまま転写
        let mut dst = Image::new();
        assert!(dst.create(2, 2, 32));
        {
            let mut canvas = Canvas::from_image(&mut dst);
            assert!(canvas.draw_image_rect(0, 0, 2, 2, &src, 0, 0, 2, 2, 1.0));
        }
        assert_eq!(pixel(&dst, 0, 0), 0xFFFF_0000);
        assert_eq!(pixel(&dst, 1, 1), 0xFFFF_0000);

        // 不透明度 0.5: アルファがおよそ半分になる(ColorMatrix の m[3][3])
        assert!(dst.create(2, 2, 32));
        {
            let mut canvas = Canvas::from_image(&mut dst);
            assert!(canvas.draw_image_rect(0, 0, 2, 2, &src, 0, 0, 2, 2, 0.5));
        }
        let alpha = pixel(&dst, 0, 0) >> 24;
        assert!((120..=135).contains(&alpha), "alpha={alpha}");
    }

    #[test]
    fn canvas_fill_gradient() {
        ensure_gdiplus();

        // 水平: 左端が赤、右端が青に近い
        let mut image = Image::new();
        assert!(image.create(16, 1, 32));
        {
            let mut canvas = Canvas::from_image(&mut image);
            assert!(canvas.fill_gradient(
                Color::from_rgb(255, 0, 0),
                Color::from_rgb(0, 0, 255),
                &rect(0, 0, 16, 1),
                GradientDirection::Horz,
            ));
        }
        let left = pixel(&image, 0, 0);
        let right = pixel(&image, 15, 0);
        assert!((left >> 16) & 0xFF > 0xC0, "left={left:08X}");
        assert!(left & 0xFF < 0x40, "left={left:08X}");
        assert!(right & 0xFF > 0xC0, "right={right:08X}");
        assert!((right >> 16) & 0xFF < 0x40, "right={right:08X}");

        // 垂直: 上端が赤、下端が青に近い
        assert!(image.create(1, 16, 32));
        {
            let mut canvas = Canvas::from_image(&mut image);
            assert!(canvas.fill_gradient(
                Color::from_rgb(255, 0, 0),
                Color::from_rgb(0, 0, 255),
                &rect(0, 0, 1, 16),
                GradientDirection::Vert,
            ));
        }
        let top = pixel(&image, 0, 0);
        let bottom = pixel(&image, 0, 15);
        assert!((top >> 16) & 0xFF > 0xC0, "top={top:08X}");
        assert!(bottom & 0xFF > 0xC0, "bottom={bottom:08X}");
    }

    #[test]
    fn canvas_draw_text() {
        ensure_gdiplus();
        let font = make_font();
        let brush = Brush::from_rgba(255, 255, 255, 255);

        let mut image = Image::new();
        assert!(image.create(64, 32, 32));
        {
            let mut canvas = Canvas::from_image(&mut image);
            assert!(canvas.draw_text(
                &w("Hi"),
                &font,
                &rect(0, 0, 64, 32),
                &brush,
                TextFlag::NONE
            ));

            // 空文字列・未生成フォント・未生成ブラシでは false(Graphics.cpp:618-622)
            assert!(!canvas.draw_text(&w(""), &font, &rect(0, 0, 64, 32), &brush, TextFlag::NONE));
            assert!(!canvas.draw_text(
                &w("A\0ignored"),
                &Font::new(),
                &rect(0, 0, 64, 32),
                &brush,
                TextFlag::NONE
            ));
            assert!(!canvas.draw_text(
                &w("A"),
                &font,
                &rect(0, 0, 64, 32),
                &Brush::new(),
                TextFlag::NONE
            ));
        }
        assert!(any_nonzero_pixel(&image));
    }

    #[test]
    fn canvas_draw_text_path() {
        ensure_gdiplus();
        let font = make_font();
        let brush = Brush::from_rgba(255, 255, 255, 255);

        let mut image = Image::new();
        assert!(image.create(64, 32, 32));
        {
            let mut canvas = Canvas::from_image(&mut image);
            // Draw_Path は GraphicsPath 経由(Graphics.cpp:629-648)
            assert!(canvas.draw_text(
                &w("Hi"),
                &font,
                &rect(0, 0, 64, 32),
                &brush,
                TextFlag::DRAW_PATH | TextFlag::DRAW_ANTIALIAS
            ));
        }
        assert!(any_nonzero_pixel(&image));
    }

    #[test]
    fn canvas_get_text_size() {
        ensure_gdiplus();
        let font = make_font();
        let mut image = Image::new();
        assert!(image.create(8, 8, 32));
        let mut canvas = Canvas::from_image(&mut image);

        let mut size = SIZE { cx: 1000, cy: 1000 };
        assert!(canvas.get_text_size(&w("Hello"), &font, TextFlag::FORMAT_NO_WRAP, &mut size));
        assert!(size.cx > 0 && size.cy > 0, "size=({}, {})", size.cx, size.cy);

        // 長い文字列ほど幅が大きい
        let mut size2 = SIZE { cx: 1000, cy: 1000 };
        assert!(canvas.get_text_size(
            &w("Hello Hello Hello"),
            &font,
            TextFlag::FORMAT_NO_WRAP,
            &mut size2
        ));
        assert!(size2.cx > size.cx);

        // 空文字列は (0, 0) で true(Graphics.cpp:671-678)
        let mut size3 = SIZE { cx: 123, cy: 456 };
        assert!(canvas.get_text_size(&w(""), &font, TextFlag::NONE, &mut size3));
        assert_eq!((size3.cx, size3.cy), (0, 0));

        // 未生成のフォントでは false(Graphics.cpp:674-675)
        let mut size4 = SIZE { cx: 10, cy: 10 };
        assert!(!canvas.get_text_size(&w("A"), &Font::new(), TextFlag::NONE, &mut size4));
        assert_eq!((size4.cx, size4.cy), (0, 0));

        // NoWrap なし: 入力サイズがレイアウト矩形になり、狭い幅では折り返して
        // 高さが増える(Graphics.cpp:709-715)
        let text = w("The quick brown fox jumps over the lazy dog");
        let mut nowrap = SIZE { cx: 0, cy: 0 };
        assert!(canvas.get_text_size(&text, &font, TextFlag::FORMAT_NO_WRAP, &mut nowrap));
        let mut wrapped = SIZE { cx: 60, cy: 1000 };
        assert!(canvas.get_text_size(&text, &font, TextFlag::NONE, &mut wrapped));
        assert!(
            wrapped.cy > nowrap.cy,
            "wrapped={}, nowrap={}",
            wrapped.cy,
            nowrap.cy
        );
        assert!(wrapped.cx < nowrap.cx);
    }

    #[test]
    fn canvas_draw_outline_text() {
        ensure_gdiplus();
        let font = make_font();
        let brush = Brush::from_rgba(255, 255, 255, 255);

        let mut image = Image::new();
        assert!(image.create(64, 32, 32));
        {
            let mut canvas = Canvas::from_image(&mut image);
            assert!(canvas.draw_outline_text(
                &w("Hi"),
                &font,
                &rect(0, 0, 64, 32),
                &brush,
                Color::from_rgb(0, 0, 0),
                2.0,
                TextFlag::NONE
            ));
            // 空文字列では false(Graphics.cpp:731-735)
            assert!(!canvas.draw_outline_text(
                &w(""),
                &font,
                &rect(0, 0, 64, 32),
                &brush,
                Color::from_rgb(0, 0, 0),
                2.0,
                TextFlag::NONE
            ));
        }
        assert!(any_nonzero_pixel(&image));
    }

    #[test]
    fn canvas_get_outline_text_size() {
        ensure_gdiplus();
        let font = make_font();
        let mut image = Image::new();
        assert!(image.create(8, 8, 32));
        let mut canvas = Canvas::from_image(&mut image);

        let mut thin = SIZE { cx: 0, cy: 0 };
        assert!(canvas.get_outline_text_size(
            &w("Hello"),
            &font,
            1.0,
            TextFlag::FORMAT_NO_WRAP,
            &mut thin
        ));
        assert!(thin.cx > 0 && thin.cy > 0, "thin=({}, {})", thin.cx, thin.cy);

        // ペン幅の分だけ境界が大きくなる(GdipGetPathWorldBounds はペン込み)
        let mut thick = SIZE { cx: 0, cy: 0 };
        assert!(canvas.get_outline_text_size(
            &w("Hello"),
            &font,
            8.0,
            TextFlag::FORMAT_NO_WRAP,
            &mut thick
        ));
        assert!(thick.cx > thin.cx);
        assert!(thick.cy > thin.cy);

        // 空文字列は (0, 0) で true(Graphics.cpp:775-782)
        let mut size = SIZE { cx: 123, cy: 456 };
        assert!(canvas.get_outline_text_size(&w(""), &font, 1.0, TextFlag::NONE, &mut size));
        assert_eq!((size.cx, size.cy), (0, 0));
    }

    #[test]
    fn canvas_font_metrics() {
        ensure_gdiplus();
        let font = make_font();
        let mut image = Image::new();
        assert!(image.create(8, 8, 32));
        let canvas = Canvas::from_image(&mut image);

        let spacing = canvas.get_line_spacing(&font);
        let ascent = canvas.get_font_ascent(&font);
        let descent = canvas.get_font_descent(&font);
        assert!(spacing > 0.0, "spacing={spacing}");
        assert!(ascent > 0.0, "ascent={ascent}");
        assert!(descent > 0.0, "descent={descent}");
        assert!(ascent > descent);
        // アセント + ディセントは行間の近辺(セル高 <= 行送り)
        assert!(ascent + descent <= spacing + 0.5);
        assert!(ascent + descent > spacing * 0.7);

        // 未生成のフォントでは 0.0(Graphics.cpp:824-825 / 832-833 / 850-851)
        let empty = Font::new();
        assert_eq!(canvas.get_line_spacing(&empty), 0.0);
        assert_eq!(canvas.get_font_ascent(&empty), 0.0);
        assert_eq!(canvas.get_font_descent(&empty), 0.0);
    }
}
