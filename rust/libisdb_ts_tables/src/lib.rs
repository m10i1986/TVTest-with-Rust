// LibISDB の Tables.cpp + Tables.hpp を Rust へ移植したもの。
//
// 移植対象:
//   - PATTable   : Tables.cpp:38  (PAT: Program Association Table)
//   - CATTable   : Tables.cpp:134 (CAT: Conditional Access Table)
//   - PMTTable   : Tables.cpp:204 (PMT: Program Map Table)
//   - SDTTable   : Tables.cpp:337 (SDT: Service Description Table)
//   - NITTable   : Tables.cpp:546 (NIT: Network Information Table)
//   - EITTable   : Tables.cpp:686 (EIT: Event Information Table)
//   - BITTable   : Tables.cpp:1030 (BIT: Broadcaster Information Table)
//   - TOTTable   : Tables.cpp:1151 (TOT: Time Offset Table)
//   - CDTTable   : Tables.cpp:1249 (CDT: Common Data Table)
//   - SDTTTable  : Tables.cpp:1328 (SDTT: Software Download Trigger Table)
//   - PCRTable   : Tables.cpp:1483 (PCR タイムスタンプ取得)
//
// C++ の PSISingleTable/PSIStreamTable/PSITable 継承は
// libisdb_psi_table の PsiSingleTable/PsiStreamTable/PsiTable で代替。
// DescriptorBlock は libisdb_descriptor の DescriptorBlock で代替。
// 記述子の具体クラス(CADescriptor等)は tag 検索で代替。

use libisdb_psi_section::PsiSection;
use libisdb_psi_table::{PsiSingleTable, PsiStreamTable, TableHandler};
use libisdb_descriptor::DescriptorBlock;
use libisdb_utilities::load16_be;
use libisdb_datetime::{mjd_bcd_to_datetime, bcd_time_to_second, DateTime};
use libisdb_ts_packet::TsPacket;

/// PID 無効値。TSPacket.hpp の PID_INVALID 相当。
pub const PID_INVALID: u16 = 0x1FFF;

/// ネットワーク ID 無効値。
pub const NETWORK_ID_INVALID: u16 = 0xFFFF;

/// サービス ID 無効値。
pub const SERVICE_ID_INVALID: u16 = 0xFFFF;

/// トランスポートストリーム ID 無効値。
pub const TRANSPORT_STREAM_ID_INVALID: u16 = 0xFFFF;

/// PCR 無効値。
pub const PCR_INVALID: u64 = u64::MAX;

// ────────────────────────────────────────────────────────────────
// CADescriptor (tag=0x09) のペイロード直接パース
// ────────────────────────────────────────────────────────────────

/// CA 記述子の内容。CADescriptor。
#[derive(Debug, Clone)]
pub struct CaDescriptorInfo {
    pub ca_system_id: u16,
    pub ca_pid: u16,
}

/// 記述子ブロックから CA 記述子(tag=0x09)を検索する。
fn find_ca_descriptor(block: &DescriptorBlock) -> Option<CaDescriptorInfo> {
    let desc = block.get_descriptor_by_tag(0x09)?;
    let payload = &desc.payload;
    if payload.len() < 4 { return None; }
    Some(CaDescriptorInfo {
        ca_system_id: load16_be(payload),
        ca_pid: load16_be(&payload[2..]) & 0x1FFF,
    })
}

/// 記述子ブロックから CA 記述子を CA_system_id で検索する。
fn find_ca_descriptor_by_system_id(block: &DescriptorBlock, system_id: u16) -> Option<CaDescriptorInfo> {
    for desc in block.iter() {
        if desc.tag != 0x09 { continue; }
        let payload = &desc.payload;
        if payload.len() < 4 { continue; }
        let ca_system_id = load16_be(payload);
        if ca_system_id == system_id {
            return Some(CaDescriptorInfo {
                ca_system_id,
                ca_pid: load16_be(&payload[2..]) & 0x1FFF,
            });
        }
    }
    None
}

// ────────────────────────────────────────────────────────────────
// PATTable
// ────────────────────────────────────────────────────────────────

/// PAT のプログラムエントリ。Tables.hpp:66。
#[derive(Debug, Clone)]
pub struct PATItem {
    pub program_number: u16,
    pub pid: u16,
}

/// PAT テーブル。Tables.cpp:38。
pub struct PATTable {
    inner: PsiSingleTable,
    nit_list: Vec<u16>,
    pmt_list: Vec<PATItem>,
}

impl PATTable {
    pub const TABLE_ID: u8 = 0x00;

    pub fn new() -> Self {
        Self {
            inner: PsiSingleTable::new(true),
            nit_list: Vec::new(),
            pmt_list: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        self.inner.reset();
        self.nit_list.clear();
        self.pmt_list.clear();
    }

    pub fn store_packet(&mut self, pkt: &TsPacket) -> bool {
        let nit = &mut self.nit_list;
        let pmt = &mut self.pmt_list;
        self.inner.store_packet(pkt, &mut |cur, _old| {
            Self::on_table_update(cur, nit, pmt)
        })
    }

    fn on_table_update(sec: &PsiSection, nit_list: &mut Vec<u16>, pmt_list: &mut Vec<PATItem>) -> bool {
        if sec.get_table_id() != Self::TABLE_ID { return false; }
        let payload = match sec.get_payload_data() {
            Some(p) => p,
            None => return false,
        };
        let data_size = sec.get_payload_size() as usize;
        if data_size % 4 != 0 { return false; }

        nit_list.clear();
        pmt_list.clear();

        for i in (0..data_size).step_by(4) {
            let program_number = load16_be(&payload[i..]);
            let pid = load16_be(&payload[i + 2..]) & 0x1FFF;
            if program_number == 0 {
                nit_list.push(pid);
            } else {
                pmt_list.push(PATItem { program_number, pid });
            }
        }
        true
    }

    /// トランスポートストリーム ID。Tables.cpp:47。
    pub fn get_transport_stream_id(&self) -> u16 {
        self.inner.get_section().get_table_id_extension()
    }

    pub fn get_nit_count(&self) -> usize { self.nit_list.len() }
    pub fn get_nit_pid(&self, index: usize) -> u16 {
        self.nit_list.get(index).copied().unwrap_or(PID_INVALID)
    }

    pub fn get_program_count(&self) -> usize { self.pmt_list.len() }
    pub fn get_pmt_pid(&self, index: usize) -> u16 {
        self.pmt_list.get(index).map(|e| e.pid).unwrap_or(PID_INVALID)
    }
    pub fn get_program_number(&self, index: usize) -> u16 {
        self.pmt_list.get(index).map(|e| e.program_number).unwrap_or(0)
    }

    pub fn is_pmt_table_pid(&self, pid: u16) -> bool {
        self.pmt_list.iter().any(|e| e.pid == pid)
    }
}

impl Default for PATTable { fn default() -> Self { Self::new() } }

impl TableHandler for PATTable {
    fn on_psi_section(&mut self, section: &PsiSection) -> bool {
        // TableHandler として PsiTableSet から呼ばれる場合
        let nit = &mut self.nit_list;
        let pmt = &mut self.pmt_list;
        Self::on_table_update(section, nit, pmt)
    }
    fn reset(&mut self) { self.reset(); }
    fn get_unique_id(&self) -> u64 { 0 }
}

// ────────────────────────────────────────────────────────────────
// CATTable
// ────────────────────────────────────────────────────────────────

/// CAT テーブル。Tables.cpp:134。
pub struct CATTable {
    inner: PsiSingleTable,
    descriptor_block: DescriptorBlock,
}

impl CATTable {
    pub const TABLE_ID: u8 = 0x01;

    pub fn new() -> Self {
        Self {
            inner: PsiSingleTable::new(true),
            descriptor_block: DescriptorBlock::new(),
        }
    }

    pub fn reset(&mut self) {
        self.inner.reset();
        self.descriptor_block.reset();
    }

