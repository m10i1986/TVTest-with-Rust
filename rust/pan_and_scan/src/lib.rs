//! TVTest のパン&スキャン設定(src/PanAndScanOptions.cpp / PanAndScanOptions.h)の純粋部分の移植。
//!
//! パン&スキャン情報(切り出し位置/サイズの百分率とアスペクト比)を、設定入出力や
//! ダイアログから切り離した純粋なモデルとして表現する:
//! - [`PanAndScanInfo`](表示位置/サイズ/係数/アスペクト比)
//! - [`PanAndScanPreset`](情報 + 名前 + ID)
//! - [`format_pan_and_scan_info`] / [`parse_pan_and_scan_info`](CSV ⇄ 構造体、検証付き)
//! - [`format_value`] / [`get_value`](百分率値のフォーマット/パース)
//! - [`PanAndScanPresetList`](既定プリセットと ID 検索)
//!
//! # 対象外(Win32 / I-O 依存)
//! 設定入出力(`ReadSettings`/`WriteSettings`)、ダイアログ(`DlgProc`)、インポート/
//! エクスポート、`CCoreEngine`/`CCommandManager` 連携。

/// 百分率の係数(PanAndScanOptions.cpp 34 `FACTOR_PERCENTAGE`)。
pub const FACTOR_PERCENTAGE: i32 = 100;
/// 水平方向の係数(PanAndScanOptions.cpp 35 `HORZ_FACTOR`)。
pub const HORZ_FACTOR: i32 = FACTOR_PERCENTAGE * 100;
/// 垂直方向の係数(PanAndScanOptions.cpp 36 `VERT_FACTOR`)。
pub const VERT_FACTOR: i32 = FACTOR_PERCENTAGE * 100;
/// プリセット名の最大長(PanAndScanOptions.h 41 `MAX_NAME`)。
pub const MAX_NAME: usize = 64;

/// パン&スキャン情報(CoreEngine.h 64-72 `CCoreEngine::PanAndScanInfo`)。
///
/// `x_pos`/`y_pos`/`width`/`height` は `x_factor`/`y_factor` を 100% とした座標・寸法。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PanAndScanInfo {
    pub x_pos: i32,
    pub y_pos: i32,
    pub width: i32,
    pub height: i32,
    pub x_factor: i32,
    pub y_factor: i32,
    pub x_aspect: i32,
    pub y_aspect: i32,
}

/// プリセット 1 件(PanAndScanOptions.h 43-48 `CPanAndScanOptions::PanAndScanInfo`)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanAndScanPreset {
    pub info: PanAndScanInfo,
    pub name: String,
    pub id: u32,
}

/// 値を百分率文字列にフォーマットする(PanAndScanOptions.cpp 41-53 `FormatValue`)。
///
/// `value * 100 / factor` を百分率(整数 2 桁の小数)とみなして文字列化する。
/// 端数が無ければ整数のみ、あれば小数 2 桁。
pub fn format_value(value: i32, factor: i32) -> String {
    let percentage = if factor != 0 { (value * 100) / factor } else { 0 };
    if percentage % 100 == 0 {
        format!("{}", percentage / 100)
    } else {
        format!("{}.{:02}", percentage / 100, percentage.abs() % 100)
    }
}

/// 百分率文字列を値にパースする(PanAndScanOptions.cpp 56-76 `GetValue`)。
///
/// 整数部に `factor` を掛け、小数部は桁ごとに `factor` 未満まで取り込む。
pub fn get_value(text: &str, factor: i32) -> i32 {
    let bytes = text.as_bytes();
    let mut value: i32 = 0;
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        value = value * 10 + (bytes[i] - b'0') as i32;
        i += 1;
    }
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        let mut decimal = 1;
        while decimal < factor && i < bytes.len() && bytes[i].is_ascii_digit() {
            value = value * 10 + (bytes[i] - b'0') as i32;
            decimal *= 10;
            i += 1;
        }
        value = value * factor / decimal;
    } else {
        value *= factor;
    }
    value
}

/// パン&スキャン情報を CSV 文字列にする(PanAndScanOptions.cpp 79-90 `FormatPanAndScanInfo`)。
///
/// 形式は `XPos,YPos,Width,Height,XAspect,YAspect`(位置/サイズは百分率)。
pub fn format_pan_and_scan_info(info: &PanAndScanInfo) -> String {
    format!(
        "{},{},{},{},{},{}",
        format_value(info.x_pos, FACTOR_PERCENTAGE),
        format_value(info.y_pos, FACTOR_PERCENTAGE),
        format_value(info.width, FACTOR_PERCENTAGE),
        format_value(info.height, FACTOR_PERCENTAGE),
        info.x_aspect,
        info.y_aspect
    )
}

