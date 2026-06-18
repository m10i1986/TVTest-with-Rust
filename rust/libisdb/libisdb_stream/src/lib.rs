// Rust port of LibISDB/Base/Stream.hpp
//
// Stream は読み書き可能なバイトストリームの抽象基底(C++ では ErrorHandler 継承)。
// FileStreamBase(Win32 ファイル I/O)はプラットフォーム依存のため対象外とし、本クレートは
// 抽象 trait `Stream` と、テスト/メモリ用途の `MemoryStream` 実装を提供する。
//
// 設計上の相違:
//   - C++ の純粋仮想クラス → Rust の trait
//   - ErrorHandler(エラー状態保持)は省略(呼び出し側で扱う)
//   - Read/Write は void* + size → &mut [u8] / &[u8](戻り値は処理バイト数)

/// シーク基準。C++ `Stream::SetPosType`(Stream.hpp:50)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetPosType {
    Begin,
    Current,
    End,
}

/// バイトストリーム抽象。C++ `Stream`(Stream.hpp:38)に対応する。
pub trait Stream {
    /// ストリームを閉じる。C++ `Close`。
    fn close(&mut self) -> bool;

    /// 開いているか。C++ `IsOpen`。
    fn is_open(&self) -> bool;

    /// `buf` へ読み込む。読み込んだバイト数を返す。C++ `Read`。
    fn read(&mut self, buf: &mut [u8]) -> usize;

    /// `buf` を書き込む。書き込んだバイト数を返す。C++ `Write`。
    fn write(&mut self, buf: &[u8]) -> usize;

    /// バッファをフラッシュする。C++ `Flush`。
    fn flush(&mut self) -> bool;

    /// ストリーム全体のサイズ。C++ `GetSize`。
    fn get_size(&mut self) -> u64;

    /// 現在位置。C++ `GetPos`。
    fn get_pos(&mut self) -> i64;

    /// 位置を設定する。C++ `SetPos`。
    fn set_pos(&mut self, pos: i64, pos_type: SetPosType) -> bool;

    /// 末尾に達しているか。C++ `IsEnd`。
    fn is_end(&self) -> bool;
}

// ---------------------------------------------------------------------------
// MemoryStream — メモリ上のバイトストリーム(テスト/メモリ用途)
// ---------------------------------------------------------------------------

/// メモリ上のバイト列を読み書きする `Stream` 実装。
pub struct MemoryStream {
    data: Vec<u8>,
    pos: usize,
    open: bool,
}

impl MemoryStream {
    /// 既存データを読み出すストリームを作る。
    pub fn new(data: Vec<u8>) -> Self {
        Self { data, pos: 0, open: true }
    }

    /// 空のストリームを作る(書き込み用)。
    pub fn empty() -> Self {
        Self::new(Vec::new())
    }

    /// 現在の内部バッファを参照する。
    pub fn data(&self) -> &[u8] {
        &self.data
    }
}

impl Stream for MemoryStream {
    fn close(&mut self) -> bool {
        self.open = false;
        true
    }

    fn is_open(&self) -> bool {
        self.open
    }

    fn read(&mut self, buf: &mut [u8]) -> usize {
        if !self.open || buf.is_empty() {
            return 0;
        }
        let avail = self.data.len().saturating_sub(self.pos);
        let n = avail.min(buf.len());
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        n
    }

    fn write(&mut self, buf: &[u8]) -> usize {
        if !self.open || buf.is_empty() {
            return 0;
        }
        let end = self.pos + buf.len();
        if end > self.data.len() {
            self.data.resize(end, 0);
        }
        self.data[self.pos..end].copy_from_slice(buf);
        self.pos = end;
        buf.len()
    }

    fn flush(&mut self) -> bool {
        true
    }

    fn get_size(&mut self) -> u64 {
        self.data.len() as u64
    }

    fn get_pos(&mut self) -> i64 {
        self.pos as i64
    }

    fn set_pos(&mut self, pos: i64, pos_type: SetPosType) -> bool {
        let base = match pos_type {
            SetPosType::Begin => 0i64,
            SetPosType::Current => self.pos as i64,
            SetPosType::End => self.data.len() as i64,
        };
        let new_pos = base + pos;
        if new_pos < 0 {
            return false;
        }
        // ファイル同様、末尾より先へのシークも許容する。
        self.pos = new_pos as usize;
        true
    }

    fn is_end(&self) -> bool {
        self.pos >= self.data.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_sequential() {
        let mut s = MemoryStream::new(vec![1, 2, 3, 4, 5]);
        assert!(s.is_open());
        let mut buf = [0u8; 3];
        assert_eq!(s.read(&mut buf), 3);
        assert_eq!(buf, [1, 2, 3]);
        assert!(!s.is_end());
        assert_eq!(s.read(&mut buf), 2);
        assert_eq!(&buf[..2], &[4, 5]);
        assert!(s.is_end());
        assert_eq!(s.read(&mut buf), 0);
    }

    #[test]
    fn test_read_empty_buffer() {
        let mut s = MemoryStream::new(vec![1, 2, 3]);
        let mut empty: [u8; 0] = [];
        assert_eq!(s.read(&mut empty), 0);
    }

    #[test]
    fn test_is_end_on_empty_stream() {
        let s = MemoryStream::empty();
        assert!(s.is_end());
    }

    #[test]
    fn test_write_then_read_back() {
        let mut s = MemoryStream::empty();
        assert_eq!(s.write(&[10, 20, 30]), 3);
        assert_eq!(s.get_size(), 3);
        assert_eq!(s.get_pos(), 3);
        assert!(s.set_pos(0, SetPosType::Begin));
        let mut buf = [0u8; 3];
        assert_eq!(s.read(&mut buf), 3);
        assert_eq!(buf, [10, 20, 30]);
    }

    #[test]
    fn test_set_pos_variants() {
        let mut s = MemoryStream::new(vec![0; 10]);
        assert!(s.set_pos(3, SetPosType::Begin));
        assert_eq!(s.get_pos(), 3);
        assert!(s.set_pos(2, SetPosType::Current));
        assert_eq!(s.get_pos(), 5);
        assert!(s.set_pos(-1, SetPosType::End));
        assert_eq!(s.get_pos(), 9);
        // 負の位置は不可
        assert!(!s.set_pos(-1, SetPosType::Begin));
        assert_eq!(s.get_pos(), 9);
    }

    #[test]
    fn test_close_blocks_io() {
        let mut s = MemoryStream::new(vec![1, 2, 3]);
        assert!(s.close());
        assert!(!s.is_open());
        let mut buf = [0u8; 3];
        assert_eq!(s.read(&mut buf), 0);
        assert_eq!(s.write(&[9]), 0);
    }

    #[test]
    fn test_get_size() {
        let mut s = MemoryStream::new(vec![0; 42]);
        assert_eq!(s.get_size(), 42);
    }
}
