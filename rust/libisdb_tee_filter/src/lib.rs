// Rust port of LibISDB/Filters/TeeFilter.cpp + TeeFilter.hpp
//
// 2分配フィルタ。入力データを2つの出力スロットへそのまま分配する。
//
// 原実装との対応:
//   - C++ の MultiOutputFilter<2> 継承 → libisdb_filter_base の MultiOutputSlots<2> を保持
//   - C++ の ReceiveData: OutputData(pData, 0); OutputData(pData, 1);
//     → 各 send は内部で rewind してから下流へ渡す(両出力が先頭から全要素を受け取る)

use libisdb_filter_base::{DataStream, FilterBase, FilterSink, MultiOutputSlots};

/// 出力スロット数。TeeFilter.hpp:39 `MultiOutputFilter<2>`。
const OUTPUT_COUNT: usize = 2;

/// 2分配フィルタ。TeeFilter.hpp:38。
pub struct TeeFilter {
    outputs: MultiOutputSlots<OUTPUT_COUNT>,
}

impl Default for TeeFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl TeeFilter {
    pub fn new() -> Self {
        Self {
            outputs: MultiOutputSlots::new(),
        }
    }

    /// 指定インデックスの下流フィルタを接続する。
    pub fn connect_output(&mut self, index: usize, sink: Box<dyn FilterSink>) -> bool {
        self.outputs.connect(index, sink)
    }

    /// 指定インデックスの下流フィルタを切断する。
    pub fn disconnect_output(&mut self, index: usize) {
        self.outputs.disconnect(index);
    }

    /// 全ての下流フィルタを切断する。
    pub fn disconnect_all_outputs(&mut self) {
        self.outputs.disconnect_all();
    }

    /// 指定インデックスの下流が接続されているか。
    pub fn is_output_connected(&self, index: usize) -> bool {
        self.outputs.is_connected(index)
    }
}

impl FilterBase for TeeFilter {
    fn input_count(&self) -> usize {
        1
    }
    fn output_count(&self) -> usize {
        OUTPUT_COUNT
    }
}

