//! TVTest のドライバ管理(`src/DriverManager.cpp` / `DriverManager.h`)の純粋ロジックを
//! 移植したクレート。
//!
//! 移植対象:
//! - チューナー仕様フラグ(`TunerSpec::Flag`)とその文字列パース(`LoadTunerSpec`)。
//! - チューナー名のワイルドカードマッチ(`GetTunerSpec` の `PathMatchSpec`)。
//! - パスユーティリティ(`PathFindFileName` / `PathFindExtension`)。
//! - 連番ドライバ(`BonDriver*N.dll`)の先頭ファイル名判定(`GetAllServiceList` の連番スキップ)。
//! - ファイル名による検索(`FindByFileName`)とドライバ一覧の整列(`Find` の `lstrcmpi` ソート)。
//!
//! BonDriver ロード・ディレクトリ走査(`FindFirstFileEx`)・`CTuningSpaceList`・`CSettings` I/O
//! (Win32/I-O)は対象外。`PathMatchSpec`/`IsEqualFileName`/`lstrcmpi` の大小無視は ASCII 近似
//! (他クレートと同方針)。

use bitflags::bitflags;

bitflags! {
    /// チューナー仕様フラグ(DriverManager.h:73-81 の `TunerSpec::Flag`)。
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct TunerSpecFlag: u32 {
        /// ネットワーク経由。
        const NETWORK = 0x0001;
        /// ファイル再生。
        const FILE = 0x0002;
        /// 仮想チューナー。
        const VIRTUAL = 0x0004;
        /// 揮発性(取り外し等)。
        const VOLATILE = 0x0008;
        /// チャンネル列挙をしない。
        const NO_ENUM_CHANNEL = 0x0010;
    }
}

/// フラグ名 → フラグの対応表(DriverManager.cpp:313-322 の `FlagList`)。
const FLAG_ENTRIES: [(&str, TunerSpecFlag); 5] = [
    ("network", TunerSpecFlag::NETWORK),
    ("file", TunerSpecFlag::FILE),
    ("virtual", TunerSpecFlag::VIRTUAL),
    ("volatile", TunerSpecFlag::VOLATILE),
    ("no-enum-channel", TunerSpecFlag::NO_ENUM_CHANNEL),
];

/// チューナー仕様フラグ文字列をパース(DriverManager.cpp:311-332 の `LoadTunerSpec` 属性解析)。
///
/// `|` 区切りの各属性を前後空白除去のうえ大小無視で名前照合し、一致したフラグを立てる。
/// 未知の属性は無視。
pub fn parse_tuner_spec_flags(value: &str) -> TunerSpecFlag {
    let mut flags = TunerSpecFlag::empty();
    for attribute in value.split('|') {
        let attribute = attribute.trim();
        for (name, flag) in FLAG_ENTRIES {
            if attribute.eq_ignore_ascii_case(name) {
                flags |= flag;
                break;
            }
        }
    }
    flags
}

/// パスからファイル名部分を取得(`PathFindFileName` 相当)。最後の `\` または `/` の次から末尾まで。
pub fn path_find_file_name(path: &str) -> &str {
    match path.rfind(['\\', '/']) {
        Some(pos) => &path[pos + 1..],
        None => path,
    }
}

/// パスから拡張子を取得(`PathFindExtension` 相当)。ファイル名部分の最後の `.` から末尾まで。
/// 拡張子が無ければ空文字列。
pub fn path_find_extension(path: &str) -> &str {
    let name = path_find_file_name(path);
    match name.rfind('.') {
        Some(pos) => &name[pos..],
        None => "",
    }
}

