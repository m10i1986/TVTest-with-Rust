//! `CFont`(`Graphics.cpp:450-500` / `Graphics.h:137-153`)の移植。

use std::ptr::null_mut;

use windows::core::PCWSTR;
use windows::Win32::Graphics::Gdi::{LOGFONTW, FW_BOLD};
use windows::Win32::Graphics::GdiPlus::{
    FontStyleBold, FontStyleItalic, FontStyleStrikeout, FontStyleUnderline, GdipCreateFont,
    GdipCreateFontFamilyFromName, GdipDeleteFont, GdipDeleteFontFamily, GdipGetFamily,
    GdipGetFontSize, GdipGetFontStyle, GpFont, GpFontFamily, Ok as GpOk, UnitPixel,
};

/// `GpFontFamily` の RAII ハンドル(`Gdiplus::FontFamily` のデストラクタ相当)。
#[derive(Debug)]
pub(crate) struct FontFamilyHandle(pub(crate) *mut GpFontFamily);

impl Drop for FontFamilyHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { GdipDeleteFontFamily(self.0) };
        }
    }
}

/// `CFont`(`Graphics.h:137-153`)。`Gdiplus::Font`(`GpFont`)の RAII
/// ラッパー。
#[derive(Debug)]
pub struct Font {
    font: *mut GpFont,
}

impl Font {
    /// `CFont()`(`Graphics.h:140`)。未生成状態で生成する。
    #[must_use]
    pub const fn new() -> Self {
        Self { font: null_mut() }
    }

    /// `CFont(const LOGFONT &lf)`(`Graphics.cpp:450-453`)。
    /// 生成に失敗した場合は未生成の `Font` になる。
    #[must_use]
    pub fn from_logfont(lf: &LOGFONTW) -> Self {
        let mut font = Self::new();
        font.create(lf);
        font
    }

    /// `CCanvas` が内部の `GpFont` にアクセスするためのポインタ取得
    /// (`Graphics.h:152` の `friend class CCanvas` 相当)。
    pub(crate) const fn as_font_ptr(&self) -> *mut GpFont {
        self.font
    }

    /// `Free`(`Graphics.cpp:456-459`)。フォントを解放する。
    pub fn free(&mut self) {
        if !self.font.is_null() {
            unsafe { GdipDeleteFont(self.font) };
            self.font = null_mut();
        }
    }

    /// `Create`(`Graphics.cpp:462-482`)。`LOGFONTW` からフォントを生成する。
    ///
    /// - スタイル変換(`Graphics.cpp:464-472`): `lfWeight >= FW_BOLD` で Bold、
    ///   `lfItalic` / `lfUnderline` / `lfStrikeOut` が非 0 でそれぞれ
    ///   Italic / Underline / Strikeout。
    /// - サイズは `abs(lfHeight)` ピクセル(`UnitPixel`、`Graphics.cpp:477-479`)。
    ///
    /// 原実装は `Gdiplus::Font(faceName, size, style, unit)` コンストラクタで、
    /// 内部的に `GdipCreateFontFamilyFromName` → `GdipCreateFont` を行う。
    /// 本移植ではフォントファミリ生成に失敗したら `false` を返す
    /// (SDK によっては C++ 側が GenericSansSerif へフォールバックする実装も
    /// あるが、本移植ではフォールバックしない方針とした)。
    pub fn create(&mut self, lf: &LOGFONTW) -> bool {
        self.free();

        let mut style = 0i32;
        if lf.lfWeight >= FW_BOLD.0 as i32 {
            style |= FontStyleBold.0;
        }
        if lf.lfItalic != 0 {
            style |= FontStyleItalic.0;
        }
        if lf.lfUnderline != 0 {
            style |= FontStyleUnderline.0;
        }
        if lf.lfStrikeOut != 0 {
            style |= FontStyleStrikeout.0;
        }

        // lfFaceName の NUL 終端が保証されないケースに備えて 33 要素へコピー
        let mut face = [0u16; 33];
        face[..32].copy_from_slice(&lf.lfFaceName);

        let mut family = null_mut();
        let status = unsafe {
            GdipCreateFontFamilyFromName(PCWSTR(face.as_ptr()), null_mut(), &mut family)
        };
        if status != GpOk || family.is_null() {
            if !family.is_null() {
                unsafe { GdipDeleteFontFamily(family) };
            }
            return false;
        }
        let family = FontFamilyHandle(family);

        let mut font = null_mut();
        let status = unsafe {
            GdipCreateFont(
                family.0,
                lf.lfHeight.unsigned_abs() as f32,
                style,
                UnitPixel,
                &mut font,
            )
        };
        if status == GpOk && !font.is_null() {
            self.font = font;
            true
        } else {
            // VerifyConstruct(Graphics.cpp:491-500)相当
            if !font.is_null() {
                unsafe { GdipDeleteFont(font) };
            }
            false
        }
    }

    /// `IsCreated`(`Graphics.cpp:485-488`)。フォントが生成されているか
    /// 取得する。
    #[must_use]
    pub fn is_created(&self) -> bool {
        !self.font.is_null()
    }

    /// `Gdiplus::Font::GetFamily`(`GdipGetFamily`)。取得したファミリは
    /// `FontFamilyHandle` の Drop で解放される。
    pub(crate) fn get_family(&self) -> Option<FontFamilyHandle> {
        if self.font.is_null() {
            return None;
        }
        let mut family = null_mut();
        let status = unsafe { GdipGetFamily(self.font, &mut family) };
        if status == GpOk && !family.is_null() {
            Some(FontFamilyHandle(family))
        } else {
            if !family.is_null() {
                unsafe { GdipDeleteFontFamily(family) };
            }
            None
        }
    }

    /// `Gdiplus::Font::GetStyle`(`GdipGetFontStyle`)。
    pub(crate) fn get_style(&self) -> i32 {
        let mut style = 0i32;
        let _ = unsafe { GdipGetFontStyle(self.font, &mut style) };
        style
    }

    /// `Gdiplus::Font::GetSize`(`GdipGetFontSize`)。
    pub(crate) fn get_size(&self) -> f32 {
        let mut size = 0.0f32;
        let _ = unsafe { GdipGetFontSize(self.font, &mut size) };
        size
    }
}

impl Default for Font {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Font {
    fn drop(&mut self) {
        self.free();
    }
}
