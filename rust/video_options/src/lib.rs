//! TVTest VideoOptions.cpp / VideoOptions.h の純粋部分の Rust 移植。
//!
//! 映像レンダラの既定テーブルと名前⇔種別変換(VideoRenderer.cpp:446-478 相当)、
//! 表示ストレッチモードの列挙(ViewerFilter.hpp:92-96)、ReadSettings の
//! ストレッチモード変換ロジック(VideoOptions.cpp:108-130)、更新フラグ定数
//! (VideoOptions.h:83-89)を移植する。
//!
//! DlgProc(ダイアログ)・CSettings I/O 本体・DirectShow 連携(VideoRenderer::
//! IsAvailable/CreateRenderer、FilterFinder によるデコーダ列挙)は対象外。

#![forbid(unsafe_code)]

/// 映像レンダラの種類(LibISDB::DirectShow::VideoRenderer::RendererType, VideoRenderer.hpp:45-57)。
///
/// 判別子の値は `EnumRendererName`/`ParseName`(VideoRenderer.cpp:446-478)の配列添字と
/// 一致させる必要があるため、`Invalid`を除き明示的に振っている。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RendererType {
    Invalid = -1,
    #[default]
    Default = 0,
    Vmr7 = 1,
    Vmr9 = 2,
    Vmr7Renderless = 3,
    Vmr9Renderless = 4,
    Evr = 5,
    OverlayMixer = 6,
    MadVr = 7,
    EvrCustomPresenter = 8,
    McpVideoRenderer = 9,
}

/// `EnumRendererName`(VideoRenderer.cpp:446-463)の名前テーブル。配列添字が `RendererType` の値。
const RENDERER_NAME_LIST: &[&str] = &[
    "Default",
    "VMR7",
    "VMR9",
    "VMR7 Renderless",
    "VMR9 Renderless",
    "EVR",
    "Overlay Mixer",
    "madVR",
    "EVR Custom Presenter",
    "MPC Video Renderer",
];

impl RendererType {
    fn from_index(index: i32) -> Option<Self> {
        match index {
            0 => Some(Self::Default),
            1 => Some(Self::Vmr7),
            2 => Some(Self::Vmr9),
            3 => Some(Self::Vmr7Renderless),
            4 => Some(Self::Vmr9Renderless),
            5 => Some(Self::Evr),
            6 => Some(Self::OverlayMixer),
            7 => Some(Self::MadVr),
            8 => Some(Self::EvrCustomPresenter),
            9 => Some(Self::McpVideoRenderer),
            _ => None,
        }
    }

    /// `static_cast<RendererType>(Index)` に相当するインデックス化。
    pub fn to_index(self) -> i32 {
        self as i32
    }
}

/// `VideoRenderer::EnumRendererName(int Index)`(VideoRenderer.cpp:446-463)。
///
/// 範囲外は `None`(原実装は `nullptr`)。
pub fn enum_renderer_name(index: i32) -> Option<&'static str> {
    if index < 0 {
        return None;
    }
    RENDERER_NAME_LIST.get(index as usize).copied()
}

/// `VideoRenderer::EnumRendererName(RendererType Type)`(VideoRenderer.hpp:85)。
pub fn enum_renderer_name_for_type(renderer: RendererType) -> Option<&'static str> {
    enum_renderer_name(renderer.to_index())
}

/// `VideoRenderer::ParseName(LPCTSTR pszName)`(VideoRenderer.cpp:468-478)。
///
/// 大小無視で `EnumRendererName` の一覧を先頭から線形検索し、一致した最初の
/// インデックスを返す。一致しなければ `RendererType::Invalid`。
pub fn parse_renderer_name(name: &str) -> RendererType {
    for (i, candidate) in RENDERER_NAME_LIST.iter().enumerate() {
        if candidate.eq_ignore_ascii_case(name) {
            return RendererType::from_index(i as i32).unwrap_or(RendererType::Invalid);
        }
    }
    RendererType::Invalid
}

/// `CVideoOptions::RendererInfo`(VideoOptions.h:37-41)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RendererInfo {
    pub renderer: RendererType,
    pub name: &'static str,
}

/// `CVideoOptions::m_RendererList`(VideoOptions.cpp:36-47)。
///
/// 原実装ではコメントアウトされている VMR7/VMR7Renderless/OverlayMixer は
/// ダイアログの選択肢に出さないため、このリストには含めない(GetRendererInfo が
/// 参照するのはこのテーブルのみで、EnumRendererName の全種別テーブルとは別物)。
pub const RENDERER_LIST: &[RendererInfo] = &[
    RendererInfo {
        renderer: RendererType::Default,
        name: "システムデフォルト",
    },
    RendererInfo {
        renderer: RendererType::Vmr9,
        name: "VMR9",
    },
    RendererInfo {
        renderer: RendererType::Vmr9Renderless,
        name: "VMR9 Renderless",
    },
    RendererInfo {
        renderer: RendererType::Evr,
        name: "EVR",
    },
    RendererInfo {
        renderer: RendererType::EvrCustomPresenter,
        name: "EVR (Custom Presenter)",
    },
    RendererInfo {
        renderer: RendererType::MadVr,
        name: "madVR",
    },
    RendererInfo {
        renderer: RendererType::McpVideoRenderer,
        name: "MPC Video Renderer",
    },
];

