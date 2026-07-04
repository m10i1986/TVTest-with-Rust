//! `CCanvas`(`Graphics.cpp:505-929` / `Graphics.h:155-195`)の移植。

use std::marker::PhantomData;
use std::ptr::{null, null_mut};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{RECT, SIZE};
use windows::Win32::Graphics::Gdi::HDC;
use windows::Win32::Graphics::GdiPlus::{
    ColorAdjustTypeDefault, ColorMatrix, ColorMatrixFlagsDefault, CompositingModeSourceCopy,
    CompositingModeSourceOver, FillModeAlternate, GdipAddPathString, GdipCreateFromHDC,
    GdipCreateImageAttributes, GdipCreateLineBrushFromRect, GdipCreatePath, GdipCreatePen1,
    GdipCreateStringFormat, GdipDeleteBrush, GdipDeleteGraphics, GdipDeletePath, GdipDeletePen,
    GdipDeleteStringFormat, GdipDisposeImageAttributes, GdipDrawImageRectI, GdipDrawImageRectRectI,
    GdipDrawPath, GdipDrawString, GdipFillPath, GdipFillRectangle, GdipFillRectangleI,
    GdipGetCellAscent, GdipGetCellDescent, GdipGetEmHeight, GdipGetFontHeight,
    GdipGetImageGraphicsContext, GdipGetPathWorldBounds, GdipGetSmoothingMode,
    GdipGetStringFormatFlags, GdipGraphicsClear, GdipMeasureString, GdipSetCompositingMode,
    GdipSetImageAttributesColorMatrix, GdipSetPenLineJoin, GdipSetSmoothingMode,
    GdipSetStringFormatAlign, GdipSetStringFormatFlags, GdipSetStringFormatLineAlign,
    GdipSetStringFormatTrimming, GdipSetTextRenderingHint, GpGraphics, GpPath, GpPen,
    GpStringFormat, LineJoinRound, LinearGradientModeHorizontal, LinearGradientModeVertical,
    Ok as GpOk, PointF, RectF, SmoothingMode, SmoothingModeAntiAlias, StringAlignmentCenter,
    StringAlignmentFar, StringAlignmentNear, StringFormatFlagsLineLimit, StringFormatFlagsNoClip,
    StringFormatFlagsNoWrap, StringTrimmingCharacter, StringTrimmingEllipsisCharacter,
    StringTrimmingEllipsisWord, StringTrimmingNone, TextRenderingHintAntiAlias,
    TextRenderingHintAntiAliasGridFit, TextRenderingHintClearTypeGridFit,
    TextRenderingHintSingleBitPerPixel, TextRenderingHintSingleBitPerPixelGridFit,
    TextRenderingHintSystemDefault, UnitPixel, UnitWorld, WrapModeTile,
};

use crate::brush::Brush;
use crate::font::Font;
use crate::image::Image;
use crate::types::{Color, GradientDirection, TextFlag};

/// `GdiplusRectF(const RECT&)`(`Graphics.cpp:52-59`)。
fn rectf_from_rect(rect: &RECT) -> RectF {
    RectF {
        X: rect.left as f32,
        Y: rect.top as f32,
        Width: (rect.right - rect.left) as f32,
        Height: (rect.bottom - rect.top) as f32,
    }
}

/// `&[u16]` の文字列を最初の NUL で切り詰める(`LPCTSTR` の意味論)。
fn trim_at_nul(text: &[u16]) -> &[u16] {
    let len = text.iter().position(|&c| c == 0).unwrap_or(text.len());
    &text[..len]
}

/// `Gdiplus::StringFormat` の RAII ラッパー。
///
/// 既定コンストラクタは `GdipCreateStringFormat(0, LANG_NEUTRAL)` 相当。
struct StringFormat(*mut GpStringFormat);

impl StringFormat {
    fn new() -> Option<Self> {
        let mut format = null_mut();
        if unsafe { GdipCreateStringFormat(0, 0, &mut format) } == GpOk && !format.is_null() {
            Some(Self(format))
        } else {
            None
        }
    }
}

impl Drop for StringFormat {
    fn drop(&mut self) {
        unsafe { GdipDeleteStringFormat(self.0) };
    }
}

/// `Gdiplus::GraphicsPath` の RAII ラッパー。
///
/// 既定コンストラクタは `GdipCreatePath(FillModeAlternate)` 相当。
struct GraphicsPath(*mut GpPath);

