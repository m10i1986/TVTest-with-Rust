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

//! TVTest `DrawUtil`(`src/DrawUtil.cpp` / `DrawUtil.h`)の Rust 移植(その1: 純粋ロジック基盤)。
//!
//! `DrawUtil` は GDI 描画ユーティリティの集合で規模が大きいため、複数段に分けて移植する。
//! この第1段では、描画から独立してテスト可能な色・アルファ計算、グラデーション/光沢の色決定、
//! 枠塗りの矩形計算、マージンのスケーリングといった純粋ロジックを切り出す。
//! GDI に依存するラッパー(`CFont`/`CBrush`/`CBitmap` など)や描画関数(`Fill`/`FillGradient` など)は
//! 後続段で薄いラッパーとして移植する。
//!
//! 色値は原実装と同じく `COLORREF`(`0x00BBGGRR`、R が下位バイト)を `u32` で表す。
//! 混色は [`tvtest_util::mix_color`]、`MulDiv` は [`tvtest_dpi_util::mul_div`] に委譲する。

#![cfg(windows)]

use core::ffi::c_void;
use core::ptr::{copy_nonoverlapping, null_mut};
use std::mem::{size_of, transmute};

use windows::core::{s, w, BOOL, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HANDLE, HWND, RECT, SIZE};
use windows::Win32::Graphics::Gdi::{
    AlphaBlend, BitBlt, CreateBitmap, CreateBrushIndirect, CreateCompatibleBitmap,
    CreateCompatibleDC, CreateDIBSection, CreateFontIndirectW, CreateSolidBrush, DeleteDC,
    DeleteObject, DrawTextW, FillRect, GetCurrentObject, GetDC, GetDCPenColor, GetDIBColorTable,
    GetObjectW, GetStockObject, GetTextFaceW, GetTextMetricsW, GradientFill, LineTo, MoveToEx,
    ReleaseDC, SelectObject, SetBkMode, SetDCBrushColor, SetDCPenColor, SetDIBColorTable,
    SetStretchBltMode, SetTextColor, StretchBlt, AC_SRC_ALPHA, AC_SRC_OVER, BACKGROUND_MODE, BITMAP,
    BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, DC_BRUSH, DC_PEN, DEFAULT_GUI_FONT, DIBSECTION,
    DIB_RGB_COLORS, DRAW_TEXT_FORMAT, FW_NORMAL, GRADIENT_FILL_RECT_H, GRADIENT_FILL_RECT_V,
    GRADIENT_RECT, HBITMAP, HBRUSH, HDC, HFONT, HGDIOBJ, LOGBRUSH, LOGFONTW, OBJ_BITMAP, RGBQUAD,
    SRCCOPY, STRETCH_BLT_MODE, STRETCH_HALFTONE, TEXTMETRICW, TRANSPARENT, TRIVERTEX,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::UI::Controls::{
    CloseThemeData, DrawThemeBackground, DrawThemeText, GetThemeColor, GetThemeFont, GetThemeInt,
    GetThemeMargins, GetThemePartSize, GetThemeSysFont, GetThemeTextExtent,
    GetThemeTransitionDuration, ImageList_Add, ImageList_Create, ImageList_Destroy, IsAppThemed,
    IsThemeBackgroundPartiallyTransparent, OpenThemeData, HIMAGELIST, HTHEME, ILC_COLOR32, MARGINS,
    THEME_PROPERTY_SYMBOL_ID, TS_TRUE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CopyImage, CreateIconIndirect, SystemParametersInfoW, FE_FONTSMOOTHINGCLEARTYPE, HICON,
    ICONINFO, IMAGE_BITMAP, IMAGE_FLAGS, NONCLIENTMETRICSW, SPI_GETFONTSMOOTHING,
    SPI_GETFONTSMOOTHINGTYPE, SPI_GETNONCLIENTMETRICS, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
};

use tvtest_dpi_util::mul_div;
use tvtest_util::mix_color;
use tvtest_winutil::is_windows_10_creators_update_or_later;

/// 白(`RGB(255, 255, 255)`)の `COLORREF`。
const WHITE: u32 = 0x00FF_FFFF;

/// 無効な色を表す `COLORREF`(`CLR_INVALID`)。
const CLR_INVALID: u32 = 0xFFFF_FFFF;

// COLORREF の各チャンネル取り出し(GetRValue / GetGValue / GetBValue 相当)。
#[inline]
fn get_r(color: u32) -> u32 {
    color & 0xFF
}
#[inline]
fn get_g(color: u32) -> u32 {
    (color >> 8) & 0xFF
}
#[inline]
fn get_b(color: u32) -> u32 {
    (color >> 16) & 0xFF
}

// ---------------------------------------------------------------------------
// RGBA(DrawUtil.h:31)
// ---------------------------------------------------------------------------

/// アルファ付きの色。原実装の `DrawUtil::RGBA`(DrawUtil.h:31)。
///
/// `COLORREF` との相互変換を持つ。既定値は全成分 0(`RGBA() = default`)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rgba {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

impl Rgba {
    /// 各成分を指定して生成(`RGBA(r, g, b, a)`、DrawUtil.h:39)。
    pub fn new(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self {
            red,
            green,
            blue,
            alpha,
        }
    }

    /// 不透明(`alpha = 255`)で生成(`RGBA(r, g, b)` 既定引数、DrawUtil.h:39)。
    pub fn from_rgb(red: u8, green: u8, blue: u8) -> Self {
        Self::new(red, green, blue, 255)
    }

    /// `COLORREF` から生成。`alpha` は 255(`RGBA(COLORREF c)`、DrawUtil.h:40)。
    pub fn from_colorref(color: u32) -> Self {
        Self {
            red: get_r(color) as u8,
            green: get_g(color) as u8,
            blue: get_b(color) as u8,
            alpha: 255,
        }
    }

    /// `COLORREF` へ変換(`GetCOLORREF` = `RGB(Red, Green, Blue)`、DrawUtil.h:47)。
    pub fn to_colorref(self) -> u32 {
        (self.red as u32) | ((self.green as u32) << 8) | ((self.blue as u32) << 16)
    }

    /// 成分を設定する(`Set`、DrawUtil.h:46)。
    pub fn set(&mut self, red: u8, green: u8, blue: u8, alpha: u8) {
        self.red = red;
        self.green = green;
        self.blue = blue;
        self.alpha = alpha;
    }
}

// ---------------------------------------------------------------------------
// 色・アルファの基本計算
// ---------------------------------------------------------------------------

/// 0..=255*255 の値を 255 で割る高速近似。原実装 `DIVIDE_BY_255`(DrawUtil.cpp:37)。
///
/// `((v + 1) * 257) >> 16` で `v / 255` を四捨五入相当で求める。
pub fn divide_by_255(v: u32) -> u8 {
    (((v + 1) * 257) >> 16) as u8
}

/// 2つのアルファ値を位置 `pos`(0..=`max`)で線形補間する。原実装 `BlendAlpha`(DrawUtil.cpp:117)。
///
/// `max <= 0` のときは単純平均を返す。`pos = 0` で `alpha1`、`pos = max` で `alpha2`。
pub fn blend_alpha(alpha1: i32, alpha2: i32, pos: i32, max: i32) -> u8 {
    if max <= 0 {
        ((alpha1 + alpha2) / 2) as u8
    } else {
        ((alpha1 * (max - pos) + alpha2 * pos) / max) as u8
    }
}

/// グラデーション端点の各チャンネルを `TRIVERTEX`(16bit)用にスケールする。
///
/// 原実装は `GdiGradientFill` 用に `GetRValue(Color) << 8` 等とする(DrawUtil.cpp:101-109)。
pub fn channel_to_trivertex(value: u8) -> u16 {
    (value as u16) << 8
}

// ---------------------------------------------------------------------------
// 縞々グラデーションの位置→混色比率(DrawUtil.cpp:268-288)
// ---------------------------------------------------------------------------

/// 線形グラデーションの混色比率。原実装の `(右端 - x) * 255 / (幅 - 1)` 部分(DrawUtil.cpp:272, 287)。
///
/// `local` は 0 始まりの画素位置、`span` は幅/高さ(2 以上)。返り値は [`mix_color`] の `Color1` 比率。
/// `local = 0` で 255(始端=`Color1`)、`local = span - 1` で 0(終端=`Color2`)。
pub fn linear_gradient_ratio(local: i32, span: i32) -> u8 {
    (((span - 1 - local) * 255) / (span - 1)) as u8
}

/// 左右(上下)対称グラデーションの混色比率。原実装の `abs(Center - x*2) * 255 / (幅 - 1)` 部分
/// (DrawUtil.cpp:273, 288)。
///
/// `local` は 0 始まりの画素位置、`span` は幅/高さ(2 以上)。中央で 0(=`Color2`)、両端で 255(=`Color1`)。
pub fn mirror_gradient_ratio(local: i32, span: i32) -> u8 {
    (((span - 1 - 2 * local).abs() * 255) / (span - 1)) as u8
}

// ---------------------------------------------------------------------------
// 光沢グラデーションの色決定(DrawUtil.cpp:202-243)
// ---------------------------------------------------------------------------

/// [`glossy_gradient_colors`] の結果。前半・後半の各グラデーション端点色(`COLORREF`)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlossyGradientColors {
    /// 前半グラデーションの始端色。
    pub first_start: u32,
    /// 前半グラデーションの終端色。
    pub first_end: u32,
    /// 後半グラデーションの始端色。
    pub second_start: u32,
    /// 後半グラデーションの終端色。
    pub second_end: u32,
}

/// 光沢グラデーションの 4 端点色を計算する。原実装 `FillGlossyGradient` の色決定部
/// (DrawUtil.cpp:222-241)。
///
/// `is_mirror` は方向が `HorzMirror` / `VertMirror`(対称)かどうか。
/// 非対称時は中央色 = `MixColor(Color1, Color2, 128)`・端色 = `Color2`、
/// 対称時は中央色 = `Color2`・端色 = `Color1`。
/// 前半は白との混色(`gloss_ratio1` / `gloss_ratio2`)、後半は中央色→端色。
pub fn glossy_gradient_colors(
    color1: u32,
    color2: u32,
    is_mirror: bool,
    gloss_ratio1: u8,
    gloss_ratio2: u8,
) -> GlossyGradientColors {
    let (center, end) = if is_mirror {
        (color2, color1)
    } else {
        (mix_color(color1, color2, 128), color2)
    };
    GlossyGradientColors {
        first_start: mix_color(WHITE, color1, gloss_ratio1),
        first_end: mix_color(WHITE, center, gloss_ratio2),
        second_start: center,
        second_end: end,
    }
}

// ---------------------------------------------------------------------------
// オーバーレイ用ピクセル/透過色(DrawUtil.cpp:378-382, 527)
// ---------------------------------------------------------------------------

/// 単色オーバーレイ用の 32bit ピクセル値(不透明)。原実装 `ColorOverlay`(DrawUtil.cpp:378-382)。
///
/// `0xFF000000 | R<<16 | G<<8 | B`。メモリ上(リトルエンディアン)では B, G, R, 0xFF の BGRA となる。
pub fn color_overlay_pixel(color: u32) -> u32 {
    0xFF00_0000 | (get_r(color) << 16) | (get_g(color) << 8) | get_b(color)
}

/// 単色 DIB 描画の透過色。原実装 `DrawMonoColorDIB` の `Color ^ 0x00FFFFFF`(DrawUtil.cpp:527)。
pub fn mono_color_transparent(color: u32) -> u32 {
    color ^ 0x00FF_FFFF
}

// ---------------------------------------------------------------------------
// 枠塗りの矩形計算(DrawUtil.cpp:407-439)
// ---------------------------------------------------------------------------

/// 枠(`border`)から内側の空き矩形(`empty`)を除いた領域のうち、描画矩形(`paint`)と重なる部分を
/// 塗りつぶすべき矩形群として返す。原実装 `FillBorder`(DrawUtil.cpp:407-439)の矩形計算部。
///
/// 返り値は原実装が `FillRect` を呼ぶ順(上帯・下帯・左帯・右帯)で、面積が正のもののみを含む。
/// 原実装で `pPaintRect == nullptr` の場合は呼び出し側が `border` を渡す。
pub fn fill_border_rects(border: RECT, empty: RECT, paint: RECT) -> Vec<RECT> {
    let mut rects = Vec::new();

    // 上下の帯(描画矩形が枠と水平方向に重なる場合)。
    if paint.left < border.right && paint.right > border.left {
        let left = paint.left.max(border.left);
        let right = paint.right.min(border.right);

        // 上帯
        let top = paint.top.max(border.top);
        let bottom = paint.bottom.min(empty.top);
        if top < bottom {
            rects.push(RECT {
                left,
                top,
                right,
                bottom,
            });
        }

        // 下帯
        let top = empty.bottom.max(paint.top);
        let bottom = paint.bottom.min(border.bottom);
        if top < bottom {
            rects.push(RECT {
                left,
                top,
                right,
                bottom,
            });
        }
    }

    // 左右の帯(描画矩形が空き矩形と垂直方向に重なる場合)。
    if paint.top < empty.bottom && paint.bottom > empty.top {
        let top = empty.top.max(paint.top);
        let bottom = empty.bottom.min(paint.bottom);

        // 左帯
        let left = paint.left.max(border.left);
        let right = empty.left.min(paint.right);
        if left < right {
            rects.push(RECT {
                left,
                top,
                right,
                bottom,
            });
        }

        // 右帯
        let left = paint.left.max(empty.right);
        let right = paint.right.min(border.right);
        if left < right {
            rects.push(RECT {
                left,
                top,
                right,
                bottom,
            });
        }
    }

    rects
}

// ---------------------------------------------------------------------------
// マージンのスケーリング(DrawUtil.cpp:1875)
// ---------------------------------------------------------------------------

/// `MARGINS` の 4 辺を `num/denom` でスケールする。原実装 `CUxTheme::ScaleMargins`(DrawUtil.cpp:1875)。
///
/// 各辺に `MulDiv`([`mul_div`])を適用する。
pub fn scale_margins(margins: MARGINS, num: i32, denom: i32) -> MARGINS {
    MARGINS {
        cxLeftWidth: mul_div(margins.cxLeftWidth, num, denom),
        cxRightWidth: mul_div(margins.cxRightWidth, num, denom),
        cyTopHeight: mul_div(margins.cyTopHeight, num, denom),
        cyBottomHeight: mul_div(margins.cyBottomHeight, num, denom),
    }
}

// ---------------------------------------------------------------------------
// 塗りつぶし方向(DrawUtil.h:51)
// ---------------------------------------------------------------------------

/// グラデーション等の塗り方向。原実装 `DrawUtil::FillDirection`(DrawUtil.h:51)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillDirection {
    /// 水平方向
    Horz,
    /// 垂直方向
    Vert,
    /// 左右対称
    HorzMirror,
    /// 上下対称
    VertMirror,
}

impl FillDirection {
    /// 対称(Mirror)方向かどうか。
    fn is_mirror(self) -> bool {
        matches!(self, FillDirection::HorzMirror | FillDirection::VertMirror)
    }

    /// 水平系(`Horz` / `HorzMirror`)かどうか。
    fn is_horizontal(self) -> bool {
        matches!(self, FillDirection::Horz | FillDirection::HorzMirror)
    }
}

// ---------------------------------------------------------------------------
// 塗りつぶし描画関数(DrawUtil.cpp)
//
// アルファ合成を伴う関数(FillGradient の RGBA 版・GlossOverlay・ColorOverlay)は
// DIB セクションのピクセル操作が必要なため後続段で移植する。
// ---------------------------------------------------------------------------

/// 単色で矩形を塗りつぶす。原実装 `Fill`(DrawUtil.cpp:47)。
pub fn fill(hdc: HDC, rect: &RECT, color: u32) -> bool {
    if hdc.0.is_null() {
        return false;
    }
    // SAFETY: hdc は有効。DC ブラシ色を一時変更して塗り、元に戻す。
    unsafe {
        let old_color = SetDCBrushColor(hdc, COLORREF(color));
        let brush = HBRUSH(GetStockObject(DC_BRUSH).0);
        let result = FillRect(hdc, rect, brush);
        SetDCBrushColor(hdc, old_color);
        result != 0
    }
}

/// 2色のグラデーションで矩形を塗りつぶす。原実装 `FillGradient`(COLORREF 版、DrawUtil.cpp:59)。
pub fn fill_gradient(
    hdc: HDC,
    rect: &RECT,
    color1: u32,
    color2: u32,
    direction: FillDirection,
) -> bool {
    if hdc.0.is_null() || rect.left >= rect.right || rect.top >= rect.bottom {
        return false;
    }

    // 1px 幅(高さ)は中間色で単色塗り。
    if (rect.right - rect.left == 1 && direction.is_horizontal())
        || (rect.bottom - rect.top == 1 && !direction.is_horizontal())
    {
        return fill(hdc, rect, mix_color(color1, color2, 128));
    }

    // 対称方向は半分ずつ色を入れ替えて再帰する。
    if direction.is_mirror() {
        let mut rc = *rect;
        if direction == FillDirection::HorzMirror {
            rc.right = (rect.left + rect.right) / 2;
            if rc.right > rc.left {
                fill_gradient(hdc, &rc, color1, color2, FillDirection::Horz);
                rc.left = rc.right;
            }
            rc.right = rect.right;
            fill_gradient(hdc, &rc, color2, color1, FillDirection::Horz);
        } else {
            rc.bottom = (rect.top + rect.bottom) / 2;
            if rc.bottom > rc.top {
                fill_gradient(hdc, &rc, color1, color2, FillDirection::Vert);
                rc.top = rc.bottom;
            }
            rc.bottom = rect.bottom;
            fill_gradient(hdc, &rc, color2, color1, FillDirection::Vert);
        }
        return true;
    }

    // TRIVERTEX による GdiGradientFill。
    let vert = [
        TRIVERTEX {
            x: rect.left,
            y: rect.top,
            Red: channel_to_trivertex(get_r(color1) as u8),
            Green: channel_to_trivertex(get_g(color1) as u8),
            Blue: channel_to_trivertex(get_b(color1) as u8),
            Alpha: 0,
        },
        TRIVERTEX {
            x: rect.right,
            y: rect.bottom,
            Red: channel_to_trivertex(get_r(color2) as u8),
            Green: channel_to_trivertex(get_g(color2) as u8),
            Blue: channel_to_trivertex(get_b(color2) as u8),
            Alpha: 0,
        },
    ];
    let mesh = GRADIENT_RECT {
        UpperLeft: 0,
        LowerRight: 1,
    };
    let mode = if direction == FillDirection::Horz {
        GRADIENT_FILL_RECT_H
    } else {
        GRADIENT_FILL_RECT_V
    };
    // SAFETY: vert/mesh は有効なローカル。
    unsafe { GradientFill(hdc, &vert, &mesh as *const GRADIENT_RECT as *const c_void, 1, mode) }
        .as_bool()
}

