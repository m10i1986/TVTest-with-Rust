//! TVTest のズーム設定(`src/ZoomOptions.cpp` / `ZoomOptions.h`)の純粋ロジックを移植したクレート。
//!
//! 移植対象は `CZoomOptions` のうちプラットフォーム非依存な部分:
//! - ズーム率(分数→百分率)/ズームサイズのプリセットモデル(`ZoomInfo` と既定テーブル)
//! - 表示順序(`m_Order`)の設定(`ReadSettings`)・直列化(`WriteSettings`)
//! - コマンド ID ↔ インデックス解決(`GetIndexByCommand` / `GetZoomInfoByCommand`)
//! - メニュー項目テキストとチェック状態の算出(`SetMenu` の純粋部分)
//! - コマンドテキストのカスタムサフィックス(`FormatCommandText` の純粋部分)
//! - カスタムズームの値検証(`ReadSettings` のカスタム読み込み)
//!
//! ダイアログ(`DlgProc`)、メニュー生成(`InsertMenu`)、設定入出力(`CSettings`)、
//! `LoadString`、`CCommandManager::ParseIDText`(コマンドテキスト→ID 変換)は Win32/I-O
//! 依存のため対象外。コマンドテキストの解決は呼び出し側の責務とし、本クレートは解決済みの
//! コマンド ID を受け取る。

// ズームコマンド ID(resource.h 155-167, 502-503)。
/// 標準ズームコマンドの先頭 ID(`CM_ZOOM_20`)。
pub const CM_ZOOM_FIRST: i32 = 100;
/// 標準ズームコマンドの末尾 ID(`CM_ZOOM_300`)。
pub const CM_ZOOM_LAST: i32 = 110;
/// カスタムズームコマンドの先頭 ID。
pub const CM_CUSTOMZOOM_FIRST: i32 = 19000;
/// カスタムズームコマンドの末尾 ID。
pub const CM_CUSTOMZOOM_LAST: i32 = 19009;

/// ズームコマンドの総数(標準 11 + カスタム 10、ZoomOptions.h:68)。
pub const NUM_ZOOM_COMMANDS: usize = 21;
/// 標準ズームコマンドの数(`CM_ZOOM_20`〜`CM_ZOOM_300`)。
pub const NUM_STANDARD_ZOOM_COMMANDS: usize = 11;
/// カスタムズームコマンドの数。
pub const NUM_CUSTOM_ZOOM_COMMANDS: usize = 10;
/// カスタムズーム率の上限(ZoomOptions.h:69)。
pub const MAX_RATE: i32 = 1000;

// 既定サイズの基準(ZoomOptions.cpp:45-46、非ワンセグ版)。
const BASE_WIDTH: i32 = 1920;
const BASE_HEIGHT: i32 = 1080;

/// Win32 `MulDiv` 相当(64bit 中間・四捨五入は端数をゼロから遠い側へ・0除算/桁あふれで -1)。
///
/// `ZoomRate::get_percentage` が `::MulDiv(Rate, 100, Factor)` を使う(ZoomOptions.h:51)ため、
/// プラットフォーム非依存を保つべく忠実に再実装する。
pub fn mul_div(number: i32, numerator: i32, denominator: i32) -> i32 {
    if denominator == 0 {
        return -1;
    }
    let product = number as i64 * numerator as i64;
    let denom = denominator as i64;
    // 商の符号に合わせて分母の半分を加減し、ゼロ方向への切り捨て除算で「ゼロから遠い側」へ丸める。
    let half = denom.abs() / 2;
    let rounded = if (product < 0) ^ (denominator < 0) {
        product - half
    } else {
        product + half
    };
    let result = rounded / denom;
    if result < i32::MIN as i64 || result > i32::MAX as i64 {
        return -1;
    }
    result as i32
}

/// ズームの指定方式(ZoomOptions.h:40-44 の `ZoomType`)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ZoomType {
    /// 倍率(分数)指定。
    #[default]
    Rate,
    /// 画素サイズ指定。
    Size,
}

impl ZoomType {
    /// 設定値(整数)からの復元。`CheckEnumRange` 相当で範囲外は `None`。
    pub fn from_int(value: i32) -> Option<ZoomType> {
        match value {
            0 => Some(ZoomType::Rate),
            1 => Some(ZoomType::Size),
            _ => None,
        }
    }

    /// 設定値(整数)への変換。
    pub fn to_int(self) -> i32 {
        self as i32
    }
}

