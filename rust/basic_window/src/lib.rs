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

//! TVTest の `CBasicWindow`(原実装 `src/BasicWindow.cpp` / `src/BasicWindow.h`)を
//! `windows` クレート(windows-rs)で Rust へ移植したもの。
//!
//! `CBasicWindow` はウィンドウ(HWND)を保持し、位置・表示・スタイル・メッセージ送出などを
//! ラップする基底クラス。
//!
//! 本クレートは次の純粋計算を Win32 から分離し、Rust の単体テストで原実装と一致させる:
//! ウィンドウ未生成時に保持する位置状態 ([`WindowPosition`])、モニタ内移動
//! ([`move_to_monitor_inside_offset`])、最大化配置のモニタ/作業領域座標変換
//! ([`normalize_placement_to_monitor`] / [`normalize_placement_to_work`])、不透明度検証
//! ([`opacity_is_valid`])。HWND 依存部分は [`BasicWindow`] が薄くラップする。
//!
//! 派生クラスのうち `CCustomWindow`(原実装の WndProc によるメッセージ振り分け)も本クレートで
//! 移植する。メッセージ分類 ([`classify_message`]) と生成/破棄系の戻り値決定
//! ([`nccreate_outcome`] / [`create_outcome`]) を純粋関数に切り出し、`GWLP_USERDATA` を使う
//! トランポリン ([`custom_wnd_proc`]) と仮想関数契約 ([`CustomWindowHandler`]) を提供する。
//! `CPopupWindow` のダークモード状態 ([`PopupDarkModeState`]) は、各 DarkMode API の結果を
//! 引数注入する純粋遷移と、移植済みの [`tvtest_dark_mode`] を呼ぶ Win32 駆動メソッド
//! ([`PopupDarkModeState::handle_create`] / [`PopupDarkModeState::handle_setting_change`])の
//! 二層で表す。
//! 実ウィンドウ生成(原実装で純粋仮想の `Create`)は派生側の責務のため本クレートには含めない。

#![cfg(windows)]

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, InvalidateRect, MapWindowPoints, MonitorFromRect, RedrawWindow, UpdateWindow,
    HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST, RDW_ALLCHILDREN, RDW_ERASE, RDW_FRAME,
    RDW_INVALIDATE, RDW_UPDATENOW, REDRAW_WINDOW_FLAGS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, DestroyWindow, GetClientRect, GetParent, GetWindowLongPtrW, GetWindowLongW,
    GetWindowPlacement, GetWindowRect, IsIconic, IsWindowVisible, IsZoomed, MoveWindow,
    PostMessageW, SendMessageW, SetLayeredWindowAttributes, SetParent, SetWindowLongPtrW,
    SetWindowLongW, SetWindowPlacement, SetWindowPos, ShowWindow, CREATESTRUCTW, GWLP_USERDATA,
    GWL_EXSTYLE, GWL_STYLE, LWA_ALPHA, SWP_DRAWFRAME, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SWP_NOZORDER, SW_HIDE, SW_MAXIMIZE, SW_RESTORE, SW_SHOW, SW_SHOWNORMAL, WINDOWPLACEMENT,
    WM_CREATE, WM_DESTROY, WM_NCCREATE, WM_SIZE, WS_CHILD, WS_EX_LAYERED, WS_EX_TOOLWINDOW,
};

// ===========================================================================
// 純粋ロジック: 矩形・位置・モニタ幾何
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

    fn to_win32(self) -> RECT {
        RECT { left: self.left, top: self.top, right: self.right, bottom: self.bottom }
    }

    /// Win32 `OffsetRect` 相当。矩形全体を平行移動する。
    pub fn offset(self, dx: i32, dy: i32) -> Self {
        Self {
            left: self.left + dx,
            top: self.top + dy,
            right: self.right + dx,
            bottom: self.bottom + dy,
        }
    }
}

/// ウィンドウ未生成時に保持する位置・サイズ・最大化状態。
/// 原実装 `CBasicWindow::m_WindowPosition` (BasicWindow.h:43)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WindowPosition {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
    pub maximized: bool,
}

impl WindowPosition {
    pub fn new() -> Self {
        Self::default()
    }

    /// 位置・サイズを設定する。負のサイズは拒否(原実装 `SetPosition` の検証, BasicWindow.cpp:49)。
    pub fn set(&mut self, left: i32, top: i32, width: i32, height: i32) -> bool {
        if width < 0 || height < 0 {
            return false;
        }
        self.left = left;
        self.top = top;
        self.width = width;
        self.height = height;
        true
    }

    /// `RECT`(左上・右下)から設定する。原実装 `SetPosition(const RECT*)` (BasicWindow.cpp:87)。
    pub fn set_from_rect(&mut self, rc: Rect) -> bool {
        self.set(rc.left, rc.top, rc.right - rc.left, rc.bottom - rc.top)
    }

    /// `(Left, Top, Width, Height)` を返す。
    pub fn get(&self) -> (i32, i32, i32, i32) {
        (self.left, self.top, self.width, self.height)
    }

    /// `RECT`(左上・右下)へ変換する。原実装 `GetPosition(RECT*)` の `SetRect` (BasicWindow.cpp:163)。
    pub fn to_rect(&self) -> Rect {
        Rect::new(self.left, self.top, self.left + self.width, self.top + self.height)
    }
}

/// 矩形 `rc` がモニタ `monitor` の外にはみ出している場合に、内側へ寄せるオフセット
/// `(dx, dy)` を返す。完全に内側(少なくとも一部が重なる)なら `None`。
/// 原実装 `CBasicWindow::MoveToMonitorInside` (BasicWindow.cpp:298-314)。
pub fn move_to_monitor_inside_offset(rc: Rect, monitor: Rect) -> Option<(i32, i32)> {
    if rc.left >= monitor.right
        || rc.top >= monitor.bottom
        || rc.right <= monitor.left
        || rc.bottom <= monitor.top
    {
        let mut x_offset = 0;
        let mut y_offset = 0;
        if rc.left >= monitor.right {
            x_offset = monitor.right - rc.right;
        } else if rc.right <= monitor.left {
            x_offset = monitor.left - rc.left;
        }
        if rc.top >= monitor.bottom {
            y_offset = monitor.bottom - rc.bottom;
        } else if rc.bottom <= monitor.top {
            y_offset = monitor.top - rc.top;
        }
        Some((x_offset, y_offset))
    } else {
        None
    }
}

