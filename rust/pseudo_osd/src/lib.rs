/*
  TVTest
  Copyright(c) 2008-2020 DBCTRADO

  This program is free software; you can redistribute it and/or modify
  it under the terms of the GNU General Public License as published by
  the Free Software Foundation; either version 2 of the License, or
  (at your option) any later version.

  This program is distributed in the hope that it will be useful,
  but WITHOUT ANY WARRANTY; without even the implied warranty of
  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
  GNU General Public License for more details.

  You should have received a copy of the GNU General Public License
  along with this program; if not, write to the Free Software
  Foundation, Inc., 59 Temple Place, Suite 330, Boston, MA  02111-1307  USA
*/

//! TVTest の疑似 OSD ウィンドウ `CPseudoOSD`(原実装 `src/PseudoOSD.cpp` /
//! `src/PseudoOSD.h`)を `windows` クレート(windows-rs)で Rust へフル移植したもの。
//!
//! 移植対象(PseudoOSD.cpp 全 668 行):
//! - ウィンドウクラス登録([`initialize`]、PseudoOSD.cpp:58-78)と
//!   クラス判定([`is_pseudo_osd`]、:81-87)。
//! - [`PseudoOsd`][]:生成/破棄(:104-156)、表示/非表示(:159-244)、
//!   タイマーによる自動非表示とワイプアニメーション(WndProc :694-803)、
//!   テキスト・アイコン・画像の設定(:260-274, :413-437)、
//!   位置(:277-313)、色・フォント・スタイル(:316-348)、
//!   テキストサイズ計測(:351-410)、親ウィンドウ移動追従(:440-458)、
//!   非レイヤード描画(:461-539)、レイヤードウィンドウ更新(:566-685)。
//! - [`TextStyle`] / [`ImageFlag`] / [`ImageEffect`](PseudoOSD.h:36-63)。
//!
//! ## 原実装との差異
//!
//! - `CPseudoOSD::Initialize(HINSTANCE)` は引数を取らず、`GetModuleHandleW(None)`
//!   で自モジュールの `HINSTANCE` を取得する([`initialize`])。
//! - `IsPseudoOSD` の `lstrcmpi` はロケール依存比較だが、クラス名は ASCII
//!   固定のため ASCII 大小無視比較で代替する([`is_pseudo_osd`])。
//! - C++ の「`this` を `lpCreateParams` 経由で `GWLP_USERDATA` に格納」パターンは、
//!   `Box::into_raw` で固定アドレス化した内部状態 `Inner` への生ポインタで実現する。
//!   WndProc 再入時の `&mut` 二重借用を避けるため、フィールド操作は生ポインタ
//!   経由の関数に集約している。
//! - `UpdateLayeredWindow` 内で `CImage::Create` に失敗した場合、原実装は
//!   取得済みの DC を解放せずに return する(PseudoOSD.cpp:585-586 のリーク)が、
//!   本移植では解放してから戻る。
//! - レイヤードウィンドウ描画・計測(`Graphics::CCanvas` 相当)は GDI+ を使うため、
//!   事前に `tvtest_graphics::GraphicsCore` の初期化が必要(原実装ではアプリ本体が
//!   `CGraphicsCore` を初期化している)。未初期化の場合、該当経路は何も描画しない。
//!
//! ## 対象外
//!
//! なし(PseudoOSD.cpp の全メンバーを移植)。

#![cfg(windows)]

use std::ffi::c_void;
use std::sync::Mutex;

use bitflags::bitflags;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, ClientToScreen, CreateCompatibleDC, DeleteDC, DrawTextW, EndPaint, GetDC,
    GetObjectW, MapWindowPoints, OffsetRect, PtInRect, RedrawWindow, ReleaseDC, SelectObject,
    SetBkMode, SetTextColor, UpdateWindow, AC_SRC_ALPHA, AC_SRC_OVER, BACKGROUND_MODE, BITMAP,
    BLENDFUNCTION, DT_CALCRECT, DT_CENTER, DT_NOPREFIX, DT_RIGHT, DT_SINGLELINE, DT_WORDBREAK,
    HBITMAP, HDC, HGDIOBJ, HPALETTE, LOGFONTW, PAINTSTRUCT, RDW_INVALIDATE, RDW_UPDATENOW,
    TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, CreateWindowExW, DefWindowProcW, DestroyWindow, GetClassNameW,
    GetClientRect, GetParent, GetWindowLongPtrW, GetWindowRect, IsWindowVisible, MoveWindow,
    RegisterClassW, SendMessageW, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    UpdateLayeredWindow, CREATESTRUCTW, CS_HREDRAW, GWLP_USERDATA, GWL_STYLE, HTTRANSPARENT,
    HWND_TOP, SET_WINDOW_POS_FLAGS, SWP_NOACTIVATE, SWP_NOZORDER, SW_HIDE, SW_SHOW, SW_SHOWNA,
    ULW_ALPHA, WINDOW_EX_STYLE, WM_CREATE, WM_DESTROY, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MBUTTONDBLCLK, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEMOVE, WM_NCHITTEST,
    WM_PAINT, WM_RBUTTONDBLCLK, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SIZE, WM_TIMER, WNDCLASSW,
    WS_CHILD, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TRANSPARENT, WS_POPUP, WS_VISIBLE,
};

use tvtest_draw_util::{
    color_overlay, draw_bitmap, fill, get_default_ui_font, gloss_overlay, Bitmap as GdiBitmap,
    Font as GdiFont,
};
use tvtest_graphics::{
    Brush, Canvas, Color, Font as GpFont, GradientDirection, Image, TextFlag,
};
use tvtest_window_util::WindowTimerManager;
use tvtest_winutil::is_windows_8_or_later;

// ---------------------------------------------------------------------------
// 定数(PseudoOSD.cpp:31-54)
// ---------------------------------------------------------------------------

/// 自動非表示タイマーの識別子(PseudoOSD.cpp:35)。
const TIMER_ID_HIDE: u32 = 0x0001;
/// アニメーションタイマーの識別子(PseudoOSD.cpp:36)。
const TIMER_ID_ANIMATION: u32 = 0x0002;

/// アニメーションの段階数(PseudoOSD.cpp:38)。
const ANIMATION_FRAMES: i32 = 4;
/// アニメーションの間隔(ミリ秒)(PseudoOSD.cpp:39)。
const ANIMATION_INTERVAL: u32 = 50;

/// ウィンドウクラス名 `APP_NAME TEXT(" Pseudo OSD")`(PseudoOSD.cpp:54)。
const WINDOW_CLASS_NAME: PCWSTR = windows::core::w!("TVTest Pseudo OSD");

/// `RGB(r, g, b)` 相当の COLORREF(`0x00BBGGRR`)を作る。
const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
}

/// 縁取りテキストのアウトライン幅。原実装 `GetOutlineWidth`(PseudoOSD.cpp:46-49)。
fn get_outline_width(font_size: i32) -> f32 {
    font_size as f32 / 5.0
}

// ---------------------------------------------------------------------------
// フラグ型(PseudoOSD.h:36-63)
// ---------------------------------------------------------------------------

bitflags! {
    /// テキストの配置・装飾スタイル。原実装 `CPseudoOSD::TextStyle`(PseudoOSD.h:36-50)。
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
    pub struct TextStyle: u32 {
        /// `Right`(PseudoOSD.h:39)。右寄せ。
        const RIGHT = 0x0001;
        /// `HorzCenter`(PseudoOSD.h:40)。水平中央寄せ。
        const HORZ_CENTER = 0x0002;
        /// `HorzAlignMask`(PseudoOSD.h:41)。水平アライメントのマスク。
        const HORZ_ALIGN_MASK = 0x0003;
        /// `Bottom`(PseudoOSD.h:43)。下寄せ。
        const BOTTOM = 0x0004;
        /// `VertCenter`(PseudoOSD.h:44)。垂直中央寄せ。
        const VERT_CENTER = 0x0008;
        /// `VertAlignMask`(PseudoOSD.h:45)。垂直アライメントのマスク。
        const VERT_ALIGN_MASK = 0x000C;
        /// `Outline`(PseudoOSD.h:46)。縁取り文字。
        const OUTLINE = 0x0010;
        /// `FillBackground`(PseudoOSD.h:47)。テキスト背景を半透明黒で塗る。
        const FILL_BACKGROUND = 0x0020;
        /// `MultiLine`(PseudoOSD.h:48)。複数行(折り返しあり)。
        const MULTI_LINE = 0x0040;
    }
}

impl TextStyle {
    /// `TextStyle::None`(PseudoOSD.h:37)。
    pub const NONE: Self = Self::empty();
    /// `Left`(PseudoOSD.h:38)。左寄せ(既定)。
    pub const LEFT: Self = Self::empty();
    /// `Top`(PseudoOSD.h:42)。上寄せ(既定)。
    pub const TOP: Self = Self::empty();
}

bitflags! {
    /// 画像表示のフラグ。原実装 `CPseudoOSD::ImageFlag`(PseudoOSD.h:52-56)。
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
    pub struct ImageFlag: u32 {
        /// `DirectSource`(PseudoOSD.h:54)。ビットマップをそのまま
        /// レイヤードウィンドウの転送元にする。
        const DIRECT_SOURCE = 0x0001;
    }
}

impl ImageFlag {
    /// `ImageFlag::None`(PseudoOSD.h:53)。
    pub const NONE: Self = Self::empty();
}

bitflags! {
    /// 画像に重ねる効果。原実装 `CPseudoOSD::ImageEffect`(PseudoOSD.h:58-63)。
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
    pub struct ImageEffect: u32 {
        /// `Gloss`(PseudoOSD.h:60)。光沢効果。
        const GLOSS = 0x0001;
        /// `Dark`(PseudoOSD.h:61)。暗くする効果。
        const DARK = 0x0002;
    }
}

