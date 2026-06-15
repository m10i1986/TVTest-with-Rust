// LibISDB の TSPacket.cpp + TSPacket.hpp を Rust へ移植したもの。
//
// 移植対象:
//   - TSPacket : TSPacket.cpp:79 (MPEG-2 TS パケット解析)
//     - ParsePacket  : TSPacket.cpp:79
//     - ReparsePacket: TSPacket.cpp:143
//     - GetPayloadData / GetPayloadSize : TSPacket.cpp:175
//     - SetPID       : TSPacket.cpp:220
//
// C++ 版は DataBuffer を継承して内部バッファを持つが、
// Rust 版は [u8; TS_PACKET_SIZE] の固定配列を保持し Vec を不要とする。
//
// DataBuffer は Rust の Vec<u8> / slice で自然に代替できるため独立クレートは作らない。

/// TS パケットサイズ。
pub const TS_PACKET_SIZE: usize = 188;

/// NULL PID。
pub const PID_NULL: u16 = 0x1FFF;

/// 連続性カウンタ配列の PID 数 (8192)。
pub const NUM_PID: usize = 8192;

/// パース結果。TSPacket.hpp:78。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseResult {
    /// 正常。
    Ok,
    /// フォーマットエラー。
    FormatError,
    /// トランスポートエラー(ビットエラー)。
    TransportError,
    /// 連続性カウンタエラー(ドロップ)。
    ContinuityError,
}

/// TS パケットヘッダ情報。TSPacket.hpp:43。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TSPacketHeader {
    pub sync_byte: u8,
    pub transport_error_indicator: bool,
    pub payload_unit_start_indicator: bool,
    pub transport_priority: bool,
    pub pid: u16,
    pub transport_scrambling_control: u8,
    pub adaptation_field_control: u8,
    pub continuity_counter: u8,
}

/// アダプテーションフィールドヘッダ情報。TSPacket.hpp:55。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdaptationFieldHeader {
    pub adaptation_field_length: u8,
    pub flags: u8,
    pub discontinuity_indicator: bool,
    pub option_size: u8,
}

/// TS パケット。TSPacket.hpp:39。
///
/// 生データは 188 バイトの固定配列で保持する。
#[derive(Clone)]
pub struct TsPacket {
    pub data: [u8; TS_PACKET_SIZE],
    header: TSPacketHeader,
    adaptation_field: AdaptationFieldHeader,
}

impl Default for TsPacket {
    fn default() -> Self {
        Self {
            data: [0u8; TS_PACKET_SIZE],
            header: TSPacketHeader::default(),
            adaptation_field: AdaptationFieldHeader::default(),
        }
    }
}

impl TsPacket {
    /// 188 バイトのスライスから作成する。TSPacket.cpp:40。
    pub fn new(data: &[u8; TS_PACKET_SIZE]) -> Self {
        let mut pkt = TsPacket::default();
        pkt.data.copy_from_slice(data);
        pkt
    }

