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

//! TVTest `DPIUtil`(`src/DPIUtil.cpp` / `DPIUtil.h`)の Rust 移植。
//!
//! DPI スケーリングの計算や Per-Monitor DPI 対応の判定を提供する。
//! プラットフォーム非依存な計算(DPI スケーリングの [`mul_div`]、
//! システムメトリクスのフォールバックスケーリング [`scale_metric_with_dpi`]、
//! `PROCESS_DPI_AWARENESS` のマッピング [`awareness_kind_from_process_value`]、
//! コモンダイアログのコンテキスト選択 [`common_dialog_context_is_system_aware`])を
//! 純粋関数として切り出してテストし、Win32 依存部分は薄いラッパーとして windows-rs で実装する。
//!
//! Win32 連携:
//! - user32.dll の DPI 関数(`GetDpiForWindow` など)は OS により存在しないため、
//!   原実装の `GET_MODULE_FUNCTION`(`GetModuleHandle` + `GetProcAddress`)を再現して動的ロードする。
//! - shcore.dll(`GetDpiForMonitor` / `GetProcessDpiAwareness`)は `Util::LoadSystemLibrary`
//!   相当(`LoadLibraryEx` + `LOAD_LIBRARY_SEARCH_SYSTEM32`)で読み込む。
//! - OS バージョン判定は [`tvtest_winutil`] に委譲する。

#![cfg(windows)]

use core::ffi::c_void;
use std::mem::transmute;
use std::sync::atomic::{AtomicI32, Ordering};

use windows::core::{s, w, BOOL, HRESULT, PCSTR, PCWSTR};
use windows::Win32::Foundation::{FreeLibrary, FARPROC, HANDLE, HMODULE, HWND, RECT, S_OK};
use windows::Win32::Graphics::Gdi::{
    GetDC, GetDeviceCaps, MonitorFromWindow, ReleaseDC, HMONITOR, LOGPIXELSY, MONITOR_DEFAULTTONULL,
};
use windows::Win32::System::LibraryLoader::{
    GetModuleHandleW, GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32,
};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE,
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, DPI_AWARENESS_CONTEXT_SYSTEM_AWARE,
    DPI_AWARENESS_CONTEXT_UNAWARE, MDT_EFFECTIVE_DPI, MONITOR_DPI_TYPE, PROCESS_DPI_AWARENESS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AdjustWindowRectEx, GetSystemMetrics, IsWindow, SM_CXVSCROLL, SYSTEM_METRICS_INDEX,
    WINDOW_EX_STYLE, WINDOW_STYLE,
};

use tvtest_winutil::{
    is_windows_10_anniversary_update_or_later, is_windows_10_creators_update_or_later,
    is_windows_8_1_or_later,
};

// ---------------------------------------------------------------------------
// 純粋ロジック(プラットフォーム非依存)
// ---------------------------------------------------------------------------

/// Win32 `MulDiv` 相当の整数演算。
///
/// `number * numerator / denominator` を 64bit 中間値で計算し、最近接整数へ
/// 四捨五入(端数 0.5 はゼロから遠い側へ丸める)する。`denominator == 0` または
/// 結果が `i32` 範囲を超える場合は `-1` を返す(Win32 `MulDiv` の仕様)。
///
/// DPIUtil.cpp:208 の `::MulDiv(Value, DPI, GetSystemDPI())` で用いられる。
pub fn mul_div(number: i32, numerator: i32, denominator: i32) -> i32 {
    if denominator == 0 {
        return -1;
    }

    let product = (number as i64) * (numerator as i64);
    let denom = denominator as i64;
    let negative = (product < 0) != (denom < 0);

    // 絶対値で四捨五入(端数 0.5 は切り上げ = ゼロから遠い側)。
    let abs_product = product.unsigned_abs();
    let abs_denom = denom.unsigned_abs();
    let abs_quotient = (abs_product + abs_denom / 2) / abs_denom;

    let result = if negative {
        -(abs_quotient as i128)
    } else {
        abs_quotient as i128
    };

    if result < i32::MIN as i128 || result > i32::MAX as i128 {
        return -1;
    }
    result as i32
}

