#![cfg(windows)]
//! TVTest の `CThemeDraw`(`src/ThemeDraw.cpp` / `src/ThemeDraw.h`)の Rust 移植。
//!
//! `HDC` と [`StyleScaling`] を保持し、各種テーマスタイルを描画する薄いラッパー。`Theme` の自由関数
//! ([`tvtest_theme`])へ委譲するが、枠線(`BorderStyle`)の各辺幅だけは描画前に `ToPixels` で
//! DPI スケーリングする点が `Theme::Draw` 直接呼び出しとの違い(ThemeDraw.cpp:116)。
//!
//! 原実装のコンストラクタは `(CStyleManager*, CStyleScaling*)` を取り、scaling が null のとき
//! `pStyleManager->InitStyleScaling(&m_StyleScaling)`(システム DPI 取得を伴う Win32 依存)で
//! 初期化する。本移植では呼び出し側が初期化済みの [`StyleScaling`] を渡す前提とし、その分岐は
//! 呼び出し側の責務とする([`StyleManager::init_style_scaling`](tvtest_style::StyleManager::init_style_scaling) を参照)。

use tvtest_style::StyleScaling;
use tvtest_theme::{
    self as theme, BackgroundStyle, BorderStyle, BorderType, FillStyle, ForegroundStyle,
    GradientStyle, SolidStyle,
};
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Gdi::{DRAW_TEXT_FORMAT, HDC};

/// `HDC` + DPI スケーリングを保持するテーマ描画ヘルパー。原実装 `Theme::CThemeDraw`(ThemeDraw.h:34)。
pub struct ThemeDraw {
    scaling: StyleScaling,
    hdc: HDC,
}

impl ThemeDraw {
    /// 初期化済みの [`StyleScaling`] を与えて生成する。
    ///
    /// 原実装コンストラクタ(ThemeDraw.cpp:34)の scaling 注入相当。`hdc` は未設定(NULL)。
    /// [`begin`](Self::begin) で描画先 `HDC` を設定する。
    pub fn new(scaling: StyleScaling) -> Self {
        Self {
            scaling,
            hdc: HDC::default(),
        }
    }

    /// 描画先 `HDC` を設定する(`Begin`、ThemeDraw.cpp:47)。NULL の場合 `false` を返す。
    pub fn begin(&mut self, hdc: HDC) -> bool {
        self.hdc = hdc;
        !hdc.0.is_null()
    }

    /// 描画先 `HDC` を解除する(`End`、ThemeDraw.cpp:54)。
    pub fn end(&mut self) {
        self.hdc = HDC::default();
    }

    /// 保持する [`StyleScaling`] を返す(`GetStyleScaling`、ThemeDraw.h:52)。
    pub fn style_scaling(&self) -> &StyleScaling {
        &self.scaling
    }

    /// 単色塗りを描画する(`Draw(SolidStyle, RECT)`、ThemeDraw.cpp:60)。
    pub fn draw_solid(&self, style: &SolidStyle, rect: &RECT) -> bool {
        if self.hdc.0.is_null() {
            return false;
        }
        theme::draw_solid(self.hdc, rect, style)
    }

    /// グラデーション塗りを描画する(`Draw(GradientStyle, RECT)`、ThemeDraw.cpp:68)。
    pub fn draw_gradient(&self, style: &GradientStyle, rect: &RECT) -> bool {
        if self.hdc.0.is_null() {
            return false;
        }
        theme::draw_gradient(self.hdc, rect, style)
    }

    /// 塗りスタイルを描画する(`Draw(FillStyle, RECT)`、ThemeDraw.cpp:76)。
    pub fn draw_fill(&self, style: &FillStyle, rect: &RECT) -> bool {
        if self.hdc.0.is_null() {
            return false;
        }
        theme::draw_fill(self.hdc, rect, style)
    }

    /// 背景を描画する(矩形不変版、`Draw(BackgroundStyle, const RECT&)`、ThemeDraw.cpp:84)。
    pub fn draw_background(&self, style: &BackgroundStyle, rect: &RECT) -> bool {
        let mut rc = *rect;
        self.draw_background_rect(style, &mut rc)
    }

