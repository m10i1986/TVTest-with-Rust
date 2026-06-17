// Rust port of LibISDB/Filters/SourceFilter.cpp + SourceFilter.hpp
//                + LibISDB/Base/EventListener.hpp
//
// SourceFilter は LibISDB のソースフィルタ基底クラス(SingleOutputFilter を継承)で、
// 派生クラス(StreamSourceFilter / BonDriverSourceFilter 等)が共有する以下の
// 純粋ロジックをこのクレートに移植する:
//   - SourceMode フラグ(Push / Pull)とそのビット演算
//   - SetSourceMode の検証ロジック(SourceFilter.cpp:48)
//   - Finalize() が CloseSource() を呼ぶ規約(SourceFilter.cpp:42)
//   - EventListenerList<T>(Base/EventListener.hpp:48)の追加/削除/列挙
//
// 設計上の相違:
//   - C++ の純粋仮想クラス階層 → Rust の trait + 共有状態 struct (SourceFilterState)
//   - C++ の EventListenerList は raw ポインタ(T*)を保持しポインタ同値で重複排除する。
//     Rust では Rc<RefCell<T>> を保持し Rc::ptr_eq で同値判定する(同じ意味論)。
//   - C++ のイベント通知 (CallEventListener(&EventListener::OnXxx, this)) は
//     リスナへ SourceFilter* を渡すが、Rust では self を借用中のリスナに self を
//     再度渡せないため source ポインタ引数は省く(リスナが必要なら自前で保持する)。
//   - MutexLock は省略(呼び出し側で同期する設計。filter_base と同方針)。

use std::cell::RefCell;
use std::rc::Rc;

// ---------------------------------------------------------------------------
// SourceMode (SourceFilter.hpp:43)
// ---------------------------------------------------------------------------

bitflags::bitflags! {
    /// ソースの駆動方式。C++ の `SourceFilter::SourceMode` に対応する。
    ///
    /// - `PUSH`: ソース側がデータを能動的に下流へ送り出す
    /// - `PULL`: 下流(AsyncStreamingFilter 等)が `FetchSource` で能動的に引き出す
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
    pub struct SourceMode: u32 {
        const PUSH = 0x0001;
        const PULL = 0x0002;
    }
}

// ---------------------------------------------------------------------------
// EventListenerList<T> (Base/EventListener.hpp:48)
// ---------------------------------------------------------------------------

/// イベントリスナのリスト。C++ の `EventListenerList<T>` テンプレートに対応する。
///
/// 重複登録はリスナの同値性(`Rc::ptr_eq`)で排除する。これは C++ がリスナの
/// raw ポインタを `std::ranges::find` で比較するのと同じ意味論。
pub struct EventListenerList<T: ?Sized> {
    listeners: Vec<Rc<RefCell<T>>>,
}

impl<T: ?Sized> EventListenerList<T> {
    pub fn new() -> Self {
        Self { listeners: Vec::new() }
    }

    /// リスナを追加する。既に登録済み(同一インスタンス)なら `false`。
    /// C++ `AddEventListener` (EventListener.hpp:51)。nullptr 相当は型で排除済み。
    pub fn add_event_listener(&mut self, listener: Rc<RefCell<T>>) -> bool {
        if self.listeners.iter().any(|l| Rc::ptr_eq(l, &listener)) {
            return false;
        }
        self.listeners.push(listener);
        true
    }

    /// リスナを削除する。登録されていなければ `false`。
    /// C++ `RemoveEventListener` (EventListener.hpp:66)。
    pub fn remove_event_listener(&mut self, listener: &Rc<RefCell<T>>) -> bool {
        match self.listeners.iter().position(|l| Rc::ptr_eq(l, listener)) {
            Some(pos) => {
                self.listeners.remove(pos);
                true
            }
            None => false,
        }
    }

    /// 全リスナを削除する。C++ `RemoveAllEventListeners` (EventListener.hpp:80)。
    pub fn remove_all_event_listeners(&mut self) {
        self.listeners.clear();
    }

    /// 登録リスナ数。C++ `GetEventListenerCount` (EventListener.hpp:87)。
    pub fn event_listener_count(&self) -> usize {
        self.listeners.len()
    }

    /// 全リスナを登録順に呼び出す。C++ `CallEventListener` (EventListener.hpp:94)。
    ///
    /// メンバ関数ポインタ + 可変長引数の代わりにクロージャを受け取り、各リスナへ
    /// 適用する(例: `list.call_event_listener(|l| l.on_source_opened())`)。
    pub fn call_event_listener<F: FnMut(&mut T)>(&self, mut f: F) {
        for l in &self.listeners {
            f(&mut l.borrow_mut());
        }
    }
}

impl<T: ?Sized> Default for EventListenerList<T> {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// SourceEventListener (SourceFilter.hpp:49)
// ---------------------------------------------------------------------------

/// ソースフィルタのイベントリスナ。C++ の `SourceFilter::EventListener` に対応する。
///
/// 既定実装は全て何もしない(C++ の空仮想関数に対応)。
pub trait SourceEventListener {
    fn on_graph_reset(&mut self) {}
    fn on_source_opened(&mut self) {}
    fn on_source_closed(&mut self) {}
    fn on_source_changed(&mut self) {}
    fn on_source_change_failed(&mut self) {}
    fn on_source_end(&mut self) {}
    fn on_streaming_start(&mut self) {}
    fn on_streaming_stop(&mut self) {}
}

/// 共有リスナハンドル。
pub type SourceEventListenerHandle = Rc<RefCell<dyn SourceEventListener>>;

// ---------------------------------------------------------------------------
// SourceFilterState — SourceFilter の共有メンバ (SourceFilter.hpp:83-84)
// ---------------------------------------------------------------------------

/// `SourceFilter` の派生クラスが共有で持つ状態。
/// C++ の `m_SourceMode` + `m_EventListenerList` に対応する。
pub struct SourceFilterState {
    source_mode: SourceMode,
    listeners: EventListenerList<dyn SourceEventListener>,
}

impl SourceFilterState {
    /// C++ `SourceFilter::SourceFilter(SourceMode Mode)` (SourceFilter.cpp:36)。
    /// 構築時はモード検証を行わず、そのまま保持する。
    pub fn new(mode: SourceMode) -> Self {
        Self {
            source_mode: mode,
            listeners: EventListenerList::new(),
        }
    }

