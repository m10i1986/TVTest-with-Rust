// Rust port of LibISDB/Filters/FilterBase.cpp + FilterBase.hpp
//                + LibISDB/Base/DataStream.hpp
//
// 設計上の相違:
//   - C++ の仮想関数クラス階層 → Rust の trait + struct composition
//   - C++ の MutexLock → フィルタ内部の処理はスレッド呼び出し側で同期する設計
//     (実際の移植クレートでは Mutex を個別に追加する)
//   - C++ の DataStream は DataBuffer の仮想ポインタを持つ
//     Rust では type_id (u32) + data (&[u8]) で型を識別

// ---------------------------------------------------------------------------
// TypeID 定数 (DataBuffer.hpp: GetTypeID 相当)
// ---------------------------------------------------------------------------

/// C++ の `DataBuffer::TypeID` 相当。基本的なバイトバッファ。
pub const TYPE_ID_DATA_BUFFER: u32 = 0x00000000;

/// C++ の `TSPacket::TypeID` 相当。188 バイト TS パケット。
pub const TYPE_ID_TS_PACKET: u32 = 0x00000001;

// ---------------------------------------------------------------------------
// DataStream trait (DataStream.hpp:39)
// ---------------------------------------------------------------------------

/// データのシーケンスを表すイテレータ的な trait。
///
/// C++ の `DataStream` 純粋仮想クラスに対応する。
/// 呼び出し側は `rewind()` → `data()` / `type_id()` → `next()` の順に使う。
pub trait DataStream {
    /// 現在の要素の TypeID を返す。
    fn type_id(&self) -> u32;

    /// 現在の要素のバイト列を返す。
    fn data(&self) -> &[u8];

    /// 次の要素に進む。次の要素があれば `true`。
    fn next(&mut self) -> bool;

    /// 先頭要素に巻き戻す。
    fn rewind(&mut self);

    /// 現在の要素が `TSPacket` (TypeID == TYPE_ID_TS_PACKET) かどうか。
    fn is_ts_packet(&self) -> bool {
        self.type_id() == TYPE_ID_TS_PACKET
    }
}

// ---------------------------------------------------------------------------
// SingleDataStream (DataStream.hpp:59 SingleDataStream<T>)
// ---------------------------------------------------------------------------

/// 単一要素の DataStream。
pub struct SingleDataStream<'a> {
    type_id: u32,
    data: &'a [u8],
    exhausted: bool,
}

impl<'a> SingleDataStream<'a> {
    pub fn new(type_id: u32, data: &'a [u8]) -> Self {
        Self { type_id, data, exhausted: false }
    }

    pub fn ts_packet(data: &'a [u8]) -> Self {
        Self::new(TYPE_ID_TS_PACKET, data)
    }
}

impl DataStream for SingleDataStream<'_> {
    fn type_id(&self) -> u32 { self.type_id }
    fn data(&self) -> &[u8] { self.data }
    fn next(&mut self) -> bool { false }
    fn rewind(&mut self) { self.exhausted = false; }
}

// ---------------------------------------------------------------------------
// SliceDataStream — スライスの全要素を順に返す
// ---------------------------------------------------------------------------

/// スライスの各要素を順に返す DataStream。
/// C++ の `BasicDataStream<T>` に対応する。
pub struct SliceDataStream<'a> {
    type_id: u32,
    items: &'a [&'a [u8]],
    pos: usize,
}

impl<'a> SliceDataStream<'a> {
    pub fn new(type_id: u32, items: &'a [&'a [u8]]) -> Self {
        Self { type_id, items, pos: 0 }
    }
}

impl DataStream for SliceDataStream<'_> {
    fn type_id(&self) -> u32 { self.type_id }
    fn data(&self) -> &[u8] {
        if self.pos < self.items.len() { self.items[self.pos] } else { &[] }
    }
    fn next(&mut self) -> bool {
        if self.pos + 1 < self.items.len() { self.pos += 1; true } else { false }
    }
    fn rewind(&mut self) { self.pos = 0; }
}

