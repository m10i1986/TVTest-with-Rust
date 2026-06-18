// libisdb_recorder_filter のテスト
//
// ポンプモデルの同期録画(StreamSelector 絞り込み → StreamWriter 書き出し)、サービス
// 選択 / アクティブサービス追従 / 一時停止 / 書き込みエラー通知 / パススルーを検証する。
//
// 録画データは CloseWriter 時に writer がドロップされるため、書き出し先を共有 Arc で保持する
// SharedWriter を使い、stop(フラッシュ)後に内容を確認する。

use super::*;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use libisdb_filter_base::{FilterFn, SingleDataStream, TYPE_ID_DATA_BUFFER};
use libisdb_stream_selector::EsInfo;
use libisdb_ts_info::STREAM_TYPE_H264;

// 書き出し先を共有 Arc に持つ StreamWriter(stop 後も内容を確認できる)。
struct SharedWriter {
    data: Arc<Mutex<Vec<u8>>>,
    name: String,
    open: bool,
    write_size: u64,
    fail: bool,
}

impl SharedWriter {
    fn opened(data: Arc<Mutex<Vec<u8>>>) -> Self {
        Self { data, name: "rec.ts".to_string(), open: true, write_size: 0, fail: false }
    }
    fn failing(data: Arc<Mutex<Vec<u8>>>) -> Self {
        Self { data, name: "rec.ts".to_string(), open: true, write_size: 0, fail: true }
    }
}

impl StreamWriter for SharedWriter {
    fn open(&mut self, name: &str, _flags: OpenFlag) -> bool {
        if self.open {
            return false;
        }
        self.open = true;
        self.name = name.to_string();
        self.write_size = 0;
        true
    }
    fn reopen(&mut self, name: &str, _flags: OpenFlag) -> bool {
        self.open = true;
        self.name = name.to_string();
        true
    }
    fn close(&mut self) {
        self.open = false;
    }
    fn is_open(&self) -> bool {
        self.open
    }
    fn write(&mut self, buf: &[u8]) -> usize {
        if self.fail {
            return 0;
        }
        self.data.lock().unwrap().extend_from_slice(buf);
        self.write_size += buf.len() as u64;
        buf.len()
    }
    fn get_file_name(&self) -> Option<String> {
        if self.open && !self.name.is_empty() {
            Some(self.name.clone())
        } else {
            None
        }
    }
    fn get_write_size(&self) -> u64 {
        self.write_size
    }
    fn is_write_size_available(&self) -> bool {
        self.open
    }
}

// 188 バイト TS パケットを作る。fill で内容を識別する。
fn make_packet(pid: u16, fill: u8) -> [u8; TS_PACKET_SIZE] {
    let mut p = [fill; TS_PACKET_SIZE];
    p[0] = 0x47;
    p[1] = ((pid >> 8) & 0x1F) as u8;
    p[2] = (pid & 0xFF) as u8;
    p[3] = 0x10; // payload only, CC 0
    p
}

fn feed_packet(rec: &mut RecorderFilter, pkt: &[u8]) {
    let mut s = SingleDataStream::ts_packet(pkt);
    rec.receive_data(&mut s);
}

#[derive(Default)]
struct ErrRec {
    count: usize,
    last_id: TaskId,
}
impl RecorderEventListener for ErrRec {
    fn on_write_error(&mut self, task_id: TaskId) {
        self.count += 1;
        self.last_id = task_id;
    }
}

// ──────────────────────────────────────────────
// 基本
// ──────────────────────────────────────────────

#[test]
fn test_default_state() {
    let rec = RecorderFilter::new();
    assert_eq!(rec.task_count(), 0);
    assert_eq!(rec.active_service_id(), SERVICE_ID_INVALID);
}

#[test]
fn test_create_and_delete_task() {
    let mut rec = RecorderFilter::new();
    let data = Arc::new(Mutex::new(Vec::new()));
    let id = rec
        .create_task(Some(Box::new(SharedWriter::opened(data))), None)
        .unwrap();
    assert_eq!(rec.task_count(), 1);
    assert!(rec.is_task_valid(id));
    assert_eq!(rec.get_task_id_by_index(0), Some(id));

    assert!(rec.delete_task(id));
    assert_eq!(rec.task_count(), 0);
    assert!(!rec.is_task_valid(id));
    // 既に削除済み
    assert!(!rec.delete_task(id));
}

#[test]
fn test_default_options() {
    let opts = RecordingOptions::default();
    assert_eq!(opts.service_id, SERVICE_ID_INVALID);
    assert_eq!(opts.stream_flags, stream_flag::ALL);
    assert_eq!(opts.write_cache_size, 0);
    assert_eq!(opts.max_pending_size, 0);
    assert!(opts.clear_pending_buffer_on_service_changed);
    assert!(!opts.follow_active_service);
}

