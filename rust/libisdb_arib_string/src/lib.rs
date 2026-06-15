// Rust port of LibISDB/Base/ARIBString.cpp + JISKanjiMap.cpp
// ARIBString.cpp:69, JISKanjiMap.cpp:1

mod tables;

// JISKanjiMap.cpp:1953
#[inline]
fn is_ligature(code: u32) -> bool {
    code > 0x10FFFF
}

// JISKanjiMap.cpp:1999
fn get_kanji_code(plane: u8, code: u16) -> u32 {
    let row = (code >> 8) as usize;
    let cell = (code & 0xFF) as usize;
    if row < 0x21 || row > 0x7E || cell < 0x21 || cell > 0x7E {
        return 0;
    }
    let r = row - 0x21;
    let c = cell - 0x21;
    if plane == 1 {
        return tables::KANJI_PLANE1_TABLE[r][c];
    }
    if plane == 2 {
        if row <= 0x2F {
            return tables::KANJI_PLANE2_TABLE1[r][c];
        }
        if row >= 0x6E {
            let r2 = row - 0x6E;
            if r2 < 17 {
                return tables::KANJI_PLANE2_TABLE2[r2][c];
            }
        }
    }
    0
}

// JISKanjiMap.cpp:2047 — convert JIS X 0213 kanji code to UTF-16 code units
pub fn jisx0213_kanji_to_utf16(plane: u8, code: u16) -> Vec<u16> {
    let unicode = get_kanji_code(plane, code);
    if unicode == 0 {
        return vec![];
    }
    if is_ligature(unicode) {
        return vec![(unicode & 0xFFFF) as u16, (unicode >> 16) as u16];
    }
    if unicode <= 0xFFFF {
        return vec![unicode as u16];
    }
    // surrogate pair
    let cp = unicode - 0x10000;
    vec![
        0xD800u16 | (cp >> 10) as u16,
        0xDC00u16 | (cp & 0x03FF) as u16,
    ]
}

// ARIBString.cpp:1227
fn utf8_to_codepoint(data: &[u8]) -> (u32, usize) {
    if data.is_empty() {
        return (0, 0);
    }
    let b0 = data[0] as u32;
    if b0 < 0x80 {
        return (b0, 1);
    }
    if b0 >= 0xC2 && b0 < 0xE0 && data.len() >= 2 {
        let b1 = data[1] as u32;
        if b1 >= 0x80 && b1 < 0xC0 {
            return (((b0 & 0x1F) << 6) | (b1 & 0x3F), 2);
        }
    }
    if b0 >= 0xE0 && b0 < 0xF0 && data.len() >= 3 {
        let b1 = data[1] as u32;
        let b2 = data[2] as u32;
        if b1 >= 0x80 && b1 < 0xC0 && b2 >= 0x80 && b2 < 0xC0
            && ((b0 & 0x0F) != 0 || (b1 & 0x20) != 0)
        {
            let cp = ((b0 & 0x0F) << 12) | ((b1 & 0x3F) << 6) | (b2 & 0x3F);
            if cp < 0xD800 || cp >= 0xE000 {
                return (cp, 3);
            }
            return (0, 3);
        }
    }
    if b0 >= 0xF0 && b0 < 0xF8 && data.len() >= 4 {
        let b1 = data[1] as u32;
        let b2 = data[2] as u32;
        let b3 = data[3] as u32;
        if b1 >= 0x80 && b1 < 0xC0 && b2 >= 0x80 && b2 < 0xC0 && b3 >= 0x80 && b3 < 0xC0
            && ((b0 & 0x07) != 0 || (b1 & 0x30) != 0)
        {
            let cp = ((b0 & 0x07) << 18) | ((b1 & 0x3F) << 12) | ((b2 & 0x3F) << 6) | (b3 & 0x3F);
            if cp < 0x110000 {
                return (cp, 4);
            }
            return (0, 4);
        }
    }
    (0, 1)
}

// ARIBString.cpp:416 — CodeSet enumeration
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum CodeSet {
    Kanji = 0,
    Alphanumeric,
    Hiragana,
    Katakana,
    Mosaic_A,
    Mosaic_B,
    Mosaic_C,
    Mosaic_D,
    ProportionalAlphanumeric,
    ProportionalHiragana,
    ProportionalKatakana,
    JIS_X0201_Katakana,
    JIS_KanjiPlane1,
    JIS_KanjiPlane2,
    AdditionalSymbols,
    LatinExtension,
    LatinSpecial,
    Macro,
    DRCS_0,
    DRCS_1,
    DRCS_2,
    DRCS_3,
    DRCS_4,
    DRCS_5,
    DRCS_6,
    DRCS_7,
    DRCS_8,
    DRCS_9,
    DRCS_10,
    DRCS_11,
    DRCS_12,
    DRCS_13,
    DRCS_14,
    DRCS_15,
    Unknown,
}

// ARIBString.cpp:1212
fn is_double_byte_code_set(set: CodeSet) -> bool {
    matches!(
        set,
        CodeSet::Kanji
            | CodeSet::JIS_KanjiPlane1
            | CodeSet::JIS_KanjiPlane2
            | CodeSet::AdditionalSymbols
            | CodeSet::DRCS_0
    )
}

// ARIBString.cpp:99 — decode flags
#[derive(Clone, Copy, Default)]
pub struct DecodeFlags {
    pub caption: bool,
    pub one_seg: bool,
    pub latin: bool,
    pub ucs: bool,
    pub use_char_size: bool,
    pub unicode_symbol: bool,
}

// ARIBString.cpp:300 — CharSize
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CharSize {
    Normal,
    Medium,
    Small,
    Micro,
    HighW,
    WidthW,
    SizeW,
    Special1,
    Special2,
}

impl Default for CharSize {
    fn default() -> Self { CharSize::Normal }
}

// ARIBString.hpp:86 — FormatInfo (字幕の書式情報)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FormatInfo {
    /// 書式が適用される文字位置(デコード後文字列の UTF-16 単位オフセット)
    pub pos: usize,
    /// 文字サイズ
    pub size: CharSize,
    /// 文字色インデックス
    pub char_color_index: u8,
    /// 背景色インデックス
    pub back_color_index: u8,
    /// ラスタ色インデックス
    pub raster_color_index: u8,
}

// ARIBString.cpp:416
fn code_point_to_utf16(cp: u32) -> Vec<u16> {
    if cp < 0x10000 {
        vec![cp as u16]
    } else {
        let cp = cp - 0x10000;
        vec![0xD800u16 | (cp >> 10) as u16, 0xDC00u16 | (cp & 0x03FF) as u16]
    }
}

// ARIBString.cpp:630
fn put_alphanumeric_char(code: u16, use_char_size: bool, char_size: CharSize, is_latin: bool) -> &'static str {
    static FULLWIDTH: [&str; 96] = [
        "　","！","\"","＃","＄","％","＆","'","（","）","＊","＋","，","－","．","／",
        "０","１","２","３","４","５","６","７","８","９","：","；","＜","＝","＞","？",
        "＠","Ａ","Ｂ","Ｃ","Ｄ","Ｅ","Ｆ","Ｇ","Ｈ","Ｉ","Ｊ","Ｋ","Ｌ","Ｍ","Ｎ","Ｏ",
        "Ｐ","Ｑ","Ｒ","Ｓ","Ｔ","Ｕ","Ｖ","Ｗ","Ｘ","Ｙ","Ｚ","［","￥","］","＾","＿",
        "｀","ａ","ｂ","ｃ","ｄ","ｅ","ｆ","ｇ","ｈ","ｉ","ｊ","ｋ","ｌ","ｍ","ｎ","ｏ",
        "ｐ","ｑ","ｒ","ｓ","ｔ","ｕ","ｖ","ｗ","ｘ","ｙ","ｚ","｛","｜","｝","￣","　",
    ];
    static HALFWIDTH: [&str; 96] = [
        " ","!","\"","#","$","%","&","'","(",")",  "*","+",",","-",".","/",
        "0","1","2","3","4","5","6","7","8","9",":",";","<","=",">","?",
        "@","A","B","C","D","E","F","G","H","I","J","K","L","M","N","O",
        "P","Q","R","S","T","U","V","W","X","Y","Z","[","\u{a5}","]","^","_",
        "`","a","b","c","d","e","f","g","h","i","j","k","l","m","n","o",
        "p","q","r","s","t","u","v","w","x","y","z","{","|","}","\u{203e}"," ",
    ];
    let half = is_latin || (use_char_size && char_size == CharSize::Medium);
    let table = if half { &HALFWIDTH } else { &FULLWIDTH };
    let idx = if code < 0x20 { 0 } else { (code - 0x20) as usize };
    if idx < table.len() { table[idx] } else { "　" }
}

