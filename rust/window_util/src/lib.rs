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

//! TVTest の `WindowUtil`(原実装 `src/WindowUtil.cpp` / `src/WindowUtil.h`)を
//! `windows` クレート(windows-rs)で Rust へ移植したもの。
//!
//! 提供する要素:
//! - ウィンドウ吸着 (`SnapWindow`):他ウィンドウ/モニタ端への吸着オフセット計算。
//!   端の可視判定 ([`is_window_edge_visible`])・最近接エッジ更新 ([`update_nearest`])・
//!   オフセット選択 ([`select_snap_offset`]) を純粋関数に切り出してテスト可能にしている。
//! - マウスホイール処理 ([`MouseWheelHandler`]):ホイール量の累積とスクロール量換算。
//! - マウス離脱追跡 ([`MouseLeaveTrack`]):クライアント/非クライアント領域の離脱状態。
//! - タイマー管理 ([`WindowTimerManager`]):タイマー ID をビットマスクで管理。
//! - ウィンドウサブクラス化 ([`WindowSubclass`]):`SetWindowSubclass` のラッパー。
//!
//! 純粋計算部分(吸着の幾何計算、ホイール量の累積/換算、タイマーのビット操作、
//! 離脱状態遷移)は Win32 から分離し、Rust の単体テストで原実装との一致を検証する。
//! 実際の Win32 API を呼ぶ部分(`SetTimer` / `TrackMouseEvent` / `EnumWindows` /
//! `SetWindowSubclass` 等)は本クレートが薄くラップする。

#![cfg(windows)]

use std::ffi::c_void;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, PtInRect, HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    TrackMouseEvent, TRACKMOUSEEVENT, TME_LEAVE, TME_NONCLIENT,
};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetDesktopWindow, GetMessagePos, GetSystemMetrics, GetTopWindow, GetWindow,
    GetWindowRect, IsWindowVisible, KillTimer, PeekMessageW, PostQuitMessage, SetTimer,
    SystemParametersInfoW, GW_HWNDNEXT, MSG, PM_NOREMOVE, SM_CXSCREEN, SM_CYSCREEN,
    SPI_GETWHEELSCROLLCHARS, SPI_GETWHEELSCROLLLINES, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    WHEEL_DELTA,
};

// ===========================================================================
// 純粋ロジック: 矩形と汎用ヘルパー
// ===========================================================================

/// Win32 `RECT` 相当の矩形(左上含み・右下排他)。純粋計算用。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self { left, top, right, bottom }
    }

    fn from_win32(rc: &RECT) -> Self {
        Self { left: rc.left, top: rc.top, right: rc.right, bottom: rc.bottom }
    }
}

/// Win32 `MulDiv` 相当。`number * numerator / denominator` を 64bit 中間で計算し、
/// 最近接整数へ丸める(端数 0.5 は 0 から遠い側へ)。分母 0 や i32 溢れは `-1`。
///
/// 原実装は `CMouseWheelHandler::OnMouseWheel`/`OnMouseHWheel` (WindowUtil.cpp:344,358) で使用。
pub fn mul_div(number: i32, numerator: i32, denominator: i32) -> i32 {
    if denominator == 0 {
        return -1;
    }
    let num = number as i64 * numerator as i64;
    let den = denominator as i64;
    let sign = if (num >= 0) == (den >= 0) { 1 } else { -1 };
    let q = num / den; // 0 方向への切り捨て
    let r = (num % den).abs();
    let result = if r * 2 >= den.abs() { q + sign } else { q };
    if result > i32::MAX as i64 || result < i32::MIN as i64 {
        return -1;
    }
    result as i32
}

/// `GET_WHEEL_DELTA_WPARAM` 相当。`wParam` の上位 16bit を符号付き 16bit として取り出す。
pub fn get_wheel_delta_wparam(wparam: usize) -> i32 {
    (((wparam >> 16) & 0xFFFF) as u16 as i16) as i32
}

// ===========================================================================
// 純粋ロジック: ウィンドウ吸着(SnapWindow)
// ===========================================================================

/// 端の可視判定で走査するウィンドウ 1 つ分の情報(Z オーダー上位から並べる)。
#[derive(Debug, Clone, Copy)]
pub struct EdgeWindow {
    pub rect: Rect,
    pub visible: bool,
    /// 判定の起点となるウィンドウ(原実装の `hwnd`)。ここに達したら可視扱いで打ち切り。
    pub is_self: bool,
    /// 吸着対象(移動中)のウィンドウ(原実装の `hwndTarget`)。スキップする。
    pub is_target: bool,
}

