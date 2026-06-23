//! TVTest のログ機能(src/Logger.cpp / Logger.h)のモデル層移植。
//!
//! ログ項目とその蓄積・索引を表現する:
//! - [`LogType`](情報 / 警告 / エラー)
//! - [`LogItem`](種別 + 本文 + 通し番号 + 時刻)
//! - [`Logger`](ログの追加 / 消去 / 件数 / 索引取得 / ファイル出力フラグ)
//!
//! # 対象外(Win32 / I-O 依存)
//! ロケール依存の整形(`Format`/`FormatTime` = `GetDateFormat`/`GetTimeFormat`/
//! `WideCharToMultiByte`)、時刻のローカル変換(`GetTime` = `FileTimeToSystemTime`/
//! `SystemTimeToTzSpecificLocalTime`)、ファイル入出力(`SaveToFile`/排他ロック)、
//! 設定入出力、ダイアログ(`DlgProc`)、クリップボード、既定ログファイル名取得。
//!
//! 本文は C++ の `String`(`std::wstring`)に合わせて UTF-16 の `Vec<u16>` で保持する。
//! 時刻は OS 依存(`GetSystemTimeAsFileTime`)のため、生成時に値(FILETIME 相当の
//! 100ns 単位 `u64`)を引数で受け取り、純粋に保持するだけとする。

/// ログ項目の種別(Logger.h 37-41 `CLogItem::LogType`)。
///
/// 列挙順(0:情報 / 1:警告 / 2:エラー)は原実装で `static_cast<int>` してアイコン
/// 索引に用いるため、[`LogType::index`] で同じ値を得られるようにしてある。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogType {
    Information,
    Warning,
    Error,
}

impl LogType {
    /// 列挙順に対応する索引(0:Information / 1:Warning / 2:Error)を返す。
    ///
    /// 原実装が `static_cast<int>(GetType())` でアイコン索引に使う値に一致する。
    pub fn index(self) -> i32 {
        match self {
            LogType::Information => 0,
            LogType::Warning => 1,
            LogType::Error => 2,
        }
    }
}

/// ログ項目(Logger.h 34-60 `CLogItem`)。
///
/// 本文は UTF-16(`Vec<u16>`)。`time` は FILETIME 相当(1601-01-01 からの 100ns 単位)
/// を表す `u64`。原実装は構築時に `GetSystemTimeAsFileTime` で設定するが、ここでは
/// OS 非依存にするため呼び出し側が値を渡す。
#[derive(Clone, Debug)]
pub struct LogItem {
    time: u64,
    text: Vec<u16>,
    log_type: LogType,
    serial_number: u32,
}

impl LogItem {
    /// ログ項目を生成する(Logger.cpp 46-52 `CLogItem::CLogItem`)。
    pub fn new(log_type: LogType, text: &[u16], serial_number: u32, time: u64) -> Self {
        Self {
            time,
            text: text.to_vec(),
            log_type,
            serial_number,
        }
    }

    /// 本文を返す(`GetText`)。
    pub fn text(&self) -> &[u16] {
        &self.text
    }

    /// 種別を返す(`GetType`)。
    pub fn log_type(&self) -> LogType {
        self.log_type
    }

    /// 通し番号を返す(`GetSerialNumber`)。
    pub fn serial_number(&self) -> u32 {
        self.serial_number
    }

    /// 時刻(FILETIME 相当の 100ns 単位)を返す。
    pub fn time(&self) -> u64 {
        self.time
    }
}

/// ログ機能のモデル(Logger.h 62-117 `CLogger`)。
///
/// ログ項目を蓄積し、通し番号で索引する。スレッド排他(`MutexLock`)・ファイル出力・
/// 設定入出力・ダイアログは対象外。
#[derive(Clone, Debug, Default)]
pub struct Logger {
    log_list: Vec<LogItem>,
    serial_number: u32,
    output_to_file: bool,
}

impl Logger {
    /// 空のロガーを生成する。
    pub fn new() -> Self {
        Self::default()
    }

    /// 本文を指定してログを 1 件追加する(Logger.cpp 191-240 `AddLogRaw`)。
    ///
    /// 本文が空のときは `false`(原実装どおり)。追加時に現在の通し番号を割り当てて
    /// から内部カウンタを進める。`time` は FILETIME 相当の値を呼び出し側が渡す。
    /// ファイル出力は対象外のため、ここでは項目の蓄積のみ行う。
    pub fn add_log_raw(&mut self, log_type: LogType, text: &[u16], time: u64) -> bool {
        if text.is_empty() {
            return false;
        }
        let item = LogItem::new(log_type, text, self.serial_number, time);
        self.serial_number += 1;
        self.log_list.push(item);
        true
    }

    /// 全ログを消去する(Logger.cpp 243-248 `Clear`)。
    ///
    /// 原実装と同じく通し番号カウンタ(`serial_number`)は維持する。
    pub fn clear(&mut self) {
        self.log_list.clear();
    }

    /// ログ件数を返す(Logger.cpp 251-256 `GetLogCount`)。
    pub fn log_count(&self) -> usize {
        self.log_list.len()
    }

    /// 索引でログ項目を取得する(Logger.cpp 259-272 `GetLog`)。
    pub fn get_log(&self, index: usize) -> Option<&LogItem> {
        self.log_list.get(index)
    }