impl GraphicsPath {
    fn new() -> Option<Self> {
        let mut path = null_mut();
        if unsafe { GdipCreatePath(FillModeAlternate, &mut path) } == GpOk && !path.is_null() {
            Some(Self(path))
        } else {
            None
        }
    }
}

impl Drop for GraphicsPath {
    fn drop(&mut self) {
        unsafe { GdipDeletePath(self.0) };
    }
}

/// `Gdiplus::Pen(color, width)` の RAII ラッパー(単位は `UnitWorld`)。
struct Pen(*mut GpPen);

impl Pen {
    fn new(argb: u32, width: f32) -> Option<Self> {
        let mut pen = null_mut();
        if unsafe { GdipCreatePen1(argb, width, UnitWorld, &mut pen) } == GpOk && !pen.is_null() {
            Some(Self(pen))
        } else {
            None
        }
    }
}

impl Drop for Pen {
    fn drop(&mut self) {
        unsafe { GdipDeletePen(self.0) };
    }
}

/// `CCanvas`(`Graphics.h:155-195`)。`Gdiplus::Graphics`(`GpGraphics`)の
/// RAII ラッパー。
///
/// ライフタイム `'a` は [`Canvas::from_image`] が対象 [`Image`] を可変借用する
/// 期間を表す(原実装では「`CImage` は `CCanvas` より長生きであること」が
/// 暗黙の前提)。
#[derive(Debug)]
pub struct Canvas<'a> {
    graphics: *mut GpGraphics,
    _borrow: PhantomData<&'a mut Image>,
}

impl<'a> Canvas<'a> {
    /// `CCanvas(HDC hdc)`(`Graphics.cpp:505-511`)。デバイスコンテキストに
    /// 描画するキャンバスを生成する。
    ///
    /// `hdc` が NULL または生成失敗時は「未生成のキャンバス」となり、以後の
    /// 全操作が `false` / 0 を返す(原実装と同じ)。
    ///
    /// # Safety
    ///
    /// `hdc` は有効なデバイスコンテキストで、返された `Canvas` を使用する間
    /// 有効であり続けること。
    #[must_use]
    pub unsafe fn from_hdc(hdc: HDC) -> Self {
        let mut graphics = null_mut();
        if !hdc.0.is_null() {
            let status = unsafe { GdipCreateFromHDC(hdc, &mut graphics) };
            // VerifyConstruct(Graphics.cpp:866-875)相当
            if status != GpOk && !graphics.is_null() {
                unsafe { GdipDeleteGraphics(graphics) };
                graphics = null_mut();
            }
        }
        Self {
            graphics,
            _borrow: PhantomData,
        }
    }

    /// `CCanvas(CImage *pImage)`(`Graphics.cpp:514-520`)。画像に描画する
    /// キャンバスを生成する。
    ///
    /// `image` が未生成の場合や生成失敗時は「未生成のキャンバス」となる。
    /// キャンバスの生存中は `image` を可変借用する(GDI+ では `Graphics` が
    /// 結び付いている間ビットマップへの他の操作が失敗するため、この借用は
    /// 原実装の暗黙の制約を型で表現したもの)。
    #[must_use]
    pub fn from_image(image: &'a mut Image) -> Self {
        let mut graphics = null_mut();
        if image.is_created() {
            let status =
                unsafe { GdipGetImageGraphicsContext(image.as_bitmap_ptr().cast(), &mut graphics) };
            // VerifyConstruct(Graphics.cpp:866-875)相当
            if status != GpOk && !graphics.is_null() {
                unsafe { GdipDeleteGraphics(graphics) };
                graphics = null_mut();
            }
        }
        Self {
            graphics,
            _borrow: PhantomData,
        }
    }

    /// キャンバスが生成されているか取得する(Rust 追加の補助関数。原実装では
    /// `m_Graphics` の null チェックが各メソッド冒頭に相当)。
    #[must_use]
    pub fn is_created(&self) -> bool {
        !self.graphics.is_null()
    }

    /// `Clear`(`Graphics.cpp:523-528`)。指定色で全体をクリアする。
    pub fn clear(&mut self, r: u8, g: u8, b: u8, a: u8) -> bool {
        if self.graphics.is_null() {
            return false;
        }
        unsafe { GdipGraphicsClear(self.graphics, crate::types::make_argb(a, r, g, b)) == GpOk }
    }