// ARIBString.cpp:659
fn put_hiragana_char(code: u16) -> &'static str {
    static TABLE: [&str; 96] = [
        "　","ぁ","あ","ぃ","い","ぅ","う","ぇ","え","ぉ","お","か","が","き","ぎ","く",
        "ぐ","け","げ","こ","ご","さ","ざ","し","じ","す","ず","せ","ぜ","そ","ぞ","た",
        "だ","ち","ぢ","っ","つ","づ","て","で","と","ど","な","に","ぬ","ね","の","は",
        "ば","ぱ","ひ","び","ぴ","ふ","ぶ","ぷ","へ","べ","ぺ","ほ","ぼ","ぽ","ま","み",
        "む","め","も","ゃ","や","ゅ","ゆ","ょ","よ","ら","り","る","れ","ろ","ゎ","わ",
        "ゐ","ゑ","を","ん","　","　","　","ゝ","ゞ","ー","。","「","」","、","・","　",
    ];
    let idx = if code < 0x20 { 0 } else { (code - 0x20) as usize };
    if idx < TABLE.len() { TABLE[idx] } else { "　" }
}

// ARIBString.cpp:676
fn put_katakana_char(code: u16) -> &'static str {
    static TABLE: [&str; 96] = [
        "　","ァ","ア","ィ","イ","ゥ","ウ","ェ","エ","ォ","オ","カ","ガ","キ","ギ","ク",
        "グ","ケ","ゲ","コ","ゴ","サ","ザ","シ","ジ","ス","ズ","セ","ゼ","ソ","ゾ","タ",
        "ダ","チ","ヂ","ッ","ツ","ヅ","テ","デ","ト","ド","ナ","ニ","ヌ","ネ","ノ","ハ",
        "バ","パ","ヒ","ビ","ピ","フ","ブ","プ","ヘ","ベ","ペ","ホ","ボ","ポ","マ","ミ",
        "ム","メ","モ","ャ","ヤ","ュ","ユ","ョ","ヨ","ラ","リ","ル","レ","ロ","ヮ","ワ",
        "ヰ","ヱ","ヲ","ン","ヴ","ヵ","ヶ","ヽ","ヾ","ー","。","「","」","、","・","　",
    ];
    let idx = if code < 0x20 { 0 } else { (code - 0x20) as usize };
    if idx < TABLE.len() { TABLE[idx] } else { "　" }
}

// ARIBString.cpp:693
fn put_jis_katakana_char(code: u16) -> &'static str {
    static TABLE: [&str; 64] = [
        "　","。","「","」","、","・","ヲ","ァ","ィ","ゥ","ェ","ォ","ャ","ュ","ョ","ッ",
        "ー","ア","イ","ウ","エ","オ","カ","キ","ク","ケ","コ","サ","シ","ス","セ","ソ",
        "タ","チ","ツ","テ","ト","ナ","ニ","ヌ","ネ","ノ","ハ","ヒ","フ","ヘ","ホ","マ",
        "ミ","ム","メ","モ","ヤ","ユ","ヨ","ラ","リ","ル","レ","ロ","ワ","ン","゛","゜",
    ];
    if code < 0x20 || code >= 0x60 { "　" } else { TABLE[(code - 0x20) as usize] }
}

// ARIBString.cpp:708
fn put_latin_extension_char(code: u16) -> &'static str {
    static TABLE: [&str; 96] = [
        " ","\u{a1}","\u{a2}","\u{a3}","\u{20ac}","\u{a5}","\u{160}","\u{a7}",
        "\u{161}","\u{a9}","\u{aa}","\u{ab}","\u{ac}","\u{ff}","\u{ae}","\u{af}",
        "\u{b0}","\u{b1}","\u{b2}","\u{b3}","\u{17d}","\u{3bc}","\u{b6}","\u{b7}",
        "\u{17e}","\u{b9}","\u{ba}","\u{bb}","\u{152}","\u{153}","\u{178}","\u{bf}",
        "\u{c0}","\u{c1}","\u{c2}","\u{c3}","\u{c4}","\u{c5}","\u{c6}","\u{c7}",
        "\u{c8}","\u{c9}","\u{ca}","\u{cb}","\u{cc}","\u{cd}","\u{ce}","\u{cf}",
        "\u{d0}","\u{d1}","\u{d2}","\u{d3}","\u{d4}","\u{d5}","\u{d6}","\u{d7}",
        "\u{d8}","\u{d9}","\u{da}","\u{db}","\u{dc}","\u{dd}","\u{de}","\u{df}",
        "\u{e0}","\u{e1}","\u{e2}","\u{e3}","\u{e4}","\u{e5}","\u{e6}","\u{e7}",
        "\u{e8}","\u{e9}","\u{ea}","\u{eb}","\u{ec}","\u{ed}","\u{ee}","\u{ef}",
        "\u{f0}","\u{f1}","\u{f2}","\u{f3}","\u{f4}","\u{f5}","\u{f6}","\u{f7}",
        "\u{f8}","\u{f9}","\u{fa}","\u{fb}","\u{fc}","\u{fd}","\u{fe}"," ",
    ];
    let idx = if code < 0x20 { 0 } else { (code - 0x20) as usize };
    if idx < TABLE.len() { TABLE[idx] } else { " " }
}

// ARIBString.cpp:725
fn put_latin_special_char(code: u16) -> &'static str {
    static TABLE: [&str; 48] = [
        " ","\u{266a}"," "," "," "," "," "," "," "," "," "," "," "," "," "," ",
        "\u{a4}","\u{a6}","\u{a8}","\u{b4}","\u{b8}","\u{bc}","\u{bd}","\u{be}",
        " "," "," "," "," "," "," "," ",
        "\u{2026}","\u{2588}","\u{2018}","\u{2019}","\u{201c}","\u{201d}","\u{2022}","\u{2122}",
        "\u{215b}","\u{215c}","\u{215d}","\u{215e}"," "," "," "," ",
    ];
    if code < 0x20 || code >= 0x50 { " " } else { TABLE[(code - 0x20) as usize] }
}

