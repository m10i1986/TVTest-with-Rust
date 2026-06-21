#![cfg(windows)]
//! TVTest の `CColorScheme`(`src/ColorScheme.h` / `src/ColorScheme.cpp`)の基盤部分。
//!
//! 提供するもの:
//! - [`indices`] — 色 / グラデーション / 枠線の索引定数(`ColorScheme.h` の enum から機械生成)。
//!   値は [`CColorScheme`] 内部配列の添字で、`tvtest_theme_manager` の `STYLE_LIST` から参照される。
//! - [`ColorSchemeSource`] — 色設定から色やテーマスタイルを取得する抽象トレイト
//!   (原実装 `CColorScheme` の参照面)。
//! - [`CColorScheme`] — 色設定本体。約250色の既定値テーブル(ダーク/ライト)・グラデーション/枠線
//!   定義(`data` モジュールに機械生成)を保持し、色やテーマスタイルを返す。[`ColorSchemeSource`]
//!   を実装する。
//!
//! `Load`/`Save`(`CSettings` のファイル I/O)と `CColorSchemeList`(ディレクトリ走査)は Win32/
//! 設定層に依存するため対象外。

pub mod indices;
mod data;

use data::{BORDER_INFO_LIST, COLOR_INFO_LIST, CUSTOM_DEFAULT_BORDER_LIST, GRADIENT_INFO_LIST};
use indices::{NUM_BORDERS, NUM_COLORS, NUM_GRADIENTS};
use tvtest_theme::{
    BorderStyle, BorderType, BorderWidth, FillStyle, FillType, GradientDirection, GradientStyle,
    GradientType, SolidStyle, ThemeColor,
};

const N_COLORS: usize = NUM_COLORS as usize;
const N_GRADIENTS: usize = NUM_GRADIENTS as usize;
const N_BORDERS: usize = NUM_BORDERS as usize;

/// 無効な色を表す値(Win32 `CLR_INVALID`)。
pub const CLR_INVALID: u32 = 0xFFFF_FFFF;

/// `HEXRGB` マクロ(ColorScheme.cpp:65)。`0xRRGGBB` を `COLORREF`(`0x00BBGGRR`)へ変換する。
pub const fn hexrgb(hex: u32) -> u32 {
    let r = (hex >> 16) & 0xFF;
    let g = (hex >> 8) & 0xFF;
    let b = hex & 0xFF;
    r | (g << 8) | (b << 16)
}

/// 色設定(`CColorScheme`)から色・テーマスタイルを取得するための抽象。
///
/// `CThemeManager` はこのトレイト越しに色設定へアクセスするため、具体的な `CColorScheme` 実装
/// (既定色テーブルや `Load`/`Save`)とは独立してテーマ構築ロジックを検証できる。
pub trait ColorSchemeSource {
    /// 色索引(`indices::COLOR_*`)から `COLORREF` を得る。無効時は [`CLR_INVALID`]。
    /// 原実装 `CColorScheme::GetColor(int)`。
    fn get_color(&self, color_type: i32) -> u32;

    /// 色名から `COLORREF` を得る。無効時は [`CLR_INVALID`]。
    /// 原実装 `CColorScheme::GetColor(LPCTSTR)`。
    fn get_color_by_name(&self, name: &str) -> u32;

    /// グラデーション索引(`indices::GRADIENT_*`)から塗りスタイルを得る。
    /// 原実装 `CColorScheme::GetFillStyle(int, Theme::FillStyle*)`。
    fn get_fill_style(&self, gradient: i32) -> FillStyle;

    /// 枠線索引(`indices::BORDER_*`)から枠線スタイルを得る。
    /// 原実装 `CColorScheme::GetBorderStyle(int, Theme::BorderStyle*)`。
    fn get_border_style(&self, border: i32) -> BorderStyle;
}

// ---------------------------------------------------------------------------
// データテーブルのエントリ型(data モジュールが参照、ColorScheme.h:474-494)
// ---------------------------------------------------------------------------

/// 色 1 件の定義(`CColorScheme::ColorInfo`、ColorScheme.h:474)。
#[derive(Clone, Copy)]
pub(crate) struct ColorInfo {
    pub(crate) default_color: u32,
    pub(crate) default_light_color: u32,
    pub(crate) text: &'static str,
    pub(crate) name: &'static str,
}

