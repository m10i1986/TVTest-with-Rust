//! TVTest の `CTextDraw`(`src/TextDraw.cpp`)のうち、プラットフォーム非依存な行レイアウト処理。
//!
//! 移植対象は日本語の禁則処理(行頭/行末禁則文字に基づく行分割位置の調整)と行数計算。文字列は
//! 原実装の `wchar_t`(UTF-16)に合わせ `&[u16]` で扱う。
//!
//! 文字幅の実測(何文字が指定幅に収まるか)は GDI / DirectWrite 依存のため、[`FitCharCounter`]
//! トレイトで抽象化する。`CTextDraw::Begin`/`End`/`Draw`(HDC 描画)や GDI/DirectWrite エンジン
//! (`CTextDrawEngine_GDI` / `CTextDrawEngine_DirectWrite`)は Win32 依存のため対象外。

/// キャリッジリターン(`\r`)。
const CR: u16 = b'\r' as u16;
/// ラインフィード(`\n`)。
const LF: u16 = b'\n' as u16;

/// 行頭禁則文字(TextDraw.cpp:34)。これらの文字で行を始めない。
const START_PROHIBIT_CHARS: &str =
    ")）]］｣」』】〉》”、。,，.．!！?？ー〜～…・ァィゥェォッャュョヮヵヶぁぃぅぇぉっゃゅょゎゝゞ々";

/// 行末禁則文字(TextDraw.cpp:37)。これらの文字で行を終えない。
const END_PROHIBIT_CHARS: &str = "(（[［｢「『【〈《“#＃▽▼";

/// 文字幅の実測を提供する抽象。原実装 `CTextDrawEngine::GetFitCharCount`。
///
/// `text`(1 行分)のうち、先頭から数えて幅 `width`(ピクセル)に収まる文字数を返す。GDI なら
/// `GetTextExtentExPoint`、DirectWrite なら独自実測で実装する想定。
pub trait FitCharCounter {
    /// `text` の先頭から `width` に収まる文字数(`GetFitCharCount`、TextDraw.cpp:308)。
    fn get_fit_char_count(&self, text: &[u16], width: i32) -> i32;
}

/// 行頭禁則文字か(`IsStartProhibitChar`、TextDraw.cpp:317)。
pub fn is_start_prohibit_char(ch: u16) -> bool {
    START_PROHIBIT_CHARS.chars().any(|c| c as u32 == ch as u32)
}

/// 行末禁則文字か(`IsEndProhibitChar`、TextDraw.cpp:323)。
pub fn is_end_prohibit_char(ch: u16) -> bool {
    END_PROHIBIT_CHARS.chars().any(|c| c as u32 == ch as u32)
}

/// `text` の `i` 番目の符号単位を得る。範囲外は `0`(ヌル終端相当)。
fn char_at(text: &[u16], i: usize) -> u16 {
    text.get(i).copied().unwrap_or(0)
}

/// ヌル終端までの符号単位数(`StringCharLength` 相当)。
fn string_char_length(text: &[u16]) -> usize {
    text.iter().take_while(|&&c| c != 0).count()
}

/// 1 行の長さ(改行/ヌルまでの符号単位数、`GetLineLength`、TextDraw.cpp:256)。
pub fn get_line_length(text: &[u16]) -> usize {
    text.iter()
        .position(|&c| c == 0 || c == CR || c == LF)
        .unwrap_or(text.len())
}

