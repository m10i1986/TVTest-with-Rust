// Rust port of LibISDB/Base/StreamBuffer.cpp
//
// Ported types:
//   StreamBuffer           -> pub struct StreamBuffer     (StreamBuffer.hpp:43)
//   StreamBuffer::Reader   -> pub trait Reader            (StreamBuffer.hpp:51)
//   StreamBuffer::SequentialReader -> pub struct SequentialReader (StreamBuffer.hpp:69)
//   StreamBuffer::QueueBlock -> (private) struct QueueBlock
//
// Threading:
//   C++ uses MutexLock (recursive mutex). Rust uses Mutex<StreamBufferInner>;
//   SetSize calls push_back internally (same Mutex), so push_back is implemented
//   as a method on StreamBufferInner to avoid re-locking.
//
// Reader identity:
//   C++ keyed m_ReaderPosList by Reader* pointer. Rust uses a per-reader u64 ID
//   from an AtomicU64 counter.
//
// QueueBlock::GetDataSize():
//   Returns GetPos() (the write cursor), NOT GetDataSize(). This is intentional:
//   after Reuse (SetPos(0)), the write cursor resets while data_size tracks the
//   high watermark.

use libisdb_data_storage::{DataStorage, DataStorageManager, MemoryDataStorageManager, SizeType};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub type PosType = i64;

pub const POS_BEGIN: PosType = -1;   // StreamBuffer.hpp:48
pub const POS_INVALID: PosType = -2; // StreamBuffer.hpp:49

static READER_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// QueueBlock (StreamBuffer.hpp:110, StreamBuffer.cpp:469-590)
// ---------------------------------------------------------------------------

struct QueueBlock {
    storage: Option<Box<dyn DataStorage>>,
    serial_pos: PosType,
}

impl QueueBlock {
    fn new() -> Self {
        Self { storage: None, serial_pos: POS_INVALID }
    }

    fn set_storage(&mut self, storage: Box<dyn DataStorage>) {
        self.storage = Some(storage);
    }

    // StreamBuffer.cpp:512-516
    #[allow(dead_code)]
    fn free(&mut self) {
        if let Some(s) = &mut self.storage {
            s.free();
        }
    }

    // StreamBuffer.cpp:519-524
    fn reuse(&mut self) {
        if let Some(s) = &mut self.storage {
            s.set_pos(0);
        }
        self.serial_pos = POS_INVALID;
    }

    // StreamBuffer.cpp:527-543
    // Write は capacity-pos バイトに制限。
    fn write(&mut self, data: &[u8]) -> usize {
        let s = match &mut self.storage {
            Some(s) => s,
            None => return 0,
        };
        let capacity = s.get_capacity();
        let pos = s.get_pos();
        if pos >= capacity {
            return 0;
        }
        let write_size = data.len().min((capacity - pos) as usize);
        if write_size > 0 {
            s.write(&data[..write_size])
        } else {
            0
        }
    }

    // StreamBuffer.cpp:546-566
    // Read は [Offset, GetPos()) の範囲に制限し、終了後 GetPos() を復元。
    fn read(&mut self, offset: usize, buf: &mut [u8]) -> usize {
        let s = match &mut self.storage {
            Some(s) => s,
            None => return 0,
        };
        let write_pos: SizeType = s.get_pos();
        if offset as SizeType >= write_pos {
            return 0;
        }
        if !s.set_pos(offset as SizeType) {
            return 0;
        }
        let read_size = buf.len().min((write_pos - offset as SizeType) as usize);
        let n = if read_size > 0 {
            s.read(&mut buf[..read_size])
        } else {
            0
        };
        let _ = s.set_pos(write_pos); // restore write cursor
        n
    }

    // StreamBuffer.cpp:569-573
    fn get_capacity(&self) -> usize {
        self.storage.as_ref().map(|s| s.get_capacity() as usize).unwrap_or(0)
    }

    // StreamBuffer.cpp:576-581
    // NOTE: GetDataSize returns GetPos() (write cursor), not storage.get_data_size().
    fn get_data_size(&self) -> usize {
        self.storage.as_ref().map(|s| s.get_pos() as usize).unwrap_or(0)
    }

    // StreamBuffer.cpp:584-590
    fn is_full(&self) -> bool {
        self.storage.as_ref().map(|s| s.is_end()).unwrap_or(true)
    }

