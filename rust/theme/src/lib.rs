#![cfg(windows)]
//! TVTest の `Theme` 名前空間(`src/Theme.cpp` / `src/Theme.h`)の Rust 移植。
//!
//! 色・塗り(単色/グラデーション)・枠線・背景/前景のスタイル構造体と、それらを
//! `HDC` へ描画する関数群を提供する。描画は [`tvtest_draw_util`] の `fill` / `fill_gradient`
//! 系・`draw_text` を土台とする。
//!
//! 文字列・色は原実装の挙動へ厳密一致させ、各関数に原実装の `ファイル:行` をコメントで残す。
//! 純粋ロジック(スタイル合成・矩形演算)は単体テストで等価性を検証し、`HDC` を要する描画関数は
//! [`tvtest_draw_util::Offscreen`] 上のスモークテストで検証する。

use tvtest_draw_util::{
    draw_text, fill, fill_glossy_gradient, fill_gradient, fill_interlaced_gradient, FillDirection,
};
use windows::Win32::Foundation::{COLORREF, RECT};
use windows::Win32::Graphics::Gdi::{
    GetDCPenColor, GetStockObject, LineTo, MoveToEx, Rectangle, SelectObject, SetDCPenColor, DC_PEN,
    DRAW_TEXT_FORMAT, HDC, NULL_BRUSH,
};

/// テーマで用いる色。原実装 `typedef DrawUtil::RGBA ThemeColor`(Theme.h:35)。
pub use tvtest_draw_util::Rgba as ThemeColor;
/// 単色化可能なビットマップ。原実装 `typedef DrawUtil::CMonoColorBitmap ThemeBitmap`(Theme.h:36)。
pub use tvtest_draw_util::MonoColorBitmap as ThemeBitmap;
/// 単色化可能なアイコンリスト。原実装 `typedef DrawUtil::CMonoColorIconList IconList`(Theme.h:37)。
pub use tvtest_draw_util::MonoColorIconList as IconList;

// ---------------------------------------------------------------------------
// スタイル構造体・列挙(Theme.h)
// ---------------------------------------------------------------------------

/// 単色塗りのスタイル。原実装 `Theme::SolidStyle`(Theme.h:39)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SolidStyle {
    pub color: ThemeColor,
}

impl SolidStyle {
    /// 色を指定して生成(`SolidStyle(const ThemeColor &color)`、Theme.h:44)。
    pub fn new(color: ThemeColor) -> Self {
        Self { color }
    }
}

/// グラデーションの種別。原実装 `Theme::GradientType`(Theme.h:49)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GradientType {
    #[default]
    Normal,
    Glossy,
    Interlaced,
}

/// グラデーションの方向。原実装 `Theme::GradientDirection`(Theme.h:55)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GradientDirection {
    #[default]
    Horz,
    Vert,
    HorzMirror,
    VertMirror,
}

/// グラデーションの回転種別。原実装 `Theme::GradientStyle::RotateType`(Theme.h:64)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotateType {
    Left,
    Right,
    OneEighty,
}

/// グラデーション塗りのスタイル。原実装 `Theme::GradientStyle`(Theme.h:62)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GradientStyle {
    pub kind: GradientType,
    pub direction: GradientDirection,
    pub color1: ThemeColor,
    pub color2: ThemeColor,
}

impl GradientStyle {
    /// 各要素を指定して生成(`GradientStyle(type, dir, color1, color2)`、Theme.h:76)。
    pub fn new(
        kind: GradientType,
        direction: GradientDirection,
        color1: ThemeColor,
        color2: ThemeColor,
    ) -> Self {
        Self {
            kind,
            direction,
            color1,
            color2,
        }
    }

    /// 実質単色かどうか(`IsSolid`、Theme.h:88)。
    pub fn is_solid(&self) -> bool {
        self.kind == GradientType::Normal && self.color1 == self.color2
    }

    /// グラデーションを回転する(`Rotate`、Theme.cpp:37)。
    ///
    /// `Left`/`Right` は方向を水平⇔垂直で入れ替え、`Left`/`OneEighty` かつ方向が `Horz`/`Vert`
    /// のとき 2 色を入れ替える。判定は方向入れ替え後の値で行う(原実装どおり)。
    pub fn rotate(&mut self, rotate: RotateType) {
        if matches!(rotate, RotateType::Left | RotateType::Right) {
            self.direction = match self.direction {
                GradientDirection::Horz => GradientDirection::Vert,
                GradientDirection::Vert => GradientDirection::Horz,
                GradientDirection::HorzMirror => GradientDirection::VertMirror,
                GradientDirection::VertMirror => GradientDirection::HorzMirror,
            };
        }
        if matches!(rotate, RotateType::Left | RotateType::OneEighty)
            && matches!(
                self.direction,
                GradientDirection::Horz | GradientDirection::Vert
            )
        {
            std::mem::swap(&mut self.color1, &mut self.color2);
        }
    }
}

/// 塗りの種別。原実装 `Theme::FillType`(Theme.h:92)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FillType {
    #[default]
    None,
    Solid,
    Gradient,
}

/// 塗りのスタイル(単色 or グラデーション)。原実装 `Theme::FillStyle`(Theme.h:98)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FillStyle {
    pub kind: FillType,
    pub solid: SolidStyle,
    pub gradient: GradientStyle,
}