/// CSV 文字列をパン&スキャン情報にパースする(PanAndScanOptions.cpp 93-131 `ParsePanAndScanInfo`)。
///
/// 6 フィールド必要。空フィールドは未設定(0 のまま)。`x_factor`/`y_factor` は
/// `HORZ_FACTOR`/`VERT_FACTOR` を設定する。位置・サイズ・アスペクト比の妥当性検証に
/// 失敗したら `None`。
pub fn parse_pan_and_scan_info(text: &str) -> Option<PanAndScanInfo> {
    let fields: Vec<&str> = text.split(',').collect();
    // 原実装は 6 フィールドを処理するまで進み、足りなければ false。
    if fields.len() < 6 {
        return None;
    }

    let mut info = PanAndScanInfo::default();
    for (j, field) in fields.iter().take(6).enumerate() {
        // 各フィールドの先頭の空白を飛ばす。
        let value = field.trim_start_matches(' ');
        if value.is_empty() {
            continue;
        }
        match j {
            0 => info.x_pos = get_value(value, FACTOR_PERCENTAGE),
            1 => info.y_pos = get_value(value, FACTOR_PERCENTAGE),
            2 => info.width = get_value(value, FACTOR_PERCENTAGE),
            3 => info.height = get_value(value, FACTOR_PERCENTAGE),
            4 => info.x_aspect = get_value(value, 1),
            5 => info.y_aspect = get_value(value, 1),
            _ => unreachable!(),
        }
    }

    info.x_factor = HORZ_FACTOR;
    info.y_factor = VERT_FACTOR;

    if info.x_pos < 0
        || info.y_pos < 0
        || info.width < 1
        || info.height < 1
        || info.x_pos + info.width > info.x_factor
        || info.y_pos + info.height > info.y_factor
        || info.x_aspect < 1
        || info.y_aspect < 1
    {
        return None;
    }

    Some(info)
}

/// 既定のプリセット一覧(PanAndScanOptions.cpp 135-140 `DefaultPresetList`)。
pub fn default_presets() -> Vec<PanAndScanPreset> {
    // 位置/サイズ/アスペクト比からパン&スキャン情報を作る(係数は固定)。
    fn info(x_pos: i32, y_pos: i32, width: i32, height: i32, x_aspect: i32, y_aspect: i32) -> PanAndScanInfo {
        PanAndScanInfo {
            x_pos,
            y_pos,
            width,
            height,
            x_factor: HORZ_FACTOR,
            y_factor: VERT_FACTOR,
            x_aspect,
            y_aspect,
        }
    }
    vec![
        PanAndScanPreset {
            info: info(0, 1281, 10000, 7438, 239, 100),
            name: "2.39:1 シネスコ".to_string(),
            id: 1,
        },
        PanAndScanPreset {
            info: info(0, 1218, 10000, 7565, 235, 100),
            name: "2.35:1 シネスコ".to_string(),
            id: 2,
        },
        PanAndScanPreset {
            info: info(0, 195, 10000, 9610, 185, 100),
            name: "1.85:1 ビスタ(米)".to_string(),
            id: 3,
        },
        PanAndScanPreset {
            info: info(331, 0, 9338, 10000, 166, 100),
            name: "1.66:1 ビスタ(欧)".to_string(),
            id: 4,
        },
    ]
}

/// プリセットリスト(CPanAndScanOptions の `m_PresetList`/`m_PresetID` のモデル)。
#[derive(Clone, Debug)]
pub struct PanAndScanPresetList {
    presets: Vec<PanAndScanPreset>,
    preset_id: u32,
}

impl Default for PanAndScanPresetList {
    fn default() -> Self {
        let presets = default_presets();
        // m_PresetID = 既定プリセット数 + 1(PanAndScanOptions.cpp 146)。
        let preset_id = presets.len() as u32 + 1;
        Self { presets, preset_id }
    }
}

impl PanAndScanPresetList {
    /// 既定プリセットを読み込んだリストを生成する。
    pub fn new() -> Self {
        Self::default()
    }

    /// プリセット数を返す(PanAndScanOptions.cpp 209-212 `GetPresetCount`)。
    pub fn preset_count(&self) -> usize {
        self.presets.len()
    }

    /// 次に割り当てるプリセット ID を返す。
    pub fn next_preset_id(&self) -> u32 {
        self.preset_id
    }

    /// 索引でプリセットを返す(PanAndScanOptions.cpp 215-223 `GetPreset`)。
    pub fn get_preset(&self, index: usize) -> Option<&PanAndScanPreset> {
        self.presets.get(index)
    }

    /// ID でプリセットを返す(PanAndScanOptions.cpp 226-235 `GetPresetByID`)。
    pub fn get_preset_by_id(&self, id: u32) -> Option<&PanAndScanPreset> {
        let index = self.find_preset_by_id(id)?;
        self.presets.get(index)
    }

    /// 索引のプリセット ID を返す(PanAndScanOptions.cpp 238-243 `GetPresetID`。範囲外は 0)。
    pub fn get_preset_id(&self, index: usize) -> u32 {
        self.presets.get(index).map_or(0, |p| p.id)
    }