// ---------------------------------------------------------------------------
// VecDataStream — 所有するバイト列のシーケンス
// ---------------------------------------------------------------------------

/// 所有するバイトベクタのシーケンスを返す DataStream。
pub struct VecDataStream {
    type_id: u32,
    items: Vec<Vec<u8>>,
    pos: usize,
}

impl VecDataStream {
    pub fn new(type_id: u32, items: Vec<Vec<u8>>) -> Self {
        Self { type_id, items, pos: 0 }
    }

    pub fn single(type_id: u32, data: Vec<u8>) -> Self {
        Self::new(type_id, vec![data])
    }
}

impl DataStream for VecDataStream {
    fn type_id(&self) -> u32 { self.type_id }
    fn data(&self) -> &[u8] {
        if self.pos < self.items.len() { &self.items[self.pos] } else { &[] }
    }
    fn next(&mut self) -> bool {
        if self.pos + 1 < self.items.len() { self.pos += 1; true } else { false }
    }
    fn rewind(&mut self) { self.pos = 0; }
}

// ---------------------------------------------------------------------------
// FilterSink trait (FilterBase.hpp:41)
// ---------------------------------------------------------------------------

/// データ受け取りインターフェース。C++ の `FilterSink` 仮想クラスに対応する。
pub trait FilterSink: Send + Sync {
    fn receive_data(&mut self, stream: &mut dyn DataStream) -> bool;
}

// ---------------------------------------------------------------------------
// OutputSlot — 単一出力スロット (FilterBase.hpp:89 OutputFilterInfo 相当)
// ---------------------------------------------------------------------------

/// 単一の下流 FilterSink への接続スロット。
/// `SingleOutputFilter` の `m_OutputFilter` に対応する。
pub struct OutputSlot {
    sink: Option<Box<dyn FilterSink>>,
}

impl OutputSlot {
    pub fn new() -> Self { Self { sink: None } }

    /// 下流シンクを設定する。
    pub fn connect(&mut self, sink: Box<dyn FilterSink>) { self.sink = Some(sink); }

    /// 接続を解除する。
    pub fn disconnect(&mut self) { self.sink = None; }

    /// 接続済みかどうか。
    pub fn is_connected(&self) -> bool { self.sink.is_some() }

    /// 下流に data を送る。`rewind()` してから `receive_data` を呼ぶ。
    /// C++ の `FilterBase::OutputData(DataStream*)` 相当。
    pub fn send(&mut self, stream: &mut dyn DataStream) -> bool {
        stream.rewind();
        match &mut self.sink {
            Some(s) => s.receive_data(stream),
            None => false,
        }
    }
}

impl Default for OutputSlot {
    fn default() -> Self { Self::new() }
}

// ---------------------------------------------------------------------------
// MultiOutputSlots — N 個の出力スロット
// ---------------------------------------------------------------------------

/// N 個の出力スロット。`MultiOutputFilter<N>` に対応する。
pub struct MultiOutputSlots<const N: usize> {
    slots: [OutputSlot; N],
}

impl<const N: usize> MultiOutputSlots<N> {
    pub fn new() -> Self {
        // `[OutputSlot::new(); N]` は Copy のため使えない → 手動で初期化
        Self { slots: std::array::from_fn(|_| OutputSlot::new()) }
    }

    pub fn connect(&mut self, index: usize, sink: Box<dyn FilterSink>) -> bool {
        if index >= N { return false; }
        self.slots[index].connect(sink);
        true
    }

    pub fn disconnect(&mut self, index: usize) {
        if index < N { self.slots[index].disconnect(); }
    }

    pub fn disconnect_all(&mut self) {
        for s in &mut self.slots { s.disconnect(); }
    }

    pub fn send(&mut self, stream: &mut dyn DataStream, index: usize) -> bool {
        if index >= N { return false; }
        self.slots[index].send(stream)
    }

