// LibISDB の PSITable.cpp + PSITable.hpp を Rust へ移植したもの。
//
// 移植対象:
//   - PSITableBase  : PSITable.hpp:47 + PSITable.cpp:36
//   - PSITable      : PSITable.hpp:87 + PSITable.cpp:80 (マルチセクション管理)
//   - PSISingleTable: PSITable.hpp:139 + PSITable.cpp:308
//   - PSIStreamTable: PSITable.hpp:165 + PSITable.cpp:345
//   - PSITableSet   : PSITable.hpp:191 + PSITable.cpp:385
//
// C++ の設計:
//   - PSITableBase は PIDMapTarget + PSISectionParser::PSISectionHandler を継承
//   - PSITable は複数テーブル ID + 複数セクション番号を管理する
//   - CreateSectionTable は純粋仮想関数→ Rust では trait の fn として定義
//
// Rust での代替:
//   - PIDMapTarget trait は libisdb_pid_map で定義済み
//   - PSISectionHandler は PsiSectionParser の store_packet で FnMut として渡す
//   - 仮想コールバック on_psi_section → PsiSectionHandler trait として定義
//   - SectionHandler コールバック → Box<dyn FnMut> フィールドで保持

use libisdb_psi_section::{PsiSection, PsiSectionParser};
use libisdb_pid_map::PIDMapTarget;
use libisdb_ts_packet::TsPacket;
use std::collections::BTreeMap;

// ────────────────────────────────────────────────────────────────
// PSISectionHandler trait (C++ の PSISectionParser::PSISectionHandler 相当)
// ────────────────────────────────────────────────────────────────

/// PSI セクション受信時のコールバック trait。PSITable.hpp:46。
pub trait PsiSectionHandler {
    fn on_psi_section(&mut self, section: &PsiSection) -> bool;
}

// ────────────────────────────────────────────────────────────────
// PSITableBase
// ────────────────────────────────────────────────────────────────

/// PSI テーブル基底。PSITable.cpp:36。
///
/// PIDMapTarget として TS パケットを受け取り PsiSectionParser で組み立てる。
/// on_psi_section は具体型で実装する。
pub struct PsiTableBase {
    section_parser: PsiSectionParser,
    unique_id: u64,
    section_handler: Option<Box<dyn FnMut(&PsiSection) + 'static>>,
}

impl PsiTableBase {
    pub fn new(is_extended: bool, ignore_section_number: bool) -> Self {
        Self {
            section_parser: PsiSectionParser::new(is_extended, ignore_section_number),
            unique_id: 0,
            section_handler: None,
        }
    }

    /// リセット。PSITable.cpp:43。
    pub fn reset_parser(&mut self) {
        self.section_parser.reset();
    }

    /// CRC エラー数。PSITable.cpp:49。
    pub fn get_crc_error_count(&self) -> u64 {
        self.section_parser.get_crc_error_count()
    }

    pub fn set_unique_id(&mut self, id: u64) { self.unique_id = id; }
    pub fn get_unique_id(&self) -> u64 { self.unique_id }

    /// セクションハンドラを設定。PSITable.cpp:55。
    pub fn set_section_handler<F: FnMut(&PsiSection) + 'static>(&mut self, f: F) {
        self.section_handler = Some(Box::new(f));
    }

    /// TS パケットを処理し、セクション完成時に handler を呼ぶ。PSITable.cpp:61。
    pub fn store_packet_with<H: PsiSectionHandler>(&mut self, pkt: &TsPacket, handler: &mut H) {
        let parser = &mut self.section_parser;
        parser.store_packet(pkt, &mut |sec| {
            handler.on_psi_section(sec);
        });
    }
}

// ────────────────────────────────────────────────────────────────
// PSISingleTable
// ────────────────────────────────────────────────────────────────

/// 単独 PSI テーブル。PSITable.cpp:308。
///
/// セクションが更新された場合のみコールバックを呼ぶ。
pub struct PsiSingleTable {
    base: PsiTableBase,
    cur_section: PsiSection,
}

impl PsiSingleTable {
    pub fn new(is_extended: bool) -> Self {
        Self {
            base: PsiTableBase::new(is_extended, false),
            cur_section: PsiSection::new(),
        }
    }

