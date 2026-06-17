// LogoDownloaderFilter のテスト。
// parse_logo_module の単体検証、DSM-CC セクション再構成、CDT 直接取得、
// PAT→PMT→NIT→DSM-CC→SDTT のフルパイプライン、パススルーを検証する。

use std::cell::RefCell;
use std::rc::Rc;

use libisdb_crc::crc32_mpeg2;
use libisdb_filter_base::{DataStream, FilterSink, SingleDataStream, TYPE_ID_TS_PACKET};
use libisdb_ts_packet::TS_PACKET_SIZE;

use super::*;

// ── テストヘルパ ─────────────────────────────────────────────

fn append_crc(s: &mut Vec<u8>) {
    let crc = crc32_mpeg2(s, 0xFFFF_FFFF);
    s.extend_from_slice(&crc.to_be_bytes());
}

/// PSI ロングセクション(8 バイトヘッダ + payload + CRC32)を構築する。
fn make_section(table_id: u8, tid_ext: u16, payload: &[u8]) -> Vec<u8> {
    let section_length = (5 + payload.len() + 4) as u16;
    let mut s = Vec::new();
    s.push(table_id);
    s.push(0xB0 | ((section_length >> 8) as u8));
    s.push((section_length & 0xFF) as u8);
    s.push((tid_ext >> 8) as u8);
    s.push((tid_ext & 0xFF) as u8);
    s.push(0xC1); // version=0, current_next=1
    s.push(0x00); // section_number
    s.push(0x00); // last_section_number
    s.extend_from_slice(payload);
    append_crc(&mut s);
    s
}

/// 単一セクションを 1 TS パケット(PUSI=1、末尾 0xFF スタッフィング)に詰める。
fn make_ts_packet(pid: u16, section: &[u8]) -> Vec<u8> {
    let mut data = vec![0xFFu8; TS_PACKET_SIZE];
    data[0] = 0x47;
    data[1] = 0x40 | ((pid >> 8) & 0x1F) as u8; // PUSI=1
    data[2] = (pid & 0xFF) as u8;
    data[3] = 0x10; // afc=01, cc=0
    data[4] = 0x00; // pointer_field
    let n = section.len().min(TS_PACKET_SIZE - 5);
    data[5..5 + n].copy_from_slice(&section[..n]);
    data
}

fn feed(filter: &mut LogoDownloaderFilter, packet: &[u8]) {
    let mut stream = SingleDataStream::new(TYPE_ID_TS_PACKET, packet);
    filter.receive_data(&mut stream);
}

// ── セクション payload ビルダ ────────────────────────────────

fn pat_payload(programs: &[(u16, u16)]) -> Vec<u8> {
    let mut v = Vec::new();
    for &(program_number, pmt_pid) in programs {
        v.extend_from_slice(&program_number.to_be_bytes());
        v.push(0xE0 | ((pmt_pid >> 8) as u8));
        v.push((pmt_pid & 0xFF) as u8);
    }
    v
}

/// PMT: 1 ES(stream_type, es_pid)に StreamIdDescriptor(component_tag)を付ける。
fn pmt_payload(pcr_pid: u16, stream_type: u8, es_pid: u16, component_tag: u8) -> Vec<u8> {
    let es_info = [0x52u8, 0x01, component_tag]; // StreamIdDescriptor
    let mut v = Vec::new();
    v.push(0xE0 | ((pcr_pid >> 8) as u8));
    v.push((pcr_pid & 0xFF) as u8);
    v.push(0xF0); // program_info_length = 0
    v.push(0x00);
    v.push(stream_type);
    v.push(0xE0 | ((es_pid >> 8) as u8));
    v.push((es_pid & 0xFF) as u8);
    v.push(0xF0 | ((es_info.len() >> 8) as u8));
    v.push((es_info.len() & 0xFF) as u8);
    v.extend_from_slice(&es_info);
    v
}

