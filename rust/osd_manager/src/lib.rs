#![cfg(windows)]
#![forbid(unsafe_code)]
//! TVTest の OSD 管理(`src/OSDManager.cpp` / `src/OSDManager.h`)の純粋ロジック部分の
//! Rust 移植。
//!
//! 移植範囲:
//! - [`OsdStyle`](OSDManager.h:94-118)の既定値・`set_style`(OSDManager.cpp:563-587)・
//!   `normalize_style`(OSDManager.cpp:590-603)。
//! - 音量 OSD: バーテキスト生成(OSDManager.cpp:249-258)、最大幅(:263-291)、
//!   フォントサイズ(:298-303 / :324-329)、合成描画座標(:307-313)、レイアウト(:338-346)。
//! - 合成テキスト: フォントサイズ(:453-455)、描画座標(:461-478)。
//! - テキスト OSD: フォントサイズ・余白(:499-502)、計測入力(:514-520)、
//!   単一行判定(:523)、位置(:533-537)。
//! - チャンネル OSD: 表示モード判定(:169-220)、ロゴ効果(:172-177)、
//!   切替中テキスト色(:213-217)、アニメーション可否(:159-163)、ロゴ位置(:182-185)。
//! - `ShowFlag` / `EventInfoOSDFlag`(OSDManager.h:56-69)、フェード時間選択(:119-123)、
//!   合成描画パス判定(:125-126)、番組情報 OSD の表示時間(:394-399)と
//!   Manual/Auto フラグ更新(:386-392)。
//!
//! 対象外(呼び出し側の責務):
//! - `CAppMain` / `CCoreEngine` / `LibISDB::ViewerFilter` への配線。合成描画
//!   (`ViewerFilter::DrawText`)や `GetSourceRect` / `IsDrawTextSupported` の取得は
//!   呼び出し側が行い、結果(矩形・bool)を本クレートの関数へ渡す。
//! - `CLogoManager::GetAssociatedLogoBitmap` によるロゴビットマップ取得
//!   (本クレートへは「ロゴが得られたか」の bool を渡す)。
//! - `CPseudoOSD` / `CEventInfoOSD` のウィンドウ生成・表示(`Create` / `Show` / `Hide` /
//!   `SetPosition` 等)と GDI フォント生成(`CreateFontIndirect`)。
//! - `DrawText` によるテキスト計測。[`volume_osd_max_width`] の各幅は呼び出し側が
//!   `DT_NOPREFIX | DT_SINGLELINE | DT_CALCRECT` で計測して渡す。
//! - `CEventHandler`(`GetOSDClientInfo` / `SetOSDHideTimer`)とのやり取り。
//!
//! 矩形は `tvtest_style` と同じく Win32 の [`RECT`] を使う(windows-rs は
//! `Win32_Foundation` のみ)。

use tvtest_dpi_util::mul_div;
use tvtest_osd_options::ChannelChangeType;
use tvtest_style::{IntValue, Margins, Size, StyleManager, StyleScaling, UnitType};
use tvtest_util::mix_color;
use windows::Win32::Foundation::RECT;

// ---------------------------------------------------------------------------
// フラグ(OSDManager.h:56-69)
// ---------------------------------------------------------------------------

bitflags::bitflags! {
    /// `COSDManager::ShowFlag`(OSDManager.h:56-61)。
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct ShowFlags: u32 {
        /// フェードアウトしない(`ShowFlag::NoFade`)。
        const NO_FADE = 0x0001;
        /// 疑似 OSD を強制する(`ShowFlag::Pseudo`)。
        const PSEUDO = 0x0002;
    }
}

bitflags::bitflags! {
    /// `COSDManager::EventInfoOSDFlag`(OSDManager.h:63-69)。
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct EventInfoOsdFlags: u32 {
        /// 手動表示(`EventInfoOSDFlag::Manual`)。
        const MANUAL = 0x0001;
        /// 自動表示(`EventInfoOSDFlag::Auto`)。
        const AUTO = 0x0002;
        /// 次番組(`EventInfoOSDFlag::Next`)。
        const NEXT = 0x0004;
    }
}

// ---------------------------------------------------------------------------
// OSDStyle(OSDManager.h:94-118)
// ---------------------------------------------------------------------------

/// OSD のスタイル値。原実装 `COSDManager::OSDStyle`(OSDManager.h:94-118)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsdStyle {
    /// OSD の余白(`Margin{8}`、OSDManager.h:96)。
    pub margin: Margins,
    /// テキストサイズ比(クライアント幅 / この値、`TextSizeRatio{28}`、OSDManager.h:97)。
    pub text_size_ratio: IntValue,
    /// テキストサイズ下限(`TextSizeMin{12}`、OSDManager.h:98)。
    pub text_size_min: IntValue,
    /// テキストサイズ上限(`TextSizeMax{100}`、OSDManager.h:99)。
    pub text_size_max: IntValue,
    /// 合成テキストサイズ比(`CompositeTextSizeRatio{24}`、OSDManager.h:100)。
    pub composite_text_size_ratio: IntValue,
    /// 合成テキストサイズ下限(`CompositeTextSizeMin{12}`、OSDManager.h:101)。
    pub composite_text_size_min: IntValue,
    /// 合成テキストサイズ上限(`CompositeTextSizeMax{100}`、OSDManager.h:102)。
    pub composite_text_size_max: IntValue,
    /// チャンネルロゴの大きさ(`LogoSize{64, 36}`、OSDManager.h:103)。
    pub logo_size: Size,
    /// ロゴの画像効果("gloss" で光沢、`LogoEffect`、OSDManager.h:104)。
    pub logo_effect: String,
    /// チャンネル OSD のアニメーション(`fChannelAnimation = true`、OSDManager.h:105)。
    pub channel_animation: bool,
    /// 音量 OSD の余白(`VolumeMargin{16}`、OSDManager.h:106)。
    pub volume_margin: Margins,
    /// 音量テキストサイズ下限(`VolumeTextSizeMin{10}`、OSDManager.h:107)。
    pub volume_text_size_min: IntValue,
    /// 音量テキストサイズ上限(`VolumeTextSizeMax{50}`、OSDManager.h:108)。
    pub volume_text_size_max: IntValue,
    /// 音量テキストの横縮尺(% 単位、`VolumeHorizontalScale{60}`、OSDManager.h:109)。
    pub volume_horizontal_scale: IntValue,
    /// 音量バーの段数(`VolumeSteps{20}`、OSDManager.h:110)。
    pub volume_steps: IntValue,
    /// 音量バーの充填文字(`VolumeTextFill{TEXT("■")}`、OSDManager.h:111)。
    pub volume_text_fill: String,
    /// 音量バーの残り文字(`VolumeTextRemain{TEXT("□")}`、OSDManager.h:112)。
    pub volume_text_remain: String,
}

impl Default for OsdStyle {
    /// メンバ初期化子による既定値(OSDManager.h:96-112)。
    ///
    /// 数値の単位は C++ の `ValueTemplate(T v)` / `Margins(int m)` / `Size(int w, int h)` の
    /// 既定(Style.h:69,87,105)に合わせてすべて `LogicalPixel`。
    fn default() -> Self {
        Self {
            margin: Margins::uniform(8, UnitType::LogicalPixel),
            text_size_ratio: IntValue::with_logical(28),
            text_size_min: IntValue::with_logical(12),
            text_size_max: IntValue::with_logical(100),
            composite_text_size_ratio: IntValue::with_logical(24),
            composite_text_size_min: IntValue::with_logical(12),
            composite_text_size_max: IntValue::with_logical(100),
            logo_size: Size::new(64, 36, UnitType::LogicalPixel),
            logo_effect: String::new(),
            channel_animation: true,
            volume_margin: Margins::uniform(16, UnitType::LogicalPixel),
            volume_text_size_min: IntValue::with_logical(10),
            volume_text_size_max: IntValue::with_logical(50),
            volume_horizontal_scale: IntValue::with_logical(60),
            volume_steps: IntValue::with_logical(20),
            volume_text_fill: "■".to_string(),
            volume_text_remain: "□".to_string(),
        }
    }
}

/// `CStyleManager::Get(name, IntValue*)` の in-place 上書き(見つかったときだけ更新)。
fn merge_int(style_manager: &StyleManager, name: &str, value: &mut IntValue) -> bool {
    if let Some(v) = style_manager.get_int_value(name) {
        *value = v;
        true
    } else {
        false
    }
}

