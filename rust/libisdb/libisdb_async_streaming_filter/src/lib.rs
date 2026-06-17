// Rust port of LibISDB/Filters/AsyncStreamingFilter.cpp + AsyncStreamingFilter.hpp
//
// AsyncStreamingFilter は SingleIOFilter + StreamingThread で、ReceiveData で
// 受け取ったデータを StreamBuffer に積み、別スレッド(StreamingThread::ProcessStream)
// が StreamBuffer から読み出して OutputData で下流へ流す「非同期ストリーミング」。
// SourceMode::Pull のソースに対しては PullSourceThread が FetchSource を駆動する。
//
// 設計上の相違(スレッドモデル → ポンプモデル):
//   - 本プロジェクトでは FilterSink/FilterBase を非 Send とし(下流フィルタは PSI
//     テーブル等の非 Send 状態を持つため)、フィルタを別スレッドへ move できない。
//     そこで C++ の StreamingThread/PullSourceThread の「ループ」は移植せず、その
//     1 反復に相当する `process_stream()` / `pull_source()` を公開し、スレッドの
//     スケジューリングは呼び出し側に委ねる(one_seg_pat_generator 等と同方針)。
//     入力バイト列→出力バイト列(output_buffer_size 単位で分割)という観測可能な
//     データ変換は C++ と一致する。
//   - MutexLock(m_FilterLock)は省略(呼び出し側で同期)。
//   - 出力は DataBuffer(TYPE_ID_DATA_BUFFER)として下流へ送る(C++ も m_OutputBuffer
//     は DataBuffer)。

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use libisdb_filter_base::{
    DataStream, FilterBase, FilterSink, OutputSlot, SingleDataStream, TYPE_ID_DATA_BUFFER,
};
use libisdb_source_filter::{SourceFilter, SourceMode};
use libisdb_stream_buffer::{Reader, SequentialReader, StreamBuffer};
use libisdb_ts_packet::TS_PACKET_SIZE;

/// ソースフィルタの共有ハンドル。
pub type SourceFilterHandle = Rc<RefCell<dyn SourceFilter>>;

/// 非同期ストリーミングフィルタ。C++ の `AsyncStreamingFilter` に対応する。
pub struct AsyncStreamingFilter {
    buffering_enabled: bool,
    clear_on_reset: bool,
    stream_buffer: Option<Arc<StreamBuffer>>,
    reader: SequentialReader,
    output_buffer: Vec<u8>,
    output_buffer_size: usize,
    source_filter: Option<SourceFilterHandle>,
    started: bool,
    output: OutputSlot,
}

impl AsyncStreamingFilter {
    // AsyncStreamingFilter.cpp:37
    pub fn new() -> Self {
        Self {
            buffering_enabled: true,
            clear_on_reset: true,
            stream_buffer: None,
            reader: SequentialReader::new(),
            output_buffer: Vec::new(),
            output_buffer_size: 256 * TS_PACKET_SIZE,
            source_filter: None,
            started: false,
            output: OutputSlot::new(),
        }
    }

    /// 下流シンクへの出力スロット(ProcessStream の OutputData 先)。
    pub fn output(&mut self) -> &mut OutputSlot {
        &mut self.output
    }

    pub fn is_started(&self) -> bool {
        self.started
    }

    // AsyncStreamingFilter.cpp:110
    pub fn create_buffer(
        &mut self,
        block_size: usize,
        min_block_count: usize,
        max_block_count: usize,
    ) -> bool {
        if block_size == 0 || max_block_count == 0 || min_block_count > max_block_count {
            return false;
        }

        let buffer = Arc::new(StreamBuffer::new());
        if !buffer.create(block_size, min_block_count, max_block_count, None) {
            return false;
        }

        self.set_buffer(buffer)
    }

    // AsyncStreamingFilter.cpp:125
    pub fn delete_buffer(&mut self) {
        self.reader.close();
        self.stream_buffer = None;
    }

    // AsyncStreamingFilter.cpp:134
    pub fn is_buffer_created(&self) -> bool {
        self.stream_buffer.is_some()
    }

    // AsyncStreamingFilter.cpp:140
    pub fn clear_buffer(&self) {
        if let Some(buf) = &self.stream_buffer {
            buf.clear();
        }
    }

    // AsyncStreamingFilter.cpp:149
    pub fn set_buffer(&mut self, buffer: Arc<StreamBuffer>) -> bool {
        let same = self
            .stream_buffer
            .as_ref()
            .is_some_and(|b| Arc::ptr_eq(b, &buffer));
        if !same {
            let was_open = self.reader.is_open();
            if was_open {
                self.reader.close();
            }
            self.stream_buffer = Some(Arc::clone(&buffer));
            if was_open {
                self.reader.open(buffer);
            }
        }
        true
    }

    // AsyncStreamingFilter.cpp:171
    pub fn get_buffer(&self) -> Option<Arc<StreamBuffer>> {
        self.stream_buffer.clone()
    }

    // AsyncStreamingFilter.cpp:177
    pub fn detach_buffer(&mut self) -> Option<Arc<StreamBuffer>> {
        self.reader.close();
        self.stream_buffer.take()
    }

