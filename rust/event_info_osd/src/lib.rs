//! TVTest の番組情報 OSD(`src/EventInfoOSD.cpp` / `src/EventInfoOSD.h`)の
//! 描画・レイアウト部分の Rust 移植。
//!
//! 移植対象:
//! - [`ColorScheme`](`CEventInfoOSD::ColorScheme`、EventInfoOSD.h:37-44)。
//! - [`EventInfoOsdStyle`](`CEventInfoOSD::EventInfoOSDStyle`、EventInfoOSD.h:75-86)。
//!   [`EventInfoOsdStyle::set_style`](EventInfoOSD.cpp:187)/
//!   [`EventInfoOsdStyle::normalize_style`](EventInfoOSD.cpp:205)/
//!   [`EventInfoOsdStyle::adjust_position`](EventInfoOSD.cpp:149)。
//! - [`build_title_text`][]: タイトル行の合成(EventInfoOSD.cpp:246-257)。
//! - [`build_body_text`]: 本文テキストの合成(EventInfoOSD.cpp:312-354。
//!   「内容」「概要」を含む拡張テキスト項目の優先表示)。
//! - [`draw`][]: GDI+(tvtest_graphics)による描画(EventInfoOSD.cpp:237-375)。
//! - [`create_bitmap`]: 32bpp 画像へ描画して HBITMAP を返す
//!   (EventInfoOSD.cpp:215-234)。
//!
//! ## 対象外(後続で統合予定)
//!
//! - CPseudoOSD への配線: `Show` / `Hide` / `IsVisible` / `IsCreated` / `Update` /
//!   `SetPosition` / `GetPosition` / `OnParentMove`(EventInfoOSD.cpp:48-146、181-184)。
//!   並行実装中の pseudo_osd クレートに依存するため本クレートでは移植しない。
//!   `SetEventInfo` / `SetColorScheme` / `SetFont` 等の単純な setter 群も、
//!   状態保持クラスごと統合時に移植する(本クレートは描画に必要な値を引数で受ける)。
//!
//! ## 原実装との差異
//!
//! - ロゴ画像: 原実装は `GetAppClass().LogoManager.GetAssociatedLogoImage`
//!   (EventInfoOSD.cpp:281-282)で取得するが、AppClass 依存のため
//!   ロゴ画像を `Option<&Image>` 引数で注入する設計に変えている。
//! - 開始時刻: 原実装は `EpgUtil::FormatEventTime` 内で EpgTimeToDisplayTime
//!   (AppClass の時刻モード依存)による変換を行うが、本移植では
//!   「表示用時刻に変換済み」の時刻を `Option<&SystemTime>` で受ける
//!   (tvtest_epg_util の doc 参照)。`None` は開始時刻が無効
//!   (`!StartTime.IsValid()`、EpgUtil.cpp:63-66)の場合に相当し、時刻部を出さない。
//!
//! [`draw`] / [`create_bitmap`] は GDI+ を使うため、事前に
//! `tvtest_graphics::GraphicsCore` の初期化(GdiplusStartup)が必要。

#![cfg(windows)]

use libisdb_event_info::EventInfo;
use tvtest_dpi_util::mul_div;
use tvtest_epg_util::{format_event_time, FormatEventTimeFlag};
use tvtest_graphics::{Brush, Canvas, Color, Font, Image, TextFlag};
use tvtest_style::{IntValue, Margins, StyleManager, StyleScaling, UnitType};
use tvtest_theme::ThemeColor;
use tvtest_util::SystemTime;

use windows::Win32::Foundation::{RECT, SIZE};
use windows::Win32::Graphics::Gdi::{HBITMAP, LOGFONTW};

// ---------------------------------------------------------------------------
// ColorScheme(EventInfoOSD.h:37-44)
// ---------------------------------------------------------------------------

/// OSD の配色。原実装 `CEventInfoOSD::ColorScheme`(EventInfoOSD.h:37-44)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorScheme {
    /// 背景色。既定 `{0, 0, 0, 160}`(EventInfoOSD.h:39)。
    pub back: ThemeColor,
    /// 本文色。既定 `{255, 255, 255, 255}`(EventInfoOSD.h:40)。
    pub text: ThemeColor,
    /// 本文の縁取り色。既定 `{0, 0, 0, 255}`(EventInfoOSD.h:41)。
    pub text_outline: ThemeColor,
    /// タイトル色。既定 `{192, 224, 255, 255}`(EventInfoOSD.h:42)。
    pub title: ThemeColor,
    /// タイトルの縁取り色。既定 `{0, 0, 0, 255}`(EventInfoOSD.h:43)。
    pub title_outline: ThemeColor,
}

impl Default for ColorScheme {
    fn default() -> Self {
        Self {
            back: ThemeColor::new(0, 0, 0, 160),
            text: ThemeColor::new(255, 255, 255, 255),
            text_outline: ThemeColor::new(0, 0, 0, 255),
            title: ThemeColor::new(192, 224, 255, 255),
            title_outline: ThemeColor::new(0, 0, 0, 255),
        }
    }
}

// ---------------------------------------------------------------------------
// EventInfoOsdStyle(EventInfoOSD.h:75-86)
// ---------------------------------------------------------------------------