/// `CStyleManager::Get(name, Margins*)`(Style.cpp:308)の in-place 版。
///
/// `tvtest_style::StyleManager::get_margins` は見つからない辺を既定値(0)へ
/// リセットするため、「見つかった辺だけ上書きする」原実装の挙動をここで再現する。
fn merge_margins(style_manager: &StyleManager, name: &str, value: &mut Margins) {
    if let Some(m) = style_manager.get_int_value(name) {
        value.left = m;
        value.top = m;
        value.right = m;
        value.bottom = m;
    }
    merge_int(style_manager, &format!("{name}.left"), &mut value.left);
    merge_int(style_manager, &format!("{name}.top"), &mut value.top);
    merge_int(style_manager, &format!("{name}.right"), &mut value.right);
    merge_int(style_manager, &format!("{name}.bottom"), &mut value.bottom);
}

/// `CStyleManager::Get(name, Size*)`(Style.cpp:266)の in-place 版。
///
/// 幅・高さそれぞれ見つかった成分だけ上書きする(原実装と同じ)。
fn merge_size(style_manager: &StyleManager, name: &str, value: &mut Size) {
    merge_int(style_manager, &format!("{name}.width"), &mut value.width);
    merge_int(style_manager, &format!("{name}.height"), &mut value.height);
}

impl OsdStyle {
    /// スタイル値を読み込む。原実装 `OSDStyle::SetStyle`(OSDManager.cpp:563-587)。
    ///
    /// 冒頭で自身を既定値へリセット(`*this = OSDStyle()`、:567)した後、各キーを読む。
    /// `osd.text-size-ratio` / `osd.composite-text-size-ratio` は「取得成功かつ値が正」の
    /// ときだけ上書きする(:569-574)。
    pub fn set_style(&mut self, style_manager: &StyleManager) {
        *self = OsdStyle::default(); // :567

        merge_margins(style_manager, "osd.margin", &mut self.margin); // :568
        if let Some(v) = style_manager.get_int_value("osd.text-size-ratio") {
            if v.value > 0 {
                self.text_size_ratio = v; // :569-570
            }
        }
        merge_int(style_manager, "osd.text-size-min", &mut self.text_size_min); // :571
        merge_int(style_manager, "osd.text-size-max", &mut self.text_size_max); // :572
        if let Some(v) = style_manager.get_int_value("osd.composite-text-size-ratio") {
            if v.value > 0 {
                self.composite_text_size_ratio = v; // :573-574
            }
        }
        merge_int(
            style_manager,
            "osd.composite-text-size-min",
            &mut self.composite_text_size_min,
        ); // :575
        merge_int(
            style_manager,
            "osd.composite-text-size-max",
            &mut self.composite_text_size_max,
        ); // :576
        merge_size(style_manager, "channel-osd.logo", &mut self.logo_size); // :577
        if let Some(s) = style_manager.get_string("channel-osd.logo.effect") {
            self.logo_effect = s; // :578
        }
        if let Some(b) = style_manager.get_bool("channel-osd.animation") {
            self.channel_animation = b; // :579
        }
        merge_margins(style_manager, "volume-osd.margin", &mut self.volume_margin); // :580
        merge_int(
            style_manager,
            "volume-osd.text-size-min",
            &mut self.volume_text_size_min,
        ); // :581
        merge_int(
            style_manager,
            "volume-osd.text-size-max",
            &mut self.volume_text_size_max,
        ); // :582
        merge_int(
            style_manager,
            "volume-osd.horizontal-scale",
            &mut self.volume_horizontal_scale,
        ); // :583
        merge_int(style_manager, "volume-osd.steps", &mut self.volume_steps); // :584
        if let Some(s) = style_manager.get_string("volume-osd.text.fill") {
            self.volume_text_fill = s; // :585
        }
        if let Some(s) = style_manager.get_string("volume-osd.text.remain") {
            self.volume_text_remain = s; // :586
        }
    }

    /// 物理ピクセルへ正規化する。原実装 `OSDStyle::NormalizeStyle`(OSDManager.cpp:590-603)。
    ///
    /// 原実装の引数 `pStyleManager` は未使用のため省いた。`TextSizeRatio` /
    /// `CompositeTextSizeRatio` / `VolumeHorizontalScale` / `VolumeSteps` は比率・段数で
    /// あり原実装でも換算しない。
    pub fn normalize_style(&mut self, scaling: &StyleScaling) {
        scaling.to_pixels(&mut self.text_size_min); // :594
        scaling.to_pixels(&mut self.text_size_max); // :595
        scaling.to_pixels(&mut self.composite_text_size_min); // :596
        scaling.to_pixels(&mut self.composite_text_size_max); // :597
        scaling.to_pixels_margins(&mut self.margin); // :598
        scaling.to_pixels_size(&mut self.logo_size); // :599
        scaling.to_pixels_margins(&mut self.volume_margin); // :600
        scaling.to_pixels(&mut self.volume_text_size_min); // :601
        scaling.to_pixels(&mut self.volume_text_size_max); // :602
    }

    /// クランプ済みの音量バー段数(`std::clamp(VolumeSteps.Value, 2, 100)`、
    /// OSDManager.cpp:249)。
    #[must_use]
    pub fn volume_steps_clamped(&self) -> i32 {
        self.volume_steps.value.clamp(2, 100)
    }
}

// ---------------------------------------------------------------------------
// 位置(SetPosition 引数の組)
// ---------------------------------------------------------------------------

/// `CPseudoOSD::SetPosition(Left, Top, Width, Height)` へ渡す位置とサイズ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OsdPosition {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
}

// ---------------------------------------------------------------------------
// ShowOSD(OSDManager.cpp:100-136)
// ---------------------------------------------------------------------------

/// フェード時間の選択。原実装 `ShowOSD`(OSDManager.cpp:119-123)。
///
/// [`ShowFlags::NO_FADE`] が立っていれば 0、さもなくばオプション値
/// (`COSDOptions::GetFadeTime`)。
#[must_use]
pub fn osd_fade_time(flags: ShowFlags, option_fade_time: u32) -> u32 {
    if flags.contains(ShowFlags::NO_FADE) {
        0
    } else {
        option_fade_time
    }
}

/// 合成描画(`ViewerFilter::DrawText`)を使うか。原実装 `ShowOSD`(OSDManager.cpp:125-126)。
///
/// `!GetPseudoOSD() && !fForcePseudoOSD && IsDrawTextSupported()`。
/// `ShowChannelOSD`(:191-192)・`ShowVolumeOSD`(:293-294)でも同条件。
/// 偽なら疑似 OSD(`CPseudoOSD`)で表示する(:128-133)。
#[must_use]
pub fn use_composite(pseudo_osd_option: bool, force_pseudo: bool, draw_text_supported: bool) -> bool {
    !pseudo_osd_option && !force_pseudo && draw_text_supported
}

// ---------------------------------------------------------------------------
// チャンネル OSD(OSDManager.cpp:145-223)
// ---------------------------------------------------------------------------

/// ロゴに適用する画像効果。原実装 `CPseudoOSD::ImageEffect` のうち
/// `ShowChannelOSD` が使う値(OSDManager.cpp:168-177)。
///
/// `pseudo_osd` クレートへ依存しないよう本クレートで独自に定義する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogoEffect {
    /// 効果なし(`ImageEffect::None`)。
    #[default]
    None,
    /// 光沢(`ImageEffect::Gloss`)。
    Gloss,
    /// 暗転(`ImageEffect::Dark`)。
    Dark,
}

/// ロゴの画像効果判定。原実装 `ShowChannelOSD`(OSDManager.cpp:172-177)。
///
/// 切替中(`fChanging`)なら [`LogoEffect::Dark`]、そうでなくスタイルの
/// `channel-osd.logo.effect` が "gloss"(大文字小文字無視)なら [`LogoEffect::Gloss`]、
/// それ以外は [`LogoEffect::None`]。原実装はロゴビットマップが取得できたときだけ
/// この判定を行う(取得できなければ `None` のまま)。
///
/// 大文字小文字の比較は原実装 `StringUtility::IsEqualNoCase`(`lstrcmpiW`)によるが、
/// 比較対象の "gloss" は ASCII のみなので ASCII 大小無視比較で等価。
#[must_use]
pub fn channel_logo_effect(changing: bool, logo_effect_style: &str) -> LogoEffect {
    if changing {
        LogoEffect::Dark
    } else if logo_effect_style.eq_ignore_ascii_case("gloss") {
        LogoEffect::Gloss
    } else {
        LogoEffect::None
    }
}