/// `GetSystemMetricsWithDPI` のフォールバックスケーリング計算(DPIUtil.cpp:206-210)。
///
/// `GetSystemMetricsForDpi` が使えない環境で `GetSystemMetrics` の戻り値 `value` を
/// `system_dpi` 基準から `dpi` へスケールする。`fallback_scaling` が偽、または
/// `value == 0` の場合はスケールせずそのまま返す。
pub fn scale_metric_with_dpi(value: i32, dpi: i32, system_dpi: i32, fallback_scaling: bool) -> i32 {
    if fallback_scaling && value != 0 {
        mul_div(value, dpi, system_dpi)
    } else {
        value
    }
}

/// `GetWindowDPIAwareness` のフォールバックで得られる `PROCESS_DPI_AWARENESS` の種別。
///
/// DPIUtil.cpp:265-269 の `switch` に対応する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DpiAwarenessKind {
    /// `PROCESS_DPI_UNAWARE`
    Unaware,
    /// `PROCESS_SYSTEM_DPI_AWARE`
    SystemAware,
    /// `PROCESS_PER_MONITOR_DPI_AWARE`
    PerMonitorAware,
}

/// `PROCESS_DPI_AWARENESS`(0/1/2)を [`DpiAwarenessKind`] へ写像する(DPIUtil.cpp:265-269)。
///
/// 既知の値以外は `None`(原実装では該当する `case` がなく `nullptr` を返す)。
pub fn awareness_kind_from_process_value(value: i32) -> Option<DpiAwarenessKind> {
    match value {
        0 => Some(DpiAwarenessKind::Unaware),
        1 => Some(DpiAwarenessKind::SystemAware),
        2 => Some(DpiAwarenessKind::PerMonitorAware),
        _ => None,
    }
}

/// `CommonDialogDPIBlock` のコンテキスト選択(DPIUtil.cpp:316-322)。
///
/// 現在のスレッドコンテキストが `PER_MONITOR_AWARE_V2` ならコンテキストを
/// 変更しない(`nullptr`)。そうでなければ `SYSTEM_AWARE` に切り替える。
/// 戻り値が `true` のとき `SYSTEM_AWARE` を適用すべきことを表す。
pub fn common_dialog_context_is_system_aware(current_is_per_monitor_v2: bool) -> bool {
    !current_is_per_monitor_v2
}

// ---------------------------------------------------------------------------
// 動的ロードのための関数ポインタ型と補助
// ---------------------------------------------------------------------------

type GetDpiForSystemFn = unsafe extern "system" fn() -> u32;
type GetDpiForWindowFn = unsafe extern "system" fn(HWND) -> u32;
type EnableNonClientDpiScalingFn = unsafe extern "system" fn(HWND) -> BOOL;
type AdjustWindowRectExForDpiFn =
    unsafe extern "system" fn(*mut RECT, u32, BOOL, u32, u32) -> BOOL;
type SystemParametersInfoForDpiFn =
    unsafe extern "system" fn(u32, u32, *mut c_void, u32, u32) -> BOOL;
type GetSystemMetricsForDpiFn = unsafe extern "system" fn(i32, u32) -> i32;
type SetThreadDpiAwarenessContextFn =
    unsafe extern "system" fn(DPI_AWARENESS_CONTEXT) -> DPI_AWARENESS_CONTEXT;
type GetThreadDpiAwarenessContextFn = unsafe extern "system" fn() -> DPI_AWARENESS_CONTEXT;
type GetWindowDpiAwarenessContextFn =
    unsafe extern "system" fn(HWND) -> DPI_AWARENESS_CONTEXT;
type AreDpiAwarenessContextsEqualFn =
    unsafe extern "system" fn(DPI_AWARENESS_CONTEXT, DPI_AWARENESS_CONTEXT) -> BOOL;
// shcore.dll
type GetDpiForMonitorFn =
    unsafe extern "system" fn(HMONITOR, MONITOR_DPI_TYPE, *mut u32, *mut u32) -> HRESULT;
type GetProcessDpiAwarenessFn =
    unsafe extern "system" fn(HANDLE, *mut PROCESS_DPI_AWARENESS) -> HRESULT;

/// user32.dll の関数を名前で解決する。`GET_MODULE_FUNCTION`(Util.h:278)相当。
///
/// user32.dll は常にロード済みのため `GetModuleHandle` で取得する。
fn user32_proc(name: PCSTR) -> FARPROC {
    let hmodule = unsafe { GetModuleHandleW(w!("user32.dll")) }.unwrap_or_default();
    unsafe { GetProcAddress(hmodule, name) }
}

