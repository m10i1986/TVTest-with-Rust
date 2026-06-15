// Rust port of LibISDB/TS/OneSegPATGenerator.cpp + OneSegPATGenerator.hpp
// OneSegPATGenerator.cpp:39, OneSegPATGenerator.hpp:39
//
// ワンセグ放送には PAT が含まれないことがあるため、NIT/PMT から PAT を生成する。
//
// 原実装は PIDMapManager + PSITableBase(コールバック) でパケットをルーティングして
// NIT / PMT をパースしているが、その部分(NITMultiTable/PMTTable のパース)は
// 既存の libisdb_ts_tables / libisdb_psi_table クレートが担う責務である。
// 本クレートでは OneSegPATGenerator 固有のロジック、すなわち
//   - PMT 受信回数に基づく「PAT を生成すべきか」の判定 (StorePacket:63)
//   - TSID の管理 (SetTransportStreamID:177 / OnNITSection:186)
//   - PAT TS パケットのバイト列生成 (GetPATPacket:95)
// を純粋なロジックとして移植する。NIT/PMT のパースは呼び出し側が行い、
// その結果(TSID やサービス ID と PMT PID の対応)を本構造体へ通知する。

use libisdb_ts_packet::TS_PACKET_SIZE;
use libisdb_ts_info::{ONESEG_PMT_PID_FIRST, ONESEG_PMT_PID_LAST};
use libisdb_crc::crc32_mpeg2;
use libisdb_utilities::store32_be;

/// 無効な TSID (LibISDBConsts.hpp:37 TRANSPORT_STREAM_ID_INVALID)
pub const TRANSPORT_STREAM_ID_INVALID: u16 = 0x0000;

/// ワンセグ PMT PID の個数 (LibISDBConsts.hpp:70)
pub const ONESEG_PMT_PID_COUNT: usize =
    (ONESEG_PMT_PID_LAST - ONESEG_PMT_PID_FIRST + 1) as usize;

/// PMT が PAT_GEN_PMT_COUNT 回来る間に PAT が来なければ PAT 無しとみなす
/// (OneSegPATGenerator.cpp:74)
const PAT_GEN_PMT_COUNT: u8 = 5;

/// PID がワンセグ PMT PID 範囲かどうか (LibISDBConsts.hpp:72 Is1SegPMTPID)
#[inline]
pub fn is_1seg_pmt_pid(pid: u16) -> bool {
    (ONESEG_PMT_PID_FIRST..=ONESEG_PMT_PID_LAST).contains(&pid)
}

/// ワンセグ PAT 生成器。
///
/// 原実装の OneSegPATGenerator から、PID マップ・PSI テーブルパースを除いた
/// 状態管理部分。
#[derive(Debug, Clone)]
pub struct OneSegPatGenerator {
    transport_stream_id: u16,
    has_pat: bool,
    generate_pat: bool,
    continuity_counter: u8,
    /// 各ワンセグ PMT PID の受信回数 (OneSegPATGenerator.hpp:57 m_PMTCount)
    pmt_count: [u8; ONESEG_PMT_PID_COUNT],
}

impl Default for OneSegPatGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl OneSegPatGenerator {
    /// OneSegPATGenerator::OneSegPATGenerator (OneSegPATGenerator.cpp:39)
    pub fn new() -> Self {
        let mut s = Self {
            transport_stream_id: TRANSPORT_STREAM_ID_INVALID,
            has_pat: false,
            generate_pat: false,
            continuity_counter: 0,
            pmt_count: [0; ONESEG_PMT_PID_COUNT],
        };
        s.reset();
        s
    }

    /// OneSegPATGenerator::Reset (OneSegPATGenerator.cpp:45)
    ///
    /// 原実装では PID マップ(NIT/PMT)の再構築も行うが、その責務は呼び出し側にある。
    pub fn reset(&mut self) {
        self.transport_stream_id = TRANSPORT_STREAM_ID_INVALID;
        self.has_pat = false;
        self.generate_pat = false;
        self.continuity_counter = 0;
        self.pmt_count = [0; ONESEG_PMT_PID_COUNT];
    }

