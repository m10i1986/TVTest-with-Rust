#![cfg(windows)]
//! TVTest の `CThemeManager`(`src/ThemeManager.cpp` / `src/ThemeManager.h`)の Rust 移植。
//!
//! スタイル索引(`STYLE_*`)ごとに「どのグラデーション/枠線/前景色を使うか」を定めた表
//! [`STYLE_LIST`](styles::STYLE_LIST) を持ち、[`ColorSchemeSource`] から実際の色を引いて
//! [`tvtest_theme`] のスタイル(`Style`/`FillStyle`/`BorderStyle`/`BackgroundStyle`/
//! `ForegroundStyle`)を組み立てる。
//!
//! `STYLE_LIST` と `STYLE_*` 定数は原実装の表から機械生成([`styles`] モジュール)。色設定本体は
//! [`tvtest_color_scheme::ColorSchemeSource`] トレイトで抽象化し、`CColorScheme` 本体の移植
//! (既定色テーブル等)とは独立してテストする。

use tvtest_color_scheme::{ColorSchemeSource, CLR_INVALID};
use tvtest_theme::{
    BackgroundStyle, BorderStyle, FillStyle, ForegroundStyle, SolidStyle, Style, ThemeColor,
};

pub mod styles;
pub use styles::*;

// ---------------------------------------------------------------------------
// グラデーション索引の「単色」エンコード(ThemeManager.cpp:36-39)
// ---------------------------------------------------------------------------

/// 単色塗りを示すフラグ。原実装 `GRADIENT_SOLID_FLAG`(ThemeManager.cpp:36)。
pub const GRADIENT_SOLID_FLAG: i32 = 0x1000;

/// 色索引を「単色塗り」エンコードする。原実装 `GRADIENT_SOLID`(ThemeManager.cpp:37)。
pub const fn gradient_solid(color: i32) -> i32 {
    color | GRADIENT_SOLID_FLAG
}

/// グラデーション値が単色塗りエンコードか判定する。原実装 `GRADIENT_IS_SOLID`(ThemeManager.cpp:38)。
pub const fn gradient_is_solid(gradient: i32) -> bool {
    (gradient & GRADIENT_SOLID_FLAG) != 0
}

/// 単色塗りエンコードから色索引を取り出す。原実装 `GRADIENT_GET_SOLID`(ThemeManager.cpp:39)。
pub const fn gradient_get_solid(gradient: i32) -> i32 {
    gradient & 0x0FFF
}

/// スタイル定義表 [`STYLE_LIST`](styles::STYLE_LIST) の 1 エントリ。原実装 `CThemeManager::StyleInfo`。
///
/// `gradient` は `-1`(塗りなし)/ グラデーション索引 / [`gradient_solid`] エンコード(色索引)。
/// `border` は `-1`(枠なし)/ 枠線索引。`fore_color` は `-1`(前景なし)/ 色索引。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StyleInfo {
    pub name: &'static str,
    pub gradient: i32,
    pub border: i32,
    pub fore_color: i32,
}

// ---------------------------------------------------------------------------
// CThemeManager(ThemeManager.cpp:456-585)
// ---------------------------------------------------------------------------

/// 色設定からテーマスタイルを組み立てる。原実装 `Theme::CThemeManager`(ThemeManager.h:35)。
pub struct ThemeManager<'a> {
    color_scheme: &'a dyn ColorSchemeSource,
}

impl<'a> ThemeManager<'a> {
    /// 色設定を与えて生成する(`CThemeManager(const CColorScheme*)`、ThemeManager.cpp:456)。
    pub fn new(color_scheme: &'a dyn ColorSchemeSource) -> Self {
        Self { color_scheme }
    }

    /// 色索引から色を得る(`GetColor(int)`、ThemeManager.cpp:462)。無効色は既定(全 0)。
    pub fn get_color(&self, color_type: i32) -> ThemeColor {
        let cr = self.color_scheme.get_color(color_type);
        if cr == CLR_INVALID {
            ThemeColor::default()
        } else {
            ThemeColor::from_colorref(cr)
        }
    }