/// ズーム率(ZoomOptions.h:46-52 の `ZoomRate`)。`rate`/`factor` の分数で倍率を表す。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZoomRate {
    /// 分子。
    pub rate: i32,
    /// 分母。
    pub factor: i32,
}

impl ZoomRate {
    /// 百分率(`Factor != 0` なら `MulDiv(Rate, 100, Factor)`、それ以外は 0、ZoomOptions.h:51)。
    pub fn get_percentage(&self) -> i32 {
        if self.factor != 0 {
            mul_div(self.rate, 100, self.factor)
        } else {
            0
        }
    }
}

/// ズームサイズ(ZoomOptions.h:54-58 の `ZoomSize`)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZoomSize {
    /// 幅(画素)。
    pub width: i32,
    /// 高さ(画素)。
    pub height: i32,
}

/// ズーム設定 1 項目(ZoomOptions.h:60-66 の `ZoomInfo`)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZoomInfo {
    /// 指定方式。
    pub zoom_type: ZoomType,
    /// 倍率指定値。
    pub rate: ZoomRate,
    /// サイズ指定値。
    pub size: ZoomSize,
    /// メニュー等に表示するか。
    pub visible: bool,
}

/// 既定ズームリストの 1 エントリ(ZoomOptions.h:85-89 の `ZoomCommandInfo`)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZoomCommandInfo {
    /// コマンド ID。
    pub command: i32,
    /// 既定のズーム設定。
    pub info: ZoomInfo,
}

const fn zc(
    command: i32,
    rate: i32,
    factor: i32,
    width: i32,
    height: i32,
    visible: bool,
) -> ZoomCommandInfo {
    ZoomCommandInfo {
        command,
        info: ZoomInfo {
            zoom_type: ZoomType::Rate,
            rate: ZoomRate { rate, factor },
            size: ZoomSize { width, height },
            visible,
        },
    }
}

/// 既定ズームリスト(ZoomOptions.cpp:52-74、非ワンセグ版)。
///
/// `m_DefaultZoomList` 相当。`visible` は元の `!f1Seg`/`true`/`f1Seg` を非ワンセグ(`f1Seg=false`)で
/// 評価した結果(20〜200% は表示・250/300% とカスタムは非表示)。配列の添字がそのまま
/// `ZoomOptions` の `zoom_list`/`order` のインデックスになる。
pub static DEFAULT_ZOOM_LIST: [ZoomCommandInfo; NUM_ZOOM_COMMANDS] = [
    zc(CM_ZOOM_FIRST, 1, 5, BASE_WIDTH / 5, BASE_HEIGHT / 5, true), // CM_ZOOM_20
    zc(CM_ZOOM_FIRST + 1, 1, 4, BASE_WIDTH / 4, BASE_HEIGHT / 4, true), // CM_ZOOM_25
    zc(CM_ZOOM_FIRST + 2, 1, 3, BASE_WIDTH / 3, BASE_HEIGHT / 3, true), // CM_ZOOM_33
    zc(CM_ZOOM_FIRST + 3, 1, 2, BASE_WIDTH / 2, BASE_HEIGHT / 2, true), // CM_ZOOM_50
    zc(CM_ZOOM_FIRST + 4, 2, 3, BASE_WIDTH * 2 / 3, BASE_HEIGHT * 2 / 3, true), // CM_ZOOM_66
    zc(CM_ZOOM_FIRST + 5, 3, 4, BASE_WIDTH * 3 / 4, BASE_HEIGHT * 3 / 4, true), // CM_ZOOM_75
    zc(CM_ZOOM_FIRST + 6, 1, 1, BASE_WIDTH, BASE_HEIGHT, true),     // CM_ZOOM_100
    zc(CM_ZOOM_FIRST + 7, 3, 2, BASE_WIDTH * 3 / 2, BASE_HEIGHT * 3 / 2, true), // CM_ZOOM_150
    zc(CM_ZOOM_FIRST + 8, 2, 1, BASE_WIDTH * 2, BASE_HEIGHT * 2, true), // CM_ZOOM_200
    zc(CM_ZOOM_FIRST + 9, 5, 2, BASE_WIDTH * 5 / 2, BASE_HEIGHT * 5 / 2, false), // CM_ZOOM_250
    zc(CM_ZOOM_FIRST + 10, 3, 1, BASE_WIDTH * 3, BASE_HEIGHT * 3, false), // CM_ZOOM_300
    zc(CM_CUSTOMZOOM_FIRST, 100, 100, BASE_WIDTH, BASE_HEIGHT, false),
    zc(CM_CUSTOMZOOM_FIRST + 1, 100, 100, BASE_WIDTH, BASE_HEIGHT, false),
    zc(CM_CUSTOMZOOM_FIRST + 2, 100, 100, BASE_WIDTH, BASE_HEIGHT, false),
    zc(CM_CUSTOMZOOM_FIRST + 3, 100, 100, BASE_WIDTH, BASE_HEIGHT, false),
    zc(CM_CUSTOMZOOM_FIRST + 4, 100, 100, BASE_WIDTH, BASE_HEIGHT, false),
    zc(CM_CUSTOMZOOM_FIRST + 5, 100, 100, BASE_WIDTH, BASE_HEIGHT, false),
    zc(CM_CUSTOMZOOM_FIRST + 6, 100, 100, BASE_WIDTH, BASE_HEIGHT, false),
    zc(CM_CUSTOMZOOM_FIRST + 7, 100, 100, BASE_WIDTH, BASE_HEIGHT, false),
    zc(CM_CUSTOMZOOM_FIRST + 8, 100, 100, BASE_WIDTH, BASE_HEIGHT, false),
    zc(CM_CUSTOMZOOM_FIRST + 9, 100, 100, BASE_WIDTH, BASE_HEIGHT, false),
];