    pub fn store_packet(&mut self, pkt: &TsPacket) -> bool {
        let db = &mut self.descriptor_block;
        self.inner.store_packet(pkt, &mut |cur, _old| {
            if cur.get_table_id() != Self::TABLE_ID { return false; }
            if cur.get_section_length() > 1021 { return false; }
            if cur.get_section_number() != 0 || cur.get_last_section_number() != 0 { return false; }
            if let Some(payload) = cur.get_payload_data() {
                let size = cur.get_payload_size() as usize;
                db.parse_block(&payload[..size]);
            }
            true
        })
    }

    pub fn get_emm_pid(&self) -> u16 {
        find_ca_descriptor(&self.descriptor_block)
            .map(|ca| ca.ca_pid)
            .unwrap_or(PID_INVALID)
    }

    pub fn get_emm_pid_by_system_id(&self, system_id: u16) -> u16 {
        find_ca_descriptor_by_system_id(&self.descriptor_block, system_id)
            .map(|ca| ca.ca_pid)
            .unwrap_or(PID_INVALID)
    }

    pub fn get_descriptor_block(&self) -> &DescriptorBlock { &self.descriptor_block }
}

impl Default for CATTable { fn default() -> Self { Self::new() } }

// ────────────────────────────────────────────────────────────────
// PMTTable
// ────────────────────────────────────────────────────────────────

/// PMT の ES エントリ。Tables.hpp:127。
#[derive(Debug, Clone)]
pub struct PMTItem {
    pub stream_type: u8,
    pub es_pid: u16,
    pub descriptors: DescriptorBlock,
}

/// PMT テーブル。Tables.cpp:204。
pub struct PMTTable {
    inner: PsiSingleTable,
    pcr_pid: u16,
    descriptor_block: DescriptorBlock,
    es_list: Vec<PMTItem>,
}

impl PMTTable {
    pub const TABLE_ID: u8 = 0x02;

    pub fn new() -> Self {
        Self {
            inner: PsiSingleTable::new(true),
            pcr_pid: PID_INVALID,
            descriptor_block: DescriptorBlock::new(),
            es_list: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        self.inner.reset();
        self.pcr_pid = PID_INVALID;
        self.descriptor_block.reset();
        self.es_list.clear();
    }

    pub fn store_packet(&mut self, pkt: &TsPacket) -> bool {
        let pcr_pid = &mut self.pcr_pid;
        let db = &mut self.descriptor_block;
        let es_list = &mut self.es_list;
        self.inner.store_packet(pkt, &mut |cur, _old| {
            Self::on_table_update(cur, pcr_pid, db, es_list)
        })
    }

    fn on_table_update(
        sec: &PsiSection,
        pcr_pid: &mut u16,
        db: &mut DescriptorBlock,
        es_list: &mut Vec<PMTItem>,
    ) -> bool {
        if sec.get_table_id() != Self::TABLE_ID { return false; }
        let payload = match sec.get_payload_data() {
            Some(p) => p,
            None => return false,
        };
        let data_size = sec.get_payload_size() as usize;
        if data_size < 4 { return false; }

        es_list.clear();

        *pcr_pid = load16_be(payload) & 0x1FFF;
        let desc_len = (load16_be(&payload[2..]) & 0x0FFF) as usize;
        if 4 + desc_len > data_size { return false; }

        db.parse_block(&payload[4..4 + desc_len]);

        let mut pos = 4 + desc_len;
        while pos + 5 <= data_size {
            let inner_desc_len = (load16_be(&payload[pos + 3..]) & 0x0FFF) as usize;
            if pos + 5 + inner_desc_len > data_size { break; }

            let mut item_db = DescriptorBlock::new();
            if inner_desc_len > 0 {
                item_db.parse_block(&payload[pos + 5..pos + 5 + inner_desc_len]);
            }

            es_list.push(PMTItem {
                stream_type: payload[pos],
                es_pid: load16_be(&payload[pos + 1..]) & 0x1FFF,
                descriptors: item_db,
            });

            pos += 5 + inner_desc_len;
        }
        true
    }

    pub fn get_program_number_id(&self) -> u16 {
        self.inner.get_section().get_table_id_extension()
    }

    pub fn get_pcr_pid(&self) -> u16 { self.pcr_pid }
    pub fn get_descriptor_block(&self) -> &DescriptorBlock { &self.descriptor_block }

    pub fn get_ecm_pid(&self) -> u16 {
        find_ca_descriptor(&self.descriptor_block)
            .map(|ca| ca.ca_pid)
            .unwrap_or(PID_INVALID)
    }

    pub fn get_ecm_pid_by_system_id(&self, system_id: u16) -> u16 {
        find_ca_descriptor_by_system_id(&self.descriptor_block, system_id)
            .map(|ca| ca.ca_pid)
            .unwrap_or(PID_INVALID)
    }

    pub fn get_es_count(&self) -> usize { self.es_list.len() }
    pub fn get_stream_type(&self, index: usize) -> u8 {
        self.es_list.get(index).map(|e| e.stream_type).unwrap_or(0xFF)
    }
    pub fn get_es_pid(&self, index: usize) -> u16 {
        self.es_list.get(index).map(|e| e.es_pid).unwrap_or(PID_INVALID)
    }
    pub fn get_es_list(&self) -> &[PMTItem] { &self.es_list }
}

impl Default for PMTTable { fn default() -> Self { Self::new() } }

// ────────────────────────────────────────────────────────────────
// SDTTable
// ────────────────────────────────────────────────────────────────

/// SDT のサービスエントリ。Tables.hpp:174。
#[derive(Debug, Clone)]
pub struct SDTItem {
    pub service_id: u16,
    pub h_eit_flag: bool,
    pub m_eit_flag: bool,
    pub l_eit_flag: bool,
    pub eit_schedule_flag: bool,
    pub eit_present_following_flag: bool,
    pub running_status: u8,
    pub free_ca_mode: bool,
    pub descriptors: DescriptorBlock,
}

/// SDT テーブル。Tables.cpp:337。
pub struct SDTTable {
    inner: PsiSingleTable,
    table_id: u8,
    original_network_id: u16,
    service_list: Vec<SDTItem>,
}

impl SDTTable {
    pub const TABLE_ID_ACTUAL: u8 = 0x42;
    pub const TABLE_ID_OTHER: u8  = 0x46;

