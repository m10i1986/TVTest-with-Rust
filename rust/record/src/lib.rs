//! TVTest の録画(src/Record.cpp / Record.h)のモデル層移植。
//!
//! 録画の純粋なロジックを、録画エンジンやファイル入出力から切り離して表現する:
//! - [`RecordingSettings`](保存するストリームの種別フラグ等の設定)
//! - [`RecordTime`](録画/予約の基準時刻 = 日時 + ティックカウント)
//! - [`TimeSpec`](開始/停止の時刻指定 = 未指定 / 日時 / 経過時間)
//! - [`RecordManager`](予約/録画の状態と、開始/停止の時刻判定)
//!
//! 原実装の時刻判定は `::GetSystemTime()` / `Util::GetTickCount()` で「現在」を取得するが、
//! 本移植ではそれらを引数として受け取り、純粋関数として扱う。
//!
//! # 対象外(Win32 / エンジン / I-O 依存)
//! 録画エンジン(`LibISDB::RecorderFilter`/`TSEngine`/`CRecordTask` の実体)、ファイル名
//! 生成(`GenerateFilePath` = `CVariableStringMap`)、ファイル入出力、設定ダイアログ、
//! 書き込みプラグイン列挙、`SetCurrentTime` の OS 時刻取得。

use libisdb_stream_selector::stream_flag;
use tvtest_util::{compare_system_time, diff_system_time, offset_system_time, SystemTime};

/// 録画設定(Record.h 34-58 `CRecordingSettings`)。
///
/// `save_stream` は [`libisdb_stream_selector::stream_flag`] のビットマスク。
/// 字幕/データ放送の保存可否はそのビットで判定する。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordingSettings {
    pub cur_service_only: bool,
    /// 保存するストリームの種別(`StreamSelector::StreamFlag` ビットマスク)。
    pub save_stream: u32,
    pub write_plugin: String,
    pub write_cache_size: u32,
    pub max_pending_size: u32,
    pub pre_allocation_unit: u64,
    pub time_shift_buffer_size: u32,
    pub enable_time_shift: bool,
}

impl RecordingSettings {
    /// 書き込みキャッシュサイズの既定値(Record.h 37)。
    pub const WRITE_CACHE_SIZE_DEFAULT: u32 = 0x0010_0000;
    /// 最大保留サイズの既定値(Record.h 38)。
    pub const MAX_PENDING_SIZE_DEFAULT: u32 = 0x1000_0000;
    /// タイムシフトバッファサイズの既定値(Record.h 39)。
    pub const TIMESHIFT_BUFFER_SIZE_DEFAULT: u32 = 32 * 0x0010_0000;

    /// 字幕を保存するか(Record.cpp 36-39 `IsSaveCaption`)。
    pub fn is_save_caption(&self) -> bool {
        self.test_save_stream_flag(stream_flag::CAPTION)
    }

    /// 字幕の保存可否を設定する(Record.cpp 42-45 `SetSaveCaption`)。
    pub fn set_save_caption(&mut self, save: bool) {
        self.set_save_stream_flag(stream_flag::CAPTION, save);
    }

    /// データ放送(データカルーセル)を保存するか(Record.cpp 48-51 `IsSaveDataCarrousel`)。
    pub fn is_save_data_carrousel(&self) -> bool {
        self.test_save_stream_flag(stream_flag::DATA_CARROUSEL)
    }

    /// データ放送の保存可否を設定する(Record.cpp 54-57 `SetSaveDataCarrousel`)。
    pub fn set_save_data_carrousel(&mut self, save: bool) {
        self.set_save_stream_flag(stream_flag::DATA_CARROUSEL, save);
    }

    /// 指定フラグがすべて立っているか(Record.cpp 60-63 `TestSaveStreamFlag`)。
    pub fn test_save_stream_flag(&self, flag: u32) -> bool {
        (self.save_stream & flag) == flag
    }