/// コマンド ID がカスタムズーム(`CM_CUSTOMZOOM_FIRST`〜`CM_CUSTOMZOOM_LAST`)か。
pub fn is_custom_command(command: i32) -> bool {
    (CM_CUSTOMZOOM_FIRST..=CM_CUSTOMZOOM_LAST).contains(&command)
}

/// `SetMenu` が生成する 1 メニュー項目(ZoomOptions.cpp:203-249 の純粋出力)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZoomMenuItem {
    /// 項目に割り当てるコマンド ID(`m_DefaultZoomList[...].Command`)。
    pub command: i32,
    /// 表示テキスト。
    pub text: String,
    /// チェック(現在のズームと一致)状態。
    pub checked: bool,
}

/// メニューに表示する 1 項目のテキスト(ZoomOptions.cpp:217-242)。
///
/// `Rate` 種別は `"{}%"`(端数があれば `" ({}/{})"` を付加)、`Size` 種別は `"{} x {}"`。
pub fn format_menu_text(info: &ZoomInfo) -> String {
    match info.zoom_type {
        ZoomType::Rate => {
            let mut text = format!("{}%", info.rate.get_percentage());
            if info.rate.factor != 0 && info.rate.rate * 100 % info.rate.factor != 0 {
                text.push_str(&format!(" ({}/{})", info.rate.rate, info.rate.factor));
            }
            text
        }
        ZoomType::Size => format!("{} x {}", info.size.width, info.size.height),
    }
}

/// CZoomOptions のモデル層。`m_ZoomList`(各コマンドのズーム設定)と `m_Order`(表示順)を保持する。
#[derive(Clone, Debug)]
pub struct ZoomOptions {
    zoom_list: [ZoomInfo; NUM_ZOOM_COMMANDS],
    order: [usize; NUM_ZOOM_COMMANDS],
}

impl Default for ZoomOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl ZoomOptions {
    /// 既定リストで初期化(ZoomOptions.cpp:77-84 のコンストラクタ。`order` は恒等順)。
    pub fn new() -> Self {
        let mut zoom_list = [DEFAULT_ZOOM_LIST[0].info; NUM_ZOOM_COMMANDS];
        let mut order = [0usize; NUM_ZOOM_COMMANDS];
        for i in 0..NUM_ZOOM_COMMANDS {
            zoom_list[i] = DEFAULT_ZOOM_LIST[i].info;
            order[i] = i;
        }
        Self { zoom_list, order }
    }

    /// インデックスに対応する既定コマンド ID(`m_DefaultZoomList[index].Command`)。
    pub fn default_command(&self, index: usize) -> i32 {
        DEFAULT_ZOOM_LIST[index].command
    }

    /// コマンド ID からインデックスを解決(ZoomOptions.cpp:263-270 の `GetIndexByCommand`)。
    pub fn get_index_by_command(&self, command: i32) -> Option<usize> {
        DEFAULT_ZOOM_LIST
            .iter()
            .position(|e| e.command == command)
    }

