//! TVTest の番組表外部ツール(`src/ProgramGuideTool.cpp` / `ProgramGuideTool.h`)の純粋ロジックを
//! 移植したクレート。
//!
//! 移植対象:
//! - コマンド文字列からの実行ファイル名抽出(`CProgramGuideTool::GetCommandFileName`)。引用符
//!   または空白区切り、サロゲートペア対応のマルチバイト文字長(`StringCharLength`)。
//! - ツール(名前 + コマンド)モデル(`CProgramGuideTool`)と `GetPath`。
//! - ツールリスト管理(`CProgramGuideToolList`)。
//! - EPG 変数のパラメータ表と整数キーワードの文字列化(`CEpgVariableStringMap` の純粋部分)。
//!
//! ダイアログ(`DlgProc`)・アイコン取得(`SHGetFileInfo`)・`ShellExecute`・iEPG ファイル書き出し・
//! `VariableString` 展開(Win32/I-O)は対象外。文字列は原実装の `wchar_t` に合わせ `&[u16]` で扱う。

const QUOTE: u16 = b'"' as u16;
const SPACE: u16 = b' ' as u16;

/// 文字列先頭 1 文字のコード単位数(StringUtility.h:62-87 の `StringCharLength`/`StringNextChar`)。
///
/// 空 or ヌル(`0`)は 0、上位サロゲート(`0xD800..=0xDBFF`)に下位サロゲート(`0xDC00..=0xDFFF`)が
/// 続けばサロゲートペアで 2、それ以外は 1。`CharNextW` のヌルで進まない(長さ 0)挙動を再現する。
pub fn first_char_len(s: &[u16]) -> usize {
    match s.first() {
        None | Some(&0) => 0,
        Some(&c) => {
            if (0xD800..=0xDBFF).contains(&c)
                && matches!(s.get(1), Some(&low) if (0xDC00..=0xDFFF).contains(&low))
            {
                2
            } else {
                1
            }
        }
    }
}

/// コマンド文字列から実行ファイル名を抽出(ProgramGuideTool.cpp:209-237 の `GetCommandFileName`)。
///
/// 先頭が `"` ならそこから次の `"` まで、そうでなければ最初の空白までを実行ファイル名とする。
/// `max_file_name` は格納バッファのサイズ(ヌル終端を含む)で、`length + char_len >= max_file_name`
/// になる(= ヌル分の空きが無くなる)と長すぎとして `None` を返す。不正な文字(長さ 0)でも `None`。
/// 成功時は `(実行ファイル名, 残り(パラメータ部)開始インデックス)` を返す。区切り文字は消費される。
pub fn parse_command_file_name(command: &[u16], max_file_name: usize) -> Option<(Vec<u16>, usize)> {
    let mut pos = 0;
    let delimiter = if command.first() == Some(&QUOTE) {
        pos += 1;
        QUOTE
    } else {
        SPACE
    };

    let mut file_name: Vec<u16> = Vec::new();
    let mut length = 0usize;
    loop {
        match command.get(pos) {
            None | Some(&0) => break,
            Some(&c) if c == delimiter => break,
            _ => {}
        }
        let char_len = first_char_len(&command[pos..]);
        if char_len == 0 || length + char_len >= max_file_name {
            return None;
        }
        file_name.extend_from_slice(&command[pos..pos + char_len]);
        pos += char_len;
        length += char_len;
    }
    if command.get(pos) == Some(&delimiter) {
        pos += 1;
    }
    Some((file_name, pos))
}

/// 番組表外部ツール(ProgramGuideTool.h:38-73 の `CProgramGuideTool`)。
///
/// 名前と起動コマンド文字列(実行ファイル + パラメータ)を保持する。文字列は `wchar` 慣習で
/// `Vec<u16>`。アイコン・ダイアログ・実行(`Execute`)は Win32/I-O のため対象外。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProgramGuideTool {
    name: Vec<u16>,
    command: Vec<u16>,
}