/// OSD のスタイル値。原実装 `CEventInfoOSD::EventInfoOSDStyle`
/// (EventInfoOSD.h:75-86)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventInfoOsdStyle {
    /// 画面に対する余白(各辺のパーセント値)。既定
    /// `{50, 50, 0, 0, UnitType::Undefined}`(EventInfoOSD.h:77)。
    /// パーセント値のため [`Self::normalize_style`] ではピクセル換算しない。
    pub margin: Margins,
    /// 内側の余白。既定 `{4}`(全辺 4、単位 LogicalPixel。EventInfoOSD.h:78)。
    pub padding: Margins,
    /// フォントサイズ = 幅 / TextSizeRatio。既定 `22`(EventInfoOSD.h:79)。
    pub text_size_ratio: IntValue,
    /// フォントサイズの下限。既定 `12`(EventInfoOSD.h:80)。
    pub text_size_min: IntValue,
    /// フォントサイズの上限。既定 `24`(EventInfoOSD.h:81)。
    pub text_size_max: IntValue,
    /// 縁取り幅(フォントサイズに対するパーセント値)。既定 `20`
    /// (EventInfoOSD.h:82)。
    pub text_outline: IntValue,
    /// ヒンティングを使うか。既定 `true`(EventInfoOSD.h:83 fUseHinting)。
    pub use_hinting: bool,
    /// パス描画を使うか。既定 `true`(EventInfoOSD.h:84 fUsePath)。
    pub use_path: bool,
    /// ロゴを表示するか。既定 `true`(EventInfoOSD.h:85 fShowLogo)。
    pub show_logo: bool,
}

impl Default for EventInfoOsdStyle {
    fn default() -> Self {
        Self {
            margin: Margins::new(50, 50, 0, 0, UnitType::Undefined),
            padding: Margins::uniform(4, UnitType::LogicalPixel),
            text_size_ratio: IntValue::with_logical(22),
            text_size_min: IntValue::with_logical(12),
            text_size_max: IntValue::with_logical(24),
            text_outline: IntValue::with_logical(20),
            use_hinting: true,
            use_path: true,
            show_logo: true,
        }
    }
}

impl EventInfoOsdStyle {
    /// スタイルマネージャから値を読み込む。原実装 `CEventInfoOSD::SetStyle`
    /// (EventInfoOSD.cpp:187-202)。
    ///
    /// まず既定値へ戻し(`m_Style = {}`、EventInfoOSD.cpp:189)、存在する
    /// 項目だけ上書きする。`event-osd.text-size-ratio` は正の値のみ採用
    /// (EventInfoOSD.cpp:193-195)。
    pub fn set_style(&mut self, style_manager: &StyleManager) {
        *self = Self::default();

        if let Some(v) = style_manager.get_margins("event-osd.margin") {
            self.margin = v;
        }
        if let Some(v) = style_manager.get_margins("event-osd.padding") {
            self.padding = v;
        }
        if let Some(v) = style_manager.get_int_value("event-osd.text-size-ratio") {
            if v.value > 0 {
                self.text_size_ratio = v;
            }
        }
        if let Some(v) = style_manager.get_int_value("event-osd.text-size-min") {
            self.text_size_min = v;
        }
        if let Some(v) = style_manager.get_int_value("event-osd.text-size-max") {
            self.text_size_max = v;
        }
        if let Some(v) = style_manager.get_int_value("event-osd.text-outline") {
            self.text_outline = v;
        }
        if let Some(v) = style_manager.get_bool("event-osd.use-hinting") {
            self.use_hinting = v;
        }
        if let Some(v) = style_manager.get_bool("event-osd.use-path") {
            self.use_path = v;
        }
        if let Some(v) = style_manager.get_bool("event-osd.logo.show") {
            self.show_logo = v;
        }
    }

    /// スタイル値を物理ピクセルへ正規化する。原実装
    /// `CEventInfoOSD::NormalizeStyle`(EventInfoOSD.cpp:205-212)。
    ///
    /// Padding / TextSizeMin / TextSizeMax のみ換算する。Margin は
    /// パーセント値のため換算しない。原実装の第1引数 `pStyleManager` は
    /// 未使用のため省いている。
    pub fn normalize_style(&mut self, scaling: &StyleScaling) {
        scaling.to_pixels_margins(&mut self.padding);
        scaling.to_pixels(&mut self.text_size_min);
        scaling.to_pixels(&mut self.text_size_max);
    }

    /// 表示位置を Margin(パーセント)ぶん内側へ調整する。原実装
    /// `CEventInfoOSD::AdjustPosition`(EventInfoOSD.cpp:149-160)。
    ///
    /// 各辺を `MulDiv(幅または高さ, Margin の各辺, 100)` だけ内側へ寄せ、
    /// 結果が正の面積を持つとき `true` を返す。
    pub fn adjust_position(&self, rect: &mut RECT) -> bool {
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;

        rect.left += mul_div(width, self.margin.left.value, 100);
        rect.top += mul_div(height, self.margin.top.value, 100);
        rect.right -= mul_div(width, self.margin.right.value, 100);
        rect.bottom -= mul_div(height, self.margin.bottom.value, 100);

        rect.left < rect.right && rect.top < rect.bottom
    }
}

// ---------------------------------------------------------------------------
// テキスト合成
// ---------------------------------------------------------------------------

/// `StringUtility::Trim(s, " \t\r\n")` 相当の文字集合。
const TRIM_CHARS: &[char] = &[' ', '\t', '\r', '\n'];

/// タイトル行のテキストを合成する。原実装 `CEventInfoOSD::Draw` 内の
/// タイトル合成部(EventInfoOSD.cpp:246-257)。
///
/// `display_start_time` は表示用時刻に変換済みの開始時刻。`Some` なら
/// `FormatEventTime(..., UndecidedText)` の結果 + 空白を前置し、`None`
/// (開始時刻が無効、EpgUtil.cpp:63-66 相当)なら時刻部なしで
/// イベント名のみを返す。
pub fn build_title_text(event: &EventInfo, display_start_time: Option<&SystemTime>) -> String {
    let mut text = String::new();

    if let Some(start_time) = display_start_time {
        let time_text = format_event_time(
            start_time,
            event.duration,
            FormatEventTimeFlag::UNDECIDED_TEXT,
        );
        if !time_text.is_empty() {
            text.push_str(&time_text);
            text.push(' ');
        }
    }
    text.push_str(&event.event_name);

    text
}