    /// 全ズーム設定(インデックス順)。
    pub fn zoom_list(&self) -> &[ZoomInfo] {
        &self.zoom_list
    }

    /// 現在の表示順(各要素は `zoom_list` のインデックス)。
    pub fn order(&self) -> &[usize] {
        &self.order
    }

    /// インデックス指定のズーム設定取得。
    pub fn get_zoom_info(&self, index: usize) -> Option<&ZoomInfo> {
        self.zoom_list.get(index)
    }

    /// コマンド ID 指定のズーム設定取得(ZoomOptions.cpp:252-260 の `GetZoomInfoByCommand`)。
    pub fn get_zoom_info_by_command(&self, command: i32) -> Option<&ZoomInfo> {
        let index = self.get_index_by_command(command)?;
        Some(&self.zoom_list[index])
    }

    /// カスタムインデックス(0〜9)→ `zoom_list` のインデックスへ変換。
    pub fn custom_list_index(custom_index: usize) -> Option<usize> {
        if custom_index < NUM_CUSTOM_ZOOM_COMMANDS {
            Some(NUM_STANDARD_ZOOM_COMMANDS + custom_index)
        } else {
            None
        }
    }

    /// カスタムズームの倍率を設定(ZoomOptions.cpp:104-105。`0 < rate <= MAX_RATE` のみ反映)。
    /// 反映したら `true`。
    pub fn set_custom_rate(&mut self, custom_index: usize, rate: i32) -> bool {
        match Self::custom_list_index(custom_index) {
            Some(index) if rate > 0 && rate <= MAX_RATE => {
                self.zoom_list[index].rate.rate = rate;
                true
            }
            _ => false,
        }
    }

    /// カスタムズームの指定方式を設定(ZoomOptions.cpp:107-108)。
    pub fn set_custom_type(&mut self, custom_index: usize, zoom_type: ZoomType) -> bool {
        match Self::custom_list_index(custom_index) {
            Some(index) => {
                self.zoom_list[index].zoom_type = zoom_type;
                true
            }
            None => false,
        }
    }

    /// カスタムズームの幅を設定(ZoomOptions.cpp:110-111。`width > 0` のみ反映)。
    pub fn set_custom_width(&mut self, custom_index: usize, width: i32) -> bool {
        match Self::custom_list_index(custom_index) {
            Some(index) if width > 0 => {
                self.zoom_list[index].size.width = width;
                true
            }
            _ => false,
        }
    }

    /// カスタムズームの高さを設定(ZoomOptions.cpp:113-114。`height > 0` のみ反映)。
    pub fn set_custom_height(&mut self, custom_index: usize, height: i32) -> bool {
        match Self::custom_list_index(custom_index) {
            Some(index) if height > 0 => {
                self.zoom_list[index].size.height = height;
                true
            }
            _ => false,
        }
    }

    /// カスタムズームの設定をまるごと差し替え(ZoomOptions.cpp:490 の `m_ZoomList[Index] = m_ZoomSettingList[Index]`)。
    pub fn set_custom_zoom_info(&mut self, custom_index: usize, info: ZoomInfo) -> bool {
        match Self::custom_list_index(custom_index) {
            Some(index) => {
                self.zoom_list[index] = info;
                true
            }
            None => false,
        }
    }

    /// インデックス指定で表示フラグを設定。
    pub fn set_visible_by_index(&mut self, index: usize, visible: bool) -> bool {
        match self.zoom_list.get_mut(index) {
            Some(info) => {
                info.visible = visible;
                true
            }
            None => false,
        }
    }

    /// コマンド ID 指定で表示フラグを設定。
    pub fn set_visible_by_command(&mut self, command: i32, visible: bool) -> bool {
        match self.get_index_by_command(command) {
            Some(index) => {
                self.zoom_list[index].visible = visible;
                true
            }
            None => false,
        }
    }

    /// 表示順を設定(ZoomOptions.cpp:487 の `m_Order[i] = Index`)。
    ///
    /// `order` は `0..NUM_ZOOM_COMMANDS` の順列(全インデックスを重複なく 1 回ずつ)である必要があり、
    /// それ以外は何もせず `false` を返す。
    pub fn set_order(&mut self, order: &[usize]) -> bool {
        if order.len() != NUM_ZOOM_COMMANDS {
            return false;
        }
        let mut seen = [false; NUM_ZOOM_COMMANDS];
        for &idx in order {
            if idx >= NUM_ZOOM_COMMANDS || seen[idx] {
                return false;
            }
            seen[idx] = true;
        }
        for (i, &idx) in order.iter().enumerate() {
            self.order[i] = idx;
        }
        true
    }

