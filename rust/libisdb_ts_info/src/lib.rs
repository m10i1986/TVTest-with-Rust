// LibISDB の TSInformation.cpp + LibISDBConsts.hpp を Rust へ移植したもの。
//
// 移植対象:
//   - 定数群                : LibISDBConsts.hpp (PID, STREAM_TYPE, LANGUAGE_CODE 等)
//   - get_stream_type_text  : GetStreamTypeText:34 (stream_type → テキスト)
//   - get_area_text_ja      : GetAreaText_ja:79 (地域コード → 日本語テキスト)
//   - get_video_component_type_text_ja / get_audio_component_type_text_ja
//   - get_predefined_pid_text : GetPredefinedPIDText:227 (既定義 PID → テキスト)
//   - get_language_text_ja  : GetLanguageText_ja:249 (ISO 639 言語コード → 日本語テキスト)
//   - language_code_to_text : LanguageCodeToText:295 (言語コード → 3 文字 ASCII)

// --- PID 定数 ---
pub const PID_PAT: u16  = 0x0000;
pub const PID_CAT: u16  = 0x0001;
pub const PID_NIT: u16  = 0x0010;
pub const PID_SDT: u16  = 0x0011;
pub const PID_HEIT: u16 = 0x0012;
pub const PID_TOT: u16  = 0x0014;
pub const PID_SDTT: u16 = 0x0023;
pub const PID_BIT: u16  = 0x0024;
pub const PID_NBIT: u16 = 0x0025;
pub const PID_MEIT: u16 = 0x0026;
pub const PID_LEIT: u16 = 0x0027;
pub const PID_CDT: u16  = 0x0029;
pub const PID_NULL: u16 = 0x1FFF;
pub const PID_MAX: u16  = 0x1FFF;
pub const PID_INVALID: u16 = 0xFFFF;

pub const ONESEG_PMT_PID_FIRST: u16 = 0x1FC8;
pub const ONESEG_PMT_PID_LAST: u16  = 0x1FCF;

pub fn is_1seg_pmt_pid(pid: u16) -> bool {
    pid >= ONESEG_PMT_PID_FIRST && pid <= ONESEG_PMT_PID_LAST
}

// --- stream_type 定数 ---
pub const STREAM_TYPE_MPEG1_VIDEO: u8                   = 0x01;
pub const STREAM_TYPE_MPEG2_VIDEO: u8                   = 0x02;
pub const STREAM_TYPE_MPEG1_AUDIO: u8                   = 0x03;
pub const STREAM_TYPE_MPEG2_AUDIO: u8                   = 0x04;
pub const STREAM_TYPE_PRIVATE_SECTIONS: u8              = 0x05;
pub const STREAM_TYPE_PRIVATE_DATA: u8                  = 0x06;
pub const STREAM_TYPE_MHEG: u8                          = 0x07;
pub const STREAM_TYPE_DSM_CC: u8                        = 0x08;
pub const STREAM_TYPE_ITU_T_REC_H222_1: u8              = 0x09;
pub const STREAM_TYPE_ISO_IEC_13818_6_TYPE_A: u8        = 0x0A;
pub const STREAM_TYPE_ISO_IEC_13818_6_TYPE_B: u8        = 0x0B;
pub const STREAM_TYPE_ISO_IEC_13818_6_TYPE_C: u8        = 0x0C;
pub const STREAM_TYPE_ISO_IEC_13818_6_TYPE_D: u8        = 0x0D;
pub const STREAM_TYPE_ISO_IEC_13818_1_AUXILIARY: u8     = 0x0E;
pub const STREAM_TYPE_AAC: u8                           = 0x0F;
pub const STREAM_TYPE_MPEG4_VISUAL: u8                  = 0x10;
pub const STREAM_TYPE_MPEG4_AUDIO: u8                   = 0x11;
pub const STREAM_TYPE_ISO_IEC_14496_1_IN_PES: u8        = 0x12;
pub const STREAM_TYPE_ISO_IEC_14496_1_IN_SECTIONS: u8   = 0x13;
pub const STREAM_TYPE_ISO_IEC_13818_6_DOWNLOAD: u8      = 0x14;
pub const STREAM_TYPE_METADATA_IN_PES: u8               = 0x15;
pub const STREAM_TYPE_METADATA_IN_SECTIONS: u8          = 0x16;
pub const STREAM_TYPE_METADATA_IN_DATA_CAROUSEL: u8     = 0x17;
pub const STREAM_TYPE_METADATA_IN_OBJECT_CAROUSEL: u8   = 0x18;
pub const STREAM_TYPE_METADATA_IN_DOWNLOAD_PROTOCOL: u8 = 0x19;
pub const STREAM_TYPE_IPMP: u8                          = 0x1A;
pub const STREAM_TYPE_H264: u8                          = 0x1B;
pub const STREAM_TYPE_H265: u8                          = 0x24;
pub const STREAM_TYPE_USER_PRIVATE: u8                  = 0x80;
pub const STREAM_TYPE_AC3: u8                           = 0x81;
pub const STREAM_TYPE_DTS: u8                           = 0x82;
pub const STREAM_TYPE_TRUEHD: u8                        = 0x83;
pub const STREAM_TYPE_DOLBY_DIGITAL_PLUS: u8            = 0x87;
pub const STREAM_TYPE_UNINITIALIZED: u8                 = 0x00;
pub const STREAM_TYPE_INVALID: u8                       = 0xFF;
pub const STREAM_TYPE_CAPTION: u8                       = STREAM_TYPE_PRIVATE_DATA;
pub const STREAM_TYPE_DATA_CARROUSEL: u8                = STREAM_TYPE_ISO_IEC_13818_6_TYPE_D;