    /// `SetComposition`(`Graphics.cpp:531-539`)。合成モードを設定する。
    ///
    /// `composite` が `true` なら `CompositingModeSourceOver`(アルファ合成)、
    /// `false` なら `CompositingModeSourceCopy`(上書き)。
    pub fn set_composition(&mut self, composite: bool) -> bool {
        if self.graphics.is_null() {
            return false;
        }
        let mode = if composite {
            CompositingModeSourceOver
        } else {
            CompositingModeSourceCopy
        };
        unsafe { GdipSetCompositingMode(self.graphics, mode) == GpOk }
    }

    /// `DrawImage(const CImage*, int, int)`(`Graphics.cpp:542-551`)。
    /// 画像を等倍で描画する。
    pub fn draw_image(&mut self, image: &Image, x: i32, y: i32) -> bool {
        if self.graphics.is_null() || !image.is_created() {
            return false;
        }
        unsafe {
            GdipDrawImageRectI(
                self.graphics,
                image.as_bitmap_ptr().cast(),
                x,
                y,
                image.get_width(),
                image.get_height(),
            ) == GpOk
        }
    }

    /// `DrawImage(int, int, int, int, const CImage*, int, int, int, int, float)`
    /// (`Graphics.cpp:554-577`)。画像の矩形を拡縮・不透明度付きで描画する。
    ///
    /// `opacity` は 0.0-1.0。単位行列の `m[3][3]`(アルファ係数)のみ
    /// `opacity` にした `ColorMatrix` を `ImageAttributes` に設定して描画する
    /// (`Graphics.cpp:560-574`)。
    // 原実装(Graphics.cpp:554-556)の引数構成を踏襲するため引数が多い。
    #[allow(clippy::too_many_arguments)]
    pub fn draw_image_rect(
        &mut self,
        dst_x: i32,
        dst_y: i32,
        dst_width: i32,
        dst_height: i32,
        image: &Image,
        src_x: i32,
        src_y: i32,
        src_width: i32,
        src_height: i32,
        opacity: f32,
    ) -> bool {
        if self.graphics.is_null() || !image.is_created() {
            return false;
        }

        let mut attributes = null_mut();
        if unsafe { GdipCreateImageAttributes(&mut attributes) } != GpOk || attributes.is_null() {
            return false;
        }

        // 単位行列(m[3][3] = opacity)。flat API の ColorMatrix は行優先の
        // [f32; 25] なので m[i][j] は m[i * 5 + j]。
        let mut matrix = ColorMatrix { m: [0.0; 25] };
        matrix.m[0] = 1.0;
        matrix.m[6] = 1.0;
        matrix.m[12] = 1.0;
        matrix.m[18] = opacity;
        matrix.m[24] = 1.0;
        // 原実装(Graphics.cpp:569)は SetColorMatrix の戻り値を確認しない
        let _ = unsafe {
            GdipSetImageAttributesColorMatrix(
                attributes,
                ColorAdjustTypeDefault,
                true,
                &matrix,
                null(),
                ColorMatrixFlagsDefault,
            )
        };

        let status = unsafe {
            GdipDrawImageRectRectI(
                self.graphics,
                image.as_bitmap_ptr().cast(),
                dst_x,
                dst_y,
                dst_width,
                dst_height,
                src_x,
                src_y,
                src_width,
                src_height,
                UnitPixel,
                attributes,
                0,
                null_mut(),
            )
        };
        let _ = unsafe { GdipDisposeImageAttributes(attributes) };
        status == GpOk
    }

    /// `FillRect`(`Graphics.cpp:580-589`)。矩形をブラシで塗りつぶす。
    pub fn fill_rect(&mut self, brush: &Brush, rect: &RECT) -> bool {
        if self.graphics.is_null() || !brush.is_created() {
            return false;
        }
        unsafe {
            GdipFillRectangleI(
                self.graphics,
                brush.as_brush_ptr(),
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
            ) == GpOk
        }
    }