    /// 設定から読み込んだ順序リストを適用(ZoomOptions.cpp:117-162 の `ReadSettings` 順序構築)。
    ///
    /// `entries` は解決済みの `(コマンド ID, 表示フラグ)` の並び(設定の `ZoomList{n}` を
    /// `ParseIDText` で解決した結果)。先頭から順に、未知でなく(コマンド ID != 0)既知コマンドで、
    /// まだ順序に積まれていないものを順序へ積み、その表示フラグを反映する。残りのインデックスは
    /// 元の並び順で末尾に補充する。`entries` が空なら(`ZoomListCount > 0` 不成立に相当)何もしない。
    pub fn apply_zoom_list_order(&mut self, entries: &[(i32, bool)]) {
        if entries.is_empty() {
            return;
        }
        let mut count = 0usize;
        for &(command, visible) in entries.iter().take(NUM_ZOOM_COMMANDS) {
            if command == 0 {
                continue;
            }
            if let Some(j) = self.get_index_by_command(command) {
                if !self.order[..count].contains(&j) {
                    self.order[count] = j;
                    self.zoom_list[j].visible = visible;
                    count += 1;
                }
            }
        }
        if count < NUM_ZOOM_COMMANDS {
            for i in 0..NUM_ZOOM_COMMANDS {
                if !self.order[..count].contains(&i) {
                    self.order[count] = i;
                    count += 1;
                }
            }
        }
    }

    /// 表示順を設定保存用に直列化(ZoomOptions.cpp:186-197 の `WriteSettings` 順序出力)。
    ///
    /// 各順序位置について `(既定コマンド ID, 表示フラグ)` を返す。コマンド ID の文字列化
    /// (`GetCommandIDText`)は呼び出し側の責務。
    pub fn serialize_order(&self) -> Vec<(i32, bool)> {
        self.order
            .iter()
            .map(|&index| (self.default_command(index), self.zoom_list[index].visible))
            .collect()
    }

    /// メニュー項目を構築(ZoomOptions.cpp:203-249 の `SetMenu` 純粋部分)。
    ///
    /// 表示順に可視項目だけを並べ、各項目のテキストと、`cur_zoom` と一致する最初の項目に対する
    /// チェック状態を算出する。チェックは Rate 種別・Size 種別それぞれで最初の一致のみ付く
    /// (`fRateCheck`/`fSizeCheck`)。
    pub fn build_menu(&self, cur_zoom: Option<&ZoomInfo>) -> Vec<ZoomMenuItem> {
        let mut items = Vec::new();
        let mut rate_checked = false;
        let mut size_checked = false;
        for i in 0..NUM_ZOOM_COMMANDS {
            let index = self.order[i];
            let info = &self.zoom_list[index];
            if !info.visible {
                continue;
            }
            let checked = match info.zoom_type {
                ZoomType::Rate => {
                    if !rate_checked
                        && cur_zoom.is_some_and(|c| {
                            c.rate.get_percentage() == info.rate.get_percentage()
                        })
                    {
                        rate_checked = true;
                        true
                    } else {
                        false
                    }
                }
                ZoomType::Size => {
                    if !size_checked
                        && cur_zoom.is_some_and(|c| {
                            c.size.width == info.size.width && c.size.height == info.size.height
                        })
                    {
                        size_checked = true;
                        true
                    } else {
                        false
                    }
                }
            };
            items.push(ZoomMenuItem {
                command: self.default_command(index),
                text: format_menu_text(info),
                checked,
            });
        }
        items
    }

