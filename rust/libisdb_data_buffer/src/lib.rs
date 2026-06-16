// LibISDB/Base/DataBuffer.cpp の Rust 移植。
//
// 原実装(DataBuffer)はデータサイズ(m_DataSize)と確保済み容量(m_BufferSize)を
// 分離管理する動的バイトバッファで、各種パーサーの基底クラスとして使われる。
// Rust では Vec<u8> がほぼ等価だが、本クレートは原実装固有の以下の挙動を
// 忠実に再現する:
//   - AllocateBuffer の独自成長戦略(AllocateUnit=0x100000 への切り上げ /
//     確保単位未満では DataSize*2 への拡張)
//   - データサイズと容量の分離(SetSize/ClearSize/FreeBuffer)
//   - GetData がサイズ 0 のとき None(原実装は nullptr) を返す
//   - SetAt/GetAt の範囲外アクセスの扱い(原実装は no-op / 0 を返す)
//   - TrimHead / TrimTail
//
// 原実装は malloc/realloc/free を仮想関数(Allocate/Free/ReAllocate)で
// 抽象化しているが、Rust では Vec<u8> の内部メモリ管理に委ねる。確保容量
// (capacity 相当)は m_buffer_size を別途持って原実装の成長戦略を再現する。
//
// 各関数のコメントに原実装の DataBuffer.cpp の行番号を付す。

/// AllocateBuffer の確保単位(DataBuffer.cpp:230, 0x100000 = 1MiB)。
const ALLOCATE_UNIT: usize = 0x10_0000;

/// メモリデータバッファ(DataBuffer.hpp:35)。
///
/// `data` はデータ部のみを保持(`len()` == 原実装の `m_DataSize`)。
/// `buffer_size` は原実装の `m_BufferSize`(確保済み容量)を再現する追跡値で、
/// 成長戦略の判定に用いる。Rust の `Vec` 実体の `capacity()` とは別管理。
#[derive(Debug, Clone, Default)]
pub struct DataBuffer {
    data: Vec<u8>,
    buffer_size: usize,
}

