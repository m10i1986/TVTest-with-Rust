//! TVTest の `Util.cpp` / `Util.h` のうちアイコンサイズ/ロード/生成関連関数を windows-rs で
//! 移植したクレート。
//!
//! 移植対象:
//! - `IconSizeType`(`Util.h:123-126`)。
//! - `GetStandardIconSize`(`Util.cpp:1182-1207`、`GetSystemMetrics` による標準アイコン
//!   サイズ取得)。
//! - `LoadIconStandardSize`(`Util.cpp:1210-1230`、`LoadIconMetric` を優先し失敗時は
//!   `LoadImageW` にフォールバック)。
//! - `LoadIconSpecificSize`(`Util.cpp:1233-1244`、`LoadIconWithScaleDown` を優先し
//!   失敗時は `LoadImageW` にフォールバック)。
//! - `LoadSystemIcon`(2 オーバーロード、`Util.cpp:1247-1292`。`IconSizeType` 版は
//!   `LoadIconMetric` 優先+`LoadIcon`/`CopyImage` フォールバック、幅高さ指定版は
//!   `LoadIconWithScaleDown` 優先+標準サイズなら `IconSizeType` 版へ委譲+
//!   `LoadIcon`/`CopyImage` フォールバック)。
//! - `CreateEmptyIcon`(`Util.cpp:1125-1179`。マスクビットマップ(`CreateBitmap`)+
//!   1bpp 以外ならカラービットマップ(`CreateDIBSection`)を組み立て、
//!   `CreateIconIndirect` で空(単色/透明)アイコンを生成する)。
//! - `CreateIconFromBitmap`(`Util.cpp:926-967`。マスク(`CreateIconMaskBitmap`、
//!   `Util.cpp:868-891`)+カラー(`CreateIconColorBitmap`、`Util.cpp:893-924`)の
//!   中間ビットマップを組み立て、`CreateIconIndirect` でビットマップからアイコンを
//!   生成する)。
//! - `SaveIconFromBitmap`(`Util.cpp:998-1122`。ビットマップを 24 ビット固定の
//!   ICO ファイルとして保存する。ICO ヘッダ(`ICONDIR`/`ICONDIRENTRY`、
//!   `Util.cpp:970-992`)はリトルエンディアンのバイト列として手書きで直列化し、
//!   ファイル書き出しは `CreateFile`/`WriteFile` の代わりに `std::fs` を用いる)。
//!
//! ## `LoadIconMetric`/`LoadIconWithScaleDown` を静的リンクしない理由
//!
//! この 2 関数は `comctl32.dll` の Common Controls v6 で追加された API で、
//! プロセスに v6 マニフェスト(`ISOLATIONAWARE`/`ComCtl32.dll` の
//! `assemblyIdentity version="6.0.0.0"` 指定)が無いと、OS は代わりにレガシーな
//! v5(バージョン 5.82、システムが常備するもの)をロードする。v5 にはこれらの
//! エクスポートが存在しないため、`windows_core::link!` が生成する静的インポート
//! (通常のインポートテーブル解決)では、マニフェストの無いプロセス
//! (`cargo test` のテストバイナリを含む)が **起動時に `STATUS_ENTRYPOINT_NOT_FOUND`
//! で即座にクラッシュ**してしまう。TVTest 本体はマニフェストで v6 を明示するため
//! 実運用では問題にならないが、ライブラリ単体では呼び出し元のマニフェストに
//! 依存させたくない。そこで本クレートは `GetProcAddress` による実行時の動的解決を行い、
//! エクスポートが存在しなければ静かに `None`(=フォールバック実行)を返す設計にする。
//! これは原実装の「失敗したら `LoadImage`/`CopyImage` にフォールバックする」という
//! 意図とも自然に合致する。
//!
//! `tvtest_winutil`/`tvtest_gui_util` に続く、windows-rs による GUI 層 Win32 API の
//! 本格移植の一環。

#![cfg(windows)]

use std::io::Write;
use std::path::Path;
use std::sync::OnceLock;

use windows::core::{s, PCWSTR};
use windows::Win32::Foundation::{HANDLE, HINSTANCE, RECT};
use windows::Win32::Graphics::Gdi::{
    CreateBitmap, CreateCompatibleBitmap, CreateCompatibleDC, CreateDIBSection, DeleteDC,
    DeleteObject, FillRect, GetDC, GetDIBits, GetObjectW, GetStockObject, ReleaseDC, SelectObject,
    SetStretchBltMode, StretchBlt, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BITMAPV5HEADER,
    BI_BITFIELDS, BI_RGB, BLACK_BRUSH, DIB_RGB_COLORS, HBITMAP, HBRUSH, RGBQUAD, SRCCOPY,
    STRETCH_BLT_MODE, STRETCH_HALFTONE,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
use windows::Win32::UI::WindowsAndMessaging::{
    CopyImage, CreateIconIndirect, GetSystemMetrics, LoadIconW, LoadImageW, HICON, ICONINFO,
    IMAGE_FLAGS, IMAGE_ICON, LR_DEFAULTCOLOR, SM_CXICON, SM_CXSMICON, SM_CYICON, SM_CYSMICON,
};

/// `IconSizeType`(`Util.h:123-126`)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconSizeType {
    Small,
    Normal,
}

/// `LIM_SMALL`/`LIM_LARGE`(`commctrl.h`)。
const LIM_SMALL: i32 = 0;
const LIM_LARGE: i32 = 1;

type LoadIconMetricFn =
    unsafe extern "system" fn(HINSTANCE, PCWSTR, i32, *mut HICON) -> windows::core::HRESULT;
type LoadIconWithScaleDownFn =
    unsafe extern "system" fn(HINSTANCE, PCWSTR, i32, i32, *mut HICON) -> windows::core::HRESULT;

fn comctl32_proc(name: windows::core::PCSTR) -> Option<usize> {
    unsafe {
        let hmodule = GetModuleHandleA(s!("comctl32.dll")).ok()?;
        let addr = GetProcAddress(hmodule, name)?;
        Some(addr as usize)
    }
}

fn load_icon_metric_fn() -> Option<LoadIconMetricFn> {
    static CACHE: OnceLock<Option<usize>> = OnceLock::new();
    let addr = *CACHE.get_or_init(|| comctl32_proc(s!("LoadIconMetric")));
    addr.map(|a| unsafe { std::mem::transmute::<usize, LoadIconMetricFn>(a) })
}

fn load_icon_with_scale_down_fn() -> Option<LoadIconWithScaleDownFn> {
    static CACHE: OnceLock<Option<usize>> = OnceLock::new();
    let addr = *CACHE.get_or_init(|| comctl32_proc(s!("LoadIconWithScaleDown")));
    addr.map(|a| unsafe { std::mem::transmute::<usize, LoadIconWithScaleDownFn>(a) })
}

