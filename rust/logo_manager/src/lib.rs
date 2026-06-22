//! TVTest の `CLogoManager`(src/LogoManager.cpp / LogoManager.h)のモデル層移植。
//!
//! 放送局ロゴの管理のうち、Win32/画像処理に依存しない以下の純粋ロジックを移植する:
//! - ロゴ格納のキー生成([`map_key`] / [`id_map_key`])
//! - ロゴバージョンの比較(12bit ラップアラウンド考慮、[`compare_logo_version`])
//! - SMALL/BIG 指定時の優先順位によるロゴタイプ選択([`LogoStore::find_logo`])
//! - サービスごとのロゴ ID 対応表の管理([`LogoStore::set_logo_id`])
//! - 利用可能なロゴタイプの算出([`LogoStore::available_logo_type`])
//! - ダウンロード時の更新判定([`LogoStore::on_logo_downloaded`])
//!
//! # 対象外(Win32 / 画像 / I/O 依存)
//! bitmap 化(`GetLogoBitmap`)、画像生成(`Graphics::CImage`)、アイコン生成、
//! ファイル入出力(`SaveLogoFile`/`LoadLogoFile`/`SaveLogoIDMap`)、ロック、
//! `LibISDB::LogoDownloaderFilter` 連携。
//!
//! 時刻は原実装の `LibISDB::DateTime` の代わりに、後の時刻ほど大きい `u64` で表す
//! (バージョン同値時の更新判定にのみ用いる)。

use std::collections::BTreeMap;

/// ロゴタイプ(LogoManager.h 77-88)。
pub const LOGOTYPE_48X24: u8 = 0;
pub const LOGOTYPE_36X24: u8 = 1;
pub const LOGOTYPE_48X27: u8 = 2;
pub const LOGOTYPE_72X36: u8 = 3;
pub const LOGOTYPE_54X36: u8 = 4;
pub const LOGOTYPE_64X36: u8 = 5;
pub const LOGOTYPE_FIRST: u8 = LOGOTYPE_48X24;
pub const LOGOTYPE_LAST: u8 = LOGOTYPE_64X36;
/// 取得できる中から小さいものを優先。
pub const LOGOTYPE_SMALL: u8 = 0xFF;
/// 取得できる中から大きいものを優先。
pub const LOGOTYPE_BIG: u8 = 0xFE;

/// SMALL 指定時の探索優先順位(LogoManager.cpp 629)。
const SMALL_LOGO_PRIORITY: [u8; 6] = [2, 0, 1, 5, 3, 4];
/// BIG 指定時の探索優先順位(LogoManager.cpp 630)。
const BIG_LOGO_PRIORITY: [u8; 6] = [5, 3, 4, 2, 0, 1];

/// ロゴ格納マップのキー(LogoManager.h 123-125 `GetMapKey`)。
pub fn map_key(network_id: u16, logo_id: u16, logo_type: u8) -> u64 {
    ((network_id as u64) << 24) | ((logo_id as u64) << 8) | (logo_type as u64)
}

/// ロゴ ID 対応表のキー(LogoManager.h 127-129 `GetIDMapKey`)。
pub fn id_map_key(network_id: u16, service_id: u16) -> u32 {
    ((network_id as u32) << 16) | (service_id as u32)
}

/// ロゴバージョンを比較する(LogoManager.cpp 97-106 `CompareLogoVersion`)。
///
/// ロゴバージョンは 12bit でラップアラウンドするため、2048 を境に巡回比較する。
/// 戻り値は `Version1` が古ければ負、同じなら 0、新しければ正。
pub fn compare_logo_version(version1: u16, version2: u16) -> i32 {
    if version1 == version2 {
        return 0;
    }
    if (version1 <= 2047 && version2 <= 2047) || (version1 >= 2048 && version2 >= 2048) {
        return if version1 < version2 { -1 } else { 1 };
    }
    if version1 <= 2047 {
        return 1;
    }
    -1
}

/// ロゴが対応するサービス(LogoManager のダウンロードデータの ServiceList 要素)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogoService {
    pub network_id: u16,
    pub service_id: u16,
}

/// 1 件のロゴデータ(LibISDB のロゴデータ / `CLogoData` に相当)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogoData {
    pub network_id: u16,
    pub logo_id: u16,
    pub logo_version: u16,
    pub logo_type: u8,
    pub data: Vec<u8>,
    pub time: u64,
    pub service_list: Vec<LogoService>,
}

