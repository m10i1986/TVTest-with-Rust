// TVTest の EpgUtil.cpp / EpgUtil.h のうちプラットフォーム非依存な純粋ロジックを Rust へ移植。
//
// 移植対象:
//   - VideoType / get_video_type   : ComponentType → SD/HD 判定
//   - ContentNibble / get_event_genre : コンテンツニブルからジャンル抽出
//   - map_arib_symbol              : ARIB 外字テーブルによる文字列変換(UTF-16)
//   - EpgGenre::get_text           : ジャンルテキストテーブル引き(CEpgGenre::GetText)
//   - FormatEventTimeFlag / MAX_EVENT_TIME_LENGTH / format_event_time
//                                  : 番組時刻の文字列整形(EpgUtil.cpp:82 の
//                                    SYSTEMTIME 版に相当する純粋部分)
//   - get_day_of_week_text         : 曜日テキスト(Util.cpp:350 GetDayOfWeekText。
//                                    format_event_time が参照するためここに置く)
//
// format_event_time の制限:
//   原実装の EpgTimeToDisplayTime(GetAppClass().EpgOptions の時刻モード依存、
//   EpgUtil.cpp:149)は移植不可のため、入力の開始時刻は「表示用時刻に変換済み」として
//   扱う(= FormatEventTimeFlag::NO_CONVERT 相当の動作)。フラグ自体は API 忠実性の
//   ため定義している。
//
// 非対象(AppMain / Win32 / LibISDB 依存):
//   - EpgTimeToDisplayTime  : GetAppClass().EpgOptions 依存
//   - CEpgIcons::DrawIcon   : GDI 依存
//   - CEpgTheme::*          : Theme::CThemeManager / GDI 依存
//
// 文字列は原実装の wchar_t(UTF-16)に合わせ Vec<u16> / &[u16] ベースで扱う。
// BMP 外文字(U+1Fxxx 等、ARIB 外字変換先)はサロゲートペア(2 u16)として扱う。
// format_event_time の出力は BMP 内文字のみのため String で返す。

use tvtest_util::{offset_system_time, SystemTime};

/// 映像種別。原実装 EpgUtil.h EpgUtil::VideoType。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoType {
    Unknown,
    Sd,
    Hd,
}

/// ComponentType から映像種別を判定する。原実装 EpgUtil.cpp:36 GetVideoType。
///
/// 下位4ビット(aspect 等)が 1-4 の範囲内で、
/// 上位4ビット(解像度区分)によって SD/HD を判別する。
pub fn get_video_type(component_type: u8) -> VideoType {
    if (component_type & 0x0F) >= 1 && (component_type & 0x0F) <= 4 {
        match component_type >> 4 {
            0x0 | 0xA | 0xD | 0xF => VideoType::Sd,
            0x9 | 0xB | 0xC | 0xE => VideoType::Hd,
            _ => VideoType::Unknown,
        }
    } else {
        VideoType::Unknown
    }
}

/// コンテンツニブル(ジャンル情報の 1 エントリ)。
/// 原実装 LibISDB::EventInfo::ContentNibbleInfo::NibbleList の要素に対応。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContentNibble {
    pub content_nibble_level1: u8,
    pub content_nibble_level2: u8,
}

/// コンテンツニブルリスト。原実装 LibISDB::EventInfo::ContentNibbleInfo に対応。
#[derive(Debug, Clone)]
pub struct ContentNibbleInfo {
    pub nibble_list: Vec<ContentNibble>,
}

/// コンテンツニブルリストからジャンルを抽出する。
/// 原実装 EpgUtil.cpp:359 GetEventGenre(const ContentNibbleInfo &, ...) 。
///
/// NibbleCount 個のニブルを走査し、ContentNibbleLevel1 が 0xE でない最初の項目を返す。
/// 見つかれば (level1, level2)、見つからなければ None。
/// 原実装は -1 を「見つからなかった」として返すが、ここでは Option で表す。
pub fn get_event_genre(nibble_info: &ContentNibbleInfo) -> Option<(i32, i32)> {
    for nibble in nibble_info.nibble_list.iter() {
        if nibble.content_nibble_level1 != 0xE {
            return Some((
                nibble.content_nibble_level1 as i32,
                nibble.content_nibble_level2 as i32,
            ));
        }
    }
    None
}

