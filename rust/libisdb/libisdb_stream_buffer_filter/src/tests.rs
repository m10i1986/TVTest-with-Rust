// libisdb_stream_buffer_filter のテスト
//
// 通常運用(出力 StreamBuffer のみ設定・入力/キャッシュ未設定)では同期動作するため
// 決定論的に検証できる: buffering 無効=蓄積しない / 有効=出力バッファへ直接 push /
// パススルーで下流へ転送 / Reset の clear_on_reset 連動 / 各種バッファ管理。

use super::*;
use std::cell::RefCell;
use std::rc::Rc;

use libisdb_filter_base::{FilterFn, SingleDataStream, SliceDataStream, TYPE_ID_TS_PACKET};
use libisdb_stream_buffer::{Reader, SequentialReader};

// 出力バッファの内容を読み出すヘルパ。
fn read_all(buf: &Arc<StreamBuffer>) -> Vec<u8> {
    let end = buf.get_end_pos();
    if end <= 0 {
        return Vec::new();
    }
    let mut reader = SequentialReader::new();
    reader.open(Arc::clone(buf));
    let mut out = vec![0u8; end as usize];
    let n = reader.read(&mut out);
    out.truncate(n);
    out
}

// 下流で受信したデータを記録するシンクを output に接続する。
fn attach_recorder(filter: &mut StreamBufferFilter) -> Rc<RefCell<Vec<Vec<u8>>>> {
    let received = Rc::new(RefCell::new(Vec::<Vec<u8>>::new()));
    let sink = received.clone();
    filter.output().connect(Box::new(FilterFn(move |stream: &mut dyn DataStream| {
        sink.borrow_mut().push(stream.data().to_vec());
        while stream.next() {
            sink.borrow_mut().push(stream.data().to_vec());
        }
        true
    })));
    received
}

// ──────────────────────────────────────────────
// 既定値
// ──────────────────────────────────────────────

#[test]
fn test_default_flags() {
    let filter = StreamBufferFilter::new();
    assert!(!filter.get_buffering_enabled()); // C++ 既定 false
    assert!(filter.get_clear_on_reset()); // C++ 既定 true
    assert!(!filter.is_buffer_created());
}

// ──────────────────────────────────────────────
// バッファリング無効 / 有効
// ──────────────────────────────────────────────

#[test]
fn test_buffering_disabled_does_not_accumulate() {
    let mut filter = StreamBufferFilter::new();
    assert!(filter.create_memory_buffer(256, 2, 8));

    // 既定で buffering 無効
    let data = [0xABu8; 188];
    let mut s = SingleDataStream::new(TYPE_ID_TS_PACKET, &data);
    assert!(filter.receive_data(&mut s));

    let buf = filter.get_buffer().unwrap();
    assert!(buf.is_empty(), "buffering 無効なので蓄積されない");
}

#[test]
fn test_buffering_enabled_writes_to_output_buffer() {
    let mut filter = StreamBufferFilter::new();
    assert!(filter.create_memory_buffer(256, 2, 8));
    // 入力バッファ無し → start はスレッドを起動せず同期動作
    assert!(filter.set_buffering_enabled(true));
    assert!(filter.get_buffering_enabled());

    let data: Vec<u8> = (0u8..188).collect();
    let mut s = SingleDataStream::new(TYPE_ID_TS_PACKET, &data);
    assert!(filter.receive_data(&mut s));

    let buf = filter.get_buffer().unwrap();
    assert_eq!(read_all(&buf), data, "buffering 有効で出力バッファへ同期 push される");
}

#[test]
fn test_buffering_enabled_multi_item_stream() {
    let mut filter = StreamBufferFilter::new();
    assert!(filter.create_memory_buffer(512, 2, 8));
    assert!(filter.set_buffering_enabled(true));

    let a = [1u8; 100];
    let b = [2u8; 100];
    let items: &[&[u8]] = &[&a, &b];
    let mut s = SliceDataStream::new(TYPE_ID_TS_PACKET, items);
    assert!(filter.receive_data(&mut s));

    let buf = filter.get_buffer().unwrap();
    let mut expected = Vec::new();
    expected.extend_from_slice(&a);
    expected.extend_from_slice(&b);
    assert_eq!(read_all(&buf), expected, "複数要素が全て蓄積される");
}

// ──────────────────────────────────────────────
// パススルー
// ──────────────────────────────────────────────

#[test]
fn test_passthrough_forwards_downstream() {
    let mut filter = StreamBufferFilter::new();
    let received = attach_recorder(&mut filter);

    // buffering 無効でも下流へ転送される
    let data = [0x47u8; 188];
    let mut s = SingleDataStream::new(TYPE_ID_TS_PACKET, &data);
    assert!(filter.receive_data(&mut s));

    let got = received.borrow().clone();
    assert_eq!(got, vec![data.to_vec()]);
}