impl ImageEffect {
    /// `ImageEffect::None`(PseudoOSD.h:59)。
    pub const NONE: Self = Self::empty();
}

// ---------------------------------------------------------------------------
// ウィンドウクラス登録(PseudoOSD.cpp:58-87)
// ---------------------------------------------------------------------------

/// クラス登録済みガード。原実装の static `m_hinst`(PseudoOSD.cpp:55)相当。
/// 登録失敗時は false のままで、次回の [`initialize`] が再試行する(原実装と同じ)。
static CLASS_REGISTERED: Mutex<bool> = Mutex::new(false);

/// ウィンドウクラス "TVTest Pseudo OSD" を登録する。
/// 原実装 `CPseudoOSD::Initialize`(PseudoOSD.cpp:58-78)。
///
/// 原実装は `HINSTANCE` を引数に取るが、本移植では `GetModuleHandleW(None)` で
/// 自モジュールを使う。多重呼び出しは何もせず `true`。
pub fn initialize() -> bool {
    let mut registered = CLASS_REGISTERED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !*registered {
        // SAFETY: GetModuleHandleW(None) は自プロセスのモジュールハンドルを返す。
        let hinst: HINSTANCE = match unsafe { GetModuleHandleW(None) } {
            Ok(hmodule) => hmodule.into(),
            Err(_) => return false,
        };
        let wc = WNDCLASSW {
            style: CS_HREDRAW,
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: Default::default(),
            hCursor: Default::default(),
            hbrBackground: Default::default(),
            lpszMenuName: PCWSTR::null(),
            lpszClassName: WINDOW_CLASS_NAME,
        };
        // SAFETY: wc は初期化済みの WNDCLASSW。lpszClassName は 'static な文字列。
        if unsafe { RegisterClassW(&wc) } == 0 {
            return false;
        }
        *registered = true;
    }
    true
}

/// ASCII 大文字を小文字にする(クラス名比較用)。
fn to_lower_u16(c: u16) -> u16 {
    if (u16::from(b'A')..=u16::from(b'Z')).contains(&c) {
        c + 32
    } else {
        c
    }
}

/// 2 つのワイド文字列を ASCII 大小無視で比較する。
fn wide_eq_ascii_ci(a: &[u16], b: &[u16]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b.iter())
            .all(|(&x, &y)| to_lower_u16(x) == to_lower_u16(y))
}

/// ウィンドウが疑似 OSD かクラス名で判定する。
/// 原実装 `CPseudoOSD::IsPseudoOSD`(PseudoOSD.cpp:81-87)。
///
/// 原実装は `lstrcmpi`(ロケール依存)だが、クラス名は ASCII 固定のため
/// ASCII 大小無視比較で代替する。
pub fn is_pseudo_osd(hwnd: HWND) -> bool {
    let mut buf = [0u16; 64];
    // SAFETY: buf は書き込み可能なバッファ。無効な hwnd では 0 が返る。
    let len = unsafe { GetClassNameW(hwnd, &mut buf) };
    if len <= 0 {
        return false;
    }
    // SAFETY: WINDOW_CLASS_NAME は NUL 終端の 'static ワイド文字列。
    let class = unsafe { WINDOW_CLASS_NAME.as_wide() };
    wide_eq_ascii_ci(&buf[..len as usize], class)
}

// ---------------------------------------------------------------------------
// 内部状態(PseudoOSD.h:89-110 のメンバー変数)
// ---------------------------------------------------------------------------

/// ウィンドウ位置(PseudoOSD.h:102-104 の無名構造体)。
#[derive(Clone, Copy, Debug, Default)]
struct Position {
    left: i32,
    top: i32,
    width: i32,
    height: i32,
}

/// `CPseudoOSD` の全メンバー変数(PseudoOSD.h:89-110)。
///
/// `Box::into_raw` で固定アドレス化し、生ポインタを `GWLP_USERDATA` に格納して
/// WndProc から復元する(原実装の `this` 格納パターン相当)。WndProc 再入時の
/// `&mut` 二重借用を避けるため、操作は `*mut Inner` を取る関数に集約する。
struct Inner {
    /// `m_hwnd`(PseudoOSD.h:90)。
    hwnd: HWND,
    /// `m_crBackColor = RGB(0, 0, 0)`(PseudoOSD.h:91)。
    back_color: u32,
    /// `m_crTextColor = RGB(0, 255, 128)`(PseudoOSD.h:92)。
    text_color: u32,
    /// `m_Font`(PseudoOSD.h:93)。
    font: GdiFont,
    /// `m_TextStyle = TextStyle::Outline`(PseudoOSD.h:94)。
    text_style: TextStyle,
    /// `m_Text`(PseudoOSD.h:95)。UTF-16(NUL なし)。
    text: Vec<u16>,
    /// `m_hbmIcon`(PseudoOSD.h:96)。
    hbm_icon: HBITMAP,
    /// `m_IconWidth`(PseudoOSD.h:97)。
    icon_width: i32,
    /// `m_IconHeight`(PseudoOSD.h:98)。
    icon_height: i32,
    /// `m_hbm`(PseudoOSD.h:99)。
    hbm: HBITMAP,
    /// `m_ImageEffect`(PseudoOSD.h:100)。
    image_effect: ImageEffect,
    /// `m_ImageFlags`(PseudoOSD.h:101)。
    image_flags: ImageFlag,
    /// `m_Position`(PseudoOSD.h:102-104)。
    position: Position,
    /// `m_Timer`(PseudoOSD.h:105)。
    timer: WindowTimerManager,
    /// `m_AnimationCount`(PseudoOSD.h:106)。
    animation_count: i32,
    /// `m_fLayeredWindow`(PseudoOSD.h:107)。
    layered_window: bool,
    /// `m_fPopupLayeredWindow`(PseudoOSD.h:108)。
    popup_layered_window: bool,
    /// `m_hwndParent`(PseudoOSD.h:109)。
    hwnd_parent: HWND,
    /// `m_ParentPosition`(PseudoOSD.h:110)。
    parent_position: POINT,
}

// ---------------------------------------------------------------------------
// 公開型 PseudoOsd
// ---------------------------------------------------------------------------

/// 疑似 OSD ウィンドウ。原実装 `CPseudoOSD`(PseudoOSD.h:33-121)。
///
/// 映像上にテキスト・ロゴ画像を重ねて表示するための子ウィンドウ
/// (またはポップアップレイヤードウィンドウ)を管理する。
///
/// ウィンドウはスレッド親和のため、本型は `Send`/`Sync` にならない
/// (生ポインタ保持による自動判定)。`Drop` で [`PseudoOsd::destroy`] 相当を
/// 実行する(原実装 `~CPseudoOSD`、PseudoOSD.cpp:98-101)。
///
/// 表示するビットマップ(`set_text` のアイコン / `set_image` の画像)は、
/// 表示中は有効な GDI ハンドルであり続ける必要がある(原実装と同じ契約。
/// 所有権は移らず、破棄は呼び出し側の責任)。
pub struct PseudoOsd {
    inner: *mut Inner,
}

impl PseudoOsd {
    /// 生成する。原実装 `CPseudoOSD::CPseudoOSD`(PseudoOSD.cpp:90-95)。
    ///
    /// フォントは `DrawUtil::GetDefaultUIFont` 相当([`get_default_ui_font`])で
    /// 初期化する。
    #[must_use]
    pub fn new() -> Self {
        let mut font = GdiFont::new();
        if let Some(lf) = get_default_ui_font() {
            font.create(&lf);
        }
        let inner = Box::new(Inner {
            hwnd: HWND::default(),
            back_color: rgb(0, 0, 0),
            text_color: rgb(0, 255, 128),
            font,
            text_style: TextStyle::OUTLINE,
            text: Vec::new(),
            hbm_icon: HBITMAP::default(),
            icon_width: 0,
            icon_height: 0,
            hbm: HBITMAP::default(),
            image_effect: ImageEffect::NONE,
            image_flags: ImageFlag::NONE,
            position: Position::default(),
            timer: WindowTimerManager::new(),
            animation_count: 0,
            layered_window: false,
            popup_layered_window: false,
            hwnd_parent: HWND::default(),
            parent_position: POINT::default(),
        });
        Self {
            inner: Box::into_raw(inner),
        }
    }

    /// ウィンドウを生成する。原実装 `CPseudoOSD::Create`(PseudoOSD.cpp:104-142)。
    ///
    /// 既にウィンドウがあり、親と `layered_window` が同じなら何もせず `true`。
    /// 違えば作り直す。`layered_window` が `true` かつ Windows 8 未満なら
    /// ポップアップレイヤードウィンドウ(`WS_POPUP` + スクリーン座標)、
    /// それ以外は子ウィンドウ(`WS_CHILD`)になる。
    ///
    /// 事前に [`initialize`] でクラス登録が必要(原実装と同じ)。
    pub fn create(&mut self, hwnd_parent: HWND, layered_window: bool) -> bool {
        // SAFETY: self.inner は Box::into_raw で得た有効なポインタ。
        unsafe { osd_create(self.inner, hwnd_parent, layered_window) }
    }

    /// ウィンドウを破棄する。原実装 `CPseudoOSD::Destroy`(PseudoOSD.cpp:145-150)。
    pub fn destroy(&mut self) -> bool {
        // SAFETY: self.inner は有効。DestroyWindow → WM_DESTROY で hwnd がクリアされる。
        unsafe { osd_destroy(self.inner) }
    }

    /// ウィンドウが生成済みか。原実装 `CPseudoOSD::IsCreated`(PseudoOSD.cpp:153-156)。
    #[must_use]
    pub fn is_created(&self) -> bool {
        // SAFETY: self.inner は有効。
        unsafe { !(*self.inner).hwnd.0.is_null() }
    }

