// LibISDB/TS/TSDownload.cpp の Rust 移植。
//
// DSM-CC によるデータダウンロード処理を扱う:
//   - `DataModule`: ダウンロードブロックを蓄積しモジュールを再構成する。
//   - `DownloadInfoIndicationParser`: DII (DownloadInfoIndication) メッセージのパース。
//   - `DownloadDataBlockParser`: DDB (DownloadDataBlock) メッセージのパース。
//
// 原実装は EventHandler 仮想クラスでコールバックするが、Rust では
// クロージャ / 戻り値で表現する。Load16/Load32 はビッグエンディアン
// (Utilities.hpp:96/114) で、libisdb_utilities の load16_be/load32_be と等価。

use libisdb_utilities::{load16_be, load32_be};

/// ダウンロードデータモジュール。TSDownload.hpp:37 / TSDownload.cpp:37。
///
/// 受信したブロックを蓄積し、全ブロックが揃ったらモジュールデータを再構成する。
/// 原実装の `OnComplete` 仮想関数は、完成したデータを `StoreBlock` の戻り値
/// (`StoreBlockResult`) で受け取る形に置き換えた。
#[derive(Clone, Debug)]
pub struct DataModule {
    download_id: u32,
    block_size: u16,
    module_id: u16,
    module_size: u32,
    module_version: u8,
    num_blocks: u16,
    num_downloaded_blocks: u16,
    /// 再構成中のモジュールデータ。原実装は最初のブロック受信時に確保するが、
    /// Rust では生成時に確保する (挙動差なし: 内容は受信ブロックで埋まる)。
    data: Vec<u8>,
    /// 各ブロックの受信済みフラグ (BitTable 相当)。
    block_downloaded: Vec<bool>,
}

/// `StoreBlock` の結果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreBlockResult {
    /// ブロックを格納できなかった (ブロック番号超過 / データ不足)。原実装の false。
    Rejected,
    /// ブロックを格納した (または既に受信済みだった)。モジュールは未完成。原実装の true。
    Stored,
    /// このブロックでモジュールが完成した。原実装は OnComplete が呼ばれるケース。
    Completed,
}

impl DataModule {
    /// TSDownload.cpp:37。num_blocks = (module_size - 1) / block_size + 1。
    ///
    /// 原実装は block_size==0 や module_size==0 を弾かないが、Rust では
    /// ゼロ除算 panic を避けるため block_size==0 のときは num_blocks=0 とする
    /// (呼び出し側は DII パーサ側で block_size>0 を保証している)。
    pub fn new(
        download_id: u32,
        block_size: u16,
        module_id: u16,
        module_size: u32,
        module_version: u8,
    ) -> Self {
        let num_blocks = if block_size == 0 || module_size == 0 {
            0
        } else {
            (((module_size - 1) / block_size as u32) + 1) as u16
        };
        Self {
            download_id,
            block_size,
            module_id,
            module_size,
            module_version,
            num_blocks,
            num_downloaded_blocks: 0,
            data: vec![0u8; module_size as usize],
            block_downloaded: vec![false; num_blocks as usize],
        }
    }

    pub fn download_id(&self) -> u32 {
        self.download_id
    }
    pub fn block_size(&self) -> u16 {
        self.block_size
    }
    pub fn module_id(&self) -> u16 {
        self.module_id
    }
    pub fn module_size(&self) -> u32 {
        self.module_size
    }
    pub fn module_version(&self) -> u8 {
        self.module_version
    }
    pub fn num_blocks(&self) -> u16 {
        self.num_blocks
    }

    /// 全ブロック受信済みか。TSDownload.hpp:49。
    pub fn is_complete(&self) -> bool {
        self.num_downloaded_blocks == self.num_blocks
    }

    /// 指定ブロックが受信済みか。TSDownload.cpp:91。
    pub fn is_block_downloaded(&self, block_number: u16) -> bool {
        if block_number >= self.num_blocks {
            return false;
        }
        self.block_downloaded[block_number as usize]
    }