/// ASCII 大小無視のファイル名一致(`IsEqualFileName` の近似)。
pub fn is_equal_file_name(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// ワイルドカードパターン照合(`PathMatchSpec` の基本相当)。
///
/// `spec` を `;` で区切った各パターンのいずれかにマッチすれば真。各パターンは `*`(0 文字以上)・
/// `?`(任意 1 文字)を解釈し、それ以外の文字は ASCII 大小無視で比較する。DOS ワイルドカードの
/// 特殊挙動(`.` の暗黙マッチ等)は再現しない。
pub fn path_match_spec(name: &str, spec: &str) -> bool {
    spec.split(';')
        .any(|pattern| wildcard_match_ci(name, pattern))
}

fn wildcard_match_ci(name: &str, pattern: &str) -> bool {
    let n: Vec<char> = name.chars().collect();
    let p: Vec<char> = pattern.chars().collect();
    let mut i = 0; // name のインデックス
    let mut j = 0; // pattern のインデックス
    let mut star_j: Option<usize> = None; // 直近の '*' の pattern インデックス
    let mut star_i = 0; // その '*' でマッチ開始した name インデックス

    while i < n.len() {
        if j < p.len() && (p[j] == '?' || p[j].eq_ignore_ascii_case(&n[i])) {
            i += 1;
            j += 1;
        } else if j < p.len() && p[j] == '*' {
            star_j = Some(j);
            star_i = i;
            j += 1;
        } else if let Some(sj) = star_j {
            j = sj + 1;
            star_i += 1;
            i = star_i;
        } else {
            return false;
        }
    }
    while j < p.len() && p[j] == '*' {
        j += 1;
    }
    j == p.len()
}

/// 連番ドライバの先頭ファイル名(DriverManager.cpp:256-265 の `GetAllServiceList` 連番判定)。
///
/// 拡張子の直前の文字が `1`〜`9` のとき、その文字を `0` に置き換えたファイル名を返す
/// (例: `BonDriver_PT-S1.dll` → `BonDriver_PT-S0.dll`)。拡張子が先頭(`.dll` 等)や直前が
/// `1`〜`9` でなければ `None`。`file_name` はパスを含まないファイル名を想定する。
pub fn sequel_driver_first_file_name(file_name: &str) -> Option<String> {
    // PathFindExtension 相当: 最後の '.' の位置(無ければ末尾 = len = ヌル位置)。
    let ext_pos = file_name.rfind('.').unwrap_or(file_name.len());
    if ext_pos == 0 {
        // pszExtension > pszFileName が不成立(先頭が拡張子)。
        return None;
    }
    let bytes = file_name.as_bytes();
    let c = bytes[ext_pos - 1];
    if (b'1'..=b'9').contains(&c) {
        let mut new_bytes = bytes.to_vec();
        new_bytes[ext_pos - 1] = b'0';
        // ASCII の '1'..'9' を '0' に置換するだけなので妥当な UTF-8。
        String::from_utf8(new_bytes).ok()
    } else {
        None
    }
}

/// チューナー仕様 1 件(DriverManager.h:99-103 の `TunerSpecInfo`)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TunerSpecInfo {
    /// チューナー名のマスク(ワイルドカードパターン)。
    pub tuner_mask: String,
    /// 仕様フラグ。
    pub flags: TunerSpecFlag,
}

/// CDriverManager のモデル層。ドライバファイル名一覧とチューナー仕様一覧を保持する。
///
/// ディレクトリ走査(`Find`)・BonDriver ロード(`CDriverInfo`)は Win32/I-O のため対象外。
/// 走査で得たファイル名一覧の整列・検索、設定から読んだ仕様一覧の照会といった純粋部分のみ扱う。
#[derive(Clone, Debug, Default)]
pub struct DriverManager {
    driver_files: Vec<String>,
    tuner_spec_list: Vec<TunerSpecInfo>,
}

impl DriverManager {
    /// 空のマネージャを生成。
    pub fn new() -> Self {
        Self::default()
    }

    /// ドライバ一覧をクリア(`Clear`)。チューナー仕様一覧は保持する(C++ `Clear` と同じ)。
    pub fn clear(&mut self) {
        self.driver_files.clear();
    }

    /// ドライバファイル名を追加(`Find` のファイル列挙結果に相当)。
    pub fn add_driver_file(&mut self, file_name: &str) {
        self.driver_files.push(file_name.to_string());
    }

    /// ドライバ数(`NumDrivers`)。
    pub fn num_drivers(&self) -> usize {
        self.driver_files.len()
    }

    /// インデックス指定でドライバファイル名を取得。
    pub fn get_driver_file(&self, index: usize) -> Option<&str> {
        self.driver_files.get(index).map(String::as_str)
    }

    /// ドライバファイル名一覧を大小無視で整列(`Find` の `lstrcmpi` ソート、DriverManager.cpp:205-212)。
    pub fn sort_drivers(&mut self) {
        self.driver_files.sort_by_key(|a| a.to_ascii_lowercase());
    }

    /// ファイル名でドライバを検索(`FindByFileName`)。空文字列や未登録は `None`。
    pub fn find_by_file_name(&self, file_name: &str) -> Option<usize> {
        if file_name.is_empty() {
            return None;
        }
        self.driver_files
            .iter()
            .position(|f| is_equal_file_name(f, file_name))
    }

