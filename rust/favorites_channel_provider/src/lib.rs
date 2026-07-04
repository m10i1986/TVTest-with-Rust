#![forbid(unsafe_code)]
//! TVTest のお気に入りチャンネルプロバイダ(`src/Epg.cpp` / `src/Epg.h`、
//! `CEpg::CChannelProviderManager::CFavoritesChannelProvider`)のグループ化ロジックの
//! Rust 移植。
//!
//! お気に入りフォルダ階層を「フォルダ 1 つにつきグループ 1 つ」の配列へ変換する。
//! 各グループは自身の直下チャンネルと、直下サブフォルダ配下の全チャンネル
//! (サブフォルダ自体は別グループとして分離)を保持する。
//!
//! 移植範囲:
//! - [`GroupInfo`](Epg.h:70-75)、[`build_groups`](`AddFavoritesChannels`、
//!   Epg.cpp:291-319)/[`collect_sub_items`](`AddSubItems`、:322-334)。
//! - [`parse_group_id`](`ParseGroupID`、:196-212): グループ ID 文字列からの
//!   インデックス解決(線形探索 + 旧バージョン互換の `"0"` 特例)。
//! - [`group_id`](`GetGroupID`、:186-193): インデックスからグループ ID 取得。
//!
//! 対象外(呼び出し側の責務):
//! - `CProgramGuideBaseChannelProvider` / `CTuningSpaceList` への実際の配線
//!   (`Update`、:151-171)。本クレートは `Vec<GroupInfo>` を返すのみで、
//!   `CTuningSpaceInfo` への変換は呼び出し側が行う。
//! - ルートグループ名を `"お気に入り"` へ上書きする処理(:155)。これは
//!   グループ配列の先頭に対する呼び出し側の後処理として扱う
//!   ([`build_groups`] のドキュメントに明記)。
//! - `GetBonDriver` / `GetBonDriverFileName`(:215-282): チューナー起動判定・
//!   `CAppMain` 経由のチャンネル検索。

use tvtest_favorites::{FavoriteChannel, FavoriteFolder, FavoriteItem};
use tvtest_string_utility::{default_encode_chars, encode};

/// お気に入りの 1 グループ(フォルダ 1 つに対応)。原実装 `GroupInfo`(Epg.h:70-75)。
#[derive(Debug, Clone, Default)]
pub struct GroupInfo {
    /// フォルダ名(`Folder.GetName()`、Epg.cpp:295)。
    pub name: String,
    /// グループ ID(ルートは `"\"`、それ以外はエンコード済みフォルダ名を
    /// `\` で連結したパス、:296-299)。
    pub id: String,
    /// このグループが保持するチャンネル一覧(直下 + サブフォルダ配下の平坦化、
    /// :302-318)。
    pub channels: Vec<FavoriteChannel>,
}

/// フォルダパスの 1 セグメントをエンコードする。原実装
/// `StringUtility::Encode(pItem->GetName(), &Name)`(Epg.cpp:308、既定の
/// エンコード対象文字での呼び出し)。
fn encode_path_segment(name: &str) -> String {
    let utf16: Vec<u16> = name.encode_utf16().collect();
    let encoded = encode(&utf16, &default_encode_chars());
    String::from_utf16_lossy(&encoded)
}

/// フォルダ階層をグループの配列へ変換する。原実装 `AddFavoritesChannels`
/// (Epg.cpp:291-319、再帰)。
///
/// `folder` はルートフォルダ、`path` は空文字列(`String()`、Epg.cpp:154)で
/// 呼び出す。各再帰呼び出しでまず現在のフォルダ用グループを 1 つ追加し
/// (:294-300)、直下の項目を走査してサブフォルダなら [`collect_sub_items`] で
/// そのサブフォルダ配下の全チャンネルを現在のグループへ追加してから
/// 再帰する(:305-313)、チャンネルなら直接現在のグループへ追加する(:314-317)。
///
/// 返り値の先頭要素は必ずルートフォルダに対応するグループ(`id == "\\"`)。
/// 原実装はここで得た `m_GroupList.front()->Name` を呼び出し側
/// (`Update`、:155)が `"お気に入り"` へ上書きする。
#[must_use]
pub fn build_groups(root: &FavoriteFolder) -> Vec<GroupInfo> {
    let mut groups = Vec::new();
    add_favorites_channels(root, "", &mut groups);
    groups
}