    // AsyncStreamingFilter.cpp:187
    pub fn set_buffering_enabled(&mut self, enabled: bool) -> bool {
        self.buffering_enabled = enabled;
        true
    }

    pub fn get_buffering_enabled(&self) -> bool {
        self.buffering_enabled
    }

    // AsyncStreamingFilter.cpp:197
    pub fn set_clear_on_reset(&mut self, clear: bool) {
        self.clear_on_reset = clear;
    }

    pub fn get_clear_on_reset(&self) -> bool {
        self.clear_on_reset
    }

    // AsyncStreamingFilter.cpp:205
    pub fn set_output_buffer_size(&mut self, size: usize) -> bool {
        if size < TS_PACKET_SIZE {
            return false;
        }
        self.output_buffer_size = size;
        true
    }

    pub fn get_output_buffer_size(&self) -> usize {
        self.output_buffer_size
    }

    // AsyncStreamingFilter.cpp:218
    /// ソースフィルタを設定する。ストリーミング中は変更不可(`false`)。
    pub fn set_source_filter(&mut self, source: Option<SourceFilterHandle>) -> bool {
        if self.started {
            return false;
        }
        self.source_filter = source;
        true
    }

    /// 設定済みソースが Pull モードか(C++ StreamingLoop の PullSourceThread 起動条件)。
    /// AsyncStreamingFilter.cpp:266
    pub fn source_uses_pull(&self) -> bool {
        self.source_filter
            .as_ref()
            .is_some_and(|s| s.borrow().get_source_mode().intersects(SourceMode::PULL))
    }

    // FilterBase::StartStreaming (AsyncStreamingFilter.cpp:62)
    pub fn start_streaming(&mut self) -> bool {
        if let Some(buf) = &self.stream_buffer {
            self.reader.open(Arc::clone(buf));
        }

        if !self.started {
            self.output_buffer = vec![0u8; self.output_buffer_size];
            self.started = true;
        }

        true
    }

    // FilterBase::StopStreaming (AsyncStreamingFilter.cpp:83)
    pub fn stop_streaming(&mut self) -> bool {
        self.reader.close();
        self.output_buffer = Vec::new();
        self.started = false;
        true
    }

    // StreamingThread::ProcessStream (AsyncStreamingFilter.cpp:273)
    /// StreamBuffer から最大 output_buffer_size バイト読み出して下流へ送る。
    /// 読み出せた場合は `true`(C++ は OutputData の結果に関わらず読めたら true)。
    pub fn process_stream(&mut self) -> bool {
        if !self.reader.is_data_available() {
            return false;
        }

        self.output_buffer.resize(self.output_buffer_size, 0);
        let read = self.reader.read(&mut self.output_buffer);
        if read == 0 {
            return false;
        }
        self.output_buffer.truncate(read);

        // フィールドを分割借用して下流へ送る(self.output_buffer と self.output は別フィールド)
        let mut stream = SingleDataStream::new(TYPE_ID_DATA_BUFFER, &self.output_buffer);
        self.output.send(&mut stream);

        true
    }

    // PullSourceThread::ProcessStream (AsyncStreamingFilter.cpp:304)
    /// StreamBuffer の空き分だけソースから引き出す。空きが無ければ `false`。
    pub fn pull_source(&self) -> bool {
        let free_space = match &self.stream_buffer {
            Some(buf) => buf.get_free_space(),
            None => return false,
        };
        if free_space == 0 {
            return false;
        }
        if let Some(src) = &self.source_filter {
            src.borrow_mut().fetch_source(free_space);
        }
        true
    }

    // AsyncStreamingFilter.cpp:229
    /// StreamBuffer に残るデータを全て下流へ流し切る。
    ///
    /// C++ は背後スレッドが処理し終えるのを sleep して待つが、ポンプモデルでは
    /// `process_stream` を回してドレインする(到達状態=全データ出力済みは同じ)。
    pub fn wait_for_end_of_stream(&mut self) -> bool {
        if !self.started {
            return true;
        }
        while self.reader.is_data_available() {
            if !self.process_stream() {
                break;
            }
        }
        true
    }
}

impl Default for AsyncStreamingFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl FilterBase for AsyncStreamingFilter {
    fn input_count(&self) -> usize {
        1
    }
    fn output_count(&self) -> usize {
        1
    }

    // AsyncStreamingFilter.cpp:52
    fn reset(&mut self) {
        if self.clear_on_reset {
            if let Some(buf) = &self.stream_buffer {
                buf.clear();
            }
        }
    }

    fn start_streaming(&mut self) -> bool {
        AsyncStreamingFilter::start_streaming(self)
    }

    fn stop_streaming(&mut self) -> bool {
        AsyncStreamingFilter::stop_streaming(self)
    }
}

impl FilterSink for AsyncStreamingFilter {
    // AsyncStreamingFilter.cpp:96
    // SingleIOFilter::ReceiveData を上書きし、パススルーせず StreamBuffer に積むだけ
    // (出力は process_stream で行う)。
    fn receive_data(&mut self, stream: &mut dyn DataStream) -> bool {
        if self.buffering_enabled {
            if let Some(buf) = &self.stream_buffer {
                loop {
                    buf.push_back(stream.data());
                    if !stream.next() {
                        break;
                    }
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests;
