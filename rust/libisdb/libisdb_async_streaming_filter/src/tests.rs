// libisdb_async_streaming_filter のテスト
//
// ReceiveData は StreamBuffer に積むだけ(パススルーしない)で、出力は process_stream
// で行う、という非同期フィルタのデータフローをポンプモデルで決定論的に検証する。

use super::*;
use std::cell::RefCell;
use std::rc::Rc;

use libisdb_filter_base::{FilterFn, SingleDataStream, SliceDataStream, TYPE_ID_TS_PACKET};
use libisdb_source_filter::{SourceFilterState, SourceMode};

// 下流で受信したチャンクを記録するシンクを接続する。
fn attach_recorder(filter: &mut AsyncStreamingFilter) -> Rc<RefCell<Vec<Vec<u8>>>> {
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

// fetch_source の要求サイズを記録するモックソースフィルタ。
struct MockSource {
    state: SourceFilterState,
    available: SourceMode,
    fetched: Vec<usize>,
    open: bool,
}

impl MockSource {
    fn new(mode: SourceMode, available: SourceMode) -> Self {
        Self {
            state: SourceFilterState::new(mode),
            available,
            fetched: Vec::new(),
            open: false,
        }
    }
}

impl SourceFilter for MockSource {
    fn open_source(&mut self, _name: &str) -> bool {
        self.open = true;
        true
    }
    fn close_source(&mut self) -> bool {
        self.open = false;
        true
    }
    fn is_source_open(&self) -> bool {
        self.open
    }
    fn fetch_source(&mut self, request_size: usize) -> bool {
        self.fetched.push(request_size);
        true
    }
    fn available_source_modes(&self) -> SourceMode {
        self.available
    }
    fn source_state(&self) -> &SourceFilterState {
        &self.state
    }
    fn source_state_mut(&mut self) -> &mut SourceFilterState {
        &mut self.state
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
// 既定値 / バッファ管理
// ──────────────────────────────────────────────

#[test]
fn test_default_state() {
    let filter = AsyncStreamingFilter::new();
    assert!(filter.get_buffering_enabled()); // C++ 既定 true
    assert!(filter.get_clear_on_reset()); // C++ 既定 true
    assert_eq!(filter.get_output_buffer_size(), 256 * 188);
    assert!(!filter.is_started());
    assert!(!filter.is_buffer_created());
}

#[test]
fn test_create_buffer_invalid_params() {
    let mut filter = AsyncStreamingFilter::new();
    assert!(!filter.create_buffer(0, 2, 8));
    assert!(!filter.create_buffer(512, 2, 0));
    assert!(!filter.create_buffer(512, 8, 2));
    assert!(!filter.is_buffer_created());
}

#[test]
fn test_buffer_set_get_detach_delete() {
    let mut filter = AsyncStreamingFilter::new();
    assert!(filter.create_buffer(512, 2, 8));
    assert!(filter.is_buffer_created());

    let buf = filter.get_buffer().unwrap();

    let other = Arc::new(StreamBuffer::new());
    assert!(other.create(512, 2, 8, None));
    assert!(filter.set_buffer(Arc::clone(&other)));
    assert!(Arc::ptr_eq(&filter.get_buffer().unwrap(), &other));
    assert!(!Arc::ptr_eq(&filter.get_buffer().unwrap(), &buf));

    let detached = filter.detach_buffer().unwrap();
    assert!(Arc::ptr_eq(&detached, &other));
    assert!(!filter.is_buffer_created());

    assert!(filter.set_buffer(other));
    filter.delete_buffer();
    assert!(!filter.is_buffer_created());
}

// ──────────────────────────────────────────────
// ReceiveData(蓄積のみ)→ process_stream(出力)
// ──────────────────────────────────────────────

#[test]
fn test_receive_buffers_then_process_outputs() {
    let mut filter = AsyncStreamingFilter::new();
    assert!(filter.create_buffer(1024, 2, 8));
    let received = attach_recorder(&mut filter);

    assert!(filter.start_streaming());

    let data: Vec<u8> = (0u8..188).collect();
    let mut s = SingleDataStream::new(TYPE_ID_TS_PACKET, &data);
    assert!(filter.receive_data(&mut s));

    // receive_data 単体では下流へ出ない(パススルーしない)
    assert!(received.borrow().is_empty());

    // process_stream で初めて下流へ出力される
    assert!(filter.process_stream());
    assert_eq!(concat(&received.borrow()), data);

    // もうデータは無い
    assert!(!filter.process_stream());
}

#[test]
fn test_buffering_disabled_does_not_buffer() {
    let mut filter = AsyncStreamingFilter::new();
    assert!(filter.create_buffer(1024, 2, 8));
    assert!(filter.set_buffering_enabled(false));
    assert!(filter.start_streaming());

    let data = [0xAAu8; 188];
    let mut s = SingleDataStream::new(TYPE_ID_TS_PACKET, &data);
    filter.receive_data(&mut s);

    // 何も積まれていないので process_stream は false
    assert!(!filter.process_stream());
    assert!(filter.get_buffer().unwrap().is_empty());
}

#[test]
fn test_no_buffer_no_panic() {
    // バッファ未作成でも receive_data / process_stream はパニックしない
    let mut filter = AsyncStreamingFilter::new();
    assert!(filter.start_streaming());
    let data = [1u8; 10];
    let mut s = SingleDataStream::new(TYPE_ID_TS_PACKET, &data);
    assert!(filter.receive_data(&mut s));
    assert!(!filter.process_stream());
}

#[test]
fn test_process_stream_chunks_by_output_buffer_size() {
    let mut filter = AsyncStreamingFilter::new();
    assert!(filter.create_buffer(4096, 2, 8));
    assert!(filter.set_output_buffer_size(188));
    let received = attach_recorder(&mut filter);
    assert!(filter.start_streaming());

    let data: Vec<u8> = (0..500u32).map(|i| (i % 256) as u8).collect();
    let mut s = SingleDataStream::new(TYPE_ID_TS_PACKET, &data);
    filter.receive_data(&mut s);

    // ドレインするまで pump
    let mut iterations = 0;
    while filter.process_stream() {
        iterations += 1;
        assert!(iterations < 100, "無限ループ防止");
    }

    let chunks = received.borrow().clone();
    // 各チャンクは output_buffer_size 以下
    for c in &chunks {
        assert!(c.len() <= 188);
    }
    // 連結すると元データに一致
    assert_eq!(concat(&chunks), data);
}

#[test]
fn test_multi_item_stream_all_buffered() {
    let mut filter = AsyncStreamingFilter::new();
    assert!(filter.create_buffer(2048, 2, 8));
    let received = attach_recorder(&mut filter);
    assert!(filter.start_streaming());

    let a = [1u8; 100];
    let b = [2u8; 100];
    let items: &[&[u8]] = &[&a, &b];
    let mut s = SliceDataStream::new(TYPE_ID_TS_PACKET, items);
    filter.receive_data(&mut s);

    assert!(filter.wait_for_end_of_stream());

    let mut expected = Vec::new();
    expected.extend_from_slice(&a);
    expected.extend_from_slice(&b);
    assert_eq!(concat(&received.borrow()), expected);
}

// ──────────────────────────────────────────────
// wait_for_end_of_stream
// ──────────────────────────────────────────────

#[test]
fn test_wait_for_end_of_stream_drains() {
    let mut filter = AsyncStreamingFilter::new();
    assert!(filter.create_buffer(4096, 2, 8));
    assert!(filter.set_output_buffer_size(188));
    let received = attach_recorder(&mut filter);
    assert!(filter.start_streaming());

    let data: Vec<u8> = (0..1000u32).map(|i| (i % 256) as u8).collect();
    let mut s = SingleDataStream::new(TYPE_ID_TS_PACKET, &data);
    filter.receive_data(&mut s);

    assert!(filter.wait_for_end_of_stream());
    assert_eq!(concat(&received.borrow()), data);
    assert!(!filter.process_stream(), "ドレイン済み");
}

#[test]
fn test_wait_for_end_of_stream_not_started() {
    let mut filter = AsyncStreamingFilter::new();
    // 未開始なら true(C++ も IsStarted() でない場合は即 true)
    assert!(filter.wait_for_end_of_stream());
}

// ──────────────────────────────────────────────
// 各種設定
// ──────────────────────────────────────────────

#[test]
fn test_set_output_buffer_size_validation() {
    let mut filter = AsyncStreamingFilter::new();
    assert!(!filter.set_output_buffer_size(187)); // < TS_PACKET_SIZE
    assert_eq!(filter.get_output_buffer_size(), 256 * 188); // 変わらない
    assert!(filter.set_output_buffer_size(188));
    assert_eq!(filter.get_output_buffer_size(), 188);
    assert!(filter.set_output_buffer_size(1024));
    assert_eq!(filter.get_output_buffer_size(), 1024);
}

#[test]
fn test_set_source_filter_rejected_when_started() {
    let mut filter = AsyncStreamingFilter::new();
    assert!(filter.start_streaming());

    let src: SourceFilterHandle =
        Rc::new(RefCell::new(MockSource::new(SourceMode::PULL, SourceMode::PULL)));
    assert!(!filter.set_source_filter(Some(src)), "開始中は設定不可");
}

#[test]
fn test_reset_clears_buffer_only_when_enabled() {
    let mut filter = AsyncStreamingFilter::new();
    assert!(filter.create_buffer(1024, 2, 8));
    assert!(filter.start_streaming());

    let data = [0x33u8; 100];
    let mut s = SingleDataStream::new(TYPE_ID_TS_PACKET, &data);
    filter.receive_data(&mut s);
    assert!(!filter.get_buffer().unwrap().is_empty());

    // 既定 clear_on_reset=true → reset でクリア
    filter.reset();
    assert!(filter.get_buffer().unwrap().is_empty());

    // clear_on_reset=false なら保持
    filter.set_clear_on_reset(false);
    let mut s2 = SingleDataStream::new(TYPE_ID_TS_PACKET, &data);
    filter.receive_data(&mut s2);
    filter.reset();
    assert!(!filter.get_buffer().unwrap().is_empty());
}

// ──────────────────────────────────────────────
// Pull ソース連動
// ──────────────────────────────────────────────

#[test]
fn test_source_uses_pull() {
    let mut filter = AsyncStreamingFilter::new();

    // Pull ソース
    let pull: SourceFilterHandle =
        Rc::new(RefCell::new(MockSource::new(SourceMode::PULL, SourceMode::PULL)));
    assert!(filter.set_source_filter(Some(pull)));
    assert!(filter.source_uses_pull());

    // Push ソースに差し替え
    let push: SourceFilterHandle =
        Rc::new(RefCell::new(MockSource::new(SourceMode::PUSH, SourceMode::PUSH)));
    assert!(filter.set_source_filter(Some(push)));
    assert!(!filter.source_uses_pull());

    // ソース無し
    assert!(filter.set_source_filter(None));
    assert!(!filter.source_uses_pull());
}

#[test]
fn test_pull_source_fetches_free_space() {
    let mut filter = AsyncStreamingFilter::new();
    assert!(filter.create_buffer(512, 2, 4));

    let src = Rc::new(RefCell::new(MockSource::new(SourceMode::PULL, SourceMode::PULL)));
    let handle: SourceFilterHandle = src.clone();
    assert!(filter.set_source_filter(Some(handle)));

    let free = filter.get_buffer().unwrap().get_free_space();
    assert!(free > 0);

    assert!(filter.pull_source());
    // fetch_source が空きサイズで 1 回呼ばれる
    assert_eq!(src.borrow().fetched, vec![free]);
}

#[test]
fn test_pull_source_without_buffer_is_false() {
    let mut filter = AsyncStreamingFilter::new();
    let src: SourceFilterHandle =
        Rc::new(RefCell::new(MockSource::new(SourceMode::PULL, SourceMode::PULL)));
    assert!(filter.set_source_filter(Some(src)));
    // バッファ無し → false
    assert!(!filter.pull_source());
}