/// 作業領域基準の配置矩形をモニタ基準へ変換する。原実装 `SetPosition` の `OffsetRect`
/// (BasicWindow.cpp:70-73)。`WINDOWPLACEMENT::rcNormalPosition` は作業領域基準のため、
/// 設定前にモニタ基準へずらす。
pub fn normalize_placement_to_monitor(rect: Rect, monitor: Rect, work: Rect) -> Rect {
    rect.offset(monitor.left - work.left, monitor.top - work.top)
}

/// モニタ基準の配置矩形を作業領域基準へ変換する(上の逆)。原実装 `GetPosition` の
/// `OffsetRect` (BasicWindow.cpp:129-132)。
pub fn normalize_placement_to_work(rect: Rect, monitor: Rect, work: Rect) -> Rect {
    rect.offset(work.left - monitor.left, work.top - monitor.top)
}

/// 不透明度が有効範囲 `0..=255` か。原実装 `SetOpacity` の検証 (BasicWindow.cpp:434)。
pub fn opacity_is_valid(opacity: i32) -> bool {
    (0..=255).contains(&opacity)
}

// ===========================================================================
// Win32 ラッパー: CBasicWindow
// ===========================================================================

/// `CBasicWindow` の具象メソッドを移植したウィンドウラッパー。
/// 原実装 `CBasicWindow` (BasicWindow.h:39, BasicWindow.cpp)。
///
/// ウィンドウ生成(`Create`)は原実装でも純粋仮想で派生クラスが実装するため、本構造体は
/// 生成済みの HWND を保持する/しない両状態を扱う。HWND 未保持時は [`WindowPosition`] を使う。
#[derive(Debug, Default)]
pub struct BasicWindow {
    hwnd: HWND,
    window_position: WindowPosition,
}

impl BasicWindow {
    pub fn new() -> Self {
        Self::default()
    }

    /// 保持している HWND。
    pub fn handle(&self) -> HWND {
        self.hwnd
    }

    /// HWND を外部(WndProc の生成処理など)から設定する。
    pub fn set_handle(&mut self, hwnd: HWND) {
        self.hwnd = hwnd;
    }

    /// 未生成時の位置状態への参照。
    pub fn position_state(&self) -> &WindowPosition {
        &self.window_position
    }

    /// 原実装 `IsCreated` (BasicWindow.h:64)。
    pub fn is_created(&self) -> bool {
        !self.hwnd.0.is_null()
    }

    /// 原実装 `Destroy` (BasicWindow.cpp:38)。
    pub fn destroy(&mut self) {
        if !self.hwnd.0.is_null() {
            let _ = unsafe { DestroyWindow(self.hwnd) };
            // OnDestroy で m_hwnd は nullptr になる(WndProc 経由)。ここでは保険でクリア。
            self.hwnd = HWND::default();
        }
    }

    fn get_window_style_raw(&self) -> u32 {
        if self.hwnd.0.is_null() {
            0
        } else {
            unsafe { GetWindowLongW(self.hwnd, GWL_STYLE) as u32 }
        }
    }

    fn get_window_ex_style_raw(&self) -> u32 {
        if self.hwnd.0.is_null() {
            0
        } else {
            unsafe { GetWindowLongW(self.hwnd, GWL_EXSTYLE) as u32 }
        }
    }

    /// 原実装 `SetPosition(int, int, int, int)` (BasicWindow.cpp:47)。
    pub fn set_position(&mut self, left: i32, top: i32, width: i32, height: i32) -> bool {
        if width < 0 || height < 0 {
            return false;
        }
        if !self.hwnd.0.is_null() {
            let is_child = (self.get_window_style_raw() & WS_CHILD.0) != 0;
            let zoomed = unsafe { IsZoomed(self.hwnd) }.as_bool();
            let iconic = unsafe { IsIconic(self.hwnd) }.as_bool();
            if is_child || (!zoomed && !iconic) {
                let _ = unsafe {
                    MoveWindow(self.hwnd, left, top, width, height, true)
                };
            } else {
                let mut wp = WINDOWPLACEMENT {
                    length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
                    ..Default::default()
                };
                unsafe {
                    let _ = GetWindowPlacement(self.hwnd, &mut wp);
                }
                wp.rcNormalPosition = RECT {
                    left,
                    top,
                    right: left + width,
                    bottom: top + height,
                };
                if (self.get_window_ex_style_raw() & WS_EX_TOOLWINDOW.0) == 0 {
                    if let Some((monitor, work)) = monitor_and_work_from_rect(wp.rcNormalPosition) {
                        let adjusted = normalize_placement_to_monitor(
                            Rect::from_win32(&wp.rcNormalPosition),
                            monitor,
                            work,
                        );
                        wp.rcNormalPosition = adjusted.to_win32();
                    }
                }
                let _ = unsafe { SetWindowPlacement(self.hwnd, &wp) };
            }
        } else {
            self.window_position.set(left, top, width, height);
        }
        true
    }

    /// 原実装 `SetPosition(const RECT*)` (BasicWindow.cpp:87)。
    pub fn set_position_rect(&mut self, rc: Rect) -> bool {
        self.set_position(rc.left, rc.top, rc.right - rc.left, rc.bottom - rc.top)
    }

