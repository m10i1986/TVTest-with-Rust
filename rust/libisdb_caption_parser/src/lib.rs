// Rust port of LibISDB/TS/CaptionParser.cpp + CaptionParser.hpp
// CaptionParser.cpp:38, CaptionParser.hpp:40
//
// ARIB 字幕(closed caption)の解析。
//
// 原実装は PESParser(PES パケット組み立て)を内包し、その PacketHandler として
// OnPESPacket で PES ペイロードを受け取り、data_group() → caption_management_data()
// / caption_data() → data_unit() の階層を解析する。
//
// 本クレートでは PES パケットの組み立て(libisdb_pes_packet の責務)は呼び出し側に委ね、
// PES ペイロード(&[u8])を入力として data_group 以降の構造解析を行う。
// 字幕本文のテキストと書式情報(FormatInfo: 色・サイズ)は
// libisdb_arib_string::decode_caption_to_string でデコードしてコールバックする。
//
// 移植対象:
//   - OnPESPacket               : CaptionParser.cpp:115 (parse_pes_payload として)
//   - ParseManagementData       : CaptionParser.cpp:166
//   - ParseCaptionData          : CaptionParser.cpp:241
//   - ParseUnitData             : CaptionParser.cpp:280
//   - 言語リスト管理            : GetLanguageIndexByTag:94 等
//
// FormatList(表示位置・色・サイズ等の書式情報)は libisdb_arib_string::decode_caption_to_string
// で取得し、on_caption へ渡す。
//
// スコープ外(原実装の該当箇所はコメントで明示):
//   - DRCS(外字)展開: ParseDRCSUnitData:334。DRCSMap が未移植のため data_unit を
//     読み飛ばす(構造解析は維持)。

use libisdb_crc::crc16_ccitt;
use libisdb_utilities::{load16_be, load24_be};
use libisdb_arib_string::{decode_caption_to_string, DecodeFlags, FormatInfo};
use libisdb_ts_info::{LANGUAGE_CODE_POR, LANGUAGE_CODE_SPA};

/// 言語情報。CaptionParser.hpp:44 LanguageInfo。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct LanguageInfo {
    pub language_tag: u8,
    pub dmf: u8,
    pub dc: u8,
    pub language_code: u32,
    pub format: u8,
    pub tcs: u8,
    pub rollup_mode: u8,
}

/// DRCS(外字)ビットマップ。CaptionParser.hpp:74 DRCSBitmap。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrcsBitmap {
    pub width: u8,
    pub height: u8,
    pub depth: u8,
    pub bits_per_pixel: u8,
    /// ビットマップデータ(width*height*bits_per_pixel ビットをパックしたもの)
    pub data: Vec<u8>,
}

/// 字幕イベントハンドラ。CaptionParser.hpp:63 CaptionHandler。
pub trait CaptionHandler {
    /// 言語情報リストが更新された。OnLanguageUpdate:68。
    fn on_language_update(&mut self, _languages: &[LanguageInfo]) {}
    /// 字幕本文が得られた。OnCaption:69。
    /// `language` はデータグループインデックス(字幕データの場合 DataGroupID & 0x1F)。
    /// `format_list` は文字位置ごとの書式情報(色・サイズ)。
    fn on_caption(&mut self, _language: u8, _text: &str, _format_list: &[FormatInfo]) {}
    /// DRCS(外字)が得られた。CaptionParser.cpp:414 SetDRCS。
    /// `character_code` は DRCS の文字コード。
    fn on_drcs(&mut self, _character_code: u16, _bitmap: &DrcsBitmap) {}
}

/// 字幕解析器。CaptionParser クラス本体。
pub struct CaptionParser {
    one_seg: bool,
    language_list: Vec<LanguageInfo>,
    data_group_version: u8,
    data_group_id: u8,
}

impl CaptionParser {
    /// CaptionParser::CaptionParser (CaptionParser.cpp:38)
    pub fn new(one_seg: bool) -> Self {
        Self {
            one_seg,
            language_list: Vec::new(),
            data_group_version: 0xFF,
            data_group_id: 0x00,
        }
    }

