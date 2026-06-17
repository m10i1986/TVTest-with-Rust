// Rust port of LibISDB/Base/DataStorage.cpp + DataStorageManager.cpp
//
// Ported types:
//   DataStorage       -> pub trait DataStorage     (DataStorage.hpp:40)
//   MemoryDataStorage -> pub struct MemoryDataStorage (DataStorage.hpp:61)
//   DataStorageManager       -> pub trait DataStorageManager (DataStorageManager.hpp:38)
//   MemoryDataStorageManager -> pub struct MemoryDataStorageManager (DataStorageManager.hpp:47)
//
// Deferred (OS-dependent):
//   StreamDataStorage, FileDataStorage (depend on FileStream / platform file I/O)
//
// Divergences:
//   - SizeType = Stream::SizeType = unsigned long long (u64 on Windows). Rust uses u64.
//   - MemoryDataStorage uses Vec<u8> directly instead of DataBuffer, because GetBuffer()
//     in C++ returns the full-capacity raw pointer for write access, while our Rust
//     DataBuffer only exposes the data region. The observable behaviour is identical.
//   - Allocate() resets data_size and pos (C++ AllocateBuffer does not, but Allocate is
//     always called once on a freshly created object in StreamBuffer's usage).
//   - Buffer is zero-initialised on Allocate (C++ malloc does not zero-fill; safe side).

/// SizeType = Stream::SizeType (DataStorage.hpp:43)
/// On Windows: unsigned long long = u64.
pub type SizeType = u64;

// ---------------------------------------------------------------------------
// DataStorage trait (DataStorage.hpp:40, DataStorage.cpp:36-51)
// ---------------------------------------------------------------------------

/// データストレージ基底トレイト。
/// デフォルト実装 (is_allocated / is_full / is_end) は原実装の DataStorage.cpp:36-51 に対応。
pub trait DataStorage: Send {
    fn allocate(&mut self, size: SizeType) -> bool;
    fn free(&mut self);
    fn get_capacity(&self) -> SizeType;
    fn get_data_size(&self) -> SizeType;
    fn read(&mut self, buf: &mut [u8]) -> usize;
    fn write(&mut self, data: &[u8]) -> usize;
    fn set_pos(&mut self, pos: SizeType) -> bool;
    fn get_pos(&self) -> SizeType;

    // DataStorage.cpp:36-38
    fn is_allocated(&self) -> bool {
        self.get_capacity() > 0
    }
    // DataStorage.cpp:42-44
    fn is_full(&self) -> bool {
        self.get_capacity() <= self.get_data_size()
    }
    // DataStorage.cpp:48-50
    fn is_end(&self) -> bool {
        self.get_capacity() <= self.get_pos()
    }
}

// ---------------------------------------------------------------------------
// MemoryDataStorage (DataStorage.hpp:61, DataStorage.cpp:56-124)
// ---------------------------------------------------------------------------

/// メモリ上のデータストレージ。
///
/// `buffer.len()` = allocated capacity (= m_BufferSize 相当)
/// `data_size`    = written data size (= m_Buffer.GetSize() 相当)
/// `pos`          = current read/write cursor (= m_Pos)
#[derive(Debug, Default)]
pub struct MemoryDataStorage {
    buffer: Vec<u8>,
    data_size: usize,
    pos: usize,
}

impl MemoryDataStorage {
    pub fn new() -> Self {
        Self::default()
    }
}

impl DataStorage for MemoryDataStorage {
    // DataStorage.cpp:56-61
    fn allocate(&mut self, size: SizeType) -> bool {
        if size > usize::MAX as SizeType {
            return false;
        }
        let size = size as usize;
        self.buffer = vec![0u8; size];
        self.data_size = 0;
        self.pos = 0;
        true
    }

    // DataStorage.cpp:64-67
    fn free(&mut self) {
        *self = Self::default();
    }

    // DataStorage.cpp:71-73
    fn get_capacity(&self) -> SizeType {
        self.buffer.len() as SizeType
    }

    // DataStorage.cpp:77-79
    fn get_data_size(&self) -> SizeType {
        self.data_size as SizeType
    }