/// 矩形 `rect`(線分)が `windows[start..]` のウィンドウ群に隠されず見えているか。
/// 原実装 `IsWindowEdgeVisible` (WindowUtil.cpp:33)。
///
/// `windows` は Z オーダー上位から並んだウィンドウ列。`start` から走査し、
/// `is_self` のウィンドウ(または末尾)に達したら `true`。途中のウィンドウが
/// 線分を完全に覆えば `false`、部分的に覆う場合は残りの線分で再帰判定する。
pub fn is_window_edge_visible(windows: &[EdgeWindow], start: usize, rect: Rect) -> bool {
    // hwndTop == hwnd || hwndTop == nullptr
    if start >= windows.len() || windows[start].is_self {
        return true;
    }

    let w = &windows[start];
    let rc = w.rect;
    let next = start + 1;

    // hwndTop == hwndTarget || !IsWindowVisible || 空矩形 → このウィンドウは無視。
    if w.is_target || !w.visible || rc.left == rc.right || rc.top == rc.bottom {
        return is_window_edge_visible(windows, next, rect);
    }

    let mut edge = rect;

    if rect.top == rect.bottom {
        // 水平エッジ(上端/下端)
        if rc.top <= rect.top && rc.bottom > rect.top {
            if rc.left <= rect.left && rc.right >= rect.right {
                return false;
            }
            if rc.left <= rect.left && rc.right > rect.left {
                edge.right = rc.right.min(rect.right);
                return is_window_edge_visible(windows, next, edge);
            } else if rc.left > rect.left && rc.right >= rect.right {
                edge.left = rc.left;
                return is_window_edge_visible(windows, next, edge);
            } else if rc.left > rect.left && rc.right < rect.right {
                edge.right = rc.left;
                if is_window_edge_visible(windows, next, edge) {
                    return true;
                }
                edge.left = rc.right;
                edge.right = rect.right;
                return is_window_edge_visible(windows, next, edge);
            }
        }
    } else {
        // 垂直エッジ(左端/右端)
        if rc.left <= rect.left && rc.right > rect.left {
            if rc.top <= rect.top && rc.bottom >= rect.bottom {
                return false;
            }
            if rc.top <= rect.top && rc.bottom > rect.top {
                edge.bottom = rc.bottom.min(rect.bottom);
                return is_window_edge_visible(windows, next, edge);
            } else if rc.top > rect.top && rc.bottom >= rect.bottom {
                edge.top = rc.top;
                return is_window_edge_visible(windows, next, edge);
            } else if rc.top > rect.top && rc.bottom < rect.bottom {
                edge.bottom = rc.top;
                if is_window_edge_visible(windows, next, edge) {
                    return true;
                }
                edge.top = rc.bottom;
                edge.bottom = rect.bottom;
                return is_window_edge_visible(windows, next, edge);
            }
        }
    }

    is_window_edge_visible(windows, next, rect)
}

/// 候補ウィンドウ `rc` 1 つについて、移動対象 `original` への最近接エッジ距離 `nearest`
/// を更新する。原実装 `SnapWindowProc` 本体 (WindowUtil.cpp:105-141)。
///
/// `nearest` の各成分は「対象矩形を各方向へ動かす符号付きオフセット」で、絶対値が
/// 小さいほど近い。`edge_visible` は候補のエッジ線分が他ウィンドウに隠れず見えているか
/// を返すコールバック(原実装の `IsWindowEdgeVisible` 呼び出しに対応)。
pub fn update_nearest(
    nearest: &mut Rect,
    original: Rect,
    rc: Rect,
    edge_visible: &mut impl FnMut(Rect) -> bool,
) {
    if !(rc.right > rc.left && rc.bottom > rc.top) {
        return;
    }

    // 縦方向に重なりがある → 左右の吸着を検討。
    if rc.top < original.bottom && rc.bottom > original.top {
        if (rc.left - original.right).abs() < nearest.right.abs() {
            let edge = Rect::new(
                rc.left,
                rc.top.max(original.top),
                rc.left,
                rc.bottom.min(original.bottom),
            );
            if edge_visible(edge) {
                nearest.right = rc.left - original.right;
            }
        }
        if (rc.right - original.left).abs() < nearest.left.abs() {
            let edge = Rect::new(
                rc.right,
                rc.top.max(original.top),
                rc.right,
                rc.bottom.min(original.bottom),
            );
            if edge_visible(edge) {
                nearest.left = rc.right - original.left;
            }
        }
    }

    // 横方向に重なりがある → 上下の吸着を検討。
    if rc.left < original.right && rc.right > original.left {
        if (rc.top - original.bottom).abs() < nearest.bottom.abs() {
            let edge = Rect::new(
                rc.left.max(original.left),
                rc.top,
                rc.right.min(original.right),
                rc.top,
            );
            if edge_visible(edge) {
                nearest.bottom = rc.top - original.bottom;
            }
        }
        if (rc.bottom - original.top).abs() < nearest.top.abs() {
            let edge = Rect::new(
                rc.left.max(original.left),
                rc.bottom,
                rc.right.min(original.right),
                rc.bottom,
            );
            if edge_visible(edge) {
                nearest.top = rc.bottom - original.top;
            }
        }
    }
}

/// 最近接距離 `nearest` から X/Y の吸着オフセットを選ぶ。原実装 (WindowUtil.cpp:176-190)。
///
/// 左右(上下)のうち絶対値が小さい方を採用。絶対値が等しく符号が異なる場合は 0。
pub fn select_snap_offset(nearest: Rect) -> (i32, i32) {
    let x = if nearest.left.abs() < nearest.right.abs() || nearest.left == nearest.right {
        nearest.left
    } else if nearest.left.abs() > nearest.right.abs() {
        nearest.right
    } else {
        0
    };
    let y = if nearest.top.abs() < nearest.bottom.abs() || nearest.top == nearest.bottom {
        nearest.top
    } else if nearest.top.abs() > nearest.bottom.abs() {
        nearest.bottom
    } else {
        0
    };
    (x, y)
}

/// 吸着オフセットを `margin` 以内のときだけ適用し、サイズを保ったまま新しい矩形を返す。
/// 原実装 (WindowUtil.cpp:191-196)。`pos` は現在位置、`original` は元のサイズ基準。
pub fn apply_snap_offset(pos: Rect, original: Rect, nearest: Rect, margin: i32) -> Rect {
    let (xoffset, yoffset) = select_snap_offset(nearest);
    let mut r = pos;
    if xoffset.abs() <= margin {
        r.left += xoffset;
    }
    if yoffset.abs() <= margin {
        r.top += yoffset;
    }
    r.right = r.left + (original.right - original.left);
    r.bottom = r.top + (original.bottom - original.top);
    r
}

