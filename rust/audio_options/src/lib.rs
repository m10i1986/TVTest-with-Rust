//! TVTest の音声設定(`src/AudioOptions.cpp` / `AudioOptions.h`)の純粋ロジックを
//! 移植したクレート。
//!
//! 移植対象:
//! - 音声言語優先順位(`AudioLanguageInfo`)のエンコード/デコード(リストボックス ItemData の Param)。
//! - 言語優先順位の設定文字列(`LangPriority{n}`)のパース・整形(`ReadSettings`/`WriteSettings`)。
//! - 言語コードテーブル(`m_AudioLanguageList`)。
//! - サラウンドミキシング行列・ダウンミックス行列の設定文字列パース・整形(`FormatDouble` 含む)。
//! - 既定のサラウンドミキシング行列・ダウンミックス行列。
//!
//! 対象外(Win32 / DirectShow / LibISDB / CSettings 依存):
//! - `CAudioOptions::DlgProc` / `CSurroundOptionsDialog`(ダイアログ)。
//! - `ReadSettings`/`WriteSettings` の `CSettings` I/O 本体(本クレートは値の変換のみ提供)。
//! - `ApplyMediaViewerOptions`(`ViewerFilter`)、音声デバイス/フィルタ列挙(`DeviceEnumerator`)。
//! - `GetLanguageText`(`LibISDB::GetLanguageText_ja`)。
//! - `SPDIFOptions` / `AACDecoderType` の設定(単純な int 読み書きで純粋ロジックなし)。

#![forbid(unsafe_code)]
// PSQR(1/√2)は原実装(AudioOptions.cpp:70)のリテラルをそのまま保持するため、
// 既知定数への近似(approx_constant)と f64 の表現精度超過(excessive_precision)を許可する。
#![allow(clippy::approx_constant)]
#![allow(clippy::excessive_precision)]

/// 副音声フラグ。リストボックス ItemData / Param の最上位寄りビット。AudioOptions.h:90
pub const LANGUAGE_FLAG_SUB: u32 = 0x0100_0000;

/// 言語コード(`LibISDB::LANGUAGE_CODE_*`、3 文字 ISO 639 コードを 24bit にパックした値)。
/// 値の出典: `src/LibISDB/LibISDB/LibISDBConsts.hpp`。
pub const LANGUAGE_CODE_JPN: u32 = 0x006A_706E; // jpn 日本語
/// 英語。
pub const LANGUAGE_CODE_ENG: u32 = 0x0065_6E67; // eng
/// ドイツ語。
pub const LANGUAGE_CODE_DEU: u32 = 0x0064_6575; // deu
/// フランス語。
pub const LANGUAGE_CODE_FRA: u32 = 0x0066_7261; // fra
/// イタリア語。
pub const LANGUAGE_CODE_ITA: u32 = 0x0069_7461; // ita
/// 韓国語。
pub const LANGUAGE_CODE_KOR: u32 = 0x006B_6F72; // kor
/// ロシア語。
pub const LANGUAGE_CODE_RUS: u32 = 0x0072_7573; // rus
/// スペイン語。
pub const LANGUAGE_CODE_SPA: u32 = 0x0073_7061; // spa
/// 中国語。
pub const LANGUAGE_CODE_ZHO: u32 = 0x007A_686F; // zho
/// その他。
pub const LANGUAGE_CODE_ETC: u32 = 0x0065_7463; // etc

/// 音声言語優先順位の選択候補テーブル(AudioOptions.cpp:78-89 の `m_AudioLanguageList`)。
pub const AUDIO_LANGUAGE_LIST: [u32; 10] = [
    LANGUAGE_CODE_JPN, // 日本語
    LANGUAGE_CODE_ENG, // 英語
    LANGUAGE_CODE_DEU, // ドイツ語
    LANGUAGE_CODE_FRA, // フランス語
    LANGUAGE_CODE_ITA, // イタリア語
    LANGUAGE_CODE_KOR, // 韓国語
    LANGUAGE_CODE_RUS, // ロシア語
    LANGUAGE_CODE_SPA, // スペイン語
    LANGUAGE_CODE_ZHO, // 中国語
    LANGUAGE_CODE_ETC, // その他
];

