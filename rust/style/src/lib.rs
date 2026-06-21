#![cfg(windows)]
//! TVTest の `Style` モジュール(`src/Style.cpp` / `src/Style.h`)の Rust 移植。
//!
//! DPI に応じた単位付き値型(`IntValue` / `Size` / `Margins`)、スタイル値の名前付きマップ
//! (`StyleManager`)、DPI スケーリング演算(`StyleScaling`)を提供する。Theme/ThemeDraw や
//! 各 UI ウィンドウのレイアウト土台となる。
//!
//! DPI 換算は Win32 `MulDiv` 互換の [`tvtest_dpi_util::mul_div`] を用い、値は原実装の挙動へ厳密一致
//! させる。各関数に原実装の `ファイル:行` をコメントで残す。
//!
//! Win32 依存で本クレートの対象外:
//! - `CStyleManager::Load`(`CSettings` のファイル I/O)
//! - `CStyleManager::InitStyleScaling(HMONITOR/HWND/RECT)`(モニタ DPI 取得) →
//!   DPI 選択ロジックのみ純粋関数 [`StyleManager::init_style_scaling`] に切り出す
//! - `CStyleScaling::GetScaledSystemMetrics` / `AdjustWindowRect`(Win32 メトリクス)
//! - `Style::GetFontHeight`(`GetTextMetrics`)

use std::collections::HashMap;
use tvtest_dpi_util::mul_div;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Gdi::LOGFONTW;

// ---------------------------------------------------------------------------
// 列挙(Style.h:34-47)
// ---------------------------------------------------------------------------

/// スタイル値の型。原実装 `Style::ValueType`(Style.h:34)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ValueType {
    #[default]
    Void,
    Int,
    Bool,
    /// 原実装の `ValueType::String`。
    Str,
}

/// 値の単位。原実装 `Style::UnitType`(Style.h:41)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UnitType {
    #[default]
    Undefined,
    /// 論理ピクセル(96 DPI 基準)。
    LogicalPixel,
    /// 物理ピクセル(スケール済み)。
    PhysicalPixel,
    /// ポイント(1pt = 1/72in)。
    Point,
    /// DIP(1dp = 1/160in)。原実装 `UnitType::DIP`。
    Dip,
}

// ---------------------------------------------------------------------------
// 値型(Style.h:63-112)
// ---------------------------------------------------------------------------

/// 単位付き整数値。原実装 `Style::IntValue`(= `ValueTemplate<int>`、Style.h:63,79)。
///
/// 既定は `value = 0` / `unit = Undefined`(`ValueTemplate() = default`)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntValue {
    pub value: i32,
    pub unit: UnitType,
}

impl Default for IntValue {
    fn default() -> Self {
        Self {
            value: 0,
            unit: UnitType::Undefined,
        }
    }
}

impl IntValue {
    /// 値と単位を指定して生成(`ValueTemplate(T v, UnitType u)`、Style.h:69)。
    pub fn new(value: i32, unit: UnitType) -> Self {
        Self { value, unit }
    }

    /// 値のみ指定(単位は `LogicalPixel`)。原実装 `ValueTemplate(T v)` の既定単位(Style.h:69)。
    pub fn with_logical(value: i32) -> Self {
        Self {
            value,
            unit: UnitType::LogicalPixel,
        }
    }

    /// 値のみの一致判定(`operator==(T v)`、Style.h:75)。単位は無視する。
    pub fn eq_value(&self, value: i32) -> bool {
        self.value == value
    }
}

/// 幅・高さの組。原実装 `Style::Size`(Style.h:81)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Size {
    pub width: IntValue,
    pub height: IntValue,
}

impl Size {
    /// 幅・高さ・単位を指定して生成(`Size(int w, int h, UnitType u)`、Style.h:87)。
    pub fn new(width: i32, height: i32, unit: UnitType) -> Self {
        Self {
            width: IntValue::new(width, unit),
            height: IntValue::new(height, unit),
        }
    }
}

/// 余白(各辺)。原実装 `Style::Margins`(Style.h:93)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Margins {
    pub left: IntValue,
    pub top: IntValue,
    pub right: IntValue,
    pub bottom: IntValue,
}

impl Margins {
    /// 各辺・単位を指定して生成(`Margins(l, t, r, b, u)`、Style.h:101)。
    pub fn new(left: i32, top: i32, right: i32, bottom: i32, unit: UnitType) -> Self {
        Self {
            left: IntValue::new(left, unit),
            top: IntValue::new(top, unit),
            right: IntValue::new(right, unit),
            bottom: IntValue::new(bottom, unit),
        }
    }

    /// 全辺同一の余白(`Margins(int m, UnitType u)`、Style.h:105)。
    pub fn uniform(margin: i32, unit: UnitType) -> Self {
        Self::new(margin, margin, margin, margin, unit)
    }

    /// 左右の合計(`Horz`、Style.h:110)。
    pub fn horz(&self) -> i32 {
        self.left.value + self.right.value
    }

    /// 上下の合計(`Vert`、Style.h:111)。
    pub fn vert(&self) -> i32 {
        self.top.value + self.bottom.value
    }
}