    pub fn reset(&mut self) {
        self.base.reset_parser();
        self.cur_section.reset();
    }

    pub fn get_crc_error_count(&self) -> u64 { self.base.get_crc_error_count() }
    pub fn set_unique_id(&mut self, id: u64) { self.base.set_unique_id(id); }
    pub fn get_unique_id(&self) -> u64 { self.base.get_unique_id() }

    pub fn set_section_handler<F: FnMut(&PsiSection) + 'static>(&mut self, f: F) {
        self.base.set_section_handler(f);
    }

    pub fn get_section(&self) -> &PsiSection { &self.cur_section }

    /// テーブル更新コールバック。デフォルトは常に true。PSITable.cpp:337。
    pub fn on_table_update_default(_cur: &PsiSection, _old: &PsiSection) -> bool { true }

    /// TS パケット処理(table_update コールバック付き)。PSITable.cpp:321。
    pub fn store_packet<F>(&mut self, pkt: &TsPacket, on_table_update: &mut F) -> bool
    where
        F: FnMut(&PsiSection, &PsiSection) -> bool,
    {
        let mut updated = false;
        let base = &mut self.base;
        let cur = &mut self.cur_section;
        let handler = &mut base.section_handler;

        base.section_parser.store_packet(pkt, &mut |sec| {
            if sec != cur {
                let old = cur.clone();
                if on_table_update(sec, &old) {
                    *cur = sec.clone();
                    updated = true;
                    if let Some(h) = handler.as_mut() {
                        h(sec);
                    }
                }
            }
        });

        updated
    }
}

// ────────────────────────────────────────────────────────────────
// PSIStreamTable
// ────────────────────────────────────────────────────────────────

/// ストリーム PSI テーブル。PSITable.cpp:345。
///
/// セクションが到着するたびにコールバックを呼ぶ(バージョンチェックなし)。
pub struct PsiStreamTable {
    base: PsiTableBase,
}

impl PsiStreamTable {
    pub fn new(is_extended: bool, ignore_section_number: bool) -> Self {
        Self {
            base: PsiTableBase::new(is_extended, ignore_section_number),
        }
    }

    pub fn reset(&mut self) {
        self.base.reset_parser();
    }

    pub fn get_crc_error_count(&self) -> u64 { self.base.get_crc_error_count() }
    pub fn set_unique_id(&mut self, id: u64) { self.base.set_unique_id(id); }
    pub fn get_unique_id(&self) -> u64 { self.base.get_unique_id() }

    pub fn set_section_handler<F: FnMut(&PsiSection) + 'static>(&mut self, f: F) {
        self.base.set_section_handler(f);
    }

    /// TS パケット処理(table_update コールバック付き)。PSITable.cpp:357。
    pub fn store_packet<F>(&mut self, pkt: &TsPacket, on_table_update: &mut F) -> bool
    where
        F: FnMut(&PsiSection) -> bool,
    {
        let mut updated = false;
        let base = &mut self.base;
        let handler = &mut base.section_handler;

        base.section_parser.store_packet(pkt, &mut |sec| {
            if on_table_update(sec) {
                updated = true;
                if let Some(h) = handler.as_mut() {
                    h(sec);
                }
            }
        });

        updated
    }
}

// ────────────────────────────────────────────────────────────────
// PSITable (multi-section manager)
// ────────────────────────────────────────────────────────────────

/// セクション項目。PSITable.hpp:119。
pub struct SectionItem {
    /// サブテーブル(具体型はクロージャで生成)。
    pub data: Option<PsiSection>,
    pub is_updated: bool,
}

/// テーブル項目。PSITable.hpp:117。
pub struct TableItem {
    pub unique_id: u64,
    pub table_id: u8,
    pub last_section_number: u16,
    pub version_number: u8,
    pub section_list: Vec<SectionItem>,
}

/// PSI マルチセクションテーブル。PSITable.cpp:80。
///
/// テーブルID×セクション番号でセクションを管理する。
/// CreateSectionTable は `create_section_table` クロージャで代替。
pub struct PsiTable {
    base: PsiTableBase,
    table_list: Vec<TableItem>,
    last_updated_section_index: i32,
    last_updated_section_number: u16,
    section_handler: Option<Box<dyn FnMut(&PsiSection) + 'static>>,
}