// ARIBString.cpp:739 — additional symbols
fn put_symbols_char(code: u16, unicode_symbol: bool) -> Option<&'static str> {
    // 90/01 - 90/40  (0x7A21 - 0x7A48)
    static SYM_90_01: [Option<&str>; 40] = [
        Some("\u{26cc}"),Some("\u{26cd}"),Some("\u{2757}"),Some("\u{26cf}"),
        Some("\u{26d0}"),Some("\u{26d1}"),None,           Some("\u{26d2}"),
        Some("\u{26d5}"),Some("\u{26d3}"),Some("\u{26d4}"),None,
        None,            None,            None,            Some("\u{1f17f}"),
        Some("\u{1f18a}"),None,           None,            Some("\u{26d6}"),
        Some("\u{26d7}"),Some("\u{26d8}"),Some("\u{26d9}"),Some("\u{26da}"),
        Some("\u{26db}"),Some("\u{26dc}"),Some("\u{26dd}"),Some("\u{26de}"),
        Some("\u{26df}"),Some("\u{26e0}"),Some("\u{26e1}"),Some("\u{2b55}"),
        Some("\u{3248}"),Some("\u{3249}"),Some("\u{324a}"),Some("\u{324b}"),
        Some("\u{324c}"),Some("\u{324d}"),Some("\u{324e}"),Some("\u{324f}"),
    ];

    // 90/45 - 90/84 (0x7A4D - 0x7A74)
    static SYM_90_45: [&str; 40] = [
        "10.","11.","12.","[HV]","[SD]","[Ｐ]","[Ｗ]","[MV]",
        "[手]","[字]","[双]","[デ]","[Ｓ]","[二]","[多]","[解]",
        "[SS]","[Ｂ]","[Ｎ]","■","●","[天]","[交]","[映]",
        "[無]","[料]","[年齢制限]","[前]","[後]","[再]","[新]","[初]",
        "[終]","[生]","[販]","[声]","[吹]","[PPV]","(秘)","ほか",
    ];
    static SYM_90_45_U: [&str; 40] = [
        "\u{2491}","\u{2492}","\u{2493}","\u{1f14a}",
        "\u{1f14c}","\u{1f13f}","\u{1f146}","\u{1f14b}",
        "\u{1f210}","\u{1f211}","\u{1f212}","\u{1f213}",
        "\u{1f142}","\u{1f214}","\u{1f215}","\u{1f216}",
        "\u{1f14d}","\u{1f131}","\u{1f13d}","\u{2b1b}",
        "\u{2b24}","\u{1f217}","\u{1f218}","\u{1f219}",
        "\u{1f21a}","\u{1f21b}","\u{26bf}","\u{1f21c}",
        "\u{1f21d}","\u{1f21e}","\u{1f21f}","\u{1f220}",
        "\u{1f221}","\u{1f222}","\u{1f223}","\u{1f224}",
        "\u{1f225}","\u{1f14e}","\u{3299}","\u{1f200}",
    ];

    // 91/01 - 91/49 (0x7B21 - 0x7B51)
    static SYM_91: [Option<&str>; 49] = [
        Some("\u{26e3}"),Some("\u{2b56}"),Some("\u{2b57}"),Some("\u{2b58}"),
        Some("\u{2b59}"),Some("\u{2613}"),Some("\u{328b}"),Some("\u{3012}"),
        Some("\u{26e8}"),Some("\u{3246}"),Some("\u{3245}"),Some("\u{26e9}"),
        Some("\u{fd6}"),Some("\u{26ea}"),Some("\u{26eb}"),Some("\u{26ec}"),
        Some("\u{2668}"),Some("\u{26ed}"),Some("\u{26ee}"),Some("\u{26ef}"),
        Some("\u{2693}"),Some("\u{2708}"),Some("\u{26f0}"),Some("\u{26f1}"),
        Some("\u{26f2}"),Some("\u{26f3}"),Some("\u{26f4}"),Some("\u{26f5}"),
        Some("\u{1f157}"),Some("\u{24b9}"),Some("\u{24c8}"),Some("\u{26f6}"),
        Some("\u{1f15f}"),Some("\u{1f18b}"),Some("\u{1f18d}"),Some("\u{1f18c}"),
        Some("\u{1f179}"),Some("\u{26f7}"),Some("\u{26f8}"),Some("\u{26f9}"),
        Some("\u{26fa}"),Some("\u{1f17b}"),Some("\u{260e}"),Some("\u{26fb}"),
        Some("\u{26fc}"),Some("\u{26fd}"),Some("\u{26fe}"),Some("\u{1f17c}"),
        Some("\u{26ff}"),
    ];

    // 92/01 - 92/91 (0x7C21 - 0x7C7B)
    static SYM_92: [&str; 91] = [
        "→","←","↑","↓","○","●","年","月","日","円","㎡","立方ｍ","㎝","平方㎝","立方㎝","０.",
        "１.","２.","３.","４.","５.","６.","７.","８.","９.","氏","副","元","故","前","新","０,",
        "１,","２,","３,","４,","５,","６,","７,","８,","９,","(社)","(財)","(有)","(株)","(代)","(問)","＞",
        "＜","【","】","◇","^2","^3","(CD)","(vn)","(ob)","(cb)","(ce","mb)","(hp)","(br)","(p)","(s)",
        "(ms)","(t)","(bs)","(b)","(tb)","(tp)","(ds)","(ag)","(eg)","(vo)","(fl)","(ke",
        "y)","(sa","x)","(sy","n)","(or","g)","(pe","r)","(R)","(C)","(箏)","DJ","[演]","Fax",
    ];
    static SYM_92_U: [&str; 91] = [
        "\u{27a1}","\u{2b05}","\u{2b06}","\u{2b07}","\u{2b2f}","\u{2b2e}","年","月",
        "日","円","㎡","\u{33a5}","㎝","\u{33a0}","\u{33a4}","\u{1f100}",
        "\u{2488}","\u{2489}","\u{248a}","\u{248b}","\u{248c}","\u{248d}","\u{248e}","\u{248f}",
        "\u{2490}","氏","副","元","故","前","新","\u{1f101}",
        "\u{1f102}","\u{1f103}","\u{1f104}","\u{1f105}","\u{1f106}","\u{1f107}","\u{1f108}","\u{1f109}",
        "\u{1f10a}","\u{3233}","\u{3236}","\u{3232}","\u{3231}","\u{3239}","\u{3244}","\u{25b6}",
        "\u{25c0}","\u{3016}","\u{3017}","\u{27d0}","\u{b2}","\u{b3}","\u{1f12d}","(vn)",
        "(ob)","(cb)","(ce","mb)","(hp)","(br)","(p)","(s)",
        "(ms)","(t)","(bs)","(b)","(tb)","(tp)","(ds)","(ag)",
        "(eg)","(vo)","(fl)","(ke","y)","(sa","x)","(sy",
        "n)","(or","g)","(pe","r)","\u{1f12c}","\u{1f12b}","\u{3247}",
        "\u{1f190}","\u{1f226}","\u{213b}",
    ];

    // 93/01 - 93/91 (0x7D21 - 0x7D7B)
    static SYM_93: [Option<&str>; 91] = [
        Some("(月)"),Some("(火)"),Some("(水)"),Some("(木)"),Some("(金)"),Some("(土)"),Some("(日)"),Some("(祝)"),
        Some("㍾"),Some("㍽"),Some("㍼"),Some("㍻"),
        Some("№"),Some("℡"),Some("(〒)"),Some("○"),
        Some("〔本〕"),Some("〔三〕"),Some("〔二〕"),Some("〔安〕"),Some("〔点〕"),Some("〔打〕"),Some("〔盗〕"),Some("〔勝〕"),
        Some("〔敗〕"),Some("〔Ｓ〕"),Some("［投］"),Some("［捕］"),Some("［一］"),Some("［二］"),Some("［三］"),Some("［遊］"),
        Some("［左］"),Some("［中］"),Some("［右］"),Some("［指］"),Some("［走］"),Some("［打］"),Some("㍑"),Some("㎏"),
        Some("Hz"),Some("ha"),Some("km"),Some("平方km"),Some("hPa"),None,None,Some("1/2"),
        Some("0/3"),Some("1/3"),Some("2/3"),Some("1/4"),Some("3/4"),Some("1/5"),Some("2/5"),Some("3/5"),
        Some("4/5"),Some("1/6"),Some("5/6"),Some("1/7"),Some("1/8"),Some("1/9"),Some("1/10"),Some("晴れ"),
        Some("曇り"),Some("雨"),Some("雪"),Some("△"),Some("▲"),Some("▽"),Some("▼"),Some("◆"),
        Some("・"),Some("・"),Some("・"),Some("◇"),Some("◎"),Some("!!"),Some("!?"),Some("曇/晴"),
        Some("雨"),Some("雨"),Some("雪"),Some("大雪"),Some("雷"),Some("雷雨"),Some("　"),Some("・"),
        Some("・"),Some("♪"),Some("℡"),
    ];
    static SYM_93_U: [Option<&str>; 91] = [
        Some("\u{322a}"),Some("\u{322b}"),Some("\u{322c}"),Some("\u{322d}"),
        Some("\u{322e}"),Some("\u{322f}"),Some("\u{3230}"),Some("\u{3237}"),
        Some("㍾"),Some("㍽"),Some("㍼"),Some("㍻"),
        Some("№"),Some("℡"),Some("\u{3036}"),Some("\u{26be}"),
        Some("\u{1f240}"),Some("\u{1f241}"),Some("\u{1f242}"),Some("\u{1f243}"),
        Some("\u{1f244}"),Some("\u{1f245}"),Some("\u{1f246}"),Some("\u{1f247}"),
        Some("\u{1f248}"),Some("\u{1f12a}"),Some("\u{1f227}"),Some("\u{1f228}"),
        Some("\u{1f229}"),Some("\u{1f214}"),Some("\u{1f22a}"),Some("\u{1f22b}"),
        Some("\u{1f22c}"),Some("\u{1f22d}"),Some("\u{1f22e}"),Some("\u{1f22f}"),
        Some("\u{1f230}"),Some("\u{1f231}"),Some("\u{2113}"),Some("㎏"),
        Some("\u{3390}"),Some("\u{33ca}"),Some("\u{339e}"),Some("\u{33a2}"),
        Some("\u{3371}"),None,None,Some("\u{bd}"),
        Some("\u{2189}"),Some("\u{2153}"),Some("\u{2154}"),Some("\u{bc}"),
        Some("\u{be}"),Some("\u{2155}"),Some("\u{2156}"),Some("\u{2157}"),
        Some("\u{2158}"),Some("\u{2159}"),Some("\u{215a}"),Some("\u{2150}"),
        Some("\u{215b}"),Some("\u{2151}"),Some("\u{2152}"),Some("\u{2600}"),
        Some("\u{2601}"),Some("\u{2602}"),Some("\u{26c4}"),Some("\u{2616}"),
        Some("\u{2617}"),Some("\u{26c9}"),Some("\u{26ca}"),Some("\u{2666}"),
        Some("\u{2665}"),Some("\u{2663}"),Some("\u{2660}"),Some("\u{26cb}"),
        Some("\u{2a00}"),Some("\u{203c}"),Some("\u{2049}"),Some("\u{26c5}"),
        Some("\u{2614}"),Some("\u{26c6}"),Some("\u{2603}"),Some("\u{26c7}"),
        Some("\u{26a1}"),Some("\u{26c8}"),Some("　"),Some("\u{269e}"),
        Some("\u{269f}"),Some("\u{266c}"),Some("\u{260e}"),
    ];

    // 94/01 - 94/93 (0x7E21 - 0x7E7D)
    static SYM_94: [&str; 93] = [
        "Ⅰ","Ⅱ","Ⅲ","Ⅳ","Ⅴ","Ⅵ","Ⅶ","Ⅷ","Ⅸ","Ⅹ","XI","XⅡ",
        "⑰","⑱","⑲","⑳",
        "(1)","(2)","(3)","(4)","(5)","(6)","(7)","(8)","(9)","(10)","(11)","(12)",
        "(21)","(22)","(23)","(24)",
        "(A)","(B)","(C)","(D)","(E)","(F)","(G)","(H)","(I)","(J)","(K)","(L)",
        "(M)","(N)","(O)","(P)","(Q)","(R)","(S)","(T)","(U)","(V)","(W)","(X)","(Y)","(Z)",
        "(25)","(26)","(27)","(28)","(29)","(30)",
        "①","②","③","④","⑤","⑥","⑦","⑧","⑨","⑩","⑪","⑫","⑬","⑭","⑮","⑯",
        "①","②","③","④","⑤","⑥","⑦","⑧","⑨","⑩","⑪","⑫","(31)",
    ];
    static SYM_94_U: [&str; 93] = [
        "Ⅰ","Ⅱ","Ⅲ","Ⅳ","Ⅴ","Ⅵ","Ⅶ","Ⅷ","Ⅸ","Ⅹ","\u{216a}","\u{216b}",
        "⑰","⑱","⑲","⑳",
        "\u{2474}","\u{2475}","\u{2476}","\u{2477}","\u{2478}","\u{2479}","\u{247a}","\u{247b}",
        "\u{247c}","\u{247d}","\u{247e}","\u{247f}",
        "\u{3251}","\u{3252}","\u{3253}","\u{3254}",
        "\u{1f110}","\u{1f111}","\u{1f112}","\u{1f113}","\u{1f114}","\u{1f115}","\u{1f116}","\u{1f117}",
        "\u{1f118}","\u{1f119}","\u{1f11a}","\u{1f11b}","\u{1f11c}","\u{1f11d}","\u{1f11e}","\u{1f11f}",
        "\u{1f120}","\u{1f121}","\u{1f122}","\u{1f123}","\u{1f124}","\u{1f125}","\u{1f126}","\u{1f127}",
        "\u{1f128}","\u{1f129}","\u{3255}","\u{3256}","\u{3257}","\u{3258}","\u{3259}","\u{325a}",
        "①","②","③","④","⑤","⑥","⑦","⑧","⑨","⑩","⑪","⑫","⑬","⑭","⑮","⑯",
        "\u{2776}","\u{2777}","\u{2778}","\u{2779}","\u{277a}","\u{277b}","\u{277c}","\u{277d}",
        "\u{277e}","\u{277f}","\u{24eb}","\u{24ec}","\u{325b}",
    ];

    // KanjiTable1: 0x7521-0x757E (94 entries but only 94 used; C++ has 0x757D-0x757E = 2 entries)
    static KANJI1: [&str; 94] = [
        "\u{3402}","\u{20158}","\u{4efd}","\u{4eff}","\u{4f9a}","\u{4fc9}","\u{509c}","\u{511e}",
        "\u{51bc}","\u{351f}","\u{5307}","\u{5361}","\u{536c}","\u{8a79}","\u{20bb7}","\u{544d}",
        "\u{5496}","\u{549c}","\u{54a9}","\u{550e}","\u{554a}","\u{5672}","\u{56e4}","\u{5733}",
        "\u{5734}","\u{fa10}","\u{5880}","\u{59e4}","\u{5a23}","\u{5a55}","\u{5bec}","\u{fa11}",
        "\u{37e2}","\u{5eac}","\u{5f34}","\u{5f45}","\u{5fb7}","\u{6017}","\u{fa6b}","\u{6130}",
        "\u{6624}","\u{66c8}","\u{66d9}","\u{66fa}","\u{66fb}","\u{6852}","\u{9fc4}","\u{6911}",
        "\u{693b}","\u{6a45}","\u{6a91}","\u{6adb}","\u{233cc}","\u{233fe}","\u{235c4}","\u{6bf1}",
        "\u{6ce0}","\u{6d2e}","\u{fa45}","\u{6dbf}","\u{6dca}","\u{6df8}","\u{fa46}","\u{6f5e}",
        "\u{6ff9}","\u{7064}","\u{fa6c}","\u{242ee}","\u{7147}","\u{71c1}","\u{7200}","\u{739f}",
        "\u{73a8}","\u{73c9}","\u{73d6}","\u{741b}","\u{7421}","\u{fa4a}","\u{7426}","\u{742a}",
        "\u{742c}","\u{7439}","\u{744b}","\u{3eda}","\u{7575}","\u{7581}","\u{7772}","\u{4093}",
        "\u{78c8}","\u{78e0}","\u{7947}","\u{79ae}","\u{9fc6}","\u{4103}",
    ];

    // KanjiTable2: 0x7621-0x764B (43 entries)
    static KANJI2: [&str; 43] = [
        "\u{9fc5}","\u{79da}","\u{7a1e}","\u{7b7f}","\u{7c31}","\u{4264}","\u{7d8b}","\u{7fa1}",
        "\u{8118}","\u{813a}","\u{fa6d}","\u{82ae}","\u{845b}","\u{84dc}","\u{84ec}","\u{8559}",
        "\u{85ce}","\u{8755}","\u{87ec}","\u{880b}","\u{88f5}","\u{89d2}","\u{8af6}","\u{8dce}",
        "\u{8fbb}","\u{8ff6}","\u{90dd}","\u{9127}","\u{912d}","\u{91b2}","\u{9233}","\u{9288}",
        "\u{9321}","\u{9348}","\u{9592}","\u{96de}","\u{9903}","\u{9940}","\u{9ad9}","\u{9bd6}",
        "\u{9dd7}","\u{9eb4}","\u{9eb5}",
    ];

    match code {
        0x7521..=0x757E => {
            let idx = (code - 0x7521) as usize;
            if idx < KANJI1.len() { Some(KANJI1[idx]) } else { None }
        }
        0x7621..=0x764B => {
            let idx = (code - 0x7621) as usize;
            if idx < KANJI2.len() { Some(KANJI2[idx]) } else { None }
        }
        0x7A21..=0x7A48 => {
            let idx = (code - 0x7A21) as usize;
            SYM_90_01[idx]
        }
        0x7A4D..=0x7A74 => {
            let idx = (code - 0x7A4D) as usize;
            let t = if unicode_symbol { SYM_90_45_U[idx] } else { SYM_90_45[idx] };
            Some(t)
        }
        0x7B21..=0x7B51 => {
            let idx = (code - 0x7B21) as usize;
            SYM_91[idx]
        }
        0x7C21..=0x7C7B => {
            let idx = (code - 0x7C21) as usize;
            let t = if unicode_symbol { SYM_92_U[idx] } else { SYM_92[idx] };
            Some(t)
        }
        0x7D21..=0x7D7B => {
            let idx = (code - 0x7D21) as usize;
            if unicode_symbol { SYM_93_U[idx] } else { SYM_93[idx] }
        }
        0x7E21..=0x7E7D => {
            let idx = (code - 0x7E21) as usize;
            let t = if unicode_symbol { SYM_94_U[idx] } else { SYM_94[idx] };
            Some(t)
        }
        _ => None,
    }
}

