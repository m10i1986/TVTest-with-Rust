//! TVTest の お気に入り(src/Favorites.cpp / Favorites.h)のモデル層移植。
//!
//! フォルダとチャンネルからなる再帰的なツリー構造を表現する:
//! - [`FavoriteItem`](フォルダ or チャンネル)
//! - [`FavoriteFolder`](子項目の追加/削除/移動/検索/列挙)
//! - [`FavoriteChannel`](チャンネル情報 + BonDriver 指定)
//! - [`FavoritesManager`](ルートフォルダ + コマンド ID からのチャンネル解決)
//!
//! # 対象外(Win32 / I-O 依存)
//! メニュー構築(`SetMenu`/`CFavoritesMenu`)、整理ダイアログ
//! (`COrganizeFavoritesDialog`)、ファイル入出力(`Load`/`Save`)。

use tvtest_channel_list::ChannelInfo;

/// お気に入りチャンネルのコマンド ID 範囲(resource.h 487-488)。
pub const CM_FAVORITECHANNEL_FIRST: i32 = 14000;
pub const CM_FAVORITECHANNEL_LAST: i32 = 14999;

/// お気に入りの項目(Favorites.h 35-100 `CFavoriteItem` 派生)。
///
/// `ChannelInfo` が `PartialEq` を持たないため、本型も等価比較は提供しない。
#[derive(Clone, Debug)]
pub enum FavoriteItem {
    Folder(FavoriteFolder),
    Channel(FavoriteChannel),
}

impl FavoriteItem {
    /// 名前を返す(`CFavoriteItem::GetName`)。
    pub fn name(&self) -> &str {
        match self {
            FavoriteItem::Folder(f) => &f.name,
            FavoriteItem::Channel(c) => &c.name,
        }
    }
}

/// お気に入りフォルダ(Favorites.h 56-80 `CFavoriteFolder`)。
#[derive(Clone, Debug, Default)]
pub struct FavoriteFolder {
    pub name: String,
    children: Vec<FavoriteItem>,
}

impl FavoriteFolder {
    /// 空のフォルダを生成する。
    pub fn new() -> Self {
        Self::default()
    }

    /// 名前付きフォルダを生成する。
    pub fn with_name(name: &str) -> Self {
        Self {
            name: name.to_string(),
            children: Vec::new(),
        }
    }

    /// 名前と子項目を消去する(Favorites.cpp 102-106 `Clear`)。
    pub fn clear(&mut self) {
        self.name.clear();
        self.children.clear();
    }

    /// 直下の項目数(Favorites.cpp 108-111 `GetItemCount`)。
    pub fn item_count(&self) -> usize {
        self.children.len()
    }