    /// PAT パケットを受信したことを通知する。
    /// (StorePacket:67 の `PID == PID_PAT` 分岐)
    pub fn notify_pat_received(&mut self) {
        self.has_pat = true;
    }

    /// NIT セクションから得られた TSID を通知する。
    /// 部分受信記述子が存在しサービスがある場合のみ有効な TSID を、
    /// それ以外は TRANSPORT_STREAM_ID_INVALID を渡す。
    /// (OnNITSection:186)
    pub fn notify_nit_transport_stream_id(&mut self, transport_stream_id: u16) {
        if self.transport_stream_id != transport_stream_id {
            self.transport_stream_id = transport_stream_id;
            self.has_pat = false;
        }
    }

    /// 指定したワンセグ PMT PID の PMT セクションを受信したことを通知する。
    /// 戻り値が true のとき、PAT を生成して出力すべきタイミングである。
    /// (StorePacket:69 の PMT 分岐)
    ///
    /// `pid` がワンセグ PMT PID 範囲外の場合は何もせず false を返す。
    pub fn notify_pmt_section(&mut self, pid: u16) -> bool {
        if !is_1seg_pmt_pid(pid) {
            return false;
        }
        if self.has_pat {
            return false;
        }

        if !self.generate_pat {
            let index = (pid - ONESEG_PMT_PID_FIRST) as usize;
            if self.pmt_count[index] < PAT_GEN_PMT_COUNT {
                self.pmt_count[index] += 1;
                if self.pmt_count[index] == PAT_GEN_PMT_COUNT {
                    self.generate_pat = true;
                }
            }
        }

        self.generate_pat && (self.transport_stream_id != TRANSPORT_STREAM_ID_INVALID)
    }

    /// TSID が予め分かっている場合に指定することで、NIT を待たずに PAT を生成できる。
    /// 既に TSID が設定済みの場合は false を返す。
    /// (SetTransportStreamID:177)
    pub fn set_transport_stream_id(&mut self, transport_stream_id: u16) -> bool {
        if self.transport_stream_id != TRANSPORT_STREAM_ID_INVALID {
            return false;
        }
        self.transport_stream_id = transport_stream_id;
        true
    }

    /// 現在の TSID を返す。
    pub fn transport_stream_id(&self) -> u16 {
        self.transport_stream_id
    }

    /// PAT を生成すべき状態かどうか。
    pub fn is_generate_pat(&self) -> bool {
        self.generate_pat
    }