    /// ブロックを格納する。TSDownload.cpp:58。
    ///
    /// - ブロック番号が範囲外なら Rejected。
    /// - 既に受信済みなら Stored (原実装は true を返すだけで何もしない)。
    /// - 最終ブロックは module_size - offset バイト、それ以外は block_size バイト。
    ///   data_size がそのサイズに満たなければ Rejected。
    /// - 格納後に完成したら Completed。
    pub fn store_block(&mut self, block_number: u16, data: &[u8]) -> StoreBlockResult {
        if block_number >= self.num_blocks {
            return StoreBlockResult::Rejected;
        }

        if self.is_block_downloaded(block_number) {
            return StoreBlockResult::Stored;
        }

        let offset = block_number as usize * self.block_size as usize;
        let size = if block_number < self.num_blocks - 1 {
            self.block_size as usize
        } else {
            self.module_size as usize - offset
        };
        if data.len() < size {
            return StoreBlockResult::Rejected;
        }

        self.data[offset..offset + size].copy_from_slice(&data[..size]);

        self.block_downloaded[block_number as usize] = true;
        self.num_downloaded_blocks += 1;

        if self.is_complete() {
            StoreBlockResult::Completed
        } else {
            StoreBlockResult::Stored
        }
    }

    /// 再構成中 / 完成済みのモジュールデータへの参照。
    /// 原実装の OnComplete(pData, ModuleSize) で渡されるバッファに相当。
    pub fn data(&self) -> &[u8] {
        &self.data
    }
}

/// DII (DownloadInfoIndication) のメッセージ情報。TSDownload.hpp:71。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MessageInfo {
    pub protocol_discriminator: u8,
    pub dsmcc_type: u8,
    pub message_id: u16,
    pub transaction_id: u32,
    pub download_id: u32,
    pub block_size: u16,
    pub window_size: u8,
    pub ack_period: u8,
    pub tc_download_window: u32,
    pub tc_download_scenario: u32,
}

/// DII のモジュール記述子から取り出した Name (タグ 0x02)。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModuleName {
    /// 原実装は raw ポインタ + 長さ。Rust では生バイト列を保持する。
    pub text: Vec<u8>,
}

/// DII のモジュール記述子から取り出した CRC32 (タグ 0x05、長さ 4)。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModuleCrc {
    pub is_valid: bool,
    pub crc32: u32,
}

/// DII の 1 モジュール分の情報。TSDownload.hpp:84。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModuleInfo {
    pub module_id: u16,
    pub module_size: u32,
    pub module_version: u8,
    pub name: ModuleName,
    pub crc: ModuleCrc,
}