// --- ISO 639 言語コード ---
pub const LANGUAGE_CODE_JPN: u32     = 0x6A706E; // jpn
pub const LANGUAGE_CODE_ENG: u32     = 0x656E67; // eng
pub const LANGUAGE_CODE_DEU: u32     = 0x646575; // deu
pub const LANGUAGE_CODE_FRA: u32     = 0x667261; // fra
pub const LANGUAGE_CODE_ITA: u32     = 0x697461; // ita
pub const LANGUAGE_CODE_RUS: u32     = 0x727573; // rus
pub const LANGUAGE_CODE_ZHO: u32     = 0x7A686F; // zho
pub const LANGUAGE_CODE_KOR: u32     = 0x6B6F72; // kor
pub const LANGUAGE_CODE_POR: u32     = 0x706F72; // por
pub const LANGUAGE_CODE_SPA: u32     = 0x737061; // spa
pub const LANGUAGE_CODE_ETC: u32     = 0x657463; // etc
pub const LANGUAGE_CODE_INVALID: u32 = 0x000000;

// --- 関数群 ---

/// stream_type 値をテキスト名に変換する。TSInformation.cpp:34。
pub fn get_stream_type_text(stream_type: u8) -> Option<&'static str> {
    match stream_type {
        STREAM_TYPE_MPEG1_VIDEO                   => Some("MPEG-1 Video"),
        STREAM_TYPE_MPEG2_VIDEO                   => Some("MPEG-2 Video"),
        STREAM_TYPE_MPEG1_AUDIO                   => Some("MPEG-1 Audio"),
        STREAM_TYPE_MPEG2_AUDIO                   => Some("MPEG-2 Audio"),
        STREAM_TYPE_PRIVATE_SECTIONS              => Some("private_sections"),
        STREAM_TYPE_PRIVATE_DATA                  => Some("private data"),
        STREAM_TYPE_MHEG                          => Some("MHEG"),
        STREAM_TYPE_DSM_CC                        => Some("DSM-CC"),
        STREAM_TYPE_ITU_T_REC_H222_1              => Some("H.222.1"),
        STREAM_TYPE_ISO_IEC_13818_6_TYPE_A        => Some("ISO/IEC 13818-6 type A"),
        STREAM_TYPE_ISO_IEC_13818_6_TYPE_B        => Some("ISO/IEC 13818-6 type B"),
        STREAM_TYPE_ISO_IEC_13818_6_TYPE_C        => Some("ISO/IEC 13818-6 type C"),
        STREAM_TYPE_ISO_IEC_13818_6_TYPE_D        => Some("ISO/IEC 13818-6 type D"),
        STREAM_TYPE_ISO_IEC_13818_1_AUXILIARY     => Some("auxiliary"),
        STREAM_TYPE_AAC                           => Some("AAC"),
        STREAM_TYPE_MPEG4_VISUAL                  => Some("MPEG-4 Visual"),
        STREAM_TYPE_MPEG4_AUDIO                   => Some("MPEG-4 Audio"),
        STREAM_TYPE_ISO_IEC_14496_1_IN_PES        => Some("ISO/IEC 14496-1 in PES packets"),
        STREAM_TYPE_ISO_IEC_14496_1_IN_SECTIONS   => Some("ISO/IEC 14496-1 in ISO/IEC 14496_sections"),
        STREAM_TYPE_ISO_IEC_13818_6_DOWNLOAD      => Some("ISO/IEC 13818-6 Synchronized Download Protocol"),
        STREAM_TYPE_METADATA_IN_PES               => Some("Metadata in PES packets"),
        STREAM_TYPE_METADATA_IN_SECTIONS          => Some("Metadata in metadata_sections"),
        STREAM_TYPE_METADATA_IN_DATA_CAROUSEL     => Some("Metadata in ISO/IEC 13818-6 Data Carousel"),
        STREAM_TYPE_METADATA_IN_OBJECT_CAROUSEL   => Some("Metadata in ISO/IEC 13818-6 Object Carousel"),
        STREAM_TYPE_METADATA_IN_DOWNLOAD_PROTOCOL => Some("Metadata in ISO/IEC 13818-6 Synchronized Download Protocol"),
        STREAM_TYPE_IPMP                          => Some("IPMP"),
        STREAM_TYPE_H264                          => Some("H.264"),
        STREAM_TYPE_H265                          => Some("H.265"),
        STREAM_TYPE_USER_PRIVATE                  => Some("user private"),
        STREAM_TYPE_AC3                           => Some("AC-3"),
        STREAM_TYPE_DTS                           => Some("DTS"),
        STREAM_TYPE_TRUEHD                        => Some("TrueHD"),
        STREAM_TYPE_DOLBY_DIGITAL_PLUS            => Some("Dolby Digital Plus"),
        _                                         => None,
    }
}