    /// 通し番号でログ項目を取得する(Logger.cpp 275-292 `GetLogBySerialNumber`)。
    ///
    /// 先頭項目の通し番号を基点に、`[first, first + len)` の範囲なら相対位置で取得。
    /// 消去後も項目は先頭から連番のため、この単純な算術で原実装と一致する。
    pub fn get_log_by_serial_number(&self, serial_number: u32) -> Option<&LogItem> {
        let first = self.log_list.first()?.serial_number();
        if serial_number < first || serial_number >= first + self.log_list.len() as u32 {
            return None;
        }
        self.log_list.get((serial_number - first) as usize)
    }

    /// 蓄積済みの全ログ項目を返す。
    pub fn log_items(&self) -> &[LogItem] {
        &self.log_list
    }

    /// ファイル出力フラグを設定する(Logger.cpp 295-301 `SetOutputToFile`)。
    pub fn set_output_to_file(&mut self, output: bool) {
        self.output_to_file = output;
    }

    /// ファイル出力フラグを返す(Logger.h 88 `GetOutputToFile`)。
    pub fn output_to_file(&self) -> bool {
        self.output_to_file
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn log_type_index_matches_enum_order() {
        assert_eq!(LogType::Information.index(), 0);
        assert_eq!(LogType::Warning.index(), 1);
        assert_eq!(LogType::Error.index(), 2);
    }

    #[test]
    fn log_item_holds_fields() {
        let item = LogItem::new(LogType::Warning, &w("hello"), 42, 1234);
        assert_eq!(item.text(), w("hello").as_slice());
        assert_eq!(item.log_type(), LogType::Warning);
        assert_eq!(item.serial_number(), 42);
        assert_eq!(item.time(), 1234);
    }

    #[test]
    fn add_log_assigns_incrementing_serial() {
        let mut logger = Logger::new();
        assert!(logger.add_log_raw(LogType::Information, &w("a"), 0));
        assert!(logger.add_log_raw(LogType::Warning, &w("b"), 0));
        assert!(logger.add_log_raw(LogType::Error, &w("c"), 0));
        assert_eq!(logger.log_count(), 3);
        assert_eq!(logger.get_log(0).unwrap().serial_number(), 0);
        assert_eq!(logger.get_log(1).unwrap().serial_number(), 1);
        assert_eq!(logger.get_log(2).unwrap().serial_number(), 2);
    }

    #[test]
    fn add_log_rejects_empty_text() {
        let mut logger = Logger::new();
        assert!(!logger.add_log_raw(LogType::Information, &[], 0));
        assert_eq!(logger.log_count(), 0);
        // 通し番号は消費されない。
        assert!(logger.add_log_raw(LogType::Information, &w("x"), 0));
        assert_eq!(logger.get_log(0).unwrap().serial_number(), 0);
    }

    #[test]
    fn get_log_out_of_range_is_none() {
        let mut logger = Logger::new();
        logger.add_log_raw(LogType::Information, &w("a"), 0);
        assert!(logger.get_log(1).is_none());
    }

    #[test]
    fn clear_keeps_serial_counter() {
        let mut logger = Logger::new();
        logger.add_log_raw(LogType::Information, &w("a"), 0);
        logger.add_log_raw(LogType::Information, &w("b"), 0);
        logger.clear();
        assert_eq!(logger.log_count(), 0);
        // 消去後も通し番号は続きから割り当てられる。
        logger.add_log_raw(LogType::Information, &w("c"), 0);
        assert_eq!(logger.get_log(0).unwrap().serial_number(), 2);
    }

    #[test]
    fn get_log_by_serial_number_after_clear() {
        let mut logger = Logger::new();
        logger.add_log_raw(LogType::Information, &w("a"), 0); // serial 0
        logger.add_log_raw(LogType::Information, &w("b"), 0); // serial 1
        logger.clear();
        logger.add_log_raw(LogType::Information, &w("c"), 0); // serial 2
        logger.add_log_raw(LogType::Information, &w("d"), 0); // serial 3

        // 先頭の通し番号は 2。範囲外(消去済みの 0/1)は None。
        assert!(logger.get_log_by_serial_number(0).is_none());
        assert!(logger.get_log_by_serial_number(1).is_none());
        assert_eq!(logger.get_log_by_serial_number(2).unwrap().text(), w("c").as_slice());
        assert_eq!(logger.get_log_by_serial_number(3).unwrap().text(), w("d").as_slice());
        // 範囲の上端より先は None。
        assert!(logger.get_log_by_serial_number(4).is_none());
    }

    #[test]
    fn get_log_by_serial_number_empty_is_none() {
        let logger = Logger::new();
        assert!(logger.get_log_by_serial_number(0).is_none());
    }

    #[test]
    fn output_to_file_flag() {
        let mut logger = Logger::new();
        assert!(!logger.output_to_file());
        logger.set_output_to_file(true);
        assert!(logger.output_to_file());
        logger.set_output_to_file(false);
        assert!(!logger.output_to_file());
    }

    #[test]
    fn log_items_returns_all() {
        let mut logger = Logger::new();
        logger.add_log_raw(LogType::Information, &w("a"), 0);
        logger.add_log_raw(LogType::Warning, &w("b"), 0);
        let items = logger.log_items();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].text(), w("a").as_slice());
        assert_eq!(items[1].log_type(), LogType::Warning);
    }
}
