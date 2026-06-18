// Rust port of LibISDB/Filters/RecorderFilter.cpp + RecorderFilter.hpp
//
// RecorderFilter は入力 TS を 1 つ以上の録画タスク(RecordingTask)へ分配し、各タスクが
// StreamSelector でサービス/ストリーム種別を絞り込んで StreamWriter へ書き出す
// (SingleIOFilter。データは下流へそのままパススルーする)。
//
// 設計上の相違:
//   - スレッドモデル → ポンプモデル(続34 AsyncStreamingFilter / 続35 StreamSourceFilter と
//     同方針)。C++ の RecordingDataStreamer は StreamBufferDataStreamer(録画スレッド)を
//     継承するが、本移植では録画スレッド・保留バッファ(MaxPendingSize の入力バッファ)は
//     起こさず、DataStreamer の同期モード(入力バッファ無し+出力キャッシュ)で書き出す。
//     観測可能な録画ファイル内容(絞り込み後のバイト列)は C++ と一致する。MaxPendingSize は
//     受理・保持するが保留スレッドは生成しない。
//   - StreamSelector の PAT/PMT/CAT 解析は libisdb_stream_selector の方針どおり呼び出し側責務。
//     既定(ServiceID = SERVICE_ID_INVALID)は全パケット通過 = 全録画で PSI 情報は不要。
//     特定サービスを録画する場合は set_pmt_pid_list / set_emm_pid_list で PID 情報を与える。
//   - shared_ptr<RecordingTask> による所有 → RecorderFilter が所有し、`TaskId`(u64)で参照する。
//   - 書き込みエラー通知(OnWriteError)はポンプモデルでは書き込み呼び出し側(process_data)で
//     同期的に検出して通知する。多段(StreamerEventListener → TaskEventListener → EventListener)の
//     リスナチェーンは RecorderFilter のリスナへ集約する。
//   - MutexLock は省略(呼び出し側で同期)。

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use libisdb_data_streamer::{DataOutput, DataStreamer};
use libisdb_filter_base::{DataStream, FilterBase, FilterSink, OutputSlot};
use libisdb_stream_selector::{stream_flag, PacketAction, PmtPidInfo, StreamSelector, SERVICE_ID_INVALID};
use libisdb_stream_writer::{OpenFlag, StreamWriter};
use libisdb_ts_packet::TS_PACKET_SIZE;

/// 録画タスクの識別子。C++ の `shared_ptr<RecordingTask>` の代替。
pub type TaskId = u64;

/// 無効なサイズ。C++ `RecordingStatistics::INVALID_SIZE`(RecorderFilter.hpp:64)。
pub const INVALID_SIZE: u64 = u64::MAX;

/// 最小キャッシュサイズ。C++ `MinCacheSize`(RecorderFilter.cpp:104)。
const MIN_CACHE_SIZE: usize = 1024;

// ---------------------------------------------------------------------------
// RecordingOptions / RecordingStatistics (RecorderFilter.hpp:53 / :63)
// ---------------------------------------------------------------------------

/// 録画設定。C++ `RecorderFilter::RecordingOptions`。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordingOptions {
    pub service_id: u16,
    pub follow_active_service: bool,
    pub stream_flags: u32,
    pub write_cache_size: usize,
    pub max_pending_size: usize,
    pub clear_pending_buffer_on_service_changed: bool,
}

impl Default for RecordingOptions {
    fn default() -> Self {
        Self {
            service_id: SERVICE_ID_INVALID,
            follow_active_service: false,
            stream_flags: stream_flag::ALL,
            write_cache_size: 0,
            max_pending_size: 0,
            clear_pending_buffer_on_service_changed: true,
        }
    }
}

/// 録画統計情報。C++ `RecorderFilter::RecordingStatistics`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordingStatistics {
    pub input_bytes: u64,
    pub output_bytes: u64,
    pub output_count: u64,
    pub write_bytes: u64,
    pub write_error_count: u32,
}