    /// 表示する。原実装 `CPseudoOSD::Show`(PseudoOSD.cpp:159-224)。
    ///
    /// `time` が 0 より大きければ `time` ミリ秒後に自動で隠れる。さらに
    /// `animation` が `true` なら幅 1/4 から 4 段階で広がるワイプ
    /// アニメーションを行う(タイマー間隔 50ms)。
    pub fn show(&mut self, time: u32, animation: bool) -> bool {
        // SAFETY: self.inner は有効。
        unsafe { osd_show(self.inner, time, animation) }
    }

    /// 隠す。原実装 `CPseudoOSD::Hide`(PseudoOSD.cpp:227-235)。
    ///
    /// テキストと画像(`m_hbm`)はクリアされる。
    pub fn hide(&mut self) -> bool {
        // SAFETY: self.inner は有効。
        unsafe { osd_hide(self.inner) }
    }

    /// 表示中か。原実装 `CPseudoOSD::IsVisible`(PseudoOSD.cpp:238-244)。
    ///
    /// 親が非表示でも判定できるよう `GWL_STYLE` の `WS_VISIBLE` で判定する。
    #[must_use]
    pub fn is_visible(&self) -> bool {
        // SAFETY: self.inner は有効。
        unsafe {
            let hwnd = (*self.inner).hwnd;
            if hwnd.0.is_null() {
                return false;
            }
            (GetWindowLongPtrW(hwnd, GWL_STYLE) as u32) & WS_VISIBLE.0 != 0
        }
    }

    /// 再描画する。原実装 `CPseudoOSD::Update`(PseudoOSD.cpp:247-257)。
    pub fn update(&mut self) -> bool {
        // SAFETY: self.inner は有効。
        unsafe { osd_update(self.inner) }
    }

    /// テキストと(任意で)アイコンを設定する。
    /// 原実装 `CPseudoOSD::SetText`(PseudoOSD.cpp:260-274)。
    ///
    /// `text` は UTF-16 で、最初の NUL で切り詰める(`LPCTSTR` の意味論)。
    /// `hbm_icon` が非 NULL ならアイコンのサイズと効果も設定される。
    /// 画像(`set_image` のビットマップ)はクリアされる。
    /// `hbm_icon` は表示中は有効な GDI ビットマップであり続けること。
    pub fn set_text(
        &mut self,
        text: &[u16],
        hbm_icon: HBITMAP,
        icon_width: i32,
        icon_height: i32,
        effect: ImageEffect,
    ) -> bool {
        let len = text.iter().position(|&c| c == 0).unwrap_or(text.len());
        // SAFETY: self.inner は有効。Win32 呼び出しが無いため再入しない。
        unsafe {
            (*self.inner).text.clear();
            (*self.inner).text.extend_from_slice(&text[..len]);
            (*self.inner).hbm_icon = hbm_icon;
            if !hbm_icon.0.is_null() {
                (*self.inner).icon_width = icon_width;
                (*self.inner).icon_height = icon_height;
                (*self.inner).image_effect = effect;
            } else {
                (*self.inner).icon_width = 0;
                (*self.inner).icon_height = 0;
            }
            (*self.inner).hbm = HBITMAP::default();
        }
        true
    }

    /// 位置とサイズを設定する。原実装 `CPseudoOSD::SetPosition`(PseudoOSD.cpp:277-300)。
    ///
    /// `width`/`height` が 0 以下なら `false`。ウィンドウ生成済みなら移動も行う。
    pub fn set_position(&mut self, left: i32, top: i32, width: i32, height: i32) -> bool {
        // SAFETY: self.inner は有効。
        unsafe { osd_set_position(self.inner, left, top, width, height) }
    }

    /// 位置とサイズ `(left, top, width, height)` を取得する。
    /// 原実装 `CPseudoOSD::GetPosition`(PseudoOSD.cpp:303-313)。
    #[must_use]
    pub fn get_position(&self) -> (i32, i32, i32, i32) {
        // SAFETY: self.inner は有効。
        unsafe {
            let p = (*self.inner).position;
            (p.left, p.top, p.width, p.height)
        }
    }

    /// テキスト色(COLORREF)を設定する。
    /// 原実装 `CPseudoOSD::SetTextColor`(PseudoOSD.cpp:316-323)。
    pub fn set_text_color(&mut self, cr_text: u32) {
        // SAFETY: self.inner は有効。
        unsafe {
            (*self.inner).text_color = cr_text;
        }
    }

    /// テキストの高さ(ピクセル)を設定する。
    /// 原実装 `CPseudoOSD::SetTextHeight`(PseudoOSD.cpp:326-335)。
    pub fn set_text_height(&mut self, height: i32) -> bool {
        // SAFETY: self.inner は有効。GDI 呼び出しのみで再入しない。
        unsafe {
            let Some(mut lf) = (*self.inner).font.get_log_font() else {
                return false;
            };
            lf.lfWidth = 0;
            lf.lfHeight = -height;
            (*self.inner).font.create(&lf)
        }
    }

    /// テキストスタイルを設定する。
    /// 原実装 `CPseudoOSD::SetTextStyle`(PseudoOSD.cpp:338-342)。
    pub fn set_text_style(&mut self, style: TextStyle) -> bool {
        // SAFETY: self.inner は有効。
        unsafe {
            (*self.inner).text_style = style;
        }
        true
    }

    /// フォントを設定する。原実装 `CPseudoOSD::SetFont`(PseudoOSD.cpp:345-348)。
    pub fn set_font(&mut self, font: &LOGFONTW) -> bool {
        // SAFETY: self.inner は有効。
        unsafe { (*self.inner).font.create(font) }
    }

    /// テキストの描画サイズを計測する。
    /// 原実装 `CPseudoOSD::CalcTextSize`(PseudoOSD.cpp:351-410)。
    ///
    /// `size` は入出力:複数行(`MULTI_LINE`)では入力の `cx` が折り返し幅として
    /// 使われる。テキストが空なら `(0, 0)` で `true`。
    /// 非レイヤード時は `DrawText` の `DT_CALCRECT`、レイヤード時は GDI+
    /// (`Canvas::get_text_size` / `get_outline_text_size`)で計測する。
    /// ウィンドウ未生成時は `CreateCompatibleDC(None)` の DC を使う。
    pub fn calc_text_size(&mut self, size: &mut SIZE) -> bool {
        // SAFETY: self.inner は有効。
        unsafe { osd_calc_text_size(self.inner, size) }
    }

    /// 画像を設定する。原実装 `CPseudoOSD::SetImage`(PseudoOSD.cpp:413-437)。
    ///
    /// テキストとアイコンはクリアされる。`hbm` は表示中は有効な GDI
    /// ビットマップであり続けること(所有権は移らない)。
    pub fn set_image(&mut self, hbm: HBITMAP, effect: ImageEffect, flags: ImageFlag) -> bool {
        // SAFETY: self.inner は有効。Win32 呼び出しが無いため再入しない。
        unsafe {
            (*self.inner).hbm = hbm;
            (*self.inner).text.clear();
            (*self.inner).hbm_icon = HBITMAP::default();
            (*self.inner).image_effect = effect;
            (*self.inner).image_flags = flags;
        }
        true
    }

    /// 親ウィンドウ移動時に呼び、ポップアップレイヤードウィンドウを追従させる。
    /// 原実装 `CPseudoOSD::OnParentMove`(PseudoOSD.cpp:440-458)。
    pub fn on_parent_move(&mut self) {
        // SAFETY: self.inner は有効。
        unsafe { osd_on_parent_move(self.inner) }
    }
}

impl Default for PseudoOsd {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for PseudoOsd {
    /// 原実装 `~CPseudoOSD`(PseudoOSD.cpp:98-101)。ウィンドウを破棄する。
    fn drop(&mut self) {
        // SAFETY: self.inner は Box::into_raw で得たポインタで、ここが唯一の解放点。
        // DestroyWindow → WM_DESTROY は Inner 解放前に完了する(同一スレッド)。
        unsafe {
            osd_destroy(self.inner);
            drop(Box::from_raw(self.inner));
        }
    }
}

// ---------------------------------------------------------------------------
// 内部実装(*mut Inner を取る関数群)
// ---------------------------------------------------------------------------
//
// WndProc からの再入(例: MoveWindow → WM_SIZE → UpdateLayeredWindow)があるため、
// Inner への長生きする `&mut` を作らず、生ポインタ経由でフィールドを操作する。
// 各関数の Safety 条件: `inner` は生存中の `Inner` を指し、呼び出しスレッドは
// ウィンドウを作成したスレッドであること。

/// 原実装 `CPseudoOSD::Create`(PseudoOSD.cpp:104-142)。
///
/// # Safety
///
/// `inner` は生存中の `Inner` を指すこと。
unsafe fn osd_create(inner: *mut Inner, hwnd_parent: HWND, layered_window: bool) -> bool {
    let hwnd = (*inner).hwnd;
    if !hwnd.0.is_null() {
        if GetParent(hwnd).map(|p| p == hwnd_parent).unwrap_or(false)
            && (*inner).layered_window == layered_window
        {
            return true;
        }
        osd_destroy(inner);
    }

    (*inner).layered_window = layered_window;
    // Windows 8 以降はレイヤードの子ウィンドウが使えるため常に false になるが、
    // 原実装(PseudoOSD.cpp:114-115)どおり判定を移植する。
    (*inner).popup_layered_window = layered_window && !is_windows_8_or_later();
    (*inner).hwnd_parent = hwnd_parent;

    let hinst: HINSTANCE = match GetModuleHandleW(None) {
        Ok(hmodule) => hmodule.into(),
        Err(_) => return false,
    };
    let lpparam: Option<*const c_void> = Some(inner as *const c_void);

    if (*inner).popup_layered_window {
        let mut pt = POINT {
            x: (*inner).position.left,
            y: (*inner).position.top,
        };
        let _ = ClientToScreen(hwnd_parent, &mut pt);

        if CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE,
            WINDOW_CLASS_NAME,
            PCWSTR::null(),
            WS_POPUP,
            pt.x,
            pt.y,
            (*inner).position.width,
            (*inner).position.height,
            Some(hwnd_parent),
            None,
            Some(hinst),
            lpparam,
        )
        .is_err()
        {
            return false;
        }

        let mut rc = RECT::default();
        let _ = GetWindowRect(hwnd_parent, &mut rc);
        (*inner).parent_position.x = rc.left;
        (*inner).parent_position.y = rc.top;
        return true;
    }

