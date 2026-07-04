//! `CColor` / `GradientDirection` / `TextFlag`(`Graphics.h:43-83`)の移植。

use bitflags::bitflags;

/// GDI+ の ARGB 値を組み立てる(`Gdiplus::Color(a, r, g, b)` 相当)。
///
/// `GdiplusColor`(`Graphics.cpp:37-45`)が行う変換の共通部分。
pub(crate) const fn make_argb(a: u8, r: u8, g: u8, b: u8) -> u32 {
    ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

/// `CColor`(`Graphics.h:72-83`)。8bit RGBA の色。
///
/// 既定値は全成分 0(`Graphics.h:75-78`)。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Color {
    /// `Red`(`Graphics.h:75`)。
    pub red: u8,
    /// `Green`(`Graphics.h:76`)。
    pub green: u8,
    /// `Blue`(`Graphics.h:77`)。
    pub blue: u8,
    /// `Alpha`(`Graphics.h:78`)。
    pub alpha: u8,
}

impl Color {
    /// `CColor(BYTE r, BYTE g, BYTE b, BYTE a)`(`Graphics.h:81`)。
    #[must_use]
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self {
            red: r,
            green: g,
            blue: b,
            alpha: a,
        }
    }

    /// `CColor(BYTE r, BYTE g, BYTE b)`(`Graphics.h:81` のデフォルト引数
    /// `a = 255` 相当)。
    #[must_use]
    pub const fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Self::new(r, g, b, 255)
    }

    /// `CColor(COLORREF cr)`(`Graphics.h:82`)。COLORREF(`0x00BBGGRR`)から
    /// 変換する。アルファは 255。
    #[must_use]
    pub const fn from_colorref(cr: u32) -> Self {
        Self::new(
            (cr & 0xFF) as u8,
            ((cr >> 8) & 0xFF) as u8,
            ((cr >> 16) & 0xFF) as u8,
            255,
        )
    }

    /// `GdiplusColor(const CColor &Color)`(`Graphics.cpp:42-45`)。
    /// GDI+ の ARGB 値(`0xAARRGGBB`)へ変換する。
    #[must_use]
    pub const fn to_argb(self) -> u32 {
        make_argb(self.alpha, self.red, self.green, self.blue)
    }
}

/// `GradientDirection`(`Graphics.h:43-46`)。グラデーションの方向。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GradientDirection {
    /// 水平方向(`Horz`)。
    Horz,
    /// 垂直方向(`Vert`)。
    Vert,
}

bitflags! {
    /// `TextFlag`(`Graphics.h:48-70`)。テキスト描画/計測のフラグ。
    ///
    /// `Format_Left` / `Format_Top` / `None` は値 0 のため、bitflags の
    /// 関連定数として別途定義している([`TextFlag::FORMAT_LEFT`] 等)。
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
    pub struct TextFlag: u32 {
        /// `Format_Right`(`Graphics.h:51`)。右寄せ。
        const FORMAT_RIGHT = 0x0000_0001;
        /// `Format_HorzCenter`(`Graphics.h:52`)。水平中央寄せ。
        const FORMAT_HORZ_CENTER = 0x0000_0002;
        /// `Format_HorzAlignMask`(`Graphics.h:53`)。水平アライメントのマスク。
        const FORMAT_HORZ_ALIGN_MASK = 0x0000_0003;
        /// `Format_Bottom`(`Graphics.h:55`)。下寄せ。
        const FORMAT_BOTTOM = 0x0000_0004;
        /// `Format_VertCenter`(`Graphics.h:56`)。垂直中央寄せ。
        const FORMAT_VERT_CENTER = 0x0000_0008;
        /// `Format_VertAlignMask`(`Graphics.h:57`)。垂直アライメントのマスク。
        const FORMAT_VERT_ALIGN_MASK = 0x0000_000C;
        /// `Format_NoWrap`(`Graphics.h:58`)。折り返しなし。
        const FORMAT_NO_WRAP = 0x0000_0010;
        /// `Format_NoClip`(`Graphics.h:59`)。クリッピングなし。
        const FORMAT_NO_CLIP = 0x0000_0020;
        /// `Format_EndEllipsis`(`Graphics.h:60`)。末尾を省略記号にする。
        const FORMAT_END_ELLIPSIS = 0x0000_0040;
        /// `Format_WordEllipsis`(`Graphics.h:61`)。単語単位で省略記号にする。
        const FORMAT_WORD_ELLIPSIS = 0x0000_0080;
        /// `Format_TrimChar`(`Graphics.h:62`)。文字単位で切り詰める。
        const FORMAT_TRIM_CHAR = 0x0000_0100;
        /// `Format_ClipLastLine`(`Graphics.h:63`)。はみ出す行を表示しない
        /// (`StringFormatFlagsLineLimit`)。
        const FORMAT_CLIP_LAST_LINE = 0x0000_0200;
        /// `Draw_Antialias`(`Graphics.h:64`)。アンチエイリアス描画。
        const DRAW_ANTIALIAS = 0x0000_1000;
        /// `Draw_NoAntialias`(`Graphics.h:65`)。アンチエイリアスなし描画。
        const DRAW_NO_ANTIALIAS = 0x0000_2000;
        /// `Draw_ClearType`(`Graphics.h:66`)。ClearType 描画。
        const DRAW_CLEAR_TYPE = 0x0000_4000;
        /// `Draw_Hinting`(`Graphics.h:67`)。ヒンティング有効。
        const DRAW_HINTING = 0x0000_8000;
        /// `Draw_Path`(`Graphics.h:68`)。`GraphicsPath` 経由で描画する。
        const DRAW_PATH = 0x0001_0000;
    }
}

impl TextFlag {
    /// `TextFlag::None`(`Graphics.h:49`)。
    pub const NONE: Self = Self::empty();
    /// `Format_Left`(`Graphics.h:50`)。左寄せ(既定)。
    pub const FORMAT_LEFT: Self = Self::empty();
    /// `Format_Top`(`Graphics.h:54`)。上寄せ(既定)。
    pub const FORMAT_TOP: Self = Self::empty();
}