    // DataStorage.cpp:83-93
    // Read は data_size までに制限 (GetSize() = m_DataSize が上限)。
    fn read(&mut self, buf: &mut [u8]) -> usize {
        if self.pos >= self.data_size {
            return 0;
        }
        let copy = buf.len().min(self.data_size - self.pos);
        buf[..copy].copy_from_slice(&self.buffer[self.pos..self.pos + copy]);
        self.pos += copy;
        copy
    }

    // DataStorage.cpp:96-108
    // Write は capacity (GetBufferSize()) までに制限。書き込み後 data_size を必要に応じて更新。
    fn write(&mut self, data: &[u8]) -> usize {
        let capacity = self.buffer.len();
        if self.pos >= capacity {
            return 0;
        }
        let copy = data.len().min(capacity - self.pos);
        self.buffer[self.pos..self.pos + copy].copy_from_slice(&data[..copy]);
        self.pos += copy;
        if self.data_size < self.pos {
            self.data_size = self.pos; // DataStorage.cpp:104-105: m_Buffer.SetSize(m_Pos)
        }
        copy
    }

    // DataStorage.cpp:111-118
    fn set_pos(&mut self, pos: SizeType) -> bool {
        if pos > usize::MAX as SizeType {
            return false;
        }
        let pos = pos as usize;
        if pos > self.buffer.len() {
            return false;
        }
        self.pos = pos;
        true
    }

    // DataStorage.cpp:122-124
    fn get_pos(&self) -> SizeType {
        self.pos as SizeType
    }
}

// ---------------------------------------------------------------------------
// DataStorageManager trait (DataStorageManager.hpp:38)
// ---------------------------------------------------------------------------

pub trait DataStorageManager: Send {
    fn create_data_storage(&self) -> Box<dyn DataStorage>;
}

// ---------------------------------------------------------------------------
// MemoryDataStorageManager (DataStorageManager.hpp:47, DataStorageManager.cpp:36-39)
// ---------------------------------------------------------------------------

pub struct MemoryDataStorageManager;