/// ロゴ情報(LogoManager.h 40-47 `LogoInfo`)。`updated_time` は原実装の `FILETIME` の
/// 代わりに [`LogoData::time`] をそのまま用いる。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogoInfo {
    pub network_id: u16,
    pub logo_id: u16,
    pub logo_version: u16,
    pub logo_type: u8,
    pub updated_time: u64,
}

/// 透明ロゴとみなすデータサイズの上限(LogoManager.cpp 517)。
const TRANSPARENT_LOGO_MAX_SIZE: usize = 93;

/// `CLogoManager` のモデル層(ロゴと ID 対応表の保持・検索・更新判定)。
#[derive(Debug, Default)]
pub struct LogoStore {
    // map_key(NID, LogoID, LogoType) -> ロゴデータ。
    logo_map: BTreeMap<u64, LogoData>,
    // id_map_key(NID, SID) -> LogoID。
    logo_id_map: BTreeMap<u32, u16>,
    force_update: bool,
    logo_updated: bool,
    logo_id_map_updated: bool,
}

impl LogoStore {
    /// 空のストアを生成する。
    pub fn new() -> Self {
        Self::default()
    }

    /// 保持しているロゴと対応表を消去する(LogoManager.cpp `Clear`)。
    pub fn clear(&mut self) {
        self.logo_map.clear();
        self.logo_id_map.clear();
        self.logo_updated = false;
        self.logo_id_map_updated = false;
    }

    /// 強制更新フラグを設定する(LogoManager.h 56 `SetForceUpdate`)。
    pub fn set_force_update(&mut self, force: bool) {
        self.force_update = force;
    }

    /// 強制更新フラグを返す。
    pub fn force_update(&self) -> bool {
        self.force_update
    }

    /// ロゴデータに更新があったか(`IsLogoDataUpdated`)。
    pub fn is_logo_updated(&self) -> bool {
        self.logo_updated
    }

    /// ロゴ ID 対応表に更新があったか(`IsLogoIDMapUpdated`)。
    pub fn is_logo_id_map_updated(&self) -> bool {
        self.logo_id_map_updated
    }

    /// サービスへロゴ ID を対応付ける(LogoManager.cpp 597-620 `SetLogoIDMap`)。
    ///
    /// 未登録なら登録、`update` 指定時かつ既存と異なれば更新。いずれかで対応表を
    /// 変更したら true。
    pub fn set_logo_id(&mut self, network_id: u16, service_id: u16, logo_id: u16, update: bool) -> bool {
        let key = id_map_key(network_id, service_id);
        match self.logo_id_map.get_mut(&key) {
            None => {
                self.logo_id_map.insert(key, logo_id);
                self.logo_id_map_updated = true;
                true
            }
            Some(existing) => {
                if update && *existing != logo_id {
                    *existing = logo_id;
                    self.logo_id_map_updated = true;
                    true
                } else {
                    false
                }
            }
        }
    }

    /// サービスへロゴ ID を対応付ける(LogoManager.h 58 `AssociateLogoID`)。
    /// `set_logo_id(.., true)` と同じ。
    pub fn associate_logo_id(&mut self, network_id: u16, service_id: u16, logo_id: u16) -> bool {
        self.set_logo_id(network_id, service_id, logo_id, true)
    }

    /// サービスに対応付けられたロゴ ID を返す。
    pub fn logo_id(&self, network_id: u16, service_id: u16) -> Option<u16> {
        self.logo_id_map.get(&id_map_key(network_id, service_id)).copied()
    }

    /// ロゴを読み込み追加する(LogoManager.cpp 290-298 `LoadLogoFile` の格納処理に相当)。
    ///
    /// 既存があればバージョンが新しい場合のみ置き換える。
    pub fn add_logo(&mut self, data: LogoData) {
        let key = map_key(data.network_id, data.logo_id, data.logo_type);
        match self.logo_map.get(&key) {
            Some(existing) => {
                if compare_logo_version(existing.logo_version, data.logo_version) < 0 {
                    self.logo_map.insert(key, data);
                }
            }
            None => {
                self.logo_map.insert(key, data);
            }
        }
    }

