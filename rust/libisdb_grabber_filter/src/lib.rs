// Rust port of LibISDB/Filters/GrabberFilter.cpp + GrabberFilter.hpp
//
// データ取り込みフィルタ。SingleIOFilter として、登録された Grabber 群に各データ
// バッファを渡して観測させる。Grabber が false を返したバッファは下流へ出力しない
// (フィルタリング)。それ以外のバッファはそのまま下流へ流す。
//
// 原実装との対応:
//   - C++ の SingleIOFilter 継承 → FilterBase + FilterSink を実装
//   - C++ の Grabber* リスト(呼び出し側が所有する生ポインタ)→
//     Rust では Rc<RefCell<dyn Grabber>> を共有保持(呼び出し側もクローンを持ち状態を読める)
//   - AddGrabber/RemoveGrabber の同一性判定(ポインタ比較)→ Rc::ptr_eq

use std::cell::RefCell;
use std::rc::Rc;

use libisdb_filter_base::{DataStream, FilterBase, FilterSink, OutputSlot, VecDataStream};

/// データ取り込みインターフェース。GrabberFilter.hpp:43。
pub trait Grabber {
    /// データバッファを受け取る。`false` を返すとそのバッファは下流へ出力されない。
    /// GrabberFilter.hpp:48。
    fn receive_data(&mut self, data: &[u8]) -> bool {
        let _ = data;
        true
    }

    /// リセット通知。GrabberFilter.hpp:49。
    fn on_reset(&mut self) {}
}

/// 共有 Grabber ハンドル。呼び出し側はこのクローンを保持して状態を読める。
pub type GrabberHandle = Rc<RefCell<dyn Grabber>>;

/// データ取り込みフィルタ。GrabberFilter.hpp:39。
pub struct GrabberFilter {
    grabbers: Vec<GrabberHandle>,
    output: OutputSlot,
}

impl Default for GrabberFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl GrabberFilter {
    pub fn new() -> Self {
        Self {
            grabbers: Vec::new(),
            output: OutputSlot::new(),
        }
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

    // ── Grabber 管理 (GrabberFilter.cpp:72-) ────────────────────

    /// Grabber を追加する。既に登録済み(同一 Rc)なら false。
    /// GrabberFilter::AddGrabber (cpp:72)
    pub fn add_grabber(&mut self, grabber: GrabberHandle) -> bool {
        if self.grabbers.iter().any(|g| Rc::ptr_eq(g, &grabber)) {
            return false;
        }
        self.grabbers.push(grabber);
        true
    }

    /// Grabber を削除する。未登録なら false。
    /// GrabberFilter::RemoveGrabber (cpp:89)
    pub fn remove_grabber(&mut self, grabber: &GrabberHandle) -> bool {
        if let Some(pos) = self.grabbers.iter().position(|g| Rc::ptr_eq(g, grabber)) {
            self.grabbers.remove(pos);
            true
        } else {
            false
        }
    }

    /// 登録されている Grabber の数。
    pub fn grabber_count(&self) -> usize {
        self.grabbers.len()
    }
}

// ---------------------------------------------------------------------------
// FilterBase 実装
// ---------------------------------------------------------------------------

impl FilterBase for GrabberFilter {
    fn input_count(&self) -> usize {
        1
    }
    fn output_count(&self) -> usize {
        1
    }