/// 行列の列数(チャンネル数: FL/FR/FC/LFE/SL/SR)。
pub const NUM_CHANNELS: usize = 6;

/// サラウンドミキシング行列の行数。
pub const SURROUND_MIXING_MATRIX_ROWS: usize = 6;

/// ダウンミックス行列の行数(出力 L/R)。
pub const DOWN_MIX_MATRIX_ROWS: usize = 2;

/// 1/√2。原実装(AudioOptions.cpp:70)のリテラルを保持する。
const PSQR: f64 = 1.0 / 1.414_213_562_373_095_048_801_688_724_209_7;

/// 既定のサラウンドミキシング行列(AudioOptions.cpp:59-68、6×6 の単位行列)。
pub const DEFAULT_SURROUND_MIXING_MATRIX: [[f64; NUM_CHANNELS]; SURROUND_MIXING_MATRIX_ROWS] = [
    [1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0, 0.0, 0.0],
    [0.0, 0.0, 0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
];

/// 既定のダウンミックス行列(AudioOptions.cpp:71-76、2×6)。
pub const DEFAULT_DOWN_MIX_MATRIX: [[f64; NUM_CHANNELS]; DOWN_MIX_MATRIX_ROWS] = [
    [1.0, 0.0, PSQR, PSQR, PSQR, 0.0],
    [0.0, 1.0, PSQR, PSQR, 0.0, PSQR],
];

/// 音声言語優先順位の 1 項目(AudioOptions.h:38-44 の `AudioLanguageInfo`)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioLanguageInfo {
    /// 言語コード(3 文字を 24bit にパックした値)。
    pub language: u32,
    /// 副音声か。
    pub sub: bool,
}

/// `AudioLanguageInfo` をリストボックス ItemData の Param へエンコードする。
///
/// AudioOptions.cpp:383-385 / 558-559。`Param = Language | (fSub ? LANGUAGE_FLAG_SUB : 0)`。
#[must_use]
pub fn encode_language_param(info: &AudioLanguageInfo) -> u32 {
    let mut param = info.language;
    if info.sub {
        param |= LANGUAGE_FLAG_SUB;
    }
    param
}

/// Param を `AudioLanguageInfo` へデコードする。
///
/// AudioOptions.cpp:558-559。`Language = Param & 0x00FFFFFF`、`fSub = (Param & LANGUAGE_FLAG_SUB) != 0`。
#[must_use]
pub fn decode_language_param(param: u32) -> AudioLanguageInfo {
    AudioLanguageInfo {
        language: param & 0x00FF_FFFF,
        sub: (param & LANGUAGE_FLAG_SUB) != 0,
    }
}

/// 設定文字列(`LangPriority{n}` の値)から `AudioLanguageInfo` をパースする。
///
/// AudioOptions.cpp:150-155。長さ 3 未満は `None`(原実装ではループ終了条件)。
/// `Language = (Value[0]<<16) | (Value[1]<<8) | Value[2]`、`fSub = Value.len()>=4 && Value[3]=='2'`。
#[must_use]
pub fn parse_language_entry(value: &[u16]) -> Option<AudioLanguageInfo> {
    if value.len() < 3 {
        return None;
    }
    let language = (u32::from(value[0]) << 16) | (u32::from(value[1]) << 8) | u32::from(value[2]);
    let sub = value.len() >= 4 && value[3] == u16::from(b'2');
    Some(AudioLanguageInfo { language, sub })
}