/// DII (DownloadInfoIndication) をパースする。TSDownload.cpp:116。
///
/// 成功すると `(MessageInfo, Vec<ModuleInfo>)` を返す。失敗 (長さ不足 /
/// block_size 範囲外 / 各種境界超過) のとき None。原実装の EventHandler::OnDataModule
/// コールバックは、戻り値のモジュールリストを呼び出し側が走査する形に置き換えた。
pub fn parse_download_info_indication(data: &[u8]) -> Option<(MessageInfo, Vec<ModuleInfo>)> {
    if data.len() < 34 {
        return None;
    }

    let mut message = MessageInfo {
        protocol_discriminator: data[0],
        dsmcc_type: data[1],
        message_id: load16_be(&data[2..4]),
        transaction_id: load32_be(&data[4..8]),
        ..Default::default()
    };

    let adaptation_length = data[9];
    // MessageLength = Load16(&data[10]) は原実装でも未使用 (コメントアウト)。
    if 12 + adaptation_length as usize > data.len() {
        return None;
    }
    let mut pos = 12 + adaptation_length as usize;

    // 以降のフィールドを読むには pos+16+2 まで必要。原実装はここで明示チェック
    // しないが (data.len()>=34 と後続の CompatDesc チェックで実質担保)、
    // Rust ではスライス panic を避けるため境界チェックを追加する。
    if pos + 18 > data.len() {
        return None;
    }

    message.download_id = load32_be(&data[pos..pos + 4]);
    message.block_size = load16_be(&data[pos + 4..pos + 6]);
    if message.block_size == 0 || message.block_size > 4066 {
        return None;
    }
    message.window_size = data[pos + 6];
    message.ack_period = data[pos + 7];
    message.tc_download_window = load32_be(&data[pos + 8..pos + 12]);
    message.tc_download_scenario = load32_be(&data[pos + 12..pos + 16]);

    // Compatibility Descriptor
    let compat_desc_length = load16_be(&data[pos + 16..pos + 18]) as usize;
    if pos + 18 + compat_desc_length + 2 > data.len() {
        return None;
    }
    pos += 18 + compat_desc_length;

    let number_of_modules = load16_be(&data[pos..pos + 2]);
    pos += 2;

    let mut modules = Vec::with_capacity(number_of_modules as usize);

    for _ in 0..number_of_modules {
        if pos + 8 > data.len() {
            return None;
        }

        let module_id = load16_be(&data[pos..pos + 2]);
        let module_size = load32_be(&data[pos + 2..pos + 6]);
        let module_version = data[pos + 6];

        let module_info_length = data[pos + 7] as usize;
        pos += 8;
        if pos + module_info_length > data.len() {
            return None;
        }

        let mut module = ModuleInfo {
            module_id,
            module_size,
            module_version,
            ..Default::default()
        };

        // モジュール記述子ループ。DescPos + 2 < ModuleInfoLength の間。
        let mut desc_pos = 0usize;
        while desc_pos + 2 < module_info_length {
            let desc_tag = data[pos + desc_pos];
            let desc_length = data[pos + desc_pos + 1] as usize;

            desc_pos += 2;

            if desc_pos + desc_length > module_info_length {
                break;
            }

            match desc_tag {
                0x02 => {
                    // Name descriptor
                    module.name.text = data[pos + desc_pos..pos + desc_pos + desc_length].to_vec();
                }
                0x05 => {
                    // CRC32 descriptor
                    if desc_length == 4 {
                        module.crc.is_valid = true;
                        module.crc.crc32 = load32_be(&data[pos + desc_pos..pos + desc_pos + 4]);
                    }
                }
                _ => {}
            }

            desc_pos += desc_length;
        }

        modules.push(module);

        pos += module_info_length;
    }

    Some((message, modules))
}

/// DDB (DownloadDataBlock) の 1 ブロック分の情報。TSDownload.hpp:118。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DataBlockInfo {
    pub protocol_discriminator: u8,
    pub dsmcc_type: u8,
    pub message_id: u16,
    pub download_id: u32,
    pub module_id: u16,
    pub module_version: u8,
    pub block_number: u16,
    /// ブロックの実データ。原実装は raw ポインタ + DataSize。
    pub data: Vec<u8>,
}