impl PsiTable {
    pub fn new(is_extended: bool, ignore_section_number: bool) -> Self {
        Self {
            base: PsiTableBase::new(is_extended, ignore_section_number),
            table_list: Vec::new(),
            last_updated_section_index: -1,
            last_updated_section_number: 0xFFFF,
            section_handler: None,
        }
    }

    pub fn reset(&mut self) {
        self.base.reset_parser();
        self.table_list.clear();
        self.last_updated_section_index = -1;
        self.last_updated_section_number = 0xFFFF;
    }

    pub fn get_crc_error_count(&self) -> u64 { self.base.get_crc_error_count() }
    pub fn set_unique_id(&mut self, id: u64) { self.base.set_unique_id(id); }
    pub fn get_unique_id(&self) -> u64 { self.base.get_unique_id() }

    pub fn set_section_handler<F: FnMut(&PsiSection) + 'static>(&mut self, f: F) {
        self.section_handler = Some(Box::new(f));
    }

    pub fn get_table_count(&self) -> usize { self.table_list.len() }

    pub fn get_table_id(&self, index: usize) -> Option<u8> {
        self.table_list.get(index).map(|t| t.table_id)
    }

    pub fn get_table_unique_id(&self, index: usize) -> Option<u64> {
        self.table_list.get(index).map(|t| t.unique_id)
    }

    pub fn get_table_index_by_table_id(&self, table_id: u8) -> Option<usize> {
        self.table_list.iter().position(|t| t.table_id == table_id)
    }

    pub fn get_table_index_by_unique_id(&self, unique_id: u64) -> Option<usize> {
        self.table_list.iter().position(|t| t.unique_id == unique_id)
    }

    pub fn get_section_count(&self, index: usize) -> u16 {
        self.table_list.get(index)
            .map(|t| t.last_section_number + 1)
            .unwrap_or(0)
    }

    pub fn get_section(&self, index: usize, section_number: u16) -> Option<&PsiSection> {
        let table = self.table_list.get(index)?;
        if section_number > table.last_section_number { return None; }
        table.section_list.get(section_number as usize)
            .and_then(|s| s.data.as_ref())
    }

    pub fn get_last_updated_section(&self) -> Option<&PsiSection> {
        if self.last_updated_section_index < 0 { return None; }
        let idx = self.last_updated_section_index as usize;
        let table = self.table_list.get(idx)?;
        if self.last_updated_section_number > table.last_section_number { return None; }
        table.section_list.get(self.last_updated_section_number as usize)
            .and_then(|s| s.data.as_ref())
    }

    pub fn reset_table(&mut self, index: usize) -> bool {
        let table = match self.table_list.get_mut(index) {
            Some(t) => t,
            None => return false,
        };
        for s in &mut table.section_list {
            s.data = None;
            s.is_updated = false;
        }
        true
    }

    pub fn reset_section(&mut self, index: usize, section_number: u16) -> bool {
        let table = match self.table_list.get_mut(index) {
            Some(t) => t,
            None => return false,
        };
        if section_number > table.last_section_number { return false; }
        if let Some(s) = table.section_list.get_mut(section_number as usize) {
            s.data = None;
            s.is_updated = false;
        }
        true
    }

    pub fn is_section_complete(&self, index: usize, last_section_number: u16) -> bool {
        let table = match self.table_list.get(index) {
            Some(t) => t,
            None => return false,
        };
        let limit = (last_section_number as usize).min(table.section_list.len().saturating_sub(1));
        for i in 0..=limit {
            let s = match table.section_list.get(i) {
                Some(s) => s,
                None => return false,
            };
            if s.data.is_none() || !s.is_updated { return false; }
        }
        true
    }

    /// デフォルトの unique_id 計算(table_id_extension)。PSITable.cpp:300。
    pub fn get_section_table_unique_id(section: &PsiSection) -> u64 {
        section.get_table_id_extension() as u64
    }

    /// TS パケット処理。PSITable.cpp:242。
    ///
    /// `get_unique_id` は `GetSectionTableUniqueID` 相当。
    pub fn store_packet<G>(&mut self, pkt: &TsPacket, get_unique_id: &mut G) -> bool
    where
        G: FnMut(&PsiSection) -> u64,
    {
        let mut updated = false;

        // borrow splitting のため フィールドを個別に取り出す
        let parser = &mut self.base.section_parser;
        let table_list = &mut self.table_list;
        let last_idx = &mut self.last_updated_section_index;
        let last_sec = &mut self.last_updated_section_number;
        let handler = &mut self.section_handler;

        parser.store_packet(pkt, &mut |sec| {
            // current_next_indicator チェック
            if !sec.get_current_next_indicator() { return; }
            if sec.get_section_number() > sec.get_last_section_number() { return; }
            if sec.get_payload_size() == 0 { return; }

            let uid = get_unique_id(sec);
            let index = table_list.iter().position(|t| t.unique_id == uid);

            let index = if let Some(i) = index {
                let t = &mut table_list[i];
                if t.version_number != sec.get_version_number()
                    || t.last_section_number != sec.get_last_section_number() as u16
                {
                    // バージョン更新
                    t.last_section_number = sec.get_last_section_number() as u16;
                    t.version_number = sec.get_version_number();
                    let new_len = (t.last_section_number as usize) + 1;
                    t.section_list.clear();
                    t.section_list.resize_with(new_len, || SectionItem { data: None, is_updated: false });
                }
                i
            } else {
                let last_sec_num = sec.get_last_section_number() as u16;
                let new_len = (last_sec_num as usize) + 1;
                let mut sec_list = Vec::with_capacity(new_len);
                for _ in 0..new_len {
                    sec_list.push(SectionItem { data: None, is_updated: false });
                }
                table_list.push(TableItem {
                    unique_id: uid,
                    table_id: sec.get_table_id(),
                    last_section_number: last_sec_num,
                    version_number: sec.get_version_number(),
                    section_list: sec_list,
                });
                table_list.len() - 1
            };

            let sec_num = sec.get_section_number() as u16;
            if let Some(s) = table_list[index].section_list.get_mut(sec_num as usize) {
                s.data = Some(sec.clone());
                s.is_updated = true;
            }

            *last_idx = index as i32;
            *last_sec = sec_num;
            updated = true;

            if let Some(h) = handler.as_mut() {
                h(sec);
            }
        });

        updated
    }
}

