// Rust port of LibISDB/Filters/LogoDownloaderFilter.cpp + LogoDownloaderFilter.hpp
//
// ロゴ取得フィルタ。SingleIOFilter として、入力 TS パケットから局ロゴを取得し
// LogoHandler へ通知しつつ、入力をそのまま下流へ渡す(パススルー)。
//
// ロゴの取得経路は 2 系統:
//   1. CDT (PID 0x0029, DATA_TYPE_LOGO): データモジュールから直接ロゴを取り出す
//   2. DSM-CC データカルーセル: PAT→PMT で「全受信機共通データ」ES(component_tag
//      0x79/0x7A の data_carousel)を見つけ、NIT の service_type が
//      SERVICE_TYPE_ENGINEERING のサービスについてその ES を DSM-CC として解析。
//      DII(0x3B)/DDB(0x3C)でモジュールを再構成し、SDTT の DownloadContentDescriptor
//      でバージョン更新を検知したら完成済みモジュールからロゴを列挙する。
//
// 原実装との対応:
//   - C++ の SingleIOFilter 継承 → FilterBase + FilterSink(ProcessData 後にパススルー出力)
//   - C++ の PIDMapManager + PSITable/CreateWithHandler 自己参照コールバックを避け、
//     本フィルタが各テーブル(PAT/PMT/NIT/CDT/SDTT/TOT)と DSM-CC パーサを直接保持し
//     PID でルーティングする(他フィルタ移植と同じ平坦化方式)。
//   - C++ は NIT を NITMultiTable で扱い IsNITComplete を待つが、本移植は単一 NITTable の
//     セクション毎処理とした(service_type 検出は単調なため、各セクション到着時に走査して
//     蓄積すれば最終状態は一致する)。
//   - LogoHandler は Rc<RefCell<dyn LogoHandler>> 共有ハンドル(GrabberFilter 等と同方式)。

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use libisdb_datetime::DateTime;
use libisdb_descriptor::{DownloadContentDescriptor, ServiceListDescriptor, StreamIdDescriptor};
use libisdb_filter_base::{DataStream, FilterBase, FilterSink, OutputSlot};
use libisdb_psi_section::PsiSectionParser;
use libisdb_ts_download::{
    parse_download_data_block, parse_download_info_indication, DataBlockInfo, DataModule,
    MessageInfo, ModuleInfo,
};
use libisdb_ts_info::{PID_CDT, PID_NIT, PID_PAT, PID_SDTT, PID_TOT, STREAM_TYPE_DATA_CARROUSEL};
use libisdb_ts_packet::{TsPacket, TS_PACKET_SIZE};
use libisdb_ts_tables::{CDTTable, NITTable, PATTable, PMTTable, TOTTable};
use libisdb_utilities::load16_be;

/// エンジニアリングサービス。LibISDBConsts.hpp:129。
const SERVICE_TYPE_ENGINEERING: u8 = 0xA4;
/// 無効なサービス種別。LibISDBConsts.hpp:144。
const SERVICE_TYPE_INVALID: u8 = 0xFF;

// ---------------------------------------------------------------------------
// 公開型
// ---------------------------------------------------------------------------

/// ロゴが属するサービス。LogoDownloaderFilter.hpp:48。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct LogoService {
    pub network_id: u16,
    pub transport_stream_id: u16,
    pub service_id: u16,
}

/// 取得したロゴ。LogoDownloaderFilter.hpp:54。
///
/// 原実装は `const uint8_t *pData` + `DataSize` だが、本移植では所有 `Vec<u8>` で保持する。
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct LogoData {
    pub network_id: u16,
    pub service_list: Vec<LogoService>,
    pub logo_id: u16,
    pub logo_version: u16,
    pub logo_type: u8,
    pub data: Vec<u8>,
    pub time: DateTime,
}

/// ロゴ取得通知ハンドラ。LogoDownloaderFilter.hpp:65 (LogoHandler)。
pub trait LogoHandler {
    fn on_logo_downloaded(&mut self, data: &LogoData);
}