    #[allow(dead_code)]
    fn get_serial_pos(&self) -> PosType {
        self.serial_pos
    }

    fn set_serial_pos(&mut self, pos: PosType) {
        self.serial_pos = pos;
    }
}

// ---------------------------------------------------------------------------
// StreamBufferInner — mutex-protected state
// ---------------------------------------------------------------------------

struct StreamBufferInner {
    block_size: usize,
    min_block_count: usize,
    max_block_count: usize,
    queue: VecDeque<QueueBlock>,
    serial_pos: PosType,
    data_storage_manager: Option<Box<dyn DataStorageManager>>,
    reader_pos_list: HashMap<u64, PosType>,
}

impl StreamBufferInner {
    fn new() -> Self {
        Self {
            block_size: 0,
            min_block_count: 0,
            max_block_count: 0,
            queue: VecDeque::new(),
            serial_pos: 0,
            data_storage_manager: None,
            reader_pos_list: HashMap::new(),
        }
    }

    // StreamBuffer.cpp:455-464
    fn check_buffer_size(block_size: usize, min_block_count: usize, max_block_count: usize) -> bool {
        if block_size == 0 || max_block_count == 0 {
            return false;
        }
        if block_size > usize::MAX / max_block_count {
            return false;
        }
        if min_block_count > max_block_count {
            return false;
        }
        true
    }

    // StreamBuffer.cpp:388-396
    // ブロックの serial_pos から end(serial_pos + capacity) までに reader_pos が含まれれば locked。
    fn is_block_locked(&self, block: &QueueBlock) -> bool {
        let limit = block.serial_pos + block.get_capacity() as PosType;
        for &pos in self.reader_pos_list.values() {
            if pos >= 0 && pos < limit {
                return true;
            }
        }
        false
    }

    // StreamBuffer.cpp:399-409
    fn free_unused_blocks(&mut self) {
        if self.min_block_count < self.max_block_count
            && self.queue.len() > self.min_block_count
        {
            loop {
                if self.queue.len() <= self.min_block_count {
                    break;
                }
                let locked = match self.queue.front() {
                    Some(b) => self.is_block_locked(b),
                    None => break,
                };
                if locked {
                    break;
                }
                self.queue.pop_front();
            }
        }
    }

    // StreamBuffer.cpp:294-307
    fn get_block_index_by_serial_pos(&self, pos: PosType) -> Option<usize> {
        if self.queue.is_empty() {
            return None;
        }
        let first = self.queue.front().unwrap().serial_pos;
        if pos < first {
            return None;
        }
        let index = ((pos - first) as usize) / self.block_size;
        if index >= self.queue.len() {
            return None;
        }
        Some(index)
    }

    // StreamBuffer.cpp:224-282 (PushBack)
    fn push_back(&mut self, data: &[u8]) -> usize {
        if data.is_empty() || self.block_size == 0 {
            return 0;
        }

        let mut pos = 0usize;

        // 末尾ブロックに空きがあれば先に書き込む (StreamBuffer.cpp:236-245)
        if let Some(last) = self.queue.back_mut() {
            if !last.is_full() {
                let copy = last.write(&data[pos..]);
                self.serial_pos += copy as PosType;
                if copy == data.len() - pos || !last.is_full() {
                    return pos + copy;
                }
                pos += copy;
            }
        }

        loop {
            let mut block = QueueBlock::new();

            if self.queue.len() < self.max_block_count {
                // 新規ブロックを作成 (StreamBuffer.cpp:250-259)
                let storage = match &mut self.data_storage_manager {
                    Some(m) => m.create_data_storage(),
                    None => break,
                };
                let mut storage = storage;
                if !storage.allocate(self.block_size as SizeType) {
                    break;
                }
                block.set_storage(storage);
            } else {
                // 先頭ブロックを再利用 (StreamBuffer.cpp:261-267)
                let front_locked = match self.queue.front() {
                    Some(b) => self.is_block_locked(b),
                    None => true,
                };
                if front_locked {
                    break;
                }
                block = self.queue.pop_front().unwrap();
                block.reuse();
            }

            let copy = block.write(&data[pos..]);
            pos += copy;
            let is_full = block.is_full();

            let spos = self.serial_pos;
            block.set_serial_pos(spos);
            self.serial_pos += copy as PosType;
            self.queue.push_back(block);

            if pos >= data.len() {
                break;
            }
            if !is_full {
                break; // データが途切れた (通常はここに到達しない)
            }
        }

        pos
    }

