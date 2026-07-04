//! TVTest の GUI ユーティリティ(`src/GUIUtil.cpp` / `src/GUIUtil.h`)を windows-rs で
//! 移植したクレート。
//!
//! 移植対象:
//! - `CIcon`(`GUIUtil.h:31-53`。`HICON` の RAII ラッパー)。コピー時は `CopyIcon`
//!   (`GUIUtil.cpp:49-59`)、破棄時は `DestroyIcon`(`GUIUtil.cpp:89-95`)を呼ぶ。
//!   `Load`(`GUIUtil.cpp:62-69`、`LoadIconW`)、`Attach`/`Detach`
//!   (`GUIUtil.cpp:72-86`)を含む。
//! - `CreateImageListFromIcons`(2 オーバーロード、`GUIUtil.cpp:100-133`)。
//!   アイコン識別子の配列からイメージリスト(`HIMAGELIST`)を生成する。依存する
//!   `LoadIconSpecificSize`/`GetStandardIconSize`/`CreateEmptyIcon` は
//!   `tvtest_icon_util` に移植済みのものを利用する。
//! - `SetWindowIcon`(`GUIUtil.cpp:136-146`)。大小 2 種のアイコンを読み込んで
//!   `WM_SETICON` でウィンドウに設定する。
//!
//! windows-rs を用いた GUI 層 Win32 API 呼び出しの本格移植の一環(`tvtest_winutil` に続く)。

#![cfg(windows)]

use tvtest_icon_util::{create_empty_icon, get_standard_icon_size, load_icon_specific_size};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, WPARAM};
use windows::Win32::UI::Controls::{
    ImageList_Create, ImageList_ReplaceIcon, HIMAGELIST, ILC_COLOR32, ILC_MASK,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CopyIcon, DestroyIcon, GetSystemMetrics, LoadIconW, LoadImageW, SendMessageW, HICON, ICON_BIG,
    ICON_SMALL, IMAGE_ICON, LR_DEFAULTSIZE, LR_SHARED, SM_CXSMICON, SM_CYSMICON, WM_SETICON,
};

/// `IconSizeType`(`Util.h:123-126`)。[`create_image_list_from_icons_by_size`] の
/// 引数として使うため `tvtest_icon_util` から再公開する。
pub use tvtest_icon_util::IconSizeType;

/// `CIcon`(`GUIUtil.h:31-53`)。`HICON` の所有権を持つ RAII ラッパー。
///
/// 原実装は `operator HICON() const` / `operator bool() const` を持つが、Rust では
/// [`CIcon::handle`] / [`CIcon::is_created`] として明示的に提供する。
#[derive(Default)]
pub struct CIcon {
    hico: HICON,
}

impl CIcon {
    /// 既定コンストラクタ(`GUIUtil.h:34`、`CIcon() = default`)。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `HICON` を直接受け取るコンストラクタ(`GUIUtil.h:36`)。所有権を引き継ぐ。
    #[must_use]
    pub fn from_handle(hico: HICON) -> Self {
        Self { hico }
    }

    /// `Load`(`GUIUtil.cpp:62-69`、`LoadIconW`)。
    ///
    /// # Safety
    /// `hinst` は有効なモジュールハンドルであること。`name` はリソース識別子
    /// (`PCWSTR` として渡せる、`MAKEINTRESOURCE` 由来の値を含む)であること。
    pub unsafe fn load(&mut self, hinst: HINSTANCE, name: PCWSTR) -> bool {
        self.destroy();
        self.hico = LoadIconW(Some(hinst), name).unwrap_or_default();
        !self.hico.0.is_null()
    }

    /// `Attach`(`GUIUtil.cpp:72-76`)。既存ハンドルを破棄してから所有権を受け取る。
    pub fn attach(&mut self, hico: HICON) {
        self.destroy();
        self.hico = hico;
    }

    /// `Detach`(`GUIUtil.cpp:79-86`)。所有権を手放し、呼び出し元に返す。
    pub fn detach(&mut self) -> HICON {
        std::mem::take(&mut self.hico)
    }

    /// `Destroy`(`GUIUtil.cpp:89-95`、`DestroyIcon`)。
    pub fn destroy(&mut self) {
        if !self.hico.0.is_null() {
            let hico = std::mem::take(&mut self.hico);
            unsafe {
                let _ = DestroyIcon(hico);
            }
        }
    }

    /// `IsCreated`(`GUIUtil.h:48`)。
    #[must_use]
    pub fn is_created(&self) -> bool {
        !self.hico.0.is_null()
    }

    /// `GetHandle`(`GUIUtil.h:49`)。
    #[must_use]
    pub fn handle(&self) -> HICON {
        self.hico
    }
}