/// チャンネル OSD のアニメーション可否。原実装 `ShowChannelOSD`(OSDManager.cpp:159-163)。
///
/// `fAnimation = fChannelAnimation && !fChanging`(:159)の後、
/// `!fChannelAnimation || m_OSD.IsVisible()` なら偽へ落とす(:162-163)。
/// 原実装では間に `GetOSDClientInfo` が挟まるが、イベントハンドラが `fAnimation` を
/// 書き換えない前提で合成した純関数。
#[must_use]
pub fn channel_osd_animation(channel_animation: bool, changing: bool, osd_visible: bool) -> bool {
    channel_animation && !changing && !osd_visible
}

/// チャンネル切替中のテキスト色。原実装 `ShowChannelOSD`(OSDManager.cpp:213-217)。
///
/// 切替中は `MixColor(GetTextColor(), RGB(0, 0, 0), 160)` で暗くする。
#[must_use]
pub fn channel_osd_text_color(text_color: u32, changing: bool) -> u32 {
    if changing {
        mix_color(text_color, 0x0000_0000, 160)
    } else {
        text_color
    }
}

/// チャンネルロゴ(と疑似 OSD テキスト)の表示位置。原実装 `ShowChannelOSD`
/// (OSDManager.cpp:182-185 / :196-199)。
///
/// `(ClientRect.left + Margin.Left, ClientRect.top + Margin.Top,
/// LogoSize.Width, LogoSize.Height)`。
#[must_use]
pub fn channel_logo_position(rc_client: &RECT, style: &OsdStyle) -> OsdPosition {
    OsdPosition {
        left: rc_client.left + style.margin.left.value,
        top: rc_client.top + style.margin.top.value,
        width: style.logo_size.width.value,
        height: style.logo_size.height.value,
    }
}

/// チャンネル OSD の表示モード。原実装 `ShowChannelOSD` の分岐(OSDManager.cpp:169-220)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelOsdMode {
    /// ロゴのみ疑似 OSD で表示して終了(:179-188)。
    LogoOnly,
    /// 合成描画パス(:191-209)。`show_logo` ならロゴを疑似 OSD で表示(:193-201)、
    /// `show_text` ならテキストを `CompositeText` で合成(:203-209)。
    /// 両方偽なら何も表示しない(原実装と同じ)。
    Composite { show_logo: bool, show_text: bool },
    /// 疑似 OSD にテキスト(+ロゴ)を表示(:210-220)。
    PseudoOsd,
}

/// チャンネル OSD の表示モード判定。原実装 `ShowChannelOSD`(OSDManager.cpp:169-220)。
///
/// - `has_logo`: `CLogoManager::GetAssociatedLogoBitmap` でロゴが得られたか(:170-171)。
///   原実装は `ChangeType != TextOnly` のときだけ取得を試みるため、`TextOnly` では
///   本関数内で偽として扱う(:169)。
/// - `text_empty`: 表示テキストが空か(`IsStringEmpty(pszText)`、:203)。
/// - `pseudo_osd_option` / `force_pseudo` / `draw_text_supported`: [`use_composite`] の
///   入力(:191-192)。
///
/// `ChangeType == LogoOnly` かつロゴがあれば合成可否に関わらず [`ChannelOsdMode::LogoOnly`]
/// (:179-188)。
#[must_use]
pub fn decide_channel_osd_mode(
    change_type: ChannelChangeType,
    has_logo: bool,
    text_empty: bool,
    pseudo_osd_option: bool,
    force_pseudo: bool,
    draw_text_supported: bool,
) -> ChannelOsdMode {
    // :169-171 ロゴは TextOnly 以外でのみ取得される
    let has_logo = has_logo && change_type != ChannelChangeType::TextOnly;

    // :179-188
    if change_type == ChannelChangeType::LogoOnly && has_logo {
        return ChannelOsdMode::LogoOnly;
    }

    // :191-192
    if use_composite(pseudo_osd_option, force_pseudo, draw_text_supported) {
        ChannelOsdMode::Composite {
            show_logo: has_logo,                                                  // :193
            show_text: change_type != ChannelChangeType::LogoOnly && !text_empty, // :203
        }
    } else {
        ChannelOsdMode::PseudoOsd // :210-220
    }
}

// ---------------------------------------------------------------------------
// 音量 OSD(OSDManager.cpp:232-352)
// ---------------------------------------------------------------------------

/// 音量バーテキストの生成。原実装 `ShowVolumeOSD`(OSDManager.cpp:249-258)。
///
/// `steps = clamp(VolumeSteps, 2, 100)`(:249)、充填数は
/// `min(Volume, 100) / (100 / steps)`(整数除算、:254)。残りは `steps` に達するまで
/// `VolumeTextRemain` を並べ(:256-257)、末尾へ `" {Volume}"` を付ける(:258)。
///
/// 原実装と同じく、充填数が `steps` を超える場合(例: `steps = 40` で
/// `100 / (100 / 40 = 2) = 50`)はそのまま超過した数だけ並べる。
/// 負の音量では充填 0 個・残り `steps` 個になる。
#[must_use]
pub fn build_volume_text(volume: i32, style: &OsdStyle) -> String {
    let steps = style.volume_steps_clamped();
    let fill_count = std::cmp::min(volume, 100) / (100 / steps);

    let mut text = String::new();
    let mut i = 0;
    while i < fill_count {
        text.push_str(&style.volume_text_fill); // :254-255
        i += 1;
    }
    while i < steps {
        text.push_str(&style.volume_text_remain); // :256-257
        i += 1;
    }
    text.push_str(&format!(" {volume}")); // :258
    text
}

/// 音量 OSD の最大幅。原実装 `ShowVolumeOSD`(OSDManager.cpp:263-291)。
///
/// `max(充填文字幅, 残り文字幅) * steps + パーセント表示幅`(:290)。
/// 各幅は呼び出し側が `VolumeTextSizeMax` の高さのフォントで
/// `VolumeTextFill` / `VolumeTextRemain` / `" 100"` を
/// `DrawText(DT_NOPREFIX | DT_SINGLELINE | DT_CALCRECT)` で計測した `right` を渡す
/// (:269-284)。`steps` は [`OsdStyle::volume_steps_clamped`] の値。
#[must_use]
pub fn volume_osd_max_width(
    fill_width: i32,
    remain_width: i32,
    percentage_width: i32,
    steps: i32,
) -> i32 {
    std::cmp::max(fill_width, remain_width) * steps + percentage_width
}

/// 音量 OSD のフォントサイズ。原実装 `ShowVolumeOSD`(OSDManager.cpp:298-303 /
/// :324-329、両者同式)。
///
/// `clamp(MulDiv(available_width - VolumeMargin.Horz(),
/// VolumeTextSizeMax * VolumeHorizontalScale, volume_osd_max_width * 100),
/// VolumeTextSizeMin, VolumeTextSizeMax)`。
///
/// `available_width` は合成描画ではソース矩形幅(:300)、疑似 OSD では
/// クライアント矩形幅(:326)。`MulDiv` は Win32 互換(最近接丸め、分母 0 は -1)なので
/// `volume_osd_max_width == 0` のときは下限へクランプされる。
/// `VolumeTextSizeMin > VolumeTextSizeMax` の場合はパニックする
/// (C++ の `std::clamp` では未定義動作)。
#[must_use]
pub fn volume_font_size(available_width: i32, style: &OsdStyle, volume_osd_max_width: i32) -> i32 {
    mul_div(
        available_width - style.volume_margin.horz(),
        style.volume_text_size_max.value * style.volume_horizontal_scale.value,
        volume_osd_max_width * 100,
    )
    .clamp(
        style.volume_text_size_min.value,
        style.volume_text_size_max.value,
    )
}

/// 合成描画時の音量テキスト描画座標。原実装 `ShowVolumeOSD`(OSDManager.cpp:307-313)。
///
/// ソース矩形座標系で
/// `x = rcSrc.left + VolumeMargin.Left * src幅 / client幅`、
/// `y = rcSrc.bottom - FontSize - VolumeMargin.Bottom * src高 / client高`。
/// クライアント矩形の幅・高さが 0 だとパニックする(C++ ではゼロ除算)。
#[must_use]
pub fn volume_composite_text_position(
    rc_client: &RECT,
    rc_src: &RECT,
    font_size: i32,
    style: &OsdStyle,
) -> (i32, i32) {
    let x = rc_src.left
        + style.volume_margin.left.value * (rc_src.right - rc_src.left)
            / (rc_client.right - rc_client.left);
    let y = rc_src.bottom
        - font_size
        - style.volume_margin.bottom.value * (rc_src.bottom - rc_src.top)
            / (rc_client.bottom - rc_client.top);
    (x, y)
}