    /// 指定フラグを設定/解除する(Record.cpp 66-72 `SetSaveStreamFlag`)。
    pub fn set_save_stream_flag(&mut self, flag: u32, set: bool) {
        if set {
            self.save_stream |= flag;
        } else {
            self.save_stream &= !flag;
        }
    }
}

impl Default for RecordingSettings {
    fn default() -> Self {
        Self {
            cur_service_only: false,
            save_stream: stream_flag::ALL,
            write_plugin: String::new(),
            write_cache_size: Self::WRITE_CACHE_SIZE_DEFAULT,
            max_pending_size: Self::MAX_PENDING_SIZE_DEFAULT,
            pre_allocation_unit: 0,
            time_shift_buffer_size: Self::TIMESHIFT_BUFFER_SIZE_DEFAULT,
            enable_time_shift: false,
        }
    }
}

/// 録画/予約の基準時刻(Record.h 60-73 `CRecordTime`)。
///
/// 日時(`SYSTEMTIME`)とティックカウント(`GetTickCount64`)の対を保持する。原実装の
/// `SetCurrentTime` は OS 時刻を取得するが、本移植では値を引数で受け取る。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecordTime {
    time: SystemTime,
    tick_time: u64,
}

impl RecordTime {
    /// クリア済みの時刻を生成する。
    pub fn new() -> Self {
        Self::default()
    }

    /// 現在時刻を設定する(Record.cpp 83-88 `SetCurrentTime`。OS 取得値を注入)。
    pub fn set_current_time(&mut self, time: SystemTime, tick_time: u64) {
        self.time = time;
        self.tick_time = tick_time;
    }

    /// 日時を返す(Record.cpp 91-97 `GetTime`。無効なら `None`)。
    pub fn get_time(&self) -> Option<SystemTime> {
        if !self.is_valid() {
            return None;
        }
        Some(self.time)
    }

    /// ティックカウントを返す(`GetTickTime`)。
    pub fn tick_time(&self) -> u64 {
        self.tick_time
    }

    /// 時刻をクリアする(Record.cpp 100-104 `Clear`)。
    pub fn clear(&mut self) {
        self.time = SystemTime::default();
        self.tick_time = 0;
    }

    /// 有効な時刻か(Record.cpp 107-110 `IsValid`。`year != 0`)。
    pub fn is_valid(&self) -> bool {
        self.time.year != 0
    }
}

/// 開始/停止の時刻指定(Record.h 132-145 `TimeSpecType`/`TimeSpecInfo`)。
///
/// C++ の `union`(日時 or 経過時間)を Rust の列挙体で表現する。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TimeSpec {
    /// 未指定。
    #[default]
    NotSpecified,
    /// 絶対日時で指定。
    DateTime(SystemTime),
    /// 基準時刻からの経過時間(ミリ秒)で指定。
    Duration(u64),
}

/// 録画の要求元(Record.h 147-151 `RecordClient`)。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RecordClient {
    /// ユーザー操作。
    #[default]
    User,
    /// コマンドライン。
    CommandLine,
    /// プラグイン。
    Plugin,
}

/// 録画マネージャーのモデル(Record.h 128-239 `CRecordManager`)。
///
/// 予約/録画の状態と時刻指定を保持し、開始/停止の判定を純粋に行う。録画エンジンの
/// 実体操作(`CRecordTask`)・ファイル入出力・ダイアログは対象外。録画開始時刻は
/// `CRecordTask::GetStartTime`(ティックカウント)に相当する値を保持する。
#[derive(Clone, Debug, Default)]
pub struct RecordManager {
    recording: bool,
    reserved: bool,
    file_name: String,
    reserve_time: RecordTime,
    start_time_spec: TimeSpec,
    stop_time_spec: TimeSpec,
    stop_on_event_end: bool,
    client: RecordClient,
    /// 録画開始時のティックカウント(`CRecordTask::GetStartTime` 相当)。
    record_start_tick: u64,
}

impl RecordManager {
    /// 録画マネージャーを生成する。
    pub fn new() -> Self {
        Self::default()
    }

