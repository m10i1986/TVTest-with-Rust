// LibISDB の ADTSParser.cpp + ADTSParser.hpp を Rust へ移植したもの。
//
// 移植対象:
//   - ADTSFrame     : ADTSParser.hpp:38  (ADTS フレームヘッダ保持)
//     - parse_header  : ADTSParser.cpp:42
//     - get_sampling_freq : ADTSParser.cpp:89
//   - AdtsParser    : ADTSParser.cpp:100 (ADTS フレームをバイトストリームから抽出)
//     - store_es      : ADTSParser.cpp:118
//     - sync_frame    : ADTSParser.cpp:227
//
// C++ の DataBuffer 継承は Vec<u8> で代替する。
// PESPacket 依存の StorePacket / OnPESPacket は libisdb_pes_packet を使わず
// ES スライスを直接受け取る store_es を中心に実装する。

/// サンプリング周波数テーブル。ADTSParser.cpp:91。
const SAMPLING_FREQ_TABLE: [u32; 12] = [
    96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000,
];

/// ADTS ヘッダ情報。ADTSParser.hpp:62。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdtsHeader {
    // adts_fixed_header()
    pub mpeg_version: bool,
    pub protection_absent: bool,
    pub profile: u8,
    pub sampling_freq_index: u8,
    pub private_bit: bool,
    pub channel_config: u8,
    pub original_copy: bool,
    pub home: bool,
    // adts_variable_header()
    pub copyright_id_bit: bool,
    pub copyright_id_start: bool,
    pub frame_length: u16,
    pub buffer_fullness: u16,
    pub raw_data_block_num: u8,
}

/// ADTS フレーム。ADTSParser.hpp:38。
///
/// 生データは Vec<u8> で保持する。
#[derive(Debug, Clone, Default)]
pub struct AdtsFrame {
    pub data: Vec<u8>,
    header: AdtsHeader,
}

impl AdtsFrame {
    pub fn new() -> Self {
        Self::default()
    }

    /// ヘッダを解析する。ADTSParser.cpp:42。
    pub fn parse_header(&mut self) -> bool {
        if self.data.len() < 7 {
            return false;
        }
        let d = &self.data;
        if d[0] != 0xFF || (d[1] & 0xF6) != 0xF0 {
            return false; // syncword 及び layer 異常
        }

        // adts_fixed_header()
        self.header.mpeg_version       = (d[1] & 0x08) != 0;
        self.header.protection_absent  = (d[1] & 0x01) != 0;
        self.header.profile            = (d[2] & 0xC0) >> 6;
        self.header.sampling_freq_index= (d[2] & 0x3C) >> 2;
        self.header.private_bit        = (d[2] & 0x02) != 0;
        self.header.channel_config     = ((d[2] & 0x01) << 2) | ((d[3] & 0xC0) >> 6);
        self.header.original_copy      = (d[3] & 0x20) != 0;
        self.header.home               = (d[3] & 0x10) != 0;

        // adts_variable_header()
        self.header.copyright_id_bit   = (d[3] & 0x08) != 0;
        self.header.copyright_id_start = (d[3] & 0x04) != 0;
        self.header.frame_length       = (((d[3] & 0x03) as u16) << 11)
            | ((d[4] as u16) << 3)
            | ((d[5] as u16 & 0xE0) >> 5);
        self.header.buffer_fullness    = (((d[5] & 0x1F) as u16) << 6)
            | ((d[6] as u16 & 0xFC) >> 2);
        self.header.raw_data_block_num = d[6] & 0x03;

        if self.header.profile == 3 {
            return false; // 未定義のプロファイル
        }
        if self.header.sampling_freq_index > 0x0B {
            return false; // 未定義のサンプリング周波数
        }
        if self.header.channel_config >= 3 && self.header.channel_config != 6 {
            return false; // チャンネル数異常
        }
        let min_len = if self.header.protection_absent { 7u16 } else { 9u16 };
        if self.header.frame_length < min_len {
            return false; // フレーム長異常
        }
        if self.header.raw_data_block_num != 0 {
            return false; // 複数 AAC フレーム非対応
        }

        true
    }