    CreateWindowExW(
        if layered_window {
            WS_EX_LAYERED | WS_EX_TRANSPARENT
        } else {
            WINDOW_EX_STYLE(0)
        },
        WINDOW_CLASS_NAME,
        PCWSTR::null(),
        WS_CHILD,
        (*inner).position.left,
        (*inner).position.top,
        (*inner).position.width,
        (*inner).position.height,
        Some(hwnd_parent),
        None,
        Some(hinst),
        lpparam,
    )
    .is_ok()
}

/// 原実装 `CPseudoOSD::Destroy`(PseudoOSD.cpp:145-150)。
///
/// # Safety
///
/// `inner` は生存中の `Inner` を指すこと。
unsafe fn osd_destroy(inner: *mut Inner) -> bool {
    let hwnd = (*inner).hwnd;
    if !hwnd.0.is_null() {
        let _ = DestroyWindow(hwnd);
    }
    true
}

/// 原実装 `CPseudoOSD::Show`(PseudoOSD.cpp:159-224)。
///
/// # Safety
///
/// `inner` は生存中の `Inner` を指すこと。
unsafe fn osd_show(inner: *mut Inner, time: u32, animation: bool) -> bool {
    let hwnd = (*inner).hwnd;
    if hwnd.0.is_null() {
        return false;
    }

    if (*inner).popup_layered_window {
        if time > 0 {
            let mut pt = POINT {
                x: (*inner).position.left,
                y: (*inner).position.top,
            };
            let _ = ClientToScreen((*inner).hwnd_parent, &mut pt);
            (*inner).timer.begin_timer(TIMER_ID_HIDE, time);
            if animation {
                (*inner).animation_count = 0;
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    pt.x,
                    pt.y,
                    (*inner).position.width / ANIMATION_FRAMES,
                    (*inner).position.height,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                (*inner).timer.begin_timer(TIMER_ID_ANIMATION, ANIMATION_INTERVAL);
            } else {
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    pt.x,
                    pt.y,
                    (*inner).position.width,
                    (*inner).position.height,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
        } else {
            (*inner).timer.end_timer(TIMER_ID_HIDE);
        }
        update_layered_window_content(inner);
        let _ = ShowWindow(hwnd, SW_SHOWNA);
        let _ = UpdateWindow(hwnd);
        return true;
    }

    if time > 0 {
        (*inner).timer.begin_timer(TIMER_ID_HIDE, time);
        if animation {
            (*inner).animation_count = 0;
            let _ = MoveWindow(
                hwnd,
                (*inner).position.left,
                (*inner).position.top,
                (*inner).position.width / ANIMATION_FRAMES,
                (*inner).position.height,
                true,
            );
            (*inner).timer.begin_timer(TIMER_ID_ANIMATION, ANIMATION_INTERVAL);
        } else {
            let _ = MoveWindow(
                hwnd,
                (*inner).position.left,
                (*inner).position.top,
                (*inner).position.width,
                (*inner).position.height,
                true,
            );
        }
    } else {
        (*inner).timer.end_timer(TIMER_ID_HIDE);
    }
    if (*inner).layered_window {
        update_layered_window_content(inner);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = BringWindowToTop(hwnd);
        let _ = UpdateWindow(hwnd);
    } else if IsWindowVisible(hwnd).as_bool() {
        let _ = RedrawWindow(Some(hwnd), None, None, RDW_INVALIDATE | RDW_UPDATENOW);
    } else {
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = BringWindowToTop(hwnd);
        let _ = UpdateWindow(hwnd);
    }

    true
}

/// 原実装 `CPseudoOSD::Hide`(PseudoOSD.cpp:227-235)。
///
/// # Safety
///
/// `inner` は生存中の `Inner` を指すこと。
unsafe fn osd_hide(inner: *mut Inner) -> bool {
    let hwnd = (*inner).hwnd;
    if hwnd.0.is_null() {
        return false;
    }
    let _ = ShowWindow(hwnd, SW_HIDE);
    (*inner).text.clear();
    (*inner).hbm = HBITMAP::default();
    true
}

/// 原実装 `CPseudoOSD::Update`(PseudoOSD.cpp:247-257)。
///
/// # Safety
///
/// `inner` は生存中の `Inner` を指すこと。
unsafe fn osd_update(inner: *mut Inner) -> bool {
    let hwnd = (*inner).hwnd;
    if !hwnd.0.is_null() {
        if (*inner).layered_window {
            update_layered_window_content(inner);
        } else {
            let _ = RedrawWindow(Some(hwnd), None, None, RDW_INVALIDATE | RDW_UPDATENOW);
        }
    }
    true
}

/// 原実装 `CPseudoOSD::SetPosition`(PseudoOSD.cpp:277-300)。
///
/// # Safety
///
/// `inner` は生存中の `Inner` を指すこと。
unsafe fn osd_set_position(inner: *mut Inner, left: i32, top: i32, width: i32, height: i32) -> bool {
    if width <= 0 || height <= 0 {
        return false;
    }

    (*inner).position = Position {
        left,
        top,
        width,
        height,
    };

    let hwnd = (*inner).hwnd;
    if !hwnd.0.is_null() {
        if (*inner).popup_layered_window {
            let mut pt = POINT { x: left, y: top };
            let _ = ClientToScreen((*inner).hwnd_parent, &mut pt);
            let _ = SetWindowPos(
                hwnd,
                None,
                pt.x,
                pt.y,
                width,
                height,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        } else {
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOP),
                left,
                top,
                width,
                height,
                SET_WINDOW_POS_FLAGS(0),
            );
        }
    }

    true
}

/// 原実装 `CPseudoOSD::CalcTextSize`(PseudoOSD.cpp:351-410)。
///
/// # Safety
///
/// `inner` は生存中の `Inner` を指すこと。
unsafe fn osd_calc_text_size(inner: *mut Inner, size: &mut SIZE) -> bool {
    if (*inner).text.is_empty() {
        size.cx = 0;
        size.cy = 0;
        return true;
    }

    let hwnd = (*inner).hwnd;
    let hdc = if !hwnd.0.is_null() {
        GetDC(Some(hwnd))
    } else {
        CreateCompatibleDC(None)
    };

    let result;
    if !(*inner).layered_window {
        let hfont_old = SelectObject(hdc, HGDIOBJ((*inner).font.handle().0));
        let mut rc = RECT {
            left: 0,
            top: 0,
            right: size.cx,
            bottom: 0,
        };
        let mut format = DT_CALCRECT | DT_NOPREFIX;
        if (*inner).text_style.contains(TextStyle::MULTI_LINE) {
            format |= DT_WORDBREAK;
        } else {
            format |= DT_SINGLELINE;
        }
        // DrawTextW は &mut [u16] を要求する(DT_MODIFYSTRING なしでは変更されない)。
        let mut text = (*inner).text.clone();
        result = DrawTextW(hdc, &mut text, &mut rc, format) != 0;
        if result {
            size.cx = rc.right;
            size.cy = rc.bottom;
        } else {
            size.cx = 0;
            size.cy = 0;
        }
        let _ = SelectObject(hdc, hfont_old);
    } else {
        // Canvas は DC より先に破棄する(スコープで保証)。
        let mut canvas = Canvas::from_hdc(hdc);
        let lf = (*inner).font.get_log_font().unwrap_or_default();
        let font = GpFont::from_logfont(&lf);

        let mut text_flags = TextFlag::DRAW_ANTIALIAS | TextFlag::DRAW_HINTING;
        if !(*inner).text_style.contains(TextStyle::MULTI_LINE) {
            text_flags |= TextFlag::FORMAT_NO_WRAP;
        }

        if (*inner).text_style.contains(TextStyle::OUTLINE) {
            result = canvas.get_outline_text_size(
                &(*inner).text,
                &font,
                get_outline_width(lf.lfHeight.abs()),
                text_flags,
                size,
            );
        } else {
            result = canvas.get_text_size(&(*inner).text, &font, text_flags, size);
        }
    }

    if !hwnd.0.is_null() {
        let _ = ReleaseDC(Some(hwnd), hdc);
    } else {
        let _ = DeleteDC(hdc);
    }

    result
}