    // StreamBuffer.cpp:412-452 (Read internal)
    fn read(&mut self, pos: &mut PosType, buf: &mut [u8]) -> usize {
        let size = buf.len();
        let (mut it_idx, mut offset) = match self.get_block_index_by_serial_pos(*pos) {
            None => {
                if self.queue.is_empty()
                    || *pos > self.queue.front().unwrap().serial_pos
                {
                    return 0;
                }
                *pos = self.queue.front().unwrap().serial_pos;
                (0usize, 0usize)
            }
            Some(idx) => {
                let off = (*pos - self.queue[idx].serial_pos) as usize;
                if self.queue[idx].get_data_size() <= off {
                    (idx + 1, 0usize)
                } else {
                    (idx, off)
                }
            }
        };

        let mut read_size = 0usize;

        while it_idx < self.queue.len() && read_size < size {
            let copy = {
                let block = &mut self.queue[it_idx];
                block.read(offset, &mut buf[read_size..size])
            };
            read_size += copy;
            *pos = self.queue[it_idx].serial_pos + offset as PosType + copy as PosType;
            offset = 0;
            it_idx += 1;
        }

        read_size
    }

    // StreamBuffer.cpp:349-357
    fn get_begin_pos(&self) -> PosType {
        self.queue
            .front()
            .map(|b| b.serial_pos)
            .unwrap_or(self.serial_pos)
    }

    // StreamBuffer.cpp:360-368
    fn get_end_pos(&self) -> PosType {
        self.queue
            .back()
            .map(|b| b.serial_pos + b.get_data_size() as PosType)
            .unwrap_or(self.serial_pos)
    }

    // StreamBuffer.cpp:371-385
    fn get_data_range(&self) -> Option<(PosType, PosType)> {
        if self.queue.is_empty() {
            return None;
        }
        let begin = self.queue.front().unwrap().serial_pos;
        let back = self.queue.back().unwrap();
        let end = back.serial_pos + back.get_data_size() as PosType;
        Some((begin, end))
    }
}

// ---------------------------------------------------------------------------
// StreamBuffer (StreamBuffer.hpp:43)
// ---------------------------------------------------------------------------

/// スレッドセーフなリングキュー型ストリームバッファ。
///
/// ブロック単位でデータを蓄積し、複数の `SequentialReader` が独立して読み進められる。
pub struct StreamBuffer {
    inner: Mutex<StreamBufferInner>,
}

impl Default for StreamBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamBuffer {
    pub fn new() -> Self {
        Self { inner: Mutex::new(StreamBufferInner::new()) }
    }

    // StreamBuffer.cpp:51-76
    pub fn create(
        &self,
        block_size: usize,
        min_block_count: usize,
        max_block_count: usize,
        manager: Option<Box<dyn DataStorageManager>>,
    ) -> bool {
        if !StreamBufferInner::check_buffer_size(block_size, min_block_count, max_block_count) {
            return false;
        }
        let mut inner = self.inner.lock().unwrap();
        inner.block_size = block_size;
        inner.min_block_count = min_block_count;
        inner.max_block_count = max_block_count;
        inner.queue.clear();
        inner.serial_pos = 0;
        inner.data_storage_manager =
            Some(manager.unwrap_or_else(|| Box::new(MemoryDataStorageManager)));
        true
    }

    // StreamBuffer.cpp:79-89
    pub fn destroy(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.queue.clear();
        inner.block_size = 0;
        inner.min_block_count = 0;
        inner.max_block_count = 0;
        inner.serial_pos = 0;
        inner.data_storage_manager = None;
    }

    // StreamBuffer.cpp:92-95
    pub fn is_created(&self) -> bool {
        self.inner.lock().unwrap().block_size > 0
    }

    // StreamBuffer.cpp:98-103
    pub fn clear(&self) {
        self.inner.lock().unwrap().queue.clear();
    }