/// NIT: 1 TS に ServiceListDescriptor(service_id, service_type)を付ける。
fn nit_payload(ts_id: u16, onid: u16, service_id: u16, service_type: u8) -> Vec<u8> {
    let sld = [
        0x41u8, 0x03,
        (service_id >> 8) as u8, (service_id & 0xFF) as u8, service_type,
    ];
    let mut ts_entry = Vec::new();
    ts_entry.extend_from_slice(&ts_id.to_be_bytes());
    ts_entry.extend_from_slice(&onid.to_be_bytes());
    ts_entry.push(0xF0 | ((sld.len() >> 8) as u8));
    ts_entry.push((sld.len() & 0xFF) as u8);
    ts_entry.extend_from_slice(&sld);

    let mut v = Vec::new();
    v.push(0xF0); // network_descriptor_length = 0
    v.push(0x00);
    v.push(0xF0 | ((ts_entry.len() >> 8) as u8));
    v.push((ts_entry.len() & 0xFF) as u8);
    v.extend_from_slice(&ts_entry);
    v
}

/// CDT (DATA_TYPE_LOGO): module_data をそのまま載せる(記述子なし)。
fn cdt_payload(onid: u16, module_data: &[u8]) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&onid.to_be_bytes());
    v.push(CDTTable::DATA_TYPE_LOGO);
    v.push(0x00); // descriptors_loop_length = 0 (reserved 4bit + 12bit)
    v.push(0x00);
    v.extend_from_slice(module_data);
    v
}

/// DownloadContentDescriptor(tag 0xC9) の最小形(download_id のみ意味を持つ)。
fn download_content_descriptor(download_id: u32) -> Vec<u8> {
    let mut body = Vec::new();
    body.push(0x00); // flags すべて 0
    body.extend_from_slice(&0u32.to_be_bytes()); // component_size
    body.extend_from_slice(&download_id.to_be_bytes()); // download_id
    body.extend_from_slice(&0u32.to_be_bytes()); // time_out_value_DII
    body.extend_from_slice(&[0u8, 0, 0]); // leak_rate
    body.push(0x00); // component_tag
    body.push(0x00); // private_data_length = 0
    let mut v = vec![0xC9u8, body.len() as u8];
    v.extend_from_slice(&body);
    v
}

/// SDTT(is_common): 1 content に DownloadContentDescriptor を付ける。
fn sdtt_payload(ts_id: u16, onid: u16, service_id: u16, new_version: u16, download_id: u32) -> Vec<u8> {
    let desc = download_content_descriptor(download_id);
    let content_desc_len = desc.len(); // schedule_desc_len = 0
    let mut content = Vec::new();
    content.push(0x00); // group_id(4) + target_version_hi(4)
    content.push(0x00); // target_version_lo
    content.push((new_version >> 4) as u8); // new_version_hi(8)
    content.push(((new_version & 0x0F) << 4) as u8); // new_version_lo(4) + download_level(2) + version_indicator(2)
    content.push((content_desc_len >> 4) as u8); // content_description_length hi
    content.push((((content_desc_len & 0x0F) << 4) as u8) | 0x00); // content len lo + schedule len hi
    content.push(0x00); // schedule_description_length(continued) hi
    content.push(0x00); // schedule len lo(4) + reserved
    content.extend_from_slice(&desc);

    let mut v = Vec::new();
    v.extend_from_slice(&ts_id.to_be_bytes());
    v.extend_from_slice(&onid.to_be_bytes());
    v.extend_from_slice(&service_id.to_be_bytes());
    v.push(0x01); // num_of_contents
    v.extend_from_slice(&content);
    v
}

const TEST_DOWNLOAD_ID: u32 = 0xAABB_CCDD;