// ARIBString.cpp:1143
fn designation_gset(code_g: &mut [CodeSet; 4], index_g: usize, code: u8) -> bool {
    match code {
        0x42 => { code_g[index_g] = CodeSet::Kanji;                    true }
        0x4A => { code_g[index_g] = CodeSet::Alphanumeric;             true }
        0x30 => { code_g[index_g] = CodeSet::Hiragana;                 true }
        0x31 => { code_g[index_g] = CodeSet::Katakana;                 true }
        0x32 => { code_g[index_g] = CodeSet::Mosaic_A;                 true }
        0x33 => { code_g[index_g] = CodeSet::Mosaic_B;                 true }
        0x34 => { code_g[index_g] = CodeSet::Mosaic_C;                 true }
        0x35 => { code_g[index_g] = CodeSet::Mosaic_D;                 true }
        0x36 => { code_g[index_g] = CodeSet::ProportionalAlphanumeric; true }
        0x37 => { code_g[index_g] = CodeSet::ProportionalHiragana;     true }
        0x38 => { code_g[index_g] = CodeSet::ProportionalKatakana;     true }
        0x49 => { code_g[index_g] = CodeSet::JIS_X0201_Katakana;       true }
        0x4B => { code_g[index_g] = CodeSet::LatinExtension;           true }
        0x4C => { code_g[index_g] = CodeSet::LatinSpecial;             true }
        0x39 => { code_g[index_g] = CodeSet::JIS_KanjiPlane1;          true }
        0x3A => { code_g[index_g] = CodeSet::JIS_KanjiPlane2;          true }
        0x3B => { code_g[index_g] = CodeSet::AdditionalSymbols;        true }
        _ => false,
    }
}