impl Clone for CIcon {
    /// コピーコンストラクタ(`GUIUtil.cpp:31-34`, `49-59`)。`CopyIcon` で複製する。
    /// 元のハンドルが空なら複製先も空になる。
    fn clone(&self) -> Self {
        if self.hico.0.is_null() {
            return Self::default();
        }
        let copied = unsafe { CopyIcon(self.hico) }.unwrap_or_default();
        Self { hico: copied }
    }
}

impl Drop for CIcon {
    fn drop(&mut self) {
        self.destroy();
    }
}

/// `CreateImageListFromIcons`(幅・高さ指定版、`GUIUtil.cpp:100-122`)。
///
/// `icons` の各要素をリソース識別子として `LoadIconSpecificSize` で読み込み、
/// 読み込めない場合(要素が null の場合を含む)は `CreateEmptyIcon(Width, Height, 32)`
/// の空アイコンで補い、`ILC_COLOR32 | ILC_MASK` のイメージリストへ順に追加する。
/// 追加し終えたアイコンは原実装どおり `DestroyIcon` で破棄する
/// (`GUIUtil.cpp:117-118`)。
///
/// 原実装の `ppszIcons == nullptr || IconCount <= 0` ガード(`GUIUtil.cpp:103-104`)は、
/// Rust では空スライスの検査に対応する。また原実装の `ImageList_AddIcon` は
/// `commctrl.h` で `ImageList_ReplaceIcon(himl, -1, hicon)` に展開されるマクロのため、
/// windows-rs では `ImageList_ReplaceIcon` を直接呼ぶ。
///
/// 戻り値のイメージリストは呼び出し元が `ImageList_Destroy` で破棄すること。
///
/// # Safety
/// `hinst` は有効なモジュールハンドル(または null)であること。`icons` の各要素は
/// 有効なリソース識別子(`MAKEINTRESOURCE` 由来の値を含む)または null であること。
#[must_use]
pub unsafe fn create_image_list_from_icons(
    hinst: HINSTANCE,
    icons: &[PCWSTR],
    width: i32,
    height: i32,
) -> Option<HIMAGELIST> {
    if icons.is_empty() {
        return None;
    }

    let himl =
        unsafe { ImageList_Create(width, height, ILC_COLOR32 | ILC_MASK, icons.len() as i32, 1) };
    if himl.is_invalid() {
        return None;
    }

    for name in icons {
        let mut hicon: Option<HICON> = None;

        if !name.is_null() {
            hicon = unsafe { load_icon_specific_size(hinst, *name, width, height) };
        }
        if hicon.is_none() {
            hicon = create_empty_icon(width, height, 32);
        }
        let hicon = hicon.unwrap_or_default();
        unsafe {
            let _ = ImageList_ReplaceIcon(himl, -1, hicon);
        }
        if !hicon.0.is_null() {
            unsafe {
                let _ = DestroyIcon(hicon);
            }
        }
    }

    Some(himl)
}

/// `CreateImageListFromIcons`(`IconSizeType` 指定版、`GUIUtil.cpp:125-133`)。
///
/// `GetStandardIconSize`(`tvtest_icon_util` に移植済み)で標準アイコンサイズを
/// 取得し、幅・高さ指定版 [`create_image_list_from_icons`] へ委譲する。
///
/// # Safety
/// [`create_image_list_from_icons`] と同じ。
#[must_use]
pub unsafe fn create_image_list_from_icons_by_size(
    hinst: HINSTANCE,
    icons: &[PCWSTR],
    size: IconSizeType,
) -> Option<HIMAGELIST> {
    let (width, height) = get_standard_icon_size(size)?;
    unsafe { create_image_list_from_icons(hinst, icons, width, height) }
}