impl DataBuffer {
    /// 空のバッファを生成(DataBuffer() = default)。
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
            buffer_size: 0,
        }
    }

    /// 容量 `buffer_size` を確保した空(サイズ 0)のバッファ(DataBuffer.cpp:48)。
    pub fn with_buffer_size(buffer_size: usize) -> Self {
        let mut b = Self::new();
        b.allocate_buffer(buffer_size);
        b
    }

    /// `data` の内容をコピーして生成(DataBuffer.cpp:54)。
    pub fn from_data(data: &[u8]) -> Self {
        let mut b = Self::new();
        b.set_data(data);
        b
    }

    /// サイズ `size` を `filler` で埋めたバッファ(DataBuffer.cpp:60)。
    pub fn with_filler(size: usize, filler: u8) -> Self {
        let mut b = Self::new();
        b.set_size_filled(size, filler);
        b
    }

    /// データ部の先頭ポインタ相当(DataBuffer.cpp:114)。
    /// 原実装はサイズ 0 のとき nullptr を返すため、Rust では None を返す。
    pub fn get_data(&self) -> Option<&[u8]> {
        if !self.data.is_empty() {
            Some(&self.data)
        } else {
            None
        }
    }

    /// データ部の可変スライス相当(GetData() noexcept, DataBuffer.cpp:108)。
    pub fn get_data_mut(&mut self) -> Option<&mut [u8]> {
        if !self.data.is_empty() {
            Some(&mut self.data)
        } else {
            None
        }
    }

    /// 確保済みバッファ全体の先頭相当(GetBuffer, DataBuffer.hpp:56)。
    /// 原実装は容量分の生ポインタを返すが、Rust ではデータ部スライスを返す。
    pub fn get_buffer(&self) -> &[u8] {
        &self.data
    }

    /// データサイズ(GetSize, DataBuffer.hpp:57 = m_DataSize)。
    pub fn size(&self) -> usize {
        self.data.len()
    }

    /// 確保済み容量(GetBufferSize, DataBuffer.hpp:58 = m_BufferSize)。
    pub fn buffer_size(&self) -> usize {
        self.buffer_size
    }

    /// 指定位置に 1 バイト書き込む(DataBuffer.cpp:120)。
    /// 原実装は Pos >= m_DataSize のとき何もしない(LIBISDB_TRACE_ERROR_IF_NOT)。
    pub fn set_at(&mut self, pos: usize, data: u8) {
        if pos < self.data.len() {
            self.data[pos] = data;
        }
    }

    /// 指定位置の 1 バイトを取得(DataBuffer.cpp:127)。
    /// 原実装は範囲外のとき 0x00 を返す。
    pub fn get_at(&self, pos: usize) -> u8 {
        if pos < self.data.len() {
            self.data[pos]
        } else {
            0x00
        }
    }

    /// データを丸ごと差し替える(DataBuffer.cpp:133)。新しいデータサイズを返す。
    pub fn set_data(&mut self, data: &[u8]) -> usize {
        if !data.is_empty() {
            // AllocateBuffer が要求量を満たせなければ現サイズを返す(原実装:139)。
            if self.allocate_buffer(data.len()) < data.len() {
                return self.data.len();
            }
            self.data.clear();
            self.data.extend_from_slice(data);
        } else {
            // DataSize == 0: バッファ確保はせずサイズだけ 0 にする(原実装:145)。
            self.data.clear();
        }
        self.data.len()
    }

    /// 末尾にデータを追記する(DataBuffer.cpp:151)。新しいデータサイズを返す。
    pub fn add_data(&mut self, data: &[u8]) -> usize {
        if !data.is_empty() {
            // m_DataSize + DataSize のオーバーフロー判定(原実装:156)。
            let new_size = match self.data.len().checked_add(data.len()) {
                Some(n) => n,
                None => return self.data.len(),
            };
            if self.allocate_buffer(new_size) < new_size {
                return self.data.len();
            }
            self.data.extend_from_slice(data);
        }
        self.data.len()
    }

    /// 別の DataBuffer の内容を末尾に追記する(DataBuffer.cpp:172)。
    pub fn add_buffer(&mut self, other: &DataBuffer) -> usize {
        // 原実装は other.m_pData / m_DataSize を直接渡す(データ部のみ)。
        let other_data = other.data.clone();
        self.add_data(&other_data)
    }

    /// 末尾に 1 バイト追記する(DataBuffer.cpp:178)。
    pub fn add_byte(&mut self, data: u8) -> usize {
        // m_DataSize + 1 がオーバーフローすると <= m_DataSize に引っ掛かるため
        // オーバーフロー判定は不要(原実装コメント:180-181)。
        let want = self.data.len().wrapping_add(1);
        if self.allocate_buffer(want) <= self.data.len() {
            return self.data.len();
        }
        self.data.push(data);
        self.data.len()
    }

    /// 先頭から `trim_size` バイトを削る(DataBuffer.cpp:193)。
    pub fn trim_head(&mut self, trim_size: usize) -> usize {
        if trim_size >= self.data.len() {
            self.data.clear();
        } else if !self.data.is_empty() {
            // std::memmove 相当。Vec::drain で前方を除去。
            self.data.drain(0..trim_size);
        }
        self.data.len()
    }

    /// 末尾から `trim_size` バイトを削る(DataBuffer.cpp:206)。
    pub fn trim_tail(&mut self, trim_size: usize) -> usize {
        if trim_size >= self.data.len() {
            self.data.clear();
        } else {
            let new_len = self.data.len() - trim_size;
            self.data.truncate(new_len);
        }
        self.data.len()
    }

    /// バッファ容量を確保する(DataBuffer.cpp:218)。確保後の容量を返す。
    ///
    /// 原実装の成長戦略を忠実再現する:
    ///   - 既存容量で足りるなら何もしない。
    ///   - 初回確保(m_pData == nullptr)は要求サイズちょうど。
    ///   - 拡張時は AllocateUnit(1MiB) 未満なら DataSize*2 まで広げ、
    ///     1MiB 以上なら AllocateUnit 境界へ切り上げる。
    pub fn allocate_buffer(&mut self, size: usize) -> usize {
        // 原実装は Size > RSIZE_MAX で失敗。Rust では usize の最大が上限なので
        // この判定は事実上発生しないが、忠実性のためコメントで明示する。
        if size <= self.buffer_size {
            return self.buffer_size;
        }

        let new_buffer_size = if self.buffer_size == 0 {
            // 初回確保(m_pData == nullptr 相当): 要求サイズちょうど(原実装:226)。
            size
        } else {
            // 拡張(原実装:229)。
            let mut buffer_size = size;
            if buffer_size < ALLOCATE_UNIT {
                // 1MiB 未満: DataSize*2 まで広げる(原実装:234)。
                let twice = self.data.len().saturating_mul(2);
                if buffer_size < twice {
                    buffer_size = twice;
                }
            } else if buffer_size <= usize::MAX - ALLOCATE_UNIT {
                // 1MiB 境界へ切り上げ(原実装:237)。
                buffer_size = (buffer_size + (ALLOCATE_UNIT - 1)) & !(ALLOCATE_UNIT - 1);
            }
            buffer_size
        };

        // Vec の実メモリも要求容量まで確保しておく(realloc 相当)。
        if new_buffer_size > self.data.capacity() {
            self.data.reserve(new_buffer_size - self.data.len());
        }
        self.buffer_size = new_buffer_size;
        self.buffer_size
    }

    /// データサイズを `size` に設定する(DataBuffer.cpp:252)。
    ///
    /// 拡張部分の中身は未定義(原実装は memset しない)。Rust では Vec を
    /// 0 で伸長する(安全側)。縮小時はそのまま truncate。確保失敗時は現サイズ。
    pub fn set_size(&mut self, size: usize) -> usize {
        if size > 0 {
            if self.allocate_buffer(size) < size {
                return self.data.len();
            }
        }
        self.data.resize(size, 0x00);
        self.data.len()
    }

    /// データサイズを `size` に設定し、全体を `filler` で埋める(DataBuffer.cpp:265)。
    pub fn set_size_filled(&mut self, size: usize, filler: u8) -> usize {
        if self.set_size(size) < size {
            return self.data.len();
        }
        if size > 0 {
            // std::memset 相当(原実装:271)。
            for b in self.data.iter_mut() {
                *b = filler;
            }
        }
        self.data.len()
    }

    /// データサイズだけを 0 にする(容量は維持, DataBuffer.cpp:277)。
    pub fn clear_size(&mut self) {
        self.data.clear();
    }

    /// バッファを完全に解放する(DataBuffer.cpp:283)。
    pub fn free_buffer(&mut self) {
        self.data = Vec::new();
        self.buffer_size = 0;
    }
}