/// DII の DSM-CC メッセージ payload を組み立てる(ts_download の build_dii と同形式)。
fn dii_payload(block_size: u16, modules: &[(u16, u32, u8, Vec<u8>)]) -> Vec<u8> {
    let mut v = Vec::new();
    v.push(0x11); // protocol_discriminator
    v.push(0x03); // dsmcc_type
    v.extend_from_slice(&0x1006u16.to_be_bytes()); // message_id
    v.extend_from_slice(&0x1234_5678u32.to_be_bytes()); // transaction_id
    v.push(0x00); // reserved
    v.push(0x00); // adaptation_length
    v.extend_from_slice(&0u16.to_be_bytes()); // message_length
    v.extend_from_slice(&TEST_DOWNLOAD_ID.to_be_bytes()); // download_id
    v.extend_from_slice(&block_size.to_be_bytes());
    v.push(0x01); // window_size
    v.push(0x02); // ack_period
    v.extend_from_slice(&0u32.to_be_bytes()); // tc_download_window
    v.extend_from_slice(&0u32.to_be_bytes()); // tc_download_scenario
    v.extend_from_slice(&0u16.to_be_bytes()); // compat_desc_length = 0
    v.extend_from_slice(&(modules.len() as u16).to_be_bytes()); // number_of_modules
    for (id, size, version, info) in modules {
        v.extend_from_slice(&id.to_be_bytes());
        v.extend_from_slice(&size.to_be_bytes());
        v.push(*version);
        v.push(info.len() as u8);
        v.extend_from_slice(info);
    }
    while v.len() < 34 {
        v.push(0x00);
    }
    v
}

/// DDB の DSM-CC メッセージ payload を組み立てる。
fn ddb_payload(module_id: u16, module_version: u8, block_number: u16, block: &[u8]) -> Vec<u8> {
    let mut v = Vec::new();
    v.push(0x11);
    v.push(0x03);
    v.extend_from_slice(&0x1003u16.to_be_bytes());
    v.extend_from_slice(&TEST_DOWNLOAD_ID.to_be_bytes());
    v.push(0x00); // reserved
    v.push(0x00); // adaptation_length
    v.extend_from_slice(&0u16.to_be_bytes()); // message_length
    v.extend_from_slice(&module_id.to_be_bytes());
    v.push(module_version);
    v.push(0x00); // reserved
    v.extend_from_slice(&block_number.to_be_bytes());
    v.extend_from_slice(block);
    v
}

/// DSM-CC ロゴモジュールのバイト列(parse_logo_module 形式)。
/// 1 ロゴ: logo_id=5 / 1 サービス(nid,tsid,sid) / data=[DE AD BE EF]。
fn logo_module_bytes() -> Vec<u8> {
    let mut v = Vec::new();
    v.push(0x01); // logo_type
    v.extend_from_slice(&1u16.to_be_bytes()); // number_of_loop = 1
    v.push(0x00); // logo_id hi (bit0)
    v.push(0x05); // logo_id lo → 5
    v.push(0x01); // number_of_services = 1
    v.extend_from_slice(&0x7FE0u16.to_be_bytes()); // network_id
    v.extend_from_slice(&0x0001u16.to_be_bytes()); // transport_stream_id
    v.extend_from_slice(&0x0400u16.to_be_bytes()); // service_id
    v.extend_from_slice(&4u16.to_be_bytes()); // data_size = 4
    v.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]); // data
    v
}

struct RecordingLogoHandler {
    logos: Rc<RefCell<Vec<LogoData>>>,
}

impl LogoHandler for RecordingLogoHandler {
    fn on_logo_downloaded(&mut self, data: &LogoData) {
        self.logos.borrow_mut().push(data.clone());
    }
}

fn new_handler() -> (LogoHandlerHandle, Rc<RefCell<Vec<LogoData>>>) {
    let logos = Rc::new(RefCell::new(Vec::new()));
    let handler: LogoHandlerHandle = Rc::new(RefCell::new(RecordingLogoHandler { logos: logos.clone() }));
    (handler, logos)
}

// ── parse_logo_module 単体テスト ─────────────────────────────

