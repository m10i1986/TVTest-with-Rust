/*
  TVTest
  Copyright(c) 2008-2020 DBCTRADO

  This program is free software; you can redistribute it and/or modify
  it under the terms of the GNU General Public License as published by
  the Free Software Foundation; either version 2 of the License, or
  (at your option) any later version.

  This program is distributed in the hope that it will be useful,
  but WITHOUT ANY WARRANTY; without even the implied warranty of
  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
  GNU General Public License for more details.

  You should have received a copy of the GNU General Public License
  along with this program; if not, write to the Free Software
  Foundation, Inc., 59 Temple Place, Suite 330, Boston, MA  02111-1307  USA
*/

//! TVTest `CIniFile` の中核(メモリ上の INI 解析・直列化・CRUD)を Rust に移植したもの。
//!
//! 原実装 (`src/IniFile.cpp`)。ファイル I/O・グローバルロック(`CreateFile`/`ReadFile`/
//! `WriteFile`/Mutex)は Win32 依存のため本クレートでは扱わず、別途 winutil 側で
//! テキストの読み書きラッパを設ける想定。本クレートは [`IniData`] として、
//! - INI テキストのパース([`IniData::parse`])
//! - セクション/値の追加・取得・削除([`IniData::set_value`] 等)
//! - テキストへの直列化([`IniData::serialize`])
//! を、原実装の `std::list` ベースの挙動(挿入位置・空行挿入など)を忠実に再現して提供する。
//!
//! セクション名・キー名の大小無視比較は、原実装の `IsEqualNoCase`(LibISDB::StringEqualsI)を
//! ASCII 範囲で近似する(INI のキーは実質 ASCII)。

/// INI のエントリ(キー=値、またはコメント/空行)。原実装 `CIniFile::CEntry`。
///
/// `name` が空のエントリは「コメント行」または「空行」を表し、値の検索対象にならない。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub value: String,
}

impl Entry {
    /// 名前と値を持つエントリを作る。原実装 `CEntry(pszName, pszValue)` (IniFile.cpp:525)。
    pub fn new(name: &str, value: &str) -> Self {
        Entry {
            name: name.to_string(),
            value: value.to_string(),
        }
    }

    /// 1 行のテキストからエントリを解釈する。原実装 `CEntry(const String&)` (IniFile.cpp:507)。
    ///
    /// - 先頭の空白を除いた最初の非空白が `;` ならコメント行(全体を value に保持、name は空)。
    /// - `=` があれば左を name(trim)、右を value とする。
    /// - `=` が無ければ全体を value とする(name は空)。
    pub fn from_text(text: &str) -> Self {
        let chars: Vec<char> = text.chars().collect();
        let first_non_ws = chars.iter().position(|&c| c != ' ' && c != '\t');

        if let Some(pos) = first_non_ws {
            if chars[pos] == ';' {
                return Entry {
                    name: String::new(),
                    value: text.to_string(),
                };
            }
        }

        if let Some(eq) = text.find('=') {
            let mut name = text[..eq].to_string();
            trim_in_place(&mut name);
            Entry {
                name,
                value: text[eq + 1..].to_string(),
            }
        } else {
            Entry {
                name: String::new(),
                value: text.to_string(),
            }
        }
    }
}

/// 1 つのセクション。原実装 `CIniFile::CSectionData`。
#[derive(Debug, Clone, Default)]
pub struct Section {
    pub name: String,
    pub entries: Vec<Entry>,
}

/// INI データ全体(メモリ表現)。原実装 `CIniFile` のうち in-memory 部分。
#[derive(Debug, Clone, Default)]
pub struct IniData {
    sections: Vec<Section>,
}

/// 前後の空白(半角スペース・タブ)を取り除く。原実装の `StringUtility::Trim`(既定 spaces=" \t")相当。
fn trim_in_place(s: &mut String) {
    let trimmed = s.trim_matches(|c| c == ' ' || c == '\t');
    if trimmed.len() != s.len() {
        *s = trimmed.to_string();
    }
}

