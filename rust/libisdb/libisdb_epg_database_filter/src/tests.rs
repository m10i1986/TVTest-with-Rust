// EPGDatabaseFilter のテスト。
// PID ルーティング(H-EIT/L-EIT/TOT)、EPGDatabase 更新、パススルー出力、
// 1 パケット複数セクション、スケジュールリセット連動を検証する。

use std::cell::RefCell;
use std::rc::Rc;

use libisdb_crc::crc32_mpeg2;
use libisdb_datetime::{datetime_to_mjd, DateTime};
use libisdb_epg_database::EpgDatabase;
use libisdb_filter_base::{DataStream, FilterSink, SingleDataStream, TYPE_ID_TS_PACKET};
use libisdb_ts_packet::TS_PACKET_SIZE;

use super::*;

// ── テストヘルパ ─────────────────────────────────────────────

fn append_crc(s: &mut Vec<u8>) {
    let crc = crc32_mpeg2(s, 0xFFFF_FFFF);
    s.push(((crc >> 24) & 0xFF) as u8);
    s.push(((crc >> 16) & 0xFF) as u8);
    s.push(((crc >> 8) & 0xFF) as u8);
    s.push((crc & 0xFF) as u8);
}

fn bcd(v: u32) -> u8 {
    (((v / 10) % 10) as u8) << 4 | ((v % 10) as u8)
}

fn dt(year: i32, month: i32, day: i32, hour: i32, min: i32, sec: i32) -> DateTime {
    DateTime { year, month, day, day_of_week: 0, hour, minute: min, second: sec, millisecond: 0 }
}

/// 単一セクションを 1 TS パケットに詰める(PUSI=1、末尾 0xFF スタッフィング)。
fn make_ts_packet(pid: u16, section: &[u8]) -> Vec<u8> {
    make_ts_packet_multi(pid, &[section.to_vec()])
}

/// 複数セクションを 1 TS パケットに詰める。
fn make_ts_packet_multi(pid: u16, sections: &[Vec<u8>]) -> Vec<u8> {
    let mut data = vec![0xFFu8; TS_PACKET_SIZE];
    data[0] = 0x47;
    data[1] = 0x40 | ((pid >> 8) & 0x1F) as u8; // PUSI=1
    data[2] = (pid & 0xFF) as u8;
    data[3] = 0x10; // afc=01 (payload only), cc=0
    data[4] = 0x00; // pointer_field
    let mut pos = 5;
    for sec in sections {
        let n = sec.len().min(TS_PACKET_SIZE - pos);
        data[pos..pos + n].copy_from_slice(&sec[..n]);
        pos += n;
    }
    data
}

/// EIT セクション。events: (event_id, start_time, duration_secs)。
#[allow(clippy::too_many_arguments)]
fn make_eit_section(
    table_id: u8, version: u8, section_number: u8, last_section_number: u8,
    seg_last_section: u8, last_table_id: u8,
    service_id: u16, ts_id: u16, net_id: u16,
    events: &[(u16, DateTime, u32)],
) -> Vec<u8> {
    let mut ev = Vec::new();
    for (eid, st, dur) in events {
        let mjd = datetime_to_mjd(st);
        ev.push((eid >> 8) as u8);
        ev.push((eid & 0xFF) as u8);
        ev.push((mjd >> 8) as u8);
        ev.push((mjd & 0xFF) as u8);
        ev.push(bcd(st.hour as u32));
        ev.push(bcd(st.minute as u32));
        ev.push(bcd(st.second as u32));
        ev.push(bcd(dur / 3600));
        ev.push(bcd((dur % 3600) / 60));
        ev.push(bcd(dur % 60));
        ev.push(0x80); // running_status=4, free_ca=0, desc_len_high=0
        ev.push(0x00); // desc_len_low=0
    }

    let payload_len = 6 + ev.len();
    let section_length = (5 + payload_len + 4) as u16;
    let mut s = Vec::new();
    s.push(table_id);
    s.push(0xB0 | ((section_length >> 8) as u8));
    s.push((section_length & 0xFF) as u8);
    s.push((service_id >> 8) as u8);
    s.push((service_id & 0xFF) as u8);
    s.push(0xC0 | ((version & 0x1F) << 1) | 0x01); // version, current_next=1
    s.push(section_number);
    s.push(last_section_number);
    s.push((ts_id >> 8) as u8);
    s.push((ts_id & 0xFF) as u8);
    s.push((net_id >> 8) as u8);
    s.push((net_id & 0xFF) as u8);
    s.push(seg_last_section);
    s.push(last_table_id);
    s.extend_from_slice(&ev);
    append_crc(&mut s);
    s
}