    /// CaptionParser::Reset (CaptionParser.cpp:50)
    pub fn reset(&mut self) {
        self.language_list.clear();
        self.data_group_version = 0xFF;
        self.data_group_id = 0x00;
    }

    pub fn is_1seg(&self) -> bool {
        self.one_seg
    }

    /// GetLanguageCount (CaptionParser.cpp:77)
    pub fn language_count(&self) -> usize {
        self.language_list.len()
    }

    /// GetLanguageInfo (CaptionParser.cpp:83)
    pub fn language_info(&self, index: usize) -> Option<&LanguageInfo> {
        self.language_list.get(index)
    }

    /// GetLanguageIndexByTag (CaptionParser.cpp:94)
    pub fn language_index_by_tag(&self, language_tag: u8) -> Option<usize> {
        self.language_list
            .iter()
            .position(|l| l.language_tag == language_tag)
    }

    /// GetLanguageCodeByTag (CaptionParser.cpp:105)
    pub fn language_code_by_tag(&self, language_tag: u8) -> u32 {
        match self.language_index_by_tag(language_tag) {
            Some(i) => self.language_list[i].language_code,
            None => 0,
        }
    }

    /// PES パケットのペイロードを解析する。OnPESPacket (CaptionParser.cpp:115)。
    ///
    /// `handler` には言語更新・字幕本文が通知される。解析エラー時は false。
    pub fn parse_pes_payload<H: CaptionHandler>(
        &mut self,
        data: &[u8],
        handler: &mut H,
    ) -> bool {
        if data.len() < 3 {
            return false;
        }
        // data_identifier (0x80: 字幕, 0x81: 文字スーパー)
        if data[0] != 0x80 && data[0] != 0x81 {
            return false;
        }
        // private_stream_id
        if data[1] != 0xFF {
            return false;
        }

        // PES_data_packet_header_length
        let header_length = (data[2] & 0x0F) as usize;
        if 3 + header_length + 5 >= data.len() {
            return false;
        }

        let mut pos = 3 + header_length;

        // data_group()
        let data_group_id = data[pos] >> 2;
        let data_group_version = data[pos] & 0x03;
        // data[pos+1]=data_group_link_number, data[pos+2]=last_data_group_link_number
        let data_group_size = load16_be(&data[pos + 3..]) as usize;
        if pos + 5 + data_group_size + 2 > data.len() {
            return false;
        }
        // CRC_16: data_group() 全体(5 + data_group_size + 2)で 0 になるべき
        if crc16_ccitt(&data[pos..pos + 5 + data_group_size + 2], 0x0000) != 0 {
            return false;
        }
        pos += 5;

        if self.data_group_version != data_group_version {
            self.language_list.clear();
            self.data_group_version = data_group_version;
        }
        self.data_group_id = data_group_id;

        if data_group_id == 0x00 || data_group_id == 0x20 {
            // 字幕管理データ
            self.parse_management_data(&data[pos..pos + data_group_size], handler)
        } else {
            // 字幕データ
            self.parse_caption_data(
                &data[pos..pos + data_group_size],
                data_group_id & 0x1F,
                handler,
            )
        }
    }

