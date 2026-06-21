#![cfg(windows)]
//! TVTest の `CColorScheme`(`src/ColorScheme.h` / `src/ColorScheme.cpp`)の基盤部分。
//!
//! 現段階では以下のみを提供する:
//! - [`indices`] — 色 / グラデーション / 枠線の索引定数(`ColorScheme.h` の enum から機械生成)。
//!   値は `CColorScheme` 内部配列の添字で、`tvtest_theme_manager` の `STYLE_LIST` から参照される。
//! - [`ColorSchemeSource`] — 色設定から色やテーマスタイルを取得する抽象トレイト
//!   (原実装 `CColorScheme` の参照面)。
//!
//! `CColorScheme` 本体(約250色の既定値テーブル・グラデーション/枠線定義・`Load`/`Save` の
//! `CSettings` I/O)は規模が大きいため後続段で移植する。本トレイトを実装することで、テーマ層
//! ([`tvtest_theme_manager`])は具体的な色設定実装と分離してテストできる。

pub mod indices;

use tvtest_theme::{BorderStyle, FillStyle};

/// 無効な色を表す値(Win32 `CLR_INVALID`)。
pub const CLR_INVALID: u32 = 0xFFFF_FFFF;

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