/// ARIB 外字変換テーブル。原実装 EpgUtil.cpp:441 MapList。
/// 変換元(UTF-8)→ 変換先(UTF-8)のペア。
/// 変換先が BMP 外文字の場合、出力時にサロゲートペアとして展開する。
const ARIB_SYMBOL_MAP: &[(&str, &str)] = &[
    ("[HV]",       "\u{1f14a}"),
    ("[SD]",       "\u{1f14c}"),
    ("[Ｐ]",       "\u{1f13f}"),
    ("[Ｗ]",       "\u{1f146}"),
    ("[MV]",       "\u{1f14b}"),
    ("[手]",       "\u{1f210}"),
    ("[字]",       "\u{1f211}"),
    ("[双]",       "\u{1f212}"),
    ("[デ]",       "\u{1f213}"),
    ("[Ｓ]",       "\u{1f142}"),
    ("[二]",       "\u{1f214}"),
    ("[多]",       "\u{1f215}"),
    ("[解]",       "\u{1f216}"),
    ("[SS]",       "\u{1f14d}"),
    ("[Ｂ]",       "\u{1f131}"),
    ("[Ｎ]",       "\u{1f13d}"),
    ("[天]",       "\u{1f217}"),
    ("[交]",       "\u{1f218}"),
    ("[映]",       "\u{1f219}"),
    ("[無]",       "\u{1f21a}"),
    ("[料]",       "\u{1f21b}"),
    ("[年齢制限]", "\u{26bf}"),
    ("[前]",       "\u{1f21c}"),
    ("[後]",       "\u{1f21d}"),
    ("[再]",       "\u{1f21e}"),
    ("[新]",       "\u{1f21f}"),
    ("[初]",       "\u{1f220}"),
    ("[終]",       "\u{1f221}"),
    ("[生]",       "\u{1f222}"),
    ("[販]",       "\u{1f223}"),
    ("[声]",       "\u{1f224}"),
    ("[吹]",       "\u{1f225}"),
    ("[PPV]",      "\u{1f14e}"),
    ("(秘)",       "\u{3299}"),
];

/// ARIB 外字テーブルを使って文字列を変換する。
/// 原実装 EpgUtil.cpp:431 MapARIBSymbol(LPCWSTR, LPWSTR, size_t)。
///
/// `src`(UTF-16)を先頭から走査し、テーブルの変換元 str と前方一致したら
/// 変換先 str(UTF-16 展開)を追記する。一致しなければ元の u16 をそのまま追記。
/// 出力は Vec<u16>。
///
/// 原実装は DestLength(バッファ上限)で切り捨てるが、本関数は Vec で動的に確保するため
/// 上限なしで完全な変換結果を返す。BMP 外文字はサロゲートペアとして正確に出力する。
pub fn map_arib_symbol(src: &[u16]) -> Vec<u16> {
    let mut dst: Vec<u16> = Vec::with_capacity(src.len());
    let mut pos = 0usize;

    'outer: while pos < src.len() {
        // テーブルの各エントリと先頭から前方一致を試みる。
        for &(from_str, to_str) in ARIB_SYMBOL_MAP.iter() {
            let from: Vec<u16> = from_str.encode_utf16().collect();
            let flen = from.len();
            if pos + flen <= src.len() && src[pos..pos + flen] == from[..] {
                // 変換先を UTF-16 で展開(BMP 外はサロゲートペア)。
                let to: Vec<u16> = to_str.encode_utf16().collect();
                dst.extend_from_slice(&to);
                pos += flen;
                continue 'outer;
            }
        }
        // 一致なし: 元の u16 をそのまま。
        dst.push(src[pos]);
        pos += 1;
    }

    dst
}

/// ジャンルテキスト定数。`GENRE_OTHER`(0xF)は 12 番目のジャンル(存在しない)相当として
/// 返す文字列を別途定義する。
pub const GENRE_OTHER: i32 = 0xF;