// ──────────────────────────────────────────────
// 録画(既定 = 全録画)
// ──────────────────────────────────────────────

#[test]
fn test_record_all_default_options() {
    let mut rec = RecorderFilter::new();
    let data = Arc::new(Mutex::new(Vec::new()));
    let id = rec
        .create_task(Some(Box::new(SharedWriter::opened(data.clone()))), None)
        .unwrap();

    let mut expected = Vec::new();
    for i in 0..10u8 {
        let pkt = make_packet(0x0100 + i as u16, i);
        expected.extend_from_slice(&pkt);
        feed_packet(&mut rec, &pkt);
    }

    // stop(delete)で残キャッシュをフラッシュ
    rec.delete_task(id);
    assert_eq!(*data.lock().unwrap(), expected);
}

#[test]
fn test_record_via_data_buffer() {
    let mut rec = RecorderFilter::new();
    let data = Arc::new(Mutex::new(Vec::new()));
    let id = rec
        .create_task(Some(Box::new(SharedWriter::opened(data.clone()))), None)
        .unwrap();

    let payload = vec![0xAAu8; 300];
    let mut s = SingleDataStream::new(TYPE_ID_DATA_BUFFER, &payload);
    rec.receive_data(&mut s);

    rec.delete_task(id);
    assert_eq!(*data.lock().unwrap(), payload);
}

// ──────────────────────────────────────────────
// サービス絞り込み
// ──────────────────────────────────────────────

fn pmt_two_services() -> Vec<PmtPidInfo> {
    vec![
        PmtPidInfo {
            service_id: 0x0400,
            pmt_pid: 0x1FC8,
            pcr_pid: 0x0100,
            ecm_pid_list: vec![],
            es_list: vec![EsInfo { pid: 0x0101, stream_type: STREAM_TYPE_H264 }],
        },
        PmtPidInfo {
            service_id: 0x0401,
            pmt_pid: 0x1FC9,
            pcr_pid: 0x0110,
            ecm_pid_list: vec![],
            es_list: vec![EsInfo { pid: 0x0111, stream_type: STREAM_TYPE_H264 }],
        },
    ]
}

#[test]
fn test_record_filters_by_service() {
    let mut rec = RecorderFilter::new();
    let data = Arc::new(Mutex::new(Vec::new()));
    let opts = RecordingOptions { service_id: 0x0400, ..Default::default() };
    let id = rec
        .create_task(Some(Box::new(SharedWriter::opened(data.clone()))), Some(opts))
        .unwrap();
    rec.get_task_mut(id).unwrap().set_pmt_pid_list(pmt_two_services());

    let pkt_target = make_packet(0x0101, 0xA1); // 対象サービスの ES → 通過
    let pkt_other = make_packet(0x0200, 0xB2); //  対象外 PID(>=0x0030)→ 破棄
    let pkt_sdt = make_packet(0x0011, 0xC3); //   PID<0x0030 → 常に通過

    feed_packet(&mut rec, &pkt_target);
    feed_packet(&mut rec, &pkt_other);
    feed_packet(&mut rec, &pkt_sdt);

    rec.delete_task(id);

    let mut expected = Vec::new();
    expected.extend_from_slice(&pkt_target);
    expected.extend_from_slice(&pkt_sdt);
    assert_eq!(*data.lock().unwrap(), expected);
}

#[test]
fn test_follow_active_service() {
    let mut rec = RecorderFilter::new();
    let data = Arc::new(Mutex::new(Vec::new()));
    let opts = RecordingOptions {
        service_id: 0x0400,
        follow_active_service: true,
        ..Default::default()
    };
    let id = rec
        .create_task(Some(Box::new(SharedWriter::opened(data.clone()))), Some(opts))
        .unwrap();
    rec.get_task_mut(id).unwrap().set_pmt_pid_list(pmt_two_services());

    let pkt_400 = make_packet(0x0101, 0x40); // service 0x0400 の ES
    let pkt_401 = make_packet(0x0111, 0x41); // service 0x0401 の ES

    // 切替前: 0x0400 を録画(0x0101 通過)
    feed_packet(&mut rec, &pkt_400);

    // アクティブサービスを 0x0401 へ → 追従して対象が切り替わる
    rec.set_active_service_id(0x0401);
    feed_packet(&mut rec, &pkt_401); // 通過
    feed_packet(&mut rec, &pkt_400); // 旧サービス → 破棄

    rec.delete_task(id);

    let mut expected = Vec::new();
    expected.extend_from_slice(&pkt_400);
    expected.extend_from_slice(&pkt_401);
    assert_eq!(*data.lock().unwrap(), expected);
}

