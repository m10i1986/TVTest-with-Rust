// Rust port of LibISDB/Filters/StreamSourceFilter.cpp + StreamSourceFilter.hpp
//
// StreamSourceFilter は Stream(ファイル等)から読み出して下流へ流すソースフィルタ
// (SourceFilter + Thread)。Pull モードでは FetchSource で要求分を読み出し、Push モード
// では読み込みスレッド(StreamingMain)が連続的に読み出して OutputData する。
//
// 設計上の相違(スレッドモデル → ポンプモデル):
//   - 続34 AsyncStreamingFilter と同方針。FilterSink は非 Send で下流を別スレッドへ
//     move できないため、Thread/StreamingMain の「ループ」とリクエストキュー
//     (RequestType/AddRequest/WaitAllRequests)は移植せず、Push 読み込みの 1 反復に
//     相当する `process_stream()` を公開し、スケジューリングは呼び出し側に委ねる。
//     入力 Stream → 出力バイト列という観測可能なデータ変換は C++ と一致する。
//   - 名前指定の OpenSource は Win32 ファイル I/O 依存のため対象外(false を返す)。
//     代わりに任意の Stream を渡す `open_source_stream`(C++ OpenSource(Stream*))を提供。
//   - MutexLock/ConditionVariable は省略(呼び出し側で同期)。

use libisdb_filter_base::{FilterBase, OutputSlot, SingleDataStream, TYPE_ID_DATA_BUFFER};
use libisdb_source_filter::{SourceFilter, SourceFilterState, SourceMode};
use libisdb_stream::Stream;
use libisdb_ts_packet::TS_PACKET_SIZE;

/// ストリームソースフィルタ。C++ の `StreamSourceFilter`(`SourceFilter` 派生)に対応する。
pub struct StreamSourceFilter {
    state: SourceFilterState,
    stream: Option<Box<dyn Stream>>,
    output_buffer: Vec<u8>,
    output_buffer_size: usize,
    input_bytes: u64,
    is_streaming: bool,
    output: OutputSlot,
}

impl StreamSourceFilter {
    // StreamSourceFilter.cpp:37
    pub fn new() -> Self {
        Self {
            state: SourceFilterState::new(SourceMode::PUSH),
            stream: None,
            output_buffer: Vec::new(),
            output_buffer_size: 256 * TS_PACKET_SIZE,
            input_bytes: 0,
            is_streaming: false,
            output: OutputSlot::new(),
        }
    }

    /// 下流シンクへの出力スロット(OutputData 先)。
    pub fn output(&mut self) -> &mut OutputSlot {
        &mut self.output
    }

    pub fn is_streaming(&self) -> bool {
        self.is_streaming
    }

    // StreamSourceFilter.cpp:169 (OpenSource(Stream*))
    /// 任意の `Stream` をソースとして開く。既にソースが開いていれば `false`。
    pub fn open_source_stream(&mut self, stream: Box<dyn Stream>) -> bool {
        if self.stream.is_some() {
            return false;
        }

        self.stream = Some(stream);
        self.is_streaming = false;

        // C++ は Push モードならここで読み込みスレッドを起動するが、ポンプモデルでは
        // process_stream を呼び出し側が駆動する。
        self.state.notify_source_opened();

        true
    }

    // StreamSourceFilter.cpp:262
    pub fn set_output_buffer_size(&mut self, size: usize) -> bool {
        if size < 1 {
            return false;
        }
        self.output_buffer_size = size;
        true
    }

    pub fn get_output_buffer_size(&self) -> usize {
        self.output_buffer_size
    }

    // StreamSourceFilter.hpp:73
    pub fn get_input_bytes(&self) -> u64 {
        self.input_bytes
    }

    // FilterBase::StartStreaming (StreamSourceFilter.cpp:74)
    pub fn start_streaming(&mut self) -> bool {
        if self.stream.is_none() {
            return false;
        }
        if self.is_streaming {
            return true;
        }

        self.output_buffer = vec![0u8; self.output_buffer_size];
        self.is_streaming = true;

        self.state.notify_streaming_start();

        true
    }

    // FilterBase::StopStreaming (StreamSourceFilter.cpp:110)
    pub fn stop_streaming(&mut self) -> bool {
        if self.is_streaming {
            self.is_streaming = false;
        }

        self.output_buffer = Vec::new();

        self.state.notify_streaming_stop();

        true
    }