    /// 直下に項目が無いか。
    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }

    /// 子孫を含む全項目数(Favorites.cpp 113-126 `GetSubItemCount`)。
    pub fn sub_item_count(&self) -> usize {
        let mut count = self.children.len();
        for item in &self.children {
            if let FavoriteItem::Folder(folder) = item {
                count += folder.sub_item_count();
            }
        }
        count
    }

    /// 直下の子項目への参照を返す(Favorites.cpp 128-142 `GetItem`)。
    pub fn get_item(&self, index: usize) -> Option<&FavoriteItem> {
        self.children.get(index)
    }

    /// 直下の子項目への可変参照を返す。
    pub fn get_item_mut(&mut self, index: usize) -> Option<&mut FavoriteItem> {
        self.children.get_mut(index)
    }

    /// 末尾へ項目を追加する(Favorites.cpp 144-152 `AddItem`)。
    pub fn add_item(&mut self, item: FavoriteItem) {
        self.children.push(item);
    }

    /// 指定位置へ項目を挿入する(Favorites.cpp 154-164 `AddItem(Pos, ...)`)。
    /// `pos` が項目数を超える場合は false。
    pub fn add_item_at(&mut self, pos: usize, item: FavoriteItem) -> bool {
        if pos > self.children.len() {
            return false;
        }
        self.children.insert(pos, item);
        true
    }

    /// 指定位置の項目を削除する(Favorites.cpp 166-176 `DeleteItem`)。
    pub fn delete_item(&mut self, index: usize) -> bool {
        if index >= self.children.len() {
            return false;
        }
        self.children.remove(index);
        true
    }

    /// 指定位置の項目を取り出す(Favorites.cpp 178-189 `RemoveItem`)。
    pub fn remove_item(&mut self, index: usize) -> Option<FavoriteItem> {
        if index >= self.children.len() {
            return None;
        }
        Some(self.children.remove(index))
    }

    /// 項目を移動する(Favorites.cpp 191-207 `MoveItem`)。
    ///
    /// `to` は `from` を取り除いた後の並びでの挿入位置(原実装どおり)。
    pub fn move_item(&mut self, from: usize, to: usize) -> bool {
        let len = self.children.len();
        if from >= len || to >= len {
            return false;
        }
        if from != to {
            let item = self.children.remove(from);
            self.children.insert(to, item);
        }
        true
    }

    /// 同名の直下サブフォルダを探す(Favorites.cpp 209-223 `FindSubFolder`)。
    pub fn find_sub_folder(&self, name: &str) -> Option<&FavoriteFolder> {
        self.children.iter().find_map(|item| match item {
            FavoriteItem::Folder(folder) if folder.name == name => Some(folder),
            _ => None,
        })
    }

    /// 同名の直下サブフォルダを可変参照で探す。
    pub fn find_sub_folder_mut(&mut self, name: &str) -> Option<&mut FavoriteFolder> {
        self.children.iter_mut().find_map(|item| match item {
            FavoriteItem::Folder(folder) if folder.name == name => Some(folder),
            _ => None,
        })
    }

    /// 項目を訪問者で再帰列挙する(Favorites.cpp 256-291 `CFavoriteItemEnumerator::EnumItems`)。
    ///
    /// フォルダは `folder_item` を呼んでから再帰、チャンネルは親フォルダと共に
    /// `channel_item` を呼ぶ。いずれかが false を返したら列挙を中断して false。
    pub fn enum_items<V: FavoriteVisitor>(&self, visitor: &mut V) -> bool {
        for item in &self.children {
            match item {
                FavoriteItem::Folder(folder) => {
                    if !visitor.folder_item(folder) {
                        return false;
                    }
                    if !folder.enum_items(visitor) {
                        return false;
                    }
                }
                FavoriteItem::Channel(channel) => {
                    if !visitor.channel_item(self, channel) {
                        return false;
                    }
                }
            }
        }
        true
    }
}

/// お気に入りチャンネル(Favorites.h 82-100 `CFavoriteChannel`)。
#[derive(Clone, Debug)]
pub struct FavoriteChannel {
    pub name: String,
    pub channel_info: ChannelInfo,
    pub bon_driver_file_name: String,
    pub force_bon_driver_change: bool,
}

impl FavoriteChannel {
    /// チャンネル情報から生成する(Favorites.cpp 226-231。名前はチャンネル名)。
    pub fn new(channel_info: ChannelInfo) -> Self {
        let name = channel_info.name.clone();
        Self {
            name,
            channel_info,
            bon_driver_file_name: String::new(),
            force_bon_driver_change: false,
        }
    }

    /// BonDriver ファイル名を設定する(Favorites.cpp 238-246 `SetBonDriverFileName`)。
    pub fn set_bon_driver_file_name(&mut self, file_name: &str) {
        self.bon_driver_file_name = file_name.to_string();
    }
}

/// お気に入り項目の訪問者(Favorites.h 102-110 `CFavoriteItemEnumerator`)。
pub trait FavoriteVisitor {
    /// フォルダ項目。false で列挙中断。
    fn folder_item(&mut self, _folder: &FavoriteFolder) -> bool {
        true
    }
    /// チャンネル項目(親フォルダ付き)。false で列挙中断。
    fn channel_item(&mut self, _parent: &FavoriteFolder, _channel: &FavoriteChannel) -> bool {
        true
    }
}

/// コマンド ID(`CM_FAVORITECHANNEL_FIRST` 基点)からチャンネルを再帰検索する
/// (Favorites.cpp 384-421 `GetChannelByCommandSub`)。
fn get_channel_by_command_sub<'a>(
    folder: &'a FavoriteFolder,
    command: i32,
    base: &mut i32,
) -> Option<&'a FavoriteChannel> {
    for item in &folder.children {
        match item {
            FavoriteItem::Folder(sub) => {
                if let Some(channel) = get_channel_by_command_sub(sub, command, base) {
                    return Some(channel);
                }
            }
            FavoriteItem::Channel(channel) => {
                if command == *base {
                    return Some(channel);
                }
                *base += 1;
            }
        }
    }
    None
}

/// お気に入り管理(Favorites.h 208-242 `CFavoritesManager`)。
#[derive(Clone, Debug, Default)]
pub struct FavoritesManager {
    root_folder: FavoriteFolder,
    modified: bool,
}

impl FavoritesManager {
    /// 空の管理を生成する。
    pub fn new() -> Self {
        Self::default()
    }

