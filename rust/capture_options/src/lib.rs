//! TVTest のキャプチャ設定(`src/CaptureOptions.cpp` / `CaptureOptions.h`)の純粋ロジックを
//! 移植したクレート。
//!
//! 移植対象:
//! - サイズ種別(`SIZE_TYPE_*`)・プリセットサイズインデックス・パーセンテージインデックスの
//!   定数(`CaptureOptions.h:37-70`)。
//! - プリセットサイズテーブル `m_SizeList`(`CaptureOptions.cpp:113-131`)。
//! - パーセンテーブルテーブル `m_PercentageList`(`CaptureOptions.cpp:134-140`)。
//! - `ReadSettings` のうち `CaptureSizeType` の範囲チェックと旧設定互換の読み替え
//!   (`RAW` → `ORIGINAL`)、`CaptureWidth`/`CaptureHeight` と `m_SizeList` の一致検索、
//!   `CaptureRatioNum`/`CaptureRatioDenom` と `m_PercentageList` の一致検索
//!   (`CaptureOptions.cpp:170-198`)。
//! - `SetPresetCaptureSize`/`GetPresetCaptureSize` の単一整数(コンボボックスのインデックス)と
//!   種別・インデックスの相互変換(`CaptureOptions.cpp:230-266`)。
//!
//! 対象外(Win32 / ファイルシステム / LibISDB 依存):
//! - `CCaptureOptions::DlgProc`(ダイアログ)。
//! - `ReadSettings`/`WriteSettings` の `CSettings` I/O 本体(本クレートは値の変換のみ提供)。
//! - `GenerateFileName`/`GetCommentText`(`VariableString`/ファイルシステム連携)。
//! - `SaveImage`(`CImageCodec`)。
//! - `CCaptureVariableStringMap`(イベント変数文字列マップ)。
//! - `OpenSaveFolder`(`ShellExecute`)。

#![forbid(unsafe_code)]

/// サイズ種別。値は原実装の宣言順(`CaptureOptions.h:37-44`)のまま
/// `SIZE_TYPE_ORIGINAL=0, SIZE_TYPE_VIEW=1, SIZE_TYPE_RAW=2, SIZE_TYPE_CUSTOM=3,
/// SIZE_TYPE_PERCENTAGE=4`。
pub const SIZE_TYPE_ORIGINAL: i32 = 0;
/// 表示されている大きさ。
pub const SIZE_TYPE_VIEW: i32 = 1;
/// 生データの大きさ(旧設定互換用。読み込み時は `SIZE_TYPE_ORIGINAL` に読み替えられる)。
pub const SIZE_TYPE_RAW: i32 = 2;
/// カスタムサイズ。
pub const SIZE_TYPE_CUSTOM: i32 = 3;
/// パーセンテージ指定。
pub const SIZE_TYPE_PERCENTAGE: i32 = 4;
/// サイズ種別の最大値(`CaptureOptions.h:43`)。
pub const SIZE_TYPE_LAST: i32 = SIZE_TYPE_PERCENTAGE;

/// プリセットサイズインデックスの最大値(`CaptureOptions.h:61`、15 種、0 始まり)。
pub const SIZE_LAST: usize = 14;

/// パーセンテージインデックスの最大値(`CaptureOptions.h:69`、5 種、0 始まり)。
pub const PERCENTAGE_LAST: usize = 4;

/// プリセットサイズテーブル(`CaptureOptions.cpp:113-131` の `m_SizeList`)。
/// (幅, 高さ)のペア。16:9 系 8 個 + 4:3 系 7 個の順。
pub const SIZE_LIST: [(u32, u32); SIZE_LAST + 1] = [
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

/// パーセンテージテーブル(`CaptureOptions.cpp:134-140` の `m_PercentageList`)。
/// (分子, 分母)のペア。
pub const PERCENTAGE_LIST: [(u8, u8); PERCENTAGE_LAST + 1] = [
    (3, 4), // 75%
    (2, 3), // 66%
    (1, 2), // 50%
    (1, 3), // 33%
    (1, 4), // 25%
];

/// プリセットキャプチャサイズ。単一整数(コンボボックスのインデックス)を種別ごとに
/// 表現したもの(`SetPresetCaptureSize`/`GetPresetCaptureSize` の往復変換用)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresetCaptureSize {
    /// 元の大きさ(`SIZE_TYPE_ORIGINAL`)。
    Original,
    /// 表示されている大きさ(`SIZE_TYPE_VIEW`)。
    View,
    /// パーセンテージ指定。`PERCENTAGE_LIST` のインデックス。
    Percentage(usize),
    /// カスタムサイズ。`SIZE_LIST` のインデックス。
    Custom(usize),
}