    /// 原実装 `GetPosition(int*, int*, int*, int*)` (BasicWindow.cpp:96)。
    pub fn get_position(&self) -> (i32, i32, i32, i32) {
        if self.hwnd.0.is_null() {
            return self.window_position.get();
        }

        let rc = if (self.get_window_style_raw() & WS_CHILD.0) != 0 {
            let mut rc = RECT::default();
            unsafe {
                let _ = GetWindowRect(self.hwnd, &mut rc);
                let parent = GetParent(self.hwnd).unwrap_or_default();
                let mut pts = [
                    POINT { x: rc.left, y: rc.top },
                    POINT { x: rc.right, y: rc.bottom },
                ];
                MapWindowPoints(None, Some(parent), &mut pts);
                rc = RECT {
                    left: pts[0].x,
                    top: pts[0].y,
                    right: pts[1].x,
                    bottom: pts[1].y,
                };
            }
            rc
        } else {
            let mut wp = WINDOWPLACEMENT {
                length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
                ..Default::default()
            };
            unsafe {
                let _ = GetWindowPlacement(self.hwnd, &mut wp);
            }
            if wp.showCmd == SW_SHOWNORMAL.0 as u32 {
                let mut rc = RECT::default();
                unsafe {
                    let _ = GetWindowRect(self.hwnd, &mut rc);
                }
                rc
            } else {
                let mut rect = wp.rcNormalPosition;
                if (self.get_window_ex_style_raw() & WS_EX_TOOLWINDOW.0) == 0 {
                    if let Some((monitor, work)) = monitor_and_work_from_rect(rect) {
                        rect = normalize_placement_to_work(Rect::from_win32(&rect), monitor, work)
                            .to_win32();
                    }
                }
                rect
            }
        };

        (rc.left, rc.top, rc.right - rc.left, rc.bottom - rc.top)
    }

    /// 原実装 `GetPosition(RECT*)` (BasicWindow.cpp:158)。
    pub fn get_position_rect(&self) -> Rect {
        let (left, top, width, height) = self.get_position();
        Rect::new(left, top, left + width, top + height)
    }

    /// 原実装 `GetWidth` (BasicWindow.cpp:167)。
    pub fn width(&self) -> i32 {
        self.get_position().2
    }

    /// 原実装 `GetHeight` (BasicWindow.cpp:176)。
    pub fn height(&self) -> i32 {
        self.get_position().3
    }

    /// 原実装 `GetScreenPosition` (BasicWindow.cpp:185)。
    pub fn get_screen_position(&self) -> Option<Rect> {
        if self.hwnd.0.is_null() {
            return Some(self.get_position_rect());
        }
        let mut rc = RECT::default();
        if unsafe { GetWindowRect(self.hwnd, &mut rc) }.is_ok() {
            Some(Rect::from_win32(&rc))
        } else {
            None
        }
    }

    /// 原実装 `SetVisible` (BasicWindow.cpp:195)。
    pub fn set_visible(&self, visible: bool) {
        if !self.hwnd.0.is_null() {
            unsafe {
                let _ = ShowWindow(self.hwnd, if visible { SW_SHOW } else { SW_HIDE });
            }
        }
    }

    /// 原実装 `GetVisible` (BasicWindow.cpp:202)。
    pub fn get_visible(&self) -> bool {
        !self.hwnd.0.is_null() && unsafe { IsWindowVisible(self.hwnd) }.as_bool()
    }

    /// 原実装 `SetMaximize` (BasicWindow.cpp:208)。
    pub fn set_maximize(&mut self, maximize: bool) -> bool {
        if !self.hwnd.0.is_null() {
            unsafe {
                let _ = ShowWindow(self.hwnd, if maximize { SW_MAXIMIZE } else { SW_RESTORE });
            }
        } else {
            self.window_position.maximized = maximize;
        }
        true
    }

    /// 原実装 `GetMaximize` (BasicWindow.cpp:219)。
    pub fn get_maximize(&self) -> bool {
        if !self.hwnd.0.is_null() {
            unsafe { IsZoomed(self.hwnd) }.as_bool()
        } else {
            self.window_position.maximized
        }
    }

    /// 原実装 `Invalidate(bool)` (BasicWindow.cpp:227)。
    pub fn invalidate(&self, erase: bool) -> bool {
        !self.hwnd.0.is_null()
            && unsafe { InvalidateRect(Some(self.hwnd), None, erase) }.as_bool()
    }

    /// 原実装 `Invalidate(const RECT*, bool)` (BasicWindow.cpp:233)。
    pub fn invalidate_rect(&self, rc: Rect, erase: bool) -> bool {
        if self.hwnd.0.is_null() {
            return false;
        }
        let r = rc.to_win32();
        unsafe { InvalidateRect(Some(self.hwnd), Some(&r), erase) }.as_bool()
    }

    /// 原実装 `Update` (BasicWindow.cpp:239)。
    pub fn update(&self) -> bool {
        !self.hwnd.0.is_null() && unsafe { UpdateWindow(self.hwnd) }.as_bool()
    }

    /// 原実装 `Redraw` (BasicWindow.cpp:245)。`flags` の既定は原実装と同じ。
    pub fn redraw(&self, rc: Option<Rect>, flags: REDRAW_WINDOW_FLAGS) -> bool {
        if self.hwnd.0.is_null() {
            return false;
        }
        let r = rc.map(|r| r.to_win32());
        let prc = r.as_ref().map(|r| r as *const RECT);
        unsafe { RedrawWindow(Some(self.hwnd), prc, None, flags) }.as_bool()
    }

    /// 原実装の `Redraw` 既定フラグ `RDW_ERASE | RDW_INVALIDATE | RDW_UPDATENOW`。
    pub fn redraw_default(&self) -> bool {
        self.redraw(None, RDW_ERASE | RDW_INVALIDATE | RDW_UPDATENOW)
    }

    /// 原実装 `GetClientRect` (BasicWindow.cpp:251)。
    pub fn get_client_rect(&self) -> Option<Rect> {
        if self.hwnd.0.is_null() {
            return None;
        }
        let mut rc = RECT::default();
        if unsafe { GetClientRect(self.hwnd, &mut rc) }.is_ok() {
            Some(Rect::from_win32(&rc))
        } else {
            None
        }
    }

    /// 原実装 `GetClientSize` (BasicWindow.cpp:257)。
    pub fn get_client_size(&self) -> Option<SIZE> {
        let rc = self.get_client_rect()?;
        Some(SIZE { cx: rc.right, cy: rc.bottom })
    }

    /// 原実装 `SetParent(HWND)` (BasicWindow.cpp:269)。
    pub fn set_parent(&self, parent: Option<HWND>) -> bool {
        !self.hwnd.0.is_null() && unsafe { SetParent(self.hwnd, parent) }.is_ok()
    }

    /// 原実装 `GetParent` (BasicWindow.cpp:281)。
    pub fn get_parent(&self) -> Option<HWND> {
        if self.hwnd.0.is_null() {
            return None;
        }
        unsafe { GetParent(self.hwnd) }.ok()
    }