    /// `FillGradient`(`Graphics.cpp:592-611`)。矩形をグラデーションで
    /// 塗りつぶす。
    ///
    /// 矩形は左上へ 0.1、幅・高さへ 0.2 拡げて補正する(継ぎ目対策、
    /// `Graphics.cpp:597-601`)。
    pub fn fill_gradient(
        &mut self,
        color1: Color,
        color2: Color,
        rect: &RECT,
        direction: GradientDirection,
    ) -> bool {
        if self.graphics.is_null() {
            return false;
        }
        let rect_f = RectF {
            X: rect.left as f32 - 0.1,
            Y: rect.top as f32 - 0.1,
            Width: (rect.right - rect.left) as f32 + 0.2,
            Height: (rect.bottom - rect.top) as f32 + 0.2,
        };
        let mode = match direction {
            GradientDirection::Horz => LinearGradientModeHorizontal,
            GradientDirection::Vert => LinearGradientModeVertical,
        };
        let mut brush = null_mut();
        if unsafe {
            GdipCreateLineBrushFromRect(
                &rect_f,
                color1.to_argb(),
                color2.to_argb(),
                mode,
                WrapModeTile,
                &mut brush,
            )
        } != GpOk
            || brush.is_null()
        {
            return false;
        }
        let status = unsafe {
            GdipFillRectangle(
                self.graphics,
                brush.cast(),
                rect_f.X,
                rect_f.Y,
                rect_f.Width,
                rect_f.Height,
            )
        };
        let _ = unsafe { GdipDeleteBrush(brush.cast()) };
        status == GpOk
    }

    /// `DrawText`(`Graphics.cpp:614-660`)。テキストを描画する。
    ///
    /// `text` は UTF-16(最初の NUL で切り詰め)。空文字列、未生成の
    /// フォント/ブラシ/キャンバスでは `false`。
    /// [`TextFlag::DRAW_PATH`] 指定時は `GraphicsPath` 経由で描画する
    /// (`Graphics.cpp:629-648`)。
    pub fn draw_text(
        &mut self,
        text: &[u16],
        font: &Font,
        rect: &RECT,
        brush: &Brush,
        flags: TextFlag,
    ) -> bool {
        let text = trim_at_nul(text);
        if self.graphics.is_null() || text.is_empty() || !font.is_created() || !brush.is_created()
        {
            return false;
        }

        let Some(format) = StringFormat::new() else {
            return false;
        };
        Self::set_string_format(&format, flags);
        self.set_text_rendering_hint(flags);

        if flags.contains(TextFlag::DRAW_PATH) {
            let Some(path) = GraphicsPath::new() else {
                return false;
            };
            let Some(family) = font.get_family() else {
                return false;
            };
            let layout = rectf_from_rect(rect);
            if unsafe {
                GdipAddPathString(
                    path.0,
                    PCWSTR(text.as_ptr()),
                    text.len() as i32,
                    family.0,
                    font.get_style(),
                    font.get_size(),
                    &layout,
                    format.0,
                )
            } != GpOk
            {
                return false;
            }

            // SmoothingMode を退避して AntiAlias で FillPath(Graphics.cpp:640-645)
            let mut old_mode = SmoothingMode(0);
            let _ = unsafe { GdipGetSmoothingMode(self.graphics, &mut old_mode) };
            let _ = unsafe { GdipSetSmoothingMode(self.graphics, SmoothingModeAntiAlias) };
            let _ = unsafe { GdipFillPath(self.graphics, brush.as_brush_ptr(), path.0) };
            let _ = unsafe { GdipSetSmoothingMode(self.graphics, old_mode) };

            return true;
        }

        unsafe {
            GdipDrawString(
                self.graphics,
                PCWSTR(text.as_ptr()),
                text.len() as i32,
                font.as_font_ptr(),
                &rectf_from_rect(rect),
                format.0,
                brush.as_brush_ptr(),
            ) == GpOk
        }
    }