    /// ファイル名を設定する(Record.cpp `SetFileName`)。
    pub fn set_file_name(&mut self, file_name: &str) {
        self.file_name = file_name.to_string();
    }

    /// ファイル名を返す(`GetFileName`)。
    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    /// 録画中か(`IsRecording`)。
    pub fn is_recording(&self) -> bool {
        self.recording
    }

    /// 予約中か(`IsReserved`)。
    pub fn is_reserved(&self) -> bool {
        self.reserved
    }

    /// 開始時刻指定を返す(Record.cpp 468-472 `GetStartTimeSpec`)。
    pub fn start_time_spec(&self) -> TimeSpec {
        self.start_time_spec
    }

    /// 停止時刻指定を返す(Record.cpp 485-489 `GetStopTimeSpec`)。
    pub fn stop_time_spec(&self) -> TimeSpec {
        self.stop_time_spec
    }

    /// 開始時刻指定を設定する(Record.cpp 452-465 `SetStartTimeSpec`)。
    ///
    /// 録画中は変更不可で `false`。`NotSpecified` 以外を指定すると予約状態にし、
    /// 予約基準時刻に現在時刻(注入値)を記録する。それ以外は予約を解除する。
    pub fn set_start_time_spec(&mut self, spec: TimeSpec, now: SystemTime, now_tick: u64) -> bool {
        if self.recording {
            return false;
        }
        if spec != TimeSpec::NotSpecified {
            self.reserved = true;
            self.reserve_time.set_current_time(now, now_tick);
            self.start_time_spec = spec;
        } else {
            self.reserved = false;
            self.start_time_spec = TimeSpec::NotSpecified;
        }
        true
    }

    /// 停止時刻指定を設定する(Record.cpp 475-482 `SetStopTimeSpec`)。
    pub fn set_stop_time_spec(&mut self, spec: TimeSpec) {
        self.stop_time_spec = spec;
    }

    /// 停止時刻が指定されているか(Record.cpp 492-495 `IsStopTimeSpecified`)。
    pub fn is_stop_time_specified(&self) -> bool {
        self.stop_time_spec != TimeSpec::NotSpecified
    }

    /// イベント終了で停止するかを設定する(`SetStopOnEventEnd`)。
    pub fn set_stop_on_event_end(&mut self, stop: bool) {
        self.stop_on_event_end = stop;
    }

    /// イベント終了で停止するか(`GetStopOnEventEnd`)。
    pub fn stop_on_event_end(&self) -> bool {
        self.stop_on_event_end
    }

    /// 要求元を返す(`GetClient`)。
    pub fn client(&self) -> RecordClient {
        self.client
    }

    /// 要求元を設定する(`SetClient`)。
    pub fn set_client(&mut self, client: RecordClient) {
        self.client = client;
    }

    /// 録画を開始する(Record.cpp 530-558 `StartRecord` の状態遷移部)。
    ///
    /// 既に録画中なら `false`。開始すると予約を解除し、開始時刻指定もクリアする。
    /// `start_tick` は `CRecordTask` の録画開始ティック(経過時間判定の基準)。
    /// エンジン操作・ファイル名設定・タイムシフト処理は対象外。
    pub fn start_record(&mut self, start_tick: u64) -> bool {
        if self.recording {
            return false;
        }
        self.recording = true;
        self.reserved = false;
        self.start_time_spec = TimeSpec::NotSpecified;
        self.record_start_tick = start_tick;
        true
    }

    /// 録画を停止する(Record.cpp 561-572 `StopRecord` の状態遷移部)。
    pub fn stop_record(&mut self) {
        if self.recording {
            self.recording = false;
        }
    }

    /// 予約を取り消す(Record.cpp 602-608 `CancelReserve`)。予約していなければ `false`。
    pub fn cancel_reserve(&mut self) -> bool {
        if !self.reserved {
            return false;
        }
        self.reserved = false;
        true
    }