#[test]
fn test_parse_logo_module_single() {
    let mut out = Vec::new();
    parse_logo_module(&logo_module_bytes(), &mut out);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].logo_type, 1);
    assert_eq!(out[0].logo_id, 5);
    assert_eq!(out[0].service_list.len(), 1);
    assert_eq!(out[0].service_list[0].network_id, 0x7FE0);
    assert_eq!(out[0].service_list[0].transport_stream_id, 0x0001);
    assert_eq!(out[0].service_list[0].service_id, 0x0400);
    assert_eq!(out[0].data, vec![0xDE, 0xAD, 0xBE, 0xEF]);
}

#[test]
fn test_parse_logo_module_too_short() {
    let mut out = Vec::new();
    parse_logo_module(&[0x01, 0x00], &mut out);
    assert!(out.is_empty());
}

#[test]
fn test_parse_logo_module_invalid_type() {
    // logo_type > 0x05 は無効
    let mut data = logo_module_bytes();
    data[0] = 0x06;
    let mut out = Vec::new();
    parse_logo_module(&data, &mut out);
    assert!(out.is_empty());
}

#[test]
fn test_parse_logo_module_no_services_skipped() {
    // number_of_services=0 の loop は emit されない
    let mut v = Vec::new();
    v.push(0x01); // logo_type
    v.extend_from_slice(&1u16.to_be_bytes()); // number_of_loop=1
    v.push(0x00);
    v.push(0x05); // logo_id
    v.push(0x00); // number_of_services=0
    v.extend_from_slice(&0u16.to_be_bytes()); // data_size=0
    // 末尾に余白を足して境界チェックを通す
    v.extend_from_slice(&[0u8; 4]);
    let mut out = Vec::new();
    parse_logo_module(&v, &mut out);
    assert!(out.is_empty());
}

// ── DSM-CC セクション再構成 ──────────────────────────────────

#[test]
fn test_dsmcc_section_reassembly() {
    let module = logo_module_bytes();
    let module_size = module.len() as u32;

    let mut dsmcc = DsmccSection::new();

    // DII: module_id=0x0010, name="LOGO-00", block_size=モジュール全体(=1ブロック)
    let name_desc = {
        let mut d = vec![0x02u8, 0x07];
        d.extend_from_slice(b"LOGO-00");
        d
    };
    let dii = dii_payload(module_size as u16, &[(0x0010, module_size, 1, name_desc)]);
    dsmcc.store_packet(&parse_pkt(0x0200, &make_section(0x3B, 0, &dii)));

    // DDB: block 0 にモジュール全体
    let ddb = ddb_payload(0x0010, 1, 0, &module);
    dsmcc.store_packet(&parse_pkt(0x0200, &make_section(0x3C, 0, &ddb)));

    let logos = dsmcc.enum_logo_data(TEST_DOWNLOAD_ID);
    assert_eq!(logos.len(), 1);
    assert_eq!(logos[0].logo_id, 5);
    assert_eq!(logos[0].data, vec![0xDE, 0xAD, 0xBE, 0xEF]);

    // 別の download_id では何も出ない
    assert!(dsmcc.enum_logo_data(0x1234_5678).is_empty());
}

/// バイト列を TsPacket にして返す(DsmccSection 直接テスト用)。
fn parse_pkt(pid: u16, section: &[u8]) -> TsPacket {
    let bytes = make_ts_packet(pid, section);
    let mut arr = [0u8; TS_PACKET_SIZE];
    arr.copy_from_slice(&bytes[..TS_PACKET_SIZE]);
    let mut pkt = TsPacket::new(&arr);
    pkt.parse_packet(None);
    pkt
}

// ── CDT 直接取得 ─────────────────────────────────────────────