impl ProgramGuideTool {
    /// 名前とコマンドから生成。
    pub fn new(name: &[u16], command: &[u16]) -> Self {
        Self {
            name: name.to_vec(),
            command: command.to_vec(),
        }
    }

    /// `&str` から生成する補助コンストラクタ(UTF-16 へ変換)。
    pub fn from_text(name: &str, command: &str) -> Self {
        Self {
            name: name.encode_utf16().collect(),
            command: command.encode_utf16().collect(),
        }
    }

    /// 名前(`GetName`)。
    pub fn name(&self) -> &[u16] {
        &self.name
    }

    /// コマンド文字列(`GetCommand`)。
    pub fn command(&self) -> &[u16] {
        &self.command
    }

    /// コマンドから実行ファイル名を取得(ProgramGuideTool.cpp:135-140 の `GetPath`)。
    ///
    /// `max_length` はバッファサイズ(ヌル終端含む)。長すぎ/不正なら `None`。
    pub fn get_path(&self, max_length: usize) -> Option<Vec<u16>> {
        parse_command_file_name(&self.command, max_length).map(|(file_name, _)| file_name)
    }
}

/// 番組表外部ツールのリスト(ProgramGuideTool.h:75-90 の `CProgramGuideToolList`)。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProgramGuideToolList {
    tools: Vec<ProgramGuideTool>,
}

impl ProgramGuideToolList {
    /// 空のリストを生成。
    pub fn new() -> Self {
        Self::default()
    }

    /// 全削除(`Clear`)。
    pub fn clear(&mut self) {
        self.tools.clear();
    }

    /// ツールを末尾に追加(`Add`)。常に `true`(C++ の null 拒否は Rust では発生しない)。
    pub fn add(&mut self, tool: ProgramGuideTool) -> bool {
        self.tools.push(tool);
        true
    }

    /// インデックス指定でツール取得(`GetTool`)。範囲外は `None`。
    pub fn get_tool(&self, index: usize) -> Option<&ProgramGuideTool> {
        self.tools.get(index)
    }

    /// インデックス指定でツール取得(可変)。
    pub fn get_tool_mut(&mut self, index: usize) -> Option<&mut ProgramGuideTool> {
        self.tools.get_mut(index)
    }

    /// ツール数(`NumTools`)。
    pub fn num_tools(&self) -> usize {
        self.tools.len()
    }

    /// 全ツール。
    pub fn tools(&self) -> &[ProgramGuideTool] {
        &self.tools
    }
}

/// 番組の長さ(秒)→分(ProgramGuideTool.cpp:95 の `(Duration + 59) / 60`、切り上げ)。
///
/// 原実装の `(Duration + 59) / 60` と等価な切り上げ除算(通常範囲で挙動一致・オーバーフロー安全)。
pub fn duration_min(duration_sec: u32) -> u32 {
    duration_sec.div_ceil(60)
}

/// EPG 変数のパラメータ表(キーワード, 説明)(ProgramGuideTool.cpp:56-64 の `m_EpgParameterList`)。
///
/// `eid`/`sid` は原実装でコメントアウト(`GetLocalString` では処理されるが一覧には出さない)。
pub static EPG_PARAMETER_LIST: [(&str, &str); 5] = [
    ("nid", "ネットワークID"),
    ("tsid", "ストリームID"),
    ("tvpid", "iEPGファイル"),
    ("duration-sec", "番組の長さ(秒単位)"),
    ("duration-min", "番組の長さ(分単位)"),
];

/// EPG イベントの数値(`format_epg_keyword` 用)。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EpgEventValues {
    /// イベント ID(`eid`)。
    pub event_id: u16,
    /// ネットワーク ID(`nid`)。
    pub network_id: u16,
    /// トランスポートストリーム ID(`tsid`)。
    pub transport_stream_id: u16,
    /// サービス ID(`sid`)。
    pub service_id: u16,
    /// 番組の長さ(秒)。
    pub duration_sec: u32,
}