    /// `GetTextSize`(`Graphics.cpp:663-722`)。テキストの描画サイズを計測する。
    ///
    /// `size` は入出力: [`TextFlag::FORMAT_NO_WRAP`] が無い場合、入力値が
    /// レイアウト矩形のサイズとして使われる(`Graphics.cpp:669` / `710-714`)。
    /// 結果は右端/下端に +1.0 した切り捨て整数(`Graphics.cpp:717-718`)。
    /// 空文字列では `(0, 0)` で `true`。
    pub fn get_text_size(
        &mut self,
        text: &[u16],
        font: &Font,
        flags: TextFlag,
        size: &mut SIZE,
    ) -> bool {
        let layout_size = *size;

        size.cx = 0;
        size.cy = 0;

        if self.graphics.is_null() || !font.is_created() {
            return false;
        }

        let text = trim_at_nul(text);
        if text.is_empty() {
            return true;
        }

        let Some(format) = StringFormat::new() else {
            return false;
        };
        Self::set_string_format(&format, flags);
        self.set_text_rendering_hint(flags);

        // NoWrap なら原点 PointF(= 幅高さ 0 のレイアウト矩形)で計測
        // (Graphics.cpp:703-708)、それ以外は入力サイズの矩形(709-715)
        let layout = if flags.contains(TextFlag::FORMAT_NO_WRAP) {
            let origin = PointF { X: 0.0, Y: 0.0 };
            RectF {
                X: origin.X,
                Y: origin.Y,
                Width: 0.0,
                Height: 0.0,
            }
        } else {
            RectF {
                X: 0.0,
                Y: 0.0,
                Width: layout_size.cx as f32,
                Height: layout_size.cy as f32,
            }
        };
        let mut bounds = RectF::default();
        if unsafe {
            GdipMeasureString(
                self.graphics,
                PCWSTR(text.as_ptr()),
                text.len() as i32,
                font.as_font_ptr(),
                &layout,
                format.0,
                &mut bounds,
                null_mut(),
                null_mut(),
            )
        } != GpOk
        {
            return false;
        }

        size.cx = (bounds.X + bounds.Width + 1.0) as i32;
        size.cy = (bounds.Y + bounds.Height + 1.0) as i32;

        true
    }

    /// `DrawOutlineText`(`Graphics.cpp:725-764`)。縁取り付きテキストを
    /// 描画する。
    ///
    /// パスへ文字列を追加し、`outline_color` / `outline_width` のペン
    /// (`LineJoinRound`)で輪郭を描いた後、ブラシで塗る。描画中は
    /// `SmoothingModeAntiAlias`(元のモードへ復元、`Graphics.cpp:752-761`)。
    // 原実装(Graphics.cpp:725-729)の引数構成を踏襲するため引数が多い。
    #[allow(clippy::too_many_arguments)]
    pub fn draw_outline_text(
        &mut self,
        text: &[u16],
        font: &Font,
        rect: &RECT,
        brush: &Brush,
        outline_color: Color,
        outline_width: f32,
        flags: TextFlag,
    ) -> bool {
        let text = trim_at_nul(text);
        if self.graphics.is_null() || text.is_empty() || !font.is_created() || !brush.is_created()
        {
            return false;
        }

        let Some(format) = StringFormat::new() else {
            return false;
        };
        Self::set_string_format(&format, flags);
        self.set_text_rendering_hint(flags);

        let Some(path) = GraphicsPath::new() else {
            return false;
        };
        let Some(family) = font.get_family() else {
            return false;
        };
        let layout = rectf_from_rect(rect);
        if unsafe {
            GdipAddPathString(
                path.0,
                PCWSTR(text.as_ptr()),
                text.len() as i32,
                family.0,
                font.get_style(),
                font.get_size(),
                &layout,
                format.0,
            )
        } != GpOk
        {
            return false;
        }

        let mut old_mode = SmoothingMode(0);
        let _ = unsafe { GdipGetSmoothingMode(self.graphics, &mut old_mode) };
        let _ = unsafe { GdipSetSmoothingMode(self.graphics, SmoothingModeAntiAlias) };

        // 原実装(Graphics.cpp:755-757)は Pen 生成・描画の戻り値を確認しない
        if let Some(pen) = Pen::new(outline_color.to_argb(), outline_width) {
            let _ = unsafe { GdipSetPenLineJoin(pen.0, LineJoinRound) };
            let _ = unsafe { GdipDrawPath(self.graphics, pen.0, path.0) };
        }

        let _ = unsafe { GdipFillPath(self.graphics, brush.as_brush_ptr(), path.0) };

        let _ = unsafe { GdipSetSmoothingMode(self.graphics, old_mode) };

        true
    }