    /// ID から索引を探す(PanAndScanOptions.cpp 246-253 `FindPresetByID`。無ければ `None`)。
    pub fn find_preset_by_id(&self, id: u32) -> Option<usize> {
        self.presets.iter().position(|p| p.id == id)
    }

    /// 全プリセットを返す。
    pub fn presets(&self) -> &[PanAndScanPreset] {
        &self.presets
    }

    /// プリセットを消去する(設定読み込みで上書きする前段)。
    pub fn clear(&mut self) {
        self.presets.clear();
    }

    /// プリセットを追加する。
    pub fn add_preset(&mut self, preset: PanAndScanPreset) {
        self.presets.push(preset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_value_whole_and_fraction() {
        assert_eq!(format_value(10000, FACTOR_PERCENTAGE), "100");
        assert_eq!(format_value(0, FACTOR_PERCENTAGE), "0");
        assert_eq!(format_value(1281, FACTOR_PERCENTAGE), "12.81");
        assert_eq!(format_value(7438, FACTOR_PERCENTAGE), "74.38");
        // 端数が 1 桁でもゼロ詰め 2 桁。
        assert_eq!(format_value(105, FACTOR_PERCENTAGE), "1.05");
    }

    #[test]
    fn get_value_integer_and_decimal() {
        assert_eq!(get_value("100", FACTOR_PERCENTAGE), 10000);
        assert_eq!(get_value("50", FACTOR_PERCENTAGE), 5000);
        assert_eq!(get_value("12.81", FACTOR_PERCENTAGE), 1281);
        assert_eq!(get_value("74.38", FACTOR_PERCENTAGE), 7438);
        // factor=1 は整数そのまま。
        assert_eq!(get_value("239", 1), 239);
    }

    #[test]
    fn format_parse_round_trip_on_presets() {
        for preset in default_presets() {
            let text = format_pan_and_scan_info(&preset.info);
            let parsed = parse_pan_and_scan_info(&text).expect("既定プリセットは妥当");
            assert_eq!(parsed, preset.info, "プリセット {} で往復一致", preset.name);
        }
    }

    #[test]
    fn parse_sets_factors() {
        let info = parse_pan_and_scan_info("0,0,100,100,16,9").unwrap();
        assert_eq!(info.x_factor, HORZ_FACTOR);
        assert_eq!(info.y_factor, VERT_FACTOR);
        assert_eq!(info.width, 10000);
        assert_eq!(info.x_aspect, 16);
        assert_eq!(info.y_aspect, 9);
    }

    #[test]
    fn parse_rejects_too_few_fields() {
        assert!(parse_pan_and_scan_info("0,0,100,100,16").is_none());
        assert!(parse_pan_and_scan_info("").is_none());
    }

    #[test]
    fn parse_rejects_out_of_range() {
        // XPos + Width が係数(100%)を超える。
        assert!(parse_pan_and_scan_info("50,0,80,100,16,9").is_none());
        // Width < 1。
        assert!(parse_pan_and_scan_info("0,0,0,100,16,9").is_none());
        // アスペクト比 0。
        assert!(parse_pan_and_scan_info("0,0,100,100,0,9").is_none());
    }

    #[test]
    fn parse_skips_leading_spaces() {
        let info = parse_pan_and_scan_info("0, 0, 100, 100, 16, 9").unwrap();
        assert_eq!(info.width, 10000);
        assert_eq!(info.x_aspect, 16);
    }

    #[test]
    fn parse_ignores_extra_fields() {
        // 6 フィールドより多くても先頭 6 つで判定する。
        let info = parse_pan_and_scan_info("0,0,100,100,16,9,extra").unwrap();
        assert_eq!(info.y_aspect, 9);
    }

    #[test]
    fn preset_list_defaults() {
        let list = PanAndScanPresetList::new();
        assert_eq!(list.preset_count(), 4);
        assert_eq!(list.next_preset_id(), 5);
        assert_eq!(list.get_preset(0).unwrap().name, "2.39:1 シネスコ");
        assert_eq!(list.get_preset_id(3), 4);
        assert_eq!(list.get_preset_id(99), 0);
    }

    #[test]
    fn preset_list_find_by_id() {
        let list = PanAndScanPresetList::new();
        assert_eq!(list.find_preset_by_id(3), Some(2));
        assert_eq!(list.find_preset_by_id(99), None);
        assert_eq!(list.get_preset_by_id(2).unwrap().name, "2.35:1 シネスコ");
        assert!(list.get_preset_by_id(99).is_none());
    }

    #[test]
    fn preset_list_clear_and_add() {
        let mut list = PanAndScanPresetList::new();
        list.clear();
        assert_eq!(list.preset_count(), 0);
        list.add_preset(PanAndScanPreset {
            info: PanAndScanInfo::default(),
            name: "テスト".to_string(),
            id: 100,
        });
        assert_eq!(list.preset_count(), 1);
        assert_eq!(list.find_preset_by_id(100), Some(0));
    }
}
