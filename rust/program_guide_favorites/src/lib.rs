// TVTest の ProgramGuideFavorites.cpp (純粋部分) を Rust へ移植したもの。
//
// 移植対象:
//   - FavoriteInfo 構造体        : CProgramGuideFavorites::FavoriteInfo (ProgramGuideFavorites.h:34)
//   - FavoriteInfo::set_default_colors : FavoriteInfo::SetDefaultColors:99
//   - ProgramGuideFavorites 構造体: CProgramGuideFavorites (ProgramGuideFavorites.h:32)
//     - clear          : Clear:37
//     - count          : GetCount:44
//     - add            : Add:49
//     - get            : Get:57 / Get:68 / Get:77
//     - set            : Set:86
//
// CProgramGuideFavoritesDialog(Win32ダイアログ)は移植対象外。
// 色は RGB(r,g,b) を u32 で保持する(0x00RRGGBB 形式)。

/// 番組表お気に入りエントリ。原実装 CProgramGuideFavorites::FavoriteInfo (ProgramGuideFavorites.h:34)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FavoriteInfo {
    pub name: String,
    pub group_id: String,
    pub label: String,
    pub back_color: u32,
    pub text_color: u32,
}

impl FavoriteInfo {
    pub fn new() -> Self {
        FavoriteInfo {
            name: String::new(),
            group_id: String::new(),
            label: String::new(),
            back_color: 0x00FFFFFF,
            text_color: 0x00000000,
        }
    }

    /// ラベル文字列から地上/BS/CS/お気に入りを判定してデフォルト色を設定する。
    /// 原実装 FavoriteInfo::SetDefaultColors:99。
    pub fn set_default_colors(&mut self) {
        const SPACE_TERRESTRIAL: u32 = 0x1;
        const SPACE_BS: u32          = 0x2;
        const SPACE_CS: u32          = 0x4;
        const SPACE_FAVORITES: u32   = 0x8;

        let mut space: u32 = 0;
        let label = &self.label;

        if label.contains('地') || label.contains("UHF") || label.contains("VHF") {
            space |= SPACE_TERRESTRIAL;
        }
        if label.contains("BS") {
            space |= SPACE_BS;
        }
        if label.contains("CS") {
            space |= SPACE_CS;
        }
        if label.contains("お気に入り") {
            space |= SPACE_FAVORITES;
        }

        let (back, text) = match space {
            SPACE_TERRESTRIAL => (rgb(12, 200, 87),  rgb(255, 255, 255)),
            SPACE_BS          => (rgb(52, 102, 240), rgb(255, 255, 255)),
            SPACE_CS          => (rgb(240, 82, 71),  rgb(255, 255, 255)),
            SPACE_FAVORITES   => (rgb(255, 240, 195), rgb(0, 0, 0)),
            _                 => (rgb(255, 255, 255), rgb(0, 0, 0)),
        };
        self.back_color = back;
        self.text_color = text;
    }
}

impl Default for FavoriteInfo {
    fn default() -> Self {
        Self::new()
    }
}

/// RGB 値を u32 に変換するヘルパー(COLORREF と同じ 0x00RRGGBB 形式)。
pub const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
}

/// 番組表お気に入りリスト。原実装 CProgramGuideFavorites (ProgramGuideFavorites.h:32)。
#[derive(Debug, Clone, Default)]
pub struct ProgramGuideFavorites {
    list: Vec<FavoriteInfo>,
    fixed_width: bool,
}

impl ProgramGuideFavorites {
    pub fn new() -> Self {
        ProgramGuideFavorites {
            list: Vec::new(),
            fixed_width: true,
        }
    }

    /// リストをクリアする。原実装 Clear:37。
    pub fn clear(&mut self) {
        self.list.clear();
    }

    /// エントリ数を返す。原実装 GetCount:44。
    pub fn count(&self) -> usize {
        self.list.len()
    }

    /// エントリを追加する。原実装 Add:49。
    pub fn add(&mut self, info: FavoriteInfo) -> bool {
        self.list.push(info);
        true
    }

    /// インデックスでエントリを取得する(不変参照)。原実装 Get(const):77。
    pub fn get(&self, index: usize) -> Option<&FavoriteInfo> {
        self.list.get(index)
    }

    /// インデックスでエントリを取得する(可変参照)。原実装 Get:68。
    pub fn get_mut(&mut self, index: usize) -> Option<&mut FavoriteInfo> {
        self.list.get_mut(index)
    }

