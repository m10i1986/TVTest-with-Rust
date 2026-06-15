// TVTest の VariableManager.cpp を Rust へ移植したもの。
//
// 移植対象:
//   - VariableFlag enum       : CVariableManager::VariableFlag (VariableManager.h:35)
//   - GetVariable trait       : CVariableManager::IGetVariable (VariableManager.h:48)
//   - VariableManager 構造体  : CVariableManager (VariableManager.h:32)
//     - register_variable     : CVariableManager::RegisterVariable:32
//     - get_variable          : CVariableManager::GetVariable:60
//     - get_preferred_variable: CVariableManager::GetPreferredVariable:85
//     - get_variable_list     : CVariableManager::GetVariableList:110
//
// IGetVariable は Rust の trait で抽象化。
// キーワードは原実装どおり登録時に小文字化して保持。
// 内部ストレージは BTreeMap(std::set の代替)でキーワード昇順。

use std::collections::BTreeMap;
/// ASCII 文字の小文字化。CharLowerBuff(Win32)の ASCII 範囲近似。
fn ascii_to_lower_u16(s: &mut Vec<u16>) {
    for c in s.iter_mut() {
        if *c >= u16::from(b'A') && *c <= u16::from(b'Z') {
            *c += 32;
        }
    }
}

bitflags::bitflags! {
    /// 原実装 CVariableManager::VariableFlag (VariableManager.h:35)。
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct VariableFlag: u32 {
        const OVERRIDE = 0x0001;
    }
}

/// 原実装 CVariableManager::IGetVariable (VariableManager.h:48)。
pub trait GetVariable: Send + Sync {
    fn get_variable(&self, keyword: &[u16]) -> Option<Vec<u16>>;
}

/// 変数エントリの情報(読み取り専用ビュー)。
#[derive(Debug, Clone)]
pub struct VariableInfo {
    pub keyword: Vec<u16>,
    pub flags: VariableFlag,
    pub description: Vec<u16>,
}

struct VariableEntry {
    keyword: Vec<u16>,
    value: Vec<u16>,
    getter: Option<Box<dyn GetVariable>>,
    description: Vec<u16>,
    flags: VariableFlag,
}

/// キーワード→値マップ。原実装 CVariableManager (VariableManager.h:32)。
///
/// キーワードは登録時に ASCII 小文字化して保持。BTreeMap でアルファベット順に管理。
#[derive(Default)]
pub struct VariableManager {
    variables: BTreeMap<Vec<u16>, VariableEntry>,
}

impl VariableManager {
    pub fn new() -> Self {
        VariableManager {
            variables: BTreeMap::new(),
        }
    }

    /// 変数を登録する。原実装 RegisterVariable:32。
    ///
    /// `keyword` が空なら false。同じキーワードが既存なら上書き。
    /// `getter` が Some なら値取得時に呼び出す。None なら `value` を直接返す。
    pub fn register_variable(
        &mut self,
        keyword: &[u16],
        value: &[u16],
        getter: Option<Box<dyn GetVariable>>,
        description: &[u16],
        flags: VariableFlag,
    ) -> bool {
        if keyword.is_empty() {
            return false;
        }
        let mut key = keyword.to_vec();
        ascii_to_lower_u16(&mut key);

        let entry = VariableEntry {
            keyword: key.clone(),
            value: value.to_vec(),
            getter,
            description: description.to_vec(),
            flags,
        };
        self.variables.insert(key, entry);
        true
    }

    /// 変数値を取得する。原実装 GetVariable:60。
    pub fn get_variable(&self, keyword: &[u16]) -> Option<Vec<u16>> {
        let mut key = keyword.to_vec();
        ascii_to_lower_u16(&mut key);
        let entry = self.variables.get(&key)?;
        if let Some(getter) = &entry.getter {
            getter.get_variable(keyword)
        } else {
            Some(entry.value.clone())
        }
    }

    /// Override フラグが立っている変数値を取得する。原実装 GetPreferredVariable:85。
    pub fn get_preferred_variable(&self, keyword: &[u16]) -> Option<Vec<u16>> {
        let mut key = keyword.to_vec();
        ascii_to_lower_u16(&mut key);
        let entry = self.variables.get(&key)?;
        if !entry.flags.contains(VariableFlag::OVERRIDE) {
            return None;
        }
        if let Some(getter) = &entry.getter {
            getter.get_variable(keyword)
        } else {
            Some(entry.value.clone())
        }
    }