fn designation_drcs(code_g: &mut [CodeSet; 4], index_g: usize, code: u8) -> bool {
    if (0x40..=0x4F).contains(&code) {
        let n = (code - 0x40) as u8;
        code_g[index_g] = match n {
            0  => CodeSet::DRCS_0,  1  => CodeSet::DRCS_1,  2  => CodeSet::DRCS_2,
            3  => CodeSet::DRCS_3,  4  => CodeSet::DRCS_4,  5  => CodeSet::DRCS_5,
            6  => CodeSet::DRCS_6,  7  => CodeSet::DRCS_7,  8  => CodeSet::DRCS_8,
            9  => CodeSet::DRCS_9,  10 => CodeSet::DRCS_10, 11 => CodeSet::DRCS_11,
            12 => CodeSet::DRCS_12, 13 => CodeSet::DRCS_13, 14 => CodeSet::DRCS_14,
            _  => CodeSet::DRCS_15,
        };
        true
    } else if code == 0x70 {
        code_g[index_g] = CodeSet::Macro;
        true
    } else {
        false
    }
}

// Decoder state — ARIBString.cpp:99
struct DecoderState {
    code_g: [CodeSet; 4],
    locking_gl: usize,
    locking_gr: usize,
    single_gl: i32,
    esc_seq_count: u8,
    esc_seq_index: usize,
    is_esc_seq_drcs: bool,
    char_size: CharSize,
    rpc: u8,
    is_latin: bool,
    is_ucs: bool,
    use_char_size: bool,
    unicode_symbol: bool,
    // 字幕書式(FormatList)用。ARIBString.cpp:141
    char_color_index: u8,
    back_color_index: u8,
    raster_color_index: u8,
    def_palette: u8,
}

impl DecoderState {
    fn new(flags: &DecodeFlags) -> Self {
        let is_caption = flags.caption;
        let is_latin = flags.latin;
        let is_1seg = flags.one_seg;

        let mut code_g = [
            CodeSet::Kanji,
            CodeSet::Alphanumeric,
            CodeSet::Hiragana,
            if is_caption { CodeSet::Macro } else { CodeSet::Katakana },
        ];
        let (locking_gl, locking_gr);

        if is_latin {
            code_g[0] = CodeSet::Alphanumeric;
            code_g[2] = CodeSet::LatinExtension;
            code_g[3] = CodeSet::LatinSpecial;
            locking_gl = 0;
            locking_gr = 2;
        } else if is_caption && is_1seg {
            code_g[1] = CodeSet::DRCS_1;
            locking_gl = 1;
            locking_gr = 0;
        } else {
            locking_gl = 0;
            locking_gr = 2;
        }

        // 字幕時の初期色 (ARIBString.cpp:140)
        let (char_color_index, back_color_index, raster_color_index) = if is_caption {
            (7, 8, 8)
        } else {
            (0, 0, 0)
        };

        DecoderState {
            code_g,
            locking_gl,
            locking_gr,
            single_gl: -1,
            esc_seq_count: 0,
            esc_seq_index: 0,
            is_esc_seq_drcs: false,
            char_size: if is_latin { CharSize::Medium } else { CharSize::Normal },
            rpc: 1,
            is_latin,
            is_ucs: flags.ucs,
            use_char_size: flags.use_char_size,
            unicode_symbol: flags.unicode_symbol,
            char_color_index,
            back_color_index,
            raster_color_index,
            def_palette: 0,
        }
    }
}

// ARIBString.cpp:1186 — SetFormat
// 現在の書式状態を FormatList に記録する。同位置の既存エントリは上書き。
fn set_format(format_list: &mut Vec<FormatInfo>, state: &DecoderState, pos: usize) {
    let format = FormatInfo {
        pos,
        size: state.char_size,
        char_color_index: state.char_color_index,
        back_color_index: state.back_color_index,
        raster_color_index: state.raster_color_index,
    };
    if let Some(last) = format_list.last_mut() {
        if last.pos == pos {
            *last = format;
            return;
        }
    }
    format_list.push(format);
}

fn decode_char_to_utf16(code: u16, set: CodeSet, state: &DecoderState, dst: &mut Vec<u16>) {
    match set {
        CodeSet::Kanji | CodeSet::JIS_KanjiPlane1 => {
            decode_kanji(code, 1, state, dst);
        }
        CodeSet::JIS_KanjiPlane2 => {
            decode_kanji(code, 2, state, dst);
        }
        CodeSet::Alphanumeric | CodeSet::ProportionalAlphanumeric => {
            let s = put_alphanumeric_char(code, state.use_char_size, state.char_size, state.is_latin);
            dst.extend(s.encode_utf16());
        }
        CodeSet::Hiragana | CodeSet::ProportionalHiragana => {
            dst.extend(put_hiragana_char(code).encode_utf16());
        }
        CodeSet::Katakana | CodeSet::ProportionalKatakana => {
            dst.extend(put_katakana_char(code).encode_utf16());
        }
        CodeSet::JIS_X0201_Katakana => {
            dst.extend(put_jis_katakana_char(code).encode_utf16());
        }
        CodeSet::LatinExtension => {
            dst.extend(put_latin_extension_char(code).encode_utf16());
        }
        CodeSet::LatinSpecial => {
            dst.extend(put_latin_special_char(code).encode_utf16());
        }
        CodeSet::AdditionalSymbols => {
            if let Some(s) = put_symbols_char(code, state.unicode_symbol) {
                dst.extend(s.encode_utf16());
            } else {
                dst.extend("□".encode_utf16());
            }
        }
        _ => {
            dst.extend("□".encode_utf16());
        }
    }
}

// ARIBString.cpp:511
fn decode_kanji(code: u16, plane: u8, state: &DecoderState, dst: &mut Vec<u16>) {
    if plane == 1 && code >= 0x7521 {
        if let Some(s) = put_symbols_char(code, state.unicode_symbol) {
            dst.extend(s.encode_utf16());
        } else {
            dst.extend("□".encode_utf16());
        }
        return;
    }

    // full-to-half conversion for medium char size
    if state.use_char_size && state.char_size == CharSize::Medium && plane == 1 {
        let first = (code >> 8) as u8;
        let second = (code & 0xFF) as u8;
        let mut alnm: u8 = 0;
        if first == 0x23
            && ((second >= 0x30 && second <= 0x39)
                || (second >= 0x41 && second <= 0x5A)
                || (second >= 0x61 && second <= 0x7A))
        {
            alnm = second;
        } else if first == 0x21 {
            const MAP: [(u8, u8); 30] = [
                (0x21,0x20),(0x24,0x2C),(0x25,0x2E),(0x27,0x3A),(0x28,0x3B),(0x29,0x3F),
                (0x2A,0x21),(0x2E,0x60),(0x30,0x5E),(0x31,0x7E),(0x32,0x5F),(0x3F,0x2F),
                (0x43,0x7C),(0x4A,0x28),(0x4B,0x29),(0x4E,0x5B),(0x4F,0x5D),(0x50,0x7B),
                (0x51,0x7D),(0x5C,0x2B),(0x61,0x3D),(0x63,0x3C),(0x64,0x3E),(0x6F,0x5C),
                (0x70,0x24),(0x73,0x25),(0x74,0x23),(0x75,0x26),(0x76,0x2A),(0x77,0x40),
            ];
            for &(from, to) in &MAP {
                if from > second { break; }
                if from == second { alnm = to; break; }
            }
        }
        if alnm != 0 {
            let s = put_alphanumeric_char(alnm as u16, true, CharSize::Medium, false);
            dst.extend(s.encode_utf16());
            return;
        }
    }

    let units = jisx0213_kanji_to_utf16(plane, code);
    if units.is_empty() {
        dst.extend("□".encode_utf16());
    } else {
        dst.extend(units);
    }
}