    /// 予約の基準時刻を返す(Record.cpp 428-433 `GetReserveTime`)。
    pub fn get_reserve_time(&self) -> Option<SystemTime> {
        if !self.reserved {
            return None;
        }
        self.reserve_time.get_time()
    }

    /// 予約された開始日時を返す(Record.cpp 436-449 `GetReservedStartTime`)。
    ///
    /// 日時指定ならその日時、経過時間指定なら予約基準時刻にオフセットを加算した日時。
    pub fn get_reserved_start_time(&self) -> Option<SystemTime> {
        if !self.reserved {
            return None;
        }
        match self.start_time_spec {
            TimeSpec::DateTime(dt) => Some(dt),
            TimeSpec::Duration(duration) => {
                let mut time = self.reserve_time.get_time()?;
                offset_system_time(&mut time, duration as i64);
                Some(time)
            }
            TimeSpec::NotSpecified => None,
        }
    }

    /// 録画を開始すべきか判定する(Record.cpp 654-685 `QueryStart`)。
    ///
    /// `offset`(ミリ秒)だけ先読みして判定できる。`now`/`now_tick` は現在時刻(注入値)。
    pub fn query_start(&self, offset: i32, now: &SystemTime, now_tick: u64) -> bool {
        if !self.reserved {
            return false;
        }
        match self.start_time_spec {
            TimeSpec::DateTime(dt) => {
                let mut st = *now;
                if offset != 0 {
                    offset_system_time(&mut st, offset as i64);
                }
                compare_system_time(&st, &dt) >= 0
            }
            TimeSpec::Duration(duration) => {
                self.duration_reached(self.reserve_time.tick_time(), now_tick, offset, duration)
            }
            TimeSpec::NotSpecified => false,
        }
    }

    /// 録画を停止すべきか判定する(Record.cpp 688-720 `QueryStop`)。
    pub fn query_stop(&self, offset: i32, now: &SystemTime, now_tick: u64) -> bool {
        if !self.recording {
            return false;
        }
        match self.stop_time_spec {
            TimeSpec::DateTime(dt) => {
                let mut st = *now;
                if offset != 0 {
                    offset_system_time(&mut st, offset as i64);
                }
                compare_system_time(&st, &dt) >= 0
            }
            TimeSpec::Duration(duration) => {
                self.duration_reached(self.record_start_tick, now_tick, offset, duration)
            }
            TimeSpec::NotSpecified => false,
        }
    }

    /// 残り録画時間(ミリ秒)を返す(Record.cpp 627-651 `GetRemainTime`)。
    ///
    /// 録画していなければ -1。停止指定が無くても -1。
    pub fn get_remain_time(&self, now: &SystemTime, now_tick: u64) -> i64 {
        if !self.recording {
            return -1;
        }
        match self.stop_time_spec {
            TimeSpec::DateTime(dt) => diff_system_time(now, &dt),
            TimeSpec::Duration(duration) => {
                duration as i64 - (now_tick - self.record_start_tick) as i64
            }
            TimeSpec::NotSpecified => -1,
        }
    }