// ──────────────────────────────────────────────
// 一時停止 / 再開
// ──────────────────────────────────────────────

#[test]
fn test_pause_resume() {
    let mut rec = RecorderFilter::new();
    let data = Arc::new(Mutex::new(Vec::new()));
    let id = rec
        .create_task(Some(Box::new(SharedWriter::opened(data.clone()))), None)
        .unwrap();

    let a = make_packet(0x0100, 0xA0);
    let b = make_packet(0x0100, 0xB0);
    let c = make_packet(0x0100, 0xC0);

    feed_packet(&mut rec, &a);
    rec.get_task_mut(id).unwrap().pause();
    assert!(rec.get_task(id).unwrap().is_paused());
    feed_packet(&mut rec, &b); // 一時停止中 → 破棄
    rec.get_task_mut(id).unwrap().resume();
    assert!(!rec.get_task(id).unwrap().is_paused());
    feed_packet(&mut rec, &c);

    rec.delete_task(id);

    let mut expected = Vec::new();
    expected.extend_from_slice(&a);
    expected.extend_from_slice(&c);
    assert_eq!(*data.lock().unwrap(), expected);
}

// ──────────────────────────────────────────────
// 複数タスク
// ──────────────────────────────────────────────

#[test]
fn test_multiple_tasks_record_independently() {
    let mut rec = RecorderFilter::new();
    let data1 = Arc::new(Mutex::new(Vec::new()));
    let data2 = Arc::new(Mutex::new(Vec::new()));
    let id1 = rec
        .create_task(Some(Box::new(SharedWriter::opened(data1.clone()))), None)
        .unwrap();
    let id2 = rec
        .create_task(Some(Box::new(SharedWriter::opened(data2.clone()))), None)
        .unwrap();
    assert_eq!(rec.task_count(), 2);

    let pkt = make_packet(0x0100, 0x55);
    feed_packet(&mut rec, &pkt);

    rec.delete_task(id1);
    rec.delete_task(id2);

    assert_eq!(*data1.lock().unwrap(), pkt.to_vec());
    assert_eq!(*data2.lock().unwrap(), pkt.to_vec());
}

// ──────────────────────────────────────────────
// 書き込みエラー通知
// ──────────────────────────────────────────────

#[test]
fn test_write_error_notifies_listener() {
    let mut rec = RecorderFilter::new();
    let listener = Rc::new(RefCell::new(ErrRec::default()));
    let handle: RecorderEventHandle = listener.clone();
    rec.add_event_listener(handle);

    let data = Arc::new(Mutex::new(Vec::new()));
    let id = rec
        .create_task(Some(Box::new(SharedWriter::failing(data))), None)
        .unwrap();

    // 既定キャッシュ 1024。6 パケット(1128 バイト)でフラッシュが発生し書き込み失敗。
    for i in 0..7u8 {
        let pkt = make_packet(0x0100, i);
        feed_packet(&mut rec, &pkt);
    }

    // 初回エラーで 1 回だけ通知(output_error_notified で再通知抑止)
    assert_eq!(listener.borrow().count, 1);
    assert_eq!(listener.borrow().last_id, id);
}

#[test]
fn test_event_listener_add_remove_dedup() {
    let mut rec = RecorderFilter::new();
    let listener = Rc::new(RefCell::new(ErrRec::default()));
    let handle: RecorderEventHandle = listener.clone();

    assert!(rec.add_event_listener(handle.clone()));
    // 同一リスナの二重登録は不可
    assert!(!rec.add_event_listener(handle.clone()));
    assert!(rec.remove_event_listener(&handle));
    // 既に削除済み
    assert!(!rec.remove_event_listener(&handle));
}

// ──────────────────────────────────────────────
// パススルー
// ──────────────────────────────────────────────

#[test]
fn test_passthrough_to_downstream() {
    let mut rec = RecorderFilter::new();
    let received = Rc::new(RefCell::new(Vec::<Vec<u8>>::new()));
    let sink = received.clone();
    rec.output().connect(Box::new(FilterFn(move |stream: &mut dyn DataStream| {
        sink.borrow_mut().push(stream.data().to_vec());
        true
    })));

    let data = Arc::new(Mutex::new(Vec::new()));
    let id = rec
        .create_task(Some(Box::new(SharedWriter::opened(data.clone()))), None)
        .unwrap();

    let pkt = make_packet(0x0100, 0x77);
    feed_packet(&mut rec, &pkt);

    // 下流へパススルーされている
    assert_eq!(received.borrow().len(), 1);
    assert_eq!(received.borrow()[0], pkt.to_vec());

    // 録画もされている
    rec.delete_task(id);
    assert_eq!(*data.lock().unwrap(), pkt.to_vec());
}