    /// caption_management_data() (CaptionParser.cpp:166)
    fn parse_management_data<H: CaptionHandler>(
        &mut self,
        data: &[u8],
        handler: &mut H,
    ) -> bool {
        if data.len() < 2 + 5 + 3 {
            return false;
        }

        let mut pos = 0usize;
        let tmd = data[pos] >> 6;
        pos += 1;
        if tmd == 0b10 {
            // OTM (時刻情報) — 5 バイト読み飛ばし
            pos += 5;
        }

        let num_languages = data[pos] as usize;
        pos += 1;
        if pos + num_languages * 5 + 3 > data.len() {
            return false;
        }

        let mut changed = false;
        for _ in 0..num_languages {
            let mut lang = LanguageInfo {
                language_tag: data[pos] >> 5,
                dmf: data[pos] & 0x0F,
                ..Default::default()
            };
            if lang.dmf == 0b1100 || lang.dmf == 0b1101 || lang.dmf == 0b1110 {
                lang.dc = data[pos + 1];
                pos += 1;
            }
            lang.language_code = load24_be(&data[pos + 1..]);
            lang.format = data[pos + 4] >> 4;
            lang.tcs = (data[pos + 4] & 0x0C) >> 2;
            lang.rollup_mode = data[pos + 4] & 0x03;

            match self.language_index_by_tag(lang.language_tag) {
                None => {
                    self.language_list.push(lang);
                    changed = true;
                }
                Some(i) => {
                    if self.language_list[i] != lang {
                        self.language_list[i] = lang;
                        changed = true;
                    }
                }
            }

            pos += 5;
        }

        if changed {
            handler.on_language_update(&self.language_list);
        }

        let unit_loop_length = load24_be(&data[pos..]) as usize;
        pos += 3;
        if unit_loop_length > 0 && pos + unit_loop_length <= data.len() {
            let mut read_size = 0usize;
            while read_size < unit_loop_length {
                let mut size = unit_loop_length - read_size;
                if !self.parse_unit_data(&data[pos + read_size..], &mut size, 0, handler) {
                    return false;
                }
                read_size += size;
            }
        }

        true
    }

    /// caption_data() (CaptionParser.cpp:241)
    fn parse_caption_data<H: CaptionHandler>(
        &mut self,
        data: &[u8],
        data_group_index: u8,
        handler: &mut H,
    ) -> bool {
        if data.len() <= 1 + 3 {
            return false;
        }

        let mut pos = 0usize;
        let tmd = data[pos] >> 6;
        pos += 1;
        if tmd == 0b01 || tmd == 0b10 {
            // STM (時刻情報)
            if pos + 5 + 3 >= data.len() {
                return false;
            }
            pos += 5;
        }

        let unit_loop_length = load24_be(&data[pos..]) as usize;
        pos += 3;
        if unit_loop_length > 0 && pos + unit_loop_length <= data.len() {
            let mut read_size = 0usize;
            while read_size < unit_loop_length {
                let mut size = unit_loop_length - read_size;
                if !self.parse_unit_data(
                    &data[pos + read_size..],
                    &mut size,
                    data_group_index,
                    handler,
                ) {
                    return false;
                }
                read_size += size;
            }
        }

        true
    }

    /// data_unit() (CaptionParser.cpp:280)
    fn parse_unit_data<H: CaptionHandler>(
        &mut self,
        data: &[u8],
        data_size: &mut usize,
        data_group_index: u8,
        handler: &mut H,
    ) -> bool {
        if *data_size < 5 {
            return false;
        }
        // unit_separator
        if data[0] != 0x1F {
            return false;
        }

        let unit_size = load24_be(&data[2..]) as usize;
        if 5 + unit_size > *data_size {
            return false;
        }

        let data_unit_parameter = data[1];

        // DRCS(0x30/0x31): ビットマップを抽出して通知 (ParseDRCSUnitData:334)
        if data_unit_parameter == 0x30 || data_unit_parameter == 0x31 {
            parse_drcs_unit_data(&data[5..5 + unit_size], handler);
            *data_size = 5 + unit_size;
            return true;
        }
        // 本文(0x20)以外は読み飛ばす
        if data_unit_parameter != 0x20 {
            *data_size = 5 + unit_size;
            return true;
        }

        if unit_size > 0 {
            // ARIBStringDecoder::DecodeFlag (CaptionParser.cpp:306)
            let mut flags = DecodeFlags {
                caption: true,
                one_seg: self.one_seg,
                ..Default::default()
            };

            // 言語インデックス決定 (CaptionParser.cpp:308)
            let lang_index: Option<usize> = if data_group_index != 0 {
                self.language_index_by_tag(data_group_index - 1)
            } else if self.language_list.is_empty() {
                None
            } else {
                Some(0)
            };

            // 原則として字幕管理の情報が必要。1Seg は特別扱い (CaptionParser.cpp:314)
            if lang_index.is_some() || self.one_seg {
                if let Some(i) = lang_index {
                    let info = &self.language_list[i];
                    if info.tcs == 1 {
                        flags.ucs = true;
                    }
                    if info.language_code == LANGUAGE_CODE_POR
                        || info.language_code == LANGUAGE_CODE_SPA
                    {
                        flags.latin = true;
                    }
                }
                if let Some((text, format_list)) =
                    decode_caption_to_string(&data[5..5 + unit_size], flags)
                {
                    if !text.is_empty() {
                        handler.on_caption(data_group_index, &text, &format_list);
                    }
                }
            }
        }

        *data_size = 5 + unit_size;
        true
    }
}