    /// 色名から色を得る(`GetColor(LPCTSTR)`、ThemeManager.cpp:471)。無効色は既定(全 0)。
    pub fn get_color_by_name(&self, name: &str) -> ThemeColor {
        let cr = self.color_scheme.get_color_by_name(name);
        if cr == CLR_INVALID {
            ThemeColor::default()
        } else {
            ThemeColor::from_colorref(cr)
        }
    }

    /// スタイル一式(背景 + 前景)を得る(`GetStyle`、ThemeManager.cpp:480)。範囲外は `None`。
    pub fn get_style(&self, style_type: i32) -> Option<Style> {
        if !is_valid_style(style_type) {
            return None;
        }
        Some(Style::new(
            self.get_background_style(style_type)?,
            self.get_foreground_style(style_type)?,
        ))
    }

    /// 塗りスタイルを得る(`GetFillStyle`、ThemeManager.cpp:492)。
    ///
    /// `gradient` が単色エンコードなら色索引から `Solid`(原実装どおり `CLR_INVALID` チェックは
    /// 行わず生の色を使う)、通常索引なら色設定の塗りスタイル、`-1` なら `None`。
    pub fn get_fill_style(&self, style_type: i32) -> Option<FillStyle> {
        if !is_valid_style(style_type) {
            return None;
        }
        let info = &STYLE_LIST[style_type as usize];
        let style = if info.gradient >= 0 {
            if gradient_is_solid(info.gradient) {
                FillStyle::from_solid(SolidStyle::new(ThemeColor::from_colorref(
                    self.color_scheme.get_color(gradient_get_solid(info.gradient)),
                )))
            } else {
                self.color_scheme.get_fill_style(info.gradient)
            }
        } else {
            FillStyle::default()
        };
        Some(style)
    }

    /// 枠線スタイルを得る(`GetBorderStyle`、ThemeManager.cpp:520)。`-1` なら枠なし。
    pub fn get_border_style(&self, style_type: i32) -> Option<BorderStyle> {
        if !is_valid_style(style_type) {
            return None;
        }
        let info = &STYLE_LIST[style_type as usize];
        let style = if info.border >= 0 {
            self.color_scheme.get_border_style(info.border)
        } else {
            BorderStyle::default()
        };
        Some(style)
    }

    /// 背景スタイル(塗り + 枠線)を得る(`GetBackgroundStyle`、ThemeManager.cpp:536)。
    pub fn get_background_style(&self, style_type: i32) -> Option<BackgroundStyle> {
        if !is_valid_style(style_type) {
            return None;
        }
        Some(BackgroundStyle::new(
            self.get_fill_style(style_type)?,
            self.get_border_style(style_type)?,
        ))
    }

    /// 前景スタイル(文字色)を得る(`GetForegroundStyle`、ThemeManager.cpp:548)。
    ///
    /// `fore_color` が `>= 0` なら `Solid`(`GetColor` 経由で `CLR_INVALID` は既定色)、`-1` なら `None`。
    pub fn get_foreground_style(&self, style_type: i32) -> Option<ForegroundStyle> {
        if !is_valid_style(style_type) {
            return None;
        }
        let info = &STYLE_LIST[style_type as usize];
        let fill = if info.fore_color >= 0 {
            FillStyle::from_solid(SolidStyle::new(self.get_color(info.fore_color)))
        } else {
            FillStyle::default()
        };
        Some(ForegroundStyle::new(fill))
    }