/// `AudioLanguageInfo` を設定文字列(`LangPriority{n}` の値)へ整形する。
///
/// AudioOptions.cpp:208-211。`"{:c}{:c}{:c}{}"` 形式で 3 文字 + 副音声なら `"2"`。
/// 第 1 文字(`Lang>>16`)は原実装どおりマスクなし、第 2/3 は `& 0xFF`。
#[must_use]
pub fn format_language_entry(info: &AudioLanguageInfo) -> Vec<u16> {
    let lang = info.language;
    let mut result = vec![
        (lang >> 16) as u16,
        ((lang >> 8) & 0xFF) as u16,
        (lang & 0xFF) as u16,
    ];
    if info.sub {
        result.push(u16::from(b'2'));
    }
    result
}

/// 倍精度値を行列設定文字列用に整形する(AudioOptions.cpp:41-54 の `FormatDouble`)。
///
/// `"{:.4f}"` で 4 桁固定小数にした後、末尾の余分な `'0'` を除去する。整数値は小数点も含めて
/// 末尾が落ちる(例: `1.0` → `"1"`)。原実装のループ(末尾の `'0'` をスキップし、`'0'` 以外に
/// 当たったら 1 つ進めて打ち切る)を忠実に再現する。
#[must_use]
pub fn format_double(value: f64) -> String {
    let s = format!("{value:.4}");
    let bytes = s.as_bytes();
    let length = bytes.len();
    // 原実装: for (i = Length-1; i > 1; i--) { if (s[i] != '0') { i++; break; } ... }
    let mut i = length - 1;
    while i > 1 {
        if bytes[i] != b'0' {
            i += 1;
            break;
        }
        i -= 1;
    }
    // format!("{:.4}", f64) は ASCII のみ生成するため、バイト境界でのスライスは安全。
    s[..i].to_string()
}

/// 行列設定文字列をパースする(AudioOptions.cpp:114-141)。
///
/// カンマ区切りで `rows * NUM_CHANNELS` 要素以上あればパースして行列を返す。要素数不足は `None`
/// (原実装では既定値を維持)。各要素は `strtod` 相当で変換し、`±HUGE_VAL`(無限大)は `0.0` に
/// 置き換える。
#[must_use]
pub fn parse_matrix(buffer: &[u16], rows: usize) -> Option<Vec<[f64; NUM_CHANNELS]>> {
    let parts = split_u16(buffer, u16::from(b','));
    if parts.len() < rows * NUM_CHANNELS {
        return None;
    }
    let mut matrix = Vec::with_capacity(rows);
    for i in 0..rows {
        let mut row = [0.0_f64; NUM_CHANNELS];
        for (j, cell) in row.iter_mut().enumerate() {
            let value = parse_double_u16(&parts[i * NUM_CHANNELS + j]);
            *cell = if value.is_infinite() { 0.0 } else { value };
        }
        matrix.push(row);
    }
    Some(matrix)
}

/// 行列を設定文字列へ整形する(AudioOptions.cpp:176-198)。
///
/// 行優先で全要素を `format_double` し、カンマで連結する。
#[must_use]
pub fn format_matrix(matrix: &[[f64; NUM_CHANNELS]]) -> Vec<u16> {
    let mut buffer = String::new();
    for row in matrix {
        for &value in row {
            if !buffer.is_empty() {
                buffer.push(',');
            }
            buffer.push_str(&format_double(value));
        }
    }
    buffer.encode_utf16().collect()
}

/// 単一文字を区切りに `&[u16]` を分割する(`StringUtility::Split` の 1 文字版、StringUtility.cpp:360)。
///
/// 区切りが無くても末尾の 1 要素は必ず返るため、要素数は「区切り数 + 1」になる。
fn split_u16(src: &[u16], delimiter: u16) -> Vec<Vec<u16>> {
    let mut result = Vec::new();
    let mut start = 0;
    for (i, &c) in src.iter().enumerate() {
        if c == delimiter {
            result.push(src[start..i].to_vec());
            start = i + 1;
        }
    }
    result.push(src[start..].to_vec());
    result
}

/// `&[u16]` を `strtod` 相当で倍精度に変換する(`std::_tcstod`)。
fn parse_double_u16(s: &[u16]) -> f64 {
    let text: String = char::decode_utf16(s.iter().copied())
        .map(|r| r.unwrap_or('\u{FFFD}'))
        .collect();
    parse_double_str(&text)
}