/// DRCS の Depth から BitsPerPixel を求める。CaptionParser.cpp:381。
fn drcs_depth_to_bpp(mode: u8, depth: u8) -> u8 {
    if mode == 0x00 {
        1
    } else if depth == 0 {
        1
    } else if depth <= 2 {
        2
    } else if depth <= 6 {
        3
    } else if depth <= 14 {
        4
    } else if depth <= 30 {
        5
    } else if depth <= 62 {
        6
    } else if depth <= 126 {
        7
    } else if depth <= 254 {
        8
    } else {
        9
    }
}

/// data_unit() の DRCS 部分を解析する。ParseDRCSUnitData (CaptionParser.cpp:334)。
///
/// 各文字コードについて、最初のフォント(j==0)のビットマップを on_drcs で通知する。
/// ジオメトリック(Mode > 0x01)は非対応で読み飛ばす。
fn parse_drcs_unit_data<H: CaptionHandler>(data: &[u8], handler: &mut H) -> bool {
    let mut pos = 0usize;
    if data.is_empty() {
        return false;
    }

    let number_of_code = data[pos] as usize;
    pos += 1;

    for _ in 0..number_of_code {
        if pos + 3 > data.len() {
            return false;
        }
        let character_code = load16_be(&data[pos..]);
        let number_of_font = data[pos + 2] as usize;
        pos += 3;

        for j in 0..number_of_font {
            if pos + 1 > data.len() {
                return false;
            }
            let mode = data[pos] & 0x0F;
            pos += 1;

            if mode <= 0x01 {
                if pos + 3 > data.len() {
                    return false;
                }
                let depth = data[pos];
                let width = data[pos + 1];
                let height = data[pos + 2];
                if width == 0 || height == 0 {
                    return false;
                }
                pos += 3;

                let bits_per_pixel = drcs_depth_to_bpp(mode, depth);
                let bitmap_data_size =
                    ((width as usize * height as usize * bits_per_pixel as usize) + 7) >> 3;
                if pos + bitmap_data_size > data.len() {
                    return false;
                }

                if j == 0 {
                    let bitmap = DrcsBitmap {
                        width,
                        height,
                        depth,
                        bits_per_pixel,
                        data: data[pos..pos + bitmap_data_size].to_vec(),
                    };
                    handler.on_drcs(character_code, &bitmap);
                }

                pos += bitmap_data_size;
            } else {
                // ジオメトリック(非対応): RegionX/Y(2) + GeometricDataLength(2) + data
                if pos + 4 > data.len() {
                    return false;
                }
                let geometric_data_length = load16_be(&data[pos + 2..]) as usize;
                pos += 4;
                if pos + geometric_data_length > data.len() {
                    return false;
                }
                pos += geometric_data_length;
            }
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// テスト用ハンドラ: 言語更新と字幕本文(+書式)・DRCS を記録する。
    #[derive(Default)]
    struct RecordHandler {
        languages: Vec<Vec<LanguageInfo>>,
        captions: Vec<(u8, String)>,
        formats: Vec<Vec<FormatInfo>>,
        drcs: Vec<(u16, DrcsBitmap)>,
    }

    impl CaptionHandler for RecordHandler {
        fn on_language_update(&mut self, languages: &[LanguageInfo]) {
            self.languages.push(languages.to_vec());
        }
        fn on_caption(&mut self, language: u8, text: &str, format_list: &[FormatInfo]) {
            self.captions.push((language, text.to_string()));
            self.formats.push(format_list.to_vec());
        }
        fn on_drcs(&mut self, character_code: u16, bitmap: &DrcsBitmap) {
            self.drcs.push((character_code, bitmap.clone()));
        }
    }

    /// CRC16-CCITT(init=0) を付与して data_group() を組み立てる。
    /// 戻り値は PES ペイロード全体。
    fn build_pes_payload(data_group_id: u8, data_group_version: u8, body: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        v.push(0x80); // data_identifier
        v.push(0xFF); // private_stream_id
        v.push(0x00); // PES_data_packet_header_length = 0

        // data_group()
        let mut dg = Vec::new();
        dg.push((data_group_id << 2) | (data_group_version & 0x03));
        dg.push(0x00); // data_group_link_number
        dg.push(0x00); // last_data_group_link_number
        dg.extend_from_slice(&(body.len() as u16).to_be_bytes()); // data_group_size
        dg.extend_from_slice(body);
        // CRC_16 を末尾に付与
        let crc = crc16_ccitt(&dg, 0x0000);
        dg.extend_from_slice(&crc.to_be_bytes());

        v.extend_from_slice(&dg);
        v
    }

    /// 字幕管理データ本文を組み立てる(言語1件)。
    fn build_management_body(lang: &LanguageInfo) -> Vec<u8> {
        let mut b = Vec::new();
        b.push(0x00); // TMD=00(free)
        b.push(0x01); // num_languages = 1
        // language loop (DMF が拡張でない前提で 5 バイト)
        b.push((lang.language_tag << 5) | (lang.dmf & 0x0F));
        b.push((lang.language_code >> 16) as u8);
        b.push((lang.language_code >> 8) as u8);
        b.push((lang.language_code & 0xFF) as u8);
        b.push((lang.format << 4) | ((lang.tcs & 0x03) << 2) | (lang.rollup_mode & 0x03));
        // unit_loop_length = 0
        b.extend_from_slice(&[0x00, 0x00, 0x00]);
        b
    }

    #[test]
    fn test_new_and_reset() {
        let mut p = CaptionParser::new(false);
        assert!(!p.is_1seg());
        assert_eq!(p.language_count(), 0);
        p.reset();
        assert_eq!(p.language_count(), 0);
    }

    #[test]
    fn test_rejects_short_data() {
        let mut p = CaptionParser::new(false);
        let mut h = RecordHandler::default();
        assert!(!p.parse_pes_payload(&[0x80, 0xFF], &mut h));
    }

    #[test]
    fn test_rejects_bad_data_identifier() {
        let mut p = CaptionParser::new(false);
        let mut h = RecordHandler::default();
        assert!(!p.parse_pes_payload(&[0x00, 0xFF, 0x00, 0x00, 0x00, 0x00], &mut h));
    }

    #[test]
    fn test_rejects_bad_private_stream_id() {
        let mut p = CaptionParser::new(false);
        let mut h = RecordHandler::default();
        assert!(!p.parse_pes_payload(&[0x80, 0x00, 0x00, 0x00, 0x00, 0x00], &mut h));
    }

    #[test]
    fn test_rejects_crc_error() {
        let mut p = CaptionParser::new(false);
        let mut h = RecordHandler::default();
        let lang = LanguageInfo {
            language_tag: 0,
            dmf: 0,
            dc: 0,
            language_code: 0x6A706E, // jpn
            format: 7,
            tcs: 0,
            rollup_mode: 0,
        };
        let body = build_management_body(&lang);
        let mut payload = build_pes_payload(0x00, 0, &body);
        // CRC を破壊
        let n = payload.len();
        payload[n - 1] ^= 0xFF;
        assert!(!p.parse_pes_payload(&payload, &mut h));
    }

    #[test]
    fn test_management_data_adds_language() {
        let mut p = CaptionParser::new(false);
        let mut h = RecordHandler::default();
        let lang = LanguageInfo {
            language_tag: 0,
            dmf: 0,
            dc: 0,
            language_code: 0x6A706E, // jpn
            format: 7,
            tcs: 0,
            rollup_mode: 1,
        };
        let body = build_management_body(&lang);
        let payload = build_pes_payload(0x00, 0, &body);
        assert!(p.parse_pes_payload(&payload, &mut h));
        assert_eq!(p.language_count(), 1);
        assert_eq!(p.language_info(0).unwrap().language_code, 0x6A706E);
        assert_eq!(p.language_info(0).unwrap().rollup_mode, 1);
        assert_eq!(h.languages.len(), 1);
    }

    #[test]
    fn test_language_index_and_code_by_tag() {
        let mut p = CaptionParser::new(false);
        let mut h = RecordHandler::default();
        let lang = LanguageInfo {
            language_tag: 2,
            dmf: 0,
            dc: 0,
            language_code: 0x656E67, // eng
            format: 7,
            tcs: 0,
            rollup_mode: 0,
        };
        let body = build_management_body(&lang);
        let payload = build_pes_payload(0x00, 0, &body);
        assert!(p.parse_pes_payload(&payload, &mut h));
        assert_eq!(p.language_index_by_tag(2), Some(0));
        assert_eq!(p.language_index_by_tag(5), None);
        assert_eq!(p.language_code_by_tag(2), 0x656E67);
        assert_eq!(p.language_code_by_tag(5), 0);
    }

    #[test]
    fn test_management_data_version_change_clears() {
        let mut p = CaptionParser::new(false);
        let mut h = RecordHandler::default();
        let lang = LanguageInfo {
            language_tag: 0,
            dmf: 0,
            dc: 0,
            language_code: 0x6A706E,
            format: 7,
            tcs: 0,
            rollup_mode: 0,
        };
        // version 0
        let payload0 = build_pes_payload(0x00, 0, &build_management_body(&lang));
        assert!(p.parse_pes_payload(&payload0, &mut h));
        assert_eq!(p.language_count(), 1);
        // version 1 → リストがクリアされてから追加される
        let lang2 = LanguageInfo {
            language_tag: 1,
            language_code: 0x656E67,
            ..lang
        };
        let payload1 = build_pes_payload(0x00, 1, &build_management_body(&lang2));
        assert!(p.parse_pes_payload(&payload1, &mut h));
        assert_eq!(p.language_count(), 1);
        assert_eq!(p.language_info(0).unwrap().language_tag, 1);
    }

    #[test]
    fn test_caption_data_decodes_text() {
        let mut p = CaptionParser::new(false);
        let mut h = RecordHandler::default();
        // 先に管理データで言語を登録
        let lang = LanguageInfo {
            language_tag: 0,
            dmf: 0,
            dc: 0,
            language_code: 0x6A706E,
            format: 7,
            tcs: 0,
            rollup_mode: 0,
        };
        let mp = build_pes_payload(0x00, 0, &build_management_body(&lang));
        assert!(p.parse_pes_payload(&mp, &mut h));

        // 字幕データ: data_unit(0x20) に ASCII テキスト "AB" を載せる
        // ARIB の英数字符号で 'A'(0x41) 'B'(0x42) は G0(漢字)初期状態だが、
        // 初期 GL=Kanji のため英数字にするには符号集合切替が要る。簡単のため
        // alphanumeric 集合へ LS1(0x0E)等が必要だが、ここでは "数字" を使わず
        // decode の結果が空でないことだけを確認する目的でスペース(0x20)を使う。
        let caption_text: &[u8] = &[0x20, 0x20]; // GL=Kanji 初期。スペース2個
        let mut unit_body = Vec::new();
        // data_unit
        let mut du = Vec::new();
        du.push(0x1F); // unit_separator
        du.push(0x20); // data_unit_parameter = 本文
        du.extend_from_slice(&(caption_text.len() as u32).to_be_bytes()[1..4]); // 24bit unit_size
        du.extend_from_slice(caption_text);

        // caption_data() 本文
        unit_body.push(0x00); // TMD=00
        unit_body.extend_from_slice(&(du.len() as u32).to_be_bytes()[1..4]); // unit_loop_length(24bit)
        unit_body.extend_from_slice(&du);

        // DataGroupID = 0x01 (字幕データ第1言語)
        let cp = build_pes_payload(0x01, 0, &unit_body);
        assert!(p.parse_pes_payload(&cp, &mut h));
        // テキストが通知される(スペースのデコード結果)
        assert!(!h.captions.is_empty());
        assert_eq!(h.captions[0].0, 0x01);
    }

    #[test]
    fn test_caption_data_with_format_info() {
        let mut p = CaptionParser::new(false);
        let mut h = RecordHandler::default();
        let lang = LanguageInfo {
            language_tag: 0,
            dmf: 0,
            dc: 0,
            language_code: 0x6A706E,
            format: 7,
            tcs: 0,
            rollup_mode: 0,
        };
        let mp = build_pes_payload(0x00, 0, &build_management_body(&lang));
        assert!(p.parse_pes_payload(&mp, &mut h));

        // 字幕本文: 色設定(0x81) + スペース。書式情報が通知される
        let caption_text: &[u8] = &[0x81, 0x20];
        let mut du = Vec::new();
        du.push(0x1F);
        du.push(0x20); // 本文
        du.extend_from_slice(&(caption_text.len() as u32).to_be_bytes()[1..4]);
        du.extend_from_slice(caption_text);

        let mut unit_body = Vec::new();
        unit_body.push(0x00);
        unit_body.extend_from_slice(&(du.len() as u32).to_be_bytes()[1..4]);
        unit_body.extend_from_slice(&du);

        let cp = build_pes_payload(0x01, 0, &unit_body);
        assert!(p.parse_pes_payload(&cp, &mut h));
        assert!(!h.captions.is_empty());
        // 書式情報リストに色設定が記録されている
        assert!(!h.formats[0].is_empty());
        assert_eq!(h.formats[0][0].char_color_index, 1);
    }

    #[test]
    fn test_caption_data_malformed_drcs_unit_safe() {
        let mut p = CaptionParser::new(true); // 1Seg
        let mut h = RecordHandler::default();

        // 不正な DRCS data_unit(0x30): number_of_code=0xAA だが本体は足りない
        // → parse_drcs_unit_data は途中で停止し、字幕本文の通知は無い(クラッシュしない)
        let drcs_payload: &[u8] = &[0xAA, 0xBB];
        let mut du = Vec::new();
        du.push(0x1F);
        du.push(0x30); // DRCS
        du.extend_from_slice(&(drcs_payload.len() as u32).to_be_bytes()[1..4]);
        du.extend_from_slice(drcs_payload);

        let mut unit_body = Vec::new();
        unit_body.push(0x00); // TMD
        unit_body.extend_from_slice(&(du.len() as u32).to_be_bytes()[1..4]);
        unit_body.extend_from_slice(&du);

        let cp = build_pes_payload(0x01, 0, &unit_body);
        assert!(p.parse_pes_payload(&cp, &mut h));
        assert!(h.captions.is_empty());
        assert!(h.drcs.is_empty());
    }

    #[test]
    fn test_caption_data_drcs_bitmap_extracted() {
        let mut p = CaptionParser::new(true); // 1Seg
        let mut h = RecordHandler::default();

        // 正常な DRCS data_unit(0x30):
        //   NumberOfCode=1
        //   CharacterCode=0xEC00, NumberOfFont=1
        //   Mode=0x00(1bpp), Depth=0, Width=8, Height=8
        //   BitmapDataSize = (8*8*1 + 7) >> 3 = 8 バイト
        let mut drcs = Vec::new();
        drcs.push(0x01); // NumberOfCode
        drcs.extend_from_slice(&0xEC00u16.to_be_bytes()); // CharacterCode
        drcs.push(0x01); // NumberOfFont
        drcs.push(0x00); // FontID(高位) + Mode=0x00
        drcs.push(0x00); // Depth
        drcs.push(0x08); // Width
        drcs.push(0x08); // Height
        let bitmap_bytes: [u8; 8] = [0x81, 0x42, 0x24, 0x18, 0x18, 0x24, 0x42, 0x81];
        drcs.extend_from_slice(&bitmap_bytes); // 8 バイトのビットマップ

        let mut du = Vec::new();
        du.push(0x1F);
        du.push(0x30); // DRCS
        du.extend_from_slice(&(drcs.len() as u32).to_be_bytes()[1..4]);
        du.extend_from_slice(&drcs);

        let mut unit_body = Vec::new();
        unit_body.push(0x00); // TMD
        unit_body.extend_from_slice(&(du.len() as u32).to_be_bytes()[1..4]);
        unit_body.extend_from_slice(&du);

        let cp = build_pes_payload(0x01, 0, &unit_body);
        assert!(p.parse_pes_payload(&cp, &mut h));

        assert_eq!(h.drcs.len(), 1);
        let (code, bm) = &h.drcs[0];
        assert_eq!(*code, 0xEC00);
        assert_eq!(bm.width, 8);
        assert_eq!(bm.height, 8);
        assert_eq!(bm.bits_per_pixel, 1);
        assert_eq!(bm.data, bitmap_bytes.to_vec());
    }

    #[test]
    fn test_drcs_depth_to_bpp() {
        // Mode=0 は常に 1bpp
        assert_eq!(drcs_depth_to_bpp(0x00, 255), 1);
        // Mode=1: Depth に応じた bpp
        assert_eq!(drcs_depth_to_bpp(0x01, 0), 1);
        assert_eq!(drcs_depth_to_bpp(0x01, 2), 2);
        assert_eq!(drcs_depth_to_bpp(0x01, 6), 3);
        assert_eq!(drcs_depth_to_bpp(0x01, 14), 4);
        assert_eq!(drcs_depth_to_bpp(0x01, 30), 5);
        assert_eq!(drcs_depth_to_bpp(0x01, 62), 6);
        assert_eq!(drcs_depth_to_bpp(0x01, 126), 7);
        assert_eq!(drcs_depth_to_bpp(0x01, 254), 8);
        assert_eq!(drcs_depth_to_bpp(0x01, 255), 9);
    }

    #[test]
    fn test_drcs_geometric_skipped() {
        // ジオメトリック(Mode=0x02)は読み飛ばされ、on_drcs は呼ばれない
        let mut p = CaptionParser::new(true);
        let mut h = RecordHandler::default();

        let geo: [u8; 3] = [0xAA, 0xBB, 0xCC];
        let mut drcs = Vec::new();
        drcs.push(0x01); // NumberOfCode
        drcs.extend_from_slice(&0xEC01u16.to_be_bytes());
        drcs.push(0x01); // NumberOfFont
        drcs.push(0x02); // Mode=0x02 (geometric)
        drcs.push(0x00); // RegionX
        drcs.push(0x00); // RegionY
        drcs.extend_from_slice(&(geo.len() as u16).to_be_bytes()); // GeometricDataLength
        drcs.extend_from_slice(&geo);

        let mut du = Vec::new();
        du.push(0x1F);
        du.push(0x31); // DRCS (0x31 も DRCS)
        du.extend_from_slice(&(drcs.len() as u32).to_be_bytes()[1..4]);
        du.extend_from_slice(&drcs);

        let mut unit_body = Vec::new();
        unit_body.push(0x00);
        unit_body.extend_from_slice(&(du.len() as u32).to_be_bytes()[1..4]);
        unit_body.extend_from_slice(&du);

        let cp = build_pes_payload(0x01, 0, &unit_body);
        assert!(p.parse_pes_payload(&cp, &mut h));
        assert!(h.drcs.is_empty());
    }

    #[test]
    fn test_management_with_extended_dmf() {
        // DMF が拡張(0b1100)の場合、DC バイトが追加で読まれる
        let mut p = CaptionParser::new(false);
        let mut h = RecordHandler::default();

        let mut body = Vec::new();
        body.push(0x00); // TMD=00
        body.push(0x01); // num_languages=1
        body.push((0u8 << 5) | 0b1100); // language_tag=0, DMF=1100(拡張)
        body.push(0x55); // DC
        body.push(0x6A); // language_code...
        body.push(0x70);
        body.push(0x6E);
        body.push((7u8 << 4) | (0 << 2) | 0); // format/tcs/rollup
        body.extend_from_slice(&[0x00, 0x00, 0x00]); // unit_loop_length=0

        let payload = build_pes_payload(0x00, 0, &body);
        assert!(p.parse_pes_payload(&payload, &mut h));
        assert_eq!(p.language_count(), 1);
        assert_eq!(p.language_info(0).unwrap().dc, 0x55);
        assert_eq!(p.language_info(0).unwrap().language_code, 0x6A706E);
    }
}