/// `CaptureSizeType` 設定値を正規化する(`ReadSettings`、`CaptureOptions.cpp:170-177` 相当)。
///
/// `0 <= size <= SIZE_TYPE_LAST` の範囲外は `None`。範囲内で `SIZE_TYPE_RAW` は旧設定互換で
/// `SIZE_TYPE_ORIGINAL` に読み替える。それ以外はそのまま返す。
#[must_use]
pub fn normalize_capture_size_type(size: i32) -> Option<i32> {
    if !(0..=SIZE_TYPE_LAST).contains(&size) {
        return None;
    }
    if size == SIZE_TYPE_RAW {
        Some(SIZE_TYPE_ORIGINAL)
    } else {
        Some(size)
    }
}

/// `SIZE_LIST` を線形検索し、`(width, height)` と一致する最初のインデックスを返す
/// (`ReadSettings`、`CaptureOptions.cpp:178-187` 相当)。
#[must_use]
pub fn find_size_index(width: u32, height: u32) -> Option<usize> {
    SIZE_LIST
        .iter()
        .position(|&(cx, cy)| cx == width && cy == height)
}

/// `PERCENTAGE_LIST` を線形検索し、`(num, denom)` と一致する最初のインデックスを返す
/// (`ReadSettings`、`CaptureOptions.cpp:188-198` 相当)。
#[must_use]
pub fn find_percentage_index(num: u8, denom: u8) -> Option<usize> {
    PERCENTAGE_LIST
        .iter()
        .position(|&(n, d)| n == num && d == denom)
}

/// 単一整数(コンボボックスのインデックス)から `PresetCaptureSize` へ変換する
/// (`SetPresetCaptureSize`、`CaptureOptions.cpp:230-246` 相当)。
///
/// `size < 0` は `None`。`size <= SIZE_TYPE_VIEW`(1)ならそのまま `Original`/`View`。
/// 次に `size - 2 <= PERCENTAGE_LAST` ならパーセンテージ指定。
/// 次に `size - (2 + PERCENTAGE_LAST + 1) <= SIZE_LAST` ならカスタムサイズ。
/// それ以外は `None`。
#[must_use]
pub fn set_preset_capture_size(size: i32) -> Option<PresetCaptureSize> {
    if size < 0 {
        return None;
    }
    if size <= SIZE_TYPE_VIEW {
        return Some(if size == SIZE_TYPE_ORIGINAL {
            PresetCaptureSize::Original
        } else {
            PresetCaptureSize::View
        });
    }
    if size - 2 <= PERCENTAGE_LAST as i32 {
        return Some(PresetCaptureSize::Percentage((size - 2) as usize));
    }
    if size - (2 + PERCENTAGE_LAST as i32 + 1) <= SIZE_LAST as i32 {
        return Some(PresetCaptureSize::Custom(
            (size - (2 + PERCENTAGE_LAST as i32 + 1)) as usize,
        ));
    }
    None
}

/// `PresetCaptureSize` から単一整数(コンボボックスのインデックス)へ変換する
/// (`GetPresetCaptureSize`、`CaptureOptions.cpp:249-266` 相当)。
#[must_use]
pub fn get_preset_capture_size(preset: PresetCaptureSize) -> i32 {
    match preset {
        PresetCaptureSize::Original => SIZE_TYPE_ORIGINAL,
        PresetCaptureSize::View => SIZE_TYPE_VIEW,
        PresetCaptureSize::Percentage(index) => 2 + index as i32,
        PresetCaptureSize::Custom(index) => 2 + (PERCENTAGE_LAST as i32 + 1) + index as i32,
    }
}

/// パーセンテージインデックスから `(num, denom)` を取得する
/// (`GetSizePercentage`、`CaptureOptions.cpp:269-276` 相当)。範囲外は `None`。
#[must_use]
pub fn get_size_percentage(index: usize) -> Option<(u8, u8)> {
    PERCENTAGE_LIST.get(index).copied()
}