/// 疑似 OSD の音量テキスト計測入力(`CalcTextSize` へ渡す SIZE)。原実装
/// `ShowVolumeOSD`(OSDManager.cpp:338-341)。
///
/// `(client幅 - VolumeMargin.Left, client高 - VolumeMargin.Bottom)`。
#[must_use]
pub fn volume_osd_calc_size_input(rc_client: &RECT, style: &OsdStyle) -> (i32, i32) {
    (
        rc_client.right - rc_client.left - style.volume_margin.left.value,
        rc_client.bottom - rc_client.top - style.volume_margin.bottom.value,
    )
}

/// 疑似 OSD の音量 OSD 位置。原実装 `ShowVolumeOSD`(OSDManager.cpp:343-346)。
///
/// `measured` は `CalcTextSize` の結果 `(cx, cy)`。位置は
/// `(client.left + VolumeMargin.Left, client.bottom - cy - VolumeMargin.Bottom,
/// cx + FontSize / 4, cy)`。
#[must_use]
pub fn volume_osd_position(
    rc_client: &RECT,
    measured: (i32, i32),
    font_size: i32,
    style: &OsdStyle,
) -> OsdPosition {
    OsdPosition {
        left: rc_client.left + style.volume_margin.left.value,
        top: rc_client.bottom - measured.1 - style.volume_margin.bottom.value,
        width: measured.0 + font_size / 4,
        height: measured.1,
    }
}

// ---------------------------------------------------------------------------
// 合成テキスト(OSDManager.cpp:439-491)
// ---------------------------------------------------------------------------

/// 合成テキストのフォントサイズ。原実装 `CompositeText`(OSDManager.cpp:453-455)。
///
/// `clamp(ソース矩形幅 / CompositeTextSizeRatio, CompositeTextSizeMin,
/// CompositeTextSizeMax)`(整数除算)。`CompositeTextSizeRatio` は
/// [`OsdStyle::set_style`] が正値のみ受け付けるため 0 にはならない。
#[must_use]
pub fn composite_text_font_size(src_width: i32, style: &OsdStyle) -> i32 {
    (src_width / style.composite_text_size_ratio.value).clamp(
        style.composite_text_size_min.value,
        style.composite_text_size_max.value,
    )
}

/// 合成テキストの描画座標。原実装 `CompositeText`(OSDManager.cpp:461-478)。
///
/// - `zoom`: `CUICore::GetZoomRate` が成功したときの `Some((Rate, Factor))`。
///   `None` ならクライアント/ソース矩形の縦横比からフォールバックする(:463-470)。
///   このとき `(client幅 / src幅) < (client高 / src高)` の比較は原実装どおり
///   **整数除算のまま**行う(src 幅・高さが 0 だとパニック。C++ ではゼロ除算)。
/// - `Rate != 0` なら `src.left += (Margin.Left + LeftOffset) * Factor / Rate`、
///   `src.top += (rcClient.top + Margin.Top) * Factor / Rate`(:472-474)。
/// - `Rate == 0` なら `+16` / `+48` のフォールバック(:475-478)。
///
/// 返り値はソース矩形座標系での描画位置 `(left, top)`
/// (`ViewerFilter::DrawText` へ渡す座標、:480-482)。
#[must_use]
pub fn composite_text_position(
    rc_client: &RECT,
    rc_src: &RECT,
    margin: &Margins,
    left_offset: i32,
    zoom: Option<(i32, i32)>,
) -> (i32, i32) {
    let (rate, factor) = match zoom {
        Some(z) => z,
        None => {
            // :463-470
            let client_w = rc_client.right - rc_client.left;
            let client_h = rc_client.bottom - rc_client.top;
            let src_w = rc_src.right - rc_src.left;
            let src_h = rc_src.bottom - rc_src.top;
            if client_w / src_w < client_h / src_h {
                (client_w, src_w)
            } else {
                (client_h, src_h)
            }
        }
    };

    let mut left = rc_src.left;
    let mut top = rc_src.top;
    if rate != 0 {
        // :472-474
        left += (margin.left.value + left_offset) * factor / rate;
        top += (rc_client.top + margin.top.value) * factor / rate;
    } else {
        // :475-478
        left += 16;
        top += 48;
    }
    (left, top)
}

// ---------------------------------------------------------------------------
// テキスト OSD(OSDManager.cpp:494-540)
// ---------------------------------------------------------------------------

/// テキスト OSD のフォントサイズ。原実装 `CreateTextOSD`(OSDManager.cpp:499-501)。
///
/// `clamp(クライアント幅 / TextSizeRatio, TextSizeMin, TextSizeMax)`(整数除算)。
#[must_use]
pub fn text_osd_font_size(client_width: i32, style: &OsdStyle) -> i32 {
    (client_width / style.text_size_ratio.value)
        .clamp(style.text_size_min.value, style.text_size_max.value)
}

/// テキスト OSD のテキスト余白。原実装 `CreateTextOSD`(OSDManager.cpp:502)。
///
/// `FontSize / 2`。
#[must_use]
pub fn text_osd_text_margin(font_size: i32) -> i32 {
    font_size / 2
}

/// テキスト OSD の計測入力(`CalcTextSize` へ渡す SIZE)。原実装 `CreateTextOSD`
/// (OSDManager.cpp:514-520)。
///
/// `(client幅 - (Margin.Left + Margin.Right) - ImageWidth - TextMargin,
/// client高 - (Margin.Top + Margin.Bottom))`。
#[must_use]
pub fn text_osd_calc_size_input(
    rc_client: &RECT,
    style: &OsdStyle,
    image_width: i32,
    text_margin: i32,
) -> (i32, i32) {
    (
        (rc_client.right - rc_client.left)
            - (style.margin.left.value + style.margin.right.value)
            - image_width
            - text_margin,
        (rc_client.bottom - rc_client.top) - (style.margin.top.value + style.margin.bottom.value),
    )
}

/// 計測結果が単一行とみなせるか。原実装 `CreateTextOSD`(OSDManager.cpp:523)。
///
/// `sz.cy < FontSize * 3 / 2`(整数除算)。テキストスタイルの対応:
/// - 複数行(偽): `Left | VertCenter | Outline | FillBackground | MultiLine`(:506-511)
/// - 単一行(真): `HorzCenter | VertCenter | Outline | FillBackground`(:526-530)
///
/// スタイルフラグ自体は `CPseudoOSD::TextStyle` のため本クレートでは扱わず、
/// 単一行かどうかの bool のみ返す。
#[must_use]
pub fn is_single_line(measured_height: i32, font_size: i32) -> bool {
    measured_height < font_size * 3 / 2
}

/// テキスト OSD の位置。原実装 `CreateTextOSD`(OSDManager.cpp:533-537)。
///
/// `measured` は `CalcTextSize` の結果 `(cx, cy)`。位置は
/// `(client.left + Margin.Left, client.top + Margin.Top,
/// cx + TextMargin + ImageWidth, max(cy, ImageHeight))`。
#[must_use]
pub fn text_osd_position(
    rc_client: &RECT,
    style: &OsdStyle,
    measured: (i32, i32),
    text_margin: i32,
    image_width: i32,
    image_height: i32,
) -> OsdPosition {
    OsdPosition {
        left: rc_client.left + style.margin.left.value,
        top: rc_client.top + style.margin.top.value,
        width: measured.0 + text_margin + image_width,
        height: std::cmp::max(measured.1, image_height),
    }
}

// ---------------------------------------------------------------------------
// 番組情報 OSD(OSDManager.cpp:361-400)
// ---------------------------------------------------------------------------