    /// GrabberFilter::Reset (cpp:37)
    fn reset(&mut self) {
        for g in &self.grabbers {
            g.borrow_mut().on_reset();
        }
    }
}

// ---------------------------------------------------------------------------
// FilterSink 実装(SingleIOFilter)
// ---------------------------------------------------------------------------

impl FilterSink for GrabberFilter {
    /// GrabberFilter::ReceiveData (cpp:46)
    fn receive_data(&mut self, stream: &mut dyn DataStream) -> bool {
        let type_id = stream.type_id();
        let mut output_seq: Vec<Vec<u8>> = Vec::new();

        loop {
            let buffer = stream.data();
            let mut filtered = false;

            for g in &self.grabbers {
                if !g.borrow_mut().receive_data(buffer) {
                    filtered = true;
                }
            }

            if !filtered {
                output_seq.push(buffer.to_vec());
            }

            if !stream.next() {
                break;
            }
        }

        if !output_seq.is_empty() {
            // OutputData(m_OutputSequence): 元の TypeID を保って下流へ
            let mut out_stream = VecDataStream::new(type_id, output_seq);
            self.output.send(&mut out_stream);
        }

        true
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use libisdb_filter_base::{SingleDataStream, TYPE_ID_TS_PACKET};
    use std::cell::Cell;

    /// データを記録し、`pass` の値を返す Grabber。
    struct RecordingGrabber {
        log: Rc<RefCell<Vec<Vec<u8>>>>,
        resets: Rc<Cell<u32>>,
        pass: bool,
    }
    impl Grabber for RecordingGrabber {
        fn receive_data(&mut self, data: &[u8]) -> bool {
            self.log.borrow_mut().push(data.to_vec());
            self.pass
        }
        fn on_reset(&mut self) {
            self.resets.set(self.resets.get() + 1);
        }
    }

    fn make_grabber(
        pass: bool,
    ) -> (Rc<RefCell<Vec<Vec<u8>>>>, Rc<Cell<u32>>, GrabberHandle) {
        let log = Rc::new(RefCell::new(Vec::new()));
        let resets = Rc::new(Cell::new(0u32));
        let handle: GrabberHandle = Rc::new(RefCell::new(RecordingGrabber {
            log: log.clone(),
            resets: resets.clone(),
            pass,
        }));
        (log, resets, handle)
    }

    /// 出力を記録するシンク(type_id も記録)。
    struct Recorder {
        items: Rc<RefCell<Vec<Vec<u8>>>>,
        type_id: Rc<Cell<u32>>,
    }
    impl FilterSink for Recorder {
        fn receive_data(&mut self, stream: &mut dyn DataStream) -> bool {
            self.type_id.set(stream.type_id());
            self.items.borrow_mut().push(stream.data().to_vec());
            while stream.next() {
                self.items.borrow_mut().push(stream.data().to_vec());
            }
            true
        }
    }
    fn recorder() -> (Rc<RefCell<Vec<Vec<u8>>>>, Rc<Cell<u32>>, Box<dyn FilterSink>) {
        let items = Rc::new(RefCell::new(Vec::new()));
        let type_id = Rc::new(Cell::new(0u32));
        let sink = Box::new(Recorder {
            items: items.clone(),
            type_id: type_id.clone(),
        });
        (items, type_id, sink)
    }

    fn feed_multi(filter: &mut GrabberFilter, buffers: &[&[u8]]) {
        let items: Vec<Vec<u8>> = buffers.iter().map(|b| b.to_vec()).collect();
        let mut stream = VecDataStream::new(TYPE_ID_TS_PACKET, items);
        filter.receive_data(&mut stream);
    }

    #[test]
    fn test_grabber_observes_and_passes() {
        let (log, _resets, handle) = make_grabber(true);
        let (out, _tid, sink) = recorder();

        let mut filter = GrabberFilter::new();
        filter.add_grabber(handle);
        filter.connect_output(sink);

        feed_multi(&mut filter, &[b"AAA", b"BBB"]);

        // grabber が両方観測
        assert_eq!(*log.borrow(), vec![b"AAA".to_vec(), b"BBB".to_vec()]);
        // 下流にも両方届く
        assert_eq!(*out.borrow(), vec![b"AAA".to_vec(), b"BBB".to_vec()]);
    }

    #[test]
    fn test_grabber_filters_out() {
        // pass=false の grabber → 全バッファが下流から除外される(が観測はされる)
        let (log, _resets, handle) = make_grabber(false);
        let (out, _tid, sink) = recorder();

        let mut filter = GrabberFilter::new();
        filter.add_grabber(handle);
        filter.connect_output(sink);

        feed_multi(&mut filter, &[b"X", b"Y"]);

        assert_eq!(*log.borrow(), vec![b"X".to_vec(), b"Y".to_vec()]); // 観測される
        assert!(out.borrow().is_empty()); // 出力はされない
    }

    #[test]
    fn test_multiple_grabbers_any_false_filters() {
        // grabber1: pass=true, grabber2: pass=false → filtered
        let (log1, _r1, h1) = make_grabber(true);
        let (log2, _r2, h2) = make_grabber(false);
        let (out, _tid, sink) = recorder();

        let mut filter = GrabberFilter::new();
        filter.add_grabber(h1);
        filter.add_grabber(h2);
        filter.connect_output(sink);

        feed_multi(&mut filter, &[b"Z"]);

        // 両方が観測する(grabber1 が false を返しても grabber2 も呼ばれる)
        assert_eq!(*log1.borrow(), vec![b"Z".to_vec()]);
        assert_eq!(*log2.borrow(), vec![b"Z".to_vec()]);
        // どれか1つでも false → 出力なし
        assert!(out.borrow().is_empty());
    }

    #[test]
    fn test_add_duplicate_grabber() {
        let (_log, _resets, handle) = make_grabber(true);
        let mut filter = GrabberFilter::new();

        assert!(filter.add_grabber(handle.clone()));
        assert!(!filter.add_grabber(handle.clone())); // 同一 Rc は拒否
        assert_eq!(filter.grabber_count(), 1);
    }

    #[test]
    fn test_remove_grabber() {
        let (log, _resets, handle) = make_grabber(true);
        let mut filter = GrabberFilter::new();
        filter.add_grabber(handle.clone());

        assert!(filter.remove_grabber(&handle));
        assert_eq!(filter.grabber_count(), 0);

        // 削除後はもう観測しない
        feed_multi(&mut filter, &[b"data"]);
        assert!(log.borrow().is_empty());
    }

    #[test]
    fn test_remove_nonexistent_grabber() {
        let (_log, _resets, handle) = make_grabber(true);
        let mut filter = GrabberFilter::new();
        // 未登録の削除は false
        assert!(!filter.remove_grabber(&handle));
    }

    #[test]
    fn test_reset_calls_on_reset() {
        let (_log, resets, handle) = make_grabber(true);
        let mut filter = GrabberFilter::new();
        filter.add_grabber(handle);

        assert_eq!(resets.get(), 0);
        filter.reset();
        assert_eq!(resets.get(), 1);
        filter.reset();
        assert_eq!(resets.get(), 2);
    }

    #[test]
    fn test_no_grabbers_passthrough() {
        let (out, tid, sink) = recorder();
        let mut filter = GrabberFilter::new();
        filter.connect_output(sink);

        feed_multi(&mut filter, &[b"AAA", b"BBB"]);

        // grabber 無し → 全バッファがそのまま下流へ
        assert_eq!(*out.borrow(), vec![b"AAA".to_vec(), b"BBB".to_vec()]);
        // 出力 TypeID は入力(TYPE_ID_TS_PACKET)を保つ
        assert_eq!(tid.get(), TYPE_ID_TS_PACKET);
    }

    #[test]
    fn test_partial_filtering_with_stateful_grabber() {
        // 状態を持つ grabber: 偶数番目だけ通す
        struct EvenGrabber {
            count: usize,
        }
        impl Grabber for EvenGrabber {
            fn receive_data(&mut self, _data: &[u8]) -> bool {
                let pass = self.count % 2 == 0;
                self.count += 1;
                pass
            }
        }
        let handle: GrabberHandle = Rc::new(RefCell::new(EvenGrabber { count: 0 }));
        let (out, _tid, sink) = recorder();

        let mut filter = GrabberFilter::new();
        filter.add_grabber(handle);
        filter.connect_output(sink);

        feed_multi(&mut filter, &[b"0", b"1", b"2", b"3"]);

        // 0,2 番目だけ通る
        assert_eq!(*out.borrow(), vec![b"0".to_vec(), b"2".to_vec()]);
    }

    #[test]
    fn test_filter_base_ports() {
        let filter = GrabberFilter::new();
        assert_eq!(filter.input_count(), 1);
        assert_eq!(filter.output_count(), 1);
    }

    #[test]
    fn test_single_data_stream_input() {
        // SingleDataStream(1要素)でも動作
        let (log, _resets, handle) = make_grabber(true);
        let (out, _tid, sink) = recorder();
        let mut filter = GrabberFilter::new();
        filter.add_grabber(handle);
        filter.connect_output(sink);

        let mut stream = SingleDataStream::new(TYPE_ID_TS_PACKET, b"single");
        filter.receive_data(&mut stream);

        assert_eq!(*log.borrow(), vec![b"single".to_vec()]);
        assert_eq!(*out.borrow(), vec![b"single".to_vec()]);
    }
}
