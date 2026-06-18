// Rust port of LibISDB/Base/StreamWriter.cpp + StreamWriter.hpp
//
// StreamWriter はストリーム書き出しの基底クラス。C++ では実装 FileStreamWriter が
// FileStream(Win32 ファイル I/O)へ書き出す。
//
// 設計上の相違:
//   - C++ の純粋仮想クラス StreamWriter → Rust の trait `StreamWriter`。
//   - ErrorHandler(エラー状態保持)は省略(呼び出し側で扱う)。
//   - FileStreamWriter は FileStream(Win32)依存のため対象外とし、本クレートは
//     trait と、テスト/メモリ用途の `MemoryStreamWriter`(メモリ上に書き出す実装)を
//     提供する。SizeType(unsigned long long)→ u64。
//   - GetFileName(String*)→bool は Rust では `Option<String>` を返す形へ。

use bitflags::bitflags;

bitflags! {
    /// オープンフラグ。C++ `StreamWriter::OpenFlag`(StreamWriter.hpp:46)。
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct OpenFlag: u32 {
        /// 指定なし。
        const NONE = 0x0000;
        /// 上書き。
        const OVERWRITE = 0x0001;
    }
}

/// ストリーム書き出し抽象。C++ `StreamWriter`(StreamWriter.hpp:39)に対応する。
pub trait StreamWriter {
    /// ファイルを開く。既に開いていれば `false`。C++ `Open`。
    fn open(&mut self, file_name: &str, flags: OpenFlag) -> bool;

    /// 開き直す。新規オープンに成功してから旧ファイルを閉じる。C++ `Reopen`。
    fn reopen(&mut self, file_name: &str, flags: OpenFlag) -> bool;

    /// 閉じる。C++ `Close`。
    fn close(&mut self);

    /// 開いているか。C++ `IsOpen`。
    fn is_open(&self) -> bool;

    /// `buf` を書き込む。書き込んだバイト数を返す。C++ `Write`。
    fn write(&mut self, buf: &[u8]) -> usize;

    /// ファイル名を取得する。開いていない/空なら `None`。C++ `GetFileName`。
    fn get_file_name(&self) -> Option<String>;

    /// 累計書き込みバイト数。C++ `GetWriteSize`。
    fn get_write_size(&self) -> u64;

    /// 書き込みサイズが取得可能か。C++ `IsWriteSizeAvailable`。
    fn is_write_size_available(&self) -> bool;

    /// 事前割り当て単位を設定する。既定は非対応で `false`。C++ `SetPreallocationUnit`。
    fn set_preallocation_unit(&mut self, _unit: u64) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// MemoryStreamWriter — メモリ上へ書き出す StreamWriter(テスト/メモリ用途)
// ---------------------------------------------------------------------------

/// メモリ上のバッファへ書き出す `StreamWriter` 実装。
///
/// FileStreamWriter は FileStream を内部に持つが、こちらは `(ファイル名, バッファ)`を保持する。
/// メモリには既存ファイルが存在しないため `OpenFlag`(Overwrite/New)による分岐は観測されない
/// (受理はするが常に新規バッファを作る)。`write_size` は C++ 同様 Close/Reopen でリセットしない
/// (累計値が継続する)。
#[derive(Default)]
pub struct MemoryStreamWriter {
    file: Option<(String, Vec<u8>)>,
    write_size: u64,
}

impl MemoryStreamWriter {
    pub fn new() -> Self {
        Self::default()
    }

    /// 現在開いているバッファの中身を参照する(テスト用)。
    pub fn data(&self) -> Option<&[u8]> {
        self.file.as_ref().map(|(_, buf)| buf.as_slice())
    }
}

impl StreamWriter for MemoryStreamWriter {
    // StreamWriter.cpp:48
    fn open(&mut self, file_name: &str, _flags: OpenFlag) -> bool {
        if self.file.is_some() {
            return false;
        }
        self.file = Some((file_name.to_string(), Vec::new()));
        self.write_size = 0;
        true
    }

    // StreamWriter.cpp:66 — 新規オープン成功後に旧ファイルを閉じる。write_size はリセットしない。
    fn reopen(&mut self, file_name: &str, _flags: OpenFlag) -> bool {
        self.file = Some((file_name.to_string(), Vec::new()));
        true
    }