// ARIBString.cpp:1068
fn process_escape_seq(state: &mut DecoderState, code: u8) {
    match state.esc_seq_count {
        1 => match code {
            0x6E => { state.locking_gl = 2; state.esc_seq_count = 0; return; }
            0x6F => { state.locking_gl = 3; state.esc_seq_count = 0; return; }
            0x7E => { state.locking_gr = 1; state.esc_seq_count = 0; return; }
            0x7D => { state.locking_gr = 2; state.esc_seq_count = 0; return; }
            0x7C => { state.locking_gr = 3; state.esc_seq_count = 0; return; }
            0x24 | 0x28 => state.esc_seq_index = 0,
            0x29 => state.esc_seq_index = 1,
            0x2A => state.esc_seq_index = 2,
            0x2B => state.esc_seq_index = 3,
            _ => { state.esc_seq_count = 0; return; }
        },
        2 => {
            if designation_gset(&mut state.code_g, state.esc_seq_index, code) {
                state.esc_seq_count = 0;
                return;
            }
            match code {
                0x20 => state.is_esc_seq_drcs = true,
                0x28 => { state.is_esc_seq_drcs = true;  state.esc_seq_index = 0; }
                0x29 => { state.is_esc_seq_drcs = false; state.esc_seq_index = 1; }
                0x2A => { state.is_esc_seq_drcs = false; state.esc_seq_index = 2; }
                0x2B => { state.is_esc_seq_drcs = false; state.esc_seq_index = 3; }
                _ => { state.esc_seq_count = 0; return; }
            }
        }
        3 => {
            if !state.is_esc_seq_drcs {
                if designation_gset(&mut state.code_g, state.esc_seq_index, code) {
                    state.esc_seq_count = 0;
                    return;
                }
            } else if designation_drcs(&mut state.code_g, state.esc_seq_index, code) {
                state.esc_seq_count = 0;
                return;
            }
            if code == 0x20 {
                state.is_esc_seq_drcs = true;
            } else {
                state.esc_seq_count = 0;
                return;
            }
        }
        4 => {
            designation_drcs(&mut state.code_g, state.esc_seq_index, code);
            state.esc_seq_count = 0;
            return;
        }
        _ => {}
    }
    state.esc_seq_count += 1;
}

// ARIBString.cpp:182
fn decode_string(
    src: &[u8],
    state: &mut DecoderState,
    mut format_list: Option<&mut Vec<FormatInfo>>,
) -> Option<Vec<u16>> {
    let mut dst: Vec<u16> = Vec::new();
    let mut pos = 0usize;

    while pos < src.len() {
        let b = src[pos];

        if state.esc_seq_count != 0 {
            process_escape_seq(state, b);
            pos += 1;
            continue;
        }

        // UCS non-control characters
        if state.is_ucs
            && ((b >= 0x21 && b <= 0x7E)
                || (b >= 0x80
                    && !((b == 0xC2)
                        && (src.len() - pos >= 2)
                        && (src[pos + 1] >= 0x80)
                        && (src[pos + 1] < 0xA1))))
        {
            if b >= 0xFE {
                return None; // UTF-16 BOM unsupported
            }
            let old_len = dst.len();
            let (cp, clen) = utf8_to_codepoint(&src[pos..]);
            if cp == 0 {
                dst.extend("□".encode_utf16());
            } else if (0xEC00..=0xF8FF).contains(&cp) {
                dst.extend("□".encode_utf16()); // DRCS private area
            } else {
                dst.extend(code_point_to_utf16(cp));
            }
            let rpc = state.rpc;
            if rpc > 1 {
                let chunk: Vec<u16> = dst[old_len..].to_vec();
                for _ in 1..rpc {
                    dst.extend_from_slice(&chunk);
                }
            }
            state.rpc = 1;
            pos += clen;
            continue;
        }

        // GL area (0x21-0x7E)
        if !state.is_ucs && b >= 0x21 && b <= 0x7E {
            let cs_idx = if state.single_gl >= 0 { state.single_gl as usize } else { state.locking_gl };
            state.single_gl = -1;
            let cur_set = state.code_g[cs_idx];
            let old_len = dst.len();
            if is_double_byte_code_set(cur_set) {
                if src.len() - pos < 2 { return None; }
                let code = ((src[pos] as u16) << 8) | src[pos + 1] as u16;
                decode_char_to_utf16(code, cur_set, state, &mut dst);
                pos += 2;
            } else {
                decode_char_to_utf16(b as u16, cur_set, state, &mut dst);
                pos += 1;
            }
            let rpc = state.rpc;
            if rpc > 1 && dst.len() > old_len {
                let chunk: Vec<u16> = dst[old_len..].to_vec();
                for _ in 1..rpc { dst.extend_from_slice(&chunk); }
            }
            state.rpc = 1;
            continue;
        }

        // GR area (0xA1-0xFE)
        if !state.is_ucs && b >= 0xA1 && b <= 0xFE {
            let cur_set = state.code_g[state.locking_gr];
            let old_len = dst.len();
            if is_double_byte_code_set(cur_set) {
                if src.len() - pos < 2 { return None; }
                let code = (((src[pos] as u16) << 8) | src[pos + 1] as u16) & 0x7F7F;
                decode_char_to_utf16(code, cur_set, state, &mut dst);
                pos += 2;
            } else {
                decode_char_to_utf16((b & 0x7F) as u16, cur_set, state, &mut dst);
                pos += 1;
            }
            let rpc = state.rpc;
            if rpc > 1 && dst.len() > old_len {
                let chunk: Vec<u16> = dst[old_len..].to_vec();
                for _ in 1..rpc { dst.extend_from_slice(&chunk); }
            }
            state.rpc = 1;
            continue;
        }

        // Control codes
        let ctrl = if state.is_ucs && b == 0xC2 {
            pos += 1;
            if pos >= src.len() { break; }
            src[pos]
        } else {
            b
        };

        match ctrl {
            0x0D => dst.extend("\n".encode_utf16()),
            0x0F => state.locking_gl = 0,
            0x0E => state.locking_gl = 1,
            0x19 => state.single_gl = 2,
            0x1D => state.single_gl = 3,
            0x1B => state.esc_seq_count = 1,
            0x20 => {
                if state.char_size == CharSize::Small || state.char_size == CharSize::Micro {
                    dst.push(0x20);
                } else {
                    dst.extend("　".encode_utf16());
                }
            }
            0xA0 => dst.push(0x20),
            // 文字色 (CSI 系の前景色 0x80-0x87)。ARIBString.cpp:288
            0x80..=0x87 => {
                state.char_color_index = (state.def_palette << 4) | (ctrl & 0x0F);
                if let Some(fl) = format_list.as_deref_mut() {
                    set_format(fl, state, dst.len());
                }
            }
            0x88 => {
                state.char_size = CharSize::Small;
                if let Some(fl) = format_list.as_deref_mut() {
                    set_format(fl, state, dst.len());
                }
            }
            0x89 => {
                state.char_size = CharSize::Medium;
                if let Some(fl) = format_list.as_deref_mut() {
                    set_format(fl, state, dst.len());
                }
            }
            0x8A => {
                state.char_size = CharSize::Normal;
                if let Some(fl) = format_list.as_deref_mut() {
                    set_format(fl, state, dst.len());
                }
            }
            0x8B => {
                pos += 1;
                if pos < src.len() {
                    state.char_size = match src[pos] {
                        0x60 => CharSize::Micro,
                        0x41 => CharSize::HighW,
                        0x44 => CharSize::WidthW,
                        0x45 => CharSize::SizeW,
                        0x6B => CharSize::Special1,
                        0x64 => CharSize::Special2,
                        _ => state.char_size,
                    };
                }
                if let Some(fl) = format_list.as_deref_mut() {
                    set_format(fl, state, dst.len());
                }
            }
            0x0C => dst.push(0x0C),
            0x16 | 0x91 | 0x93 | 0x94 | 0x97 => {
                pos += 1;
            }
            0x1C => { pos += 2; }
            // COL 色設定。ARIBString.cpp:335
            0x90 => {
                pos += 1;
                if pos < src.len() {
                    if src[pos] == 0x20 {
                        pos += 1;
                        if pos < src.len() {
                            state.def_palette = src[pos] & 0x0F;
                        }
                    } else {
                        match src[pos] & 0xF0 {
                            0x40 => state.char_color_index = src[pos] & 0x0F,
                            0x50 => state.back_color_index = src[pos] & 0x0F,
                            _ => {}
                        }
                        if let Some(fl) = format_list.as_deref_mut() {
                            set_format(fl, state, dst.len());
                        }
                    }
                }
            }
            0x95 => {
                loop {
                    pos += 1;
                    if pos >= src.len() { break; }
                    if src[pos] == 0x4F { break; }
                }
            }
            0x98 => {
                pos += 1;
                if pos < src.len() {
                    state.rpc = src[pos] & 0x3F;
                }
            }
            0x9B => {
                loop {
                    pos += 1;
                    if pos >= src.len() { break; }
                    if src[pos] > 0x3B { break; }
                }
            }
            0x9D => {
                pos += 1;
                if pos < src.len() {
                    if src[pos] == 0x20 {
                        pos += 1;
                    } else {
                        while pos < src.len() && !(0x40..=0x43).contains(&src[pos]) {
                            pos += 1;
                        }
                    }
                }
            }
            _ => {}
        }

        pos += 1;
    }

    Some(dst)
}