impl FillStyle {
    /// 単色塗りから生成(`FillStyle(const SolidStyle &solid)`、Theme.h:105)。
    pub fn from_solid(solid: SolidStyle) -> Self {
        Self {
            kind: FillType::Solid,
            solid,
            gradient: GradientStyle::default(),
        }
    }

    /// グラデーション塗りから生成(`FillStyle(const GradientStyle &gradient)`、Theme.h:106)。
    pub fn from_gradient(gradient: GradientStyle) -> Self {
        Self {
            kind: FillType::Gradient,
            solid: SolidStyle::default(),
            gradient,
        }
    }

    /// 単色相当の代表色を得る(`GetSolidColor`、Theme.cpp:55)。
    ///
    /// グラデーションは 2 色を比率 128 で混色した色を返す。`None` は既定色(全 0)。
    pub fn get_solid_color(&self) -> ThemeColor {
        match self.kind {
            FillType::Solid => self.solid.color,
            FillType::Gradient => mix_color(self.gradient.color1, self.gradient.color2, 128),
            FillType::None => ThemeColor::default(),
        }
    }
}

/// 枠線の種別。原実装 `Theme::BorderType`(Theme.h:113)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BorderType {
    #[default]
    None,
    Solid,
    Sunken,
    Raised,
}

/// 枠線の各辺幅。原実装 `Theme::BorderWidth`(Theme.h:120)。各辺の既定値は 1。
///
/// 原実装の `Style::IntValue`(DPI スケール対応の値型)は、Theme.cpp が整数値のみを使うため
/// `i32` でモデル化する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BorderWidth {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Default for BorderWidth {
    /// 全辺 1(Theme.h:122-125)。
    fn default() -> Self {
        Self {
            left: 1,
            top: 1,
            right: 1,
            bottom: 1,
        }
    }
}

impl BorderWidth {
    /// 全辺同一幅で生成(`BorderWidth(int Width)`、Theme.h:128)。
    pub fn uniform(width: i32) -> Self {
        Self {
            left: width,
            top: width,
            right: width,
            bottom: width,
        }
    }
}

/// 枠線のスタイル。原実装 `Theme::BorderStyle`(Theme.h:133)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BorderStyle {
    pub kind: BorderType,
    pub color: ThemeColor,
    pub width: BorderWidth,
}

impl BorderStyle {
    /// 種別と色を指定して生成(`BorderStyle(type, color)`、Theme.h:140)。幅は既定(全辺 1)。
    pub fn new(kind: BorderType, color: ThemeColor) -> Self {
        Self {
            kind,
            color,
            width: BorderWidth::default(),
        }
    }
}

/// 背景のスタイル(塗り + 枠線)。原実装 `Theme::BackgroundStyle`(Theme.h:145)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BackgroundStyle {
    pub fill: FillStyle,
    pub border: BorderStyle,
}

impl BackgroundStyle {
    /// 塗りと枠線を指定して生成(`BackgroundStyle(fill, border)`、Theme.h:151)。
    pub fn new(fill: FillStyle, border: BorderStyle) -> Self {
        Self { fill, border }
    }

    /// 塗りのみ指定して生成(`BackgroundStyle(const FillStyle &fill)`、Theme.h:152)。
    pub fn from_fill(fill: FillStyle) -> Self {
        Self {
            fill,
            border: BorderStyle::default(),
        }
    }
}

/// 前景のスタイル(文字色などの塗り)。原実装 `Theme::ForegroundStyle`(Theme.h:157)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ForegroundStyle {
    pub fill: FillStyle,
}

impl ForegroundStyle {
    /// 塗りを指定して生成(`ForegroundStyle(const FillStyle &fill)`、Theme.h:162)。
    pub fn new(fill: FillStyle) -> Self {
        Self { fill }
    }
}

/// 背景 + 前景のスタイル一式。原実装 `Theme::Style`(Theme.h:167)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Style {
    pub back: BackgroundStyle,
    pub fore: ForegroundStyle,
}

impl Style {
    /// 背景と前景を指定して生成(`Style(back, fore)`、Theme.h:173)。
    pub fn new(back: BackgroundStyle, fore: ForegroundStyle) -> Self {
        Self { back, fore }
    }
}

// ---------------------------------------------------------------------------
// 色合成・矩形演算(純粋ロジック)
// ---------------------------------------------------------------------------

/// 2 色を比率 `ratio`(0..=255)で混色する。原実装 `MixColor`(Theme.cpp:299)。
///
/// `ratio = 255` で `color1`、`ratio = 0` で `color2`。アルファも同様に混合する。
/// 原実装の既定比率は 128。
pub fn mix_color(color1: ThemeColor, color2: ThemeColor, ratio: u8) -> ThemeColor {
    let r = ratio as i32;
    let inv = 255 - r;
    ThemeColor::new(
        ((color1.red as i32 * r + color2.red as i32 * inv) / 255) as u8,
        ((color1.green as i32 * r + color2.green as i32 * inv) / 255) as u8,
        ((color1.blue as i32 * r + color2.blue as i32 * inv) / 255) as u8,
        ((color1.alpha as i32 * r + color2.alpha as i32 * inv) / 255) as u8,
    )
}