// ────────────────────────────────────────────────────────────────
// PSITableSet
// ────────────────────────────────────────────────────────────────

/// PSI テーブル集合。PSITable.cpp:385。
///
/// 複数テーブル ID を BTreeMap で管理する。
/// 各エントリは PsiSingleTableEntry として保持。
pub struct PsiTableSetEntry {
    pub section_parser: PsiSectionParser,
    pub unique_id: u64,
    pub cur_section: PsiSection,
    pub section_handler: Option<Box<dyn FnMut(&PsiSection) + 'static>>,
}

pub struct PsiTableSet {
    base: PsiTableBase,
    table_map: BTreeMap<u8, Box<dyn TableHandler>>,
    last_updated_table_id: u8,
    last_updated_section_number: u8,
    last_updated_table_unique_id: u64,
    section_handler: Option<Box<dyn FnMut(&PsiSection) + 'static>>,
}

/// PsiTableSet に登録される各テーブルの trait。
pub trait TableHandler: 'static {
    fn on_psi_section(&mut self, section: &PsiSection) -> bool;
    fn reset(&mut self);
    fn get_unique_id(&self) -> u64;
}

impl PsiTableSet {
    pub fn new(is_extended: bool) -> Self {
        Self {
            base: PsiTableBase::new(is_extended, false),
            table_map: BTreeMap::new(),
            last_updated_table_id: 0xFF,
            last_updated_section_number: 0xFF,
            last_updated_table_unique_id: 0,
            section_handler: None,
        }
    }

    pub fn reset(&mut self) {
        self.base.reset_parser();
        for entry in self.table_map.values_mut() {
            entry.reset();
        }
        self.last_updated_table_id = 0xFF;
        self.last_updated_section_number = 0xFF;
        self.last_updated_table_unique_id = 0;
    }