/// `LoadIconMetric` を利用可能なら呼び出す。`comctl32.dll` v6 のエクスポートが
/// 見つからない(マニフェスト無し等)場合は `None`。
fn try_load_icon_metric(hinst: Option<HINSTANCE>, name: PCWSTR, metric: i32) -> Option<HICON> {
    let f = load_icon_metric_fn()?;
    let mut hico = HICON::default();
    let hr = unsafe { f(hinst.unwrap_or_default(), name, metric, &mut hico) };
    if hr.is_ok() {
        Some(hico)
    } else {
        None
    }
}

/// `LoadIconWithScaleDown` を利用可能なら呼び出す。`None` は `comctl32.dll` v6
/// のエクスポートが見つからない場合。
fn try_load_icon_with_scale_down(
    hinst: Option<HINSTANCE>,
    name: PCWSTR,
    cx: i32,
    cy: i32,
) -> Option<HICON> {
    let f = load_icon_with_scale_down_fn()?;
    let mut hico = HICON::default();
    let hr = unsafe { f(hinst.unwrap_or_default(), name, cx, cy, &mut hico) };
    if hr.is_ok() {
        Some(hico)
    } else {
        None
    }
}

fn get_system_metrics(index: windows::Win32::UI::WindowsAndMessaging::SYSTEM_METRICS_INDEX) -> i32 {
    unsafe { GetSystemMetrics(index) }
}

/// `GetStandardIconSize`(`Util.cpp:1182-1207`)。
#[must_use]
pub fn get_standard_icon_size(size: IconSizeType) -> Option<(i32, i32)> {
    match size {
        IconSizeType::Small => Some((get_system_metrics(SM_CXSMICON), get_system_metrics(SM_CYSMICON))),
        IconSizeType::Normal => Some((get_system_metrics(SM_CXICON), get_system_metrics(SM_CYICON))),
    }
}

fn hicon_from_handle(handle: HANDLE) -> HICON {
    HICON(handle.0)
}

fn copy_icon_image(hico: HICON, width: i32, height: i32) -> Option<HICON> {
    unsafe { CopyImage(HANDLE(hico.0), IMAGE_ICON, width, height, IMAGE_FLAGS(0)) }
        .ok()
        .map(hicon_from_handle)
}

/// `LoadIconStandardSize`(`Util.cpp:1210-1230`)。
///
/// # Safety
/// `hinst` は有効なモジュールハンドルであること。`name` はリソース識別子であること。
pub unsafe fn load_icon_standard_size(
    hinst: HINSTANCE,
    name: PCWSTR,
    size: IconSizeType,
) -> Option<HICON> {
    let metric = match size {
        IconSizeType::Small => LIM_SMALL,
        IconSizeType::Normal => LIM_LARGE,
    };

    if let Some(hico) = try_load_icon_metric(Some(hinst), name, metric) {
        return Some(hico);
    }

    let (width, height) = get_standard_icon_size(size)?;
    unsafe { LoadImageW(Some(hinst), name, IMAGE_ICON, width, height, LR_DEFAULTCOLOR) }
        .ok()
        .map(hicon_from_handle)
}

/// `LoadIconSpecificSize`(`Util.cpp:1233-1244`)。
///
/// # Safety
/// `hinst` は有効なモジュールハンドルであること。`name` はリソース識別子であること。
pub unsafe fn load_icon_specific_size(
    hinst: HINSTANCE,
    name: PCWSTR,
    width: i32,
    height: i32,
) -> Option<HICON> {
    if width <= 0 || height <= 0 {
        return None;
    }

    if let Some(hico) = try_load_icon_with_scale_down(Some(hinst), name, width, height) {
        return Some(hico);
    }

    unsafe { LoadImageW(Some(hinst), name, IMAGE_ICON, width, height, LR_DEFAULTCOLOR) }
        .ok()
        .map(hicon_from_handle)
}

/// `LoadSystemIcon(LPCTSTR, IconSizeType)`(`Util.cpp:1247-1270`)。
///
/// # Safety
/// `name` は `LoadIconW`/`LoadIconMetric` に渡せる有効なリソース識別子であること。
pub unsafe fn load_system_icon_by_size(name: PCWSTR, size: IconSizeType) -> Option<HICON> {
    let metric = match size {
        IconSizeType::Small => LIM_SMALL,
        IconSizeType::Normal => LIM_LARGE,
    };

    if let Some(hico) = try_load_icon_metric(None, name, metric) {
        return Some(hico);
    }

    let (width, height) = get_standard_icon_size(size)?;

    let hico = unsafe { LoadIconW(None, name) }.ok()?;
    copy_icon_image(hico, width, height)
}

/// `LoadSystemIcon(LPCTSTR, int, int)`(`Util.cpp:1273-1292`)。
///
/// 幅高さが標準サイズ(`SM_CXICON`/`SM_CYICON` または `SM_CXSMICON`/`SM_CYSMICON`)と
/// 一致する場合は [`load_system_icon_by_size`] に委譲する。
///
/// # Safety
/// `name` は `LoadIconW`/`LoadIconWithScaleDown` に渡せる有効なリソース識別子であること。
pub unsafe fn load_system_icon_by_dimensions(name: PCWSTR, width: i32, height: i32) -> Option<HICON> {
    if width <= 0 || height <= 0 {
        return None;
    }

    if let Some(hico) = try_load_icon_with_scale_down(None, name, width, height) {
        return Some(hico);
    }

    if width == get_system_metrics(SM_CXICON) && height == get_system_metrics(SM_CYICON) {
        return unsafe { load_system_icon_by_size(name, IconSizeType::Normal) };
    }
    if width == get_system_metrics(SM_CXSMICON) && height == get_system_metrics(SM_CYSMICON) {
        return unsafe { load_system_icon_by_size(name, IconSizeType::Small) };
    }

    let hico = unsafe { LoadIconW(None, name) }.ok()?;
    copy_icon_image(hico, width, height)
}

/// マスクビットマップ(モノクロ AND マスク)を作る(`CreateEmptyIcon`、
/// `Util.cpp:1132-1140`)。
///
/// `bits_per_pixel == 1` のときは 2 プレーン(AND マスク+XOR マスク相当)、
/// それ以外は 1 プレーン(全面不透明)で `0xFF` 埋め、`bits_per_pixel == 1` の
/// ときだけ後半プレーンを `0x00` で埋める。失敗時(`CreateBitmap` が
/// `HBITMAP::default()` を返す)は `None`。
fn create_mask_bitmap(width: i32, height: i32, bits_per_pixel: i32) -> Option<HBITMAP> {
    let planes: i32 = if bits_per_pixel == 1 { 2 } else { 1 };
    // (Width+15)/16*2 は「1 行あたりのバイト数を 16bit(2byte)境界に切り上げたもの」
    // (モノクロビットマップの走査線バイト数、Util.cpp:1134)。
    let stride = ((width + 15) / 16) * 2;
    let size = (stride as usize) * (height as usize);

    let mut mask_bits = vec![0xFFu8; size * (planes as usize)];
    if bits_per_pixel == 1 {
        mask_bits[size..size * 2].fill(0x00);
    }

    let hbm = unsafe {
        CreateBitmap(width, height * planes, 1, 1, Some(mask_bits.as_ptr().cast()))
    };
    if hbm.is_invalid() {
        None
    } else {
        Some(hbm)
    }
}