    // StreamWriter.cpp:81
    fn close(&mut self) {
        self.file = None;
    }

    // StreamWriter.cpp:90
    fn is_open(&self) -> bool {
        self.file.is_some()
    }

    // StreamWriter.cpp:96
    fn write(&mut self, buf: &[u8]) -> usize {
        match self.file.as_mut() {
            Some((_, data)) => {
                data.extend_from_slice(buf);
                self.write_size += buf.len() as u64;
                buf.len()
            }
            None => 0,
        }
    }

    // StreamWriter.cpp:110
    fn get_file_name(&self) -> Option<String> {
        match self.file.as_ref() {
            Some((name, _)) if !name.is_empty() => Some(name.clone()),
            _ => None,
        }
    }

    // StreamWriter.cpp:126
    fn get_write_size(&self) -> u64 {
        self.write_size
    }

    // StreamWriter.cpp:132
    fn is_write_size_available(&self) -> bool {
        self.file.is_some()
    }

    // メモリ実装は事前割り当てに対応しない。
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_close_state() {
        let mut w = MemoryStreamWriter::new();
        assert!(!w.is_open());
        assert!(w.open("a.ts", OpenFlag::NONE));
        assert!(w.is_open());
        assert!(w.is_write_size_available());
        w.close();
        assert!(!w.is_open());
        assert!(!w.is_write_size_available());
    }

    #[test]
    fn test_open_twice_fails() {
        let mut w = MemoryStreamWriter::new();
        assert!(w.open("a.ts", OpenFlag::NONE));
        // 既に開いていれば false
        assert!(!w.open("b.ts", OpenFlag::OVERWRITE));
        // 元のファイル名のまま
        assert_eq!(w.get_file_name().as_deref(), Some("a.ts"));
    }

    #[test]
    fn test_write_accumulates() {
        let mut w = MemoryStreamWriter::new();
        w.open("a.ts", OpenFlag::NONE);
        assert_eq!(w.write(&[1, 2, 3]), 3);
        assert_eq!(w.write(&[4, 5]), 2);
        assert_eq!(w.get_write_size(), 5);
        assert_eq!(w.data(), Some(&[1u8, 2, 3, 4, 5][..]));
    }

    #[test]
    fn test_write_when_closed_returns_zero() {
        let mut w = MemoryStreamWriter::new();
        assert_eq!(w.write(&[1, 2, 3]), 0);
        assert_eq!(w.get_write_size(), 0);
    }

    #[test]
    fn test_open_resets_write_size() {
        let mut w = MemoryStreamWriter::new();
        w.open("a.ts", OpenFlag::NONE);
        w.write(&[0; 10]);
        assert_eq!(w.get_write_size(), 10);
        w.close();
        // 再オープンは write_size を 0 に戻す(C++ Open と同じ)
        assert!(w.open("b.ts", OpenFlag::NONE));
        assert_eq!(w.get_write_size(), 0);
    }

    #[test]
    fn test_reopen_keeps_write_size_and_resets_buffer() {
        let mut w = MemoryStreamWriter::new();
        w.open("a.ts", OpenFlag::NONE);
        w.write(&[1, 2, 3]);
        assert_eq!(w.get_write_size(), 3);
        // Reopen は write_size をリセットせず、バッファだけ作り直す(C++ 同様)
        assert!(w.reopen("b.ts", OpenFlag::OVERWRITE));
        assert_eq!(w.get_file_name().as_deref(), Some("b.ts"));
        assert_eq!(w.data(), Some(&[][..]));
        w.write(&[9]);
        assert_eq!(w.get_write_size(), 4);
    }

    #[test]
    fn test_get_file_name_when_closed_is_none() {
        let w = MemoryStreamWriter::new();
        assert_eq!(w.get_file_name(), None);
    }

    #[test]
    fn test_set_preallocation_unit_default_false() {
        let mut w = MemoryStreamWriter::new();
        w.open("a.ts", OpenFlag::NONE);
        assert!(!w.set_preallocation_unit(1024 * 1024));
    }
}