    /// 現在のソースモード。
    pub fn source_mode(&self) -> SourceMode {
        self.source_mode
    }

    /// リスナリストへの参照(イベント通知のため)。
    pub fn listeners(&self) -> &EventListenerList<dyn SourceEventListener> {
        &self.listeners
    }

    // --- イベント通知ヘルパ (CallEventListener(&EventListener::OnXxx, this) 相当) ---

    pub fn notify_graph_reset(&self) {
        self.listeners.call_event_listener(|l| l.on_graph_reset());
    }
    pub fn notify_source_opened(&self) {
        self.listeners.call_event_listener(|l| l.on_source_opened());
    }
    pub fn notify_source_closed(&self) {
        self.listeners.call_event_listener(|l| l.on_source_closed());
    }
    pub fn notify_source_changed(&self) {
        self.listeners.call_event_listener(|l| l.on_source_changed());
    }
    pub fn notify_source_change_failed(&self) {
        self.listeners.call_event_listener(|l| l.on_source_change_failed());
    }
    pub fn notify_source_end(&self) {
        self.listeners.call_event_listener(|l| l.on_source_end());
    }
    pub fn notify_streaming_start(&self) {
        self.listeners.call_event_listener(|l| l.on_streaming_start());
    }
    pub fn notify_streaming_stop(&self) {
        self.listeners.call_event_listener(|l| l.on_streaming_stop());
    }
}

// ---------------------------------------------------------------------------
// SourceFilter trait (SourceFilter.hpp:39)
// ---------------------------------------------------------------------------

/// ソースフィルタ基底 trait。C++ の `SourceFilter`(`SingleOutputFilter` 派生)に対応する。
///
/// 派生フィルタは抽象メソッド(`open_source` 等)と共有状態アクセサ
/// (`source_state` / `source_state_mut`)を実装する。検証付きの `set_source_mode`、
/// `finalize`、リスナ操作は既定実装として提供する。
///
/// 出力ポート(SingleOutputFilter の OutputSlot)はこの trait の責務外とし、派生側で
/// `libisdb_filter_base::OutputSlot` を保持する(他フィルタ移植と同方針)。
pub trait SourceFilter {
    // --- 抽象メソッド (SourceFilter.hpp:67-73) ---

    /// ソースを開く。C++ `OpenSource(const String &)`(純粋仮想)。
    fn open_source(&mut self, name: &str) -> bool;

    /// ソースを閉じる。C++ `CloseSource()`(純粋仮想)。
    fn close_source(&mut self) -> bool;

    /// ソースが開いているか。C++ `IsSourceOpen()`(純粋仮想)。
    fn is_source_open(&self) -> bool;

    /// Pull モードでデータを引き出す。C++ `FetchSource(size_t)`(既定 false)。
    fn fetch_source(&mut self, _request_size: usize) -> bool {
        false
    }

    /// 利用可能なソースモード。C++ `GetAvailableSourceModes()`(純粋仮想)。
    fn available_source_modes(&self) -> SourceMode;

    // --- 共有状態アクセサ ---

    fn source_state(&self) -> &SourceFilterState;
    fn source_state_mut(&mut self) -> &mut SourceFilterState;

    // --- 既定実装 ---

    /// C++ `FilterBase::Finalize` のオーバーライド(SourceFilter.cpp:42)。CloseSource を呼ぶ。
    fn finalize(&mut self) {
        self.close_source();
    }

    /// 現在のソースモード。C++ `GetSourceMode()`(SourceFilter.hpp:75)。
    fn get_source_mode(&self) -> SourceMode {
        self.source_state().source_mode
    }

    /// ソースモードを設定する。C++ `SetSourceMode(SourceMode)`(SourceFilter.cpp:48)。
    ///
    /// 検証: モードがちょうど `PUSH` か `PULL` のいずれか単独で、かつ
    /// `available_source_modes()` に含まれていること。違反時は `false`(変更なし)。
    fn set_source_mode(&mut self, mode: SourceMode) -> bool {
        if mode != SourceMode::PUSH && mode != SourceMode::PULL {
            return false;
        }
        if !self.available_source_modes().intersects(mode) {
            return false;
        }
        self.source_state_mut().source_mode = mode;
        true
    }

    /// リスナを追加する。C++ `AddEventListener`(SourceFilter.cpp:61)。
    fn add_event_listener(&mut self, listener: SourceEventListenerHandle) -> bool {
        self.source_state_mut().listeners.add_event_listener(listener)
    }

    /// リスナを削除する。C++ `RemoveEventListener`(SourceFilter.cpp:67)。
    fn remove_event_listener(&mut self, listener: &SourceEventListenerHandle) -> bool {
        self.source_state_mut().listeners.remove_event_listener(listener)
    }
}

#[cfg(test)]
mod tests;