/// カラービットマップ(DIB セクション)を作る(`CreateEmptyIcon`、`Util.cpp:1142-1169`)。
/// `bits_per_pixel == 1` の場合は呼び出されない(原実装のガード条件をそのまま維持)。
fn create_color_bitmap(width: i32, height: i32, bits_per_pixel: i32) -> Option<HBITMAP> {
    let header_size = if bits_per_pixel == 32 {
        std::mem::size_of::<BITMAPV5HEADER>()
    } else {
        std::mem::size_of::<BITMAPINFOHEADER>()
    };
    let palette_size = if bits_per_pixel <= 8 {
        (1usize << bits_per_pixel) * std::mem::size_of::<windows::Win32::Graphics::Gdi::RGBQUAD>()
    } else {
        0
    };

    let mut buffer = vec![0u8; header_size + palette_size];

    // BITMAPINFOHEADER の共通フィールドを先頭に書き込む(BITMAPV5HEADER も
    // 同一レイアウトの先頭部分を共有する、Util.cpp:1150-1154)。
    {
        let header = buffer.as_mut_ptr().cast::<BITMAPINFOHEADER>();
        unsafe {
            (*header).biSize = header_size as u32;
            (*header).biWidth = width;
            (*header).biHeight = height;
            (*header).biPlanes = 1;
            (*header).biBitCount = bits_per_pixel as u16;
        }
    }

    if bits_per_pixel == 32 {
        let header_v5 = buffer.as_mut_ptr().cast::<BITMAPV5HEADER>();
        unsafe {
            (*header_v5).bV5Compression = BI_BITFIELDS;
            (*header_v5).bV5RedMask = 0x00FF_0000;
            (*header_v5).bV5GreenMask = 0x0000_FF00;
            (*header_v5).bV5BlueMask = 0x0000_00FF;
            (*header_v5).bV5AlphaMask = 0xFF00_0000;
        }
    }

    let pbmi = buffer.as_ptr().cast::<BITMAPINFO>();
    let mut pbits: *mut std::ffi::c_void = std::ptr::null_mut();
    let hbm = unsafe { CreateDIBSection(None, pbmi, DIB_RGB_COLORS, &mut pbits, None, 0) }.ok()?;

    if !pbits.is_null() {
        // (Width*BitsPerPixel+31)/32*4 は走査線バイト数を 32bit(4byte)境界に
        // 切り上げたもの(Util.cpp:1167)。
        let stride = ((width * bits_per_pixel + 31) / 32) * 4;
        let zero_size = (stride as usize) * (height as usize);
        unsafe {
            std::ptr::write_bytes(pbits.cast::<u8>(), 0, zero_size);
        }
    }

    Some(hbm)
}

/// `CreateEmptyIcon`(`Util.cpp:1125-1179`)。
///
/// `width`/`height` が正でなければ `None`。マスクビットマップと
/// (`bits_per_pixel != 1` の場合)カラービットマップを組み立てて
/// `CreateIconIndirect` でアイコンを生成する。一時ビットマップは
/// 生成後に `DeleteObject` で解放する(原実装 `Util.cpp:1173-1176`)。
#[must_use]
pub fn create_empty_icon(width: i32, height: i32, bits_per_pixel: i32) -> Option<HICON> {
    if width <= 0 || height <= 0 {
        return None;
    }

    let hbm_mask = create_mask_bitmap(width, height, bits_per_pixel);

    let hbm_color = if bits_per_pixel != 1 {
        create_color_bitmap(width, height, bits_per_pixel)
    } else {
        None
    };

    let icon_info = ICONINFO {
        fIcon: windows::core::BOOL(1),
        xHotspot: 0,
        yHotspot: 0,
        hbmMask: hbm_mask.unwrap_or_default(),
        hbmColor: hbm_color.unwrap_or_default(),
    };

    let hicon = unsafe { CreateIconIndirect(&icon_info) }.ok();

    if let Some(hbm) = hbm_mask {
        unsafe {
            let _ = DeleteObject(hbm.into());
        }
    }
    if let Some(hbm) = hbm_color {
        unsafe {
            let _ = DeleteObject(hbm.into());
        }
    }

    hicon
}

/// ICO ファイルに保存されるアイコンのビット数(24 ビット固定、`Util.cpp:1026`)。
const ICO_BIT_COUNT: i32 = 24;

/// `sizeof(ICONDIR)`(1 バイトパッキング時 22 = `ICONDIR` 先頭 6 バイト+
/// `ICONDIRENTRY` 16 バイト、`Util.cpp:970-992`)。
const ICONDIR_SIZE: u32 = 22;

/// `CreateIconMaskBitmap` のマスクビットパターン構築部(`Util.cpp:871-888`)。
///
/// 全体を `0xFF`(透過)で初期化し、中央の `image_width`×`image_height` 領域を
/// `0`(不透過)にクリアする。`image_width == icon_width` のときは行単位、
/// それ以外は `p[x >> 3] &= ~(0x80 >> (x & 7))` のビット単位でクリアする。
/// 引数は `0 <= image_* <= icon_*` を前提とする(呼び出し元で検証済み)。
fn build_icon_mask_bits(
    icon_width: i32,
    icon_height: i32,
    image_width: i32,
    image_height: i32,
) -> Vec<u8> {
    // (IconWidth+15)/16*2 は走査線バイト数を 16bit(2byte)境界に切り上げたもの
    // (モノクロビットマップの走査線バイト数、Util.cpp:871)。
    let bytes_per_line = ((icon_width + 15) / 16 * 2) as usize;
    let mut bits = vec![0xFFu8; bytes_per_line * icon_height as usize];
    let top = ((icon_height - image_height) / 2) as usize;

    if image_width == icon_width {
        let start = top * bytes_per_line;
        bits[start..start + image_height as usize * bytes_per_line].fill(0x00);
    } else {
        let left = ((icon_width - image_width) / 2) as usize;
        for row in bits
            .chunks_exact_mut(bytes_per_line)
            .skip(top)
            .take(image_height as usize)
        {
            for x in left..left + image_width as usize {
                row[x >> 3] &= !(0x80u8 >> (x & 7));
            }
        }
    }

    bits
}