impl FilterSink for TeeFilter {
    /// TeeFilter::ReceiveData (TeeFilter.cpp:36)
    fn receive_data(&mut self, stream: &mut dyn DataStream) -> bool {
        // OutputData(pData, 0); OutputData(pData, 1);
        // MultiOutputSlots::send は内部で rewind してから下流へ渡すため、
        // スロット0が stream を消費した後でもスロット1は先頭から受け取れる。
        self.outputs.send(stream, 0);
        self.outputs.send(stream, 1);
        true
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use libisdb_filter_base::{SingleDataStream, VecDataStream, TYPE_ID_TS_PACKET};
    use std::cell::RefCell;
    use std::rc::Rc;

    /// 受信した各要素を記録するシンク。stream を最後まで消費する。
    struct Recorder {
        items: Rc<RefCell<Vec<Vec<u8>>>>,
    }
    impl FilterSink for Recorder {
        fn receive_data(&mut self, stream: &mut dyn DataStream) -> bool {
            self.items.borrow_mut().push(stream.data().to_vec());
            while stream.next() {
                self.items.borrow_mut().push(stream.data().to_vec());
            }
            true
        }
    }

    fn recorder() -> (Rc<RefCell<Vec<Vec<u8>>>>, Box<dyn FilterSink>) {
        let store = Rc::new(RefCell::new(Vec::new()));
        (store.clone(), Box::new(Recorder { items: store }))
    }

    #[test]
    fn test_output_count() {
        let f = TeeFilter::new();
        assert_eq!(f.input_count(), 1);
        assert_eq!(f.output_count(), 2);
    }

    #[test]
    fn test_both_outputs_receive_same_data() {
        let (s0, sink0) = recorder();
        let (s1, sink1) = recorder();

        let mut tee = TeeFilter::new();
        assert!(tee.connect_output(0, sink0));
        assert!(tee.connect_output(1, sink1));

        let mut stream = SingleDataStream::new(TYPE_ID_TS_PACKET, &[1, 2, 3]);
        assert!(tee.receive_data(&mut stream));

        assert_eq!(*s0.borrow(), vec![vec![1, 2, 3]]);
        assert_eq!(*s1.borrow(), vec![vec![1, 2, 3]]);
    }

    #[test]
    fn test_second_output_gets_rewound_stream() {
        // マルチ要素ストリーム: スロット0が全消費した後でもスロット1は先頭から受け取る
        let (s0, sink0) = recorder();
        let (s1, sink1) = recorder();

        let mut tee = TeeFilter::new();
        tee.connect_output(0, sink0);
        tee.connect_output(1, sink1);

        let items: Vec<Vec<u8>> = vec![b"AAA".to_vec(), b"BBB".to_vec(), b"CCC".to_vec()];
        let mut stream = VecDataStream::new(TYPE_ID_TS_PACKET, items);
        tee.receive_data(&mut stream);

        let expected = vec![b"AAA".to_vec(), b"BBB".to_vec(), b"CCC".to_vec()];
        assert_eq!(*s0.borrow(), expected);
        assert_eq!(*s1.borrow(), expected); // rewind されているので同じ
    }

    #[test]
    fn test_only_one_output_connected() {
        let (s0, sink0) = recorder();

        let mut tee = TeeFilter::new();
        tee.connect_output(0, sink0);
        // スロット1は未接続

        assert!(tee.is_output_connected(0));
        assert!(!tee.is_output_connected(1));

        let mut stream = SingleDataStream::new(TYPE_ID_TS_PACKET, &[9]);
        assert!(tee.receive_data(&mut stream)); // 未接続スロットでも問題なし

        assert_eq!(*s0.borrow(), vec![vec![9]]);
    }

    #[test]
    fn test_no_outputs_connected() {
        let mut tee = TeeFilter::new();
        let mut stream = SingleDataStream::new(TYPE_ID_TS_PACKET, &[0]);
        // 出力未接続でも true を返し panic しない
        assert!(tee.receive_data(&mut stream));
    }

    #[test]
    fn test_disconnect_outputs() {
        let (_s0, sink0) = recorder();
        let (_s1, sink1) = recorder();

        let mut tee = TeeFilter::new();
        tee.connect_output(0, sink0);
        tee.connect_output(1, sink1);
        assert!(tee.is_output_connected(0));
        assert!(tee.is_output_connected(1));

        tee.disconnect_output(0);
        assert!(!tee.is_output_connected(0));
        assert!(tee.is_output_connected(1));

        tee.disconnect_all_outputs();
        assert!(!tee.is_output_connected(0));
        assert!(!tee.is_output_connected(1));
    }

    #[test]
    fn test_connect_out_of_range() {
        let (_s, sink) = recorder();
        let mut tee = TeeFilter::new();
        // インデックス2は範囲外(0,1のみ)
        assert!(!tee.connect_output(2, sink));
    }

    #[test]
    fn test_chained_tee() {
        // Tee → (Tee, Recorder) のように下流に別の Tee を繋ぐ
        let (leaf_a, sink_a) = recorder();
        let (leaf_b, sink_b) = recorder();
        let (leaf_c, sink_c) = recorder();

        let mut inner = TeeFilter::new();
        inner.connect_output(0, sink_a);
        inner.connect_output(1, sink_b);

        let mut outer = TeeFilter::new();
        outer.connect_output(0, Box::new(inner)); // inner Tee を下流に
        outer.connect_output(1, sink_c);

        let mut stream = SingleDataStream::new(TYPE_ID_TS_PACKET, &[7, 7]);
        outer.receive_data(&mut stream);

        // outer → inner → a, b  および outer → c の全てに届く
        assert_eq!(*leaf_a.borrow(), vec![vec![7, 7]]);
        assert_eq!(*leaf_b.borrow(), vec![vec![7, 7]]);
        assert_eq!(*leaf_c.borrow(), vec![vec![7, 7]]);
    }
}