// ARIBString.cpp:69 — public decode API: ARIB 8-unit code → UTF-16
pub fn decode(src: &[u8], flags: DecodeFlags) -> Option<Vec<u16>> {
    if src.is_empty() {
        return None;
    }
    let mut state = DecoderState::new(&flags);
    decode_string(src, &mut state, None)
}

// Convenience: decode to UTF-8 String
pub fn decode_to_string(src: &[u8], flags: DecodeFlags) -> Option<String> {
    let utf16 = decode(src, flags)?;
    Some(String::from_utf16_lossy(&utf16).to_string())
}

/// ARIBString.cpp:113 — DecodeCaption: 字幕デコード(書式情報リスト付き)。
///
/// デコード後の UTF-16 文字列と、文字位置ごとの書式情報リスト(FormatInfo)を返す。
/// 書式情報は色・文字サイズの制御コードが現れた位置で記録される。
pub fn decode_caption(src: &[u8], flags: DecodeFlags) -> Option<(Vec<u16>, Vec<FormatInfo>)> {
    if src.is_empty() {
        return None;
    }
    // 字幕デコードでは caption フラグを有効にする (原実装の DecodeCaption 既定)
    let mut state = DecoderState::new(&flags);
    let mut format_list: Vec<FormatInfo> = Vec::new();
    let dst = decode_string(src, &mut state, Some(&mut format_list))?;
    Some((dst, format_list))
}