#[test]
fn test_cdt_logo_via_filter() {
    let (handler, logos) = new_handler();
    let mut filter = LogoDownloaderFilter::new();
    filter.set_logo_handler(Some(handler));

    // CDT のロゴモジュール: logo_type/logo_id/logo_version/data_size/data
    let mut module = Vec::new();
    module.push(0x01); // logo_type
    module.extend_from_slice(&0x0005u16.to_be_bytes()); // logo_id = 5
    module.extend_from_slice(&0x0003u16.to_be_bytes()); // logo_version = 3
    module.extend_from_slice(&4u16.to_be_bytes()); // data_size = 4
    module.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);

    let sec = make_section(0xC8, 0, &cdt_payload(0x7FE0, &module));
    feed(&mut filter, &make_ts_packet(PID_CDT, &sec));

    let logos = logos.borrow();
    assert_eq!(logos.len(), 1);
    assert_eq!(logos[0].network_id, 0x7FE0);
    assert_eq!(logos[0].logo_id, 5);
    assert_eq!(logos[0].logo_version, 3);
    assert_eq!(logos[0].logo_type, 1);
    assert_eq!(logos[0].data, vec![0xDE, 0xAD, 0xBE, 0xEF]);
    assert!(logos[0].service_list.is_empty());
}

#[test]
fn test_cdt_non_logo_ignored() {
    let (handler, logos) = new_handler();
    let mut filter = LogoDownloaderFilter::new();
    filter.set_logo_handler(Some(handler));

    // data_type != LOGO の CDT(payload の data_type バイトを差し替え)
    let mut payload = cdt_payload(0x7FE0, &[0u8; 10]);
    payload[2] = 0x02; // DATA_TYPE != LOGO
    let sec = make_section(0xC8, 0, &payload);
    feed(&mut filter, &make_ts_packet(PID_CDT, &sec));

    assert!(logos.borrow().is_empty());
}

// ── フルパイプライン(DSM-CC) ────────────────────────────────

#[test]
fn test_full_pipeline_dsmcc() {
    let (handler, logos) = new_handler();
    let mut filter = LogoDownloaderFilter::new();
    filter.set_logo_handler(Some(handler));

    // PAT: service 0x0400 → PMT PID 0x0100
    feed(&mut filter, &make_ts_packet(PID_PAT, &make_section(0x00, 0x0001, &pat_payload(&[(0x0400, 0x0100)]))));
    // PMT: data_carousel ES 0x0200, component_tag 0x79
    feed(&mut filter, &make_ts_packet(0x0100, &make_section(0x02, 0x0400, &pmt_payload(0x01F0, STREAM_TYPE_DATA_CARROUSEL, 0x0200, 0x79))));
    // NIT: service 0x0400 を ENGINEERING に → データ ES 0x0200 がマップされる
    feed(&mut filter, &make_ts_packet(PID_NIT, &make_section(0x40, 0x7FE0, &nit_payload(0x0001, 0x7FE0, 0x0400, SERVICE_TYPE_ENGINEERING))));

    // DII + DDB をデータ ES 0x0200 へ
    let module = logo_module_bytes();
    let module_size = module.len() as u32;
    let name_desc = {
        let mut d = vec![0x02u8, 0x07];
        d.extend_from_slice(b"LOGO-00");
        d
    };
    let dii = dii_payload(module_size as u16, &[(0x0010, module_size, 1, name_desc)]);
    feed(&mut filter, &make_ts_packet(0x0200, &make_section(0x3B, 0, &dii)));
    let ddb = ddb_payload(0x0010, 1, 0, &module);
    feed(&mut filter, &make_ts_packet(0x0200, &make_section(0x3C, 0, &ddb)));

    // この時点ではまだ通知されない(SDTT のバージョン更新が引き金)
    assert!(logos.borrow().is_empty());

    // SDTT: download_id のバージョン更新 → ロゴ列挙・通知
    feed(&mut filter, &make_ts_packet(PID_SDTT, &make_section(0xC3, 0xFFFE, &sdtt_payload(0x0001, 0x7FE0, 0x0400, 5, TEST_DOWNLOAD_ID))));

    let logos = logos.borrow();
    assert_eq!(logos.len(), 1);
    assert_eq!(logos[0].network_id, 0x7FE0);
    assert_eq!(logos[0].logo_id, 5);
    assert_eq!(logos[0].logo_version, 5); // SDTT の new_version
    assert_eq!(logos[0].service_list.len(), 1);
    assert_eq!(logos[0].service_list[0].service_id, 0x0400);
    assert_eq!(logos[0].data, vec![0xDE, 0xAD, 0xBE, 0xEF]);
}

