// TVTest の NetworkDefinition.cpp / NetworkDefinition.h を Rust へ移植したもの。
//
// 放送ネットワーク(地上/BS/CS)の種別判定と、サービス ID からリモコンキー ID を
// 求めるロジックを扱う。原実装の CSettings 依存は LoadSettings のみで、それ以外の
// 判定・算出ロジックはプラットフォーム非依存のため厳密に移植する。
//
// 設定ファイル読み込み(CSettings)は呼び出し側に委ね、本クレートでは
// 「読み込んだ値を適用する」純粋関数(apply_key_id_assign_list 等)を提供する。
//
// 文字列は原実装の wchar_t(UTF-16)に合わせ &[u16] / Vec<u16> ベースで扱う。

use tvtest_string_utility as su;

/// ネットワーク種別。原実装 NetworkDefinition.h:35 CNetworkDefinition::NetworkType。
/// 判別値は原実装の enum class と同じ並び(Unknown=0, Terrestrial=1, BS=2, CS=3)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkType {
    Unknown = 0,
    Terrestrial = 1,
    Bs = 2,
    Cs = 3,
}

/// ネットワーク情報。原実装 NetworkDefinition.h:42 CNetworkDefinition::NetworkInfo。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkInfo {
    pub network_id: u16,
    pub name: Vec<u16>,
    pub network_type: NetworkType,
}

impl NetworkInfo {
    /// 原実装 NetworkDefinition.cpp:268 のコンストラクタ。
    pub fn new(network_id: u16, name: &[u16], network_type: NetworkType) -> Self {
        Self {
            network_id,
            name: name.to_vec(),
            network_type,
        }
    }
}

/// リモコンキー ID 割り当て情報。
/// 原実装 NetworkDefinition.h:54 CNetworkDefinition::RemoteControlKeyIDAssignInfo。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteControlKeyIDAssignInfo {
    pub network_id: u16,
    pub first_service_id: u16,
    pub last_service_id: u16,
    pub subtrahend: u16,
    pub divisor: u16,
}

/// 既定のリモコンキー ID 割り当てリスト。
/// 原実装 NetworkDefinition.cpp:44 m_DefaultKeyIDAssignList。
pub const DEFAULT_KEY_ID_ASSIGN_LIST: [RemoteControlKeyIDAssignInfo; 3] = [
    // BS 1-3ch
    RemoteControlKeyIDAssignInfo { network_id: 4, first_service_id: 101, last_service_id: 103, subtrahend: 100, divisor: 0 },
    // BS 4-12ch
    RemoteControlKeyIDAssignInfo { network_id: 4, first_service_id: 141, last_service_id: 229, subtrahend: 100, divisor: 10 },
    RemoteControlKeyIDAssignInfo { network_id: 10, first_service_id: 32769, last_service_id: 33767, subtrahend: 32768, divisor: 0 },
];

/// 文字列を WORD(u16)へ変換する。基数 0(自動判定)で読み取り、0xFFFF を超えたら 0。
/// 原実装 NetworkDefinition.cpp:33 StrToWord。
pub fn str_to_word(s: &[u16]) -> u16 {
    let value = su::string_to_uint64(s);
    if value > 0xFFFF {
        return 0;
    }
    value as u16
}

/// ネットワーク名から種別を判定する。
/// 原実装 NetworkDefinition.cpp:235 CNetworkDefinition::GetNetworkTypeFromName。
///
/// 名前の先頭が "T" / "BS" / "CS"(大小無視)で一致し、かつ直後が終端または '.' の場合に
/// 対応する種別を返す。いずれにも該当しなければ Unknown。
pub fn get_network_type_from_name(name: &[u16]) -> NetworkType {
    const LIST: [(NetworkType, &str); 3] = [
        (NetworkType::Terrestrial, "T"),
        (NetworkType::Bs, "BS"),
        (NetworkType::Cs, "CS"),
    ];
    let dot = u16::from(b'.');
    for (ty, prefix) in LIST.iter() {
        let pp: Vec<u16> = prefix.encode_utf16().collect();
        let len = pp.len();
        if name.len() >= len
            && eq_ignore_ascii_case_u16(&name[..len], prefix)
            && (name.len() == len || name[len] == dot)
        {
            return *ty;
        }
    }
    NetworkType::Unknown
}

/// 大小無視の ASCII 比較(原実装の `::StrCmpNI` の ASCII 近似)。
/// ネットワーク種別名は ASCII のため、これで原実装と一致する。
fn eq_ignore_ascii_case_u16(a: &[u16], b: &str) -> bool {
    let bb: Vec<u16> = b.encode_utf16().collect();
    if a.len() != bb.len() {
        return false;
    }
    a.iter().zip(bb.iter()).all(|(&x, &y)| {
        let lx = if (b'A' as u16..=b'Z' as u16).contains(&x) { x + 32 } else { x };
        let ly = if (b'A' as u16..=b'Z' as u16).contains(&y) { y + 32 } else { y };
        lx == ly
    })
}