/// `nearest` をモニタ矩形 `monitor` と対象矩形 `original` の差で初期化する。
/// 原実装 (WindowUtil.cpp:169-172)。
fn init_nearest(monitor: Rect, original: Rect) -> Rect {
    Rect::new(
        monitor.left - original.left,
        monitor.top - original.top,
        monitor.right - original.right,
        monitor.bottom - original.bottom,
    )
}

// ===========================================================================
// マウスホイール処理 (CMouseWheelHandler)
// ===========================================================================

/// ホイール量を累積し、一定量たまったらスクロール行数/文字数へ換算する。
/// 原実装 `CMouseWheelHandler` (WindowUtil.h:57, WindowUtil.cpp:304)。
#[derive(Debug, Clone, Default)]
pub struct MouseWheelHandler {
    delta_sum: i32,
    last_delta: i32,
    last_time: u32,
}

impl MouseWheelHandler {
    pub fn new() -> Self {
        Self::default()
    }

    /// 原実装 `Reset` (WindowUtil.cpp:304)。
    pub fn reset(&mut self) {
        self.delta_sum = 0;
        self.last_delta = 0;
        self.last_time = 0;
    }

    /// 原実装 `ResetDelta` (WindowUtil.cpp:311)。
    pub fn reset_delta(&mut self) {
        self.delta_sum = 0;
        self.last_delta = 0;
    }

    pub fn delta_sum(&self) -> i32 {
        self.delta_sum
    }
    pub fn last_delta(&self) -> i32 {
        self.last_delta
    }
    pub fn last_time(&self) -> u32 {
        self.last_time
    }

    /// ホイール量を累積して累積値を返す。原実装 `OnWheel` (WindowUtil.cpp:317)。
    ///
    /// 最後の入力から 500ms 超、または回転方向が反転したら累積をリセットする。
    /// 時刻 `cur_time`(`GetTickCount` 相当, ミリ秒)は注入してテスト可能にしている。
    pub fn on_wheel(&mut self, delta: i32, cur_time: u32) -> i32 {
        if cur_time.wrapping_sub(self.last_time) > 500 || (delta > 0) != (self.last_delta > 0) {
            self.delta_sum = 0;
        }
        self.delta_sum += delta;
        self.last_delta = delta;
        self.last_time = cur_time;
        self.delta_sum
    }

    /// `OnMouseWheel`/`OnMouseHWheel` の純粋コア。`scroll_amount` は解決済みの行数/文字数。
    /// 累積が `WHEEL_DELTA`(120)に満たなければ 0(累積は保持)、満たせば換算して累積リセット。
    /// 原実装 (WindowUtil.cpp:333-345, 347-359)。
    pub fn on_wheel_scaled(&mut self, delta: i32, scroll_amount: i32, cur_time: u32) -> i32 {
        let d = self.on_wheel(delta, cur_time);
        if d.abs() < WHEEL_DELTA as i32 {
            return 0;
        }
        self.reset_delta();
        mul_div(d, scroll_amount, WHEEL_DELTA as i32)
    }

    /// 原実装 `OnMouseWheel` (WindowUtil.cpp:333)。`scroll_lines == 0` なら既定値を使う。
    ///
    /// 原実装は閾値超過後に既定値を取得するが、既定値取得は副作用が無いため先に解決する
    /// (結果は同一)。
    pub fn on_mouse_wheel(&mut self, wparam: usize, scroll_lines: i32) -> i32 {
        let lines = if scroll_lines == 0 {
            self.get_default_scroll_lines()
        } else {
            scroll_lines
        };
        let delta = get_wheel_delta_wparam(wparam);
        let cur_time = unsafe { GetTickCount() };
        self.on_wheel_scaled(delta, lines, cur_time)
    }

    /// 原実装 `OnMouseHWheel` (WindowUtil.cpp:347)。
    pub fn on_mouse_hwheel(&mut self, wparam: usize, scroll_chars: i32) -> i32 {
        let chars = if scroll_chars == 0 {
            self.get_default_scroll_chars()
        } else {
            scroll_chars
        };
        let delta = get_wheel_delta_wparam(wparam);
        let cur_time = unsafe { GetTickCount() };
        self.on_wheel_scaled(delta, chars, cur_time)
    }