#[test]
fn test_sdtt_same_version_no_reemit() {
    let (handler, logos) = new_handler();
    let mut filter = LogoDownloaderFilter::new();
    filter.set_logo_handler(Some(handler));

    feed(&mut filter, &make_ts_packet(PID_PAT, &make_section(0x00, 0x0001, &pat_payload(&[(0x0400, 0x0100)]))));
    feed(&mut filter, &make_ts_packet(0x0100, &make_section(0x02, 0x0400, &pmt_payload(0x01F0, STREAM_TYPE_DATA_CARROUSEL, 0x0200, 0x79))));
    feed(&mut filter, &make_ts_packet(PID_NIT, &make_section(0x40, 0x7FE0, &nit_payload(0x0001, 0x7FE0, 0x0400, SERVICE_TYPE_ENGINEERING))));
    let module = logo_module_bytes();
    let module_size = module.len() as u32;
    let name_desc = { let mut d = vec![0x02u8, 0x07]; d.extend_from_slice(b"LOGO-00"); d };
    feed(&mut filter, &make_ts_packet(0x0200, &make_section(0x3B, 0, &dii_payload(module_size as u16, &[(0x0010, module_size, 1, name_desc)]))));
    feed(&mut filter, &make_ts_packet(0x0200, &make_section(0x3C, 0, &ddb_payload(0x0010, 1, 0, &module))));

    let sdtt = make_ts_packet(PID_SDTT, &make_section(0xC3, 0xFFFE, &sdtt_payload(0x0001, 0x7FE0, 0x0400, 5, TEST_DOWNLOAD_ID)));
    feed(&mut filter, &sdtt);
    assert_eq!(logos.borrow().len(), 1);
    // 同じバージョンの SDTT を再投入 → version_map で重複排除され再通知されない
    feed(&mut filter, &sdtt);
    assert_eq!(logos.borrow().len(), 1);
}

// ── パススルー / 無ハンドラ ──────────────────────────────────

#[test]
fn test_passthrough() {
    let mut filter = LogoDownloaderFilter::new();
    let received = Rc::new(RefCell::new(Vec::new()));
    filter.connect_output(Box::new(PassRecorder { received: received.clone() }));

    let pkt = make_ts_packet(0x0123, &[0xDE, 0xAD]);
    feed(&mut filter, &pkt);

    let got = received.borrow();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0], pkt);
}

struct PassRecorder {
    received: Rc<RefCell<Vec<Vec<u8>>>>,
}
impl FilterSink for PassRecorder {
    fn receive_data(&mut self, stream: &mut dyn DataStream) -> bool {
        loop {
            self.received.borrow_mut().push(stream.data().to_vec());
            if !stream.next() {
                break;
            }
        }
        true
    }
}

#[test]
fn test_no_handler_no_panic() {
    // ハンドラ未設定でも CDT 投入で panic しない
    let mut filter = LogoDownloaderFilter::new();
    let mut module = Vec::new();
    module.push(0x01);
    module.extend_from_slice(&0x0005u16.to_be_bytes());
    module.extend_from_slice(&0x0003u16.to_be_bytes());
    module.extend_from_slice(&4u16.to_be_bytes());
    module.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
    let sec = make_section(0xC8, 0, &cdt_payload(0x7FE0, &module));
    feed(&mut filter, &make_ts_packet(PID_CDT, &sec));
    // panic しなければ OK
}