    /// 経過時間指定の到達判定(QueryStart/QueryStop の Duration 分岐共通)。
    ///
    /// `start_tick` から `now_tick` までの経過に `offset` を加味し、`duration` 以上なら `true`。
    /// 原実装どおり、`offset` がマイナスで経過を打ち消すほど大きい場合は即 `true`。
    fn duration_reached(&self, start_tick: u64, now_tick: u64, offset: i32, duration: u64) -> bool {
        let mut span = now_tick - start_tick;
        if offset != 0 {
            if (offset as i64) <= -(span as i64) {
                return true;
            }
            span = (span as i64 + offset as i64) as u64;
        }
        span >= duration
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// テスト用の日時を作る(曜日は未使用)。
    fn st(year: u16, month: u16, day: u16, hour: u16, minute: u16, second: u16) -> SystemTime {
        SystemTime {
            year,
            month,
            day_of_week: 0,
            day,
            hour,
            minute,
            second,
            milliseconds: 0,
        }
    }

    #[test]
    fn settings_default_saves_all() {
        let s = RecordingSettings::default();
        assert_eq!(s.save_stream, stream_flag::ALL);
        assert!(s.is_save_caption());
        assert!(s.is_save_data_carrousel());
        assert_eq!(s.write_cache_size, 0x0010_0000);
        assert_eq!(s.max_pending_size, 0x1000_0000);
        assert_eq!(s.time_shift_buffer_size, 32 * 0x0010_0000);
    }

    #[test]
    fn settings_toggle_stream_flags() {
        let mut s = RecordingSettings::default();
        s.set_save_caption(false);
        assert!(!s.is_save_caption());
        // データ放送ビットは影響を受けない。
        assert!(s.is_save_data_carrousel());
        s.set_save_data_carrousel(false);
        assert!(!s.is_save_data_carrousel());
        s.set_save_caption(true);
        assert!(s.is_save_caption());
    }

    #[test]
    fn record_time_validity() {
        let mut t = RecordTime::new();
        assert!(!t.is_valid());
        assert!(t.get_time().is_none());
        t.set_current_time(st(2026, 6, 23, 12, 0, 0), 1000);
        assert!(t.is_valid());
        assert_eq!(t.get_time().unwrap().year, 2026);
        assert_eq!(t.tick_time(), 1000);
        t.clear();
        assert!(!t.is_valid());
        assert_eq!(t.tick_time(), 0);
    }

    #[test]
    fn set_start_time_spec_reserves() {
        let mut m = RecordManager::new();
        assert!(m.set_start_time_spec(
            TimeSpec::DateTime(st(2026, 6, 23, 20, 0, 0)),
            st(2026, 6, 23, 19, 0, 0),
            5000,
        ));
        assert!(m.is_reserved());
        assert_eq!(m.get_reserve_time().unwrap().hour, 19);
        // NotSpecified を渡すと予約解除。
        assert!(m.set_start_time_spec(TimeSpec::NotSpecified, st(2026, 6, 23, 19, 0, 0), 6000));
        assert!(!m.is_reserved());
        assert!(m.get_reserve_time().is_none());
    }

    #[test]
    fn set_start_time_spec_fails_while_recording() {
        let mut m = RecordManager::new();
        m.start_record(0);
        assert!(!m.set_start_time_spec(
            TimeSpec::DateTime(st(2026, 6, 23, 20, 0, 0)),
            st(2026, 6, 23, 19, 0, 0),
            0,
        ));
    }

    #[test]
    fn start_record_clears_reservation() {
        let mut m = RecordManager::new();
        m.set_start_time_spec(
            TimeSpec::DateTime(st(2026, 6, 23, 20, 0, 0)),
            st(2026, 6, 23, 19, 0, 0),
            0,
        );
        assert!(m.is_reserved());
        assert!(m.start_record(1000));
        assert!(m.is_recording());
        assert!(!m.is_reserved());
        assert_eq!(m.start_time_spec(), TimeSpec::NotSpecified);
        // 二重開始は失敗。
        assert!(!m.start_record(2000));
        m.stop_record();
        assert!(!m.is_recording());
    }

    #[test]
    fn cancel_reserve_behaviour() {
        let mut m = RecordManager::new();
        assert!(!m.cancel_reserve());
        m.set_start_time_spec(
            TimeSpec::Duration(60_000),
            st(2026, 6, 23, 19, 0, 0),
            0,
        );
        assert!(m.cancel_reserve());
        assert!(!m.is_reserved());
    }

    #[test]
    fn reserved_start_time_datetime_and_duration() {
        let mut m = RecordManager::new();
        // 日時指定。
        m.set_start_time_spec(
            TimeSpec::DateTime(st(2026, 6, 23, 20, 0, 0)),
            st(2026, 6, 23, 19, 0, 0),
            0,
        );
        assert_eq!(m.get_reserved_start_time().unwrap().hour, 20);
        // 経過時間指定: 基準 19:00 + 30 分 = 19:30。
        m.set_start_time_spec(
            TimeSpec::Duration(30 * 60 * 1000),
            st(2026, 6, 23, 19, 0, 0),
            0,
        );
        let t = m.get_reserved_start_time().unwrap();
        assert_eq!((t.hour, t.minute), (19, 30));
    }

    #[test]
    fn query_start_datetime() {
        let mut m = RecordManager::new();
        m.set_start_time_spec(
            TimeSpec::DateTime(st(2026, 6, 23, 20, 0, 0)),
            st(2026, 6, 23, 19, 0, 0),
            0,
        );
        // 19:59 ではまだ。
        assert!(!m.query_start(0, &st(2026, 6, 23, 19, 59, 0), 0));
        // 20:00 で開始。
        assert!(m.query_start(0, &st(2026, 6, 23, 20, 0, 0), 0));
        // offset 60 秒先読みで 19:59 でも開始。
        assert!(m.query_start(60 * 1000, &st(2026, 6, 23, 19, 59, 0), 0));
    }

    #[test]
    fn query_start_duration() {
        let mut m = RecordManager::new();
        // 基準ティック 1000、duration 5000ms。
        m.set_start_time_spec(TimeSpec::Duration(5000), st(2026, 6, 23, 19, 0, 0), 1000);
        // now_tick 4000 → span 3000 < 5000。
        assert!(!m.query_start(0, &st(2026, 6, 23, 19, 0, 0), 4000));
        // now_tick 6000 → span 5000 >= 5000。
        assert!(m.query_start(0, &st(2026, 6, 23, 19, 0, 0), 6000));
        // span 3000 + offset 2000 = 5000 で到達。
        assert!(m.query_start(2000, &st(2026, 6, 23, 19, 0, 0), 4000));
        // offset が大きなマイナス(経過を打ち消す)→ 即 true。
        assert!(m.query_start(-4000, &st(2026, 6, 23, 19, 0, 0), 4000));
    }

    #[test]
    fn query_stop_requires_recording() {
        let mut m = RecordManager::new();
        m.set_stop_time_spec(TimeSpec::Duration(5000));
        // 録画していなければ常に false。
        assert!(!m.query_stop(0, &st(2026, 6, 23, 19, 0, 0), 999_999));
        m.start_record(1000);
        // now_tick 6000 → span 5000 >= 5000。
        assert!(m.query_stop(0, &st(2026, 6, 23, 19, 0, 0), 6000));
    }

    #[test]
    fn remain_time_datetime_and_duration() {
        let mut m = RecordManager::new();
        m.start_record(1000);
        // 停止指定なし → -1。
        assert_eq!(m.get_remain_time(&st(2026, 6, 23, 19, 0, 0), 2000), -1);
        // 経過時間指定 5000ms、現在 span 2000 → 残り 3000。
        m.set_stop_time_spec(TimeSpec::Duration(5000));
        assert_eq!(m.get_remain_time(&st(2026, 6, 23, 19, 0, 0), 3000), 3000);
        // 日時指定: 現在 19:00、停止 19:10 → 残り 600000ms。
        m.set_stop_time_spec(TimeSpec::DateTime(st(2026, 6, 23, 19, 10, 0)));
        assert_eq!(
            m.get_remain_time(&st(2026, 6, 23, 19, 0, 0), 3000),
            10 * 60 * 1000
        );
    }

    #[test]
    fn remain_time_not_recording() {
        let m = RecordManager::new();
        assert_eq!(m.get_remain_time(&st(2026, 6, 23, 19, 0, 0), 0), -1);
    }

    #[test]
    fn stop_time_specified_flag() {
        let mut m = RecordManager::new();
        assert!(!m.is_stop_time_specified());
        m.set_stop_time_spec(TimeSpec::Duration(1000));
        assert!(m.is_stop_time_specified());
        m.set_stop_time_spec(TimeSpec::NotSpecified);
        assert!(!m.is_stop_time_specified());
    }
}