/// フォント(LOGFONT + ポイントサイズ)。原実装 `Style::Font`(Style.h:114)。
///
/// `operator==`(`CompareLogFont` + Size 比較)は本移植では用途が無いため未実装。
#[derive(Clone, Copy, Default)]
pub struct Font {
    pub log_font: LOGFONTW,
    pub size: IntValue,
}

impl Font {
    /// LOGFONT とサイズ・単位を指定して生成(`Font(const LOGFONT &lf, int s, UnitType u)`、Style.h:121)。
    pub fn new(log_font: LOGFONTW, size: i32, unit: UnitType) -> Self {
        Self {
            log_font,
            size: IntValue::new(size, unit),
        }
    }
}

// ---------------------------------------------------------------------------
// スタイル値マップ(Style.h:49-61,155 / Style.cpp:106-470)
// ---------------------------------------------------------------------------

/// スタイル値の中身。原実装 `Style::StyleInfo::Value`(union)+ `Type` を 1 つの enum で表す。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum StyleValue {
    #[default]
    Void,
    Int(i32),
    Bool(bool),
    Str(String),
}

impl StyleValue {
    /// 対応する [`ValueType`] を返す(原実装 `StyleInfo::Type`)。
    pub fn value_type(&self) -> ValueType {
        match self {
            StyleValue::Void => ValueType::Void,
            StyleValue::Int(_) => ValueType::Int,
            StyleValue::Bool(_) => ValueType::Bool,
            StyleValue::Str(_) => ValueType::Str,
        }
    }
}

/// 名前付きスタイル値。原実装 `Style::StyleInfo`(Style.h:49)。
///
/// `unit` は `value` が [`StyleValue::Int`] のときのみ意味を持つ(原実装どおり)。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StyleInfo {
    pub name: String,
    pub value: StyleValue,
    pub unit: UnitType,
}

/// スタイル値の名前付きマップと設定フラグ。原実装 `Style::CStyleManager`(Style.h:155)。
///
/// ファイル読込(`Load`)は対象外。設定フラグ(forced_dpi/scale_font 等)は `Load` が設定する
/// 値に相当し、本移植ではセッターで与える。
#[derive(Debug, Clone, Default)]
pub struct StyleManager {
    style_map: HashMap<String, StyleInfo>,
    forced_dpi: i32,
    handle_dpi_changed: bool,
    scale_font: bool,
    use_dark_menu: bool,
    dark_dialog: bool,
}

impl StyleManager {
    /// 既定値で生成する。原実装の `Load` 前の初期状態(各フラグ既定 true、forced_dpi=0)。
    pub fn new() -> Self {
        Self {
            style_map: HashMap::new(),
            forced_dpi: 0,
            handle_dpi_changed: true,
            scale_font: true,
            use_dark_menu: true,
            dark_dialog: true,
        }
    }

    /// スタイル情報を丸ごと登録/置換する(`Set(const StyleInfo&)`、Style.cpp:106)。
    pub fn set_info(&mut self, info: StyleInfo) -> bool {
        if info.name.is_empty() {
            return false;
        }
        self.style_map.insert(info.name.clone(), info);
        true
    }

    /// 名前でスタイル情報を取得する(`Get(StyleInfo*)`、Style.cpp:122)。
    pub fn get_info(&self, name: &str) -> Option<StyleInfo> {
        if name.is_empty() {
            return None;
        }
        self.style_map.get(name).cloned()
    }

    /// 整数値を登録する(`Set(name, IntValue)`、Style.cpp:137)。
    pub fn set_int_value(&mut self, name: &str, value: IntValue) -> bool {
        if name.is_empty() {
            return false;
        }
        match self.style_map.get_mut(name) {
            Some(info) => {
                info.value = StyleValue::Int(value.value);
                info.unit = value.unit;
            }
            None => {
                self.style_map.insert(
                    name.to_string(),
                    StyleInfo {
                        name: name.to_string(),
                        value: StyleValue::Int(value.value),
                        unit: value.unit,
                    },
                );
            }
        }
        true
    }

    /// 整数値を取得する(`Get(name, IntValue*)`、Style.cpp:160)。型が Int でなければ `None`。
    pub fn get_int_value(&self, name: &str) -> Option<IntValue> {
        if name.is_empty() {
            return None;
        }
        match self.style_map.get(name) {
            Some(StyleInfo {
                value: StyleValue::Int(v),
                unit,
                ..
            }) => Some(IntValue::new(*v, *unit)),
            _ => None,
        }
    }

    /// 真偽値を登録する(`Set(name, bool)`、Style.cpp:176)。
    pub fn set_bool(&mut self, name: &str, value: bool) -> bool {
        if name.is_empty() {
            return false;
        }
        match self.style_map.get_mut(name) {
            Some(info) => info.value = StyleValue::Bool(value),
            None => {
                self.style_map.insert(
                    name.to_string(),
                    StyleInfo {
                        name: name.to_string(),
                        value: StyleValue::Bool(value),
                        unit: UnitType::Undefined,
                    },
                );
            }
        }
        true
    }

    /// 真偽値を取得する(`Get(name, bool*)`、Style.cpp:197)。型が Bool でなければ `None`。
    pub fn get_bool(&self, name: &str) -> Option<bool> {
        if name.is_empty() {
            return None;
        }
        match self.style_map.get(name) {
            Some(StyleInfo {
                value: StyleValue::Bool(b),
                ..
            }) => Some(*b),
            _ => None,
        }
    }