/// 等価比較(DataBuffer.cpp:99)。データサイズが等しく内容が一致するか。
impl PartialEq for DataBuffer {
    fn eq(&self, other: &Self) -> bool {
        // 原実装は m_DataSize の一致 + memcmp。容量(m_BufferSize)は比較しない。
        self.data == other.data
    }
}

impl Eq for DataBuffer {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_is_empty() {
        let b = DataBuffer::new();
        assert_eq!(b.size(), 0);
        assert_eq!(b.buffer_size(), 0);
        assert!(b.get_data().is_none());
    }

    #[test]
    fn test_from_data() {
        let b = DataBuffer::from_data(&[1, 2, 3, 4]);
        assert_eq!(b.size(), 4);
        assert_eq!(b.get_data(), Some(&[1u8, 2, 3, 4][..]));
        // 初回確保は要求サイズちょうど。
        assert_eq!(b.buffer_size(), 4);
    }

    #[test]
    fn test_set_data_then_smaller_keeps_capacity() {
        let mut b = DataBuffer::from_data(&[0; 100]);
        assert_eq!(b.buffer_size(), 100);
        // より小さいデータを設定しても容量は縮まない(原実装の AllocateBuffer は縮小しない)。
        b.set_data(&[9, 9, 9]);
        assert_eq!(b.size(), 3);
        assert_eq!(b.buffer_size(), 100);
        assert_eq!(b.get_data(), Some(&[9u8, 9, 9][..]));
    }

    #[test]
    fn test_set_data_empty_clears_size_only() {
        let mut b = DataBuffer::from_data(&[1, 2, 3]);
        let cap = b.buffer_size();
        let n = b.set_data(&[]);
        assert_eq!(n, 0);
        assert_eq!(b.size(), 0);
        // 空データ設定では容量は維持(原実装は DataSize==0 で AllocateBuffer を呼ばない)。
        assert_eq!(b.buffer_size(), cap);
        assert!(b.get_data().is_none());
    }

    #[test]
    fn test_add_data_and_add_byte() {
        let mut b = DataBuffer::new();
        assert_eq!(b.add_data(&[1, 2]), 2);
        assert_eq!(b.add_byte(3), 3);
        assert_eq!(b.add_data(&[4, 5]), 5);
        assert_eq!(b.get_data(), Some(&[1u8, 2, 3, 4, 5][..]));
    }

    #[test]
    fn test_add_buffer() {
        let mut a = DataBuffer::from_data(&[1, 2, 3]);
        let other = DataBuffer::from_data(&[4, 5]);
        assert_eq!(a.add_buffer(&other), 5);
        assert_eq!(a.get_data(), Some(&[1u8, 2, 3, 4, 5][..]));
    }

    #[test]
    fn test_set_at_get_at() {
        let mut b = DataBuffer::from_data(&[10, 20, 30]);
        b.set_at(1, 99);
        assert_eq!(b.get_at(1), 99);
        // 範囲外 set は no-op、範囲外 get は 0。
        b.set_at(5, 77);
        assert_eq!(b.get_at(5), 0);
        assert_eq!(b.size(), 3);
    }