    /// PAT TS パケットを生成する。(GetPATPacket:95)
    ///
    /// `pmt_list` は ONESEG_PMT_PID の順(先頭 = ONESEG_PMT_PID_FIRST)に並んだ
    /// 各 PMT のサービス ID。`None` または `Some(0)` は「その PID に有効な PMT 無し」。
    /// 先頭(index 0)の PMT が無い場合は `None` を返す(GetPATPacket:108)。
    /// TSID が未設定の場合も `None`。
    ///
    /// 成功時は 188 バイトの TS パケットを返し、内部の継続性カウンタを 1 進める。
    pub fn generate_pat_packet(
        &mut self,
        pmt_list: &[Option<u16>],
    ) -> Option<[u8; TS_PACKET_SIZE]> {
        if self.transport_stream_id == TRANSPORT_STREAM_ID_INVALID {
            return None;
        }

        // 有効な PMT(サービス ID != 0)を ONESEG_PMT_PID 範囲で集計
        let mut entries: Vec<(u16, u16)> = Vec::with_capacity(ONESEG_PMT_PID_COUNT); // (service_id, pmt_pid)
        for i in 0..ONESEG_PMT_PID_COUNT {
            let service_id = pmt_list.get(i).copied().flatten().unwrap_or(0);
            if service_id != 0 {
                entries.push((service_id, ONESEG_PMT_PID_FIRST + i as u16));
            } else if i == 0 {
                // 先頭 PMT が無い (GetPATPacket:108)
                return None;
            }
        }

        let pmt_count = entries.len();
        // SectionLength = 5 + (PMTCount + 1) * 4 + 4 (GetPATPacket:114)
        let section_length: u16 = 5 + ((pmt_count as u16) + 1) * 4 + 4;

        let mut data = [0u8; TS_PACKET_SIZE];

        // TS header (GetPATPacket:122)
        data[0] = 0x47; // Sync
        data[1] = 0x60;
        data[2] = 0x00;
        data[3] = 0x10 | (self.continuity_counter & 0x0F);
        data[4] = 0x00; // pointer_field

        // PAT (GetPATPacket:129)
        data[5] = 0x00; // table_id
        data[6] = 0xF0 | (section_length >> 8) as u8;
        data[7] = (section_length & 0xFF) as u8;
        data[8] = (self.transport_stream_id >> 8) as u8;
        data[9] = (self.transport_stream_id & 0xFF) as u8;
        data[10] = 0xC1; // reserved(2)+version(5)+current_next_indicator(1)
        data[11] = 0x00; // section_number
        data[12] = 0x00; // last_section_number

        // network_PID エントリ (program_number = 0 → NIT PID = 0x0010)
        data[13] = 0x00;
        data[14] = 0x00;
        data[15] = 0xE0; // reserved(3) + NIT PID(high)
        data[16] = 0x10; // NIT PID(low)

        let mut pos = 17;
        for &(service_id, pid) in &entries {
            data[pos] = (service_id >> 8) as u8;
            data[pos + 1] = (service_id & 0xFF) as u8;
            data[pos + 2] = 0xE0 | (pid >> 8) as u8;
            data[pos + 3] = (pid & 0xFF) as u8;
            pos += 4;
        }

        // CRC32 (GetPATPacket:156) — section_length に対応する範囲 = data[5..](8 + (PMTCount+1)*4 バイト)
        let crc_len = 8 + (pmt_count + 1) * 4;
        let crc = crc32_mpeg2(&data[5..5 + crc_len], 0xFFFF_FFFF);
        store32_be(&mut data[pos..pos + 4], crc);
        pos += 4;

        // 残りを 0xFF で埋める (GetPATPacket:159)
        for b in data.iter_mut().take(TS_PACKET_SIZE).skip(pos) {
            *b = 0xFF;
        }

        self.continuity_counter = self.continuity_counter.wrapping_add(1);

        Some(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_1seg_pmt_pid() {
        assert!(!is_1seg_pmt_pid(ONESEG_PMT_PID_FIRST - 1));
        assert!(is_1seg_pmt_pid(ONESEG_PMT_PID_FIRST));
        assert!(is_1seg_pmt_pid(0x1FCC));
        assert!(is_1seg_pmt_pid(ONESEG_PMT_PID_LAST));
        assert!(!is_1seg_pmt_pid(ONESEG_PMT_PID_LAST + 1));
    }

    #[test]
    fn test_pmt_pid_count() {
        assert_eq!(ONESEG_PMT_PID_COUNT, 8);
    }

    #[test]
    fn test_new_is_reset() {
        let g = OneSegPatGenerator::new();
        assert_eq!(g.transport_stream_id(), TRANSPORT_STREAM_ID_INVALID);
        assert!(!g.is_generate_pat());
    }

    #[test]
    fn test_set_transport_stream_id_once() {
        let mut g = OneSegPatGenerator::new();
        assert!(g.set_transport_stream_id(0x1234));
        assert_eq!(g.transport_stream_id(), 0x1234);
        // 2回目は失敗
        assert!(!g.set_transport_stream_id(0x5678));
        assert_eq!(g.transport_stream_id(), 0x1234);
    }

    #[test]
    fn test_pmt_count_triggers_generate_after_5() {
        let mut g = OneSegPatGenerator::new();
        g.set_transport_stream_id(0x0AAA);
        let pid = ONESEG_PMT_PID_FIRST;
        // 4回目までは generate にならない
        for _ in 0..4 {
            assert!(!g.notify_pmt_section(pid));
            assert!(!g.is_generate_pat());
        }
        // 5回目で generate になり、TSID 設定済みなので true
        assert!(g.notify_pmt_section(pid));
        assert!(g.is_generate_pat());
    }

    #[test]
    fn test_pmt_section_without_tsid_no_trigger() {
        let mut g = OneSegPatGenerator::new();
        let pid = ONESEG_PMT_PID_FIRST;
        for _ in 0..10 {
            // TSID 未設定なので false のまま
            assert!(!g.notify_pmt_section(pid));
        }
        assert!(g.is_generate_pat()); // generate フラグ自体は立つ
    }

    #[test]
    fn test_pat_received_suppresses_generation() {
        let mut g = OneSegPatGenerator::new();
        g.set_transport_stream_id(0x0AAA);
        g.notify_pat_received();
        let pid = ONESEG_PMT_PID_FIRST;
        for _ in 0..10 {
            assert!(!g.notify_pmt_section(pid));
        }
        assert!(!g.is_generate_pat());
    }

    #[test]
    fn test_notify_pmt_out_of_range() {
        let mut g = OneSegPatGenerator::new();
        g.set_transport_stream_id(0x0AAA);
        assert!(!g.notify_pmt_section(0x0100)); // 範囲外
        assert!(!g.is_generate_pat());
    }

    #[test]
    fn test_generate_pat_packet_no_tsid() {
        let mut g = OneSegPatGenerator::new();
        let pmt_list = [Some(0x0400u16)];
        assert!(g.generate_pat_packet(&pmt_list).is_none());
    }

    #[test]
    fn test_generate_pat_packet_no_first_pmt() {
        let mut g = OneSegPatGenerator::new();
        g.set_transport_stream_id(0x0AAA);
        // 先頭 PMT が無い
        let pmt_list = [None, Some(0x0400u16)];
        assert!(g.generate_pat_packet(&pmt_list).is_none());
    }

    #[test]
    fn test_generate_pat_packet_single_service() {
        let mut g = OneSegPatGenerator::new();
        let tsid = 0x7FE0;
        g.set_transport_stream_id(tsid);
        let pmt_list = [Some(0x0400u16)];
        let pkt = g.generate_pat_packet(&pmt_list).expect("should generate");

        // TS header
        assert_eq!(pkt[0], 0x47);
        assert_eq!(pkt[1], 0x60); // PID_PAT(0x0000) high + payload_unit_start
        assert_eq!(pkt[2], 0x00);
        assert_eq!(pkt[3] & 0xF0, 0x10); // adaptation=01, CC=0
        assert_eq!(pkt[4], 0x00); // pointer_field

        // PAT
        assert_eq!(pkt[5], 0x00); // table_id
        // SectionLength = 5 + (1+1)*4 + 4 = 17
        let section_length = (((pkt[6] & 0x0F) as u16) << 8) | pkt[7] as u16;
        assert_eq!(section_length, 17);
        // TSID
        assert_eq!(((pkt[8] as u16) << 8) | pkt[9] as u16, tsid);
        assert_eq!(pkt[10], 0xC1);

        // network entry (program 0 → NIT 0x0010)
        assert_eq!(pkt[13], 0x00);
        assert_eq!(pkt[14], 0x00);
        assert_eq!(((pkt[15] as u16 & 0x1F) << 8) | pkt[16] as u16, 0x0010);

        // service entry
        assert_eq!(((pkt[17] as u16) << 8) | pkt[18] as u16, 0x0400);
        assert_eq!(((pkt[19] as u16 & 0x1F) << 8) | pkt[20] as u16, ONESEG_PMT_PID_FIRST);

        // CRC は data[5..5+16] に対する MPEG2 CRC
        let crc = crc32_mpeg2(&pkt[5..5 + 16], 0xFFFF_FFFF);
        let stored = ((pkt[21] as u32) << 24)
            | ((pkt[22] as u32) << 16)
            | ((pkt[23] as u32) << 8)
            | pkt[24] as u32;
        assert_eq!(stored, crc);

        // 末尾は 0xFF
        assert_eq!(pkt[25], 0xFF);
        assert_eq!(pkt[TS_PACKET_SIZE - 1], 0xFF);
    }

    #[test]
    fn test_generate_pat_packet_crc_self_check() {
        // 生成したパケットの PAT セクション全体(CRC 含む)に対する CRC が 0 になることを確認
        let mut g = OneSegPatGenerator::new();
        g.set_transport_stream_id(0x1234);
        let pmt_list = [Some(0x0400u16), Some(0x0401u16), Some(0x0402u16)];
        let pkt = g.generate_pat_packet(&pmt_list).unwrap();
        let pmt_count = 3;
        let total = 8 + (pmt_count + 1) * 4 + 4; // section body + CRC
        let check = crc32_mpeg2(&pkt[5..5 + total], 0xFFFF_FFFF);
        assert_eq!(check, 0);
    }

    #[test]
    fn test_generate_pat_packet_skips_empty_middle() {
        let mut g = OneSegPatGenerator::new();
        g.set_transport_stream_id(0x1234);
        // 先頭あり、2番目なし、3番目あり
        let pmt_list = [Some(0x0400u16), None, Some(0x0402u16)];
        let pkt = g.generate_pat_packet(&pmt_list).unwrap();
        // PMTCount = 2 → SectionLength = 5 + 3*4 + 4 = 21
        let section_length = (((pkt[6] & 0x0F) as u16) << 8) | pkt[7] as u16;
        assert_eq!(section_length, 21);
        // service entries: 0x0400@PID_FIRST, 0x0402@PID_FIRST+2
        assert_eq!(((pkt[17] as u16) << 8) | pkt[18] as u16, 0x0400);
        assert_eq!(((pkt[19] as u16 & 0x1F) << 8) | pkt[20] as u16, ONESEG_PMT_PID_FIRST);
        assert_eq!(((pkt[21] as u16) << 8) | pkt[22] as u16, 0x0402);
        assert_eq!(((pkt[23] as u16 & 0x1F) << 8) | pkt[24] as u16, ONESEG_PMT_PID_FIRST + 2);
    }

    #[test]
    fn test_continuity_counter_increments() {
        let mut g = OneSegPatGenerator::new();
        g.set_transport_stream_id(0x1234);
        let pmt_list = [Some(0x0400u16)];
        let pkt0 = g.generate_pat_packet(&pmt_list).unwrap();
        let pkt1 = g.generate_pat_packet(&pmt_list).unwrap();
        assert_eq!(pkt0[3] & 0x0F, 0);
        assert_eq!(pkt1[3] & 0x0F, 1);
    }

    #[test]
    fn test_generated_packet_parses_ok() {
        let mut g = OneSegPatGenerator::new();
        g.set_transport_stream_id(0x1234);
        let pmt_list = [Some(0x0400u16)];
        let bytes = g.generate_pat_packet(&pmt_list).unwrap();
        let mut pkt = libisdb_ts_packet::TsPacket::new(&bytes);
        let result = pkt.parse_packet(None);
        // パースが成功し、PID が PAT(0) であること
        assert!(matches!(result, libisdb_ts_packet::ParseResult::Ok));
        assert_eq!(pkt.get_pid(), 0x0000);
    }

    #[test]
    fn test_nit_tsid_resets_has_pat() {
        let mut g = OneSegPatGenerator::new();
        g.notify_pat_received();
        // 新しい TSID 通知で has_pat がリセットされる
        g.notify_nit_transport_stream_id(0x5678);
        assert_eq!(g.transport_stream_id(), 0x5678);
        // has_pat がリセットされているので PMT カウントが進む
        let pid = ONESEG_PMT_PID_FIRST;
        for _ in 0..5 {
            g.notify_pmt_section(pid);
        }
        assert!(g.is_generate_pat());
    }

    #[test]
    fn test_reset_clears_state() {
        let mut g = OneSegPatGenerator::new();
        g.set_transport_stream_id(0x1234);
        let pid = ONESEG_PMT_PID_FIRST;
        for _ in 0..5 {
            g.notify_pmt_section(pid);
        }
        assert!(g.is_generate_pat());
        g.reset();
        assert!(!g.is_generate_pat());
        assert_eq!(g.transport_stream_id(), TRANSPORT_STREAM_ID_INVALID);
    }
}