    pub fn is_connected(&self, index: usize) -> bool {
        if index >= N { false } else { self.slots[index].is_connected() }
    }
}

impl<const N: usize> Default for MultiOutputSlots<N> {
    fn default() -> Self { Self::new() }
}

// ---------------------------------------------------------------------------
// FilterBase trait (FilterBase.hpp:49)
// ---------------------------------------------------------------------------

/// フィルタ基底 trait。C++ の `FilterBase` 仮想クラスに対応する。
///
/// 各フィルタはこの trait を実装し、必要に応じて `FilterSink` も実装する。
pub trait FilterBase: Send + Sync {
    /// フィルタを初期化する。C++ の `Initialize()`。
    fn initialize(&mut self) -> bool { true }

    /// フィルタを終了する。C++ の `Finalize()`。
    fn finalize(&mut self) {}

    /// フィルタをリセットする。C++ の `Reset()`。
    fn reset(&mut self) {}

    /// ストリーミングを開始する。C++ の `StartStreaming()`。
    fn start_streaming(&mut self) -> bool { true }

    /// ストリーミングを停止する。C++ の `StopStreaming()`。
    fn stop_streaming(&mut self) -> bool { true }

    /// 入力ポート数。C++ の `GetInputCount()`。
    fn input_count(&self) -> usize { 0 }

    /// 出力ポート数。C++ の `GetOutputCount()`。
    fn output_count(&self) -> usize { 0 }
}

// ---------------------------------------------------------------------------
// 便利型: FilterFn — クロージャを FilterSink として使う
// ---------------------------------------------------------------------------

/// クロージャを `FilterSink` として使うためのラッパ。
/// テスト等でシンプルな受け口が欲しい場合に便利。
pub struct FilterFn<F: FnMut(&mut dyn DataStream) -> bool + Send + Sync>(pub F);

impl<F: FnMut(&mut dyn DataStream) -> bool + Send + Sync> FilterSink for FilterFn<F> {
    fn receive_data(&mut self, stream: &mut dyn DataStream) -> bool {
        (self.0)(stream)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ──────────────────────────────────────────────
    // DataStream テスト
    // ──────────────────────────────────────────────

    #[test]
    fn test_single_data_stream_basic() {
        let data = b"hello world";
        let mut stream = SingleDataStream::new(TYPE_ID_DATA_BUFFER, data);

        assert_eq!(stream.type_id(), TYPE_ID_DATA_BUFFER);
        assert_eq!(stream.data(), data.as_slice());
        assert!(!stream.next()); // 単一要素なので次はない

        stream.rewind();
        assert_eq!(stream.data(), data.as_slice()); // rewind 後も同じ
    }

    #[test]
    fn test_single_data_stream_ts_packet() {
        let pkt = vec![0x47u8; 188];
        let stream = SingleDataStream::ts_packet(&pkt);
        assert!(stream.is_ts_packet());
        assert_eq!(stream.data(), pkt.as_slice());
    }

    #[test]
    fn test_slice_data_stream() {
        let a = b"AAAA".as_slice();
        let b_data = b"BBBB".as_slice();
        let c = b"CCCC".as_slice();
        let items: &[&[u8]] = &[a, b_data, c];
        let mut stream = SliceDataStream::new(TYPE_ID_TS_PACKET, items);

        assert_eq!(stream.data(), a);
        assert!(stream.next());
        assert_eq!(stream.data(), b_data);
        assert!(stream.next());
        assert_eq!(stream.data(), c);
        assert!(!stream.next()); // これ以上なし

        stream.rewind();
        assert_eq!(stream.data(), a); // 先頭に戻る
    }

    #[test]
    fn test_vec_data_stream_single() {
        let mut stream = VecDataStream::single(TYPE_ID_TS_PACKET, b"test".to_vec());
        assert_eq!(stream.data(), b"test");
        assert!(!stream.next());
        stream.rewind();
        assert_eq!(stream.data(), b"test");
    }

    #[test]
    fn test_vec_data_stream_multi() {
        let mut stream = VecDataStream::new(
            TYPE_ID_DATA_BUFFER,
            vec![b"AAA".to_vec(), b"BBB".to_vec()],
        );
        assert_eq!(stream.data(), b"AAA");
        assert!(stream.next());
        assert_eq!(stream.data(), b"BBB");
        assert!(!stream.next());
        stream.rewind();
        assert_eq!(stream.data(), b"AAA");
    }

    // ──────────────────────────────────────────────
    // OutputSlot テスト
    // ──────────────────────────────────────────────

    #[test]
    fn test_output_slot_basic() {
        let mut slot = OutputSlot::new();
        assert!(!slot.is_connected());

        let received = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Vec<u8>>::new()));
        let received2 = received.clone();

