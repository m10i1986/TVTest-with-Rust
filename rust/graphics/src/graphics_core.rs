//! `CGraphicsCore`(`Graphics.cpp:64-92` / `Graphics.h:197-213`)の移植。

use windows::Win32::Graphics::GdiPlus::{GdiplusShutdown, GdiplusStartup, GdiplusStartupInput, Ok as GpOk};

/// `CGraphicsCore`(`Graphics.h:197-213`)。GDI+ の初期化/終了(`GdiplusStartup` /
/// `GdiplusShutdown`)を RAII で管理する。
///
/// 原実装と同じくコピー不可(Rust では `Clone` を実装しないことで表現)。
#[derive(Debug)]
pub struct GraphicsCore {
    initialized: bool,
    token: usize,
}

impl GraphicsCore {
    /// `CGraphicsCore()`(`Graphics.h:200`)。未初期化状態で生成する。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            initialized: false,
            token: 0,
        }
    }

    /// `Initialize`(`Graphics.cpp:70-83`)。GDI+ を初期化する。
    ///
    /// `GdiplusVersion = 1` で `GdiplusStartup` を呼ぶ。既に初期化済みなら何もせず
    /// `true`。失敗時は `false`。
    pub fn initialize(&mut self) -> bool {
        if !self.initialized {
            let input = GdiplusStartupInput {
                GdiplusVersion: 1,
                DebugEventCallback: 0,
                SuppressBackgroundThread: false.into(),
                SuppressExternalCodecs: false.into(),
            };
            let mut token = 0usize;
            if unsafe { GdiplusStartup(&mut token, &input, std::ptr::null_mut()) } != GpOk {
                return false;
            }
            self.token = token;
            self.initialized = true;
        }
        true
    }

    /// `Finalize`(`Graphics.cpp:86-92`)。GDI+ を終了する。未初期化なら何もしない。
    pub fn finalize(&mut self) {
        if self.initialized {
            unsafe { GdiplusShutdown(self.token) };
            self.initialized = false;
        }
    }

    /// `IsInitialized`(`Graphics.h:208`)。初期化済みか取得する。
    #[must_use]
    pub const fn is_initialized(&self) -> bool {
        self.initialized
    }
}

impl Default for GraphicsCore {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for GraphicsCore {
    /// `~CGraphicsCore`(`Graphics.cpp:64-67`)。`Finalize` を呼ぶ。
    fn drop(&mut self) {
        self.finalize();
    }
}