/// ネットワーク定義。原実装 CNetworkDefinition の状態と純粋メソッドを移植する。
#[derive(Debug, Clone)]
pub struct NetworkDefinition {
    network_info_list: Vec<NetworkInfo>,
    key_id_assign_list: Vec<RemoteControlKeyIDAssignInfo>,
}

impl Default for NetworkDefinition {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkDefinition {
    /// 既定の定義で初期化する。原実装 NetworkDefinition.cpp:51 のコンストラクタ。
    pub fn new() -> Self {
        let network_info_list = vec![
            NetworkInfo::new(4, &su::to_u16("BS"), NetworkType::Bs),
            NetworkInfo::new(6, &su::to_u16("CS.SP-Basic"), NetworkType::Cs),
            NetworkInfo::new(7, &su::to_u16("CS.SP-Basic"), NetworkType::Cs),
            NetworkInfo::new(10, &su::to_u16("CS.SP-Premium"), NetworkType::Cs),
        ];
        let key_id_assign_list = DEFAULT_KEY_ID_ASSIGN_LIST.to_vec();
        Self {
            network_info_list,
            key_id_assign_list,
        }
    }

    /// NetworkInfoList を更新/追加する。原実装 LoadSettings:70-85 の NetworkInfoList 適用部分。
    /// 同一 NetworkID があれば置換、無ければ追加。NetworkID==0(変換失敗)は無視する。
    /// `name` から種別を判定する点も原実装どおり。
    pub fn apply_network_info(&mut self, network_id: u16, name: &[u16]) {
        if network_id == 0 {
            return;
        }
        let network_type = get_network_type_from_name(name);
        let info = NetworkInfo::new(network_id, name, network_type);
        if let Some(slot) = self
            .network_info_list
            .iter_mut()
            .find(|e| e.network_id == network_id)
        {
            *slot = info;
        } else {
            self.network_info_list.push(info);
        }
    }

    /// リモコンキー ID 割り当てリストを差し替える。原実装 LoadSettings:89-134 の適用部分。
    /// `list` は設定から読み取った割り当て群。これにデフォルトを補完して保持する。
    /// 補完条件は原実装どおり「同一 NetworkID かつ ServiceID 範囲が重なる項目が無ければ
    /// デフォルトを追加」。
    pub fn apply_key_id_assign_list(&mut self, list: &[RemoteControlKeyIDAssignInfo]) {
        let mut result: Vec<RemoteControlKeyIDAssignInfo> = list.to_vec();

        for def_info in DEFAULT_KEY_ID_ASSIGN_LIST.iter() {
            let overlaps = result.iter().any(|info| {
                info.network_id == def_info.network_id
                    && info.first_service_id <= def_info.last_service_id
                    && info.last_service_id >= def_info.first_service_id
            });
            if !overlaps {
                result.push(*def_info);
            }
        }

        self.key_id_assign_list = result;
    }

    /// 設定文字列 1 行(カンマ区切り)を割り当て情報へパースする。
    /// 原実装 LoadSettings:100-118 の Split + StrToWord 部分。
    /// 要素数 3 未満なら None(無効行)。
    pub fn parse_key_id_assign(value: &[u16]) -> Option<RemoteControlKeyIDAssignInfo> {
        let array = su::split(value, &su::to_u16(","));
        if array.len() < 3 {
            return None;
        }
        let mut info = RemoteControlKeyIDAssignInfo {
            network_id: str_to_word(&array[0]),
            first_service_id: str_to_word(&array[1]),
            last_service_id: str_to_word(&array[2]),
            subtrahend: 0,
            divisor: 0,
        };
        if array.len() >= 4 {
            info.subtrahend = str_to_word(&array[3]);
            if array.len() >= 5 {
                info.divisor = str_to_word(&array[4]);
            }
        }
        Some(info)
    }

    /// NetworkID に対応する NetworkInfo を返す。原実装 GetNetworkInfoByID。
    pub fn get_network_info_by_id(&self, network_id: u16) -> Option<&NetworkInfo> {
        self.network_info_list
            .iter()
            .find(|e| e.network_id == network_id)
    }

    /// NetworkID の種別を返す。未登録なら Terrestrial(原実装 GetNetworkType:150)。
    pub fn get_network_type(&self, network_id: u16) -> NetworkType {
        match self.get_network_info_by_id(network_id) {
            None => NetworkType::Terrestrial,
            Some(info) => info.network_type,
        }
    }