/// グラデーション 1 件の定義(`CColorScheme::GradientInfo`、ColorScheme.h:481)。
#[derive(Clone, Copy)]
pub(crate) struct GradientInfo {
    pub(crate) text: &'static str,
    pub(crate) direction: GradientDirection,
    pub(crate) enable_direction: bool,
    pub(crate) color1: i32,
    pub(crate) color2: i32,
}

/// 枠線 1 件の定義(`CColorScheme::BorderInfo`、ColorScheme.h:489)。
#[derive(Clone, Copy)]
pub(crate) struct BorderInfo {
    /// 設定キー名。`Load`/`Save`(未移植)で使うため現状は未参照。
    #[allow(dead_code)]
    pub(crate) text: &'static str,
    pub(crate) default_type: BorderType,
    pub(crate) color: i32,
}

// ---------------------------------------------------------------------------
// 色設定本体(ColorScheme.h:38 / ColorScheme.cpp)
// ---------------------------------------------------------------------------

/// 既定スキームの種別(`CColorScheme::BaseSchemeType`、ColorScheme.h:415)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BaseSchemeType {
    #[default]
    Dark,
    Light,
}

/// 色設定のグラデーション記述子(`CColorScheme::GradientStyle`、ColorScheme.h:399)。
///
/// 種別と方向のみを持つ(色は色索引から別途引く)。`Theme::GradientStyle`(色を含む)とは別物。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchemeGradientStyle {
    pub kind: GradientType,
    pub direction: GradientDirection,
}

/// 色設定の塗り記述子(`CColorScheme::FillStyle`、ColorScheme.h:407)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchemeFillStyle {
    pub kind: FillType,
    pub gradient: SchemeGradientStyle,
}

/// TVTest の色設定(`CColorScheme`、ColorScheme.h:38)。
///
/// 約250色 + グラデーション56 + 枠線55 を保持し、色やテーマスタイルを返す。`Load`/`Save`(`CSettings`)
/// は対象外。生成時は [`set_default`](Self::set_default) でダークの既定値が入る。
pub struct CColorScheme {
    base_scheme: BaseSchemeType,
    color_list: [u32; N_COLORS],
    fill_list: [SchemeFillStyle; N_GRADIENTS],
    border_list: [BorderType; N_BORDERS],
    name: String,
    file_name: String,
    loaded_flags: [bool; N_COLORS],
}

impl Default for CColorScheme {
    fn default() -> Self {
        Self::new()
    }
}

impl CColorScheme {
    /// 既定(ダーク)で生成する(`CColorScheme()`、ColorScheme.cpp:483)。
    pub fn new() -> Self {
        let mut scheme = Self {
            base_scheme: BaseSchemeType::Dark,
            color_list: [0; N_COLORS],
            fill_list: [SchemeFillStyle {
                kind: FillType::Gradient,
                gradient: SchemeGradientStyle {
                    kind: GradientType::Normal,
                    direction: GradientDirection::Vert,
                },
            }; N_GRADIENTS],
            border_list: [BorderType::None; N_BORDERS],
            name: String::new(),
            file_name: String::new(),
            loaded_flags: [false; N_COLORS],
        };
        scheme.set_default();
        scheme
    }

    /// 既定値を設定する(`SetDefault`、ColorScheme.cpp:1022)。ダーク基準・各色は既定色・塗りは
    /// グラデーション(Normal + 表の方向)・枠線はカスタム既定。
    pub fn set_default(&mut self) {
        self.base_scheme = BaseSchemeType::Dark;
        for (dst, info) in self.color_list.iter_mut().zip(COLOR_INFO_LIST.iter()) {
            *dst = info.default_color;
        }
        for (dst, info) in self.fill_list.iter_mut().zip(GRADIENT_INFO_LIST.iter()) {
            *dst = SchemeFillStyle {
                kind: FillType::Gradient,
                gradient: SchemeGradientStyle {
                    kind: GradientType::Normal,
                    direction: info.direction,
                },
            };
        }
        for (dst, &border) in self.border_list.iter_mut().zip(CUSTOM_DEFAULT_BORDER_LIST.iter()) {
            *dst = border;
        }
    }