/// `CVideoOptions::GetRendererInfo(int Index, RendererInfo *pInfo)`(VideoOptions.cpp:50-56)。
pub fn get_renderer_info(index: i32) -> Option<RendererInfo> {
    if index < 0 {
        return None;
    }
    RENDERER_LIST.get(index as usize).copied()
}

/// `LibISDB::ViewerFilter::ViewStretchMode`(ViewerFilter.hpp:92-96)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewStretchMode {
    #[default]
    KeepAspectRatio,
    Crop,
    Fit,
}

/// `ReadSettings` の `FrameCut`(bool)→ストレッチモード変換(VideoOptions.cpp:111-116)。
///
/// `true` なら全体表示優先の `Crop`、`false` ならアスペクト比保持。
pub fn stretch_mode_from_frame_cut(frame_cut: bool) -> ViewStretchMode {
    if frame_cut {
        ViewStretchMode::Crop
    } else {
        ViewStretchMode::KeepAspectRatio
    }
}

/// `WriteSettings` の `FrameCut` 書き出し(VideoOptions.cpp:147)= `Crop` かどうか。
pub fn frame_cut_from_stretch_mode(mode: ViewStretchMode) -> bool {
    mode == ViewStretchMode::Crop
}

/// `ReadSettings` の `FullscreenStretchMode`/`MaximizeStretchMode`(整数値)→
/// ストレッチモード変換(VideoOptions.cpp:118-130)。
///
/// 値が `1` なら `Crop`、それ以外(0 含む)は `KeepAspectRatio`。原実装は
/// `Fit` を書き込まないため往復では出現しないが、`WriteSettings` は
/// `static_cast<int>` でそのまま整数化するので `Fit` を渡されても
/// `KeepAspectRatio` と区別なく扱われる(=このテーブルには現れない値)。
pub fn stretch_mode_from_legacy_int(value: i32) -> ViewStretchMode {
    if value == 1 {
        ViewStretchMode::Crop
    } else {
        ViewStretchMode::KeepAspectRatio
    }
}

/// `WriteSettings` の `FullscreenStretchMode`/`MaximizeStretchMode` 書き出し
/// (VideoOptions.cpp:148-149)= `static_cast<int>(mode)`。
pub fn stretch_mode_to_legacy_int(mode: ViewStretchMode) -> i32 {
    mode as i32
}

/// `CVideoOptions::SetVideoRendererType`(VideoOptions.cpp:208-214)。
///
/// `EnumRendererName` が `None` を返す(未知の)種別は拒否する。
pub fn is_valid_renderer_type(renderer: RendererType) -> bool {
    enum_renderer_name_for_type(renderer).is_some()
}

/// `CVideoOptions` の更新フラグ(VideoOptions.h:83-89)。
pub mod update_flag {
    pub const DECODER: u32 = 0x0000_0001;
    pub const RENDERER: u32 = 0x0000_0002;
    pub const MASK_CUT_AREA: u32 = 0x0000_0004;
    pub const IGNORE_DISPLAY_EXTENSION: u32 = 0x0000_0008;
    pub const CLIP_TO_DEVICE: u32 = 0x0000_0010;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enum_renderer_name_matches_table() {
        assert_eq!(enum_renderer_name(0), Some("Default"));
        assert_eq!(enum_renderer_name(2), Some("VMR9"));
        assert_eq!(enum_renderer_name(9), Some("MPC Video Renderer"));
    }

    #[test]
    fn enum_renderer_name_out_of_range_is_none() {
        assert_eq!(enum_renderer_name(-1), None);
        assert_eq!(enum_renderer_name(10), None);
    }

    #[test]
    fn enum_renderer_name_for_type_roundtrip() {
        assert_eq!(enum_renderer_name_for_type(RendererType::Evr), Some("EVR"));
        assert_eq!(
            enum_renderer_name_for_type(RendererType::McpVideoRenderer),
            Some("MPC Video Renderer")
        );
    }

    #[test]
    fn enum_renderer_name_for_invalid_is_none() {
        assert_eq!(enum_renderer_name_for_type(RendererType::Invalid), None);
    }