/// system32 配下のライブラリを読み込む。`Util::LoadSystemLibrary` 相当。
///
/// `LOAD_LIBRARY_SEARCH_SYSTEM32` で検索パスを system32 に限定する(DLL プリロード対策)。
fn load_system_library(name: PCWSTR) -> Option<HMODULE> {
    unsafe { LoadLibraryExW(name, None, LOAD_LIBRARY_SEARCH_SYSTEM32) }.ok()
}

// 無名 namespace の内部ヘルパー(DPIUtil.cpp:75-108)

/// `MySetThreadDpiAwarenessContext`(DPIUtil.cpp:75)。
fn my_set_thread_dpi_awareness_context(context: DPI_AWARENESS_CONTEXT) -> DPI_AWARENESS_CONTEXT {
    if is_windows_10_anniversary_update_or_later() {
        if let Some(proc) = user32_proc(s!("SetThreadDpiAwarenessContext")) {
            // SAFETY: user32 の SetThreadDpiAwarenessContext は型 alias と一致する。
            let f: SetThreadDpiAwarenessContextFn = unsafe { transmute(proc) };
            return unsafe { f(context) };
        }
    }
    DPI_AWARENESS_CONTEXT::default()
}

/// `MyGetThreadDpiAwarenessContext`(DPIUtil.cpp:87)。
fn my_get_thread_dpi_awareness_context() -> DPI_AWARENESS_CONTEXT {
    if is_windows_10_anniversary_update_or_later() {
        if let Some(proc) = user32_proc(s!("GetThreadDpiAwarenessContext")) {
            // SAFETY: user32 の GetThreadDpiAwarenessContext は型 alias と一致する。
            let f: GetThreadDpiAwarenessContextFn = unsafe { transmute(proc) };
            return unsafe { f() };
        }
    }
    DPI_AWARENESS_CONTEXT::default()
}

/// `MyAreDpiAwarenessContextsEqual`(DPIUtil.cpp:99)。
///
/// API が無ければハンドル値の単純比較にフォールバックする。
fn my_are_dpi_awareness_contexts_equal(
    context1: DPI_AWARENESS_CONTEXT,
    context2: DPI_AWARENESS_CONTEXT,
) -> bool {
    if is_windows_10_anniversary_update_or_later() {
        if let Some(proc) = user32_proc(s!("AreDpiAwarenessContextsEqual")) {
            // SAFETY: user32 の AreDpiAwarenessContextsEqual は型 alias と一致する。
            let f: AreDpiAwarenessContextsEqualFn = unsafe { transmute(proc) };
            return unsafe { f(context1, context2) }.as_bool();
        }
    }
    context1 == context2
}

// ---------------------------------------------------------------------------
// 公開 API(Win32 連携)
// ---------------------------------------------------------------------------

/// 非クライアント領域の自動 DPI スケーリングを有効化する。
/// `EnableNonClientDPIScaling`(DPIUtil.cpp:114)。
pub fn enable_non_client_dpi_scaling(hwnd: HWND) -> bool {
    if is_windows_10_anniversary_update_or_later() {
        if let Some(proc) = user32_proc(s!("EnableNonClientDpiScaling")) {
            // SAFETY: user32 の EnableNonClientDpiScaling は型 alias と一致する。
            let f: EnableNonClientDpiScalingFn = unsafe { transmute(proc) };
            return unsafe { f(hwnd) }.as_bool();
        }
    }
    false
}

/// システム DPI を取得する。`GetSystemDPI`(DPIUtil.cpp:126)。
///
/// `GetDpiForSystem` が使える環境では毎回その値を返す(キャッシュしない)。
/// 使えない環境では `GetDeviceCaps(LOGPIXELSY)` の結果をキャッシュして返す
/// (原実装の `static int SystemDPI` に対応)。
pub fn get_system_dpi() -> i32 {
    let cached = SYSTEM_DPI_CACHE.load(Ordering::Relaxed);
    if cached != 0 {
        return cached;
    }

    if let Some(proc) = user32_proc(s!("GetDpiForSystem")) {
        // SAFETY: user32 の GetDpiForSystem は型 alias と一致する。
        let f: GetDpiForSystemFn = unsafe { transmute(proc) };
        return unsafe { f() } as i32;
    }

    // フォールバックのみキャッシュする(原実装と同じ)。
    let mut system_dpi = 0;
    unsafe {
        let hdc = GetDC(None);
        if !hdc.is_invalid() {
            system_dpi = GetDeviceCaps(Some(hdc), LOGPIXELSY);
            ReleaseDC(None, hdc);
        }
    }
    SYSTEM_DPI_CACHE.store(system_dpi, Ordering::Relaxed);
    system_dpi
}