    pub fn get_crc_error_count(&self) -> u64 { self.base.get_crc_error_count() }

    pub fn set_section_handler<F: FnMut(&PsiSection) + 'static>(&mut self, f: F) {
        self.section_handler = Some(Box::new(f));
    }

    /// テーブルを登録。PSITable.cpp:413。
    pub fn map_table(&mut self, table_id: u8, handler: Box<dyn TableHandler>) -> bool {
        self.unmap_table(table_id);
        self.table_map.insert(table_id, handler);
        true
    }

    /// テーブルを解除。PSITable.cpp:426。
    pub fn unmap_table(&mut self, table_id: u8) -> bool {
        self.table_map.remove(&table_id).is_some()
    }

    /// 全テーブル解除。PSITable.cpp:439。
    pub fn unmap_all_tables(&mut self) {
        self.table_map.clear();
    }

    pub fn get_table(&self, table_id: u8) -> Option<&dyn TableHandler> {
        self.table_map.get(&table_id).map(|b| b.as_ref())
    }

    pub fn get_table_mut(&mut self, table_id: u8) -> Option<&mut dyn TableHandler> {
        self.table_map.get_mut(&table_id).map(|b| b.as_mut())
    }

    pub fn get_last_updated_table_id(&self) -> u8 { self.last_updated_table_id }
    pub fn get_last_updated_section_number(&self) -> u8 { self.last_updated_section_number }
    pub fn get_last_updated_table_unique_id(&self) -> u64 { self.last_updated_table_unique_id }

    /// TS パケット処理。PSITable.cpp:473。
    pub fn store_packet(&mut self, pkt: &TsPacket) -> bool {
        let mut updated = false;

        let parser = &mut self.base.section_parser;
        let table_map = &mut self.table_map;
        let last_tid = &mut self.last_updated_table_id;
        let last_sec = &mut self.last_updated_section_number;
        let last_uid = &mut self.last_updated_table_unique_id;
        let handler = &mut self.section_handler;

        parser.store_packet(pkt, &mut |sec| {
            if let Some(entry) = table_map.get_mut(&sec.get_table_id()) {
                if entry.on_psi_section(sec) {
                    *last_tid = sec.get_table_id();
                    *last_sec = sec.get_section_number();
                    *last_uid = entry.get_unique_id();
                    updated = true;
                    if let Some(h) = handler.as_mut() {
                        h(sec);
                    }
                }
            }
        });

        updated
    }
}