    #[test]
    fn parse_name_case_insensitive() {
        assert_eq!(parse_renderer_name("evr"), RendererType::Evr);
        assert_eq!(parse_renderer_name("MADVR"), RendererType::MadVr);
        assert_eq!(parse_renderer_name("Default"), RendererType::Default);
    }

    #[test]
    fn parse_name_unknown_is_invalid() {
        assert_eq!(parse_renderer_name(""), RendererType::Invalid);
        assert_eq!(parse_renderer_name("NoSuchRenderer"), RendererType::Invalid);
    }

    #[test]
    fn renderer_list_get_by_index() {
        let info = get_renderer_info(0).unwrap();
        assert_eq!(info.renderer, RendererType::Default);
        assert_eq!(info.name, "システムデフォルト");

        let info = get_renderer_info(6).unwrap();
        assert_eq!(info.renderer, RendererType::McpVideoRenderer);
    }

    #[test]
    fn renderer_list_out_of_range_is_none() {
        assert!(get_renderer_info(-1).is_none());
        assert!(get_renderer_info(7).is_none());
    }

    #[test]
    fn renderer_list_excludes_commented_out_entries() {
        // 原実装で `//` コメントアウトされている VMR7/VMR7Renderless/OverlayMixer は
        // ダイアログ選択肢テーブルに含まれない。
        assert!(RENDERER_LIST
            .iter()
            .all(|info| info.renderer != RendererType::Vmr7));
        assert!(RENDERER_LIST
            .iter()
            .all(|info| info.renderer != RendererType::Vmr7Renderless));
        assert!(RENDERER_LIST
            .iter()
            .all(|info| info.renderer != RendererType::OverlayMixer));
        assert_eq!(RENDERER_LIST.len(), 7);
    }

    #[test]
    fn stretch_mode_from_frame_cut_true_is_crop() {
        assert_eq!(stretch_mode_from_frame_cut(true), ViewStretchMode::Crop);
    }

    #[test]
    fn stretch_mode_from_frame_cut_false_is_keep_aspect_ratio() {
        assert_eq!(
            stretch_mode_from_frame_cut(false),
            ViewStretchMode::KeepAspectRatio
        );
    }

    #[test]
    fn frame_cut_roundtrip() {
        assert!(frame_cut_from_stretch_mode(stretch_mode_from_frame_cut(
            true
        )));
        assert!(!frame_cut_from_stretch_mode(stretch_mode_from_frame_cut(
            false
        )));
    }

    #[test]
    fn stretch_mode_from_legacy_int_one_is_crop() {
        assert_eq!(stretch_mode_from_legacy_int(1), ViewStretchMode::Crop);
    }

    #[test]
    fn stretch_mode_from_legacy_int_other_is_keep_aspect_ratio() {
        assert_eq!(
            stretch_mode_from_legacy_int(0),
            ViewStretchMode::KeepAspectRatio
        );
        assert_eq!(
            stretch_mode_from_legacy_int(2),
            ViewStretchMode::KeepAspectRatio
        );
        assert_eq!(
            stretch_mode_from_legacy_int(-1),
            ViewStretchMode::KeepAspectRatio
        );
    }

    #[test]
    fn stretch_mode_to_legacy_int_matches_static_cast() {
        assert_eq!(
            stretch_mode_to_legacy_int(ViewStretchMode::KeepAspectRatio),
            0
        );
        assert_eq!(stretch_mode_to_legacy_int(ViewStretchMode::Crop), 1);
        assert_eq!(stretch_mode_to_legacy_int(ViewStretchMode::Fit), 2);
    }

    #[test]
    fn is_valid_renderer_type_accepts_known() {
        assert!(is_valid_renderer_type(RendererType::Default));
        assert!(is_valid_renderer_type(RendererType::McpVideoRenderer));
    }

    #[test]
    fn is_valid_renderer_type_rejects_invalid() {
        assert!(!is_valid_renderer_type(RendererType::Invalid));
    }

    #[test]
    fn update_flags_match_header() {
        assert_eq!(update_flag::DECODER, 0x0000_0001);
        assert_eq!(update_flag::RENDERER, 0x0000_0002);
        assert_eq!(update_flag::MASK_CUT_AREA, 0x0000_0004);
        assert_eq!(update_flag::IGNORE_DISPLAY_EXTENSION, 0x0000_0008);
        assert_eq!(update_flag::CLIP_TO_DEVICE, 0x0000_0010);
    }

    #[test]
    fn default_renderer_type_is_default() {
        assert_eq!(RendererType::default(), RendererType::Default);
    }

    #[test]
    fn default_stretch_mode_is_keep_aspect_ratio() {
        assert_eq!(ViewStretchMode::default(), ViewStretchMode::KeepAspectRatio);
    }
}
