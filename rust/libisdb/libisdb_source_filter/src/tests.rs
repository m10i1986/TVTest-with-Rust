// libisdb_source_filter のテスト
//
// SourceMode のビット演算 / SetSourceMode 検証 / EventListenerList の追加・削除・
// 列挙 / SourceEventListener のディスパッチ / Finalize→CloseSource を検証する。

use super::*;

// ──────────────────────────────────────────────
// テスト用のモックソースフィルタ
// ──────────────────────────────────────────────

/// Push/Pull 両対応のモックソースフィルタ(StreamSourceFilter 相当の挙動)。
struct MockSource {
    state: SourceFilterState,
    available: SourceMode,
    open: bool,
    close_called: usize,
}

impl MockSource {
    fn new(initial: SourceMode, available: SourceMode) -> Self {
        Self {
            state: SourceFilterState::new(initial),
            available,
            open: false,
            close_called: 0,
        }
    }
}

impl SourceFilter for MockSource {
    fn open_source(&mut self, _name: &str) -> bool {
        self.open = true;
        true
    }
    fn close_source(&mut self) -> bool {
        self.close_called += 1;
        self.open = false;
        true
    }
    fn is_source_open(&self) -> bool {
        self.open
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

/// 各コールバックの呼び出し回数を記録するリスナ。
#[derive(Default)]
struct RecordingListener {
    graph_reset: usize,
    opened: usize,
    closed: usize,
    changed: usize,
    change_failed: usize,
    end: usize,
    streaming_start: usize,
    streaming_stop: usize,
}

impl SourceEventListener for RecordingListener {
    fn on_graph_reset(&mut self) {
        self.graph_reset += 1;
    }
    fn on_source_opened(&mut self) {
        self.opened += 1;
    }
    fn on_source_closed(&mut self) {
        self.closed += 1;
    }
    fn on_source_changed(&mut self) {
        self.changed += 1;
    }
    fn on_source_change_failed(&mut self) {
        self.change_failed += 1;
    }
    fn on_source_end(&mut self) {
        self.end += 1;
    }
    fn on_streaming_start(&mut self) {
        self.streaming_start += 1;
    }
    fn on_streaming_stop(&mut self) {
        self.streaming_stop += 1;
    }
}

// ──────────────────────────────────────────────
// SourceMode
// ──────────────────────────────────────────────

#[test]
fn test_source_mode_bits() {
    assert_eq!(SourceMode::PUSH.bits(), 0x0001);
    assert_eq!(SourceMode::PULL.bits(), 0x0002);

    let both = SourceMode::PUSH | SourceMode::PULL;
    assert!(both.contains(SourceMode::PUSH));
    assert!(both.contains(SourceMode::PULL));
    assert!(both.intersects(SourceMode::PUSH));
    assert_ne!(both, SourceMode::PUSH);
    assert_ne!(both, SourceMode::PULL);
}

// ──────────────────────────────────────────────
// SetSourceMode の検証 (SourceFilter.cpp:48)
// ──────────────────────────────────────────────

#[test]
fn test_set_source_mode_accepts_available_single_mode() {
    let mut src = MockSource::new(SourceMode::PUSH, SourceMode::PUSH | SourceMode::PULL);

    assert!(src.set_source_mode(SourceMode::PULL));
    assert_eq!(src.get_source_mode(), SourceMode::PULL);

    assert!(src.set_source_mode(SourceMode::PUSH));
    assert_eq!(src.get_source_mode(), SourceMode::PUSH);
}

#[test]
fn test_set_source_mode_rejects_unavailable_mode() {
    // Push のみ対応のソースに Pull を要求 → 拒否・変更なし
    let mut src = MockSource::new(SourceMode::PUSH, SourceMode::PUSH);

    assert!(!src.set_source_mode(SourceMode::PULL));
    assert_eq!(src.get_source_mode(), SourceMode::PUSH);
}

#[test]
fn test_set_source_mode_rejects_combined_flags() {
    // ちょうど Push か Pull の単独でなければ拒否(原実装 Mode != Push && Mode != Pull)
    let mut src = MockSource::new(SourceMode::PUSH, SourceMode::PUSH | SourceMode::PULL);

    assert!(!src.set_source_mode(SourceMode::PUSH | SourceMode::PULL));
    assert_eq!(src.get_source_mode(), SourceMode::PUSH);
}

#[test]
fn test_set_source_mode_rejects_empty() {
    let mut src = MockSource::new(SourceMode::PUSH, SourceMode::PUSH | SourceMode::PULL);

    assert!(!src.set_source_mode(SourceMode::empty()));
    assert_eq!(src.get_source_mode(), SourceMode::PUSH);
}

// ──────────────────────────────────────────────
// Finalize → CloseSource (SourceFilter.cpp:42)
// ──────────────────────────────────────────────

#[test]
fn test_finalize_calls_close_source() {
    let mut src = MockSource::new(SourceMode::PUSH, SourceMode::PUSH);
    src.open_source("dummy");
    assert!(src.is_source_open());

    src.finalize();
    assert!(!src.is_source_open());
    assert_eq!(src.close_called, 1);
}

#[test]
fn test_fetch_source_default_false() {
    let mut src = MockSource::new(SourceMode::PUSH, SourceMode::PUSH);
    assert!(!src.fetch_source(1024));
}

// ──────────────────────────────────────────────
// EventListenerList の追加 / 削除 / 重複排除
// ──────────────────────────────────────────────

#[test]
fn test_event_listener_add_dedup() {
    let mut list: EventListenerList<dyn SourceEventListener> = EventListenerList::new();
    let a: SourceEventListenerHandle = Rc::new(RefCell::new(RecordingListener::default()));
    let b: SourceEventListenerHandle = Rc::new(RefCell::new(RecordingListener::default()));

    assert!(list.add_event_listener(a.clone()));
    assert_eq!(list.event_listener_count(), 1);

    // 同一インスタンスの再追加は拒否
    assert!(!list.add_event_listener(a.clone()));
    assert_eq!(list.event_listener_count(), 1);

    // 別インスタンスは追加可
    assert!(list.add_event_listener(b.clone()));
    assert_eq!(list.event_listener_count(), 2);
}

#[test]
fn test_event_listener_remove() {
    let mut list: EventListenerList<dyn SourceEventListener> = EventListenerList::new();
    let a: SourceEventListenerHandle = Rc::new(RefCell::new(RecordingListener::default()));
    let b: SourceEventListenerHandle = Rc::new(RefCell::new(RecordingListener::default()));

    list.add_event_listener(a.clone());

    // 未登録の削除は false
    assert!(!list.remove_event_listener(&b));
    // 登録済みの削除は true
    assert!(list.remove_event_listener(&a));
    assert_eq!(list.event_listener_count(), 0);
    // 2 回目は false
    assert!(!list.remove_event_listener(&a));
}

#[test]
fn test_event_listener_remove_all() {
    let mut list: EventListenerList<dyn SourceEventListener> = EventListenerList::new();
    for _ in 0..3 {
        let l: SourceEventListenerHandle = Rc::new(RefCell::new(RecordingListener::default()));
        list.add_event_listener(l);
    }
    assert_eq!(list.event_listener_count(), 3);

    list.remove_all_event_listeners();
    assert_eq!(list.event_listener_count(), 0);
}

// ──────────────────────────────────────────────
// イベント通知のディスパッチ
// ──────────────────────────────────────────────

#[test]
fn test_notify_dispatches_to_all_listeners_in_order() {
    let mut src = MockSource::new(SourceMode::PUSH, SourceMode::PUSH | SourceMode::PULL);

    let l1 = Rc::new(RefCell::new(RecordingListener::default()));
    let l2 = Rc::new(RefCell::new(RecordingListener::default()));
    src.add_event_listener(l1.clone());
    src.add_event_listener(l2.clone());

    src.source_state().notify_source_opened();
    src.source_state().notify_source_opened();
    src.source_state().notify_streaming_start();
    src.source_state().notify_source_end();

    for l in [&l1, &l2] {
        let r = l.borrow();
        assert_eq!(r.opened, 2);
        assert_eq!(r.streaming_start, 1);
        assert_eq!(r.end, 1);
        // 呼んでいないものは 0 のまま
        assert_eq!(r.closed, 0);
        assert_eq!(r.graph_reset, 0);
    }
}

#[test]
fn test_notify_each_callback_routes_to_correct_method() {
    let mut src = MockSource::new(SourceMode::PUSH, SourceMode::PUSH | SourceMode::PULL);
    let l = Rc::new(RefCell::new(RecordingListener::default()));
    src.add_event_listener(l.clone());

    let s = src.source_state();
    s.notify_graph_reset();
    s.notify_source_opened();
    s.notify_source_closed();
    s.notify_source_changed();
    s.notify_source_change_failed();
    s.notify_source_end();
    s.notify_streaming_start();
    s.notify_streaming_stop();

    let r = l.borrow();
    assert_eq!(r.graph_reset, 1);
    assert_eq!(r.opened, 1);
    assert_eq!(r.closed, 1);
    assert_eq!(r.changed, 1);
    assert_eq!(r.change_failed, 1);
    assert_eq!(r.end, 1);
    assert_eq!(r.streaming_start, 1);
    assert_eq!(r.streaming_stop, 1);
}

#[test]
fn test_remove_listener_stops_notifications() {
    let mut src = MockSource::new(SourceMode::PUSH, SourceMode::PUSH);
    let l = Rc::new(RefCell::new(RecordingListener::default()));
    let handle: SourceEventListenerHandle = l.clone();
    src.add_event_listener(handle.clone());

    src.source_state().notify_source_opened();
    assert_eq!(l.borrow().opened, 1);

    assert!(src.remove_event_listener(&handle));
    src.source_state().notify_source_opened();
    // 削除後は増えない
    assert_eq!(l.borrow().opened, 1);
}

#[test]
fn test_default_listener_impl_does_nothing() {
    // 全コールバックを既定実装(空)に任せたリスナでもパニックしない
    struct SilentListener;
    impl SourceEventListener for SilentListener {}

    let mut list: EventListenerList<dyn SourceEventListener> = EventListenerList::new();
    let l: SourceEventListenerHandle = Rc::new(RefCell::new(SilentListener));
    list.add_event_listener(l);

    list.call_event_listener(|x| x.on_source_opened());
    list.call_event_listener(|x| x.on_source_end());
}
