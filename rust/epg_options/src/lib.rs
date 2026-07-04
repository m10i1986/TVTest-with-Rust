//! TVTest の `EpgOptions.cpp` / `EpgOptions.h` の純粋部分を移植したもの。
//!
//! EPG/ロゴデータの保存・読み込みに関する設定値のモデルと、`CEpgFileLoader` の
//! 非同期ロード状態機械(8状態の遷移ロジックと `IsLoading`/`IsEpgDataLoading` の判定式)、
//! `LoadEpgFile` のロード可否判定(`EpgFileLoadFlag` とパス存在確認の組み合わせ)を扱う。
//!
//! `DlgProc`(ダイアログ)・`CSettings` I/O本体・実スレッド生成/待機・実ファイルI/Oは対象外。
//! これらが必要とする判定結果を呼び出し側が本クレートの純粋関数へ渡す設計とする。

use std::sync::atomic::{AtomicI32, Ordering};

/// EPG時刻の表示モード。原実装 `CEpgOptions::EpgTimeMode`(EpgOptions.h:59-65)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EpgTimeMode {
    Raw,
    #[default]
    JST,
    Local,
    UTC,
}

impl EpgTimeMode {
    /// `int` から変換する。原実装の `CheckEnumRange` 相当の範囲チェック
    /// (EpgOptions.cpp:78-80: 範囲外の値は無視して既定値のまま据え置く呼び出し側規約)。
    pub fn from_i32(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::Raw),
            1 => Some(Self::JST),
            2 => Some(Self::Local),
            3 => Some(Self::UTC),
            _ => None,
        }
    }

    pub fn to_i32(self) -> i32 {
        self as i32
    }
}

bitflags::bitflags! {
    /// EPGファイルロード対象。原実装 `CEpgOptions::EpgFileLoadFlag`(EpgOptions.h:51-57)。
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct EpgFileLoadFlag: u32 {
        const NONE      = 0x0000;
        const EPG_DATA  = 0x0001;
        const EDCB_DATA = 0x0002;
        const ALL_DATA  = Self::EPG_DATA.bits() | Self::EDCB_DATA.bits();
    }
}

/// `LoadEpgFile` 呼び出し時の設定値・環境判定を渡すための入力。
/// 原実装 `LoadEpgFile`(EpgOptions.cpp:149-189)のうち、ファイルI/O部分を呼び出し側の
/// 事前判定結果(`epg_data_path_exists`/`edcb_folder_is_directory`)として受け取る。
#[derive(Debug, Clone, Copy)]
pub struct LoadEpgFileRequest {
    pub flags: EpgFileLoadFlag,
    /// `m_fSaveEpgFile`。
    pub save_epg_file: bool,
    /// EPGデータファイルの絶対パスが解決でき、かつ実在する。
    pub epg_data_path_exists: bool,
    /// `m_fUseEDCBData`。
    pub use_edcb_data: bool,
    /// `m_EDCBDataFolder` が空でない。
    pub edcb_folder_configured: bool,
    /// EDCBデータフォルダの絶対パスが解決でき、かつディレクトリとして実在する。
    pub edcb_folder_is_directory: bool,
}

/// `LoadEpgFile` の判定結果。どちらの経路もロードしない場合は原実装は早期に `true` を返して
/// 何もしない(EpgOptions.cpp:176-188 の `if` が偽になるケース)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoadEpgFilePlan {
    /// `pEpgDataStore` を使ったEPGファイルロードを行うか。
    pub should_load_epg_data_store: bool,
    /// `CEpgDataLoader` を使ったEDCBデータロードを行うか。
    pub should_load_edcb_data: bool,
}

impl LoadEpgFilePlan {
    /// いずれのロードも不要か。原実装は `pEpgDataStore == nullptr && pEdcbDataLoader == nullptr`
    /// のとき `CEpgFileLoader` を生成せず即 `true` を返す(EpgOptions.cpp:176-188)。
    pub fn is_noop(&self) -> bool {
        !self.should_load_epg_data_store && !self.should_load_edcb_data
    }
}