/// `SetWindowIcon`(`GUIUtil.cpp:136-146`)。
///
/// 大アイコンを `LR_DEFAULTSIZE | LR_SHARED` で、小アイコン(`SM_CXSMICON` ×
/// `SM_CYSMICON`)を `LR_SHARED` で読み込み、それぞれ `WM_SETICON`(`ICON_BIG` /
/// `ICON_SMALL`)としてウィンドウへ送信する。`LR_SHARED` 指定のためアイコンの破棄は
/// 不要。原実装同様、読み込みに失敗した場合は null をそのまま送信する
/// (アイコン解除に相当)。
///
/// # Safety
/// `hwnd` は有効なウィンドウハンドル、`hinst` は有効なモジュールハンドル
/// (または null)、`icon_name` は有効なリソース識別子であること。
pub unsafe fn set_window_icon(hwnd: HWND, hinst: HINSTANCE, icon_name: PCWSTR) {
    unsafe {
        let hico = LoadImageW(
            Some(hinst),
            icon_name,
            IMAGE_ICON,
            0,
            0,
            LR_DEFAULTSIZE | LR_SHARED,
        )
        .map(|handle| HICON(handle.0))
        .unwrap_or_default();
        let _ = SendMessageW(
            hwnd,
            WM_SETICON,
            Some(WPARAM(ICON_BIG as usize)),
            Some(LPARAM(hico.0 as isize)),
        );

        let hico = LoadImageW(
            Some(hinst),
            icon_name,
            IMAGE_ICON,
            GetSystemMetrics(SM_CXSMICON),
            GetSystemMetrics(SM_CYSMICON),
            LR_SHARED,
        )
        .map(|handle| HICON(handle.0))
        .unwrap_or_default();
        let _ = SendMessageW(
            hwnd,
            WM_SETICON,
            Some(WPARAM(ICON_SMALL as usize)),
            Some(LPARAM(hico.0 as isize)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::core::w;
    use windows::Win32::UI::Controls::{
        ImageList_Destroy, ImageList_GetIconSize, ImageList_GetImageCount,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, HWND_MESSAGE, IDI_APPLICATION, IDI_ERROR, WINDOW_EX_STYLE,
        WINDOW_STYLE, WM_GETICON,
    };

    fn load_system_icon(name: PCWSTR) -> HICON {
        unsafe { LoadIconW(None, name).expect("system icon should load") }
    }

    #[test]
    fn default_is_not_created() {
        let icon = CIcon::new();
        assert!(!icon.is_created());
        assert!(icon.handle().0.is_null());
    }

    #[test]
    fn attach_takes_ownership() {
        let hico = load_system_icon(IDI_APPLICATION);
        let mut icon = CIcon::new();
        icon.attach(hico);
        assert!(icon.is_created());
        assert_eq!(icon.handle().0, hico.0);
    }

    #[test]
    fn detach_releases_ownership() {
        let hico = load_system_icon(IDI_APPLICATION);
        let mut icon = CIcon::new();
        icon.attach(hico);
        let detached = icon.detach();
        assert!(!icon.is_created());
        assert_eq!(detached.0, hico.0);
        unsafe {
            let _ = DestroyIcon(detached);
        }
    }

    #[test]
    fn attach_replaces_previous_handle() {
        let hico1 = load_system_icon(IDI_APPLICATION);
        let hico2 = load_system_icon(IDI_ERROR);
        let mut icon = CIcon::new();
        icon.attach(hico1);
        icon.attach(hico2);
        assert_eq!(icon.handle().0, hico2.0);
    }

    #[test]
    fn destroy_clears_handle() {
        let hico = load_system_icon(IDI_APPLICATION);
        let mut icon = CIcon::new();
        icon.attach(hico);
        icon.destroy();
        assert!(!icon.is_created());
    }

    #[test]
    fn destroy_is_idempotent() {
        let mut icon = CIcon::new();
        icon.destroy();
        icon.destroy();
        assert!(!icon.is_created());
    }

    #[test]
    fn clone_of_empty_is_empty() {
        let icon = CIcon::new();
        let cloned = icon.clone();
        assert!(!cloned.is_created());
    }

    #[test]
    fn clone_duplicates_handle() {
        let hico = load_system_icon(IDI_APPLICATION);
        let mut icon = CIcon::new();
        icon.attach(hico);
        let cloned = icon.clone();
        assert!(cloned.is_created());
        // CopyIcon は別ハンドルを返す(同一ハンドル値にはならない)。
        assert_ne!(cloned.handle().0, icon.handle().0);
    }

    #[test]
    fn drop_destroys_handle() {
        let hico = load_system_icon(IDI_APPLICATION);
        {
            let mut icon = CIcon::new();
            icon.attach(hico);
        }
        // Drop 後に DestroyIcon が呼ばれていること自体は外部から直接観測できないが、
        // 少なくとも Drop 実行時にパニックしないことを確認する。
    }

    #[test]
    fn from_handle_constructs_with_ownership() {
        let hico = load_system_icon(IDI_APPLICATION);
        let icon = CIcon::from_handle(hico);
        assert!(icon.is_created());
        assert_eq!(icon.handle().0, hico.0);
    }

    // ---- CreateImageListFromIcons(GUIUtil.cpp:100-133)----

    #[test]
    fn create_image_list_from_icons_rejects_empty_slice() {
        // 原実装の ppszIcons == nullptr || IconCount <= 0 に対応(GUIUtil.cpp:103-104)。
        let himl = unsafe { create_image_list_from_icons(HINSTANCE::default(), &[], 16, 16) };
        assert!(himl.is_none());
    }

    #[test]
    fn create_image_list_from_icons_counts_all_entries() {
        // null 識別子(空アイコンで補われる)と実在するシステムアイコン識別子を混在
        // させても、全要素分の画像が追加される。
        let icons = [PCWSTR::null(), IDI_APPLICATION, IDI_ERROR];
        let himl = unsafe { create_image_list_from_icons(HINSTANCE::default(), &icons, 16, 16) }
            .expect("image list should be created");
        assert!(!himl.is_invalid());
        let count = unsafe { ImageList_GetImageCount(himl) };
        unsafe {
            let _ = ImageList_Destroy(Some(himl));
        }
        assert_eq!(count, icons.len() as i32);
    }

    #[test]
    fn create_image_list_from_icons_all_null_entries_get_empty_icons() {
        // 全要素 null でも CreateEmptyIcon(GUIUtil.cpp:115-116)で補われる。
        let icons = [PCWSTR::null(), PCWSTR::null()];
        let himl = unsafe { create_image_list_from_icons(HINSTANCE::default(), &icons, 32, 32) }
            .expect("image list should be created");
        let count = unsafe { ImageList_GetImageCount(himl) };
        unsafe {
            let _ = ImageList_Destroy(Some(himl));
        }
        assert_eq!(count, icons.len() as i32);
    }

    #[test]
    fn create_image_list_from_icons_by_size_uses_standard_size() {
        let icons = [IDI_APPLICATION];
        let himl = unsafe {
            create_image_list_from_icons_by_size(HINSTANCE::default(), &icons, IconSizeType::Small)
        }
        .expect("image list should be created");

        let count = unsafe { ImageList_GetImageCount(himl) };
        let mut cx = 0;
        let mut cy = 0;
        let size_ok = unsafe { ImageList_GetIconSize(himl, Some(&mut cx), Some(&mut cy)) };
        unsafe {
            let _ = ImageList_Destroy(Some(himl));
        }

        assert_eq!(count, icons.len() as i32);
        assert!(size_ok.as_bool());
        let (sw, sh) = tvtest_icon_util::get_standard_icon_size(IconSizeType::Small).unwrap();
        assert_eq!((cx, cy), (sw, sh));
    }

    // ---- SetWindowIcon(GUIUtil.cpp:136-146)----

    /// テスト用のメッセージオンリーウィンドウ(親 `HWND_MESSAGE`、非表示)を作る。
    /// `WM_SETICON`/`WM_GETICON` は `DefWindowProc` が処理するため、組み込みの
    /// `STATIC` クラスで十分。
    fn create_message_only_window() -> HWND {
        unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                PCWSTR::null(),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )
            .expect("message-only window should be created")
        }
    }

    fn get_window_icon(hwnd: HWND, icon_type: u32) -> isize {
        unsafe { SendMessageW(hwnd, WM_GETICON, Some(WPARAM(icon_type as usize)), None).0 }
    }

    #[test]
    fn set_window_icon_sets_big_and_small_icons() {
        let hwnd = create_message_only_window();

        // hinst=null + IDI_APPLICATION は LR_SHARED 指定の LoadImage で OEM アイコン
        // として読み込まれる。
        unsafe {
            set_window_icon(hwnd, HINSTANCE::default(), IDI_APPLICATION);
        }
        let big = get_window_icon(hwnd, ICON_BIG);
        let small = get_window_icon(hwnd, ICON_SMALL);

        unsafe {
            let _ = DestroyWindow(hwnd);
        }

        assert_ne!(big, 0, "big icon should be set");
        assert_ne!(small, 0, "small icon should be set");
    }

    #[test]
    fn set_window_icon_sends_null_for_missing_icon() {
        // 原実装は LoadImage 失敗時も戻り値(null)をそのまま WM_SETICON で送信する
        // (GUIUtil.cpp:138-145)ため、既存のアイコンは解除される。
        let hwnd = create_message_only_window();

        unsafe {
            set_window_icon(hwnd, HINSTANCE::default(), IDI_APPLICATION);
        }
        assert_ne!(get_window_icon(hwnd, ICON_BIG), 0);

        unsafe {
            set_window_icon(
                hwnd,
                HINSTANCE::default(),
                w!("tvtest_gui_util_no_such_icon"),
            );
        }
        let big = get_window_icon(hwnd, ICON_BIG);
        let small = get_window_icon(hwnd, ICON_SMALL);

        unsafe {
            let _ = DestroyWindow(hwnd);
        }

        assert_eq!(big, 0, "big icon should be cleared");
        assert_eq!(small, 0, "small icon should be cleared");
    }
}