/// `CreateIconFromBitmap`/`SaveIconFromBitmap` 共通の画像サイズ自動計算
/// (`Util.cpp:938-948`/`Util.cpp:1013-1023`)。
///
/// 元ビットマップがアイコンに収まるならそのままのサイズ、収まらないなら
/// アスペクト比を保ってアイコンに内接するサイズ(最小 1)へ縮小する。
/// `bm_width`/`bm_height` は正であることを前提とする(`GetObject` が返す
/// 実在ビットマップのサイズ)。
fn calc_fit_image_size(bm_width: i32, bm_height: i32, icon_width: i32, icon_height: i32) -> (i32, i32) {
    if bm_width <= icon_width && bm_height <= icon_height {
        (bm_width, bm_height)
    } else {
        let image_width = std::cmp::min(bm_width * icon_height / bm_height, icon_width).max(1);
        let image_height = std::cmp::min(bm_height * icon_width / bm_width, icon_height).max(1);
        (image_width, image_height)
    }
}

/// ICO ファイル先頭ヘッダ(`ICONDIR` 6 バイト+`ICONDIRENTRY` 16 バイト+
/// `BITMAPINFOHEADER` 40 バイト)をリトルエンディアンのバイト列として構築する
/// (`Util.cpp:1032-1056`)。
///
/// 原実装の `#include <pshpack1.h>` による 1 バイトパッキング構造体
/// (`Util.cpp:970-992`)の代わりに手書きで直列化する(`repr(C, packed)` 構造体の
/// バイト列化は避ける)。`bWidth`/`bHeight` は BYTE のため 256 以上は原実装同様に
/// 切り詰められる。`biHeight` は原実装どおり 2 倍(カラー+マスクの合計高さ)で
/// 書き込む(`Util.cpp:1100`)。
fn build_ico_file_header(
    icon_width: i32,
    icon_height: i32,
    pixel_bytes: u32,
    mask_bytes: u32,
) -> Vec<u8> {
    let bmih_size = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    let mut buf = Vec::with_capacity((ICONDIR_SIZE + bmih_size) as usize);

    // ICONDIR(Util.cpp:1033-1035)。
    buf.extend_from_slice(&0u16.to_le_bytes()); // idReserved
    buf.extend_from_slice(&1u16.to_le_bytes()); // idType
    buf.extend_from_slice(&1u16.to_le_bytes()); // idCount

    // ICONDIRENTRY(Util.cpp:1036-1043)。
    buf.push(icon_width as u8); // bWidth
    buf.push(icon_height as u8); // bHeight
    buf.push(0); // bColorCount
    buf.push(0); // bReserved
    buf.extend_from_slice(&1u16.to_le_bytes()); // wPlanes
    buf.extend_from_slice(&(ICO_BIT_COUNT as u16).to_le_bytes()); // wBitCount
    buf.extend_from_slice(&(bmih_size + pixel_bytes + mask_bytes).to_le_bytes()); // dwBytesInRes
    buf.extend_from_slice(&ICONDIR_SIZE.to_le_bytes()); // dwImageOffset

    // BITMAPINFOHEADER(Util.cpp:1045-1056)。biHeight のみ 2 倍(Util.cpp:1100)。
    buf.extend_from_slice(&bmih_size.to_le_bytes()); // biSize
    buf.extend_from_slice(&icon_width.to_le_bytes()); // biWidth
    buf.extend_from_slice(&(icon_height * 2).to_le_bytes()); // biHeight
    buf.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
    buf.extend_from_slice(&(ICO_BIT_COUNT as u16).to_le_bytes()); // biBitCount
    buf.extend_from_slice(&BI_RGB.0.to_le_bytes()); // biCompression
    buf.extend_from_slice(&[0u8; 20]); // biSizeImage 〜 biClrImportant(すべて 0)

    buf
}

/// `GetObject` による `BITMAP` 構造体の取得(`Util.cpp:936`/`Util.cpp:1008-1010`)。
/// 戻り値が `sizeof(BITMAP)` と一致しなければ `None`。
unsafe fn get_bitmap_object(hbm: HBITMAP) -> Option<BITMAP> {
    let mut bm = BITMAP::default();
    let size = std::mem::size_of::<BITMAP>() as i32;
    let result = unsafe { GetObjectW(hbm.into(), size, Some(std::ptr::from_mut(&mut bm).cast())) };
    if result != size {
        return None;
    }
    Some(bm)
}

/// `CreateIconMaskBitmap`(`Util.cpp:868-891`)。
///
/// 既存の `CreateEmptyIcon` 用ヘルパー [`create_mask_bitmap`] とは別物
/// (こちらは中央の画像領域だけを不透過にする)。
fn create_icon_mask_bitmap(
    icon_width: i32,
    icon_height: i32,
    image_width: i32,
    image_height: i32,
) -> Option<HBITMAP> {
    let bits = build_icon_mask_bits(icon_width, icon_height, image_width, image_height);
    let hbm =
        unsafe { CreateBitmap(icon_width, icon_height, 1, 1, Some(bits.as_ptr().cast())) };
    if hbm.is_invalid() {
        None
    } else {
        Some(hbm)
    }
}

/// `CreateIconColorBitmap`(`Util.cpp:893-924`)。
///
/// 画面互換のカラービットマップ(`icon_width`×`icon_height`)を作り、元ビットマップ
/// 全体を `STRETCH_HALFTONE` で `image_width`×`image_height` へ伸縮して中央に描画する。
/// 画像がアイコンより小さい場合は先に全面を黒ブラシで塗り潰す(マスクで透過される
/// 領域なので色は表示されない)。
///
/// # Safety
/// `hbm` は有効なビットマップハンドルであること。
unsafe fn create_icon_color_bitmap(
    hbm: HBITMAP,
    icon_width: i32,
    icon_height: i32,
    image_width: i32,
    image_height: i32,
) -> Option<HBITMAP> {
    unsafe {
        let hdc = GetDC(None);
        let hbm_icon = CreateCompatibleBitmap(hdc, icon_width, icon_height);
        if !hbm_icon.is_invalid() {
            let mut bm = BITMAP::default();
            let _ = GetObjectW(
                hbm.into(),
                std::mem::size_of::<BITMAP>() as i32,
                Some(std::ptr::from_mut(&mut bm).cast()),
            );
            let hdc_src = CreateCompatibleDC(Some(hdc));
            let hbm_src_old = SelectObject(hdc_src, hbm.into());
            let hdc_dest = CreateCompatibleDC(Some(hdc));
            let hbm_dest_old = SelectObject(hdc_dest, hbm_icon.into());

            if image_width < icon_width || image_height < icon_height {
                let rc = RECT {
                    left: 0,
                    top: 0,
                    right: icon_width,
                    bottom: icon_height,
                };
                let _ = FillRect(hdc_dest, &rc, HBRUSH(GetStockObject(BLACK_BRUSH).0));
            }
            let old_stretch_mode = SetStretchBltMode(hdc_dest, STRETCH_HALFTONE);
            let _ = StretchBlt(
                hdc_dest,
                (icon_width - image_width) / 2,
                (icon_height - image_height) / 2,
                image_width,
                image_height,
                Some(hdc_src),
                0,
                0,
                bm.bmWidth,
                bm.bmHeight,
                SRCCOPY,
            );
            let _ = SetStretchBltMode(hdc_dest, STRETCH_BLT_MODE(old_stretch_mode));
            let _ = SelectObject(hdc_dest, hbm_dest_old);
            let _ = DeleteDC(hdc_dest);
            let _ = SelectObject(hdc_src, hbm_src_old);
            let _ = DeleteDC(hdc_src);
        }
        let _ = ReleaseDC(None, hdc);

        if hbm_icon.is_invalid() {
            None
        } else {
            Some(hbm_icon)
        }
    }
}