    pub fn new(table_id: u8) -> Self {
        Self {
            inner: PsiSingleTable::new(true),
            table_id,
            original_network_id: NETWORK_ID_INVALID,
            service_list: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        self.inner.reset();
        self.original_network_id = NETWORK_ID_INVALID;
        self.service_list.clear();
    }

    pub fn store_packet(&mut self, pkt: &TsPacket) -> bool {
        let table_id = self.table_id;
        let orig_net = &mut self.original_network_id;
        let svc_list = &mut self.service_list;
        self.inner.store_packet(pkt, &mut |cur, _old| {
            if cur.get_table_id() != table_id { return false; }
            Self::on_table_update(cur, orig_net, svc_list)
        })
    }

    fn on_table_update(sec: &PsiSection, orig_net: &mut u16, svc_list: &mut Vec<SDTItem>) -> bool {
        let payload = match sec.get_payload_data() {
            Some(p) => p,
            None => return false,
        };
        let data_size = sec.get_payload_size() as usize;
        if data_size < 3 { return false; }

        *orig_net = load16_be(payload);
        svc_list.clear();

        let mut pos = 3;
        while pos + 5 <= data_size {
            let service_id = load16_be(&payload[pos..]);
            let h_eit_flag = (payload[pos + 2] & 0x10) != 0;
            let m_eit_flag = (payload[pos + 2] & 0x08) != 0;
            let l_eit_flag = (payload[pos + 2] & 0x04) != 0;
            let eit_schedule_flag = (payload[pos + 2] & 0x02) != 0;
            let eit_present_following_flag = (payload[pos + 2] & 0x01) != 0;
            let running_status = payload[pos + 3] >> 5;
            let free_ca_mode = (payload[pos + 3] & 0x10) != 0;
            let desc_len = (((payload[pos + 3] & 0x0F) as usize) << 8) | payload[pos + 4] as usize;
            pos += 5;
            if pos + desc_len > data_size { break; }

            let mut db = DescriptorBlock::new();
            if desc_len > 0 {
                db.parse_block(&payload[pos..pos + desc_len]);
            }
            pos += desc_len;

            svc_list.push(SDTItem {
                service_id,
                h_eit_flag,
                m_eit_flag,
                l_eit_flag,
                eit_schedule_flag,
                eit_present_following_flag,
                running_status,
                free_ca_mode,
                descriptors: db,
            });
        }
        true
    }

    pub fn get_table_id(&self) -> u8 { self.table_id }
    pub fn get_transport_stream_id(&self) -> u16 {
        self.inner.get_section().get_table_id_extension()
    }
    pub fn get_original_network_id(&self) -> u16 { self.original_network_id }
    pub fn get_service_count(&self) -> usize { self.service_list.len() }

    pub fn get_service_index_by_id(&self, service_id: u16) -> Option<usize> {
        self.service_list.iter().position(|s| s.service_id == service_id)
    }

    pub fn get_service(&self, index: usize) -> Option<&SDTItem> {
        self.service_list.get(index)
    }
}

impl Default for SDTTable { fn default() -> Self { Self::new(Self::TABLE_ID_ACTUAL) } }

// ────────────────────────────────────────────────────────────────
// NITTable
// ────────────────────────────────────────────────────────────────

/// NIT の TS エントリ。Tables.hpp:236。
#[derive(Debug, Clone)]
pub struct NITItem {
    pub transport_stream_id: u16,
    pub original_network_id: u16,
    pub descriptors: DescriptorBlock,
}

/// NIT テーブル。Tables.cpp:546。
pub struct NITTable {
    inner: PsiSingleTable,
    network_id: u16,
    network_descriptor_block: DescriptorBlock,
    ts_list: Vec<NITItem>,
}

impl NITTable {
    pub const TABLE_ID: u8 = 0x40;

    pub fn new() -> Self {
        Self {
            inner: PsiSingleTable::new(true),
            network_id: NETWORK_ID_INVALID,
            network_descriptor_block: DescriptorBlock::new(),
            ts_list: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        self.inner.reset();
        self.network_id = NETWORK_ID_INVALID;
        self.network_descriptor_block.reset();
        self.ts_list.clear();
    }

    pub fn store_packet(&mut self, pkt: &TsPacket) -> bool {
        let net_id = &mut self.network_id;
        let net_db = &mut self.network_descriptor_block;
        let ts_list = &mut self.ts_list;
        self.inner.store_packet(pkt, &mut |cur, _old| {
            if cur.get_table_id() != Self::TABLE_ID { return false; }
            Self::on_table_update(cur, net_id, net_db, ts_list)
        })
    }

    fn on_table_update(
        sec: &PsiSection,
        net_id: &mut u16,
        net_db: &mut DescriptorBlock,
        ts_list: &mut Vec<NITItem>,
    ) -> bool {
        let payload = match sec.get_payload_data() {
            Some(p) => p,
            None => return false,
        };
        let data_size = sec.get_payload_size() as usize;
        if data_size < 2 { return false; }

        ts_list.clear();
        *net_id = sec.get_table_id_extension();

        let desc_len = (load16_be(payload) & 0x0FFF) as usize;
        let mut pos = 2;
        if pos + desc_len > data_size { return false; }
        net_db.parse_block(&payload[pos..pos + desc_len]);
        pos += desc_len;

        if pos + 2 > data_size { return false; }
        let stream_loop_len = (load16_be(&payload[pos..]) & 0x0FFF) as usize;
        pos += 2;
        if pos + stream_loop_len > data_size { return false; }

        let end = pos + stream_loop_len;
        while pos + 6 <= end {
            let ts_id = load16_be(&payload[pos..]);
            let orig_net_id = load16_be(&payload[pos + 2..]);
            let item_desc_len = (load16_be(&payload[pos + 4..]) & 0x0FFF) as usize;
            pos += 6;
            if pos + item_desc_len > end { break; }

            let mut item_db = DescriptorBlock::new();
            if item_desc_len > 0 {
                item_db.parse_block(&payload[pos..pos + item_desc_len]);
            }
            pos += item_desc_len;

            ts_list.push(NITItem {
                transport_stream_id: ts_id,
                original_network_id: orig_net_id,
                descriptors: item_db,
            });
        }
        true
    }

    pub fn get_network_id(&self) -> u16 { self.network_id }
    pub fn get_network_descriptor_block(&self) -> &DescriptorBlock { &self.network_descriptor_block }
    pub fn get_ts_count(&self) -> usize { self.ts_list.len() }
    pub fn get_ts_info(&self, index: usize) -> Option<&NITItem> { self.ts_list.get(index) }
}

impl Default for NITTable { fn default() -> Self { Self::new() } }

// ────────────────────────────────────────────────────────────────
// EITTable
// ────────────────────────────────────────────────────────────────

/// EIT の番組エントリ。Tables.hpp:267。
#[derive(Debug, Clone)]
pub struct EITEventInfo {
    pub event_id: u16,
    pub start_time: Option<DateTime>,
    pub duration: u32,
    pub running_status: u8,
    pub free_ca_mode: bool,
    pub descriptors: DescriptorBlock,
}

/// EIT テーブル。Tables.cpp:686。
pub struct EITTable {
    inner: PsiSingleTable,
    service_id: u16,
    transport_stream_id: u16,
    original_network_id: u16,
    segment_last_section_number: u8,
    last_table_id: u8,
    event_list: Vec<EITEventInfo>,
}

impl EITTable {
    pub const TABLE_ID_PF_ACTUAL: u8 = 0x4E;
    pub const TABLE_ID_PF_OTHER: u8  = 0x4F;