    // StreamBuffer.cpp:106-161
    pub fn set_size(
        &self,
        block_size: usize,
        min_block_count: usize,
        max_block_count: usize,
        discard: bool,
    ) -> bool {
        if !StreamBufferInner::check_buffer_size(block_size, min_block_count, max_block_count) {
            return false;
        }
        let mut inner = self.inner.lock().unwrap();

        if inner.block_size != block_size {
            inner.block_size = block_size;
            inner.min_block_count = min_block_count;
            inner.max_block_count = max_block_count;

            if !inner.queue.is_empty() {
                // キューを取り出し、新サイズに収まる末尾データを再 push_back する
                let old_queue: Vec<QueueBlock> = inner.queue.drain(..).collect();
                let max_size = block_size * max_block_count;

                // 末尾から累積して収まる最初のインデックスを探す
                let mut total = 0usize;
                let mut start = 0usize;
                for i in (0..old_queue.len()).rev() {
                    let ds = old_queue[i].get_data_size();
                    if ds > max_size.saturating_sub(total) {
                        start = i + 1;
                        break;
                    }
                    total += ds;
                }

                // 再 push_back (inner.push_back はロック不要 = StreamBufferInner メソッド)
                let mut tmp: Vec<u8> = Vec::new();
                for mut block in old_queue.into_iter().skip(start) {
                    let ds = block.get_data_size();
                    if ds == 0 {
                        continue;
                    }
                    tmp.resize(ds, 0u8);
                    let n = block.read(0, &mut tmp[..ds]);
                    if n > 0 {
                        inner.push_back(&tmp[..n]);
                    }
                }
            }
        } else if inner.max_block_count != max_block_count {
            inner.max_block_count = max_block_count;
            if discard {
                while inner.queue.len() > max_block_count {
                    inner.queue.pop_front();
                }
            }
        }

        inner.min_block_count = min_block_count;
        true
    }

    // StreamBuffer.cpp:164-169
    pub fn is_empty(&self) -> bool {
        self.inner.lock().unwrap().queue.is_empty()
    }

    // StreamBuffer.cpp:172-182
    pub fn is_full(&self) -> bool {
        let inner = self.inner.lock().unwrap();
        if inner.max_block_count == 0 {
            return true;
        }
        if inner.queue.len() < inner.max_block_count {
            return false;
        }
        inner.queue.back().map(|b| b.is_full()).unwrap_or(false)
    }

    // StreamBuffer.cpp:185-221
    pub fn get_free_space(&self) -> usize {
        let inner = self.inner.lock().unwrap();
        let mut free = 0usize;

        if inner.queue.len() < inner.max_block_count {
            free += (inner.max_block_count - inner.queue.len()) * inner.block_size;
        }

        if inner.queue.len() >= 2 {
            let discardable = if inner.queue.len() > inner.min_block_count {
                (inner.queue.len() - inner.min_block_count).min(inner.queue.len() - 1)
            } else {
                1usize.min(inner.queue.len() - 1)
            };
            for i in 0..discardable {
                if inner.is_block_locked(&inner.queue[i]) {
                    break;
                }
                free += inner.queue[i].get_capacity();
            }
        }

        if let Some(back) = inner.queue.back() {
            if !back.is_full() {
                let cap = back.get_capacity();
                let ds = back.get_data_size();
                if ds < cap {
                    free += cap - ds;
                }
            }
        }

        free
    }

    // StreamBuffer.cpp:224 (PushBack(uint8_t*, size_t))
    pub fn push_back(&self, data: &[u8]) -> usize {
        self.inner.lock().unwrap().push_back(data)
    }

    pub fn get_block_size(&self) -> usize {
        self.inner.lock().unwrap().block_size
    }

    pub fn get_min_block_count(&self) -> usize {
        self.inner.lock().unwrap().min_block_count
    }

    pub fn get_max_block_count(&self) -> usize {
        self.inner.lock().unwrap().max_block_count
    }

    // StreamBuffer.cpp:349-357
    pub fn get_begin_pos(&self) -> PosType {
        self.inner.lock().unwrap().get_begin_pos()
    }

    // StreamBuffer.cpp:360-368
    pub fn get_end_pos(&self) -> PosType {
        self.inner.lock().unwrap().get_end_pos()
    }

    // StreamBuffer.cpp:371-385
    pub fn get_data_range(&self) -> Option<(PosType, PosType)> {
        self.inner.lock().unwrap().get_data_range()
    }

    // 内部: SequentialReader から呼ばれる
    fn read_at(&self, pos: &mut PosType, buf: &mut [u8]) -> usize {
        self.inner.lock().unwrap().read(pos, buf)
    }