    /// リセットする。ADTSParser.cpp:81。
    pub fn reset(&mut self) {
        self.data.clear();
        self.header = AdtsHeader::default();
    }

    /// データのみクリア(ヘッダは保持)。ADTSParser.cpp:83 (ClearSize 相当)。
    pub fn clear_size(&mut self) {
        self.data.clear();
    }

    /// サンプリング周波数を返す。ADTSParser.cpp:89。
    pub fn get_sampling_freq(&self) -> u32 {
        let idx = self.header.sampling_freq_index as usize;
        if idx < SAMPLING_FREQ_TABLE.len() {
            SAMPLING_FREQ_TABLE[idx]
        } else {
            0
        }
    }

    pub fn get_profile(&self) -> u8 { self.header.profile }
    pub fn get_sampling_freq_index(&self) -> u8 { self.header.sampling_freq_index }
    pub fn get_private_bit(&self) -> bool { self.header.private_bit }
    pub fn get_channel_config(&self) -> u8 { self.header.channel_config }
    pub fn get_original_copy(&self) -> bool { self.header.original_copy }
    pub fn get_home(&self) -> bool { self.header.home }
    pub fn get_copyright_id_bit(&self) -> bool { self.header.copyright_id_bit }
    pub fn get_copyright_id_start(&self) -> bool { self.header.copyright_id_start }
    pub fn get_frame_length(&self) -> u16 { self.header.frame_length }
    pub fn get_buffer_fullness(&self) -> u16 { self.header.buffer_fullness }
    pub fn get_raw_data_block_num(&self) -> u8 { self.header.raw_data_block_num }
    pub fn get_size(&self) -> usize { self.data.len() }
}

/// ADTS パーサー。ADTSParser.cpp:100。
///
/// バイトストリームから ADTS フレームを抽出し、クロージャで通知する。
pub struct AdtsParser {
    frame: AdtsFrame,
    is_storing: bool,
}

impl AdtsParser {
    /// 新規作成。ADTSParser.cpp:100。
    pub fn new() -> Self {
        Self {
            frame: AdtsFrame::new(),
            is_storing: false,
        }
    }

    /// リセットする。ADTSParser.cpp:207。
    pub fn reset(&mut self) {
        self.is_storing = false;
        self.frame.reset();
    }

    /// ES スライスを処理する。ADTSParser.cpp:118。
    ///
    /// フレームが完成するたびに `handler` を呼び出す。
    /// 戻り値はフレームが見つかったかどうか。
    pub fn store_es<F>(&mut self, data: &[u8], handler: &mut F) -> bool
    where
        F: FnMut(&AdtsFrame),
    {
        if data.is_empty() {
            return false;
        }

        let mut frame_found = false;
        let mut pos = 0;

        while pos < data.len() {
            if !self.is_storing {
                // ヘッダを検索する
                if self.sync_frame(data[pos]) {
                    self.is_storing = true;
                    frame_found = true;
                }
                pos += 1;
            } else {
                // データをストアする
                let store_remain = (self.frame.get_frame_length() as usize)
                    .saturating_sub(self.frame.get_size());
                let data_remain = data.len() - pos;

                if store_remain <= data_remain {
                    self.frame.data.extend_from_slice(&data[pos..pos + store_remain]);
                    pos += store_remain;
                    self.is_storing = false;

                    handler(&self.frame);

                    // 次のフレームのためバッファクリア
                    self.frame.clear_size();
                } else {
                    self.frame.data.extend_from_slice(&data[pos..]);
                    break;
                }
            }
        }

        frame_found
    }

    /// 1バイトずつ同期を取る。ADTSParser.cpp:227。
    fn sync_frame(&mut self, byte: u8) -> bool {
        match self.frame.get_size() {
            0 => {
                if byte == 0xFF {
                    self.frame.data.push(byte);
                }
                false
            }
            1 => {
                if (byte & 0xF6) == 0xF0 {
                    self.frame.data.push(byte);
                } else {
                    self.frame.clear_size();
                }
                false
            }
            2..=5 => {
                self.frame.data.push(byte);
                false
            }
            6 => {
                self.frame.data.push(byte);
                if self.frame.parse_header() {
                    true
                } else {
                    self.frame.clear_size();
                    false
                }
            }
            _ => {
                self.frame.clear_size();
                false
            }
        }
    }
}