    /// 文字列値を登録する(`Set(name, String)`、Style.cpp:212)。
    pub fn set_string(&mut self, name: &str, value: &str) -> bool {
        if name.is_empty() {
            return false;
        }
        match self.style_map.get_mut(name) {
            Some(info) => info.value = StyleValue::Str(value.to_string()),
            None => {
                self.style_map.insert(
                    name.to_string(),
                    StyleInfo {
                        name: name.to_string(),
                        value: StyleValue::Str(value.to_string()),
                        unit: UnitType::Undefined,
                    },
                );
            }
        }
        true
    }

    /// 文字列値を取得する(`Get(name, String*)`、Style.cpp:233)。型が String でなければ `None`。
    pub fn get_string(&self, name: &str) -> Option<String> {
        if name.is_empty() {
            return None;
        }
        match self.style_map.get(name) {
            Some(StyleInfo {
                value: StyleValue::Str(s),
                ..
            }) => Some(s.clone()),
            _ => None,
        }
    }

    /// サイズを `name.width` / `name.height` として登録する(`Set(name, Size)`、Style.cpp:248)。
    pub fn set_size(&mut self, name: &str, value: &Size) -> bool {
        if name.is_empty() {
            return false;
        }
        self.set_int_value(&format!("{name}.width"), value.width);
        self.set_int_value(&format!("{name}.height"), value.height);
        true
    }

    /// サイズを取得する(`Get(name, Size*)`、Style.cpp:266)。幅か高さのどちらかが取れれば `Some`。
    pub fn get_size(&self, name: &str) -> Option<Size> {
        if name.is_empty() {
            return None;
        }
        let width = self.get_int_value(&format!("{name}.width"));
        let height = self.get_int_value(&format!("{name}.height"));
        if width.is_none() && height.is_none() {
            return None;
        }
        let mut size = Size::default();
        if let Some(w) = width {
            size.width = w;
        }
        if let Some(h) = height {
            size.height = h;
        }
        Some(size)
    }

    /// 余白を `name.left/.top/.right/.bottom` として登録する(`Set(name, Margins)`、Style.cpp:284)。
    pub fn set_margins(&mut self, name: &str, value: &Margins) -> bool {
        if name.is_empty() {
            return false;
        }
        self.set_int_value(&format!("{name}.left"), value.left);
        self.set_int_value(&format!("{name}.top"), value.top);
        self.set_int_value(&format!("{name}.right"), value.right);
        self.set_int_value(&format!("{name}.bottom"), value.bottom);
        true
    }

    /// 余白を取得する(`Get(name, Margins*)`、Style.cpp:308)。
    ///
    /// まず `name` 自体の単一値があれば全辺に適用し、続いて各辺(`.left` 等)で上書きする。
    /// いずれか 1 つでも取れれば `Some`。
    pub fn get_margins(&self, name: &str) -> Option<Margins> {
        if name.is_empty() {
            return None;
        }
        let mut margins = Margins::default();
        let mut ok = false;

        if let Some(margin) = self.get_int_value(name) {
            margins.left = margin;
            margins.top = margin;
            margins.right = margin;
            margins.bottom = margin;
            ok = true;
        }
        if let Some(v) = self.get_int_value(&format!("{name}.left")) {
            margins.left = v;
            ok = true;
        }
        if let Some(v) = self.get_int_value(&format!("{name}.top")) {
            margins.top = v;
            ok = true;
        }
        if let Some(v) = self.get_int_value(&format!("{name}.right")) {
            margins.right = v;
            ok = true;
        }
        if let Some(v) = self.get_int_value(&format!("{name}.bottom")) {
            margins.bottom = v;
            ok = true;
        }

        if ok {
            Some(margins)
        } else {
            None
        }
    }

    /// `CStyleScaling` を初期化する純粋ロジック(`InitStyleScaling(HMONITOR)`、Style.cpp:346)。
    ///
    /// `forced_dpi > 0` ならそれを採用、さもなくば `system_dpi`。`monitor_dpi` が `Some(非0)` の
    /// ときはモニタ DPI で上書きする(原実装の `MonitorDPI != 0` 条件に対応)。
    /// モニタ取得自体(`MonitorFromWindow` 等)は呼び出し側の責務。
    pub fn init_style_scaling(
        &self,
        scaling: &mut StyleScaling,
        system_dpi: i32,
        monitor_dpi: Option<i32>,
    ) {
        let dpi = if self.forced_dpi > 0 {
            self.forced_dpi
        } else {
            let mut dpi = system_dpi;
            if let Some(monitor) = monitor_dpi {
                if monitor != 0 {
                    dpi = monitor;
                }
            }
            dpi
        };

        scaling.set_dpi(dpi);
        scaling.set_system_dpi(system_dpi);
        scaling.set_scale_font(self.scale_font);
    }

    /// LOGFONT の高さからポイントサイズを求めて `Font::size` に設定する
    /// (`AssignFontSizeFromLogFont`、Style.cpp:423)。`system_dpi` は注入する。
    pub fn assign_font_size_from_log_font(font: &mut Font, system_dpi: i32) -> bool {
        font.size.value = mul_div(font.log_font.lfHeight.abs(), 72, system_dpi);
        font.size.unit = if font.size.value != 0 {
            UnitType::Point
        } else {
            UnitType::Undefined
        };
        true
    }