    /// 背景を描画し、枠線分だけ `rect` を内側へ縮める(`Draw(BackgroundStyle, RECT*)`、ThemeDraw.cpp:91)。
    ///
    /// 枠線がある場合は(スケール済みの)枠を描いてから内側を塗る。戻り値は塗り描画の結果。
    pub fn draw_background_rect(&self, style: &BackgroundStyle, rect: &mut RECT) -> bool {
        if self.hdc.0.is_null() {
            return false;
        }
        if style.border.kind != BorderType::None {
            self.draw_border_rect(&style.border, rect);
        }
        theme::draw_fill(self.hdc, rect, &style.fill)
    }

    /// 前景(文字)を描画する(`Draw(ForegroundStyle, RECT, text, Flags)`、ThemeDraw.cpp:101)。
    pub fn draw_foreground(
        &self,
        style: &ForegroundStyle,
        rect: &RECT,
        text: &[u16],
        flags: DRAW_TEXT_FORMAT,
    ) -> bool {
        if self.hdc.0.is_null() {
            return false;
        }
        theme::draw_foreground(self.hdc, rect, style, text, flags)
    }

    /// 枠線を描画する(矩形不変版、`Draw(BorderStyle, const RECT&)`、ThemeDraw.cpp:109)。
    pub fn draw_border(&self, style: &BorderStyle, rect: &RECT) -> bool {
        let mut rc = *rect;
        self.draw_border_rect(style, &mut rc)
    }

    /// 枠線を描画し、`rect` を枠の内側へ縮める(`Draw(BorderStyle, RECT*)`、ThemeDraw.cpp:116)。
    ///
    /// `Theme::Draw` 直接呼び出しと異なり、各辺の幅を `StyleScaling::to_pixels` で物理ピクセルへ
    /// 換算してから描画する(これが ThemeDraw の本質)。
    pub fn draw_border_rect(&self, style: &BorderStyle, rect: &mut RECT) -> bool {
        if self.hdc.0.is_null() {
            return false;
        }

        let mut draw_style = *style;
        self.scaling.to_pixels(&mut draw_style.width.left);
        self.scaling.to_pixels(&mut draw_style.width.top);
        self.scaling.to_pixels(&mut draw_style.width.right);
        self.scaling.to_pixels(&mut draw_style.width.bottom);

        theme::draw_border_rect(self.hdc, rect, &draw_style)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tvtest_draw_util::Offscreen;
    use tvtest_theme::{BorderWidth, ThemeColor};
    use tvtest_style::IntValue;
    use windows::Win32::Graphics::Gdi::DT_LEFT;

    fn make_offscreen(width: i32, height: i32) -> Offscreen {
        let mut off = Offscreen::new();
        assert!(off.create(width, height, None));
        off
    }

    fn scaling_at(dpi: i32) -> StyleScaling {
        let mut s = StyleScaling::default();
        assert!(s.set_dpi(dpi));
        s
    }

    fn rc(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left,
            top,
            right,
            bottom,
        }
    }

    #[test]
    fn begin_end_and_not_begun() {
        let mut td = ThemeDraw::new(StyleScaling::default());
        // begin 前は描画系が false
        assert!(!td.draw_solid(&SolidStyle::new(ThemeColor::from_rgb(255, 0, 0)), &rc(0, 0, 10, 10)));

        let off = make_offscreen(20, 20);
        assert!(td.begin(off.dc()));
        assert!(td.draw_solid(&SolidStyle::new(ThemeColor::from_rgb(0, 0, 255)), &rc(0, 0, 20, 20)));

        td.end();
        assert!(!td.draw_solid(&SolidStyle::new(ThemeColor::from_rgb(0, 0, 255)), &rc(0, 0, 20, 20)));
    }

    #[test]
    fn begin_null_hdc_returns_false() {
        let mut td = ThemeDraw::new(StyleScaling::default());
        assert!(!td.begin(HDC::default()));
    }

    #[test]
    fn style_scaling_getter() {
        let td = ThemeDraw::new(scaling_at(144));
        assert_eq!(td.style_scaling().get_dpi(), 144);
    }