    /// 色索引から `COLORREF` を得る(`GetColor(int)`、ColorScheme.cpp:495)。範囲外は [`CLR_INVALID`]。
    pub fn get_color(&self, color_type: i32) -> u32 {
        if !(0..NUM_COLORS).contains(&color_type) {
            return CLR_INVALID;
        }
        self.color_list[color_type as usize]
    }

    /// 色名(設定キー)から `COLORREF` を得る(`GetColor(LPCTSTR)`、ColorScheme.cpp:503)。
    /// 大小無視(原実装 `lstrcmpi`)。見つからなければ [`CLR_INVALID`]。
    pub fn get_color_by_name(&self, name: &str) -> u32 {
        for (i, info) in COLOR_INFO_LIST.iter().enumerate() {
            if info.text.eq_ignore_ascii_case(name) {
                return self.color_list[i];
            }
        }
        CLR_INVALID
    }

    /// 色を設定する(`SetColor`、ColorScheme.cpp:513)。範囲外は失敗。
    pub fn set_color(&mut self, color_type: i32, color: u32) -> bool {
        if !(0..NUM_COLORS).contains(&color_type) {
            return false;
        }
        self.color_list[color_type as usize] = color;
        true
    }

    /// グラデーション種別を得る(`GetGradientType(int)`、ColorScheme.cpp:522)。範囲外は `Normal`。
    pub fn get_gradient_type(&self, gradient: i32) -> GradientType {
        if !(0..NUM_GRADIENTS).contains(&gradient) {
            return GradientType::Normal;
        }
        self.fill_list[gradient as usize].gradient.kind
    }

    /// 名前(`...Gradient`)からグラデーション種別を得る(`GetGradientType(LPCTSTR)`、ColorScheme.cpp:530)。
    pub fn get_gradient_type_by_name(&self, name: &str) -> GradientType {
        if name.len() > 8 {
            let (prefix, suffix) = name.split_at(name.len() - 8);
            if suffix.eq_ignore_ascii_case("Gradient") {
                for (i, info) in GRADIENT_INFO_LIST.iter().enumerate() {
                    if info.text.eq_ignore_ascii_case(prefix) {
                        return self.fill_list[i].gradient.kind;
                    }
                }
            }
        }
        GradientType::Normal
    }

    /// グラデーションの種別・方向を設定する(`SetGradientStyle`、ColorScheme.cpp:545)。範囲外は失敗。
    pub fn set_gradient_style(&mut self, gradient: i32, style: SchemeGradientStyle) -> bool {
        if !(0..NUM_GRADIENTS).contains(&gradient) {
            return false;
        }
        self.fill_list[gradient as usize].gradient = style;
        true
    }

    /// グラデーションの種別・方向(色なし)を得る(`GetGradientStyle(int, GradientStyle*)`、ColorScheme.cpp:555)。
    pub fn get_gradient_style(&self, gradient: i32) -> Option<SchemeGradientStyle> {
        if !(0..NUM_GRADIENTS).contains(&gradient) {
            return None;
        }
        Some(self.fill_list[gradient as usize].gradient)
    }

    /// 色付きの `Theme::GradientStyle` を得る(`GetGradientStyle(int, Theme::GradientStyle*)`、ColorScheme.cpp:564)。
    ///
    /// `Color1` 索引が `>= 0` なら色索引から、さもなくば既定色(全 0)。
    pub fn get_theme_gradient_style(&self, gradient: i32) -> Option<GradientStyle> {
        if !(0..NUM_GRADIENTS).contains(&gradient) {
            return None;
        }
        let g = gradient as usize;
        let info = &GRADIENT_INFO_LIST[g];
        let (color1, color2) = if info.color1 >= 0 {
            (
                ThemeColor::from_colorref(self.color_list[info.color1 as usize]),
                ThemeColor::from_colorref(self.color_list[info.color2 as usize]),
            )
        } else {
            (ThemeColor::default(), ThemeColor::default())
        };
        Some(GradientStyle {
            kind: self.fill_list[g].gradient.kind,
            direction: self.fill_list[g].gradient.direction,
            color1,
            color2,
        })
    }