    /// ルートフォルダへの参照(Favorites.h 218-219 `GetRootFolder`)。
    pub fn root_folder(&self) -> &FavoriteFolder {
        &self.root_folder
    }

    /// ルートフォルダへの可変参照。
    pub fn root_folder_mut(&mut self) -> &mut FavoriteFolder {
        &mut self.root_folder
    }

    /// ルート直下にチャンネルを追加する(Favorites.cpp 297-310 `AddChannel`)。
    pub fn add_channel(&mut self, channel_info: ChannelInfo, bon_driver_file_name: &str) -> bool {
        let mut channel = FavoriteChannel::new(channel_info);
        channel.set_bon_driver_file_name(bon_driver_file_name);
        self.root_folder.add_item(FavoriteItem::Channel(channel));
        self.modified = true;
        true
    }

    /// 変更フラグ(Favorites.h 225 `GetModified`)。
    pub fn modified(&self) -> bool {
        self.modified
    }

    /// 変更フラグを設定する(Favorites.h 226 `SetModified`)。
    pub fn set_modified(&mut self, modified: bool) {
        self.modified = modified;
    }

    /// コマンド ID からチャンネルを解決する(Favorites.cpp 373-382 `GetChannelByCommand`)。
    ///
    /// チャンネルは深さ優先で `CM_FAVORITECHANNEL_FIRST` から順に番号付けされる
    /// (フォルダは番号を消費しない)。範囲外/不一致は `None`。
    pub fn get_channel_by_command(&self, command: i32) -> Option<&FavoriteChannel> {
        if !(CM_FAVORITECHANNEL_FIRST..=CM_FAVORITECHANNEL_LAST).contains(&command) {
            return None;
        }
        let mut base = CM_FAVORITECHANNEL_FIRST;
        get_channel_by_command_sub(&self.root_folder, command, &mut base)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(name: &str) -> FavoriteChannel {
        FavoriteChannel::new(ChannelInfo::new(0, 0, 0, name))
    }

    fn channel_item(name: &str) -> FavoriteItem {
        FavoriteItem::Channel(channel(name))
    }

    fn folder_item(name: &str, children: Vec<FavoriteItem>) -> FavoriteItem {
        let mut folder = FavoriteFolder::with_name(name);
        for c in children {
            folder.add_item(c);
        }
        FavoriteItem::Folder(folder)
    }

    #[test]
    fn add_and_get_items() {
        let mut f = FavoriteFolder::new();
        assert!(f.is_empty());
        f.add_item(channel_item("A"));
        f.add_item(channel_item("B"));
        assert_eq!(f.item_count(), 2);
        assert_eq!(f.get_item(0).unwrap().name(), "A");
        assert_eq!(f.get_item(1).unwrap().name(), "B");
        assert!(f.get_item(2).is_none());
    }

    #[test]
    fn add_item_at_positions() {
        let mut f = FavoriteFolder::new();
        f.add_item(channel_item("A"));
        f.add_item(channel_item("C"));
        // 中間へ挿入
        assert!(f.add_item_at(1, channel_item("B")));
        assert_eq!(f.get_item(1).unwrap().name(), "B");
        // 末尾(= len)へ挿入可
        assert!(f.add_item_at(3, channel_item("D")));
        // 範囲外
        assert!(!f.add_item_at(99, channel_item("X")));
        assert_eq!(f.item_count(), 4);
    }

    #[test]
    fn delete_and_remove() {
        let mut f = FavoriteFolder::new();
        f.add_item(channel_item("A"));
        f.add_item(channel_item("B"));
        assert!(f.delete_item(0));
        assert_eq!(f.get_item(0).unwrap().name(), "B");
        assert!(!f.delete_item(5));
        let removed = f.remove_item(0).unwrap();
        assert_eq!(removed.name(), "B");
        assert!(f.is_empty());
        assert!(f.remove_item(0).is_none());
    }

    #[test]
    fn move_item_reorders() {
        let mut f = FavoriteFolder::new();
        for n in ["A", "B", "C", "D"] {
            f.add_item(channel_item(n));
        }
        // A(0) を位置 2 へ -> [B, C, A, D]
        assert!(f.move_item(0, 2));
        assert_eq!(f.get_item(0).unwrap().name(), "B");
        assert_eq!(f.get_item(1).unwrap().name(), "C");
        assert_eq!(f.get_item(2).unwrap().name(), "A");
        assert_eq!(f.get_item(3).unwrap().name(), "D");
        // from == to は何もせず true
        assert!(f.move_item(1, 1));
        // 範囲外は false
        assert!(!f.move_item(0, 99));
    }

    #[test]
    fn sub_item_count_recursive() {
        let mut root = FavoriteFolder::new();
        root.add_item(channel_item("A"));
        root.add_item(folder_item("F", vec![channel_item("B"), channel_item("C")]));
        // 直下 2 + サブフォルダ内 2 = 4
        assert_eq!(root.item_count(), 2);
        assert_eq!(root.sub_item_count(), 4);
    }

    #[test]
    fn find_sub_folder() {
        let mut root = FavoriteFolder::new();
        root.add_item(channel_item("A"));
        root.add_item(folder_item("Target", vec![]));
        assert!(root.find_sub_folder("Target").is_some());
        assert!(root.find_sub_folder("Missing").is_none());
        // チャンネルは対象外
        assert!(root.find_sub_folder("A").is_none());
        // 可変参照で名前変更
        root.find_sub_folder_mut("Target").unwrap().name = "Renamed".to_string();
        assert!(root.find_sub_folder("Renamed").is_some());
    }

    struct CollectVisitor {
        folders: Vec<String>,
        channels: Vec<String>,
    }

    impl FavoriteVisitor for CollectVisitor {
        fn folder_item(&mut self, folder: &FavoriteFolder) -> bool {
            self.folders.push(folder.name.clone());
            true
        }
        fn channel_item(&mut self, parent: &FavoriteFolder, channel: &FavoriteChannel) -> bool {
            self.channels.push(format!("{}/{}", parent.name, channel.name));
            true
        }
    }

    #[test]
    fn enum_items_order() {
        let mut root = FavoriteFolder::with_name("root");
        root.add_item(folder_item("F1", vec![channel_item("A"), channel_item("B")]));
        root.add_item(channel_item("C"));

        let mut visitor = CollectVisitor {
            folders: Vec::new(),
            channels: Vec::new(),
        };
        assert!(root.enum_items(&mut visitor));
        assert_eq!(visitor.folders, vec!["F1"]);
        // F1 配下を先に列挙してから root 直下の C
        assert_eq!(visitor.channels, vec!["F1/A", "F1/B", "root/C"]);
    }

    struct StopVisitor;
    impl FavoriteVisitor for StopVisitor {
        fn channel_item(&mut self, _parent: &FavoriteFolder, _channel: &FavoriteChannel) -> bool {
            false // 最初のチャンネルで中断
        }
    }

    #[test]
    fn enum_items_can_stop() {
        let mut root = FavoriteFolder::new();
        root.add_item(channel_item("A"));
        assert!(!root.enum_items(&mut StopVisitor));
    }

    #[test]
    fn manager_add_channel_sets_modified() {
        let mut m = FavoritesManager::new();
        assert!(!m.modified());
        assert!(m.add_channel(ChannelInfo::new(0, 0, 0, "NHK"), "BonDriver.dll"));
        assert!(m.modified());
        assert_eq!(m.root_folder().item_count(), 1);
        if let FavoriteItem::Channel(c) = m.root_folder().get_item(0).unwrap() {
            assert_eq!(c.name, "NHK");
            assert_eq!(c.bon_driver_file_name, "BonDriver.dll");
        } else {
            panic!("expected channel");
        }
    }

    #[test]
    fn get_channel_by_command_dfs_numbering() {
        let mut m = FavoritesManager::new();
        let root = m.root_folder_mut();
        // [ F1[A, B], C, F2[D] ] -> DFS チャンネル順 A,B,C,D
        root.add_item(folder_item("F1", vec![channel_item("A"), channel_item("B")]));
        root.add_item(channel_item("C"));
        root.add_item(folder_item("F2", vec![channel_item("D")]));

        assert_eq!(
            m.get_channel_by_command(CM_FAVORITECHANNEL_FIRST).unwrap().name,
            "A"
        );
        assert_eq!(
            m.get_channel_by_command(CM_FAVORITECHANNEL_FIRST + 1).unwrap().name,
            "B"
        );
        assert_eq!(
            m.get_channel_by_command(CM_FAVORITECHANNEL_FIRST + 2).unwrap().name,
            "C"
        );
        assert_eq!(
            m.get_channel_by_command(CM_FAVORITECHANNEL_FIRST + 3).unwrap().name,
            "D"
        );
        // 範囲内だが該当なし
        assert!(m.get_channel_by_command(CM_FAVORITECHANNEL_FIRST + 4).is_none());
        // 範囲外
        assert!(m.get_channel_by_command(CM_FAVORITECHANNEL_FIRST - 1).is_none());
        assert!(m.get_channel_by_command(CM_FAVORITECHANNEL_LAST + 1).is_none());
    }
}
