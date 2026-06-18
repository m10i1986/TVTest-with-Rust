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
//! 派生の `CCustomWindow`(WndProc によるメッセージ振り分け)/`CPopupWindow`
//! (ダークモード対応)は、同じメッセージ処理を共有する `CView` の移植および
//! `DarkMode.cpp` の移植と合わせて別途対応する。

#![cfg(windows)]

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, InvalidateRect, MapWindowPoints, MonitorFromRect, RedrawWindow, UpdateWindow,
    HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST, RDW_ALLCHILDREN, RDW_ERASE, RDW_FRAME,
    RDW_INVALIDATE, RDW_UPDATENOW, REDRAW_WINDOW_FLAGS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, GetClientRect, GetParent, GetWindowLongW, GetWindowPlacement, GetWindowRect,
    IsIconic, IsWindowVisible, IsZoomed, MoveWindow, PostMessageW, SendMessageW,
    SetLayeredWindowAttributes, SetParent, SetWindowLongW, SetWindowPlacement, SetWindowPos,
    ShowWindow, GWL_EXSTYLE, GWL_STYLE, LWA_ALPHA, SWP_DRAWFRAME, SWP_FRAMECHANGED, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_HIDE, SW_MAXIMIZE, SW_RESTORE, SW_SHOW, SW_SHOWNORMAL,
    WINDOWPLACEMENT, WS_CHILD, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WM_SIZE,
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
// テスト(純粋ロジック)
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

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
}