    /// 原実装 `MoveToMonitorInside` (BasicWindow.cpp:289)。モニタ外なら内側へ移動して `true`。
    pub fn move_to_monitor_inside(&mut self) -> bool {
        let rc = self.get_position_rect();
        let win_rc = rc.to_win32();
        let monitor = unsafe {
            let hmon = MonitorFromRect(&win_rc, MONITOR_DEFAULTTONEAREST);
            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if !GetMonitorInfoW(hmon, &mut mi).as_bool() {
                return false;
            }
            Rect::from_win32(&mi.rcMonitor)
        };
        if let Some((dx, dy)) = move_to_monitor_inside_offset(rc, monitor) {
            self.set_position_rect(rc.offset(dx, dy));
            true
        } else {
            false
        }
    }

    /// 原実装 `GetWindowStyle` (BasicWindow.cpp:318)。
    pub fn get_window_style(&self) -> u32 {
        self.get_window_style_raw()
    }

    /// 原実装 `SetWindowStyle` (BasicWindow.cpp:326)。
    pub fn set_window_style(&self, style: u32, frame_change: bool) -> bool {
        if self.hwnd.0.is_null() {
            return false;
        }
        unsafe {
            SetWindowLongW(self.hwnd, GWL_STYLE, style as i32);
            if frame_change {
                let _ = SetWindowPos(
                    self.hwnd,
                    None,
                    0,
                    0,
                    0,
                    0,
                    SWP_FRAMECHANGED
                        | SWP_DRAWFRAME
                        | SWP_NOZORDER
                        | SWP_NOMOVE
                        | SWP_NOSIZE
                        | SWP_NOACTIVATE,
                );
            }
        }
        true
    }

    /// 原実装 `GetWindowExStyle` (BasicWindow.cpp:339)。
    pub fn get_window_ex_style(&self) -> u32 {
        self.get_window_ex_style_raw()
    }

    /// 原実装 `SetWindowExStyle` (BasicWindow.cpp:347)。
    pub fn set_window_ex_style(&self, ex_style: u32, frame_change: bool) -> bool {
        if self.hwnd.0.is_null() {
            return false;
        }
        unsafe {
            SetWindowLongW(self.hwnd, GWL_EXSTYLE, ex_style as i32);
            if frame_change {
                let _ = SetWindowPos(
                    self.hwnd,
                    None,
                    0,
                    0,
                    0,
                    0,
                    SWP_FRAMECHANGED
                        | SWP_DRAWFRAME
                        | SWP_NOZORDER
                        | SWP_NOMOVE
                        | SWP_NOSIZE
                        | SWP_NOACTIVATE,
                );
            }
        }
        true
    }

    /// 原実装 `SendMessage` (BasicWindow.cpp:403)。
    pub fn send_message(&self, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if self.hwnd.0.is_null() {
            return LRESULT(0);
        }
        unsafe { SendMessageW(self.hwnd, msg, Some(wparam), Some(lparam)) }
    }

    /// 原実装 `PostMessage` (BasicWindow.cpp:411)。
    pub fn post_message(&self, msg: u32, wparam: WPARAM, lparam: LPARAM) -> bool {
        !self.hwnd.0.is_null()
            && unsafe { PostMessageW(Some(self.hwnd), msg, wparam, lparam) }.is_ok()
    }

    /// 原実装 `SendSizeMessage` (BasicWindow.cpp:419)。クライアント領域サイズで `WM_SIZE` を送る。
    pub fn send_size_message(&self) -> bool {
        if self.hwnd.0.is_null() {
            return false;
        }
        let mut rc = RECT::default();
        if unsafe { GetClientRect(self.hwnd, &mut rc) }.is_err() {
            return false;
        }
        let lparam = LPARAM(((rc.right & 0xFFFF) | (rc.bottom << 16)) as isize);
        unsafe {
            SendMessageW(self.hwnd, WM_SIZE, Some(WPARAM(0)), Some(lparam));
        }
        true
    }

    /// 原実装 `SetOpacity` (BasicWindow.cpp:432)。レイヤードウィンドウで不透明度を設定する。
    pub fn set_opacity(&self, opacity: i32, clear_layered: bool) -> bool {
        if !opacity_is_valid(opacity) || self.hwnd.0.is_null() {
            return false;
        }

        // 子ウィンドウのレイヤード化は Windows 8 以降のみ可。
        if (self.get_window_style_raw() & WS_CHILD.0) != 0
            && !tvtest_winutil::is_windows_8_or_later()
        {
            return false;
        }

        let ex_style = self.get_window_ex_style_raw();

        if opacity < 255 {
            if (ex_style & WS_EX_LAYERED.0) == 0 {
                self.set_window_ex_style(ex_style | WS_EX_LAYERED.0, false);
            }
            if unsafe {
                SetLayeredWindowAttributes(self.hwnd, windows::Win32::Foundation::COLORREF(0), opacity as u8, LWA_ALPHA)
            }
            .is_err()
            {
                return false;
            }
        } else if (ex_style & WS_EX_LAYERED.0) != 0 {
            if clear_layered {
                self.set_window_ex_style(ex_style ^ WS_EX_LAYERED.0, false);
                self.redraw(None, RDW_ERASE | RDW_INVALIDATE | RDW_FRAME | RDW_ALLCHILDREN);
            } else {
                unsafe {
                    let _ = SetLayeredWindowAttributes(
                        self.hwnd,
                        windows::Win32::Foundation::COLORREF(0),
                        255,
                        LWA_ALPHA,
                    );
                }
            }
        }

        true
    }
}