/// 禁則処理で行分割位置を調整する(`AdjustLineLength`、TextDraw.cpp:265)。
///
/// `text` は行頭からの残りテキスト、`length` は仮の分割位置(収まる文字数)。`japanese_hyphenation`
/// が真のとき、行末/行頭の禁則文字を避けるよう分割位置を手前へ詰めた値を返す。
///
/// - `length < 1` のときは残りテキストの符号単位数(最低 1)を返す。
/// - 行末(`text[length-1]`)が行末禁則文字なら、禁則でない位置まで手前へ詰める。
/// - 次の行頭(`text[length]`)が行頭禁則文字なら、その文字を次行へ送らないよう手前へ詰める
///   (詰めた先の直前が行末禁則文字なら、さらに手前へ)。
pub fn adjust_line_length(text: &[u16], length: usize, japanese_hyphenation: bool) -> usize {
    if length < 1 {
        let l = string_char_length(text);
        return if l < 1 { 1 } else { l };
    }

    if japanese_hyphenation && length > 1 {
        if is_end_prohibit_char(char_at(text, length - 1)) {
            // 行末が行末禁則文字 → 禁則でない位置まで手前へ。
            let mut p = length as isize - 2;
            while p >= 0 {
                if !is_end_prohibit_char(char_at(text, p as usize)) {
                    return p as usize + 1;
                }
                p -= 1;
            }
        } else if is_start_prohibit_char(char_at(text, length)) {
            // 次の行頭が行頭禁則文字 → その文字を行末側へ残すよう手前へ。
            let mut p = length as isize - 1;
            while p > 0 {
                if !is_start_prohibit_char(char_at(text, p as usize)) {
                    if is_end_prohibit_char(char_at(text, (p - 1) as usize)) {
                        let mut q = p - 1;
                        while q >= 0 {
                            if !is_end_prohibit_char(char_at(text, q as usize)) {
                                return q as usize + 1;
                            }
                            q -= 1;
                        }
                    } else {
                        return p as usize;
                    }
                    break;
                }
                p -= 1;
            }
        }
    }

    length
}