/// 本文テキストを合成する。原実装 `CEventInfoOSD::Draw` 内の本文合成部
/// (EventInfoOSD.cpp:312-354)。
///
/// 1. EventText を前後 Trim(`" \t\r\n"`)。
/// 2. 拡張テキスト 1 周目: Description に「内容」または「概要」を含む項目を
///    優先して追記し、リストから除去(EventInfoOSD.cpp:320-333)。
///    Text は前後 Trim し、空でなければ `Text + "\r\n"` を追記。
/// 3. 2 周目(残り): Text を末尾 TrimEnd し、空でなければ Description
///    (非空なら `Description + "\r\n"`)に続けて `Text + "\r\n"` を追記
///    (EventInfoOSD.cpp:335-345)。
/// 4. 拡張テキスト全体を末尾 TrimEnd(`"\r\n"`)し、EventText と両方あれば
///    `"\r\n\r\n"` で結合(EventInfoOSD.cpp:347-353)。
pub fn build_body_text(event: &EventInfo) -> String {
    let mut text = event.event_text.trim_matches(TRIM_CHARS).to_string();

    if !event.extended_text.is_empty() {
        let mut extended = String::new();

        // 1周目: 「番組内容」などを優先して表示する(EventInfoOSD.cpp:320-333)。
        // 原実装はリストのコピーから該当項目を erase するが、ここでは残りを
        // 集めて 2 周目に回す。
        let mut remaining = Vec::new();
        for item in &event.extended_text {
            if item.description.contains("内容") || item.description.contains("概要") {
                let item_text = item.text.trim_matches(TRIM_CHARS);
                if !item_text.is_empty() {
                    extended.push_str(item_text);
                    extended.push_str("\r\n");
                }
            } else {
                remaining.push(item);
            }
        }

        // 2周目: 残りの項目(EventInfoOSD.cpp:335-345)。
        for item in remaining {
            let item_text = item.text.trim_end_matches(TRIM_CHARS);
            if !item_text.is_empty() {
                if !item.description.is_empty() {
                    extended.push_str(&item.description);
                    extended.push_str("\r\n");
                }
                extended.push_str(item_text);
                extended.push_str("\r\n");
            }
        }

        // TrimEnd(ExtendedText, "\r\n")(EventInfoOSD.cpp:347)。
        let extended = extended.trim_end_matches(['\r', '\n']);

        if !extended.is_empty() {
            if !text.is_empty() {
                text.push_str("\r\n\r\n");
            }
            text.push_str(extended);
        }
    }

    text
}

// ---------------------------------------------------------------------------
// 描画
// ---------------------------------------------------------------------------

/// [`ThemeColor`] を graphics の [`Color`] へ変換する。原実装
/// `GraphicsColorFromThemeColor`(EventInfoOSD.cpp:37-40)。
pub fn graphics_color_from_theme_color(color: ThemeColor) -> Color {
    Color::new(color.red, color.green, color.blue, color.alpha)
}

/// NUL 終端ワイド文字列の長さ。
fn wide_len(s: &[u16]) -> usize {
    s.iter().position(|&c| c == 0).unwrap_or(s.len())
}

/// 2つの NUL 終端ワイド文字列を大小区別ありで比較(`lstrcmp` 相当)。
fn wide_eq(a: &[u16], b: &[u16]) -> bool {
    let la = wide_len(a);
    let lb = wide_len(b);
    la == lb && a[..la] == b[..lb]
}

/// 2つの `LOGFONTW` を比較する。原実装 `CompareLogFont`(Util.cpp:589)。
///
/// `lfFaceName` 直前までの数値フィールド(28 バイト相当)と、`lfFaceName`
/// (`lstrcmp` 相当、大小区別あり)を比較する。
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

/// EventInfo の開始時刻を表示用の [`SystemTime`] として取り出す。
///
/// 原実装 `EpgUtil::FormatEventTime(const LibISDB::EventInfo &, ...)` の
/// 有効性チェック + `DateTime::ToSYSTEMTIME()`(EpgUtil.cpp:63-70、:78)に
/// 相当する。EpgTimeToDisplayTime(AppClass 依存)は移植していないため、
/// 変換なし(NoConvert 相当)の値を返す。無効な開始時刻は `None`。
pub fn display_start_time(event: &EventInfo) -> Option<SystemTime> {
    if !event.start_time.is_valid() {
        return None;
    }
    let t = &event.start_time;
    Some(SystemTime {
        year: t.year as u16,
        month: t.month as u16,
        day_of_week: t.day_of_week as u16,
        day: t.day as u16,
        hour: t.hour as u16,
        minute: t.minute as u16,
        second: t.second as u16,
        milliseconds: t.millisecond as u16,
    })
}