    /// `"123px"` 形式を [`IntValue`] へ解釈する(`ParseValue`、Style.cpp:438)。
    pub fn parse_value(text: &str) -> Option<IntValue> {
        let first = text.chars().next()?;
        if !first.is_ascii_digit() && first != '-' && first != '+' {
            return None;
        }
        let (value, rest) = parse_leading_int(text);
        let unit = Self::parse_unit(rest);
        if unit == UnitType::Undefined {
            return None;
        }
        Some(IntValue::new(value, unit))
    }

    /// 単位文字列を [`UnitType`] へ解釈する(`ParseUnit`、Style.cpp:458)。
    ///
    /// 空 → `LogicalPixel`、`px` → `PhysicalPixel`、`pt` → `Point`、`dp` → `Dip`、他 → `Undefined`。
    /// 比較は大小無視(原実装 `lstrcmpi` を ASCII 近似)。
    pub fn parse_unit(unit: &str) -> UnitType {
        if unit.is_empty() {
            return UnitType::LogicalPixel;
        }
        if unit.eq_ignore_ascii_case("px") {
            return UnitType::PhysicalPixel;
        }
        if unit.eq_ignore_ascii_case("pt") {
            return UnitType::Point;
        }
        if unit.eq_ignore_ascii_case("dp") {
            return UnitType::Dip;
        }
        UnitType::Undefined
    }

    /// 強制 DPI を設定する(`Load` が `[Settings] DPI` から設定する値に相当)。
    pub fn set_forced_dpi(&mut self, dpi: i32) {
        self.forced_dpi = dpi;
    }

    /// 強制 DPI(`GetForcedDPI`、Style.cpp:399)。未設定なら 0。
    pub fn forced_dpi(&self) -> i32 {
        self.forced_dpi
    }

    /// DPI 変更を処理するか(`IsHandleDPIChanged`、Style.cpp:405)。
    pub fn is_handle_dpi_changed(&self) -> bool {
        self.handle_dpi_changed
    }

    /// `Load` が設定する `HandleDPIChanged` 値に相当。
    pub fn set_handle_dpi_changed(&mut self, value: bool) {
        self.handle_dpi_changed = value;
    }

    /// フォントをスケールするか(`Load` の `ScaleFont`)。`init_style_scaling` で参照する。
    pub fn set_scale_font(&mut self, value: bool) {
        self.scale_font = value;
    }

    /// ダークメニューを使うか(`IsUseDarkMenu`、Style.cpp:411)。
    pub fn is_use_dark_menu(&self) -> bool {
        self.use_dark_menu
    }

    /// `Load` が設定する `UseDarkMenu` 値に相当。
    pub fn set_use_dark_menu(&mut self, value: bool) {
        self.use_dark_menu = value;
    }

    /// ダークダイアログか(`IsDarkDialog`、Style.cpp:417)。
    pub fn is_dark_dialog(&self) -> bool {
        self.dark_dialog
    }

    /// `Load` が設定する `DarkDialog` 値に相当。
    pub fn set_dark_dialog(&mut self, value: bool) {
        self.dark_dialog = value;
    }
}

/// 先頭の整数(任意の符号 + 数字列)を解釈し、`(値, 残り文字列)` を返す。
///
/// 原実装の `_tcstol`(基数 10)相当。符号のみで数字が無い場合は変換なし扱いとして
/// `(0, 入力全体)` を返す(`_tcstol` の endptr=先頭と同じ挙動)。
fn parse_leading_int(s: &str) -> (i32, &str) {
    let bytes = s.as_bytes();
    let mut i = 0;
    let negative = match bytes.first() {
        Some(b'+') => {
            i = 1;
            false
        }
        Some(b'-') => {
            i = 1;
            true
        }
        _ => false,
    };

    let digit_start = i;
    let mut value: i64 = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        value = value * 10 + i64::from(bytes[i] - b'0');
        // 桁あふれは i32 範囲でクランプ(_tcstol も LONG_MAX/MIN へ飽和)。
        if value > i64::from(i32::MAX) + 1 {
            value = i64::from(i32::MAX) + 1;
        }
        i += 1;
    }

    if i == digit_start {
        // 数字が 1 文字も無い → 変換なし。先頭から再解釈させる。
        return (0, s);
    }

    let signed = if negative { -value } else { value };
    let clamped = signed.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
    (clamped, &s[i..])
}

// ---------------------------------------------------------------------------
// DPI スケーリング(Style.h:129 / Style.cpp:475-665)
// ---------------------------------------------------------------------------

/// DPI に基づく単位換算。原実装 `Style::CStyleScaling`(Style.h:129)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StyleScaling {
    dpi: i32,
    system_dpi: i32,
    scale_font: bool,
}

impl Default for StyleScaling {
    /// 原実装のメンバ既定値(`m_DPI = 96` / `m_SystemDPI = 96` / `m_fScaleFont = true`、Style.h:150)。
    fn default() -> Self {
        Self {
            dpi: 96,
            system_dpi: 96,
            scale_font: true,
        }
    }
}