#[test]
fn test_passthrough_with_buffering_and_multi_item() {
    let mut filter = StreamBufferFilter::new();
    assert!(filter.create_memory_buffer(512, 2, 8));
    assert!(filter.set_buffering_enabled(true));
    let received = attach_recorder(&mut filter);

    let a = [1u8; 50];
    let b = [2u8; 50];
    let items: &[&[u8]] = &[&a, &b];
    let mut s = SliceDataStream::new(TYPE_ID_TS_PACKET, items);
    assert!(filter.receive_data(&mut s));

    // process_data がストリームを消費した後でも、OutputSlot::send が rewind するため
    // 下流は全要素を受け取る
    let got = received.borrow().clone();
    assert_eq!(got, vec![a.to_vec(), b.to_vec()]);
}

// ──────────────────────────────────────────────
// バッファ管理
// ──────────────────────────────────────────────

#[test]
fn test_create_memory_buffer_invalid_params() {
    let filter = StreamBufferFilter::new();
    assert!(!filter.create_memory_buffer(0, 2, 8)); // block_size 0
    assert!(!filter.create_memory_buffer(256, 2, 0)); // max 0
    assert!(!filter.create_memory_buffer(256, 8, 2)); // min > max
    assert!(!filter.is_buffer_created());
}

#[test]
fn test_buffer_set_get_detach_delete() {
    let filter = StreamBufferFilter::new();
    assert!(!filter.is_buffer_created());

    let buf = Arc::new(StreamBuffer::new());
    assert!(buf.create(256, 2, 8, None));
    assert!(filter.set_buffer(Arc::clone(&buf)));
    assert!(filter.is_buffer_created());

    let got = filter.get_buffer().unwrap();
    assert!(Arc::ptr_eq(&got, &buf));

    let detached = filter.detach_buffer().unwrap();
    assert!(Arc::ptr_eq(&detached, &buf));
    assert!(!filter.is_buffer_created(), "detach 後は保持しない");

    // 再設定して delete
    assert!(filter.set_buffer(buf));
    assert!(filter.is_buffer_created());
    filter.delete_buffer();
    assert!(!filter.is_buffer_created());
}

#[test]
fn test_clear_buffer_empties_output() {
    let mut filter = StreamBufferFilter::new();
    assert!(filter.create_memory_buffer(256, 2, 8));
    assert!(filter.set_buffering_enabled(true));

    let data = [0x55u8; 100];
    let mut s = SingleDataStream::new(TYPE_ID_TS_PACKET, &data);
    filter.receive_data(&mut s);
    assert!(!filter.get_buffer().unwrap().is_empty());

    filter.clear_buffer();
    assert!(filter.get_buffer().unwrap().is_empty());
}

// ──────────────────────────────────────────────
// Reset の clear_on_reset 連動
// ──────────────────────────────────────────────

#[test]
fn test_reset_clears_when_clear_on_reset() {
    let mut filter = StreamBufferFilter::new();
    assert!(filter.create_memory_buffer(256, 2, 8));
    assert!(filter.set_buffering_enabled(true));

    let data = [0x66u8; 100];
    let mut s = SingleDataStream::new(TYPE_ID_TS_PACKET, &data);
    filter.receive_data(&mut s);
    assert!(!filter.get_buffer().unwrap().is_empty());

    // 既定 clear_on_reset=true → Reset で出力バッファがクリアされる
    filter.reset();
    assert!(filter.get_buffer().unwrap().is_empty());
}

#[test]
fn test_reset_keeps_buffer_when_clear_on_reset_false() {
    let mut filter = StreamBufferFilter::new();
    assert!(filter.create_memory_buffer(256, 2, 8));
    assert!(filter.set_buffering_enabled(true));
    filter.set_clear_on_reset(false);
    assert!(!filter.get_clear_on_reset());

    let data = [0x77u8; 100];
    let mut s = SingleDataStream::new(TYPE_ID_TS_PACKET, &data);
    filter.receive_data(&mut s);
    assert!(!filter.get_buffer().unwrap().is_empty());

    filter.reset();
    assert!(!filter.get_buffer().unwrap().is_empty(), "clear_on_reset=false なら保持");
}

// ──────────────────────────────────────────────
// ペンディング(入力)バッファ / buffering トグル
// ──────────────────────────────────────────────

#[test]
fn test_set_pending_buffer_size_creates_and_resizes() {
    let filter = StreamBufferFilter::new();
    // 作成
    assert!(filter.set_pending_buffer_size(1024, 16));
    // 既存ならリサイズ(true)
    assert!(filter.set_pending_buffer_size(2048, 8));
}

#[test]
fn test_set_buffering_enabled_idempotent_and_toggle() {
    let mut filter = StreamBufferFilter::new();
    assert!(filter.create_memory_buffer(256, 2, 8));

    // 同値設定でも true(変更なし)
    assert!(filter.set_buffering_enabled(false));
    assert!(!filter.get_buffering_enabled());

    // 有効化
    assert!(filter.set_buffering_enabled(true));
    assert!(filter.get_buffering_enabled());
    // 再度有効化(変更なし)
    assert!(filter.set_buffering_enabled(true));

    // 無効化(stop→flush→close)
    assert!(filter.set_buffering_enabled(false));
    assert!(!filter.get_buffering_enabled());
}