    /// 登録済み変数の一覧を返す。原実装 GetVariableList:110。
    pub fn get_variable_list(&self) -> Vec<VariableInfo> {
        self.variables
            .values()
            .map(|e| VariableInfo {
                keyword: e.keyword.clone(),
                flags: e.flags,
                description: e.description.clone(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }
    fn s(v: &[u16]) -> String {
        String::from_utf16_lossy(v).to_owned()
    }

    #[test]
    fn test_register_and_get() {
        let mut vm = VariableManager::new();
        assert!(vm.register_variable(&w("Channel"), &w("NHK"), None, &w("チャンネル名"), VariableFlag::empty()));
        // キーワードは大小無視。
        let v = vm.get_variable(&w("channel")).unwrap();
        assert_eq!(s(&v), "NHK");
        let v = vm.get_variable(&w("CHANNEL")).unwrap();
        assert_eq!(s(&v), "NHK");
    }

    #[test]
    fn test_register_empty_keyword_fails() {
        let mut vm = VariableManager::new();
        assert!(!vm.register_variable(&[], &w("X"), None, &[], VariableFlag::empty()));
    }

    #[test]
    fn test_overwrite_existing() {
        let mut vm = VariableManager::new();
        vm.register_variable(&w("key"), &w("old"), None, &[], VariableFlag::empty());
        vm.register_variable(&w("key"), &w("new"), None, &[], VariableFlag::empty());
        let v = vm.get_variable(&w("key")).unwrap();
        assert_eq!(s(&v), "new");
    }

    #[test]
    fn test_get_variable_not_found() {
        let vm = VariableManager::new();
        assert!(vm.get_variable(&w("missing")).is_none());
    }

    struct MockGetter(String);
    impl GetVariable for MockGetter {
        fn get_variable(&self, _keyword: &[u16]) -> Option<Vec<u16>> {
            Some(self.0.encode_utf16().collect())
        }
    }

    #[test]
    fn test_getter_callback() {
        let mut vm = VariableManager::new();
        vm.register_variable(
            &w("Dynamic"),
            &[],
            Some(Box::new(MockGetter("dynamic_value".to_string()))),
            &[],
            VariableFlag::empty(),
        );
        let v = vm.get_variable(&w("dynamic")).unwrap();
        assert_eq!(s(&v), "dynamic_value");
    }

    #[test]
    fn test_get_preferred_variable_override() {
        let mut vm = VariableManager::new();
        vm.register_variable(&w("key1"), &w("v1"), None, &[], VariableFlag::OVERRIDE);
        vm.register_variable(&w("key2"), &w("v2"), None, &[], VariableFlag::empty());

        let v = vm.get_preferred_variable(&w("key1")).unwrap();
        assert_eq!(s(&v), "v1");
        assert!(vm.get_preferred_variable(&w("key2")).is_none());
    }

    #[test]
    fn test_get_variable_list_sorted() {
        let mut vm = VariableManager::new();
        vm.register_variable(&w("bbb"), &w("B"), None, &w("desc B"), VariableFlag::empty());
        vm.register_variable(&w("aaa"), &w("A"), None, &w("desc A"), VariableFlag::empty());
        vm.register_variable(&w("ccc"), &w("C"), None, &w("desc C"), VariableFlag::empty());

        let list = vm.get_variable_list();
        assert_eq!(list.len(), 3);
        // BTreeMap なので aaa/bbb/ccc の順。
        assert_eq!(s(&list[0].keyword), "aaa");
        assert_eq!(s(&list[1].keyword), "bbb");
        assert_eq!(s(&list[2].keyword), "ccc");
    }

    #[test]
    fn test_description_stored() {
        let mut vm = VariableManager::new();
        vm.register_variable(&w("X"), &w("val"), None, &w("説明"), VariableFlag::empty());
        let list = vm.get_variable_list();
        assert_eq!(s(&list[0].description), "説明");
    }
}