    // StreamBuffer.cpp:321-329
    fn set_reader_pos_pub(&self, id: u64, pos: PosType) {
        let mut inner = self.inner.lock().unwrap();
        inner.reader_pos_list.insert(id, pos);
        inner.free_unused_blocks();
    }

    // StreamBuffer.cpp:332-346
    fn reset_reader_pos_pub(&self, id: u64) -> bool {
        let mut inner = self.inner.lock().unwrap();
        let existed = inner.reader_pos_list.remove(&id).is_some();
        if existed {
            inner.free_unused_blocks();
        }
        existed
    }
}

// ---------------------------------------------------------------------------
// Reader trait (StreamBuffer.hpp:51)
// ---------------------------------------------------------------------------

pub trait Reader {
    fn open(&mut self, buffer: Arc<StreamBuffer>) -> bool;
    fn close(&mut self);
    fn is_open(&self) -> bool;
    fn read(&mut self, buf: &mut [u8]) -> usize;
    fn set_pos(&mut self, pos: PosType) -> bool;
    fn seek_to_begin(&mut self) -> bool;
    fn seek_to_end(&mut self) -> bool;
    fn is_data_available(&self) -> bool;
}

// ---------------------------------------------------------------------------
// SequentialReader (StreamBuffer.hpp:69, StreamBuffer.cpp:614-726)
// ---------------------------------------------------------------------------

/// 順次読み出しリーダー。`Arc<StreamBuffer>` を共有して読み進める。
pub struct SequentialReader {
    buffer: Option<Arc<StreamBuffer>>,
    id: u64,
    pos: PosType,
}

impl Default for SequentialReader {
    fn default() -> Self {
        Self::new()
    }
}

impl SequentialReader {
    pub fn new() -> Self {
        Self {
            buffer: None,
            id: READER_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
            pos: POS_INVALID,
        }
    }

    pub fn get_pos(&self) -> PosType {
        self.pos
    }
}

impl Reader for SequentialReader {
    // StreamBuffer.cpp:626-634
    fn open(&mut self, buffer: Arc<StreamBuffer>) -> bool {
        if self.buffer.is_some() {
            return false;
        }
        let begin = buffer.get_begin_pos();
        buffer.set_reader_pos_pub(self.id, begin);
        self.pos = begin;
        self.buffer = Some(buffer);
        true
    }

    // StreamBuffer.cpp:638-642
    fn close(&mut self) {
        if let Some(buf) = self.buffer.take() {
            buf.reset_reader_pos_pub(self.id);
        }
        self.pos = POS_INVALID;
    }

    fn is_open(&self) -> bool {
        self.buffer.is_some()
    }

    // StreamBuffer.cpp:646-657
    fn read(&mut self, buf: &mut [u8]) -> usize {
        if buf.is_empty() {
            return 0;
        }
        let buffer = match &self.buffer {
            Some(b) => b.clone(),
            None => return 0,
        };
        let old_pos = self.pos;
        let n = buffer.read_at(&mut self.pos, buf);
        if self.pos != old_pos {
            buffer.set_reader_pos_pub(self.id, self.pos);
        }
        n
    }

    // StreamBuffer.cpp:660-671
    fn set_pos(&mut self, pos: PosType) -> bool {
        if self.buffer.is_none() || pos < 0 {
            return false;
        }
        if pos != self.pos {
            self.pos = pos;
            if let Some(buf) = &self.buffer {
                buf.set_reader_pos_pub(self.id, self.pos);
            }
        }
        true
    }

    // StreamBuffer.cpp:674-686
    fn seek_to_begin(&mut self) -> bool {
        let buffer = match &self.buffer {
            Some(b) => b.clone(),
            None => return false,
        };
        let pos = buffer.get_begin_pos();
        if pos != self.pos {
            self.pos = pos;
            buffer.set_reader_pos_pub(self.id, pos);
        }
        true
    }

    // StreamBuffer.cpp:689-701
    fn seek_to_end(&mut self) -> bool {
        let buffer = match &self.buffer {
            Some(b) => b.clone(),
            None => return false,
        };
        let pos = buffer.get_end_pos();
        if pos != self.pos {
            self.pos = pos;
            buffer.set_reader_pos_pub(self.id, pos);
        }
        true
    }