/// 地域コードを日本語テキストに変換する。TSInformation.cpp:79。
pub fn get_area_text_ja(area_code: u16) -> Option<&'static str> {
    match area_code {
        0x5A5 => Some("関東広域圏"),
        0x72A => Some("中京広域圏"),
        0x8D5 => Some("近畿広域圏"),
        0x699 => Some("鳥取・島根圏"),
        0x553 => Some("岡山・香川圏"),
        0x16B => Some("北海道"),
        0x467 => Some("青森"),
        0x5D4 => Some("岩手"),
        0x758 => Some("宮城"),
        0xAC6 => Some("秋田"),
        0xE4C => Some("山形"),
        0x1AE => Some("福島"),
        0xC69 => Some("茨城"),
        0xE38 => Some("栃木"),
        0x98B => Some("群馬"),
        0x64B => Some("埼玉"),
        0x1C7 => Some("千葉"),
        0xAAC => Some("東京"),
        0x56C => Some("神奈川"),
        0x4CE => Some("新潟"),
        0x539 => Some("富山"),
        0x6A6 => Some("石川"),
        0x92D => Some("福井"),
        0xD4A => Some("山梨"),
        0x9D2 => Some("長野"),
        0xA65 => Some("岐阜"),
        0xA5A => Some("静岡"),
        0x966 => Some("愛知"),
        0x2DC => Some("三重"),
        0xCE4 => Some("滋賀"),
        0x59A => Some("京都"),
        0xCB2 => Some("大阪"),
        0x674 => Some("兵庫"),
        0xA93 => Some("奈良"),
        0x396 => Some("和歌山"),
        0xD23 => Some("鳥取"),
        0x31B => Some("島根"),
        0x2B5 => Some("岡山"),
        0xB31 => Some("広島"),
        0xB98 => Some("山口"),
        0xE62 => Some("徳島"),
        0x9B4 => Some("香川"),
        0x19D => Some("愛媛"),
        0x2E3 => Some("高知"),
        0x62D => Some("福岡"),
        0x959 => Some("佐賀"),
        0xA2B => Some("長崎"),
        0x8A7 => Some("熊本"),
        0xC8D => Some("大分"),
        0xD1C => Some("宮崎"),
        0xD45 => Some("鹿児島"),
        0x372 => Some("沖縄"),
        _     => None,
    }
}