/// 番組情報 OSD を描画する。原実装 `CEventInfoOSD::Draw`
/// (EventInfoOSD.cpp:237-375)。
///
/// - `rect`: 描画先の矩形(通常は画像全体 `{0, 0, Width, Height}`)。
/// - `display_start_time`: 表示用時刻に変換済みの開始時刻
///   ([`display_start_time`] 参照)。`None` ならタイトルに時刻部を出さない。
/// - `logo`: 局ロゴ画像。原実装の `LogoManager.GetAssociatedLogoImage`
///   (EventInfoOSD.cpp:281-282)の結果に相当し、`None` は取得失敗と同じ扱い。
///   `style.show_logo` が false のときは参照しない。
/// - `font` / `title_font`: 本文/タイトルの LOGFONT(`m_Font` / `m_TitleFont`)。
///
/// 呼び出し前に `GraphicsCore` の初期化が必要。
#[allow(clippy::too_many_arguments)]
pub fn draw(
    canvas: &mut Canvas,
    rect: &RECT,
    event: &EventInfo,
    display_start_time: Option<&SystemTime>,
    logo: Option<&Image>,
    color_scheme: &ColorScheme,
    style: &EventInfoOsdStyle,
    font: &LOGFONTW,
    title_font: &LOGFONTW,
) {
    // 背景クリア(EventInfoOSD.cpp:239)。
    canvas.clear(
        color_scheme.back.red,
        color_scheme.back.green,
        color_scheme.back.blue,
        color_scheme.back.alpha,
    );

    // Padding を減算し、空になったら終わり(EventInfoOSD.cpp:241-244)。
    let mut content_rect = *rect;
    tvtest_style::subtract(&mut content_rect, &style.padding);
    if content_rect.left >= content_rect.right || content_rect.top >= content_rect.bottom {
        return;
    }

    // タイトルテキスト(EventInfoOSD.cpp:246-257)。
    let title_text: Vec<u16> = build_title_text(event, display_start_time)
        .encode_utf16()
        .collect();

    // フォントサイズ = clamp(幅 / TextSizeRatio, Min, Max)(EventInfoOSD.cpp:259-261)。
    let font_size = ((content_rect.right - content_rect.left) / style.text_size_ratio.value)
        .clamp(style.text_size_min.value, style.text_size_max.value);

    // タイトルフォント(EventInfoOSD.cpp:262-265)。
    let mut lf = *title_font;
    lf.lfHeight = -font_size;
    lf.lfWidth = 0;
    let mut gp_font = Font::from_logfont(&lf);

    // 縁取り幅(EventInfoOSD.cpp:267)。
    let outline_width = (style.text_outline.value * font_size) as f32 / 100.0;

    // テキストフラグ(EventInfoOSD.cpp:269-274)。
    let mut text_flags = TextFlag::DRAW_ANTIALIAS;
    if style.use_hinting {
        text_flags |= TextFlag::DRAW_HINTING;
    }
    if style.use_path {
        text_flags |= TextFlag::DRAW_PATH;
    }
    let draw_text_flags =
        text_flags | TextFlag::FORMAT_END_ELLIPSIS | TextFlag::FORMAT_CLIP_LAST_LINE;

    let mut title_rect = content_rect;

    // ロゴ(EventInfoOSD.cpp:278-291)。
    if style.show_logo {
        if let Some(logo) = logo {
            let logo_height = font_size;
            let logo_width = mul_div(logo_height, 16, 9);
            canvas.draw_image_rect(
                title_rect.left,
                title_rect.top
                    + ((canvas.get_line_spacing(&gp_font) as i32 - logo_height) / 2).max(0),
                logo_width,
                logo_height,
                logo,
                0,
                0,
                logo.get_width(),
                logo.get_height(),
                1.0,
            );
            title_rect.left += logo_width + font_size / 4;
        }
    }

    // タイトルの計測(EventInfoOSD.cpp:293-297)。
    let mut title_size = SIZE {
        cx: title_rect.right - title_rect.left,
        cy: title_rect.bottom - title_rect.top,
    };
    if outline_width > 0.0 {
        canvas.get_outline_text_size(
            &title_text,
            &gp_font,
            outline_width,
            text_flags,
            &mut title_size,
        );
    } else {
        canvas.get_text_size(&title_text, &gp_font, text_flags, &mut title_size);
    }

    // タイトルの描画(EventInfoOSD.cpp:299-310)。
    // ぴったりのサイズで指定すると最後の行が表示されないことがあるため、
    // TitleRect.bottom の切り詰めは描画後に行う(原実装コメント参照)。
    let mut brush = Brush::from_color(graphics_color_from_theme_color(color_scheme.title));
    if outline_width > 0.0 {
        canvas.draw_outline_text(
            &title_text,
            &gp_font,
            &title_rect,
            &brush,
            graphics_color_from_theme_color(color_scheme.title_outline),
            outline_width,
            draw_text_flags,
        );
    } else {
        canvas.draw_text(&title_text, &gp_font, &title_rect, &brush, draw_text_flags);
    }
    title_rect.bottom = title_rect.top + title_size.cy;

    // 本文(EventInfoOSD.cpp:312-374)。
    if content_rect.bottom > title_rect.bottom {
        let body_text = build_body_text(event);

        if !body_text.is_empty() {
            // 本文フォント(EventInfoOSD.cpp:357-361)。原実装同様、調整後の
            // LOGFONT が m_Font と一致する場合はフォントを作り直さない
            // (この場合タイトルフォントのまま描画される)。
            let mut lf = *font;
            lf.lfHeight = -font_size;
            lf.lfWidth = 0;
            if !compare_log_font(&lf, font) {
                gp_font.create(&lf);
            }

            brush.create_solid_brush_color(graphics_color_from_theme_color(color_scheme.text));
            let text_rect = RECT {
                left: content_rect.left,
                top: title_rect.bottom,
                right: content_rect.right,
                bottom: content_rect.bottom,
            };
            let body_text: Vec<u16> = body_text.encode_utf16().collect();
            if outline_width > 0.0 {
                canvas.draw_outline_text(
                    &body_text,
                    &gp_font,
                    &text_rect,
                    &brush,
                    graphics_color_from_theme_color(color_scheme.text_outline),
                    outline_width,
                    draw_text_flags,
                );
            } else {
                canvas.draw_text(&body_text, &gp_font, &text_rect, &brush, draw_text_flags);
            }
        }
    }
}