/// 原実装 `CPseudoOSD::OnParentMove`(PseudoOSD.cpp:440-458)。
///
/// # Safety
///
/// `inner` は生存中の `Inner` を指すこと。
unsafe fn osd_on_parent_move(inner: *mut Inner) {
    let hwnd = (*inner).hwnd;
    if !hwnd.0.is_null() && (*inner).popup_layered_window {
        let mut rc_parent = RECT::default();
        let mut rc = RECT::default();
        let _ = GetWindowRect((*inner).hwnd_parent, &mut rc_parent);
        let _ = GetWindowRect(hwnd, &mut rc);
        let _ = OffsetRect(
            &mut rc,
            rc_parent.left - (*inner).parent_position.x,
            rc_parent.top - (*inner).parent_position.y,
        );
        let _ = SetWindowPos(
            hwnd,
            None,
            rc.left,
            rc.top,
            rc.right - rc.left,
            rc.bottom - rc.top,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
        (*inner).parent_position.x = rc_parent.left;
        (*inner).parent_position.y = rc_parent.top;
    }
}

/// アニメーション中は段階に応じて縮めたアイコン幅を返す(PseudoOSD.cpp:471-475)。
///
/// # Safety
///
/// `inner` は生存中の `Inner` を指すこと。
unsafe fn current_icon_width(inner: *mut Inner) -> i32 {
    if (*inner).timer.is_timer_enabled(TIMER_ID_ANIMATION) {
        (*inner).icon_width * ((*inner).animation_count + 1) / ANIMATION_FRAMES
    } else {
        (*inner).icon_width
    }
}

/// 非レイヤードウィンドウの描画。原実装 `CPseudoOSD::Draw`(PseudoOSD.cpp:461-530)。
///
/// # Safety
///
/// `inner` は生存中の `Inner` を指し、`hdc` は有効な DC であること。
unsafe fn osd_draw(inner: *mut Inner, hdc: HDC, paint_rect: &RECT) {
    let hwnd = (*inner).hwnd;
    let mut rc = RECT::default();
    let _ = GetClientRect(hwnd, &mut rc);

    if !(*inner).text.is_empty() {
        fill(hdc, paint_rect, (*inner).back_color);

        if !(*inner).hbm_icon.0.is_null() {
            let icon_width = current_icon_width(inner);
            draw_bitmap(
                hdc,
                0,
                (rc.bottom - (*inner).icon_height) / 2,
                icon_width,
                (*inner).icon_height,
                (*inner).hbm_icon,
                None,
                255,
            );
            let icon_top = (rc.bottom - (*inner).icon_height) / 2;
            let rc_icon = RECT {
                left: 0,
                top: icon_top,
                right: icon_width,
                bottom: icon_top + (*inner).icon_height,
            };
            draw_image_effect_hdc(inner, hdc, &rc_icon);
            rc.left += icon_width;
        }

        let hfont_old = SelectObject(hdc, HGDIOBJ((*inner).font.handle().0));
        let old_text_color = SetTextColor(hdc, COLORREF((*inner).text_color));
        let old_bk_mode = SetBkMode(hdc, TRANSPARENT);

        let mut format = DT_NOPREFIX;
        if (*inner).text_style.contains(TextStyle::MULTI_LINE) {
            format |= DT_WORDBREAK;
            if (*inner).timer.is_timer_enabled(TIMER_ID_ANIMATION) {
                rc.right = rc.left + (*inner).position.width - (*inner).icon_width;
            }
        } else {
            format |= DT_SINGLELINE;
        }
        let horz_align = (*inner).text_style & TextStyle::HORZ_ALIGN_MASK;
        if horz_align == TextStyle::RIGHT {
            format |= DT_RIGHT;
        } else if horz_align == TextStyle::HORZ_CENTER {
            format |= DT_CENTER;
        }
        let vert_align = (*inner).text_style & TextStyle::VERT_ALIGN_MASK;
        if vert_align == TextStyle::BOTTOM || vert_align == TextStyle::VERT_CENTER {
            let mut rc_text = RECT {
                left: 0,
                top: 0,
                right: rc.right - rc.left,
                bottom: 0,
            };
            let mut text = (*inner).text.clone();
            let _ = DrawTextW(hdc, &mut text, &mut rc_text, format | DT_CALCRECT);
            if rc_text.bottom < rc.bottom - rc.top {
                if vert_align == TextStyle::BOTTOM {
                    rc.top = rc.bottom - rc_text.bottom;
                } else {
                    rc.top += ((rc.bottom - rc.top) - rc_text.bottom) / 2;
                }
            }
        }

        let mut text = (*inner).text.clone();
        let _ = DrawTextW(hdc, &mut text, &mut rc, format);

        let _ = SetBkMode(hdc, BACKGROUND_MODE(old_bk_mode as u32));
        let _ = SetTextColor(hdc, old_text_color);
        let _ = SelectObject(hdc, hfont_old);
    } else if !(*inner).hbm.0.is_null() {
        let mut bm = BITMAP::default();
        let _ = GetObjectW(
            HGDIOBJ((*inner).hbm.0),
            std::mem::size_of::<BITMAP>() as i32,
            Some(std::ptr::from_mut(&mut bm).cast()),
        );
        let rc_bitmap = RECT {
            left: 0,
            top: 0,
            right: bm.bmWidth,
            bottom: bm.bmHeight,
        };
        draw_bitmap(
            hdc,
            0,
            0,
            rc.right,
            rc.bottom,
            (*inner).hbm,
            Some(&rc_bitmap),
            255,
        );
        draw_image_effect_hdc(inner, hdc, &rc);
    }
}

/// 画像効果(HDC 版)。原実装 `CPseudoOSD::DrawImageEffect`(PseudoOSD.cpp:533-539)。
///
/// `Gloss` は `DrawUtil::GlossOverlay` の既定値(192, 32, 32, 0)、
/// `Dark` は黒の不透明度 64 の `ColorOverlay`。
///
/// # Safety
///
/// `inner` は生存中の `Inner` を指し、`hdc` は有効な DC であること。
unsafe fn draw_image_effect_hdc(inner: *mut Inner, hdc: HDC, rect: &RECT) {
    if (*inner).image_effect.contains(ImageEffect::GLOSS) {
        gloss_overlay(hdc, rect, 192, 32, 32, 0);
    }
    if (*inner).image_effect.contains(ImageEffect::DARK) {
        color_overlay(hdc, rect, rgb(0, 0, 0), 64);
    }
}

/// 画像効果(Canvas 版)。原実装 `CPseudoOSD::DrawImageEffect`(PseudoOSD.cpp:542-563)。
///
/// `Gloss` は上半分に白(192→32)、下半分に黒(32→0)の垂直グラデーション、
/// `Dark` は黒(アルファ 64)の塗りつぶし。
///
/// # Safety
///
/// `inner` は生存中の `Inner` を指すこと。
unsafe fn draw_image_effect_canvas(inner: *mut Inner, canvas: &mut Canvas, rect: &RECT) {
    if (*inner).image_effect.contains(ImageEffect::GLOSS) {
        let mut r = *rect;
        r.bottom = (r.top + r.bottom) / 2;
        canvas.fill_gradient(
            Color::new(255, 255, 255, 192),
            Color::new(255, 255, 255, 32),
            &r,
            GradientDirection::Vert,
        );
        r.top = r.bottom;
        r.bottom = rect.bottom;
        canvas.fill_gradient(
            Color::new(0, 0, 0, 32),
            Color::new(0, 0, 0, 0),
            &r,
            GradientDirection::Vert,
        );
    }

    if (*inner).image_effect.contains(ImageEffect::DARK) {
        let brush = Brush::from_rgba(0, 0, 0, 64);
        canvas.fill_rect(&brush, rect);
    }
}

/// レイヤードウィンドウの内容を更新する。
/// 原実装 `CPseudoOSD::UpdateLayeredWindow`(PseudoOSD.cpp:566-685)。
///
/// `DirectSource` 画像はそのまま転送元 DC に選択し、それ以外は 32bpp の
/// GDI+ 画像へ Canvas で描画して HBITMAP 化したものを使い、
/// `::UpdateLayeredWindow`(`ULW_ALPHA`、ピクセルアルファ)で反映する。
///
/// # Safety
///
/// `inner` は生存中の `Inner` を指すこと。
unsafe fn update_layered_window_content(inner: *mut Inner) {
    let hwnd = (*inner).hwnd;
    let mut rc_window = RECT::default();
    let _ = GetWindowRect(hwnd, &mut rc_window);
    let width = rc_window.right - rc_window.left;
    let height = rc_window.bottom - rc_window.top;
    if width < 1 || height < 1 {
        return;
    }

    let hdc = GetDC(Some(hwnd));
    let hdc_src = CreateCompatibleDC(Some(hdc));

    // DrawUtil::CBitmap Bitmap(PseudoOSD.cpp:577)相当。スコープ終端で解放される。
    let mut bitmap = GdiBitmap::new();
    let hbm_old;

    if !(*inner).hbm.0.is_null() && (*inner).image_flags.contains(ImageFlag::DIRECT_SOURCE) {
        hbm_old = SelectObject(hdc_src, HGDIOBJ((*inner).hbm.0));
    } else {
        let mut canvas_image = Image::new();
        if !canvas_image.create(width, height, 32) {
            // 原実装(PseudoOSD.cpp:585-586)は DC を解放せず return する(リーク)が、
            // 本移植では解放してから戻る。
            let _ = DeleteDC(hdc_src);
            let _ = ReleaseDC(Some(hwnd), hdc);
            return;
        }
        canvas_image.clear();

        {
            let mut canvas = Canvas::from_image(&mut canvas_image);

            let mut rc = RECT {
                left: 0,
                top: 0,
                right: width,
                bottom: height,
            };

            if !(*inner).text.is_empty() {
                // アイコン画像。Canvas 使用中は生存させる(create_from_bitmap は
                // ピクセルデータを参照し続けるため)。
                let mut icon_image = Image::new();
                if !(*inner).hbm_icon.0.is_null() {
                    // SAFETY: hbm_icon は呼び出し側が表示中有効性を保証する GDI
                    // ビットマップ(set_text の契約)。icon_image はこのスコープ内
                    // でのみ使用する。
                    let _ = icon_image.create_from_bitmap((*inner).hbm_icon, HPALETTE::default());
                    let icon_width = current_icon_width(inner);
                    canvas.draw_image_rect(
                        0,
                        (height - (*inner).icon_height) / 2,
                        icon_width,
                        (*inner).icon_height,
                        &icon_image,
                        0,
                        0,
                        (*inner).icon_width,
                        (*inner).icon_height,
                        1.0,
                    );
                    let icon_top = (height - (*inner).icon_height) / 2;
                    let mut rc_icon = RECT {
                        left: 0,
                        top: icon_top,
                        right: icon_width,
                        bottom: icon_top + (*inner).icon_height,
                    };
                    if rc_icon.top < 0 {
                        rc_icon.top = 0;
                    }
                    if rc_icon.right > width {
                        rc_icon.right = width;
                    }
                    if rc_icon.bottom > height {
                        rc_icon.bottom = height;
                    }
                    draw_image_effect_canvas(inner, &mut canvas, &rc_icon);
                    rc.left += icon_width;
                }

                if (*inner).text_style.contains(TextStyle::FILL_BACKGROUND) {
                    let back_brush = Brush::from_rgba(0, 0, 0, 128);
                    canvas.fill_rect(&back_brush, &rc);
                }

                let text_color = Color::from_colorref((*inner).text_color);
                let text_brush = Brush::from_rgba(
                    text_color.red,
                    text_color.green,
                    text_color.blue,
                    255,
                );
                let lf = (*inner).font.get_log_font().unwrap_or_default();
                let font = GpFont::from_logfont(&lf);

                let mut draw_text_flags = TextFlag::DRAW_ANTIALIAS | TextFlag::DRAW_HINTING;
                if (*inner).text_style.contains(TextStyle::RIGHT) {
                    draw_text_flags |= TextFlag::FORMAT_RIGHT;
                } else if (*inner).text_style.contains(TextStyle::HORZ_CENTER) {
                    draw_text_flags |= TextFlag::FORMAT_HORZ_CENTER;
                }
                if (*inner).text_style.contains(TextStyle::BOTTOM) {
                    draw_text_flags |= TextFlag::FORMAT_BOTTOM;
                }
                if (*inner).text_style.contains(TextStyle::VERT_CENTER) {
                    draw_text_flags |= TextFlag::FORMAT_VERT_CENTER;
                }
                if !(*inner).text_style.contains(TextStyle::MULTI_LINE) {
                    draw_text_flags |= TextFlag::FORMAT_NO_WRAP;
                }

                if (*inner).text_style.contains(TextStyle::MULTI_LINE)
                    && (*inner).timer.is_timer_enabled(TIMER_ID_ANIMATION)
                {
                    rc.right = rc.left + (*inner).position.width - (*inner).icon_width;
                }

                if (*inner).text_style.contains(TextStyle::OUTLINE) {
                    canvas.draw_outline_text(
                        &(*inner).text,
                        &font,
                        &rc,
                        &text_brush,
                        Color::new(0, 0, 0, 160),
                        get_outline_width(lf.lfHeight.abs()),
                        draw_text_flags,
                    );
                } else {
                    canvas.draw_text(&(*inner).text, &font, &rc, &text_brush, draw_text_flags);
                }
            } else if !(*inner).hbm.0.is_null() {
                let mut image = Image::new();
                // SAFETY: hbm は呼び出し側が表示中有効性を保証する GDI ビットマップ
                // (set_image の契約)。image はこのスコープ内でのみ使用する。
                if image.create_from_bitmap((*inner).hbm, HPALETTE::default()) {
                    canvas.draw_image(&image, 0, 0);
                    let rc_image = RECT {
                        left: 0,
                        top: 0,
                        right: image.get_width(),
                        bottom: image.get_height(),
                    };
                    draw_image_effect_canvas(inner, &mut canvas, &rc_image);
                }
            }
        }

        bitmap.attach(canvas_image.create_hbitmap());
        hbm_old = SelectObject(hdc_src, HGDIOBJ(bitmap.handle().0));
    }

    let sz = SIZE {
        cx: width,
        cy: height,
    };
    let pt_src = POINT { x: 0, y: 0 };
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };
    let _ = UpdateLayeredWindow(
        hwnd,
        Some(hdc),
        None,
        Some(&sz),
        Some(hdc_src),
        Some(&pt_src),
        COLORREF(0),
        Some(&blend),
        ULW_ALPHA,
    );

    let _ = SelectObject(hdc_src, hbm_old);
    let _ = DeleteDC(hdc_src);
    let _ = ReleaseDC(Some(hwnd), hdc);
}