    /// TS パケットを解析する。TSPacket.cpp:79。
    ///
    /// `continuity_counters` が `Some` の場合は連続性チェックを行う。
    pub fn parse_packet(&mut self, continuity_counters: Option<&mut [u8; NUM_PID]>) -> ParseResult {
        let hdr = load32_be(&self.data[0..4]);
        self.header.sync_byte                    = (hdr >> 24) as u8;
        self.header.transport_error_indicator    = (hdr & 0x0080_0000) != 0;
        self.header.payload_unit_start_indicator = (hdr & 0x0040_0000) != 0;
        self.header.transport_priority           = (hdr & 0x0020_0000) != 0;
        self.header.pid                          = ((hdr >> 8) & 0x1FFF) as u16;
        self.header.transport_scrambling_control = ((hdr >> 6) & 0x03) as u8;
        self.header.adaptation_field_control     = ((hdr >> 4) & 0x03) as u8;
        self.header.continuity_counter           = (hdr & 0x0F) as u8;

        self.adaptation_field = AdaptationFieldHeader::default();

        if self.header.adaptation_field_control & 0x02 != 0 {
            self.adaptation_field.adaptation_field_length = self.data[4];
            if self.adaptation_field.adaptation_field_length > 0 {
                self.adaptation_field.flags = self.data[5];
                self.adaptation_field.discontinuity_indicator =
                    (self.adaptation_field.flags & 0x80) != 0;
                if self.adaptation_field.adaptation_field_length > 1 {
                    self.adaptation_field.option_size =
                        self.adaptation_field.adaptation_field_length - 1;
                }
            }
        }

        if self.header.sync_byte != 0x47 {
            return ParseResult::FormatError;
        }
        if self.header.transport_error_indicator {
            return ParseResult::TransportError;
        }
        if self.header.pid >= 0x0002 && self.header.pid <= 0x000F {
            return ParseResult::FormatError;
        }
        if self.header.transport_scrambling_control == 0x01 {
            return ParseResult::FormatError;
        }
        if self.header.adaptation_field_control == 0x00 {
            return ParseResult::FormatError;
        }
        if self.header.adaptation_field_control == 0x02
            && self.adaptation_field.adaptation_field_length > 183
        {
            return ParseResult::FormatError;
        }
        if self.header.adaptation_field_control == 0x03
            && self.adaptation_field.adaptation_field_length > 182
        {
            return ParseResult::FormatError;
        }

        if let Some(counters) = continuity_counters {
            if self.header.pid != PID_NULL {
                let idx = self.header.pid as usize;
                let old_counter = counters[idx];
                let new_counter = if self.header.adaptation_field_control & 0x01 != 0 {
                    self.header.continuity_counter
                } else {
                    0x10
                };
                counters[idx] = new_counter;

                if !self.adaptation_field.discontinuity_indicator
                    && old_counter < 0x10
                    && new_counter < 0x10
                    && ((old_counter + 1) & 0x0F) != new_counter
                {
                    return ParseResult::ContinuityError;
                }
            }
        }

        ParseResult::Ok
    }

    /// 以前に parse_packet が成功したデータを再解析する。TSPacket.cpp:143。
    pub fn reparse_packet(&mut self) {
        let hdr = load32_be(&self.data[0..4]);
        self.header.payload_unit_start_indicator = (hdr & 0x0040_0000) != 0;
        self.header.transport_priority           = (hdr & 0x0020_0000) != 0;
        self.header.pid                          = ((hdr >> 8) & 0x1FFF) as u16;
        self.header.transport_scrambling_control = ((hdr >> 6) & 0x03) as u8;
        self.header.adaptation_field_control     = ((hdr >> 4) & 0x03) as u8;
        self.header.continuity_counter           = (hdr & 0x0F) as u8;

        self.adaptation_field = AdaptationFieldHeader::default();

        if self.header.adaptation_field_control & 0x02 != 0 {
            self.adaptation_field.adaptation_field_length = self.data[4];
            if self.adaptation_field.adaptation_field_length > 0 {
                self.adaptation_field.flags = self.data[5];
                self.adaptation_field.discontinuity_indicator =
                    (self.adaptation_field.flags & 0x80) != 0;
                if self.adaptation_field.adaptation_field_length > 1 {
                    self.adaptation_field.option_size =
                        self.adaptation_field.adaptation_field_length - 1;
                }
            }
        }
    }

    /// ペイロード先頭スライスを返す。TSPacket.cpp:175。
    pub fn get_payload_data(&self) -> Option<&[u8]> {
        let start = self.payload_start_pos()?;
        Some(&self.data[start..])
    }

    /// ペイロードサイズを返す。TSPacket.cpp:205。
    pub fn get_payload_size(&self) -> u8 {
        match self.header.adaptation_field_control {
            1 => (TS_PACKET_SIZE - 4) as u8,
            3 => {
                let afl = self.adaptation_field.adaptation_field_length as usize;
                if afl + 5 >= TS_PACKET_SIZE {
                    0
                } else {
                    (TS_PACKET_SIZE - afl - 5) as u8
                }
            }
            _ => 0,
        }
    }

    /// PID を返す。TSPacket.hpp:99。
    pub fn get_pid(&self) -> u16 { self.header.pid }