impl StyleScaling {
    /// DPI を設定する(`SetDPI`、Style.cpp:475)。1 未満は失敗。
    pub fn set_dpi(&mut self, dpi: i32) -> bool {
        if dpi < 1 {
            return false;
        }
        self.dpi = dpi;
        true
    }

    /// DPI を取得する(`GetDPI`、Style.cpp:486)。
    pub fn get_dpi(&self) -> i32 {
        self.dpi
    }

    /// システム DPI を設定する(`SetSystemDPI`、Style.cpp:492)。1 未満は失敗。
    pub fn set_system_dpi(&mut self, dpi: i32) -> bool {
        if dpi < 1 {
            return false;
        }
        self.system_dpi = dpi;
        true
    }

    /// システム DPI を取得する(`GetSystemDPI`、Style.cpp:503)。
    pub fn get_system_dpi(&self) -> i32 {
        self.system_dpi
    }

    /// フォントをスケールするか設定する(`SetScaleFont`、Style.cpp:509)。
    pub fn set_scale_font(&mut self, scale: bool) {
        self.scale_font = scale;
    }

    /// [`IntValue`] を物理ピクセルへ換算する(`ToPixels(IntValue*)`、Style.cpp:515)。
    ///
    /// 既に `PhysicalPixel` なら何もせず `true`、`Undefined` は `false`。換算後は単位を
    /// `PhysicalPixel` にする。
    pub fn to_pixels(&self, value: &mut IntValue) -> bool {
        match value.unit {
            UnitType::LogicalPixel => {
                value.value = self.logical_pixels_to_physical_pixels(value.value)
            }
            UnitType::PhysicalPixel => return true,
            UnitType::Point => value.value = self.points_to_pixels(value.value),
            UnitType::Dip => value.value = self.dip_to_pixels(value.value),
            UnitType::Undefined => return false,
        }
        value.unit = UnitType::PhysicalPixel;
        true
    }

    /// [`Size`] を物理ピクセルへ換算する(`ToPixels(Size*)`、Style.cpp:546)。
    pub fn to_pixels_size(&self, value: &mut Size) -> bool {
        self.to_pixels(&mut value.width);
        self.to_pixels(&mut value.height);
        true
    }

    /// [`Margins`] を物理ピクセルへ換算する(`ToPixels(Margins*)`、Style.cpp:558)。
    pub fn to_pixels_margins(&self, value: &mut Margins) -> bool {
        self.to_pixels(&mut value.left);
        self.to_pixels(&mut value.top);
        self.to_pixels(&mut value.right);
        self.to_pixels(&mut value.bottom);
        true
    }

    /// 値+単位を物理ピクセルへ換算する(`ToPixels(int, UnitType)`、Style.cpp:572)。
    ///
    /// `Undefined` は 0(原実装 switch の default)。
    pub fn to_pixels_int(&self, value: i32, unit: UnitType) -> i32 {
        match unit {
            UnitType::LogicalPixel => self.logical_pixels_to_physical_pixels(value),
            UnitType::PhysicalPixel => value,
            UnitType::Point => self.points_to_pixels(value),
            UnitType::Dip => self.dip_to_pixels(value),
            UnitType::Undefined => 0,
        }
    }

    /// 論理ピクセル → 物理ピクセル(`LogicalPixelsToPhysicalPixels`、Style.cpp:592)。
    pub fn logical_pixels_to_physical_pixels(&self, pixels: i32) -> i32 {
        mul_div(pixels, self.dpi, 96)
    }

    /// ポイント → ピクセル(`PointsToPixels`、Style.cpp:598)。1pt = 1/72in。
    pub fn points_to_pixels(&self, points: i32) -> i32 {
        mul_div(points, self.dpi, 72)
    }

    /// DIP → ピクセル(`DipToPixels`、Style.cpp:605)。1dp = 1/160in。
    pub fn dip_to_pixels(&self, dip: i32) -> i32 {
        mul_div(dip, self.dpi, 160)
    }

    /// 単位を変換する(`ConvertUnit`、Style.cpp:612)。
    ///
    /// いったん物理ピクセルへ換算してから目的単位へ。`Undefined` の目的単位は元値のまま
    /// (原実装 switch の default)。
    pub fn convert_unit(&self, value: i32, src_unit: UnitType, dst_unit: UnitType) -> i32 {
        match dst_unit {
            UnitType::LogicalPixel => mul_div(self.to_pixels_int(value, src_unit), 96, self.dpi),
            UnitType::PhysicalPixel => self.to_pixels_int(value, src_unit),
            UnitType::Point => mul_div(self.to_pixels_int(value, src_unit), 72, self.dpi),
            UnitType::Dip => mul_div(self.to_pixels_int(value, src_unit), 160, self.dpi),
            UnitType::Undefined => value,
        }
    }

    /// フォントの論理高さをサイズ・単位から実体化する(`RealizeFontSize`、Style.cpp:632)。
    ///
    /// フォントスケール無効、または換算サイズが 0 なら `false`。高さの符号は保つ。
    pub fn realize_font_size(&self, font: &mut Font) -> bool {
        if !self.scale_font {
            return false;
        }
        let size = self.to_pixels_int(font.size.value, font.size.unit);
        if size == 0 {
            return false;
        }
        font.log_font.lfHeight = if font.log_font.lfHeight >= 0 { size } else { -size };
        true
    }
}