/// `GetSystemDPI` のフォールバック値キャッシュ(原実装の `static int SystemDPI`)。
static SYSTEM_DPI_CACHE: AtomicI32 = AtomicI32::new(0);

/// モニターの DPI を取得する。`GetMonitorDPI`(DPIUtil.cpp:153)。
///
/// 取得できない場合は 0 を返す。
pub fn get_monitor_dpi(monitor: HMONITOR) -> i32 {
    if !monitor.is_invalid() && is_windows_8_1_or_later() {
        if let Some(hlib) = load_system_library(w!("shcore.dll")) {
            let mut dpi_x: u32 = 0;
            let mut dpi_y: u32 = 0;
            let mut ok = false;
            if let Some(proc) = unsafe { GetProcAddress(hlib, s!("GetDpiForMonitor")) } {
                // SAFETY: shcore の GetDpiForMonitor は型 alias と一致する。
                let f: GetDpiForMonitorFn = unsafe { transmute(proc) };
                ok = unsafe { f(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) } == S_OK;
            }
            unsafe {
                let _ = FreeLibrary(hlib);
            }
            if ok {
                return dpi_y as i32;
            }
        }
    }
    0
}

/// ウィンドウの DPI を取得する。`GetWindowDPI`(DPIUtil.cpp:174)。
///
/// 無効なウィンドウハンドルでは 0 を返す。
pub fn get_window_dpi(hwnd: HWND) -> i32 {
    // SAFETY: 無効なハンドルでも安全に FALSE が返る。
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        return 0;
    }

    if is_windows_10_anniversary_update_or_later() {
        if let Some(proc) = user32_proc(s!("GetDpiForWindow")) {
            // SAFETY: user32 の GetDpiForWindow は型 alias と一致する。
            let f: GetDpiForWindowFn = unsafe { transmute(proc) };
            return unsafe { f(hwnd) } as i32;
        }
    }

    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONULL) };
    if !monitor.is_invalid() {
        get_monitor_dpi(monitor)
    } else {
        get_system_dpi()
    }
}

/// DPI を指定してシステムメトリクスを取得する。
/// `GetSystemMetricsWithDPI`(DPIUtil.cpp:197)。
///
/// `GetSystemMetricsForDpi` が使えない環境では `GetSystemMetrics` の結果を
/// [`scale_metric_with_dpi`] でスケールする。
pub fn get_system_metrics_with_dpi(index: i32, dpi: i32, fallback_scaling: bool) -> i32 {
    if is_windows_10_anniversary_update_or_later() {
        if let Some(proc) = user32_proc(s!("GetSystemMetricsForDpi")) {
            // SAFETY: user32 の GetSystemMetricsForDpi は型 alias と一致する。
            let f: GetSystemMetricsForDpiFn = unsafe { transmute(proc) };
            return unsafe { f(index, dpi as u32) };
        }
    }

    let value = unsafe { GetSystemMetrics(SYSTEM_METRICS_INDEX(index)) };
    scale_metric_with_dpi(value, dpi, get_system_dpi(), fallback_scaling)
}

/// DPI を指定して `SystemParametersInfo` を呼び出す。
/// `SystemParametersInfoWithDPI`(DPIUtil.cpp:214)。
///
/// 対応 API が無い環境では何もせず `false` を返す(原実装と同じ)。
///
/// # Safety
/// `p_param` は `action` が要求するバッファを指す有効なポインタでなければならない。
pub unsafe fn system_parameters_info_with_dpi(
    action: u32,
    param: u32,
    p_param: *mut c_void,
    flags: u32,
    dpi: i32,
) -> bool {
    if is_windows_10_anniversary_update_or_later() {
        if let Some(proc) = user32_proc(s!("SystemParametersInfoForDpi")) {
            // SAFETY: user32 の SystemParametersInfoForDpi は型 alias と一致する。
            let f: SystemParametersInfoForDpiFn = transmute(proc);
            return f(action, param, p_param, flags, dpi as u32).as_bool();
        }
    }
    false
}