/// DDB (DownloadDataBlock) をパースする。TSDownload.cpp:215。
///
/// 成功すると `DataBlockInfo` を返す。失敗 (長さ不足 / 境界超過) のとき None。
/// 原実装の `data_size` は DataSize - Pos で計算され、本体は &pData[Pos] を指す。
pub fn parse_download_data_block(data: &[u8]) -> Option<DataBlockInfo> {
    if data.len() < 12 {
        return None;
    }

    let protocol_discriminator = data[0];
    let dsmcc_type = data[1];
    let message_id = load16_be(&data[2..4]);
    let download_id = load32_be(&data[4..8]);

    let adaptation_length = data[9];
    // MessageLength = Load16(&data[10]) は原実装でも未使用 (コメントアウト)。
    // 原実装の判定: 12 + AdaptationLength + 6 >= DataSize なら false。
    // (ヘッダ 6 バイトを読んだ後にブロックデータが 1 バイト以上残る必要がある)
    if 12 + adaptation_length as usize + 6 >= data.len() {
        return None;
    }

    let pos = 12 + adaptation_length as usize;

    let module_id = load16_be(&data[pos..pos + 2]);
    let module_version = data[pos + 2];
    let block_number = load16_be(&data[pos + 4..pos + 6]);
    let pos = pos + 6;

    Some(DataBlockInfo {
        protocol_discriminator,
        dsmcc_type,
        message_id,
        download_id,
        module_id,
        module_version,
        block_number,
        data: data[pos..].to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── DataModule ───

    #[test]
    fn test_datamodule_num_blocks() {
        // module_size=10, block_size=4 → ceil(10/4)=3
        let m = DataModule::new(1, 4, 10, 10, 0);
        assert_eq!(m.num_blocks(), 3);
        assert!(!m.is_complete());
        assert_eq!(m.module_size(), 10);
        assert_eq!(m.block_size(), 4);
        assert_eq!(m.module_id(), 10);
        assert_eq!(m.download_id(), 1);
        assert_eq!(m.module_version(), 0);
    }

    #[test]
    fn test_datamodule_num_blocks_exact() {
        // ちょうど割り切れる: module_size=8, block_size=4 → 2
        let m = DataModule::new(1, 4, 0, 8, 0);
        assert_eq!(m.num_blocks(), 2);
    }

    #[test]
    fn test_datamodule_zero_block_size() {
        // ゼロ除算ガード: block_size=0 → num_blocks=0
        let m = DataModule::new(1, 0, 0, 10, 0);
        assert_eq!(m.num_blocks(), 0);
    }

    #[test]
    fn test_datamodule_store_block_complete() {
        // module_size=10, block_size=4 → 3 ブロック (4,4,2)
        let mut m = DataModule::new(1, 4, 0, 10, 0);

        assert_eq!(m.store_block(0, &[1, 2, 3, 4]), StoreBlockResult::Stored);
        assert!(m.is_block_downloaded(0));
        assert!(!m.is_complete());

        assert_eq!(m.store_block(1, &[5, 6, 7, 8]), StoreBlockResult::Stored);
        assert!(!m.is_complete());

        // 最終ブロックは 2 バイト
        assert_eq!(m.store_block(2, &[9, 10]), StoreBlockResult::Completed);
        assert!(m.is_complete());

        assert_eq!(m.data(), &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    }

    #[test]
    fn test_datamodule_store_block_out_of_range() {
        let mut m = DataModule::new(1, 4, 0, 10, 0);
        assert_eq!(m.store_block(3, &[0, 0, 0, 0]), StoreBlockResult::Rejected);
    }

    #[test]
    fn test_datamodule_store_block_already_downloaded() {
        let mut m = DataModule::new(1, 4, 0, 10, 0);
        assert_eq!(m.store_block(0, &[1, 2, 3, 4]), StoreBlockResult::Stored);
        // 2 回目は受信済みで Stored (上書きせず)
        assert_eq!(m.store_block(0, &[9, 9, 9, 9]), StoreBlockResult::Stored);
        assert_eq!(&m.data()[0..4], &[1, 2, 3, 4]);
    }

    #[test]
    fn test_datamodule_store_block_data_too_small() {
        let mut m = DataModule::new(1, 4, 0, 10, 0);
        // 1 番目のブロックは 4 バイト必要だが 3 バイトしかない
        assert_eq!(m.store_block(0, &[1, 2, 3]), StoreBlockResult::Rejected);
        assert!(!m.is_block_downloaded(0));
    }

    #[test]
    fn test_datamodule_store_block_extra_data_ok() {
        // data_size がブロックサイズより大きいのは許可 (必要分だけコピー)
        let mut m = DataModule::new(1, 4, 0, 8, 0);
        assert_eq!(
            m.store_block(0, &[1, 2, 3, 4, 99, 99]),
            StoreBlockResult::Stored
        );
        assert_eq!(&m.data()[0..4], &[1, 2, 3, 4]);
    }

    #[test]
    fn test_datamodule_single_block() {
        // module_size <= block_size → 1 ブロックで完成
        let mut m = DataModule::new(1, 16, 0, 5, 0);
        assert_eq!(m.num_blocks(), 1);
        assert_eq!(
            m.store_block(0, &[1, 2, 3, 4, 5]),
            StoreBlockResult::Completed
        );
        assert_eq!(m.data(), &[1, 2, 3, 4, 5]);
    }

    // ─── DownloadInfoIndication (DII) ───

    /// DII のテストデータを組み立てる。
    /// adaptation_length=0, compat_desc_length=0 とし、モジュール記述子を付与可能。
    fn build_dii(
        block_size: u16,
        modules: &[(u16, u32, u8, Vec<u8>)], // (id, size, version, module_info bytes)
    ) -> Vec<u8> {
        let mut v = Vec::new();
        v.push(0x11); // protocol_discriminator
        v.push(0x03); // dsmcc_type
        v.extend_from_slice(&0x1006u16.to_be_bytes()); // message_id
        v.extend_from_slice(&0x1234_5678u32.to_be_bytes()); // transaction_id
        v.push(0x00); // reserved (data[8])
        v.push(0x00); // adaptation_length (data[9])
        v.extend_from_slice(&0u16.to_be_bytes()); // message_length (data[10..12], 未使用)
        // pos = 12
        v.extend_from_slice(&0xAABB_CCDDu32.to_be_bytes()); // download_id
        v.extend_from_slice(&block_size.to_be_bytes()); // block_size
        v.push(0x01); // window_size
        v.push(0x02); // ack_period
        v.extend_from_slice(&0u32.to_be_bytes()); // tc_download_window
        v.extend_from_slice(&0u32.to_be_bytes()); // tc_download_scenario
        v.extend_from_slice(&0u16.to_be_bytes()); // compat_desc_length = 0
        v.extend_from_slice(&(modules.len() as u16).to_be_bytes()); // number_of_modules
        for (id, size, version, info) in modules {
            v.extend_from_slice(&id.to_be_bytes());
            v.extend_from_slice(&size.to_be_bytes());
            v.push(*version);
            v.push(info.len() as u8); // module_info_length
            v.extend_from_slice(info);
        }
        // 原実装は DataSize < 34 を弾く。ヘッダ + 固定部 + number_of_modules で
        // 32 バイトになるため、モジュールが少ないケースでも 34 バイト以上に
        // なるよう末尾にパディングを補う (number_of_modules 以降は走査されない)。
        while v.len() < 34 {
            v.push(0x00);
        }
        v
    }

    #[test]
    fn test_dii_too_short() {
        assert!(parse_download_info_indication(&[0u8; 33]).is_none());
    }

    #[test]
    fn test_dii_no_modules() {
        let data = build_dii(4066, &[]);
        let (msg, modules) = parse_download_info_indication(&data).unwrap();
        assert_eq!(msg.protocol_discriminator, 0x11);
        assert_eq!(msg.dsmcc_type, 0x03);
        assert_eq!(msg.message_id, 0x1006);
        assert_eq!(msg.transaction_id, 0x1234_5678);
        assert_eq!(msg.download_id, 0xAABB_CCDD);
        assert_eq!(msg.block_size, 4066);
        assert_eq!(msg.window_size, 0x01);
        assert_eq!(msg.ack_period, 0x02);
        assert!(modules.is_empty());
    }

    #[test]
    fn test_dii_block_size_invalid() {
        // block_size=0 は不正
        let data = build_dii(0, &[]);
        assert!(parse_download_info_indication(&data).is_none());
        // block_size > 4066 も不正
        let data = build_dii(4067, &[]);
        assert!(parse_download_info_indication(&data).is_none());
    }

    #[test]
    fn test_dii_with_module_and_descriptors() {
        // モジュール記述子: Name(tag=0x02) "AB" + CRC32(tag=0x05) 0x11223344
        let mut info = Vec::new();
        info.push(0x02); // Name tag
        info.push(0x02); // length
        info.extend_from_slice(b"AB");
        info.push(0x05); // CRC32 tag
        info.push(0x04); // length
        info.extend_from_slice(&0x1122_3344u32.to_be_bytes());

        let data = build_dii(2048, &[(0x0010, 12345, 7, info)]);
        let (_msg, modules) = parse_download_info_indication(&data).unwrap();
        assert_eq!(modules.len(), 1);
        let m = &modules[0];
        assert_eq!(m.module_id, 0x0010);
        assert_eq!(m.module_size, 12345);
        assert_eq!(m.module_version, 7);
        assert_eq!(m.name.text, b"AB");
        assert!(m.crc.is_valid);
        assert_eq!(m.crc.crc32, 0x1122_3344);
    }

    #[test]
    fn test_dii_crc_wrong_length_ignored() {
        // CRC32 tag だが length != 4 → 無視 (is_valid=false)
        let mut info = Vec::new();
        info.push(0x05); // CRC32 tag
        info.push(0x02); // length=2 (不正)
        info.extend_from_slice(&[0xAA, 0xBB]);

        let data = build_dii(2048, &[(0x0001, 100, 0, info)]);
        let (_msg, modules) = parse_download_info_indication(&data).unwrap();
        assert!(!modules[0].crc.is_valid);
    }

    #[test]
    fn test_dii_module_overrun() {
        // module_info_length が実データを超える → None
        let mut data = build_dii(2048, &[(0x0001, 100, 0, vec![])]);
        // 最後のモジュールの module_info_length バイトを大きく改ざん
        let last = data.len() - 1;
        data[last] = 0xFF;
        assert!(parse_download_info_indication(&data).is_none());
    }

    // ─── DownloadDataBlock (DDB) ───

    /// DDB のテストデータを組み立てる (adaptation_length=0)。
    fn build_ddb(module_id: u16, module_version: u8, block_number: u16, block: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        v.push(0x11); // protocol_discriminator
        v.push(0x03); // dsmcc_type
        v.extend_from_slice(&0x1003u16.to_be_bytes()); // message_id
        v.extend_from_slice(&0xAABB_CCDDu32.to_be_bytes()); // download_id
        v.push(0x00); // reserved (data[8])
        v.push(0x00); // adaptation_length (data[9])
        v.extend_from_slice(&0u16.to_be_bytes()); // message_length (data[10..12])
        // pos = 12
        v.extend_from_slice(&module_id.to_be_bytes());
        v.push(module_version);
        v.push(0x00); // reserved (data[pos+3])
        v.extend_from_slice(&block_number.to_be_bytes());
        v.extend_from_slice(block);
        v
    }

    #[test]
    fn test_ddb_too_short() {
        assert!(parse_download_data_block(&[0u8; 11]).is_none());
    }

    #[test]
    fn test_ddb_basic() {
        let block = [10u8, 20, 30, 40];
        let data = build_ddb(0x0010, 5, 3, &block);
        let db = parse_download_data_block(&data).unwrap();
        assert_eq!(db.protocol_discriminator, 0x11);
        assert_eq!(db.dsmcc_type, 0x03);
        assert_eq!(db.message_id, 0x1003);
        assert_eq!(db.download_id, 0xAABB_CCDD);
        assert_eq!(db.module_id, 0x0010);
        assert_eq!(db.module_version, 5);
        assert_eq!(db.block_number, 3);
        assert_eq!(db.data, block);
    }

    #[test]
    fn test_ddb_no_block_data() {
        // ヘッダ直後にブロックデータが無い → None
        // (12 + 0 + 6 >= DataSize: data.len()==18 だと 18>=18 で None)
        let data = build_ddb(0x0001, 0, 0, &[]);
        assert_eq!(data.len(), 18);
        assert!(parse_download_data_block(&data).is_none());
    }

    #[test]
    fn test_ddb_one_byte_block() {
        // 1 バイトだけあれば OK (18 < 19)
        let data = build_ddb(0x0001, 0, 0, &[0x42]);
        let db = parse_download_data_block(&data).unwrap();
        assert_eq!(db.data, vec![0x42]);
    }

    // ─── DII → DataModule 統合 ───

    #[test]
    fn test_dii_to_datamodule_pipeline() {
        // DII でモジュールを宣言 → DataModule を生成 → DDB でブロックを順に投入して完成
        let dii = build_dii(4, &[(0x0020, 10, 1, vec![])]);
        let (msg, modules) = parse_download_info_indication(&dii).unwrap();
        let m = &modules[0];

        let mut module = DataModule::new(
            msg.download_id,
            msg.block_size,
            m.module_id,
            m.module_size,
            m.module_version,
        );
        assert_eq!(module.num_blocks(), 3); // ceil(10/4)

        let blocks: [&[u8]; 3] = [&[1, 2, 3, 4], &[5, 6, 7, 8], &[9, 10]];
        let mut completed = false;
        for (i, b) in blocks.iter().enumerate() {
            let ddb = build_ddb(m.module_id, m.module_version, i as u16, b);
            let db = parse_download_data_block(&ddb).unwrap();
            if module.store_block(db.block_number, &db.data) == StoreBlockResult::Completed {
                completed = true;
            }
        }
        assert!(completed);
        assert!(module.is_complete());
        assert_eq!(module.data(), &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    }
}
