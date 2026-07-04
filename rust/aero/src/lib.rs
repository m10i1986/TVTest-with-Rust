//! TVTest の Aero Glass / バッファードペイント(`src/Aero.cpp` / `src/Aero.h`)を
//! windows-rs で移植したクレート。
//!
//! 移植対象:
//! - `CAeroGlass`(`Aero.h:28-34`)。DWM コンポジションの有効判定
//!   (`IsEnabled`、`Aero.cpp:36-41`)、クライアント領域へのグラス適用
//!   (`ApplyAeroGlass`、`Aero.cpp:45-58`)、非クライアント領域レンダリングの
//!   有効/無効化(`EnableNcRendering`、`Aero.cpp:62-67`)。
//! - `CBufferedPaint`(`Aero.h:36-56`、`Aero.cpp:100-162`)。uxtheme の
//!   バッファードペイント(`BeginBufferedPaint`/`EndBufferedPaint` 等)の
//!   RAII ラッパー。
//! - `CDoubleBufferingDraw`(`Aero.h:58-63`、`Aero.cpp:167-184`)。
//!   `WM_PAINT` 処理でバッファードペイントを使ったダブルバッファリング描画を
//!   行う抽象クラス。Rust では trait(`draw` が必須メソッド、`on_paint` が
//!   デフォルト実装)として移植する。
//!
//! ## 原実装との差異
//!
//! - 原実装は `dwmapi.dll` を `#pragma comment(lib,"dwmapi.lib")`(`Aero.cpp:28`)で
//!   静的リンクしており、DLL の動的ロード(`LoadLibrary`/`GetProcAddress`)は
//!   行っていない。`dwmapi.dll` / `uxtheme.dll` は Vista 以降常在のため、本クレートも
//!   windows-rs の通常のインポート解決(`windows_core::link!`)をそのまま使う。
//! - `CBufferedPaintInitializer`(`Aero.cpp:72-97`)はプロセス全体で 1 度だけ
//!   `BufferedPaintInit` を呼ぶグローバルオブジェクトで、静的デストラクタで
//!   `BufferedPaintUnInit`(`Aero.cpp:75-79`)を呼ぶ。Rust には静的デストラクタが
//!   ないため `BufferedPaintUnInit` は呼ばない(プロセス終了時に OS が解放するため
//!   実害はない)。初期化状態は `Mutex<bool>` のグローバル static で管理し、
//!   原実装と同じく「失敗したら次回の `initialize` で再試行できる」挙動を保つ。

#![cfg(windows)]

use std::sync::Mutex;

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Dwm::{
    DwmExtendFrameIntoClientArea, DwmIsCompositionEnabled, DwmSetWindowAttribute,
    DWMNCRENDERINGPOLICY, DWMNCRP_DISABLED, DWMNCRP_USEWINDOWSTYLE, DWMWA_NCRENDERING_POLICY,
};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, HDC, PAINTSTRUCT};
use windows::Win32::UI::Controls::{
    BeginBufferedPaint, BufferedPaintClear, BufferedPaintInit, BufferedPaintSetAlpha,
    EndBufferedPaint, BPBF_TOPDOWNDIB, BPPF_ERASE, BP_PAINTPARAMS, BP_PAINTPARAMS_FLAGS, MARGINS,
};
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;

/// `CAeroGlass`(`Aero.h:28-34`)。DWM(Aero Glass)関連のユーティリティ。
///
/// 原実装は状態を持たないクラスのため、Rust ではユニット構造体の関連関数として
/// 提供する。
pub struct CAeroGlass;

impl CAeroGlass {
    /// `IsEnabled`(`Aero.cpp:36-41`)。コンポジションが有効か取得する。
    ///
    /// `DwmIsCompositionEnabled` が成功しかつ `TRUE` を返した場合のみ `true`。
    /// Windows 8 以降ではコンポジションは常に有効。
    #[must_use]
    pub fn is_enabled() -> bool {
        unsafe { DwmIsCompositionEnabled() }.is_ok_and(|enabled| enabled.as_bool())
    }