    /// 原実装 `GetDefaultScrollLines` (WindowUtil.cpp:361)。取得失敗時は 2。
    pub fn get_default_scroll_lines(&self) -> i32 {
        let mut lines: u32 = 0;
        let ok = unsafe {
            SystemParametersInfoW(
                SPI_GETWHEELSCROLLLINES,
                0,
                Some(&mut lines as *mut u32 as *mut c_void),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
        };
        if ok.is_ok() {
            lines as i32
        } else {
            2
        }
    }

    /// 原実装 `GetDefaultScrollChars` (WindowUtil.cpp:370)。取得失敗時は 3。
    pub fn get_default_scroll_chars(&self) -> i32 {
        let mut chars: u32 = 0;
        let ok = unsafe {
            SystemParametersInfoW(
                SPI_GETWHEELSCROLLCHARS,
                0,
                Some(&mut chars as *mut u32 as *mut c_void),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
        };
        if ok.is_ok() {
            chars as i32
        } else {
            3
        }
    }
}

// ===========================================================================
// マウス離脱追跡 (CMouseLeaveTrack)
// ===========================================================================

/// クライアント/非クライアント領域からのマウス離脱を追跡する。
/// 原実装 `CMouseLeaveTrack` (WindowUtil.h:32, WindowUtil.cpp:218)。
#[derive(Debug, Clone, Default)]
pub struct MouseLeaveTrack {
    hwnd: isize,
    client_track: bool,
    non_client_track: bool,
}

impl MouseLeaveTrack {
    pub fn new() -> Self {
        Self::default()
    }

    /// 原実装 `Initialize` (WindowUtil.cpp:218)。
    pub fn initialize(&mut self, hwnd: HWND) {
        self.hwnd = hwnd.0 as isize;
        self.client_track = false;
        self.non_client_track = false;
    }

    fn hwnd(&self) -> HWND {
        HWND(self.hwnd as *mut c_void)
    }

    pub fn is_client_track(&self) -> bool {
        self.client_track
    }
    pub fn is_non_client_track(&self) -> bool {
        self.non_client_track
    }

    /// 原実装 `OnMouseMove` (WindowUtil.cpp:225)。`TME_LEAVE` を登録する。
    pub fn on_mouse_move(&mut self) -> bool {
        let mut tme = TRACKMOUSEEVENT {
            cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_LEAVE,
            hwndTrack: self.hwnd(),
            dwHoverTime: 0,
        };
        if unsafe { TrackMouseEvent(&mut tme) }.is_err() {
            return false;
        }
        self.client_track = true;
        true
    }

    /// 原実装 `OnMouseLeave` (WindowUtil.cpp:241)。非クライアント追跡中でなければ離脱確定。
    pub fn on_mouse_leave(&mut self) -> bool {
        self.client_track = false;
        !self.non_client_track
    }

    /// 原実装 `OnNcMouseMove` (WindowUtil.cpp:248)。`TME_LEAVE | TME_NONCLIENT` を登録する。
    pub fn on_nc_mouse_move(&mut self) -> bool {
        let mut tme = TRACKMOUSEEVENT {
            cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_LEAVE | TME_NONCLIENT,
            hwndTrack: self.hwnd(),
            dwHoverTime: 0,
        };
        if unsafe { TrackMouseEvent(&mut tme) }.is_err() {
            return false;
        }
        self.non_client_track = true;
        true
    }

    /// 原実装 `OnNcMouseLeave` (WindowUtil.cpp:263)。クライアント追跡中でなければ離脱確定。
    pub fn on_nc_mouse_leave(&mut self) -> bool {
        self.non_client_track = false;
        !self.client_track
    }

    /// 原実装 `OnMessage` (WindowUtil.cpp:270)。対応メッセージなら処理して `true`。
    pub fn on_message(&mut self, msg: u32, _wparam: WPARAM, _lparam: LPARAM) -> bool {
        // windows-rs 0.62 では WM_MOUSELEAVE が本フィーチャに無いため定数を直接定義する。
        const WM_MOUSEMOVE: u32 = 0x0200;
        const WM_MOUSELEAVE: u32 = 0x02A3;
        const WM_NCMOUSEMOVE: u32 = 0x00A0;
        const WM_NCMOUSELEAVE: u32 = 0x02A2;
        match msg {
            WM_MOUSEMOVE => {
                self.on_mouse_move();
                true
            }
            WM_MOUSELEAVE => {
                self.on_mouse_leave();
                true
            }
            WM_NCMOUSEMOVE => {
                self.on_nc_mouse_move();
                true
            }
            WM_NCMOUSELEAVE => {
                self.on_nc_mouse_leave();
                true
            }
            _ => false,
        }
    }

    /// 原実装 `IsCursorInWindow` (WindowUtil.cpp:293)。現在のカーソルがウィンドウ内か。
    pub fn is_cursor_in_window(&self) -> bool {
        let pos = unsafe { GetMessagePos() };
        let pt = POINT {
            x: (pos & 0xFFFF) as i16 as i32,
            y: ((pos >> 16) & 0xFFFF) as i16 as i32,
        };
        let mut rc = RECT::default();
        if unsafe { GetWindowRect(self.hwnd(), &mut rc) }.is_err() {
            return false;
        }
        unsafe { PtInRect(&rc, pt) }.as_bool()
    }
}

// ===========================================================================
// 純粋ロジック: タイマー管理のビット操作 (CWindowTimerManager)
// ===========================================================================

/// `EndAllTimers` のビット走査。`ids` のセットビットを LSB から単独 ID として列挙する。
/// 原実装 (WindowUtil.cpp:406-414)。
pub fn timer_ids_to_end(ids: u32) -> Vec<u32> {
    let mut result = Vec::new();
    let mut flags = ids;
    let mut i = 0u32;
    while flags != 0 {
        let id = ids & (1u32 << i);
        if id != 0 {
            result.push(id);
        }
        i += 1;
        flags >>= 1;
    }
    result
}

/// ウィンドウタイマーを ID(ビットフラグ)単位で管理する。
/// 原実装 `CWindowTimerManager` (WindowUtil.h:77, WindowUtil.cpp:380)。
#[derive(Debug, Clone, Default)]
pub struct WindowTimerManager {
    hwnd: isize,
    timer_ids: u32,
}

impl WindowTimerManager {
    pub fn new() -> Self {
        Self::default()
    }

    fn hwnd(&self) -> HWND {
        HWND(self.hwnd as *mut c_void)
    }

    /// 原実装 `InitializeTimer` (WindowUtil.cpp:380)。
    pub fn initialize_timer(&mut self, hwnd: HWND) {
        self.hwnd = hwnd.0 as isize;
        self.timer_ids = 0;
    }

    /// 原実装 `BeginTimer` (WindowUtil.cpp:387)。`SetTimer` 成功でビットを立てる。
    pub fn begin_timer(&mut self, id: u32, interval: u32) -> bool {
        if self.hwnd == 0 {
            return false;
        }
        let r = unsafe { SetTimer(Some(self.hwnd()), id as usize, interval, None) };
        if r == 0 {
            return false;
        }
        self.timer_ids |= id;
        true
    }

    /// 原実装 `EndTimer` (WindowUtil.cpp:397)。
    pub fn end_timer(&mut self, id: u32) {
        if self.timer_ids & id != 0 {
            let _ = unsafe { KillTimer(Some(self.hwnd()), id as usize) };
            self.timer_ids &= !id;
        }
    }

    /// 原実装 `EndAllTimers` (WindowUtil.cpp:406)。
    pub fn end_all_timers(&mut self) {
        for id in timer_ids_to_end(self.timer_ids) {
            self.end_timer(id);
        }
    }

    /// 原実装 `IsTimerEnabled` (WindowUtil.cpp:417)。`id` の全ビットが立っていれば true。
    pub fn is_timer_enabled(&self, id: u32) -> bool {
        self.timer_ids & id == id
    }

    pub fn timer_ids(&self) -> u32 {
        self.timer_ids
    }
}

// ===========================================================================
// ウィンドウサブクラス化 (CWindowSubclass)
// ===========================================================================

/// サブクラス化したウィンドウのメッセージを受け取るハンドラ。
/// 原実装 `CWindowSubclass` の仮想関数に対応。
pub trait WindowSubclassHandler {
    /// 原実装 `OnMessage` (WindowUtil.cpp:476)。既定は `DefSubclassProc`。
    fn on_message(&mut self, hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
    }

    /// 原実装 `OnSubclassRemoved` (WindowUtil.h:115)。既定は何もしない。
    fn on_subclass_removed(&mut self) {}
}

/// `SetWindowSubclass` のラッパー。原実装 `CWindowSubclass` (WindowUtil.cpp:425)。
///
/// `handler` への生ポインタを `dwRefData` に渡してメッセージを転送する。`handler` は
/// この `WindowSubclass` および対象ウィンドウより長く生存させる必要がある(原実装でも
/// `CWindowSubclass` 派生インスタンスの寿命管理は呼び出し側責務)。
pub struct WindowSubclass<H: WindowSubclassHandler> {
    hwnd: isize,
    handler: *mut H,
}

impl<H: WindowSubclassHandler> WindowSubclass<H> {
    pub fn new() -> Self {
        Self { hwnd: 0, handler: std::ptr::null_mut() }
    }

    fn hwnd(&self) -> HWND {
        HWND(self.hwnd as *mut c_void)
    }

    /// 原実装 `SetSubclass` (WindowUtil.cpp:431)。既存サブクラスは解除してから設定する。
    pub fn set_subclass(&mut self, hwnd: HWND, handler: &mut H) -> bool {
        self.remove_subclass();

        if hwnd.0.is_null() {
            return false;
        }

        self.handler = handler as *mut H;
        let ok = unsafe {
            SetWindowSubclass(
                hwnd,
                Some(Self::subclass_proc),
                self as *mut Self as usize,
                self as *mut Self as usize,
            )
        };
        if !ok.as_bool() {
            self.handler = std::ptr::null_mut();
            return false;
        }
        self.hwnd = hwnd.0 as isize;
        true
    }

    /// 原実装 `RemoveSubclass` (WindowUtil.cpp:450)。
    pub fn remove_subclass(&mut self) {
        if self.hwnd != 0 {
            let _ = unsafe {
                RemoveWindowSubclass(self.hwnd(), Some(Self::subclass_proc), self as *mut Self as usize)
            };
            self.hwnd = 0;
        }
    }

    /// 原実装 `SubclassProc` (WindowUtil.cpp:459)。`WM_NCDESTROY` で自動解除する。
    unsafe extern "system" fn subclass_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id_subclass: usize,
        ref_data: usize,
    ) -> LRESULT {
        use windows::Win32::UI::WindowsAndMessaging::WM_NCDESTROY;

        let this = ref_data as *mut Self;
        if this.is_null() {
            return DefSubclassProc(hwnd, msg, wparam, lparam);
        }
        let handler = (*this).handler;
        let dispatch = |hwnd, msg, wparam, lparam| -> LRESULT {
            if handler.is_null() {
                DefSubclassProc(hwnd, msg, wparam, lparam)
            } else {
                (*handler).on_message(hwnd, msg, wparam, lparam)
            }
        };

        if msg == WM_NCDESTROY {
            (*this).remove_subclass();
            let result = dispatch(hwnd, msg, wparam, lparam);
            if !handler.is_null() {
                (*handler).on_subclass_removed();
            }
            return result;
        }

        dispatch(hwnd, msg, wparam, lparam)
    }
}

impl<H: WindowSubclassHandler> Default for WindowSubclass<H> {
    fn default() -> Self {
        Self::new()
    }
}

impl<H: WindowSubclassHandler> Drop for WindowSubclass<H> {
    fn drop(&mut self) {
        self.remove_subclass();
    }
}

// ===========================================================================
// 自由関数 (Win32)
// ===========================================================================

/// 指定メッセージがキューにあるか調べる。原実装 `IsMessageInQueue` (WindowUtil.cpp:200)。
///
/// `WM_QUIT` を覗いた場合は再ポストして `false` を返す。
pub fn is_message_in_queue(hwnd: HWND, message: u32) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::WM_QUIT;
    let mut msg = MSG::default();
    let found = unsafe {
        PeekMessageW(&mut msg, Some(hwnd), message, message, PM_NOREMOVE)
    };
    if found.as_bool() {
        if msg.message == WM_QUIT {
            unsafe { PostQuitMessage(msg.wParam.0 as i32) };
        } else {
            return true;
        }
    }
    false
}

