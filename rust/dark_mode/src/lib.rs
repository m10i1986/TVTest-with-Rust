/*
  TVTest
  Copyright(c) 2008-2022 DBCTRADO

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

//! TVTest のダークモード補助関数群を Rust へ移植したもの。原実装 (`src/DarkMode.cpp`)。
//!
//! 判定ロジックのうちプラットフォーム非依存な部分(色の暗さ判定 [`is_dark_theme_color`]、
//! `WM_SETTINGCHANGE` の対象判定 [`is_dark_mode_setting_name`]、アプリモード選択
//! [`preferred_app_mode_for`]、ウィンドウフレーム属性のフォールバック [`frame_dark_mode_result`])を
//! 純粋関数に切り出して単体テストで検証する。
//!
//! Win32 接続部分は windows-rs で実装する:
//! - 非公開の uxtheme.dll 序数エクスポート(`ShouldAppsUseDarkMode` 等)は原実装の
//!   `GET_MODULE_FUNCTION_ORDINAL`(名前→失敗時に序数で `GetProcAddress`)に倣って解決する。
//! - フレームのダークモードは `DwmSetWindowAttribute`、ハイコントラスト判定は
//!   `SystemParametersInfoW` を用いる。
//!
//! OS バージョン判定は移植済みの [`tvtest_winutil`] を利用する。
//!
//! 原実装ヘッダ (`DarkMode.h`) の `IsDarkThemeStyle`(`Theme::FillStyle` /
//! `Theme::BackgroundStyle` を受け取る inline)は `Theme.h` が未移植のため対象外とし、
//! その土台となる [`is_dark_theme_color`] までを本クレートで提供する。

#![cfg(windows)]

use core::ffi::c_void;
use std::mem::{size_of, transmute};

use windows::core::{s, w, BOOL, PCSTR, PCWSTR};
use windows::Win32::Foundation::{FARPROC, HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWINDOWATTRIBUTE};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
use windows::Win32::UI::Controls::SetWindowTheme;
use windows::Win32::UI::WindowsAndMessaging::{
    IsWindow, SystemParametersInfoW, SPI_GETHIGHCONTRAST, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    WM_SETTINGCHANGE,
};

use tvtest_winutil::{
    is_windows_10_19h1_or_later, is_windows_10_20h1_or_later, is_windows_10_rs5_or_later,
};

// ---------------------------------------------------------------------------
// 純粋ロジック(プラットフォーム非依存・テスト対象)
// ---------------------------------------------------------------------------

/// 色が「暗い」とみなせるか(輝度が中間より低いか)。原実装 `IsDarkThemeColor` (DarkMode.cpp:152)。
///
/// 原実装は `Theme::ThemeColor`(8bit RGB)を受け取るが、本クレートでは Theme 非依存とするため
/// R/G/B を直接受け取る。判定式は ITU-R 系の輝度近似 `R*299 + G*587 + B*114` と
/// 中間値 `255*500`(= 127500)の比較で、原実装と同一。
pub fn is_dark_theme_color(red: u8, green: u8, blue: u8) -> bool {
    (red as u32) * 299 + (green as u32) * 587 + (blue as u32) * 114 < 255 * 500
}

/// `WM_SETTINGCHANGE` の通知名がダークモード変更を表す `"ImmersiveColorSet"` か。
/// 原実装 `IsDarkModeSettingChanged` の名前比較 (DarkMode.cpp:235、`lstrcmpi`)。
///
/// 原実装は `lstrcmpi`(大文字小文字無視)で比較する。対象が ASCII 文字列のため
/// ASCII の大小無視比較で等価。
pub fn is_dark_mode_setting_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("ImmersiveColorSet")
}

/// アプリのダークモード許可フラグから設定すべき `PreferredAppMode` を選ぶ。
/// 原実装 `SetAppAllowDarkMode` のモード選択 (DarkMode.cpp:178)。
pub fn preferred_app_mode_for(allow: bool) -> PreferredAppMode {
    if allow {
        PreferredAppMode::AllowDark
    } else {
        PreferredAppMode::Default
    }
}

/// ウィンドウフレームのダークモード設定の最終結果。原実装 `SetWindowFrameDarkMode` の
/// 戻り値ロジック (DarkMode.cpp:208-211)。
///
/// `DWMWA_USE_IMMERSIVE_DARK_MODE`(属性 20)が成功すれば真。失敗時は、20H1 より前の OS に
/// 限り旧属性(19)の成否を結果とする。20H1 以降は属性 19 を使わない。
pub fn frame_dark_mode_result(attr20_ok: bool, is_20h1_or_later: bool, attr19_ok: bool) -> bool {
    attr20_ok || (!is_20h1_or_later && attr19_ok)
}

// ---------------------------------------------------------------------------
// uxtheme.dll の非公開エクスポート(序数ロード)
// ---------------------------------------------------------------------------

/// 原実装の匿名 namespace にある `PreferredAppMode`(DarkMode.cpp:35)。
/// `SetPreferredAppMode` に渡す。基底型は C++ の `enum class`(int)に合わせる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum PreferredAppMode {
    Default = 0,
    AllowDark = 1,
    ForceDark = 2,
    ForceLight = 3,
}

type ShouldAppsUseDarkModeFn = unsafe extern "system" fn() -> u8;
type AllowDarkModeForWindowFn = unsafe extern "system" fn(HWND, BOOL) -> BOOL;
type FlushMenuThemesFn = unsafe extern "system" fn();
type RefreshImmersiveColorPolicyStateFn = unsafe extern "system" fn();
type IsDarkModeAllowedForWindowFn = unsafe extern "system" fn(HWND) -> BOOL;
type ShouldSystemUseDarkModeFn = unsafe extern "system" fn() -> BOOL;
type SetPreferredAppModeFn = unsafe extern "system" fn(PreferredAppMode) -> PreferredAppMode;

/// uxtheme.dll から関数を取得する。原実装 `GET_MODULE_FUNCTION_ORDINAL`(Util.h:278)に倣い、
/// まず名前で、見つからなければ序数で `GetProcAddress` する。
///
/// uxtheme のダークモード関連は名前なしエクスポートのため、実際は序数で解決される。
fn uxtheme_proc(name: PCSTR, ordinal: u16) -> FARPROC {
    // GetModuleHandle が失敗した場合でも GetProcAddress(NULL, ...) は失敗(None)を返す。
    let hmodule = unsafe { GetModuleHandleW(w!("uxtheme.dll")) }.unwrap_or_default();
    let by_name = unsafe { GetProcAddress(hmodule, name) };
    if by_name.is_some() {
        return by_name;
    }
    // MAKEINTRESOURCEA(ordinal) 相当。下位ワードに序数を置いたポインタを渡す。
    unsafe { GetProcAddress(hmodule, PCSTR(ordinal as usize as *const u8)) }
}

/// 原実装 `ShouldAppsUseDarkMode` (DarkMode.cpp:43、uxtheme 序数 132)。
fn should_apps_use_dark_mode() -> bool {
    if !is_windows_10_rs5_or_later() {
        return false;
    }
    match uxtheme_proc(s!("ShouldAppsUseDarkMode"), 132) {
        // SAFETY: 取得したポインタは uxtheme 序数 132 の `BOOLEAN()` 関数。
        Some(proc) => unsafe {
            let f: ShouldAppsUseDarkModeFn = transmute(proc);
            f() != 0
        },
        None => false,
    }
}

/// 原実装 `AllowDarkModeForWindow` (DarkMode.cpp:57、uxtheme 序数 133)。
fn allow_dark_mode_for_window(hwnd: HWND, allow: bool) -> bool {
    if !is_windows_10_rs5_or_later() {
        return false;
    }
    match uxtheme_proc(s!("AllowDarkModeForWindow"), 133) {
        // SAFETY: 取得したポインタは uxtheme 序数 133 の `BOOL(HWND, BOOL)` 関数。
        Some(proc) => unsafe {
            let f: AllowDarkModeForWindowFn = transmute(proc);
            // 原実装は呼び出し結果を無視し、常に TRUE を返す。
            let _ = f(hwnd, BOOL::from(allow));
            true
        },
        None => false,
    }
}

/// 原実装 `FlushMenuThemes` (DarkMode.cpp:73、uxtheme 序数 136)。
/// 原実装では `SetAppAllowDarkMode` 内でコメントアウトされており未使用。対応保持のため残す。
#[allow(dead_code)]
fn flush_menu_themes() {
    if !is_windows_10_rs5_or_later() {
        return;
    }
    if let Some(proc) = uxtheme_proc(s!("FlushMenuThemes"), 136) {
        // SAFETY: 取得したポインタは uxtheme 序数 136 の `void()` 関数。
        unsafe {
            let f: FlushMenuThemesFn = transmute(proc);
            f();
        }
    }
}

/// 原実装 `RefreshImmersiveColorPolicyState` (DarkMode.cpp:87、uxtheme 序数 104)。
fn refresh_immersive_color_policy_state() {
    if !is_windows_10_rs5_or_later() {
        return;
    }
    if let Some(proc) = uxtheme_proc(s!("RefreshImmersiveColorPolicyState"), 104) {
        // SAFETY: 取得したポインタは uxtheme 序数 104 の `void()` 関数。
        unsafe {
            let f: RefreshImmersiveColorPolicyStateFn = transmute(proc);
            f();
        }
    }
}

/// 原実装 `IsDarkModeAllowedForWindow` (DarkMode.cpp:101、uxtheme 序数 137)。
/// 原実装でも公開関数からは未使用。対応保持のため残す。
#[allow(dead_code)]
fn is_dark_mode_allowed_for_window(hwnd: HWND) -> bool {
    if !is_windows_10_rs5_or_later() {
        return false;
    }
    match uxtheme_proc(s!("IsDarkModeAllowedForWindow"), 137) {
        // SAFETY: 取得したポインタは uxtheme 序数 137 の `BOOL(HWND)` 関数。
        Some(proc) => unsafe {
            let f: IsDarkModeAllowedForWindowFn = transmute(proc);
            f(hwnd).as_bool()
        },
        None => false,
    }
}

/// 原実装 `ShouldSystemUseDarkMode` (DarkMode.cpp:115、uxtheme 序数 138)。
/// 原実装でも公開関数からは未使用。対応保持のため残す。
#[allow(dead_code)]
fn should_system_use_dark_mode() -> bool {
    if !is_windows_10_19h1_or_later() {
        return false;
    }
    match uxtheme_proc(s!("ShouldSystemUseDarkMode"), 138) {
        // SAFETY: 取得したポインタは uxtheme 序数 138 の `BOOL()` 関数。
        Some(proc) => unsafe {
            let f: ShouldSystemUseDarkModeFn = transmute(proc);
            f().as_bool()
        },
        None => false,
    }
}

/// 原実装 `SetPreferredAppMode` (DarkMode.cpp:129、uxtheme 序数 135)。
fn set_preferred_app_mode(mode: PreferredAppMode) -> PreferredAppMode {
    if !is_windows_10_19h1_or_later() {
        return PreferredAppMode::Default;
    }
    match uxtheme_proc(s!("SetPreferredAppMode"), 135) {
        // SAFETY: 取得したポインタは uxtheme 序数 135 の
        // `PreferredAppMode(PreferredAppMode)` 関数。
        Some(proc) => unsafe {
            let f: SetPreferredAppModeFn = transmute(proc);
            f(mode)
        },
        None => PreferredAppMode::Default,
    }
}

// ---------------------------------------------------------------------------
// 公開 API(Win32 接続)
// ---------------------------------------------------------------------------

/// ダークテーマ(ウィンドウ単位の `DarkMode_Explorer` テーマ)が利用可能か。
/// 原実装 `IsDarkThemeSupported` (DarkMode.cpp:146)。
pub fn is_dark_theme_supported() -> bool {
    is_windows_10_rs5_or_later()
}

/// ウィンドウに `DarkMode_Explorer` テーマ(解除時は既定)を適用する。
/// 原実装 `SetWindowDarkTheme` (DarkMode.cpp:158)。
pub fn set_window_dark_theme(hwnd: HWND, dark: bool) -> bool {
    if !is_dark_theme_supported() {
        return false;
    }
    let sub_app_name = if dark {
        w!("DarkMode_Explorer")
    } else {
        PCWSTR::null()
    };
    // SAFETY: hwnd の有効性は呼び出し側が保証する(原実装も同様)。
    unsafe { SetWindowTheme(hwnd, sub_app_name, PCWSTR::null()).is_ok() }
}

/// アプリ単位のダークモード許可が利用可能か。原実装 `IsDarkAppModeSupported` (DarkMode.cpp:167)。
pub fn is_dark_app_mode_supported() -> bool {
    is_windows_10_19h1_or_later()
}

/// アプリ全体のダークモードを許可/解除する。原実装 `SetAppAllowDarkMode` (DarkMode.cpp:173)。
pub fn set_app_allow_dark_mode(allow: bool) -> bool {
    if !is_dark_app_mode_supported() {
        return false;
    }
    set_preferred_app_mode(preferred_app_mode_for(allow));
    // 原実装はここで FlushMenuThemes(); をコメントアウトしている。
    refresh_immersive_color_policy_state();
    true
}

/// ウィンドウ単位でダークモードを許可/解除する。原実装 `SetWindowAllowDarkMode` (DarkMode.cpp:189)。
pub fn set_window_allow_dark_mode(hwnd: HWND, allow: bool) -> bool {
    // SAFETY: ハンドルの判定のみ。無効なら原実装同様 false を返す。
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        return false;
    }
    allow_dark_mode_for_window(hwnd, allow)
}

/// ウィンドウのフレーム(タイトルバー等)をダークモードにする。
/// 原実装 `SetWindowFrameDarkMode` (DarkMode.cpp:198)。
pub fn set_window_frame_dark_mode(hwnd: HWND, dark_mode: bool) -> bool {
    if !is_windows_10_rs5_or_later() {
        return false;
    }
    // SAFETY: ハンドルの判定のみ。
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        return false;
    }

    let dark = BOOL::from(dark_mode);
    let is_20h1 = is_windows_10_20h1_or_later();

    // DWMWA_USE_IMMERSIVE_DARK_MODE = 20。失敗かつ 20H1 より前なら旧属性 19 を試す(原実装の短絡)。
    let attr20_ok = set_dwm_dark_frame_attribute(hwnd, 20, dark);
    let attr19_ok = if !attr20_ok && !is_20h1 {
        set_dwm_dark_frame_attribute(hwnd, 19, dark)
    } else {
        false
    };

    frame_dark_mode_result(attr20_ok, is_20h1, attr19_ok)
}

/// `DwmSetWindowAttribute` で `BOOL` 属性を設定し、成功したかを返す。
fn set_dwm_dark_frame_attribute(hwnd: HWND, attribute: i32, value: BOOL) -> bool {
    // SAFETY: value は呼び出し中のみ有効なローカル参照を渡す。
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWINDOWATTRIBUTE(attribute),
            &value as *const BOOL as *const c_void,
            size_of::<BOOL>() as u32,
        )
        .is_ok()
    }
}

/// アプリがダークモードを使うべきか(ハイコントラストでなく、システムがダークを推奨)。
/// 原実装 `IsDarkMode` (DarkMode.cpp:215)。
pub fn is_dark_mode() -> bool {
    !is_high_contrast() && should_apps_use_dark_mode()
}

/// ハイコントラストモードが有効か。原実装 `IsHighContrast` (DarkMode.cpp:221)。
pub fn is_high_contrast() -> bool {
    let mut high_contrast = HIGHCONTRASTW {
        cbSize: size_of::<HIGHCONTRASTW>() as u32,
        ..Default::default()
    };
    // SAFETY: high_contrast は SystemParametersInfoW が書き込むのに十分なサイズを持つ。
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            size_of::<HIGHCONTRASTW>() as u32,
            Some(&mut high_contrast as *mut HIGHCONTRASTW as *mut c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    }
    .is_ok();
    ok && (high_contrast.dwFlags.0 & HCF_HIGHCONTRASTON.0) != 0
}

/// `WM_SETTINGCHANGE` がダークモードの変更を示すか判定する。
/// 原実装 `IsDarkModeSettingChanged` (DarkMode.cpp:230)。
///
/// `hwnd` / `wparam` は原実装でも未使用だがシグネチャ対応のため受け取る。
pub fn is_dark_mode_setting_changed(
    _hwnd: HWND,
    message: u32,
    _wparam: WPARAM,
    lparam: LPARAM,
) -> bool {
    if message == WM_SETTINGCHANGE {
        let psz = lparam.0 as *const u16;
        if !psz.is_null() {
            // SAFETY: WM_SETTINGCHANGE の lParam は NUL 終端のワイド文字列。
            let name = unsafe { wide_to_string(psz) };
            return is_dark_mode_setting_name(&name);
        }
    }
    false
}

/// NUL 終端のワイド文字列ポインタを `String` 化する。
///
/// # Safety
/// `psz` は NUL 終端された有効なワイド文字列を指していなければならない。
unsafe fn wide_to_string(psz: *const u16) -> String {
    let mut len = 0usize;
    while *psz.add(len) != 0 {
        len += 1;
    }
    let slice = std::slice::from_raw_parts(psz, len);
    String::from_utf16_lossy(slice)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ----- 純粋ロジック -----

    #[test]
    fn test_is_dark_theme_color() {
        // 黒は暗い、白は暗くない。
        assert!(is_dark_theme_color(0, 0, 0));
        assert!(!is_dark_theme_color(255, 255, 255));
        // 中間グレーの境界。輝度 = level*1000、中間値 = 127500。
        // 127 -> 127000 < 127500 -> 暗い / 128 -> 128000 >= 127500 -> 暗くない。
        assert!(is_dark_theme_color(127, 127, 127));
        assert!(!is_dark_theme_color(128, 128, 128));
        // 純色の重み付け(緑が最も明るく寄与)。
        assert!(!is_dark_theme_color(0, 255, 0)); // 587*255 = 149685 >= 127500
        assert!(is_dark_theme_color(0, 0, 255)); // 114*255 = 29070 < 127500
        assert!(is_dark_theme_color(255, 0, 0)); // 299*255 = 76245 < 127500
    }

    #[test]
    fn test_is_dark_mode_setting_name() {
        assert!(is_dark_mode_setting_name("ImmersiveColorSet"));
        // lstrcmpi 相当の大小無視。
        assert!(is_dark_mode_setting_name("immersivecolorset"));
        assert!(is_dark_mode_setting_name("IMMERSIVECOLORSET"));
        // 別の通知名・空文字列は対象外。
        assert!(!is_dark_mode_setting_name("Environment"));
        assert!(!is_dark_mode_setting_name(""));
        assert!(!is_dark_mode_setting_name("ImmersiveColorSet "));
    }

    #[test]
    fn test_preferred_app_mode_for() {
        assert_eq!(preferred_app_mode_for(true), PreferredAppMode::AllowDark);
        assert_eq!(preferred_app_mode_for(false), PreferredAppMode::Default);
    }

    #[test]
    fn test_frame_dark_mode_result() {
        // 属性 20 成功なら、他に関わらず真。
        assert!(frame_dark_mode_result(true, true, false));
        assert!(frame_dark_mode_result(true, false, false));
        // 属性 20 失敗・20H1 より前は属性 19 の結果に従う。
        assert!(frame_dark_mode_result(false, false, true));
        assert!(!frame_dark_mode_result(false, false, false));
        // 属性 20 失敗・20H1 以降は属性 19 を使わない(常に偽)。
        assert!(!frame_dark_mode_result(false, true, true));
        assert!(!frame_dark_mode_result(false, true, false));
    }

    // ----- Win32 接続(実機でパニックしない・整合する) -----

    #[test]
    fn test_support_flags_consistent_with_os() {
        // サポート判定は winutil の OS 判定そのもの。
        assert_eq!(is_dark_theme_supported(), is_windows_10_rs5_or_later());
        assert_eq!(is_dark_app_mode_supported(), is_windows_10_19h1_or_later());
    }

    #[test]
    fn test_is_high_contrast_does_not_panic() {
        // 値は環境依存。呼び出してパニックしないことのみ確認。
        let _ = is_high_contrast();
    }

    #[test]
    fn test_is_dark_mode_does_not_panic() {
        let _ = is_dark_mode();
    }

    #[test]
    fn test_is_dark_mode_setting_changed_non_setting_message() {
        // WM_SETTINGCHANGE 以外は常に false(lparam は参照されない)。
        assert!(!is_dark_mode_setting_changed(
            HWND::default(),
            0,
            WPARAM(0),
            LPARAM(0)
        ));
    }

    #[test]
    fn test_is_dark_mode_setting_changed_null_lparam() {
        // WM_SETTINGCHANGE でも lParam が NULL なら false。
        assert!(!is_dark_mode_setting_changed(
            HWND::default(),
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(0)
        ));
    }

    #[test]
    fn test_is_dark_mode_setting_changed_matches_name() {
        // lParam にワイド文字列を渡したときの一致/不一致。
        let mut hit: Vec<u16> = "ImmersiveColorSet".encode_utf16().collect();
        hit.push(0);
        assert!(is_dark_mode_setting_changed(
            HWND::default(),
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(hit.as_ptr() as isize)
        ));

        let mut miss: Vec<u16> = "Environment".encode_utf16().collect();
        miss.push(0);
        assert!(!is_dark_mode_setting_changed(
            HWND::default(),
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(miss.as_ptr() as isize)
        ));
    }
}