/// ジャンルのテキストを返す。原実装 EpgUtil.cpp:549 CEpgGenre::GetText。
///
/// `level2 < 0` の場合は Level1 のカテゴリ名を返す。
/// Level1 が範囲外で `GENRE_OTHER(0xF)` なら "その他"。
/// `level2 >= 0` の場合はサブカテゴリ名。`nullptr`(原実装)は None として返す。
pub fn epg_genre_get_text(level1: i32, level2: i32) -> Option<&'static str> {
    const GENRE_LIST: &[(&str, &[Option<&str>; 16])] = &[
        ("ニュース／報道", &[
            Some("定時・総合"), Some("天気"), Some("特集・ドキュメント"), Some("政治・国会"),
            Some("経済・市況"), Some("海外・国際"), Some("解説"), Some("討論・会談"),
            Some("報道特番"), Some("ローカル・地域"), Some("交通"),
            None, None, None, None, Some("その他"),
        ]),
        ("スポーツ", &[
            Some("スポーツニュース"), Some("野球"), Some("サッカー"), Some("ゴルフ"),
            Some("その他の球技"), Some("相撲・格闘技"), Some("オリンピック・国際大会"),
            Some("マラソン・陸上・水泳"), Some("モータースポーツ"),
            Some("マリン・ウィンタースポーツ"), Some("競馬・公営競技"),
            None, None, None, None, Some("その他"),
        ]),
        ("情報／ワイドショー", &[
            Some("芸能・ワイドショー"), Some("ファッション"), Some("暮らし・住まい"),
            Some("健康・医療"), Some("ショッピング・通販"), Some("グルメ・料理"),
            Some("イベント"), Some("番組紹介・お知らせ"),
            None, None, None, None, None, None, None, Some("その他"),
        ]),
        ("ドラマ", &[
            Some("国内ドラマ"), Some("海外ドラマ"), Some("時代劇"),
            None, None, None, None, None, None, None, None, None, None, None, None, Some("その他"),
        ]),
        ("音楽", &[
            Some("国内ロック・ポップス"), Some("海外ロック・ポップス"), Some("クラシック・オペラ"),
            Some("ジャズ・フュージョン"), Some("歌謡曲・演歌"), Some("ライブ・コンサート"),
            Some("ランキング・リクエスト"), Some("カラオケ・のど自慢"), Some("民謡・邦楽"),
            Some("童謡・キッズ"), Some("民族音楽・ワールドミュージック"),
            None, None, None, None, Some("その他"),
        ]),
        ("バラエティ", &[
            Some("クイズ"), Some("ゲーム"), Some("トークバラエティ"), Some("お笑い・コメディ"),
            Some("音楽バラエティ"), Some("旅バラエティ"), Some("料理バラエティ"),
            None, None, None, None, None, None, None, None, Some("その他"),
        ]),
        ("映画", &[
            Some("洋画"), Some("邦画"), Some("アニメ"),
            None, None, None, None, None, None, None, None, None, None, None, None, Some("その他"),
        ]),
        ("アニメ／特撮", &[
            Some("国内アニメ"), Some("海外アニメ"), Some("特撮"),
            None, None, None, None, None, None, None, None, None, None, None, None, Some("その他"),
        ]),
        ("ドキュメンタリー／教養", &[
            Some("社会・時事"), Some("歴史・紀行"), Some("自然・動物・環境"),
            Some("宇宙・科学・医学"), Some("カルチャー・伝統文化"), Some("文学・文芸"),
            Some("スポーツ"), Some("ドキュメンタリー全般"), Some("インタビュー・討論"),
            None, None, None, None, None, None, Some("その他"),
        ]),
        ("劇場／公演", &[
            Some("現代劇・新劇"), Some("ミュージカル"), Some("ダンス・バレエ"),
            Some("落語・演芸"), Some("歌舞伎・古典"),
            None, None, None, None, None, None, None, None, None, None, Some("その他"),
        ]),
        ("趣味／教育", &[
            Some("旅・釣り・アウトドア"), Some("園芸・ペット・手芸"), Some("音楽・美術・工芸"),
            Some("囲碁・将棋"), Some("麻雀・パチンコ"), Some("車・オートバイ"),
            Some("コンピュータ・TVゲーム"), Some("会話・語学"), Some("幼児・小学生"),
            Some("中学生・高校生"), Some("大学生・受験"), Some("生涯学習・資格"),
            Some("教育問題"), None, None, Some("その他"),
        ]),
        ("福祉", &[
            Some("高齢者"), Some("障害者"), Some("社会福祉"), Some("ボランティア"),
            Some("手話"), Some("文字(字幕)"), Some("音声解説"),
            None, None, None, None, None, None, None, None, Some("その他"),
        ]),
    ];

    if level2 < 0 {
        if level1 >= 0 && (level1 as usize) < GENRE_LIST.len() {
            return Some(GENRE_LIST[level1 as usize].0);
        }
        if level1 == GENRE_OTHER {
            return Some("その他");
        }
        return None;
    }

    if level1 >= 0
        && (level1 as usize) < GENRE_LIST.len()
        && level2 >= 0
        && (level2 as usize) < 16
    {
        return GENRE_LIST[level1 as usize].1[level2 as usize];
    }

    None
}

