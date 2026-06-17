// Rust port of LibISDB/Filters/StreamBufferFilter.cpp + StreamBufferFilter.hpp
//
// StreamBufferFilter は SingleIOFilter(入力をパススルーしつつ副作用としてバッファに
// 蓄積する)で、内部に StreamBufferDataStreamer を持つ。
//
// 通常運用(出力 StreamBuffer を CreateMemoryBuffer で設定し、入力(ペンディング)
// バッファ・出力キャッシュは未設定)では、ProcessData→DataStreamer::InputData が
// 入力バッファを持たないため同期的に出力 StreamBuffer へ直接 push される
// (AllocateOutputCacheBuffer を呼ぶのは RecorderFilter のみ)。
//
// 設計上の相違:
//   - C++ の仮想関数階層 → FilterBase + FilterSink 実装(他フィルタ移植と同方針)
//   - SingleIOFilter::ReceiveData(FilterBase.cpp:219)= ProcessData→OutputData の
//     パススルーを receive_data で再現(OutputSlot::send が rewind してから下流へ)
//   - MutexLock(m_FilterLock)はスレッド同期を data_streamer 側へ委譲し省略
//     (他フィルタ移植と同方針)

use std::sync::Arc;
use std::time::Duration;

use libisdb_data_streamer::StreamBufferDataStreamer;
use libisdb_filter_base::{DataStream, FilterBase, FilterSink, OutputSlot};
use libisdb_stream_buffer::StreamBuffer;

/// ストリームバッファフィルタ。C++ の `StreamBufferFilter`(`SingleIOFilter` 派生)に対応する。
pub struct StreamBufferFilter {
    data_streamer: StreamBufferDataStreamer,
    buffering_enabled: bool,
    clear_on_reset: bool,
    output: OutputSlot,
}

impl StreamBufferFilter {
    // StreamBufferFilter.cpp:36
    pub fn new() -> Self {
        Self {
            data_streamer: StreamBufferDataStreamer::new(),
            buffering_enabled: false,
            clear_on_reset: true,
            output: OutputSlot::new(),
        }
    }

    /// 下流シンクへの出力スロット(パススルー先)。
    pub fn output(&mut self) -> &mut OutputSlot {
        &mut self.output
    }

    // StreamBufferFilter.cpp:65
    /// メモリ上に出力バッファを作成して設定する。
    pub fn create_memory_buffer(
        &self,
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

        self.data_streamer.set_output_buffer(buffer)
    }

    // StreamBufferFilter.cpp:85
    pub fn delete_buffer(&self) {
        self.data_streamer.free_output_buffer();
    }

    // StreamBufferFilter.cpp:93
    pub fn is_buffer_created(&self) -> bool {
        self.data_streamer.has_output_buffer()
    }

    // StreamBufferFilter.cpp:99
    pub fn clear_buffer(&self) {
        self.data_streamer.clear_output_buffer();
    }

    // StreamBufferFilter.cpp:105
    pub fn set_buffer(&self, buffer: Arc<StreamBuffer>) -> bool {
        self.data_streamer.set_output_buffer(buffer)
    }

    // StreamBufferFilter.cpp:111
    pub fn get_buffer(&self) -> Option<Arc<StreamBuffer>> {
        self.data_streamer.get_output_buffer()
    }

    // StreamBufferFilter.cpp:117
    pub fn detach_buffer(&self) -> Option<Arc<StreamBuffer>> {
        self.data_streamer.detach_output_buffer()
    }

    // StreamBufferFilter.cpp:125
    /// ペンディング(入力)バッファのサイズを設定する。既存ならリサイズ、無ければ作成。
    pub fn set_pending_buffer_size(&self, block_size: usize, max_block_count: usize) -> bool {
        if self.data_streamer.streamer.has_input_buffer() {
            match self.data_streamer.streamer.get_input_buffer() {
                Some(buf) => buf.set_size(block_size, 0, max_block_count, true),
                None => false,
            }
        } else {
            self.data_streamer
                .streamer
                .create_input_buffer(block_size, 0, max_block_count)
        }
    }

    // StreamBufferFilter.cpp:141
    /// バッファリングの有効/無効を切り替える。
    ///
    /// 有効化: `Initialize`→`Start`(入力バッファが無い場合はスレッドを起動せず同期動作)。
    /// 無効化: `Stop`→`FlushBuffer`(最大 10 秒)→`Close`。
    pub fn set_buffering_enabled(&mut self, enabled: bool) -> bool {
        if self.buffering_enabled != enabled {
            if enabled {
                self.data_streamer.streamer.initialize();
                if !self.data_streamer.streamer.start() {
                    return false;
                }
            } else {
                self.data_streamer.streamer.stop();
                self.data_streamer
                    .streamer
                    .flush_buffer(Duration::from_secs(10));
                self.data_streamer.streamer.close();
            }

            self.buffering_enabled = enabled;
        }

        true
    }

    pub fn get_buffering_enabled(&self) -> bool {
        self.buffering_enabled
    }

    // StreamBufferFilter.cpp:163
    pub fn set_clear_on_reset(&mut self, clear: bool) {
        self.clear_on_reset = clear;
    }

    pub fn get_clear_on_reset(&self) -> bool {
        self.clear_on_reset
    }

    // StreamBufferFilter.cpp:53
    fn process_data(&mut self, stream: &mut dyn DataStream) -> bool {
        if self.buffering_enabled {
            loop {
                self.data_streamer.streamer.input_data(stream.data());
                if !stream.next() {
                    break;
                }
            }
        }
        true
    }
}

impl Default for StreamBufferFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl FilterBase for StreamBufferFilter {
    fn input_count(&self) -> usize {
        1
    }
    fn output_count(&self) -> usize {
        1
    }

    // StreamBufferFilter.cpp:43
    fn reset(&mut self) {
        if self.clear_on_reset {
            self.data_streamer.streamer.clear_buffer();
        }
    }
}

impl FilterSink for StreamBufferFilter {
    // SingleIOFilter::ReceiveData (FilterBase.cpp:219): ProcessData -> OutputData
    fn receive_data(&mut self, stream: &mut dyn DataStream) -> bool {
        self.process_data(stream);
        self.output.send(stream);
        true
    }
}

#[cfg(test)]
mod tests;