/// 矩形が属するモニタの「モニタ矩形」「作業領域矩形」を取得する。失敗時 `None`。
fn monitor_and_work_from_rect(rc: RECT) -> Option<(Rect, Rect)> {
    unsafe {
        let hmon: HMONITOR = MonitorFromRect(&rc, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(hmon, &mut mi).as_bool() {
            Some((Rect::from_win32(&mi.rcMonitor), Rect::from_win32(&mi.rcWork)))
        } else {
            None
        }
    }
}

// ===========================================================================
// CCustomWindow: WndProc によるメッセージ振り分け
// ===========================================================================

/// `CCustomWindow` のメッセージ処理仮想関数に対応するハンドラ。
/// 原実装 `CCustomWindow` (BasicWindow.h:98, BasicWindow.cpp:470-512)。
///
/// 生成/破棄系メッセージ(`WM_NCCREATE` / `WM_CREATE` / `WM_DESTROY`)では `handle_message` が、
/// それ以外では `on_message` が呼ばれる。既定の `handle_message` は `on_message` に委譲し、
/// 既定の `on_message` は `DefWindowProc` を返す(原実装 BasicWindow.cpp:503-512 と同じ)。
pub trait CustomWindowHandler {
    /// 原実装 `OnCreate` 相当 (BasicWindow.cpp:375)。`WM_NCCREATE` 受信時に HWND を結び付ける
    /// (`pWindow->m_hwnd = hwnd`)。生成失敗時は null HWND で呼ばれる(`m_hwnd = nullptr`)。
    fn set_handle(&mut self, hwnd: HWND);

    /// 原実装 `CBasicWindow::OnDestroy` (BasicWindow.cpp:386)。`WM_DESTROY` 受信時に
    /// 位置を保存して HWND を切り離す。
    fn on_destroy(&mut self);

    /// 原実装 `HandleMessage` (BasicWindow.cpp:503)。既定は `OnMessage` に委譲。
    fn handle_message(&mut self, hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        self.on_message(hwnd, msg, wparam, lparam)
    }

    /// 原実装 `OnMessage` (BasicWindow.cpp:509)。既定は `DefWindowProc`。
    fn on_message(&mut self, hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }
}

/// `WndProc` 内でメッセージを振り分ける種別。原実装 `CCustomWindow::WndProc`
/// (BasicWindow.cpp:470) の `if` 連鎖に対応。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CustomWindowRoute {
    /// `WM_NCCREATE`: ここで `this` を結び付け、`handle_message` の結果で生成可否を返す。
    NcCreate,
    /// `WM_CREATE`: `handle_message` が負なら生成失敗(-1)。
    Create,
    /// `WM_DESTROY`: `handle_message` 後に `OnDestroy`。
    Destroy,
    /// それ以外: `on_message`。
    Other,
}

/// メッセージを [`CustomWindowRoute`] に分類する。原実装 `WndProc` の分岐 (BasicWindow.cpp:474-497)。
pub fn classify_message(msg: u32) -> CustomWindowRoute {
    match msg {
        WM_NCCREATE => CustomWindowRoute::NcCreate,
        WM_CREATE => CustomWindowRoute::Create,
        WM_DESTROY => CustomWindowRoute::Destroy,
        _ => CustomWindowRoute::Other,
    }
}

/// `WM_NCCREATE` の `handle_message` 結果から `(戻り値, HWND を切り離すか)` を決める。
/// 原実装 (BasicWindow.cpp:475-480): 結果が 0(偽)なら `m_hwnd=nullptr` して `FALSE`、
/// それ以外は `TRUE`。
pub fn nccreate_outcome(handle_result: LRESULT) -> (LRESULT, bool) {
    if handle_result.0 == 0 {
        (LRESULT(0), true) // FALSE、HWND 切り離し
    } else {
        (LRESULT(1), false) // TRUE
    }
}

/// `WM_CREATE` の `handle_message` 結果から `(戻り値, HWND を切り離すか)` を決める。
/// 原実装 (BasicWindow.cpp:485-491): 負なら `m_hwnd=nullptr` して `-1`、それ以外は `0`。
pub fn create_outcome(handle_result: LRESULT) -> (LRESULT, bool) {
    if handle_result.0 < 0 {
        (LRESULT(-1), true)
    } else {
        (LRESULT(0), false)
    }
}

/// 原実装 `CCustomWindow::WndProc` (BasicWindow.cpp:470) のトランポリン。
///
/// 派生ウィンドウのウィンドウクラスの `lpfnWndProc` にこの関数を登録し、`CreateWindowEx` の
/// 最終引数(`lpParam`)に `*mut H`(ハンドラ)を渡す。`WM_NCCREATE` で `lpCreateParams` から
/// ハンドラを取り出して `GWLP_USERDATA` に保存し(原実装 `OnCreate`, BasicWindow.cpp:375)、
/// 以降は `GWLP_USERDATA` から復元する(原実装 `GetBasicWindow`, BasicWindow.cpp:397)。
///
/// # Safety
/// `H` ハンドラはウィンドウより長く生存している必要がある。`lpParam` には有効な `*mut H` を
/// 渡すこと。複数の関心事(生成パラメータ・ユーザーデータ)を生ポインタ経由で扱う。
pub unsafe extern "system" fn custom_wnd_proc<H: CustomWindowHandler>(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if classify_message(msg) == CustomWindowRoute::NcCreate {
        // OnCreate 相当: lpCreateParams から this を取り出して USERDATA に保存する。
        let cs = lparam.0 as *const CREATESTRUCTW;
        if cs.is_null() {
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }
        let this = (*cs).lpCreateParams as *mut H;
        if this.is_null() {
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, this as isize);
        (*this).set_handle(hwnd);

        let r = (*this).handle_message(hwnd, msg, wparam, lparam);
        let (ret, clear) = nccreate_outcome(r);
        if clear {
            // 原実装は m_hwnd のみ null 化(USERDATA はそのまま)。
            (*this).set_handle(HWND::default());
        }
        return ret;
    }

    let this = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut H;
    if this.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }

    match classify_message(msg) {
        CustomWindowRoute::Create => {
            let r = (*this).handle_message(hwnd, msg, wparam, lparam);
            let (ret, clear) = create_outcome(r);
            if clear {
                (*this).set_handle(HWND::default());
            }
            ret
        }
        CustomWindowRoute::Destroy => {
            (*this).handle_message(hwnd, msg, wparam, lparam);
            // OnDestroy: 位置保存 + m_hwnd/USERDATA クリア。
            (*this).on_destroy();
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            LRESULT(0)
        }
        // NcCreate は上で処理済み。
        _ => (*this).on_message(hwnd, msg, wparam, lparam),
    }
}