bitflags::bitflags! {
    /// 番組時刻整形のフラグ。原実装 EpgUtil.h:46 FormatEventTimeFlag。
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct FormatEventTimeFlag: u32 {
        /// 時を 2 桁ゼロ埋めで表記する(EpgUtil.h:48 Hour2Digits)。
        const HOUR_2DIGITS = 0x0001;
        /// 開始時刻のみ表記する(EpgUtil.h:49 StartOnly)。
        const START_ONLY = 0x0002;
        /// 日付を付ける(EpgUtil.h:50 Date)。
        const DATE = 0x0004;
        /// 日付に年も付ける(EpgUtil.h:51 Year。DATE と併用)。
        const YEAR = 0x0008;
        /// 終了時刻未定のとき「(終了未定)」を表記する(EpgUtil.h:52 UndecidedText)。
        const UNDECIDED_TEXT = 0x0010;
        /// 表示用時刻への変換を行わない(EpgUtil.h:53 NoConvert)。
        ///
        /// 本移植では EpgTimeToDisplayTime(AppClass 依存)を移植していないため、
        /// 入力は本フラグの有無に関わらず常に表示用時刻として扱われる
        /// (= 常に NoConvert 相当)。API 忠実性のため定義のみ残している。
        const NO_CONVERT = 0x0020;
    }
}

/// 番組時刻テキストの最大長。原実装 EpgUtil.h:57 MAX_EVENT_TIME_LENGTH。
///
/// 原実装はこの長さの TCHAR バッファへ書き込むが、本移植は String を返すため
/// 切り詰めは行わない。定数は API 忠実性のため定義している。
pub const MAX_EVENT_TIME_LENGTH: usize = 64;

/// 曜日テキスト(0=日 〜 6=土)。原実装 Util.cpp:350 GetDayOfWeekText。
///
/// 範囲外は全角「？」(U+FF1F、Util.cpp:353)を返す。
pub fn get_day_of_week_text(day_of_week: i32) -> &'static str {
    const DAY_OF_WEEK_TEXT: [&str; 7] = ["日", "月", "火", "水", "木", "金", "土"];
    if !(0..=6).contains(&day_of_week) {
        return "\u{FF1F}";
    }
    DAY_OF_WEEK_TEXT[day_of_week as usize]
}