/// `CreateIconFromBitmap`(`Util.cpp:926-967`)。
///
/// ビットマップからアイコンを生成する。`image_width`/`image_height` はアイコン内に
/// 描画する画像のサイズで、どちらかが 0 の場合は元ビットマップのサイズから自動計算
/// する([`calc_fit_image_size`]、`Util.cpp:933-949`)。引数が不正な場合
/// (`hbm` が null、`icon_*` が非正、`image_*` が負またはアイコンより大きい)は
/// `None`。中間ビットマップは生成後に `DeleteObject` で解放する
/// (`Util.cpp:964-965`)。
///
/// # Safety
/// `hbm` は有効なビットマップハンドル(または null)であること。
#[must_use]
pub unsafe fn create_icon_from_bitmap(
    hbm: HBITMAP,
    icon_width: i32,
    icon_height: i32,
    image_width: i32,
    image_height: i32,
) -> Option<HICON> {
    if hbm.is_invalid()
        || icon_width <= 0
        || icon_height <= 0
        || image_width < 0
        || image_width > icon_width
        || image_height < 0
        || image_height > icon_height
    {
        return None;
    }

    let (image_width, image_height) = if image_width == 0 || image_height == 0 {
        let bm = unsafe { get_bitmap_object(hbm) }?;
        calc_fit_image_size(bm.bmWidth, bm.bmHeight, icon_width, icon_height)
    } else {
        (image_width, image_height)
    };

    let hbm_mask = create_icon_mask_bitmap(icon_width, icon_height, image_width, image_height)?;
    let Some(hbm_color) =
        (unsafe { create_icon_color_bitmap(hbm, icon_width, icon_height, image_width, image_height) })
    else {
        unsafe {
            let _ = DeleteObject(hbm_mask.into());
        }
        return None;
    };

    let icon_info = ICONINFO {
        fIcon: windows::core::BOOL(1),
        xHotspot: 0,
        yHotspot: 0,
        hbmMask: hbm_mask,
        hbmColor: hbm_color,
    };
    let hicon = unsafe { CreateIconIndirect(&icon_info) }.ok();

    unsafe {
        let _ = DeleteObject(hbm_mask.into());
        let _ = DeleteObject(hbm_color.into());
    }

    hicon
}

/// ICO ファイル本体の書き出し(`Util.cpp:1094-1111` の `CreateFile`/`WriteFile` 列を
/// `std::fs` で代替)。すべての書き込みが成功したときのみ `true`。
fn write_ico_file(path: &Path, header: &[u8], color_bits: &[u8], mask_bits: &[u8]) -> bool {
    fn write_all(
        path: &Path,
        header: &[u8],
        color_bits: &[u8],
        mask_bits: &[u8],
    ) -> std::io::Result<()> {
        let mut file = std::fs::File::create(path)?;
        file.write_all(header)?;
        file.write_all(color_bits)?;
        file.write_all(mask_bits)?;
        Ok(())
    }
    write_all(path, header, color_bits, mask_bits).is_ok()
}