    #[test]
    fn test_trim_head() {
        let mut b = DataBuffer::from_data(&[1, 2, 3, 4, 5]);
        assert_eq!(b.trim_head(2), 3);
        assert_eq!(b.get_data(), Some(&[3u8, 4, 5][..]));
        // trim_size >= size のときは全消去。
        assert_eq!(b.trim_head(100), 0);
        assert!(b.get_data().is_none());
    }

    #[test]
    fn test_trim_tail() {
        let mut b = DataBuffer::from_data(&[1, 2, 3, 4, 5]);
        assert_eq!(b.trim_tail(2), 3);
        assert_eq!(b.get_data(), Some(&[1u8, 2, 3][..]));
        assert_eq!(b.trim_tail(100), 0);
        assert!(b.get_data().is_none());
    }

    #[test]
    fn test_set_size_grow_and_shrink() {
        let mut b = DataBuffer::from_data(&[1, 2, 3]);
        assert_eq!(b.set_size(5), 5);
        assert_eq!(b.size(), 5);
        assert_eq!(b.set_size(2), 2);
        assert_eq!(b.size(), 2);
    }

    #[test]
    fn test_set_size_filled() {
        let b = DataBuffer::with_filler(4, 0xAB);
        assert_eq!(b.size(), 4);
        assert_eq!(b.get_data(), Some(&[0xAB, 0xAB, 0xAB, 0xAB][..]));
    }

    #[test]
    fn test_clear_size_keeps_capacity() {
        let mut b = DataBuffer::from_data(&[0; 50]);
        let cap = b.buffer_size();
        b.clear_size();
        assert_eq!(b.size(), 0);
        assert_eq!(b.buffer_size(), cap);
    }

    #[test]
    fn test_free_buffer() {
        let mut b = DataBuffer::from_data(&[0; 50]);
        b.free_buffer();
        assert_eq!(b.size(), 0);
        assert_eq!(b.buffer_size(), 0);
    }

    #[test]
    fn test_equality() {
        let a = DataBuffer::from_data(&[1, 2, 3]);
        let b = DataBuffer::from_data(&[1, 2, 3]);
        let c = DataBuffer::from_data(&[1, 2, 4]);
        let d = DataBuffer::from_data(&[1, 2]);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
        // 空同士は等しい。
        assert_eq!(DataBuffer::new(), DataBuffer::new());
    }

    #[test]
    fn test_equality_ignores_capacity() {
        // 容量が違っても内容が同じなら等しい(==は m_BufferSize を見ない)。
        let mut a = DataBuffer::with_buffer_size(1000);
        a.set_data(&[7, 8, 9]);
        let b = DataBuffer::from_data(&[7, 8, 9]);
        assert_ne!(a.buffer_size(), b.buffer_size());
        assert_eq!(a, b);
    }

    #[test]
    fn test_allocate_buffer_growth_small() {
        // 1MiB 未満の拡張は DataSize*2 まで広げる(原実装:234)。
        let mut b = DataBuffer::from_data(&[0; 100]);
        // 現データ 100 → 容量 100。120 を要求すると max(120, 100*2=200) = 200。
        let cap = b.allocate_buffer(120);
        assert_eq!(cap, 200);
        assert_eq!(b.buffer_size(), 200);
    }

    #[test]
    fn test_allocate_buffer_first_alloc_exact() {
        // 初回確保(buffer_size==0)は要求サイズちょうど。
        let mut b = DataBuffer::new();
        assert_eq!(b.allocate_buffer(123), 123);
        assert_eq!(b.buffer_size(), 123);
    }

    #[test]
    fn test_allocate_buffer_growth_large_rounds_to_unit() {
        // 1MiB 以上の拡張は AllocateUnit(1MiB) 境界へ切り上げ(原実装:237)。
        // まず初回確保で buffer_size を 1 にして「拡張」経路に入れる。
        let mut b = DataBuffer::from_data(&[0u8]);
        assert_eq!(b.buffer_size(), 1);
        // 1MiB + 1 バイトを要求 → 2MiB(0x200000)へ切り上げ。
        let want = ALLOCATE_UNIT + 1;
        let cap = b.allocate_buffer(want);
        assert_eq!(cap, ALLOCATE_UNIT * 2);
    }

    #[test]
    fn test_allocate_buffer_no_shrink() {
        // 既存容量で足りるなら何もしない(縮小しない)。
        let mut b = DataBuffer::with_buffer_size(500);
        assert_eq!(b.allocate_buffer(100), 500);
        assert_eq!(b.buffer_size(), 500);
    }
}