/// DPI を指定してウィンドウ矩形を調整する。
/// `AdjustWindowRectWithDPI`(DPIUtil.cpp:227)。
///
/// 対応 API が無い環境では DPI 非対応の `AdjustWindowRectEx` にフォールバックする。
pub fn adjust_window_rect_with_dpi(
    rect: &mut RECT,
    style: u32,
    ex_style: u32,
    menu: bool,
    dpi: i32,
) -> bool {
    if is_windows_10_anniversary_update_or_later() {
        if let Some(proc) = user32_proc(s!("AdjustWindowRectExForDpi")) {
            // SAFETY: user32 の AdjustWindowRectExForDpi は型 alias と一致する。
            // 引数順は (lpRect, dwStyle, bMenu, dwExStyle, dpi)。
            let f: AdjustWindowRectExForDpiFn = unsafe { transmute(proc) };
            return unsafe {
                f(rect as *mut RECT, style, BOOL::from(menu), ex_style, dpi as u32)
            }
            .as_bool();
        }
    }

    // SAFETY: rect は有効な参照から得たポインタ。
    unsafe {
        AdjustWindowRectEx(
            rect as *mut RECT,
            WINDOW_STYLE(style),
            menu,
            WINDOW_EX_STYLE(ex_style),
        )
    }
    .is_ok()
}

/// 縦スクロールバーの幅を取得する。`GetScrollBarWidth`(DPIUtil.cpp:240)。
///
/// Per-Monitor DPI V2 のウィンドウでは DPI を考慮した幅を、そうでなければ
/// `GetSystemMetrics(SM_CXVSCROLL)` を返す。
pub fn get_scroll_bar_width(hwnd: HWND) -> i32 {
    if is_window_per_monitor_dpi_v2(hwnd) {
        return get_system_metrics_with_dpi(SM_CXVSCROLL.0, get_window_dpi(hwnd), true);
    }
    unsafe { GetSystemMetrics(SM_CXVSCROLL) }
}

/// ウィンドウの DPI Awareness コンテキストを取得する。
/// `GetWindowDPIAwareness`(DPIUtil.cpp:249)。
///
/// 取得できない場合は既定値(`nullptr` 相当)を返す。
pub fn get_window_dpi_awareness(hwnd: HWND) -> DPI_AWARENESS_CONTEXT {
    if is_windows_10_anniversary_update_or_later() {
        if let Some(proc) = user32_proc(s!("GetWindowDpiAwarenessContext")) {
            // SAFETY: user32 の GetWindowDpiAwarenessContext は型 alias と一致する。
            let f: GetWindowDpiAwarenessContextFn = unsafe { transmute(proc) };
            return unsafe { f(hwnd) };
        }
    }

    if is_windows_8_1_or_later() {
        if let Some(hlib) = load_system_library(w!("shcore.dll")) {
            let mut awareness = PROCESS_DPI_AWARENESS::default();
            let mut got = false;
            if let Some(proc) = unsafe { GetProcAddress(hlib, s!("GetProcessDpiAwareness")) } {
                // SAFETY: shcore の GetProcessDpiAwareness は型 alias と一致する。
                let f: GetProcessDpiAwarenessFn = unsafe { transmute(proc) };
                got = unsafe { f(HANDLE::default(), &mut awareness) } == S_OK;
            }
            // 原実装は条件成立時(DPIUtil.cpp:264)とブロック終了後(:271)で
            // FreeLibrary を二重に呼ぶが、Rust では二重解放を避け 1 回に統一する
            // (観測される動作は同じ)。
            unsafe {
                let _ = FreeLibrary(hlib);
            }
            if got {
                return match awareness_kind_from_process_value(awareness.0) {
                    Some(DpiAwarenessKind::Unaware) => DPI_AWARENESS_CONTEXT_UNAWARE,
                    Some(DpiAwarenessKind::SystemAware) => DPI_AWARENESS_CONTEXT_SYSTEM_AWARE,
                    Some(DpiAwarenessKind::PerMonitorAware) => {
                        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE
                    }
                    None => DPI_AWARENESS_CONTEXT::default(),
                };
            }
        }
    }

    DPI_AWARENESS_CONTEXT::default()
}

