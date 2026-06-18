// libisdb_stream_source_filter のテスト
//
// Pull(fetch_source)/ Push(process_stream)双方のデータフローと、ソース/ストリーミング
// のイベント通知、各種ガード(SetSourceMode のオープン中ロック等)をポンプモデルで検証する。

use super::*;
use std::cell::RefCell;
use std::rc::Rc;

use libisdb_filter_base::{DataStream, FilterFn};
use libisdb_source_filter::{SourceEventListener, SourceEventListenerHandle};
use libisdb_stream::MemoryStream;

// 下流で受信したチャンクを記録するシンクを接続する。
fn attach_recorder(filter: &mut StreamSourceFilter) -> Rc<RefCell<Vec<Vec<u8>>>> {
    let received = Rc::new(RefCell::new(Vec::<Vec<u8>>::new()));
    let sink = received.clone();
    filter
        .output()
        .connect(Box::new(FilterFn(move |stream: &mut dyn DataStream| {
            sink.borrow_mut().push(stream.data().to_vec());
            while stream.next() {
                sink.borrow_mut().push(stream.data().to_vec());
            }
            true
        })));
    received
}

#[derive(Default)]
struct RecListener {
    opened: usize,
    closed: usize,
    end: usize,
    streaming_start: usize,
    streaming_stop: usize,
    graph_reset: usize,
}

impl SourceEventListener for RecListener {
    fn on_source_opened(&mut self) {
        self.opened += 1;
    }
    fn on_source_closed(&mut self) {
        self.closed += 1;
    }
    fn on_source_end(&mut self) {
        self.end += 1;
    }
    fn on_streaming_start(&mut self) {
        self.streaming_start += 1;
    }
    fn on_streaming_stop(&mut self) {
        self.streaming_stop += 1;
    }
    fn on_graph_reset(&mut self) {
        self.graph_reset += 1;
    }
}

fn concat(chunks: &[Vec<u8>]) -> Vec<u8> {
    let mut v = Vec::new();
    for c in chunks {
        v.extend_from_slice(c);
    }
    v
}

// ──────────────────────────────────────────────
// 既定値 / オープン
// ──────────────────────────────────────────────

#[test]
fn test_default_state() {
    let filter = StreamSourceFilter::new();
    assert_eq!(filter.get_source_mode(), SourceMode::PUSH); // C++ 既定 Push
    assert_eq!(
        filter.available_source_modes(),
        SourceMode::PUSH | SourceMode::PULL
    );
    assert!(!filter.is_source_open());
    assert!(!filter.is_streaming());
    assert_eq!(filter.get_output_buffer_size(), 256 * 188);
    assert_eq!(filter.get_input_bytes(), 0);
}

#[test]
fn test_open_source_stream_twice_fails() {
    let mut filter = StreamSourceFilter::new();
    assert!(filter.open_source_stream(Box::new(MemoryStream::new(vec![1, 2, 3]))));
    assert!(filter.is_source_open());
    // 既にオープン中なら false
    assert!(!filter.open_source_stream(Box::new(MemoryStream::new(vec![4, 5, 6]))));
}

#[test]
fn test_open_source_by_name_unsupported() {
    let mut filter = StreamSourceFilter::new();
    assert!(!filter.open_source("dummy.ts"));
}

#[test]
fn test_open_source_notifies_listener() {
    let mut filter = StreamSourceFilter::new();
    let l = Rc::new(RefCell::new(RecListener::default()));
    let handle: SourceEventListenerHandle = l.clone();
    filter.add_event_listener(handle);

    filter.open_source_stream(Box::new(MemoryStream::new(vec![1, 2, 3])));
    assert_eq!(l.borrow().opened, 1);
}

// ──────────────────────────────────────────────
// SetSourceMode
// ──────────────────────────────────────────────

