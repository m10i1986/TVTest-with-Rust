//! `CBufferedPaint` の未初期化状態の検証。
//!
//! `Begin` は `BufferedPaintInitializer` が未初期化なら `nullptr` を返す
//! (`Aero.cpp:108-109`)。初期化状態はプロセス全体のグローバルであり、
//! `src/lib.rs` 内の単体テスト群は `initialize()` を呼ぶため同一プロセスでは
//! 未初期化状態を検証できない。統合テスト(別プロセスで実行される)として分離し、
//! このファイルには本テスト 1 つだけを置く(同一バイナリ内の他テストが並行して
//! 初期化してしまうのを防ぐため)。

#![cfg(windows)]

use tvtest_aero::CBufferedPaint;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Gdi::{CreateCompatibleDC, DeleteDC};

#[test]
fn begin_fails_before_initialize_and_succeeds_after() {
    // 未初期化状態では is_supported は false、begin は None(Aero.cpp:108-109)。
    assert!(!CBufferedPaint::is_supported());

    let hdc = unsafe { CreateCompatibleDC(None) };
    assert!(!hdc.is_invalid());
    let rc = RECT {
        left: 0,
        top: 0,
        right: 32,
        bottom: 32,
    };

    let mut paint = CBufferedPaint::new();
    assert!(unsafe { paint.begin(hdc, &rc, false) }.is_none());

    // initialize 後は is_supported が true になり begin も成功する
    // (Aero.cpp:81-89, 153-162)。
    assert!(CBufferedPaint::initialize());
    assert!(CBufferedPaint::is_supported());
    assert!(unsafe { paint.begin(hdc, &rc, false) }.is_some());
    assert!(paint.end(false));

    unsafe {
        let _ = DeleteDC(hdc);
    }
}