    /// インデックスでエントリを上書きする。原実装 Set:86。
    pub fn set(&mut self, index: usize, info: FavoriteInfo) -> bool {
        if index >= self.list.len() {
            return false;
        }
        self.list[index] = info;
        true
    }

    pub fn get_fixed_width(&self) -> bool {
        self.fixed_width
    }

    pub fn set_fixed_width(&mut self, fixed: bool) {
        self.fixed_width = fixed;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_and_count() {
        let mut fav = ProgramGuideFavorites::new();
        assert_eq!(fav.count(), 0);
        fav.add(FavoriteInfo { label: "地上デジタル".to_string(), ..Default::default() });
        assert_eq!(fav.count(), 1);
    }

    #[test]
    fn test_get_returns_entry() {
        let mut fav = ProgramGuideFavorites::new();
        let info = FavoriteInfo { name: "テスト".to_string(), ..Default::default() };
        fav.add(info.clone());
        assert_eq!(fav.get(0).unwrap().name, "テスト");
        assert!(fav.get(1).is_none());
    }

    #[test]
    fn test_set_replaces_entry() {
        let mut fav = ProgramGuideFavorites::new();
        fav.add(FavoriteInfo { name: "old".to_string(), ..Default::default() });
        let new_info = FavoriteInfo { name: "new".to_string(), ..Default::default() };
        assert!(fav.set(0, new_info));
        assert_eq!(fav.get(0).unwrap().name, "new");
    }

    #[test]
    fn test_set_out_of_bounds() {
        let mut fav = ProgramGuideFavorites::new();
        assert!(!fav.set(0, FavoriteInfo::default()));
    }

    #[test]
    fn test_clear() {
        let mut fav = ProgramGuideFavorites::new();
        fav.add(FavoriteInfo::default());
        fav.clear();
        assert_eq!(fav.count(), 0);
    }

    #[test]
    fn test_set_default_colors_terrestrial() {
        let mut info = FavoriteInfo { label: "地上デジタル".to_string(), ..Default::default() };
        info.set_default_colors();
        assert_eq!(info.back_color, rgb(12, 200, 87));
        assert_eq!(info.text_color, rgb(255, 255, 255));
    }

    #[test]
    fn test_set_default_colors_uhf() {
        let mut info = FavoriteInfo { label: "UHF 全国".to_string(), ..Default::default() };
        info.set_default_colors();
        assert_eq!(info.back_color, rgb(12, 200, 87));
    }

    #[test]
    fn test_set_default_colors_bs() {
        let mut info = FavoriteInfo { label: "BS放送".to_string(), ..Default::default() };
        info.set_default_colors();
        assert_eq!(info.back_color, rgb(52, 102, 240));
        assert_eq!(info.text_color, rgb(255, 255, 255));
    }

    #[test]
    fn test_set_default_colors_cs() {
        let mut info = FavoriteInfo { label: "CS110".to_string(), ..Default::default() };
        info.set_default_colors();
        assert_eq!(info.back_color, rgb(240, 82, 71));
        assert_eq!(info.text_color, rgb(255, 255, 255));
    }

    #[test]
    fn test_set_default_colors_favorites() {
        let mut info = FavoriteInfo { label: "お気に入り".to_string(), ..Default::default() };
        info.set_default_colors();
        assert_eq!(info.back_color, rgb(255, 240, 195));
        assert_eq!(info.text_color, rgb(0, 0, 0));
    }

    #[test]
    fn test_set_default_colors_unknown() {
        let mut info = FavoriteInfo { label: "その他".to_string(), ..Default::default() };
        info.set_default_colors();
        assert_eq!(info.back_color, rgb(255, 255, 255));
        assert_eq!(info.text_color, rgb(0, 0, 0));
    }

    #[test]
    fn test_get_mut() {
        let mut fav = ProgramGuideFavorites::new();
        fav.add(FavoriteInfo { name: "before".to_string(), ..Default::default() });
        fav.get_mut(0).unwrap().name = "after".to_string();
        assert_eq!(fav.get(0).unwrap().name, "after");
    }

    #[test]
    fn test_fixed_width_default() {
        let fav = ProgramGuideFavorites::new();
        assert!(fav.get_fixed_width());
    }

    #[test]
    fn test_set_fixed_width() {
        let mut fav = ProgramGuideFavorites::new();
        fav.set_fixed_width(false);
        assert!(!fav.get_fixed_width());
    }
}