fn add_favorites_channels(folder: &FavoriteFolder, path: &str, groups: &mut Vec<GroupInfo>) {
    let group_index = groups.len();
    let id = if path.is_empty() {
        "\\".to_string() // :296-297
    } else {
        path.to_string() // :298-299
    };
    groups.push(GroupInfo {
        name: folder.name.clone(), // :295
        id,
        channels: Vec::new(),
    });

    for i in 0..folder.item_count() {
        match folder.get_item(i) {
            Some(FavoriteItem::Folder(sub_folder)) => {
                let mut folder_path = String::with_capacity(path.len() + 1 + sub_folder.name.len());
                folder_path.push_str(path);
                folder_path.push('\\'); // :310
                folder_path.push_str(&encode_path_segment(&sub_folder.name)); // :308,311

                collect_sub_items(sub_folder, &mut groups[group_index].channels); // :312
                add_favorites_channels(sub_folder, &folder_path, groups); // :313
            }
            Some(FavoriteItem::Channel(channel)) => {
                groups[group_index].channels.push(channel.clone()); // :316
            }
            None => {}
        }
    }
}

/// サブフォルダ配下の全チャンネルを平坦化して集める。原実装 `AddSubItems`
/// (Epg.cpp:322-334、再帰)。フォルダ自体はグループを作らず、チャンネルのみ
/// `channels` へ追加する。
fn collect_sub_items(folder: &FavoriteFolder, channels: &mut Vec<FavoriteChannel>) {
    for i in 0..folder.item_count() {
        match folder.get_item(i) {
            Some(FavoriteItem::Folder(sub_folder)) => {
                collect_sub_items(sub_folder, channels); // :329
            }
            Some(FavoriteItem::Channel(channel)) => {
                channels.push(channel.clone()); // :331
            }
            None => {}
        }
    }
}

/// グループ ID からインデックスを解決する。原実装 `ParseGroupID`
/// (Epg.cpp:196-212)。
///
/// 空文字列は `-1`(:199-200)。`groups` を先頭から線形探索し、`id` と完全一致
/// する最初のインデックスを返す(:202-205)。見つからず、かつ `id == "0"` なら
/// 以前のバージョンとの互換用にインデックス `0`(:208-209、`groups` が空でも
/// 原実装同様 `0` を返す)。それ以外は `-1`(:211)。
#[must_use]
pub fn parse_group_id(groups: &[GroupInfo], id: &str) -> i32 {
    if id.is_empty() {
        return -1; // :199-200
    }
    for (i, group) in groups.iter().enumerate() {
        if group.id == id {
            return i as i32; // :202-205
        }
    }
    if id == "0" {
        return 0; // :207-209
    }
    -1 // :211
}

/// インデックスからグループ ID を取得する。原実装 `GetGroupID`(Epg.cpp:186-193)。
///
/// `group` が範囲外なら `None`(:189-190)。
#[must_use]
pub fn group_id(groups: &[GroupInfo], group: usize) -> Option<&str> {
    groups.get(group).map(|g| g.id.as_str()) // :191-192
}

#[cfg(test)]
mod tests {
    use super::*;
    use tvtest_channel_list::ChannelInfo;
    use tvtest_favorites::FavoritesManager;

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

    // ----- build_groups(Epg.cpp:291-319) -----