/// ビデオコンポーネントタイプを日本語テキストに変換する。TSInformation.cpp:157。
pub fn get_video_component_type_text_ja(component_type: u8) -> Option<&'static str> {
    match component_type {
        0x01 => Some("480i[4:3]"),
        0x02 => Some("480i[16:9] パンベクトルあり"),
        0x03 => Some("480i[16:9]"),
        0x04 => Some("480i[>16:9]"),
        0x83 => Some("4320p[16:9]"),
        0x91 => Some("2160p[4:3]"),
        0x92 => Some("2160p[16:9] パンベクトルあり"),
        0x93 => Some("2160p[16:9]"),
        0x94 => Some("2160p[>16:9]"),
        0xA1 => Some("480p[4:3]"),
        0xA2 => Some("480p[16:9] パンベクトルあり"),
        0xA3 => Some("480p[16:9]"),
        0xA4 => Some("480p[>16:9]"),
        0xB1 => Some("1080i[4:3]"),
        0xB2 => Some("1080i[16:9] パンベクトルあり"),
        0xB3 => Some("1080i[16:9]"),
        0xB4 => Some("1080i[>16:9]"),
        0xC1 => Some("720p[4:3]"),
        0xC2 => Some("720p[16:9] パンベクトルあり"),
        0xC3 => Some("720p[16:9]"),
        0xC4 => Some("720p[>16:9]"),
        0xD1 => Some("240p[4:3]"),
        0xD2 => Some("240p[16:9] パンベクトルあり"),
        0xD3 => Some("240p[16:9]"),
        0xD4 => Some("240p[>16:9]"),
        0xE1 => Some("1080p[4:3]"),
        0xE2 => Some("1080p[16:9] パンベクトルあり"),
        0xE3 => Some("1080p[16:9]"),
        0xE4 => Some("1080p[>16:9]"),
        0xF1 => Some("180p[4:3]"),
        0xF2 => Some("180p[16:9] パンベクトルあり"),
        0xF3 => Some("180p[16:9]"),
        0xF4 => Some("180p[>16:9]"),
        _    => None,
    }
}

/// オーディオコンポーネントタイプを日本語テキストに変換する。TSInformation.cpp:199。
pub fn get_audio_component_type_text_ja(component_type: u8) -> Option<&'static str> {
    match component_type {
        0x01 => Some("Mono"),
        0x02 => Some("Dual mono"),
        0x03 => Some("Stereo"),
        0x04 => Some("3ch[2/1]"),
        0x05 => Some("3ch[3/0]"),
        0x06 => Some("4ch[2/2]"),
        0x07 => Some("4ch[3/1]"),
        0x08 => Some("5ch"),
        0x09 => Some("5.1ch"),
        0x0A => Some("6.1ch[3/3.1]"),
        0x0B => Some("6.1ch[2/0/0-2/0/2-0.1]"),
        0x0C => Some("7.1ch[5/2.1]"),
        0x0D => Some("7.1ch[3/2/2.1]"),
        0x0E => Some("7.1ch[2/0/0-3/0/2-0.1]"),
        0x0F => Some("7.1ch[0/2/0-3/0/2-0.1]"),
        0x10 => Some("10.2ch"),
        0x11 => Some("22.2ch"),
        0x40 => Some("視覚障害者用音声解説"),
        0x41 => Some("聴覚障害者用音声"),
        _    => None,
    }
}

/// コンポーネントタイプを日本語テキストに変換する。TSInformation.cpp:142。
pub fn get_component_type_text_ja(stream_content: u8, component_type: u8) -> Option<&'static str> {
    match stream_content {
        0x01 | 0x05 => get_video_component_type_text_ja(component_type),
        0x02        => get_audio_component_type_text_ja(component_type),
        _           => None,
    }
}