    /// スタイル名を得る(`GetStyleName`、ThemeManager.cpp:566)。範囲外は `None`。
    pub fn get_style_name(&self, style_type: i32) -> Option<&'static str> {
        if !is_valid_style(style_type) {
            return None;
        }
        Some(STYLE_LIST[style_type as usize].name)
    }

    /// スタイル名から索引を得る(`ParseStyleName`、ThemeManager.cpp:574)。
    ///
    /// 大小無視(原実装 `lstrcmpi` を ASCII 近似)。空文字・未知の名前は `-1`。
    pub fn parse_style_name(&self, name: &str) -> i32 {
        if name.is_empty() {
            return -1;
        }
        for (i, info) in STYLE_LIST.iter().enumerate() {
            if info.name.eq_ignore_ascii_case(name) {
                return i as i32;
            }
        }
        -1
    }
}

/// スタイル索引が範囲内か(`Type >= 0 && Type < NUM_STYLES`、ThemeManager.cpp 各所)。
fn is_valid_style(style_type: i32) -> bool {
    (0..NUM_STYLES).contains(&style_type)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tvtest_color_scheme::indices::*;
    use tvtest_theme::{BorderType, FillType, GradientDirection, GradientStyle, GradientType};

    /// テスト用の色設定。色は索引から決定的に算出し、特定の索引のみ無効色を返す。
    struct MockScheme;

    /// 前景の `CLR_INVALID` フォールバックを検証するための番兵色索引。
    const INVALID_COLOR_INDEX: i32 = COLOR_STATUSTEXT;

    impl MockScheme {
        /// 索引から決定的な色を作る(無効番兵を除く)。
        fn color_for(index: i32) -> u32 {
            0x0001_0203u32.wrapping_add(index as u32)
        }
    }

    impl ColorSchemeSource for MockScheme {
        fn get_color(&self, color_type: i32) -> u32 {
            if color_type == INVALID_COLOR_INDEX {
                CLR_INVALID
            } else {
                Self::color_for(color_type)
            }
        }

        fn get_color_by_name(&self, name: &str) -> u32 {
            if name == "known" {
                0x0011_2233
            } else {
                CLR_INVALID
            }
        }

        fn get_fill_style(&self, _gradient: i32) -> FillStyle {
            // グラデーション索引経由の塗りを示すマーカー。
            FillStyle::from_gradient(GradientStyle::new(
                GradientType::Glossy,
                GradientDirection::Vert,
                ThemeColor::from_rgb(1, 2, 3),
                ThemeColor::from_rgb(4, 5, 6),
            ))
        }

        fn get_border_style(&self, _border: i32) -> BorderStyle {
            // 枠線索引経由の枠を示すマーカー。
            BorderStyle::new(BorderType::Sunken, ThemeColor::from_rgb(7, 8, 9))
        }
    }

    fn manager() -> ThemeManager<'static> {
        ThemeManager::new(&MockScheme)
    }

    #[test]
    fn style_table_size() {
        assert_eq!(STYLE_LIST.len() as i32, NUM_STYLES);
        assert_eq!(NUM_STYLES, 68);
    }

    #[test]
    fn get_color_valid_and_invalid() {
        let m = manager();
        // 通常の色索引 -> from_colorref
        let expected = ThemeColor::from_colorref(MockScheme::color_for(COLOR_PANELBACK));
        assert_eq!(m.get_color(COLOR_PANELBACK), expected);
        // 無効色 -> 既定(全 0)
        assert_eq!(m.get_color(INVALID_COLOR_INDEX), ThemeColor::default());
    }

    #[test]
    fn get_color_by_name_cases() {
        let m = manager();
        assert_eq!(m.get_color_by_name("known"), ThemeColor::from_colorref(0x0011_2233));
        assert_eq!(m.get_color_by_name("unknown"), ThemeColor::default());
    }

    #[test]
    fn fill_style_solid_uses_raw_color() {
        let m = manager();
        // STYLE_WINDOW_FRAME は gradient_solid(COLOR_WINDOWFRAMEBACK)
        let fill = m.get_fill_style(STYLE_WINDOW_FRAME).unwrap();
        assert_eq!(fill.kind, FillType::Solid);
        // 単色は CLR_INVALID チェックなしの生の色
        assert_eq!(
            fill.solid.color,
            ThemeColor::from_colorref(MockScheme::color_for(COLOR_WINDOWFRAMEBACK))
        );
    }

    #[test]
    fn fill_style_gradient_delegates_to_scheme() {
        let m = manager();
        // STYLE_STATUSBAR_ITEM は通常グラデーション(GRADIENT_STATUSBACK)
        let fill = m.get_fill_style(STYLE_STATUSBAR_ITEM).unwrap();
        assert_eq!(fill.kind, FillType::Gradient);
        assert_eq!(fill.gradient.kind, GradientType::Glossy); // モックのマーカー
    }

    #[test]
    fn fill_style_none_when_no_gradient() {
        let m = manager();
        // STYLE_SCREEN は gradient = -1
        let fill = m.get_fill_style(STYLE_SCREEN).unwrap();
        assert_eq!(fill.kind, FillType::None);
    }

    #[test]
    fn border_style_present_and_none() {
        let m = manager();
        // STYLE_SCREEN は BORDER_SCREEN を持つ
        let border = m.get_border_style(STYLE_SCREEN).unwrap();
        assert_eq!(border.kind, BorderType::Sunken); // モックのマーカー
        // STYLE_PANEL_CONTENT は border = -1
        let none_border = m.get_border_style(STYLE_PANEL_CONTENT).unwrap();
        assert_eq!(none_border.kind, BorderType::None);
    }

    #[test]
    fn foreground_style_uses_checked_color() {
        let m = manager();
        // STYLE_STATUSBAR_ITEM の前景は COLOR_STATUSTEXT(= 無効番兵)
        // GetColor(checked) 経由なので既定色になる
        let fg = m.get_foreground_style(STYLE_STATUSBAR_ITEM).unwrap();
        assert_eq!(fg.fill.kind, FillType::Solid);
        assert_eq!(fg.fill.solid.color, ThemeColor::default());

        // STYLE_SCREEN は前景なし
        let fg_none = m.get_foreground_style(STYLE_SCREEN).unwrap();
        assert_eq!(fg_none.fill.kind, FillType::None);
    }

    #[test]
    fn get_style_combines_background_and_foreground() {
        let m = manager();
        let style = m.get_style(STYLE_STATUSBAR_ITEM).unwrap();
        // 背景の塗りはグラデーション(マーカー)、枠はモックの Sunken
        assert_eq!(style.back.fill.kind, FillType::Gradient);
        assert_eq!(style.back.border.kind, BorderType::Sunken);
        // 前景は Solid(無効番兵 → 既定色)
        assert_eq!(style.fore.fill.kind, FillType::Solid);
    }

    #[test]
    fn range_checks_return_none() {
        let m = manager();
        assert!(m.get_style(-1).is_none());
        assert!(m.get_fill_style(NUM_STYLES).is_none());
        assert!(m.get_border_style(-1).is_none());
        assert!(m.get_background_style(NUM_STYLES).is_none());
        assert!(m.get_foreground_style(-1).is_none());
        assert!(m.get_style_name(NUM_STYLES).is_none());
    }

    #[test]
    fn style_name_and_parse() {
        let m = manager();
        assert_eq!(m.get_style_name(STYLE_SCREEN), Some("screen"));
        assert_eq!(m.parse_style_name("screen"), STYLE_SCREEN);
        // 大小無視
        assert_eq!(m.parse_style_name("Status-Bar.Item"), STYLE_STATUSBAR_ITEM);
        // 未知・空
        assert_eq!(m.parse_style_name("nope"), -1);
        assert_eq!(m.parse_style_name(""), -1);
    }

    #[test]
    fn gradient_solid_encoding() {
        assert!(gradient_is_solid(gradient_solid(COLOR_PANELBACK)));
        assert_eq!(gradient_get_solid(gradient_solid(COLOR_PANELBACK)), COLOR_PANELBACK);
        assert!(!gradient_is_solid(GRADIENT_STATUSBACK));
    }
}