/// プリセットサイズインデックスから `(width, height)` を取得する
/// (`GetCustomSize`、`CaptureOptions.cpp:279-286` 相当)。範囲外は `None`。
#[must_use]
pub fn get_custom_size(index: usize) -> Option<(u32, u32)> {
    SIZE_LIST.get(index).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_list_contents() {
        assert_eq!(SIZE_LIST.len(), 15);
        assert_eq!(SIZE_LIST[0], (1920, 1080));
        assert_eq!(SIZE_LIST[7], (320, 180));
        assert_eq!(SIZE_LIST[8], (1440, 1080));
        assert_eq!(SIZE_LIST[14], (320, 240));
    }

    #[test]
    fn percentage_list_contents() {
        assert_eq!(PERCENTAGE_LIST.len(), 5);
        assert_eq!(PERCENTAGE_LIST[0], (3, 4));
        assert_eq!(PERCENTAGE_LIST[1], (2, 3));
        assert_eq!(PERCENTAGE_LIST[2], (1, 2));
        assert_eq!(PERCENTAGE_LIST[3], (1, 3));
        assert_eq!(PERCENTAGE_LIST[4], (1, 4));
    }

    #[test]
    fn normalize_capture_size_type_original() {
        assert_eq!(normalize_capture_size_type(0), Some(SIZE_TYPE_ORIGINAL));
    }

    #[test]
    fn normalize_capture_size_type_view() {
        assert_eq!(normalize_capture_size_type(1), Some(SIZE_TYPE_VIEW));
    }

    #[test]
    fn normalize_capture_size_type_raw_becomes_original() {
        // 旧設定互換: RAW(2) は ORIGINAL(0) に読み替え。
        assert_eq!(normalize_capture_size_type(2), Some(SIZE_TYPE_ORIGINAL));
    }

    #[test]
    fn normalize_capture_size_type_custom() {
        assert_eq!(normalize_capture_size_type(3), Some(SIZE_TYPE_CUSTOM));
    }

    #[test]
    fn normalize_capture_size_type_percentage() {
        assert_eq!(normalize_capture_size_type(4), Some(SIZE_TYPE_PERCENTAGE));
    }

    #[test]
    fn normalize_capture_size_type_out_of_range() {
        assert_eq!(normalize_capture_size_type(-1), None);
        assert_eq!(normalize_capture_size_type(5), None);
    }

    #[test]
    fn find_size_index_match() {
        assert_eq!(find_size_index(1920, 1080), Some(0));
        assert_eq!(find_size_index(320, 240), Some(14));
        assert_eq!(find_size_index(1440, 1080), Some(8));
    }

    #[test]
    fn find_size_index_no_match() {
        assert_eq!(find_size_index(100, 100), None);
        // 幅は一致するが高さが違う場合は不一致。
        assert_eq!(find_size_index(1920, 1081), None);
    }

    #[test]
    fn find_percentage_index_match() {
        assert_eq!(find_percentage_index(3, 4), Some(0));
        assert_eq!(find_percentage_index(1, 4), Some(4));
        assert_eq!(find_percentage_index(1, 2), Some(2));
    }

    #[test]
    fn find_percentage_index_no_match() {
        assert_eq!(find_percentage_index(1, 5), None);
        assert_eq!(find_percentage_index(9, 9), None);
    }

    #[test]
    fn set_preset_capture_size_negative_is_none() {
        assert_eq!(set_preset_capture_size(-1), None);
    }

    #[test]
    fn set_preset_capture_size_original_and_view() {
        assert_eq!(set_preset_capture_size(0), Some(PresetCaptureSize::Original));
        assert_eq!(set_preset_capture_size(1), Some(PresetCaptureSize::View));
    }

    #[test]
    fn set_preset_capture_size_percentage_range() {
        assert_eq!(
            set_preset_capture_size(2),
            Some(PresetCaptureSize::Percentage(0))
        );
        assert_eq!(
            set_preset_capture_size(6),
            Some(PresetCaptureSize::Percentage(4))
        );
    }

    #[test]
    fn set_preset_capture_size_custom_range() {
        assert_eq!(set_preset_capture_size(7), Some(PresetCaptureSize::Custom(0)));
        assert_eq!(
            set_preset_capture_size(21),
            Some(PresetCaptureSize::Custom(14))
        );
    }

    #[test]
    fn set_preset_capture_size_too_large_is_none() {
        assert_eq!(set_preset_capture_size(22), None);
    }

    #[test]
    fn preset_capture_size_round_trip_original_view() {
        for size in 0..=1 {
            let preset = set_preset_capture_size(size).unwrap();
            assert_eq!(get_preset_capture_size(preset), size);
        }
    }

    #[test]
    fn preset_capture_size_round_trip_percentage() {
        for index in 0..=PERCENTAGE_LAST {
            let preset = PresetCaptureSize::Percentage(index);
            let size = get_preset_capture_size(preset);
            assert_eq!(set_preset_capture_size(size), Some(preset));
        }
    }

    #[test]
    fn preset_capture_size_round_trip_custom() {
        for index in 0..=SIZE_LAST {
            let preset = PresetCaptureSize::Custom(index);
            let size = get_preset_capture_size(preset);
            assert_eq!(set_preset_capture_size(size), Some(preset));
        }
    }

    #[test]
    fn get_size_percentage_basic() {
        assert_eq!(get_size_percentage(0), Some((3, 4)));
        assert_eq!(get_size_percentage(4), Some((1, 4)));
        assert_eq!(get_size_percentage(5), None);
    }

    #[test]
    fn get_custom_size_basic() {
        assert_eq!(get_custom_size(0), Some((1920, 1080)));
        assert_eq!(get_custom_size(14), Some((320, 240)));
        assert_eq!(get_custom_size(15), None);
    }
}