    /// PID を設定する。TSPacket.cpp:220。
    pub fn set_pid(&mut self, pid: u16) {
        let pid = pid & 0x1FFF;
        self.data[1] = (self.data[1] & 0xE0) | ((pid >> 8) as u8);
        self.data[2] = pid as u8;
        self.header.pid = pid;
    }

    pub fn get_transport_error_indicator(&self) -> bool { self.header.transport_error_indicator }
    pub fn get_payload_unit_start_indicator(&self) -> bool { self.header.payload_unit_start_indicator }
    pub fn get_transport_priority(&self) -> bool { self.header.transport_priority }
    pub fn get_transport_scrambling_control(&self) -> u8 { self.header.transport_scrambling_control }
    pub fn have_adaptation_field(&self) -> bool { self.header.adaptation_field_control & 0x02 != 0 }
    pub fn have_payload(&self) -> bool { self.header.adaptation_field_control & 0x01 != 0 }
    pub fn is_scrambled(&self) -> bool { self.header.transport_scrambling_control & 0x02 != 0 }
    pub fn get_discontinuity_indicator(&self) -> bool { self.adaptation_field.discontinuity_indicator }
    pub fn get_random_access_indicator(&self) -> bool { self.adaptation_field.flags & 0x40 != 0 }
    pub fn get_es_priority_indicator(&self) -> bool { self.adaptation_field.flags & 0x20 != 0 }
    pub fn get_pcr_flag(&self) -> bool { self.adaptation_field.flags & 0x10 != 0 }
    pub fn get_opcr_flag(&self) -> bool { self.adaptation_field.flags & 0x08 != 0 }
    pub fn get_splicing_point_flag(&self) -> bool { self.adaptation_field.flags & 0x04 != 0 }
    pub fn get_transport_private_data_flag(&self) -> bool { self.adaptation_field.flags & 0x02 != 0 }
    pub fn get_adaptation_field_ext_flag(&self) -> bool { self.adaptation_field.flags & 0x01 != 0 }
    pub fn get_option_size(&self) -> u8 { self.adaptation_field.option_size }
    pub fn get_option_data(&self) -> Option<&[u8]> {
        let sz = self.adaptation_field.option_size as usize;
        if sz > 0 {
            Some(&self.data[6..6 + sz])
        } else {
            None
        }
    }
    pub fn get_continuity_counter(&self) -> u8 { self.header.continuity_counter }

    fn payload_start_pos(&self) -> Option<usize> {
        match self.header.adaptation_field_control {
            1 => Some(4),
            3 => {
                let pos = self.adaptation_field.adaptation_field_length as usize + 5;
                if pos < TS_PACKET_SIZE { Some(pos) } else { None }
            }
            _ => None,
        }
    }
}