/// ロゴハンドラの共有ハンドル。C++ の `LogoHandler *`(外部所有)に相当。
pub type LogoHandlerHandle = Rc<RefCell<dyn LogoHandler>>;

// ---------------------------------------------------------------------------
// 非公開: DSM-CC データカルーセルのロゴ抽出
// ---------------------------------------------------------------------------

/// ロゴモジュールから取り出した 1 ロゴ分の情報。
/// LogoDownloaderFilter.cpp:55 (LogoDataModule::LogoInfo)。
struct LogoInfo {
    logo_type: u8,
    logo_id: u16,
    service_list: Vec<LogoService>,
    data: Vec<u8>,
}

/// 完成したロゴモジュールのデータを解析して LogoInfo を列挙する。
/// LogoDownloaderFilter.cpp:92 (LogoDataModule::OnComplete)。
fn parse_logo_module(data: &[u8], out: &mut Vec<LogoInfo>) {
    let module_size = data.len();
    if module_size < 3 {
        return;
    }

    let logo_type = data[0];
    if logo_type > 0x05 {
        return;
    }

    let number_of_loop = load16_be(&data[1..]);
    let mut pos = 3usize;

    for _ in 0..number_of_loop {
        if pos + 3 >= module_size {
            return;
        }

        let logo_id = (((data[pos] & 0x01) as u16) << 8) | data[pos + 1] as u16;
        let number_of_services = data[pos + 2] as usize;
        pos += 3;
        if pos + 6 * number_of_services + 2 >= module_size {
            return;
        }

        let mut service_list = Vec::with_capacity(number_of_services);
        for _ in 0..number_of_services {
            service_list.push(LogoService {
                network_id: load16_be(&data[pos..]),
                transport_stream_id: load16_be(&data[pos + 2..]),
                service_id: load16_be(&data[pos + 4..]),
            });
            pos += 6;
        }

        let data_size = load16_be(&data[pos..]) as usize;
        pos += 2;
        if pos + data_size > module_size {
            return;
        }

        if number_of_services > 0 && data_size > 0 {
            out.push(LogoInfo {
                logo_type,
                logo_id,
                service_list,
                data: data[pos..pos + data_size].to_vec(),
            });
        }

        pos += data_size;
    }
}

/// DSM-CC セクション(1 つのデータ ES PID 分)。
/// LogoDownloaderFilter.cpp:152 (DSMCCSection)。
///
/// table_id 0x3B(DII)/0x3C(DDB)を解析し、"LOGO-0…"/"CS_LOGO-0…" という名前の
/// モジュールを再構成する。C++ の PSIStreamTable + DII/DDB EventHandler を、
/// PsiSectionParser + ts_download のパース関数で置き換える。
struct DsmccSection {
    parser: PsiSectionParser,
    /// module_id → ロゴモジュール(LOGO 名のモジュールのみ保持)
    modules: HashMap<u16, DataModule>,
}

impl DsmccSection {
    fn new() -> Self {
        // PSIStreamTable(true, true): 拡張セクション + セクション番号無視
        Self {
            parser: PsiSectionParser::new(true, true),
            modules: HashMap::new(),
        }
    }

    /// TS パケットを格納し、DII/DDB を解析する。
    fn store_packet(&mut self, pkt: &TsPacket) {
        let Self { parser, modules } = self;
        parser.store_packet(pkt, &mut |sec| {
            let payload = match sec.get_payload_data() {
                Some(p) => p,
                None => return,
            };
            match sec.get_table_id() {
                0x3B => {
                    // DII
                    if let Some((msg, module_infos)) = parse_download_info_indication(payload) {
                        for mi in &module_infos {
                            Self::on_data_module(modules, &msg, mi);
                        }
                    }
                }
                0x3C => {
                    // DDB
                    if let Some(db) = parse_download_data_block(payload) {
                        Self::on_data_block(modules, &db);
                    }
                }
                _ => {}
            }
        });
    }