    /// ダウンロードされたロゴを反映する(LogoManager.cpp 514-563 `OnLogoDownloaded`)。
    ///
    /// 透明ロゴ(データサイズ <= 93)は無視。バージョン/時刻/データを比較して更新し、
    /// ServiceList の各サービスへロゴ ID を対応付ける。bitmap 化・ファイル保存は対象外。
    /// 何らかの更新があれば true。
    pub fn on_logo_downloaded(&mut self, data: &LogoData) -> bool {
        // 透明なロゴは除外。
        if data.data.len() <= TRANSPARENT_LOGO_MAX_SIZE {
            return false;
        }

        let key = map_key(data.network_id, data.logo_id, data.logo_type);
        let mut updated = false;

        match self.logo_map.get_mut(&key) {
            Some(existing) => {
                let ver_cmp = if self.force_update {
                    -1
                } else {
                    // バージョンが新しい場合のみ更新。
                    compare_logo_version(existing.logo_version, data.logo_version)
                };
                if ver_cmp < 0 || (ver_cmp == 0 && existing.time < data.time) {
                    // BS/CS はバージョンが共通のため、データを比較して更新を確認する。
                    if data.data != existing.data {
                        *existing = data.clone();
                        updated = true;
                    } else if ver_cmp < 0 && existing.logo_version != data.logo_version {
                        existing.logo_version = data.logo_version;
                        updated = true;
                    }
                }
            }
            None => {
                self.logo_map.insert(key, data.clone());
                updated = true;
            }
        }

        if updated {
            self.logo_updated = true;
        }

        for service in &data.service_list {
            let key = id_map_key(service.network_id, service.service_id);
            match self.logo_id_map.get_mut(&key) {
                None => {
                    self.logo_id_map.insert(key, data.logo_id);
                    self.logo_id_map_updated = true;
                }
                Some(existing) => {
                    if updated && *existing != data.logo_id {
                        *existing = data.logo_id;
                        self.logo_id_map_updated = true;
                    }
                }
            }
        }

        updated
    }

    /// ロゴを検索する(LogoManager.cpp 623-643 `FindLogoData` の検索部)。
    ///
    /// SMALL/BIG 指定時は優先順位に従い、最初に見つかったタイプのロゴを返す。
    /// 該当が無ければ `None`(原実装ではファイルからの読み込みを試みるが対象外)。
    pub fn find_logo(&self, network_id: u16, logo_id: u16, logo_type: u8) -> Option<&LogoData> {
        if logo_type == LOGOTYPE_SMALL || logo_type == LOGOTYPE_BIG {
            let priority = if logo_type == LOGOTYPE_SMALL {
                &SMALL_LOGO_PRIORITY
            } else {
                &BIG_LOGO_PRIORITY
            };
            for &t in priority {
                if let Some(data) = self.logo_map.get(&map_key(network_id, logo_id, t)) {
                    return Some(data);
                }
            }
            None
        } else {
            self.logo_map.get(&map_key(network_id, logo_id, logo_type))
        }
    }

    /// 指定のロゴが存在するか(LogoManager.cpp 459-464 `IsLogoAvailable`)。
    ///
    /// 原実装は `GetMapKey(NetworkID, ServiceID, LogoType)` を引く(LogoID の位置に
    /// ServiceID を渡す)ため、その挙動を忠実に再現する。
    pub fn is_logo_available(&self, network_id: u16, service_id: u16, logo_type: u8) -> bool {
        self.logo_map.contains_key(&map_key(network_id, service_id, logo_type))
    }

    /// 利用可能なロゴタイプのビットフラグを返す(LogoManager.cpp 467-482 `GetAvailableLogoType`)。
    ///
    /// サービスにロゴ ID が対応付いていなければ 0。各タイプ i が存在すれば bit i が立つ。
    pub fn available_logo_type(&self, network_id: u16, service_id: u16) -> u32 {
        let logo_id = match self.logo_id_map.get(&id_map_key(network_id, service_id)) {
            Some(&id) => id,
            None => return 0,
        };
        let mut flags = 0u32;
        for i in LOGOTYPE_FIRST..=LOGOTYPE_LAST {
            if self.logo_map.contains_key(&map_key(network_id, logo_id, i)) {
                flags |= 1u32 << i;
            }
        }
        flags
    }