/// 光沢のあるグラデーションで塗りつぶす。原実装 `FillGlossyGradient`(DrawUtil.cpp:202)。
pub fn fill_glossy_gradient(
    hdc: HDC,
    rect: &RECT,
    color1: u32,
    color2: u32,
    direction: FillDirection,
    gloss_ratio1: i32,
    gloss_ratio2: i32,
) -> bool {
    let colors = glossy_gradient_colors(
        color1,
        color2,
        direction.is_mirror(),
        gloss_ratio1 as u8,
        gloss_ratio2 as u8,
    );
    let dir = if direction.is_horizontal() {
        FillDirection::Horz
    } else {
        FillDirection::Vert
    };

    // 前半の矩形。
    let mut rc = *rect;
    if direction.is_horizontal() {
        rc.right = (rect.left + rect.right) / 2;
        rc.bottom = rect.bottom;
    } else {
        rc.right = rect.right;
        rc.bottom = (rect.top + rect.bottom) / 2;
    }
    fill_gradient(hdc, &rc, colors.first_start, colors.first_end, dir);

    // 後半の矩形。
    if direction.is_horizontal() {
        rc.left = rc.right;
        rc.right = rect.right;
    } else {
        rc.top = rc.bottom;
        rc.bottom = rect.bottom;
    }
    fill_gradient(hdc, &rc, colors.second_start, colors.second_end, dir);
    true
}

/// 縞々のグラデーションで塗りつぶす。原実装 `FillInterlacedGradient`(DrawUtil.cpp:247)。
pub fn fill_interlaced_gradient(
    hdc: HDC,
    rect: &RECT,
    color1: u32,
    color2: u32,
    direction: FillDirection,
    line_color: u32,
    line_opacity: i32,
) -> bool {
    if hdc.0.is_null() {
        return false;
    }
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    if width <= 0 || height <= 0 {
        return false;
    }
    if width == 1 || height == 1 {
        return fill(hdc, rect, mix_color(color1, color2, 128));
    }

    // SAFETY: hdc は有効。DC ペンで 1px 線を引いて塗る。
    unsafe {
        let pen_old = SelectObject(hdc, GetStockObject(DC_PEN));
        let old_pen_color = GetDCPenColor(hdc);

        if direction.is_horizontal() {
            for x in rect.left..rect.right {
                let local = x - rect.left;
                let ratio = if direction == FillDirection::Horz {
                    linear_gradient_ratio(local, width)
                } else {
                    mirror_gradient_ratio(local, width)
                };
                let mut color = mix_color(color1, color2, ratio);
                if local % 2 == 1 {
                    color = mix_color(line_color, color, line_opacity as u8);
                }
                SetDCPenColor(hdc, COLORREF(color));
                let _ = MoveToEx(hdc, x, rect.top, None);
                let _ = LineTo(hdc, x, rect.bottom);
            }
        } else {
            for y in rect.top..rect.bottom {
                let local = y - rect.top;
                let ratio = if direction == FillDirection::Vert {
                    linear_gradient_ratio(local, height)
                } else {
                    mirror_gradient_ratio(local, height)
                };
                let mut color = mix_color(color1, color2, ratio);
                if local % 2 == 1 {
                    color = mix_color(line_color, color, line_opacity as u8);
                }
                SetDCPenColor(hdc, COLORREF(color));
                let _ = MoveToEx(hdc, rect.left, y, None);
                let _ = LineTo(hdc, rect.right, y);
            }
        }

        SetDCPenColor(hdc, old_pen_color);
        let _ = SelectObject(hdc, pen_old);
    }
    true
}

/// 矩形の周囲(枠)をブラシで塗りつぶす。原実装 `FillBorder`(DrawUtil.cpp:407)。
///
/// `paint` が `None` のときは `border` 全体を描画範囲とする。
pub fn fill_border(
    hdc: HDC,
    border: &RECT,
    empty: &RECT,
    paint: Option<&RECT>,
    hbr: HBRUSH,
) -> bool {
    let paint = paint.unwrap_or(border);
    let rects = fill_border_rects(*border, *empty, *paint);
    for rc in &rects {
        // SAFETY: hdc/hbr の有効性は呼び出し側責務(原実装も同様)。
        unsafe {
            FillRect(hdc, rc, hbr);
        }
    }
    true
}

/// 矩形の周囲(枠)を単色で塗りつぶす。原実装 `FillBorder`(色指定版、DrawUtil.cpp:442)。
pub fn fill_border_color(
    hdc: HDC,
    border: &RECT,
    empty: &RECT,
    paint: Option<&RECT>,
    color: u32,
) -> bool {
    // SAFETY: hdc の有効性は呼び出し側責務。DC ブラシ色を一時変更して塗る。
    unsafe {
        let old_color = SetDCBrushColor(hdc, COLORREF(color));
        let brush = HBRUSH(GetStockObject(DC_BRUSH).0);
        let result = fill_border(hdc, border, empty, paint, brush);
        SetDCBrushColor(hdc, old_color);
        result
    }
}

/// 矩形の周囲を指定幅・単色で塗りつぶす。原実装 `FillBorder`(幅指定版、DrawUtil.cpp:453)。
pub fn fill_border_width(
    hdc: HDC,
    border: &RECT,
    border_width: i32,
    paint: Option<&RECT>,
    color: u32,
) -> bool {
    // InflateRect(-border_width, -border_width) 相当。
    let empty = RECT {
        left: border.left + border_width,
        top: border.top + border_width,
        right: border.right - border_width,
        bottom: border.bottom - border_width,
    };
    fill_border_color(hdc, border, &empty, paint, color)
}

/// アルファ付き2色のグラデーションで塗りつぶす。原実装 `FillGradient`(RGBA 版、DrawUtil.cpp:125)。
///
/// 両端が不透明なら COLORREF 版へ委譲する。半透明を含む場合は一時ビットマップに不透明グラデを描き、
/// 列/行ごとに `AlphaBlend`(`SourceConstantAlpha` を [`blend_alpha`] で補間)して合成する。
pub fn fill_gradient_rgba(
    hdc: HDC,
    rect: &RECT,
    color1: Rgba,
    color2: Rgba,
    direction: FillDirection,
) -> bool {
    if hdc.0.is_null() || rect.left >= rect.right || rect.top >= rect.bottom {
        return false;
    }

    // 両端不透明なら COLORREF 版で十分。
    if color1.alpha == 255 && color2.alpha == 255 {
        return fill_gradient(hdc, rect, color1.to_colorref(), color2.to_colorref(), direction);
    }

    // 対称方向は半分ずつ色を入れ替えて再帰する。
    if direction.is_mirror() {
        let mut rc = *rect;
        if direction == FillDirection::HorzMirror {
            rc.right = (rect.left + rect.right) / 2;
            if rc.right > rc.left {
                fill_gradient_rgba(hdc, &rc, color1, color2, FillDirection::Horz);
                rc.left = rc.right;
            }
            rc.right = rect.right;
            fill_gradient_rgba(hdc, &rc, color2, color1, FillDirection::Horz);
        } else {
            rc.bottom = (rect.top + rect.bottom) / 2;
            if rc.bottom > rc.top {
                fill_gradient_rgba(hdc, &rc, color1, color2, FillDirection::Vert);
                rc.top = rc.bottom;
            }
            rc.bottom = rect.bottom;
            fill_gradient_rgba(hdc, &rc, color2, color1, FillDirection::Vert);
        }
        return true;
    }

    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;

    // SAFETY: hdc は有効。一時ビットマップに不透明グラデを描いて列/行ごとに合成する。
    unsafe {
        let hbm = CreateCompatibleBitmap(hdc, width, height);
        if hbm.0.is_null() {
            return false;
        }
        let hdc_mem = CreateCompatibleDC(Some(hdc));
        let old_bmp = SelectObject(hdc_mem, HGDIOBJ(hbm.0));

        let rc = RECT {
            left: 0,
            top: 0,
            right: width,
            bottom: height,
        };
        fill_gradient(hdc_mem, &rc, color1.to_colorref(), color2.to_colorref(), direction);

        if direction == FillDirection::Horz {
            for x in 0..width {
                let alpha = blend_alpha(color1.alpha as i32, color2.alpha as i32, x, width - 1);
                if alpha != 0 {
                    let bf = BLENDFUNCTION {
                        BlendOp: AC_SRC_OVER as u8,
                        BlendFlags: 0,
                        SourceConstantAlpha: alpha,
                        AlphaFormat: 0,
                    };
                    let _ = AlphaBlend(
                        hdc,
                        x + rect.left,
                        rect.top,
                        1,
                        height,
                        hdc_mem,
                        x,
                        0,
                        1,
                        height,
                        bf,
                    );
                }
            }
        } else {
            for y in 0..height {
                let alpha = blend_alpha(color1.alpha as i32, color2.alpha as i32, y, height - 1);
                if alpha != 0 {
                    let bf = BLENDFUNCTION {
                        BlendOp: AC_SRC_OVER as u8,
                        BlendFlags: 0,
                        SourceConstantAlpha: alpha,
                        AlphaFormat: 0,
                    };
                    let _ = AlphaBlend(
                        hdc,
                        rect.left,
                        y + rect.top,
                        width,
                        1,
                        hdc_mem,
                        0,
                        y,
                        width,
                        1,
                        bf,
                    );
                }
            }
        }

        let _ = SelectObject(hdc_mem, old_bmp);
        let _ = DeleteDC(hdc_mem);
        let _ = DeleteObject(HGDIOBJ(hbm.0));
    }
    true
}

/// 32bpp トップダウン DIB セクションを作る共通処理。失敗時は `None`。
///
/// 成功時は `(HBITMAP, ピクセル先頭ポインタ)` を返す。
fn create_overlay_dib(width: i32, height: i32) -> Option<(HBITMAP, *mut c_void)> {
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut pbits: *mut c_void = core::ptr::null_mut();
    // SAFETY: bmi/pbits は有効。CreateDIBSection が pbits にピクセル先頭を返す。
    match unsafe { CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut pbits, None, 0) } {
        Ok(hbm) if !hbm.0.is_null() && !pbits.is_null() => Some((hbm, pbits)),
        _ => None,
    }
}

/// DIB を premultiplied/定数アルファで `hdc` に重ねて破棄する共通処理。
fn alpha_blend_overlay_dib(
    hdc: HDC,
    rect: &RECT,
    width: i32,
    height: i32,
    hbm: HBITMAP,
    source_constant_alpha: u8,
    premultiplied: bool,
) -> bool {
    // SAFETY: hbm は有効。メモリ DC に選択してアルファ合成し、確実に破棄する。
    unsafe {
        let hdc_mem = CreateCompatibleDC(Some(hdc));
        if hdc_mem.0.is_null() {
            let _ = DeleteObject(HGDIOBJ(hbm.0));
            return false;
        }
        let hbm_old = SelectObject(hdc_mem, HGDIOBJ(hbm.0));
        let bf = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: source_constant_alpha,
            AlphaFormat: if premultiplied { AC_SRC_ALPHA as u8 } else { 0 },
        };
        let _ = AlphaBlend(
            hdc, rect.left, rect.top, width, height, hdc_mem, 0, 0, width, height, bf,
        );
        let _ = SelectObject(hdc_mem, hbm_old);
        let _ = DeleteDC(hdc_mem);
        let _ = DeleteObject(HGDIOBJ(hbm.0));
    }
    true
}

/// 光沢(上半分=ハイライト・下半分=シャドウ)を重ねる。原実装 `GlossOverlay`(DrawUtil.cpp:305)。
pub fn gloss_overlay(
    hdc: HDC,
    rect: &RECT,
    highlight1: i32,
    highlight2: i32,
    shadow1: i32,
    shadow2: i32,
) -> bool {
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    if width <= 0 || height <= 0 {
        return false;
    }

    let (hbm, pbits) = match create_overlay_dib(width, height) {
        Some(v) => v,
        None => return false,
    };

    let row_bytes = (width * 4) as usize;
    let center = height / 2;
    let base = pbits as *mut u8;

    // 上半分: ハイライト(全バイト = アルファ → premultiplied で白を α 重ね)。
    for y in 0..center {
        let alpha = blend_alpha(highlight1, highlight2, y, center - 1);
        // SAFETY: base は width*height*4 バイトの DIB。行内に収まる。
        let row = unsafe { base.add(y as usize * row_bytes) };
        for b in 0..row_bytes {
            unsafe {
                *row.add(b) = alpha;
            }
        }
    }
    // 下半分: シャドウ(アルファのみ → premultiplied で黒を α 重ね)。
    for y in center..height {
        let alpha = blend_alpha(shadow1, shadow2, y - center, height - center - 1);
        // SAFETY: 同上。
        let row = unsafe { base.add(y as usize * row_bytes) };
        for x in 0..width {
            let px = unsafe { row.add(x as usize * 4) };
            unsafe {
                *px = 0;
                *px.add(1) = 0;
                *px.add(2) = 0;
                *px.add(3) = alpha;
            }
        }
    }

    alpha_blend_overlay_dib(hdc, rect, width, height, hbm, 255, true)
}

/// 単色を指定不透明度で重ねる。原実装 `ColorOverlay`(DrawUtil.cpp:360)。
pub fn color_overlay(hdc: HDC, rect: &RECT, color: u32, opacity: u8) -> bool {
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    if width <= 0 || height <= 0 {
        return false;
    }

    let (hbm, pbits) = match create_overlay_dib(width, height) {
        Some(v) => v,
        None => return false,
    };

    let pixel = color_overlay_pixel(color);
    let count = (width * height) as usize;
    let p = pbits as *mut u32;
    for i in 0..count {
        // SAFETY: p は width*height 個の u32 を持つ DIB。
        unsafe {
            *p.add(i) = pixel;
        }
    }

    alpha_blend_overlay_dib(hdc, rect, width, height, hbm, opacity, false)
}

// ---------------------------------------------------------------------------
// フォント(DrawUtil.cpp / DrawUtil.h)
// ---------------------------------------------------------------------------

/// システムフォントの種別。原実装 `DrawUtil::FontType`(DrawUtil.h:100)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontType {
    /// メッセージフォント
    Message,
    /// メニューフォント
    Menu,
    /// キャプションフォント
    Caption,
    /// 小キャプションフォント
    SmallCaption,
    /// ステータスフォント
    Status,
}

/// NUL 終端ワイド文字列の長さ(NUL を含まない)。
fn wide_len(s: &[u16]) -> usize {
    s.iter().position(|&c| c == 0).unwrap_or(s.len())
}

/// ASCII 大文字を小文字へ。
fn to_lower16(c: u16) -> u16 {
    if (b'A' as u16..=b'Z' as u16).contains(&c) {
        c + 32
    } else {
        c
    }
}

/// 2つの NUL 終端ワイド文字列を大小区別ありで比較(`lstrcmp` 相当)。
fn wide_eq(a: &[u16], b: &[u16]) -> bool {
    let la = wide_len(a);
    let lb = wide_len(b);
    la == lb && a[..la] == b[..lb]
}

/// NUL 終端ワイド文字列を `str` と大小区別ありで比較(`lstrcmp` 相当)。
fn wide_eq_str(wide: &[u16], s: &str) -> bool {
    wide[..wide_len(wide)].iter().copied().eq(s.encode_utf16())
}

/// NUL 終端ワイド文字列を `str` と ASCII 大小無視で比較(`lstrcmpi` 相当)。
fn wide_eq_str_ci(wide: &[u16], s: &str) -> bool {
    let w = &wide[..wide_len(wide)];
    let s16: Vec<u16> = s.encode_utf16().collect();
    w.len() == s16.len()
        && w.iter()
            .zip(s16.iter())
            .all(|(&a, &b)| to_lower16(a) == to_lower16(b))
}

/// NUL 終端ワイド文字列同士を ASCII 大小無視で比較(`lstrcmpi` 相当)。
fn wide_eq_ci(a: &[u16], b: &[u16]) -> bool {
    let la = wide_len(a);
    let lb = wide_len(b);
    la == lb
        && a[..la]
            .iter()
            .zip(b[..lb].iter())
            .all(|(&x, &y)| to_lower16(x) == to_lower16(y))
}

/// `LOGFONTW.lfFaceName` に face 名を設定する(NUL 終端、容量超過は切り詰め)。
fn set_face_name(log_font: &mut LOGFONTW, name: &str) {
    let src: Vec<u16> = name.encode_utf16().collect();
    let cap = log_font.lfFaceName.len();
    let n = src.len().min(cap - 1);
    for c in log_font.lfFaceName.iter_mut() {
        *c = 0;
    }
    log_font.lfFaceName[..n].copy_from_slice(&src[..n]);
}

/// 2つの `LOGFONTW` を比較する。原実装 `CompareLogFont`(Util.cpp:589)。
///
/// 数値フィールド(`lfFaceName` 直前までの 28 バイト相当)を比較し、`lfFaceName` を
/// 大小区別あり(`lstrcmp`)で比較する。Win32 型を扱うため tvtest_util ではなく本クレートに置く。
fn compare_log_font(f1: &LOGFONTW, f2: &LOGFONTW) -> bool {
    f1.lfHeight == f2.lfHeight
        && f1.lfWidth == f2.lfWidth
        && f1.lfEscapement == f2.lfEscapement
        && f1.lfOrientation == f2.lfOrientation
        && f1.lfWeight == f2.lfWeight
        && f1.lfItalic == f2.lfItalic
        && f1.lfUnderline == f2.lfUnderline
        && f1.lfStrikeOut == f2.lfStrikeOut
        && f1.lfCharSet == f2.lfCharSet
        && f1.lfOutPrecision == f2.lfOutPrecision
        && f1.lfClipPrecision == f2.lfClipPrecision
        && f1.lfQuality == f2.lfQuality
        && f1.lfPitchAndFamily == f2.lfPitchAndFamily
        && wide_eq(&f1.lfFaceName, &f2.lfFaceName)
}