    /// `ApplyAeroGlass`(`Aero.cpp:45-58`)。クライアント領域を透けさせる。
    ///
    /// `rect` の各フィールドは `MARGINS`(left→`cxLeftWidth`、right→`cxRightWidth`、
    /// top→`cyTopHeight`、bottom→`cyBottomHeight`)として解釈される
    /// (`Aero.cpp:50-55`)。コンポジションが無効なら何もせず `false`
    /// (`Aero.cpp:47-48`)。
    ///
    /// # Safety
    /// `hwnd` は有効なトップレベルウィンドウのハンドルであること
    /// (無効なハンドルでも API がエラーを返すだけだが、原実装同様に生ハンドルを
    /// 受け取るため unsafe とする)。
    pub unsafe fn apply_aero_glass(hwnd: HWND, rect: &RECT) -> bool {
        if !Self::is_enabled() {
            return false;
        }

        let margins = MARGINS {
            cxLeftWidth: rect.left,
            cxRightWidth: rect.right,
            cyTopHeight: rect.top,
            cyBottomHeight: rect.bottom,
        };

        unsafe { DwmExtendFrameIntoClientArea(hwnd, &margins) }.is_ok()
    }

    /// `EnableNcRendering`(`Aero.cpp:62-67`)。フレーム(非クライアント領域)の
    /// DWM レンダリングを有効/無効にする。
    ///
    /// `enable` が `true` なら `DWMNCRP_USEWINDOWSTYLE`、`false` なら
    /// `DWMNCRP_DISABLED` を `DWMWA_NCRENDERING_POLICY` に設定する。
    ///
    /// # Safety
    /// `hwnd` は有効なウィンドウハンドルであること。
    pub unsafe fn enable_nc_rendering(hwnd: HWND, enable: bool) -> bool {
        let ncrp: DWMNCRENDERINGPOLICY = if enable {
            DWMNCRP_USEWINDOWSTYLE
        } else {
            DWMNCRP_DISABLED
        };

        unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_NCRENDERING_POLICY,
                std::ptr::from_ref(&ncrp).cast(),
                std::mem::size_of::<DWMNCRENDERINGPOLICY>() as u32,
            )
        }
        .is_ok()
    }
}

/// `CBufferedPaintInitializer`(`Aero.cpp:72-97`)の初期化状態。
///
/// 原実装はグローバルオブジェクト `BufferedPaintInitializer`(`Aero.cpp:97`)の
/// メンバ `m_fInitialized` で管理する。Rust ではグローバル static の `Mutex<bool>` で
/// 管理する(`Mutex` なのは「未初期化なら `BufferedPaintInit` を呼んで成功時のみ
/// フラグを立てる」列を原実装どおりアトミックに行うため。失敗時はフラグが立たず、
/// 次回呼び出しで再試行される)。
static BUFFERED_PAINT_INITIALIZED: Mutex<bool> = Mutex::new(false);

/// `CBufferedPaint`(`Aero.h:36-56`、`Aero.cpp:100-162`)。
/// uxtheme バッファードペイント(`HPAINTBUFFER`)の RAII ラッパー。
///
/// 原実装はコピー禁止(`Aero.h:42-43`)のため `Clone` を実装しない。
/// デストラクタは `End(false)` を呼ぶ(`Aero.cpp:100-103`)。
///
/// windows-rs 0.62 では `HPAINTBUFFER` は `isize` として表現される
/// (0 が null 相当)。
#[derive(Default)]
pub struct CBufferedPaint {
    /// `m_hPaintBuffer`(`Aero.h:55`)。0 は未使用(null)。
    paint_buffer: isize,
}