/// ウィンドウが Per-Monitor DPI V1 かを判定する。
/// `IsWindowPerMonitorDPIV1`(DPIUtil.cpp:279)。
pub fn is_window_per_monitor_dpi_v1(hwnd: HWND) -> bool {
    my_are_dpi_awareness_contexts_equal(
        get_window_dpi_awareness(hwnd),
        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE,
    )
}

/// ウィンドウが Per-Monitor DPI V2 かを判定する。
/// `IsWindowPerMonitorDPIV2`(DPIUtil.cpp:285)。
pub fn is_window_per_monitor_dpi_v2(hwnd: HWND) -> bool {
    if is_windows_10_creators_update_or_later() {
        if let Some(proc) = user32_proc(s!("GetWindowDpiAwarenessContext")) {
            // SAFETY: user32 の GetWindowDpiAwarenessContext は型 alias と一致する。
            let f: GetWindowDpiAwarenessContextFn = unsafe { transmute(proc) };
            let context = unsafe { f(hwnd) };
            return my_are_dpi_awareness_contexts_equal(
                context,
                DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
            );
        }
    }
    false
}

/// Per-Monitor DPI V2 が利用可能かを判定する。
/// `IsPerMonitorDPIV2Available`(DPIUtil.cpp:298)。
pub fn is_per_monitor_dpi_v2_available() -> bool {
    is_windows_10_creators_update_or_later()
}

// ---------------------------------------------------------------------------
// DPI Awareness コンテキストの一時切り替え(RAII)
// ---------------------------------------------------------------------------

/// スレッドの DPI Awareness コンテキストを一時的に切り替える RAII ガード。
/// `DPIBlockBase`(DPIUtil.cpp:304)に対応する。
///
/// 生成時に指定コンテキストへ切り替え、`Drop` 時に元のコンテキストへ戻す。
/// コンストラクタに既定値(`nullptr` 相当)を渡した場合は何もしない。
pub struct DpiBlock {
    old_context: DPI_AWARENESS_CONTEXT,
}

impl DpiBlock {
    /// 指定コンテキストへ切り替える。`DPIBlockBase::DPIBlockBase`(DPIUtil.cpp:304)。
    pub fn new(context: DPI_AWARENESS_CONTEXT) -> Self {
        let old_context = if context.0.is_null() {
            DPI_AWARENESS_CONTEXT::default()
        } else {
            my_set_thread_dpi_awareness_context(context)
        };
        Self { old_context }
    }

    /// `SystemDPIBlock`(DPIUtil.h:66)相当。System DPI Aware に切り替える。
    pub fn new_system() -> Self {
        Self::new(DPI_AWARENESS_CONTEXT_SYSTEM_AWARE)
    }

    /// `PerMonitorDPIBlock`(DPIUtil.h:73)相当。Per-Monitor DPI Aware に切り替える。
    pub fn new_per_monitor() -> Self {
        Self::new(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE)
    }