/// `SaveIconFromBitmap`(`Util.cpp:998-1122`)。
///
/// ビットマップを 24 ビット固定の ICO ファイルとして保存する(原実装コメント
/// `Util.cpp:994-997`)。`image_width`/`image_height` のどちらかが 0 の場合は
/// 元ビットマップのサイズから自動計算する(`Util.cpp:1012-1024`)。引数が不正な
/// 場合(ファイル名が空、`hbm` が null、サイズ不正)や途中で失敗した場合は `false`。
///
/// # Safety
/// `hbm` は有効なビットマップハンドル(または null)であること。
pub unsafe fn save_icon_from_bitmap(
    file_name: &Path,
    hbm: HBITMAP,
    icon_width: i32,
    icon_height: i32,
    image_width: i32,
    image_height: i32,
) -> bool {
    if file_name.as_os_str().is_empty()
        || hbm.is_invalid()
        || icon_width <= 0
        || icon_height <= 0
        || image_width < 0
        || image_width > icon_width
        || image_height < 0
        || image_height > icon_height
    {
        return false;
    }

    let Some(bm) = (unsafe { get_bitmap_object(hbm) }) else {
        return false;
    };

    let (image_width, image_height) = if image_width == 0 || image_height == 0 {
        calc_fit_image_size(bm.bmWidth, bm.bmHeight, icon_width, icon_height)
    } else {
        (image_width, image_height)
    };

    // (IconWidth*BitCount+31)/32*4 / (IconWidth+31)/32*4 は走査線バイト数を
    // 32bit(4byte)境界に切り上げたもの(Util.cpp:1027-1030)。
    let pixel_row_bytes = ((icon_width * ICO_BIT_COUNT + 31) / 32 * 4) as u32;
    let pixel_bytes = pixel_row_bytes * icon_height as u32;
    let mask_row_bytes = ((icon_width + 31) / 32 * 4) as u32;
    let mask_bytes = mask_row_bytes * icon_height as u32;

    // 24bpp DIB セクション用ヘッダ(Util.cpp:1045-1061)。
    let bmih = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: icon_width,
        biHeight: icon_height,
        biPlanes: 1,
        biBitCount: ICO_BIT_COUNT as u16,
        biCompression: BI_RGB.0,
        ..Default::default()
    };
    let bmi = BITMAPINFO {
        bmiHeader: bmih,
        ..Default::default()
    };
    let mut color_bits_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
    let Ok(hbm_color) =
        (unsafe { CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut color_bits_ptr, None, 0) })
    else {
        return false;
    };

    let mut ok = false;
    unsafe {
        // カラービットマップへの描画(Util.cpp:1063-1080、CreateIconColorBitmap と
        // 同じ手順だが DC は CreateCompatibleDC(nullptr) で作る)。
        let hdc_src = CreateCompatibleDC(None);
        let hbm_src_old = SelectObject(hdc_src, hbm.into());
        let hdc_dst = CreateCompatibleDC(None);
        let hbm_dst_old = SelectObject(hdc_dst, hbm_color.into());

        if image_width < icon_width || image_height < icon_height {
            let rc = RECT {
                left: 0,
                top: 0,
                right: icon_width,
                bottom: icon_height,
            };
            let _ = FillRect(hdc_dst, &rc, HBRUSH(GetStockObject(BLACK_BRUSH).0));
        }
        let old_stretch_mode = SetStretchBltMode(hdc_dst, STRETCH_HALFTONE);
        let _ = StretchBlt(
            hdc_dst,
            (icon_width - image_width) / 2,
            (icon_height - image_height) / 2,
            image_width,
            image_height,
            Some(hdc_src),
            0,
            0,
            bm.bmWidth,
            bm.bmHeight,
            SRCCOPY,
        );
        let _ = SetStretchBltMode(hdc_dst, STRETCH_BLT_MODE(old_stretch_mode));
        let _ = SelectObject(hdc_dst, hbm_dst_old);
        let _ = SelectObject(hdc_src, hbm_src_old);

        if let Some(hbm_mask) =
            create_icon_mask_bitmap(icon_width, icon_height, image_width, image_height)
        {
            // 1bpp+2 色パレット({0,0,0},{255,255,255})の BITMAPINFO
            // (Util.cpp:1084-1090)。可変長パレットを含むため Vec<u8> 上に組み立てる
            // (create_color_bitmap と同方式)。Vec<u8> のアラインメントは 1 のため
            // ヘッダは write_unaligned で書き込む。
            let mut mask_info = vec![
                0u8;
                std::mem::size_of::<BITMAPINFOHEADER>()
                    + std::mem::size_of::<RGBQUAD>() * 2
            ];
            let mut mask_header = bmih;
            mask_header.biBitCount = 1;
            mask_info
                .as_mut_ptr()
                .cast::<BITMAPINFOHEADER>()
                .write_unaligned(mask_header);
            let palette = mask_info
                .as_mut_ptr()
                .add(std::mem::size_of::<BITMAPINFOHEADER>())
                .cast::<RGBQUAD>();
            palette.write_unaligned(RGBQUAD {
                rgbBlue: 0,
                rgbGreen: 0,
                rgbRed: 0,
                rgbReserved: 0,
            });
            palette.add(1).write_unaligned(RGBQUAD {
                rgbBlue: 255,
                rgbGreen: 255,
                rgbRed: 255,
                rgbReserved: 0,
            });

            let mut mask_bits_buf = vec![0u8; mask_bytes as usize];
            let _ = GetDIBits(
                hdc_src,
                hbm_mask,
                0,
                icon_height as u32,
                Some(mask_bits_buf.as_mut_ptr().cast()),
                mask_info.as_mut_ptr().cast::<BITMAPINFO>(),
                DIB_RGB_COLORS,
            );

            // ICONDIR+BITMAPINFOHEADER(biHeight は 2 倍)+カラービット+マスク
            // ビットの順で書き込む(Util.cpp:1094-1111)。
            let header = build_ico_file_header(icon_width, icon_height, pixel_bytes, mask_bytes);
            let color_bits =
                std::slice::from_raw_parts(color_bits_ptr.cast::<u8>(), pixel_bytes as usize);
            ok = write_ico_file(file_name, &header, color_bits, &mask_bits_buf);

            let _ = DeleteObject(hbm_mask.into());
        }

        let _ = DeleteDC(hdc_dst);
        let _ = DeleteDC(hdc_src);
        let _ = DeleteObject(hbm_color.into());
    }

    ok
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, IDI_APPLICATION};

    #[test]
    fn standard_icon_size_returns_positive_dimensions() {
        let (w, h) = get_standard_icon_size(IconSizeType::Normal).unwrap();
        assert!(w > 0);
        assert!(h > 0);

        let (sw, sh) = get_standard_icon_size(IconSizeType::Small).unwrap();
        assert!(sw > 0);
        assert!(sh > 0);
    }

    #[test]
    fn small_icon_is_not_larger_than_normal() {
        let (nw, nh) = get_standard_icon_size(IconSizeType::Normal).unwrap();
        let (sw, sh) = get_standard_icon_size(IconSizeType::Small).unwrap();
        assert!(sw <= nw);
        assert!(sh <= nh);
    }

    #[test]
    fn load_system_icon_by_size_succeeds_for_application_icon() {
        let hico = unsafe { load_system_icon_by_size(IDI_APPLICATION, IconSizeType::Normal) };
        assert!(hico.is_some());
        if let Some(h) = hico {
            unsafe {
                let _ = DestroyIcon(h);
            }
        }
    }

    #[test]
    fn load_system_icon_by_size_small_succeeds() {
        let hico = unsafe { load_system_icon_by_size(IDI_APPLICATION, IconSizeType::Small) };
        assert!(hico.is_some());
        if let Some(h) = hico {
            unsafe {
                let _ = DestroyIcon(h);
            }
        }
    }

    #[test]
    fn load_system_icon_by_dimensions_matches_normal_size() {
        let (w, h) = get_standard_icon_size(IconSizeType::Normal).unwrap();
        let hico = unsafe { load_system_icon_by_dimensions(IDI_APPLICATION, w, h) };
        assert!(hico.is_some());
        if let Some(h) = hico {
            unsafe {
                let _ = DestroyIcon(h);
            }
        }
    }

    #[test]
    fn load_system_icon_by_dimensions_rejects_non_positive() {
        assert!(unsafe { load_system_icon_by_dimensions(IDI_APPLICATION, 0, 16) }.is_none());
        assert!(unsafe { load_system_icon_by_dimensions(IDI_APPLICATION, 16, 0) }.is_none());
        assert!(unsafe { load_system_icon_by_dimensions(IDI_APPLICATION, -1, 16) }.is_none());
    }

    #[test]
    fn load_system_icon_by_dimensions_custom_size_succeeds() {
        let hico = unsafe { load_system_icon_by_dimensions(IDI_APPLICATION, 48, 48) };
        assert!(hico.is_some());
        if let Some(h) = hico {
            unsafe {
                let _ = DestroyIcon(h);
            }
        }
    }

    #[test]
    fn load_icon_specific_size_rejects_non_positive() {
        let hinst = HINSTANCE::default();
        assert!(unsafe { load_icon_specific_size(hinst, IDI_APPLICATION, 0, 16) }.is_none());
        assert!(unsafe { load_icon_specific_size(hinst, IDI_APPLICATION, 16, 0) }.is_none());
    }

    #[test]
    fn load_icon_specific_size_does_not_panic_with_null_hinst() {
        // hinst=NULL では自プロセスのリソースとしては見つからないため None になりうるが、
        // パニックしないことのみを確認する(comctl32 v6 未マニフェストでも安全に動作する)。
        let hinst = HINSTANCE::default();
        let result = unsafe { load_icon_specific_size(hinst, IDI_APPLICATION, 16, 16) };
        let _ = result;
    }

    #[test]
    fn comctl32_v6_export_lookup_does_not_panic() {
        // このテスト環境の comctl32.dll がレガシー版(v5)であっても、
        // GetProcAddress ベースの解決はパニックせず None を返すことを確認する。
        let _ = load_icon_metric_fn();
        let _ = load_icon_with_scale_down_fn();
    }

    #[test]
    fn create_empty_icon_rejects_non_positive_size() {
        assert!(create_empty_icon(0, 16, 1).is_none());
        assert!(create_empty_icon(16, 0, 1).is_none());
        assert!(create_empty_icon(-1, 16, 1).is_none());
    }

    #[test]
    fn create_empty_icon_monochrome_succeeds() {
        let hico = create_empty_icon(16, 16, 1);
        assert!(hico.is_some());
        if let Some(h) = hico {
            unsafe {
                let _ = DestroyIcon(h);
            }
        }
    }

    #[test]
    fn create_empty_icon_32bpp_succeeds() {
        let hico = create_empty_icon(32, 32, 32);
        assert!(hico.is_some());
        if let Some(h) = hico {
            unsafe {
                let _ = DestroyIcon(h);
            }
        }
    }

    #[test]
    fn create_empty_icon_8bpp_with_palette_succeeds() {
        let hico = create_empty_icon(16, 16, 8);
        assert!(hico.is_some());
        if let Some(h) = hico {
            unsafe {
                let _ = DestroyIcon(h);
            }
        }
    }

    #[test]
    fn create_empty_icon_24bpp_succeeds() {
        let hico = create_empty_icon(20, 20, 24);
        assert!(hico.is_some());
        if let Some(h) = hico {
            unsafe {
                let _ = DestroyIcon(h);
            }
        }
    }

    #[test]
    fn create_empty_icon_non_square_succeeds() {
        let hico = create_empty_icon(16, 32, 32);
        assert!(hico.is_some());
        if let Some(h) = hico {
            unsafe {
                let _ = DestroyIcon(h);
            }
        }
    }

    #[test]
    fn create_mask_bitmap_monochrome_has_two_planes_size() {
        // 1bpp のときは Size*2 バイト確保される(AND+XOR マスク相当)。
        // 直接は検証できないので、生成自体が成功することのみ確認する。
        let hbm = create_mask_bitmap(16, 16, 1);
        assert!(hbm.is_some());
        if let Some(h) = hbm {
            unsafe {
                let _ = DeleteObject(h.into());
            }
        }
    }

    #[test]
    fn create_color_bitmap_32bpp_succeeds() {
        let hbm = create_color_bitmap(16, 16, 32);
        assert!(hbm.is_some());
        if let Some(h) = hbm {
            unsafe {
                let _ = DeleteObject(h.into());
            }
        }
    }

    /// テスト用の 24bpp DIB セクションビットマップを作る。
    fn create_test_dib(width: i32, height: i32) -> HBITMAP {
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: height,
                biPlanes: 1,
                biBitCount: 24,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        unsafe { CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0) }.unwrap()
    }

    #[test]
    fn build_icon_mask_bits_full_image_clears_all() {
        // 画像サイズ==アイコンサイズなら全域が 0(不透過)になる。
        let bits = build_icon_mask_bits(16, 16, 16, 16);
        assert_eq!(bits.len(), 2 * 16); // BytesPerLine=(16+15)/16*2=2
        assert!(bits.iter().all(|&b| b == 0x00));
    }

    #[test]
    fn build_icon_mask_bits_centered_rect() {
        // 16x16 アイコン中央の 8x8 のみ 0、周囲は 0xFF(透過)。
        let bits = build_icon_mask_bits(16, 16, 8, 8);
        assert_eq!(bits.len(), 2 * 16);
        for y in 0..16 {
            let row = &bits[y * 2..y * 2 + 2];
            if (4..12).contains(&y) {
                // x=4..12 のビットがクリアされる。
                assert_eq!(row, &[0xF0, 0x0F], "y={y}");
            } else {
                assert_eq!(row, &[0xFF, 0xFF], "y={y}");
            }
        }
    }

    #[test]
    fn build_icon_mask_bits_partial_bits_at_byte_boundary() {
        // 20x8 アイコンに 6x4 の画像: Left=7, Top=2、x=7..13 がバイト境界を跨いで
        // クリアされる(BytesPerLine=(20+15)/16*2=4)。
        let bits = build_icon_mask_bits(20, 8, 6, 4);
        assert_eq!(bits.len(), 4 * 8);
        for y in 0..8 {
            let row = &bits[y * 4..y * 4 + 4];
            if (2..6).contains(&y) {
                // x=7 で byte0 の最下位ビット、x=8..13 で byte1 の上位 5 ビットが
                // クリアされる。
                assert_eq!(row, &[0xFE, 0x07, 0xFF, 0xFF], "y={y}");
            } else {
                assert_eq!(row, &[0xFF, 0xFF, 0xFF, 0xFF], "y={y}");
            }
        }
    }

    #[test]
    fn calc_fit_image_size_within_icon_keeps_original() {
        assert_eq!(calc_fit_image_size(16, 16, 32, 32), (16, 16));
        assert_eq!(calc_fit_image_size(32, 32, 32, 32), (32, 32));
    }

    #[test]
    fn calc_fit_image_size_shrinks_wide_image() {
        // 横長(64x32)を 16x16 に内接: 幅は上限 16、高さは 32*16/64=8。
        assert_eq!(calc_fit_image_size(64, 32, 16, 16), (16, 8));
    }

    #[test]
    fn calc_fit_image_size_shrinks_tall_image() {
        // 縦長(32x64)を 16x16 に内接: 幅は 32*16/64=8、高さは上限 16。
        assert_eq!(calc_fit_image_size(32, 64, 16, 16), (8, 16));
    }

    #[test]
    fn calc_fit_image_size_extreme_aspect_clamps_to_one() {
        // 極端なアスペクト比では 0 に丸まらず最小 1 になる(Util.cpp:943-947)。
        assert_eq!(calc_fit_image_size(1000, 1, 16, 16), (16, 1));
        assert_eq!(calc_fit_image_size(1, 1000, 16, 16), (1, 16));
    }

    #[test]
    fn build_ico_file_header_layout() {
        // 16x16/24bpp: PixelRowBytes=(16*24+31)/32*4=48, PixelBytes=768,
        // MaskRowBytes=(16+31)/32*4=4, MaskBytes=64。
        let header = build_ico_file_header(16, 16, 768, 64);
        assert_eq!(header.len(), 62); // 22(ICONDIR)+40(BITMAPINFOHEADER)

        // ICONDIR: idReserved=0, idType=1, idCount=1。
        assert_eq!(&header[0..6], &[0, 0, 1, 0, 1, 0]);
        // ICONDIRENTRY。
        assert_eq!(header[6], 16); // bWidth
        assert_eq!(header[7], 16); // bHeight
        assert_eq!(header[8], 0); // bColorCount
        assert_eq!(header[9], 0); // bReserved
        assert_eq!(u16::from_le_bytes(header[10..12].try_into().unwrap()), 1); // wPlanes
        assert_eq!(u16::from_le_bytes(header[12..14].try_into().unwrap()), 24); // wBitCount
        assert_eq!(
            u32::from_le_bytes(header[14..18].try_into().unwrap()),
            40 + 768 + 64
        ); // dwBytesInRes
        assert_eq!(u32::from_le_bytes(header[18..22].try_into().unwrap()), 22); // dwImageOffset
        // BITMAPINFOHEADER(オフセット 22)。
        assert_eq!(u32::from_le_bytes(header[22..26].try_into().unwrap()), 40); // biSize
        assert_eq!(i32::from_le_bytes(header[26..30].try_into().unwrap()), 16); // biWidth
        assert_eq!(i32::from_le_bytes(header[30..34].try_into().unwrap()), 32); // biHeight(2 倍)
        assert_eq!(u16::from_le_bytes(header[34..36].try_into().unwrap()), 1); // biPlanes
        assert_eq!(u16::from_le_bytes(header[36..38].try_into().unwrap()), 24); // biBitCount
        assert_eq!(u32::from_le_bytes(header[38..42].try_into().unwrap()), 0); // biCompression=BI_RGB
        assert!(header[42..62].iter().all(|&b| b == 0)); // biSizeImage 以降はすべて 0
    }

    #[test]
    fn create_icon_from_bitmap_succeeds_with_auto_size() {
        use windows::Win32::UI::WindowsAndMessaging::GetIconInfo;

        let hbm = create_test_dib(48, 48);
        let hico = unsafe { create_icon_from_bitmap(hbm, 32, 32, 0, 0) };
        assert!(hico.is_some());
        if let Some(h) = hico {
            let mut info = ICONINFO::default();
            unsafe {
                GetIconInfo(h, &mut info).unwrap();
            }
            assert!(info.fIcon.as_bool());
            assert!(!info.hbmMask.is_invalid());
            assert!(!info.hbmColor.is_invalid());
            unsafe {
                // GetIconInfo が返すビットマップは呼び出し側が解放する。
                let _ = DeleteObject(info.hbmMask.into());
                let _ = DeleteObject(info.hbmColor.into());
                let _ = DestroyIcon(h);
            }
        }
        unsafe {
            let _ = DeleteObject(hbm.into());
        }
    }

    #[test]
    fn create_icon_from_bitmap_succeeds_with_explicit_image_size() {
        let hbm = create_test_dib(64, 64);
        let hico = unsafe { create_icon_from_bitmap(hbm, 32, 32, 24, 16) };
        assert!(hico.is_some());
        if let Some(h) = hico {
            unsafe {
                let _ = DestroyIcon(h);
            }
        }
        unsafe {
            let _ = DeleteObject(hbm.into());
        }
    }

    #[test]
    fn create_icon_from_bitmap_rejects_invalid_args() {
        let hbm = create_test_dib(16, 16);
        // hbm が null。
        assert!(unsafe { create_icon_from_bitmap(HBITMAP::default(), 16, 16, 0, 0) }.is_none());
        // アイコンサイズが非正。
        assert!(unsafe { create_icon_from_bitmap(hbm, 0, 16, 0, 0) }.is_none());
        assert!(unsafe { create_icon_from_bitmap(hbm, 16, -1, 0, 0) }.is_none());
        // 画像サイズが負またはアイコンより大きい。
        assert!(unsafe { create_icon_from_bitmap(hbm, 16, 16, -1, 8) }.is_none());
        assert!(unsafe { create_icon_from_bitmap(hbm, 16, 16, 17, 8) }.is_none());
        assert!(unsafe { create_icon_from_bitmap(hbm, 16, 16, 8, 17) }.is_none());
        unsafe {
            let _ = DeleteObject(hbm.into());
        }
    }

    #[test]
    fn save_icon_from_bitmap_writes_valid_ico_file() {
        const ICON_W: i32 = 16;
        const ICON_H: i32 = 16;
        let hbm = create_test_dib(32, 32);
        let path = std::env::temp_dir().join(format!(
            "tvtest_icon_util_save_test_{}.ico",
            std::process::id()
        ));

        let ok = unsafe { save_icon_from_bitmap(&path, hbm, ICON_W, ICON_H, 0, 0) };
        unsafe {
            let _ = DeleteObject(hbm.into());
        }
        assert!(ok);

        let data = std::fs::read(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        let pixel_bytes = ((ICON_W * 24 + 31) / 32 * 4 * ICON_H) as usize;
        let mask_bytes = ((ICON_W + 31) / 32 * 4 * ICON_H) as usize;
        assert_eq!(data.len(), 22 + 40 + pixel_bytes + mask_bytes);
        // ICONDIR: idReserved=0, idType=1, idCount=1。
        assert_eq!(&data[0..6], &[0, 0, 1, 0, 1, 0]);
        assert_eq!(data[6], ICON_W as u8); // bWidth
        assert_eq!(data[7], ICON_H as u8); // bHeight
        assert_eq!(u32::from_le_bytes(data[18..22].try_into().unwrap()), 22); // dwImageOffset
        // BITMAPINFOHEADER(オフセット 22): biSize=40、biHeight は 2 倍。
        assert_eq!(u32::from_le_bytes(data[22..26].try_into().unwrap()), 40);
        assert_eq!(
            i32::from_le_bytes(data[30..34].try_into().unwrap()),
            ICON_H * 2
        );
    }

    #[test]
    fn save_icon_from_bitmap_rejects_invalid_args() {
        let path = std::env::temp_dir().join(format!(
            "tvtest_icon_util_save_invalid_{}.ico",
            std::process::id()
        ));
        let hbm = create_test_dib(16, 16);
        // ファイル名が空。
        assert!(!unsafe { save_icon_from_bitmap(Path::new(""), hbm, 16, 16, 0, 0) });
        // hbm が null。
        assert!(!unsafe { save_icon_from_bitmap(&path, HBITMAP::default(), 16, 16, 0, 0) });
        // サイズ不正。
        assert!(!unsafe { save_icon_from_bitmap(&path, hbm, 0, 16, 0, 0) });
        assert!(!unsafe { save_icon_from_bitmap(&path, hbm, 16, 16, 17, 16) });
        assert!(!unsafe { save_icon_from_bitmap(&path, hbm, 16, 16, 16, -1) });
        // 検証で弾かれた場合はファイルが作られない。
        assert!(!path.exists());
        unsafe {
            let _ = DeleteObject(hbm.into());
        }
    }
}