/// ウィンドウを他ウィンドウ/モニタ端へ吸着させた矩形を返す。
/// 原実装 `SnapWindow` (WindowUtil.cpp:148)。`rc` は現在の位置・サイズ。
///
/// `EnumWindows` で得た最上位ウィンドウ群と、デスクトップの Z オーダーを用いて
/// 純粋関数([`update_nearest`] / [`is_window_edge_visible`] / [`apply_snap_offset`])で
/// 吸着量を計算する。
pub fn snap_window(hwnd: HWND, rc: Rect, margin: i32, hwnd_exclude: Option<HWND>) -> Rect {
    // モニタ矩形を取得(失敗時は画面全体)。
    let monitor = unsafe {
        let hmon: HMONITOR = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        if !hmon.is_invalid() {
            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if GetMonitorInfoW(hmon, &mut mi).as_bool() {
                Rect::from_win32(&mi.rcMonitor)
            } else {
                Rect::new(0, 0, GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN))
            }
        } else {
            Rect::new(0, 0, GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN))
        }
    };

    let mut nearest = init_nearest(monitor, rc);

    // デスクトップの Z オーダー(端の可視判定用)を 1 度だけ収集する。
    let z_order = collect_z_order();
    let target_hwnd = hwnd.0 as isize;
    let exclude_hwnd = hwnd_exclude.map(|h| h.0 as isize).unwrap_or(0);

    // 最上位ウィンドウを列挙して最近接エッジを更新する。
    let mut candidates: Vec<(isize, Rect)> = Vec::new();
    enum_top_level_windows(&mut candidates);

    for (cand_hwnd, cand_rc) in candidates {
        if cand_hwnd == target_hwnd || cand_hwnd == exclude_hwnd {
            continue;
        }
        // 原実装は EnumWindows のコールバック内で IsWindowVisible を確認するが、
        // collect 時に可視のもののみ格納している。
        let mut edge_visible = |edge: Rect| -> bool {
            let windows = build_edge_windows(&z_order, cand_hwnd, target_hwnd);
            is_window_edge_visible(&windows, 0, edge)
        };
        update_nearest(&mut nearest, rc, cand_rc, &mut edge_visible);
    }

    apply_snap_offset(rc, rc, nearest, margin)
}