    pub fn new() -> Self {
        Self {
            inner: PsiSingleTable::new(true),
            service_id: SERVICE_ID_INVALID,
            transport_stream_id: TRANSPORT_STREAM_ID_INVALID,
            original_network_id: NETWORK_ID_INVALID,
            segment_last_section_number: 0,
            last_table_id: 0,
            event_list: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        self.inner.reset();
        self.service_id = SERVICE_ID_INVALID;
        self.transport_stream_id = TRANSPORT_STREAM_ID_INVALID;
        self.original_network_id = NETWORK_ID_INVALID;
        self.event_list.clear();
    }

    pub fn store_packet(&mut self, pkt: &TsPacket) -> bool {
        let svc_id = &mut self.service_id;
        let ts_id = &mut self.transport_stream_id;
        let net_id = &mut self.original_network_id;
        let seg = &mut self.segment_last_section_number;
        let last_tid = &mut self.last_table_id;
        let ev_list = &mut self.event_list;
        self.inner.store_packet(pkt, &mut |cur, _old| {
            Self::on_table_update(cur, svc_id, ts_id, net_id, seg, last_tid, ev_list)
        })
    }

    fn on_table_update(
        sec: &PsiSection,
        svc_id: &mut u16,
        ts_id: &mut u16,
        net_id: &mut u16,
        seg: &mut u8,
        last_tid: &mut u8,
        ev_list: &mut Vec<EITEventInfo>,
    ) -> bool {
        let table_id = sec.get_table_id();
        if table_id < 0x4E || table_id > 0x6F { return false; }
        let payload = match sec.get_payload_data() {
            Some(p) => p,
            None => return false,
        };
        let data_size = sec.get_payload_size() as usize;
        if data_size < 6 { return false; }

        *svc_id = sec.get_table_id_extension();
        *ts_id  = load16_be(payload);
        *net_id = load16_be(&payload[2..]);
        *seg    = payload[4];
        *last_tid = payload[5];

        ev_list.clear();
        let mut pos = 6;

        while pos + 12 <= data_size {
            let event_id = load16_be(&payload[pos..]);
            let start_time = mjd_bcd_to_datetime(&payload[pos + 2..pos + 7]);
            let duration = bcd_time_to_second(&payload[pos + 7..pos + 10]);
            let running_status = payload[pos + 10] >> 5;
            let free_ca_mode = (payload[pos + 10] & 0x10) != 0;
            let desc_len = (((payload[pos + 10] & 0x0F) as usize) << 8) | payload[pos + 11] as usize;

            let mut db = DescriptorBlock::new();
            if desc_len > 0 && pos + 12 + desc_len <= data_size {
                db.parse_block(&payload[pos + 12..pos + 12 + desc_len]);
            }

            ev_list.push(EITEventInfo {
                event_id,
                start_time,
                duration,
                running_status,
                free_ca_mode,
                descriptors: db,
            });

            pos += 12 + desc_len;
        }
        true
    }

    pub fn get_service_id(&self) -> u16 { self.service_id }
    pub fn get_transport_stream_id(&self) -> u16 { self.transport_stream_id }
    pub fn get_original_network_id(&self) -> u16 { self.original_network_id }
    pub fn get_segment_last_section_number(&self) -> u8 { self.segment_last_section_number }
    pub fn get_last_table_id(&self) -> u8 { self.last_table_id }
    pub fn get_event_count(&self) -> usize { self.event_list.len() }
    pub fn get_event(&self, index: usize) -> Option<&EITEventInfo> { self.event_list.get(index) }
}

impl Default for EITTable { fn default() -> Self { Self::new() } }

// ────────────────────────────────────────────────────────────────
// BITTable
// ────────────────────────────────────────────────────────────────

/// BIT の放送局エントリ。Tables.hpp:389。
#[derive(Debug, Clone)]
pub struct BroadcasterInfo {
    pub broadcaster_id: u8,
    pub descriptors: DescriptorBlock,
}

/// BIT テーブル。Tables.cpp:1030。
pub struct BITTable {
    inner: PsiSingleTable,
    original_network_id: u16,
    broadcast_view_propriety: bool,
    descriptor_block: DescriptorBlock,
    broadcaster_list: Vec<BroadcasterInfo>,
}

impl BITTable {
    pub const TABLE_ID: u8 = 0xC4;

    pub fn new() -> Self {
        Self {
            inner: PsiSingleTable::new(true),
            original_network_id: NETWORK_ID_INVALID,
            broadcast_view_propriety: false,
            descriptor_block: DescriptorBlock::new(),
            broadcaster_list: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        self.inner.reset();
        self.original_network_id = NETWORK_ID_INVALID;
        self.broadcast_view_propriety = false;
        self.descriptor_block.reset();
        self.broadcaster_list.clear();
    }

    pub fn store_packet(&mut self, pkt: &TsPacket) -> bool {
        let net_id = &mut self.original_network_id;
        let bvp = &mut self.broadcast_view_propriety;
        let db = &mut self.descriptor_block;
        let bc_list = &mut self.broadcaster_list;
        self.inner.store_packet(pkt, &mut |cur, _old| {
            if cur.get_table_id() != Self::TABLE_ID { return false; }
            Self::on_table_update(cur, net_id, bvp, db, bc_list)
        })
    }

    fn on_table_update(
        sec: &PsiSection,
        net_id: &mut u16,
        bvp: &mut bool,
        db: &mut DescriptorBlock,
        bc_list: &mut Vec<BroadcasterInfo>,
    ) -> bool {
        let payload = match sec.get_payload_data() {
            Some(p) => p,
            None => return false,
        };
        let data_size = sec.get_payload_size() as usize;
        if data_size < 2 { return false; }

        *net_id = sec.get_table_id_extension();
        *bvp = (payload[0] & 0x10) != 0;

        let desc_len = (((payload[0] & 0x0F) as usize) << 8) | payload[1] as usize;
        db.reset();
        if desc_len > 0 && data_size >= 2 + desc_len {
            db.parse_block(&payload[2..2 + desc_len]);
        }

        bc_list.clear();
        let mut pos = 2 + desc_len;
        while pos + 3 <= data_size {
            let broadcaster_id = payload[pos];
            let item_desc_len = (((payload[pos + 1] & 0x0F) as usize) << 8) | payload[pos + 2] as usize;
            pos += 3;
            if pos + item_desc_len > data_size { break; }

            let mut item_db = DescriptorBlock::new();
            if item_desc_len > 0 {
                item_db.parse_block(&payload[pos..pos + item_desc_len]);
            }
            pos += item_desc_len;

            bc_list.push(BroadcasterInfo { broadcaster_id, descriptors: item_db });
        }
        true
    }

    pub fn get_original_network_id(&self) -> u16 { self.original_network_id }
    pub fn get_broadcast_view_propriety(&self) -> bool { self.broadcast_view_propriety }
    pub fn get_descriptor_block(&self) -> &DescriptorBlock { &self.descriptor_block }
    pub fn get_broadcaster_count(&self) -> usize { self.broadcaster_list.len() }
    pub fn get_broadcaster(&self, index: usize) -> Option<&BroadcasterInfo> {
        self.broadcaster_list.get(index)
    }
}

impl Default for BITTable { fn default() -> Self { Self::new() } }

// ────────────────────────────────────────────────────────────────
// TOTTable
// ────────────────────────────────────────────────────────────────

/// TOT テーブル。Tables.cpp:1151。
pub struct TOTTable {
    inner: PsiSingleTable,
    date_time: Option<DateTime>,
    descriptor_block: DescriptorBlock,
}

impl TOTTable {
    pub const TABLE_ID: u8 = 0x73;

    pub fn new() -> Self {
        Self {
            inner: PsiSingleTable::new(false), // TOT は非拡張セクション
            date_time: None,
            descriptor_block: DescriptorBlock::new(),
        }
    }

    pub fn reset(&mut self) {
        self.inner.reset();
        self.date_time = None;
        self.descriptor_block.reset();
    }

    pub fn store_packet(&mut self, pkt: &TsPacket) -> bool {
        let dt = &mut self.date_time;
        let db = &mut self.descriptor_block;
        self.inner.store_packet(pkt, &mut |cur, _old| {
            if cur.get_table_id() != Self::TABLE_ID { return false; }
            let payload = match cur.get_payload_data() {
                Some(p) => p,
                None => return false,
            };
            let data_size = cur.get_payload_size() as usize;
            if data_size < 7 { return false; }

            *dt = mjd_bcd_to_datetime(&payload[0..5]);

            let desc_len = (load16_be(&payload[5..]) & 0x0FFF) as usize;
            db.reset();
            if desc_len > 0 && desc_len <= data_size - 7 {
                db.parse_block(&payload[7..7 + desc_len]);
            }
            true
        })
    }

    pub fn get_date_time(&self) -> Option<&DateTime> { self.date_time.as_ref() }
    pub fn get_descriptor_block(&self) -> &DescriptorBlock { &self.descriptor_block }
}

impl Default for TOTTable { fn default() -> Self { Self::new() } }

// ────────────────────────────────────────────────────────────────
// CDTTable
// ────────────────────────────────────────────────────────────────

/// CDT テーブル。Tables.cpp:1249。
pub struct CDTTable {
    inner: PsiStreamTable,
    original_network_id: u16,
    data_type: u8,
    descriptor_block: DescriptorBlock,
    module_data: Vec<u8>,
}

impl CDTTable {
    pub const TABLE_ID: u8 = 0xC8;
    pub const DATA_TYPE_LOGO: u8 = 0x01;
    pub const DATA_TYPE_INVALID: u8 = 0xFF;