    /// 連番ドライバとしてスキップすべきか(`GetAllServiceList` の連番スキップ判定)。
    ///
    /// 連番先頭(`...0.dll`)が登録済みなら、その連番(`...1.dll` 等)はスキップ対象。
    pub fn should_skip_as_sequel(&self, file_name: &str) -> bool {
        match sequel_driver_first_file_name(file_name) {
            Some(first) => self.find_by_file_name(&first).is_some(),
            None => false,
        }
    }

    /// チューナー仕様一覧を設定から読み込む(`LoadTunerSpec` の純粋部分、追加方式でクリアしない)。
    ///
    /// `entries` は `(チューナーマスク, 属性文字列)` の並び(`CSettings` のセクションエントリ相当)。
    pub fn load_tuner_spec(&mut self, entries: &[(String, String)]) {
        for (name, value) in entries {
            self.tuner_spec_list.push(TunerSpecInfo {
                tuner_mask: name.clone(),
                flags: parse_tuner_spec_flags(value),
            });
        }
    }

    /// チューナー仕様一覧。
    pub fn tuner_spec_list(&self) -> &[TunerSpecInfo] {
        &self.tuner_spec_list
    }

    /// チューナー名から仕様フラグを取得(`GetTunerSpec`)。
    ///
    /// チューナー名のファイル名部分を取り出し、最初にワイルドカード一致したマスクのフラグを返す。
    /// ファイル名部分が空、または一致が無ければ `None`。
    pub fn get_tuner_spec(&self, tuner_name: &str) -> Option<TunerSpecFlag> {
        let name = path_find_file_name(tuner_name);
        if name.is_empty() {
            return None;
        }
        self.tuner_spec_list
            .iter()
            .find(|e| path_match_spec(name, &e.tuner_mask))
            .map(|e| e.flags)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_flags_basic() {
        assert_eq!(
            parse_tuner_spec_flags("network|file"),
            TunerSpecFlag::NETWORK | TunerSpecFlag::FILE
        );
        // 前後空白・大小無視。
        assert_eq!(
            parse_tuner_spec_flags(" Virtual | VOLATILE "),
            TunerSpecFlag::VIRTUAL | TunerSpecFlag::VOLATILE
        );
        assert_eq!(
            parse_tuner_spec_flags("no-enum-channel"),
            TunerSpecFlag::NO_ENUM_CHANNEL
        );
        // 空・未知は無視。
        assert_eq!(parse_tuner_spec_flags(""), TunerSpecFlag::empty());
        assert_eq!(
            parse_tuner_spec_flags("unknown|foo"),
            TunerSpecFlag::empty()
        );
        // 既知 + 未知混在。
        assert_eq!(
            parse_tuner_spec_flags("file|bogus|network"),
            TunerSpecFlag::FILE | TunerSpecFlag::NETWORK
        );
    }

    #[test]
    fn path_find_file_name_cases() {
        assert_eq!(
            path_find_file_name("C:\\dir\\BonDriver.dll"),
            "BonDriver.dll"
        );
        assert_eq!(path_find_file_name("a/b/c.dll"), "c.dll");
        assert_eq!(path_find_file_name("BonDriver.dll"), "BonDriver.dll");
        assert_eq!(path_find_file_name("dir\\"), ""); // 末尾が区切り
    }

    #[test]
    fn path_find_extension_cases() {
        assert_eq!(path_find_extension("BonDriver.dll"), ".dll");
        assert_eq!(path_find_extension("a.b.dll"), ".dll"); // 最後のドット
        assert_eq!(path_find_extension("BonDriver"), ""); // 拡張子なし
        assert_eq!(path_find_extension("C:\\dir.x\\file"), ""); // ディレクトリのドットは無視
    }

    #[test]
    fn is_equal_file_name_ascii_ci() {
        assert!(is_equal_file_name("BonDriver.dll", "bondriver.DLL"));
        assert!(!is_equal_file_name("BonDriver_PT.dll", "BonDriver_PX.dll"));
    }

    #[test]
    fn wildcard_match_basics() {
        assert!(path_match_spec("BonDriver_PT.dll", "BonDriver*.dll"));
        assert!(path_match_spec("BonDriver_PT.dll", "*.dll"));
        assert!(!path_match_spec("BonDriver_PT.dll", "*.so"));
        assert!(path_match_spec("abc", "a?c"));
        assert!(!path_match_spec("abc", "a?d"));
        assert!(path_match_spec("abc", "*"));
        assert!(path_match_spec("", "*"));
        assert!(!path_match_spec("abc", "")); // 空パターンは空のみ一致
    }

    #[test]
    fn wildcard_match_case_insensitive_and_multi() {
        assert!(path_match_spec("BONDRIVER.DLL", "bondriver*"));
        // ';' 区切りの複数パターン。
        assert!(path_match_spec("x.so", "*.dll;*.so"));
        assert!(!path_match_spec("x.txt", "*.dll;*.so"));
    }

    #[test]
    fn wildcard_star_backtracking() {
        // 複数の '*' とバックトラック。
        assert!(path_match_spec("BonDriver_PT3-S0.dll", "Bon*_*-S0.dll"));
        assert!(!path_match_spec("BonDriver_PT3-S1.dll", "Bon*_*-S0.dll"));
    }

    #[test]
    fn sequel_first_file_name() {
        assert_eq!(
            sequel_driver_first_file_name("BonDriver_PT-S1.dll").as_deref(),
            Some("BonDriver_PT-S0.dll")
        );
        // 既に 0(先頭)→ None。
        assert_eq!(sequel_driver_first_file_name("BonDriver_PT-S0.dll"), None);
        // 直前が数字でない → None。
        assert_eq!(sequel_driver_first_file_name("BonDriver.dll"), None);
        // 拡張子なしで末尾が数字 → '0' 版。
        assert_eq!(
            sequel_driver_first_file_name("file1").as_deref(),
            Some("file0")
        );
        // 先頭が拡張子(`.dll`)→ None。
        assert_eq!(sequel_driver_first_file_name(".dll"), None);
    }

    #[test]
    fn driver_list_sort_and_find() {
        let mut mgr = DriverManager::new();
        mgr.add_driver_file("BonDriver_PT-S0.dll");
        mgr.add_driver_file("BonDriver_BDA.dll");
        mgr.add_driver_file("bonDriver_aaa.dll");
        mgr.sort_drivers();
        // 大小無視ソート: aaa, BDA, PT-S0。
        assert_eq!(mgr.get_driver_file(0), Some("bonDriver_aaa.dll"));
        assert_eq!(mgr.get_driver_file(1), Some("BonDriver_BDA.dll"));
        assert_eq!(mgr.get_driver_file(2), Some("BonDriver_PT-S0.dll"));
        assert_eq!(mgr.num_drivers(), 3);
        // 大小無視検索。
        assert_eq!(mgr.find_by_file_name("BONDRIVER_BDA.DLL"), Some(1));
        assert_eq!(mgr.find_by_file_name("missing.dll"), None);
        assert_eq!(mgr.find_by_file_name(""), None);
        mgr.clear();
        assert_eq!(mgr.num_drivers(), 0);
    }

    #[test]
    fn should_skip_sequel_driver() {
        let mut mgr = DriverManager::new();
        mgr.add_driver_file("BonDriver_PT-S0.dll");
        mgr.add_driver_file("BonDriver_PT-S1.dll");
        mgr.add_driver_file("BonDriver_PT-T0.dll");
        // S1 は S0 が在るのでスキップ対象。
        assert!(mgr.should_skip_as_sequel("BonDriver_PT-S1.dll"));
        // S0 自身はスキップしない。
        assert!(!mgr.should_skip_as_sequel("BonDriver_PT-S0.dll"));
        // T0 は連番先頭が無い(T-1 ではなく自身が 0)→ スキップしない。
        assert!(!mgr.should_skip_as_sequel("BonDriver_PT-T0.dll"));
        // 連番先頭が未登録ならスキップしない。
        assert!(!mgr.should_skip_as_sequel("BonDriver_XX-S1.dll"));
    }

    #[test]
    fn get_tuner_spec_matches() {
        let mut mgr = DriverManager::new();
        mgr.load_tuner_spec(&[
            (
                "BonDriver_BDA*".to_string(),
                "network|no-enum-channel".to_string(),
            ),
            ("*_File.dll".to_string(), "file|virtual".to_string()),
        ]);
        assert_eq!(mgr.tuner_spec_list().len(), 2);
        // フルパスでもファイル名部分でマッチ。
        assert_eq!(
            mgr.get_tuner_spec("C:\\dir\\BonDriver_BDA_PX.dll"),
            Some(TunerSpecFlag::NETWORK | TunerSpecFlag::NO_ENUM_CHANNEL)
        );
        assert_eq!(
            mgr.get_tuner_spec("BonDriver_Sample_File.dll"),
            Some(TunerSpecFlag::FILE | TunerSpecFlag::VIRTUAL)
        );
        // どのマスクにも一致しない。
        assert_eq!(mgr.get_tuner_spec("BonDriver_PT.dll"), None);
        // ファイル名部分が空。
        assert_eq!(mgr.get_tuner_spec("dir\\"), None);
    }
}