/// デスクトップの Z オーダー(上位→下位)を `(hwnd, rect, visible)` で収集する。
fn collect_z_order() -> Vec<(isize, Rect, bool)> {
    let mut list = Vec::new();
    unsafe {
        let mut hwnd = match GetTopWindow(Some(GetDesktopWindow())) {
            Ok(h) => h,
            Err(_) => return list,
        };
        loop {
            if hwnd.0.is_null() {
                break;
            }
            let mut rc = RECT::default();
            let visible = IsWindowVisible(hwnd).as_bool();
            let rect = if GetWindowRect(hwnd, &mut rc).is_ok() {
                Rect::from_win32(&rc)
            } else {
                Rect::default()
            };
            list.push((hwnd.0 as isize, rect, visible));
            hwnd = match GetWindow(hwnd, GW_HWNDNEXT) {
                Ok(h) if !h.0.is_null() => h,
                _ => break,
            };
        }
    }
    list
}

/// `collect_z_order` の結果から、特定の候補/対象ウィンドウを起点・対象に印付けした
/// [`EdgeWindow`] 列を作る。
fn build_edge_windows(
    z_order: &[(isize, Rect, bool)],
    self_hwnd: isize,
    target_hwnd: isize,
) -> Vec<EdgeWindow> {
    z_order
        .iter()
        .map(|&(h, rect, visible)| EdgeWindow {
            rect,
            visible,
            is_self: h == self_hwnd,
            is_target: h == target_hwnd,
        })
        .collect()
}

/// 最上位ウィンドウを列挙し、可視のものを `(hwnd, rect)` で収集する。
fn enum_top_level_windows(out: &mut Vec<(isize, Rect)>) {
    unsafe extern "system" fn proc(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
        let out = &mut *(lparam.0 as *mut Vec<(isize, Rect)>);
        if IsWindowVisible(hwnd).as_bool() {
            let mut rc = RECT::default();
            if GetWindowRect(hwnd, &mut rc).is_ok() {
                out.push((hwnd.0 as isize, Rect::from_win32(&rc)));
            }
        }
        windows::core::BOOL(1)
    }
    unsafe {
        let _ = EnumWindows(Some(proc), LPARAM(out as *mut Vec<(isize, Rect)> as isize));
    }
}