impl CBufferedPaint {
    /// 既定コンストラクタ(`Aero.h:39`、`CBufferedPaint() = default`)。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `Begin`(`Aero.cpp:106-124`)。バッファードペイントを開始し、
    /// 描画先となるバッファの `HDC` を返す。
    ///
    /// [`CBufferedPaint::initialize`] が成功していなければ `None`
    /// (`Aero.cpp:108-109`)。既にバッファを保持していれば `End(false)` 相当で
    /// 破棄してから開始する(`Aero.cpp:111-114`)。`erase` が `true` なら
    /// `BPPF_ERASE` を指定する(`Aero.cpp:117-118`)。バッファフォーマットは
    /// `BPBF_TOPDOWNDIB` 固定(`Aero.cpp:120`)。
    ///
    /// # Safety
    /// `hdc` は有効なデバイスコンテキストのハンドルであること。返された `HDC` は
    /// [`CBufferedPaint::end`](または本構造体の Drop)まで有効。
    pub unsafe fn begin(&mut self, hdc: HDC, rect: &RECT, erase: bool) -> Option<HDC> {
        if !Self::is_supported() {
            return None;
        }

        if self.paint_buffer != 0 && !self.end(false) {
            return None;
        }

        let params = BP_PAINTPARAMS {
            cbSize: std::mem::size_of::<BP_PAINTPARAMS>() as u32,
            dwFlags: if erase {
                BPPF_ERASE
            } else {
                BP_PAINTPARAMS_FLAGS(0)
            },
            prcExclude: std::ptr::null(),
            pBlendFunction: std::ptr::null(),
        };
        let mut hdc_buffer = HDC::default();
        let paint_buffer = unsafe {
            BeginBufferedPaint(
                hdc,
                rect,
                BPBF_TOPDOWNDIB,
                Some(std::ptr::from_ref(&params)),
                &mut hdc_buffer,
            )
        };
        if paint_buffer == 0 {
            return None;
        }
        self.paint_buffer = paint_buffer;
        Some(hdc_buffer)
    }

    /// `End`(`Aero.cpp:127-134`)。バッファードペイントを終了する。
    ///
    /// `update` が `true` ならバッファの内容を対象 DC へ転送する。
    /// 原実装同様、`EndBufferedPaint` の結果に関わらず常に `true` を返す
    /// (バッファ未使用時も `true`)。
    pub fn end(&mut self, update: bool) -> bool {
        if self.paint_buffer != 0 {
            unsafe {
                let _ = EndBufferedPaint(self.paint_buffer, update);
            }
            self.paint_buffer = 0;
        }
        true
    }

    /// `Clear`(`Aero.cpp:137-142`)。バッファの内容を消去する。
    ///
    /// `rect` が `None` ならバッファ全体(原実装の既定引数 `nullptr`、
    /// `Aero.h:47`)。バッファ未使用なら `false`。
    pub fn clear(&mut self, rect: Option<&RECT>) -> bool {
        if self.paint_buffer == 0 {
            return false;
        }
        unsafe { BufferedPaintClear(self.paint_buffer, rect.map(std::ptr::from_ref)) }.is_ok()
    }

    /// `SetAlpha`(`Aero.cpp:145-150`)。バッファ全体のアルファ値を設定する。
    ///
    /// バッファ未使用なら `false`。
    pub fn set_alpha(&mut self, alpha: u8) -> bool {
        if self.paint_buffer == 0 {
            return false;
        }
        unsafe { BufferedPaintSetAlpha(self.paint_buffer, None, alpha) }.is_ok()
    }

    /// `SetOpaque`(`Aero.h:49`)。アルファ値を 255(不透明)に設定する。
    pub fn set_opaque(&mut self) -> bool {
        self.set_alpha(255)
    }

    /// `Initialize`(`Aero.cpp:153-156`、実体は
    /// `CBufferedPaintInitializer::Initialize`、`Aero.cpp:81-89`)。
    /// バッファードペイントをプロセス全体で 1 度だけ初期化する。
    ///
    /// 既に初期化済みなら何もせず `true`。`BufferedPaintInit` が失敗した場合は
    /// `false`(次回呼び出しで再試行される)。
    pub fn initialize() -> bool {
        let mut initialized = BUFFERED_PAINT_INITIALIZED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !*initialized {
            if unsafe { BufferedPaintInit() }.is_err() {
                return false;
            }
            *initialized = true;
        }
        true
    }