#[test]
fn test_set_source_mode_before_open() {
    let mut filter = StreamSourceFilter::new();
    assert!(filter.set_source_mode(SourceMode::PULL));
    assert_eq!(filter.get_source_mode(), SourceMode::PULL);
    assert!(filter.set_source_mode(SourceMode::PUSH));
    assert_eq!(filter.get_source_mode(), SourceMode::PUSH);
}

#[test]
fn test_set_source_mode_blocked_when_open() {
    let mut filter = StreamSourceFilter::new();
    filter.open_source_stream(Box::new(MemoryStream::new(vec![1])));
    // オープン中は変更不可
    assert!(!filter.set_source_mode(SourceMode::PULL));
    assert_eq!(filter.get_source_mode(), SourceMode::PUSH);
}

#[test]
fn test_set_source_mode_rejects_combined() {
    let mut filter = StreamSourceFilter::new();
    assert!(!filter.set_source_mode(SourceMode::PUSH | SourceMode::PULL));
    assert_eq!(filter.get_source_mode(), SourceMode::PUSH);
}

// ──────────────────────────────────────────────
// Push: process_stream
// ──────────────────────────────────────────────

#[test]
fn test_push_process_stream_outputs_and_counts() {
    let mut filter = StreamSourceFilter::new();
    let received = attach_recorder(&mut filter);

    let data: Vec<u8> = (0u8..100).collect();
    assert!(filter.open_source_stream(Box::new(MemoryStream::new(data.clone()))));
    assert!(filter.start_streaming());

    // 1 回読み切る(output_buffer_size > データ長)
    assert!(filter.process_stream());
    assert_eq!(concat(&received.borrow()), data);
    assert_eq!(filter.get_input_bytes(), 100);

    // もう読めない
    assert!(!filter.process_stream());
}

#[test]
fn test_process_stream_counts_input_even_when_not_streaming() {
    let mut filter = StreamSourceFilter::new();
    let received = attach_recorder(&mut filter);

    let data = vec![0xABu8; 50];
    filter.open_source_stream(Box::new(MemoryStream::new(data)));
    // start_streaming を呼ばない → is_streaming = false

    assert!(filter.process_stream());
    // 入力バイトは積算されるが下流へは出さない(C++ StreamingMain と同じ)
    assert_eq!(filter.get_input_bytes(), 50);
    assert!(received.borrow().is_empty());
}

#[test]
fn test_process_stream_chunks_by_output_buffer_size() {
    let mut filter = StreamSourceFilter::new();
    assert!(filter.set_output_buffer_size(30));
    let received = attach_recorder(&mut filter);

    let data: Vec<u8> = (0..100u32).map(|i| (i % 256) as u8).collect();
    filter.open_source_stream(Box::new(MemoryStream::new(data.clone())));
    filter.start_streaming();

    let mut iterations = 0;
    while filter.process_stream() {
        iterations += 1;
        assert!(iterations < 100);
    }

    let chunks = received.borrow().clone();
    for c in &chunks {
        assert!(c.len() <= 30);
    }
    assert_eq!(concat(&chunks), data);
    assert_eq!(filter.get_input_bytes(), 100);
}

#[test]
fn test_process_stream_no_stream_is_false() {
    let mut filter = StreamSourceFilter::new();
    assert!(!filter.process_stream());
}

// ──────────────────────────────────────────────
// Pull: fetch_source
// ──────────────────────────────────────────────

#[test]
fn test_fetch_source_pull_mode() {
    let mut filter = StreamSourceFilter::new();
    assert!(filter.set_source_mode(SourceMode::PULL));
    let received = attach_recorder(&mut filter);

    let data: Vec<u8> = (0u8..60).collect();
    filter.open_source_stream(Box::new(MemoryStream::new(data.clone())));
    filter.start_streaming();

    assert!(filter.fetch_source(60));
    assert_eq!(concat(&received.borrow()), data);
}