/// 32bpp 画像へ [`draw`] で描画し、HBITMAP を生成して返す。原実装
/// `CEventInfoOSD::CreateBitmap`(EventInfoOSD.cpp:215-234)。
///
/// 原実装は生成した HBITMAP を `m_Bitmap`(DrawUtil::CBitmap)へ Attach
/// するが、本移植では所有権ごと返す。呼び出し側が `DeleteObject` で
/// 解放すること。画像生成・HBITMAP 化に失敗したら `None`
/// (EventInfoOSD.cpp:218-219、:228-229)。
///
/// 呼び出し前に `GraphicsCore` の初期化が必要。
#[allow(clippy::too_many_arguments)]
pub fn create_bitmap(
    width: i32,
    height: i32,
    event: &EventInfo,
    display_start_time: Option<&SystemTime>,
    logo: Option<&Image>,
    color_scheme: &ColorScheme,
    style: &EventInfoOsdStyle,
    font: &LOGFONTW,
    title_font: &LOGFONTW,
) -> Option<HBITMAP> {
    let mut image = Image::new();
    if !image.create(width, height, 32) {
        return None;
    }

    {
        let mut canvas = Canvas::from_image(&mut image);
        draw(
            &mut canvas,
            &RECT {
                left: 0,
                top: 0,
                right: width,
                bottom: height,
            },
            event,
            display_start_time,
            logo,
            color_scheme,
            style,
            font,
            title_font,
        );
    }

    let hbm = image.create_hbitmap();
    if hbm.is_invalid() {
        return None;
    }
    Some(hbm)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::OnceLock;

    use libisdb_datetime::DateTime;
    use libisdb_event_info::ExtendedTextInfo;
    use tvtest_graphics::GraphicsCore;
    use windows::Win32::Graphics::Gdi::{DeleteObject, GetObjectW, BITMAP, DIBSECTION};

    /// テスト全体で一度だけ GDI+ を初期化する(Shutdown はしない)。
    /// graphics クレートのテストの流儀(OnceLock + mem::forget)を踏襲。
    fn ensure_gdiplus() {
        static INIT: OnceLock<bool> = OnceLock::new();
        let ok = *INIT.get_or_init(|| {
            let mut core = GraphicsCore::new();
            let ok = core.initialize();
            // Drop の GdiplusShutdown を呼ばせず、プロセス終了まで維持する
            std::mem::forget(core);
            ok
        });
        assert!(ok, "GdiplusStartup failed");
    }

    fn logfont(face: &str, height: i32) -> LOGFONTW {
        let mut lf = LOGFONTW {
            lfHeight: height,
            lfWeight: 400, // FW_NORMAL
            ..Default::default()
        };
        for (i, c) in face.encode_utf16().enumerate() {
            lf.lfFaceName[i] = c;
        }
        lf
    }

    /// 描画済み Image のピクセルを、HBITMAP(DIB セクション)経由で
    /// トップダウン(行0 = 最上行)の Vec<u32> として読み出す。
    fn read_pixels(image: &Image) -> Vec<u32> {
        let hbm = image.create_hbitmap();
        assert!(!hbm.is_invalid());

        let mut ds = DIBSECTION::default();
        let size = std::mem::size_of::<DIBSECTION>() as i32;
        let got = unsafe { GetObjectW(hbm.into(), size, Some(std::ptr::from_mut(&mut ds).cast())) };
        assert!(got > 0);
        assert_eq!(ds.dsBm.bmBitsPixel, 32);
        assert!(!ds.dsBm.bmBits.is_null());

        let width = ds.dsBm.bmWidth as usize;
        let height = ds.dsBm.bmHeight as usize;
        let bits = ds.dsBm.bmBits.cast::<u32>();
        let mut pixels = vec![0u32; width * height];
        // DIB セクションはボトムアップなので行を反転してトップダウンにする。
        for y in 0..height {
            for x in 0..width {
                pixels[y * width + x] = unsafe { bits.add((height - 1 - y) * width + x).read() };
            }
        }

        let _ = unsafe { DeleteObject(hbm.into()) };
        pixels
    }

    /// 2024-04-01(月)20:00 開始のテスト用表示時刻。
    fn start_2000() -> SystemTime {
        SystemTime {
            year: 2024,
            month: 4,
            day: 1,
            day_of_week: 1,
            hour: 20,
            minute: 0,
            second: 0,
            milliseconds: 0,
        }
    }

    fn ext(description: &str, text: &str) -> ExtendedTextInfo {
        ExtendedTextInfo {
            description: description.to_string(),
            text: text.to_string(),
        }
    }

    // -----------------------------------------------------------------------
    // ColorScheme / EventInfoOsdStyle
    // -----------------------------------------------------------------------

    #[test]
    fn color_scheme_defaults() {
        // EventInfoOSD.h:39-43 の既定値。
        let colors = ColorScheme::default();
        assert_eq!(colors.back, ThemeColor::new(0, 0, 0, 160));
        assert_eq!(colors.text, ThemeColor::new(255, 255, 255, 255));
        assert_eq!(colors.text_outline, ThemeColor::new(0, 0, 0, 255));
        assert_eq!(colors.title, ThemeColor::new(192, 224, 255, 255));
        assert_eq!(colors.title_outline, ThemeColor::new(0, 0, 0, 255));
    }

    #[test]
    fn style_defaults() {
        // EventInfoOSD.h:77-85 の既定値。
        let style = EventInfoOsdStyle::default();
        assert_eq!(style.margin, Margins::new(50, 50, 0, 0, UnitType::Undefined));
        assert_eq!(style.padding, Margins::uniform(4, UnitType::LogicalPixel));
        assert_eq!(style.text_size_ratio, IntValue::with_logical(22));
        assert_eq!(style.text_size_min, IntValue::with_logical(12));
        assert_eq!(style.text_size_max, IntValue::with_logical(24));
        assert_eq!(style.text_outline, IntValue::with_logical(20));
        assert!(style.use_hinting);
        assert!(style.use_path);
        assert!(style.show_logo);
    }

    #[test]
    fn set_style_reads_manager_values() {
        let mut manager = StyleManager::new();
        manager.set_margins(
            "event-osd.margin",
            &Margins::new(10, 20, 30, 40, UnitType::Undefined),
        );
        manager.set_margins(
            "event-osd.padding",
            &Margins::uniform(8, UnitType::LogicalPixel),
        );
        manager.set_int_value("event-osd.text-size-ratio", IntValue::with_logical(30));
        manager.set_int_value("event-osd.text-size-min", IntValue::with_logical(10));
        manager.set_int_value("event-osd.text-size-max", IntValue::with_logical(40));
        manager.set_int_value("event-osd.text-outline", IntValue::with_logical(15));
        manager.set_bool("event-osd.use-hinting", false);
        manager.set_bool("event-osd.use-path", false);
        manager.set_bool("event-osd.logo.show", false);

        let mut style = EventInfoOsdStyle::default();
        style.set_style(&manager);
        assert_eq!(style.margin, Margins::new(10, 20, 30, 40, UnitType::Undefined));
        assert_eq!(style.padding, Margins::uniform(8, UnitType::LogicalPixel));
        assert_eq!(style.text_size_ratio.value, 30);
        assert_eq!(style.text_size_min.value, 10);
        assert_eq!(style.text_size_max.value, 40);
        assert_eq!(style.text_outline.value, 15);
        assert!(!style.use_hinting);
        assert!(!style.use_path);
        assert!(!style.show_logo);
    }

    #[test]
    fn set_style_resets_to_default_and_ignores_nonpositive_ratio() {
        // 空のマネージャなら既定値へ戻る(m_Style = {}、EventInfoOSD.cpp:189)。
        let mut style = EventInfoOsdStyle {
            text_size_ratio: IntValue::with_logical(99),
            use_path: false,
            ..Default::default()
        };
        style.set_style(&StyleManager::new());
        assert_eq!(style, EventInfoOsdStyle::default());

        // text-size-ratio は正の値のみ採用(EventInfoOSD.cpp:193-195)。
        let mut manager = StyleManager::new();
        manager.set_int_value("event-osd.text-size-ratio", IntValue::with_logical(0));
        style.set_style(&manager);
        assert_eq!(style.text_size_ratio.value, 22);

        manager.set_int_value("event-osd.text-size-ratio", IntValue::with_logical(-5));
        style.set_style(&manager);
        assert_eq!(style.text_size_ratio.value, 22);
    }

    #[test]
    fn normalize_style_converts_only_pixel_values() {
        // Padding / TextSizeMin / TextSizeMax のみ換算(EventInfoOSD.cpp:205-212)。
        let mut style = EventInfoOsdStyle::default();
        let mut scaling = StyleScaling::default();
        assert!(scaling.set_dpi(192)); // 96dpi 基準の 2 倍

        style.normalize_style(&scaling);
        assert_eq!(style.padding, Margins::uniform(8, UnitType::PhysicalPixel));
        assert_eq!(style.text_size_min, IntValue::new(24, UnitType::PhysicalPixel));
        assert_eq!(style.text_size_max, IntValue::new(48, UnitType::PhysicalPixel));
        // Margin(パーセント値)は変換されない。
        assert_eq!(style.margin, Margins::new(50, 50, 0, 0, UnitType::Undefined));
    }

    #[test]
    fn adjust_position_default_margin() {
        // 既定 Margin {50, 50, 0, 0}: 左半分・上半分を除いた右下 1/4。
        let style = EventInfoOsdStyle::default();
        let mut rect = RECT { left: 0, top: 0, right: 1000, bottom: 500 };
        assert!(style.adjust_position(&mut rect));
        assert_eq!(
            (rect.left, rect.top, rect.right, rect.bottom),
            (500, 250, 1000, 500)
        );
    }

    #[test]
    fn adjust_position_empty_result() {
        // 全辺 50% では面積が 0 になり false(EventInfoOSD.cpp:159)。
        let style = EventInfoOsdStyle {
            margin: Margins::uniform(50, UnitType::Undefined),
            ..Default::default()
        };
        let mut rect = RECT { left: 0, top: 0, right: 100, bottom: 100 };
        assert!(!style.adjust_position(&mut rect));
        assert_eq!((rect.left, rect.top, rect.right, rect.bottom), (50, 50, 50, 50));
    }

    #[test]
    fn adjust_position_muldiv_rounding() {
        // MulDiv は四捨五入(10 * 33 / 100 = 3.3 → 3、10 * 35 / 100 = 3.5 → 4)。
        let style = EventInfoOsdStyle {
            margin: Margins::new(33, 35, 0, 0, UnitType::Undefined),
            ..Default::default()
        };
        let mut rect = RECT { left: 0, top: 0, right: 10, bottom: 10 };
        assert!(style.adjust_position(&mut rect));
        assert_eq!((rect.left, rect.top), (3, 4));
    }

    // -----------------------------------------------------------------------
    // build_title_text / display_start_time
    // -----------------------------------------------------------------------

    #[test]
    fn title_text_with_time() {
        let event = EventInfo {
            event_name: "ニュース".to_string(),
            duration: 1800,
            ..Default::default()
        };
        assert_eq!(
            build_title_text(&event, Some(&start_2000())),
            "20:00\u{FF5E}20:30 ニュース"
        );
    }

    #[test]
    fn title_text_without_time() {
        // 開始時刻が無効(None)なら時刻部なし(EpgUtil.cpp:63-66 相当)。
        let event = EventInfo {
            event_name: "ニュース".to_string(),
            ..Default::default()
        };
        assert_eq!(build_title_text(&event, None), "ニュース");
    }

    #[test]
    fn title_text_undecided_end() {
        // Duration 0 → UndecidedText フラグにより「(終了未定)」
        // (EventInfoOSD.cpp:250-252)。
        let event = EventInfo {
            event_name: "特番".to_string(),
            duration: 0,
            ..Default::default()
        };
        assert_eq!(
            build_title_text(&event, Some(&start_2000())),
            "20:00\u{FF5E}(終了未定) 特番"
        );
    }

    #[test]
    fn title_text_empty_name_keeps_trailing_space() {
        // 原実装は時刻の後に必ず ' ' を足すため、イベント名が空でも空白が残る。
        let event = EventInfo {
            duration: 1800,
            ..Default::default()
        };
        assert_eq!(
            build_title_text(&event, Some(&start_2000())),
            "20:00\u{FF5E}20:30 "
        );
    }

    #[test]
    fn display_start_time_conversion() {
        // 無効な開始時刻(Default)は None。
        assert!(display_start_time(&EventInfo::default()).is_none());

        let event = EventInfo {
            start_time: DateTime {
                year: 2024,
                month: 4,
                day: 1,
                day_of_week: 1,
                hour: 20,
                minute: 30,
                second: 15,
                millisecond: 500,
            },
            ..Default::default()
        };
        let st = display_start_time(&event).unwrap();
        assert_eq!(
            (st.year, st.month, st.day, st.day_of_week),
            (2024, 4, 1, 1)
        );
        assert_eq!(
            (st.hour, st.minute, st.second, st.milliseconds),
            (20, 30, 15, 500)
        );
    }

    // -----------------------------------------------------------------------
    // build_body_text
    // -----------------------------------------------------------------------

    #[test]
    fn body_text_all_empty() {
        assert_eq!(build_body_text(&EventInfo::default()), "");
    }

    #[test]
    fn body_text_event_text_trimmed() {
        // EventText は前後 Trim(EventInfoOSD.cpp:313-314)。
        let event = EventInfo {
            event_text: " \t\r\n本文です \r\n".to_string(),
            ..Default::default()
        };
        assert_eq!(build_body_text(&event), "本文です");
    }

    #[test]
    fn body_text_content_priority() {
        // 「内容」を含む項目はリスト順に関わらず先頭へ(EventInfoOSD.cpp:320-333)。
        let event = EventInfo {
            extended_text: vec![
                ext("出演者", "俳優A"),
                ext("番組内容", " あらすじ本文 \r\n"),
            ],
            ..Default::default()
        };
        assert_eq!(build_body_text(&event), "あらすじ本文\r\n出演者\r\n俳優A");
    }

    #[test]
    fn body_text_summary_priority() {
        // 「概要」も優先される(EventInfoOSD.cpp:322-323)。
        let event = EventInfo {
            extended_text: vec![ext("その他", "ZZZ"), ext("概要", "要約")],
            ..Default::default()
        };
        assert_eq!(build_body_text(&event), "要約\r\nその他\r\nZZZ");
    }

    #[test]
    fn body_text_empty_description() {
        // Description が空なら Text のみ(EventInfoOSD.cpp:338-341)。
        let event = EventInfo {
            extended_text: vec![ext("", "本文のみ")],
            ..Default::default()
        };
        assert_eq!(build_body_text(&event), "本文のみ");
    }

    #[test]
    fn body_text_second_pass_trims_end_only() {
        // 2周目は TrimEnd のみで先頭の空白は保持(EventInfoOSD.cpp:336)。
        let event = EventInfo {
            extended_text: vec![ext("", "  字下げ本文 \r\n")],
            ..Default::default()
        };
        assert_eq!(build_body_text(&event), "  字下げ本文");
    }

    #[test]
    fn body_text_skips_empty_items() {
        // Trim 後に空になった項目は出力されない(Description も出ない)。
        let event = EventInfo {
            extended_text: vec![
                ext("番組内容", " \r\n"),
                ext("出演者", "\r\n \t"),
                ext("音楽", "BGM"),
            ],
            ..Default::default()
        };
        assert_eq!(build_body_text(&event), "音楽\r\nBGM");
    }

    #[test]
    fn body_text_joins_event_text_and_extended() {
        // 両方あれば "\r\n\r\n" で結合(EventInfoOSD.cpp:349-353)。
        let event = EventInfo {
            event_text: "本文".to_string(),
            extended_text: vec![ext("番組内容", "詳細")],
            ..Default::default()
        };
        assert_eq!(build_body_text(&event), "本文\r\n\r\n詳細");
    }

    #[test]
    fn body_text_extended_only() {
        // EventText が空なら拡張テキストのみ(結合の "\r\n\r\n" は入らない)。
        let event = EventInfo {
            event_text: " \r\n".to_string(),
            extended_text: vec![ext("番組内容", "詳細")],
            ..Default::default()
        };
        assert_eq!(build_body_text(&event), "詳細");
    }

    #[test]
    fn body_text_multiple_items_order() {
        let event = EventInfo {
            event_text: "イベントテキスト".to_string(),
            extended_text: vec![
                ext("出演者", "俳優A"),
                ext("番組内容", "あらすじ"),
                ext("", "補足"),
            ],
            ..Default::default()
        };
        assert_eq!(
            build_body_text(&event),
            "イベントテキスト\r\n\r\nあらすじ\r\n出演者\r\n俳優A\r\n補足"
        );
    }

    // -----------------------------------------------------------------------
    // graphics_color_from_theme_color / compare_log_font
    // -----------------------------------------------------------------------

    #[test]
    fn theme_color_conversion() {
        // EventInfoOSD.cpp:37-40。
        let color = graphics_color_from_theme_color(ThemeColor::new(1, 2, 3, 4));
        assert_eq!(color, Color::new(1, 2, 3, 4));
    }

    #[test]
    fn compare_log_font_matches() {
        let a = logfont("Arial", -16);
        let b = logfont("Arial", -16);
        assert!(compare_log_font(&a, &b));

        // 数値フィールドの差異。
        let c = logfont("Arial", -18);
        assert!(!compare_log_font(&a, &c));

        // フェイス名の差異(大小区別あり)。
        let d = logfont("arial", -16);
        assert!(!compare_log_font(&a, &d));
    }

    // -----------------------------------------------------------------------
    // draw / create_bitmap(GDI+ 実描画)
    // -----------------------------------------------------------------------

    fn test_event() -> EventInfo {
        EventInfo {
            event_name: "テスト番組".to_string(),
            event_text: "本文テキスト".to_string(),
            duration: 1800,
            ..Default::default()
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_to_image(
        width: i32,
        height: i32,
        event: &EventInfo,
        display_start_time: Option<&SystemTime>,
        logo: Option<&Image>,
        style: &EventInfoOsdStyle,
    ) -> Image {
        let mut image = Image::new();
        assert!(image.create(width, height, 32));
        {
            let mut canvas = Canvas::from_image(&mut image);
            draw(
                &mut canvas,
                &RECT { left: 0, top: 0, right: width, bottom: height },
                event,
                display_start_time,
                logo,
                &ColorScheme::default(),
                style,
                &logfont("Arial", -16),
                &logfont("Arial", -16),
            );
        }
        image
    }

    #[test]
    fn draw_renders_title_and_body() {
        ensure_gdiplus();

        let style = EventInfoOsdStyle::default();
        let start = start_2000();

        // テキストなしの描画(背景のみ)と比較して、テキスト描画で
        // ピクセルが変化していることを確認する。
        let empty = draw_to_image(96, 64, &EventInfo::default(), None, None, &style);
        let drawn = draw_to_image(96, 64, &test_event(), Some(&start), None, &style);
        let empty_pixels = read_pixels(&empty);
        let drawn_pixels = read_pixels(&drawn);

        // 背景のみの画像は全ピクセル同一(Clear の背景色)。
        assert!(empty_pixels.iter().all(|&p| p == empty_pixels[0]));
        // 四隅(Padding の外)は背景のまま。
        assert_eq!(drawn_pixels[0], empty_pixels[0]);
        // テキストが描画されてピクセルが変化している。
        assert!(
            drawn_pixels
                .iter()
                .zip(empty_pixels.iter())
                .any(|(&a, &b)| a != b),
            "draw did not change any pixel"
        );
    }

    #[test]
    fn draw_without_outline_or_path() {
        ensure_gdiplus();

        // TextOutline = 0 + fUsePath/fUseHinting = false → DrawText 経路
        // (EventInfoOSD.cpp:302-309 の else 側)。
        let style = EventInfoOsdStyle {
            text_outline: IntValue::with_logical(0),
            use_path: false,
            use_hinting: false,
            ..Default::default()
        };
        let empty = draw_to_image(96, 64, &EventInfo::default(), None, None, &style);
        let drawn = draw_to_image(96, 64, &test_event(), Some(&start_2000()), None, &style);
        let empty_pixels = read_pixels(&empty);
        let drawn_pixels = read_pixels(&drawn);
        assert!(
            drawn_pixels
                .iter()
                .zip(empty_pixels.iter())
                .any(|(&a, &b)| a != b),
            "draw (no outline) did not change any pixel"
        );
    }

    #[test]
    fn draw_empty_content_rect_clears_only() {
        ensure_gdiplus();

        // Padding が大きすぎて ContentRect が空 → Clear のみで終わる
        // (EventInfoOSD.cpp:241-244)。全ピクセルが背景色で一様。
        let style = EventInfoOsdStyle {
            padding: Margins::uniform(100, UnitType::LogicalPixel),
            ..Default::default()
        };
        let image = draw_to_image(32, 32, &test_event(), Some(&start_2000()), None, &style);
        let pixels = read_pixels(&image);
        assert!(pixels.iter().all(|&p| p == pixels[0]));
    }

    #[test]
    fn draw_renders_logo() {
        ensure_gdiplus();

        // 不透明の赤いロゴを注入すると、タイトル行左端のロゴ領域が赤くなる
        // (EventInfoOSD.cpp:278-291)。
        let mut logo = Image::new();
        assert!(logo.create(8, 8, 32));
        {
            let mut canvas = Canvas::from_image(&mut logo);
            assert!(canvas.clear(255, 0, 0, 255));
        }

        let style = EventInfoOsdStyle::default();
        let image = draw_to_image(96, 64, &test_event(), Some(&start_2000()), Some(&logo), &style);
        let pixels = read_pixels(&image);

        // ContentRect.left = 4、FontSize = clamp(88/22, 12, 24) = 12、
        // LogoWidth = MulDiv(12, 16, 9) = 21。y は行送りにより 4〜5 起点で
        // 高さ 12。領域中央 (10, 10) は確実にロゴ内。
        let p = pixels[10 * 96 + 10];
        let red = (p >> 16) & 0xFF;
        let green = (p >> 8) & 0xFF;
        let blue = p & 0xFF;
        assert!(red > 0xC0, "logo pixel not red: {p:08X}");
        assert!(green < 0x40 && blue < 0x40, "logo pixel not red: {p:08X}");

        // fShowLogo = false ならロゴは描かれない。
        let style_no_logo = EventInfoOsdStyle {
            show_logo: false,
            ..Default::default()
        };
        let image2 =
            draw_to_image(96, 64, &test_event(), Some(&start_2000()), Some(&logo), &style_no_logo);
        let pixels2 = read_pixels(&image2);
        let p2 = pixels2[10 * 96 + 10];
        assert!(
            ((p2 >> 16) & 0xFF) < 0xC0,
            "logo drawn despite show_logo=false: {p2:08X}"
        );
    }

    #[test]
    fn create_bitmap_returns_hbitmap() {
        ensure_gdiplus();

        let hbm = create_bitmap(
            48,
            32,
            &test_event(),
            Some(&start_2000()),
            None,
            &ColorScheme::default(),
            &EventInfoOsdStyle::default(),
            &logfont("Arial", -16),
            &logfont("Arial", -16),
        )
        .expect("create_bitmap failed");

        let mut bm = BITMAP::default();
        let size = std::mem::size_of::<BITMAP>() as i32;
        let got =
            unsafe { GetObjectW(hbm.into(), size, Some(std::ptr::from_mut(&mut bm).cast())) };
        assert_eq!(got, size);
        assert_eq!(bm.bmWidth, 48);
        assert_eq!(bm.bmHeight, 32);
        assert_eq!(bm.bmBitsPixel, 32);

        let _ = unsafe { DeleteObject(hbm.into()) };
    }

    #[test]
    fn create_bitmap_invalid_size() {
        ensure_gdiplus();

        // Image::create が失敗するサイズでは None(EventInfoOSD.cpp:218-219)。
        assert!(create_bitmap(
            0,
            32,
            &test_event(),
            None,
            None,
            &ColorScheme::default(),
            &EventInfoOsdStyle::default(),
            &logfont("Arial", -16),
            &logfont("Arial", -16),
        )
        .is_none());
    }
}