/// EPG 変数キーワードを文字列化(ProgramGuideTool.cpp:73-101 の `GetLocalString` の整数キーワード部)。
///
/// キーワードは大小無視で比較(`lstrcmpi` 相当)。`eid`/`nid`/`tsid`/`sid`/`duration-sec`/
/// `duration-min` を処理する。`tvpid`(iEPG ファイルパス = アプリディレクトリ依存)と未知の
/// キーワードは `None`(呼び出し側で `CEventVariableStringMap` へフォールバック)。
pub fn format_epg_keyword(keyword: &str, values: &EpgEventValues) -> Option<String> {
    if keyword.eq_ignore_ascii_case("eid") {
        Some(values.event_id.to_string())
    } else if keyword.eq_ignore_ascii_case("nid") {
        Some(values.network_id.to_string())
    } else if keyword.eq_ignore_ascii_case("tsid") {
        Some(values.transport_stream_id.to_string())
    } else if keyword.eq_ignore_ascii_case("sid") {
        Some(values.service_id.to_string())
    } else if keyword.eq_ignore_ascii_case("duration-sec") {
        Some(values.duration_sec.to_string())
    } else if keyword.eq_ignore_ascii_case("duration-min") {
        Some(duration_min(values.duration_sec).to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u16s(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn from_u16(s: &[u16]) -> String {
        String::from_utf16(s).unwrap()
    }

    #[test]
    fn first_char_len_cases() {
        assert_eq!(first_char_len(&[]), 0);
        assert_eq!(first_char_len(&[0]), 0);
        assert_eq!(first_char_len(&u16s("a")), 1);
        // サロゲートペア(𠮷 = U+20BB7)。
        let surrogate = u16s("𠮷");
        assert_eq!(surrogate.len(), 2);
        assert_eq!(first_char_len(&surrogate), 2);
        // 孤立した上位サロゲート(後続が下位サロゲートでない)→ 1。
        assert_eq!(first_char_len(&[0xD842, b'x' as u16]), 1);
        assert_eq!(first_char_len(&[0xD842]), 1);
    }

    #[test]
    fn parse_unquoted_with_args() {
        let cmd = u16s("notepad.exe arg1 arg2");
        let (file, rest) = parse_command_file_name(&cmd, 260).unwrap();
        assert_eq!(from_u16(&file), "notepad.exe");
        assert_eq!(from_u16(&cmd[rest..]), "arg1 arg2");
    }

    #[test]
    fn parse_quoted_path_with_spaces() {
        let cmd = u16s("\"C:\\Program Files\\app.exe\" /a /b");
        let (file, rest) = parse_command_file_name(&cmd, 260).unwrap();
        assert_eq!(from_u16(&file), "C:\\Program Files\\app.exe");
        assert_eq!(from_u16(&cmd[rest..]), " /a /b");
    }

    #[test]
    fn parse_no_args() {
        let cmd = u16s("tool.exe");
        let (file, rest) = parse_command_file_name(&cmd, 260).unwrap();
        assert_eq!(from_u16(&file), "tool.exe");
        assert_eq!(rest, cmd.len()); // 残りなし
    }

    #[test]
    fn parse_empty_command() {
        let cmd: Vec<u16> = Vec::new();
        let (file, rest) = parse_command_file_name(&cmd, 260).unwrap();
        assert!(file.is_empty());
        assert_eq!(rest, 0);
    }

    #[test]
    fn parse_too_long_returns_none() {
        let cmd = u16s("abcdef arg");
        // バッファ 6 → 最大 5 文字。"abcdef" は 6 文字目でヌル余地なし → None。
        assert!(parse_command_file_name(&cmd, 6).is_none());
        // バッファ 7 → 6 文字 + ヌルで収まる。
        let (file, _) = parse_command_file_name(&cmd, 7).unwrap();
        assert_eq!(from_u16(&file), "abcdef");
    }

    #[test]
    fn parse_keeps_surrogate_pair_intact() {
        // ファイル名にサロゲートペアを含む。
        let cmd = u16s("𠮷野家.exe param");
        let (file, rest) = parse_command_file_name(&cmd, 260).unwrap();
        assert_eq!(from_u16(&file), "𠮷野家.exe");
        assert_eq!(from_u16(&cmd[rest..]), "param");
    }

    #[test]
    fn tool_get_path() {
        let tool = ProgramGuideTool::from_text("メモ帳", "\"C:\\Windows\\notepad.exe\" %file%");
        assert_eq!(from_u16(tool.name()), "メモ帳");
        let path = tool.get_path(260).unwrap();
        assert_eq!(from_u16(&path), "C:\\Windows\\notepad.exe");
    }

    #[test]
    fn tool_get_path_too_long() {
        let tool = ProgramGuideTool::from_text("t", "abcdefghij");
        assert!(tool.get_path(5).is_none());
    }

    #[test]
    fn tool_list_crud() {
        let mut list = ProgramGuideToolList::new();
        assert_eq!(list.num_tools(), 0);
        assert!(list.add(ProgramGuideTool::from_text("A", "a.exe")));
        assert!(list.add(ProgramGuideTool::from_text("B", "b.exe")));
        assert_eq!(list.num_tools(), 2);
        assert_eq!(from_u16(list.get_tool(0).unwrap().name()), "A");
        assert_eq!(from_u16(list.get_tool(1).unwrap().command()), "b.exe");
        assert!(list.get_tool(2).is_none());
        // 可変取得で書き換え。
        *list.get_tool_mut(0).unwrap() = ProgramGuideTool::from_text("A2", "a2.exe");
        assert_eq!(from_u16(list.get_tool(0).unwrap().name()), "A2");
        list.clear();
        assert_eq!(list.num_tools(), 0);
    }

    #[test]
    fn tool_list_clone_is_deep() {
        let mut list = ProgramGuideToolList::new();
        list.add(ProgramGuideTool::from_text("A", "a.exe"));
        let cloned = list.clone();
        list.get_tool_mut(0).unwrap().command.clear();
        // クローンは独立(deep copy = operator=)。
        assert_eq!(from_u16(cloned.get_tool(0).unwrap().command()), "a.exe");
    }

    #[test]
    fn duration_min_rounds_up() {
        assert_eq!(duration_min(0), 0);
        assert_eq!(duration_min(1), 1);
        assert_eq!(duration_min(60), 1);
        assert_eq!(duration_min(61), 2);
        assert_eq!(duration_min(3600), 60);
    }

    #[test]
    fn epg_keyword_formatting() {
        let values = EpgEventValues {
            event_id: 1234,
            network_id: 32736,
            transport_stream_id: 16625,
            service_id: 1024,
            duration_sec: 1830,
        };
        assert_eq!(format_epg_keyword("eid", &values).as_deref(), Some("1234"));
        assert_eq!(format_epg_keyword("NID", &values).as_deref(), Some("32736")); // 大小無視
        assert_eq!(
            format_epg_keyword("tsid", &values).as_deref(),
            Some("16625")
        );
        assert_eq!(format_epg_keyword("sid", &values).as_deref(), Some("1024"));
        assert_eq!(
            format_epg_keyword("duration-sec", &values).as_deref(),
            Some("1830")
        );
        assert_eq!(
            format_epg_keyword("Duration-Min", &values).as_deref(),
            Some("31")
        ); // 切り上げ
           // tvpid と未知はフォールバック。
        assert_eq!(format_epg_keyword("tvpid", &values), None);
        assert_eq!(format_epg_keyword("unknown", &values), None);
    }

    #[test]
    fn epg_parameter_list_contents() {
        assert_eq!(EPG_PARAMETER_LIST.len(), 5);
        assert_eq!(EPG_PARAMETER_LIST[0].0, "nid");
        assert_eq!(EPG_PARAMETER_LIST[2].0, "tvpid");
        assert_eq!(EPG_PARAMETER_LIST[4].0, "duration-min");
    }
}