    // StreamBuffer.cpp:704-718
    fn is_data_available(&self) -> bool {
        if self.pos == POS_INVALID {
            return false;
        }
        let buffer = match &self.buffer {
            Some(b) => b,
            None => return false,
        };
        match buffer.get_data_range() {
            None => false,
            Some((begin, end)) => {
                if self.pos == POS_BEGIN {
                    end > begin
                } else {
                    end > self.pos
                }
            }
        }
    }
}

impl Drop for SequentialReader {
    fn drop(&mut self) {
        self.close();
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_buf(block_size: usize, min: usize, max: usize) -> Arc<StreamBuffer> {
        let buf = Arc::new(StreamBuffer::new());
        assert!(buf.create(block_size, min, max, None));
        buf
    }

    #[test]
    fn test_create_and_is_created() {
        let buf = StreamBuffer::new();
        assert!(!buf.is_created());
        assert!(buf.create(100, 1, 4, None));
        assert!(buf.is_created());
        assert_eq!(buf.get_block_size(), 100);
        assert_eq!(buf.get_min_block_count(), 1);
        assert_eq!(buf.get_max_block_count(), 4);
    }

    #[test]
    fn test_create_invalid_params() {
        let buf = StreamBuffer::new();
        assert!(!buf.create(0, 1, 4, None));    // block_size == 0
        assert!(!buf.create(100, 0, 0, None));  // max_block_count == 0
        assert!(!buf.create(100, 5, 3, None));  // min > max
    }

    #[test]
    fn test_destroy() {
        let buf = make_buf(100, 1, 4);
        buf.push_back(&[1u8; 100]);
        buf.destroy();
        assert!(!buf.is_created());
        assert!(buf.is_empty());
    }

    #[test]
    fn test_push_back_and_is_empty_is_full() {
        let buf = make_buf(10, 1, 2);
        assert!(buf.is_empty());
        assert!(!buf.is_full());

        let n = buf.push_back(&[0u8; 10]);
        assert_eq!(n, 10);
        assert!(!buf.is_empty());
        assert!(!buf.is_full()); // 1 block filled, max is 2

        let n2 = buf.push_back(&[1u8; 10]);
        assert_eq!(n2, 10);
        assert!(buf.is_full()); // 2 blocks filled = max
    }

    #[test]
    fn test_push_back_spills_to_next_block() {
        let buf = make_buf(10, 1, 3);
        // 25 bytes → spills across 3 blocks
        let n = buf.push_back(&(0u8..25).collect::<Vec<u8>>());
        assert_eq!(n, 25);
    }

    #[test]
    fn test_push_back_reuses_oldest_block() {
        // max=2 blocks of 5 bytes each
        let buf = make_buf(5, 1, 2);
        buf.push_back(&[1u8; 5]); // block0: serial_pos=0
        buf.push_back(&[2u8; 5]); // block1: serial_pos=5  → now full
        // pushing more data: block0 has no reader lock → reuse
        let n = buf.push_back(&[3u8; 5]);
        assert_eq!(n, 5);
        assert_eq!(buf.get_end_pos(), 15);
    }

    #[test]
    fn test_clear() {
        let buf = make_buf(10, 1, 4);
        buf.push_back(&[0u8; 30]);
        assert!(!buf.is_empty());
        buf.clear();
        assert!(buf.is_empty());
    }

    #[test]
    fn test_get_begin_end_pos() {
        let buf = make_buf(10, 1, 4);
        assert_eq!(buf.get_begin_pos(), 0);
        assert_eq!(buf.get_end_pos(), 0);
        assert!(buf.get_data_range().is_none());

        buf.push_back(&[0u8; 15]);
        let (begin, end) = buf.get_data_range().unwrap();
        assert_eq!(begin, 0);
        assert_eq!(end, 15);
    }

    #[test]
    fn test_sequential_reader_basic_read() {
        let buf = make_buf(10, 1, 4);
        let data: Vec<u8> = (0..20).collect();
        buf.push_back(&data);

        let mut reader = SequentialReader::new();
        assert!(!reader.is_open());
        assert!(reader.open(Arc::clone(&buf)));
        assert!(reader.is_open());

        let mut out = vec![0u8; 20];
        let n = reader.read(&mut out);
        assert_eq!(n, 20);
        assert_eq!(out, data);
    }

    #[test]
    fn test_sequential_reader_read_across_blocks() {
        // block=5 × max=4 = 20 bytes capacity
        let buf = make_buf(5, 1, 4);
        let data: Vec<u8> = (0u8..20).collect();
        let pushed = buf.push_back(&data);
        assert_eq!(pushed, 20);

        let mut reader = SequentialReader::new();
        reader.open(Arc::clone(&buf));

        let mut out = vec![0u8; 20];
        let n = reader.read(&mut out);
        assert_eq!(n, 20);
        assert_eq!(out, data);
    }

    #[test]
    fn test_sequential_reader_seek_to_begin() {
        // min=2 で読み進めてもブロックが解放されないことを確認したうえで seek_to_begin
        let buf = make_buf(10, 2, 4);
        buf.push_back(&[1u8; 20]);

        let mut reader = SequentialReader::new();
        reader.open(Arc::clone(&buf));

        let mut tmp = [0u8; 20];
        reader.read(&mut tmp);
        assert_eq!(reader.get_pos(), 20);

        // begin_pos は queue.front (min=2 でブロック保持) → 0 のまま
        assert!(reader.seek_to_begin());
        assert_eq!(reader.get_pos(), 0);

        let mut out = [0u8; 20];
        let n = reader.read(&mut out);
        assert_eq!(n, 20);
    }

    #[test]
    fn test_sequential_reader_seek_to_end() {
        let buf = make_buf(10, 1, 4);
        buf.push_back(&[0u8; 30]);

        let mut reader = SequentialReader::new();
        reader.open(Arc::clone(&buf));

        reader.seek_to_end();
        assert_eq!(reader.get_pos(), 30);

        // no more data
        let mut out = [0u8; 10];
        let n = reader.read(&mut out);
        assert_eq!(n, 0);
    }

    #[test]
    fn test_is_data_available() {
        let buf = make_buf(10, 1, 4);
        let mut reader = SequentialReader::new();
        reader.open(Arc::clone(&buf));

        assert!(!reader.is_data_available()); // empty

        buf.push_back(&[1u8; 10]);
        assert!(reader.is_data_available());

        // read it all
        let mut out = [0u8; 10];
        reader.read(&mut out);
        assert!(!reader.is_data_available());
    }

    #[test]
    fn test_sequential_reader_close() {
        let buf = make_buf(10, 1, 4);
        let mut reader = SequentialReader::new();
        reader.open(Arc::clone(&buf));
        assert!(reader.is_open());
        reader.close();
        assert!(!reader.is_open());
        assert_eq!(reader.get_pos(), POS_INVALID);
    }

    #[test]
    fn test_get_free_space() {
        let buf = make_buf(10, 1, 4);
        let initial_free = buf.get_free_space();
        // 4 blocks * 10 bytes = 40 bytes free initially
        assert_eq!(initial_free, 40);

        buf.push_back(&[0u8; 10]);
        // 3 blocks * 10 + (10-10)=0 free in back block = 30
        assert_eq!(buf.get_free_space(), 30);
    }

    #[test]
    fn test_set_size_max_block_count() {
        let buf = make_buf(10, 1, 4);
        buf.push_back(&[0u8; 40]); // fill 4 blocks
        assert!(buf.set_size(10, 1, 2, true)); // shrink with discard
        // 2 blocks retained
        assert_eq!(buf.get_max_block_count(), 2);
        assert!(buf.is_full());
    }

    #[test]
    fn test_reader_locks_block_from_reuse() {
        // max=2 blocks, min=1 block
        let buf = make_buf(5, 1, 2);
        buf.push_back(&[1u8; 5]); // block0 at pos 0
        buf.push_back(&[2u8; 5]); // block1 at pos 5 → full

        let mut reader = SequentialReader::new();
        reader.open(Arc::clone(&buf)); // reader at pos=0 → locks block0

        // block0 is locked by reader → new push_back cannot reuse → returns 0
        let n = buf.push_back(&[3u8; 5]);
        assert_eq!(n, 0);

        // advance reader past block0
        let mut tmp = [0u8; 5];
        reader.read(&mut tmp); // pos → 5, unlocks block0

        // now block0 can be reused
        let n2 = buf.push_back(&[3u8; 5]);
        assert_eq!(n2, 5);
    }
}