// ===========================================================================
// CPopupWindow: ダークモード状態遷移
// ===========================================================================

/// `CPopupWindow` のダークモード状態。原実装 `CPopupWindow` (BasicWindow.h:111,
/// BasicWindow.cpp:517)。`m_fAllowDarkMode` / `m_fDarkMode` に対応する。
///
/// 状態遷移 ([`on_create`](Self::on_create) / [`on_setting_change`](Self::on_setting_change)) は
/// 各 DarkMode API の結果を引数注入する純粋関数として表し単体テストで検証する。実 API
/// ([`tvtest_dark_mode`]) への接続は [`handle_create`](Self::handle_create) /
/// [`handle_setting_change`](Self::handle_setting_change) が担う。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PopupDarkModeState {
    pub allow_dark_mode: bool,
    pub dark_mode: bool,
}

impl PopupDarkModeState {
    pub fn new() -> Self {
        Self::default()
    }

    /// `WM_CREATE` 処理。原実装 (BasicWindow.cpp:520-528)。
    ///
    /// - `allow_ok`: `SetWindowAllowDarkMode(m_hwnd, true)` の結果。
    /// - `is_dark`: `TVTest::IsDarkMode()`。
    /// - `frame_set_ok`: `SetWindowFrameDarkMode(m_hwnd, true)` の結果(`is_dark` のときのみ呼ばれる)。
    pub fn on_create(&mut self, allow_ok: bool, is_dark: bool, frame_set_ok: bool) {
        if allow_ok {
            self.allow_dark_mode = true;
            if is_dark && frame_set_ok {
                self.dark_mode = true;
            }
        }
    }

    /// `WM_SETTINGCHANGE` 処理。原実装 (BasicWindow.cpp:530-543)。
    /// 戻り値は `OnDarkModeChanged` を呼ぶべきか(ダークモード状態が実際に切り替わったか)。
    ///
    /// - `setting_changed`: `IsDarkModeSettingChanged(...)`。
    /// - `is_dark`: 変更後の `TVTest::IsDarkMode()`。
    /// - `frame_set_ok`: `SetWindowFrameDarkMode(hwnd, is_dark)` の結果
    ///   (状態が異なるときのみ呼ばれる)。
    pub fn on_setting_change(
        &mut self,
        setting_changed: bool,
        is_dark: bool,
        frame_set_ok: bool,
    ) -> bool {
        if self.allow_dark_mode && setting_changed && self.dark_mode != is_dark && frame_set_ok {
            self.dark_mode = is_dark;
            return true;
        }
        false
    }

    /// `WM_CREATE` を実 DarkMode API で処理する。原実装 `CPopupWindow::HandleMessage` の
    /// `WM_CREATE` 分岐 (BasicWindow.cpp:520-528)。
    ///
    /// `SetWindowAllowDarkMode(hwnd, true)` →(許可成功かつ `IsDarkMode()` のとき)
    /// `SetWindowFrameDarkMode(hwnd, true)` を原実装の短絡順序どおり呼び、結果を
    /// [`on_create`](Self::on_create) に渡す。
    pub fn handle_create(&mut self, hwnd: HWND) {
        let allow_ok = tvtest_dark_mode::set_window_allow_dark_mode(hwnd, true);
        let (is_dark, frame_set_ok) = if allow_ok {
            let is_dark = tvtest_dark_mode::is_dark_mode();
            let frame_set_ok = if is_dark {
                tvtest_dark_mode::set_window_frame_dark_mode(hwnd, true)
            } else {
                false
            };
            (is_dark, frame_set_ok)
        } else {
            (false, false)
        };
        self.on_create(allow_ok, is_dark, frame_set_ok);
    }

    /// `WM_SETTINGCHANGE` を実 DarkMode API で処理する。原実装 `CPopupWindow::HandleMessage` の
    /// `WM_SETTINGCHANGE` 分岐 (BasicWindow.cpp:530-543)。
    ///
    /// 戻り値が `true` のとき、呼び出し側は原実装の `OnDarkModeChanged(self.dark_mode)` 相当を
    /// 行う。原実装の短絡(`m_fAllowDarkMode` → `IsDarkModeSettingChanged` → `IsDarkMode` →
    /// `SetWindowFrameDarkMode`)を保つため、各 API は必要なときのみ呼ぶ。
    pub fn handle_setting_change(
        &mut self,
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> bool {
        let setting_changed = self.allow_dark_mode
            && tvtest_dark_mode::is_dark_mode_setting_changed(hwnd, msg, wparam, lparam);
        let is_dark = if setting_changed {
            tvtest_dark_mode::is_dark_mode()
        } else {
            false
        };
        let frame_set_ok = if setting_changed && self.dark_mode != is_dark {
            tvtest_dark_mode::set_window_frame_dark_mode(hwnd, is_dark)
        } else {
            false
        };
        self.on_setting_change(setting_changed, is_dark, frame_set_ok)
    }
}

// ===========================================================================
// テスト(純粋ロジック)
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::WM_SETTINGCHANGE;

    // --- WindowPosition ---

    #[test]
    fn test_window_position_set_get() {
        let mut p = WindowPosition::new();
        assert!(p.set(10, 20, 300, 200));
        assert_eq!(p.get(), (10, 20, 300, 200));
    }

    #[test]
    fn test_window_position_rejects_negative_size() {
        let mut p = WindowPosition::new();
        assert!(!p.set(0, 0, -1, 100));
        assert!(!p.set(0, 0, 100, -5));
        // 拒否されたので値は初期のまま。
        assert_eq!(p.get(), (0, 0, 0, 0));
    }

    #[test]
    fn test_window_position_rect_roundtrip() {
        let mut p = WindowPosition::new();
        assert!(p.set_from_rect(Rect::new(10, 20, 110, 220)));
        assert_eq!(p.get(), (10, 20, 100, 200));
        assert_eq!(p.to_rect(), Rect::new(10, 20, 110, 220));
    }

    #[test]
    fn test_window_position_zero_size_ok() {
        let mut p = WindowPosition::new();
        assert!(p.set(5, 5, 0, 0));
        assert_eq!(p.get(), (5, 5, 0, 0));
    }

    // --- Rect::offset ---