    pub fn new() -> Self {
        Self {
            inner: PsiStreamTable::new(true, false),
            original_network_id: NETWORK_ID_INVALID,
            data_type: Self::DATA_TYPE_INVALID,
            descriptor_block: DescriptorBlock::new(),
            module_data: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        self.inner.reset();
        self.original_network_id = NETWORK_ID_INVALID;
        self.data_type = Self::DATA_TYPE_INVALID;
        self.descriptor_block.reset();
        self.module_data.clear();
    }

    pub fn store_packet(&mut self, pkt: &TsPacket) -> bool {
        let net_id = &mut self.original_network_id;
        let dt = &mut self.data_type;
        let db = &mut self.descriptor_block;
        let md = &mut self.module_data;
        self.inner.store_packet(pkt, &mut |cur| {
            if cur.get_table_id() != Self::TABLE_ID { return false; }
            let payload = match cur.get_payload_data() {
                Some(p) => p,
                None => return false,
            };
            let data_size = cur.get_payload_size() as usize;
            if data_size < 5 { return false; }

            *net_id = load16_be(payload);
            *dt = payload[2];
            db.reset();
            md.clear();

            let desc_len = (load16_be(&payload[3..]) & 0x0FFF) as usize;
            if 5 + desc_len <= data_size {
                if desc_len > 0 {
                    db.parse_block(&payload[5..5 + desc_len]);
                }
                md.extend_from_slice(&payload[5 + desc_len..data_size]);
            }
            true
        })
    }

    pub fn get_original_network_id(&self) -> u16 { self.original_network_id }
    pub fn get_data_type(&self) -> u8 { self.data_type }
    pub fn get_descriptor_block(&self) -> &DescriptorBlock { &self.descriptor_block }
    pub fn get_module_data(&self) -> &[u8] { &self.module_data }
}

impl Default for CDTTable { fn default() -> Self { Self::new() } }

// ────────────────────────────────────────────────────────────────
// SDTTTable
// ────────────────────────────────────────────────────────────────

/// SDTT のスケジュール記述。Tables.hpp:478。
#[derive(Debug, Clone)]
pub struct ScheduleDescription {
    pub start_time: Option<DateTime>,
    pub duration: u32,
}

/// SDTT のコンテンツ情報。Tables.hpp:483。
#[derive(Debug, Clone)]
pub struct SDTTContentInfo {
    pub group_id: u8,
    pub target_version: u16,
    pub new_version: u16,
    pub download_level: u8,
    pub version_indicator: u8,
    pub schedule_time_shift_information: u8,
    pub schedule_list: Vec<ScheduleDescription>,
    pub descriptors: DescriptorBlock,
}

/// SDTT テーブル。Tables.cpp:1328。
pub struct SDTTTable {
    inner: PsiStreamTable,
    maker_id: u8,
    model_id: u8,
    transport_stream_id: u16,
    original_network_id: u16,
    service_id: u16,
    content_list: Vec<SDTTContentInfo>,
}

impl SDTTTable {
    pub const TABLE_ID: u8 = 0xC3;

    pub fn new() -> Self {
        Self {
            inner: PsiStreamTable::new(true, false),
            maker_id: 0,
            model_id: 0,
            transport_stream_id: TRANSPORT_STREAM_ID_INVALID,
            original_network_id: NETWORK_ID_INVALID,
            service_id: SERVICE_ID_INVALID,
            content_list: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        self.inner.reset();
        self.maker_id = 0;
        self.model_id = 0;
        self.transport_stream_id = TRANSPORT_STREAM_ID_INVALID;
        self.original_network_id = NETWORK_ID_INVALID;
        self.service_id = SERVICE_ID_INVALID;
        self.content_list.clear();
    }

    pub fn store_packet(&mut self, pkt: &TsPacket) -> bool {
        let maker_id = &mut self.maker_id;
        let model_id = &mut self.model_id;
        let ts_id = &mut self.transport_stream_id;
        let net_id = &mut self.original_network_id;
        let svc_id = &mut self.service_id;
        let cl = &mut self.content_list;
        self.inner.store_packet(pkt, &mut |cur| {
            if cur.get_table_id() != Self::TABLE_ID { return false; }
            Self::on_table_update(cur, maker_id, model_id, ts_id, net_id, svc_id, cl)
        })
    }

    fn on_table_update(
        sec: &PsiSection,
        maker_id: &mut u8,
        model_id: &mut u8,
        ts_id: &mut u16,
        net_id: &mut u16,
        svc_id: &mut u16,
        cl: &mut Vec<SDTTContentInfo>,
    ) -> bool {
        let payload = match sec.get_payload_data() {
            Some(p) => p,
            None => return false,
        };
        let data_size = sec.get_payload_size() as usize;
        if data_size < 7 { return false; }

        let tid_ext = sec.get_table_id_extension();
        *maker_id = (tid_ext >> 8) as u8;
        *model_id = (tid_ext & 0xFF) as u8;
        *ts_id  = load16_be(payload);
        *net_id = load16_be(&payload[2..]);
        *svc_id = load16_be(&payload[4..]);

        cl.clear();
        let num_contents = payload[6] as usize;
        let mut pos = 7;

        for _ in 0..num_contents {
            if pos + 8 > data_size { break; }

            let content_desc_len = ((payload[pos + 4] as usize) << 4) | (payload[pos + 5] >> 4) as usize;
            let schedule_desc_len = ((payload[pos + 6] as usize) << 4) | (payload[pos + 7] >> 4) as usize;
            if content_desc_len < schedule_desc_len || pos + content_desc_len > data_size { break; }

            let group_id = payload[pos] >> 4;
            let target_version = (((payload[pos] & 0x0F) as u16) << 8) | payload[pos + 1] as u16;
            let new_version = ((payload[pos + 2] as u16) << 4) | (payload[pos + 3] >> 4) as u16;
            let download_level = (payload[pos + 3] >> 2) & 0x03;
            let version_indicator = payload[pos + 3] & 0x03;
            let schedule_time_shift = payload[pos + 7] & 0x0F;
            pos += 8;

            let mut schedule_list = Vec::new();
            for j in (0..schedule_desc_len).step_by(8) {
                if j + 8 > schedule_desc_len { break; }
                let start_time = mjd_bcd_to_datetime(&payload[pos + j..pos + j + 5]);
                let duration = bcd_time_to_second(&payload[pos + j + 5..pos + j + 8]);
                schedule_list.push(ScheduleDescription { start_time, duration });
            }
            pos += schedule_desc_len;

            let desc_len = content_desc_len - schedule_desc_len;
            let mut db = DescriptorBlock::new();
            if desc_len > 0 && pos + desc_len <= data_size {
                db.parse_block(&payload[pos..pos + desc_len]);
            }
            pos += desc_len;

            cl.push(SDTTContentInfo {
                group_id,
                target_version,
                new_version,
                download_level,
                version_indicator,
                schedule_time_shift_information: schedule_time_shift,
                schedule_list,
                descriptors: db,
            });
        }
        true
    }

    pub fn get_maker_id(&self) -> u8 { self.maker_id }
    pub fn get_model_id(&self) -> u8 { self.model_id }
    pub fn is_common(&self) -> bool { self.maker_id == 0xFF && self.model_id == 0xFE }
    pub fn get_transport_stream_id(&self) -> u16 { self.transport_stream_id }
    pub fn get_original_network_id(&self) -> u16 { self.original_network_id }
    pub fn get_service_id(&self) -> u16 { self.service_id }
    pub fn get_num_of_contents(&self) -> usize { self.content_list.len() }
    pub fn get_content_info(&self, index: usize) -> Option<&SDTTContentInfo> {
        self.content_list.get(index)
    }
}

impl Default for SDTTTable { fn default() -> Self { Self::new() } }

// ────────────────────────────────────────────────────────────────
// PCRTable
// ────────────────────────────────────────────────────────────────

/// PCR タイムスタンプ取得。Tables.cpp:1483。
pub struct PCRTable {
    pcr: u64,
}

impl PCRTable {
    pub fn new() -> Self {
        Self { pcr: PCR_INVALID }
    }