/// `LoadEpgFile` のロード可否を判定する。原実装 EpgOptions.cpp:159-174。
pub fn plan_load_epg_file(req: &LoadEpgFileRequest) -> LoadEpgFilePlan {
    let should_load_epg_data_store = req.flags.contains(EpgFileLoadFlag::EPG_DATA)
        && req.save_epg_file
        && req.epg_data_path_exists;

    let should_load_edcb_data = req.flags.contains(EpgFileLoadFlag::EDCB_DATA)
        && req.use_edcb_data
        && req.edcb_folder_configured
        && req.edcb_folder_is_directory;

    LoadEpgFilePlan {
        should_load_epg_data_store,
        should_load_edcb_data,
    }
}

/// `CEpgOptions::CEpgFileLoader` の内部状態。原実装 EpgOptions.h:119-128 の無名 enum。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum EpgFileLoaderState {
    Ready = 0,
    Error = 1,
    ThreadStart = 2,
    ThreadEnd = 3,
    EpgDataLoading = 4,
    EpgDataLoaded = 5,
    EdcbDataLoading = 6,
    EdcbDataLoaded = 7,
}

impl EpgFileLoaderState {
    fn from_i32(value: i32) -> Self {
        match value {
            0 => Self::Ready,
            1 => Self::Error,
            2 => Self::ThreadStart,
            3 => Self::ThreadEnd,
            4 => Self::EpgDataLoading,
            5 => Self::EpgDataLoaded,
            6 => Self::EdcbDataLoading,
            7 => Self::EdcbDataLoaded,
            _ => unreachable!("invalid EpgFileLoaderState value: {value}"),
        }
    }
}

/// `CEpgFileLoader` の状態機械。スレッド生成・待機・イベントハンドラ呼び出し自体は
/// 対象外で、状態遷移と `IsEpgDataLoading` の判定式のみを再現する。
/// 原実装 EpgOptions.cpp:527-682。
pub struct EpgFileLoaderStateMachine {
    state: AtomicI32,
    /// `StartLoading` で `pEpgDataStore` が非 null だったか(EpgOptions.cpp:176 相当)。
    has_epg_data_store: bool,
}

impl EpgFileLoaderStateMachine {
    /// `StartLoading` 開始時点の状態。原実装は `m_State = STATE_READY` の後、
    /// スレッド生成に成功すると `STATE_THREAD_START` へ進める(EpgOptions.cpp:566-582)。
    pub fn new(has_epg_data_store: bool) -> Self {
        Self {
            state: AtomicI32::new(EpgFileLoaderState::Ready as i32),
            has_epg_data_store,
        }
    }

    pub fn state(&self) -> EpgFileLoaderState {
        EpgFileLoaderState::from_i32(self.state.load(Ordering::SeqCst))
    }

    fn set_state(&self, state: EpgFileLoaderState) {
        self.state.store(state as i32, Ordering::SeqCst);
    }

    /// スレッド生成に成功した場合。原実装 EpgOptions.cpp:582。
    pub fn on_thread_started(&self) {
        self.set_state(EpgFileLoaderState::ThreadStart);
    }

    /// スレッド生成に失敗した場合。原実装 EpgOptions.cpp:578-580。
    pub fn on_thread_start_failed(&self) {
        self.set_state(EpgFileLoaderState::Error);
    }

    /// `CEpgDataStore::CEventHandler::OnBeginLoading`。原実装 EpgOptions.cpp:617-622。
    pub fn on_begin_epg_data_loading(&self) {
        self.set_state(EpgFileLoaderState::EpgDataLoading);
    }

    /// `CEpgDataStore::CEventHandler::OnEndLoading`。原実装 EpgOptions.cpp:625-630。
    pub fn on_end_epg_data_loading(&self) {
        self.set_state(EpgFileLoaderState::EpgDataLoaded);
    }