    /// 地上波か。原実装 IsTerrestrialNetworkID。
    pub fn is_terrestrial_network_id(&self, network_id: u16) -> bool {
        self.get_network_type(network_id) == NetworkType::Terrestrial
    }

    /// BS か。原実装 IsBSNetworkID。
    pub fn is_bs_network_id(&self, network_id: u16) -> bool {
        self.get_network_type(network_id) == NetworkType::Bs
    }

    /// CS か。原実装 IsCSNetworkID。
    pub fn is_cs_network_id(&self, network_id: u16) -> bool {
        self.get_network_type(network_id) == NetworkType::Cs
    }

    /// 衛星(BS または CS)か。原実装 IsSatelliteNetworkID。
    pub fn is_satellite_network_id(&self, network_id: u16) -> bool {
        let ty = self.get_network_type(network_id);
        ty == NetworkType::Bs || ty == NetworkType::Cs
    }

    /// 2 つの NetworkID の種別順序を比較する。原実装 GetNetworkTypeOrder:184。
    /// 同一 ID なら 0。種別が同じなら 0。一方が Unknown ならもう一方を前に。
    /// それ以外は種別の判別値の差。
    pub fn get_network_type_order(&self, network_id1: u16, network_id2: u16) -> i32 {
        if network_id1 == network_id2 {
            return 0;
        }
        let n1 = self.get_network_type(network_id1);
        let n2 = self.get_network_type(network_id2);
        if n1 == n2 {
            0
        } else if n1 == NetworkType::Unknown {
            1
        } else if n2 == NetworkType::Unknown {
            -1
        } else {
            (n1 as i32) - (n2 as i32)
        }
    }