// ---------------------------------------------------------------------------
// 自由関数(Style.cpp:670-689)
// ---------------------------------------------------------------------------

/// 矩形を余白だけ外側へ広げる(`Add(RECT*, Margins)`、Style.cpp:670)。
pub fn add(rect: &mut RECT, margins: &Margins) {
    rect.left -= margins.left.value;
    rect.top -= margins.top.value;
    rect.right += margins.right.value;
    rect.bottom += margins.bottom.value;
}

/// 矩形を余白だけ内側へ縮める(`Subtract(RECT*, Margins)`、Style.cpp:679)。
///
/// 縮めすぎて反転する場合は幅/高さ 0 にクランプする。
pub fn subtract(rect: &mut RECT, margins: &Margins) {
    rect.left += margins.left.value;
    rect.top += margins.top.value;
    rect.right -= margins.right.value;
    rect.bottom -= margins.bottom.value;
    if rect.right < rect.left {
        rect.right = rect.left;
    }
    if rect.bottom < rect.top {
        rect.bottom = rect.top;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rc(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left,
            top,
            right,
            bottom,
        }
    }

    fn make_font(lf_height: i32, size: IntValue) -> Font {
        let log_font = LOGFONTW {
            lfHeight: lf_height,
            ..Default::default()
        };
        Font { log_font, size }
    }

    // ----- 値型の既定値 -----

    #[test]
    fn intvalue_defaults() {
        assert_eq!(IntValue::default(), IntValue::new(0, UnitType::Undefined));
        assert_eq!(IntValue::with_logical(5), IntValue::new(5, UnitType::LogicalPixel));
        assert!(IntValue::new(7, UnitType::Point).eq_value(7));
        assert!(!IntValue::new(7, UnitType::Point).eq_value(8));
    }

    #[test]
    fn size_margins_helpers() {
        let s = Size::new(4, 8, UnitType::LogicalPixel);
        assert_eq!(s.width, IntValue::new(4, UnitType::LogicalPixel));
        assert_eq!(s.height, IntValue::new(8, UnitType::LogicalPixel));

        let m = Margins::new(1, 2, 3, 4, UnitType::LogicalPixel);
        assert_eq!(m.horz(), 4);
        assert_eq!(m.vert(), 6);
        assert_eq!(Margins::uniform(5, UnitType::Point).left, IntValue::new(5, UnitType::Point));
    }

    // ----- StyleScaling DPI 演算 -----

    #[test]
    fn scaling_identity_at_96() {
        let s = StyleScaling::default();
        assert_eq!(s.logical_pixels_to_physical_pixels(100), 100);
        assert_eq!(s.dip_to_pixels(160), 96); // 160dp @96 = 96px
    }

    #[test]
    fn scaling_at_192() {
        let mut s = StyleScaling::default();
        assert!(s.set_dpi(192));
        assert_eq!(s.logical_pixels_to_physical_pixels(10), 20); // MulDiv(10,192,96)
        assert_eq!(s.points_to_pixels(10), 27); // MulDiv(10,192,72)=26.67->27
        assert_eq!(s.dip_to_pixels(10), 12); // MulDiv(10,192,160)
    }

    #[test]
    fn scaling_set_dpi_rejects_below_one() {
        let mut s = StyleScaling::default();
        assert!(!s.set_dpi(0));
        assert!(!s.set_system_dpi(-1));
        assert_eq!(s.get_dpi(), 96);
        assert_eq!(s.get_system_dpi(), 96);
    }

    #[test]
    fn to_pixels_intvalue_units() {
        let mut s = StyleScaling::default();
        s.set_dpi(192);

        let mut logical = IntValue::with_logical(10);
        assert!(s.to_pixels(&mut logical));
        assert_eq!(logical, IntValue::new(20, UnitType::PhysicalPixel));

        // 既に物理ピクセル -> 変化なし・単位そのまま
        let mut physical = IntValue::new(15, UnitType::PhysicalPixel);
        assert!(s.to_pixels(&mut physical));
        assert_eq!(physical, IntValue::new(15, UnitType::PhysicalPixel));

        let mut point = IntValue::new(10, UnitType::Point);
        assert!(s.to_pixels(&mut point));
        assert_eq!(point, IntValue::new(27, UnitType::PhysicalPixel));

        let mut dip = IntValue::new(10, UnitType::Dip);
        assert!(s.to_pixels(&mut dip));
        assert_eq!(dip, IntValue::new(12, UnitType::PhysicalPixel));

        // Undefined -> false・値不変
        let mut undef = IntValue::default();
        assert!(!s.to_pixels(&mut undef));
        assert_eq!(undef, IntValue::default());
    }

    #[test]
    fn to_pixels_size_and_margins() {
        let mut s = StyleScaling::default();
        s.set_dpi(192);

        let mut size = Size::new(10, 20, UnitType::LogicalPixel);
        s.to_pixels_size(&mut size);
        assert_eq!(size.width, IntValue::new(20, UnitType::PhysicalPixel));
        assert_eq!(size.height, IntValue::new(40, UnitType::PhysicalPixel));

        let mut m = Margins::new(1, 2, 3, 4, UnitType::LogicalPixel);
        s.to_pixels_margins(&mut m);
        assert_eq!(m.left.value, 2);
        assert_eq!(m.top.value, 4);
        assert_eq!(m.right.value, 6);
        assert_eq!(m.bottom.value, 8);
        assert_eq!(m.left.unit, UnitType::PhysicalPixel);
    }

    #[test]
    fn to_pixels_int_undefined_is_zero() {
        let s = StyleScaling::default();
        assert_eq!(s.to_pixels_int(100, UnitType::Undefined), 0);
        assert_eq!(s.to_pixels_int(100, UnitType::PhysicalPixel), 100);
    }

    #[test]
    fn convert_unit_roundtrip() {
        let mut s = StyleScaling::default();
        s.set_dpi(192);
        // 論理10px -> 物理 = 20
        assert_eq!(s.convert_unit(10, UnitType::LogicalPixel, UnitType::PhysicalPixel), 20);
        // 物理20px -> 論理 = MulDiv(20,96,192) = 10
        assert_eq!(s.convert_unit(20, UnitType::PhysicalPixel, UnitType::LogicalPixel), 10);
        // Undefined 目的単位 -> 元値
        assert_eq!(s.convert_unit(42, UnitType::PhysicalPixel, UnitType::Undefined), 42);
    }

    #[test]
    fn realize_font_size_behavior() {
        let mut s = StyleScaling::default();
        s.set_dpi(96);

        // 96 DPI: 12pt -> MulDiv(12,96,72) = 16px。高さは負(下向き)を維持。
        let mut font = make_font(-1, IntValue::new(12, UnitType::Point));
        assert!(s.realize_font_size(&mut font));
        assert_eq!(font.log_font.lfHeight, -16);

        // 正の高さは正のまま
        let mut font2 = make_font(1, IntValue::new(12, UnitType::Point));
        assert!(s.realize_font_size(&mut font2));
        assert_eq!(font2.log_font.lfHeight, 16);

        // scale_font 無効 -> false
        s.set_scale_font(false);
        let mut font3 = font;
        assert!(!s.realize_font_size(&mut font3));

        // サイズ 0(Undefined) -> false
        s.set_scale_font(true);
        let mut font4 = make_font(0, IntValue::default());
        assert!(!s.realize_font_size(&mut font4));
    }

    // ----- StyleManager マップ -----

    #[test]
    fn manager_int_set_get_type_mismatch() {
        let mut m = StyleManager::new();
        assert!(m.set_int_value("a", IntValue::new(5, UnitType::Point)));
        assert_eq!(m.get_int_value("a"), Some(IntValue::new(5, UnitType::Point)));
        // bool として取ろうとすると型不一致で None
        assert_eq!(m.get_bool("a"), None);
        // 空名は失敗
        assert!(!m.set_int_value("", IntValue::default()));
        assert_eq!(m.get_int_value("missing"), None);
    }

    #[test]
    fn manager_bool_string() {
        let mut m = StyleManager::new();
        assert!(m.set_bool("flag", true));
        assert_eq!(m.get_bool("flag"), Some(true));
        assert!(m.set_string("name", "hello"));
        assert_eq!(m.get_string("name").as_deref(), Some("hello"));
        // 型を上書き(bool -> int)
        assert!(m.set_int_value("flag", IntValue::with_logical(3)));
        assert_eq!(m.get_bool("flag"), None);
        assert_eq!(m.get_int_value("flag"), Some(IntValue::with_logical(3)));
    }

    #[test]
    fn manager_size_margins() {
        let mut m = StyleManager::new();
        m.set_size("box", &Size::new(10, 20, UnitType::LogicalPixel));
        let s = m.get_size("box").unwrap();
        assert_eq!(s.width, IntValue::new(10, UnitType::LogicalPixel));
        assert_eq!(s.height, IntValue::new(20, UnitType::LogicalPixel));
        assert_eq!(m.get_size("none"), None);

        m.set_margins("pad", &Margins::new(1, 2, 3, 4, UnitType::LogicalPixel));
        let mg = m.get_margins("pad").unwrap();
        assert_eq!(mg.left.value, 1);
        assert_eq!(mg.bottom.value, 4);
    }

    #[test]
    fn manager_margins_single_value_applies_all() {
        let mut m = StyleManager::new();
        // 単一値(name 自体)を全辺へ適用、その後 .top のみ上書き
        m.set_int_value("pad", IntValue::with_logical(5));
        m.set_int_value("pad.top", IntValue::with_logical(9));
        let mg = m.get_margins("pad").unwrap();
        assert_eq!(mg.left.value, 5);
        assert_eq!(mg.right.value, 5);
        assert_eq!(mg.bottom.value, 5);
        assert_eq!(mg.top.value, 9);
    }

    #[test]
    fn manager_info_roundtrip() {
        let mut m = StyleManager::new();
        let info = StyleInfo {
            name: "x".to_string(),
            value: StyleValue::Int(7),
            unit: UnitType::Dip,
        };
        assert!(m.set_info(info.clone()));
        assert_eq!(m.get_info("x"), Some(info));
        assert_eq!(StyleValue::Bool(true).value_type(), ValueType::Bool);
    }

    // ----- ParseValue / ParseUnit -----

    #[test]
    fn parse_unit_cases() {
        assert_eq!(StyleManager::parse_unit(""), UnitType::LogicalPixel);
        assert_eq!(StyleManager::parse_unit("px"), UnitType::PhysicalPixel);
        assert_eq!(StyleManager::parse_unit("PX"), UnitType::PhysicalPixel);
        assert_eq!(StyleManager::parse_unit("pt"), UnitType::Point);
        assert_eq!(StyleManager::parse_unit("dp"), UnitType::Dip);
        assert_eq!(StyleManager::parse_unit("em"), UnitType::Undefined);
    }

    #[test]
    fn parse_value_cases() {
        assert_eq!(StyleManager::parse_value("10"), Some(IntValue::new(10, UnitType::LogicalPixel)));
        assert_eq!(StyleManager::parse_value("10px"), Some(IntValue::new(10, UnitType::PhysicalPixel)));
        assert_eq!(StyleManager::parse_value("-5pt"), Some(IntValue::new(-5, UnitType::Point)));
        assert_eq!(StyleManager::parse_value("+3"), Some(IntValue::new(3, UnitType::LogicalPixel)));
        assert_eq!(StyleManager::parse_value("8dp"), Some(IntValue::new(8, UnitType::Dip)));
        // 不正な単位 -> None
        assert_eq!(StyleManager::parse_value("10xy"), None);
        // 数字始まりでない -> None
        assert_eq!(StyleManager::parse_value("abc"), None);
        // 空 -> None
        assert_eq!(StyleManager::parse_value(""), None);
        // 符号のみ -> 変換なし・残り "-" は単位として Undefined -> None
        assert_eq!(StyleManager::parse_value("-"), None);
    }

    // ----- init_style_scaling -----

    #[test]
    fn init_scaling_uses_system_dpi() {
        let m = StyleManager::new();
        let mut s = StyleScaling::default();
        m.init_style_scaling(&mut s, 144, None);
        assert_eq!(s.get_dpi(), 144);
        assert_eq!(s.get_system_dpi(), 144);
    }

    #[test]
    fn init_scaling_monitor_override() {
        let m = StyleManager::new();
        let mut s = StyleScaling::default();
        m.init_style_scaling(&mut s, 96, Some(192));
        assert_eq!(s.get_dpi(), 192);
        assert_eq!(s.get_system_dpi(), 96);

        // monitor_dpi が 0 のときはシステム DPI を使う
        let mut s2 = StyleScaling::default();
        m.init_style_scaling(&mut s2, 120, Some(0));
        assert_eq!(s2.get_dpi(), 120);
    }

    #[test]
    fn init_scaling_forced_dpi_wins() {
        let mut m = StyleManager::new();
        m.set_forced_dpi(168);
        let mut s = StyleScaling::default();
        m.init_style_scaling(&mut s, 96, Some(192));
        assert_eq!(s.get_dpi(), 168); // forced が最優先
        assert_eq!(s.get_system_dpi(), 96);
    }

    #[test]
    fn assign_font_size_from_log_font_works() {
        // system DPI 96: MulDiv(16,72,96)=12pt
        let mut font = make_font(-16, IntValue::default());
        assert!(StyleManager::assign_font_size_from_log_font(&mut font, 96));
        assert_eq!(font.size, IntValue::new(12, UnitType::Point));

        // 高さ 0 -> サイズ0・単位 Undefined
        let mut font2 = make_font(0, IntValue::default());
        StyleManager::assign_font_size_from_log_font(&mut font2, 96);
        assert_eq!(font2.size.value, 0);
        assert_eq!(font2.size.unit, UnitType::Undefined);
    }

    // ----- 設定フラグ -----

    #[test]
    fn manager_flags_defaults_and_setters() {
        let mut m = StyleManager::new();
        assert_eq!(m.forced_dpi(), 0);
        assert!(m.is_handle_dpi_changed());
        assert!(m.is_use_dark_menu());
        assert!(m.is_dark_dialog());

        m.set_handle_dpi_changed(false);
        m.set_use_dark_menu(false);
        m.set_dark_dialog(false);
        assert!(!m.is_handle_dpi_changed());
        assert!(!m.is_use_dark_menu());
        assert!(!m.is_dark_dialog());
    }

    // ----- Add / Subtract -----

    #[test]
    fn add_subtract_margins() {
        let margins = Margins::new(2, 3, 4, 5, UnitType::PhysicalPixel);

        let mut r = rc(10, 10, 100, 100);
        add(&mut r, &margins);
        assert_eq!(r, rc(8, 7, 104, 105));

        let mut r2 = rc(10, 10, 100, 100);
        subtract(&mut r2, &margins);
        assert_eq!(r2, rc(12, 13, 96, 95));
    }

    #[test]
    fn subtract_clamps_when_inverted() {
        // 幅 4 の矩形に左右 10 ずつ縮める -> 反転を 0 にクランプ
        let margins = Margins::new(10, 10, 10, 10, UnitType::PhysicalPixel);
        let mut r = rc(0, 0, 4, 4);
        subtract(&mut r, &margins);
        assert_eq!(r.left, 10);
        assert_eq!(r.right, 10); // left へクランプ
        assert_eq!(r.top, 10);
        assert_eq!(r.bottom, 10); // top へクランプ
    }
}