#[inline]
fn load32_be(p: &[u8]) -> u32 {
    ((p[0] as u32) << 24) | ((p[1] as u32) << 16) | ((p[2] as u32) << 8) | (p[3] as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_packet(pid: u16, has_payload: bool, pusi: bool, cc: u8) -> [u8; TS_PACKET_SIZE] {
        let mut data = [0u8; TS_PACKET_SIZE];
        data[0] = 0x47;
        let pusi_bit: u8 = if pusi { 0x40 } else { 0 };
        let afc: u8 = if has_payload { 0x01 } else { 0x02 };
        data[1] = pusi_bit | ((pid >> 8) as u8 & 0x1F);
        data[2] = pid as u8;
        data[3] = (afc << 4) | (cc & 0x0F);
        data
    }

    fn make_af_packet(pid: u16, afl: u8, flags: u8, cc: u8) -> [u8; TS_PACKET_SIZE] {
        let mut data = [0u8; TS_PACKET_SIZE];
        data[0] = 0x47;
        data[1] = (pid >> 8) as u8 & 0x1F;
        data[2] = pid as u8;
        data[3] = (0x03u8 << 4) | (cc & 0x0F); // afc=3 (AF+payload)
        data[4] = afl;
        if afl > 0 {
            data[5] = flags;
        }
        data
    }

    #[test]
    fn test_parse_ok_payload_only() {
        let data = make_packet(0x101, true, false, 5);
        let mut pkt = TsPacket::new(&data);
        assert_eq!(pkt.parse_packet(None), ParseResult::Ok);
        assert_eq!(pkt.get_pid(), 0x101);
        assert!(pkt.have_payload());
        assert!(!pkt.have_adaptation_field());
        assert_eq!(pkt.get_continuity_counter(), 5);
    }

    #[test]
    fn test_parse_bad_sync() {
        let mut data = make_packet(0x101, true, false, 0);
        data[0] = 0x00;
        let mut pkt = TsPacket::new(&data);
        assert_eq!(pkt.parse_packet(None), ParseResult::FormatError);
    }

    #[test]
    fn test_parse_transport_error() {
        let mut data = make_packet(0x101, true, false, 0);
        data[1] |= 0x80;
        let mut pkt = TsPacket::new(&data);
        assert_eq!(pkt.parse_packet(None), ParseResult::TransportError);
    }

    #[test]
    fn test_parse_reserved_pid_range() {
        let data = make_packet(0x0005, true, false, 0);
        let mut pkt = TsPacket::new(&data);
        assert_eq!(pkt.parse_packet(None), ParseResult::FormatError);
    }

    #[test]
    fn test_parse_pid_boundary_low() {
        let data = make_packet(0x0001, true, false, 0);
        let mut pkt = TsPacket::new(&data);
        assert_eq!(pkt.parse_packet(None), ParseResult::Ok);
    }

    #[test]
    fn test_parse_pid_boundary_high() {
        let data = make_packet(0x0010, true, false, 0);
        let mut pkt = TsPacket::new(&data);
        assert_eq!(pkt.parse_packet(None), ParseResult::Ok);
    }

    #[test]
    fn test_payload_unit_start_indicator() {
        let data = make_packet(0x200, true, true, 0);
        let mut pkt = TsPacket::new(&data);
        pkt.parse_packet(None);
        assert!(pkt.get_payload_unit_start_indicator());
    }

    #[test]
    fn test_get_payload_data_payload_only() {
        let data = make_packet(0x101, true, false, 0);
        let mut pkt = TsPacket::new(&data);
        pkt.parse_packet(None);
        let payload = pkt.get_payload_data().unwrap();
        assert_eq!(payload.len(), TS_PACKET_SIZE - 4);
        assert_eq!(pkt.get_payload_size(), 184);
    }

    #[test]
    fn test_get_payload_data_af_and_payload() {
        let data = make_af_packet(0x101, 1, 0x00, 0);
        let mut pkt = TsPacket::new(&data);
        pkt.parse_packet(None);
        let payload = pkt.get_payload_data().unwrap();
        assert_eq!(payload.len(), TS_PACKET_SIZE - 6);
        assert_eq!(pkt.get_payload_size(), 182);
    }

    #[test]
    fn test_get_payload_data_af_only() {
        let mut data = [0u8; TS_PACKET_SIZE];
        data[0] = 0x47;
        data[1] = 0x01;
        data[2] = 0x00;
        data[3] = 0x02 << 4; // afc=2 (AF only)
        data[4] = 183;
        let mut pkt = TsPacket::new(&data);
        pkt.parse_packet(None);
        assert!(pkt.get_payload_data().is_none());
        assert_eq!(pkt.get_payload_size(), 0);
    }

    #[test]
    fn test_set_pid() {
        let data = make_packet(0x100, true, false, 0);
        let mut pkt = TsPacket::new(&data);
        pkt.parse_packet(None);
        pkt.set_pid(0x0501);
        assert_eq!(pkt.get_pid(), 0x0501);
        assert_eq!(pkt.data[1] & 0x1F, 0x05);
        assert_eq!(pkt.data[2], 0x01);
    }

    #[test]
    fn test_continuity_check_ok() {
        let mut counters = Box::new([0x10u8; NUM_PID]);
        let pid = 0x100u16;
        for cc in 0u8..16 {
            let data = make_packet(pid, true, false, cc);
            let mut pkt = TsPacket::new(&data);
            assert_eq!(pkt.parse_packet(Some(&mut counters)), ParseResult::Ok, "cc={}", cc);
        }
        let data = make_packet(pid, true, false, 0);
        let mut pkt = TsPacket::new(&data);
        assert_eq!(pkt.parse_packet(Some(&mut counters)), ParseResult::Ok);
    }

    #[test]
    fn test_continuity_check_error() {
        let mut counters = Box::new([0x10u8; NUM_PID]);
        let pid = 0x100u16;
        let data = make_packet(pid, true, false, 0);
        let mut pkt = TsPacket::new(&data);
        pkt.parse_packet(Some(&mut counters));
        let data = make_packet(pid, true, false, 2);
        let mut pkt = TsPacket::new(&data);
        assert_eq!(pkt.parse_packet(Some(&mut counters)), ParseResult::ContinuityError);
    }

    #[test]
    fn test_null_pid_no_continuity_check() {
        let mut counters = Box::new([0x10u8; NUM_PID]);
        let data = make_packet(PID_NULL, true, false, 5);
        let mut pkt = TsPacket::new(&data);
        assert_eq!(pkt.parse_packet(Some(&mut counters)), ParseResult::Ok);
        assert_eq!(counters[PID_NULL as usize], 0x10);
    }

    #[test]
    fn test_adaptation_field_flags() {
        let data = make_af_packet(0x200, 1, 0x50, 0); // RAF | PCR
        let mut pkt = TsPacket::new(&data);
        pkt.parse_packet(None);
        assert!(pkt.have_adaptation_field());
        assert!(pkt.get_random_access_indicator());
        assert!(pkt.get_pcr_flag());
        assert!(!pkt.get_discontinuity_indicator());
    }

    #[test]
    fn test_discontinuity_indicator() {
        let data = make_af_packet(0x200, 1, 0x80, 0);
        let mut pkt = TsPacket::new(&data);
        pkt.parse_packet(None);
        assert!(pkt.get_discontinuity_indicator());
    }

    #[test]
    fn test_is_scrambled() {
        let mut data = make_packet(0x100, true, false, 0);
        data[3] = (data[3] & 0x3F) | 0x80; // tsc = 0b10 → scrambled
        let mut pkt = TsPacket::new(&data);
        pkt.parse_packet(None);
        assert!(pkt.is_scrambled());
    }

    #[test]
    fn test_reparse_packet() {
        let data = make_packet(0x300, true, true, 7);
        let mut pkt = TsPacket::new(&data);
        pkt.parse_packet(None);
        pkt.reparse_packet();
        assert_eq!(pkt.get_pid(), 0x300);
        assert!(pkt.get_payload_unit_start_indicator());
        assert_eq!(pkt.get_continuity_counter(), 7);
    }

    #[test]
    fn test_pid_null_value() {
        assert_eq!(PID_NULL, 0x1FFF);
    }

    #[test]
    fn test_scrambling_control_undefined() {
        let mut data = make_packet(0x100, true, false, 0);
        data[3] = (data[3] & 0xCF) | 0x40; // tsc = 0b01 → undefined
        let mut pkt = TsPacket::new(&data);
        assert_eq!(pkt.parse_packet(None), ParseResult::FormatError);
    }

    #[test]
    fn test_afc_zero_is_error() {
        let mut data = make_packet(0x100, true, false, 0);
        data[3] = data[3] & 0x0F; // afc=0 → undefined
        let mut pkt = TsPacket::new(&data);
        assert_eq!(pkt.parse_packet(None), ParseResult::FormatError);
    }

    #[test]
    fn test_adaptation_field_length_overflow_afc2() {
        // afc=2 (AF only) で afl > 183 → FormatError
        let mut data = [0u8; TS_PACKET_SIZE];
        data[0] = 0x47;
        data[1] = 0x01;
        data[2] = 0x00;
        data[3] = 0x02 << 4; // afc=2
        data[4] = 184;       // > 183
        let mut pkt = TsPacket::new(&data);
        assert_eq!(pkt.parse_packet(None), ParseResult::FormatError);
    }

    #[test]
    fn test_adaptation_field_length_overflow_afc3() {
        // afc=3 (AF+payload) で afl > 182 → FormatError
        let data = make_af_packet(0x100, 183, 0x00, 0); // afl=183 > 182
        let mut pkt = TsPacket::new(&data);
        assert_eq!(pkt.parse_packet(None), ParseResult::FormatError);
    }
}