/// `EventInfoOSDFlag` の Manual/Auto トグル。原実装 `ShowEventInfoOSD`
/// (OSDManager.cpp:386-392)。
///
/// 要求に `Manual` があれば `Manual` を立てて `Auto` を消し、そうでなく `Auto` が
/// あれば `Auto` を立てて `Manual` を消す。どちらも無ければ変更しない。
/// その他のビット(`Next` 等)は保持される。
#[must_use]
pub fn update_event_info_osd_flags(
    current: EventInfoOsdFlags,
    requested: EventInfoOsdFlags,
) -> EventInfoOsdFlags {
    let mut flags = current;
    if requested.contains(EventInfoOsdFlags::MANUAL) {
        flags |= EventInfoOsdFlags::MANUAL; // :387
        flags &= !EventInfoOsdFlags::AUTO; // :388
    } else if requested.contains(EventInfoOsdFlags::AUTO) {
        flags |= EventInfoOsdFlags::AUTO; // :390
        flags &= !EventInfoOsdFlags::MANUAL; // :391
    }
    flags
}

/// 番組情報 OSD の表示時間(ミリ秒)。原実装 `ShowEventInfoOSD`(OSDManager.cpp:394-399)。
///
/// - `flags`: [`update_event_info_osd_flags`] 適用**後**のフラグ(原実装は更新後の
///   `m_EventInfoOSDFlags` を参照する、:396)。
/// - `Manual` かつ `GetEventInfoOSDManualShowNoAutoHide()` なら 0(自動で隠さない)。
/// - さもなくば `min(GetEventInfoOSDDuration(), u32::MAX / 1000) * 1000`。
#[must_use]
pub fn event_info_osd_show_duration(
    flags: EventInfoOsdFlags,
    manual_show_no_auto_hide: bool,
    duration_sec: u32,
) -> u32 {
    if flags.contains(EventInfoOsdFlags::MANUAL) && manual_show_no_auto_hide {
        0
    } else {
        std::cmp::min(duration_sec, u32::MAX / 1000) * 1000
    }
}