    #[test]
    fn test_rect_offset() {
        assert_eq!(
            Rect::new(0, 0, 100, 50).offset(10, -5),
            Rect::new(10, -5, 110, 45)
        );
    }

    // --- move_to_monitor_inside_offset ---

    #[test]
    fn test_move_inside_already_inside() {
        let monitor = Rect::new(0, 0, 1920, 1080);
        // 完全に内側 → None
        assert_eq!(move_to_monitor_inside_offset(Rect::new(100, 100, 500, 400), monitor), None);
        // 一部でも重なっていれば None(右にはみ出すが left < right)
        assert_eq!(move_to_monitor_inside_offset(Rect::new(1900, 100, 2200, 400), monitor), None);
    }

    #[test]
    fn test_move_inside_off_right() {
        let monitor = Rect::new(0, 0, 1920, 1080);
        // 完全に右外(left >= monitor.right=1920)
        let rc = Rect::new(2000, 100, 2300, 400);
        let (dx, dy) = move_to_monitor_inside_offset(rc, monitor).unwrap();
        // x_offset = monitor.right - rc.right = 1920 - 2300 = -380、y は範囲内なので 0
        assert_eq!((dx, dy), (-380, 0));
        // 移動後は右端が monitor.right に一致。
        let moved = rc.offset(dx, dy);
        assert_eq!(moved.right, 1920);
    }

    #[test]
    fn test_move_inside_off_left_top() {
        let monitor = Rect::new(0, 0, 1920, 1080);
        // 完全に左かつ上の外(right <= 0 かつ bottom <= 0)
        let rc = Rect::new(-500, -300, -100, -50);
        let (dx, dy) = move_to_monitor_inside_offset(rc, monitor).unwrap();
        // x_offset = monitor.left - rc.left = 0 - (-500) = 500
        // y_offset = monitor.top - rc.top = 0 - (-300) = 300
        assert_eq!((dx, dy), (500, 300));
        let moved = rc.offset(dx, dy);
        assert_eq!((moved.left, moved.top), (0, 0));
    }

    #[test]
    fn test_move_inside_off_bottom() {
        let monitor = Rect::new(0, 0, 1920, 1080);
        let rc = Rect::new(100, 1200, 400, 1400);
        let (dx, dy) = move_to_monitor_inside_offset(rc, monitor).unwrap();
        // y_offset = monitor.bottom - rc.bottom = 1080 - 1400 = -320、x は範囲内 0
        assert_eq!((dx, dy), (0, -320));
        assert_eq!(rc.offset(dx, dy).bottom, 1080);
    }

    // --- normalize_placement ---

    #[test]
    fn test_normalize_placement_roundtrip() {
        // モニタ (0,0)-(1920,1080)、作業領域はタスクバー分上に 40px(top=40)。
        let monitor = Rect::new(0, 0, 1920, 1080);
        let work = Rect::new(0, 40, 1920, 1080);
        let rect = Rect::new(100, 100, 500, 400);
        // 作業領域→モニタ: (monitor.top - work.top) = -40 だけ y を移動。
        let to_mon = normalize_placement_to_monitor(rect, monitor, work);
        assert_eq!(to_mon, rect.offset(0, -40));
        // 逆変換で戻る。
        let back = normalize_placement_to_work(to_mon, monitor, work);
        assert_eq!(back, rect);
    }

    // --- opacity_is_valid ---

    #[test]
    fn test_opacity_is_valid() {
        assert!(opacity_is_valid(0));
        assert!(opacity_is_valid(128));
        assert!(opacity_is_valid(255));
        assert!(!opacity_is_valid(-1));
        assert!(!opacity_is_valid(256));
    }

    // --- BasicWindow(未生成状態の純粋な振る舞い) ---

    #[test]
    fn test_basic_window_uncreated_position() {
        let mut w = BasicWindow::new();
        assert!(!w.is_created());
        assert!(w.set_position(10, 20, 300, 200));
        assert_eq!(w.get_position(), (10, 20, 300, 200));
        assert_eq!(w.get_position_rect(), Rect::new(10, 20, 310, 220));
        assert_eq!(w.width(), 300);
        assert_eq!(w.height(), 200);
    }

    #[test]
    fn test_basic_window_uncreated_maximize() {
        let mut w = BasicWindow::new();
        assert!(!w.get_maximize());
        assert!(w.set_maximize(true));
        assert!(w.get_maximize());
    }

    #[test]
    fn test_basic_window_uncreated_set_position_rejects_negative() {
        let mut w = BasicWindow::new();
        assert!(!w.set_position(0, 0, -1, 10));
    }

    #[test]
    fn test_basic_window_uncreated_visibility_and_handles() {
        let w = BasicWindow::new();
        // 未生成では非表示・親なし・クライアント矩形なし。
        assert!(!w.get_visible());
        assert!(w.get_parent().is_none());
        assert!(w.get_client_rect().is_none());
        assert_eq!(w.get_window_style(), 0);
        assert_eq!(w.get_window_ex_style(), 0);
        // 未生成 HWND へのメッセージは 0 / false。
        assert_eq!(w.send_message(0, WPARAM(0), LPARAM(0)).0, 0);
        assert!(!w.post_message(0, WPARAM(0), LPARAM(0)));
        assert!(!w.send_size_message());
        assert!(!w.set_opacity(128, true));
    }

    // --- CCustomWindow: メッセージ分類 ---

    #[test]
    fn test_classify_message() {
        assert_eq!(classify_message(WM_NCCREATE), CustomWindowRoute::NcCreate);
        assert_eq!(classify_message(WM_CREATE), CustomWindowRoute::Create);
        assert_eq!(classify_message(WM_DESTROY), CustomWindowRoute::Destroy);
        assert_eq!(classify_message(WM_SIZE), CustomWindowRoute::Other);
        assert_eq!(classify_message(0), CustomWindowRoute::Other);
    }

    // --- CCustomWindow: 生成可否の戻り値決定 ---