/// 2 つの塗りスタイルを比率 `ratio` で合成する。原実装 `MixStyle`(Theme.cpp:309)。
pub fn mix_style(style1: &FillStyle, style2: &FillStyle, ratio: u8) -> FillStyle {
    if ratio == 0 || style1.kind == FillType::None {
        return *style2;
    }
    if ratio == 255 || style2.kind == FillType::None {
        return *style1;
    }

    if style1.kind == style2.kind {
        match style1.kind {
            FillType::Solid => {
                return FillStyle::from_solid(SolidStyle::new(mix_color(
                    style1.solid.color,
                    style2.solid.color,
                    ratio,
                )));
            }
            FillType::Gradient => {
                if style1.gradient.kind == style2.gradient.kind
                    && style1.gradient.direction == style2.gradient.direction
                {
                    return FillStyle::from_gradient(GradientStyle::new(
                        style1.gradient.kind,
                        style1.gradient.direction,
                        mix_color(style1.gradient.color1, style2.gradient.color1, ratio),
                        mix_color(style1.gradient.color2, style2.gradient.color2, ratio),
                    ));
                }
            }
            FillType::None => {}
        }
    }

    if style1.kind == FillType::Gradient && style2.kind == FillType::Solid {
        let mut style = *style1;
        style.gradient.color1 = mix_color(style1.gradient.color1, style2.solid.color, ratio);
        style.gradient.color2 = mix_color(style1.gradient.color2, style2.solid.color, ratio);
        return style;
    }
    if style1.kind == FillType::Solid && style2.kind == FillType::Gradient {
        let mut style = *style2;
        style.gradient.color1 = mix_color(style1.solid.color, style2.gradient.color1, ratio);
        style.gradient.color2 = mix_color(style1.solid.color, style2.gradient.color2, ratio);
        return style;
    }

    FillStyle::from_solid(SolidStyle::new(mix_color(
        style1.get_solid_color(),
        style2.get_solid_color(),
        ratio,
    )))
}

/// 枠線幅だけ矩形を外側へ広げる(`AddBorderRect`、Theme.cpp:352)。
pub fn add_border_rect(style: &BorderStyle, rect: &mut RECT) -> bool {
    if style.kind != BorderType::None {
        rect.left -= style.width.left;
        rect.top -= style.width.top;
        rect.right += style.width.right;
        rect.bottom += style.width.bottom;
    }
    true
}

/// 枠線幅だけ矩形を内側へ縮める(`SubtractBorderRect`、Theme.cpp:366)。
pub fn subtract_border_rect(style: &BorderStyle, rect: &mut RECT) -> bool {
    if style.kind != BorderType::None {
        rect.left += style.width.left;
        rect.top += style.width.top;
        rect.right -= style.width.right;
        rect.bottom -= style.width.bottom;
    }
    true
}

/// 枠線の各辺幅を矩形(left/top/right/bottom)へ格納する(`GetBorderWidths`、Theme.cpp:380)。
///
/// 枠線が `None` のときは矩形を空(全 0)にする。
pub fn get_border_widths(style: &BorderStyle, rect: &mut RECT) -> bool {
    if style.kind != BorderType::None {
        rect.left = style.width.left;
        rect.top = style.width.top;
        rect.right = style.width.right;
        rect.bottom = style.width.bottom;
    } else {
        *rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
    }
    true
}

// ---------------------------------------------------------------------------
// 枠線の陰影色(Theme.cpp:178-191、ファイル内 inline ヘルパ)
// ---------------------------------------------------------------------------

/// 色の輝度(`RGBIntensity`、Theme.cpp:178)。
fn rgb_intensity(color: ThemeColor) -> u8 {
    ((color.red as u32 * 19672 + color.green as u32 * 38621 + color.blue as u32 * 7500) >> 16) as u8
}

/// 明るい縁色(`GetHighlightColor`、Theme.cpp:183)。
fn get_highlight_color(color: ThemeColor) -> ThemeColor {
    mix_color(
        ThemeColor::from_rgb(255, 255, 255),
        color,
        48 + rgb_intensity(color) / 3,
    )
}

/// 暗い縁色(`GetShadowColor`、Theme.cpp:188)。
fn get_shadow_color(color: ThemeColor) -> ThemeColor {
    mix_color(
        color,
        ThemeColor::from_rgb(0, 0, 0),
        96 + rgb_intensity(color) / 2,
    )
}

/// `GradientDirection` を DrawUtil の `FillDirection` へ写像する。
///
/// 原実装は `static_cast<DrawUtil::FillDirection>(Style.Direction)`(Theme.cpp:89 ほか)で、
/// 両 enum は同順序のため対応を 1:1 に保つ。
fn to_fill_direction(direction: GradientDirection) -> FillDirection {
    match direction {
        GradientDirection::Horz => FillDirection::Horz,
        GradientDirection::Vert => FillDirection::Vert,
        GradientDirection::HorzMirror => FillDirection::HorzMirror,
        GradientDirection::VertMirror => FillDirection::VertMirror,
    }
}

/// 枠の一辺を、描画領域 `area` との交差部分のみ塗る(`FillBorder`、Theme.cpp:193)。
fn fill_border(
    hdc: HDC,
    area: &RECT,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    color: ThemeColor,
) {
    if area.left < area.right && area.top < area.bottom {
        // 原実装は IntersectRect。交差が空なら塗らない。
        let l = area.left.max(left);
        let t = area.top.max(top);
        let r = area.right.min(right);
        let b = area.bottom.min(bottom);
        if l < r && t < b {
            let rc = RECT {
                left: l,
                top: t,
                right: r,
                bottom: b,
            };
            fill(hdc, &rc, color.to_colorref());
        }
    }
}

// ---------------------------------------------------------------------------
// 描画関数(Theme.cpp、HDC 依存)
// ---------------------------------------------------------------------------

