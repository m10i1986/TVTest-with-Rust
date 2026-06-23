//! TVTest のキャプチャ設定(src/CaptureOptions.cpp / CaptureOptions.h)の純粋部分の移植。
//!
//! キャプチャ画像サイズの指定を、設定入出力や画像コーデックから切り離した純粋なモデルとして
//! 表現する:
//! - [`SizeType`](原寸 / 表示サイズ / RAW / カスタム / 百分率)
//! - [`SIZE_LIST`] / [`PERCENTAGE_LIST`](カスタムサイズ・百分率の静的表)
//! - [`CaptureSize`](プリセット整数 ↔ 状態の符号化、表の参照)
//! - [`format_capture_variable`](`%width%`/`%height%` の値文字列化)
//!
//! # 対象外(Win32 / I-O 依存)
//! 設定入出力(`ReadSettings`/`WriteSettings`)、画像コーデック(`CImageCodec`)、ダイアログ
//! (`DlgProc`)、ファイル名生成(`GenerateFileName`)、コメント生成、画像保存。

/// キャプチャサイズの種別(CaptureOptions.h 37-44 の enum)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SizeType {
    /// 原寸(`SIZE_TYPE_ORIGINAL`)。
    Original = 0,
    /// 表示サイズ(`SIZE_TYPE_VIEW`)。
    View = 1,
    /// RAW(`SIZE_TYPE_RAW`)。
    Raw = 2,
    /// カスタム固定サイズ(`SIZE_TYPE_CUSTOM`)。
    Custom = 3,
    /// 百分率(`SIZE_TYPE_PERCENTAGE`)。
    Percentage = 4,
}

/// カスタムサイズの一覧(CaptureOptions.cpp 113-131 `m_SizeList`)。16:9 と 4:3。
pub const SIZE_LIST: [(i32, i32); 15] = [
    // 16:9
    (1920, 1080),
    (1440, 810),
    (1280, 720),
    (1024, 576),
    (960, 540),
    (800, 450),
    (640, 360),
    (320, 180),
    // 4:3
    (1440, 1080),
    (1280, 960),
    (1024, 768),
    (800, 600),
    (720, 540),
    (640, 480),
    (320, 240),
];

/// 百分率の一覧(CaptureOptions.cpp 134-140 `m_PercentageList`、分子/分母)。
pub const PERCENTAGE_LIST: [(i32, i32); 5] = [
    (3, 4), // 75%
    (2, 3), // 66%
    (1, 2), // 50%
    (1, 3), // 33%
    (1, 4), // 25%
];

/// [`SIZE_LIST`] の最終索引(`SIZE_LAST`)。
pub const SIZE_LAST: usize = SIZE_LIST.len() - 1;
/// [`PERCENTAGE_LIST`] の最終索引(`PERCENTAGE_LAST`)。
pub const PERCENTAGE_LAST: usize = PERCENTAGE_LIST.len() - 1;

/// 既定のカスタムサイズ索引(`SIZE_1920x1080`。1seg ビルドは別だが本移植は通常版)。
pub const SIZE_DEFAULT: usize = 0;
/// 既定の百分率索引(`PERCENTAGE_50`)。
pub const PERCENTAGE_50: usize = 2;

/// キャプチャサイズ設定(CaptureOptions の `m_CaptureSizeType`/`m_CaptureSize`/
/// `m_CapturePercentage` のモデル)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureSize {
    size_type: SizeType,
    capture_size: usize,
    capture_percentage: usize,
}

impl Default for CaptureSize {
    fn default() -> Self {
        // CaptureOptions.h 107-115 の既定値。
        Self {
            size_type: SizeType::Original,
            capture_size: SIZE_DEFAULT,
            capture_percentage: PERCENTAGE_50,
        }
    }
}

impl CaptureSize {
    /// 既定のキャプチャサイズ設定を生成する。
    pub fn new() -> Self {
        Self::default()
    }

    /// 現在のサイズ種別を返す(`GetCaptureSizeType`)。
    pub fn size_type(&self) -> SizeType {
        self.size_type
    }

    /// カスタムサイズの索引を返す。
    pub fn capture_size_index(&self) -> usize {
        self.capture_size
    }

    /// 百分率の索引を返す。
    pub fn capture_percentage_index(&self) -> usize {
        self.capture_percentage
    }

    /// プリセット整数から状態を設定する(CaptureOptions.cpp 230-246 `SetPresetCaptureSize`)。
    ///
    /// 整数の割り当ては「0:原寸 / 1:表示サイズ / 2..6:百分率 / 7..21:カスタム」。
    /// 範囲外は `false`。
    pub fn set_preset_capture_size(&mut self, size: i32) -> bool {
        if size < 0 {
            return false;
        }
        if size <= SizeType::View as i32 {
            // 0 = Original, 1 = View。
            self.size_type = if size == SizeType::Original as i32 {
                SizeType::Original
            } else {
                SizeType::View
            };
        } else if (size - 2) <= PERCENTAGE_LAST as i32 {
            self.size_type = SizeType::Percentage;
            self.capture_percentage = (size - 2) as usize;
        } else if size - (2 + PERCENTAGE_LAST as i32 + 1) <= SIZE_LAST as i32 {
            self.size_type = SizeType::Custom;
            self.capture_size = (size - (2 + PERCENTAGE_LAST as i32 + 1)) as usize;
        } else {
            return false;
        }
        true
    }