impl Default for RecordingStatistics {
    fn default() -> Self {
        Self {
            input_bytes: 0,
            output_bytes: 0,
            output_count: 0,
            write_bytes: INVALID_SIZE,
            write_error_count: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// EventListener (RecorderFilter.hpp:103)
// ---------------------------------------------------------------------------

/// 録画フィルタのイベントリスナ。C++ `RecorderFilter::EventListener`。
pub trait RecorderEventListener {
    /// 書き込みエラー発生時。C++ `OnWriteError(RecorderFilter*, RecordingTask*)`。
    fn on_write_error(&mut self, _task_id: TaskId) {}
}

/// リスナのハンドル(共有・非 Send)。
pub type RecorderEventHandle = Rc<RefCell<dyn RecorderEventListener>>;

// ---------------------------------------------------------------------------
// StreamWriter を出力先とする DataOutput (RecordingDataStreamer::OutputData 相当)
// ---------------------------------------------------------------------------

struct WriterInner {
    writer: Option<Box<dyn StreamWriter + Send>>,
}

#[derive(Clone)]
struct WriterShared(Arc<Mutex<WriterInner>>);

impl DataOutput for WriterShared {
    // RecorderFilter.cpp:305
    fn output_data(&mut self, data: &[u8]) -> usize {
        let mut inner = self.0.lock().unwrap();
        match inner.writer.as_mut() {
            Some(w) => w.write(data),
            None => 0,
        }
    }

    // RecorderFilter.cpp:314
    fn is_output_valid(&self) -> bool {
        self.0.lock().unwrap().writer.is_some()
    }
}

// ---------------------------------------------------------------------------
// RecordingDataStreamer (RecorderFilter.hpp:139)
// ---------------------------------------------------------------------------

/// 録画用データストリーマ。C++ `RecorderFilter::RecordingDataStreamer`。
/// DataStreamer の出力先を StreamWriter にしたもの。
struct RecordingDataStreamer {
    streamer: DataStreamer,
    writer: WriterShared,
}

impl RecordingDataStreamer {
    // RecorderFilter.cpp:214
    fn new(writer: Option<Box<dyn StreamWriter + Send>>) -> Self {
        let shared = WriterShared(Arc::new(Mutex::new(WriterInner { writer })));
        let out = shared.clone();
        Self {
            streamer: DataStreamer::new(Box::new(out)),
            writer: shared,
        }
    }

    fn allocate_cache(&self, size: usize) -> bool {
        self.streamer.allocate_output_cache_buffer(size)
    }

    fn input_data(&self, data: &[u8]) -> bool {
        self.streamer.input_data(data)
    }

    // RecorderFilter.cpp:220
    fn set_writer(&self, writer: Option<Box<dyn StreamWriter + Send>>) -> bool {
        self.writer.0.lock().unwrap().writer = writer;
        true
    }

    // RecorderFilter.cpp:230
    fn reopen_writer(&self, file_name: &str, flags: OpenFlag) -> bool {
        let mut inner = self.writer.0.lock().unwrap();
        match inner.writer.as_mut() {
            None => false,
            Some(w) => {
                if !w.reopen(file_name, flags) {
                    if !w.is_open() {
                        inner.writer = None;
                    }
                    return false;
                }
                true
            }
        }
    }

    // RecorderFilter.cpp:256
    fn close_writer(&self) {
        let has = self.writer.0.lock().unwrap().writer.is_some();
        if !has {
            return;
        }
        {
            let mut inner = self.writer.0.lock().unwrap();
            if let Some(w) = inner.writer.as_mut() {
                w.set_preallocation_unit(0);
            }
        }
        // 残キャッシュを書き出してからクローズ。
        self.streamer.flush_buffer(Duration::from_secs(10));
        let mut inner = self.writer.0.lock().unwrap();
        if let Some(w) = inner.writer.as_mut() {
            w.close();
        }
        inner.writer = None;
    }

    // RecorderFilter.cpp:269
    fn get_file_name(&self) -> Option<String> {
        self.writer.0.lock().unwrap().writer.as_ref().and_then(|w| w.get_file_name())
    }

    // RecorderFilter.cpp:283
    fn get_recording_statistics(&self) -> RecordingStatistics {
        let stats = self.streamer.get_statistics();
        let write_bytes = {
            let inner = self.writer.0.lock().unwrap();
            match inner.writer.as_ref() {
                Some(w) if w.is_write_size_available() => w.get_write_size(),
                _ => INVALID_SIZE,
            }
        };
        RecordingStatistics {
            input_bytes: stats.input_bytes,
            output_bytes: stats.output_bytes,
            output_count: stats.output_count,
            write_bytes,
            write_error_count: stats.output_error_count,
        }
    }

    fn is_output_valid(&self) -> bool {
        self.writer.0.lock().unwrap().writer.is_some()
    }

    fn clear_buffer(&self) {
        self.streamer.clear_buffer();
    }
}

// ---------------------------------------------------------------------------
// RecordingTask (RecorderFilter.hpp:159 RecordingTaskImpl)
// ---------------------------------------------------------------------------

/// 録画タスク。C++ `RecorderFilter::RecordingTaskImpl`(`RecordingTask`)。
pub struct RecordingTask {
    options: RecordingOptions,
    paused: bool,
    selector: StreamSelector,
    data_streamer: RecordingDataStreamer,
    output_error_notified: bool,
}

impl RecordingTask {
    // RecorderFilter.cpp:322
    fn new(writer: Option<Box<dyn StreamWriter + Send>>, options: Option<RecordingOptions>) -> Self {
        let options = options.unwrap_or_default();
        let mut selector = StreamSelector::new();
        selector.set_target_flags(options.service_id, options.stream_flags);
        Self {
            options,
            paused: false,
            selector,
            data_streamer: RecordingDataStreamer::new(writer),
            output_error_notified: false,
        }
    }

    // RecorderFilter.cpp:510
    fn allocate_write_cache_buffer(&self, size: usize) -> bool {
        self.data_streamer.allocate_cache(size)
    }

    // RecorderFilter.cpp:367
    /// ポンプモデルでは録画スレッド・保留バッファを起こさないため、Start は常に成功する。
    fn start(&self) -> bool {
        true
    }

    // RecorderFilter.cpp:387
    fn stop(&mut self) {
        self.data_streamer.close_writer();
    }

    // RecorderFilter.cpp:399
    pub fn pause(&mut self) -> bool {
        self.paused = true;
        true
    }

    // RecorderFilter.cpp:413
    pub fn resume(&mut self) -> bool {
        self.paused = false;
        true
    }

    // RecorderFilter.hpp:181
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    // RecorderFilter.cpp:427
    pub fn clear_buffer(&mut self) {
        self.data_streamer.clear_buffer();
    }

    // RecorderFilter.cpp:435
    pub fn set_options(&mut self, options: &RecordingOptions) -> bool {
        if options.service_id != self.options.service_id
            || options.stream_flags != self.options.stream_flags
        {
            self.options.service_id = options.service_id;
            self.options.stream_flags = options.stream_flags;
            self.selector.set_target_flags(self.options.service_id, self.options.stream_flags);
        }

        self.options.follow_active_service = options.follow_active_service;
        self.options.max_pending_size = options.max_pending_size;
        self.options.clear_pending_buffer_on_service_changed =
            options.clear_pending_buffer_on_service_changed;

        true
    }

    // RecorderFilter.hpp:186
    pub fn get_options(&self) -> &RecordingOptions {
        &self.options
    }

    // RecorderFilter.cpp:344
    /// 出力先の StreamWriter を差し替える。ポンプモデルでは録画スレッドを起こさないため
    /// 常に成功する。
    pub fn set_writer(&mut self, writer: Option<Box<dyn StreamWriter + Send>>) -> bool {
        self.data_streamer.set_writer(writer);
        true
    }

    // RecorderFilter.cpp:358
    pub fn reopen(&mut self, file_name: &str, flags: OpenFlag) -> bool {
        let r = self.data_streamer.reopen_writer(file_name, flags);
        if r {
            self.output_error_notified = false;
        }
        r
    }

    // RecorderFilter.cpp:459
    pub fn get_file_name(&self) -> Option<String> {
        self.data_streamer.get_file_name()
    }

    // RecorderFilter.cpp:465
    pub fn get_statistics(&self) -> RecordingStatistics {
        self.data_streamer.get_recording_statistics()
    }

    /// 解析済みの PMT PID 情報を StreamSelector に設定する(PAT/PMT 解析結果)。
    /// 特定サービスの録画で絞り込みを効かせる場合に呼ぶ(呼び出し側責務)。
    pub fn set_pmt_pid_list(&mut self, list: Vec<PmtPidInfo>) {
        self.selector.set_pmt_pid_list(list);
    }

    /// 解析済みの EMM PID リストを StreamSelector に設定する(CAT 解析結果)。
    pub fn set_emm_pid_list(&mut self, list: Vec<u16>) {
        self.selector.set_emm_pid_list(list);
    }

    // RecorderFilter.cpp:471
    /// TS パケットを入力する。戻り値: 新たに書き込みエラーを検出して通知すべきなら `true`。
    fn input_packet(&mut self, packet: &[u8]) -> bool {
        if self.paused {
            return false;
        }
        if packet.len() < 3 {
            return false;
        }

        let pid = (((packet[1] & 0x1F) as u16) << 8) | packet[2] as u16;
        let write_ok = match self.selector.decide_packet(pid) {
            PacketAction::Drop => return false,
            PacketAction::Pass => self.data_streamer.input_data(packet),
            PacketAction::RewritePat => {
                if packet.len() == TS_PACKET_SIZE {
                    let mut arr = [0u8; TS_PACKET_SIZE];
                    arr.copy_from_slice(packet);
                    match self.selector.make_pat(&arr) {
                        Some(pat) => self.data_streamer.input_data(&pat),
                        // MakePAT 失敗時は原パケットをそのまま通す(C++ InputPacket と同じ)
                        None => self.data_streamer.input_data(packet),
                    }
                } else {
                    self.data_streamer.input_data(packet)
                }
            }
        };

        self.note_write_result(write_ok)
    }

    // RecorderFilter.cpp:484
    /// 非 TS データバッファを入力する。戻り値は input_packet と同じ。
    fn input_data(&mut self, data: &[u8]) -> bool {
        if self.paused {
            return false;
        }
        let ok = self.data_streamer.input_data(data);
        self.note_write_result(ok)
    }

    /// 書き込み結果を反映し、初回の書き込みエラーなら通知が必要(`true`)とする。
    /// C++ `m_OutputErrorNotified` 相当(一度通知したら再通知しない)。
    fn note_write_result(&mut self, ok: bool) -> bool {
        if !ok && !self.output_error_notified {
            self.output_error_notified = true;
            return true;
        }
        false
    }

    // RecorderFilter.cpp:494
    fn on_active_service_changed(&mut self, service_id: u16) {
        if self.options.follow_active_service {
            self.options.service_id = service_id;
            self.selector.set_target_flags(service_id, self.options.stream_flags);
        }

        if self.options.clear_pending_buffer_on_service_changed
            && !self.data_streamer.is_output_valid()
        {
            self.data_streamer.clear_buffer();
        }
    }
}

impl Drop for RecordingTask {
    // C++ ~RecordingTaskImpl は Stop() を呼ぶ(残キャッシュをフラッシュ)。
    fn drop(&mut self) {
        self.stop();
    }
}

// ---------------------------------------------------------------------------
// RecorderFilter (RecorderFilter.hpp:48)
// ---------------------------------------------------------------------------

struct TaskEntry {
    id: TaskId,
    task: RecordingTask,
}

/// 録画フィルタ。C++ `RecorderFilter`(`SingleIOFilter` 派生)。
pub struct RecorderFilter {
    tasks: Vec<TaskEntry>,
    next_task_id: TaskId,
    active_service_id: u16,
    listeners: Vec<RecorderEventHandle>,
    output: OutputSlot,
}

impl RecorderFilter {
    // RecorderFilter.cpp:38
    pub fn new() -> Self {
        Self {
            tasks: Vec::new(),
            next_task_id: 1,
            active_service_id: SERVICE_ID_INVALID,
            listeners: Vec::new(),
            output: OutputSlot::new(),
        }
    }

    /// 下流シンクへの出力スロット(パススルー先)。
    pub fn output(&mut self) -> &mut OutputSlot {
        &mut self.output
    }

    // RecorderFilter.cpp:96
    /// 録画タスクを生成する。成功時は `TaskId` を返す。
    pub fn create_task(
        &mut self,
        writer: Option<Box<dyn StreamWriter + Send>>,
        options: Option<RecordingOptions>,
    ) -> Option<TaskId> {
        let cache_size = match &options {
            Some(o) => o.write_cache_size.max(MIN_CACHE_SIZE),
            None => MIN_CACHE_SIZE,
        };

        let task = RecordingTask::new(writer, options);

        if !task.allocate_write_cache_buffer(cache_size) {
            return None;
        }
        if !task.start() {
            return None;
        }

        let id = self.next_task_id;
        self.next_task_id += 1;
        self.tasks.push(TaskEntry { id, task });

        Some(id)
    }

    // RecorderFilter.cpp:136
    pub fn delete_task(&mut self, id: TaskId) -> bool {
        match self.tasks.iter().position(|e| e.id == id) {
            Some(pos) => {
                self.tasks[pos].task.stop();
                self.tasks.remove(pos);
                true
            }
            None => false,
        }
    }

    // RecorderFilter.cpp:155
    pub fn delete_all_tasks(&mut self) {
        // Drop が各タスクの stop()(残キャッシュのフラッシュ)を行う。
        self.tasks.clear();
    }

    // RecorderFilter.cpp:163
    pub fn is_task_valid(&self, id: TaskId) -> bool {
        self.tasks.iter().any(|e| e.id == id)
    }

    // RecorderFilter.cpp:169
    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }

    // RecorderFilter.cpp:175
    pub fn get_task_id_by_index(&self, index: usize) -> Option<TaskId> {
        self.tasks.get(index).map(|e| e.id)
    }

    pub fn get_task(&self, id: TaskId) -> Option<&RecordingTask> {
        self.tasks.iter().find(|e| e.id == id).map(|e| &e.task)
    }

    pub fn get_task_mut(&mut self, id: TaskId) -> Option<&mut RecordingTask> {
        self.tasks.iter_mut().find(|e| e.id == id).map(|e| &mut e.task)
    }

    // RecorderFilter.cpp:200
    pub fn add_event_listener(&mut self, listener: RecorderEventHandle) -> bool {
        if self.listeners.iter().any(|l| Rc::ptr_eq(l, &listener)) {
            return false;
        }
        self.listeners.push(listener);
        true
    }

    // RecorderFilter.cpp:206
    pub fn remove_event_listener(&mut self, listener: &RecorderEventHandle) -> bool {
        let before = self.listeners.len();
        self.listeners.retain(|l| !Rc::ptr_eq(l, listener));
        self.listeners.len() != before
    }

    // RecorderFilter.cpp:56
    pub fn set_active_service_id(&mut self, service_id: u16) {
        self.active_service_id = service_id;
        for entry in &mut self.tasks {
            entry.task.on_active_service_changed(service_id);
        }
    }

    pub fn active_service_id(&self) -> u16 {
        self.active_service_id
    }

    // RecorderFilter.cpp:65
    /// 入力データを全タスクへ分配する。書き込みエラーを検出したタスクはリスナへ通知する。
    fn process_data(&mut self, stream: &mut dyn DataStream) {
        let is_ts = stream.is_ts_packet();
        let mut errored: Vec<TaskId> = Vec::new();

        loop {
            // タスクを &mut で回す間 stream を借り続けられないため要素を複製する。
            let data = stream.data().to_vec();
            for entry in &mut self.tasks {
                let new_err = if is_ts {
                    entry.task.input_packet(&data)
                } else {
                    entry.task.input_data(&data)
                };
                if new_err {
                    errored.push(entry.id);
                }
            }
            if !stream.next() {
                break;
            }
        }

        for id in errored {
            for l in &self.listeners {
                l.borrow_mut().on_write_error(id);
            }
        }
    }
}

impl Default for RecorderFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl FilterBase for RecorderFilter {
    fn input_count(&self) -> usize {
        1
    }

    fn output_count(&self) -> usize {
        1
    }

    // RecorderFilter.cpp:50
    fn finalize(&mut self) {
        self.delete_all_tasks();
    }
}

impl FilterSink for RecorderFilter {
    // SingleIOFilter: ProcessData してから下流へパススルーする。
    fn receive_data(&mut self, stream: &mut dyn DataStream) -> bool {
        self.process_data(stream);
        self.output.send(stream)
    }
}

#[cfg(test)]
mod tests;