impl Default for AdtsParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_adts_header(
        profile: u8,
        sampling_freq_index: u8,
        channel_config: u8,
        frame_length: u16,
        protection_absent: bool,
    ) -> Vec<u8> {
        // syncword=0xFFF, ID=0(MPEG-4), layer=00, protection_absent
        let b0: u8 = 0xFF;
        let b1: u8 = 0xF0 | if protection_absent { 0x01 } else { 0x00 };
        let b2: u8 = ((profile & 0x03) << 6)
            | ((sampling_freq_index & 0x0F) << 2)
            | ((channel_config >> 2) & 0x01);
        let b3: u8 = ((channel_config & 0x03) << 6)
            | (((frame_length >> 11) & 0x03) as u8);
        let b4: u8 = ((frame_length >> 3) & 0xFF) as u8;
        let b5: u8 = (((frame_length & 0x07) as u8) << 5) | 0x1F; // buffer_fullness=0x7FF(上位5bit)
        let b6: u8 = 0xFC; // buffer_fullness 下位 6 bit = 0x3F, raw_data_block_num=0
        vec![b0, b1, b2, b3, b4, b5, b6]
    }

    fn make_adts_frame(
        profile: u8,
        sampling_freq_index: u8,
        channel_config: u8,
        audio_data: &[u8],
        protection_absent: bool,
    ) -> Vec<u8> {
        let header_size: usize = if protection_absent { 7 } else { 9 };
        let frame_length = (header_size + audio_data.len()) as u16;
        let mut frame = make_adts_header(profile, sampling_freq_index, channel_config, frame_length, protection_absent);
        frame.extend_from_slice(audio_data);
        frame
    }

    #[test]
    fn test_parse_header_ok() {
        let audio = vec![0u8; 100];
        let frame_data = make_adts_frame(1, 4, 2, &audio, true); // profile=1(LC), 44100Hz, 2ch
        let mut frame = AdtsFrame::new();
        frame.data = frame_data;
        assert!(frame.parse_header());
        assert_eq!(frame.get_profile(), 1);
        assert_eq!(frame.get_sampling_freq_index(), 4);
        assert_eq!(frame.get_channel_config(), 2);
        assert_eq!(frame.get_sampling_freq(), 44100);
        assert!(frame.header.protection_absent);
    }

    #[test]
    fn test_parse_header_bad_syncword() {
        let mut frame = AdtsFrame::new();
        frame.data = vec![0xFE, 0xF1, 0x50, 0x80, 0x00, 0x1F, 0xFC]; // wrong sync
        assert!(!frame.parse_header());
    }

    #[test]
    fn test_parse_header_undefined_profile() {
        // profile=3 は未定義
        let audio = vec![0u8; 10];
        let frame_data = make_adts_frame(3, 4, 2, &audio, true);
        let mut frame = AdtsFrame::new();
        frame.data = frame_data;
        assert!(!frame.parse_header());
    }

    #[test]
    fn test_parse_header_undefined_sampling_freq() {
        // sampling_freq_index=0x0C は未定義
        let audio = vec![0u8; 10];
        let frame_data = make_adts_frame(1, 0x0C, 2, &audio, true);
        let mut frame = AdtsFrame::new();
        frame.data = frame_data;
        assert!(!frame.parse_header());
    }

    #[test]
    fn test_parse_header_invalid_channel() {
        // channel_config=4 は異常 (3,5も同様, ただし6は OK)
        let audio = vec![0u8; 10];
        let frame_data = make_adts_frame(1, 4, 4, &audio, true);
        let mut frame = AdtsFrame::new();
        frame.data = frame_data;
        assert!(!frame.parse_header());
    }

    #[test]
    fn test_parse_header_channel_6_ok() {
        // channel_config=6 は有効 (5.1ch)
        let audio = vec![0u8; 100];
        let frame_data = make_adts_frame(1, 4, 6, &audio, true);
        let mut frame = AdtsFrame::new();
        frame.data = frame_data;
        assert!(frame.parse_header());
        assert_eq!(frame.get_channel_config(), 6);
    }

    #[test]
    fn test_sampling_freq_table() {
        let expected = [96000u32, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000];
        for (idx, &freq) in expected.iter().enumerate() {
            let mut frame = AdtsFrame::new();
            frame.header.sampling_freq_index = idx as u8;
            assert_eq!(frame.get_sampling_freq(), freq, "idx={}", idx);
        }
    }

    #[test]
    fn test_frame_length_calculation() {
        let audio = vec![0xAAu8; 200];
        let frame_data = make_adts_frame(1, 4, 2, &audio, true);
        let mut frame = AdtsFrame::new();
        frame.data = frame_data.clone();
        assert!(frame.parse_header());
        assert_eq!(frame.get_frame_length() as usize, frame_data.len());
    }

    #[test]
    fn test_parser_store_es_single_frame() {
        let audio = vec![0xBBu8; 50];
        let frame_data = make_adts_frame(1, 4, 2, &audio, true);

        let mut parser = AdtsParser::new();
        let mut frames: Vec<Vec<u8>> = Vec::new();
        let found = parser.store_es(&frame_data, &mut |f| {
            frames.push(f.data.clone());
        });
        assert!(found);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0], frame_data);
    }

    #[test]
    fn test_parser_store_es_two_frames() {
        let audio = vec![0xCCu8; 30];
        let frame1 = make_adts_frame(1, 4, 2, &audio, true);
        let frame2 = make_adts_frame(1, 4, 2, &audio, true);
        let mut stream = frame1.clone();
        stream.extend_from_slice(&frame2);

        let mut parser = AdtsParser::new();
        let mut count = 0;
        parser.store_es(&stream, &mut |_| { count += 1; });
        assert_eq!(count, 2);
    }

    #[test]
    fn test_parser_store_es_split_across_calls() {
        let audio = vec![0xDDu8; 50];
        let frame_data = make_adts_frame(1, 4, 2, &audio, true);
        let split = frame_data.len() / 2;

        let mut parser = AdtsParser::new();
        let mut frames: Vec<Vec<u8>> = Vec::new();

        parser.store_es(&frame_data[..split], &mut |f| {
            frames.push(f.data.clone());
        });
        parser.store_es(&frame_data[split..], &mut |f| {
            frames.push(f.data.clone());
        });

        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0], frame_data);
    }

    #[test]
    fn test_parser_reset() {
        let audio = vec![0xEEu8; 30];
        let partial = make_adts_frame(1, 4, 2, &audio, true);

        let mut parser = AdtsParser::new();
        parser.store_es(&partial[..10], &mut |_| {});
        parser.reset();
        assert_eq!(parser.frame.get_size(), 0);
        assert!(!parser.is_storing);
    }

    #[test]
    fn test_parser_invalid_data_skipped() {
        // ランダムデータの後に有効フレームを配置
        let garbage = vec![0x00u8; 20];
        let audio = vec![0xFFu8; 40];
        let valid_frame = make_adts_frame(1, 4, 2, &audio, true);

        let mut stream = garbage;
        stream.extend_from_slice(&valid_frame);

        let mut parser = AdtsParser::new();
        let mut count = 0;
        parser.store_es(&stream, &mut |_| { count += 1; });
        assert_eq!(count, 1);
    }

    #[test]
    fn test_adts_frame_reset() {
        let audio = vec![0u8; 50];
        let frame_data = make_adts_frame(1, 4, 2, &audio, true);
        let mut frame = AdtsFrame::new();
        frame.data = frame_data;
        frame.parse_header();
        frame.reset();
        assert_eq!(frame.get_size(), 0);
        assert_eq!(frame.get_profile(), 0);
        assert_eq!(frame.get_frame_length(), 0);
    }
}