        slot.connect(Box::new(FilterFn(move |stream: &mut dyn DataStream| {
            received2.lock().unwrap().push(stream.data().to_vec());
            while stream.next() {
                received2.lock().unwrap().push(stream.data().to_vec());
            }
            true
        })));
        assert!(slot.is_connected());

        let mut stream = SingleDataStream::new(TYPE_ID_TS_PACKET, &[1, 2, 3]);
        assert!(slot.send(&mut stream));

        let got = received.lock().unwrap().clone();
        assert_eq!(got, vec![vec![1, 2, 3]]);
    }

    #[test]
    fn test_output_slot_rewinds_before_send() {
        let mut slot = OutputSlot::new();
        let call_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let cc = call_count.clone();
        slot.connect(Box::new(FilterFn(move |_stream| {
            cc.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            true
        })));

        let mut stream = SingleDataStream::new(TYPE_ID_DATA_BUFFER, b"data");
        stream.next(); // 消費して exhausted 状態に

        // send は rewind を呼ぶので問題なく受け取れる
        slot.send(&mut stream);
        assert_eq!(call_count.load(std::sync::atomic::Ordering::Relaxed), 1);
    }

    #[test]
    fn test_output_slot_disconnect() {
        let mut slot = OutputSlot::new();
        slot.connect(Box::new(FilterFn(|_| true)));
        assert!(slot.is_connected());
        slot.disconnect();
        assert!(!slot.is_connected());

        let mut stream = SingleDataStream::new(TYPE_ID_DATA_BUFFER, &[]);
        assert!(!slot.send(&mut stream)); // 接続なし → false
    }

    // ──────────────────────────────────────────────
    // MultiOutputSlots テスト
    // ──────────────────────────────────────────────

    #[test]
    fn test_multi_output_slots_connect_send() {
        let mut slots: MultiOutputSlots<3> = MultiOutputSlots::new();

        let counts = [
            std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        ];

        for i in 0..3 {
            let c = counts[i].clone();
            slots.connect(i, Box::new(FilterFn(move |_| {
                c.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                true
            })));
        }

        let mut stream = SingleDataStream::new(TYPE_ID_DATA_BUFFER, b"x");

        slots.send(&mut stream, 0);
        slots.send(&mut stream, 1);
        slots.send(&mut stream, 2);
        assert!(!slots.send(&mut stream, 3)); // 範囲外

        for i in 0..3 {
            assert_eq!(counts[i].load(std::sync::atomic::Ordering::Relaxed), 1);
        }
    }

    #[test]
    fn test_multi_output_slots_disconnect_all() {
        let mut slots: MultiOutputSlots<2> = MultiOutputSlots::new();
        slots.connect(0, Box::new(FilterFn(|_| true)));
        slots.connect(1, Box::new(FilterFn(|_| true)));
        assert!(slots.is_connected(0));
        assert!(slots.is_connected(1));

        slots.disconnect_all();
        assert!(!slots.is_connected(0));
        assert!(!slots.is_connected(1));
    }

    // ──────────────────────────────────────────────
    // FilterBase + FilterSink 統合テスト
    // ──────────────────────────────────────────────

    // SingleIOFilter 的なフィルタのテスト用実装
    struct PassThroughFilter {
        output: OutputSlot,
        process_count: usize,
    }

    impl PassThroughFilter {
        fn new() -> Self { Self { output: OutputSlot::new(), process_count: 0 } }
    }

    impl FilterBase for PassThroughFilter {
        fn input_count(&self) -> usize { 1 }
        fn output_count(&self) -> usize { 1 }
    }

    impl FilterSink for PassThroughFilter {
        fn receive_data(&mut self, stream: &mut dyn DataStream) -> bool {
            self.process_count += 1;
            // ProcessData: 何もしない (passthrough)
            // OutputData: 下流に渡す
            self.output.send(stream)
        }
    }

    #[test]
    fn test_passthrough_filter_chain() {
        // 送り元 → PassThrough → 受け先
        let received = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Vec<u8>>::new()));
        let received2 = received.clone();

        let mut filter = PassThroughFilter::new();
        filter.output.connect(Box::new(FilterFn(move |stream: &mut dyn DataStream| {
            received2.lock().unwrap().push(stream.data().to_vec());
            true
        })));

        let mut s1 = SingleDataStream::new(TYPE_ID_TS_PACKET, &[0x47u8; 188]);
        filter.receive_data(&mut s1);

        let mut s2 = SingleDataStream::new(TYPE_ID_TS_PACKET, &[0x00u8; 188]);
        filter.receive_data(&mut s2);

        assert_eq!(filter.process_count, 2);
        let got = received.lock().unwrap().clone();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0], vec![0x47u8; 188]);
        assert_eq!(got[1], vec![0x00u8; 188]);
    }

    #[test]
    fn test_filter_no_downstream() {
        let mut filter = PassThroughFilter::new();
        // 下流なし → send は false だが process_count は増える
        let mut stream = SingleDataStream::new(TYPE_ID_DATA_BUFFER, b"test");
        let result = filter.receive_data(&mut stream);
        assert!(!result); // 下流なし
        assert_eq!(filter.process_count, 1);
    }

    #[test]
    fn test_filter_base_default_impl() {
        struct DummyFilter;
        impl FilterBase for DummyFilter {}

        let mut f = DummyFilter;
        assert!(f.initialize());
        f.reset();
        assert!(f.start_streaming());
        assert!(f.stop_streaming());
        assert_eq!(f.input_count(), 0);
        assert_eq!(f.output_count(), 0);
    }

    #[test]
    fn test_ts_packet_type_check() {
        let pkt = vec![0x47u8; 188];
        let stream = SingleDataStream::ts_packet(&pkt);
        assert!(stream.is_ts_packet());

        let raw = SingleDataStream::new(TYPE_ID_DATA_BUFFER, &[0u8; 10]);
        assert!(!raw.is_ts_packet());
    }

    #[test]
    fn test_slice_data_stream_rewind_in_slot() {
        // SliceDataStream を OutputSlot.send に渡すと rewind されることを確認
        let mut slot = OutputSlot::new();
        let call_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let cc = call_count.clone();
        slot.connect(Box::new(FilterFn(move |_| {
            cc.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            true
        })));

        let a = b"AAA".as_slice();
        let b_data = b"BBB".as_slice();
        let items: &[&[u8]] = &[a, b_data];
        let mut stream = SliceDataStream::new(TYPE_ID_TS_PACKET, items);

        // 1回目の send
        slot.send(&mut stream);
        // send 後も stream は rewind されている
        assert_eq!(stream.data(), a);
    }

    #[test]
    fn test_multi_slot_partial_connection() {
        let mut slots: MultiOutputSlots<4> = MultiOutputSlots::new();

        // slot 0 と 2 だけ接続
        slots.connect(0, Box::new(FilterFn(|_| true)));
        slots.connect(2, Box::new(FilterFn(|_| true)));

        assert!(slots.is_connected(0));
        assert!(!slots.is_connected(1));
        assert!(slots.is_connected(2));
        assert!(!slots.is_connected(3));
        assert!(!slots.is_connected(4)); // 範囲外
    }
}