/// ASCII 範囲での大小無視比較。原実装 `IsEqualNoCase` の近似。
fn eq_ignore_case(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

impl IniData {
    /// 空の INI データを作る。
    pub fn new() -> Self {
        IniData::default()
    }

    /// セクション一覧への参照。
    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    /// INI テキストを解析して取り込む。原実装 `CIniFile::Parse` (IniFile.cpp:397)。
    ///
    /// 行区切りは `\r` `\n`(CRLF/LF/CR 混在可)。`[名前]` 形式の行(長さ > 3 かつ
    /// `]` を含む)をセクション見出しとして扱い、それ以外をカレントセクションの
    /// エントリとして追加する。セクションが未作成なら空名セクションを作る。
    pub fn parse(&mut self, buffer: &str) {
        // 行に分割(CRLF/LF/CR をすべて区切りとみなす)。
        let mut chars = buffer.chars().peekable();
        let mut line = String::new();

        loop {
            match chars.next() {
                None => {
                    if !line.is_empty() {
                        self.parse_line(&line);
                    }
                    break;
                }
                Some('\r') => {
                    // 続く \n は同じ改行として消費。
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                    self.parse_line(&line);
                    line.clear();
                }
                Some('\n') => {
                    self.parse_line(&line);
                    line.clear();
                }
                Some(c) => line.push(c),
            }
        }
    }

    /// 1 行を解釈してデータへ反映する。`parse` の内部処理。
    fn parse_line(&mut self, line: &str) {
        let chars: Vec<char> = line.chars().collect();
        // 原実装: Length > 3 && Line[0]=='[' && find(']') が見つかる。
        if chars.len() > 3 && chars[0] == '[' {
            if let Some(end) = line.find(']') {
                // '[' の次から ']' の手前まで。
                let inner: String = line[1..end].to_string();
                let mut name = inner;
                trim_in_place(&mut name);
                self.create_section(&name);
                return;
            }
        }
        if self.sections.is_empty() {
            self.create_section("");
        }
        let last = self.sections.last_mut().unwrap();
        last.entries.push(Entry::from_text(line));
    }

    /// セクションを新規作成する。原実装 `CIniFile::CreateSection` (IniFile.cpp:485)。
    ///
    /// 名前付きセクションを追加する際、直前のセクションが名前付きで、その最後の
    /// エントリが空行でない(または空)場合に区切りの空行を 1 つ補う(直列化時の
    /// セクション間空行を再現するため)。
    fn create_section(&mut self, name: &str) {
        // 追加前に、直前セクションへ空行を補うか判定する。
        if !name.is_empty() && !self.sections.is_empty() {
            let prev = self.sections.last().unwrap();
            let need_blank = !prev.name.is_empty()
                && (prev.entries.is_empty()
                    || !prev.entries.last().unwrap().name.is_empty()
                    || !prev.entries.last().unwrap().value.is_empty());
            if need_blank {
                let idx = self.sections.len() - 1;
                self.sections[idx].entries.push(Entry::default());
            }
        }
        self.sections.push(Section {
            name: name.to_string(),
            entries: Vec::new(),
        });
    }

    /// セクションの位置を大小無視で検索する。原実装 `CIniFile::FindSection` (IniFile.cpp:431)。
    fn find_section(&self, name: &str) -> Option<usize> {
        if name.is_empty() {
            return None;
        }
        self.sections.iter().position(|s| eq_ignore_case(&s.name, name))
    }

    /// セクションが存在するか。原実装 `IsSectionExists` (IniFile.cpp:213)。
    pub fn is_section_exists(&self, name: &str) -> bool {
        self.find_section(name).is_some()
    }

    /// セクションを(無ければ作成して)選択し、その index を返す。
    /// 原実装 `SelectSection`(書き込み可前提)(IniFile.cpp:186)に相当する取得用。
    pub fn select_or_create_section(&mut self, name: &str) -> Option<usize> {
        if name.is_empty() {
            return None;
        }
        if let Some(idx) = self.find_section(name) {
            Some(idx)
        } else {
            self.create_section(name);
            Some(self.sections.len() - 1)
        }
    }

    /// セクションを削除する。原実装 `DeleteSection` (IniFile.cpp:221)。
    pub fn delete_section(&mut self, name: &str) -> bool {
        match self.find_section(name) {
            Some(idx) => {
                self.sections.remove(idx);
                true
            }
            None => false,
        }
    }

    /// セクション内の名前付きエントリ(キー=値)をすべて削除する(コメント行は残す)。
    /// 原実装 `ClearSection(pszSection)` (IniFile.cpp:241)。
    pub fn clear_section(&mut self, name: &str) -> bool {
        match self.find_section(name) {
            Some(idx) => {
                // 原実装は「Name が空でないものを remove」= 名前付きエントリを削除。
                self.sections[idx].entries.retain(|e| e.name.is_empty());
                true
            }
            None => false,
        }
    }

    /// 名前付きエントリの位置を大小無視で検索する。原実装 `CIniFile::FindValue` (IniFile.cpp:461)。
    fn find_value(entries: &[Entry], name: &str) -> Option<usize> {
        entries
            .iter()
            .position(|e| !e.name.is_empty() && eq_ignore_case(&e.name, name))
    }

    /// セクション内のキーの値を取得する。原実装 `GetValue` (IniFile.cpp:264)。
    ///
    /// 値が `"..."` で囲まれていれば、その囲みを取り除く(原実装どおり)。
    pub fn get_value(&self, section: &str, name: &str) -> Option<String> {
        if name.is_empty() {
            return None;
        }
        let sidx = self.find_section(section)?;
        let entries = &self.sections[sidx].entries;
        let vidx = Self::find_value(entries, name)?;
        let value = &entries[vidx].value;

        let chars: Vec<char> = value.chars().collect();
        if chars.len() >= 2 && chars[0] == '"' && chars[chars.len() - 1] == '"' {
            if chars.len() > 2 {
                Some(chars[1..chars.len() - 1].iter().collect())
            } else {
                Some(String::new())
            }
        } else {
            Some(value.clone())
        }
    }

    /// セクションにキー=値を設定する。原実装 `SetValue` (IniFile.cpp:297)。
    ///
    /// 既存キーがあれば値を更新。無ければ「最後の名前付きエントリの直後」に挿入する
    /// (末尾のコメント/空行より前に入れる原実装の挙動を再現)。名前付きが 1 つも
    /// 無ければ先頭に挿入する。セクションは無ければ作成する。
    pub fn set_value(&mut self, section: &str, name: &str, value: &str) -> bool {
        if section.is_empty() || name.is_empty() {
            return false;
        }
        let sidx = match self.select_or_create_section(section) {
            Some(i) => i,
            None => return false,
        };
        let entries = &mut self.sections[sidx].entries;

        if let Some(vidx) = Self::find_value(entries, name) {
            entries[vidx].value = value.to_string();
            return true;
        }

        // 末尾から見て最初の名前付きエントリの「直後」に挿入。
        let mut insert_at = None;
        for i in (0..entries.len()).rev() {
            if !entries[i].name.is_empty() {
                insert_at = Some(i + 1);
                break;
            }
        }
        match insert_at {
            Some(pos) => entries.insert(pos, Entry::new(name, value)),
            None => entries.insert(0, Entry::new(name, value)),
        }
        true
    }

    /// キーが存在するか。原実装 `IsValueExists` (IniFile.cpp:331)。
    pub fn is_value_exists(&self, section: &str, name: &str) -> bool {
        if name.is_empty() {
            return false;
        }
        match self.find_section(section) {
            Some(sidx) => Self::find_value(&self.sections[sidx].entries, name).is_some(),
            None => false,
        }
    }

    /// キーを削除する。原実装 `DeleteValue` (IniFile.cpp:345)。
    pub fn delete_value(&mut self, section: &str, name: &str) -> bool {
        if name.is_empty() {
            return false;
        }
        let sidx = match self.find_section(section) {
            Some(i) => i,
            None => return false,
        };
        let entries = &mut self.sections[sidx].entries;
        match Self::find_value(entries, name) {
            Some(vidx) => {
                entries.remove(vidx);
                true
            }
            None => false,
        }
    }

    /// セクション内の名前付きエントリ一覧を返す(コメント/空行は除く)。
    /// 原実装 `GetSectionEntries` (IniFile.cpp:365)。
    pub fn get_section_entries(&self, section: &str) -> Option<Vec<Entry>> {
        let sidx = self.find_section(section)?;
        Some(
            self.sections[sidx]
                .entries
                .iter()
                .filter(|e| !e.name.is_empty())
                .cloned()
                .collect(),
        )
    }

    /// 全体を INI テキストへ直列化する。原実装 `Close` の書き出しロジック (IniFile.cpp:134)。
    ///
    /// 改行は `\r\n`。名前付きエントリは `Name=Value`、名前なし(コメント/空行)は
    /// `Value` のみを出力する。空名セクションは見出し行を出さない。
    pub fn serialize(&self) -> String {
        let mut buffer = String::new();
        for section in &self.sections {
            if !section.name.is_empty() {
                buffer.push('[');
                buffer.push_str(&section.name);
                buffer.push_str("]\r\n");
            }
            for entry in &section.entries {
                if !entry.name.is_empty() {
                    buffer.push_str(&entry.name);
                    buffer.push('=');
                }
                buffer.push_str(&entry.value);
                buffer.push_str("\r\n");
            }
        }
        buffer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entry_from_text() {
        let e = Entry::from_text("key=value");
        assert_eq!(e.name, "key");
        assert_eq!(e.value, "value");

        // 名前は trim される
        let e = Entry::from_text("  key  =value");
        assert_eq!(e.name, "key");
        assert_eq!(e.value, "value");

        // コメント行
        let e = Entry::from_text("  ; comment");
        assert_eq!(e.name, "");
        assert_eq!(e.value, "  ; comment");

        // = が無い行
        let e = Entry::from_text("plain text");
        assert_eq!(e.name, "");
        assert_eq!(e.value, "plain text");

        // 値に = を含む
        let e = Entry::from_text("url=http://a?b=c");
        assert_eq!(e.name, "url");
        assert_eq!(e.value, "http://a?b=c");
    }

    #[test]
    fn test_parse_basic() {
        let mut ini = IniData::new();
        ini.parse("[Section1]\r\nkey1=value1\r\nkey2=value2\r\n[Section2]\r\nkeyA=valueA\r\n");

        assert!(ini.is_section_exists("Section1"));
        assert!(ini.is_section_exists("Section2"));
        assert_eq!(ini.get_value("Section1", "key1").as_deref(), Some("value1"));
        assert_eq!(ini.get_value("Section1", "key2").as_deref(), Some("value2"));
        assert_eq!(ini.get_value("Section2", "keyA").as_deref(), Some("valueA"));
    }

    #[test]
    fn test_parse_lf_only() {
        let mut ini = IniData::new();
        ini.parse("[Sec]\nk=v\n");
        assert_eq!(ini.get_value("Sec", "k").as_deref(), Some("v"));
    }

    #[test]
    fn test_short_section_name_is_not_header() {
        // 原実装は Length > 3 をセクション見出しの条件とするため、
        // "[S]"(3文字)は見出しにならず通常エントリ扱いになる。
        let mut ini = IniData::new();
        ini.parse("[S]\nk=v\n");
        assert!(!ini.is_section_exists("S"));
    }

    #[test]
    fn test_section_case_insensitive() {
        let mut ini = IniData::new();
        ini.parse("[Section]\nKey=Value\n");
        // セクション・キーとも大小無視
        assert_eq!(ini.get_value("SECTION", "KEY").as_deref(), Some("Value"));
        assert!(ini.is_section_exists("section"));
    }

    #[test]
    fn test_get_value_dequote() {
        let mut ini = IniData::new();
        ini.parse("[Sec]\nk=\"quoted\"\nempty=\"\"\n");
        assert_eq!(ini.get_value("Sec", "k").as_deref(), Some("quoted"));
        // "" は空文字列に
        assert_eq!(ini.get_value("Sec", "empty").as_deref(), Some(""));
    }

    #[test]
    fn test_set_value_new_and_update() {
        let mut ini = IniData::new();
        ini.parse("[Sec]\nexisting=1\n");

        // 既存キー更新
        assert!(ini.set_value("Sec", "existing", "2"));
        assert_eq!(ini.get_value("Sec", "existing").as_deref(), Some("2"));

        // 新規キー
        assert!(ini.set_value("Sec", "newkey", "x"));
        assert_eq!(ini.get_value("Sec", "newkey").as_deref(), Some("x"));

        // 新規セクション + キー
        assert!(ini.set_value("NewSec", "a", "b"));
        assert_eq!(ini.get_value("NewSec", "a").as_deref(), Some("b"));
    }

    #[test]
    fn test_set_value_insert_before_trailing_comment() {
        let mut ini = IniData::new();
        // 末尾にコメント行があるセクション
        ini.parse("[Sec]\nk1=v1\n; trailing comment\n");
        ini.set_value("Sec", "k2", "v2");

        // k2 はコメント行より前に入る(名前付きエントリの直後)。
        let entries = &ini.sections()[ini.find_section("Sec").unwrap()].entries;
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        let k1_pos = names.iter().position(|&n| n == "k1").unwrap();
        let k2_pos = names.iter().position(|&n| n == "k2").unwrap();
        assert_eq!(k2_pos, k1_pos + 1);
        // 最後はコメント行(name 空)のまま。
        assert_eq!(entries.last().unwrap().name, "");
    }

    #[test]
    fn test_delete_value_and_section() {
        let mut ini = IniData::new();
        ini.parse("[Sec]\nk1=v1\nk2=v2\n[Tee]\na=b\n");

        assert!(ini.delete_value("Sec", "k1"));
        assert!(!ini.is_value_exists("Sec", "k1"));
        assert!(ini.is_value_exists("Sec", "k2"));

        assert!(ini.delete_section("Tee"));
        assert!(!ini.is_section_exists("Tee"));

        // 存在しないものは false
        assert!(!ini.delete_value("Sec", "nope"));
        assert!(!ini.delete_section("Nope"));
    }

    #[test]
    fn test_clear_section_keeps_comments() {
        let mut ini = IniData::new();
        ini.parse("[Sec]\n; keep me\nk1=v1\nk2=v2\n");
        assert!(ini.clear_section("Sec"));
        // 名前付きは消え、コメントは残る
        assert!(!ini.is_value_exists("Sec", "k1"));
        let entries = &ini.sections()[ini.find_section("Sec").unwrap()].entries;
        assert!(entries.iter().any(|e| e.value.contains("keep me")));
    }

    #[test]
    fn test_get_section_entries() {
        let mut ini = IniData::new();
        ini.parse("[Sec]\n; comment\nk1=v1\nk2=v2\n");
        let entries = ini.get_section_entries("Sec").unwrap();
        // コメントは除かれ、2件
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "k1");
        assert_eq!(entries[1].name, "k2");
    }

    #[test]
    fn test_serialize_roundtrip() {
        let original = "[Section1]\r\nkey1=value1\r\nkey2=value2\r\n[Section2]\r\nkeyA=valueA\r\n";
        let mut ini = IniData::new();
        ini.parse(original);
        let serialized = ini.serialize();
        // 再パースして同じ値が得られる
        let mut ini2 = IniData::new();
        ini2.parse(&serialized);
        assert_eq!(ini2.get_value("Section1", "key1").as_deref(), Some("value1"));
        assert_eq!(ini2.get_value("Section2", "keyA").as_deref(), Some("valueA"));
    }

    #[test]
    fn test_serialize_section_headers() {
        let mut ini = IniData::new();
        ini.set_value("S", "k", "v");
        let s = ini.serialize();
        assert!(s.contains("[S]\r\n"));
        assert!(s.contains("k=v\r\n"));
    }
}