    /// `CommonDialogDPIBlock`(DPIUtil.cpp:316)相当。
    ///
    /// 現在のコンテキストが Per-Monitor V2 ならそのまま、そうでなければ
    /// System DPI Aware に切り替える([`common_dialog_context_is_system_aware`])。
    pub fn new_common_dialog() -> Self {
        let current = my_get_thread_dpi_awareness_context();
        let is_v2 =
            my_are_dpi_awareness_contexts_equal(current, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let context = if common_dialog_context_is_system_aware(is_v2) {
            DPI_AWARENESS_CONTEXT_SYSTEM_AWARE
        } else {
            DPI_AWARENESS_CONTEXT::default()
        };
        Self::new(context)
    }
}

impl Drop for DpiBlock {
    fn drop(&mut self) {
        // ~DPIBlockBase(DPIUtil.cpp:309)
        if !self.old_context.0.is_null() {
            my_set_thread_dpi_awareness_context(self.old_context);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ----- 純粋ロジック -----

    #[test]
    fn test_mul_div_basic() {
        // DPI スケーリングの典型例。
        assert_eq!(mul_div(100, 96, 96), 100);
        assert_eq!(mul_div(100, 144, 96), 150); // 100 * 1.5
        assert_eq!(mul_div(16, 192, 96), 32); // 2倍
    }

    #[test]
    fn test_mul_div_rounding() {
        // 端数 0.5 はゼロから遠い側へ丸める。
        assert_eq!(mul_div(10, 3, 4), 8); // 7.5 -> 8
        assert_eq!(mul_div(10, 1, 3), 3); // 3.33 -> 3
        assert_eq!(mul_div(10, 2, 3), 7); // 6.67 -> 7
        assert_eq!(mul_div(-10, 3, 4), -8); // -7.5 -> -8
        assert_eq!(mul_div(-10, 1, 3), -3); // -3.33 -> -3
    }

    #[test]
    fn test_mul_div_error() {
        // denominator == 0 は -1。
        assert_eq!(mul_div(5, 5, 0), -1);
        // 結果が i32 範囲を超える場合も -1。
        assert_eq!(mul_div(i32::MAX, i32::MAX, 1), -1);
    }

    #[test]
    fn test_scale_metric_with_dpi() {
        // スケーリング有効・値が非0ならスケールする。
        assert_eq!(scale_metric_with_dpi(16, 192, 96, true), 32);
        // 値が 0 ならそのまま。
        assert_eq!(scale_metric_with_dpi(0, 192, 96, true), 0);
        // スケーリング無効ならそのまま。
        assert_eq!(scale_metric_with_dpi(16, 192, 96, false), 16);
    }

    #[test]
    fn test_awareness_kind_from_process_value() {
        assert_eq!(
            awareness_kind_from_process_value(0),
            Some(DpiAwarenessKind::Unaware)
        );
        assert_eq!(
            awareness_kind_from_process_value(1),
            Some(DpiAwarenessKind::SystemAware)
        );
        assert_eq!(
            awareness_kind_from_process_value(2),
            Some(DpiAwarenessKind::PerMonitorAware)
        );
        assert_eq!(awareness_kind_from_process_value(3), None);
        assert_eq!(awareness_kind_from_process_value(-1), None);
    }

    #[test]
    fn test_common_dialog_context_is_system_aware() {
        // V2 なら変更しない(false)、それ以外は System Aware へ(true)。
        assert!(!common_dialog_context_is_system_aware(true));
        assert!(common_dialog_context_is_system_aware(false));
    }

    // ----- Win32 連携(実環境でパニックしないこと・妥当性) -----

    #[test]
    fn test_get_system_dpi_positive() {
        // 実環境ではシステム DPI が取得できる(96 以上が一般的)。
        assert!(get_system_dpi() > 0);
    }

    #[test]
    fn test_per_monitor_v2_available_matches_os() {
        assert_eq!(
            is_per_monitor_dpi_v2_available(),
            is_windows_10_creators_update_or_later()
        );
    }

    #[test]
    fn test_get_window_dpi_invalid_hwnd() {
        // 無効なウィンドウハンドルでは 0。
        assert_eq!(get_window_dpi(HWND::default()), 0);
    }

    #[test]
    fn test_get_monitor_dpi_invalid_monitor() {
        // 無効なモニターハンドルでは 0。
        assert_eq!(get_monitor_dpi(HMONITOR::default()), 0);
    }

    #[test]
    fn test_get_scroll_bar_width_positive() {
        // スクロールバー幅は正の値。
        assert!(get_scroll_bar_width(HWND::default()) > 0);
    }

    #[test]
    fn test_enable_non_client_dpi_scaling_does_not_panic() {
        // 無効ハンドルでもパニックしない。
        let _ = enable_non_client_dpi_scaling(HWND::default());
    }

    #[test]
    fn test_window_per_monitor_dpi_invalid_hwnd_does_not_panic() {
        let _ = is_window_per_monitor_dpi_v1(HWND::default());
        let _ = is_window_per_monitor_dpi_v2(HWND::default());
    }

    #[test]
    fn test_get_window_dpi_awareness_does_not_panic() {
        let _ = get_window_dpi_awareness(HWND::default());
    }

    #[test]
    fn test_dpi_block_system_does_not_panic() {
        // 生成と Drop(復元)がパニックしないこと。
        let _block = DpiBlock::new_system();
    }

    #[test]
    fn test_dpi_block_common_dialog_does_not_panic() {
        let _block = DpiBlock::new_common_dialog();
    }

    #[test]
    fn test_dpi_block_default_context_is_noop() {
        // 既定値(nullptr 相当)では切り替えを行わない。
        let _block = DpiBlock::new(DPI_AWARENESS_CONTEXT::default());
    }
}