    #[test]
    fn test_nccreate_outcome() {
        // 偽(0)→ FALSE(0) かつ HWND 切り離し。
        let (r, clear) = nccreate_outcome(LRESULT(0));
        assert_eq!(r.0, 0);
        assert!(clear);
        // 真(非0)→ TRUE(1)、切り離さない。
        let (r, clear) = nccreate_outcome(LRESULT(1));
        assert_eq!(r.0, 1);
        assert!(!clear);
        // 負でも非0なら真扱い → TRUE。
        let (r, clear) = nccreate_outcome(LRESULT(-1));
        assert_eq!(r.0, 1);
        assert!(!clear);
    }

    #[test]
    fn test_create_outcome() {
        // 負 → -1 かつ切り離し。
        let (r, clear) = create_outcome(LRESULT(-1));
        assert_eq!(r.0, -1);
        assert!(clear);
        // 0 → 0、切り離さない。
        let (r, clear) = create_outcome(LRESULT(0));
        assert_eq!(r.0, 0);
        assert!(!clear);
        // 正 → 0、切り離さない。
        let (r, clear) = create_outcome(LRESULT(5));
        assert_eq!(r.0, 0);
        assert!(!clear);
    }

    // --- CCustomWindow: HandleMessage の既定委譲 ---

    struct DelegateProbe {
        on_message_msg: Option<u32>,
    }

    impl CustomWindowHandler for DelegateProbe {
        fn set_handle(&mut self, _hwnd: HWND) {}
        fn on_destroy(&mut self) {}
        // handle_message は未オーバーライド(既定で on_message に委譲する)。
        fn on_message(&mut self, _hwnd: HWND, msg: u32, _w: WPARAM, _l: LPARAM) -> LRESULT {
            self.on_message_msg = Some(msg);
            LRESULT(42)
        }
    }

    #[test]
    fn test_handle_message_delegates_to_on_message() {
        let mut h = DelegateProbe { on_message_msg: None };
        let r = h.handle_message(HWND::default(), 0x1234, WPARAM(0), LPARAM(0));
        // 既定の handle_message は on_message へ委譲する。
        assert_eq!(r.0, 42);
        assert_eq!(h.on_message_msg, Some(0x1234));
    }

    // --- CPopupWindow: ダークモード状態遷移 ---

    #[test]
    fn test_popup_dark_mode_default() {
        let s = PopupDarkModeState::new();
        assert!(!s.allow_dark_mode);
        assert!(!s.dark_mode);
    }

    #[test]
    fn test_popup_on_create_allow_fail() {
        // SetWindowAllowDarkMode 失敗 → 何も変わらない。
        let mut s = PopupDarkModeState::new();
        s.on_create(false, true, true);
        assert!(!s.allow_dark_mode);
        assert!(!s.dark_mode);
    }

    #[test]
    fn test_popup_on_create_light_mode() {
        // allow 成功・ライトモード → allow のみ true。
        let mut s = PopupDarkModeState::new();
        s.on_create(true, false, true);
        assert!(s.allow_dark_mode);
        assert!(!s.dark_mode);
    }

    #[test]
    fn test_popup_on_create_dark_mode() {
        // allow 成功・ダーク・フレーム設定成功 → 両方 true。
        let mut s = PopupDarkModeState::new();
        s.on_create(true, true, true);
        assert!(s.allow_dark_mode);
        assert!(s.dark_mode);
    }

    #[test]
    fn test_popup_on_create_dark_frame_fail() {
        // allow 成功・ダークだがフレーム設定失敗 → dark は false のまま。
        let mut s = PopupDarkModeState::new();
        s.on_create(true, true, false);
        assert!(s.allow_dark_mode);
        assert!(!s.dark_mode);
    }

    #[test]
    fn test_popup_setting_change_not_allowed() {
        // allow していなければ設定変更は無視。
        let mut s = PopupDarkModeState::new();
        assert!(!s.on_setting_change(true, true, true));
        assert!(!s.dark_mode);
    }

    #[test]
    fn test_popup_setting_change_to_dark() {
        // allow 済・設定変化・ダークへ・フレーム成功 → 切替成立。
        let mut s = PopupDarkModeState { allow_dark_mode: true, dark_mode: false };
        assert!(s.on_setting_change(true, true, true));
        assert!(s.dark_mode);
    }

    #[test]
    fn test_popup_setting_change_no_state_change() {
        // 既に同じ状態(dark==is_dark)なら何も起きない。
        let mut s = PopupDarkModeState { allow_dark_mode: true, dark_mode: true };
        assert!(!s.on_setting_change(true, true, true));
        assert!(s.dark_mode);
    }

    #[test]
    fn test_popup_setting_change_setting_not_changed() {
        // 設定そのものが変わっていない → false。
        let mut s = PopupDarkModeState { allow_dark_mode: true, dark_mode: false };
        assert!(!s.on_setting_change(false, true, true));
        assert!(!s.dark_mode);
    }

    #[test]
    fn test_popup_setting_change_frame_fail() {
        // フレーム設定失敗 → 状態は変えない。
        let mut s = PopupDarkModeState { allow_dark_mode: true, dark_mode: false };
        assert!(!s.on_setting_change(true, true, false));
        assert!(!s.dark_mode);
    }

    // --- CPopupWindow: 実 DarkMode API での駆動(無効ハンドル・非該当メッセージ) ---

    #[test]
    fn test_popup_handle_create_invalid_hwnd() {
        // 無効な HWND では SetWindowAllowDarkMode が失敗するので状態は変わらない。
        let mut s = PopupDarkModeState::new();
        s.handle_create(HWND::default());
        assert!(!s.allow_dark_mode);
        assert!(!s.dark_mode);
    }

    #[test]
    fn test_popup_handle_setting_change_not_allowed() {
        // allow していなければ実 API を呼ばず false(短絡)。
        let mut s = PopupDarkModeState::new();
        assert!(!s.handle_setting_change(HWND::default(), WM_SETTINGCHANGE, WPARAM(0), LPARAM(0)));
        assert!(!s.dark_mode);
    }

    #[test]
    fn test_popup_handle_setting_change_non_setting_message() {
        // allow 済でも WM_SETTINGCHANGE 以外なら IsDarkModeSettingChanged が false → 変化なし。
        let mut s = PopupDarkModeState { allow_dark_mode: true, dark_mode: false };
        assert!(!s.handle_setting_change(HWND::default(), WM_SIZE, WPARAM(0), LPARAM(0)));
        assert!(!s.dark_mode);
    }
}