impl DataStorageManager for MemoryDataStorageManager {
    fn create_data_storage(&self) -> Box<dyn DataStorage> {
        Box::new(MemoryDataStorage::new())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_is_unallocated() {
        let s = MemoryDataStorage::new();
        assert_eq!(s.get_capacity(), 0);
        assert_eq!(s.get_data_size(), 0);
        assert_eq!(s.get_pos(), 0);
        assert!(!s.is_allocated());
    }

    #[test]
    fn test_allocate() {
        let mut s = MemoryDataStorage::new();
        assert!(s.allocate(100));
        assert_eq!(s.get_capacity(), 100);
        assert_eq!(s.get_data_size(), 0);
        assert_eq!(s.get_pos(), 0);
        assert!(s.is_allocated());
        assert!(!s.is_full());
        assert!(!s.is_end());
    }

    #[test]
    fn test_write_then_read() {
        let mut s = MemoryDataStorage::new();
        s.allocate(10);
        let written = s.write(&[1, 2, 3]);
        assert_eq!(written, 3);
        assert_eq!(s.get_pos(), 3);
        assert_eq!(s.get_data_size(), 3);

        s.set_pos(0);
        let mut buf = [0u8; 3];
        let read = s.read(&mut buf);
        assert_eq!(read, 3);
        assert_eq!(buf, [1, 2, 3]);
    }

    #[test]
    fn test_write_limited_by_capacity() {
        let mut s = MemoryDataStorage::new();
        s.allocate(4);
        let written = s.write(&[1, 2, 3, 4, 5, 6]);
        assert_eq!(written, 4);
        assert_eq!(s.get_pos(), 4);
        assert_eq!(s.get_data_size(), 4);
    }

    #[test]
    fn test_write_at_capacity_returns_zero() {
        let mut s = MemoryDataStorage::new();
        s.allocate(3);
        s.write(&[1, 2, 3]);
        let written = s.write(&[4]);
        assert_eq!(written, 0);
    }

    #[test]
    fn test_read_limited_by_data_size() {
        let mut s = MemoryDataStorage::new();
        s.allocate(10);
        s.write(&[1, 2, 3]);
        s.set_pos(0);
        let mut buf = [0u8; 10];
        let read = s.read(&mut buf);
        assert_eq!(read, 3);
        assert_eq!(&buf[..3], &[1, 2, 3]);
    }

    #[test]
    fn test_read_at_data_size_returns_zero() {
        let mut s = MemoryDataStorage::new();
        s.allocate(10);
        s.write(&[1, 2, 3]);
        // pos == data_size → read returns 0
        let mut buf = [0u8; 5];
        let read = s.read(&mut buf);
        assert_eq!(read, 0);
    }

    #[test]
    fn test_set_pos_seek() {
        let mut s = MemoryDataStorage::new();
        s.allocate(10);
        s.write(&[10, 20, 30, 40, 50]);
        s.set_pos(2);
        let mut buf = [0u8; 3];
        let read = s.read(&mut buf);
        assert_eq!(read, 3);
        assert_eq!(buf, [30, 40, 50]);
    }

    #[test]
    fn test_set_pos_boundary() {
        let mut s = MemoryDataStorage::new();
        s.allocate(10);
        assert!(s.set_pos(10));  // at capacity: ok (DataStorage.cpp:113: Pos > GetBufferSize)
        assert!(!s.set_pos(11)); // beyond capacity: fails
    }

    #[test]
    fn test_is_full_and_is_end() {
        let mut s = MemoryDataStorage::new();
        s.allocate(3);
        s.write(&[1, 2, 3]);
        assert!(s.is_full()); // get_capacity(3) <= get_data_size(3)
        assert!(s.is_end());  // get_capacity(3) <= get_pos(3)
    }

    #[test]
    fn test_free() {
        let mut s = MemoryDataStorage::new();
        s.allocate(100);
        s.write(&[1, 2, 3]);
        s.free();
        assert_eq!(s.get_capacity(), 0);
        assert_eq!(s.get_data_size(), 0);
        assert_eq!(s.get_pos(), 0);
        assert!(!s.is_allocated());
    }

    // StreamBuffer の QueueBlock が行う Write → Reuse(SetPos(0)) → Write → Read パターン
    #[test]
    fn test_reuse_pattern() {
        let mut s = MemoryDataStorage::new();
        s.allocate(10);

        // 1st write
        let w1 = s.write(&[1, 2, 3, 4, 5]);
        assert_eq!(w1, 5);
        let write_pos = s.get_pos(); // 5 (= QueueBlock::GetDataSize via GetPos)

        // QueueBlock::Read simulation: save pos, seek to 0, read, restore pos
        s.set_pos(0);
        let mut buf = [0u8; 5];
        let r1 = s.read(&mut buf);
        assert_eq!(r1, 5);
        assert_eq!(buf, [1, 2, 3, 4, 5]);
        s.set_pos(write_pos);

        // Reuse: QueueBlock::Reuse() -> m_Storage->SetPos(0)
        assert!(s.set_pos(0));

        // 2nd write after reuse
        let w2 = s.write(&[10, 20]);
        assert_eq!(w2, 2);
        assert_eq!(s.get_pos(), 2);

        // Read after reuse (QueueBlock::Read with Pos=2)
        s.set_pos(0);
        let mut buf2 = [0u8; 2];
        let r2 = s.read(&mut buf2);
        assert_eq!(r2, 2);
        assert_eq!(buf2, [10, 20]);
        s.set_pos(2);
    }

    #[test]
    fn test_memory_data_storage_manager() {
        let manager = MemoryDataStorageManager;
        let mut storage = manager.create_data_storage();
        assert!(!storage.is_allocated());
        assert!(storage.allocate(50));
        assert_eq!(storage.get_capacity(), 50);
        assert!(storage.is_allocated());
        assert!(!storage.is_full());

        let written = storage.write(&[0xAB; 50]);
        assert_eq!(written, 50);
        assert!(storage.is_full());
        assert!(storage.is_end());
    }
}