/// `NONCLIENTMETRICS` の `cbSize`。原実装 `CCSIZEOF_STRUCT(NONCLIENTMETRICS, lfMessageFont)`
/// (DrawUtil.cpp:715)= `lfMessageFont` までを含むサイズ(`iPaddedBorderWidth` を除く)。
fn nonclientmetrics_cbsize() -> u32 {
    (core::mem::offset_of!(NONCLIENTMETRICSW, lfMessageFont) + size_of::<LOGFONTW>()) as u32
}

/// 種別に対応する `NONCLIENTMETRICS` のフォントを返す。原実装 `GetNonClientFont`(DrawUtil.cpp:694)。
fn get_nonclient_font(ncm: &NONCLIENTMETRICSW, font_type: FontType) -> &LOGFONTW {
    match font_type {
        FontType::Message => &ncm.lfMessageFont,
        FontType::Menu => &ncm.lfMenuFont,
        FontType::Caption => &ncm.lfCaptionFont,
        FontType::SmallCaption => &ncm.lfSmCaptionFont,
        FontType::Status => &ncm.lfStatusFont,
    }
}

/// システムフォントを取得する。原実装 `GetSystemFont`(DrawUtil.cpp:709)。
pub fn get_system_font(font_type: FontType) -> Option<LOGFONTW> {
    let mut ncm = NONCLIENTMETRICSW {
        cbSize: nonclientmetrics_cbsize(),
        ..Default::default()
    };
    // SAFETY: ncm は有効なバッファ。SPI_GETNONCLIENTMETRICS が値を書き込む。
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETNONCLIENTMETRICS,
            ncm.cbSize,
            Some(&mut ncm as *mut NONCLIENTMETRICSW as *mut c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    if ok.is_err() {
        return None;
    }
    Some(*get_nonclient_font(&ncm, font_type))
}

/// DPI を指定してシステムフォントを取得する。原実装 `GetSystemFontWithDPI`(DrawUtil.cpp:730)。
///
/// DPI 対応 API が無い環境では非対応版で取得し、`lfHeight` を DPI でスケールする。
pub fn get_system_font_with_dpi(font_type: FontType, dpi: i32) -> Option<LOGFONTW> {
    let mut need_scaling = false;
    let mut ncm = NONCLIENTMETRICSW {
        cbSize: nonclientmetrics_cbsize(),
        ..Default::default()
    };
    // SAFETY: ncm は有効なバッファ。
    let ok = unsafe {
        tvtest_dpi_util::system_parameters_info_with_dpi(
            SPI_GETNONCLIENTMETRICS.0,
            ncm.cbSize,
            &mut ncm as *mut NONCLIENTMETRICSW as *mut c_void,
            0,
            dpi,
        )
    };
    if !ok {
        // SAFETY: 同上。
        let r = unsafe {
            SystemParametersInfoW(
                SPI_GETNONCLIENTMETRICS,
                ncm.cbSize,
                Some(&mut ncm as *mut NONCLIENTMETRICSW as *mut c_void),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
        };
        if r.is_err() {
            return None;
        }
        need_scaling = true;
    }

    let mut log_font = *get_nonclient_font(&ncm, font_type);
    if need_scaling {
        let system_dpi = tvtest_dpi_util::get_system_dpi();
        let denom = if system_dpi != 0 { system_dpi } else { 96 };
        log_font.lfHeight = mul_div(log_font.lfHeight, dpi, denom);
    }
    Some(log_font)
}

/// UI 用の既定フォントを取得する。原実装 `GetDefaultUIFont`(DrawUtil.cpp:761)。
///
/// メイリオは行間が空きすぎるため Meiryo UI に差し替える。
pub fn get_default_ui_font() -> Option<LOGFONTW> {
    let mut font = LOGFONTW::default();
    if let Some(message_font) = get_system_font(FontType::Message) {
        if wide_eq_str(&message_font.lfFaceName, "メイリオ")
            || wide_eq_str_ci(&message_font.lfFaceName, "Meiryo")
        {
            font.lfHeight = -message_font.lfHeight.abs();
            font.lfWeight = FW_NORMAL.0 as i32;
            set_face_name(&mut font, "Meiryo UI");
            if is_font_available(&font, None) {
                return Some(font);
            }
        } else {
            return Some(message_font);
        }
    }

    // フォールバック: DEFAULT_GUI_FONT。
    // 原実装(DrawUtil.cpp:785)はこのフォールバックの戻り値が反転している(通常到達しない)が、
    // ここでは取得成功時に Some を返す。
    // SAFETY: font は有効なバッファ。
    let got = unsafe {
        GetObjectW(
            GetStockObject(DEFAULT_GUI_FONT),
            size_of::<LOGFONTW>() as i32,
            Some(&mut font as *mut LOGFONTW as *mut c_void),
        )
    };
    if got == size_of::<LOGFONTW>() as i32 {
        Some(font)
    } else {
        None
    }
}

/// 指定フォントが利用可能か(実体が同名で選択されるか)。原実装 `IsFontAvailable`(DrawUtil.cpp:789)。
///
/// `hdc` が `None` のときは一時メモリ DC を使う。
pub fn is_font_available(font: &LOGFONTW, hdc: Option<HDC>) -> bool {
    // SAFETY: font は有効。失敗時 NULL。
    let hfont = unsafe { CreateFontIndirectW(font) };
    if hfont.0.is_null() {
        return false;
    }

    let (work_hdc, mem_dc) = match hdc {
        Some(h) if !h.0.is_null() => (h, None),
        _ => {
            // SAFETY: 失敗時 NULL。
            let m = unsafe { CreateCompatibleDC(None) };
            if m.0.is_null() {
                // 原実装はここで hfont を解放しないが(リーク)、本移植では解放する。
                unsafe {
                    let _ = DeleteObject(HGDIOBJ(hfont.0));
                }
                return false;
            }
            (m, Some(m))
        }
    };

    // SAFETY: work_hdc/hfont は有効。
    let available = unsafe {
        let old = SelectObject(work_hdc, HGDIOBJ(hfont.0));
        let mut face = [0u16; 32]; // LF_FACESIZE
        let len = GetTextFaceW(work_hdc, Some(&mut face));
        let result = len > 0 && wide_eq_ci(&face, &font.lfFaceName);
        let _ = SelectObject(work_hdc, old);
        result
    };

    // SAFETY: 一時 DC とフォントを破棄する(原実装は hfont を解放しないが本移植では解放)。
    unsafe {
        if let Some(m) = mem_dc {
            let _ = DeleteDC(m);
        }
        let _ = DeleteObject(HGDIOBJ(hfont.0));
    }

    available
}

/// フォントスムージングが有効か。原実装 `IsFontSmoothingEnabled`(DrawUtil.cpp:815)。
pub fn is_font_smoothing_enabled() -> bool {
    let mut enabled: i32 = 0;
    // SAFETY: enabled は有効なバッファ。
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETFONTSMOOTHING,
            0,
            Some(&mut enabled as *mut i32 as *mut c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    }
    .is_ok();
    ok && enabled != 0
}

/// ClearType が有効か。原実装 `IsClearTypeEnabled`(DrawUtil.cpp:822)。
pub fn is_clear_type_enabled() -> bool {
    if !is_font_smoothing_enabled() {
        return false;
    }
    let mut smoothing_type: u32 = 0;
    // SAFETY: smoothing_type は有効なバッファ。
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETFONTSMOOTHINGTYPE,
            0,
            Some(&mut smoothing_type as *mut u32 as *mut c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    }
    .is_ok();
    ok && smoothing_type == FE_FONTSMOOTHINGCLEARTYPE
}

/// フォント(`HFONT`)の RAII ラッパー。原実装 `DrawUtil::CFont`(DrawUtil.h:115)。
///
/// `Drop`(`~CFont`)で破棄する。`Clone`(コピーコンストラクタ/代入)は `LOGFONT` を取得して
/// 作り直す(DrawUtil.cpp:851)。`PartialEq` は `CompareLogFont` による比較(DrawUtil.cpp:866)。
pub struct Font {
    hfont: HFONT,
}

impl Font {
    /// 空のフォント(未生成)を作る。
    pub fn new() -> Self {
        Self {
            hfont: HFONT::default(),
        }
    }

    /// `LOGFONT` から生成する(`CFont(const LOGFONT&)`、DrawUtil.cpp:836)。
    pub fn from_log_font(log_font: &LOGFONTW) -> Self {
        let mut font = Self::new();
        font.create(log_font);
        font
    }

    /// 種別から生成する(`CFont(FontType)`、DrawUtil.cpp:841)。
    pub fn from_font_type(font_type: FontType) -> Self {
        let mut font = Self::new();
        font.create_from_type(font_type);
        font
    }

    /// `LOGFONT` からフォントを生成する。`CFont::Create`(DrawUtil.cpp:878)。
    pub fn create(&mut self, log_font: &LOGFONTW) -> bool {
        // SAFETY: log_font は有効。失敗時 NULL。
        let hfont = unsafe { CreateFontIndirectW(log_font) };
        if hfont.0.is_null() {
            return false;
        }
        if !self.hfont.0.is_null() {
            unsafe {
                let _ = DeleteObject(HGDIOBJ(self.hfont.0));
            }
        }
        self.hfont = hfont;
        true
    }

    /// 種別からフォントを生成する。`CFont::Create(FontType)`(DrawUtil.cpp:891)。
    pub fn create_from_type(&mut self, font_type: FontType) -> bool {
        match get_system_font(font_type) {
            Some(log_font) => self.create(&log_font),
            None => false,
        }
    }

    /// 生成済みかどうか。
    pub fn is_created(&self) -> bool {
        !self.hfont.0.is_null()
    }

    /// フォントを破棄する。`CFont::Destroy`(DrawUtil.cpp:900)。
    pub fn destroy(&mut self) {
        if !self.hfont.0.is_null() {
            // SAFETY: 自身が所有するフォント。
            unsafe {
                let _ = DeleteObject(HGDIOBJ(self.hfont.0));
            }
            self.hfont = HFONT::default();
        }
    }

    /// ハンドルを取得する(`GetHandle`、DrawUtil.h:134)。
    pub fn handle(&self) -> HFONT {
        self.hfont
    }

    /// `LOGFONT` を取得する。`CFont::GetLogFont`(DrawUtil.cpp:908)。
    pub fn get_log_font(&self) -> Option<LOGFONTW> {
        if self.hfont.0.is_null() {
            return None;
        }
        let mut log_font = LOGFONTW::default();
        // SAFETY: hfont は有効、log_font は LOGFONTW 用バッファ。
        let got = unsafe {
            GetObjectW(
                HGDIOBJ(self.hfont.0),
                size_of::<LOGFONTW>() as i32,
                Some(&mut log_font as *mut LOGFONTW as *mut c_void),
            )
        };
        if got == size_of::<LOGFONTW>() as i32 {
            Some(log_font)
        } else {
            None
        }
    }

    /// 高さを取得する。`CFont::GetHeight`(DrawUtil.cpp:915)。
    ///
    /// DC を作れない場合は `|lfHeight|` を返す。
    pub fn get_height(&self, cell: bool) -> i32 {
        if self.hfont.0.is_null() {
            return 0;
        }
        // SAFETY: 失敗時 NULL。
        let hdc = unsafe { CreateCompatibleDC(None) };
        if hdc.0.is_null() {
            return self.get_log_font().map_or(0, |lf| lf.lfHeight.abs());
        }
        let height = self.get_height_dc(hdc, cell);
        // SAFETY: 自身が作った DC。
        unsafe {
            let _ = DeleteDC(hdc);
        }
        height
    }

    /// DC を指定して高さを取得する。`CFont::GetHeight(HDC)`(DrawUtil.cpp:934)。
    pub fn get_height_dc(&self, hdc: HDC, cell: bool) -> i32 {
        if self.hfont.0.is_null() || hdc.0.is_null() {
            return 0;
        }
        // SAFETY: hfont/hdc は有効。
        let mut tm = TEXTMETRICW::default();
        unsafe {
            let old = SelectObject(hdc, HGDIOBJ(self.hfont.0));
            let _ = GetTextMetricsW(hdc, &mut tm);
            let _ = SelectObject(hdc, old);
        }
        let mut height = tm.tmHeight;
        if !cell {
            height -= tm.tmInternalLeading;
        }
        height
    }
}

impl Default for Font {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for Font {
    fn clone(&self) -> Self {
        // CFont::operator=(DrawUtil.cpp:851): LOGFONT を取得して作り直す。
        let mut font = Self::new();
        if let Some(log_font) = self.get_log_font() {
            font.create(&log_font);
        }
        font
    }
}

impl PartialEq for Font {
    fn eq(&self, other: &Self) -> bool {
        // CFont::operator==(DrawUtil.cpp:866)
        if self.hfont.0.is_null() {
            return other.hfont.0.is_null();
        }
        if other.hfont.0.is_null() {
            return false;
        }
        match (self.get_log_font(), other.get_log_font()) {
            (Some(a), Some(b)) => compare_log_font(&a, &b),
            _ => false,
        }
    }
}

impl Drop for Font {
    fn drop(&mut self) {
        self.destroy();
    }
}

// ---------------------------------------------------------------------------
// GDI ハンドルの RAII ラッパー(DrawUtil.cpp / DrawUtil.h)
// ---------------------------------------------------------------------------

/// ソリッドブラシの RAII ラッパー。原実装 `DrawUtil::CBrush`(DrawUtil.h:143)。
///
/// 生成したブラシは `Drop`(`~CBrush`)で破棄する。`Clone` は原実装のコピーコンストラクタ
/// (`LOGBRUSH` を取得して `CreateBrushIndirect` で複製、DrawUtil.cpp:963)に対応する。
pub struct Brush {
    hbr: HBRUSH,
}

impl Brush {
    /// 空のブラシ(未生成)を作る。
    pub fn new() -> Self {
        Self {
            hbr: HBRUSH::default(),
        }
    }

    /// 色を指定して生成する(`CBrush(COLORREF)`、DrawUtil.cpp:953)。
    pub fn with_color(color: u32) -> Self {
        let mut brush = Self::new();
        brush.create(color);
        brush
    }

    /// ソリッドブラシを生成する。`CBrush::Create`(DrawUtil.cpp:977)。
    pub fn create(&mut self, color: u32) -> bool {
        // SAFETY: CreateSolidBrush は失敗時 NULL を返すだけ。
        let hbr = unsafe { CreateSolidBrush(COLORREF(color)) };
        if hbr.0.is_null() {
            return false;
        }
        self.destroy();
        self.hbr = hbr;
        true
    }

    /// 生成済みかどうか。
    pub fn is_created(&self) -> bool {
        !self.hbr.0.is_null()
    }

    /// ブラシを破棄する。`CBrush::Destroy`(DrawUtil.cpp:988)。
    pub fn destroy(&mut self) {
        if !self.hbr.0.is_null() {
            // SAFETY: 自身が所有するブラシハンドルのみ破棄する。
            unsafe {
                let _ = DeleteObject(HGDIOBJ(self.hbr.0));
            }
            self.hbr = HBRUSH::default();
        }
    }

    /// ハンドルを取得する(`GetHandle`、DrawUtil.h:158)。
    pub fn handle(&self) -> HBRUSH {
        self.hbr
    }
}

impl Default for Brush {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for Brush {
    fn clone(&self) -> Self {
        // CBrush::operator=(DrawUtil.cpp:963): LOGBRUSH を取得して複製する。
        let mut brush = Self::new();
        if !self.hbr.0.is_null() {
            let mut lb = LOGBRUSH::default();
            // SAFETY: hbr は有効なブラシ、lb は LOGBRUSH 用のバッファ。
            let got = unsafe {
                GetObjectW(
                    HGDIOBJ(self.hbr.0),
                    size_of::<LOGBRUSH>() as i32,
                    Some(&mut lb as *mut LOGBRUSH as *mut c_void),
                )
            };
            if got == size_of::<LOGBRUSH>() as i32 {
                brush.hbr = unsafe { CreateBrushIndirect(&lb) };
            }
        }
        brush
    }
}

impl Drop for Brush {
    fn drop(&mut self) {
        self.destroy();
    }
}

/// メモリ DC の RAII ラッパー。原実装 `DrawUtil::CMemoryDC`(DrawUtil.h:272)。
///
/// 生成時に元のビットマップを保持し、`Drop`(`~CMemoryDC`)で選択を戻して DC を破棄する。
/// 原実装はコピー不可のため `Clone` は実装しない。
pub struct MemoryDc {
    hdc: HDC,
    hbm_old: HBITMAP,
}

impl MemoryDc {
    /// 空のメモリ DC(未生成)を作る。
    pub fn new() -> Self {
        Self {
            hdc: HDC::default(),
            hbm_old: HBITMAP::default(),
        }
    }

    /// 指定 DC と互換のメモリ DC を生成する(`CMemoryDC(HDC)`、DrawUtil.cpp:1569)。
    pub fn with_dc(hdc: HDC) -> Self {
        let mut dc = Self::new();
        dc.create(Some(hdc));
        dc
    }

    /// メモリ DC を生成する。`CMemoryDC::Create`(DrawUtil.cpp:1578)。
    ///
    /// `hdc` が `None` のときは画面と互換の DC を作る。
    pub fn create(&mut self, hdc: Option<HDC>) -> bool {
        self.delete();
        // SAFETY: CreateCompatibleDC は失敗時 NULL を返す。
        let mdc = unsafe { CreateCompatibleDC(hdc) };
        if mdc.0.is_null() {
            return false;
        }
        self.hdc = mdc;
        // SAFETY: hdc は有効。GetCurrentObject で現在のビットマップを保持する。
        self.hbm_old = HBITMAP(unsafe { GetCurrentObject(self.hdc, OBJ_BITMAP) }.0);
        true
    }

    /// メモリ DC を破棄する。`CMemoryDC::Delete`(DrawUtil.cpp:1589)。
    pub fn delete(&mut self) {
        if !self.hdc.0.is_null() {
            // SAFETY: 自身が所有する DC のみ操作する。
            unsafe {
                let _ = SelectObject(self.hdc, HGDIOBJ(self.hbm_old.0));
                let _ = DeleteDC(self.hdc);
            }
            self.hdc = HDC::default();
        }
    }

    /// 生成済みかどうか。
    pub fn is_created(&self) -> bool {
        !self.hdc.0.is_null()
    }

    /// DC ハンドルを取得する。
    pub fn dc(&self) -> HDC {
        self.hdc
    }

    /// ビットマップを選択する。`CMemoryDC::SetBitmap`(DrawUtil.cpp:1598)。
    pub fn set_bitmap(&self, hbm: HBITMAP) -> bool {
        if self.hdc.0.is_null() || hbm.0.is_null() {
            return false;
        }
        // SAFETY: hdc/hbm はともに有効。
        unsafe {
            let _ = SelectObject(self.hdc, HGDIOBJ(hbm.0));
        }
        true
    }

    /// `BitBlt` で転送する。`CMemoryDC::Draw`(DrawUtil.cpp:1606)。
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        hdc: HDC,
        dst_x: i32,
        dst_y: i32,
        src_x: i32,
        src_y: i32,
        width: i32,
        height: i32,
    ) -> bool {
        if self.hdc.0.is_null() || hdc.0.is_null() || width < 1 || height < 1 {
            return false;
        }
        // SAFETY: 双方の DC は有効、サイズは正。
        unsafe { BitBlt(hdc, dst_x, dst_y, width, height, Some(self.hdc), src_x, src_y, SRCCOPY) }
            .is_ok()
    }

    /// `StretchBlt` で拡縮転送する。`CMemoryDC::DrawStretch`(DrawUtil.cpp:1613)。
    #[allow(clippy::too_many_arguments)]
    pub fn draw_stretch(
        &self,
        hdc: HDC,
        dst_x: i32,
        dst_y: i32,
        dst_width: i32,
        dst_height: i32,
        src_x: i32,
        src_y: i32,
        src_width: i32,
        src_height: i32,
        mode: STRETCH_BLT_MODE,
    ) -> bool {
        if self.hdc.0.is_null() || hdc.0.is_null() {
            return false;
        }
        // SAFETY: 双方の DC は有効。元の伸縮モードを保存して復元する。
        unsafe {
            let old = SetStretchBltMode(hdc, mode);
            let _ = StretchBlt(
                hdc,
                dst_x,
                dst_y,
                dst_width,
                dst_height,
                Some(self.hdc),
                src_x,
                src_y,
                src_width,
                src_height,
                SRCCOPY,
            );
            SetStretchBltMode(hdc, STRETCH_BLT_MODE(old));
        }
        true
    }

    /// `AlphaBlend`(全面不透明・ソースアルファ使用)で転送する。`CMemoryDC::DrawAlpha`(DrawUtil.cpp:1625)。
    #[allow(clippy::too_many_arguments)]
    pub fn draw_alpha(
        &self,
        hdc: HDC,
        dst_x: i32,
        dst_y: i32,
        src_x: i32,
        src_y: i32,
        width: i32,
        height: i32,
    ) -> bool {
        if self.hdc.0.is_null() || hdc.0.is_null() {
            return false;
        }
        let bf = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        // SAFETY: 双方の DC は有効。
        unsafe {
            let _ = AlphaBlend(
                hdc, dst_x, dst_y, width, height, self.hdc, src_x, src_y, width, height, bf,
            );
        }
        true
    }
}

impl Default for MemoryDc {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for MemoryDc {
    fn drop(&mut self) {
        self.delete();
    }
}

/// オフスクリーン(メモリ DC + ビットマップ)の RAII ラッパー。原実装 `DrawUtil::COffscreen`(DrawUtil.h:299)。
///
/// `Drop`(`~COffscreen`)で選択を戻して DC・ビットマップを破棄する。原実装はコピー不可のため
/// `Clone` は実装しない。
pub struct Offscreen {
    hdc: HDC,
    hbm: HBITMAP,
    hbm_old: HBITMAP,
    width: i32,
    height: i32,
}

impl Offscreen {
    /// 空のオフスクリーン(未生成)を作る。
    pub fn new() -> Self {
        Self {
            hdc: HDC::default(),
            hbm: HBITMAP::default(),
            hbm_old: HBITMAP::default(),
            width: 0,
            height: 0,
        }
    }

    /// オフスクリーンを生成する。`COffscreen::Create`(DrawUtil.cpp:1640)。
    ///
    /// `hdc` が `None` のときは画面 DC を基準にする。
    pub fn create(&mut self, width: i32, height: i32, hdc: Option<HDC>) -> bool {
        if width <= 0 || height <= 0 {
            return false;
        }
        self.destroy();

        // hdc が None なら画面 DC を取得する(使い終わったら解放)。
        let (work_hdc, screen) = match hdc {
            Some(h) => (h, None),
            None => {
                // SAFETY: GetDC は失敗時 NULL を返す。
                let s = unsafe { GetDC(None) };
                if s.0.is_null() {
                    return false;
                }
                (s, Some(s))
            }
        };

        // SAFETY: work_hdc は有効。
        let mdc = unsafe { CreateCompatibleDC(Some(work_hdc)) };
        if mdc.0.is_null() {
            if let Some(s) = screen {
                unsafe {
                    ReleaseDC(None, s);
                }
            }
            return false;
        }
        self.hdc = mdc;

        // SAFETY: work_hdc は有効。
        let bmp = unsafe { CreateCompatibleBitmap(work_hdc, width, height) };
        if let Some(s) = screen {
            unsafe {
                ReleaseDC(None, s);
            }
        }
        if bmp.0.is_null() {
            self.destroy();
            return false;
        }
        self.hbm = bmp;
        // SAFETY: hdc/hbm は有効。元のビットマップを保持する。
        self.hbm_old = HBITMAP(unsafe { SelectObject(self.hdc, HGDIOBJ(self.hbm.0)) }.0);
        self.width = width;
        self.height = height;
        true
    }

    /// オフスクリーンを破棄する。`COffscreen::Destroy`(DrawUtil.cpp:1673)。
    pub fn destroy(&mut self) {
        if !self.hbm_old.0.is_null() {
            // SAFETY: 自身が所有する DC への選択を戻す。
            unsafe {
                let _ = SelectObject(self.hdc, HGDIOBJ(self.hbm_old.0));
            }
            self.hbm_old = HBITMAP::default();
        }
        if !self.hdc.0.is_null() {
            // SAFETY: 自身が所有する DC。
            unsafe {
                let _ = DeleteDC(self.hdc);
            }
            self.hdc = HDC::default();
        }
        if !self.hbm.0.is_null() {
            // SAFETY: 自身が所有するビットマップ。
            unsafe {
                let _ = DeleteObject(HGDIOBJ(self.hbm.0));
            }
            self.hbm = HBITMAP::default();
            self.width = 0;
            self.height = 0;
        }
    }

    /// 生成済みかどうか。
    pub fn is_created(&self) -> bool {
        !self.hdc.0.is_null()
    }

    /// DC ハンドルを取得する(`GetDC`、DrawUtil.h:317)。
    pub fn dc(&self) -> HDC {
        self.hdc
    }

    /// 幅を取得する。
    pub fn width(&self) -> i32 {
        self.width
    }

    /// 高さを取得する。
    pub fn height(&self) -> i32 {
        self.height
    }

    /// 内容を別の DC へ転送する。`COffscreen::CopyTo`(DrawUtil.cpp:1691)。
    pub fn copy_to(&self, hdc: HDC, dst_rect: Option<RECT>) -> bool {
        if self.hdc.0.is_null() || hdc.0.is_null() {
            return false;
        }
        let (dst_x, dst_y, width, height) = match dst_rect {
            Some(r) => {
                let mut w = r.right - r.left;
                let mut h = r.bottom - r.top;
                if w <= 0 || h <= 0 {
                    return false;
                }
                if w > self.width {
                    w = self.width;
                }
                if h > self.height {
                    h = self.height;
                }
                (r.left, r.top, w, h)
            }
            None => (0, 0, self.width, self.height),
        };
        // SAFETY: 双方の DC は有効。
        unsafe {
            let _ = BitBlt(hdc, dst_x, dst_y, width, height, Some(self.hdc), 0, 0, SRCCOPY);
        }
        true
    }
}

impl Default for Offscreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Offscreen {
    fn drop(&mut self) {
        self.destroy();
    }
}

// ---------------------------------------------------------------------------
// DIB(ビットマップ)自由関数と Bitmap ラッパー(DrawUtil.cpp / DrawUtil.h)
// ---------------------------------------------------------------------------

/// 32bpp 以下にも対応するパレット領域付きで DIB セクションを作る内部処理。
///
/// 成功時は `(HBITMAP, ピクセル先頭ポインタ)`。原実装 `CreateDIB`(DrawUtil.cpp:568)に対応。
fn create_dib_with_bits(width: i32, height: i32, bit_count: u16) -> Option<(HBITMAP, *mut c_void)> {
    // BITMAPINFOHEADER + 最大 256 エントリのパレット(原実装と同じ確保)。
    #[repr(C)]
    struct DibInfo256 {
        header: BITMAPINFOHEADER,
        colors: [RGBQUAD; 256],
    }
    let info = DibInfo256 {
        header: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: height,
            biPlanes: 1,
            biBitCount: bit_count,
            biCompression: 0, // BI_RGB
            ..Default::default()
        },
        colors: [RGBQUAD::default(); 256],
    };
    let mut pbits: *mut c_void = null_mut();
    // SAFETY: info は有効。CreateDIBSection が pbits にピクセル先頭を返す。
    match unsafe {
        CreateDIBSection(
            None,
            &info as *const DibInfo256 as *const BITMAPINFO,
            DIB_RGB_COLORS,
            &mut pbits,
            None,
            0,
        )
    } {
        Ok(hbm) if !hbm.0.is_null() => Some((hbm, pbits)),
        _ => None,
    }
}

/// DIB セクションを作る。原実装 `CreateDIB`(DrawUtil.cpp:568)。
pub fn create_dib(width: i32, height: i32, bit_count: u16) -> Option<HBITMAP> {
    create_dib_with_bits(width, height, bit_count).map(|(hbm, _)| hbm)
}

/// DIB を複製する(パレットも複製)。原実装 `DuplicateDIB`(DrawUtil.cpp:591)。
pub fn duplicate_dib(hbm_src: HBITMAP) -> Option<HBITMAP> {
    if hbm_src.0.is_null() {
        return None;
    }
    let mut bm = BITMAP::default();
    // SAFETY: hbm_src は有効。BITMAP 情報を取得する。
    let got = unsafe {
        GetObjectW(
            HGDIOBJ(hbm_src.0),
            size_of::<BITMAP>() as i32,
            Some(&mut bm as *mut BITMAP as *mut c_void),
        )
    };
    if got != size_of::<BITMAP>() as i32 || bm.bmBits.is_null() {
        return None;
    }

    let (hbm, pbits) = create_dib_with_bits(bm.bmWidth, bm.bmHeight, bm.bmBitsPixel)?;

    // ピクセルをコピー。
    let size = (bm.bmHeight * bm.bmWidthBytes) as usize;
    // SAFETY: 双方とも size バイトの有効なバッファ。
    unsafe {
        copy_nonoverlapping(bm.bmBits as *const u8, pbits as *mut u8, size);
    }

    // 8bpp 以下はカラーテーブルもコピー。
    if bm.bmBitsPixel <= 8 {
        // SAFETY: 一時 DC を作りカラーテーブルを転送する。
        unsafe {
            let hdc = CreateCompatibleDC(None);
            if hdc.0.is_null() {
                let _ = DeleteObject(HGDIOBJ(hbm.0));
                return None;
            }
            let count = 1u32 << bm.bmBitsPixel;
            let mut table = [RGBQUAD::default(); 256];
            let old = SelectObject(hdc, HGDIOBJ(hbm_src.0));
            GetDIBColorTable(hdc, 0, &mut table[..count as usize]);
            SelectObject(hdc, HGDIOBJ(hbm.0));
            SetDIBColorTable(hdc, 0, &table[..count as usize]);
            SelectObject(hdc, old);
            let _ = DeleteDC(hdc);
        }
    }

    Some(hbm)
}

/// ビットマップを拡縮した DIB を作る。原実装 `ResizeBitmap`(DrawUtil.cpp:627)。
pub fn resize_bitmap(
    hbm_src: HBITMAP,
    width: i32,
    height: i32,
    bit_count: u16,
    stretch_mode: STRETCH_BLT_MODE,
) -> Option<HBITMAP> {
    if hbm_src.0.is_null() || width < 1 || height == 0 {
        return None;
    }
    let hbm = create_dib(width, height, bit_count)?;

    // SAFETY: 一時 DC を 2 つ作り StretchBlt で転送する。
    let ok = unsafe {
        let hdc_src = CreateCompatibleDC(None);
        let hdc_dst = CreateCompatibleDC(None);
        let ok = !hdc_src.0.is_null() && !hdc_dst.0.is_null();
        if ok {
            let src_old = SelectObject(hdc_src, HGDIOBJ(hbm_src.0));
            let dst_old = SelectObject(hdc_dst, HGDIOBJ(hbm.0));
            let old_mode = SetStretchBltMode(hdc_dst, stretch_mode);
            let mut bm = BITMAP::default();
            let _ = GetObjectW(
                HGDIOBJ(hbm_src.0),
                size_of::<BITMAP>() as i32,
                Some(&mut bm as *mut BITMAP as *mut c_void),
            );
            let _ = StretchBlt(
                hdc_dst,
                0,
                0,
                width,
                height.abs(),
                Some(hdc_src),
                0,
                0,
                bm.bmWidth,
                bm.bmHeight,
                SRCCOPY,
            );
            SetStretchBltMode(hdc_dst, STRETCH_BLT_MODE(old_mode));
            let _ = SelectObject(hdc_dst, dst_old);
            let _ = SelectObject(hdc_src, src_old);
        }
        if !hdc_dst.0.is_null() {
            let _ = DeleteDC(hdc_dst);
        }
        if !hdc_src.0.is_null() {
            let _ = DeleteDC(hdc_src);
        }
        ok
    };

    if !ok {
        // SAFETY: 失敗時は作成した DIB を破棄。
        unsafe {
            let _ = DeleteObject(HGDIOBJ(hbm.0));
        }
        return None;
    }
    Some(hbm)
}

/// ビットマップ(`HBITMAP`)の RAII ラッパー。原実装 `DrawUtil::CBitmap`(DrawUtil.h:161)。
///
/// `Drop`(`~CBitmap`)で破棄する。`Clone`(コピーコンストラクタ/代入、DrawUtil.cpp:1007)は
/// DIB なら [`duplicate_dib`]、それ以外は `CopyImage` で複製する。
pub struct Bitmap {
    hbm: HBITMAP,
}

impl Bitmap {
    /// 空のビットマップ(未生成)を作る。
    pub fn new() -> Self {
        Self {
            hbm: HBITMAP::default(),
        }
    }

    /// DIB を生成する。`CBitmap::Create`(DrawUtil.cpp:1021)。
    pub fn create(&mut self, width: i32, height: i32, bit_count: u16) -> bool {
        self.destroy();
        match create_dib(width, height, bit_count) {
            Some(hbm) => {
                self.hbm = hbm;
                true
            }
            None => false,
        }
    }

    /// `BITMAPINFO`(ヘッダ + パレット + ピクセル)のバイト列から生成する。
    /// 原実装 `CBitmap::Create(const BITMAPINFO*, size_t)`(DrawUtil.cpp:1028)。
    ///
    /// `data` は先頭が `BITMAPINFOHEADER` の連続バッファ。情報部の後ろにピクセルがあればコピーする。
    pub fn create_from_dib_data(&mut self, data: &[u8]) -> bool {
        self.destroy();
        if data.len() < size_of::<BITMAPINFOHEADER>() {
            return false;
        }
        // SAFETY: data は十分な長さがあり、先頭は BITMAPINFOHEADER。
        let header = unsafe { &*(data.as_ptr() as *const BITMAPINFOHEADER) };
        let info_size = tvtest_util::calc_dib_info_size(
            header.biSize,
            header.biBitCount,
            header.biCompression,
        );
        if info_size > data.len() {
            return false;
        }

        let mut pbits: *mut c_void = null_mut();
        // SAFETY: data 先頭を BITMAPINFO として渡す。
        let hbm = match unsafe {
            CreateDIBSection(
                None,
                data.as_ptr() as *const BITMAPINFO,
                DIB_RGB_COLORS,
                &mut pbits,
                None,
                0,
            )
        } {
            Ok(h) if !h.0.is_null() => h,
            _ => return false,
        };

        if data.len() > info_size {
            let bits_size =
                tvtest_util::calc_dib_bits_size(header.biWidth, header.biBitCount, header.biHeight);
            if bits_size <= data.len() - info_size {
                // SAFETY: コピー元/先とも bits_size バイト以上の有効領域。
                unsafe {
                    copy_nonoverlapping(data.as_ptr().add(info_size), pbits as *mut u8, bits_size);
                }
            }
        }

        self.hbm = hbm;
        true
    }

    /// 既存ハンドルの所有権を受け取る。`CBitmap::Attach`(DrawUtil.cpp:1063)。
    pub fn attach(&mut self, hbm: HBITMAP) -> bool {
        if hbm.0.is_null() {
            return false;
        }
        self.destroy();
        self.hbm = hbm;
        true
    }

    /// 生成済みかどうか。
    pub fn is_created(&self) -> bool {
        !self.hbm.0.is_null()
    }

    /// ビットマップを破棄する。`CBitmap::Destroy`(DrawUtil.cpp:1072)。
    pub fn destroy(&mut self) {
        if !self.hbm.0.is_null() {
            // SAFETY: 自身が所有するビットマップ。
            unsafe {
                let _ = DeleteObject(HGDIOBJ(self.hbm.0));
            }
            self.hbm = HBITMAP::default();
        }
    }

    /// ハンドルを取得する(`GetHandle`、DrawUtil.h:181)。
    pub fn handle(&self) -> HBITMAP {
        self.hbm
    }

    /// DIB セクションかどうか。`CBitmap::IsDIB`(DrawUtil.cpp:1080)。
    pub fn is_dib(&self) -> bool {
        if self.hbm.0.is_null() {
            return false;
        }
        let mut ds = DIBSECTION::default();
        // SAFETY: hbm は有効。DIBSECTION 取得可なら DIB。
        let got = unsafe {
            GetObjectW(
                HGDIOBJ(self.hbm.0),
                size_of::<DIBSECTION>() as i32,
                Some(&mut ds as *mut DIBSECTION as *mut c_void),
            )
        };
        got == size_of::<DIBSECTION>() as i32
    }

    /// 幅。`CBitmap::GetWidth`(DrawUtil.cpp:1090)。
    pub fn width(&self) -> i32 {
        self.bitmap_info().map_or(0, |bm| bm.bmWidth)
    }

    /// 高さ。`CBitmap::GetHeight`(DrawUtil.cpp:1100)。
    pub fn height(&self) -> i32 {
        self.bitmap_info().map_or(0, |bm| bm.bmHeight)
    }

    /// `BITMAP` 情報を取得する(内部)。
    fn bitmap_info(&self) -> Option<BITMAP> {
        if self.hbm.0.is_null() {
            return None;
        }
        let mut bm = BITMAP::default();
        // SAFETY: hbm は有効。
        let got = unsafe {
            GetObjectW(
                HGDIOBJ(self.hbm.0),
                size_of::<BITMAP>() as i32,
                Some(&mut bm as *mut BITMAP as *mut c_void),
            )
        };
        if got == size_of::<BITMAP>() as i32 {
            Some(bm)
        } else {
            None
        }
    }
}

impl Default for Bitmap {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for Bitmap {
    fn clone(&self) -> Self {
        // CBitmap::operator=(DrawUtil.cpp:1007): DIB は DuplicateDIB、それ以外は CopyImage。
        let mut bitmap = Self::new();
        if !self.hbm.0.is_null() {
            bitmap.hbm = if self.is_dib() {
                duplicate_dib(self.hbm).unwrap_or_default()
            } else {
                // SAFETY: hbm は有効。CopyImage で複製。
                match unsafe {
                    CopyImage(HANDLE(self.hbm.0), IMAGE_BITMAP, 0, 0, IMAGE_FLAGS(0))
                } {
                    Ok(h) => HBITMAP(h.0),
                    Err(_) => HBITMAP::default(),
                }
            };
        }
        bitmap
    }
}

impl Drop for Bitmap {
    fn drop(&mut self) {
        self.destroy();
    }
}

// ---------------------------------------------------------------------------
// uxtheme テーマラッパー(DrawUtil.cpp / DrawUtil.h)
// ---------------------------------------------------------------------------

/// `TMT_TRANSITIONDURATIONS`(vssym32.h)。
const TMT_TRANSITIONDURATIONS: i32 = 6000;

type OpenThemeDataForDpiFn = unsafe extern "system" fn(HWND, PCWSTR, u32) -> HTHEME;

/// `OpenThemeDataForDpi` を動的に解決して呼ぶ。`GET_MODULE_FUNCTION`(Util.h:278)相当。
///
/// Windows 10 Creators Update 以降でのみ存在するため動的ロードする。
fn open_theme_data_for_dpi(hwnd: HWND, class_list: PCWSTR, dpi: u32) -> HTHEME {
    let hmodule = unsafe { GetModuleHandleW(w!("uxtheme.dll")) }.unwrap_or_default();
    match unsafe { GetProcAddress(hmodule, s!("OpenThemeDataForDpi")) } {
        Some(proc) => {
            // SAFETY: 解決した関数は OpenThemeDataForDpi のシグネチャと一致する。
            let f: OpenThemeDataForDpiFn = unsafe { transmute(proc) };
            unsafe { f(hwnd, class_list, dpi) }
        }
        None => HTHEME::default(),
    }
}

/// uxtheme のテーマ(`HTHEME`)の RAII ラッパー。原実装 `TVTest::CUxTheme`(DrawUtil.h:334)。
///
/// `Drop`(`~CUxTheme`)で `CloseThemeData` する。
pub struct UxTheme {
    htheme: HTHEME,
}

impl UxTheme {
    /// 空(未オープン)のラッパーを作る。
    pub fn new() -> Self {
        Self {
            htheme: HTHEME::default(),
        }
    }

    /// テーマを開く。`CUxTheme::Open`(DrawUtil.cpp:1743)。
    ///
    /// `dpi > 0` かつ Windows 10 Creators Update 以降なら `OpenThemeDataForDpi`(動的)を試し、
    /// 失敗時は `OpenThemeData` にフォールバックする。
    pub fn open(&mut self, hwnd: HWND, class_list: PCWSTR, dpi: i32) -> bool {
        self.close();

        if dpi > 0 && is_windows_10_creators_update_or_later() {
            let htheme = open_theme_data_for_dpi(hwnd, class_list, dpi as u32);
            if htheme.0 != 0 {
                self.htheme = htheme;
                return true;
            }
        }

        // SAFETY: class_list は有効な NUL 終端ワイド文字列。
        let htheme = unsafe { OpenThemeData(Some(hwnd), class_list) };
        if htheme.0 == 0 {
            return false;
        }
        self.htheme = htheme;
        true
    }

    /// テーマを閉じる。`CUxTheme::Close`(DrawUtil.cpp:1766)。
    pub fn close(&mut self) {
        if self.htheme.0 != 0 {
            // SAFETY: 自身が所有するテーマハンドル。
            unsafe {
                let _ = CloseThemeData(self.htheme);
            }
            self.htheme = HTHEME::default();
        }
    }

    /// 開いているか。`CUxTheme::IsOpen`(DrawUtil.cpp:1774)。
    pub fn is_open(&self) -> bool {
        self.htheme.0 != 0
    }

    /// アプリにテーマが適用されているか。`CUxTheme::IsActive`(DrawUtil.cpp:1779)。
    pub fn is_active(&self) -> bool {
        // SAFETY: 引数なしの問い合わせ。
        unsafe { IsAppThemed() }.as_bool()
    }

    /// 背景を描画する。`CUxTheme::DrawBackground`(DrawUtil.cpp:1784)。
    pub fn draw_background(&self, hdc: HDC, part_id: i32, state_id: i32, rect: &RECT) -> bool {
        if self.htheme.0 == 0 {
            return false;
        }
        // SAFETY: htheme/hdc は有効。クリップ矩形なし。
        unsafe { DrawThemeBackground(self.htheme, hdc, part_id, state_id, rect, None) }.is_ok()
    }

    /// 半透明部分は下地を描いてから背景を描画する。`CUxTheme::DrawBackground`(下地版、DrawUtil.cpp:1791)。
    pub fn draw_background_with_base(
        &self,
        hdc: HDC,
        part_id: i32,
        state_id: i32,
        base_part_id: i32,
        base_state_id: i32,
        rect: &RECT,
    ) -> bool {
        if self.htheme.0 == 0 {
            return false;
        }
        // SAFETY: htheme/hdc は有効。
        unsafe {
            if IsThemeBackgroundPartiallyTransparent(self.htheme, part_id, state_id).as_bool()
                && DrawThemeBackground(self.htheme, hdc, base_part_id, base_state_id, rect, None)
                    .is_err()
            {
                return false;
            }
            DrawThemeBackground(self.htheme, hdc, part_id, state_id, rect, None).is_ok()
        }
    }

    /// テーマテキストを描画する。`CUxTheme::DrawText`(DrawUtil.cpp:1808)。
    ///
    /// `text` は NUL を含まない文字列。
    pub fn draw_text(
        &self,
        hdc: HDC,
        part_id: i32,
        state_id: i32,
        text: &[u16],
        text_flags: u32,
        rect: &RECT,
    ) -> bool {
        if self.htheme.0 == 0 {
            return false;
        }
        // SAFETY: htheme/hdc は有効、text は有効なスライス。
        unsafe {
            DrawThemeText(
                self.htheme,
                hdc,
                part_id,
                state_id,
                text,
                DRAW_TEXT_FORMAT(text_flags),
                0,
                rect,
            )
        }
        .is_ok()
    }

    /// テーマテキストの外接矩形を取得する。`CUxTheme::GetTextExtent`(DrawUtil.cpp:1819)。
    pub fn get_text_extent(
        &self,
        hdc: HDC,
        part_id: i32,
        state_id: i32,
        text: &[u16],
        text_flags: u32,
    ) -> Option<RECT> {
        if self.htheme.0 == 0 {
            return None;
        }
        // SAFETY: htheme/hdc は有効、text は有効なスライス。外接矩形は戻り値で返る。
        unsafe {
            GetThemeTextExtent(
                self.htheme,
                hdc,
                part_id,
                state_id,
                text,
                DRAW_TEXT_FORMAT(text_flags),
                None,
            )
        }
        .ok()
    }

    /// マージンを取得する。`CUxTheme::GetMargins`(DrawUtil.cpp:1831)。
    pub fn get_margins(&self, part_id: i32, state_id: i32, prop_id: i32) -> Option<MARGINS> {
        if self.htheme.0 == 0 {
            return None;
        }
        // SAFETY: マージンは戻り値で返る。
        unsafe {
            GetThemeMargins(
                self.htheme,
                None,
                part_id,
                state_id,
                THEME_PROPERTY_SYMBOL_ID(prop_id as u32),
                None,
            )
        }
        .ok()
    }

    /// 色を取得する。`CUxTheme::GetColor`(DrawUtil.cpp:1838)。
    pub fn get_color(&self, part_id: i32, state_id: i32, prop_id: i32) -> Option<u32> {
        if self.htheme.0 == 0 {
            return None;
        }
        // SAFETY: 色は戻り値で返る。
        unsafe {
            GetThemeColor(self.htheme, part_id, state_id, THEME_PROPERTY_SYMBOL_ID(prop_id as u32))
        }
        .ok()
        .map(|c| c.0)
    }

    /// フォントを取得する。`CUxTheme::GetFont`(DrawUtil.cpp:1845)。
    pub fn get_font(&self, part_id: i32, state_id: i32, prop_id: i32) -> Option<LOGFONTW> {
        if self.htheme.0 == 0 {
            return None;
        }
        let mut log_font = LOGFONTW::default();
        // SAFETY: 出力バッファ log_font。
        let ok = unsafe {
            GetThemeFont(self.htheme, None, part_id, state_id, prop_id, &mut log_font)
        }
        .is_ok();
        ok.then_some(log_font)
    }

    /// システムフォントを取得する。`CUxTheme::GetSysFont`(DrawUtil.cpp:1852)。
    pub fn get_sys_font(&self, font_id: i32) -> Option<LOGFONTW> {
        if self.htheme.0 == 0 {
            return None;
        }
        let mut log_font = LOGFONTW::default();
        // SAFETY: 出力バッファ log_font。
        let ok = unsafe {
            GetThemeSysFont(Some(self.htheme), THEME_PROPERTY_SYMBOL_ID(font_id as u32), &mut log_font)
        }
        .is_ok();
        ok.then_some(log_font)
    }

    /// 整数プロパティを取得する。`CUxTheme::GetInt`(DrawUtil.cpp:1859)。
    pub fn get_int(&self, part_id: i32, state_id: i32, prop_id: i32) -> Option<i32> {
        if self.htheme.0 == 0 {
            return None;
        }
        // SAFETY: 整数値は戻り値で返る。
        unsafe { GetThemeInt(self.htheme, part_id, state_id, THEME_PROPERTY_SYMBOL_ID(prop_id as u32)) }
            .ok()
    }

    /// パーツの推奨サイズ(`TS_TRUE`)を取得する。`CUxTheme::GetPartSize`(DrawUtil.cpp:1867)。
    pub fn get_part_size(&self, hdc: HDC, part_id: i32, state_id: i32) -> Option<SIZE> {
        if self.htheme.0 == 0 {
            return None;
        }
        // SAFETY: htheme/hdc は有効。サイズは戻り値で返る。
        unsafe { GetThemePartSize(self.htheme, Some(hdc), part_id, state_id, None, TS_TRUE) }.ok()
    }

    /// 状態遷移の所要時間(ms)を取得する。`CUxTheme::GetTransitionDuration`(DrawUtil.cpp:1884)。
    pub fn get_transition_duration(
        &self,
        part_id: i32,
        state_id_from: i32,
        state_id_to: i32,
    ) -> Option<u32> {
        if self.htheme.0 == 0 {
            return None;
        }
        // SAFETY: 遷移時間は戻り値で返る。
        unsafe {
            GetThemeTransitionDuration(
                self.htheme,
                part_id,
                state_id_from,
                state_id_to,
                TMT_TRANSITIONDURATIONS,
            )
        }
        .ok()
    }
}

impl Default for UxTheme {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for UxTheme {
    fn drop(&mut self) {
        self.close();
    }
}

// ---------------------------------------------------------------------------
// テキスト描画(DrawUtil.cpp)
// ---------------------------------------------------------------------------

/// テキストを描画する。`DrawUtil::DrawText`(DrawUtil.cpp:668)。
///
/// 背景は常に透過(`SetBkMode(TRANSPARENT)`)で描画する。`font` が `Some` ならそのフォントを
/// 選択し、`color` が `Some` なら文字色を設定して描画する(いずれも描画後に元の値へ戻す)。
/// `hdc` が無効なら `false`。`text` は終端 NUL を含まないワイド文字列スライス。
///
/// 原実装は `RECT` をローカルにコピーして描画するため、`DT_CALCRECT` を指定しても
/// 呼び出し側の `rect` は変化しない(この挙動を踏襲する)。
pub fn draw_text(
    hdc: HDC,
    text: &[u16],
    rect: &RECT,
    format: DRAW_TEXT_FORMAT,
    font: Option<&Font>,
    color: Option<u32>,
) -> bool {
    if hdc.0.is_null() {
        return false;
    }
    // SAFETY: hdc は有効。変更した DC 状態(背景モード/文字色/フォント)は描画後に元へ戻す。
    unsafe {
        let old_bk_mode = SetBkMode(hdc, TRANSPARENT);
        let old_text_color = color.map(|c| SetTextColor(hdc, COLORREF(c)));
        let old_font = font.map(|f| SelectObject(hdc, HGDIOBJ(f.handle().0)));
        let mut buffer = text.to_vec();
        let mut rc = *rect;
        // 空文字列の描画は原実装でも no-op。空スライス(ダングリングポインタ)を
        // DrawTextW に渡さないようガードする。
        if !buffer.is_empty() {
            let _ = DrawTextW(hdc, &mut buffer, &mut rc, format);
        }
        if let Some(of) = old_font {
            SelectObject(hdc, of);
        }
        if let Some(otc) = old_text_color {
            SetTextColor(hdc, otc);
        }
        SetBkMode(hdc, BACKGROUND_MODE(old_bk_mode as u32));
    }
    true
}

// ---------------------------------------------------------------------------
// 単色化ビットマップ(DrawUtil::CMonoColorBitmap、DrawUtil.h:197 / DrawUtil.cpp:1111)
// ---------------------------------------------------------------------------

/// アルファチャンネルを持つ単色化可能なビットマップ。原実装 `DrawUtil::CMonoColorBitmap`。
///
/// 8/24bpp のソースからは「アルファのみ」を取り出し任意色で塗れる単色画像
/// (`color_image == false`)、32bpp のソースからはそのままのカラー画像
/// (`color_image == true`)を保持する。いずれも内部は 32bpp のプリマルチプライ済み
/// DIB として描画する。
///
/// リソースからの読み込み(`Load` = `LoadImage`)は実リソースが必要でテスト不能なため未移植
/// (続44-7 の [`Bitmap`] と同様)。`HBITMAP` を入力に取る [`create`](Self::create) を中核とする。
pub struct MonoColorBitmap {
    hbm: HBITMAP,
    hbm_premultiplied: HBITMAP,
    color: u32,
    color_image: bool,
}

impl MonoColorBitmap {
    /// 空(未生成)を作る。
    pub fn new() -> Self {
        Self {
            hbm: HBITMAP::default(),
            hbm_premultiplied: HBITMAP::default(),
            color: CLR_INVALID,
            color_image: false,
        }
    }

    /// ソースビットマップから生成する。`CMonoColorBitmap::Create`(DrawUtil.cpp:1171)。
    ///
    /// `hbm_src` は 8/24/32bpp の DIB(`bmBits` 有効)である必要がある。それ以外は `false`。
    pub fn create(&mut self, hbm_src: HBITMAP) -> bool {
        self.destroy();
        if hbm_src.0.is_null() {
            return false;
        }

        // SAFETY: hbm_src は GDI ビットマップハンドル。BITMAP 情報を取得する。
        let mut bm = BITMAP::default();
        unsafe {
            GetObjectW(
                HGDIOBJ(hbm_src.0),
                size_of::<BITMAP>() as i32,
                Some(&mut bm as *mut BITMAP as *mut c_void),
            );
        }
        if (bm.bmBitsPixel != 8 && bm.bmBitsPixel != 24 && bm.bmBitsPixel != 32)
            || bm.bmBits.is_null()
        {
            return false;
        }

        let Some((hbm, p_bits)) = create_dib_with_bits(bm.bmWidth, bm.bmHeight, 32) else {
            return false;
        };
        self.hbm = hbm;

        if bm.bmBitsPixel == 32 {
            self.color_image = true;
            // SAFETY: 32bpp は行パディングなし(幅*4=行バイト)のため連続コピー可。
            unsafe {
                copy_nonoverlapping(
                    bm.bmBits as *const u8,
                    p_bits as *mut u8,
                    (bm.bmWidth * 4 * bm.bmHeight) as usize,
                );
            }

            let Some((hbm_pm, pm_bits)) = create_dib_with_bits(bm.bmWidth, bm.bmHeight, 32) else {
                self.destroy();
                return false;
            };
            self.hbm_premultiplied = hbm_pm;
            // SAFETY: p_bits / pm_bits とも幅*高さ*4 バイト。各画素をプリマルチプライする。
            unsafe {
                let mut p = p_bits as *const u8;
                let mut q = pm_bits as *mut u8;
                for _ in 0..(bm.bmWidth * bm.bmHeight) {
                    let alpha = *p.add(3) as u32;
                    *q.add(0) = divide_by_255(*p.add(0) as u32 * alpha);
                    *q.add(1) = divide_by_255(*p.add(1) as u32 * alpha);
                    *q.add(2) = divide_by_255(*p.add(2) as u32 * alpha);
                    *q.add(3) = alpha as u8;
                    p = p.add(4);
                    q = q.add(4);
                }
            }
        } else {
            self.color_image = false;
            self.hbm_premultiplied = self.hbm;

            let row_bytes = ((bm.bmWidth * bm.bmBitsPixel as i32 + 31) / 32 * 4) as usize;
            // SAFETY: ソース行は row_bytes、出力は幅*4。先頭バイト(8bpp は値・24bpp は B)を
            // アルファとして取り出す。
            unsafe {
                let mut p = bm.bmBits as *const u8;
                let mut q = p_bits as *mut u8;
                for _ in 0..bm.bmHeight {
                    if bm.bmBitsPixel == 8 {
                        for x in 0..bm.bmWidth as usize {
                            *q.add(3) = *p.add(x);
                            q = q.add(4);
                        }
                    } else {
                        for x in 0..bm.bmWidth as usize {
                            *q.add(3) = *p.add(x * 3);
                            q = q.add(4);
                        }
                    }
                    p = p.add(row_bytes);
                }
            }
        }

        self.color = CLR_INVALID;
        true
    }

    /// 破棄する。`CMonoColorBitmap::Destroy`(DrawUtil.cpp:1240)。
    pub fn destroy(&mut self) {
        if !self.hbm_premultiplied.0.is_null() {
            if self.hbm_premultiplied.0 != self.hbm.0 {
                // SAFETY: 自身が所有するプリマルチプライ済み DIB(本体と別個の場合のみ解放)。
                unsafe {
                    let _ = DeleteObject(HGDIOBJ(self.hbm_premultiplied.0));
                }
            }
            self.hbm_premultiplied = HBITMAP::default();
        }
        if !self.hbm.0.is_null() {
            // SAFETY: 自身が所有する DIB。
            unsafe {
                let _ = DeleteObject(HGDIOBJ(self.hbm.0));
            }
            self.hbm = HBITMAP::default();
        }
    }

    /// ハンドルを取得する(`GetHandle`、DrawUtil.h:212)。
    pub fn handle(&self) -> HBITMAP {
        self.hbm
    }

    /// 生成済みかどうか(`IsCreated`、DrawUtil.h:213)。
    pub fn is_created(&self) -> bool {
        !self.hbm.0.is_null()
    }

    /// プリマルチプライ済み DIB を `color` で塗り直す。`CMonoColorBitmap::SetColor`(DrawUtil.cpp:1411)。
    fn set_color(&mut self, color: u32) {
        if self.color == color {
            return;
        }
        let mut bm = BITMAP::default();
        // SAFETY: hbm_premultiplied から BITMAP 取得(未生成なら 0 が返り何もしない)。
        let got = unsafe {
            GetObjectW(
                HGDIOBJ(self.hbm_premultiplied.0),
                size_of::<BITMAP>() as i32,
                Some(&mut bm as *mut BITMAP as *mut c_void),
            )
        };
        if got != size_of::<BITMAP>() as i32 {
            return;
        }
        let (red, green, blue) = (get_r(color), get_g(color), get_b(color));
        // SAFETY: bm.bmBits は 32bpp DIB のビット。アルファを保ち色を載せる。
        unsafe {
            let mut p = bm.bmBits as *mut u8;
            for _ in 0..(bm.bmWidth * bm.bmHeight) {
                let alpha = *p.add(3) as u32;
                *p.add(0) = divide_by_255(blue * alpha);
                *p.add(1) = divide_by_255(green * alpha);
                *p.add(2) = divide_by_255(red * alpha);
                p = p.add(4);
            }
        }
        self.color = color;
    }

    /// 指定矩形へ描画する。`CMonoColorBitmap::Draw`(DrawUtil.cpp:1254)。
    ///
    /// `src_width`/`src_height` が 0 以下ならビットマップ全体、`dst_width`/`dst_height` が
    /// 0 以下ならソースと同じサイズを使う。ソース範囲が画像外なら `false`。
    /// 単色画像のときは描画前に `color` で塗り直す。
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        hdc: HDC,
        dst_x: i32,
        dst_y: i32,
        dst_width: i32,
        dst_height: i32,
        src_x: i32,
        src_y: i32,
        src_width: i32,
        src_height: i32,
        color: u32,
        opacity: u8,
    ) -> bool {
        if self.hbm_premultiplied.0.is_null() {
            return false;
        }
        let mut bm = BITMAP::default();
        // SAFETY: hbm_premultiplied は有効。
        let got = unsafe {
            GetObjectW(
                HGDIOBJ(self.hbm_premultiplied.0),
                size_of::<BITMAP>() as i32,
                Some(&mut bm as *mut BITMAP as *mut c_void),
            )
        };
        if got != size_of::<BITMAP>() as i32 {
            return false;
        }
        let src_width = if src_width <= 0 { bm.bmWidth } else { src_width };
        let dst_width = if dst_width <= 0 { src_width } else { dst_width };
        let src_height = if src_height <= 0 { bm.bmHeight } else { src_height };
        let dst_height = if dst_height <= 0 { src_height } else { dst_height };
        if src_x < 0
            || src_y < 0
            || src_x + src_width > bm.bmWidth
            || src_y + src_height > bm.bmHeight
        {
            return false;
        }
        if !self.color_image {
            self.set_color(color);
        }
        // SAFETY: hdc は呼び出し側保証。作業 DC は自前生成・破棄する。
        let hdc_memory = unsafe { CreateCompatibleDC(Some(hdc)) };
        if hdc_memory.0.is_null() {
            return false;
        }
        // SAFETY: hdc_memory に対象 DIB を選択して AlphaBlend で合成する。
        unsafe {
            let hbm_old = SelectObject(hdc_memory, HGDIOBJ(self.hbm_premultiplied.0));
            let bf = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: opacity,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let _ = AlphaBlend(
                hdc, dst_x, dst_y, dst_width, dst_height, hdc_memory, src_x, src_y, src_width,
                src_height, bf,
            );
            SelectObject(hdc_memory, hbm_old);
            let _ = DeleteDC(hdc_memory);
        }
        true
    }

    /// 左上位置を指定して描画する(`Draw` のサイズ簡易版、DrawUtil.cpp:1295)。
    #[allow(clippy::too_many_arguments)]
    pub fn draw_at(
        &mut self,
        hdc: HDC,
        dst_x: i32,
        dst_y: i32,
        color: u32,
        opacity: u8,
        src_x: i32,
        src_y: i32,
        width: i32,
        height: i32,
    ) -> bool {
        self.draw(
            hdc, dst_x, dst_y, width, height, src_x, src_y, width, height, color, opacity,
        )
    }

    /// 部分領域を 32bpp DIB として取り出す。`CMonoColorBitmap::ExtractBitmap`(DrawUtil.cpp:1329)。
    ///
    /// 単色画像のときは `color` で着色し、カラー画像のときはそのままコピーする。
    /// 範囲外や未生成は `None`。返した `HBITMAP` は呼び出し側が `DeleteObject` で破棄する。
    pub fn extract_bitmap(
        &self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        color: u32,
    ) -> Option<HBITMAP> {
        if self.hbm.0.is_null() || x < 0 || y < 0 {
            return None;
        }
        let mut bm = BITMAP::default();
        // SAFETY: hbm は有効。
        let got = unsafe {
            GetObjectW(
                HGDIOBJ(self.hbm.0),
                size_of::<BITMAP>() as i32,
                Some(&mut bm as *mut BITMAP as *mut c_void),
            )
        };
        if got != size_of::<BITMAP>() as i32 || x + width > bm.bmWidth || y + height > bm.bmHeight {
            return None;
        }
        let (hbm, p_bits) = create_dib_with_bits(width, height, 32)?;
        // SAFETY: 原実装どおりボトムアップ DIB の行・画素を走査する。
        unsafe {
            let mut p = (bm.bmBits as *const u8)
                .add(((bm.bmHeight - (y + height)) * bm.bmWidthBytes + x * 4) as usize);
            let mut q = p_bits as *mut u8;
            if self.color_image {
                for _ in 0..height {
                    copy_nonoverlapping(p, q, (width * 4) as usize);
                    p = p.add(bm.bmWidthBytes as usize);
                    q = q.add((width * 4) as usize);
                }
            } else {
                let (red, green, blue) =
                    (get_r(color) as u8, get_g(color) as u8, get_b(color) as u8);
                for _ in 0..height {
                    for _ in 0..width {
                        *q.add(0) = blue;
                        *q.add(1) = green;
                        *q.add(2) = red;
                        *q.add(3) = *p.add(3);
                        p = p.add(4);
                        q = q.add(4);
                    }
                    p = p.add((bm.bmWidthBytes - width * 4) as usize);
                }
            }
        }
        Some(hbm)
    }

    /// 画像全体に対応するイメージリストを生成する。`CMonoColorBitmap::CreateImageList`(DrawUtil.cpp:1302)。
    ///
    /// 失敗時 `None`。返した `HIMAGELIST` は呼び出し側が `ImageList_Destroy` で破棄する。
    pub fn create_image_list(&self, icon_width: i32, color: u32) -> Option<HIMAGELIST> {
        if self.hbm.0.is_null() || icon_width < 1 {
            return None;
        }
        let mut bm = BITMAP::default();
        // SAFETY: hbm は有効。
        let got = unsafe {
            GetObjectW(
                HGDIOBJ(self.hbm.0),
                size_of::<BITMAP>() as i32,
                Some(&mut bm as *mut BITMAP as *mut c_void),
            )
        };
        if got != size_of::<BITMAP>() as i32 || bm.bmWidth < icon_width {
            return None;
        }
        // SAFETY: ImageList_Create は失敗時 NULL(=0)を返す。
        let himl = unsafe { ImageList_Create(icon_width, bm.bmHeight, ILC_COLOR32, 0, 1) };
        if himl.0 == 0 {
            return None;
        }
        let Some(hbm) = self.extract_bitmap(0, 0, bm.bmWidth, bm.bmHeight, color) else {
            // SAFETY: 直前に生成したイメージリストを破棄する。
            unsafe {
                let _ = ImageList_Destroy(Some(himl));
            }
            return None;
        };
        // SAFETY: hbm は本関数が生成・破棄する(マスクは無し)。
        unsafe {
            ImageList_Add(himl, hbm, None);
            let _ = DeleteObject(HGDIOBJ(hbm.0));
        }
        Some(himl)
    }

    /// 部分領域をアイコンとして取り出す。`CMonoColorBitmap::ExtractIcon`(DrawUtil.cpp:1372)。
    ///
    /// 生成失敗時 `None`。返した `HICON` は呼び出し側が `DestroyIcon` で破棄する。
    pub fn extract_icon(
        &self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        color: u32,
    ) -> Option<HICON> {
        let hbm_color = self.extract_bitmap(x, y, width, height, color)?;
        // SAFETY: 1bpp のマスクビットマップ。失敗時 NULL。
        let hbm_mask = unsafe { CreateBitmap(width, height, 1, 1, None) };
        if hbm_mask.0.is_null() {
            // SAFETY: 直前に取り出したカラービットマップを破棄する。
            unsafe {
                let _ = DeleteObject(HGDIOBJ(hbm_color.0));
            }
            return None;
        }
        let ii = ICONINFO {
            fIcon: BOOL(1),
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: hbm_mask,
            hbmColor: hbm_color,
        };
        // SAFETY: ii の各ビットマップは有効。CreateIconIndirect 後に元ビットマップを破棄する。
        let hico = unsafe { CreateIconIndirect(&ii) };
        unsafe {
            let _ = DeleteObject(HGDIOBJ(hbm_mask.0));
            let _ = DeleteObject(HGDIOBJ(hbm_color.0));
        }
        match hico {
            Ok(h) if !h.0.is_null() => Some(h),
            _ => None,
        }
    }

    /// 画像全体をアイコンとして取り出す。`CMonoColorBitmap::ExtractIcon(COLORREF)`(DrawUtil.cpp:1399)。
    pub fn extract_icon_full(&self, color: u32) -> Option<HICON> {
        if self.hbm.0.is_null() {
            return None;
        }
        let mut bm = BITMAP::default();
        // SAFETY: hbm は有効。
        let got = unsafe {
            GetObjectW(
                HGDIOBJ(self.hbm.0),
                size_of::<BITMAP>() as i32,
                Some(&mut bm as *mut BITMAP as *mut c_void),
            )
        };
        if got != size_of::<BITMAP>() as i32 {
            return None;
        }
        self.extract_icon(0, 0, bm.bmWidth, bm.bmHeight, color)
    }
}

impl Default for MonoColorBitmap {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for MonoColorBitmap {
    fn clone(&self) -> Self {
        // CMonoColorBitmap::operator=(DrawUtil.cpp:1126): 各 DIB を複製する。
        // 単色画像はコピー元では本体とプリマルチプライが同一ハンドルだが、
        // 原実装どおり両方を複製するため複製先では別個のハンドルになる。
        let mut dst = Self::new();
        if !self.hbm.0.is_null() {
            if let Some(h) = duplicate_dib(self.hbm) {
                dst.hbm = h;
            }
        }
        if !self.hbm_premultiplied.0.is_null() {
            if let Some(h) = duplicate_dib(self.hbm_premultiplied) {
                dst.hbm_premultiplied = h;
            }
        }
        dst.color = self.color;
        dst.color_image = self.color_image;
        dst
    }
}

impl Drop for MonoColorBitmap {
    fn drop(&mut self) {
        self.destroy();
    }
}

// ---------------------------------------------------------------------------
// 単色化アイコンリスト(DrawUtil::CMonoColorIconList、DrawUtil.h:236 / DrawUtil.cpp:1435)
// ---------------------------------------------------------------------------

/// 横に並んだ複数アイコンを1枚の [`MonoColorBitmap`] として保持するアイコンリスト。
/// 原実装 `DrawUtil::CMonoColorIconList`。
///
/// リソース読み込み(`Load`)は [`MonoColorBitmap`] と同じ理由で未移植。
pub struct MonoColorIconList {
    bitmap: MonoColorBitmap,
    icon_width: i32,
    icon_height: i32,
}

impl MonoColorIconList {
    /// 空(未生成)を作る。
    pub fn new() -> Self {
        Self {
            bitmap: MonoColorBitmap::new(),
            icon_width: 0,
            icon_height: 0,
        }
    }

    /// ビットマップからそのままのサイズで生成する。`CMonoColorIconList::Create`(DrawUtil.cpp:1456)。
    pub fn create(&mut self, hbm: HBITMAP, width: i32, height: i32) -> bool {
        if !self.bitmap.create(hbm) {
            return false;
        }
        self.icon_width = width;
        self.icon_height = height;
        true
    }

    /// ビットマップを拡縮して生成する。`CMonoColorIconList::Create`(5引数版、DrawUtil.cpp:1465)。
    ///
    /// `width`/`height` が `orig_width`/`orig_height` と異なる場合、各アイコンを `STRETCH_HALFTONE`
    /// で拡縮した一時 DIB を作ってから取り込む。
    pub fn create_resized(
        &mut self,
        hbm: HBITMAP,
        orig_width: i32,
        orig_height: i32,
        width: i32,
        height: i32,
    ) -> bool {
        let mut bm = BITMAP::default();
        // SAFETY: hbm は有効。
        let got = unsafe {
            GetObjectW(
                HGDIOBJ(hbm.0),
                size_of::<BITMAP>() as i32,
                Some(&mut bm as *mut BITMAP as *mut c_void),
            )
        };
        if got != size_of::<BITMAP>() as i32 || bm.bmWidth < orig_width || bm.bmHeight < orig_height
        {
            return false;
        }

        if width == orig_width && height == orig_height {
            if !self.bitmap.create(hbm) {
                return false;
            }
        } else {
            let icon_count = bm.bmWidth / orig_width;
            let Some(hbm_stretched) = create_dib(width * icon_count, height, 24) else {
                return false;
            };
            // SAFETY: 一時 DC を2つ作り、各アイコンを StretchBlt で拡縮する。
            unsafe {
                let hdc_src = CreateCompatibleDC(None);
                let hdc_dst = CreateCompatibleDC(None);
                let hbm_src_old = SelectObject(hdc_src, HGDIOBJ(hbm.0));
                let hbm_dst_old = SelectObject(hdc_dst, HGDIOBJ(hbm_stretched.0));
                let old_mode = SetStretchBltMode(hdc_dst, STRETCH_HALFTONE);
                for i in 0..icon_count {
                    let _ = StretchBlt(
                        hdc_dst,
                        width * i,
                        0,
                        width,
                        height,
                        Some(hdc_src),
                        orig_width * i,
                        0,
                        orig_width,
                        orig_height,
                        SRCCOPY,
                    );
                }
                let _ = SetStretchBltMode(hdc_dst, STRETCH_BLT_MODE(old_mode));
                SelectObject(hdc_src, hbm_src_old);
                SelectObject(hdc_dst, hbm_dst_old);
                let _ = DeleteDC(hdc_src);
                let _ = DeleteDC(hdc_dst);
            }
            let result = self.bitmap.create(hbm_stretched);
            // SAFETY: 一時ストレッチ DIB を破棄する。
            unsafe {
                let _ = DeleteObject(HGDIOBJ(hbm_stretched.0));
            }
            if !result {
                return false;
            }
        }

        self.icon_width = width;
        self.icon_height = height;
        true
    }

    /// 破棄する。`CMonoColorIconList::Destroy`(DrawUtil.cpp:1508)。
    pub fn destroy(&mut self) {
        self.bitmap.destroy();
        self.icon_width = 0;
        self.icon_height = 0;
    }

    /// 生成済みかどうか(`IsCreated`、DrawUtil.cpp:1515)。
    pub fn is_created(&self) -> bool {
        self.bitmap.is_created()
    }

    /// アイコン幅(`GetIconWidth`、DrawUtil.h:260)。
    pub fn icon_width(&self) -> i32 {
        self.icon_width
    }

    /// アイコン高さ(`GetIconHeight`、DrawUtil.h:261)。
    pub fn icon_height(&self) -> i32 {
        self.icon_height
    }

    /// 指定インデックスのアイコンを描画する。`CMonoColorIconList::Draw`(DrawUtil.cpp:1520)。
    ///
    /// 原実装はリサイズ時に GDI+(`Graphics::CCanvas`)で高品質拡縮するが、GDI+ ラッパーは
    /// 未移植のため常に `AlphaBlend`([`MonoColorBitmap::draw`])で描画する(拡縮品質のみ差異)。
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        hdc: HDC,
        dst_x: i32,
        dst_y: i32,
        dst_width: i32,
        dst_height: i32,
        icon_index: i32,
        color: u32,
        opacity: u8,
    ) -> bool {
        if hdc.0.is_null() || dst_width <= 0 || dst_height <= 0 {
            return false;
        }
        let icon_width = self.icon_width;
        let icon_height = self.icon_height;
        self.bitmap.draw(
            hdc,
            dst_x,
            dst_y,
            dst_width,
            dst_height,
            icon_index * icon_width,
            0,
            icon_width,
            icon_height,
            color,
            opacity,
        )
    }

    /// イメージリストを生成する。`CMonoColorIconList::CreateImageList`(DrawUtil.cpp:1552)。
    pub fn create_image_list(&self, color: u32) -> Option<HIMAGELIST> {
        self.bitmap.create_image_list(self.icon_width, color)
    }

    /// 指定インデックスのアイコンを 32bpp DIB として取り出す。`CMonoColorIconList::ExtractBitmap`(DrawUtil.cpp:1557)。
    pub fn extract_bitmap(&self, index: i32, color: u32) -> Option<HBITMAP> {
        self.bitmap.extract_bitmap(
            index * self.icon_width,
            0,
            self.icon_width,
            self.icon_height,
            color,
        )
    }

    /// 指定インデックスのアイコンを取り出す。`CMonoColorIconList::ExtractIcon`(DrawUtil.cpp:1562)。
    pub fn extract_icon(&self, index: i32, color: u32) -> Option<HICON> {
        self.bitmap.extract_icon(
            index * self.icon_width,
            0,
            self.icon_width,
            self.icon_height,
            color,
        )
    }
}

impl Default for MonoColorIconList {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::Gdi::STRETCH_HALFTONE;

    // ----- RGBA -----

    #[test]
    fn test_rgba_default_is_zero() {
        let c = Rgba::default();
        assert_eq!(c, Rgba::new(0, 0, 0, 0));
    }

    #[test]
    fn test_rgba_from_rgb_sets_opaque() {
        assert_eq!(Rgba::from_rgb(1, 2, 3), Rgba::new(1, 2, 3, 255));
    }

    #[test]
    fn test_rgba_colorref_roundtrip() {
        // COLORREF は 0x00BBGGRR。
        let color = 0x0034_5678; // R=0x78, G=0x56, B=0x34
        let c = Rgba::from_colorref(color);
        assert_eq!(c, Rgba::new(0x78, 0x56, 0x34, 255));
        assert_eq!(c.to_colorref(), color);
    }

    #[test]
    fn test_rgba_set() {
        let mut c = Rgba::default();
        c.set(10, 20, 30, 40);
        assert_eq!(c, Rgba::new(10, 20, 30, 40));
    }

    // ----- 色・アルファ計算 -----

    #[test]
    fn test_divide_by_255() {
        assert_eq!(divide_by_255(0), 0);
        assert_eq!(divide_by_255(255), 1);
        assert_eq!(divide_by_255(255 * 255), 255);
        // 128*255 = 32640 → 128
        assert_eq!(divide_by_255(128 * 255), 128);
        // この式は積(0..=255*255)を 255 で割る用途。境界は v=255 で 1、v=254 で 0。
        assert_eq!(divide_by_255(254), 0);
        assert_eq!(divide_by_255(200), 0);
        // 中間の積: 100*255 → 100
        assert_eq!(divide_by_255(100 * 255), 100);
    }

    #[test]
    fn test_blend_alpha() {
        // 端点
        assert_eq!(blend_alpha(0, 200, 0, 10), 0);
        assert_eq!(blend_alpha(0, 200, 10, 10), 200);
        // 中間
        assert_eq!(blend_alpha(0, 200, 5, 10), 100);
        // max <= 0 は平均
        assert_eq!(blend_alpha(40, 80, 3, 0), 60);
    }

    #[test]
    fn test_channel_to_trivertex() {
        assert_eq!(channel_to_trivertex(0), 0);
        assert_eq!(channel_to_trivertex(0xFF), 0xFF00);
        assert_eq!(channel_to_trivertex(1), 0x0100);
    }

    // ----- グラデーション比率 -----

    #[test]
    fn test_linear_gradient_ratio() {
        // span=5: local=0→255(Color1)、local=4→0(Color2)
        assert_eq!(linear_gradient_ratio(0, 5), 255);
        assert_eq!(linear_gradient_ratio(4, 5), 0);
        // 中央付近
        assert_eq!(linear_gradient_ratio(2, 5), ((2 * 255) / 4) as u8);
    }

    #[test]
    fn test_mirror_gradient_ratio() {
        // span=5: 両端=255、中央(local=2)=|4-4|*255/4=0
        assert_eq!(mirror_gradient_ratio(0, 5), 255);
        assert_eq!(mirror_gradient_ratio(4, 5), 255);
        assert_eq!(mirror_gradient_ratio(2, 5), 0);
    }

    // ----- 光沢グラデーション -----

    #[test]
    fn test_glossy_gradient_colors_non_mirror() {
        let color1 = 0x0000_0000; // 黒
        let color2 = 0x0000_00FF; // 赤(R=0xFF)
        let g = glossy_gradient_colors(color1, color2, false, 96, 48);
        // 非対称: center=mix(c1,c2,128), end=c2
        assert_eq!(g.second_start, mix_color(color1, color2, 128));
        assert_eq!(g.second_end, color2);
        assert_eq!(g.first_start, mix_color(WHITE, color1, 96));
        assert_eq!(g.first_end, mix_color(WHITE, g.second_start, 48));
    }

    #[test]
    fn test_glossy_gradient_colors_mirror() {
        let color1 = 0x0000_0000;
        let color2 = 0x0000_00FF;
        let g = glossy_gradient_colors(color1, color2, true, 96, 48);
        // 対称: center=c2, end=c1
        assert_eq!(g.second_start, color2);
        assert_eq!(g.second_end, color1);
        assert_eq!(g.first_start, mix_color(WHITE, color1, 96));
        assert_eq!(g.first_end, mix_color(WHITE, color2, 48));
    }

    // ----- オーバーレイ -----

    #[test]
    fn test_color_overlay_pixel() {
        // COLORREF 0x00BBGGRR=0x00342211 → R=0x11,G=0x22,B=0x34
        // → 0xFF112234
        assert_eq!(color_overlay_pixel(0x0034_2211), 0xFF11_2234);
        // 黒は 0xFF000000、白は 0xFFFFFFFF
        assert_eq!(color_overlay_pixel(0x0000_0000), 0xFF00_0000);
        assert_eq!(color_overlay_pixel(0x00FF_FFFF), 0xFFFF_FFFF);
    }

    #[test]
    fn test_mono_color_transparent() {
        assert_eq!(mono_color_transparent(0x0000_0000), 0x00FF_FFFF);
        assert_eq!(mono_color_transparent(0x00FF_FFFF), 0x0000_0000);
        assert_eq!(mono_color_transparent(0x0012_3456), 0x00ED_CBA9);
    }

    // ----- 枠塗り矩形 -----

    #[test]
    fn test_fill_border_rects_full() {
        // 外枠 0..100、内側空き 10..90、描画=外枠全体。
        let border = RECT {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        };
        let empty = RECT {
            left: 10,
            top: 10,
            right: 90,
            bottom: 90,
        };
        let rects = fill_border_rects(border, empty, border);
        // 上帯・下帯・左帯・右帯の 4 つ。
        assert_eq!(rects.len(), 4);
        // 上帯: left..right=0..100, top..bottom=0..10
        assert_eq!(
            rects[0],
            RECT {
                left: 0,
                top: 0,
                right: 100,
                bottom: 10
            }
        );
        // 下帯: top..bottom=90..100
        assert_eq!(
            rects[1],
            RECT {
                left: 0,
                top: 90,
                right: 100,
                bottom: 100
            }
        );
        // 左帯: left..right=0..10, top..bottom=10..90
        assert_eq!(
            rects[2],
            RECT {
                left: 0,
                top: 10,
                right: 10,
                bottom: 90
            }
        );
        // 右帯: left..right=90..100
        assert_eq!(
            rects[3],
            RECT {
                left: 90,
                top: 10,
                right: 100,
                bottom: 90
            }
        );
    }

    #[test]
    fn test_fill_border_rects_paint_clips_to_top_only() {
        let border = RECT {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        };
        let empty = RECT {
            left: 10,
            top: 10,
            right: 90,
            bottom: 90,
        };
        // 描画矩形が上端の枠のみと重なる。
        let paint = RECT {
            left: 0,
            top: 0,
            right: 100,
            bottom: 5,
        };
        let rects = fill_border_rects(border, empty, paint);
        assert_eq!(rects.len(), 1);
        assert_eq!(
            rects[0],
            RECT {
                left: 0,
                top: 0,
                right: 100,
                bottom: 5
            }
        );
    }

    #[test]
    fn test_fill_border_rects_empty_when_no_overlap() {
        let border = RECT {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        };
        let empty = RECT {
            left: 10,
            top: 10,
            right: 90,
            bottom: 90,
        };
        // 枠の外(右方)にある描画矩形。
        let paint = RECT {
            left: 200,
            top: 200,
            right: 300,
            bottom: 300,
        };
        let rects = fill_border_rects(border, empty, paint);
        assert!(rects.is_empty());
    }

    // ----- マージンスケーリング -----

    #[test]
    fn test_scale_margins() {
        let m = MARGINS {
            cxLeftWidth: 10,
            cxRightWidth: 20,
            cyTopHeight: 30,
            cyBottomHeight: 40,
        };
        // 2 倍(num=192, denom=96)
        let s = scale_margins(m, 192, 96);
        assert_eq!(s.cxLeftWidth, 20);
        assert_eq!(s.cxRightWidth, 40);
        assert_eq!(s.cyTopHeight, 60);
        assert_eq!(s.cyBottomHeight, 80);
    }

    // ----- GDI RAII ラッパー(実環境スモーク) -----

    #[test]
    fn test_brush_default_not_created() {
        let b = Brush::new();
        assert!(!b.is_created());
    }

    #[test]
    fn test_brush_create_and_clone() {
        let b = Brush::with_color(0x00_00_00_FF);
        assert!(b.is_created());
        // コピーコンストラクタ相当(LOGBRUSH 複製)。
        let c = b.clone();
        assert!(c.is_created());
    }

    #[test]
    fn test_brush_destroy_is_idempotent() {
        let mut b = Brush::with_color(0x0012_3456);
        b.destroy();
        assert!(!b.is_created());
        b.destroy();
        assert!(!b.is_created());
    }

    #[test]
    fn test_memory_dc_create_and_delete() {
        let mut m = MemoryDc::new();
        assert!(!m.is_created());
        assert!(m.create(None));
        assert!(m.is_created());
        m.delete();
        assert!(!m.is_created());
    }

    #[test]
    fn test_offscreen_create_and_copy() {
        let mut off = Offscreen::new();
        assert!(off.create(16, 16, None));
        assert!(off.is_created());
        assert_eq!(off.width(), 16);
        assert_eq!(off.height(), 16);

        // 別のオフスクリーンへ転送(クラッシュしないこと)。
        let mut dst = Offscreen::new();
        assert!(dst.create(16, 16, None));
        assert!(off.copy_to(dst.dc(), None));
    }

    #[test]
    fn test_offscreen_invalid_size() {
        let mut off = Offscreen::new();
        assert!(!off.create(0, 10, None));
        assert!(!off.create(10, -1, None));
        assert!(!off.is_created());
    }

    // ----- 描画関数(オフスクリーンへの実描画スモーク) -----

    fn make_offscreen(width: i32, height: i32) -> Offscreen {
        let mut off = Offscreen::new();
        assert!(off.create(width, height, None));
        off
    }

    #[test]
    fn test_fill_null_hdc() {
        let rc = RECT {
            left: 0,
            top: 0,
            right: 10,
            bottom: 10,
        };
        assert!(!fill(HDC::default(), &rc, 0));
    }

    #[test]
    fn test_fill_on_offscreen() {
        let off = make_offscreen(20, 20);
        let rc = RECT {
            left: 0,
            top: 0,
            right: 20,
            bottom: 20,
        };
        assert!(fill(off.dc(), &rc, 0x0000_00FF));
    }

    #[test]
    fn test_fill_gradient_variants() {
        let off = make_offscreen(20, 20);
        let rc = RECT {
            left: 0,
            top: 0,
            right: 20,
            bottom: 20,
        };
        // 非対称はクラッシュしないこと、対称は再帰後に true。
        let _ = fill_gradient(off.dc(), &rc, 0x0000_00FF, 0x00FF_0000, FillDirection::Horz);
        let _ = fill_gradient(off.dc(), &rc, 0x0000_00FF, 0x00FF_0000, FillDirection::Vert);
        assert!(fill_gradient(
            off.dc(),
            &rc,
            0x0000_00FF,
            0x00FF_0000,
            FillDirection::HorzMirror
        ));
        assert!(fill_gradient(
            off.dc(),
            &rc,
            0x0000_00FF,
            0x00FF_0000,
            FillDirection::VertMirror
        ));
    }

    #[test]
    fn test_fill_gradient_invalid_rect() {
        let off = make_offscreen(10, 10);
        // left == right の空矩形。
        let bad = RECT {
            left: 5,
            top: 0,
            right: 5,
            bottom: 10,
        };
        assert!(!fill_gradient(off.dc(), &bad, 0, 0x00FF_FFFF, FillDirection::Horz));
    }

    #[test]
    fn test_fill_glossy_and_interlaced() {
        let off = make_offscreen(20, 20);
        let rc = RECT {
            left: 0,
            top: 0,
            right: 20,
            bottom: 20,
        };
        assert!(fill_glossy_gradient(
            off.dc(),
            &rc,
            0x0020_4060,
            0x0008_0808,
            FillDirection::Vert,
            96,
            48
        ));
        assert!(fill_interlaced_gradient(
            off.dc(),
            &rc,
            0x0020_4060,
            0x0008_0808,
            FillDirection::Horz,
            0,
            48
        ));
    }

    #[test]
    fn test_fill_border_smoke() {
        let off = make_offscreen(30, 30);
        let border = RECT {
            left: 0,
            top: 0,
            right: 30,
            bottom: 30,
        };
        assert!(fill_border_width(off.dc(), &border, 3, None, 0x00FF_FFFF));
    }

    // ----- アルファ合成描画(オフスクリーンへの実描画スモーク) -----

    #[test]
    fn test_fill_gradient_rgba_opaque_delegates() {
        let off = make_offscreen(20, 20);
        let rc = RECT {
            left: 0,
            top: 0,
            right: 20,
            bottom: 20,
        };
        // 両端不透明 → COLORREF 版へ委譲。Mirror は true。
        assert!(fill_gradient_rgba(
            off.dc(),
            &rc,
            Rgba::from_rgb(0, 0, 255),
            Rgba::from_rgb(255, 0, 0),
            FillDirection::HorzMirror
        ));
    }

    #[test]
    fn test_fill_gradient_rgba_translucent() {
        let off = make_offscreen(20, 20);
        let rc = RECT {
            left: 0,
            top: 0,
            right: 20,
            bottom: 20,
        };
        // 半透明を含む → 一時ビットマップ経由で列/行合成。
        assert!(fill_gradient_rgba(
            off.dc(),
            &rc,
            Rgba::new(0, 0, 255, 0),
            Rgba::new(255, 0, 0, 255),
            FillDirection::Horz
        ));
        assert!(fill_gradient_rgba(
            off.dc(),
            &rc,
            Rgba::new(0, 0, 255, 128),
            Rgba::new(255, 0, 0, 0),
            FillDirection::Vert
        ));
    }

    #[test]
    fn test_fill_gradient_rgba_invalid() {
        let off = make_offscreen(10, 10);
        let bad = RECT {
            left: 5,
            top: 0,
            right: 5,
            bottom: 10,
        };
        assert!(!fill_gradient_rgba(
            off.dc(),
            &bad,
            Rgba::new(0, 0, 0, 0),
            Rgba::new(255, 255, 255, 128),
            FillDirection::Horz
        ));
    }

    #[test]
    fn test_gloss_overlay_smoke() {
        let off = make_offscreen(20, 20);
        let rc = RECT {
            left: 0,
            top: 0,
            right: 20,
            bottom: 20,
        };
        assert!(gloss_overlay(off.dc(), &rc, 192, 32, 32, 0));
        // 空矩形は false。
        let empty = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 10,
        };
        assert!(!gloss_overlay(off.dc(), &empty, 192, 32, 32, 0));
    }

    #[test]
    fn test_color_overlay_smoke() {
        let off = make_offscreen(20, 20);
        let rc = RECT {
            left: 0,
            top: 0,
            right: 20,
            bottom: 20,
        };
        assert!(color_overlay(off.dc(), &rc, 0x0000_00FF, 128));
        // 空矩形は false。
        let empty = RECT {
            left: 0,
            top: 0,
            right: 10,
            bottom: 0,
        };
        assert!(!color_overlay(off.dc(), &empty, 0x0000_00FF, 128));
    }

    // ----- フォント -----

    fn log_font(height: i32, face: &str) -> LOGFONTW {
        let mut lf = LOGFONTW {
            lfHeight: height,
            ..Default::default()
        };
        set_face_name(&mut lf, face);
        lf
    }

    #[test]
    fn test_set_face_name_and_wide_eq() {
        let lf = log_font(-16, "Meiryo UI");
        assert!(wide_eq_str(&lf.lfFaceName, "Meiryo UI"));
        assert!(!wide_eq_str(&lf.lfFaceName, "Meiryo"));
        assert!(wide_eq_str_ci(&lf.lfFaceName, "meiryo ui"));
    }

    #[test]
    fn test_compare_log_font() {
        let a = log_font(-16, "Tahoma");
        let b = log_font(-16, "Tahoma");
        assert!(compare_log_font(&a, &b));
        // 高さ違い
        let c = log_font(-17, "Tahoma");
        assert!(!compare_log_font(&a, &c));
        // face 違い(大小区別あり)
        let d = log_font(-16, "tahoma");
        assert!(!compare_log_font(&a, &d));
    }

    #[test]
    fn test_font_default_not_created() {
        let f = Font::new();
        assert!(!f.is_created());
        // null 同士は等しい。
        assert!(f == Font::new());
    }

    #[test]
    fn test_font_create_clone_eq() {
        let lf = log_font(-16, "Tahoma");
        let f = Font::from_log_font(&lf);
        assert!(f.is_created());
        let g = f.clone();
        assert!(g.is_created());
        // 同一 LOGFONT 由来なので等しい。
        assert!(f == g);
        // null とは等しくない。
        assert!(f != Font::new());
    }

    #[test]
    fn test_font_get_log_font_roundtrip() {
        let lf = log_font(-20, "Tahoma");
        let f = Font::from_log_font(&lf);
        let got = f.get_log_font().unwrap();
        assert_eq!(got.lfHeight, -20);
        assert!(wide_eq_str(&got.lfFaceName, "Tahoma"));
    }

    #[test]
    fn test_font_get_height_positive() {
        let f = Font::from_log_font(&log_font(-16, "Tahoma"));
        assert!(f.get_height(true) > 0);
    }

    #[test]
    fn test_get_system_font() {
        // システムフォントが取得でき、Font も生成できる。
        assert!(get_system_font(FontType::Message).is_some());
        assert!(get_system_font(FontType::Menu).is_some());
        let f = Font::from_font_type(FontType::Message);
        assert!(f.is_created());
    }

    #[test]
    fn test_get_system_font_with_dpi() {
        assert!(get_system_font_with_dpi(FontType::Message, 96).is_some());
    }

    #[test]
    fn test_get_default_ui_font() {
        assert!(get_default_ui_font().is_some());
    }

    #[test]
    fn test_font_smoothing_queries_do_not_panic() {
        let _ = is_font_smoothing_enabled();
        let _ = is_clear_type_enabled();
    }

    #[test]
    fn test_is_font_available() {
        // 存在しないフォント名は GDI が別フォントに置換するため false。
        let bad = log_font(-16, "NoSuchFontXYZ123");
        assert!(!is_font_available(&bad, None));
        // 実在フォント名はパニックしないこと(可否は環境依存)。
        let _ = is_font_available(&log_font(-16, "Tahoma"), None);
    }

    // ----- Bitmap / DIB -----

    #[test]
    fn test_create_dib() {
        let hbm = create_dib(16, 16, 32);
        assert!(hbm.is_some());
        // 後始末: Bitmap に attach して Drop で破棄。
        let mut b = Bitmap::new();
        assert!(b.attach(hbm.unwrap()));
        assert!(b.is_created());
    }

    #[test]
    fn test_bitmap_default_not_created() {
        let b = Bitmap::new();
        assert!(!b.is_created());
    }

    #[test]
    fn test_bitmap_create_props() {
        let mut b = Bitmap::new();
        assert!(!b.is_created());
        assert!(b.create(20, 10, 32));
        assert!(b.is_created());
        assert!(b.is_dib());
        assert_eq!(b.width(), 20);
        assert_eq!(b.height(), 10);
    }

    #[test]
    fn test_bitmap_clone_dib() {
        let mut b = Bitmap::new();
        assert!(b.create(8, 8, 32));
        let c = b.clone();
        assert!(c.is_created());
        assert!(c.is_dib());
        assert_eq!(c.width(), 8);
        assert_eq!(c.height(), 8);
    }

    #[test]
    fn test_duplicate_dib() {
        let mut b = Bitmap::new();
        assert!(b.create(8, 4, 32));
        let dup = duplicate_dib(b.handle());
        assert!(dup.is_some());
        let mut d = Bitmap::new();
        d.attach(dup.unwrap());
        assert_eq!(d.width(), 8);
        assert_eq!(d.height(), 4);
    }

    #[test]
    fn test_resize_bitmap() {
        let mut b = Bitmap::new();
        assert!(b.create(8, 8, 32));
        let resized = resize_bitmap(b.handle(), 16, 16, 24, STRETCH_HALFTONE);
        assert!(resized.is_some());
        let mut r = Bitmap::new();
        r.attach(resized.unwrap());
        assert_eq!(r.width(), 16);
        assert_eq!(r.height(), 16);
    }

    #[test]
    fn test_create_from_dib_data() {
        // 2x2 32bpp DIB: BITMAPINFOHEADER(40) + 2*2*4=16 バイト。
        let mut data = vec![0u8; 40 + 16];
        data[0..4].copy_from_slice(&40u32.to_le_bytes()); // biSize
        data[4..8].copy_from_slice(&2i32.to_le_bytes()); // biWidth
        data[8..12].copy_from_slice(&2i32.to_le_bytes()); // biHeight
        data[12..14].copy_from_slice(&1u16.to_le_bytes()); // biPlanes
        data[14..16].copy_from_slice(&32u16.to_le_bytes()); // biBitCount
                                                            // biCompression = 0 (BI_RGB)
        let mut b = Bitmap::new();
        assert!(b.create_from_dib_data(&data));
        assert!(b.is_created());
        assert_eq!(b.width(), 2);
        assert_eq!(b.height(), 2);
    }

    #[test]
    fn test_dib_free_functions_invalid() {
        assert!(duplicate_dib(HBITMAP::default()).is_none());
        assert!(resize_bitmap(HBITMAP::default(), 10, 10, 24, STRETCH_HALFTONE).is_none());
        // 幅・高さ不正。
        let mut b = Bitmap::new();
        assert!(b.create(8, 8, 32));
        assert!(resize_bitmap(b.handle(), 0, 10, 24, STRETCH_HALFTONE).is_none());
    }

    // ----- UxTheme -----

    #[test]
    fn test_uxtheme_not_open_returns_false_none() {
        let t = UxTheme::new();
        assert!(!t.is_open());
        let rc = RECT {
            left: 0,
            top: 0,
            right: 10,
            bottom: 10,
        };
        // 未オープンでは描画系は false、取得系は None。
        assert!(!t.draw_background(HDC::default(), 1, 1, &rc));
        assert!(!t.draw_background_with_base(HDC::default(), 1, 1, 0, 0, &rc));
        assert!(t.get_margins(1, 1, 3601).is_none());
        assert!(t.get_color(1, 1, 3801).is_none());
        assert!(t.get_int(1, 1, 2403).is_none());
        assert!(t.get_part_size(HDC::default(), 1, 1).is_none());
        assert!(t.get_transition_duration(1, 1, 2).is_none());
    }

    #[test]
    fn test_uxtheme_is_active_does_not_panic() {
        let t = UxTheme::new();
        let _ = t.is_active();
    }

    #[test]
    fn test_uxtheme_open_close() {
        let mut t = UxTheme::new();
        let class: Vec<u16> = "BUTTON\0".encode_utf16().collect();
        // テーマ環境なら open 成功(成否は環境依存だがパニックしないこと)。
        let opened = t.open(HWND::default(), PCWSTR(class.as_ptr()), 0);
        assert_eq!(opened, t.is_open());
        t.close();
        assert!(!t.is_open());
    }

    // ----- draw_text -----

    #[test]
    fn test_draw_text_null_hdc_is_false() {
        let rc = RECT {
            left: 0,
            top: 0,
            right: 10,
            bottom: 10,
        };
        let text: Vec<u16> = "x".encode_utf16().collect();
        assert!(!draw_text(
            HDC::default(),
            &text,
            &rc,
            DRAW_TEXT_FORMAT(0),
            None,
            None
        ));
    }

    #[test]
    fn test_draw_text_on_offscreen() {
        use windows::Win32::Graphics::Gdi::{DT_CENTER, DT_LEFT, DT_SINGLELINE};
        let off = make_offscreen(64, 24);
        let rc = RECT {
            left: 0,
            top: 0,
            right: 64,
            bottom: 24,
        };
        let text: Vec<u16> = "Hello".encode_utf16().collect();
        // フォント・色なし。
        assert!(draw_text(
            off.dc(),
            &text,
            &rc,
            DT_LEFT | DT_SINGLELINE,
            None,
            None
        ));
        // 色指定。
        assert!(draw_text(off.dc(), &text, &rc, DT_LEFT, None, Some(0x0000_00FF)));
        // フォント指定。
        let font = Font::from_font_type(FontType::Message);
        assert!(draw_text(
            off.dc(),
            &text,
            &rc,
            DT_CENTER,
            Some(&font),
            Some(0x0000_FF00)
        ));
        // 空文字でも true。
        assert!(draw_text(off.dc(), &[], &rc, DT_LEFT, None, None));
    }

    // ----- MonoColorBitmap -----

    /// DIB の生バイトを読み取る(テスト検証用)。
    unsafe fn dib_bits(hbm: HBITMAP, len: usize) -> Vec<u8> {
        let mut ds = DIBSECTION::default();
        let got = GetObjectW(
            HGDIOBJ(hbm.0),
            size_of::<DIBSECTION>() as i32,
            Some(&mut ds as *mut DIBSECTION as *mut c_void),
        );
        assert_eq!(got, size_of::<DIBSECTION>() as i32);
        let mut v = vec![0u8; len];
        copy_nonoverlapping(ds.dsBm.bmBits as *const u8, v.as_mut_ptr(), len);
        v
    }

    /// 32bpp の DIB ソースを作り、先頭から `bytes` を書き込む。
    unsafe fn make_src_32(w: i32, h: i32, bytes: &[u8]) -> HBITMAP {
        let (hbm, bits) = create_dib_with_bits(w, h, 32).unwrap();
        copy_nonoverlapping(bytes.as_ptr(), bits as *mut u8, bytes.len());
        hbm
    }

    /// 24bpp の DIB ソースを作り、先頭から `bytes` を書き込む。
    unsafe fn make_src_24(w: i32, h: i32, bytes: &[u8]) -> HBITMAP {
        let (hbm, bits) = create_dib_with_bits(w, h, 24).unwrap();
        copy_nonoverlapping(bytes.as_ptr(), bits as *mut u8, bytes.len());
        hbm
    }

    #[test]
    fn test_monocolor_create_null_false() {
        let mut m = MonoColorBitmap::new();
        assert!(!m.create(HBITMAP::default()));
        assert!(!m.is_created());
    }

    #[test]
    fn test_monocolor_color_image_extract_copies() {
        unsafe {
            // 2x1 の 32bpp。BGRA を各画素に格納。
            let src_bytes = [10u8, 20, 30, 40, 50, 60, 70, 80];
            let src = make_src_32(2, 1, &src_bytes);
            let mut m = MonoColorBitmap::new();
            assert!(m.create(src));
            let _ = DeleteObject(HGDIOBJ(src.0));
            assert!(m.is_created());
            // color_image なので色は無視され、そのままコピーされる。
            let ex = m.extract_bitmap(0, 0, 2, 1, 0x0000_00FF).unwrap();
            assert_eq!(dib_bits(ex, 8), src_bytes);
            let _ = DeleteObject(HGDIOBJ(ex.0));
        }
    }

    #[test]
    fn test_monocolor_mono_image_extract_colorizes() {
        unsafe {
            // 24bpp, 2x1。各画素の先頭バイト(=アルファとして扱われる)。
            let mut src = [0u8; 8];
            src[0] = 100; // 画素0 のアルファ
            src[3] = 200; // 画素1 のアルファ
            let hbm_src = make_src_24(2, 1, &src);
            let mut m = MonoColorBitmap::new();
            assert!(m.create(hbm_src));
            let _ = DeleteObject(HGDIOBJ(hbm_src.0));
            // R=0x11, G=0x22, B=0x33。
            let color = 0x0033_2211u32;
            let ex = m.extract_bitmap(0, 0, 2, 1, color).unwrap();
            assert_eq!(
                dib_bits(ex, 8),
                [0x33, 0x22, 0x11, 100, 0x33, 0x22, 0x11, 200]
            );
            let _ = DeleteObject(HGDIOBJ(ex.0));
        }
    }

    #[test]
    fn test_monocolor_extract_out_of_range() {
        unsafe {
            let src = [0u8; 8];
            let hbm = make_src_32(2, 1, &src);
            let mut m = MonoColorBitmap::new();
            assert!(m.create(hbm));
            let _ = DeleteObject(HGDIOBJ(hbm.0));
            assert!(m.extract_bitmap(-1, 0, 1, 1, 0).is_none());
            assert!(m.extract_bitmap(0, 0, 3, 1, 0).is_none());
            assert!(m.extract_bitmap(0, 0, 2, 2, 0).is_none());
        }
    }

    #[test]
    fn test_monocolor_draw_smoke() {
        unsafe {
            let src = [0u8; 8];
            let hbm = make_src_32(2, 1, &src);
            let mut m = MonoColorBitmap::new();
            assert!(m.create(hbm));
            let _ = DeleteObject(HGDIOBJ(hbm.0));
            let off = make_offscreen(8, 8);
            // 全体描画(サイズ自動)。
            assert!(m.draw(off.dc(), 0, 0, 0, 0, 0, 0, 0, 0, 0x0000_00FF, 255));
            assert!(m.draw_at(off.dc(), 1, 1, 0x0000_00FF, 128, 0, 0, 2, 1));
            // ソース範囲外は false。
            assert!(!m.draw(off.dc(), 0, 0, 2, 1, 0, 0, 5, 1, 0, 255));
        }
    }

    #[test]
    fn test_monocolor_draw_not_created_false() {
        let mut m = MonoColorBitmap::new();
        let off = make_offscreen(4, 4);
        assert!(!m.draw(off.dc(), 0, 0, 4, 4, 0, 0, 4, 4, 0, 255));
    }

    #[test]
    fn test_monocolor_create_image_list() {
        unsafe {
            let src = [0u8; 8];
            let hbm = make_src_32(2, 1, &src);
            let mut m = MonoColorBitmap::new();
            assert!(m.create(hbm));
            let _ = DeleteObject(HGDIOBJ(hbm.0));
            let himl = m.create_image_list(2, 0x0000_00FF);
            assert!(himl.is_some());
            let _ = ImageList_Destroy(Some(himl.unwrap()));
            // icon_width < 1 は None。
            assert!(m.create_image_list(0, 0).is_none());
        }
    }

    #[test]
    fn test_monocolor_extract_icon() {
        use windows::Win32::UI::WindowsAndMessaging::DestroyIcon;
        unsafe {
            let src = [0u8; 8];
            let hbm = make_src_32(2, 1, &src);
            let mut m = MonoColorBitmap::new();
            assert!(m.create(hbm));
            let _ = DeleteObject(HGDIOBJ(hbm.0));
            let icon = m.extract_icon_full(0x0000_00FF);
            assert!(icon.is_some());
            let _ = DestroyIcon(icon.unwrap());
        }
    }

    #[test]
    fn test_monocolor_clone() {
        unsafe {
            let mut src = [0u8; 8];
            src[0] = 128;
            src[3] = 64;
            let hbm = make_src_24(2, 1, &src);
            let mut m = MonoColorBitmap::new();
            assert!(m.create(hbm));
            let _ = DeleteObject(HGDIOBJ(hbm.0));
            let c = m.clone();
            assert!(c.is_created());
            // クローンから抽出しても元と同じ結果になる。
            let color = 0x0033_2211u32;
            let a = m.extract_bitmap(0, 0, 2, 1, color).unwrap();
            let b = c.extract_bitmap(0, 0, 2, 1, color).unwrap();
            assert_eq!(dib_bits(a, 8), dib_bits(b, 8));
            let _ = DeleteObject(HGDIOBJ(a.0));
            let _ = DeleteObject(HGDIOBJ(b.0));
        }
    }

    // ----- MonoColorIconList -----

    #[test]
    fn test_iconlist_create_and_extract() {
        unsafe {
            // 4x1 の 32bpp。幅2のアイコン2個。
            let src = [
                1u8, 2, 3, 4, 5, 6, 7, 8, 11, 12, 13, 14, 21, 22, 23, 24,
            ];
            let hbm = make_src_32(4, 1, &src);
            let mut list = MonoColorIconList::new();
            assert!(list.create(hbm, 2, 1));
            let _ = DeleteObject(HGDIOBJ(hbm.0));
            assert!(list.is_created());
            assert_eq!(list.icon_width(), 2);
            assert_eq!(list.icon_height(), 1);
            // index1(列2..4)を取り出す(color_image なのでそのままコピー)。
            let ex = list.extract_bitmap(1, 0x0000_00FF).unwrap();
            assert_eq!(dib_bits(ex, 8), [11, 12, 13, 14, 21, 22, 23, 24]);
            let _ = DeleteObject(HGDIOBJ(ex.0));
        }
    }

    #[test]
    fn test_iconlist_draw_smoke() {
        unsafe {
            let src = [0u8; 16];
            let hbm = make_src_32(4, 1, &src);
            let mut list = MonoColorIconList::new();
            assert!(list.create(hbm, 2, 1));
            let _ = DeleteObject(HGDIOBJ(hbm.0));
            let off = make_offscreen(8, 8);
            assert!(list.draw(off.dc(), 0, 0, 2, 1, 0, 0x0000_00FF, 255));
            // dst サイズ不正は false。
            assert!(!list.draw(off.dc(), 0, 0, 0, 1, 0, 0, 255));
            assert!(!list.draw(HDC::default(), 0, 0, 2, 1, 0, 0, 255));
        }
    }

    #[test]
    fn test_iconlist_create_resized_same_size() {
        unsafe {
            let src = [0u8; 16];
            let hbm = make_src_32(4, 1, &src);
            let mut list = MonoColorIconList::new();
            assert!(list.create_resized(hbm, 2, 1, 2, 1));
            let _ = DeleteObject(HGDIOBJ(hbm.0));
            assert!(list.is_created());
            assert_eq!(list.icon_width(), 2);
        }
    }

    #[test]
    fn test_iconlist_create_resized() {
        unsafe {
            // 4x2 24bpp(行ストライド12, 計24バイト)。orig 2x2 → 3x3。
            let bytes = vec![0u8; 24];
            let hbm = make_src_24(4, 2, &bytes);
            let mut list = MonoColorIconList::new();
            assert!(list.create_resized(hbm, 2, 2, 3, 3));
            let _ = DeleteObject(HGDIOBJ(hbm.0));
            assert!(list.is_created());
            assert_eq!(list.icon_width(), 3);
            assert_eq!(list.icon_height(), 3);
        }
    }

    #[test]
    fn test_iconlist_create_resized_invalid() {
        unsafe {
            let src = [0u8; 16];
            let hbm = make_src_32(4, 1, &src);
            let mut list = MonoColorIconList::new();
            // orig 幅8 > 実幅4 は false。
            assert!(!list.create_resized(hbm, 8, 1, 4, 1));
            let _ = DeleteObject(HGDIOBJ(hbm.0));
            assert!(!list.is_created());
        }
    }
}