/// 既定義 PID をテキストに変換する。TSInformation.cpp:227。
pub fn get_predefined_pid_text(pid: u16) -> Option<&'static str> {
    match pid {
        PID_PAT  => Some("PAT"),
        PID_CAT  => Some("CAT"),
        PID_NIT  => Some("NIT"),
        PID_SDT  => Some("SDT"),
        PID_HEIT => Some("H-EIT"),
        PID_TOT  => Some("TOT"),
        PID_SDTT => Some("SDTT"),
        PID_BIT  => Some("BIT"),
        PID_NBIT => Some("NBIT"),
        PID_MEIT => Some("M-EIT"),
        PID_LEIT => Some("L-EIT"),
        PID_CDT  => Some("CDT"),
        PID_NULL => Some("Null"),
        _        => None,
    }
}

/// ISO 639 言語コードの表示種別。TSInformation.hpp:41。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LanguageTextType {
    #[default]
    Long,
    Simple,
    Short,
}

/// 言語コードを日本語テキストに変換する。TSInformation.cpp:249。
/// 未知の言語コードは `language_code_to_text` にフォールバックする。
pub fn get_language_text_ja(
    language_code: u32,
    text_type: LanguageTextType,
) -> Option<String> {
    #[allow(clippy::type_complexity)]
    static LANGUAGE_LIST: &[(u32, &str, &str, &str)] = &[
        (LANGUAGE_CODE_JPN, "日本語",       "日本語", "日"),
        (LANGUAGE_CODE_ENG, "英語",         "英語",   "英"),
        (LANGUAGE_CODE_DEU, "ドイツ語",     "独語",   "独"),
        (LANGUAGE_CODE_FRA, "フランス語",   "仏語",   "仏"),
        (LANGUAGE_CODE_ITA, "イタリア語",   "伊語",   "伊"),
        (LANGUAGE_CODE_RUS, "ロシア語",     "露語",   "露"),
        (LANGUAGE_CODE_ZHO, "中国語",       "中国語", "中"),
        (LANGUAGE_CODE_KOR, "韓国語",       "韓国語", "韓"),
        (LANGUAGE_CODE_POR, "ポルトガル語", "葡語",   "葡"),
        (LANGUAGE_CODE_SPA, "スペイン語",   "西語",   "西"),
        (LANGUAGE_CODE_ETC, "外国語",       "外国語", "外"),
    ];

    for &(code, long, simple, short) in LANGUAGE_LIST {
        if code == language_code {
            let text = match text_type {
                LanguageTextType::Long   => long,
                LanguageTextType::Simple => simple,
                LanguageTextType::Short  => short,
            };
            return Some(text.to_owned());
        }
    }

    language_code_to_text(language_code, true)
}