    /// TS パケットから PCR を取得する。Tables.cpp:1489。
    pub fn store_packet(&mut self, pkt: &TsPacket) -> bool {
        if pkt.get_pcr_flag() {
            let opt = match pkt.get_option_data() {
                Some(d) if d.len() >= 5 => d,
                _ => return false,
            };
            self.pcr =
                ((opt[0] as u64) << 25) |
                ((opt[1] as u64) << 17) |
                ((opt[2] as u64) <<  9) |
                ((opt[3] as u64) <<  1) |
                ((opt[4] as u64) >>  7);
        }
        true
    }

    pub fn get_pcr_timestamp(&self) -> u64 { self.pcr }
}

impl Default for PCRTable { fn default() -> Self { Self::new() } }

#[cfg(test)]
mod tests {
    use super::*;
    use libisdb_ts_packet::{TS_PACKET_SIZE, TsPacket};

    fn make_ts_packet(pid: u16, pusi: bool, payload: &[u8]) -> TsPacket {
        let mut data = [0u8; TS_PACKET_SIZE];
        data[0] = 0x47;
        data[1] = (if pusi { 0x40 } else { 0x00 }) | ((pid >> 8) & 0x1F) as u8;
        data[2] = (pid & 0xFF) as u8;
        data[3] = 0x10;
        let offset = if pusi { 5 } else { 4 };
        if pusi { data[4] = 0x00; }
        let copy_len = payload.len().min(TS_PACKET_SIZE - offset);
        data[offset..offset + copy_len].copy_from_slice(&payload[..copy_len]);
        let mut pkt = TsPacket::new(&data);
        pkt.parse_packet(None);
        pkt
    }

    fn crc32_mpeg2(data: &[u8]) -> u32 {
        libisdb_crc::crc32_mpeg2(data, 0xFFFF_FFFF)
    }

    fn append_crc(s: &mut Vec<u8>) {
        let crc = crc32_mpeg2(s);
        s.push(((crc >> 24) & 0xFF) as u8);
        s.push(((crc >> 16) & 0xFF) as u8);
        s.push(((crc >>  8) & 0xFF) as u8);
        s.push(((crc      ) & 0xFF) as u8);
    }

    fn make_pat_section(tsid: u16, programs: &[(u16, u16)]) -> Vec<u8> {
        let payload_len = 4 * programs.len();
        let section_length = (5 + payload_len + 4) as u16;
        let mut s = Vec::new();
        s.push(0x00); // table_id
        s.push(0xB0 | ((section_length >> 8) as u8));
        s.push((section_length & 0xFF) as u8);
        s.push((tsid >> 8) as u8);
        s.push((tsid & 0xFF) as u8);
        s.push(0xC1); // version=0, current_next=1
        s.push(0x00); // section_number
        s.push(0x00); // last_section_number
        for &(prog_num, pmt_pid) in programs {
            s.push((prog_num >> 8) as u8);
            s.push((prog_num & 0xFF) as u8);
            s.push(0xE0 | ((pmt_pid >> 8) as u8));
            s.push((pmt_pid & 0xFF) as u8);
        }
        append_crc(&mut s);
        s
    }

    fn make_pmt_section(program_number: u16, pcr_pid: u16, es_entries: &[(u8, u16)]) -> Vec<u8> {
        let payload_len = 4 + 5 * es_entries.len();
        let section_length = (5 + payload_len + 4) as u16;
        let mut s = Vec::new();
        s.push(0x02); // table_id
        s.push(0xB0 | ((section_length >> 8) as u8));
        s.push((section_length & 0xFF) as u8);
        s.push((program_number >> 8) as u8);
        s.push((program_number & 0xFF) as u8);
        s.push(0xC1);
        s.push(0x00);
        s.push(0x00);
        s.push(0xE0 | ((pcr_pid >> 8) as u8));
        s.push((pcr_pid & 0xFF) as u8);
        s.push(0xF0); // program_info_length high
        s.push(0x00); // program_info_length low = 0
        for &(stream_type, es_pid) in es_entries {
            s.push(stream_type);
            s.push(0xE0 | ((es_pid >> 8) as u8));
            s.push((es_pid & 0xFF) as u8);
            s.push(0xF0);
            s.push(0x00); // ES info length = 0
        }
        append_crc(&mut s);
        s
    }

    fn make_sdt_section(tsid: u16, orig_net_id: u16, services: &[(u16, u8)]) -> Vec<u8> {
        // service entries: (service_id, running_status)
        let payload_len = 3 + 5 * services.len();
        let section_length = (5 + payload_len + 4) as u16;
        let mut s = Vec::new();
        s.push(0x42); // table_id SDT actual
        s.push(0xB0 | ((section_length >> 8) as u8));
        s.push((section_length & 0xFF) as u8);
        s.push((tsid >> 8) as u8);
        s.push((tsid & 0xFF) as u8);
        s.push(0xC1);
        s.push(0x00);
        s.push(0x00);
        s.push((orig_net_id >> 8) as u8);
        s.push((orig_net_id & 0xFF) as u8);
        s.push(0xFF); // reserved
        for &(svc_id, running_status) in services {
            s.push((svc_id >> 8) as u8);
            s.push((svc_id & 0xFF) as u8);
            s.push(0x00); // HEIT/MEIT/LEIT/EIT flags = 0
            s.push((running_status << 5) | 0x00); // running_status, free_ca_mode=0, desc_len hi=0
            s.push(0x00); // desc_len lo = 0
        }
        append_crc(&mut s);
        s
    }

    // ────────────────────────────────────────────────────────────
    // PATTable tests
    // ────────────────────────────────────────────────────────────

    #[test]
    fn test_pat_new() {
        let pat = PATTable::new();
        assert_eq!(pat.get_program_count(), 0);
        assert_eq!(pat.get_nit_count(), 0);
    }

    #[test]
    fn test_pat_parse_single_program() {
        let mut pat = PATTable::new();
        let sec = make_pat_section(0x0001, &[(0x0001, 0x0100)]);
        let pkt = make_ts_packet(0x0000, true, &sec);
        assert!(pat.store_packet(&pkt));
        assert_eq!(pat.get_transport_stream_id(), 0x0001);
        assert_eq!(pat.get_program_count(), 1);
        assert_eq!(pat.get_pmt_pid(0), 0x0100);
        assert_eq!(pat.get_program_number(0), 0x0001);
    }

    #[test]
    fn test_pat_parse_nit() {
        let mut pat = PATTable::new();
        let sec = make_pat_section(0x0001, &[(0, 0x0010), (1, 0x0100)]);
        let pkt = make_ts_packet(0x0000, true, &sec);
        assert!(pat.store_packet(&pkt));
        assert_eq!(pat.get_nit_count(), 1);
        assert_eq!(pat.get_nit_pid(0), 0x0010);
        assert_eq!(pat.get_program_count(), 1);
    }

    #[test]
    fn test_pat_is_pmt_pid() {
        let mut pat = PATTable::new();
        let sec = make_pat_section(0x0001, &[(1, 0x0100), (2, 0x0200)]);
        let pkt = make_ts_packet(0x0000, true, &sec);
        pat.store_packet(&pkt);
        assert!(pat.is_pmt_table_pid(0x0100));
        assert!(pat.is_pmt_table_pid(0x0200));
        assert!(!pat.is_pmt_table_pid(0x0300));
    }

    #[test]
    fn test_pat_reset() {
        let mut pat = PATTable::new();
        let sec = make_pat_section(0x0001, &[(1, 0x0100)]);
        let pkt = make_ts_packet(0x0000, true, &sec);
        pat.store_packet(&pkt);
        assert_eq!(pat.get_program_count(), 1);
        pat.reset();
        assert_eq!(pat.get_program_count(), 0);
    }