/// 字幕デコードして UTF-8 文字列と書式情報リストを返す。
pub fn decode_caption_to_string(
    src: &[u8],
    flags: DecodeFlags,
) -> Option<(String, Vec<FormatInfo>)> {
    let (utf16, format_list) = decode_caption(src, flags)?;
    Some((String::from_utf16_lossy(&utf16).to_string(), format_list))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags_default() -> DecodeFlags { DecodeFlags::default() }

    #[test]
    fn test_decode_empty_returns_none() {
        assert!(decode(&[], flags_default()).is_none());
    }

    #[test]
    fn test_hiragana_via_ss2() {
        // SS2 (0x19) invokes G2=Hiragana; 0x22 = 'あ' (index 2)
        let src = [0x19u8, 0x22];
        let result = decode_to_string(&src, flags_default()).unwrap();
        assert_eq!(result, "あ");
    }

    #[test]
    fn test_katakana_via_ss3() {
        // SS3 (0x1D) invokes G3=Katakana; 0x22 → index 2 = 'ア'
        let src = [0x1Du8, 0x22];
        let result = decode_to_string(&src, flags_default()).unwrap();
        assert_eq!(result, "ア");
    }

    #[test]
    fn test_alphanumeric_fullwidth() {
        // ESC 0x28 0x4A → G0 = Alphanumeric; 0x41 = 'Ａ' fullwidth
        let src = [0x1Bu8, 0x28, 0x4A, 0x41];
        let result = decode_to_string(&src, flags_default()).unwrap();
        assert_eq!(result, "Ａ");
    }

    #[test]
    fn test_alphanumeric_halfwidth_in_latin_mode() {
        let flags = DecodeFlags { latin: true, ..Default::default() };
        // Latin mode: G0=Alphanumeric, halfwidth
        // ESC 0x28 0x4A → G0 = Alphanumeric; 0x41 = 'A'
        let src = [0x1Bu8, 0x28, 0x4A, 0x41];
        let result = decode_to_string(&src, flags).unwrap();
        assert_eq!(result, "A");
    }

    #[test]
    fn test_newline_control() {
        let src = [0x0Du8];
        let result = decode_to_string(&src, flags_default()).unwrap();
        assert_eq!(result, "\n");
    }

    #[test]
    fn test_space_fullwidth() {
        let src = [0x20u8];
        let result = decode_to_string(&src, flags_default()).unwrap();
        assert_eq!(result, "　");
    }

    #[test]
    fn test_space_halfwidth_a0() {
        let src = [0xA0u8];
        let result = decode_to_string(&src, flags_default()).unwrap();
        assert_eq!(result, " ");
    }

    #[test]
    fn test_escape_set_hiragana_g0() {
        // ESC 0x28 0x30 → G0 = Hiragana; 0x22 = 'あ'
        let src = [0x1Bu8, 0x28, 0x30, 0x22];
        let result = decode_to_string(&src, flags_default()).unwrap();
        assert_eq!(result, "あ");
    }

    #[test]
    fn test_escape_set_katakana_g0() {
        // ESC 0x28 0x31 → G0 = Katakana; 0x22 → index 2 = 'ア'
        let src = [0x1Bu8, 0x28, 0x31, 0x22];
        let result = decode_to_string(&src, flags_default()).unwrap();
        assert_eq!(result, "ア");
    }

    #[test]
    fn test_jisx0213_kanji_to_utf16_plane1() {
        // 0x2422: row=0x24, cell=0x22 → table[3][1] = 0x3042 = 'あ'
        let units = jisx0213_kanji_to_utf16(1, 0x2422);
        assert_eq!(units.len(), 1);
        let ch = char::from_u32(units[0] as u32).unwrap();
        assert_eq!(ch, 'あ');
    }

    #[test]
    fn test_jisx0213_kanji_to_utf16_ideographic_space() {
        // 0x2121: row=0x21, cell=0x21 → table[0][0] = 0x3000 = ideographic space
        let units = jisx0213_kanji_to_utf16(1, 0x2121);
        assert_eq!(units.len(), 1);
        assert_eq!(units[0], 0x3000);
    }

    #[test]
    fn test_jisx0213_kanji_to_utf16_invalid() {
        assert!(jisx0213_kanji_to_utf16(1, 0x0000).is_empty());
        assert!(jisx0213_kanji_to_utf16(3, 0x2121).is_empty());
    }

    #[test]
    fn test_get_kanji_code_plane1_first() {
        let v = get_kanji_code(1, 0x2121);
        assert_eq!(v, 0x3000);
    }

    #[test]
    fn test_is_ligature() {
        assert!(is_ligature(0x00120000));
        assert!(!is_ligature(0x3042));
        assert!(!is_ligature(0x10FFFF));
    }

    #[test]
    fn test_rpc_repeat() {
        // 0x98 = RPC, next byte & 0x3F = repeat count=2
        // Then ESC G0=Hiragana, 0x22='あ' → repeated 2 times
        let src = [0x98u8, 0x02, 0x1B, 0x28, 0x30, 0x22];
        let result = decode_to_string(&src, flags_default()).unwrap();
        assert_eq!(result, "ああ");
    }

    #[test]
    fn test_locking_shift_g1_alphanumeric() {
        // LS1 (0x0E) → locking GL = G1 = Alphanumeric
        // 0x41 = 'Ａ' fullwidth
        let src = [0x0Eu8, 0x41];
        let result = decode_to_string(&src, flags_default()).unwrap();
        assert_eq!(result, "Ａ");
    }

    // ─── DecodeCaption / FormatList ─────────────────────────────

    fn caption_flags() -> DecodeFlags {
        DecodeFlags { caption: true, ..Default::default() }
    }

    #[test]
    fn test_decode_caption_empty_returns_none() {
        assert!(decode_caption(&[], caption_flags()).is_none());
    }

    #[test]
    fn test_decode_caption_no_format_codes() {
        // 制御コードが無ければ FormatList は空、テキストは通常デコードと一致
        let src = [0x1Bu8, 0x28, 0x30, 0x22]; // G0=Hiragana 'あ'
        let (utf16, fmt) = decode_caption(&src, caption_flags()).unwrap();
        assert_eq!(String::from_utf16_lossy(&utf16), "あ");
        assert!(fmt.is_empty());
    }

    #[test]
    fn test_decode_caption_char_color() {
        // 0x81 = 文字色1 設定。色制御の直後(位置0)に FormatInfo が記録される
        let src = [0x81u8, 0x1B, 0x28, 0x30, 0x22]; // 色設定 → 'あ'
        let (utf16, fmt) = decode_caption(&src, caption_flags()).unwrap();
        assert_eq!(String::from_utf16_lossy(&utf16), "あ");
        assert_eq!(fmt.len(), 1);
        assert_eq!(fmt[0].pos, 0);
        assert_eq!(fmt[0].char_color_index, 1);
        // 字幕初期背景色は 8
        assert_eq!(fmt[0].back_color_index, 8);
    }

    #[test]
    fn test_decode_caption_char_size_small() {
        // 文字を1つ出してからサイズ変更(SSZ 0x88)を行い、位置1に記録されることを確認
        // ESC G0=Hiragana, 'あ'(pos 0..1) → 0x88 SSZ(small) at pos 1
        let src = [0x1Bu8, 0x28, 0x30, 0x22, 0x88];
        let (utf16, fmt) = decode_caption(&src, caption_flags()).unwrap();
        assert_eq!(String::from_utf16_lossy(&utf16), "あ");
        assert_eq!(fmt.len(), 1);
        assert_eq!(fmt[0].pos, 1);
        assert_eq!(fmt[0].size, CharSize::Small);
    }

    #[test]
    fn test_decode_caption_col_background() {
        // COL(0x90) で背景色設定: 0x50|0x03 = 背景色3
        let src = [0x90u8, 0x53, 0x1B, 0x28, 0x30, 0x22];
        let (utf16, fmt) = decode_caption(&src, caption_flags()).unwrap();
        assert_eq!(String::from_utf16_lossy(&utf16), "あ");
        assert_eq!(fmt.len(), 1);
        assert_eq!(fmt[0].pos, 0);
        assert_eq!(fmt[0].back_color_index, 3);
        // 文字色は字幕初期値 7 のまま
        assert_eq!(fmt[0].char_color_index, 7);
    }

    #[test]
    fn test_decode_caption_col_foreground() {
        // COL(0x90) で文字色設定: 0x40|0x05 = 文字色5
        let src = [0x90u8, 0x45, 0x1B, 0x28, 0x30, 0x22];
        let (_, fmt) = decode_caption(&src, caption_flags()).unwrap();
        assert_eq!(fmt.len(), 1);
        assert_eq!(fmt[0].char_color_index, 5);
    }

    #[test]
    fn test_decode_caption_col_def_palette() {
        // COL(0x90) 0x20 でパレット設定 → その後の 0x80-0x87 色に反映
        // 0x90 0x20 0x01 (palette=1), then 0x82 (color2) → char_color = (1<<4)|2 = 0x12
        let src = [0x90u8, 0x20, 0x01, 0x82, 0x1B, 0x28, 0x30, 0x22];
        let (_, fmt) = decode_caption(&src, caption_flags()).unwrap();
        assert_eq!(fmt.len(), 1);
        assert_eq!(fmt[0].char_color_index, 0x12);
    }

    #[test]
    fn test_decode_caption_same_pos_overwrites() {
        // 同位置(pos 0)で複数の色設定 → 最後の値で上書き、エントリは1つ
        let src = [0x81u8, 0x83, 0x1B, 0x28, 0x30, 0x22]; // 色1 → 色3 (どちらもpos 0)
        let (_, fmt) = decode_caption(&src, caption_flags()).unwrap();
        assert_eq!(fmt.len(), 1);
        assert_eq!(fmt[0].char_color_index, 3);
    }

    #[test]
    fn test_decode_caption_multiple_positions() {
        // 'あ'(pos0) → 色変更(pos1) → 'い'(pos1..2) → サイズ変更(pos2)
        let src = [
            0x1Bu8, 0x28, 0x30, 0x22, // G0=Hiragana, 'あ'
            0x84, // 色4 (pos 1)
            0x24, // 'い' (G0=Hiragana index 4)
            0x88, // SSZ small (pos 2)
        ];
        let (utf16, fmt) = decode_caption(&src, caption_flags()).unwrap();
        assert_eq!(String::from_utf16_lossy(&utf16), "あい");
        assert_eq!(fmt.len(), 2);
        assert_eq!(fmt[0].pos, 1);
        assert_eq!(fmt[0].char_color_index, 4);
        assert_eq!(fmt[1].pos, 2);
        assert_eq!(fmt[1].size, CharSize::Small);
    }

    #[test]
    fn test_decode_does_not_emit_format() {
        // 通常の decode() は FormatList を生成しない(挙動不変の確認)。
        // 色制御を含んでもテキストは正常にデコードされる。
        let src = [0x81u8, 0x1B, 0x28, 0x30, 0x22];
        let result = decode_to_string(&src, caption_flags()).unwrap();
        assert_eq!(result, "あ");
    }

    #[test]
    fn test_symbols_additional_first() {
        // ESC 0x28 0x3B → G0 = AdditionalSymbols; bytes 0x7A 0x21 → code 0x7A21 → "\u{26cc}"
        let src = [0x1Bu8, 0x28, 0x3B, 0x7A, 0x21];
        let result = decode_to_string(&src, flags_default()).unwrap();
        assert_eq!(result, "\u{26cc}");
    }

    #[test]
    fn test_gr_area_hiragana() {
        // GR area (0xA1-0xFE), locking_gr=G2=Hiragana
        // 0xA2 & 0x7F = 0x22 → hiragana 'あ'
        let src = [0xA2u8];
        let result = decode_to_string(&src, flags_default()).unwrap();
        assert_eq!(result, "あ");
    }

    #[test]
    fn test_decode_to_string_api() {
        let src = [0x1Bu8, 0x28, 0x30, 0x22];
        let s = decode_to_string(&src, flags_default()).unwrap();
        assert_eq!(s, "あ");
    }

    #[test]
    fn test_utf8_to_codepoint_ascii() {
        let (cp, len) = utf8_to_codepoint(b"A");
        assert_eq!(cp, 0x41);
        assert_eq!(len, 1);
    }

    #[test]
    fn test_utf8_to_codepoint_3byte() {
        // 'あ' = U+3042 = E3 81 82
        let data = [0xE3u8, 0x81, 0x82];
        let (cp, len) = utf8_to_codepoint(&data);
        assert_eq!(cp, 0x3042);
        assert_eq!(len, 3);
    }

    #[test]
    fn test_put_hiragana_a() {
        // code 0x22 → index 2 → 'あ'
        assert_eq!(put_hiragana_char(0x22), "あ");
    }

    #[test]
    fn test_put_katakana_a() {
        // code 0x22 → index 2 → 'ア'
        assert_eq!(put_katakana_char(0x22), "ア");
    }

    #[test]
    fn test_put_jis_katakana() {
        // code 0x21 → index 1 → '。'
        assert_eq!(put_jis_katakana_char(0x21), "。");
    }

    #[test]
    fn test_kanji_g0_decode() {
        // G0=Kanji(default), 2-byte: 0x2421 0x00 → 'あ' = 0x3041
        // 0x2421: row=0x24, cell=0x21 → table[3][0] = 0x3041
        let src = [0x24u8, 0x21];
        let result = decode_to_string(&src, flags_default()).unwrap();
        assert_eq!(result, "ぁ");
    }

    #[test]
    fn test_locking_shift_ls2() {
        // ESC 0x6E → LS2: locking GL = G2 = Hiragana
        // then 0x22 = 'あ'
        let src = [0x1Bu8, 0x6E, 0x22];
        let result = decode_to_string(&src, flags_default()).unwrap();
        assert_eq!(result, "あ");
    }

    #[test]
    fn test_char_size_small_space() {
        // 0x88 = SSZ (small), then 0x20 = space → half-width
        let src = [0x88u8, 0x20];
        let result = decode_to_string(&src, flags_default()).unwrap();
        assert_eq!(result, " ");
    }
}
