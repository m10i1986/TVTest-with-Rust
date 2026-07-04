//! テストバイナリに Windows 8 以降対応(supportedOS)のマニフェストを埋め込む。
//!
//! レイヤードの子ウィンドウ(`WS_EX_LAYERED | WS_CHILD`)は Windows 8 以降で
//! サポートされるが、プロセスのマニフェストで Windows 8 以降への対応を宣言して
//! いないと互換コンテキストが Vista 相当になり、`CreateWindowExW` が
//! `ERROR_INVALID_PARAMETER` で失敗する。TVTest.exe 本体はマニフェストで宣言
//! しているため、テストでも同条件になるように埋め込む。

fn main() {
    println!("cargo:rerun-if-changed=windows8.manifest");
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("windows-msvc") {
        let dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR should be set");
        let manifest = std::path::Path::new(&dir).join("windows8.manifest");
        // rustc-link-arg-tests は統合テスト([[test]])のみが対象で、本クレートの
        // ようにライブラリの単体テストしか無いパッケージではエラーになるため、
        // rustc-link-arg(テストバイナリにも適用される)を使う。
        // 本パッケージはリンクを行うターゲットがテストバイナリしか無い。
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    }
}