// ──────────────────────────────────────────────
// オプション / 統計 / ファイル名 / 再オープン
// ──────────────────────────────────────────────

#[test]
fn test_set_options_updates_target() {
    let mut rec = RecorderFilter::new();
    let data = Arc::new(Mutex::new(Vec::new()));
    let id = rec
        .create_task(Some(Box::new(SharedWriter::opened(data.clone()))), None)
        .unwrap();
    rec.get_task_mut(id).unwrap().set_pmt_pid_list(pmt_two_services());

    // 既定は全録画。サービス 0x0400 に変更
    let opts = RecordingOptions { service_id: 0x0400, ..Default::default() };
    assert!(rec.get_task_mut(id).unwrap().set_options(&opts));
    assert_eq!(rec.get_task(id).unwrap().get_options().service_id, 0x0400);

    // 0x0401 の ES は破棄される
    let pkt_other = make_packet(0x0111, 0x99);
    let pkt_target = make_packet(0x0101, 0x11);
    feed_packet(&mut rec, &pkt_other);
    feed_packet(&mut rec, &pkt_target);

    rec.delete_task(id);
    assert_eq!(*data.lock().unwrap(), pkt_target.to_vec());
}

#[test]
fn test_statistics_input_bytes() {
    let mut rec = RecorderFilter::new();
    let data = Arc::new(Mutex::new(Vec::new()));
    let id = rec
        .create_task(Some(Box::new(SharedWriter::opened(data))), None)
        .unwrap();

    for i in 0..3u8 {
        let pkt = make_packet(0x0100, i);
        feed_packet(&mut rec, &pkt);
    }

    // 3 パケット = 564 バイト。キャッシュ(1024)未満なので未フラッシュ。
    let stats = rec.get_task(id).unwrap().get_statistics();
    assert_eq!(stats.input_bytes, 3 * TS_PACKET_SIZE as u64);
    assert_eq!(stats.output_bytes, 0);
    assert_eq!(stats.write_bytes, 0); // まだ書き出されていない
}

#[test]
fn test_get_file_name() {
    let mut rec = RecorderFilter::new();
    let data = Arc::new(Mutex::new(Vec::new()));
    let id = rec
        .create_task(Some(Box::new(SharedWriter::opened(data))), None)
        .unwrap();
    assert_eq!(rec.get_task(id).unwrap().get_file_name().as_deref(), Some("rec.ts"));
}

#[test]
fn test_reopen_changes_file_name() {
    let mut rec = RecorderFilter::new();
    let data = Arc::new(Mutex::new(Vec::new()));
    let id = rec
        .create_task(Some(Box::new(SharedWriter::opened(data))), None)
        .unwrap();
    assert!(rec.get_task_mut(id).unwrap().reopen("next.ts", OpenFlag::OVERWRITE));
    assert_eq!(rec.get_task(id).unwrap().get_file_name().as_deref(), Some("next.ts"));
}

#[test]
fn test_set_writer_replaces_output() {
    let mut rec = RecorderFilter::new();
    let data1 = Arc::new(Mutex::new(Vec::new()));
    let data2 = Arc::new(Mutex::new(Vec::new()));
    let id = rec
        .create_task(Some(Box::new(SharedWriter::opened(data1.clone()))), None)
        .unwrap();

    // 出力先を data2 の writer に差し替え
    assert!(rec
        .get_task_mut(id)
        .unwrap()
        .set_writer(Some(Box::new(SharedWriter::opened(data2.clone())))));

    let pkt = make_packet(0x0100, 0x88);
    feed_packet(&mut rec, &pkt);
    rec.delete_task(id);

    // 差し替え後の writer にのみ書き出される
    assert!(data1.lock().unwrap().is_empty());
    assert_eq!(*data2.lock().unwrap(), pkt.to_vec());
}

#[test]
fn test_delete_all_tasks_flushes() {
    let mut rec = RecorderFilter::new();
    let data = Arc::new(Mutex::new(Vec::new()));
    rec.create_task(Some(Box::new(SharedWriter::opened(data.clone()))), None)
        .unwrap();

    let pkt = make_packet(0x0100, 0x33);
    feed_packet(&mut rec, &pkt);

    rec.delete_all_tasks();
    assert_eq!(rec.task_count(), 0);
    // delete_all_tasks の Drop でフラッシュされている
    assert_eq!(*data.lock().unwrap(), pkt.to_vec());
}

#[test]
fn test_finalize_deletes_tasks() {
    let mut rec = RecorderFilter::new();
    let data = Arc::new(Mutex::new(Vec::new()));
    rec.create_task(Some(Box::new(SharedWriter::opened(data))), None)
        .unwrap();
    assert_eq!(rec.task_count(), 1);
    rec.finalize();
    assert_eq!(rec.task_count(), 0);
}