    /// 現在の状態をプリセット整数へ符号化する(CaptureOptions.cpp 249-266 `GetPresetCaptureSize`)。
    ///
    /// 原実装は `RAW` を処理しない(到達しない)。本移植では種別値をそのまま返す。
    pub fn preset_capture_size(&self) -> i32 {
        match self.size_type {
            SizeType::Original | SizeType::View | SizeType::Raw => self.size_type as i32,
            SizeType::Custom => 2 + (PERCENTAGE_LAST as i32 + 1) + self.capture_size as i32,
            SizeType::Percentage => 2 + self.capture_percentage as i32,
        }
    }

    /// 百分率(分子, 分母)を返す(CaptureOptions.cpp 269-276 `GetSizePercentage`)。
    pub fn size_percentage(&self) -> (i32, i32) {
        PERCENTAGE_LIST[self.capture_percentage]
    }

    /// カスタムサイズ(幅, 高さ)を返す(CaptureOptions.cpp 279-286 `GetCustomSize`)。
    pub fn custom_size(&self) -> (i32, i32) {
        SIZE_LIST[self.capture_size]
    }
}

/// キャプチャ用変数の値を文字列化する
/// (CaptureOptions.cpp 82-93 `CCaptureVariableStringMap::GetLocalString` の固有部)。
///
/// `width`/`height`(大小無視)に画像の幅/高さを返す。該当しなければ `None`。
pub fn format_capture_variable(keyword: &str, image_width: i32, image_height: i32) -> Option<String> {
    if keyword.eq_ignore_ascii_case("width") {
        Some(image_width.to_string())
    } else if keyword.eq_ignore_ascii_case("height") {
        Some(image_height.to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state() {
        let c = CaptureSize::new();
        assert_eq!(c.size_type(), SizeType::Original);
        assert_eq!(c.capture_size_index(), 0);
        assert_eq!(c.capture_percentage_index(), 2);
    }

    #[test]
    fn decode_original_and_view() {
        let mut c = CaptureSize::new();
        assert!(c.set_preset_capture_size(0));
        assert_eq!(c.size_type(), SizeType::Original);
        assert!(c.set_preset_capture_size(1));
        assert_eq!(c.size_type(), SizeType::View);
    }

    #[test]
    fn decode_percentage() {
        let mut c = CaptureSize::new();
        // 2 = 百分率の先頭(75%)。
        assert!(c.set_preset_capture_size(2));
        assert_eq!(c.size_type(), SizeType::Percentage);
        assert_eq!(c.capture_percentage_index(), 0);
        assert_eq!(c.size_percentage(), (3, 4));
        // 6 = 百分率の末尾(25%)。
        assert!(c.set_preset_capture_size(6));
        assert_eq!(c.capture_percentage_index(), 4);
        assert_eq!(c.size_percentage(), (1, 4));
    }

    #[test]
    fn decode_custom() {
        let mut c = CaptureSize::new();
        // 7 = カスタムの先頭(1920x1080)。
        assert!(c.set_preset_capture_size(7));
        assert_eq!(c.size_type(), SizeType::Custom);
        assert_eq!(c.capture_size_index(), 0);
        assert_eq!(c.custom_size(), (1920, 1080));
        // 21 = カスタムの末尾(320x240)。
        assert!(c.set_preset_capture_size(21));
        assert_eq!(c.capture_size_index(), 14);
        assert_eq!(c.custom_size(), (320, 240));
    }

    #[test]
    fn decode_out_of_range() {
        let mut c = CaptureSize::new();
        assert!(!c.set_preset_capture_size(-1));
        assert!(!c.set_preset_capture_size(22));
    }

    #[test]
    fn encode_roundtrip() {
        for size in 0..=21 {
            let mut c = CaptureSize::new();
            assert!(c.set_preset_capture_size(size), "size {size} を設定できる");
            assert_eq!(c.preset_capture_size(), size, "size {size} で往復一致");
        }
    }

    #[test]
    fn encode_specific_types() {
        let mut c = CaptureSize::new();
        c.set_preset_capture_size(0);
        assert_eq!(c.preset_capture_size(), 0); // Original
        c.set_preset_capture_size(1);
        assert_eq!(c.preset_capture_size(), 1); // View
        c.set_preset_capture_size(4);
        assert_eq!(c.preset_capture_size(), 4); // Percentage idx2(50%)
        c.set_preset_capture_size(10);
        assert_eq!(c.preset_capture_size(), 10); // Custom idx3
    }

    #[test]
    fn tables_have_expected_bounds() {
        assert_eq!(SIZE_LAST, 14);
        assert_eq!(PERCENTAGE_LAST, 4);
        assert_eq!(SIZE_LIST[0], (1920, 1080));
        assert_eq!(SIZE_LIST[7], (320, 180));
        assert_eq!(SIZE_LIST[8], (1440, 1080));
        assert_eq!(PERCENTAGE_LIST[2], (1, 2));
    }

    #[test]
    fn capture_variable_formatting() {
        assert_eq!(format_capture_variable("width", 1920, 1080).as_deref(), Some("1920"));
        assert_eq!(format_capture_variable("Height", 1920, 1080).as_deref(), Some("1080"));
        // 大小無視。
        assert_eq!(format_capture_variable("WIDTH", 640, 360).as_deref(), Some("640"));
        // 未知のキーワードは None(呼び出し側の親マップへ委譲)。
        assert!(format_capture_variable("date", 1920, 1080).is_none());
    }
}