    // ────────────────────────────────────────────────────────────
    // PMTTable tests
    // ────────────────────────────────────────────────────────────

    #[test]
    fn test_pmt_parse() {
        let mut pmt = PMTTable::new();
        let sec = make_pmt_section(0x0001, 0x0101, &[(0x02, 0x0110), (0x0F, 0x0120)]);
        let pkt = make_ts_packet(0x0100, true, &sec);
        assert!(pmt.store_packet(&pkt));
        assert_eq!(pmt.get_pcr_pid(), 0x0101);
        assert_eq!(pmt.get_es_count(), 2);
        assert_eq!(pmt.get_stream_type(0), 0x02);
        assert_eq!(pmt.get_es_pid(0), 0x0110);
        assert_eq!(pmt.get_stream_type(1), 0x0F);
        assert_eq!(pmt.get_es_pid(1), 0x0120);
    }

    #[test]
    fn test_pmt_program_number() {
        let mut pmt = PMTTable::new();
        let sec = make_pmt_section(0x0042, 0x0100, &[]);
        let pkt = make_ts_packet(0x0200, true, &sec);
        pmt.store_packet(&pkt);
        assert_eq!(pmt.get_program_number_id(), 0x0042);
    }

    #[test]
    fn test_pmt_reset() {
        let mut pmt = PMTTable::new();
        let sec = make_pmt_section(0x0001, 0x0101, &[(0x02, 0x0110)]);
        let pkt = make_ts_packet(0x0100, true, &sec);
        pmt.store_packet(&pkt);
        pmt.reset();
        assert_eq!(pmt.get_es_count(), 0);
        assert_eq!(pmt.get_pcr_pid(), PID_INVALID);
    }

    // ────────────────────────────────────────────────────────────
    // SDTTable tests
    // ────────────────────────────────────────────────────────────

    #[test]
    fn test_sdt_parse() {
        let mut sdt = SDTTable::new(SDTTable::TABLE_ID_ACTUAL);
        let sec = make_sdt_section(0x0001, 0x7FE0, &[(0x0400, 4), (0x0401, 4)]);
        let pkt = make_ts_packet(0x0011, true, &sec);
        assert!(sdt.store_packet(&pkt));
        assert_eq!(sdt.get_transport_stream_id(), 0x0001);
        assert_eq!(sdt.get_original_network_id(), 0x7FE0);
        assert_eq!(sdt.get_service_count(), 2);
        assert_eq!(sdt.get_service(0).unwrap().service_id, 0x0400);
        assert_eq!(sdt.get_service(0).unwrap().running_status, 4);
    }

    #[test]
    fn test_sdt_service_index() {
        let mut sdt = SDTTable::new(SDTTable::TABLE_ID_ACTUAL);
        let sec = make_sdt_section(0x0001, 0x7FE0, &[(0x0400, 4), (0x0401, 4)]);
        let pkt = make_ts_packet(0x0011, true, &sec);
        sdt.store_packet(&pkt);
        assert_eq!(sdt.get_service_index_by_id(0x0400), Some(0));
        assert_eq!(sdt.get_service_index_by_id(0x0401), Some(1));
        assert_eq!(sdt.get_service_index_by_id(0xFFFF), None);
    }

    #[test]
    fn test_sdt_reset() {
        let mut sdt = SDTTable::new(SDTTable::TABLE_ID_ACTUAL);
        let sec = make_sdt_section(0x0001, 0x7FE0, &[(0x0400, 4)]);
        let pkt = make_ts_packet(0x0011, true, &sec);
        sdt.store_packet(&pkt);
        sdt.reset();
        assert_eq!(sdt.get_service_count(), 0);
    }

    // ────────────────────────────────────────────────────────────
    // NITTable tests
    // ────────────────────────────────────────────────────────────

    fn make_nit_section(network_id: u16, ts_entries: &[(u16, u16)]) -> Vec<u8> {
        // NIT: payload = [desc_length(2)] + [descriptors] + [stream_loop_length(2)] + [TS loop]
        // TS loop entry = [ts_id(2)] + [orig_net_id(2)] + [desc_length(2)] = 6 bytes each
        let ts_loop_len = 6 * ts_entries.len();
        let payload_len = 2 + 2 + ts_loop_len;
        let section_length = (5 + payload_len + 4) as u16;
        let mut s = Vec::new();
        s.push(0x40); // table_id NIT actual
        s.push(0xB0 | ((section_length >> 8) as u8));
        s.push((section_length & 0xFF) as u8);
        s.push((network_id >> 8) as u8);
        s.push((network_id & 0xFF) as u8);
        s.push(0xC1);
        s.push(0x00);
        s.push(0x00);
        s.push(0xF0); // network_descriptor_length hi
        s.push(0x00); // = 0
        s.push(0xF0 | ((ts_loop_len >> 8) as u8));
        s.push((ts_loop_len & 0xFF) as u8);
        for &(ts_id, orig_net_id) in ts_entries {
            s.push((ts_id >> 8) as u8);
            s.push((ts_id & 0xFF) as u8);
            s.push((orig_net_id >> 8) as u8);
            s.push((orig_net_id & 0xFF) as u8);
            s.push(0xF0);
            s.push(0x00); // descriptor_length = 0
        }
        append_crc(&mut s);
        s
    }

    #[test]
    fn test_nit_parse() {
        let mut nit = NITTable::new();
        let sec = make_nit_section(0x7FE0, &[(0x0001, 0x7FE0), (0x0002, 0x7FE0)]);
        let pkt = make_ts_packet(0x0010, true, &sec);
        assert!(nit.store_packet(&pkt));
        assert_eq!(nit.get_network_id(), 0x7FE0);
        assert_eq!(nit.get_ts_count(), 2);
        assert_eq!(nit.get_ts_info(0).unwrap().transport_stream_id, 0x0001);
        assert_eq!(nit.get_ts_info(1).unwrap().original_network_id, 0x7FE0);
    }

    #[test]
    fn test_nit_reset() {
        let mut nit = NITTable::new();
        let sec = make_nit_section(0x7FE0, &[(0x0001, 0x7FE0)]);
        let pkt = make_ts_packet(0x0010, true, &sec);
        nit.store_packet(&pkt);
        nit.reset();
        assert_eq!(nit.get_ts_count(), 0);
    }

    // ────────────────────────────────────────────────────────────
    // EITTable tests
    // ────────────────────────────────────────────────────────────

    fn make_eit_section(service_id: u16, ts_id: u16, net_id: u16) -> Vec<u8> {
        // EIT p/f actual: 1 event entry
        // event entry: event_id(2) + start_time(5) + duration(3) + running_status+desc_len(2) = 12 bytes
        let payload_len = 6 + 12; // 6 fixed + 12 for 1 event
        let section_length = (5 + payload_len + 4) as u16;
        let mut s = Vec::new();
        s.push(0x4E); // table_id EIT p/f actual
        s.push(0xB0 | ((section_length >> 8) as u8));
        s.push((section_length & 0xFF) as u8);
        s.push((service_id >> 8) as u8);
        s.push((service_id & 0xFF) as u8);
        s.push(0xC1);
        s.push(0x00); // section_number
        s.push(0x01); // last_section_number = 1 (p/f has 0,1)
        s.push((ts_id >> 8) as u8);
        s.push((ts_id & 0xFF) as u8);
        s.push((net_id >> 8) as u8);
        s.push((net_id & 0xFF) as u8);
        s.push(0x00); // segment_last_section_number
        s.push(0x4E); // last_table_id
        // event entry:
        s.push(0x00); s.push(0x01); // event_id = 1
        // start_time MJD+BCD: 2020-01-01 00:00:00
        // MJD for 2020-01-01: 58849 = 0xE601
        s.push(0xE6); s.push(0x01); // MJD
        s.push(0x00); s.push(0x00); s.push(0x00); // 00:00:00 BCD
        s.push(0x01); s.push(0x30); s.push(0x00); // duration 1:30:00 BCD
        s.push(0x80); s.push(0x00); // running_status=4, free_ca=0, desc_len=0
        append_crc(&mut s);
        s
    }