    /// `Theme::FillStyle` を得る(`GetFillStyle`、ColorScheme.cpp:581)。範囲外は `None`。
    pub fn get_fill_style(&self, gradient: i32) -> Option<FillStyle> {
        if !(0..NUM_GRADIENTS).contains(&gradient) {
            return None;
        }
        let g = gradient as usize;
        Some(FillStyle {
            kind: self.fill_list[g].kind,
            solid: SolidStyle::default(),
            gradient: self.get_theme_gradient_style(gradient).unwrap(),
        })
    }

    /// 枠線種別を得る(`GetBorderType`、ColorScheme.cpp:591)。範囲外は `None`。
    pub fn get_border_type(&self, border: i32) -> BorderType {
        if !(0..NUM_BORDERS).contains(&border) {
            return BorderType::None;
        }
        self.border_list[border as usize]
    }

    /// 枠線種別を設定する(`SetBorderType`、ColorScheme.cpp:599)。範囲外は失敗。
    pub fn set_border_type(&mut self, border: i32, border_type: BorderType) -> bool {
        if !(0..NUM_BORDERS).contains(&border) {
            return false;
        }
        self.border_list[border as usize] = border_type;
        true
    }

    /// `Theme::BorderStyle` を得る(`GetBorderStyle`、ColorScheme.cpp:609)。範囲外は `None`。
    ///
    /// 色索引が `>= 0` なら色索引から、さもなくば既定色。幅は既定(全辺 1)。
    pub fn get_border_style(&self, border: i32) -> Option<BorderStyle> {
        if !(0..NUM_BORDERS).contains(&border) {
            return None;
        }
        let b = border as usize;
        let color = if BORDER_INFO_LIST[b].color >= 0 {
            ThemeColor::from_colorref(self.color_list[BORDER_INFO_LIST[b].color as usize])
        } else {
            ThemeColor::default()
        };
        Some(BorderStyle {
            kind: self.border_list[b],
            color,
            width: BorderWidth::default(),
        })
    }

    /// スキーム名を得る(`GetName`、ColorScheme.h:442)。
    pub fn name(&self) -> &str {
        &self.name
    }

    /// スキーム名を設定する(`SetName`、ColorScheme.cpp:622)。
    pub fn set_name(&mut self, name: &str) {
        self.name = name.to_string();
    }

    /// ファイル名を得る(`GetFileName`、ColorScheme.h:444)。
    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    /// ファイル名を設定する(`SetFileName`、ColorScheme.cpp:1009)。
    pub fn set_file_name(&mut self, file_name: &str) -> bool {
        self.file_name = file_name.to_string();
        true
    }

    /// ファイルから読み込まれたか(`IsLoadedFromFile`、ColorScheme.h:445)。
    pub fn is_loaded_from_file(&self) -> bool {
        !self.file_name.is_empty()
    }

    /// 基準スキームを設定する(`SetBaseScheme`、ColorScheme.cpp:1016)。
    pub fn set_base_scheme(&mut self, base_scheme: BaseSchemeType) {
        self.base_scheme = base_scheme;
    }

    /// 基準スキームを得る(`GetBaseScheme`、ColorScheme.h:452)。
    pub fn base_scheme(&self) -> BaseSchemeType {
        self.base_scheme
    }

    /// 指定色が読み込み済みか(`IsLoaded`、ColorScheme.cpp:1091)。範囲外は `false`。
    pub fn is_loaded(&self, color_type: i32) -> bool {
        if !(0..NUM_COLORS).contains(&color_type) {
            return false;
        }
        self.loaded_flags[color_type as usize]
    }

    /// 全色を読み込み済みにする(`SetLoaded`、ColorScheme.cpp:1099)。
    pub fn set_loaded(&mut self) {
        self.loaded_flags = [true; N_COLORS];
    }

    /// 別スキームと一致するか(`CompareScheme`、ColorScheme.cpp:1105)。
    ///
    /// 色は `other` が読み込み済みの索引のみ比較。塗り(全グラデーション)と枠線(全枠)も比較する。
    pub fn compare_scheme(&self, other: &CColorScheme) -> bool {
        for ((color, other_color), loaded) in self
            .color_list
            .iter()
            .zip(other.color_list.iter())
            .zip(other.loaded_flags.iter())
        {
            if *loaded && color != other_color {
                return false;
            }
        }
        for (fill, other_fill) in self.fill_list.iter().zip(other.fill_list.iter()) {
            if fill != other_fill {
                return false;
            }
        }
        for (border, other_border) in self.border_list.iter().zip(other.border_list.iter()) {
            if border != other_border {
                return false;
            }
        }
        true
    }