/// 文字列の先頭から数値として有効な最長プレフィックスを `strtod` 相当でパースする。
///
/// 先頭空白をスキップし、符号・整数部・小数部・指数部を切り出して変換する。数字が無ければ `0.0`。
fn parse_double_str(s: &str) -> f64 {
    let trimmed = s.trim_start();
    let bytes = trimmed.as_bytes();
    let mut idx = 0;
    if idx < bytes.len() && (bytes[idx] == b'+' || bytes[idx] == b'-') {
        idx += 1;
    }
    let mut has_digit = false;
    while idx < bytes.len() && bytes[idx].is_ascii_digit() {
        idx += 1;
        has_digit = true;
    }
    if idx < bytes.len() && bytes[idx] == b'.' {
        idx += 1;
        while idx < bytes.len() && bytes[idx].is_ascii_digit() {
            idx += 1;
            has_digit = true;
        }
    }
    if has_digit && idx < bytes.len() && (bytes[idx] == b'e' || bytes[idx] == b'E') {
        let save = idx;
        idx += 1;
        if idx < bytes.len() && (bytes[idx] == b'+' || bytes[idx] == b'-') {
            idx += 1;
        }
        let mut exp_digit = false;
        while idx < bytes.len() && bytes[idx].is_ascii_digit() {
            idx += 1;
            exp_digit = true;
        }
        if !exp_digit {
            idx = save;
        }
    }
    if !has_digit {
        return 0.0;
    }
    trimmed[..idx].parse::<f64>().unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn to_u16(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn from_u16(s: &[u16]) -> String {
        String::from_utf16_lossy(s)
    }

    #[test]
    fn audio_language_list_values() {
        // 言語コードは 3 文字 ASCII を 24bit にパックした値。
        assert_eq!(LANGUAGE_CODE_JPN, 0x6A_706E);
        assert_eq!(AUDIO_LANGUAGE_LIST.len(), 10);
        assert_eq!(AUDIO_LANGUAGE_LIST[0], LANGUAGE_CODE_JPN);
        assert_eq!(AUDIO_LANGUAGE_LIST[9], LANGUAGE_CODE_ETC);
        // 'j'=0x6A, 'p'=0x70, 'n'=0x6E。
        assert_eq!((LANGUAGE_CODE_JPN >> 16) as u8, b'j');
        assert_eq!(((LANGUAGE_CODE_JPN >> 8) & 0xFF) as u8, b'p');
        assert_eq!((LANGUAGE_CODE_JPN & 0xFF) as u8, b'n');
    }

    #[test]
    fn encode_decode_param_round_trip() {
        let info = AudioLanguageInfo {
            language: LANGUAGE_CODE_JPN,
            sub: false,
        };
        let param = encode_language_param(&info);
        assert_eq!(param, LANGUAGE_CODE_JPN);
        assert_eq!(decode_language_param(param), info);

        let sub_info = AudioLanguageInfo {
            language: LANGUAGE_CODE_ENG,
            sub: true,
        };
        let sub_param = encode_language_param(&sub_info);
        assert_eq!(sub_param, LANGUAGE_CODE_ENG | LANGUAGE_FLAG_SUB);
        assert_eq!(decode_language_param(sub_param), sub_info);
    }

    #[test]
    fn parse_language_entry_basic() {
        assert_eq!(
            parse_language_entry(&to_u16("jpn")),
            Some(AudioLanguageInfo {
                language: LANGUAGE_CODE_JPN,
                sub: false,
            })
        );
        // 4 文字目 '2' で副音声。
        assert_eq!(
            parse_language_entry(&to_u16("eng2")),
            Some(AudioLanguageInfo {
                language: LANGUAGE_CODE_ENG,
                sub: true,
            })
        );
        // 4 文字目が '2' 以外は副音声でない。
        assert_eq!(
            parse_language_entry(&to_u16("engX")),
            Some(AudioLanguageInfo {
                language: LANGUAGE_CODE_ENG,
                sub: false,
            })
        );
    }

    #[test]
    fn parse_language_entry_too_short() {
        // 長さ 3 未満は None(ループ終了条件)。
        assert_eq!(parse_language_entry(&to_u16("en")), None);
        assert_eq!(parse_language_entry(&to_u16("")), None);
    }

    #[test]
    fn format_language_entry_basic() {
        assert_eq!(
            from_u16(&format_language_entry(&AudioLanguageInfo {
                language: LANGUAGE_CODE_JPN,
                sub: false,
            })),
            "jpn"
        );
        assert_eq!(
            from_u16(&format_language_entry(&AudioLanguageInfo {
                language: LANGUAGE_CODE_JPN,
                sub: true,
            })),
            "jpn2"
        );
    }

    #[test]
    fn language_entry_round_trip() {
        for &language in &AUDIO_LANGUAGE_LIST {
            for sub in [false, true] {
                let info = AudioLanguageInfo { language, sub };
                let text = format_language_entry(&info);
                assert_eq!(parse_language_entry(&text), Some(info));
            }
        }
    }

    #[test]
    fn format_double_basic() {
        // 整数値は小数点ごと末尾が落ちる。
        assert_eq!(format_double(1.0), "1");
        assert_eq!(format_double(0.0), "0");
        // 末尾の余分な 0 のみ除去。
        assert_eq!(format_double(0.5), "0.5");
        assert_eq!(format_double(0.25), "0.25");
        // 4 桁を超える分は四捨五入され、末尾 0 が無ければそのまま。
        assert_eq!(format_double(PSQR), "0.7071");
        assert_eq!(format_double(0.123_45), "0.1235");
    }

    #[test]
    fn format_double_negative_integer_keeps_dot() {
        // 原実装のループは負の整数値・2 桁以上の整数値で小数点が残る(第 1 条件で '.' を先に拾うため)。
        // 既定行列にこれらの値は無く実害はないが、忠実な挙動として記録する。
        assert_eq!(format_double(-1.0), "-1.");
        assert_eq!(format_double(10.0), "10.");
    }

    #[test]
    fn parse_matrix_default_surround() {
        let text = format_matrix(&DEFAULT_SURROUND_MIXING_MATRIX);
        let parsed = parse_matrix(&text, SURROUND_MIXING_MATRIX_ROWS).unwrap();
        assert_eq!(parsed.len(), SURROUND_MIXING_MATRIX_ROWS);
        // 単位行列(0/1 のみ)は format_double→parse で完全一致する。
        assert_eq!(parsed.as_slice(), DEFAULT_SURROUND_MIXING_MATRIX.as_slice());
    }

    #[test]
    fn parse_matrix_too_few_elements() {
        // 要素数不足は None(既定値維持)。
        assert_eq!(
            parse_matrix(&to_u16("1,0,0"), SURROUND_MIXING_MATRIX_ROWS),
            None
        );
        assert_eq!(parse_matrix(&to_u16(""), DOWN_MIX_MATRIX_ROWS), None);
    }

    #[test]
    fn parse_matrix_infinity_to_zero() {
        // ±HUGE_VAL(無限大)は 0.0 に置き換える。
        let mut text = String::from("1e400,-1e400");
        for _ in 0..(DOWN_MIX_MATRIX_ROWS * NUM_CHANNELS - 2) {
            text.push_str(",1");
        }
        let parsed = parse_matrix(&to_u16(&text), DOWN_MIX_MATRIX_ROWS).unwrap();
        assert_eq!(parsed[0][0], 0.0);
        assert_eq!(parsed[0][1], 0.0);
        assert_eq!(parsed[0][2], 1.0);
    }

    #[test]
    fn format_matrix_default_down_mix() {
        let text = from_u16(&format_matrix(&DEFAULT_DOWN_MIX_MATRIX));
        assert_eq!(
            text,
            "1,0,0.7071,0.7071,0.7071,0,0,1,0.7071,0.7071,0,0.7071"
        );
    }
}