    /// サービス ID からリモコンキー ID を求める。原実装 GetRemoteControlKeyID:203。
    /// 割り当てリストに一致があれば (ServiceID - Subtrahend) / Divisor。
    /// 一致が無ければ ServiceID < 1000 なら ServiceID そのまま、さもなくば 0。
    pub fn get_remote_control_key_id(&self, network_id: u16, service_id: u16) -> i32 {
        for e in self.key_id_assign_list.iter() {
            if network_id == e.network_id
                && service_id >= e.first_service_id
                && service_id <= e.last_service_id
            {
                let mut key_id = service_id as i32 - e.subtrahend as i32;
                if e.divisor != 0 {
                    key_id /= e.divisor as i32;
                }
                return key_id;
            }
        }

        if service_id < 1000 {
            return service_id as i32;
        }

        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Vec<u16> {
        su::to_u16(s)
    }

    #[test]
    fn test_str_to_word() {
        assert_eq!(str_to_word(&w("4")), 4);
        assert_eq!(str_to_word(&w("0x10")), 16); // 基数0自動判定(16進)。
        assert_eq!(str_to_word(&w("65535")), 65535);
        assert_eq!(str_to_word(&w("65536")), 0); // 0xFFFF 超は 0。
        assert_eq!(str_to_word(&w("0")), 0);
    }

    #[test]
    fn test_get_network_type_from_name() {
        assert_eq!(get_network_type_from_name(&w("T")), NetworkType::Terrestrial);
        assert_eq!(get_network_type_from_name(&w("BS")), NetworkType::Bs);
        assert_eq!(get_network_type_from_name(&w("CS")), NetworkType::Cs);
        // 大小無視。
        assert_eq!(get_network_type_from_name(&w("bs")), NetworkType::Bs);
        // 直後が '.' なら一致。
        assert_eq!(get_network_type_from_name(&w("CS.SP-Basic")), NetworkType::Cs);
        // 直後が '.' でも終端でもない('X')なら不一致 → Unknown。
        assert_eq!(get_network_type_from_name(&w("BSX")), NetworkType::Unknown);
        assert_eq!(get_network_type_from_name(&w("Foo")), NetworkType::Unknown);
    }

    #[test]
    fn test_default_network_types() {
        let def = NetworkDefinition::new();
        // 既定: NID 4=BS, 6/7/10=CS。
        assert_eq!(def.get_network_type(4), NetworkType::Bs);
        assert_eq!(def.get_network_type(6), NetworkType::Cs);
        assert_eq!(def.get_network_type(7), NetworkType::Cs);
        assert_eq!(def.get_network_type(10), NetworkType::Cs);
        // 未登録は Terrestrial 扱い。
        assert_eq!(def.get_network_type(0x7880), NetworkType::Terrestrial);

        assert!(def.is_bs_network_id(4));
        assert!(def.is_cs_network_id(6));
        assert!(def.is_satellite_network_id(4));
        assert!(def.is_satellite_network_id(6));
        assert!(def.is_terrestrial_network_id(0x7880));
        assert!(!def.is_satellite_network_id(0x7880));
    }

    #[test]
    fn test_get_network_type_order() {
        let def = NetworkDefinition::new();
        // 同一 ID は 0。
        assert_eq!(def.get_network_type_order(4, 4), 0);
        // 同一種別(CS 同士)は 0。
        assert_eq!(def.get_network_type_order(6, 7), 0);
        // BS(2) vs CS(3) → 2-3 = -1。
        assert_eq!(def.get_network_type_order(4, 6), -1);
        // CS(3) vs BS(2) → 3-2 = 1。
        assert_eq!(def.get_network_type_order(6, 4), 1);
        // Terrestrial(1) vs BS(2) → 1-2 = -1。未登録 ID は Terrestrial。
        assert_eq!(def.get_network_type_order(0x7880, 4), -1);
    }

    #[test]
    fn test_get_remote_control_key_id() {
        let def = NetworkDefinition::new();
        // BS 1-3ch: NID4, SID101-103, subtrahend100, divisor0 → SID-100。
        assert_eq!(def.get_remote_control_key_id(4, 101), 1);
        assert_eq!(def.get_remote_control_key_id(4, 103), 3);
        // BS 4-12ch: NID4, SID141-229, subtrahend100, divisor10 → (SID-100)/10。
        assert_eq!(def.get_remote_control_key_id(4, 141), 4); // (141-100)/10 = 4
        assert_eq!(def.get_remote_control_key_id(4, 229), 12); // (229-100)/10 = 12
        // NID10: SID32769-33767, subtrahend32768, divisor0 → SID-32768。
        assert_eq!(def.get_remote_control_key_id(10, 32769), 1);
        // 割り当てに該当せず SID<1000 → SID そのまま。
        assert_eq!(def.get_remote_control_key_id(0x7880, 500), 500);
        // 割り当てに該当せず SID>=1000 → 0。
        assert_eq!(def.get_remote_control_key_id(0x7880, 1500), 0);
    }

    #[test]
    fn test_parse_key_id_assign() {
        // 3 要素。
        let info = NetworkDefinition::parse_key_id_assign(&w("4,101,103")).unwrap();
        assert_eq!(info.network_id, 4);
        assert_eq!(info.first_service_id, 101);
        assert_eq!(info.last_service_id, 103);
        assert_eq!(info.subtrahend, 0);
        assert_eq!(info.divisor, 0);
        // 5 要素。
        let info = NetworkDefinition::parse_key_id_assign(&w("4,141,229,100,10")).unwrap();
        assert_eq!(info.subtrahend, 100);
        assert_eq!(info.divisor, 10);
        // 要素不足は None。
        assert!(NetworkDefinition::parse_key_id_assign(&w("4,101")).is_none());
    }

    #[test]
    fn test_apply_network_info() {
        let mut def = NetworkDefinition::new();
        // 既存 NID4(BS)を上書き。名前 "T" → Terrestrial。
        def.apply_network_info(4, &w("T"));
        assert_eq!(def.get_network_type(4), NetworkType::Terrestrial);
        // 新規 NID 追加。
        def.apply_network_info(100, &w("CS.Test"));
        assert_eq!(def.get_network_type(100), NetworkType::Cs);
        // NID0 は無視。
        let before = def.get_network_info_by_id(0).is_none();
        def.apply_network_info(0, &w("BS"));
        assert!(before && def.get_network_info_by_id(0).is_none());
    }

    #[test]
    fn test_apply_key_id_assign_list_appends_defaults() {
        let mut def = NetworkDefinition::new();
        // 空リストを適用 → デフォルト 3 件が補完される。
        def.apply_key_id_assign_list(&[]);
        assert_eq!(def.get_remote_control_key_id(4, 101), 1);
        assert_eq!(def.get_remote_control_key_id(10, 32769), 1);
    }

    #[test]
    fn test_apply_key_id_assign_list_user_overrides_default() {
        let mut def = NetworkDefinition::new();
        // ユーザー定義が BS 1-3ch(NID4)と範囲が重なる → そのデフォルトは補完されず
        // ユーザー定義が優先される。
        let user = RemoteControlKeyIDAssignInfo {
            network_id: 4,
            first_service_id: 101,
            last_service_id: 103,
            subtrahend: 0, // subtrahend を変えて差を確認。
            divisor: 0,
        };
        def.apply_key_id_assign_list(&[user]);
        // ユーザー定義(subtrahend0)が先に一致 → SID そのまま。
        assert_eq!(def.get_remote_control_key_id(4, 101), 101);
        // 重ならない NID10 のデフォルトは補完される。
        assert_eq!(def.get_remote_control_key_id(10, 32769), 1);
    }
}