/// TOT セクション(非拡張、指定日時)。
fn make_tot_section(time: &DateTime) -> Vec<u8> {
    let mjd = datetime_to_mjd(time);
    let section_length: u16 = 5 + 2 + 4; // time(5) + desc_len(2) + CRC(4)
    let mut s = Vec::new();
    s.push(0x73); // table_id TOT
    s.push(0x70 | ((section_length >> 8) as u8)); // 非拡張(section_syntax_indicator=0)
    s.push((section_length & 0xFF) as u8);
    s.push((mjd >> 8) as u8);
    s.push((mjd & 0xFF) as u8);
    s.push(bcd(time.hour as u32));
    s.push(bcd(time.minute as u32));
    s.push(bcd(time.second as u32));
    s.push(0x00); // descriptor_loop_length(上位4bitは予約)
    s.push(0x00);
    append_crc(&mut s);
    s
}

/// フィルタに 1 パケットを投入する。
fn feed(filter: &mut EpgDatabaseFilter, packet: &[u8]) {
    let mut stream = SingleDataStream::new(TYPE_ID_TS_PACKET, packet);
    filter.receive_data(&mut stream);
}

/// 受信データを記録する下流シンク。
struct RecordingSink {
    received: Rc<RefCell<Vec<Vec<u8>>>>,
}

impl FilterSink for RecordingSink {
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

fn new_db() -> EpgDatabaseHandle {
    let mut db = EpgDatabase::new();
    db.no_past_events = false;
    Rc::new(RefCell::new(db))
}

// ── テスト ───────────────────────────────────────────────────

#[test]
fn test_passthrough() {
    let mut filter = EpgDatabaseFilter::new();
    let received = Rc::new(RefCell::new(Vec::new()));
    filter.connect_output(Box::new(RecordingSink { received: received.clone() }));

    // 任意の PID(EPG 対象外)でも下流へそのまま渡る
    let pkt = make_ts_packet(0x0100, &[0xDE, 0xAD, 0xBE, 0xEF]);
    feed(&mut filter, &pkt);

    let got = received.borrow();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0], pkt);
}

#[test]
fn test_heit_schedule_creates_service() {
    let db = new_db();
    let mut filter = EpgDatabaseFilter::new();
    filter.set_epg_database(Some(db.clone()));

    let sec = make_eit_section(
        0x50, 0, 0, 7, 7, 0x50, 0x0400, 0x0001, 0x7FE0,
        &[(0x1001, dt(2025, 4, 2, 8, 0, 0), 1800)],
    );
    feed(&mut filter, &make_ts_packet(0x0012, &sec));

    let db = db.borrow();
    assert_eq!(db.get_service_count(), 1);
    let svc = &db.get_service_list()[0];
    assert_eq!(svc.network_id, 0x7FE0);
    assert_eq!(svc.transport_stream_id, 0x0001);
    assert_eq!(svc.service_id, 0x0400);
    // schedule basic を受信したので has_schedule(basic) が true
    assert!(db.has_schedule(0x7FE0, 0x0001, 0x0400, false));
}

#[test]
fn test_leit_creates_service() {
    let db = new_db();
    let mut filter = EpgDatabaseFilter::new();
    filter.set_epg_database(Some(db.clone()));

    // L-EIT (PID 0x0027)
    let sec = make_eit_section(
        0x50, 0, 0, 7, 7, 0x50, 0x0500, 0x0002, 0x7FE1,
        &[(0x2001, dt(2025, 4, 3, 9, 0, 0), 1800)],
    );
    feed(&mut filter, &make_ts_packet(0x0027, &sec));

    let db = db.borrow();
    assert_eq!(db.get_service_count(), 1);
    assert!(db.has_schedule(0x7FE1, 0x0002, 0x0500, false));
}

#[test]
fn test_non_target_pid_ignored() {
    let db = new_db();
    let mut filter = EpgDatabaseFilter::new();
    filter.set_epg_database(Some(db.clone()));

    // EPG 対象外 PID に EIT 相当のデータを流しても DB は更新されない
    let sec = make_eit_section(
        0x50, 0, 0, 7, 7, 0x50, 0x0400, 0x0001, 0x7FE0,
        &[(0x1001, dt(2025, 4, 2, 8, 0, 0), 1800)],
    );
    feed(&mut filter, &make_ts_packet(0x0123, &sec));

    assert_eq!(db.borrow().get_service_count(), 0);
}

#[test]
fn test_tot_sets_time() {
    let db = new_db();
    let mut filter = EpgDatabaseFilter::new();
    filter.set_epg_database(Some(db.clone()));

    let time = dt(2025, 4, 1, 12, 0, 0);
    feed(&mut filter, &make_ts_packet(0x0014, &make_tot_section(&time)));

    let db = db.borrow();
    let tot = db.get_tot_time().expect("TOT time should be set");
    assert_eq!(tot.year, 2025);
    assert_eq!(tot.month, 4);
    assert_eq!(tot.day, 1);
    assert_eq!(tot.hour, 12);
}