/// 言語コードを 3 文字 ASCII テキストに変換する。TSInformation.cpp:295。
/// `upper_case=true` の場合は大文字化する。
pub fn language_code_to_text(language_code: u32, upper_case: bool) -> Option<String> {
    let b0 = ((language_code >> 16) & 0xFF) as u8;
    let b1 = ((language_code >>  8) & 0xFF) as u8;
    let b2 = (language_code & 0xFF) as u8;

    if !b0.is_ascii() || !b1.is_ascii() || !b2.is_ascii() {
        return None;
    }

    let mut text = String::with_capacity(3);
    if upper_case {
        text.push((b0 as char).to_ascii_uppercase());
        text.push((b1 as char).to_ascii_uppercase());
        text.push((b2 as char).to_ascii_uppercase());
    } else {
        text.push(b0 as char);
        text.push(b1 as char);
        text.push(b2 as char);
    }
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_type_known() {
        assert_eq!(get_stream_type_text(STREAM_TYPE_MPEG2_VIDEO), Some("MPEG-2 Video"));
        assert_eq!(get_stream_type_text(STREAM_TYPE_H264), Some("H.264"));
        assert_eq!(get_stream_type_text(STREAM_TYPE_AAC), Some("AAC"));
        assert_eq!(get_stream_type_text(STREAM_TYPE_AC3), Some("AC-3"));
    }

    #[test]
    fn test_stream_type_unknown() {
        assert_eq!(get_stream_type_text(0x99), None);
        assert_eq!(get_stream_type_text(STREAM_TYPE_UNINITIALIZED), None);
    }

    #[test]
    fn test_area_text_ja_known() {
        assert_eq!(get_area_text_ja(0x5A5), Some("関東広域圏"));
        assert_eq!(get_area_text_ja(0xAAC), Some("東京"));
        assert_eq!(get_area_text_ja(0x16B), Some("北海道"));
        assert_eq!(get_area_text_ja(0x372), Some("沖縄"));
    }

    #[test]
    fn test_area_text_ja_unknown() {
        assert_eq!(get_area_text_ja(0x000), None);
    }

    #[test]
    fn test_video_component_type() {
        assert_eq!(get_video_component_type_text_ja(0x01), Some("480i[4:3]"));
        assert_eq!(get_video_component_type_text_ja(0xB3), Some("1080i[16:9]"));
        assert_eq!(get_video_component_type_text_ja(0xC3), Some("720p[16:9]"));
        assert_eq!(get_video_component_type_text_ja(0xFF), None);
    }

    #[test]
    fn test_audio_component_type() {
        assert_eq!(get_audio_component_type_text_ja(0x01), Some("Mono"));
        assert_eq!(get_audio_component_type_text_ja(0x03), Some("Stereo"));
        assert_eq!(get_audio_component_type_text_ja(0x09), Some("5.1ch"));
        assert_eq!(get_audio_component_type_text_ja(0xFF), None);
    }

    #[test]
    fn test_component_type_dispatch() {
        // stream_content=1 → video
        assert_eq!(get_component_type_text_ja(0x01, 0x01), Some("480i[4:3]"));
        assert_eq!(get_component_type_text_ja(0x05, 0x01), Some("480i[4:3]"));
        // stream_content=2 → audio
        assert_eq!(get_component_type_text_ja(0x02, 0x03), Some("Stereo"));
        // unknown stream_content
        assert_eq!(get_component_type_text_ja(0x03, 0x01), None);
    }

    #[test]
    fn test_predefined_pid_text() {
        assert_eq!(get_predefined_pid_text(PID_PAT), Some("PAT"));
        assert_eq!(get_predefined_pid_text(PID_NIT), Some("NIT"));
        assert_eq!(get_predefined_pid_text(PID_NULL), Some("Null"));
        assert_eq!(get_predefined_pid_text(0x0100), None);
    }

    #[test]
    fn test_language_text_ja_known() {
        assert_eq!(
            get_language_text_ja(LANGUAGE_CODE_JPN, LanguageTextType::Long),
            Some("日本語".to_owned())
        );
        assert_eq!(
            get_language_text_ja(LANGUAGE_CODE_ENG, LanguageTextType::Short),
            Some("英".to_owned())
        );
        assert_eq!(
            get_language_text_ja(LANGUAGE_CODE_ZHO, LanguageTextType::Simple),
            Some("中国語".to_owned())
        );
    }

    #[test]
    fn test_language_text_ja_fallback() {
        // 未知コードは ASCII 変換にフォールバック
        let code = 0x666672; // "ffr"
        let result = get_language_text_ja(code, LanguageTextType::Long);
        assert_eq!(result, Some("FFR".to_owned()));
    }

    #[test]
    fn test_language_code_to_text_upper() {
        assert_eq!(language_code_to_text(LANGUAGE_CODE_JPN, true), Some("JPN".to_owned()));
        assert_eq!(language_code_to_text(LANGUAGE_CODE_ENG, true), Some("ENG".to_owned()));
    }

    #[test]
    fn test_language_code_to_text_lower() {
        assert_eq!(language_code_to_text(LANGUAGE_CODE_JPN, false), Some("jpn".to_owned()));
    }

    #[test]
    fn test_is_1seg_pmt_pid() {
        assert!(is_1seg_pmt_pid(0x1FC8));
        assert!(is_1seg_pmt_pid(0x1FCF));
        assert!(!is_1seg_pmt_pid(0x1FC7));
        assert!(!is_1seg_pmt_pid(0x1FD0));
    }

    #[test]
    fn test_pid_constants() {
        assert_eq!(PID_PAT, 0x0000);
        assert_eq!(PID_NULL, 0x1FFF);
        assert_eq!(PID_MAX, 0x1FFF);
    }

    #[test]
    fn test_stream_type_aliases() {
        assert_eq!(STREAM_TYPE_CAPTION, STREAM_TYPE_PRIVATE_DATA);
        assert_eq!(STREAM_TYPE_DATA_CARROUSEL, STREAM_TYPE_ISO_IEC_13818_6_TYPE_D);
    }
}