    // FilterBase::ResetGraph (StreamSourceFilter.cpp:58)
    /// グラフをリセットする。
    ///
    /// C++ はスレッド稼働中なら Reset リクエストを投げて待つが、ポンプモデルでは即時に
    /// OnGraphReset を通知する。ResetDownstreamFilters(下流フィルタの Reset)は
    /// OutputSlot が FilterSink のみ保持するため対象外。
    pub fn reset_graph(&mut self) {
        self.state.notify_graph_reset();
    }

    // StreamingThread の 1 反復 (StreamSourceFilter.cpp:340, Push モード読み込み)
    /// Stream から output_buffer_size 分読み出し、ストリーミング中なら下流へ送る。
    /// 読み込んだバイトは input_bytes に積算する(C++ StreamingMain と同様、ストリーミング
    /// 中でなくても積算)。EOF 到達時は OnSourceEnd を通知する。読めたら `true`。
    pub fn process_stream(&mut self) -> bool {
        if self.stream.is_none() {
            return false;
        }

        self.output_buffer.resize(self.output_buffer_size, 0);
        let read = self.stream.as_mut().unwrap().read(&mut self.output_buffer);

        if read > 0 {
            self.input_bytes += read as u64;
            if self.is_streaming {
                let mut s = SingleDataStream::new(TYPE_ID_DATA_BUFFER, &self.output_buffer[..read]);
                self.output.send(&mut s);
            }
        }

        let at_end = self.stream.as_ref().unwrap().is_end();
        if read < self.output_buffer_size && at_end {
            self.state.notify_source_end();
        }

        read > 0
    }
}

impl Default for StreamSourceFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl FilterBase for StreamSourceFilter {
    fn output_count(&self) -> usize {
        1
    }

    // StreamSourceFilter.cpp:53 (Reset は何もしない)
    fn reset(&mut self) {}

    fn start_streaming(&mut self) -> bool {
        StreamSourceFilter::start_streaming(self)
    }

    fn stop_streaming(&mut self) -> bool {
        StreamSourceFilter::stop_streaming(self)
    }
}

impl SourceFilter for StreamSourceFilter {
    // 名前指定オープンは Win32 ファイル I/O 依存のため対象外。任意 Stream は
    // open_source_stream を使う。
    fn open_source(&mut self, _name: &str) -> bool {
        false
    }

    // StreamSourceFilter.cpp:201
    fn close_source(&mut self) -> bool {
        self.is_streaming = false;
        self.stream = None;
        self.state.notify_source_closed();
        true
    }

    // StreamSourceFilter.cpp:226
    fn is_source_open(&self) -> bool {
        self.stream.is_some()
    }

    // StreamSourceFilter.cpp:232
    fn fetch_source(&mut self, request_size: usize) -> bool {
        if !self.is_streaming
            || self.stream.is_none()
            || !self.state.source_mode().intersects(SourceMode::PULL)
        {
            return false;
        }

        let req = request_size.min(self.output_buffer_size);
        self.output_buffer.resize(self.output_buffer_size, 0);
        let read = self.stream.as_mut().unwrap().read(&mut self.output_buffer[..req]);

        if read > 0 {
            let mut s = SingleDataStream::new(TYPE_ID_DATA_BUFFER, &self.output_buffer[..read]);
            self.output.send(&mut s);
        }

        let at_end = self.stream.as_ref().unwrap().is_end();
        if read < req && at_end {
            self.state.notify_source_end();
        }

        read > 0
    }

    // StreamSourceFilter.hpp:66
    fn available_source_modes(&self) -> SourceMode {
        SourceMode::PUSH | SourceMode::PULL
    }

    fn source_state(&self) -> &SourceFilterState {
        &self.state
    }

    fn source_state_mut(&mut self) -> &mut SourceFilterState {
        &mut self.state
    }

    // StreamSourceFilter.cpp:253
    /// ソースが開いている間は変更不可。それ以外は基底の検証(SourceFilter::SetSourceMode)。
    fn set_source_mode(&mut self, mode: SourceMode) -> bool {
        if self.stream.is_some() {
            return false;
        }
        let available = self.available_source_modes();
        self.state.try_set_source_mode(mode, available)
    }
}

#[cfg(test)]
mod tests;