    /// DownloadInfoIndicationParser::EventHandler::OnDataModule (cpp:248)。
    /// 名前が "LOGO-0…"(長さ7)/"CS_LOGO-0…"(長さ10)のモジュールのみ受け付ける。
    fn on_data_module(modules: &mut HashMap<u16, DataModule>, msg: &MessageInfo, mi: &ModuleInfo) {
        let name = &mi.name.text;
        let is_logo = (name.len() == 7 && name.starts_with(b"LOGO-0"))
            || (name.len() == 10 && name.starts_with(b"CS_LOGO-0"));
        if !is_logo {
            return;
        }

        let recreate = match modules.get(&mi.module_id) {
            Some(m) => {
                m.download_id() != msg.download_id
                    || m.block_size() != msg.block_size
                    || m.module_size() != mi.module_size
                    || m.module_version() != mi.module_version
            }
            None => true,
        };

        if recreate {
            modules.insert(
                mi.module_id,
                DataModule::new(
                    msg.download_id,
                    msg.block_size,
                    mi.module_id,
                    mi.module_size,
                    mi.module_version,
                ),
            );
        }
    }

    /// DownloadDataBlockParser::EventHandler::OnDataBlock (cpp:291)。
    fn on_data_block(modules: &mut HashMap<u16, DataModule>, db: &DataBlockInfo) {
        if let Some(m) = modules.get_mut(&db.module_id) {
            if m.download_id() == db.download_id && m.module_version() == db.module_version {
                m.store_block(db.block_number, &db.data);
            }
        }
    }

