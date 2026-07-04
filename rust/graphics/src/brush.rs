//! `CBrush`(`Graphics.cpp:390-445` / `Graphics.h:117-135`)の移植。

use std::ptr::null_mut;

use windows::Win32::Graphics::GdiPlus::{
    GdipCreateSolidFill, GdipDeleteBrush, GdipSetSolidFillColor, GpBrush, GpSolidFill, Ok as GpOk,
};

use crate::types::{make_argb, Color};

/// `CBrush`(`Graphics.h:117-135`)。`Gdiplus::SolidBrush`(`GpSolidFill`)の
/// RAII ラッパー。
#[derive(Debug)]
pub struct Brush {
    brush: *mut GpSolidFill,
}

impl Brush {
    /// `CBrush()`(`Graphics.h:120`)。未生成状態で生成する。
    #[must_use]
    pub const fn new() -> Self {
        Self { brush: null_mut() }
    }

    /// `CBrush(BYTE r, BYTE g, BYTE b, BYTE a)`(`Graphics.cpp:390-393`)。
    /// 生成に失敗した場合は未生成の `Brush` になる。
    #[must_use]
    pub fn from_rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        let mut brush = Self::new();
        brush.create_solid_brush(r, g, b, a);
        brush
    }

    /// `CBrush(const CColor &Color)`(`Graphics.cpp:396-399`)。
    #[must_use]
    pub fn from_color(color: Color) -> Self {
        let mut brush = Self::new();
        brush.create_solid_brush_color(color);
        brush
    }

    /// `CCanvas` が内部の `GpBrush` にアクセスするためのポインタ取得
    /// (`Graphics.h:134` の `friend class CCanvas` 相当)。
    pub(crate) fn as_brush_ptr(&self) -> *mut GpBrush {
        self.brush.cast()
    }

    /// `Free`(`Graphics.cpp:402-405`)。ブラシを解放する。
    pub fn free(&mut self) {
        if !self.brush.is_null() {
            unsafe { GdipDeleteBrush(self.brush.cast()) };
            self.brush = null_mut();
        }
    }

    /// `CreateSolidBrush(BYTE r, BYTE g, BYTE b, BYTE a)`
    /// (`Graphics.cpp:408-421`)。単色ブラシを生成する。
    ///
    /// 既にブラシがある場合は `GdipSetSolidFillColor` で色を変更し
    /// (`Graphics.cpp:412-414`)、無ければ `GdipCreateSolidFill` で新規生成する。
    pub fn create_solid_brush(&mut self, r: u8, g: u8, b: u8, a: u8) -> bool {
        let color = make_argb(a, r, g, b);

        if !self.brush.is_null() {
            unsafe { GdipSetSolidFillColor(self.brush, color) == GpOk }
        } else {
            let mut brush = null_mut();
            let status = unsafe { GdipCreateSolidFill(color, &mut brush) };
            if status == GpOk && !brush.is_null() {
                self.brush = brush;
                true
            } else {
                // VerifyConstruct(Graphics.cpp:436-445)相当
                if !brush.is_null() {
                    unsafe { GdipDeleteBrush(brush.cast()) };
                }
                false
            }
        }
    }

    /// `CreateSolidBrush(const CColor &Color)`(`Graphics.cpp:424-427`)。
    pub fn create_solid_brush_color(&mut self, color: Color) -> bool {
        self.create_solid_brush(color.red, color.green, color.blue, color.alpha)
    }

    /// `IsCreated`(`Graphics.cpp:430-433`)。ブラシが生成されているか取得する。
    #[must_use]
    pub fn is_created(&self) -> bool {
        !self.brush.is_null()
    }
}

impl Default for Brush {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Brush {
    fn drop(&mut self) {
        self.free();
    }
}