#[test]
fn test_multiple_eit_sections_one_packet() {
    // 1 パケットに 2 サービスのセクションを詰めても両方が DB に反映される
    let db = new_db();
    let mut filter = EpgDatabaseFilter::new();
    filter.set_epg_database(Some(db.clone()));

    let s1 = make_eit_section(
        0x50, 0, 0, 7, 7, 0x50, 0x0400, 0x0001, 0x7FE0,
        &[(0x1001, dt(2025, 4, 2, 8, 0, 0), 1800)],
    );
    let s2 = make_eit_section(
        0x50, 0, 0, 7, 7, 0x50, 0x0401, 0x0001, 0x7FE0,
        &[(0x2001, dt(2025, 4, 2, 9, 0, 0), 1800)],
    );
    feed(&mut filter, &make_ts_packet_multi(0x0012, &[s1, s2]));

    let db = db.borrow();
    assert_eq!(db.get_service_count(), 2);
    assert!(db.has_schedule(0x7FE0, 0x0001, 0x0400, false));
    assert!(db.has_schedule(0x7FE0, 0x0001, 0x0401, false));
}

#[test]
fn test_no_database_no_panic() {
    // EPGDatabase 未設定でもパケット投入で panic しない & パススルーする
    let mut filter = EpgDatabaseFilter::new();
    let received = Rc::new(RefCell::new(Vec::new()));
    filter.connect_output(Box::new(RecordingSink { received: received.clone() }));

    let sec = make_eit_section(
        0x50, 0, 0, 7, 7, 0x50, 0x0400, 0x0001, 0x7FE0,
        &[(0x1001, dt(2025, 4, 2, 8, 0, 0), 1800)],
    );
    let pkt = make_ts_packet(0x0012, &sec);
    feed(&mut filter, &pkt);

    assert_eq!(received.borrow().len(), 1);
    assert_eq!(received.borrow()[0], pkt);
}

#[test]
fn test_source_id_roundtrip() {
    let mut filter = EpgDatabaseFilter::new();
    assert_eq!(filter.get_source_id(), 0);
    filter.set_source_id(0x1234_5678);
    assert_eq!(filter.get_source_id(), 0x1234_5678);
}

#[test]
fn test_reset_clears_tables_and_tot() {
    let db = new_db();
    let mut filter = EpgDatabaseFilter::new();
    filter.set_epg_database(Some(db.clone()));

    // TOT を入れてから Reset すると DB の TOT 時刻がクリアされる
    feed(&mut filter, &make_ts_packet(0x0014, &make_tot_section(&dt(2025, 4, 1, 12, 0, 0))));
    assert!(db.borrow().get_tot_time().is_some());

    filter.reset();
    assert!(db.borrow().get_tot_time().is_none());
}

#[test]
fn test_reset_on_date_change() {
    // TOT の日付が進むと、次の schedule セクションでスケジュールリセットが発火し、
    // フィルタが H-EIT/L-EIT 両テーブルの該当サービスを reset する経路を通る。
    let db = new_db();
    let mut filter = EpgDatabaseFilter::new();
    filter.set_epg_database(Some(db.clone()));

    // day1: TOT + schedule section0 → サービス S 確立(schedule_updated_time=day1)
    feed(&mut filter, &make_ts_packet(0x0014, &make_tot_section(&dt(2025, 4, 1, 12, 0, 0))));
    let s0 = make_eit_section(
        0x50, 0, 0, 7, 7, 0x50, 0x0400, 0x0001, 0x7FE0,
        &[(0x1001, dt(2025, 4, 2, 8, 0, 0), 1800)],
    );
    feed(&mut filter, &make_ts_packet(0x0012, &s0));
    assert!(db.borrow().has_schedule(0x7FE0, 0x0001, 0x0400, false));

    // day2 に進める
    feed(&mut filter, &make_ts_packet(0x0014, &make_tot_section(&dt(2025, 4, 2, 12, 0, 0))));

    // day2 で別セクション(section1)を投入 → 日付変化でリセット発火
    let s1 = make_eit_section(
        0x50, 0, 1, 7, 7, 0x50, 0x0400, 0x0001, 0x7FE0,
        &[(0x1002, dt(2025, 4, 2, 9, 0, 0), 1800)],
    );
    feed(&mut filter, &make_ts_packet(0x0012, &s1));

    // リセット経路を通っても整合は保たれる(サービスは存続、on_section で再追跡)
    let db = db.borrow();
    assert_eq!(db.get_service_count(), 1);
    assert!(db.has_schedule(0x7FE0, 0x0001, 0x0400, false));
}