/// 単色塗りを描画する(`Draw(HDC, RECT, SolidStyle)`、Theme.cpp:71)。
pub fn draw_solid(hdc: HDC, rect: &RECT, style: &SolidStyle) -> bool {
    if hdc.0.is_null() {
        return false;
    }
    fill(hdc, rect, style.color.to_colorref())
}

/// グラデーション塗りを描画する(`Draw(HDC, RECT, GradientStyle)`、Theme.cpp:80)。
pub fn draw_gradient(hdc: HDC, rect: &RECT, style: &GradientStyle) -> bool {
    if hdc.0.is_null() {
        return false;
    }
    let direction = to_fill_direction(style.direction);
    let color1 = style.color1.to_colorref();
    let color2 = style.color2.to_colorref();
    match style.kind {
        GradientType::Normal => fill_gradient(hdc, rect, color1, color2, direction),
        // FillGlossyGradient の既定 GlossRatio1=96 / GlossRatio2=48(DrawUtil.h:69)。
        GradientType::Glossy => {
            fill_glossy_gradient(hdc, rect, color1, color2, direction, 96, 48)
        }
        // FillInterlacedGradient の既定 LineColor=RGB(0,0,0) / LineOpacity=48(DrawUtil.h:74)。
        GradientType::Interlaced => {
            fill_interlaced_gradient(hdc, rect, color1, color2, direction, 0, 48)
        }
    }
}

/// 塗りスタイルを描画する(`Draw(HDC, RECT, FillStyle)`、Theme.cpp:106)。
pub fn draw_fill(hdc: HDC, rect: &RECT, style: &FillStyle) -> bool {
    if hdc.0.is_null() {
        return false;
    }
    match style.kind {
        FillType::None => true,
        FillType::Solid => draw_solid(hdc, rect, &style.solid),
        FillType::Gradient => draw_gradient(hdc, rect, &style.gradient),
    }
}

/// 背景(枠線 + 塗り)を描画する(`Draw(HDC, RECT, BackgroundStyle)`、Theme.cpp:126)。
///
/// 枠線がある場合は先に枠線を描いて矩形を内側へ縮め、その内側を塗りで埋める。
pub fn draw_background(hdc: HDC, rect: &RECT, style: &BackgroundStyle) -> bool {
    if hdc.0.is_null() {
        return false;
    }
    let mut rc = *rect;
    if style.border.kind != BorderType::None {
        draw_border_rect(hdc, &mut rc, &style.border);
    }
    draw_fill(hdc, &rc, &style.fill);
    true
}

/// 前景(文字)を描画する(`Draw(HDC, RECT, ForegroundStyle, text, Flags)`、Theme.cpp:141)。
///
/// 塗りが `None` のときは描画せず `true` を返す。グラデーションは 2 色を比率 128 で混色した色を
/// 文字色とする。`flags` は `DrawText` のフォーマットフラグ。
pub fn draw_foreground(
    hdc: HDC,
    rect: &RECT,
    style: &ForegroundStyle,
    text: &[u16],
    flags: DRAW_TEXT_FORMAT,
) -> bool {
    if hdc.0.is_null() {
        return false;
    }
    let color = match style.fill.kind {
        FillType::None => return true,
        FillType::Solid => style.fill.solid.color,
        FillType::Gradient => mix_color(style.fill.gradient.color1, style.fill.gradient.color2, 128),
    };
    // DrawUtil::draw_text と同じく背景透過 + 文字色設定で描画(フォントは選択しない)。
    draw_text(hdc, text, rect, flags, None, Some(color.to_colorref()))
}

/// 枠線を描画する(矩形不変版、`Draw(HDC, RECT, BorderStyle)`、Theme.cpp:171)。
pub fn draw_border(hdc: HDC, rect: &RECT, style: &BorderStyle) -> bool {
    let mut rc = *rect;
    draw_border_rect(hdc, &mut rc, style)
}