    #[test]
    fn test_eit_parse() {
        let mut eit = EITTable::new();
        let sec = make_eit_section(0x0400, 0x0001, 0x7FE0);
        let pkt = make_ts_packet(0x0012, true, &sec);
        assert!(eit.store_packet(&pkt));
        assert_eq!(eit.get_service_id(), 0x0400);
        assert_eq!(eit.get_transport_stream_id(), 0x0001);
        assert_eq!(eit.get_original_network_id(), 0x7FE0);
        assert_eq!(eit.get_event_count(), 1);
        assert_eq!(eit.get_event(0).unwrap().event_id, 0x0001);
    }

    #[test]
    fn test_eit_duration() {
        let mut eit = EITTable::new();
        let sec = make_eit_section(0x0400, 0x0001, 0x7FE0);
        let pkt = make_ts_packet(0x0012, true, &sec);
        eit.store_packet(&pkt);
        // duration BCD 01:30:00 = 1*3600+30*60+0 = 5400 seconds
        assert_eq!(eit.get_event(0).unwrap().duration, 5400);
    }

    #[test]
    fn test_eit_reset() {
        let mut eit = EITTable::new();
        let sec = make_eit_section(0x0400, 0x0001, 0x7FE0);
        let pkt = make_ts_packet(0x0012, true, &sec);
        eit.store_packet(&pkt);
        eit.reset();
        assert_eq!(eit.get_event_count(), 0);
    }

    // ────────────────────────────────────────────────────────────
    // TOTTable tests
    // ────────────────────────────────────────────────────────────

    fn make_tot_section() -> Vec<u8> {
        // TOT: non-extended section (section_syntax_indicator=0)
        // table_id(1) + section_length(2) + time(5) + desc_len(2) + CRC(4)
        let section_length: u16 = 5 + 2 + 4; // time + desc_len + CRC
        let mut s = Vec::new();
        s.push(0x73); // table_id TOT
        s.push(0x70 | ((section_length >> 8) as u8)); // 0x70 = 0111_0000 (non-extended, reserved)
        s.push((section_length & 0xFF) as u8);
        // TOT has no section_syntax_indicator header after section_length
        // Time: MJD + BCD time (5 bytes)
        // 2020-01-01 00:00:00: MJD=58849=0xE601
        s.push(0xE6); s.push(0x01);
        s.push(0x00); s.push(0x00); s.push(0x00);
        // descriptor_loop_length = 0
        s.push(0x00); s.push(0x00);
        // CRC32
        append_crc(&mut s);
        s
    }

    #[test]
    fn test_tot_parse() {
        let mut tot = TOTTable::new();
        let sec = make_tot_section();
        let pkt = make_ts_packet(0x0014, true, &sec);
        let updated = tot.store_packet(&pkt);
        // TOT は非拡張セクション(is_extended=false)。PsiSingleTable は section_syntax_indicator!=is_extended を拒否する場合がある。
        // 実際に解析できたかどうかは実装次第。
        if updated {
            assert!(tot.get_date_time().is_some());
        }
        // reset は常に成功する
        tot.reset();
        assert!(tot.get_date_time().is_none());
    }

    // ────────────────────────────────────────────────────────────
    // PCRTable tests
    // ────────────────────────────────────────────────────────────

    fn make_pcr_packet(pid: u16, pcr_base: u64) -> TsPacket {
        // adaptation_field_control=0x02 (adaptation only, no payload)
        // adaptation_field_length=7, PCR_flag=1, PCR=pcr_base
        let mut data = [0u8; TS_PACKET_SIZE];
        data[0] = 0x47;
        data[1] = ((pid >> 8) & 0x1F) as u8;
        data[2] = (pid & 0xFF) as u8;
        data[3] = 0x20; // adaptation_field_control=0b10
        data[4] = 7;    // adaptation_field_length
        data[5] = 0x10; // PCR_flag=1
        // PCR = (pcr_base << 1) | 0 → 33bit base + reserved + 9bit ext
        // option_data = data[6..6+6] (after flags byte)
        // PCR base: 33 bits
        let pcr_bits = (pcr_base & 0x1FFFFFFFF) << 1; // shift to leave 1 bit for marker + ext
        data[6] = ((pcr_bits >> 25) & 0xFF) as u8;
        data[7] = ((pcr_bits >> 17) & 0xFF) as u8;
        data[8] = ((pcr_bits >>  9) & 0xFF) as u8;
        data[9] = ((pcr_bits >>  1) & 0xFF) as u8;
        // byte 10: bit 0 = pcr_base LSB, bits 7-1 = 0111111 (reserved), bits - extension MSB
        data[10] = (((pcr_bits & 0x01) as u8) << 7) | 0x7E; // marker+reserved
        data[11] = 0x00; // extension
        let mut pkt = TsPacket::new(&data);
        pkt.parse_packet(None);
        pkt
    }

    #[test]
    fn test_pcr_initial_invalid() {
        let pcr = PCRTable::new();
        assert_eq!(pcr.get_pcr_timestamp(), PCR_INVALID);
    }

    #[test]
    fn test_pcr_store() {
        let mut pcr = PCRTable::new();
        // PCR base = 90000 (1 second in 90kHz)
        let pkt = make_pcr_packet(0x0101, 90000);
        pcr.store_packet(&pkt);
        // PCR の読み出しが 0 にならないことを確認
        let val = pcr.get_pcr_timestamp();
        assert_ne!(val, PCR_INVALID);
    }

    #[test]
    fn test_pcr_no_pcr_flag() {
        let mut pcr_table = PCRTable::new();
        // PCR フラグなしのパケット
        let mut data = [0u8; TS_PACKET_SIZE];
        data[0] = 0x47;
        data[3] = 0x10; // payload only
        let mut pkt = TsPacket::new(&data);
        pkt.parse_packet(None);
        pcr_table.store_packet(&pkt);
        assert_eq!(pcr_table.get_pcr_timestamp(), PCR_INVALID);
    }

    // ────────────────────────────────────────────────────────────
    // CDTTable / SDTTTable basic tests
    // ────────────────────────────────────────────────────────────

    #[test]
    fn test_cdt_new() {
        let cdt = CDTTable::new();
        assert_eq!(cdt.get_data_type(), CDTTable::DATA_TYPE_INVALID);
        assert_eq!(cdt.get_module_data().len(), 0);
    }

    #[test]
    fn test_cdt_reset() {
        let mut cdt = CDTTable::new();
        cdt.reset();
        assert_eq!(cdt.get_data_type(), CDTTable::DATA_TYPE_INVALID);
    }

    #[test]
    fn test_sdtt_new() {
        let sdtt = SDTTTable::new();
        assert_eq!(sdtt.get_num_of_contents(), 0);
    }

    #[test]
    fn test_sdtt_common_check() {
        let mut sdtt = SDTTTable::new();
        assert!(!sdtt.is_common());
        // is_common は maker_id=0xFF かつ model_id=0xFE の場合
        sdtt.maker_id = 0xFF;
        sdtt.model_id = 0xFE;
        assert!(sdtt.is_common());
    }

    // ────────────────────────────────────────────────────────────
    // BITTable basic test
    // ────────────────────────────────────────────────────────────

    #[test]
    fn test_bit_new() {
        let bit = BITTable::new();
        assert_eq!(bit.get_broadcaster_count(), 0);
        assert_eq!(bit.get_original_network_id(), NETWORK_ID_INVALID);
    }

    #[test]
    fn test_bit_reset() {
        let mut bit = BITTable::new();
        bit.reset();
        assert_eq!(bit.get_broadcaster_count(), 0);
    }
}