    /// `IsSupported`(`Aero.cpp:159-162`、実体は
    /// `CBufferedPaintInitializer::IsInitialized`、`Aero.cpp:91`)。
    /// [`CBufferedPaint::initialize`] が成功済みかを返す。
    #[must_use]
    pub fn is_supported() -> bool {
        *BUFFERED_PAINT_INITIALIZED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Drop for CBufferedPaint {
    /// デストラクタ(`Aero.cpp:100-103`)。`End(false)` で内容を転送せずに破棄する。
    fn drop(&mut self) {
        let _ = self.end(false);
    }
}

/// `CDoubleBufferingDraw`(`Aero.h:58-63`)。
///
/// 原実装は純粋仮想関数 `Draw` と具象メンバ関数 `OnPaint` を持つ抽象クラス。
/// Rust では `draw` を必須メソッド、`on_paint` をデフォルト実装とする trait として
/// 移植する。
pub trait CDoubleBufferingDraw {
    /// `Draw`(`Aero.h:61`、純粋仮想関数)。`hdc` に `paint_rect` の範囲を描画する。
    fn draw(&mut self, hdc: HDC, paint_rect: &RECT);

    /// `OnPaint`(`Aero.cpp:167-184`)。`WM_PAINT` の処理。
    ///
    /// `BeginPaint`/`EndPaint` の間で、バッファードペイントが利用可能なら
    /// クライアント領域全体のバッファ上に [`CDoubleBufferingDraw::draw`] を実行して
    /// `End(true)` で転送し、利用不可(未初期化・失敗)なら `ps.hdc` に直接描画する
    /// (`Aero.cpp:175-181`)。
    ///
    /// # Safety
    /// `hwnd` は呼び出しスレッドが所有する有効なウィンドウハンドルであること。
    unsafe fn on_paint(&mut self, hwnd: HWND) {
        unsafe {
            let mut ps = PAINTSTRUCT::default();
            let _ = BeginPaint(hwnd, &mut ps);
            {
                let mut buffered_paint = CBufferedPaint::new();
                let mut rc = RECT::default();
                let _ = GetClientRect(hwnd, &mut rc);
                match buffered_paint.begin(ps.hdc, &rc, false) {
                    Some(hdc) => {
                        self.draw(hdc, &ps.rcPaint);
                        let _ = buffered_paint.end(true);
                    }
                    None => {
                        self.draw(ps.hdc, &ps.rcPaint);
                    }
                }
            }
            let _ = EndPaint(hwnd, &ps);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::core::w;
    use windows::Win32::Graphics::Gdi::{CreateCompatibleDC, DeleteDC, GetDC, ReleaseDC};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_OVERLAPPEDWINDOW,
    };

    /// テスト用の非表示トップレベルウィンドウを作る(`WS_VISIBLE` なし)。
    /// DWM 系 API はメッセージオンリーウィンドウでは動作しないため、
    /// 非表示のオーバーラップウィンドウを使う。
    fn create_hidden_window() -> HWND {
        unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("tvtest_aero test window"),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                200,
                200,
                None,
                None,
                None,
                None,
            )
            .expect("hidden test window should be created")
        }
    }

    fn destroy_window(hwnd: HWND) {
        unsafe {
            DestroyWindow(hwnd).expect("test window should be destroyed");
        }
    }

    const TEST_RECT: RECT = RECT {
        left: 0,
        top: 0,
        right: 64,
        bottom: 64,
    };

    // ---- CAeroGlass ----

    #[test]
    fn aero_glass_is_enabled_returns_true_on_win8_or_later() {
        // Windows 8 以降では DWM コンポジションは常に有効
        // (DwmIsCompositionEnabled は常に TRUE を返す)。
        assert!(CAeroGlass::is_enabled());
    }

    #[test]
    fn apply_aero_glass_succeeds_on_hidden_window() {
        let hwnd = create_hidden_window();
        let margins_rect = RECT {
            left: 4,
            top: 4,
            right: 4,
            bottom: 4,
        };
        // コンポジション有効(Win8 以降は常時)なら S_OK が返る。
        assert!(unsafe { CAeroGlass::apply_aero_glass(hwnd, &margins_rect) });
        destroy_window(hwnd);
    }

    #[test]
    fn apply_aero_glass_sheet_of_glass_succeeds() {
        // 全フィールド -1 は「シート・オブ・グラス」(全面透過)の指定。
        let hwnd = create_hidden_window();
        let margins_rect = RECT {
            left: -1,
            top: -1,
            right: -1,
            bottom: -1,
        };
        assert!(unsafe { CAeroGlass::apply_aero_glass(hwnd, &margins_rect) });
        destroy_window(hwnd);
    }

    #[test]
    fn apply_aero_glass_fails_with_null_hwnd() {
        // 無効なハンドルでは DwmExtendFrameIntoClientArea がエラーを返し false。
        let margins_rect = RECT {
            left: 1,
            top: 1,
            right: 1,
            bottom: 1,
        };
        assert!(!unsafe { CAeroGlass::apply_aero_glass(HWND::default(), &margins_rect) });
    }

    #[test]
    fn enable_nc_rendering_toggles_on_hidden_window() {
        let hwnd = create_hidden_window();
        // 無効化(DWMNCRP_DISABLED)→有効化(DWMNCRP_USEWINDOWSTYLE)の両方が成功する。
        assert!(unsafe { CAeroGlass::enable_nc_rendering(hwnd, false) });
        assert!(unsafe { CAeroGlass::enable_nc_rendering(hwnd, true) });
        destroy_window(hwnd);
    }

    #[test]
    fn enable_nc_rendering_fails_with_null_hwnd() {
        assert!(!unsafe { CAeroGlass::enable_nc_rendering(HWND::default(), true) });
    }

    // ---- CBufferedPaint ----

    #[test]
    fn buffered_paint_initialize_is_idempotent() {
        assert!(CBufferedPaint::initialize());
        assert!(CBufferedPaint::is_supported());
        // 2 回目以降は何もせず true(Aero.cpp:83-88)。
        assert!(CBufferedPaint::initialize());
        assert!(CBufferedPaint::is_supported());
    }

    #[test]
    fn buffered_paint_begin_end_on_memory_dc() {
        assert!(CBufferedPaint::initialize());
        let hdc = unsafe { CreateCompatibleDC(None) };
        assert!(!hdc.is_invalid());

        let mut paint = CBufferedPaint::new();
        let hdc_buffer = unsafe { paint.begin(hdc, &TEST_RECT, false) };
        assert!(hdc_buffer.is_some());
        assert!(!hdc_buffer.unwrap().is_invalid());
        assert!(paint.end(false));

        unsafe {
            let _ = DeleteDC(hdc);
        }
    }

    #[test]
    fn buffered_paint_begin_with_erase_flag() {
        assert!(CBufferedPaint::initialize());
        let hdc = unsafe { CreateCompatibleDC(None) };
        assert!(!hdc.is_invalid());

        let mut paint = CBufferedPaint::new();
        // BPPF_ERASE 指定(Aero.cpp:117-118)でも開始できる。
        let hdc_buffer = unsafe { paint.begin(hdc, &TEST_RECT, true) };
        assert!(hdc_buffer.is_some());
        assert!(paint.end(false));

        unsafe {
            let _ = DeleteDC(hdc);
        }
    }

    #[test]
    fn buffered_paint_begin_twice_replaces_buffer() {
        assert!(CBufferedPaint::initialize());
        let hdc = unsafe { CreateCompatibleDC(None) };

        let mut paint = CBufferedPaint::new();
        assert!(unsafe { paint.begin(hdc, &TEST_RECT, false) }.is_some());
        // バッファ保持中の再 begin は End(false) 後に開始される(Aero.cpp:111-114)。
        assert!(unsafe { paint.begin(hdc, &TEST_RECT, false) }.is_some());
        assert!(paint.end(false));

        unsafe {
            let _ = DeleteDC(hdc);
        }
    }

    #[test]
    fn buffered_paint_clear_and_set_alpha_require_active_buffer() {
        assert!(CBufferedPaint::initialize());
        let hdc = unsafe { CreateCompatibleDC(None) };

        let mut paint = CBufferedPaint::new();
        // バッファ未使用時は false(Aero.cpp:139-140, 147-148)。
        assert!(!paint.clear(None));
        assert!(!paint.set_alpha(128));
        assert!(!paint.set_opaque());

        assert!(unsafe { paint.begin(hdc, &TEST_RECT, false) }.is_some());
        // バッファ使用中は成功する。
        assert!(paint.clear(None));
        let partial = RECT {
            left: 8,
            top: 8,
            right: 32,
            bottom: 32,
        };
        assert!(paint.clear(Some(&partial)));
        assert!(paint.set_alpha(128));
        assert!(paint.set_opaque());

        assert!(paint.end(true));
        // end 後は再び false。
        assert!(!paint.clear(None));
        assert!(!paint.set_alpha(0));

        unsafe {
            let _ = DeleteDC(hdc);
        }
    }

    #[test]
    fn buffered_paint_end_without_begin_returns_true() {
        // 原実装 End はバッファ未使用でも true を返す(Aero.cpp:127-134)。
        let mut paint = CBufferedPaint::new();
        assert!(paint.end(true));
        assert!(paint.end(false));
    }

    #[test]
    fn buffered_paint_drop_without_end_does_not_panic() {
        assert!(CBufferedPaint::initialize());
        let hdc = unsafe { CreateCompatibleDC(None) };
        {
            let mut paint = CBufferedPaint::new();
            assert!(unsafe { paint.begin(hdc, &TEST_RECT, false) }.is_some());
            // end せずに Drop → End(false) 相当が呼ばれる(Aero.cpp:100-103)。
        }
        {
            let mut paint = CBufferedPaint::new();
            assert!(unsafe { paint.begin(hdc, &TEST_RECT, false) }.is_some());
            assert!(paint.end(false));
            // end 済みで Drop → 二重解放は起こらない(paint_buffer は 0 済み)。
        }
        unsafe {
            let _ = DeleteDC(hdc);
        }
    }

    #[test]
    fn buffered_paint_on_window_dc() {
        assert!(CBufferedPaint::initialize());
        let hwnd = create_hidden_window();
        let hdc = unsafe { GetDC(Some(hwnd)) };
        assert!(!hdc.is_invalid());

        let mut rc = RECT::default();
        unsafe {
            GetClientRect(hwnd, &mut rc).expect("client rect");
        }
        assert!(rc.right > 0 && rc.bottom > 0);

        let mut paint = CBufferedPaint::new();
        assert!(unsafe { paint.begin(hdc, &rc, true) }.is_some());
        assert!(paint.clear(None));
        assert!(paint.end(false));

        unsafe {
            let _ = ReleaseDC(Some(hwnd), hdc);
        }
        destroy_window(hwnd);
    }

    // ---- CDoubleBufferingDraw ----

    /// draw の呼び出し回数と渡された HDC の有効性を記録するテスト用実装。
    struct TestDraw {
        calls: u32,
        hdc_was_valid: bool,
    }

    impl CDoubleBufferingDraw for TestDraw {
        fn draw(&mut self, hdc: HDC, _paint_rect: &RECT) {
            self.calls += 1;
            self.hdc_was_valid = !hdc.is_invalid();
        }
    }

    #[test]
    fn double_buffering_draw_on_paint_calls_draw_exactly_once() {
        // バッファードペイント利用可否のどちらのパスでも draw は 1 回呼ばれる
        // (Aero.cpp:175-181)。
        assert!(CBufferedPaint::initialize());
        let hwnd = create_hidden_window();

        let mut test_draw = TestDraw {
            calls: 0,
            hdc_was_valid: false,
        };
        unsafe {
            test_draw.on_paint(hwnd);
        }
        assert_eq!(test_draw.calls, 1);
        assert!(test_draw.hdc_was_valid);

        destroy_window(hwnd);
    }
}