    #[test]
    fn build_groups_root_only() {
        let mut root = FavoriteFolder::with_name("root");
        root.add_item(channel_item("A"));
        root.add_item(channel_item("B"));

        let groups = build_groups(&root);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].id, "\\");
        assert_eq!(groups[0].channels.len(), 2);
        assert_eq!(groups[0].channels[0].name, "A");
        assert_eq!(groups[0].channels[1].name, "B");
    }

    #[test]
    fn build_groups_creates_group_per_folder() {
        let mut root = FavoriteFolder::with_name("root");
        root.add_item(channel_item("Top"));
        root.add_item(folder_item(
            "Sub",
            vec![channel_item("SubA"), channel_item("SubB")],
        ));

        let groups = build_groups(&root);
        assert_eq!(groups.len(), 2);

        // ルートグループ: 直下チャンネル + サブフォルダ配下の平坦化(AddSubItems、:312)
        assert_eq!(groups[0].id, "\\");
        assert_eq!(groups[0].channels.len(), 3);
        assert_eq!(groups[0].channels[0].name, "Top");
        assert_eq!(groups[0].channels[1].name, "SubA");
        assert_eq!(groups[0].channels[2].name, "SubB");

        // サブフォルダ自身のグループ: 直下チャンネルのみ(再帰呼び出し、:313)
        assert_eq!(groups[1].id, "\\Sub");
        assert_eq!(groups[1].channels.len(), 2);
        assert_eq!(groups[1].channels[0].name, "SubA");
        assert_eq!(groups[1].channels[1].name, "SubB");
    }

    #[test]
    fn build_groups_nested_folders_path() {
        let mut root = FavoriteFolder::with_name("root");
        root.add_item(folder_item(
            "A",
            vec![folder_item("B", vec![channel_item("C")])],
        ));

        let groups = build_groups(&root);
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].id, "\\");
        assert_eq!(groups[1].id, "\\A");
        assert_eq!(groups[2].id, "\\A\\B");
        // 各階層はすべて末端チャンネル "C" を平坦化して保持する
        for g in &groups {
            assert_eq!(g.channels.len(), 1);
            assert_eq!(g.channels[0].name, "C");
        }
    }

    #[test]
    fn build_groups_encodes_folder_name_in_path() {
        // フォルダ名に区切り文字 '\' を含む場合、パスへ入る前にエンコードされる
        // (StringUtility::Encode、Epg.cpp:308)
        let mut root = FavoriteFolder::with_name("root");
        root.add_item(folder_item("A\\B", vec![channel_item("C")]));

        let groups = build_groups(&root);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[1].id, "\\A%005CB");
    }

    #[test]
    fn build_groups_empty_root() {
        let root = FavoriteFolder::with_name("root");
        let groups = build_groups(&root);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].id, "\\");
        assert!(groups[0].channels.is_empty());
    }

    #[test]
    fn build_groups_from_favorites_manager_root() {
        let mut manager = FavoritesManager::new();
        manager.add_channel(ChannelInfo::new(0, 0, 0, "X"), "BonDriver_X.dll");
        let groups = build_groups(manager.root_folder());
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].channels.len(), 1);
    }

    // ----- parse_group_id(Epg.cpp:196-212) -----

    #[test]
    fn parse_group_id_empty_is_invalid() {
        let groups = vec![GroupInfo {
            id: "\\".to_string(),
            ..Default::default()
        }];
        assert_eq!(parse_group_id(&groups, ""), -1);
    }

    #[test]
    fn parse_group_id_exact_match() {
        let groups = vec![
            GroupInfo {
                id: "\\".to_string(),
                ..Default::default()
            },
            GroupInfo {
                id: "\\A".to_string(),
                ..Default::default()
            },
        ];
        assert_eq!(parse_group_id(&groups, "\\"), 0);
        assert_eq!(parse_group_id(&groups, "\\A"), 1);
    }

    #[test]
    fn parse_group_id_no_match_returns_invalid() {
        let groups = vec![GroupInfo {
            id: "\\".to_string(),
            ..Default::default()
        }];
        assert_eq!(parse_group_id(&groups, "\\B"), -1);
    }

    #[test]
    fn parse_group_id_legacy_zero_compat() {
        // 一致しなくても "0" は旧バージョン互換で常にインデックス 0(:207-209)
        let groups = vec![GroupInfo {
            id: "\\Something".to_string(),
            ..Default::default()
        }];
        assert_eq!(parse_group_id(&groups, "0"), 0);
    }

    #[test]
    fn parse_group_id_legacy_zero_with_empty_groups() {
        let groups: Vec<GroupInfo> = Vec::new();
        assert_eq!(parse_group_id(&groups, "0"), 0);
    }

    #[test]
    fn parse_group_id_prefers_exact_match_over_legacy() {
        // "0" が実際のグループ ID として存在すればそちらが優先される
        let groups = vec![
            GroupInfo {
                id: "\\Other".to_string(),
                ..Default::default()
            },
            GroupInfo {
                id: "0".to_string(),
                ..Default::default()
            },
        ];
        assert_eq!(parse_group_id(&groups, "0"), 1);
    }

    // ----- group_id(Epg.cpp:186-193) -----

    #[test]
    fn group_id_valid_index() {
        let groups = vec![
            GroupInfo {
                id: "\\".to_string(),
                ..Default::default()
            },
            GroupInfo {
                id: "\\A".to_string(),
                ..Default::default()
            },
        ];
        assert_eq!(group_id(&groups, 0), Some("\\"));
        assert_eq!(group_id(&groups, 1), Some("\\A"));
    }

    #[test]
    fn group_id_out_of_range() {
        let groups = vec![GroupInfo {
            id: "\\".to_string(),
            ..Default::default()
        }];
        assert_eq!(group_id(&groups, 1), None);
        assert_eq!(group_id(&groups, 100), None);
    }

    // ----- round trip: build_groups → group_id → parse_group_id -----

    #[test]
    fn round_trip_group_id_resolves_back_to_index() {
        let mut root = FavoriteFolder::with_name("root");
        root.add_item(folder_item(
            "A",
            vec![folder_item("B", vec![channel_item("C")])],
        ));
        let groups = build_groups(&root);

        for i in 0..groups.len() {
            let id = group_id(&groups, i).unwrap().to_string();
            assert_eq!(parse_group_id(&groups, &id), i as i32);
        }
    }
}