    /// `GetOutlineTextSize`(`Graphics.cpp:767-819`)。縁取り付きテキストの
    /// 描画サイズを計測する。
    ///
    /// レイアウト矩形は [`TextFlag::FORMAT_NO_WRAP`] なら 10000x10000、
    /// それ以外は入力 `size`(`Graphics.cpp:796-798`)。パスの境界は
    /// `outline_width` のペン込みで取得する(`GdipGetPathWorldBounds`)。
    /// 結果は右端/下端に +1.0 した切り捨て整数。空文字列では `(0, 0)` で
    /// `true`。
    pub fn get_outline_text_size(
        &mut self,
        text: &[u16],
        font: &Font,
        outline_width: f32,
        flags: TextFlag,
        size: &mut SIZE,
    ) -> bool {
        let layout_size = *size;

        size.cx = 0;
        size.cy = 0;

        if self.graphics.is_null() || !font.is_created() {
            return false;
        }

        let text = trim_at_nul(text);
        if text.is_empty() {
            return true;
        }

        let Some(format) = StringFormat::new() else {
            return false;
        };
        Self::set_string_format(&format, flags);
        self.set_text_rendering_hint(flags);

        let Some(path) = GraphicsPath::new() else {
            return false;
        };
        let Some(family) = font.get_family() else {
            return false;
        };
        let layout = if flags.contains(TextFlag::FORMAT_NO_WRAP) {
            RectF {
                X: 0.0,
                Y: 0.0,
                Width: 10000.0,
                Height: 10000.0,
            }
        } else {
            RectF {
                X: 0.0,
                Y: 0.0,
                Width: layout_size.cx as f32,
                Height: layout_size.cy as f32,
            }
        };
        if unsafe {
            GdipAddPathString(
                path.0,
                PCWSTR(text.as_ptr()),
                text.len() as i32,
                family.0,
                font.get_style(),
                font.get_size(),
                &layout,
                format.0,
            )
        } != GpOk
        {
            return false;
        }

        // Gdiplus::Color() の既定値は不透明の黒(0xFF000000)(Graphics.cpp:802)
        let pen = Pen::new(0xFF00_0000, outline_width);
        if let Some(pen) = &pen {
            let _ = unsafe { GdipSetPenLineJoin(pen.0, LineJoinRound) };
        }

        let mut bounds = RectF::default();
        let pen_ptr = pen.as_ref().map_or(null(), |p| p.0.cast_const());
        if unsafe { GdipGetPathWorldBounds(path.0, &mut bounds, null(), pen_ptr) } != GpOk {
            return false;
        }

        size.cx = (bounds.X + bounds.Width + 1.0) as i32;
        size.cy = (bounds.Y + bounds.Height + 1.0) as i32;

        true
    }

    /// `GetLineSpacing`(`Graphics.cpp:822-827`)。フォントの行間
    /// (`Font::GetHeight`)を取得する。失敗時は 0.0。
    #[must_use]
    pub fn get_line_spacing(&self, font: &Font) -> f32 {
        if self.graphics.is_null() || !font.is_created() {
            return 0.0;
        }
        let mut height = 0.0f32;
        if unsafe { GdipGetFontHeight(font.as_font_ptr(), self.graphics, &mut height) } != GpOk {
            return 0.0;
        }
        height
    }

    /// `GetFontAscent`(`Graphics.cpp:830-845`)。フォントのアセント
    /// (ピクセル)を取得する。失敗時・`EmHeight == 0` では 0.0。
    #[must_use]
    pub fn get_font_ascent(&self, font: &Font) -> f32 {
        self.get_font_cell_metric(font, true)
    }

    /// `GetFontDescent`(`Graphics.cpp:848-863`)。フォントのディセント
    /// (ピクセル)を取得する。失敗時・`EmHeight == 0` では 0.0。
    #[must_use]
    pub fn get_font_descent(&self, font: &Font) -> f32 {
        self.get_font_cell_metric(font, false)
    }

    /// `GetFontAscent` / `GetFontDescent` の共通部
    /// (`size * cell / em`、`Graphics.cpp:835-844` / `853-862`)。
    fn get_font_cell_metric(&self, font: &Font, ascent: bool) -> f32 {
        if self.graphics.is_null() || !font.is_created() {
            return 0.0;
        }

        let Some(family) = font.get_family() else {
            return 0.0;
        };

        let style = font.get_style();
        let mut em_height = 0u16;
        let _ = unsafe { GdipGetEmHeight(family.0, style, &mut em_height) };
        if em_height == 0 {
            return 0.0;
        }

        let mut cell = 0u16;
        let _ = unsafe {
            if ascent {
                GdipGetCellAscent(family.0, style, &mut cell)
            } else {
                GdipGetCellDescent(family.0, style, &mut cell)
            }
        };

        font.get_size() * f32::from(cell) / f32::from(em_height)
    }