    /// コマンドテキストのカスタムサフィックス(ZoomOptions.cpp:273-282 の `FormatCommandText` 純粋部分)。
    ///
    /// `LoadString` による基底テキストは Win32 依存のため対象外。カスタムコマンドのとき、
    /// 基底テキストに付加する `" : {}%"`(Rate)または `" : {} x {}"`(Size)を返す。
    /// 非カスタムコマンドは `None`。
    pub fn format_custom_suffix(&self, command: i32, info: &ZoomInfo) -> Option<String> {
        if !is_custom_command(command) {
            return None;
        }
        Some(match info.zoom_type {
            ZoomType::Rate => format!(" : {}%", info.rate.get_percentage()),
            ZoomType::Size => format!(" : {} x {}", info.size.width, info.size.height),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mul_div_matches_win32_rounding() {
        assert_eq!(mul_div(1, 100, 5), 20);
        assert_eq!(mul_div(1, 100, 4), 25);
        assert_eq!(mul_div(1, 100, 3), 33); // 33.33 → 33
        assert_eq!(mul_div(2, 100, 3), 67); // 66.67 → 67(ゼロから遠い側)
        assert_eq!(mul_div(5, 100, 2), 250);
        assert_eq!(mul_div(3, 100, 2), 150);
        assert_eq!(mul_div(1, 100, 0), -1); // 0除算
    }

    #[test]
    fn percentage_uses_muldiv_and_zero_factor() {
        assert_eq!(ZoomRate { rate: 2, factor: 3 }.get_percentage(), 67);
        assert_eq!(ZoomRate { rate: 1, factor: 1 }.get_percentage(), 100);
        assert_eq!(ZoomRate { rate: 5, factor: 2 }.get_percentage(), 250);
        assert_eq!(ZoomRate { rate: 3, factor: 0 }.get_percentage(), 0);
    }

    #[test]
    fn default_list_matches_constants() {
        let opt = ZoomOptions::new();
        assert_eq!(opt.zoom_list().len(), NUM_ZOOM_COMMANDS);
        // 恒等順で初期化される。
        assert_eq!(opt.order(), &(0..NUM_ZOOM_COMMANDS).collect::<Vec<_>>()[..]);
        // CM_ZOOM_100 は等倍・基準サイズ・表示。
        let i100 = opt.get_index_by_command(CM_ZOOM_FIRST + 6).unwrap();
        assert_eq!(i100, 6);
        let info = &opt.zoom_list()[i100];
        assert_eq!(info.rate, ZoomRate { rate: 1, factor: 1 });
        assert_eq!(info.size, ZoomSize { width: 1920, height: 1080 });
        assert!(info.visible);
        // CM_ZOOM_33 のサイズは 1920/3 = 640。
        let i33 = opt.get_index_by_command(CM_ZOOM_FIRST + 2).unwrap();
        assert_eq!(opt.zoom_list()[i33].size, ZoomSize { width: 640, height: 360 });
        // 250% / 300% は既定非表示。
        assert!(!opt.get_zoom_info_by_command(CM_ZOOM_FIRST + 9).unwrap().visible);
        assert!(!opt.get_zoom_info_by_command(CM_ZOOM_FIRST + 10).unwrap().visible);
        // カスタムは 100/100・基準サイズ・非表示。
        let cust = opt.get_zoom_info_by_command(CM_CUSTOMZOOM_FIRST).unwrap();
        assert_eq!(cust.rate, ZoomRate { rate: 100, factor: 100 });
        assert!(!cust.visible);
    }

    #[test]
    fn index_and_info_lookup() {
        let opt = ZoomOptions::new();
        assert_eq!(opt.get_index_by_command(CM_ZOOM_FIRST), Some(0));
        assert_eq!(opt.get_index_by_command(CM_CUSTOMZOOM_FIRST), Some(11));
        assert_eq!(opt.get_index_by_command(CM_CUSTOMZOOM_LAST), Some(20));
        assert_eq!(opt.get_index_by_command(12345), None);
        assert!(opt.get_zoom_info_by_command(12345).is_none());
    }

    #[test]
    fn is_custom_command_boundaries() {
        assert!(!is_custom_command(CM_CUSTOMZOOM_FIRST - 1));
        assert!(is_custom_command(CM_CUSTOMZOOM_FIRST));
        assert!(is_custom_command(CM_CUSTOMZOOM_LAST));
        assert!(!is_custom_command(CM_CUSTOMZOOM_LAST + 1));
        assert!(!is_custom_command(CM_ZOOM_FIRST));
    }

    #[test]
    fn menu_text_rate_and_size() {
        // 1/3 → 端数ありで分数付き。
        let rate_frac = ZoomInfo {
            zoom_type: ZoomType::Rate,
            rate: ZoomRate { rate: 1, factor: 3 },
            size: ZoomSize { width: 640, height: 360 },
            visible: true,
        };
        assert_eq!(format_menu_text(&rate_frac), "33% (1/3)");
        // 1/2 → 端数なしで分数なし。
        let rate_half = ZoomInfo {
            zoom_type: ZoomType::Rate,
            rate: ZoomRate { rate: 1, factor: 2 },
            size: ZoomSize { width: 960, height: 540 },
            visible: true,
        };
        assert_eq!(format_menu_text(&rate_half), "50%");
        // Size 種別。
        let size = ZoomInfo {
            zoom_type: ZoomType::Size,
            ..rate_half
        };
        assert_eq!(format_menu_text(&size), "960 x 540");
    }

    #[test]
    fn custom_suffix_only_for_custom_command() {
        let opt = ZoomOptions::new();
        let rate = ZoomInfo {
            zoom_type: ZoomType::Rate,
            rate: ZoomRate { rate: 100, factor: 100 },
            size: ZoomSize { width: 1920, height: 1080 },
            visible: false,
        };
        assert_eq!(
            opt.format_custom_suffix(CM_CUSTOMZOOM_FIRST, &rate),
            Some(" : 100%".to_string())
        );
        let size = ZoomInfo {
            zoom_type: ZoomType::Size,
            ..rate
        };
        assert_eq!(
            opt.format_custom_suffix(CM_CUSTOMZOOM_FIRST, &size),
            Some(" : 1920 x 1080".to_string())
        );
        // 非カスタムは None。
        assert_eq!(opt.format_custom_suffix(CM_ZOOM_FIRST + 6, &rate), None);
    }

    #[test]
    fn build_menu_default_visible_and_check() {
        let opt = ZoomOptions::new();
        // 既定で表示なのは 20〜200% の 9 項目。
        let cur = ZoomInfo {
            zoom_type: ZoomType::Rate,
            rate: ZoomRate { rate: 1, factor: 1 }, // 100%
            size: ZoomSize { width: 1920, height: 1080 },
            visible: true,
        };
        let menu = opt.build_menu(Some(&cur));
        assert_eq!(menu.len(), 9);
        // 先頭は CM_ZOOM_20、テキストは "20%"。
        assert_eq!(menu[0].command, CM_ZOOM_FIRST);
        assert_eq!(menu[0].text, "20%");
        // 100% の項目だけがチェックされる。
        let checked: Vec<i32> = menu.iter().filter(|m| m.checked).map(|m| m.command).collect();
        assert_eq!(checked, vec![CM_ZOOM_FIRST + 6]);
    }

    #[test]
    fn build_menu_no_cur_zoom_has_no_check() {
        let opt = ZoomOptions::new();
        let menu = opt.build_menu(None);
        assert!(menu.iter().all(|m| !m.checked));
    }

    #[test]
    fn build_menu_size_check_independent_of_rate() {
        let mut opt = ZoomOptions::new();
        // カスタム 0 を Size 種別・表示にする。
        assert!(opt.set_custom_type(0, ZoomType::Size));
        assert!(opt.set_visible_by_command(CM_CUSTOMZOOM_FIRST, true));
        let cur = ZoomInfo {
            zoom_type: ZoomType::Size,
            rate: ZoomRate { rate: 1, factor: 1 },
            size: ZoomSize { width: 1920, height: 1080 },
            visible: true,
        };
        let menu = opt.build_menu(Some(&cur));
        // Size 一致でカスタム項目がチェックされる(Rate 項目は cur が Size なので率不一致では無いが
        // GetPercentage は 100、cur.rate も 100 のため Rate 側も一致しチェックされる)。
        let custom_item = menu.iter().find(|m| m.command == CM_CUSTOMZOOM_FIRST).unwrap();
        assert!(custom_item.checked);
    }

    #[test]
    fn custom_setters_validate() {
        let mut opt = ZoomOptions::new();
        assert!(!opt.set_custom_rate(0, 0)); // rate <= 0
        assert!(!opt.set_custom_rate(0, MAX_RATE + 1)); // rate > MAX_RATE
        assert!(opt.set_custom_rate(0, 500));
        assert_eq!(opt.get_zoom_info_by_command(CM_CUSTOMZOOM_FIRST).unwrap().rate.rate, 500);
        assert!(!opt.set_custom_width(0, 0));
        assert!(opt.set_custom_width(0, 1280));
        assert!(!opt.set_custom_height(0, -1));
        assert!(opt.set_custom_height(0, 720));
        assert_eq!(
            opt.get_zoom_info_by_command(CM_CUSTOMZOOM_FIRST).unwrap().size,
            ZoomSize { width: 1280, height: 720 }
        );
        // カスタム範囲外インデックスは false。
        assert!(!opt.set_custom_rate(NUM_CUSTOM_ZOOM_COMMANDS, 100));
    }

    #[test]
    fn zoom_type_from_int() {
        assert_eq!(ZoomType::from_int(0), Some(ZoomType::Rate));
        assert_eq!(ZoomType::from_int(1), Some(ZoomType::Size));
        assert_eq!(ZoomType::from_int(2), None);
        assert_eq!(ZoomType::Rate.to_int(), 0);
        assert_eq!(ZoomType::Size.to_int(), 1);
    }

    #[test]
    fn apply_order_reorders_and_fills() {
        let mut opt = ZoomOptions::new();
        // 100% を先頭、50% を 2 番目に。表示フラグも反映。
        opt.apply_zoom_list_order(&[(CM_ZOOM_FIRST + 6, true), (CM_ZOOM_FIRST + 3, false)]);
        assert_eq!(opt.order()[0], 6);
        assert_eq!(opt.order()[1], 3);
        assert!(opt.zoom_list()[6].visible);
        assert!(!opt.zoom_list()[3].visible);
        // 残りは元の並び順で補充され、全体は順列のまま。
        let mut sorted = opt.order().to_vec();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..NUM_ZOOM_COMMANDS).collect::<Vec<_>>());
    }

    #[test]
    fn apply_order_skips_unknown_and_duplicates() {
        let mut opt = ZoomOptions::new();
        opt.apply_zoom_list_order(&[
            (CM_ZOOM_FIRST + 6, true),
            (0, true),                 // command 0 → 無視
            (99999, true),             // 未知コマンド → 無視
            (CM_ZOOM_FIRST + 6, false), // 重複 → 無視(visible 上書きされない)
        ]);
        assert_eq!(opt.order()[0], 6);
        assert!(opt.zoom_list()[6].visible); // 重複の false で上書きされない
        assert_eq!(opt.order()[1], 0); // 次は元順の先頭未使用インデックス
    }

    #[test]
    fn apply_order_empty_is_noop() {
        let mut opt = ZoomOptions::new();
        opt.set_order(&{
            let mut v: Vec<usize> = (0..NUM_ZOOM_COMMANDS).rev().collect();
            v.truncate(NUM_ZOOM_COMMANDS);
            v
        });
        let before = opt.order().to_vec();
        opt.apply_zoom_list_order(&[]);
        assert_eq!(opt.order(), &before[..]);
    }

    #[test]
    fn serialize_order_round_trips() {
        let mut opt = ZoomOptions::new();
        opt.apply_zoom_list_order(&[(CM_ZOOM_FIRST + 6, true), (CM_ZOOM_FIRST + 3, false)]);
        let serialized = opt.serialize_order();
        assert_eq!(serialized.len(), NUM_ZOOM_COMMANDS);
        assert_eq!(serialized[0], (CM_ZOOM_FIRST + 6, true));
        assert_eq!(serialized[1], (CM_ZOOM_FIRST + 3, false));
        // 直列化→別インスタンスへ適用で順序・表示が一致する。
        let mut opt2 = ZoomOptions::new();
        opt2.apply_zoom_list_order(&serialized);
        assert_eq!(opt2.order(), opt.order());
    }

    #[test]
    fn set_order_validates_permutation() {
        let mut opt = ZoomOptions::new();
        // 長さ不足。
        assert!(!opt.set_order(&[0, 1, 2]));
        // 重複あり。
        let mut dup: Vec<usize> = (0..NUM_ZOOM_COMMANDS).collect();
        dup[1] = 0;
        assert!(!opt.set_order(&dup));
        // 正しい順列。
        let rev: Vec<usize> = (0..NUM_ZOOM_COMMANDS).rev().collect();
        assert!(opt.set_order(&rev));
        assert_eq!(opt.order()[0], NUM_ZOOM_COMMANDS - 1);
    }

    #[test]
    fn set_custom_zoom_info_replaces_entry() {
        let mut opt = ZoomOptions::new();
        let info = ZoomInfo {
            zoom_type: ZoomType::Size,
            rate: ZoomRate { rate: 100, factor: 100 },
            size: ZoomSize { width: 800, height: 450 },
            visible: true,
        };
        assert!(opt.set_custom_zoom_info(2, info));
        assert_eq!(opt.get_zoom_info_by_command(CM_CUSTOMZOOM_FIRST + 2), Some(&info));
        assert!(!opt.set_custom_zoom_info(NUM_CUSTOM_ZOOM_COMMANDS, info));
    }
}