/// PsiTableSet の PIDMapTarget 実装。PSITable.cpp:61。
impl PIDMapTarget for PsiTableSet {
    fn store_packet(&mut self, pkt: &TsPacket) -> bool {
        PsiTableSet::store_packet(self, pkt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libisdb_ts_packet::{TS_PACKET_SIZE, TsPacket};

    // テスト用 PSI セクションを含む TS パケット列を生成するヘルパー
    fn make_ts_packet(pid: u16, pusi: bool, payload: &[u8]) -> TsPacket {
        let mut data = [0u8; TS_PACKET_SIZE];
        data[0] = 0x47;
        data[1] = (if pusi { 0x40 } else { 0x00 }) | ((pid >> 8) & 0x1F) as u8;
        data[2] = (pid & 0xFF) as u8;
        data[3] = 0x10; // adaptation_field_control = payload only
        // PUSI の場合は pointer_field=0x00
        let offset = if pusi { 5 } else { 4 };
        let copy_len = payload.len().min(TS_PACKET_SIZE - offset);
        if pusi { data[4] = 0x00; } // pointer_field
        data[offset..offset + copy_len].copy_from_slice(&payload[..copy_len]);
        let mut pkt = TsPacket::new(&data);
        pkt.parse_packet(None);
        pkt
    }

    /// PAT セクションバイト列を生成 (table_id=0x00, 1プログラム)
    /// PAT は非拡張ヘッダ... いや extended=true (section_syntax_indicator=1)
    fn make_pat_section(tsid: u16, program_number: u16, pmt_pid: u16) -> Vec<u8> {
        // table_id=0x00, section_syntax_indicator=1, ...
        // section_length = 4(fixed) + 4(program loop) + 4(CRC) = 12... wait
        // PAT: [table_id][section_syntax_indicator+reserved+section_length_hi][section_length_lo]
        //      [TSID_hi][TSID_lo][reserved+version+current_next][section_number][last_section_number]
        //      [program_number_hi][program_number_lo][reserved+PMT_PID_hi][PMT_PID_lo]
        //      [CRC32*4]
        let mut s: Vec<u8> = Vec::new();
        s.push(0x00); // table_id
        // section_syntax_indicator=1, '0'=0, reserved=11, section_length_hi=0
        // section_length = 9(header after length) + 4(1 program) + 4(CRC) - no, wait:
        // section_length counts from byte after section_length field to end of section
        // = table_id_extension(2) + version/cni/sec_num/last_sec_num(3) + payload + CRC(4)
        // payload here = 4 bytes (1 program entry)
        // total section_length = 2+3+4+4 = 13... no:
        //   section_length = (from table_id_extension to end)
        //   = 5 (tsid+version+sec+lastsec) + 4 (program) + 4 (CRC) = 13?
        // Actually: section_length field value = length of bytes after section_length
        //   = table_id_extension(2) + version_number..last_section_number(3) + data(N) + CRC(4)
        // For 1 program: N=4, so section_length = 2+1+1+1+4+4 = 13
        let section_length: u16 = 13;
        s.push(0xB0 | ((section_length >> 8) as u8)); // 0xB0 = 1011_0000
        s.push((section_length & 0xFF) as u8);
        // table_id_extension = TSID
        s.push((tsid >> 8) as u8);
        s.push((tsid & 0xFF) as u8);
        // version=0, current_next=1 → 0xC0|0x01 = 0xC1
        s.push(0xC1);
        s.push(0x00); // section_number
        s.push(0x00); // last_section_number
        // program entry
        s.push((program_number >> 8) as u8);
        s.push((program_number & 0xFF) as u8);
        s.push(0xE0 | ((pmt_pid >> 8) as u8));
        s.push((pmt_pid & 0xFF) as u8);
        // CRC32 (use libisdb_crc)
        let crc = libisdb_crc::crc32_mpeg2(&s[0..s.len()], 0xFFFF_FFFF);
        s.push(((crc >> 24) & 0xFF) as u8);
        s.push(((crc >> 16) & 0xFF) as u8);
        s.push(((crc >>  8) & 0xFF) as u8);
        s.push(((crc      ) & 0xFF) as u8);
        s
    }

    // ────────────────────────────────────────────────────────────
    // PsiTableBase tests
    // ────────────────────────────────────────────────────────────

    #[test]
    fn test_psi_table_base_new() {
        let base = PsiTableBase::new(true, false);
        assert_eq!(base.get_crc_error_count(), 0);
        assert_eq!(base.get_unique_id(), 0);
    }

    #[test]
    fn test_psi_table_base_unique_id() {
        let mut base = PsiTableBase::new(true, false);
        base.set_unique_id(42);
        assert_eq!(base.get_unique_id(), 42);
    }

    // ────────────────────────────────────────────────────────────
    // PsiSingleTable tests
    // ────────────────────────────────────────────────────────────

    #[test]
    fn test_psi_single_table_new() {
        let table = PsiSingleTable::new(true);
        assert_eq!(table.get_crc_error_count(), 0);
    }

    #[test]
    fn test_psi_single_table_receives_section() {
        let mut table = PsiSingleTable::new(true);
        let pat = make_pat_section(0x0001, 0x0001, 0x0100);
        let pkt = make_ts_packet(0x0000, true, &pat);

        let mut called = false;
        table.store_packet(&pkt, &mut |_cur, _old| {
            called = true;
            true
        });
        assert!(called, "PAT セクションが検出されるはず");
    }

    #[test]
    fn test_psi_single_table_same_section_no_callback() {
        let mut table = PsiSingleTable::new(true);
        let pat = make_pat_section(0x0001, 0x0001, 0x0100);
        let pkt = make_ts_packet(0x0000, true, &pat);

        let mut count = 0usize;
        table.store_packet(&pkt, &mut |_cur, _old| { count += 1; true });
        // 同じセクションを再送
        let pkt2 = make_ts_packet(0x0000, true, &pat);
        table.store_packet(&pkt2, &mut |_cur, _old| { count += 1; true });
        assert_eq!(count, 1, "同じセクションの場合はコールバックしない");
    }

    #[test]
    fn test_psi_single_table_reset() {
        let mut table = PsiSingleTable::new(true);
        let pat = make_pat_section(0x0001, 0x0001, 0x0100);
        let pkt = make_ts_packet(0x0000, true, &pat);
        let mut count = 0usize;
        table.store_packet(&pkt, &mut |_cur, _old| { count += 1; true });
        assert_eq!(count, 1);

        table.reset();

        // リセット後は再検出される
        let pkt2 = make_ts_packet(0x0000, true, &pat);
        table.store_packet(&pkt2, &mut |_cur, _old| { count += 1; true });
        assert_eq!(count, 2, "リセット後は再検出されるはず");
    }

    #[test]
    fn test_psi_single_table_section_handler() {
        let mut table = PsiSingleTable::new(true);
        let received = std::sync::Arc::new(std::sync::Mutex::new(false));
        let recv_clone = received.clone();
        table.set_section_handler(move |_| {
            *recv_clone.lock().unwrap() = true;
        });

        let pat = make_pat_section(0x0001, 0x0001, 0x0100);
        let pkt = make_ts_packet(0x0000, true, &pat);
        table.store_packet(&pkt, &mut |_, _| true);
        assert!(*received.lock().unwrap());
    }

    // ────────────────────────────────────────────────────────────
    // PsiStreamTable tests
    // ────────────────────────────────────────────────────────────

    #[test]
    fn test_psi_stream_table_receives_every_section() {
        let mut table = PsiStreamTable::new(true, false);
        let pat = make_pat_section(0x0001, 0x0001, 0x0100);
        let pkt = make_ts_packet(0x0000, true, &pat);

        let mut count = 0usize;
        table.store_packet(&pkt, &mut |_sec| { count += 1; true });
        // 同じセクション再送 → ストリームテーブルは毎回通知
        let pkt2 = make_ts_packet(0x0000, true, &pat);
        table.store_packet(&pkt2, &mut |_sec| { count += 1; true });
        assert_eq!(count, 2, "ストリームテーブルは毎回コールバックするはず");
    }

    #[test]
    fn test_psi_stream_table_reset() {
        let mut table = PsiStreamTable::new(true, false);
        table.reset();
        assert_eq!(table.get_crc_error_count(), 0);
    }

    // ────────────────────────────────────────────────────────────
    // PsiTable (multi-section) tests
    // ────────────────────────────────────────────────────────────

    #[test]
    fn test_psi_table_new() {
        let table = PsiTable::new(true, false);
        assert_eq!(table.get_table_count(), 0);
    }

    #[test]
    fn test_psi_table_receives_section() {
        let mut table = PsiTable::new(true, false);
        let pat = make_pat_section(0x0001, 0x0001, 0x0100);
        let pkt = make_ts_packet(0x0000, true, &pat);

        let updated = table.store_packet(&pkt, &mut |sec| {
            sec.get_table_id_extension() as u64
        });
        assert!(updated);
        assert_eq!(table.get_table_count(), 1);
    }

    #[test]
    fn test_psi_table_reset() {
        let mut table = PsiTable::new(true, false);
        let pat = make_pat_section(0x0001, 0x0001, 0x0100);
        let pkt = make_ts_packet(0x0000, true, &pat);
        table.store_packet(&pkt, &mut |sec| sec.get_table_id_extension() as u64);
        assert_eq!(table.get_table_count(), 1);
        table.reset();
        assert_eq!(table.get_table_count(), 0);
    }

    #[test]
    fn test_psi_table_get_section() {
        let mut table = PsiTable::new(true, false);
        let pat = make_pat_section(0x0001, 0x0001, 0x0100);
        let pkt = make_ts_packet(0x0000, true, &pat);
        table.store_packet(&pkt, &mut |sec| sec.get_table_id_extension() as u64);

        assert!(table.get_section(0, 0).is_some());
        assert!(table.get_section(0, 1).is_none());
        assert!(table.get_section(1, 0).is_none());
    }

    #[test]
    fn test_psi_table_is_section_complete() {
        let mut table = PsiTable::new(true, false);
        let pat = make_pat_section(0x0001, 0x0001, 0x0100);
        let pkt = make_ts_packet(0x0000, true, &pat);
        table.store_packet(&pkt, &mut |sec| sec.get_table_id_extension() as u64);
        // section_number=0, last_section_number=0 → 1セクションで完全
        assert!(table.is_section_complete(0, 0));
    }

    #[test]
    fn test_psi_table_reset_table() {
        let mut table = PsiTable::new(true, false);
        let pat = make_pat_section(0x0001, 0x0001, 0x0100);
        let pkt = make_ts_packet(0x0000, true, &pat);
        table.store_packet(&pkt, &mut |sec| sec.get_table_id_extension() as u64);
        assert!(table.reset_table(0));
        assert!(table.get_section(0, 0).is_none());
        assert!(!table.is_section_complete(0, 0));
    }

    #[test]
    fn test_psi_table_get_table_id() {
        let mut table = PsiTable::new(true, false);
        let pat = make_pat_section(0x0001, 0x0001, 0x0100);
        let pkt = make_ts_packet(0x0000, true, &pat);
        table.store_packet(&pkt, &mut |sec| sec.get_table_id_extension() as u64);
        assert_eq!(table.get_table_id(0), Some(0x00)); // PAT table_id
    }

    // ────────────────────────────────────────────────────────────
    // PsiTableSet tests
    // ────────────────────────────────────────────────────────────

    struct SimpleHandler {
        called: usize,
    }

    impl TableHandler for SimpleHandler {
        fn on_psi_section(&mut self, _sec: &PsiSection) -> bool {
            self.called += 1;
            true
        }
        fn reset(&mut self) {}
        fn get_unique_id(&self) -> u64 { 0 }
    }

    #[test]
    fn test_psi_table_set_map_and_dispatch() {
        let mut set = PsiTableSet::new(true);
        let handler = Box::new(SimpleHandler { called: 0 });
        set.map_table(0x00, handler);

        let pat = make_pat_section(0x0001, 0x0001, 0x0100);
        let pkt = make_ts_packet(0x0000, true, &pat);
        let updated = set.store_packet(&pkt);
        assert!(updated);
        assert_eq!(set.get_last_updated_table_id(), 0x00);
    }

    #[test]
    fn test_psi_table_set_unmap() {
        let mut set = PsiTableSet::new(true);
        set.map_table(0x00, Box::new(SimpleHandler { called: 0 }));
        assert!(set.unmap_table(0x00));
        assert!(!set.unmap_table(0x00)); // 二度目は false

        let pat = make_pat_section(0x0001, 0x0001, 0x0100);
        let pkt = make_ts_packet(0x0000, true, &pat);
        // テーブルがないので更新なし
        assert!(!set.store_packet(&pkt));
    }

    #[test]
    fn test_psi_table_set_unmap_all() {
        let mut set = PsiTableSet::new(true);
        for id in [0x00_u8, 0x01, 0x02] {
            set.map_table(id, Box::new(SimpleHandler { called: 0 }));
        }
        set.unmap_all_tables();
        assert!(set.get_table(0x00).is_none());
    }

    #[test]
    fn test_psi_table_set_reset() {
        let mut set = PsiTableSet::new(true);
        set.map_table(0x00, Box::new(SimpleHandler { called: 0 }));
        set.reset();
        assert_eq!(set.get_last_updated_table_id(), 0xFF);
    }

    #[test]
    fn test_psi_table_set_pid_map_target() {
        use libisdb_pid_map::PIDMapManager;

        let mut mgr = PIDMapManager::new();
        let set = PsiTableSet::new(true);
        mgr.map_target(0x0000, Box::new(set));

        let pat = make_pat_section(0x0001, 0x0001, 0x0100);
        let pkt = make_ts_packet(0x0000, true, &pat);
        // PIDMapManager 経由でパケットが届く
        mgr.store_packet(&pkt);
        // 検証: PsiTableSet がターゲットとして登録されている
        assert!(mgr.get_map_target(0x0000).is_some());
    }
}