/// 指定幅で折り返したときの行数(`CalcLineCount`、TextDraw.cpp:114)。
///
/// `width <= 0` のときは 0。改行(`\r` / `\n` / `\r\n`)で改行し、各行は `counter` で収まる文字数を
/// 求め、[`adjust_line_length`] で禁則調整してから次行へ進む。
pub fn calc_line_count<C: FitCharCounter>(
    text: &[u16],
    width: i32,
    counter: &C,
    japanese_hyphenation: bool,
) -> i32 {
    if width <= 0 {
        return 0;
    }

    let mut lines = 0;
    let mut pos = 0usize;

    while char_at(text, pos) != 0 {
        let c = char_at(text, pos);
        if c == CR || c == LF {
            pos += 1;
            if char_at(text, pos) == LF {
                pos += 1;
            }
            if char_at(text, pos) == 0 {
                break;
            }
            lines += 1;
            continue;
        }

        let line_len = get_line_length(&text[pos..]);
        if line_len == 0 {
            break;
        }

        let fit = counter.get_fit_char_count(&text[pos..pos + line_len], width);
        let fit = adjust_line_length(&text[pos..], fit.max(0) as usize, japanese_hyphenation);
        pos += fit;
        lines += 1;

        if char_at(text, pos) == CR {
            pos += 1;
        }
        if char_at(text, pos) == LF {
            pos += 1;
        }
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    /// 1 文字 `char_width` ピクセル固定でフィット数を返すテスト用カウンタ。
    struct FixedWidthCounter {
        char_width: i32,
    }

    impl FitCharCounter for FixedWidthCounter {
        fn get_fit_char_count(&self, text: &[u16], width: i32) -> i32 {
            let max = (width / self.char_width).max(0) as usize;
            text.len().min(max) as i32
        }
    }

    #[test]
    fn prohibit_char_membership() {
        assert!(is_start_prohibit_char('）' as u16));
        assert!(is_start_prohibit_char('、' as u16));
        assert!(is_start_prohibit_char('ー' as u16));
        assert!(!is_start_prohibit_char('あ' as u16));
        assert!(!is_start_prohibit_char('（' as u16));

        assert!(is_end_prohibit_char('（' as u16));
        assert!(is_end_prohibit_char('「' as u16));
        assert!(!is_end_prohibit_char('）' as u16));
        assert!(!is_end_prohibit_char('あ' as u16));
    }

    #[test]
    fn line_length_stops_at_newline() {
        assert_eq!(get_line_length(&w("abc")), 3);
        assert_eq!(get_line_length(&w("abc\ndef")), 3);
        assert_eq!(get_line_length(&w("abc\r\ndef")), 3);
        assert_eq!(get_line_length(&w("\nabc")), 0);
        assert_eq!(get_line_length(&w("")), 0);
    }

    #[test]
    fn adjust_no_hyphenation_keeps_length() {
        // 禁則無効なら長さそのまま
        let t = w("あい）う");
        assert_eq!(adjust_line_length(&t, 2, false), 2);
    }

    #[test]
    fn adjust_length_zero_falls_back() {
        let t = w("abcd");
        assert_eq!(adjust_line_length(&t, 0, true), 4);
        // 空文字は最低 1
        assert_eq!(adjust_line_length(&w(""), 0, true), 1);
    }

    #[test]
    fn adjust_end_prohibit_pulls_back() {
        // "あ（い": 分割位置 2 だと行末が '（'(行末禁則) → 手前 'あ' の後(=1)へ
        let t = w("あ（い");
        assert_eq!(adjust_line_length(&t, 2, true), 1);
    }

    #[test]
    fn adjust_start_prohibit_pulls_back() {
        // "あい）う": 分割位置 2 だと次行頭が '）'(行頭禁則) → 'い' を次行へ送り 1 へ
        let t = w("あい）う");
        assert_eq!(adjust_line_length(&t, 2, true), 1);
    }

    #[test]
    fn adjust_start_prohibit_with_preceding_end_prohibit() {
        // "（）あ": 分割位置 2 → 次行頭 'あ'? いや text[2]='あ' は禁則でない。別ケースを構成。
        // "（）い": 分割位置 1 だと行末 '（'(行末禁則)が先に判定され手前へ(p=-1 で見つからず長さ維持)。
        // ここでは行頭禁則の入れ子経路を検証: "あ））" 分割位置1 → 行末 'あ' 非禁則, 次行頭 text[1]='）' 行頭禁則
        // p=0 で 'あ' 非行頭禁則 → 直前(p-1=-1)参照は範囲外0=非行末禁則 → return p=0 …だが length>1 でないので発火しない。
        // 入れ子(直前が行末禁則)経路: "（あ）X" 分割位置3 → text[3]='X'? 用意: "（あ）」"
        // text: '（'(end-prohibit) 'あ' '）'(start-prohibit) '」'(start-prohibit)
        // 分割位置3: 行末 text[2]='）' は行末禁則ではない。次行頭 text[3]='」' は行頭禁則。
        // p=2: text[2]='）' 行頭禁則 → p=1: 'あ' 非行頭禁則 → 直前 text[0]='（' 行末禁則 → 入れ子:
        //   q=0: '（' 行末禁則 → q=-1 で終了(見つからず) → break → length 維持(3)。
        let t = w("（あ）」");
        assert_eq!(adjust_line_length(&t, 3, true), 3);
    }

    #[test]
    fn calc_line_count_wraps_plain_text() {
        // 1 文字 10px、幅 30 → 3 文字/行。8 文字 → 3 行(3+3+2)
        let counter = FixedWidthCounter { char_width: 10 };
        assert_eq!(calc_line_count(&w("abcdefgh"), 30, &counter, false), 3);
    }

    #[test]
    fn calc_line_count_handles_newlines() {
        let counter = FixedWidthCounter { char_width: 10 };
        // "abc\ndef" 幅30(3/行) → 2 行
        assert_eq!(calc_line_count(&w("abc\ndef"), 30, &counter, false), 2);
        // 空行を含む "abc\n\ndef" → 3 行
        assert_eq!(calc_line_count(&w("abc\n\ndef"), 30, &counter, false), 3);
    }

    #[test]
    fn calc_line_count_zero_width() {
        let counter = FixedWidthCounter { char_width: 10 };
        assert_eq!(calc_line_count(&w("abc"), 0, &counter, false), 0);
        assert_eq!(calc_line_count(&w(""), 30, &counter, false), 0);
    }

    #[test]
    fn calc_line_count_applies_kinsoku() {
        // "あ（いうえ" を 1 文字 10px・幅 20(2 文字/行)。
        // 1 行目: fit=2 → text[1]='（' 行末禁則 → 手前 1 へ。行 "あ"。
        let counter = FixedWidthCounter { char_width: 10 };
        let n = calc_line_count(&w("あ（いうえ"), 20, &counter, true);
        // あ / （い / うえ → 3 行
        assert_eq!(n, 3);
    }
}