/// 番組の開始〜終了時刻を文字列に整形する。
/// 原実装 EpgUtil.cpp:82 FormatEventTime(const SYSTEMTIME &, DWORD, ...)。
///
/// `start_time` は表示用時刻に変換済みであること(モジュール冒頭 doc 参照。
/// 原実装 EpgUtil.cpp:91-95 の EpgTimeToDisplayTime は行わない)。
/// `duration` は秒。0 のとき終了時刻は表記されず、
/// [`FormatEventTimeFlag::UNDECIDED_TEXT`] があれば「(終了未定)」になる
/// (EpgUtil.cpp:126-137)。
///
/// 書式(EpgUtil.cpp:97-145):
/// - 日付部(DATE): `月/日(曜) `、YEAR 併用で `年/` を前置。
/// - 時刻部: HOUR_2DIGITS なら `{:02}:{:02}`、通常 `{}:{:02}`。
/// - START_ONLY でなければ開始と終了を「～」(U+FF5E)で結ぶ
///   (終了が空でも「～」は付く)。
pub fn format_event_time(
    start_time: &SystemTime,
    duration: u32,
    flags: FormatEventTimeFlag,
) -> String {
    // NoConvert 相当: 入力をそのまま表示用時刻として使う(EpgUtil.cpp:91-92)。
    let start = *start_time;

    // 日付部(EpgUtil.cpp:97-113)。
    let mut date = String::new();
    if flags.contains(FormatEventTimeFlag::DATE) {
        if flags.contains(FormatEventTimeFlag::YEAR) {
            date.push_str(&format!("{}/", start.year));
        }
        date.push_str(&format!(
            "{}/{}({}) ",
            start.month,
            start.day,
            get_day_of_week_text(start.day_of_week as i32)
        ));
    }

    // 時刻書式(EpgUtil.cpp:115-116)。
    let hour_2digits = flags.contains(FormatEventTimeFlag::HOUR_2DIGITS);
    let format_time = |hour: u16, minute: u16| -> String {
        if hour_2digits {
            format!("{hour:02}:{minute:02}")
        } else {
            format!("{hour}:{minute:02}")
        }
    };

    let start_text = format_time(start.hour, start.minute);

    // 終了時刻(EpgUtil.cpp:125-138)。
    let mut end_text = String::new();
    if !flags.contains(FormatEventTimeFlag::START_ONLY) {
        if duration > 0 {
            // OffsetSystemTime(EndTime, Duration * SYSTEMTIME_SECOND)相当
            // (EpgUtil.cpp:128-129。SYSTEMTIME_SECOND = 1000ms、Util.h:63)。
            let mut end = start;
            offset_system_time(&mut end, duration as i64 * 1000);
            end_text = format_time(end.hour, end.minute);
        } else if flags.contains(FormatEventTimeFlag::UNDECIDED_TEXT) {
            end_text = "(終了未定)".to_string();
        }
    }

    // 結合(EpgUtil.cpp:140-145)。StartOnly でなければ終了が空でも「～」が付く。
    format!(
        "{}{}{}{}",
        date,
        start_text,
        if !flags.contains(FormatEventTimeFlag::START_ONLY) {
            "\u{FF5E}"
        } else {
            ""
        },
        end_text
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }
    fn s(v: &[u16]) -> String {
        String::from_utf16_lossy(v)
    }

    #[test]
    fn test_get_video_type_sd() {
        // 上位4ビット = 0x0 → SD。下位4ビット 1-4。
        assert_eq!(get_video_type(0x01), VideoType::Sd); // 480i
        assert_eq!(get_video_type(0x03), VideoType::Sd);
        assert_eq!(get_video_type(0xA1), VideoType::Sd); // 上位0xA。
        assert_eq!(get_video_type(0xD3), VideoType::Sd); // 上位0xD。
        assert_eq!(get_video_type(0xF4), VideoType::Sd); // 上位0xF。
    }

    #[test]
    fn test_get_video_type_hd() {
        // 上位4ビット = 0x9 → HD。
        assert_eq!(get_video_type(0x91), VideoType::Hd); // 1080i
        assert_eq!(get_video_type(0xB3), VideoType::Hd);
        assert_eq!(get_video_type(0xC1), VideoType::Hd);
        assert_eq!(get_video_type(0xE4), VideoType::Hd);
    }

    #[test]
    fn test_get_video_type_unknown() {
        // 下位4ビット 0 → Unknown。
        assert_eq!(get_video_type(0x00), VideoType::Unknown);
        assert_eq!(get_video_type(0x90), VideoType::Unknown);
        // 下位4ビット 5以上 → Unknown。
        assert_eq!(get_video_type(0x05), VideoType::Unknown);
        assert_eq!(get_video_type(0x95), VideoType::Unknown);
        // 上位4ビット 1-8 → Unknown(SD/HD のどちらでもない範囲)。
        assert_eq!(get_video_type(0x11), VideoType::Unknown);
        assert_eq!(get_video_type(0x81), VideoType::Unknown);
    }

    #[test]
    fn test_get_event_genre_basic() {
        let info = ContentNibbleInfo {
            nibble_list: vec![ContentNibble { content_nibble_level1: 1, content_nibble_level2: 3 }],
        };
        assert_eq!(get_event_genre(&info), Some((1, 3)));
    }

    #[test]
    fn test_get_event_genre_skip_0xe() {
        // 0xE は拡張ニブルなのでスキップ。次の非0xE が採用される。
        let info = ContentNibbleInfo {
            nibble_list: vec![
                ContentNibble { content_nibble_level1: 0xE, content_nibble_level2: 0 },
                ContentNibble { content_nibble_level1: 5, content_nibble_level2: 2 },
            ],
        };
        assert_eq!(get_event_genre(&info), Some((5, 2)));
    }

    #[test]
    fn test_get_event_genre_all_0xe() {
        let info = ContentNibbleInfo {
            nibble_list: vec![
                ContentNibble { content_nibble_level1: 0xE, content_nibble_level2: 0 },
            ],
        };
        assert_eq!(get_event_genre(&info), None);
    }

    #[test]
    fn test_get_event_genre_empty() {
        let info = ContentNibbleInfo { nibble_list: vec![] };
        assert_eq!(get_event_genre(&info), None);
    }

    #[test]
    fn test_map_arib_symbol_no_match() {
        // 変換なし → そのまま。
        let src = w("Hello");
        assert_eq!(s(&map_arib_symbol(&src)), "Hello");
    }

    #[test]
    fn test_map_arib_symbol_single() {
        // "[字]" → U+1F211(サロゲートペア)。
        let src = w("[字]");
        let result = map_arib_symbol(&src);
        // U+1F211 = サロゲートペア D83C DD11。
        let expected: Vec<u16> = "\u{1f211}".encode_utf16().collect();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_map_arib_symbol_multiple() {
        // "[二][字]" → 2つ連続変換。
        let src = w("[二][字]");
        let result = map_arib_symbol(&src);
        let expected: Vec<u16> = "\u{1f214}\u{1f211}".encode_utf16().collect();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_map_arib_symbol_mixed() {
        // "[字]テスト" → 先頭変換 + 残りはそのまま。
        let src = w("[字]テスト");
        let result = map_arib_symbol(&src);
        let expected: Vec<u16> = "\u{1f211}テスト".encode_utf16().collect();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_map_arib_symbol_bmp_char() {
        // "(秘)" → U+3299(BMP 内、サロゲートなし)。
        let src = w("(秘)");
        let result = map_arib_symbol(&src);
        let expected: Vec<u16> = "\u{3299}".encode_utf16().collect();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_map_arib_symbol_no_partial_match() {
        // "[字" → 閉じ括弧がないので一致なし → そのまま。
        let src = w("[字");
        let result = map_arib_symbol(&src);
        assert_eq!(s(&result), "[字");
    }

    #[test]
    fn test_epg_genre_get_text_level1_only() {
        assert_eq!(epg_genre_get_text(0, -1), Some("ニュース／報道"));
        assert_eq!(epg_genre_get_text(1, -1), Some("スポーツ"));
        assert_eq!(epg_genre_get_text(6, -1), Some("映画"));
        assert_eq!(epg_genre_get_text(11, -1), Some("福祉"));
        // 範囲外。
        assert_eq!(epg_genre_get_text(12, -1), None);
        assert_eq!(epg_genre_get_text(-1, -1), None);
        // GENRE_OTHER(0xF=15)。
        assert_eq!(epg_genre_get_text(GENRE_OTHER, -1), Some("その他"));
    }

    #[test]
    fn test_epg_genre_get_text_level2() {
        assert_eq!(epg_genre_get_text(0, 0), Some("定時・総合"));
        assert_eq!(epg_genre_get_text(0, 1), Some("天気"));
        // nullptr 相当は None。
        assert_eq!(epg_genre_get_text(0, 11), None);
        // "その他"(index 15)。
        assert_eq!(epg_genre_get_text(0, 15), Some("その他"));
        // 範囲外。
        assert_eq!(epg_genre_get_text(0, 16), None);
    }

    // -----------------------------------------------------------------------
    // format_event_time / get_day_of_week_text
    // -----------------------------------------------------------------------

    /// 2024-04-01(月)を基準にしたテスト用時刻。
    fn st(hour: u16, minute: u16) -> SystemTime {
        SystemTime {
            year: 2024,
            month: 4,
            day: 1,
            day_of_week: 1, // 月曜
            hour,
            minute,
            second: 0,
            milliseconds: 0,
        }
    }

    #[test]
    fn test_get_day_of_week_text() {
        assert_eq!(get_day_of_week_text(0), "日");
        assert_eq!(get_day_of_week_text(1), "月");
        assert_eq!(get_day_of_week_text(6), "土");
        // 範囲外は全角「?」(Util.cpp:352-353)。
        assert_eq!(get_day_of_week_text(-1), "\u{FF1F}");
        assert_eq!(get_day_of_week_text(7), "\u{FF1F}");
    }

    #[test]
    fn test_format_event_time_basic() {
        // 通常書式は時をゼロ埋めしない("{}:{:02}"、EpgUtil.cpp:116)。
        assert_eq!(
            format_event_time(&st(9, 5), 3600, FormatEventTimeFlag::empty()),
            "9:05\u{FF5E}10:05"
        );
    }

    #[test]
    fn test_format_event_time_hour_2digits() {
        assert_eq!(
            format_event_time(&st(9, 5), 3600, FormatEventTimeFlag::HOUR_2DIGITS),
            "09:05\u{FF5E}10:05"
        );
    }

    #[test]
    fn test_format_event_time_cross_midnight() {
        // 23:30 + 1時間 → 翌 0:30(OffsetSystemTime で日跨ぎ)。
        assert_eq!(
            format_event_time(&st(23, 30), 3600, FormatEventTimeFlag::empty()),
            "23:30\u{FF5E}0:30"
        );
        assert_eq!(
            format_event_time(&st(23, 30), 3600, FormatEventTimeFlag::HOUR_2DIGITS),
            "23:30\u{FF5E}00:30"
        );
    }

    #[test]
    fn test_format_event_time_undecided() {
        // Duration == 0 + UNDECIDED_TEXT →「(終了未定)」(EpgUtil.cpp:134-137)。
        assert_eq!(
            format_event_time(&st(20, 0), 0, FormatEventTimeFlag::UNDECIDED_TEXT),
            "20:00\u{FF5E}(終了未定)"
        );
        // フラグ無しなら終了は空のまま「～」だけ付く(EpgUtil.cpp:140-145)。
        assert_eq!(
            format_event_time(&st(20, 0), 0, FormatEventTimeFlag::empty()),
            "20:00\u{FF5E}"
        );
    }

    #[test]
    fn test_format_event_time_start_only() {
        // StartOnly では終了時刻も「～」も付かない(Duration があっても)。
        assert_eq!(
            format_event_time(&st(20, 0), 3600, FormatEventTimeFlag::START_ONLY),
            "20:00"
        );
        // StartOnly が優先され UNDECIDED_TEXT は無視される(EpgUtil.cpp:126)。
        assert_eq!(
            format_event_time(
                &st(20, 0),
                0,
                FormatEventTimeFlag::START_ONLY | FormatEventTimeFlag::UNDECIDED_TEXT
            ),
            "20:00"
        );
    }

    #[test]
    fn test_format_event_time_date() {
        // 日付は月日とも非ゼロ埋め + 曜日 + 空白(EpgUtil.cpp:105-110)。
        assert_eq!(
            format_event_time(&st(9, 0), 1800, FormatEventTimeFlag::DATE),
            "4/1(月) 9:00\u{FF5E}9:30"
        );
    }

    #[test]
    fn test_format_event_time_date_year() {
        // YEAR 併用で「年/」を前置(EpgUtil.cpp:100-104)。
        assert_eq!(
            format_event_time(
                &st(9, 0),
                1800,
                FormatEventTimeFlag::DATE | FormatEventTimeFlag::YEAR
            ),
            "2024/4/1(月) 9:00\u{FF5E}9:30"
        );
        // YEAR のみ(DATE 無し)では日付部は付かない(EpgUtil.cpp:98)。
        assert_eq!(
            format_event_time(&st(9, 0), 1800, FormatEventTimeFlag::YEAR),
            "9:00\u{FF5E}9:30"
        );
    }

    #[test]
    fn test_format_event_time_duration_minutes() {
        // 秒単位の Duration(5分番組)。
        assert_eq!(
            format_event_time(&st(9, 58), 300, FormatEventTimeFlag::empty()),
            "9:58\u{FF5E}10:03"
        );
    }

    #[test]
    fn test_format_event_time_flag_values() {
        // EpgUtil.h:46-54 の値と一致すること。
        assert_eq!(FormatEventTimeFlag::HOUR_2DIGITS.bits(), 0x0001);
        assert_eq!(FormatEventTimeFlag::START_ONLY.bits(), 0x0002);
        assert_eq!(FormatEventTimeFlag::DATE.bits(), 0x0004);
        assert_eq!(FormatEventTimeFlag::YEAR.bits(), 0x0008);
        assert_eq!(FormatEventTimeFlag::UNDECIDED_TEXT.bits(), 0x0010);
        assert_eq!(FormatEventTimeFlag::NO_CONVERT.bits(), 0x0020);
        assert_eq!(MAX_EVENT_TIME_LENGTH, 64);
    }
}