#[test]
fn test_fetch_source_clamps_to_output_buffer_size() {
    let mut filter = StreamSourceFilter::new();
    assert!(filter.set_source_mode(SourceMode::PULL));
    assert!(filter.set_output_buffer_size(10));
    let received = attach_recorder(&mut filter);

    filter.open_source_stream(Box::new(MemoryStream::new(vec![0xCDu8; 100])));
    filter.start_streaming();

    // 要求 1000 でも output_buffer_size(10)に丸められる
    assert!(filter.fetch_source(1000));
    let chunks = received.borrow().clone();
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].len(), 10);
}

#[test]
fn test_fetch_source_requires_streaming() {
    let mut filter = StreamSourceFilter::new();
    filter.set_source_mode(SourceMode::PULL);
    filter.open_source_stream(Box::new(MemoryStream::new(vec![1, 2, 3])));
    // start_streaming を呼んでいない
    assert!(!filter.fetch_source(3));
}

#[test]
fn test_fetch_source_requires_pull_mode() {
    let mut filter = StreamSourceFilter::new();
    // 既定は Push
    filter.open_source_stream(Box::new(MemoryStream::new(vec![1, 2, 3])));
    filter.start_streaming();
    assert!(!filter.fetch_source(3));
}

// ──────────────────────────────────────────────
// イベント通知 / クローズ / 停止
// ──────────────────────────────────────────────

#[test]
fn test_source_end_notification() {
    let mut filter = StreamSourceFilter::new();
    let l = Rc::new(RefCell::new(RecListener::default()));
    let handle: SourceEventListenerHandle = l.clone();
    filter.add_event_listener(handle);

    filter.open_source_stream(Box::new(MemoryStream::new(vec![1, 2, 3, 4])));
    filter.start_streaming();

    // 全部読み切ると EOF 通知(read < bufsize かつ is_end)
    filter.process_stream();
    assert_eq!(l.borrow().end, 1);
    assert_eq!(l.borrow().streaming_start, 1);
}

#[test]
fn test_close_source_notifies_and_clears() {
    let mut filter = StreamSourceFilter::new();
    let l = Rc::new(RefCell::new(RecListener::default()));
    let handle: SourceEventListenerHandle = l.clone();
    filter.add_event_listener(handle);

    filter.open_source_stream(Box::new(MemoryStream::new(vec![1, 2, 3])));
    assert!(filter.is_source_open());

    assert!(filter.close_source());
    assert!(!filter.is_source_open());
    assert_eq!(l.borrow().closed, 1);
    // クローズ後はモード変更可
    assert!(filter.set_source_mode(SourceMode::PULL));
}

#[test]
fn test_stop_streaming_notifies() {
    let mut filter = StreamSourceFilter::new();
    let l = Rc::new(RefCell::new(RecListener::default()));
    let handle: SourceEventListenerHandle = l.clone();
    filter.add_event_listener(handle);

    filter.open_source_stream(Box::new(MemoryStream::new(vec![1, 2, 3])));
    filter.start_streaming();
    assert!(filter.is_streaming());

    assert!(filter.stop_streaming());
    assert!(!filter.is_streaming());
    assert_eq!(l.borrow().streaming_stop, 1);
}

#[test]
fn test_reset_graph_notifies() {
    let mut filter = StreamSourceFilter::new();
    let l = Rc::new(RefCell::new(RecListener::default()));
    let handle: SourceEventListenerHandle = l.clone();
    filter.add_event_listener(handle);

    filter.reset_graph();
    assert_eq!(l.borrow().graph_reset, 1);
}

#[test]
fn test_start_streaming_requires_stream() {
    let mut filter = StreamSourceFilter::new();
    // ストリーム未オープン → false
    assert!(!filter.start_streaming());
}

#[test]
fn test_set_output_buffer_size_validation() {
    let mut filter = StreamSourceFilter::new();
    assert!(!filter.set_output_buffer_size(0)); // < 1
    assert!(filter.set_output_buffer_size(1));
    assert_eq!(filter.get_output_buffer_size(), 1);
}