    /// 色の表示名を得る(`GetColorName`、ColorScheme.cpp:1040)。範囲外は `None`。
    pub fn get_color_name(color_type: i32) -> Option<&'static str> {
        if !(0..NUM_COLORS).contains(&color_type) {
            return None;
        }
        Some(COLOR_INFO_LIST[color_type as usize].name)
    }

    /// 既定色を得る(`GetDefaultColor`、ColorScheme.cpp:1048)。
    ///
    /// ライト基準でライト用既定色が有効ならそれを、さもなくばダーク用既定色。範囲外は [`CLR_INVALID`]。
    pub fn get_default_color(base_scheme: BaseSchemeType, color_type: i32) -> u32 {
        if !(0..NUM_COLORS).contains(&color_type) {
            return CLR_INVALID;
        }
        let info = &COLOR_INFO_LIST[color_type as usize];
        if base_scheme == BaseSchemeType::Light && info.default_light_color != CLR_INVALID {
            info.default_light_color
        } else {
            info.default_color
        }
    }

    /// 既定グラデーション種別(`GetDefaultGradientType`、ColorScheme.cpp:1059)。常に `Normal`。
    pub fn get_default_gradient_type(_gradient: i32) -> GradientType {
        GradientType::Normal
    }

    /// 既定グラデーション(種別 `Normal` + 表の方向)を得る(`GetDefaultGradientStyle`、ColorScheme.cpp:1065)。
    pub fn get_default_gradient_style(gradient: i32) -> Option<SchemeGradientStyle> {
        if !(0..NUM_GRADIENTS).contains(&gradient) {
            return None;
        }
        Some(SchemeGradientStyle {
            kind: GradientType::Normal,
            direction: GRADIENT_INFO_LIST[gradient as usize].direction,
        })
    }

    /// グラデーション方向の変更が許可されているか(`IsGradientDirectionEnabled`、ColorScheme.cpp:1075)。
    pub fn is_gradient_direction_enabled(gradient: i32) -> bool {
        if !(0..NUM_GRADIENTS).contains(&gradient) {
            return false;
        }
        GRADIENT_INFO_LIST[gradient as usize].enable_direction
    }

    /// 既定枠線種別(`GetDefaultBorderType`、ColorScheme.cpp:1083)。範囲外は `None`。
    pub fn get_default_border_type(border: i32) -> BorderType {
        if !(0..NUM_BORDERS).contains(&border) {
            return BorderType::None;
        }
        BORDER_INFO_LIST[border as usize].default_type
    }

    /// 指定色を使うグラデーション索引を得る(`GetColorGradient`、ColorScheme.cpp:1126)。なければ `-1`。
    pub fn get_color_gradient(color_type: i32) -> i32 {
        for (i, info) in GRADIENT_INFO_LIST.iter().enumerate() {
            if info.color1 == color_type || info.color2 == color_type {
                return i as i32;
            }
        }
        -1
    }

    /// 指定色を使う枠線索引を得る(`GetColorBorder`、ColorScheme.cpp:1137)。なければ `-1`。
    pub fn get_color_border(color_type: i32) -> i32 {
        for (i, info) in BORDER_INFO_LIST.iter().enumerate() {
            if info.color == color_type {
                return i as i32;
            }
        }
        -1
    }
}

impl ColorSchemeSource for CColorScheme {
    fn get_color(&self, color_type: i32) -> u32 {
        self.get_color(color_type)
    }

    fn get_color_by_name(&self, name: &str) -> u32 {
        self.get_color_by_name(name)
    }

    fn get_fill_style(&self, gradient: i32) -> FillStyle {
        self.get_fill_style(gradient).unwrap_or_default()
    }