    /// `SetStringFormat`(`Graphics.cpp:878-912`)。`TextFlag` を
    /// `StringFormat` に反映する。
    ///
    /// 原実装どおり、既定の `FormatFlags` を取得してから NoWrap / NoClip /
    /// LineLimit を設定・解除する(`Graphics.cpp:880-893`)。
    fn set_string_format(format: &StringFormat, flags: TextFlag) {
        let mut format_flags = 0i32;
        let _ = unsafe { GdipGetStringFormatFlags(format.0, &mut format_flags) };
        if flags.contains(TextFlag::FORMAT_NO_WRAP) {
            format_flags |= StringFormatFlagsNoWrap.0;
        } else {
            format_flags &= !StringFormatFlagsNoWrap.0;
        }
        if flags.contains(TextFlag::FORMAT_NO_CLIP) {
            format_flags |= StringFormatFlagsNoClip.0;
        } else {
            format_flags &= !StringFormatFlagsNoClip.0;
        }
        if flags.contains(TextFlag::FORMAT_CLIP_LAST_LINE) {
            format_flags |= StringFormatFlagsLineLimit.0;
        } else {
            format_flags &= !StringFormatFlagsLineLimit.0;
        }
        let _ = unsafe { GdipSetStringFormatFlags(format.0, format_flags) };

        // 水平アライメント(Graphics.cpp:895-899)。マスク値 0x3 のような
        // 不正な組み合わせでは原実装の switch と同じく何も設定しない。
        let horz = flags & TextFlag::FORMAT_HORZ_ALIGN_MASK;
        if horz == TextFlag::FORMAT_LEFT {
            let _ = unsafe { GdipSetStringFormatAlign(format.0, StringAlignmentNear) };
        } else if horz == TextFlag::FORMAT_RIGHT {
            let _ = unsafe { GdipSetStringFormatAlign(format.0, StringAlignmentFar) };
        } else if horz == TextFlag::FORMAT_HORZ_CENTER {
            let _ = unsafe { GdipSetStringFormatAlign(format.0, StringAlignmentCenter) };
        }

        // 垂直アライメント(Graphics.cpp:901-905)
        let vert = flags & TextFlag::FORMAT_VERT_ALIGN_MASK;
        if vert == TextFlag::FORMAT_TOP {
            let _ = unsafe { GdipSetStringFormatLineAlign(format.0, StringAlignmentNear) };
        } else if vert == TextFlag::FORMAT_BOTTOM {
            let _ = unsafe { GdipSetStringFormatLineAlign(format.0, StringAlignmentFar) };
        } else if vert == TextFlag::FORMAT_VERT_CENTER {
            let _ = unsafe { GdipSetStringFormatLineAlign(format.0, StringAlignmentCenter) };
        }

        // トリミング(Graphics.cpp:907-911)
        let trimming = if flags.contains(TextFlag::FORMAT_END_ELLIPSIS) {
            StringTrimmingEllipsisCharacter
        } else if flags.contains(TextFlag::FORMAT_WORD_ELLIPSIS) {
            StringTrimmingEllipsisWord
        } else if flags.contains(TextFlag::FORMAT_TRIM_CHAR) {
            StringTrimmingCharacter
        } else {
            StringTrimmingNone
        };
        let _ = unsafe { GdipSetStringFormatTrimming(format.0, trimming) };
    }

    /// `SetTextRenderingHint`(`Graphics.cpp:915-929`)。`TextFlag` の
    /// 描画品質フラグを `TextRenderingHint` に反映する。
    fn set_text_rendering_hint(&mut self, flags: TextFlag) {
        let hint = if flags.contains(TextFlag::DRAW_ANTIALIAS) {
            if flags.contains(TextFlag::DRAW_HINTING) {
                TextRenderingHintAntiAliasGridFit
            } else {
                TextRenderingHintAntiAlias
            }
        } else if flags.contains(TextFlag::DRAW_NO_ANTIALIAS) {
            if flags.contains(TextFlag::DRAW_HINTING) {
                TextRenderingHintSingleBitPerPixelGridFit
            } else {
                TextRenderingHintSingleBitPerPixel
            }
        } else if flags.contains(TextFlag::DRAW_CLEAR_TYPE) {
            TextRenderingHintClearTypeGridFit
        } else {
            TextRenderingHintSystemDefault
        };
        let _ = unsafe { GdipSetTextRenderingHint(self.graphics, hint) };
    }
}

impl Drop for Canvas<'_> {
    fn drop(&mut self) {
        if !self.graphics.is_null() {
            unsafe { GdipDeleteGraphics(self.graphics) };
        }
    }
}