// ---------------------------------------------------------------------------
// ウィンドウプロシージャ(PseudoOSD.cpp:688-803)
// ---------------------------------------------------------------------------

/// `GWLP_USERDATA` から `Inner` を復元する。
/// 原実装 `CPseudoOSD::GetThis`(PseudoOSD.cpp:688-691)。
///
/// # Safety
///
/// `hwnd` は有効なウィンドウハンドルであること。WM_CREATE 前は null が返る。
unsafe fn get_this(hwnd: HWND) -> *mut Inner {
    GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Inner
}

/// `GET_X_LPARAM` 相当。
fn get_x_lparam(lparam: LPARAM) -> i32 {
    (lparam.0 & 0xFFFF) as u16 as i16 as i32
}

/// `GET_Y_LPARAM` 相当。
fn get_y_lparam(lparam: LPARAM) -> i32 {
    ((lparam.0 >> 16) & 0xFFFF) as u16 as i16 as i32
}

/// `MAKELPARAM` 相当(下位 16bit = x、上位 16bit = y。DWORD 経由でゼロ拡張)。
fn make_lparam(x: i32, y: i32) -> LPARAM {
    LPARAM(((((y as u16 as u32) << 16) | (x as u16 as u32)) as usize) as isize)
}

/// ウィンドウプロシージャ。原実装 `CPseudoOSD::WndProc`(PseudoOSD.cpp:694-803)。
///
/// # Safety
///
/// ウィンドウクラス登録経由でシステムから呼ばれる。`WM_CREATE` の
/// `lpCreateParams` には `osd_create` が渡した `*mut Inner` が入っており、
/// `WM_DESTROY` まで生存する(`PseudoOsd::drop` は DestroyWindow 完了後に
/// `Inner` を解放する)。
unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        // PseudoOSD.cpp:697-707
        WM_CREATE => {
            let cs = &*(lparam.0 as *const CREATESTRUCTW);
            let inner = cs.lpCreateParams as *mut Inner;
            (*inner).hwnd = hwnd;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, inner as isize);
            (*inner).timer.initialize_timer(hwnd);
            LRESULT(0)
        }

        // PseudoOSD.cpp:709-716
        WM_SIZE => {
            let inner = get_this(hwnd);
            if !inner.is_null() && (*inner).layered_window && IsWindowVisible(hwnd).as_bool() {
                update_layered_window_content(inner);
            }
            LRESULT(0)
        }

        // PseudoOSD.cpp:718-728
        WM_PAINT => {
            let inner = get_this(hwnd);
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            if !inner.is_null() && !(*inner).layered_window {
                osd_draw(inner, hdc, &ps.rcPaint);
            }
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }

        // PseudoOSD.cpp:730-766
        WM_TIMER => {
            let inner = get_this(hwnd);
            if !inner.is_null() {
                match wparam.0 as u32 {
                    TIMER_ID_HIDE => {
                        osd_hide(inner);
                        (*inner).timer.end_timer(TIMER_ID_HIDE);
                        (*inner).timer.end_timer(TIMER_ID_ANIMATION);
                    }
                    TIMER_ID_ANIMATION => {
                        (*inner).animation_count += 1;
                        if (*inner).popup_layered_window {
                            let mut rc = RECT::default();
                            let _ = GetWindowRect(hwnd, &mut rc);
                            let _ = SetWindowPos(
                                hwnd,
                                None,
                                rc.left,
                                rc.top,
                                (*inner).position.width * ((*inner).animation_count + 1)
                                    / ANIMATION_FRAMES,
                                (*inner).position.height,
                                SWP_NOZORDER | SWP_NOACTIVATE,
                            );
                        } else {
                            let _ = MoveWindow(
                                hwnd,
                                (*inner).position.left,
                                (*inner).position.top,
                                (*inner).position.width * ((*inner).animation_count + 1)
                                    / ANIMATION_FRAMES,
                                (*inner).position.height,
                                true,
                            );
                        }
                        let _ = UpdateWindow(hwnd);
                        if (*inner).animation_count + 1 == ANIMATION_FRAMES {
                            (*inner).timer.end_timer(TIMER_ID_ANIMATION);
                        }
                    }
                    _ => {}
                }
            }
            LRESULT(0)
        }

        // PseudoOSD.cpp:768-788: マウスメッセージは親のクライアント領域内なら
        // 座標を親のものに変換して転送する。
        WM_LBUTTONDOWN | WM_LBUTTONUP | WM_LBUTTONDBLCLK | WM_RBUTTONDOWN | WM_RBUTTONUP
        | WM_RBUTTONDBLCLK | WM_MBUTTONDOWN | WM_MBUTTONUP | WM_MBUTTONDBLCLK | WM_MOUSEMOVE => {
            let inner = get_this(hwnd);
            if !inner.is_null() {
                let mut pts = [POINT {
                    x: get_x_lparam(lparam),
                    y: get_y_lparam(lparam),
                }];
                let _ = MapWindowPoints(Some(hwnd), Some((*inner).hwnd_parent), &mut pts);
                let mut rc = RECT::default();
                let _ = GetClientRect((*inner).hwnd_parent, &mut rc);
                if PtInRect(&rc, pts[0]).as_bool() {
                    return SendMessageW(
                        (*inner).hwnd_parent,
                        msg,
                        Some(wparam),
                        Some(make_lparam(pts[0].x, pts[0].y)),
                    );
                }
            }
            LRESULT(0)
        }

        // PseudoOSD.cpp:790-791
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),

        // PseudoOSD.cpp:793-799
        WM_DESTROY => {
            let inner = get_this(hwnd);
            if !inner.is_null() {
                (*inner).hwnd = HWND::default();
            }
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

// ---------------------------------------------------------------------------
// テスト
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::OnceLock;

    use windows::core::w;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, WS_OVERLAPPEDWINDOW,
    };

    use tvtest_graphics::GraphicsCore;

    /// テスト全体で一度だけ GDI+ を初期化する(graphics クレートの流儀)。
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

    fn utf16(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    /// テスト用の非表示トップレベルウィンドウ(aero クレートのテストの流儀)。
    /// 疑似 OSD は WS_CHILD で作られるため、メッセージオンリーではなく
    /// 通常のオーバーラップウィンドウを親にする。
    fn create_parent() -> HWND {
        unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("tvtest_pseudo_osd test parent"),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                320,
                240,
                None,
                None,
                None,
                None,
            )
            .expect("hidden parent window should be created")
        }
    }

    fn destroy_parent(hwnd: HWND) {
        unsafe {
            DestroyWindow(hwnd).expect("parent window should be destroyed");
        }
    }

    fn osd_hwnd(osd: &PseudoOsd) -> HWND {
        unsafe { (*osd.inner).hwnd }
    }

    fn animation_timer_enabled(osd: &PseudoOsd) -> bool {
        unsafe { (*osd.inner).timer.is_timer_enabled(TIMER_ID_ANIMATION) }
    }

    fn hide_timer_enabled(osd: &PseudoOsd) -> bool {
        unsafe { (*osd.inner).timer.is_timer_enabled(TIMER_ID_HIDE) }
    }

    fn window_width(hwnd: HWND) -> i32 {
        let mut rc = RECT::default();
        unsafe {
            GetWindowRect(hwnd, &mut rc).expect("GetWindowRect should succeed");
        }
        rc.right - rc.left
    }

    fn send_timer(hwnd: HWND, id: u32) {
        unsafe {
            let _ = SendMessageW(hwnd, WM_TIMER, Some(WPARAM(id as usize)), None);
        }
    }

    include!(r"C:\Users\h-mineta\AppData\Local\Temp\claude\d--workspace-TVTest-with-Rust\9d281cfe-3446-4612-96a5-e5484a6a9ccd\scratchpad\dbg_layered.rs");

    // ---- フラグ値(PseudoOSD.h:36-63)----

    #[test]
    fn text_style_bits_match_original() {
        assert_eq!(TextStyle::NONE.bits(), 0x0000);
        assert_eq!(TextStyle::LEFT.bits(), 0x0000);
        assert_eq!(TextStyle::RIGHT.bits(), 0x0001);
        assert_eq!(TextStyle::HORZ_CENTER.bits(), 0x0002);
        assert_eq!(TextStyle::HORZ_ALIGN_MASK.bits(), 0x0003);
        assert_eq!(TextStyle::TOP.bits(), 0x0000);
        assert_eq!(TextStyle::BOTTOM.bits(), 0x0004);
        assert_eq!(TextStyle::VERT_CENTER.bits(), 0x0008);
        assert_eq!(TextStyle::VERT_ALIGN_MASK.bits(), 0x000C);
        assert_eq!(TextStyle::OUTLINE.bits(), 0x0010);
        assert_eq!(TextStyle::FILL_BACKGROUND.bits(), 0x0020);
        assert_eq!(TextStyle::MULTI_LINE.bits(), 0x0040);
        assert_eq!(ImageFlag::NONE.bits(), 0x0000);
        assert_eq!(ImageFlag::DIRECT_SOURCE.bits(), 0x0001);
        assert_eq!(ImageEffect::NONE.bits(), 0x0000);
        assert_eq!(ImageEffect::GLOSS.bits(), 0x0001);
        assert_eq!(ImageEffect::DARK.bits(), 0x0002);
    }

    // ---- クラス名比較ヘルパ ----

    #[test]
    fn wide_eq_ascii_ci_compares_case_insensitively() {
        let a = utf16("TVTest Pseudo OSD");
        let b = utf16("tvtest pseudo osd");
        let c = utf16("TVTest Pseudo OS_");
        assert!(wide_eq_ascii_ci(&a, &b));
        assert!(!wide_eq_ascii_ci(&a, &c));
        assert!(!wide_eq_ascii_ci(&a, &a[..5]));
    }

    // ---- Initialize / IsPseudoOSD(PseudoOSD.cpp:58-87)----

    #[test]
    fn initialize_and_is_pseudo_osd() {
        assert!(initialize());
        // 多重呼び出しでも true(PseudoOSD.cpp:60 のガード)
        assert!(initialize());

        let parent = create_parent();
        let mut osd = PseudoOsd::new();
        assert!(osd.set_position(0, 0, 100, 50));
        assert!(osd.create(parent, false));

        assert!(is_pseudo_osd(osd_hwnd(&osd)));
        assert!(!is_pseudo_osd(parent));
        assert!(!is_pseudo_osd(HWND::default()));

        drop(osd);
        destroy_parent(parent);
    }

    // ---- Create / Destroy / IsCreated(PseudoOSD.cpp:104-156)----

    #[test]
    fn create_recreate_and_destroy() {
        assert!(initialize());
        let parent = create_parent();
        let mut osd = PseudoOsd::new();
        assert!(!osd.is_created());
        assert!(osd.set_position(0, 0, 100, 50));

        assert!(osd.create(parent, false));
        assert!(osd.is_created());
        let hwnd1 = osd_hwnd(&osd);

        // 同じ親・同じ fLayeredWindow なら何もしない(PseudoOSD.cpp:106-109)
        assert!(osd.create(parent, false));
        assert_eq!(osd_hwnd(&osd), hwnd1);

        // fLayeredWindow が違えば作り直し(PseudoOSD.cpp:110)
        assert!(osd.create(parent, true));
        assert!(osd.is_created());
        let hwnd2 = osd_hwnd(&osd);
        assert_ne!(hwnd2, hwnd1);

        assert!(osd.destroy());
        assert!(!osd.is_created());
        // Destroy は未生成でも true(PseudoOSD.cpp:145-150)
        assert!(osd.destroy());

        drop(osd);
        destroy_parent(parent);
    }

    // ---- SetPosition / GetPosition(PseudoOSD.cpp:277-313)----

    #[test]
    fn set_position_and_get_position_round_trip() {
        assert!(initialize());
        let parent = create_parent();
        let mut osd = PseudoOsd::new();

        // ウィンドウ未生成でも位置は保持される
        assert!(osd.set_position(10, 20, 100, 50));
        assert_eq!(osd.get_position(), (10, 20, 100, 50));

        // 幅・高さが 0 以下なら失敗し、位置は変わらない(PseudoOSD.cpp:279-280)
        assert!(!osd.set_position(0, 0, 0, 50));
        assert!(!osd.set_position(0, 0, 100, -1));
        assert_eq!(osd.get_position(), (10, 20, 100, 50));

        // 生成後はウィンドウも移動する
        assert!(osd.create(parent, false));
        assert!(osd.set_position(5, 6, 70, 40));
        assert_eq!(osd.get_position(), (5, 6, 70, 40));
        assert_eq!(window_width(osd_hwnd(&osd)), 70);

        drop(osd);
        destroy_parent(parent);
    }

    // ---- SetTextHeight / SetFont(PseudoOSD.cpp:326-348)----

    #[test]
    fn set_text_height_updates_logfont() {
        let mut osd = PseudoOsd::new();
        assert!(osd.set_text_height(30));
        let lf = unsafe { (*osd.inner).font.get_log_font() }.expect("font should be created");
        assert_eq!(lf.lfHeight, -30);
        assert_eq!(lf.lfWidth, 0);

        // SetFont はそのまま LOGFONT から作り直す
        let mut lf2 = lf;
        lf2.lfHeight = -24;
        assert!(osd.set_font(&lf2));
        let lf3 = unsafe { (*osd.inner).font.get_log_font() }.unwrap();
        assert_eq!(lf3.lfHeight, -24);
    }

    // ---- CalcTextSize(PseudoOSD.cpp:351-410)----

    #[test]
    fn calc_text_size_empty_text_is_zero() {
        let mut osd = PseudoOsd::new();
        let mut size = SIZE { cx: 123, cy: 456 };
        assert!(osd.calc_text_size(&mut size));
        assert_eq!((size.cx, size.cy), (0, 0));
    }

    #[test]
    fn calc_text_size_non_layered_without_window() {
        // hwnd 無し → CreateCompatibleDC(None) 経路(PseudoOSD.cpp:364-365)
        let mut osd = PseudoOsd::new();
        assert!(osd.set_text_height(20));
        assert!(osd.set_text(&utf16("Hello OSD"), HBITMAP::default(), 0, 0, ImageEffect::NONE));
        let mut size = SIZE::default();
        assert!(osd.calc_text_size(&mut size));
        assert!(size.cx > 0, "cx = {}", size.cx);
        assert!(size.cy > 0, "cy = {}", size.cy);

        // 単一行の方が複数行より横に長い(同一テキストの折り返し確認)
        assert!(osd.set_text_style(TextStyle::MULTI_LINE));
        let mut ml_size = SIZE { cx: size.cx / 2, cy: 0 };
        assert!(osd.calc_text_size(&mut ml_size));
        assert!(ml_size.cy >= size.cy, "multi-line should wrap: {ml_size:?}");
    }

    #[test]
    fn calc_text_size_layered_uses_gdiplus() {
        ensure_gdiplus();
        assert!(initialize());
        let parent = create_parent();
        let mut osd = PseudoOsd::new();
        assert!(osd.set_position(0, 0, 200, 60));
        assert!(osd.create(parent, true));
        assert!(osd.set_text_height(20));
        assert!(osd.set_text(&utf16("Layered"), HBITMAP::default(), 0, 0, ImageEffect::NONE));

        // 既定スタイルは Outline → GetOutlineTextSize 経路(PseudoOSD.cpp:395-397)
        let mut outline_size = SIZE::default();
        assert!(osd.calc_text_size(&mut outline_size));
        assert!(outline_size.cx > 0 && outline_size.cy > 0, "{outline_size:?}");

        // Outline なし → GetTextSize 経路(PseudoOSD.cpp:398-401)
        assert!(osd.set_text_style(TextStyle::NONE));
        let mut plain_size = SIZE::default();
        assert!(osd.calc_text_size(&mut plain_size));
        assert!(plain_size.cx > 0 && plain_size.cy > 0, "{plain_size:?}");

        drop(osd);
        destroy_parent(parent);
    }

    // ---- SetText / SetImage(PseudoOSD.cpp:260-274, 413-437)----

    #[test]
    fn set_text_and_set_image_reset_each_other() {
        let mut osd = PseudoOsd::new();
        // ダミーの HBITMAP 値(描画しない限り参照されない)
        let dummy = HBITMAP(0x1234 as *mut c_void);

        assert!(osd.set_text(&utf16("text"), dummy, 16, 16, ImageEffect::GLOSS));
        unsafe {
            assert_eq!((*osd.inner).text, utf16("text"));
            assert_eq!((*osd.inner).hbm_icon, dummy);
            assert_eq!((*osd.inner).icon_width, 16);
            assert_eq!((*osd.inner).image_effect, ImageEffect::GLOSS);
            assert!((*osd.inner).hbm.0.is_null());
        }

        // NUL 以降は切り詰め(LPCTSTR の意味論)
        let mut with_nul = utf16("ab");
        with_nul.push(0);
        with_nul.extend_from_slice(&utf16("cd"));
        assert!(osd.set_text(&with_nul, HBITMAP::default(), 0, 0, ImageEffect::NONE));
        unsafe {
            assert_eq!((*osd.inner).text, utf16("ab"));
            // アイコン無しでは幅・高さが 0 に戻る(PseudoOSD.cpp:268-271)
            assert_eq!((*osd.inner).icon_width, 0);
            assert_eq!((*osd.inner).icon_height, 0);
        }

        // SetImage はテキストとアイコンをクリアする(PseudoOSD.cpp:415-419)
        assert!(osd.set_image(dummy, ImageEffect::DARK, ImageFlag::DIRECT_SOURCE));
        unsafe {
            assert!((*osd.inner).text.is_empty());
            assert!((*osd.inner).hbm_icon.0.is_null());
            assert_eq!((*osd.inner).hbm, dummy);
            assert_eq!((*osd.inner).image_effect, ImageEffect::DARK);
            assert_eq!((*osd.inner).image_flags, ImageFlag::DIRECT_SOURCE);
        }
    }

    // ---- Show のワイプアニメーション(PseudoOSD.cpp:159-224, 741-763)----

    #[test]
    fn show_animation_starts_at_quarter_width_and_expands() {
        assert!(initialize());
        let parent = create_parent();
        let mut osd = PseudoOsd::new();
        assert!(osd.set_position(0, 0, 200, 100));
        assert!(osd.create(parent, false));
        assert!(osd.set_text(&utf16("anim"), HBITMAP::default(), 0, 0, ImageEffect::NONE));

        // 未生成では Show は失敗する(PseudoOSD.cpp:161-162)
        {
            let mut not_created = PseudoOsd::new();
            assert!(!not_created.show(0, false));
        }

        assert!(osd.show(5000, true));
        let hwnd = osd_hwnd(&osd);
        assert!(osd.is_visible());
        assert!(hide_timer_enabled(&osd));
        assert!(animation_timer_enabled(&osd));
        // アニメ開始時は幅 Width/4(PseudoOSD.cpp:195-198)
        assert_eq!(window_width(hwnd), 200 / 4);

        // WM_TIMER を同期発火してワイプの幅遷移を検証(PseudoOSD.cpp:741-763)
        send_timer(hwnd, TIMER_ID_ANIMATION);
        assert_eq!(window_width(hwnd), 200 * 2 / 4);
        assert!(animation_timer_enabled(&osd));

        send_timer(hwnd, TIMER_ID_ANIMATION);
        assert_eq!(window_width(hwnd), 200 * 3 / 4);
        assert!(animation_timer_enabled(&osd));

        send_timer(hwnd, TIMER_ID_ANIMATION);
        assert_eq!(window_width(hwnd), 200);
        // AnimationCount+1 == ANIMATION_FRAMES でタイマー終了(PseudoOSD.cpp:760-762)
        assert!(!animation_timer_enabled(&osd));
        // 自動非表示タイマーは生きている
        assert!(hide_timer_enabled(&osd));

        drop(osd);
        destroy_parent(parent);
    }

    // ---- TIMER_ID_HIDE による自動非表示(PseudoOSD.cpp:735-739)----

    #[test]
    fn hide_timer_hides_window_and_clears_text() {
        assert!(initialize());
        let parent = create_parent();
        let mut osd = PseudoOsd::new();
        assert!(osd.set_position(0, 0, 120, 40));
        assert!(osd.create(parent, false));
        assert!(osd.set_text(&utf16("bye"), HBITMAP::default(), 0, 0, ImageEffect::NONE));

        assert!(osd.show(5000, false));
        assert!(osd.is_visible());
        assert!(hide_timer_enabled(&osd));

        send_timer(osd_hwnd(&osd), TIMER_ID_HIDE);
        assert!(!osd.is_visible());
        unsafe {
            assert!((*osd.inner).text.is_empty());
            assert!((*osd.inner).hbm.0.is_null());
        }
        assert!(!hide_timer_enabled(&osd));
        assert!(!animation_timer_enabled(&osd));

        drop(osd);
        destroy_parent(parent);
    }

    // ---- Show(Time=0)/ Hide / IsVisible(PseudoOSD.cpp:159-244)----

    #[test]
    fn show_without_time_and_hide() {
        assert!(initialize());
        let parent = create_parent();
        let mut osd = PseudoOsd::new();
        assert!(osd.set_position(0, 0, 100, 40));
        assert!(osd.create(parent, false));
        assert!(osd.set_text(&utf16("visible"), HBITMAP::default(), 0, 0, ImageEffect::NONE));

        assert!(!osd.is_visible());
        // Time=0 ではタイマーを使わない(PseudoOSD.cpp:205-207)
        assert!(osd.show(0, false));
        assert!(osd.is_visible());
        assert!(!hide_timer_enabled(&osd));
        // 幅はフルサイズのまま
        assert_eq!(window_width(osd_hwnd(&osd)), 100);

        // 表示中の再 Show は RedrawWindow 経路(PseudoOSD.cpp:214-215)
        assert!(osd.show(0, false));
        assert!(osd.is_visible());

        assert!(osd.hide());
        assert!(!osd.is_visible());
        unsafe {
            assert!((*osd.inner).text.is_empty());
        }

        // Hide は未生成では false(PseudoOSD.cpp:229-230)
        let mut not_created = PseudoOsd::new();
        assert!(!not_created.hide());

        drop(osd);
        destroy_parent(parent);
    }

    // ---- WM_NCHITTEST(PseudoOSD.cpp:790-791)----

    #[test]
    fn nchittest_returns_httransparent() {
        assert!(initialize());
        let parent = create_parent();
        let mut osd = PseudoOsd::new();
        assert!(osd.set_position(0, 0, 80, 30));
        assert!(osd.create(parent, false));

        let result = unsafe {
            SendMessageW(osd_hwnd(&osd), WM_NCHITTEST, Some(WPARAM(0)), Some(LPARAM(0)))
        };
        assert_eq!(result.0, HTTRANSPARENT as isize);

        drop(osd);
        destroy_parent(parent);
    }

    // ---- レイヤードウィンドウの表示・更新(PseudoOSD.cpp:159-224, 566-685)----

    #[test]
    fn layered_show_and_update() {
        ensure_gdiplus();
        assert!(initialize());
        let parent = create_parent();
        let mut osd = PseudoOsd::new();
        assert!(osd.set_position(0, 0, 160, 48));
        assert!(osd.create(parent, true));
        assert!(osd.set_text_height(20));
        assert!(osd.set_text(&utf16("Layered OSD"), HBITMAP::default(), 0, 0, ImageEffect::NONE));

        // UpdateLayeredWindow 経由の表示(PseudoOSD.cpp:208-212)
        assert!(osd.show(0, false));
        assert!(osd.is_visible());

        // Update → UpdateLayeredWindow(PseudoOSD.cpp:249-251)
        assert!(osd.update());

        // 可視状態でのサイズ変更 → WM_SIZE → UpdateLayeredWindow(PseudoOSD.cpp:709-716)
        assert!(osd.set_position(0, 0, 200, 48));
        assert!(osd.is_visible());

        // FillBackground + 中央寄せの描画経路も通す(PseudoOSD.cpp:623-626, 641-646)
        assert!(osd.set_text_style(
            TextStyle::FILL_BACKGROUND | TextStyle::HORZ_CENTER | TextStyle::VERT_CENTER
        ));
        assert!(osd.update());

        drop(osd);
        destroy_parent(parent);
    }

    // ---- Drop でウィンドウが破棄される(PseudoOSD.cpp:98-101)----

    #[test]
    fn drop_destroys_window() {
        assert!(initialize());
        let parent = create_parent();
        let hwnd;
        {
            let mut osd = PseudoOsd::new();
            assert!(osd.set_position(0, 0, 50, 20));
            assert!(osd.create(parent, false));
            hwnd = osd_hwnd(&osd);
            assert!(is_pseudo_osd(hwnd));
        }
        // Drop 後はウィンドウが存在しない
        assert!(!unsafe { IsWindowVisible(hwnd) }.as_bool());
        assert!(!is_pseudo_osd(hwnd));
        destroy_parent(parent);
    }
}