    #[test]
    fn border_width_scaled_at_96() {
        // 96 DPI: 既定幅 1(LogicalPixel)→ 1px。全辺 1 で 1px 経路。内側へ各 1 縮む。
        let off = make_offscreen(60, 60);
        let mut td = ThemeDraw::new(scaling_at(96));
        assert!(td.begin(off.dc()));

        let style = BorderStyle::new(BorderType::Solid, ThemeColor::from_rgb(200, 200, 200));
        let mut r = rc(0, 0, 60, 60);
        assert!(td.draw_border_rect(&style, &mut r));
        assert_eq!(r, rc(1, 1, 59, 59));
    }

    #[test]
    fn border_width_scaled_at_192() {
        // 192 DPI: 幅 1 LogicalPixel → ToPixels = MulDiv(1,192,96) = 2px。内側へ各 2 縮む。
        let off = make_offscreen(60, 60);
        let mut td = ThemeDraw::new(scaling_at(192));
        assert!(td.begin(off.dc()));

        let style = BorderStyle::new(BorderType::Solid, ThemeColor::from_rgb(200, 200, 200));
        let mut r = rc(0, 0, 60, 60);
        assert!(td.draw_border_rect(&style, &mut r));
        assert_eq!(r, rc(2, 2, 58, 58));
    }

    #[test]
    fn border_multiwidth_scaled() {
        // 192 DPI・各辺異なる論理幅 → 各辺 2 倍にスケールして縮む。
        let off = make_offscreen(80, 80);
        let mut td = ThemeDraw::new(scaling_at(192));
        assert!(td.begin(off.dc()));

        let style = BorderStyle {
            kind: BorderType::Raised,
            color: ThemeColor::from_rgb(100, 100, 100),
            width: BorderWidth {
                left: IntValue::with_logical(2),
                top: IntValue::with_logical(3),
                right: IntValue::with_logical(4),
                bottom: IntValue::with_logical(5),
            },
        };
        let mut r = rc(0, 0, 80, 80);
        assert!(td.draw_border_rect(&style, &mut r));
        // 各辺 MulDiv(w,192,96)=2w → left4/top6/right8/bottom10
        assert_eq!(r, rc(4, 6, 72, 70));
    }

    #[test]
    fn draw_background_with_border_scales_and_fills() {
        let off = make_offscreen(40, 40);
        let mut td = ThemeDraw::new(scaling_at(192));
        assert!(td.begin(off.dc()));

        let style = BackgroundStyle::new(
            FillStyle::from_solid(SolidStyle::new(ThemeColor::from_rgb(30, 30, 30))),
            BorderStyle::new(BorderType::Solid, ThemeColor::from_rgb(180, 180, 180)),
        );
        let mut r = rc(0, 0, 40, 40);
        assert!(td.draw_background_rect(&style, &mut r));
        // 既定幅 1 logical → 2px、枠分縮む
        assert_eq!(r, rc(2, 2, 38, 38));
    }

    #[test]
    fn draw_fill_gradient_foreground_smoke() {
        let off = make_offscreen(64, 24);
        let mut td = ThemeDraw::new(StyleScaling::default());
        assert!(td.begin(off.dc()));
        let r = rc(0, 0, 64, 24);

        assert!(td.draw_fill(
            &FillStyle::from_solid(SolidStyle::new(ThemeColor::from_rgb(10, 20, 30))),
            &r
        ));
        assert!(td.draw_gradient(
            &GradientStyle::new(
                tvtest_theme::GradientType::Normal,
                tvtest_theme::GradientDirection::Horz,
                ThemeColor::from_rgb(0, 0, 0),
                ThemeColor::from_rgb(255, 255, 255),
            ),
            &r
        ));
        let text: Vec<u16> = "Hi".encode_utf16().collect();
        assert!(td.draw_foreground(
            &ForegroundStyle::new(FillStyle::from_solid(SolidStyle::new(ThemeColor::from_rgb(
                255, 255, 255
            )))),
            &r,
            &text,
            DT_LEFT
        ));
    }
}