    /// ロゴ情報を取得する(LogoManager.cpp 485-511 `GetLogoInfo`)。
    ///
    /// サービスのロゴ ID を引き、指定タイプのロゴが存在すればその情報を返す。
    pub fn logo_info(&self, network_id: u16, service_id: u16, logo_type: u8) -> Option<LogoInfo> {
        let logo_id = *self.logo_id_map.get(&id_map_key(network_id, service_id))?;
        let data = self.logo_map.get(&map_key(network_id, logo_id, logo_type))?;
        Some(LogoInfo {
            network_id: data.network_id,
            logo_id: data.logo_id,
            logo_version: data.logo_version,
            logo_type: data.logo_type,
            updated_time: data.time,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn logo(network_id: u16, logo_id: u16, logo_type: u8, version: u16) -> LogoData {
        LogoData {
            network_id,
            logo_id,
            logo_version: version,
            logo_type,
            data: vec![0xAB; 100], // 透明ロゴ判定(<=93)を超えるサイズ
            time: 0,
            service_list: Vec::new(),
        }
    }

    #[test]
    fn keys() {
        assert_eq!(map_key(0x0004, 0x0102, 0x05), 0x0401_0205);
        assert_eq!(id_map_key(0x0004, 0x0810), 0x0004_0810);
    }

    #[test]
    fn version_compare_same_half() {
        assert_eq!(compare_logo_version(100, 100), 0);
        assert_eq!(compare_logo_version(100, 200), -1); // 古い
        assert_eq!(compare_logo_version(200, 100), 1); // 新しい
        assert_eq!(compare_logo_version(3000, 3500), -1);
        assert_eq!(compare_logo_version(3500, 3000), 1);
    }

    #[test]
    fn version_compare_wraparound() {
        // 低い側(<=2047)と高い側(>=2048)をまたぐ場合は巡回比較。
        // Version1 が低い側なら Version1 を「新しい」とみなす(戻り値 1)。
        assert_eq!(compare_logo_version(10, 3000), 1);
        assert_eq!(compare_logo_version(3000, 10), -1);
    }

    #[test]
    fn logo_id_map_set_and_update() {
        let mut s = LogoStore::new();
        assert!(s.set_logo_id(4, 0x10, 0x100, true)); // 新規
        assert_eq!(s.logo_id(4, 0x10), Some(0x100));
        assert!(s.is_logo_id_map_updated());
        // 同値は変化なし
        assert!(!s.set_logo_id(4, 0x10, 0x100, true));
        // update=false なら異なる値でも変えない
        assert!(!s.set_logo_id(4, 0x10, 0x200, false));
        assert_eq!(s.logo_id(4, 0x10), Some(0x100));
        // update=true で更新
        assert!(s.set_logo_id(4, 0x10, 0x200, true));
        assert_eq!(s.logo_id(4, 0x10), Some(0x200));
    }

    #[test]
    fn add_logo_replaces_only_newer() {
        let mut s = LogoStore::new();
        s.add_logo(logo(4, 0x100, LOGOTYPE_48X24, 5));
        // 古いバージョンは置き換えない
        s.add_logo(logo(4, 0x100, LOGOTYPE_48X24, 3));
        assert_eq!(
            s.find_logo(4, 0x100, LOGOTYPE_48X24).unwrap().logo_version,
            5
        );
        // 新しいバージョンは置き換える
        s.add_logo(logo(4, 0x100, LOGOTYPE_48X24, 8));
        assert_eq!(
            s.find_logo(4, 0x100, LOGOTYPE_48X24).unwrap().logo_version,
            8
        );
    }

    #[test]
    fn find_logo_small_big_priority() {
        let mut s = LogoStore::new();
        // タイプ 0 と 5 のみ存在
        s.add_logo(logo(4, 0x100, LOGOTYPE_48X24, 1)); // type 0
        s.add_logo(logo(4, 0x100, LOGOTYPE_64X36, 1)); // type 5
        // SMALL 優先 [2,0,1,5,3,4] -> 最初に存在する 0
        assert_eq!(s.find_logo(4, 0x100, LOGOTYPE_SMALL).unwrap().logo_type, 0);
        // BIG 優先 [5,3,4,2,0,1] -> 最初に存在する 5
        assert_eq!(s.find_logo(4, 0x100, LOGOTYPE_BIG).unwrap().logo_type, 5);
        // 該当タイプ無し
        assert!(s.find_logo(4, 0x100, LOGOTYPE_48X27).is_none());
        assert!(s.find_logo(4, 0x999, LOGOTYPE_SMALL).is_none());
    }

    #[test]
    fn available_logo_type_flags() {
        let mut s = LogoStore::new();
        // ID 対応が無ければ 0
        assert_eq!(s.available_logo_type(4, 0x10), 0);
        s.set_logo_id(4, 0x10, 0x100, true);
        s.add_logo(logo(4, 0x100, LOGOTYPE_48X24, 1)); // bit 0
        s.add_logo(logo(4, 0x100, LOGOTYPE_72X36, 1)); // bit 3
        assert_eq!(s.available_logo_type(4, 0x10), (1 << 0) | (1 << 3));
    }

    #[test]
    fn logo_info_lookup() {
        let mut s = LogoStore::new();
        s.set_logo_id(4, 0x10, 0x100, true);
        s.add_logo(logo(4, 0x100, LOGOTYPE_48X24, 7));
        let info = s.logo_info(4, 0x10, LOGOTYPE_48X24).unwrap();
        assert_eq!(info.network_id, 4);
        assert_eq!(info.logo_id, 0x100);
        assert_eq!(info.logo_version, 7);
        assert_eq!(info.logo_type, LOGOTYPE_48X24);
        // 未対応サービス / 未存在タイプ
        assert!(s.logo_info(4, 0x99, LOGOTYPE_48X24).is_none());
        assert!(s.logo_info(4, 0x10, LOGOTYPE_64X36).is_none());
    }

    #[test]
    fn on_logo_downloaded_excludes_transparent() {
        let mut s = LogoStore::new();
        let mut d = logo(4, 0x100, LOGOTYPE_48X24, 1);
        d.data = vec![0u8; 93]; // 透明ロゴ
        assert!(!s.on_logo_downloaded(&d));
        assert!(s.find_logo(4, 0x100, LOGOTYPE_48X24).is_none());
    }

    #[test]
    fn on_logo_downloaded_inserts_and_maps_services() {
        let mut s = LogoStore::new();
        let mut d = logo(4, 0x100, LOGOTYPE_48X24, 1);
        d.service_list = vec![
            LogoService { network_id: 4, service_id: 0x10 },
            LogoService { network_id: 4, service_id: 0x11 },
        ];
        assert!(s.on_logo_downloaded(&d));
        assert!(s.find_logo(4, 0x100, LOGOTYPE_48X24).is_some());
        assert_eq!(s.logo_id(4, 0x10), Some(0x100));
        assert_eq!(s.logo_id(4, 0x11), Some(0x100));
        assert!(s.is_logo_updated());

        // 同じものを再投入 -> 更新なし
        assert!(!s.on_logo_downloaded(&d));
    }

    #[test]
    fn on_logo_downloaded_updates_newer_version() {
        let mut s = LogoStore::new();
        s.on_logo_downloaded(&logo(4, 0x100, LOGOTYPE_48X24, 1));
        // 新しいバージョン + 異なるデータ -> 更新
        let mut d2 = logo(4, 0x100, LOGOTYPE_48X24, 2);
        d2.data = vec![0xCD; 100];
        assert!(s.on_logo_downloaded(&d2));
        assert_eq!(
            s.find_logo(4, 0x100, LOGOTYPE_48X24).unwrap().logo_version,
            2
        );
    }

    #[test]
    fn on_logo_downloaded_force_update() {
        let mut s = LogoStore::new();
        s.on_logo_downloaded(&logo(4, 0x100, LOGOTYPE_48X24, 5));
        s.set_force_update(true);
        // 古いバージョンでもデータが異なれば強制更新される
        let mut d = logo(4, 0x100, LOGOTYPE_48X24, 1);
        d.data = vec![0xEF; 100];
        assert!(s.on_logo_downloaded(&d));
        assert_eq!(
            s.find_logo(4, 0x100, LOGOTYPE_48X24).unwrap().logo_version,
            1
        );
    }

    #[test]
    fn is_logo_available_quirk() {
        // IsLogoAvailable は LogoID の位置に ServiceID を渡す原実装の挙動を再現。
        let mut s = LogoStore::new();
        s.add_logo(logo(4, 0x20, LOGOTYPE_48X24, 1)); // logo_id=0x20
        assert!(s.is_logo_available(4, 0x20, LOGOTYPE_48X24));
        assert!(!s.is_logo_available(4, 0x21, LOGOTYPE_48X24));
    }
}