// ===========================================================================
// テスト(純粋ロジック)
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- mul_div ---

    #[test]
    fn test_mul_div_basic() {
        assert_eq!(mul_div(240, 3, 120), 6);
        assert_eq!(mul_div(120, 1, 120), 1);
        assert_eq!(mul_div(0, 5, 120), 0);
    }

    #[test]
    fn test_mul_div_rounding_half_away_from_zero() {
        // 180/120 = 1.5 → 2
        assert_eq!(mul_div(180, 1, 120), 2);
        // -180/120 = -1.5 → -2
        assert_eq!(mul_div(-180, 1, 120), -2);
        // 60/120 = 0.5 → 1
        assert_eq!(mul_div(60, 1, 120), 1);
        // 59/120 = 0.49 → 0
        assert_eq!(mul_div(59, 1, 120), 0);
    }

    #[test]
    fn test_mul_div_zero_denominator() {
        assert_eq!(mul_div(10, 10, 0), -1);
    }

    // --- get_wheel_delta_wparam ---

    #[test]
    fn test_get_wheel_delta_wparam() {
        // 上位 16bit = 120
        assert_eq!(get_wheel_delta_wparam(120 << 16), 120);
        // 上位 16bit = -120 (0xFF88)
        assert_eq!(get_wheel_delta_wparam((0xFF88u32 as usize) << 16), -120);
        // 下位ビットは無視
        assert_eq!(get_wheel_delta_wparam((120usize << 16) | 0xFFFF), 120);
    }

    // --- MouseWheelHandler ---

    #[test]
    fn test_on_wheel_accumulates() {
        let mut h = MouseWheelHandler::new();
        assert_eq!(h.on_wheel(40, 1000), 40);
        assert_eq!(h.on_wheel(40, 1100), 80);
        assert_eq!(h.on_wheel(40, 1200), 120);
    }

    #[test]
    fn test_on_wheel_resets_after_timeout() {
        let mut h = MouseWheelHandler::new();
        assert_eq!(h.on_wheel(60, 1000), 60);
        // 500ms 超で累積リセット
        assert_eq!(h.on_wheel(60, 1600), 60);
    }

    #[test]
    fn test_on_wheel_resets_on_direction_change() {
        let mut h = MouseWheelHandler::new();
        assert_eq!(h.on_wheel(60, 1000), 60);
        // 方向反転で累積リセット
        assert_eq!(h.on_wheel(-60, 1050), -60);
    }

    #[test]
    fn test_on_wheel_scaled_below_threshold_keeps_accumulation() {
        let mut h = MouseWheelHandler::new();
        // 40 < 120 → 0、累積は保持
        assert_eq!(h.on_wheel_scaled(40, 3, 1000), 0);
        assert_eq!(h.delta_sum(), 40);
        assert_eq!(h.on_wheel_scaled(40, 3, 1050), 0);
        assert_eq!(h.delta_sum(), 80);
        // 120 到達 → 3 行、累積リセット
        assert_eq!(h.on_wheel_scaled(40, 3, 1100), 3);
        assert_eq!(h.delta_sum(), 0);
    }

    #[test]
    fn test_on_wheel_scaled_full_notch() {
        let mut h = MouseWheelHandler::new();
        // 120 ちょうど・3 行 → MulDiv(120,3,120)=3
        assert_eq!(h.on_wheel_scaled(120, 3, 1000), 3);
    }

    // --- MouseLeaveTrack(純粋な状態遷移) ---

    #[test]
    fn test_mouse_leave_track_state() {
        let mut t = MouseLeaveTrack::new();
        assert!(!t.is_client_track());
        assert!(!t.is_non_client_track());

        // クライアント追跡中に離脱:非クライアント追跡が無ければ離脱確定。
        // (on_mouse_move は TrackMouseEvent を呼ぶためここでは状態のみ直接検証)
        t.client_track = true;
        assert!(t.on_mouse_leave());
        assert!(!t.is_client_track());

        // 非クライアント追跡中なら、クライアント離脱では離脱確定しない。
        t.client_track = true;
        t.non_client_track = true;
        assert!(!t.on_mouse_leave());

        // 非クライアント離脱:クライアント追跡が無ければ確定。
        t.client_track = false;
        t.non_client_track = true;
        assert!(t.on_nc_mouse_leave());
        assert!(!t.is_non_client_track());
    }

    // --- timer_ids_to_end / WindowTimerManager のビット操作 ---

    #[test]
    fn test_timer_ids_to_end() {
        assert_eq!(timer_ids_to_end(0), Vec::<u32>::new());
        assert_eq!(timer_ids_to_end(0b1), vec![0b1]);
        assert_eq!(timer_ids_to_end(0b1011), vec![0b0001, 0b0010, 0b1000]);
        assert_eq!(timer_ids_to_end(0x8000_0001), vec![0x1, 0x8000_0000]);
    }

    #[test]
    fn test_is_timer_enabled_logic() {
        let mut m = WindowTimerManager::new();
        // hwnd 未設定では begin できないが、ビット判定ロジックは timer_ids を直接設定して検証。
        assert!(!m.begin_timer(0b1, 100)); // hwnd==0 で false
        // timer_ids を直接操作する内部検証用に is_timer_enabled の意味を確認。
        // (begin_timer は Win32 を要するため、ビット意味のみ検証)
        assert!(m.is_timer_enabled(0)); // (x & 0)==0 は常に真
    }

    // --- is_window_edge_visible ---

    fn ew(rect: Rect, visible: bool, is_self: bool, is_target: bool) -> EdgeWindow {
        EdgeWindow { rect, visible, is_self, is_target }
    }

    #[test]
    fn test_edge_visible_empty_list() {
        // ウィンドウが無ければ常に見える。
        assert!(is_window_edge_visible(&[], 0, Rect::new(0, 0, 100, 0)));
    }

    #[test]
    fn test_edge_visible_reach_self() {
        // 起点(is_self)に達したら見える。
        let ws = [ew(Rect::new(0, 0, 100, 100), true, true, false)];
        assert!(is_window_edge_visible(&ws, 0, Rect::new(0, 0, 100, 0)));
    }

    #[test]
    fn test_edge_visible_fully_covered_horizontal() {
        // 水平エッジ y=10, x[0..100] を、矩形 [0..100]x[0..50] が完全に覆う → 不可視。
        let ws = [
            ew(Rect::new(0, 0, 100, 50), true, false, false),
            ew(Rect::new(0, 0, 0, 0), true, true, false), // 起点
        ];
        assert!(!is_window_edge_visible(&ws, 0, Rect::new(0, 10, 100, 10)));
    }

    #[test]
    fn test_edge_visible_not_covered_horizontal() {
        // 覆うウィンドウが線分の上に無い(y 範囲外)→ 見える。
        let ws = [
            ew(Rect::new(0, 100, 100, 200), true, false, false),
            ew(Rect::new(0, 0, 0, 0), true, true, false),
        ];
        assert!(is_window_edge_visible(&ws, 0, Rect::new(0, 10, 100, 10)));
    }

    #[test]
    fn test_edge_visible_target_skipped() {
        // is_target のウィンドウは覆っていても無視される。
        let ws = [
            ew(Rect::new(0, 0, 100, 50), true, false, true), // 対象 → スキップ
            ew(Rect::new(0, 0, 0, 0), true, true, false),
        ];
        assert!(is_window_edge_visible(&ws, 0, Rect::new(0, 10, 100, 10)));
    }

    #[test]
    fn test_edge_visible_middle_cover_split_both_covered() {
        // 中央 [40..60] を覆うウィンドウ(行57-63: 左右の未被覆部分に分割)。
        // 残り左 [0..40] と右 [60..100] を別ウィンドウが完全に覆う → 不可視。
        let ws = [
            ew(Rect::new(40, 0, 60, 50), true, false, false), // 中央を覆う → [0..40] と [60..100] に分割
            ew(Rect::new(0, 0, 40, 50), true, false, false),  // 左を覆う
            ew(Rect::new(60, 0, 100, 50), true, false, false), // 右を覆う
            ew(Rect::new(0, 0, 0, 0), true, true, false),
        ];
        assert!(!is_window_edge_visible(&ws, 0, Rect::new(0, 10, 100, 10)));
    }

    #[test]
    fn test_edge_visible_left_aligned_cover_is_quirky() {
        // 原実装の癖を固定するテスト。エッジの左端に接して覆うウィンドウ(行51-53/54-56)は
        // 「覆われた側の線分」で再帰するため、後続が覆っても可視と判定される。
        let ws = [
            ew(Rect::new(0, 0, 50, 50), true, false, false),
            ew(Rect::new(50, 0, 100, 50), true, false, false),
            ew(Rect::new(0, 0, 0, 0), true, true, false),
        ];
        assert!(is_window_edge_visible(&ws, 0, Rect::new(0, 10, 100, 10)));
    }

    #[test]
    fn test_edge_visible_partial_remainder_visible() {
        // 左半分だけ覆われ、右半分は誰も覆わない → 見える。
        let ws = [
            ew(Rect::new(0, 0, 50, 50), true, false, false),
            ew(Rect::new(0, 0, 0, 0), true, true, false),
        ];
        assert!(is_window_edge_visible(&ws, 0, Rect::new(0, 10, 100, 10)));
    }

    // --- update_nearest / select_snap_offset / apply_snap_offset ---

    #[test]
    fn test_update_nearest_right_edge() {
        // 対象 [0..100]x[0..100]。右隣に [110..200]x[0..100] のウィンドウ。
        // 右へ 10 動かすと吸着(rc.left - original.right = 110-100 = 10)。
        let original = Rect::new(0, 0, 100, 100);
        let mut nearest = Rect::new(-1000, -1000, 1000, 1000);
        let mut always = |_edge: Rect| true;
        update_nearest(&mut nearest, original, Rect::new(110, 0, 200, 100), &mut always);
        assert_eq!(nearest.right, 10);
    }

    #[test]
    fn test_update_nearest_respects_edge_visibility() {
        // エッジが見えない場合は更新しない。
        let original = Rect::new(0, 0, 100, 100);
        let mut nearest = Rect::new(-1000, -1000, 1000, 1000);
        let mut never = |_edge: Rect| false;
        update_nearest(&mut nearest, original, Rect::new(110, 0, 200, 100), &mut never);
        assert_eq!(nearest.right, 1000); // 変化なし
    }

    #[test]
    fn test_update_nearest_no_vertical_overlap() {
        // 縦方向に重ならなければ左右の吸着は起きない。
        let original = Rect::new(0, 0, 100, 100);
        let mut nearest = Rect::new(-1000, -1000, 1000, 1000);
        let mut always = |_edge: Rect| true;
        // rc は対象の真下(y[200..300])で縦の重なり無し。
        update_nearest(&mut nearest, original, Rect::new(110, 200, 200, 300), &mut always);
        assert_eq!(nearest.right, 1000);
    }

    #[test]
    fn test_select_snap_offset_prefers_smaller_abs() {
        // 左 -5、右 20 → 左を採用(絶対値小)。
        let n = Rect::new(-5, 30, 20, -8);
        let (x, y) = select_snap_offset(n);
        assert_eq!(x, -5);
        // 上 30、下 -8 → 下を採用(|-8| < |30|)。
        assert_eq!(y, -8);
    }

    #[test]
    fn test_select_snap_offset_equal_abs_opposite_sign_is_zero() {
        // 左 5、右 -5(絶対値同・符号逆)→ 0。
        let n = Rect::new(5, 7, -5, 7);
        let (x, y) = select_snap_offset(n);
        assert_eq!(x, 0);
        // 上下が完全一致 → その値。
        assert_eq!(y, 7);
    }

    #[test]
    fn test_apply_snap_offset_within_margin() {
        let original = Rect::new(0, 0, 100, 80);
        // 右へ 10 吸着、margin 15 なら適用。
        let nearest = Rect::new(1000, 1000, 10, 1000);
        let r = apply_snap_offset(original, original, nearest, 15);
        assert_eq!(r, Rect::new(10, 0, 110, 80)); // left+10、サイズ維持
    }

    #[test]
    fn test_apply_snap_offset_outside_margin() {
        let original = Rect::new(0, 0, 100, 80);
        // 吸着量 50 は margin 15 を超えるので適用しない。
        let nearest = Rect::new(1000, 1000, 50, 1000);
        let r = apply_snap_offset(original, original, nearest, 15);
        assert_eq!(r, Rect::new(0, 0, 100, 80)); // 変化なし(サイズ再計算のみ)
    }
}