    /// `CEpgDataLoader::CEventHandler::OnStart`。原実装 EpgOptions.cpp:633-638。
    pub fn on_begin_edcb_data_loading(&self) {
        self.set_state(EpgFileLoaderState::EdcbDataLoading);
    }

    /// `CEpgDataLoader::CEventHandler::OnEnd`。原実装 EpgOptions.cpp:641-646。
    pub fn on_end_edcb_data_loading(&self) {
        self.set_state(EpgFileLoaderState::EdcbDataLoaded);
    }

    /// ロードスレッド終了時。原実装 `LoadThread`(EpgOptions.cpp:671-682)の
    /// `pLoader->m_State = STATE_THREAD_END`。
    pub fn on_thread_ended(&self) {
        self.set_state(EpgFileLoaderState::ThreadEnd);
    }

    /// `IsEpgDataLoading` の判定式。原実装 EpgOptions.cpp:595-604。
    /// `is_thread_running` は呼び出し側が `WaitForSingleObject(m_hThread, 0)` 相当で判定した
    /// 結果を渡す(本クレートはスレッド自体を扱わないため)。
    pub fn is_epg_data_loading(&self, is_thread_running: bool) -> bool {
        if !is_thread_running {
            return false;
        }

        let state = self.state();
        state == EpgFileLoaderState::EpgDataLoading
            || (state == EpgFileLoaderState::ThreadStart && self.has_epg_data_store)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epg_time_mode_from_i32_valid() {
        assert_eq!(EpgTimeMode::from_i32(0), Some(EpgTimeMode::Raw));
        assert_eq!(EpgTimeMode::from_i32(1), Some(EpgTimeMode::JST));
        assert_eq!(EpgTimeMode::from_i32(2), Some(EpgTimeMode::Local));
        assert_eq!(EpgTimeMode::from_i32(3), Some(EpgTimeMode::UTC));
    }

    #[test]
    fn epg_time_mode_from_i32_out_of_range() {
        assert_eq!(EpgTimeMode::from_i32(-1), None);
        assert_eq!(EpgTimeMode::from_i32(4), None);
    }

    #[test]
    fn epg_time_mode_roundtrip() {
        for m in [
            EpgTimeMode::Raw,
            EpgTimeMode::JST,
            EpgTimeMode::Local,
            EpgTimeMode::UTC,
        ] {
            assert_eq!(EpgTimeMode::from_i32(m.to_i32()), Some(m));
        }
    }

    #[test]
    fn epg_time_mode_default_is_jst() {
        assert_eq!(EpgTimeMode::default(), EpgTimeMode::JST);
    }

    #[test]
    fn load_flag_all_data_is_union() {
        assert!(EpgFileLoadFlag::ALL_DATA.contains(EpgFileLoadFlag::EPG_DATA));
        assert!(EpgFileLoadFlag::ALL_DATA.contains(EpgFileLoadFlag::EDCB_DATA));
    }

    fn base_request() -> LoadEpgFileRequest {
        LoadEpgFileRequest {
            flags: EpgFileLoadFlag::ALL_DATA,
            save_epg_file: true,
            epg_data_path_exists: true,
            use_edcb_data: true,
            edcb_folder_configured: true,
            edcb_folder_is_directory: true,
        }
    }

    #[test]
    fn plan_load_epg_file_all_conditions_met() {
        let plan = plan_load_epg_file(&base_request());
        assert!(plan.should_load_epg_data_store);
        assert!(plan.should_load_edcb_data);
        assert!(!plan.is_noop());
    }

    #[test]
    fn plan_load_epg_file_flag_not_requested() {
        let mut req = base_request();
        req.flags = EpgFileLoadFlag::EDCB_DATA;
        let plan = plan_load_epg_file(&req);
        assert!(!plan.should_load_epg_data_store);
        assert!(plan.should_load_edcb_data);
    }

    #[test]
    fn plan_load_epg_file_save_disabled() {
        let mut req = base_request();
        req.save_epg_file = false;
        let plan = plan_load_epg_file(&req);
        assert!(!plan.should_load_epg_data_store);
    }

    #[test]
    fn plan_load_epg_file_path_missing() {
        let mut req = base_request();
        req.epg_data_path_exists = false;
        let plan = plan_load_epg_file(&req);
        assert!(!plan.should_load_epg_data_store);
    }

    #[test]
    fn plan_load_epg_file_edcb_disabled() {
        let mut req = base_request();
        req.use_edcb_data = false;
        let plan = plan_load_epg_file(&req);
        assert!(!plan.should_load_edcb_data);
    }

    #[test]
    fn plan_load_epg_file_edcb_folder_empty() {
        let mut req = base_request();
        req.edcb_folder_configured = false;
        let plan = plan_load_epg_file(&req);
        assert!(!plan.should_load_edcb_data);
    }

    #[test]
    fn plan_load_epg_file_edcb_folder_not_directory() {
        let mut req = base_request();
        req.edcb_folder_is_directory = false;
        let plan = plan_load_epg_file(&req);
        assert!(!plan.should_load_edcb_data);
    }

    #[test]
    fn plan_load_epg_file_noop_when_nothing_to_load() {
        let req = LoadEpgFileRequest {
            flags: EpgFileLoadFlag::NONE,
            save_epg_file: true,
            epg_data_path_exists: true,
            use_edcb_data: true,
            edcb_folder_configured: true,
            edcb_folder_is_directory: true,
        };
        let plan = plan_load_epg_file(&req);
        assert!(plan.is_noop());
    }

    #[test]
    fn state_machine_initial_state_ready() {
        let sm = EpgFileLoaderStateMachine::new(true);
        assert_eq!(sm.state(), EpgFileLoaderState::Ready);
    }

    #[test]
    fn state_machine_thread_start_failure() {
        let sm = EpgFileLoaderStateMachine::new(true);
        sm.on_thread_start_failed();
        assert_eq!(sm.state(), EpgFileLoaderState::Error);
    }

    #[test]
    fn state_machine_epg_data_loading_flow() {
        let sm = EpgFileLoaderStateMachine::new(true);
        sm.on_thread_started();
        assert_eq!(sm.state(), EpgFileLoaderState::ThreadStart);
        // ThreadStart 中でも has_epg_data_store があれば「EPGデータロード中」扱い。
        assert!(sm.is_epg_data_loading(true));

        sm.on_begin_epg_data_loading();
        assert_eq!(sm.state(), EpgFileLoaderState::EpgDataLoading);
        assert!(sm.is_epg_data_loading(true));

        sm.on_end_epg_data_loading();
        assert_eq!(sm.state(), EpgFileLoaderState::EpgDataLoaded);
        assert!(!sm.is_epg_data_loading(true));
    }

    #[test]
    fn state_machine_edcb_data_loading_flow() {
        let sm = EpgFileLoaderStateMachine::new(false);
        sm.on_thread_started();
        // has_epg_data_store が false の ThreadStart は EPG データロード中ではない。
        assert!(!sm.is_epg_data_loading(true));

        sm.on_begin_edcb_data_loading();
        assert_eq!(sm.state(), EpgFileLoaderState::EdcbDataLoading);
        assert!(!sm.is_epg_data_loading(true));

        sm.on_end_edcb_data_loading();
        assert_eq!(sm.state(), EpgFileLoaderState::EdcbDataLoaded);
    }

    #[test]
    fn state_machine_not_loading_when_thread_not_running() {
        let sm = EpgFileLoaderStateMachine::new(true);
        sm.on_thread_started();
        // スレッドが既に終了している(呼び出し側判定)場合は false。
        assert!(!sm.is_epg_data_loading(false));
    }

    #[test]
    fn state_machine_thread_ended() {
        let sm = EpgFileLoaderStateMachine::new(true);
        sm.on_thread_started();
        sm.on_begin_epg_data_loading();
        sm.on_thread_ended();
        assert_eq!(sm.state(), EpgFileLoaderState::ThreadEnd);
    }
}