// ---------------------------------------------------------------------------
// テスト
// ---------------------------------------------------------------------------

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

    fn logical(value: i32) -> IntValue {
        IntValue::with_logical(value)
    }

    // ----- OsdStyle 既定値(OSDManager.h:96-112) -----

    #[test]
    fn osd_style_default_values() {
        let style = OsdStyle::default();
        assert_eq!(style.margin, Margins::uniform(8, UnitType::LogicalPixel));
        assert_eq!(style.text_size_ratio, logical(28));
        assert_eq!(style.text_size_min, logical(12));
        assert_eq!(style.text_size_max, logical(100));
        assert_eq!(style.composite_text_size_ratio, logical(24));
        assert_eq!(style.composite_text_size_min, logical(12));
        assert_eq!(style.composite_text_size_max, logical(100));
        assert_eq!(style.logo_size, Size::new(64, 36, UnitType::LogicalPixel));
        assert!(style.logo_effect.is_empty());
        assert!(style.channel_animation);
        assert_eq!(
            style.volume_margin,
            Margins::uniform(16, UnitType::LogicalPixel)
        );
        assert_eq!(style.volume_text_size_min, logical(10));
        assert_eq!(style.volume_text_size_max, logical(50));
        assert_eq!(style.volume_horizontal_scale, logical(60));
        assert_eq!(style.volume_steps, logical(20));
        assert_eq!(style.volume_text_fill, "■");
        assert_eq!(style.volume_text_remain, "□");
    }

    // ----- set_style(OSDManager.cpp:563-587) -----

    #[test]
    fn set_style_resets_to_default_with_empty_manager() {
        // :567 冒頭の *this = OSDStyle() リセット
        let mut style = OsdStyle {
            text_size_ratio: logical(99),
            logo_effect: "gloss".to_string(),
            channel_animation: false,
            ..OsdStyle::default()
        };
        style.set_style(&StyleManager::new());
        assert_eq!(style, OsdStyle::default());
    }

    #[test]
    fn set_style_reads_all_keys() {
        let mut manager = StyleManager::new();
        manager.set_margins("osd.margin", &Margins::uniform(4, UnitType::LogicalPixel));
        manager.set_int_value("osd.text-size-ratio", logical(30));
        manager.set_int_value("osd.text-size-min", logical(14));
        manager.set_int_value("osd.text-size-max", logical(80));
        manager.set_int_value("osd.composite-text-size-ratio", logical(26));
        manager.set_int_value("osd.composite-text-size-min", logical(16));
        manager.set_int_value("osd.composite-text-size-max", logical(90));
        manager.set_size("channel-osd.logo", &Size::new(128, 72, UnitType::LogicalPixel));
        manager.set_string("channel-osd.logo.effect", "gloss");
        manager.set_bool("channel-osd.animation", false);
        manager.set_margins(
            "volume-osd.margin",
            &Margins::uniform(24, UnitType::LogicalPixel),
        );
        manager.set_int_value("volume-osd.text-size-min", logical(11));
        manager.set_int_value("volume-osd.text-size-max", logical(60));
        manager.set_int_value("volume-osd.horizontal-scale", logical(70));
        manager.set_int_value("volume-osd.steps", logical(10));
        manager.set_string("volume-osd.text.fill", "#");
        manager.set_string("volume-osd.text.remain", "-");

        let mut style = OsdStyle::default();
        style.set_style(&manager);

        assert_eq!(style.margin, Margins::uniform(4, UnitType::LogicalPixel));
        assert_eq!(style.text_size_ratio, logical(30));
        assert_eq!(style.text_size_min, logical(14));
        assert_eq!(style.text_size_max, logical(80));
        assert_eq!(style.composite_text_size_ratio, logical(26));
        assert_eq!(style.composite_text_size_min, logical(16));
        assert_eq!(style.composite_text_size_max, logical(90));
        assert_eq!(style.logo_size, Size::new(128, 72, UnitType::LogicalPixel));
        assert_eq!(style.logo_effect, "gloss");
        assert!(!style.channel_animation);
        assert_eq!(
            style.volume_margin,
            Margins::uniform(24, UnitType::LogicalPixel)
        );
        assert_eq!(style.volume_text_size_min, logical(11));
        assert_eq!(style.volume_text_size_max, logical(60));
        assert_eq!(style.volume_horizontal_scale, logical(70));
        assert_eq!(style.volume_steps, logical(10));
        assert_eq!(style.volume_text_fill, "#");
        assert_eq!(style.volume_text_remain, "-");
    }

    #[test]
    fn set_style_ignores_non_positive_ratios() {
        // :569-574 取得成功かつ Value > 0 のときだけ上書き
        let mut manager = StyleManager::new();
        manager.set_int_value("osd.text-size-ratio", logical(0));
        manager.set_int_value("osd.composite-text-size-ratio", logical(-5));
        let mut style = OsdStyle::default();
        style.set_style(&manager);
        assert_eq!(style.text_size_ratio, logical(28));
        assert_eq!(style.composite_text_size_ratio, logical(24));

        let mut manager = StyleManager::new();
        manager.set_int_value("osd.text-size-ratio", logical(1));
        style.set_style(&manager);
        assert_eq!(style.text_size_ratio, logical(1));
    }

    #[test]
    fn set_style_merges_partial_margins_in_place() {
        // C++ の Get(name, Margins*) は見つかった辺だけ上書きする(Style.cpp:308)
        let mut manager = StyleManager::new();
        manager.set_int_value("osd.margin.left", logical(2));
        let mut style = OsdStyle::default();
        style.set_style(&manager);
        assert_eq!(style.margin.left, logical(2));
        assert_eq!(style.margin.top, logical(8));
        assert_eq!(style.margin.right, logical(8));
        assert_eq!(style.margin.bottom, logical(8));
    }

    #[test]
    fn set_style_single_margin_value_applies_to_all_sides_then_overrides() {
        // 単一値 → 全辺、続いて各辺キーで上書き(Style.cpp:316-340)
        let mut manager = StyleManager::new();
        manager.set_int_value("osd.margin", logical(5));
        manager.set_int_value("osd.margin.top", logical(9));
        let mut style = OsdStyle::default();
        style.set_style(&manager);
        assert_eq!(style.margin.left, logical(5));
        assert_eq!(style.margin.top, logical(9));
        assert_eq!(style.margin.right, logical(5));
        assert_eq!(style.margin.bottom, logical(5));
    }

    #[test]
    fn set_style_merges_partial_logo_size_in_place() {
        // C++ の Get(name, Size*) は見つかった成分だけ上書きする(Style.cpp:266)
        let mut manager = StyleManager::new();
        manager.set_int_value("channel-osd.logo.width", logical(96));
        let mut style = OsdStyle::default();
        style.set_style(&manager);
        assert_eq!(style.logo_size.width, logical(96));
        assert_eq!(style.logo_size.height, logical(36));
    }

    // ----- normalize_style(OSDManager.cpp:590-603) -----

    #[test]
    fn normalize_style_scales_target_values() {
        let mut scaling = StyleScaling::default();
        assert!(scaling.set_dpi(192)); // 2 倍
        let mut style = OsdStyle::default();
        style.normalize_style(&scaling);

        let phys = |v: i32| IntValue::new(v, UnitType::PhysicalPixel);
        assert_eq!(style.text_size_min, phys(24));
        assert_eq!(style.text_size_max, phys(200));
        assert_eq!(style.composite_text_size_min, phys(24));
        assert_eq!(style.composite_text_size_max, phys(200));
        assert_eq!(style.margin.left, phys(16));
        assert_eq!(style.margin.top, phys(16));
        assert_eq!(style.margin.right, phys(16));
        assert_eq!(style.margin.bottom, phys(16));
        assert_eq!(style.logo_size.width, phys(128));
        assert_eq!(style.logo_size.height, phys(72));
        assert_eq!(style.volume_margin.left, phys(32));
        assert_eq!(style.volume_margin.bottom, phys(32));
        assert_eq!(style.volume_text_size_min, phys(20));
        assert_eq!(style.volume_text_size_max, phys(100));
    }

    #[test]
    fn normalize_style_leaves_ratios_and_steps() {
        // :594-602 に含まれない値は換算されない
        let mut scaling = StyleScaling::default();
        assert!(scaling.set_dpi(192));
        let mut style = OsdStyle::default();
        style.normalize_style(&scaling);
        assert_eq!(style.text_size_ratio, logical(28));
        assert_eq!(style.composite_text_size_ratio, logical(24));
        assert_eq!(style.volume_horizontal_scale, logical(60));
        assert_eq!(style.volume_steps, logical(20));
    }

    // ----- build_volume_text(OSDManager.cpp:249-258) -----

    #[test]
    fn volume_text_zero() {
        let style = OsdStyle::default();
        // steps = 20、100 / 20 = 5、0 / 5 = 0 個充填
        assert_eq!(build_volume_text(0, &style), format!("{} 0", "□".repeat(20)));
    }

    #[test]
    fn volume_text_full() {
        let style = OsdStyle::default();
        // min(100, 100) / 5 = 20 個充填
        assert_eq!(
            build_volume_text(100, &style),
            format!("{} 100", "■".repeat(20))
        );
    }

    #[test]
    fn volume_text_mid_truncates() {
        let style = OsdStyle::default();
        // 47 / 5 = 9(整数除算の切り捨て)
        assert_eq!(
            build_volume_text(47, &style),
            format!("{}{} 47", "■".repeat(9), "□".repeat(11))
        );
    }

    #[test]
    fn volume_text_boundary_step() {
        let style = OsdStyle::default();
        // 4 / 5 = 0、5 / 5 = 1
        assert!(build_volume_text(4, &style).starts_with("□"));
        assert!(build_volume_text(5, &style).starts_with("■□"));
    }

    #[test]
    fn volume_text_over_100_is_clamped_but_suffix_keeps_value() {
        let style = OsdStyle::default();
        // min(Volume, 100) でバーは満杯、末尾は生の値(:254, :258)
        assert_eq!(
            build_volume_text(120, &style),
            format!("{} 120", "■".repeat(20))
        );
    }

    #[test]
    fn volume_text_negative_volume() {
        let style = OsdStyle::default();
        // -5 / 5 = -1 → 充填 0 個、残り 20 個(C++ の for ループと同じ)
        assert_eq!(
            build_volume_text(-5, &style),
            format!("{} -5", "□".repeat(20))
        );
    }

    #[test]
    fn volume_text_steps_clamped_low() {
        let style = OsdStyle {
            volume_steps: logical(1), // clamp → 2
            ..OsdStyle::default()
        };
        // 100 / 2 = 50、50 / 50 = 1
        assert_eq!(build_volume_text(50, &style), "■□ 50");
    }

    #[test]
    fn volume_text_steps_clamped_high() {
        let style = OsdStyle {
            volume_steps: logical(200), // clamp → 100
            ..OsdStyle::default()
        };
        assert_eq!(
            build_volume_text(100, &style),
            format!("{} 100", "■".repeat(100))
        );
    }

    #[test]
    fn volume_text_steps_overflow_quirk() {
        // steps = 40: 100 / 40 = 2、100 / 2 = 50 > 40。原実装どおり 50 個並べて残りは無し
        let style = OsdStyle {
            volume_steps: logical(40),
            ..OsdStyle::default()
        };
        assert_eq!(
            build_volume_text(100, &style),
            format!("{} 100", "■".repeat(50))
        );
    }

    #[test]
    fn volume_text_custom_multichar_strings() {
        let style = OsdStyle {
            volume_steps: logical(2),
            volume_text_fill: "<*>".to_string(),
            volume_text_remain: "( )".to_string(),
            ..OsdStyle::default()
        };
        assert_eq!(build_volume_text(50, &style), "<*>( ) 50");
    }

    // ----- volume_osd_max_width(OSDManager.cpp:290) -----

    #[test]
    fn volume_osd_max_width_formula() {
        assert_eq!(volume_osd_max_width(30, 28, 80, 20), 30 * 20 + 80);
        assert_eq!(volume_osd_max_width(10, 40, 5, 2), 40 * 2 + 5);
        assert_eq!(volume_osd_max_width(0, 0, 0, 100), 0);
    }

    // ----- volume_font_size(OSDManager.cpp:298-303 / :324-329) -----

    #[test]
    fn volume_font_size_in_range() {
        let style = OsdStyle::default();
        // MulDiv(1000 - 32, 50 * 60, 1000 * 100) = MulDiv(968, 3000, 100000) = 29
        assert_eq!(volume_font_size(1000, &style, 1000), 29);
    }

    #[test]
    fn volume_font_size_rounding() {
        let style = OsdStyle::default();
        // MulDiv(1032, 3000, 100000) = 30.96 → 31(最近接丸め)
        assert_eq!(volume_font_size(1064, &style, 1000), 31);
        // MulDiv(1050, 3000, 100000) = 31.5 → 32(0.5 はゼロから遠い側へ)
        assert_eq!(volume_font_size(1082, &style, 1000), 32);
    }

    #[test]
    fn volume_font_size_clamped_to_max() {
        let style = OsdStyle::default();
        assert_eq!(volume_font_size(10000, &style, 1000), 50);
    }

    #[test]
    fn volume_font_size_clamped_to_min() {
        let style = OsdStyle::default();
        assert_eq!(volume_font_size(100, &style, 1000), 10);
        // available_width - Horz() が負でも下限へ
        assert_eq!(volume_font_size(0, &style, 1000), 10);
    }

    #[test]
    fn volume_font_size_zero_max_width_falls_to_min() {
        // MulDiv の分母 0 は -1(Win32 仕様)→ 下限へクランプ
        let style = OsdStyle::default();
        assert_eq!(volume_font_size(1000, &style, 0), 10);
    }

    // ----- volume_composite_text_position(OSDManager.cpp:307-313) -----

    #[test]
    fn volume_composite_position() {
        let style = OsdStyle::default();
        let client = rc(0, 0, 1000, 500);
        let src = rc(0, 0, 500, 250);
        // x = 0 + 16 * 500 / 1000 = 8
        // y = 250 - 30 - 16 * 250 / 500 = 250 - 30 - 8 = 212
        assert_eq!(
            volume_composite_text_position(&client, &src, 30, &style),
            (8, 212)
        );
    }

    #[test]
    fn volume_composite_position_with_offset_rects() {
        let style = OsdStyle::default();
        let client = rc(10, 20, 810, 620); // 800x600
        let src = rc(100, 50, 500, 350); // 400x300
        // x = 100 + 16 * 400 / 800 = 108
        // y = 350 - 24 - 16 * 300 / 600 = 350 - 24 - 8 = 318
        assert_eq!(
            volume_composite_text_position(&client, &src, 24, &style),
            (108, 318)
        );
    }

    // ----- volume_osd_calc_size_input / volume_osd_position(OSDManager.cpp:338-346) -----

    #[test]
    fn volume_osd_calc_size_input_subtracts_left_bottom() {
        let style = OsdStyle::default();
        let client = rc(10, 20, 1010, 520);
        assert_eq!(
            volume_osd_calc_size_input(&client, &style),
            (1000 - 16, 500 - 16)
        );
    }

    #[test]
    fn volume_osd_position_formula() {
        let style = OsdStyle::default();
        let client = rc(10, 20, 1010, 520);
        // (10 + 16, 520 - 40 - 16, 300 + 30 / 4, 40)
        assert_eq!(
            volume_osd_position(&client, (300, 40), 30, &style),
            OsdPosition {
                left: 26,
                top: 464,
                width: 307,
                height: 40,
            }
        );
    }

    #[test]
    fn volume_osd_position_font_size_quarter_truncates() {
        let style = OsdStyle::default();
        let client = rc(0, 0, 100, 100);
        // FontSize / 4 = 10 / 4 = 2(切り捨て)
        assert_eq!(volume_osd_position(&client, (50, 20), 10, &style).width, 52);
    }

    // ----- composite_text_font_size(OSDManager.cpp:453-455) -----

    #[test]
    fn composite_font_size_in_range() {
        let style = OsdStyle::default();
        assert_eq!(composite_text_font_size(1920, &style), 80); // 1920 / 24
        assert_eq!(composite_text_font_size(480, &style), 20);
    }

    #[test]
    fn composite_font_size_clamped() {
        let style = OsdStyle::default();
        assert_eq!(composite_text_font_size(240, &style), 12); // 10 → min 12
        assert_eq!(composite_text_font_size(4800, &style), 100); // 200 → max 100
    }

    // ----- composite_text_position(OSDManager.cpp:461-478) -----

    #[test]
    fn composite_position_with_zoom_rate() {
        let style = OsdStyle::default();
        let client = rc(0, 0, 1000, 600);
        let src = rc(0, 0, 500, 300);
        // Rate = 2, Factor = 1: left += (8 + 0) * 1 / 2 = 4、top += (0 + 8) * 1 / 2 = 4
        assert_eq!(
            composite_text_position(&client, &src, &style.margin, 0, Some((2, 1))),
            (4, 4)
        );
    }

    #[test]
    fn composite_position_with_left_offset() {
        let style = OsdStyle::default();
        let client = rc(0, 0, 1000, 600);
        let src = rc(0, 0, 500, 300);
        // left += (8 + 64) * 1 / 1 = 72(ロゴ幅ぶんのオフセット)
        assert_eq!(
            composite_text_position(&client, &src, &style.margin, 64, Some((1, 1))),
            (72, 8)
        );
    }

    #[test]
    fn composite_position_uses_client_top_for_top() {
        // :474 は rcClient.top + Margin.Top(クライアント矩形の絶対 top を使う)
        let style = OsdStyle::default();
        let client = rc(0, 100, 1000, 700);
        let src = rc(0, 0, 500, 300);
        assert_eq!(
            composite_text_position(&client, &src, &style.margin, 0, Some((1, 1))),
            (8, 108)
        );
    }

    #[test]
    fn composite_position_zero_rate_fallback() {
        // :475-478 Rate == 0 のときは +16 / +48
        let style = OsdStyle::default();
        let client = rc(0, 0, 1000, 600);
        let src = rc(10, 20, 510, 320);
        assert_eq!(
            composite_text_position(&client, &src, &style.margin, 0, Some((0, 7))),
            (26, 68)
        );
    }

    #[test]
    fn composite_position_fallback_width_basis() {
        // zoom 無し: 1000/500 = 2 < 1000/250 = 4 → Rate = client幅, Factor = src幅
        let style = OsdStyle::default();
        let client = rc(0, 0, 1000, 1000);
        let src = rc(0, 0, 500, 250);
        // left += 8 * 500 / 1000 = 4、top += 8 * 500 / 1000 = 4
        assert_eq!(
            composite_text_position(&client, &src, &style.margin, 0, None),
            (4, 4)
        );
    }

    #[test]
    fn composite_position_fallback_height_basis() {
        // 300/500 = 0 < 400/500 = 0 は偽 → Rate = client高, Factor = src高
        let style = OsdStyle::default();
        let client = rc(0, 0, 300, 400);
        let src = rc(0, 0, 500, 500);
        // left += 8 * 500 / 400 = 10、top += 8 * 500 / 400 = 10
        assert_eq!(
            composite_text_position(&client, &src, &style.margin, 0, None),
            (10, 10)
        );
    }

    #[test]
    fn composite_position_fallback_integer_division_comparison() {
        // 999/500 = 1 < 1000/500 = 2 → 幅基準(実数比較なら 1.998 > 2.0 ではない点に注意)
        let style = OsdStyle::default();
        let client = rc(0, 0, 999, 1000);
        let src = rc(0, 0, 500, 500);
        // left += 8 * 500 / 999 = 4、top += 8 * 500 / 999 = 4
        assert_eq!(
            composite_text_position(&client, &src, &style.margin, 0, None),
            (4, 4)
        );
    }

    // ----- text_osd_font_size / text_osd_text_margin(OSDManager.cpp:499-502) -----

    #[test]
    fn text_font_size_in_range() {
        let style = OsdStyle::default();
        assert_eq!(text_osd_font_size(1920, &style), 68); // 1920 / 28 = 68
    }

    #[test]
    fn text_font_size_clamped() {
        let style = OsdStyle::default();
        assert_eq!(text_osd_font_size(200, &style), 12); // 7 → min 12
        assert_eq!(text_osd_font_size(4000, &style), 100); // 142 → max 100
    }

    #[test]
    fn text_margin_is_half_font_size() {
        assert_eq!(text_osd_text_margin(68), 34);
        assert_eq!(text_osd_text_margin(13), 6); // 切り捨て
    }

    // ----- text_osd_calc_size_input(OSDManager.cpp:514-520) -----

    #[test]
    fn text_calc_size_input_formula() {
        let style = OsdStyle::default();
        let client = rc(0, 0, 1920, 1080);
        assert_eq!(
            text_osd_calc_size_input(&client, &style, 64, 34),
            (1920 - 16 - 64 - 34, 1080 - 16)
        );
    }

    #[test]
    fn text_calc_size_input_without_image() {
        let style = OsdStyle::default();
        let client = rc(100, 50, 900, 650);
        assert_eq!(
            text_osd_calc_size_input(&client, &style, 0, 10),
            (800 - 16 - 10, 600 - 16)
        );
    }

    // ----- is_single_line(OSDManager.cpp:523) -----

    #[test]
    fn single_line_threshold() {
        // FontSize * 3 / 2 = 30
        assert!(is_single_line(29, 20));
        assert!(!is_single_line(30, 20));
    }

    #[test]
    fn single_line_threshold_odd_font_truncates() {
        // 21 * 3 / 2 = 31(整数除算)
        assert!(is_single_line(30, 21));
        assert!(!is_single_line(31, 21));
    }

    // ----- text_osd_position(OSDManager.cpp:533-537) -----

    #[test]
    fn text_position_formula() {
        let style = OsdStyle::default();
        let client = rc(10, 20, 1930, 1100);
        assert_eq!(
            text_osd_position(&client, &style, (400, 50), 34, 64, 36),
            OsdPosition {
                left: 18,
                top: 28,
                width: 400 + 34 + 64,
                height: 50, // max(50, 36)
            }
        );
    }

    #[test]
    fn text_position_height_uses_image_when_larger() {
        let style = OsdStyle::default();
        let client = rc(0, 0, 100, 100);
        assert_eq!(
            text_osd_position(&client, &style, (40, 20), 10, 64, 36).height,
            36 // max(20, 36)
        );
    }

    // ----- osd_fade_time(OSDManager.cpp:119-123) -----

    #[test]
    fn fade_time_selection() {
        assert_eq!(osd_fade_time(ShowFlags::NO_FADE, 3000), 0);
        assert_eq!(osd_fade_time(ShowFlags::NO_FADE | ShowFlags::PSEUDO, 3000), 0);
        assert_eq!(osd_fade_time(ShowFlags::empty(), 3000), 3000);
        assert_eq!(osd_fade_time(ShowFlags::PSEUDO, 3000), 3000);
    }

    // ----- use_composite(OSDManager.cpp:125-126) -----

    #[test]
    fn use_composite_conditions() {
        assert!(use_composite(false, false, true));
        assert!(!use_composite(true, false, true));
        assert!(!use_composite(false, true, true));
        assert!(!use_composite(false, false, false));
    }

    // ----- channel_logo_effect(OSDManager.cpp:172-177) -----

    #[test]
    fn logo_effect_changing_is_dark() {
        assert_eq!(channel_logo_effect(true, ""), LogoEffect::Dark);
        assert_eq!(channel_logo_effect(true, "gloss"), LogoEffect::Dark);
    }

    #[test]
    fn logo_effect_gloss_case_insensitive() {
        assert_eq!(channel_logo_effect(false, "gloss"), LogoEffect::Gloss);
        assert_eq!(channel_logo_effect(false, "GLOSS"), LogoEffect::Gloss);
        assert_eq!(channel_logo_effect(false, "Gloss"), LogoEffect::Gloss);
    }

    #[test]
    fn logo_effect_other_is_none() {
        assert_eq!(channel_logo_effect(false, ""), LogoEffect::None);
        assert_eq!(channel_logo_effect(false, "matte"), LogoEffect::None);
        assert_eq!(channel_logo_effect(false, "glossy"), LogoEffect::None);
    }

    // ----- channel_osd_animation(OSDManager.cpp:159-163) -----

    #[test]
    fn channel_animation_conditions() {
        assert!(channel_osd_animation(true, false, false));
        assert!(!channel_osd_animation(true, true, false)); // 切替中
        assert!(!channel_osd_animation(true, false, true)); // OSD 可視
        assert!(!channel_osd_animation(false, false, false)); // スタイルで無効
    }

    // ----- channel_osd_text_color(OSDManager.cpp:213-217) -----

    #[test]
    fn text_color_darkened_while_changing() {
        // MixColor(白, 黒, 160): 各成分 (255 * 160 + 0 * 95) / 255 = 160
        assert_eq!(channel_osd_text_color(0x00FF_FFFF, true), 0x00A0_A0A0);
    }

    #[test]
    fn text_color_unchanged_when_not_changing() {
        assert_eq!(channel_osd_text_color(0x00FF_FFFF, false), 0x00FF_FFFF);
        assert_eq!(channel_osd_text_color(0x0012_3456, false), 0x0012_3456);
    }

    // ----- channel_logo_position(OSDManager.cpp:182-185) -----

    #[test]
    fn logo_position_formula() {
        let style = OsdStyle::default();
        let client = rc(10, 20, 1010, 620);
        assert_eq!(
            channel_logo_position(&client, &style),
            OsdPosition {
                left: 18,
                top: 28,
                width: 64,
                height: 36,
            }
        );
    }

    // ----- decide_channel_osd_mode(OSDManager.cpp:169-220) -----

    #[test]
    fn mode_logo_only_when_logo_available() {
        // LogoOnly + ロゴあり → 合成可否に関わらず LogoOnly(:179-188)
        for (pseudo, force, draw) in [
            (false, false, true),
            (true, false, true),
            (false, true, false),
        ] {
            assert_eq!(
                decide_channel_osd_mode(
                    ChannelChangeType::LogoOnly,
                    true,
                    false,
                    pseudo,
                    force,
                    draw
                ),
                ChannelOsdMode::LogoOnly
            );
        }
    }

    #[test]
    fn mode_logo_only_without_logo() {
        // LogoOnly + ロゴ無し + 合成可 → ロゴもテキストも表示しない Composite
        assert_eq!(
            decide_channel_osd_mode(ChannelChangeType::LogoOnly, false, false, false, false, true),
            ChannelOsdMode::Composite {
                show_logo: false,
                show_text: false,
            }
        );
        // 合成不可なら PseudoOsd(:210-220)
        assert_eq!(
            decide_channel_osd_mode(ChannelChangeType::LogoOnly, false, false, true, false, true),
            ChannelOsdMode::PseudoOsd
        );
    }

    #[test]
    fn mode_logo_and_text_composite() {
        assert_eq!(
            decide_channel_osd_mode(
                ChannelChangeType::LogoAndText,
                true,
                false,
                false,
                false,
                true
            ),
            ChannelOsdMode::Composite {
                show_logo: true,
                show_text: true,
            }
        );
        // テキストが空ならロゴのみ合成側で表示
        assert_eq!(
            decide_channel_osd_mode(ChannelChangeType::LogoAndText, true, true, false, false, true),
            ChannelOsdMode::Composite {
                show_logo: true,
                show_text: false,
            }
        );
    }

    #[test]
    fn mode_text_only_ignores_logo() {
        // TextOnly ではロゴを取得しない(:169)ため has_logo は無視される
        assert_eq!(
            decide_channel_osd_mode(ChannelChangeType::TextOnly, true, false, false, false, true),
            ChannelOsdMode::Composite {
                show_logo: false,
                show_text: true,
            }
        );
        // TextOnly + LogoOnly 早期 return も起きない
        assert_eq!(
            decide_channel_osd_mode(ChannelChangeType::TextOnly, true, false, true, false, true),
            ChannelOsdMode::PseudoOsd
        );
    }

    #[test]
    fn mode_pseudo_when_composite_unavailable() {
        assert_eq!(
            decide_channel_osd_mode(
                ChannelChangeType::LogoAndText,
                true,
                false,
                false,
                true,
                true
            ),
            ChannelOsdMode::PseudoOsd
        );
        assert_eq!(
            decide_channel_osd_mode(
                ChannelChangeType::LogoAndText,
                false,
                false,
                false,
                false,
                false
            ),
            ChannelOsdMode::PseudoOsd
        );
    }

    // ----- update_event_info_osd_flags(OSDManager.cpp:386-392) -----

    #[test]
    fn event_flags_manual_clears_auto() {
        assert_eq!(
            update_event_info_osd_flags(EventInfoOsdFlags::AUTO, EventInfoOsdFlags::MANUAL),
            EventInfoOsdFlags::MANUAL
        );
    }

    #[test]
    fn event_flags_auto_clears_manual() {
        assert_eq!(
            update_event_info_osd_flags(EventInfoOsdFlags::MANUAL, EventInfoOsdFlags::AUTO),
            EventInfoOsdFlags::AUTO
        );
    }

    #[test]
    fn event_flags_manual_wins_over_auto_in_request() {
        // else if のため要求に両方あれば Manual 側の処理のみ(:386-392)
        assert_eq!(
            update_event_info_osd_flags(
                EventInfoOsdFlags::AUTO,
                EventInfoOsdFlags::MANUAL | EventInfoOsdFlags::AUTO
            ),
            EventInfoOsdFlags::MANUAL
        );
    }

    #[test]
    fn event_flags_preserves_other_bits() {
        assert_eq!(
            update_event_info_osd_flags(
                EventInfoOsdFlags::AUTO | EventInfoOsdFlags::NEXT,
                EventInfoOsdFlags::MANUAL
            ),
            EventInfoOsdFlags::MANUAL | EventInfoOsdFlags::NEXT
        );
    }

    #[test]
    fn event_flags_unchanged_without_manual_or_auto() {
        assert_eq!(
            update_event_info_osd_flags(EventInfoOsdFlags::MANUAL, EventInfoOsdFlags::NEXT),
            EventInfoOsdFlags::MANUAL
        );
        assert_eq!(
            update_event_info_osd_flags(EventInfoOsdFlags::AUTO, EventInfoOsdFlags::empty()),
            EventInfoOsdFlags::AUTO
        );
    }

    // ----- event_info_osd_show_duration(OSDManager.cpp:394-399) -----

    #[test]
    fn show_duration_zero_for_manual_no_auto_hide() {
        assert_eq!(
            event_info_osd_show_duration(EventInfoOsdFlags::MANUAL, true, 10),
            0
        );
    }

    #[test]
    fn show_duration_seconds_to_milliseconds() {
        assert_eq!(
            event_info_osd_show_duration(EventInfoOsdFlags::MANUAL, false, 10),
            10_000
        );
        assert_eq!(
            event_info_osd_show_duration(EventInfoOsdFlags::AUTO, true, 7),
            7_000
        );
        assert_eq!(
            event_info_osd_show_duration(EventInfoOsdFlags::empty(), false, 0),
            0
        );
    }

    #[test]
    fn show_duration_clamps_to_avoid_overflow() {
        // min(duration, u32::MAX / 1000) * 1000(:399)
        assert_eq!(
            event_info_osd_show_duration(EventInfoOsdFlags::AUTO, false, u32::MAX),
            (u32::MAX / 1000) * 1000
        );
        assert_eq!(
            event_info_osd_show_duration(EventInfoOsdFlags::AUTO, false, 5_000_000),
            4_294_967_000
        );
    }
}