/// 枠線を描画し、`rect` を枠の内側へ縮める(`Draw(HDC, RECT*, BorderStyle)`、Theme.cpp:203)。
///
/// 全辺幅 1 のときは DC ペンで 1px 線(Solid は矩形、Sunken/Raised は陰影付き L 字 2 本)を描く。
/// それ以外は各辺を陰影色で塗りつぶす。最後に `rect` から枠幅を差し引く。
pub fn draw_border_rect(hdc: HDC, rect: &mut RECT, style: &BorderStyle) -> bool {
    if hdc.0.is_null() {
        return false;
    }
    if style.kind == BorderType::None {
        return true;
    }

    let rc = *rect;

    if style.width.left == 1 && style.width.top == 1 && style.width.right == 1 && style.width.bottom == 1
    {
        // SAFETY: hdc は有効。DC ペン/NULL ブラシを選択して 1px 枠を描き、状態を元へ戻す。
        unsafe {
            let pen_old = SelectObject(hdc, GetStockObject(DC_PEN));
            let old_dc_pen_color = GetDCPenColor(hdc);
            let brush_old = SelectObject(hdc, GetStockObject(NULL_BRUSH));

            match style.kind {
                BorderType::Solid => {
                    SetDCPenColor(hdc, COLORREF(style.color.to_colorref()));
                    let _ = Rectangle(hdc, rc.left, rc.top, rc.right, rc.bottom);
                }
                BorderType::Sunken => {
                    SetDCPenColor(hdc, COLORREF(get_highlight_color(style.color).to_colorref()));
                    let _ = MoveToEx(hdc, rc.left + 1, rc.bottom - 1, None);
                    let _ = LineTo(hdc, rc.right - 1, rc.bottom - 1);
                    let _ = LineTo(hdc, rc.right - 1, rc.top);
                    SetDCPenColor(hdc, COLORREF(get_shadow_color(style.color).to_colorref()));
                    let _ = LineTo(hdc, rc.left, rc.top);
                    let _ = LineTo(hdc, rc.left, rc.bottom);
                }
                BorderType::Raised => {
                    SetDCPenColor(hdc, COLORREF(get_highlight_color(style.color).to_colorref()));
                    let _ = MoveToEx(hdc, rc.right - 2, rc.top, None);
                    let _ = LineTo(hdc, rc.left, rc.top);
                    let _ = LineTo(hdc, rc.left, rc.bottom - 1);
                    SetDCPenColor(hdc, COLORREF(get_shadow_color(style.color).to_colorref()));
                    let _ = LineTo(hdc, rc.right - 1, rc.bottom - 1);
                    let _ = LineTo(hdc, rc.right - 1, rc.top - 1);
                }
                // None は冒頭で早期 return 済みのため到達しない。
                BorderType::None => unreachable!(),
            }

            let _ = SelectObject(hdc, brush_old);
            SetDCPenColor(hdc, old_dc_pen_color);
            let _ = SelectObject(hdc, pen_old);
        }
    } else {
        let (color1, color2) = match style.kind {
            BorderType::Solid => (style.color, style.color),
            BorderType::Sunken => (get_shadow_color(style.color), get_highlight_color(style.color)),
            BorderType::Raised => (get_highlight_color(style.color), get_shadow_color(style.color)),
            BorderType::None => unreachable!(),
        };

        let mut rc = rc;
        if style.width.top > 0 {
            fill_border(hdc, &rc, rc.left, rc.top, rc.right, rc.top + style.width.top, color1);
            rc.top += style.width.top;
        }
        if style.width.bottom > 0 {
            fill_border(
                hdc,
                &rc,
                rc.left,
                rc.bottom - style.width.bottom,
                rc.right,
                rc.bottom,
                color2,
            );
            rc.bottom -= style.width.bottom;
        }
        if style.width.left > 0 {
            fill_border(hdc, &rc, rc.left, rc.top, rc.left + style.width.left, rc.bottom, color1);
            rc.left += style.width.left;
        }
        if style.width.right > 0 {
            fill_border(hdc, &rc, rc.right - style.width.right, rc.top, rc.right, rc.bottom, color2);
        }
    }

    subtract_border_rect(style, rect);

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use tvtest_draw_util::Offscreen;
    use windows::Win32::Graphics::Gdi::{DT_LEFT, DT_SINGLELINE};

    fn make_offscreen(width: i32, height: i32) -> Offscreen {
        let mut off = Offscreen::new();
        assert!(off.create(width, height, None));
        off
    }

    fn rc(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left,
            top,
            right,
            bottom,
        }
    }

    // ----- mix_color -----

    #[test]
    fn mix_color_endpoints() {
        let c1 = ThemeColor::new(10, 20, 30, 40);
        let c2 = ThemeColor::new(200, 150, 100, 50);
        // ratio=255 -> color1
        assert_eq!(mix_color(c1, c2, 255), c1);
        // ratio=0 -> color2
        assert_eq!(mix_color(c1, c2, 0), c2);
    }

    #[test]
    fn mix_color_half() {
        let c1 = ThemeColor::new(0, 0, 0, 0);
        let c2 = ThemeColor::new(255, 254, 100, 200);
        // ratio=128: (a*128 + b*127)/255
        let m = mix_color(c1, c2, 128);
        assert_eq!(m.red, (255 * 127 / 255) as u8); // 127
        assert_eq!(m.green, (254 * 127 / 255) as u8); // 126
        assert_eq!(m.blue, (100 * 127 / 255) as u8); // 49
        assert_eq!(m.alpha, (200 * 127 / 255) as u8); // 99
    }

    // ----- GradientStyle::rotate -----

    #[test]
    fn rotate_left_horz_swaps_and_turns_vert() {
        let mut g = GradientStyle::new(
            GradientType::Normal,
            GradientDirection::Horz,
            ThemeColor::from_rgb(1, 1, 1),
            ThemeColor::from_rgb(2, 2, 2),
        );
        g.rotate(RotateType::Left);
        // 方向は Horz->Vert、Vert は {Horz,Vert} に含まれるため色入替あり
        assert_eq!(g.direction, GradientDirection::Vert);
        assert_eq!(g.color1, ThemeColor::from_rgb(2, 2, 2));
        assert_eq!(g.color2, ThemeColor::from_rgb(1, 1, 1));
    }

    #[test]
    fn rotate_right_horz_turns_vert_no_swap() {
        let mut g = GradientStyle::new(
            GradientType::Normal,
            GradientDirection::Horz,
            ThemeColor::from_rgb(1, 1, 1),
            ThemeColor::from_rgb(2, 2, 2),
        );
        g.rotate(RotateType::Right);
        assert_eq!(g.direction, GradientDirection::Vert);
        // Right は色入替なし
        assert_eq!(g.color1, ThemeColor::from_rgb(1, 1, 1));
        assert_eq!(g.color2, ThemeColor::from_rgb(2, 2, 2));
    }

    #[test]
    fn rotate_oneeighty_horz_swaps_keeps_direction() {
        let mut g = GradientStyle::new(
            GradientType::Normal,
            GradientDirection::Horz,
            ThemeColor::from_rgb(1, 1, 1),
            ThemeColor::from_rgb(2, 2, 2),
        );
        g.rotate(RotateType::OneEighty);
        assert_eq!(g.direction, GradientDirection::Horz);
        assert_eq!(g.color1, ThemeColor::from_rgb(2, 2, 2));
        assert_eq!(g.color2, ThemeColor::from_rgb(1, 1, 1));
    }

    #[test]
    fn rotate_left_mirror_turns_no_swap() {
        let mut g = GradientStyle::new(
            GradientType::Normal,
            GradientDirection::HorzMirror,
            ThemeColor::from_rgb(1, 1, 1),
            ThemeColor::from_rgb(2, 2, 2),
        );
        g.rotate(RotateType::Left);
        // HorzMirror->VertMirror。Mirror は {Horz,Vert} に含まれないため色入替なし
        assert_eq!(g.direction, GradientDirection::VertMirror);
        assert_eq!(g.color1, ThemeColor::from_rgb(1, 1, 1));
        assert_eq!(g.color2, ThemeColor::from_rgb(2, 2, 2));
    }

    #[test]
    fn rotate_oneeighty_mirror_no_change() {
        let mut g = GradientStyle::new(
            GradientType::Normal,
            GradientDirection::VertMirror,
            ThemeColor::from_rgb(1, 1, 1),
            ThemeColor::from_rgb(2, 2, 2),
        );
        g.rotate(RotateType::OneEighty);
        // 方向は Mirror のまま、色入替もなし
        assert_eq!(g.direction, GradientDirection::VertMirror);
        assert_eq!(g.color1, ThemeColor::from_rgb(1, 1, 1));
        assert_eq!(g.color2, ThemeColor::from_rgb(2, 2, 2));
    }

    // ----- is_solid / get_solid_color -----

    #[test]
    fn is_solid_cases() {
        let same = ThemeColor::from_rgb(5, 6, 7);
        assert!(GradientStyle::new(GradientType::Normal, GradientDirection::Horz, same, same).is_solid());
        assert!(!GradientStyle::new(
            GradientType::Normal,
            GradientDirection::Horz,
            same,
            ThemeColor::from_rgb(8, 8, 8)
        )
        .is_solid());
        assert!(!GradientStyle::new(GradientType::Glossy, GradientDirection::Horz, same, same).is_solid());
    }

    #[test]
    fn get_solid_color_cases() {
        let solid = FillStyle::from_solid(SolidStyle::new(ThemeColor::from_rgb(10, 20, 30)));
        assert_eq!(solid.get_solid_color(), ThemeColor::from_rgb(10, 20, 30));

        let grad = FillStyle::from_gradient(GradientStyle::new(
            GradientType::Normal,
            GradientDirection::Horz,
            ThemeColor::from_rgb(0, 0, 0),
            ThemeColor::from_rgb(100, 100, 100),
        ));
        assert_eq!(grad.get_solid_color(), mix_color(ThemeColor::from_rgb(0, 0, 0), ThemeColor::from_rgb(100, 100, 100), 128));

        assert_eq!(FillStyle::default().get_solid_color(), ThemeColor::default());
    }

    // ----- mix_style -----

    #[test]
    fn mix_style_ratio_extremes() {
        let s1 = FillStyle::from_solid(SolidStyle::new(ThemeColor::from_rgb(10, 10, 10)));
        let s2 = FillStyle::from_solid(SolidStyle::new(ThemeColor::from_rgb(20, 20, 20)));
        assert_eq!(mix_style(&s1, &s2, 0), s2);
        assert_eq!(mix_style(&s1, &s2, 255), s1);
    }

    #[test]
    fn mix_style_none_passthrough() {
        let none = FillStyle::default();
        let solid = FillStyle::from_solid(SolidStyle::new(ThemeColor::from_rgb(20, 20, 20)));
        assert_eq!(mix_style(&none, &solid, 128), solid);
        assert_eq!(mix_style(&solid, &none, 128), solid);
    }

    #[test]
    fn mix_style_both_solid() {
        let s1 = FillStyle::from_solid(SolidStyle::new(ThemeColor::from_rgb(0, 0, 0)));
        let s2 = FillStyle::from_solid(SolidStyle::new(ThemeColor::from_rgb(100, 100, 100)));
        let m = mix_style(&s1, &s2, 128);
        assert_eq!(m.kind, FillType::Solid);
        assert_eq!(m.solid.color, mix_color(ThemeColor::from_rgb(0, 0, 0), ThemeColor::from_rgb(100, 100, 100), 128));
    }

    #[test]
    fn mix_style_both_gradient_same() {
        let g1 = GradientStyle::new(
            GradientType::Normal,
            GradientDirection::Horz,
            ThemeColor::from_rgb(0, 0, 0),
            ThemeColor::from_rgb(10, 10, 10),
        );
        let g2 = GradientStyle::new(
            GradientType::Normal,
            GradientDirection::Horz,
            ThemeColor::from_rgb(100, 100, 100),
            ThemeColor::from_rgb(200, 200, 200),
        );
        let m = mix_style(&FillStyle::from_gradient(g1), &FillStyle::from_gradient(g2), 128);
        assert_eq!(m.kind, FillType::Gradient);
        assert_eq!(m.gradient.color1, mix_color(g1.color1, g2.color1, 128));
        assert_eq!(m.gradient.color2, mix_color(g1.color2, g2.color2, 128));
    }

    #[test]
    fn mix_style_gradient_and_solid() {
        let g = GradientStyle::new(
            GradientType::Normal,
            GradientDirection::Horz,
            ThemeColor::from_rgb(0, 0, 0),
            ThemeColor::from_rgb(40, 40, 40),
        );
        let s = SolidStyle::new(ThemeColor::from_rgb(80, 80, 80));
        let m = mix_style(&FillStyle::from_gradient(g), &FillStyle::from_solid(s), 128);
        assert_eq!(m.kind, FillType::Gradient);
        assert_eq!(m.gradient.color1, mix_color(g.color1, s.color, 128));
        assert_eq!(m.gradient.color2, mix_color(g.color2, s.color, 128));

        // Solid + Gradient は style2(gradient)ベース
        let m2 = mix_style(&FillStyle::from_solid(s), &FillStyle::from_gradient(g), 128);
        assert_eq!(m2.kind, FillType::Gradient);
        assert_eq!(m2.gradient.color1, mix_color(s.color, g.color1, 128));
        assert_eq!(m2.gradient.color2, mix_color(s.color, g.color2, 128));
    }

    #[test]
    fn mix_style_gradient_different_direction_fallback() {
        let g1 = GradientStyle::new(
            GradientType::Normal,
            GradientDirection::Horz,
            ThemeColor::from_rgb(0, 0, 0),
            ThemeColor::from_rgb(40, 40, 40),
        );
        let g2 = GradientStyle::new(
            GradientType::Normal,
            GradientDirection::Vert,
            ThemeColor::from_rgb(100, 100, 100),
            ThemeColor::from_rgb(200, 200, 200),
        );
        // 方向が異なる -> 同種ブロックを抜け、Gradient+Gradient は最後の単色フォールバックへ
        let m = mix_style(&FillStyle::from_gradient(g1), &FillStyle::from_gradient(g2), 128);
        assert_eq!(m.kind, FillType::Solid);
        let expected = mix_color(
            FillStyle::from_gradient(g1).get_solid_color(),
            FillStyle::from_gradient(g2).get_solid_color(),
            128,
        );
        assert_eq!(m.solid.color, expected);
    }

    // ----- border rect 演算 -----

    #[test]
    fn add_subtract_border_rect_roundtrip() {
        let style = BorderStyle {
            kind: BorderType::Solid,
            color: ThemeColor::from_rgb(0, 0, 0),
            width: BorderWidth {
                left: 2,
                top: 3,
                right: 4,
                bottom: 5,
            },
        };
        let mut r = rc(10, 10, 100, 100);
        add_border_rect(&style, &mut r);
        assert_eq!(r, rc(8, 7, 104, 105));
        subtract_border_rect(&style, &mut r);
        assert_eq!(r, rc(10, 10, 100, 100));
    }

    #[test]
    fn border_rect_none_no_change() {
        let style = BorderStyle::default(); // None
        let mut r = rc(10, 10, 100, 100);
        add_border_rect(&style, &mut r);
        assert_eq!(r, rc(10, 10, 100, 100));
        subtract_border_rect(&style, &mut r);
        assert_eq!(r, rc(10, 10, 100, 100));
    }

    #[test]
    fn get_border_widths_cases() {
        let style = BorderStyle {
            kind: BorderType::Solid,
            color: ThemeColor::default(),
            width: BorderWidth {
                left: 2,
                top: 3,
                right: 4,
                bottom: 5,
            },
        };
        let mut r = rc(0, 0, 0, 0);
        get_border_widths(&style, &mut r);
        assert_eq!(r, rc(2, 3, 4, 5));

        let mut r2 = rc(1, 2, 3, 4);
        get_border_widths(&BorderStyle::default(), &mut r2);
        assert_eq!(r2, rc(0, 0, 0, 0));
    }

    // ----- 陰影色 -----

    #[test]
    fn rgb_intensity_values() {
        assert_eq!(rgb_intensity(ThemeColor::from_rgb(0, 0, 0)), 0);
        assert_eq!(rgb_intensity(ThemeColor::from_rgb(255, 255, 255)), 255);
        // (255*19672)>>16 = 76
        assert_eq!(rgb_intensity(ThemeColor::from_rgb(255, 0, 0)), 76);
    }

    #[test]
    fn highlight_shadow_known_values() {
        // 黒の highlight: ratio = 48, mix(white, black, 48) = 48 各成分
        assert_eq!(get_highlight_color(ThemeColor::from_rgb(0, 0, 0)), ThemeColor::new(48, 48, 48, 255));
        // 白の shadow: ratio = 96 + 255/2 = 223, mix(white, black, 223) = 223
        assert_eq!(get_shadow_color(ThemeColor::from_rgb(255, 255, 255)), ThemeColor::new(223, 223, 223, 255));
    }

    // ----- 既定値 -----

    #[test]
    fn defaults() {
        assert_eq!(BorderWidth::default(), BorderWidth { left: 1, top: 1, right: 1, bottom: 1 });
        assert_eq!(BorderWidth::uniform(3), BorderWidth { left: 3, top: 3, right: 3, bottom: 3 });
        assert_eq!(FillStyle::default().kind, FillType::None);
        assert_eq!(BorderStyle::default().kind, BorderType::None);
        assert_eq!(BorderStyle::default().width, BorderWidth::default());
        assert_eq!(GradientStyle::default().kind, GradientType::Normal);
        assert_eq!(GradientStyle::default().direction, GradientDirection::Horz);
    }

    // ----- 描画スモーク(Offscreen DC) -----

    #[test]
    fn draw_null_hdc_returns_false() {
        let r = rc(0, 0, 10, 10);
        assert!(!draw_solid(HDC::default(), &r, &SolidStyle::new(ThemeColor::from_rgb(255, 0, 0))));
        assert!(!draw_gradient(
            HDC::default(),
            &r,
            &GradientStyle::default()
        ));
        assert!(!draw_fill(HDC::default(), &r, &FillStyle::default()));
        assert!(!draw_background(HDC::default(), &r, &BackgroundStyle::default()));
        assert!(!draw_border_rect(HDC::default(), &mut rc(0, 0, 10, 10), &BorderStyle::new(BorderType::Solid, ThemeColor::default())));
        assert!(!draw_foreground(HDC::default(), &r, &ForegroundStyle::default(), &[], DT_LEFT));
    }

    #[test]
    fn draw_solid_and_fill_smoke() {
        let off = make_offscreen(32, 32);
        let r = rc(0, 0, 32, 32);
        assert!(draw_solid(off.dc(), &r, &SolidStyle::new(ThemeColor::from_rgb(0, 0, 255))));
        // FillType::None は常に true(描画なし)
        assert!(draw_fill(off.dc(), &r, &FillStyle::default()));
        assert!(draw_fill(off.dc(), &r, &FillStyle::from_solid(SolidStyle::new(ThemeColor::from_rgb(0, 255, 0)))));
    }

    #[test]
    fn draw_gradient_variants_smoke() {
        let off = make_offscreen(32, 32);
        let r = rc(0, 0, 32, 32);
        let c1 = ThemeColor::from_rgb(0, 0, 255);
        let c2 = ThemeColor::from_rgb(255, 0, 0);
        for kind in [GradientType::Normal, GradientType::Glossy, GradientType::Interlaced] {
            for dir in [
                GradientDirection::Horz,
                GradientDirection::Vert,
                GradientDirection::HorzMirror,
                GradientDirection::VertMirror,
            ] {
                let _ = draw_gradient(off.dc(), &r, &GradientStyle::new(kind, dir, c1, c2));
            }
        }
    }

    #[test]
    fn draw_background_with_border_smoke() {
        let off = make_offscreen(40, 40);
        let r = rc(0, 0, 40, 40);
        let style = BackgroundStyle::new(
            FillStyle::from_solid(SolidStyle::new(ThemeColor::from_rgb(30, 30, 30))),
            BorderStyle::new(BorderType::Raised, ThemeColor::from_rgb(128, 128, 128)),
        );
        assert!(draw_background(off.dc(), &r, &style));
    }

    #[test]
    fn draw_foreground_smoke() {
        let off = make_offscreen(64, 24);
        let r = rc(0, 0, 64, 24);
        let text: Vec<u16> = "Test".encode_utf16().collect();
        let fg = ForegroundStyle::new(FillStyle::from_solid(SolidStyle::new(ThemeColor::from_rgb(255, 255, 255))));
        assert!(draw_foreground(off.dc(), &r, &fg, &text, DT_LEFT | DT_SINGLELINE));
        // None 塗りは描画せず true
        assert!(draw_foreground(off.dc(), &r, &ForegroundStyle::default(), &text, DT_LEFT));
        // 空テキストでも true(DrawText を呼ばない)
        assert!(draw_foreground(off.dc(), &r, &fg, &[], DT_LEFT));
        // グラデーション前景(混色文字色)
        let fg_grad = ForegroundStyle::new(FillStyle::from_gradient(GradientStyle::new(
            GradientType::Normal,
            GradientDirection::Horz,
            ThemeColor::from_rgb(255, 0, 0),
            ThemeColor::from_rgb(0, 0, 255),
        )));
        assert!(draw_foreground(off.dc(), &r, &fg_grad, &text, DT_LEFT));
    }

    #[test]
    fn draw_border_1px_variants_smoke() {
        let off = make_offscreen(40, 40);
        let r = rc(0, 0, 40, 40);
        for kind in [BorderType::Solid, BorderType::Sunken, BorderType::Raised] {
            assert!(draw_border(off.dc(), &r, &BorderStyle::new(kind, ThemeColor::from_rgb(120, 120, 120))));
        }
        // None は描画なしで true
        assert!(draw_border(off.dc(), &r, &BorderStyle::default()));
    }

    #[test]
    fn draw_border_rect_subtracts_and_multiwidth() {
        let off = make_offscreen(60, 60);
        // 全辺幅 1 -> 内側へ各 1 縮む
        let mut r = rc(0, 0, 60, 60);
        assert!(draw_border_rect(off.dc(), &mut r, &BorderStyle::new(BorderType::Solid, ThemeColor::from_rgb(200, 200, 200))));
        assert_eq!(r, rc(1, 1, 59, 59));

        // 複数幅(非1px 経路) -> 枠幅分縮む
        let mut r2 = rc(0, 0, 60, 60);
        let style = BorderStyle {
            kind: BorderType::Raised,
            color: ThemeColor::from_rgb(100, 100, 100),
            width: BorderWidth {
                left: 2,
                top: 3,
                right: 4,
                bottom: 5,
            },
        };
        assert!(draw_border_rect(off.dc(), &mut r2, &style));
        assert_eq!(r2, rc(2, 3, 56, 55));
    }
}