    fn get_border_style(&self, border: i32) -> BorderStyle {
        self.get_border_style(border).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indices::*;

    #[test]
    fn hexrgb_byte_order() {
        // HEXRGB(0xRRGGBB) = COLORREF(0x00BBGGRR)
        assert_eq!(hexrgb(0x44_86_E8), 0x00_E8_86_44);
        assert_eq!(hexrgb(0x33_33_33), 0x00_33_33_33);
    }

    #[test]
    fn default_colors_loaded() {
        let s = CColorScheme::new();
        assert_eq!(s.base_scheme(), BaseSchemeType::Dark);
        assert_eq!(s.get_color(COLOR_STATUSBACK1), hexrgb(0x333333));
        assert_eq!(s.get_color(COLOR_STATUSBACK2), hexrgb(0x111111));
        // 範囲外
        assert_eq!(s.get_color(-1), CLR_INVALID);
        assert_eq!(s.get_color(NUM_COLORS), CLR_INVALID);
    }

    #[test]
    fn color_by_name_and_set() {
        let mut s = CColorScheme::new();
        assert_eq!(s.get_color_by_name("StatusText"), hexrgb(0x999999));
        // 大小無視
        assert_eq!(s.get_color_by_name("statustext"), hexrgb(0x999999));
        assert_eq!(s.get_color_by_name("nope"), CLR_INVALID);
        // 設定
        assert!(s.set_color(COLOR_STATUSTEXT, 0x0012_3456));
        assert_eq!(s.get_color(COLOR_STATUSTEXT), 0x0012_3456);
        assert!(!s.set_color(-1, 0));
    }

    #[test]
    fn gradient_style_with_colors() {
        let s = CColorScheme::new();
        // GRADIENT_STATUSBACK は色 STATUSBACK1/2、方向 Vert、種別 Normal
        let g = s.get_theme_gradient_style(GRADIENT_STATUSBACK).unwrap();
        assert_eq!(g.kind, GradientType::Normal);
        assert_eq!(g.direction, GradientDirection::Vert);
        assert_eq!(g.color1, ThemeColor::from_colorref(hexrgb(0x333333)));
        assert_eq!(g.color2, ThemeColor::from_colorref(hexrgb(0x111111)));
    }

    #[test]
    fn gradient_with_no_color_is_default() {
        let s = CColorScheme::new();
        // GRADIENT_PROGRAMGUIDE_FAVORITEBUTTON_BACK は color1=-1
        let g = s
            .get_theme_gradient_style(GRADIENT_PROGRAMGUIDE_FAVORITEBUTTON_BACK)
            .unwrap();
        assert_eq!(g.color1, ThemeColor::default());
        assert_eq!(g.color2, ThemeColor::default());
    }

    #[test]
    fn fill_style_is_gradient_by_default() {
        let s = CColorScheme::new();
        let fill = s.get_fill_style(GRADIENT_STATUSBACK).unwrap();
        assert_eq!(fill.kind, FillType::Gradient);
        assert_eq!(fill.gradient.direction, GradientDirection::Vert);
        assert!(s.get_fill_style(-1).is_none());
    }

    #[test]
    fn gradient_set_get_and_type_by_name() {
        let mut s = CColorScheme::new();
        let style = SchemeGradientStyle {
            kind: GradientType::Glossy,
            direction: GradientDirection::Horz,
        };
        assert!(s.set_gradient_style(GRADIENT_STATUSBACK, style));
        assert_eq!(s.get_gradient_style(GRADIENT_STATUSBACK), Some(style));
        assert_eq!(s.get_gradient_type(GRADIENT_STATUSBACK), GradientType::Glossy);
        // 名前経由(StatusBack + Gradient)
        assert_eq!(s.get_gradient_type_by_name("StatusBackGradient"), GradientType::Glossy);
        assert_eq!(s.get_gradient_type_by_name("Unknown"), GradientType::Normal);
    }

    #[test]
    fn border_style_and_type() {
        let s = CColorScheme::new();
        // BORDER_STATUS のカスタム既定は Raised、色は STATUSBORDER
        assert_eq!(s.get_border_type(BORDER_STATUS), BorderType::Raised);
        let border = s.get_border_style(BORDER_STATUS).unwrap();
        assert_eq!(border.kind, BorderType::Raised);
        assert_eq!(
            border.color,
            ThemeColor::from_colorref(s.get_color(COLOR_STATUSBORDER))
        );
        assert!(s.get_border_style(NUM_BORDERS).is_none());
    }

    #[test]
    fn border_type_set() {
        let mut s = CColorScheme::new();
        assert!(s.set_border_type(BORDER_STATUS, BorderType::Solid));
        assert_eq!(s.get_border_type(BORDER_STATUS), BorderType::Solid);
        assert!(!s.set_border_type(-1, BorderType::Solid));
    }

    #[test]
    fn default_border_type_differs_from_custom() {
        // BORDER_STATUSHIGHLIGHT: BorderInfo の既定は None、カスタム既定(SetDefault 後)は Sunken
        assert_eq!(
            CColorScheme::get_default_border_type(BORDER_STATUSHIGHLIGHT),
            BorderType::None
        );
        let s = CColorScheme::new();
        assert_eq!(s.get_border_type(BORDER_STATUSHIGHLIGHT), BorderType::Sunken);
    }

    #[test]
    fn default_color_light_fallback() {
        // ライト基準でライト色が無効(CLR_INVALID)なら ダーク既定へフォールバック
        assert_eq!(
            CColorScheme::get_default_color(BaseSchemeType::Light, COLOR_STATUSBACK1),
            CColorScheme::get_default_color(BaseSchemeType::Dark, COLOR_STATUSBACK1)
        );
        assert_eq!(
            CColorScheme::get_default_color(BaseSchemeType::Dark, COLOR_STATUSBACK1),
            hexrgb(0x333333)
        );
        assert_eq!(CColorScheme::get_default_color(BaseSchemeType::Dark, -1), CLR_INVALID);
    }

    #[test]
    fn statics_lookup() {
        assert_eq!(CColorScheme::get_color_name(COLOR_STATUSBACK1), Some("ステータスバー 背景1"));
        assert_eq!(CColorScheme::get_color_name(-1), None);
        assert!(!CColorScheme::is_gradient_direction_enabled(GRADIENT_STATUSBACK));
        assert!(CColorScheme::is_gradient_direction_enabled(GRADIENT_STATUSHIGHLIGHTBACK));
        // 色 → グラデーション / 枠線の逆引き
        assert_eq!(CColorScheme::get_color_gradient(COLOR_STATUSBACK1), GRADIENT_STATUSBACK);
        assert_eq!(CColorScheme::get_color_gradient(COLOR_STATUSTEXT), -1);
        assert_eq!(CColorScheme::get_color_border(COLOR_SCREENBORDER), BORDER_SCREEN);
    }

    #[test]
    fn loaded_and_compare() {
        let a = CColorScheme::new();
        let mut b = CColorScheme::new();
        // 既定同士は一致(b は未ロードなので色比較はスキップされるが塗り/枠は一致)
        assert!(a.compare_scheme(&b));
        // b の色を変えて全ロード扱い -> 不一致
        b.set_color(COLOR_STATUSBACK1, 0x0000_00FF);
        b.set_loaded();
        assert!(!a.compare_scheme(&b));
        assert!(b.is_loaded(COLOR_STATUSBACK1));
        assert!(!a.is_loaded(COLOR_STATUSBACK1));
    }

    #[test]
    fn works_as_color_scheme_source() {
        let scheme = CColorScheme::new();
        let source: &dyn ColorSchemeSource = &scheme;
        assert_eq!(source.get_color(COLOR_STATUSBACK1), hexrgb(0x333333));
        assert_eq!(source.get_color_by_name("StatusText"), hexrgb(0x999999));
        assert_eq!(source.get_fill_style(GRADIENT_STATUSBACK).kind, FillType::Gradient);
        assert_eq!(source.get_border_style(BORDER_STATUS).kind, BorderType::Raised);
        // 範囲外は既定(None 種別)
        assert_eq!(source.get_fill_style(-1).kind, FillType::None);
    }

    #[test]
    fn name_and_file() {
        let mut s = CColorScheme::new();
        assert_eq!(s.name(), "");
        s.set_name("MyScheme");
        assert_eq!(s.name(), "MyScheme");
        assert!(!s.is_loaded_from_file());
        s.set_file_name("scheme.ini");
        assert!(s.is_loaded_from_file());
    }
}