    /// 指定 download_id の完成済みモジュールからロゴを列挙する。
    /// LogoDownloaderFilter.cpp:217 (DSMCCSection::EnumLogoData)。
    fn enum_logo_data(&self, download_id: u32) -> Vec<LogoInfo> {
        let mut out = Vec::new();
        for m in self.modules.values() {
            if m.download_id() == download_id && m.is_complete() {
                parse_logo_module(m.data(), &mut out);
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 公開: LogoDownloaderFilter
// ---------------------------------------------------------------------------

/// サービス情報。LogoDownloaderFilter.hpp:100。
#[derive(Clone, Debug, Default)]
struct ServiceInfo {
    service_id: u16,
    pmt_pid: u16,
    service_type: u8,
    es_list: Vec<u16>,
}

/// ロゴ取得フィルタ。LogoDownloaderFilter.hpp:44。
pub struct LogoDownloaderFilter {
    pat_table: PATTable,
    cdt_table: CDTTable,
    sdtt_table: libisdb_ts_tables::SDTTTable,
    tot_table: TOTTable,
    nit_table: NITTable,
    pmt_tables: HashMap<u16, PMTTable>,
    /// データ ES PID → DSM-CC パーサ(エンジニアリングサービスの ES のみマップ)
    dsmcc_sections: HashMap<u16, DsmccSection>,
    service_list: Vec<ServiceInfo>,
    /// download_id → version(SDTT 由来)
    version_map: HashMap<u32, u16>,
    handler: Option<LogoHandlerHandle>,
    output: OutputSlot,
}

impl LogoDownloaderFilter {
    /// LogoDownloaderFilter::LogoDownloaderFilter (cpp:329)。コンストラクタは Reset() を呼ぶ。
    pub fn new() -> Self {
        let mut s = Self {
            pat_table: PATTable::new(),
            cdt_table: CDTTable::new(),
            sdtt_table: libisdb_ts_tables::SDTTTable::new(),
            tot_table: TOTTable::new(),
            nit_table: NITTable::new(),
            pmt_tables: HashMap::new(),
            dsmcc_sections: HashMap::new(),
            service_list: Vec::new(),
            version_map: HashMap::new(),
            handler: None,
            output: OutputSlot::new(),
        };
        s.reset_impl();
        s
    }

    // ── 出力スロット接続 ───────────────────────────────────────

    pub fn connect_output(&mut self, sink: Box<dyn FilterSink>) {
        self.output.connect(sink);
    }
    pub fn disconnect_output(&mut self) {
        self.output.disconnect();
    }
    pub fn is_output_connected(&self) -> bool {
        self.output.is_connected()
    }

    // ── 設定 (LogoDownloaderFilter.cpp:361) ─────────────────────

    /// LogoDownloaderFilter::SetLogoHandler (cpp:361)
    pub fn set_logo_handler(&mut self, handler: Option<LogoHandlerHandle>) {
        self.handler = handler;
    }

    // ── 内部処理 ───────────────────────────────────────────────

    /// LogoDownloaderFilter::Reset (cpp:336)
    fn reset_impl(&mut self) {
        self.pat_table.reset();
        self.cdt_table.reset();
        self.sdtt_table.reset();
        self.tot_table.reset();
        self.nit_table.reset();
        self.pmt_tables.clear();
        self.dsmcc_sections.clear();
        self.service_list.clear();
        self.version_map.clear();
    }

    /// LogoDownloaderFilter::ProcessData (cpp:352)
    fn process_data(&mut self, stream: &mut dyn DataStream) {
        if !stream.is_ts_packet() {
            return;
        }
        loop {
            let data = stream.data();
            if data.len() >= TS_PACKET_SIZE {
                let mut arr = [0u8; TS_PACKET_SIZE];
                arr.copy_from_slice(&data[..TS_PACKET_SIZE]);
                let mut pkt = TsPacket::new(&arr);
                pkt.parse_packet(None);
                self.store_packet(&pkt);
            }
            if !stream.next() {
                break;
            }
        }
    }

    /// PIDMapManager::StorePacketStream 相当。PID で各テーブル/パーサへルーティングする。
    fn store_packet(&mut self, pkt: &TsPacket) {
        let pid = pkt.get_pid();

        if pid == PID_PAT {
            if self.pat_table.store_packet(pkt) {
                self.on_pat_updated();
            }
        } else if pid == PID_CDT {
            if self.cdt_table.store_packet(pkt) {
                self.on_cdt();
            }
        } else if pid == PID_SDTT {
            if self.sdtt_table.store_packet(pkt) {
                self.on_sdtt();
            }
        } else if pid == PID_TOT {
            self.tot_table.store_packet(pkt);
        } else if pid == PID_NIT {
            if self.nit_table.store_packet(pkt) {
                self.on_nit();
            }
        } else if self.pmt_tables.contains_key(&pid) {
            if self.pmt_tables.get_mut(&pid).unwrap().store_packet(pkt) {
                self.on_pmt(pid);
            }
        } else if let Some(dsmcc) = self.dsmcc_sections.get_mut(&pid) {
            dsmcc.store_packet(pkt);
        }
    }

    /// LogoDownloaderFilter::OnCDTSection (cpp:383)。CDT から直接ロゴを取得する。
    fn on_cdt(&mut self) {
        if self.cdt_table.get_data_type() != CDTTable::DATA_TYPE_LOGO {
            return;
        }
        let handler = match &self.handler {
            Some(h) => h.clone(),
            None => return,
        };

        let data = self.cdt_table.get_module_data();
        let data_size = data.len();
        if data_size <= 7 {
            return;
        }

        let logo_type = data[0];
        let logo_id = load16_be(&data[1..]) & 0x01FF;
        let logo_version = load16_be(&data[3..]) & 0x0FFF;
        let payload_size = load16_be(&data[5..]) as usize;

        if logo_type > 0x05 || payload_size > data_size - 7 {
            return;
        }

        let logo = LogoData {
            network_id: self.cdt_table.get_original_network_id(),
            service_list: Vec::new(),
            logo_id,
            logo_version,
            logo_type,
            data: data[7..7 + payload_size].to_vec(),
            time: self.get_tot_time(),
        };

        handler.borrow_mut().on_logo_downloaded(&logo);
    }

    /// LogoDownloaderFilter::OnSDTTSection (cpp:414)。
    /// SDTT の DownloadContentDescriptor からバージョンを取得し、更新があれば
    /// 該当 download_id の完成済みロゴモジュールを列挙して通知する。
    fn on_sdtt(&mut self) {
        if !self.sdtt_table.is_common() {
            return;
        }

        // バージョンが更新された download_id を収集
        let mut updated: Vec<u32> = Vec::new();
        let mut i = 0;
        while let Some(ci) = self.sdtt_table.get_content_info(i) {
            let new_version = ci.new_version;
            for desc in ci.descriptors.iter() {
                if let Some(dc) = DownloadContentDescriptor::from_descriptor(desc) {
                    let did = dc.download_id;
                    let changed = self.version_map.get(&did) != Some(&new_version);
                    if changed {
                        self.version_map.insert(did, new_version);
                        if !updated.contains(&did) {
                            updated.push(did);
                        }
                    }
                }
            }
            i += 1;
        }

        if updated.is_empty() {
            return;
        }

        // マップ済みデータ ES から、更新された download_id のロゴを列挙(まず収集)
        let time = self.get_tot_time();
        let mut emissions: Vec<LogoData> = Vec::new();
        for service in &self.service_list {
            for es_pid in &service.es_list {
                if let Some(dsmcc) = self.dsmcc_sections.get(es_pid) {
                    for &did in &updated {
                        for info in dsmcc.enum_logo_data(did) {
                            if info.service_list.is_empty() {
                                continue;
                            }
                            let version = self.version_map.get(&did).copied().unwrap_or(0);
                            emissions.push(LogoData {
                                network_id: info.service_list[0].network_id,
                                service_list: info.service_list,
                                logo_id: info.logo_id,
                                logo_version: version,
                                logo_type: info.logo_type,
                                data: info.data,
                                time, // DateTime は Copy
                            });
                        }
                    }
                }
            }
        }

        if let Some(handler) = &self.handler {
            let h = handler.clone();
            for e in &emissions {
                h.borrow_mut().on_logo_downloaded(e);
            }
        }
    }

    /// LogoDownloaderFilter::OnPATSection (cpp:462)
    fn on_pat_updated(&mut self) {
        // 既存サービスの PMT / データ ES マップを解除
        let old_services = std::mem::take(&mut self.service_list);
        for svc in &old_services {
            self.pmt_tables.remove(&svc.pmt_pid);
            if svc.service_type == SERVICE_TYPE_ENGINEERING {
                for pid in &svc.es_list {
                    self.dsmcc_sections.remove(pid);
                }
            }
        }

        // PAT から再構築
        let program_count = self.pat_table.get_program_count();
        self.service_list = Vec::with_capacity(program_count);
        for i in 0..program_count {
            let pmt_pid = self.pat_table.get_pmt_pid(i);
            self.service_list.push(ServiceInfo {
                service_id: self.pat_table.get_program_number(i),
                pmt_pid,
                service_type: SERVICE_TYPE_INVALID,
                es_list: Vec::new(),
            });
            self.pmt_tables.insert(pmt_pid, PMTTable::new());
        }

        // C++ は OnPATSection で NIT を再マップ(リセット)する
        self.nit_table.reset();
    }

    /// LogoDownloaderFilter::OnPMTSection (cpp:493)
    fn on_pmt(&mut self, pmt_pid: u16) {
        let service_id = match self.pmt_tables.get(&pmt_pid) {
            Some(t) => t.get_program_number_id(),
            None => return,
        };
        let index = match self.get_service_index_by_id(service_id) {
            Some(i) => i,
            None => return,
        };

        if self.service_list[index].service_type == SERVICE_TYPE_ENGINEERING {
            self.unmap_data_es(index);
        }

        // data_carousel ES のうち component_tag 0x79/0x7A(全受信機共通データ)を収集
        let new_es: Vec<u16> = {
            let pmt = &self.pmt_tables[&pmt_pid];
            let mut list = Vec::new();
            for es in pmt.get_es_list() {
                if es.stream_type != STREAM_TYPE_DATA_CARROUSEL {
                    continue;
                }
                if let Some(d) = es.descriptors.get_descriptor_by_tag(StreamIdDescriptor::TAG) {
                    if let Some(sid) = StreamIdDescriptor::from_descriptor(d) {
                        if sid.component_tag == 0x79 || sid.component_tag == 0x7A {
                            list.push(es.es_pid);
                        }
                    }
                }
            }
            list
        };
        self.service_list[index].es_list = new_es;

        if self.service_list[index].service_type == SERVICE_TYPE_ENGINEERING {
            self.map_data_es(index);
        }
    }

    /// LogoDownloaderFilter::OnNITSection (cpp:533)
    ///
    /// 単一 NITTable の現在セクションを走査し、ServiceListDescriptor から service_type を
    /// 取得して、エンジニアリングサービスのデータ ES マップを更新する。
    fn on_nit(&mut self) {
        let ts_count = self.nit_table.get_ts_count();
        for ts_index in 0..ts_count {
            // 借用衝突を避けるため (service_id, service_type) を先に収集
            let services: Vec<(u16, u8)> = {
                let ts = match self.nit_table.get_ts_info(ts_index) {
                    Some(t) => t,
                    None => continue,
                };
                match ts.descriptors.get_descriptor_by_tag(ServiceListDescriptor::TAG) {
                    Some(d) => match ServiceListDescriptor::from_descriptor(d) {
                        Some(sld) => sld
                            .service_list
                            .iter()
                            .map(|s| (s.service_id, s.service_type))
                            .collect(),
                        None => continue,
                    },
                    None => continue,
                }
            };

            for (service_id, service_type) in services {
                let index = match self.get_service_index_by_id(service_id) {
                    Some(i) => i,
                    None => continue,
                };
                if self.service_list[index].service_type == service_type {
                    continue;
                }
                if service_type == SERVICE_TYPE_ENGINEERING {
                    self.service_list[index].service_type = service_type;
                    self.map_data_es(index);
                } else if self.service_list[index].service_type == SERVICE_TYPE_ENGINEERING {
                    self.service_list[index].service_type = service_type;
                    self.unmap_data_es(index);
                } else {
                    self.service_list[index].service_type = service_type;
                }
            }
        }
    }

    /// LogoDownloaderFilter::GetServiceIndexByID (cpp:577)
    fn get_service_index_by_id(&self, service_id: u16) -> Option<usize> {
        self.service_list.iter().position(|s| s.service_id == service_id)
    }

    /// LogoDownloaderFilter::MapDataES (cpp:587)。データ ES に DSM-CC パーサをマップ。
    fn map_data_es(&mut self, index: usize) {
        let pids = self.service_list[index].es_list.clone();
        for pid in pids {
            self.dsmcc_sections.entry(pid).or_insert_with(DsmccSection::new);
        }
    }

    /// LogoDownloaderFilter::UnmapDataES (cpp:609)
    fn unmap_data_es(&mut self, index: usize) {
        let pids = self.service_list[index].es_list.clone();
        for pid in pids {
            self.dsmcc_sections.remove(&pid);
        }
    }

    /// LogoDownloaderFilter::GetTOTTime (cpp:623)
    fn get_tot_time(&self) -> DateTime {
        // DateTime は Copy。TOT 未受信時は既定値(C++ の Reset 相当)。
        self.tot_table.get_date_time().copied().unwrap_or_default()
    }
}

impl Default for LogoDownloaderFilter {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// FilterBase 実装
// ---------------------------------------------------------------------------

impl FilterBase for LogoDownloaderFilter {
    fn input_count(&self) -> usize {
        1
    }
    fn output_count(&self) -> usize {
        1
    }

    /// LogoDownloaderFilter::Reset (cpp:336)
    fn reset(&mut self) {
        self.reset_impl();
    }
}

// ---------------------------------------------------------------------------
// FilterSink 実装(SingleIOFilter: ProcessData → OutputData パススルー)
// ---------------------------------------------------------------------------

impl FilterSink for LogoDownloaderFilter {
    fn receive_data(&mut self, stream: &mut dyn DataStream) -> bool {
        self.process_data(stream);
        self.output.send(stream);
        true
    }
}

#[cfg(test)]
mod tests;
