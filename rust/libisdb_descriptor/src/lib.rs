// LibISDB の DescriptorBase.cpp + DescriptorBlock.cpp + Descriptors.cpp を Rust へ移植したもの。
//
// 移植対象:
//   - DescriptorBase        : DescriptorBase.cpp:36
//   - DescriptorBlock       : DescriptorBlock.cpp:75
//   - CADescriptor          : Descriptors.cpp:38
//   - NetworkNameDescriptor : Descriptors.cpp:72
//   - ServiceListDescriptor : Descriptors.cpp:113
//   - ServiceDescriptor     : Descriptors.cpp:163
//   - ShortEventDescriptor  : Descriptors.cpp:387
//   - ExtendedEventDescriptor:Descriptors.cpp:434
//   - ComponentDescriptor   : Descriptors.cpp:523
//   - StreamIDDescriptor    : Descriptors.cpp:567
//   - ContentDescriptor     : Descriptors.cpp:592
//   - AudioComponentDescriptor:Descriptors.cpp:800
//   - SeriesDescriptor      : Descriptors.cpp:1650
//   - EventGroupDescriptor  : Descriptors.cpp:1722
//   - LogoTransmissionDescriptor:Descriptors.cpp:1607
//   - TerrestrialDeliverySystemDescriptor:Descriptors.cpp:2187
//   - PartialReceptionDescriptor:Descriptors.cpp:2245
//   - SystemManagementDescriptor:Descriptors.cpp:2360
//
// ARIBString は Vec<u8>(生バイト列)として保持する。

use libisdb_datetime::{mjd_to_datetime, DateTime};

pub const PID_INVALID: u16 = 0x1FFF;
pub const LANGUAGE_CODE_INVALID: u32 = 0;
pub const COMPONENT_TAG_INVALID: u8 = 0xFF;
pub const STREAM_CONTENT_INVALID: u8 = 0xFF;
pub const COMPONENT_TYPE_INVALID: u8 = 0xFF;
pub const STREAM_TYPE_INVALID: u8 = 0xFF;

fn load16(data: &[u8]) -> u16 {
    ((data[0] as u16) << 8) | (data[1] as u16)
}
fn load24(data: &[u8]) -> u32 {
    ((data[0] as u32) << 16) | ((data[1] as u32) << 8) | (data[2] as u32)
}
#[allow(dead_code)]
fn load32(data: &[u8]) -> u32 {
    ((data[0] as u32) << 24) | ((data[1] as u32) << 16) | ((data[2] as u32) << 8) | (data[3] as u32)
}

// ─── DescriptorBase ────────────────────────────────────────────

/// 記述子の基底。DescriptorBase.hpp:34。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DescriptorBase {
    pub tag: u8,
    pub length: u8,
    pub payload: Vec<u8>,
    pub is_valid: bool,
}

impl DescriptorBase {
    pub fn new() -> Self { Self::default() }

    /// DescriptorBase.cpp:53
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 2 { return None; }
        let tag    = data[0];
        let length = data[1];
        let needed = length as usize + 2;
        if data.len() < needed { return None; }
        let payload  = data[2..needed].to_vec();
        let is_valid = length > 0;
        Some(Self { tag, length, payload, is_valid })
    }

    pub fn total_size(&self) -> usize { self.length as usize + 2 }
    pub fn get_tag(&self) -> u8 { self.tag }
    pub fn get_length(&self) -> u8 { self.length }
    pub fn get_payload(&self) -> &[u8] { &self.payload }
    pub fn is_valid(&self) -> bool { self.is_valid }

    pub fn reset(&mut self) {
        self.tag = 0; self.length = 0; self.payload.clear(); self.is_valid = false;
    }
}

// ─── DescriptorBlock ───────────────────────────────────────────

/// 記述子ブロック。DescriptorBlock.hpp:38。
#[derive(Clone, Debug, Default)]
pub struct DescriptorBlock {
    descriptors: Vec<DescriptorBase>,
}

impl DescriptorBlock {
    pub fn new() -> Self { Self::default() }

    /// DescriptorBlock.cpp:75
    pub fn parse_block(&mut self, data: &[u8]) -> usize {
        self.descriptors.clear();
        if data.len() < 2 { return 0; }
        let mut pos = 0;
        while pos + 2 <= data.len() {
            if let Some(desc) = DescriptorBase::parse(&data[pos..]) {
                pos += desc.total_size();
                self.descriptors.push(desc);
            } else {
                break;
            }
        }
        self.descriptors.len()
    }

    pub fn get_descriptor_by_tag(&self, tag: u8) -> Option<&DescriptorBase> {
        self.descriptors.iter().find(|d| d.tag == tag)
    }
    pub fn get_descriptor_by_index(&self, index: usize) -> Option<&DescriptorBase> {
        self.descriptors.get(index)
    }
    pub fn get_descriptor_count(&self) -> usize { self.descriptors.len() }
    pub fn reset(&mut self) { self.descriptors.clear(); }
    pub fn iter(&self) -> impl Iterator<Item = &DescriptorBase> { self.descriptors.iter() }
}

// ─── CADescriptor (tag=0x09) ───────────────────────────────────

/// 限定受信方式記述子。Descriptors.cpp:38。
#[derive(Clone, Debug, Default)]
pub struct CaDescriptor {
    pub ca_system_id: u16,
    pub ca_pid: u16,
    pub private_data: Vec<u8>,
}

impl CaDescriptor {
    pub const TAG: u8 = 0x09;

    /// Descriptors.cpp:56
    pub fn parse(payload: &[u8], length: u8) -> Option<Self> {
        let len = length as usize;
        if len < 4 { return None; }
        if payload.len() < len { return None; }
        if (payload[2] & 0xE0) != 0xE0 { return None; }
        let ca_system_id = load16(&payload[0..2]);
        let ca_pid       = load16(&payload[2..4]) & 0x1FFF;
        let private_data = payload[4..len].to_vec();
        Some(Self { ca_system_id, ca_pid, private_data })
    }

    pub fn from_descriptor(desc: &DescriptorBase) -> Option<Self> {
        if desc.tag != Self::TAG { return None; }
        Self::parse(&desc.payload, desc.length)
    }
}

// ─── NetworkNameDescriptor (tag=0x40) ──────────────────────────

/// ネットワーク名記述子。Descriptors.cpp:72。
#[derive(Clone, Debug, Default)]
pub struct NetworkNameDescriptor {
    pub network_name: Vec<u8>,
}

impl NetworkNameDescriptor {
    pub const TAG: u8 = 0x40;

    pub fn from_descriptor(desc: &DescriptorBase) -> Option<Self> {
        if desc.tag != Self::TAG { return None; }
        Some(Self { network_name: desc.payload.clone() })
    }
}

// ─── ServiceListDescriptor (tag=0x41) ──────────────────────────

/// サービス情報。Descriptors.hpp:ServiceListDescriptor::ServiceInfo。
#[derive(Clone, Copy, Debug, Default)]
pub struct ServiceInfo {
    pub service_id: u16,
    pub service_type: u8,
}

/// サービスリスト記述子。Descriptors.cpp:113。
#[derive(Clone, Debug, Default)]
pub struct ServiceListDescriptor {
    pub service_list: Vec<ServiceInfo>,
}

impl ServiceListDescriptor {
    pub const TAG: u8 = 0x41;

    /// Descriptors.cpp:148
    pub fn from_descriptor(desc: &DescriptorBase) -> Option<Self> {
        if desc.tag != Self::TAG { return None; }
        let p = &desc.payload;
        let len = desc.length as usize;
        if len % 3 != 0 { return None; }
        let mut list = Vec::with_capacity(len / 3);
        let mut pos = 0;
        while pos + 3 <= len {
            list.push(ServiceInfo {
                service_id:   load16(&p[pos..pos+2]),
                service_type: p[pos+2],
            });
            pos += 3;
        }
        Some(Self { service_list: list })
    }
}

// ─── ServiceDescriptor (tag=0x48) ──────────────────────────────

/// サービス記述子。Descriptors.cpp:163。
#[derive(Clone, Debug, Default)]
pub struct ServiceDescriptor {
    pub service_type: u8,
    pub provider_name: Vec<u8>,
    pub service_name: Vec<u8>,
}

impl ServiceDescriptor {
    pub const TAG: u8 = 0x48;

    /// Descriptors.cpp:201
    pub fn from_descriptor(desc: &DescriptorBase) -> Option<Self> {
        if desc.tag != Self::TAG { return None; }
        let p   = &desc.payload;
        let len = desc.length as usize;
        if len < 3 { return None; }
        let service_type  = p[0];
        let prov_len      = p[1] as usize;
        if 2 + prov_len + 1 > len { return None; }
        let provider_name = p[2..2+prov_len].to_vec();
        let svc_len       = p[2+prov_len] as usize;
        if 3 + prov_len + svc_len > len { return None; }
        let service_name  = p[3+prov_len..3+prov_len+svc_len].to_vec();
        Some(Self { service_type, provider_name, service_name })
    }
}

// ─── ShortEventDescriptor (tag=0x4D) ───────────────────────────

/// 短形式イベント記述子。Descriptors.cpp:387。
#[derive(Clone, Debug, Default)]
pub struct ShortEventDescriptor {
    pub language_code: u32,
    pub event_name: Vec<u8>,
    pub event_description: Vec<u8>,
}

impl ShortEventDescriptor {
    pub const TAG: u8 = 0x4D;

    /// Descriptors.cpp:404
    pub fn from_descriptor(desc: &DescriptorBase) -> Option<Self> {
        if desc.tag != Self::TAG { return None; }
        let p   = &desc.payload;
        let len = desc.length as usize;
        if len < 5 { return None; }
        let language_code = load24(&p[0..3]);
        let mut pos = 3usize;
        let name_len = p[pos] as usize; pos += 1;
        let event_name = if name_len > 0 {
            if pos + name_len >= len { return None; }
            let v = p[pos..pos+name_len].to_vec(); pos += name_len; v
        } else { vec![] };
        let desc_len = p[pos] as usize; pos += 1;
        let event_description = if desc_len > 0 {
            if pos + desc_len > len { return None; }
            p[pos..pos+desc_len].to_vec()
        } else { vec![] };
        Some(Self { language_code, event_name, event_description })
    }
}

// ─── ExtendedEventDescriptor (tag=0x4E) ────────────────────────

/// 拡張形式イベント記述子のアイテム。
#[derive(Clone, Debug, Default)]
pub struct ExtendedEventItem {
    pub description: Vec<u8>,
    pub item_char: Vec<u8>,
}

/// 拡張形式イベント記述子。Descriptors.cpp:434。
#[derive(Clone, Debug, Default)]
pub struct ExtendedEventDescriptor {
    pub descriptor_number: u8,
    pub last_descriptor_number: u8,
    pub language_code: u32,
    pub item_list: Vec<ExtendedEventItem>,
    pub text: Vec<u8>,
}

impl ExtendedEventDescriptor {
    pub const TAG: u8 = 0x4E;

    /// Descriptors.cpp:474
    pub fn from_descriptor(desc: &DescriptorBase) -> Option<Self> {
        if desc.tag != Self::TAG { return None; }
        let p   = &desc.payload;
        let len = desc.length as usize;
        if len < 5 { return None; }
        let descriptor_number      = p[0] >> 4;
        let last_descriptor_number = p[0] & 0x0F;
        let language_code          = load24(&p[1..4]);
        let item_len               = p[4] as usize;
        let end_pos                = 5 + item_len;
        if end_pos > len { return None; }
        let mut pos = 5usize;
        let mut item_list = Vec::new();
        while pos < end_pos {
            let desc_len = p[pos] as usize; pos += 1;
            if pos + desc_len > end_pos { break; }
            let description = p[pos..pos+desc_len].to_vec(); pos += desc_len;
            let char_len = p[pos] as usize; pos += 1;
            if pos + char_len > end_pos { break; }
            let item_char_len = char_len.min(220);
            let item_char = p[pos..pos+item_char_len].to_vec(); pos += char_len;
            item_list.push(ExtendedEventItem { description, item_char });
        }
        let text = if end_pos + 1 < len {
            let txt_len = p[end_pos] as usize;
            if end_pos + 1 + txt_len <= len {
                p[end_pos+1..end_pos+1+txt_len].to_vec()
            } else { vec![] }
        } else { vec![] };
        Some(Self { descriptor_number, last_descriptor_number, language_code, item_list, text })
    }
}

// ─── ComponentDescriptor (tag=0x50) ────────────────────────────

/// コンポーネント記述子。Descriptors.cpp:523。
#[derive(Clone, Debug, Default)]
pub struct ComponentDescriptor {
    pub stream_content: u8,
    pub component_type: u8,
    pub component_tag: u8,
    pub language_code: u32,
    pub text: Vec<u8>,
}

impl ComponentDescriptor {
    pub const TAG: u8 = 0x50;

    /// Descriptors.cpp:549
    pub fn from_descriptor(desc: &DescriptorBase) -> Option<Self> {
        if desc.tag != Self::TAG { return None; }
        let p   = &desc.payload;
        let len = desc.length as usize;
        if len < 6 { return None; }
        let stream_content = p[0] & 0x0F;
        if stream_content != 0x01 { return None; }
        let component_type = p[1];
        let component_tag  = p[2];
        let language_code  = load24(&p[3..6]);
        let text = if len > 6 { p[6..len.min(6+16)].to_vec() } else { vec![] };
        Some(Self { stream_content, component_type, component_tag, language_code, text })
    }
}

// ─── StreamIDDescriptor (tag=0x52) ─────────────────────────────

/// ストリーム識別記述子。Descriptors.cpp:567。
#[derive(Clone, Debug, Default)]
pub struct StreamIdDescriptor {
    pub component_tag: u8,
}

impl StreamIdDescriptor {
    pub const TAG: u8 = 0x52;

    pub fn from_descriptor(desc: &DescriptorBase) -> Option<Self> {
        if desc.tag != Self::TAG { return None; }
        if desc.length != 1 { return None; }
        Some(Self { component_tag: desc.payload[0] })
    }
}

// ─── ContentDescriptor (tag=0x54) ──────────────────────────────

/// コンテンツニブル情報。Descriptors.hpp:ContentDescriptor::NibbleInfo。
#[derive(Clone, Copy, Debug, Default)]
pub struct ContentNibbleInfo {
    pub content_nibble_level1: u8,
    pub content_nibble_level2: u8,
    pub user_nibble1: u8,
    pub user_nibble2: u8,
}

/// コンテンツ記述子。Descriptors.cpp:592。
#[derive(Clone, Debug, Default)]
pub struct ContentDescriptor {
    pub nibble_list: Vec<ContentNibbleInfo>,
}

impl ContentDescriptor {
    pub const TAG: u8 = 0x54;

    /// Descriptors.cpp:619
    pub fn from_descriptor(desc: &DescriptorBase) -> Option<Self> {
        if desc.tag != Self::TAG { return None; }
        let len = desc.length as usize;
        if len > 14 { return None; }
        let p = &desc.payload;
        let count = len / 2;
        let mut nibble_list = Vec::with_capacity(count);
        for i in 0..count {
            nibble_list.push(ContentNibbleInfo {
                content_nibble_level1: p[i*2]   >> 4,
                content_nibble_level2: p[i*2]   & 0x0F,
                user_nibble1:          p[i*2+1] >> 4,
                user_nibble2:          p[i*2+1] & 0x0F,
            });
        }
        Some(Self { nibble_list })
    }
}

// ─── AudioComponentDescriptor (tag=0xC4) ───────────────────────

/// 音声コンポーネント記述子。Descriptors.cpp:800。
#[derive(Clone, Debug, Default)]
pub struct AudioComponentDescriptor {
    pub stream_content: u8,
    pub component_type: u8,
    pub component_tag: u8,
    pub stream_type: u8,
    pub simulcast_group_tag: u8,
    pub es_multi_lingual_flag: bool,
    pub main_component_flag: bool,
    pub quality_indicator: u8,
    pub sampling_rate: u8,
    pub language_code: u32,
    pub language_code2: u32,
    pub text: Vec<u8>,
}

impl AudioComponentDescriptor {
    pub const TAG: u8 = 0xC4;

    /// Descriptors.cpp:857
    pub fn from_descriptor(desc: &DescriptorBase) -> Option<Self> {
        if desc.tag != Self::TAG { return None; }
        let p   = &desc.payload;
        let len = desc.length as usize;
        if len < 9 { return None; }
        let stream_content      = p[0] & 0x0F;
        if stream_content != 0x02 { return None; }
        let component_type      = p[1];
        let component_tag       = p[2];
        let stream_type         = p[3];
        let simulcast_group_tag = p[4];
        let es_multi_lingual_flag = (p[5] & 0x80) != 0;
        let main_component_flag   = (p[5] & 0x40) != 0;
        let quality_indicator     = (p[5] & 0x30) >> 4;
        let sampling_rate         = (p[5] & 0x0E) >> 1;
        let language_code         = load24(&p[6..9]);
        let mut pos = 9usize;
        let language_code2 = if es_multi_lingual_flag {
            if pos + 3 > len { return None; }
            let lc2 = load24(&p[pos..pos+3]); pos += 3; lc2
        } else { LANGUAGE_CODE_INVALID };
        let text = if pos < len {
            p[pos..len.min(pos+33)].to_vec()
        } else { vec![] };
        Some(Self {
            stream_content, component_type, component_tag, stream_type, simulcast_group_tag,
            es_multi_lingual_flag, main_component_flag, quality_indicator, sampling_rate,
            language_code, language_code2, text,
        })
    }
}

// ─── LogoTransmissionDescriptor (tag=0xCF) ─────────────────────

pub const LOGO_TRANSMISSION_CDT1: u8 = 0x01;
pub const LOGO_TRANSMISSION_CDT2: u8 = 0x02;
pub const LOGO_TRANSMISSION_CHAR: u8 = 0x03;

/// ロゴ伝送記述子。Descriptors.cpp:1607。
#[derive(Clone, Debug, Default)]
pub struct LogoTransmissionDescriptor {
    pub logo_transmission_type: u8,
    pub logo_id: u16,
    pub logo_version: u16,
    pub download_data_id: u16,
    pub logo_char: Vec<u8>,
}

impl LogoTransmissionDescriptor {
    pub const TAG: u8 = 0xCF;

    /// Descriptors.cpp:1570
    pub fn from_descriptor(desc: &DescriptorBase) -> Option<Self> {
        if desc.tag != Self::TAG { return None; }
        let p   = &desc.payload;
        let len = desc.length as usize;
        if len < 1 { return None; }
        let t = p[0];
        let mut logo_id = 0xFFFF;
        let mut logo_version = 0xFFFF;
        let mut download_data_id = 0xFFFF;
        let mut logo_char = vec![];
        match t {
            LOGO_TRANSMISSION_CDT1 => {
                if len < 7 { return None; }
                logo_id          = load16(&p[1..3]) & 0x01FF;
                logo_version     = load16(&p[3..5]) & 0x0FFF;
                download_data_id = load16(&p[5..7]);
            }
            LOGO_TRANSMISSION_CDT2 => {
                if len < 3 { return None; }
                logo_id = load16(&p[1..3]) & 0x01FF;
            }
            LOGO_TRANSMISSION_CHAR => {
                if len >= 2 {
                    logo_char = p[1..len].to_vec();
                }
            }
            _ => {}
        }
        Some(Self { logo_transmission_type: t, logo_id, logo_version, download_data_id, logo_char })
    }
}

// ─── SeriesDescriptor (tag=0xD5) ───────────────────────────────

pub const SERIES_ID_INVALID: u16 = 0xFFFF;
pub const PROGRAM_PATTERN_INVALID: u8 = 0xFF;

/// シリーズ記述子。Descriptors.cpp:1650。
#[derive(Clone, Debug, Default)]
pub struct SeriesDescriptor {
    pub series_id: u16,
    pub repeat_label: u8,
    pub program_pattern: u8,
    pub expire_date_valid: bool,
    pub expire_date: DateTime,
    pub episode_number: u16,
    pub last_episode_number: u16,
    pub series_name: Vec<u8>,
}

impl SeriesDescriptor {
    pub const TAG: u8 = 0xD5;

    /// Descriptors.cpp:1696
    pub fn from_descriptor(desc: &DescriptorBase) -> Option<Self> {
        if desc.tag != Self::TAG { return None; }
        let p   = &desc.payload;
        let len = desc.length as usize;
        if len < 8 { return None; }
        let series_id           = load16(&p[0..2]);
        let repeat_label        = p[2] >> 4;
        let program_pattern     = (p[2] & 0x0E) >> 1;
        let expire_date_valid   = (p[2] & 0x01) != 0;
        let expire_date = if expire_date_valid {
            mjd_to_datetime(load16(&p[3..5]))
        } else { DateTime::default() };
        let episode_number      = ((p[5] as u16) << 4) | ((p[6] as u16) >> 4);
        let last_episode_number = (((p[6] & 0x0F) as u16) << 8) | (p[7] as u16);
        let series_name = if len > 8 { p[8..len].to_vec() } else { vec![] };
        Some(Self {
            series_id, repeat_label, program_pattern, expire_date_valid, expire_date,
            episode_number, last_episode_number, series_name,
        })
    }
}

// ─── EventGroupDescriptor (tag=0xD6) ───────────────────────────

/// イベントグループ記述子のイベント情報。
#[derive(Clone, Copy, Debug, Default)]
pub struct EventGroupEventInfo {
    pub network_id: u16,
    pub transport_stream_id: u16,
    pub service_id: u16,
    pub event_id: u16,
}

/// イベントグループ記述子。Descriptors.cpp:1722。
#[derive(Clone, Debug, Default)]
pub struct EventGroupDescriptor {
    pub group_type: u8,
    pub event_list: Vec<EventGroupEventInfo>,
}

impl EventGroupDescriptor {
    pub const TAG: u8 = 0xD6;

    /// Descriptors.cpp:1762
    pub fn from_descriptor(desc: &DescriptorBase) -> Option<Self> {
        if desc.tag != Self::TAG { return None; }
        let p   = &desc.payload;
        let len = desc.length as usize;
        if len < 1 { return None; }
        let group_type   = p[0] >> 4;
        let event_count  = (p[0] & 0x0F) as usize;
        let mut event_list = Vec::new();
        const NETWORK_ID_INVALID: u16 = 0xFFFF;
        const TS_ID_INVALID: u16 = 0xFFFF;
        if group_type != 0x04 && group_type != 0x05 {
            let mut pos = 1usize;
            if pos + event_count * 4 > len { return None; }
            for _ in 0..event_count {
                event_list.push(EventGroupEventInfo {
                    service_id:          load16(&p[pos..pos+2]),
                    event_id:            load16(&p[pos+2..pos+4]),
                    network_id:          NETWORK_ID_INVALID,
                    transport_stream_id: TS_ID_INVALID,
                });
                pos += 4;
            }
        } else {
            if event_count != 0 { return None; }
            let mut pos = 1usize;
            while pos + 8 <= len {
                event_list.push(EventGroupEventInfo {
                    network_id:          load16(&p[pos..pos+2]),
                    transport_stream_id: load16(&p[pos+2..pos+4]),
                    service_id:          load16(&p[pos+4..pos+6]),
                    event_id:            load16(&p[pos+6..pos+8]),
                });
                pos += 8;
            }
        }
        Some(Self { group_type, event_list })
    }
}

// ─── HierarchicalTransmissionDescriptor (tag=0xC0) ─────────────

/// 階層伝送記述子。Descriptors.cpp:720 (StoreContents)。
/// ワンセグ等の階層伝送で、対応する高階層/低階層 ES を示す。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HierarchicalTransmissionDescriptor {
    /// 品質レベル (0=低品質/1=高品質)
    pub quality_level: u8,
    /// 参照先 PID (reference_PID, 13bit)
    pub reference_pid: u16,
}

impl HierarchicalTransmissionDescriptor {
    pub const TAG: u8 = 0xC0;

    /// Descriptors.cpp:720
    pub fn from_descriptor(desc: &DescriptorBase) -> Option<Self> {
        if desc.tag != Self::TAG { return None; }
        if desc.length != 3 { return None; }
        let p = &desc.payload;
        let quality_level = p[0] & 0x01;
        let reference_pid = load16(&p[1..3]) & 0x1FFF;
        Some(Self { quality_level, reference_pid })
    }
}

// ─── SatelliteDeliverySystemDescriptor (tag=0x43) ──────────────

/// 衛星分配システム記述子。Descriptors.cpp:197 (StoreContents)。
#[derive(Clone, Debug, Default)]
pub struct SatelliteDeliverySystemDescriptor {
    /// 周波数 (BCD 8桁: GHz単位×100000、例 0x012345678→12.345678GHz相当の生BCD値)
    pub frequency: u32,
    /// 軌道位置 (BCD 4桁)
    pub orbital_position: u16,
    /// 東経/西経フラグ (true=東経)
    pub west_east_flag: bool,
    /// 偏波 (2bit)
    pub polarization: u8,
    /// 変調方式 (5bit)
    pub modulation: u8,
    /// シンボルレート (BCD 7桁)
    pub symbol_rate: u32,
    /// 内符号 (FEC inner, 4bit)
    pub fec_inner: u8,
}

impl SatelliteDeliverySystemDescriptor {
    pub const TAG: u8 = 0x43;

    /// 連続する `digits` 個の BCD ニブルを数値に変換する。
    fn get_bcd(p: &[u8], digits: usize) -> u32 {
        let mut value: u32 = 0;
        for i in 0..digits {
            let byte = p[i / 2];
            let nibble = if i % 2 == 0 { byte >> 4 } else { byte & 0x0F };
            value = value * 10 + nibble as u32;
        }
        value
    }

    /// Descriptors.cpp:197
    pub fn from_descriptor(desc: &DescriptorBase) -> Option<Self> {
        if desc.tag != Self::TAG { return None; }
        if desc.length != 11 { return None; }
        let p = &desc.payload;
        let frequency       = Self::get_bcd(&p[0..4], 8);
        let orbital_position = Self::get_bcd(&p[4..6], 4) as u16;
        let west_east_flag  = (p[6] & 0x80) != 0;
        let polarization    = (p[6] >> 5) & 0x03;
        let modulation      = p[6] & 0x1F;
        let symbol_rate     = Self::get_bcd(&p[7..11], 7);
        let fec_inner       = p[10] & 0x0F;
        Some(Self {
            frequency, orbital_position, west_east_flag,
            polarization, modulation, symbol_rate, fec_inner,
        })
    }
}

// ─── CableDeliverySystemDescriptor (tag=0x44) ──────────────────

/// 有線分配システム記述子。Descriptors.cpp:237 (StoreContents)。
#[derive(Clone, Debug, Default)]
pub struct CableDeliverySystemDescriptor {
    /// 周波数 (BCD 8桁)
    pub frequency: u32,
    /// フレームタイプ (4bit)
    pub frame_type: u8,
    /// 外符号 (FEC outer, 4bit)
    pub fec_outer: u8,
    /// 変調方式 (8bit)
    pub modulation: u8,
    /// シンボルレート (BCD 7桁)
    pub symbol_rate: u32,
    /// 内符号 (FEC inner, 4bit)
    pub fec_inner: u8,
}

impl CableDeliverySystemDescriptor {
    pub const TAG: u8 = 0x44;

    /// Descriptors.cpp:237
    pub fn from_descriptor(desc: &DescriptorBase) -> Option<Self> {
        if desc.tag != Self::TAG { return None; }
        if desc.length != 11 { return None; }
        let p = &desc.payload;
        let frequency   = SatelliteDeliverySystemDescriptor::get_bcd(&p[0..4], 8);
        let frame_type  = (p[5] >> 4) & 0x0F;
        let fec_outer   = p[5] & 0x0F;
        let modulation  = p[6];
        let symbol_rate = SatelliteDeliverySystemDescriptor::get_bcd(&p[7..11], 7);
        let fec_inner   = p[10] & 0x0F;
        Some(Self {
            frequency, frame_type, fec_outer,
            modulation, symbol_rate, fec_inner,
        })
    }
}

// ─── ComponentGroupDescriptor (tag=0xD9) ───────────────────────

/// コンポーネントグループ記述子の CA ユニット情報。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ComponentGroupCaUnit {
    pub ca_unit_id: u8,
    /// このユニットに属する component_tag のリスト
    pub component_tag: Vec<u8>,
}

/// コンポーネントグループ記述子のグループ情報。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ComponentGroupInfo {
    pub component_group_id: u8,
    pub ca_unit_list: Vec<ComponentGroupCaUnit>,
    /// total_bit_rate (TotalBitRateFlag が false のときは 0)
    pub total_bit_rate: u8,
    /// text_char (ARIB 生バイト列)
    pub text: Vec<u8>,
}

// ─── BroadcasterNameDescriptor (tag=0xD8) ──────────────────────

/// ブロードキャスタ名記述子。Descriptors.cpp:1937 (StoreContents)。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BroadcasterNameDescriptor {
    /// broadcaster_name (ARIB 生バイト列)
    pub broadcaster_name: Vec<u8>,
}

impl BroadcasterNameDescriptor {
    pub const TAG: u8 = 0xD8;

    /// Descriptors.cpp:1937
    pub fn from_descriptor(desc: &DescriptorBase) -> Option<Self> {
        if desc.tag != Self::TAG { return None; }
        let len = desc.length as usize;
        // 元実装は m_Length > 0 のとき payload を name に格納、それ以外は空。
        let broadcaster_name = if len > 0 {
            desc.payload[..len].to_vec()
        } else {
            Vec::new()
        };
        Some(Self { broadcaster_name })
    }
}

// ─── ExtendedBroadcasterDescriptor (tag=0xCE) ──────────────────

/// 地上デジタルテレビジョン放送ブロードキャスタの情報。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TerrestrialBroadcasterInfo {
    pub terrestrial_broadcaster_id: u16,
    pub affiliation_id_list: Vec<u8>,
    pub broadcaster_id_list: Vec<BroadcasterIdEntry>,
}

/// broadcaster_id ループの 1 要素。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BroadcasterIdEntry {
    pub original_network_id: u16,
    pub broadcaster_id: u8,
}

/// 拡張ブロードキャスタ記述子。Descriptors.cpp:1511 (StoreContents)。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExtendedBroadcasterDescriptor {
    pub broadcaster_type: u8,
    /// broadcaster_type が地上(0x01)/地上音声(0x02)のときのみ Some。
    pub terrestrial: Option<TerrestrialBroadcasterInfo>,
}

impl ExtendedBroadcasterDescriptor {
    pub const TAG: u8 = 0xCE;
    pub const BROADCASTER_TYPE_TERRESTRIAL: u8 = 0x01;
    pub const BROADCASTER_TYPE_TERRESTRIAL_SOUND: u8 = 0x02;

    /// Descriptors.cpp:1511
    pub fn from_descriptor(desc: &DescriptorBase) -> Option<Self> {
        if desc.tag != Self::TAG { return None; }
        let len = desc.length as usize;
        if len < 1 { return None; }
        let p = &desc.payload;

        let broadcaster_type = p[0] >> 4;

        let mut terrestrial = None;
        if broadcaster_type == Self::BROADCASTER_TYPE_TERRESTRIAL
            || broadcaster_type == Self::BROADCASTER_TYPE_TERRESTRIAL_SOUND
        {
            if len < 4 { return None; }

            let terrestrial_broadcaster_id = load16(&p[1..3]);
            let num_of_affiliation_id = (p[3] >> 4) as usize;
            let num_of_broadcaster_id = (p[3] & 0x0F) as usize;

            if len < 4 + num_of_affiliation_id + num_of_broadcaster_id * 3 {
                return None;
            }

            let affiliation_id_list = p[4..4 + num_of_affiliation_id].to_vec();

            let mut pos = 4 + num_of_affiliation_id;
            let mut broadcaster_id_list = Vec::with_capacity(num_of_broadcaster_id);
            for _ in 0..num_of_broadcaster_id {
                broadcaster_id_list.push(BroadcasterIdEntry {
                    original_network_id: load16(&p[pos..pos + 2]),
                    broadcaster_id: p[pos + 2],
                });
                pos += 3;
            }

            terrestrial = Some(TerrestrialBroadcasterInfo {
                terrestrial_broadcaster_id,
                affiliation_id_list,
                broadcaster_id_list,
            });
        }

        Some(Self { broadcaster_type, terrestrial })
    }
}

// ─── SIParameterDescriptor (tag=0xD7) ──────────────────────────

/// SI 伝送パラメータ記述子の各テーブルごとの情報。Descriptors.hpp:1003。
///
/// 元実装は union で表現していたが、Rust では table_id ごとの enum で表す。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SIParameterTableInfo {
    /// NIT / SDT / BIT / NBIT (table_cycle 8bit)
    Nit { table_cycle: u8 },
    /// SDTT / LDT / CDT (table_cycle 16bit)
    Ldt { table_cycle: u16 },
    /// EIT[p/f other] など (table_cycle 8bit)
    EitPf { table_cycle: u8 },
    /// 地上 H-EIT[p/f], M-EIT, L-EIT
    Hmleit {
        heit_table_cycle: u8,
        meit_table_cycle: u8,
        leit_table_cycle: u8,
        num_of_meit_event: u8,
        num_of_leit_event: u8,
    },
    /// EIT[schedule]
    EitSchedule {
        media_type_list: Vec<SIParameterEitScheduleMediaType>,
    },
}

/// EIT[schedule] の media_type ごとの情報。Descriptors.hpp:1060。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SIParameterEitScheduleMediaType {
    pub media_type: u8,
    pub pattern: u8,
    pub eit_other_flag: bool,
    pub schedule_range: u8,
    pub base_cycle: u16,
    pub cycle_group: Vec<SIParameterEitScheduleCycleGroup>,
}

/// EIT[schedule] の cycle_group。Descriptors.hpp:1067。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SIParameterEitScheduleCycleGroup {
    pub num_of_segment: u8,
    pub cycle: u8,
}

/// SI 伝送パラメータ記述子の 1 テーブルエントリ (table_id + 詳細)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SIParameterTableEntry {
    pub table_id: u8,
    pub info: SIParameterTableInfo,
}

/// SI 伝送パラメータ記述子。Descriptors.cpp:1804 (StoreContents)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SIParameterDescriptor {
    pub parameter_version: u8,
    /// update_time (MJD; 元実装は時刻ゼロの日付のみ)
    pub update_time: libisdb_datetime::DateTime,
    pub table_list: Vec<SIParameterTableEntry>,
}

impl SIParameterDescriptor {
    pub const TAG: u8 = 0xD7;

    pub const TABLE_ID_NIT: u8 = 0x40;
    pub const TABLE_ID_SDT_ACTUAL: u8 = 0x42;
    pub const TABLE_ID_SDT_OTHER: u8 = 0x46;
    pub const TABLE_ID_EIT_PF_ACTUAL: u8 = 0x4E;
    pub const TABLE_ID_EIT_PF_OTHER: u8 = 0x4F;
    pub const TABLE_ID_EIT_SCHEDULE_ACTUAL: u8 = 0x50;
    pub const TABLE_ID_EIT_SCHEDULE_EXTENDED: u8 = 0x58;
    pub const TABLE_ID_EIT_SCHEDULE_OTHER: u8 = 0x60;
    pub const TABLE_ID_SDTT: u8 = 0xC3;
    pub const TABLE_ID_BIT: u8 = 0xC4;
    pub const TABLE_ID_NBIT_MSG: u8 = 0xC5;
    pub const TABLE_ID_NBIT_REF: u8 = 0xC6;
    pub const TABLE_ID_LDT: u8 = 0xC7;
    pub const TABLE_ID_CDT: u8 = 0xC8;

    /// Descriptors.cpp:1804
    pub fn from_descriptor(desc: &DescriptorBase) -> Option<Self> {
        if desc.tag != Self::TAG { return None; }
        let len = desc.length as usize;
        if len < 3 { return None; }
        let p = &desc.payload;

        let parameter_version = p[0];
        let update_time = libisdb_datetime::mjd_to_datetime(load16(&p[1..3]));

        let mut table_list = Vec::new();

        let mut pos = 3usize;
        while pos + 3 <= len {
            let table_id = p[pos];
            let description_length = p[pos + 1] as usize;
            pos += 2;
            if pos + description_length > len { break; }

            let info: Option<SIParameterTableInfo> = match table_id {
                Self::TABLE_ID_NIT
                | Self::TABLE_ID_SDT_ACTUAL
                | Self::TABLE_ID_SDT_OTHER
                | Self::TABLE_ID_BIT
                | Self::TABLE_ID_NBIT_MSG
                | Self::TABLE_ID_NBIT_REF => {
                    if description_length == 1 {
                        Some(SIParameterTableInfo::Nit {
                            table_cycle: Self::bcd8(&p[pos..]),
                        })
                    } else {
                        None
                    }
                }
                Self::TABLE_ID_SDTT | Self::TABLE_ID_LDT | Self::TABLE_ID_CDT => {
                    if description_length == 2 {
                        Some(SIParameterTableInfo::Ldt {
                            table_cycle: SatelliteDeliverySystemDescriptor::get_bcd(&p[pos..], 4) as u16,
                        })
                    } else {
                        None
                    }
                }
                Self::TABLE_ID_EIT_PF_ACTUAL if description_length == 4 => {
                    // Terrestrial (H-EIT[p/f], M-EIT, L-EIT)
                    Some(SIParameterTableInfo::Hmleit {
                        heit_table_cycle: Self::bcd8(&p[pos..]),
                        meit_table_cycle: Self::bcd8(&p[pos + 1..]),
                        leit_table_cycle: Self::bcd8(&p[pos + 2..]),
                        num_of_meit_event: p[pos + 3] >> 4,
                        num_of_leit_event: p[pos + 3] & 0x0F,
                    })
                }
                Self::TABLE_ID_EIT_PF_ACTUAL | Self::TABLE_ID_EIT_PF_OTHER => {
                    // description_length == 4 の EIT_PF_ACTUAL は上で処理済み。
                    if description_length == 1 {
                        Some(SIParameterTableInfo::EitPf {
                            table_cycle: Self::bcd8(&p[pos..]),
                        })
                    } else {
                        None
                    }
                }
                Self::TABLE_ID_EIT_SCHEDULE_ACTUAL
                | Self::TABLE_ID_EIT_SCHEDULE_EXTENDED
                | Self::TABLE_ID_EIT_SCHEDULE_OTHER => {
                    if description_length >= 4 {
                        let end_pos = pos + description_length;
                        let mut media_type_list = Vec::new();
                        let mut q = pos;
                        while q + 4 <= end_pos {
                            let cycle_group_count = (p[q + 3] & 0x03) as usize;
                            let mut media = SIParameterEitScheduleMediaType {
                                media_type: p[q] >> 6,
                                pattern: (p[q] >> 4) & 0x03,
                                eit_other_flag: (p[q] & 0x08) != 0,
                                schedule_range: Self::bcd8(&p[q + 1..]),
                                base_cycle: SatelliteDeliverySystemDescriptor::get_bcd(&p[q + 2..], 3) as u16,
                                cycle_group: Vec::with_capacity(cycle_group_count),
                            };
                            q += 4;
                            if q + cycle_group_count * 2 > end_pos { break; }
                            for _ in 0..cycle_group_count {
                                media.cycle_group.push(SIParameterEitScheduleCycleGroup {
                                    num_of_segment: Self::bcd8(&p[q..]),
                                    cycle: Self::bcd8(&p[q + 1..]),
                                });
                                q += 2;
                            }
                            media_type_list.push(media);
                        }
                        Some(SIParameterTableInfo::EitSchedule { media_type_list })
                    } else {
                        None
                    }
                }
                _ => None,
            };

            if let Some(info) = info {
                table_list.push(SIParameterTableEntry { table_id, info });
            }

            pos += description_length;
        }

        Some(Self { parameter_version, update_time, table_list })
    }

    /// 1 バイト = BCD 2 桁を数値に変換 (GetBCD(uint8_t) 相当)。
    fn bcd8(p: &[u8]) -> u8 {
        SatelliteDeliverySystemDescriptor::get_bcd(p, 2) as u8
    }
}

/// コンポーネントグループ記述子。Descriptors.cpp:1984 (StoreContents)。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ComponentGroupDescriptor {
    pub component_group_type: u8,
    pub total_bit_rate_flag: bool,
    pub group_list: Vec<ComponentGroupInfo>,
}

impl ComponentGroupDescriptor {
    pub const TAG: u8 = 0xD9;

    /// Descriptors.cpp:1984
    pub fn from_descriptor(desc: &DescriptorBase) -> Option<Self> {
        if desc.tag != Self::TAG { return None; }
        let len = desc.length as usize;
        if len < 1 { return None; }
        let p = &desc.payload;

        let component_group_type = p[0] >> 5;
        let total_bit_rate_flag  = (p[0] & 0x10) != 0;
        let num_of_group = (p[0] & 0x0F) as usize;

        let mut group_list = Vec::with_capacity(num_of_group);
        let mut pos = 1usize;

        for _ in 0..num_of_group {
            if pos + 2 > len { break; }
            let mut group = ComponentGroupInfo {
                component_group_id: p[pos] >> 4,
                ..Default::default()
            };
            let num_of_ca_unit = (p[pos] & 0x0F) as usize;
            pos += 1;

            for _ in 0..num_of_ca_unit {
                if pos >= len { return None; }
                let ca_unit_id = p[pos] >> 4;
                let num_of_component = (p[pos] & 0x0F) as usize;
                pos += 1;
                if pos + num_of_component > len { return None; }
                let component_tag = p[pos..pos + num_of_component].to_vec();
                pos += num_of_component;
                group.ca_unit_list.push(ComponentGroupCaUnit { ca_unit_id, component_tag });
            }

            if total_bit_rate_flag {
                if pos >= len { return None; }
                group.total_bit_rate = p[pos];
                pos += 1;
            }

            if pos >= len { return None; }
            let text_length = p[pos] as usize;
            pos += 1;
            if text_length > 0 {
                if pos + text_length > len { return None; }
                group.text = p[pos..pos + text_length].to_vec();
                pos += text_length;
            }

            group_list.push(group);
        }

        Some(Self { component_group_type, total_bit_rate_flag, group_list })
    }
}

// ─── TerrestrialDeliverySystemDescriptor (tag=0xFA) ────────────

/// 地上デジタル伝送方式記述子。Descriptors.cpp:2187。
#[derive(Clone, Debug, Default)]
pub struct TerrestrialDeliverySystemDescriptor {
    pub area_code: u16,
    pub guard_interval: u8,
    pub transmission_mode: u8,
    pub frequency: Vec<u16>,
}

impl TerrestrialDeliverySystemDescriptor {
    pub const TAG: u8 = 0xFA;

    /// Descriptors.cpp:2214
    pub fn from_descriptor(desc: &DescriptorBase) -> Option<Self> {
        if desc.tag != Self::TAG { return None; }
        let p   = &desc.payload;
        let len = desc.length as usize;
        if len < 4 { return None; }
        let area_code        = ((p[0] as u16) << 4) | ((p[1] as u16) >> 4);
        let guard_interval   = (p[1] & 0x0C) >> 2;
        let transmission_mode= p[1] & 0x03;
        let freq_count = (len - 2) / 2;
        let mut frequency = Vec::with_capacity(freq_count);
        for i in 0..freq_count {
            frequency.push(load16(&p[2+i*2..4+i*2]));
        }
        Some(Self { area_code, guard_interval, transmission_mode, frequency })
    }
}

// ─── PartialReceptionDescriptor (tag=0xFB) ─────────────────────

/// 部分受信記述子。Descriptors.cpp:2245。
#[derive(Clone, Debug, Default)]
pub struct PartialReceptionDescriptor {
    pub service_list: Vec<u16>,
}

impl PartialReceptionDescriptor {
    pub const TAG: u8 = 0xFB;

    pub fn from_descriptor(desc: &DescriptorBase) -> Option<Self> {
        if desc.tag != Self::TAG { return None; }
        let p = &desc.payload;
        let count = (desc.length as usize / 2).min(3);
        let service_list = (0..count).map(|i| load16(&p[i*2..i*2+2])).collect();
        Some(Self { service_list })
    }
}

// ─── SystemManagementDescriptor (tag=0xFE) ─────────────────────

/// システム管理記述子。Descriptors.cpp:2360。
#[derive(Clone, Debug, Default)]
pub struct SystemManagementDescriptor {
    pub broadcasting_flag: u8,
    pub broadcasting_id: u8,
    pub additional_broadcasting_id: u8,
}

impl SystemManagementDescriptor {
    pub const TAG: u8 = 0xFE;

    pub fn from_descriptor(desc: &DescriptorBase) -> Option<Self> {
        if desc.tag != Self::TAG { return None; }
        if desc.length != 2 { return None; }
        let p = &desc.payload;
        Some(Self {
            broadcasting_flag:          (p[0] & 0xC0) >> 6,
            broadcasting_id:             p[0] & 0x3F,
            additional_broadcasting_id:  p[1],
        })
    }
}

// ─── tests ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── DescriptorBase ──

    #[test]
    fn test_descriptor_base_parse_valid() {
        let data = [0x09u8, 0x04, 0x00, 0x01, 0x02, 0x03];
        let desc = DescriptorBase::parse(&data).unwrap();
        assert_eq!(desc.tag, 0x09);
        assert_eq!(desc.length, 4);
        assert_eq!(desc.payload, &[0x00, 0x01, 0x02, 0x03]);
        assert!(desc.is_valid());
        assert_eq!(desc.total_size(), 6);
    }

    #[test]
    fn test_descriptor_base_parse_zero_length() {
        let data = [0x40u8, 0x00];
        let desc = DescriptorBase::parse(&data).unwrap();
        assert!(!desc.is_valid());
    }

    #[test]
    fn test_descriptor_base_parse_too_short() {
        assert!(DescriptorBase::parse(&[0x09u8]).is_none());
    }

    #[test]
    fn test_descriptor_base_parse_truncated() {
        assert!(DescriptorBase::parse(&[0x09u8, 0x04, 0x00, 0x01]).is_none());
    }

    #[test]
    fn test_descriptor_base_reset() {
        let mut desc = DescriptorBase::parse(&[0x09u8, 0x02, 0xAA, 0xBB]).unwrap();
        desc.reset();
        assert!(!desc.is_valid());
    }

    // ── DescriptorBlock ──

    #[test]
    fn test_descriptor_block_parse() {
        let data = [0x01u8, 0x02, 0xAA, 0xBB, 0x40, 0x01, 0xCC];
        let mut block = DescriptorBlock::new();
        assert_eq!(block.parse_block(&data), 2);
        assert_eq!(block.get_descriptor_count(), 2);
        assert_eq!(block.get_descriptor_by_index(0).unwrap().tag, 0x01);
        assert_eq!(block.get_descriptor_by_index(1).unwrap().tag, 0x40);
    }

    #[test]
    fn test_descriptor_block_get_by_tag() {
        let data = [0x09u8, 0x02, 0xAA, 0xBB, 0x40, 0x01, 0xCC];
        let mut block = DescriptorBlock::new();
        block.parse_block(&data);
        assert!(block.get_descriptor_by_tag(0x09).is_some());
        assert!(block.get_descriptor_by_tag(0xFF).is_none());
    }

    #[test]
    fn test_descriptor_block_empty() {
        let mut block = DescriptorBlock::new();
        assert_eq!(block.parse_block(&[]), 0);
    }

    #[test]
    fn test_descriptor_block_reset() {
        let mut block = DescriptorBlock::new();
        block.parse_block(&[0x09u8, 0x02, 0xAA, 0xBB]);
        block.reset();
        assert_eq!(block.get_descriptor_count(), 0);
    }

    #[test]
    fn test_descriptor_block_iter() {
        let data = [0x01u8, 0x01, 0xAA, 0x02, 0x01, 0xBB, 0x03, 0x01, 0xCC];
        let mut block = DescriptorBlock::new();
        block.parse_block(&data);
        let tags: Vec<u8> = block.iter().map(|d| d.tag).collect();
        assert_eq!(tags, vec![0x01, 0x02, 0x03]);
    }

    #[test]
    fn test_descriptor_base_equality() {
        let data = [0x09u8, 0x02, 0x01, 0x02];
        assert_eq!(
            DescriptorBase::parse(&data).unwrap(),
            DescriptorBase::parse(&data).unwrap()
        );
    }

    #[test]
    fn test_descriptor_block_truncated_payload() {
        let data = [0x01u8, 0x02, 0xAA, 0xBB, 0x40, 0x04, 0xCC];
        let mut block = DescriptorBlock::new();
        assert_eq!(block.parse_block(&data), 1);
    }

    #[test]
    fn test_descriptor_block_single_byte() {
        let mut block = DescriptorBlock::new();
        assert_eq!(block.parse_block(&[0x09u8]), 0);
    }

    // ── CADescriptor ──

    #[test]
    fn test_ca_descriptor_parse() {
        // CA_system_id=0x0005, CA_PID=0x0101, private=[]
        // pPayload[2] must have 0xE0 set: 0xE1 = 0b11100001
        let payload = [0x00u8, 0x05, 0xE1, 0x01];
        let desc = DescriptorBase { tag: 0x09, length: 4, payload: payload.to_vec(), is_valid: true };
        let ca = CaDescriptor::from_descriptor(&desc).unwrap();
        assert_eq!(ca.ca_system_id, 0x0005);
        assert_eq!(ca.ca_pid, 0x0101);
        assert!(ca.private_data.is_empty());
    }

    #[test]
    fn test_ca_descriptor_wrong_tag() {
        let desc = DescriptorBase { tag: 0x01, length: 4, payload: vec![0,0,0xE0,0], is_valid: true };
        assert!(CaDescriptor::from_descriptor(&desc).is_none());
    }

    // ── NetworkNameDescriptor ──

    #[test]
    fn test_network_name_descriptor() {
        let name = b"TestNet";
        let desc = DescriptorBase { tag: 0x40, length: name.len() as u8, payload: name.to_vec(), is_valid: true };
        let nd = NetworkNameDescriptor::from_descriptor(&desc).unwrap();
        assert_eq!(nd.network_name, name);
    }

    // ── ServiceListDescriptor ──

    #[test]
    fn test_service_list_descriptor() {
        // 2 entries: (0x0001, 0x01), (0x0002, 0x02)
        let payload = [0x00u8, 0x01, 0x01, 0x00, 0x02, 0x02];
        let desc = DescriptorBase { tag: 0x41, length: 6, payload: payload.to_vec(), is_valid: true };
        let sld = ServiceListDescriptor::from_descriptor(&desc).unwrap();
        assert_eq!(sld.service_list.len(), 2);
        assert_eq!(sld.service_list[0].service_id, 1);
        assert_eq!(sld.service_list[1].service_type, 2);
    }

    // ── ServiceDescriptor ──

    #[test]
    fn test_service_descriptor() {
        // type=0x01, provider_len=3, "ABC", service_len=3, "XYZ"
        let mut p = vec![0x01u8, 0x03];
        p.extend_from_slice(b"ABC");
        p.push(0x03);
        p.extend_from_slice(b"XYZ");
        let desc = DescriptorBase { tag: 0x48, length: p.len() as u8, payload: p, is_valid: true };
        let sd = ServiceDescriptor::from_descriptor(&desc).unwrap();
        assert_eq!(sd.service_type, 0x01);
        assert_eq!(sd.provider_name, b"ABC");
        assert_eq!(sd.service_name, b"XYZ");
    }

    // ── ShortEventDescriptor ──

    #[test]
    fn test_short_event_descriptor() {
        // lang=0x6A706E ("jpn"), name_len=3, "ABC", desc_len=2, "DE"
        let mut p = vec![0x6A, 0x70, 0x6E, 0x03];
        p.extend_from_slice(b"ABC");
        p.push(0x02);
        p.extend_from_slice(b"DE");
        let desc = DescriptorBase { tag: 0x4D, length: p.len() as u8, payload: p, is_valid: true };
        let sed = ShortEventDescriptor::from_descriptor(&desc).unwrap();
        assert_eq!(sed.language_code, 0x6A706E);
        assert_eq!(sed.event_name, b"ABC");
        assert_eq!(sed.event_description, b"DE");
    }

    #[test]
    fn test_short_event_descriptor_too_short() {
        let desc = DescriptorBase { tag: 0x4D, length: 3, payload: vec![0,0,0], is_valid: true };
        assert!(ShortEventDescriptor::from_descriptor(&desc).is_none());
    }

    // ── ContentDescriptor ──

    #[test]
    fn test_content_descriptor() {
        // 2 nibbles: (0x01, 0x02, 0x03, 0x04), (0x05, 0x06, 0x07, 0x08)
        let payload = [0x12u8, 0x34, 0x56, 0x78];
        let desc = DescriptorBase { tag: 0x54, length: 4, payload: payload.to_vec(), is_valid: true };
        let cd = ContentDescriptor::from_descriptor(&desc).unwrap();
        assert_eq!(cd.nibble_list.len(), 2);
        assert_eq!(cd.nibble_list[0].content_nibble_level1, 0x01);
        assert_eq!(cd.nibble_list[0].content_nibble_level2, 0x02);
        assert_eq!(cd.nibble_list[1].content_nibble_level1, 0x05);
    }

    #[test]
    fn test_content_descriptor_too_large() {
        let payload = vec![0u8; 16];
        let desc = DescriptorBase { tag: 0x54, length: 16, payload, is_valid: true };
        assert!(ContentDescriptor::from_descriptor(&desc).is_none());
    }

    // ── ComponentDescriptor ──

    #[test]
    fn test_component_descriptor() {
        // stream_content=0x01, component_type=0xB3, tag=0x00, lang=jpn
        let p = vec![0x01u8, 0xB3, 0x00, 0x6A, 0x70, 0x6E];
        let desc = DescriptorBase { tag: 0x50, length: 6, payload: p, is_valid: true };
        let cd = ComponentDescriptor::from_descriptor(&desc).unwrap();
        assert_eq!(cd.stream_content, 0x01);
        assert_eq!(cd.component_type, 0xB3);
        assert!(cd.text.is_empty());
    }

    #[test]
    fn test_component_descriptor_wrong_stream_content() {
        let p = vec![0x02u8, 0xB3, 0x00, 0x6A, 0x70, 0x6E];
        let desc = DescriptorBase { tag: 0x50, length: 6, payload: p, is_valid: true };
        assert!(ComponentDescriptor::from_descriptor(&desc).is_none());
    }

    // ── AudioComponentDescriptor ──

    #[test]
    fn test_audio_component_descriptor() {
        // stream_content=0x02, ...
        let mut p = vec![0x02u8, 0x01, 0x00, 0x0F, 0xFF, 0x40, 0x6A, 0x70, 0x6E];
        p.extend_from_slice(b"Audio");
        let desc = DescriptorBase { tag: 0xC4, length: p.len() as u8, payload: p, is_valid: true };
        let ad = AudioComponentDescriptor::from_descriptor(&desc).unwrap();
        assert_eq!(ad.stream_content, 0x02);
        assert!(ad.main_component_flag);
        assert_eq!(ad.language_code, 0x6A706E);
        assert_eq!(&ad.text, b"Audio");
    }

    #[test]
    fn test_audio_component_descriptor_wrong_stream_content() {
        let p = vec![0x01u8, 0x01, 0x00, 0x0F, 0xFF, 0x40, 0x6A, 0x70, 0x6E];
        let desc = DescriptorBase { tag: 0xC4, length: 9, payload: p, is_valid: true };
        assert!(AudioComponentDescriptor::from_descriptor(&desc).is_none());
    }

    // ── SeriesDescriptor ──

    #[test]
    fn test_series_descriptor() {
        // series_id=0x0001, byte2=0x00 (repeat=0, pattern=0, expire_valid=0)
        // MJD for expire=0x0000 (irrelevant), episode=0x001, last=0x001, name="ドラマ"
        let mut p = vec![0x00u8, 0x01, 0x00, 0x00, 0x00, 0x00, 0x10, 0x01];
        p.extend_from_slice("ドラマ".as_bytes());
        let desc = DescriptorBase { tag: 0xD5, length: p.len() as u8, payload: p, is_valid: true };
        let sd = SeriesDescriptor::from_descriptor(&desc).unwrap();
        assert_eq!(sd.series_id, 0x0001);
        assert!(!sd.expire_date_valid);
        assert_eq!(sd.episode_number, 1);
    }

    // ── EventGroupDescriptor ──

    #[test]
    fn test_event_group_descriptor_basic() {
        // group_type=0x01, count=2: (svc=0x0001,evt=0x0002), (svc=0x0003,evt=0x0004)
        let p = vec![0x12u8, 0x00, 0x01, 0x00, 0x02, 0x00, 0x03, 0x00, 0x04];
        let desc = DescriptorBase { tag: 0xD6, length: p.len() as u8, payload: p, is_valid: true };
        let eg = EventGroupDescriptor::from_descriptor(&desc).unwrap();
        assert_eq!(eg.group_type, 0x01);
        assert_eq!(eg.event_list.len(), 2);
        assert_eq!(eg.event_list[0].service_id, 1);
        assert_eq!(eg.event_list[1].event_id, 4);
    }

    // ── LogoTransmissionDescriptor ──

    #[test]
    fn test_logo_transmission_cdt1() {
        // type=1, logo_id(9bit)=0x0005, version(12bit)=0x001, data_id=0x0100
        // p[1..3] = 0x00 0x05 → logo_id = 5
        // p[3..5] = 0x00 0x01 → logo_version = 1
        // p[5..7] = 0x01 0x00 → download_data_id = 0x0100
        let p = vec![0x01u8, 0x00, 0x05, 0x00, 0x01, 0x01, 0x00];
        let desc = DescriptorBase { tag: 0xCF, length: 7, payload: p, is_valid: true };
        let lt = LogoTransmissionDescriptor::from_descriptor(&desc).unwrap();
        assert_eq!(lt.logo_transmission_type, 1);
        assert_eq!(lt.logo_id, 5);
        assert_eq!(lt.logo_version, 1);
        assert_eq!(lt.download_data_id, 0x0100);
    }

    // ── HierarchicalTransmissionDescriptor ──

    #[test]
    fn test_hierarchical_transmission() {
        // quality_level=1, reference_PID=0x0123 (上位3bitはマスクされる)
        let payload = vec![0x01, 0xE1, 0x23]; // p[1..3]=0xE123 & 0x1FFF = 0x0123
        let desc = DescriptorBase { tag: 0xC0, length: 3, payload, is_valid: true };
        let h = HierarchicalTransmissionDescriptor::from_descriptor(&desc).expect("hierarchical");
        assert_eq!(h.quality_level, 1);
        assert_eq!(h.reference_pid, 0x0123);
    }

    #[test]
    fn test_hierarchical_transmission_bad_length() {
        let desc = DescriptorBase { tag: 0xC0, length: 2, payload: vec![0; 2], is_valid: true };
        assert!(HierarchicalTransmissionDescriptor::from_descriptor(&desc).is_none());
    }

    #[test]
    fn test_hierarchical_transmission_wrong_tag() {
        let desc = DescriptorBase { tag: 0xC1, length: 3, payload: vec![0; 3], is_valid: true };
        assert!(HierarchicalTransmissionDescriptor::from_descriptor(&desc).is_none());
    }

    // ── SatelliteDeliverySystemDescriptor ──

    #[test]
    fn test_satellite_delivery_system() {
        // frequency BCD 8桁 = 12345678, orbital_position BCD 4桁 = 1100
        // p[6]: west_east(1)=1, polarization(2)=01, modulation(5)=00001 → 0b1010_0001 = 0xA1
        // symbol_rate BCD 7桁 = 0234560 (上位7ニブル), fec_inner = p[10]&0x0F
        let payload: [u8; 11] = [
            0x12, 0x34, 0x56, 0x78, // frequency
            0x11, 0x00,             // orbital_position
            0xA1,                   // we(1)+pol(01)+mod(00001)
            0x02, 0x34, 0x56, 0x07, // symbol_rate(7) + fec_inner(low nibble of p[10])
        ];
        let desc = DescriptorBase { tag: 0x43, length: 11, payload: payload.to_vec(), is_valid: true };
        let s = SatelliteDeliverySystemDescriptor::from_descriptor(&desc).expect("satellite");
        assert_eq!(s.frequency, 12345678);
        assert_eq!(s.orbital_position, 1100);
        assert!(s.west_east_flag);
        assert_eq!(s.polarization, 0b01);
        assert_eq!(s.modulation, 0b00001);
        // symbol_rate: BCD 7桁 = p[7..]の上位7ニブル = 0,2,3,4,5,6,0
        assert_eq!(s.symbol_rate, 234560);
        assert_eq!(s.fec_inner, 0x07);
    }

    #[test]
    fn test_satellite_delivery_system_bad_length() {
        let desc = DescriptorBase { tag: 0x43, length: 5, payload: vec![0; 5], is_valid: true };
        assert!(SatelliteDeliverySystemDescriptor::from_descriptor(&desc).is_none());
    }

    // ── CableDeliverySystemDescriptor ──

    #[test]
    fn test_cable_delivery_system() {
        // frequency BCD 8桁 = 12345678
        // p[5]: frame_type(4)=0b0101, fec_outer(4)=0b0010 → 0x52
        // p[6]: modulation = 0x07 (8bit すべて)
        // symbol_rate BCD 7桁 = 0234560, fec_inner = p[10]&0x0F
        let payload: [u8; 11] = [
            0x12, 0x34, 0x56, 0x78, // frequency
            0x00,                   // 未使用
            0x52,                   // frame_type(0101)+fec_outer(0010)
            0x07,                   // modulation (8bit)
            0x02, 0x34, 0x56, 0x07, // symbol_rate(7) + fec_inner(low nibble)
        ];
        let desc = DescriptorBase { tag: 0x44, length: 11, payload: payload.to_vec(), is_valid: true };
        let c = CableDeliverySystemDescriptor::from_descriptor(&desc).expect("cable");
        assert_eq!(c.frequency, 12345678);
        assert_eq!(c.frame_type, 0b0101);
        assert_eq!(c.fec_outer, 0b0010);
        assert_eq!(c.modulation, 0x07);
        assert_eq!(c.symbol_rate, 234560);
        assert_eq!(c.fec_inner, 0x07);
    }

    #[test]
    fn test_cable_delivery_system_bad_length() {
        let desc = DescriptorBase { tag: 0x44, length: 5, payload: vec![0; 5], is_valid: true };
        assert!(CableDeliverySystemDescriptor::from_descriptor(&desc).is_none());
    }

    #[test]
    fn test_cable_delivery_system_wrong_tag() {
        let desc = DescriptorBase { tag: 0x43, length: 11, payload: vec![0; 11], is_valid: true };
        assert!(CableDeliverySystemDescriptor::from_descriptor(&desc).is_none());
    }

    // ── ComponentGroupDescriptor ──

    #[test]
    fn test_component_group_descriptor() {
        // component_group_type=001, total_bit_rate_flag=1, num_of_group=2
        // p[0] = 0b001_1_0010 = 0x32
        // group0: id=1, num_ca_unit=1; ca_unit: id=1, num_comp=2, tags=[0x10,0x11]
        //   total_bit_rate=0x50; text_len=1, text=[0x41 'A' (ESC省略のため生バイト)]
        // group1: id=2, num_ca_unit=0; total_bit_rate=0x60; text_len=0
        let payload: Vec<u8> = vec![
            0x32,                   // type/flag/num_of_group
            // group0
            0x11,                   // group_id=1, num_ca_unit=1
            0x12,                   // ca_unit_id=1, num_component=2
            0x10, 0x11,             // component_tags
            0x50,                   // total_bit_rate
            0x01, 0x41,             // text_len=1, text
            // group1
            0x20,                   // group_id=2, num_ca_unit=0
            0x60,                   // total_bit_rate
            0x00,                   // text_len=0
        ];
        let desc = DescriptorBase { tag: 0xD9, length: payload.len() as u8, payload, is_valid: true };
        let cg = ComponentGroupDescriptor::from_descriptor(&desc).expect("component group");
        assert_eq!(cg.component_group_type, 0b001);
        assert!(cg.total_bit_rate_flag);
        assert_eq!(cg.group_list.len(), 2);

        let g0 = &cg.group_list[0];
        assert_eq!(g0.component_group_id, 1);
        assert_eq!(g0.ca_unit_list.len(), 1);
        assert_eq!(g0.ca_unit_list[0].ca_unit_id, 1);
        assert_eq!(g0.ca_unit_list[0].component_tag, vec![0x10, 0x11]);
        assert_eq!(g0.total_bit_rate, 0x50);
        assert_eq!(g0.text, vec![0x41]);

        let g1 = &cg.group_list[1];
        assert_eq!(g1.component_group_id, 2);
        assert!(g1.ca_unit_list.is_empty());
        assert_eq!(g1.total_bit_rate, 0x60);
        assert!(g1.text.is_empty());
    }

    #[test]
    fn test_component_group_no_bitrate_flag() {
        // total_bit_rate_flag=0, num_of_group=1
        // p[0] = 0b000_0_0001 = 0x01
        // group0: id=0, num_ca_unit=0; text_len=0
        let payload: Vec<u8> = vec![0x01, 0x00, 0x00];
        let desc = DescriptorBase { tag: 0xD9, length: payload.len() as u8, payload, is_valid: true };
        let cg = ComponentGroupDescriptor::from_descriptor(&desc).expect("component group");
        assert!(!cg.total_bit_rate_flag);
        assert_eq!(cg.group_list.len(), 1);
        assert_eq!(cg.group_list[0].total_bit_rate, 0);
    }

    #[test]
    fn test_component_group_wrong_tag() {
        let desc = DescriptorBase { tag: 0xD8, length: 1, payload: vec![0x00], is_valid: true };
        assert!(ComponentGroupDescriptor::from_descriptor(&desc).is_none());
    }

    // ── TerrestrialDeliverySystemDescriptor ──

    #[test]
    fn test_terrestrial_delivery_system() {
        // area_code = (0xAB<<4)|(0xC0>>4) = 0xABC, guard=0b00, mode=0b01
        // freq[0]=0x1234
        let p = vec![0xABu8, 0xC1, 0x12, 0x34];
        let desc = DescriptorBase { tag: 0xFA, length: 4, payload: p, is_valid: true };
        let td = TerrestrialDeliverySystemDescriptor::from_descriptor(&desc).unwrap();
        assert_eq!(td.area_code, 0xABC);
        assert_eq!(td.transmission_mode, 1);
        assert_eq!(td.frequency, vec![0x1234]);
    }

    // ── PartialReceptionDescriptor ──

    #[test]
    fn test_partial_reception_descriptor() {
        let p = vec![0x00u8, 0x01, 0x00, 0x02, 0x00, 0x03];
        let desc = DescriptorBase { tag: 0xFB, length: 6, payload: p, is_valid: true };
        let pr = PartialReceptionDescriptor::from_descriptor(&desc).unwrap();
        assert_eq!(pr.service_list, vec![1, 2, 3]);
    }

    #[test]
    fn test_partial_reception_descriptor_max3() {
        // 4 entries but capped at 3
        let p = vec![0x00u8, 0x01, 0x00, 0x02, 0x00, 0x03, 0x00, 0x04];
        let desc = DescriptorBase { tag: 0xFB, length: 8, payload: p, is_valid: true };
        let pr = PartialReceptionDescriptor::from_descriptor(&desc).unwrap();
        assert_eq!(pr.service_list.len(), 3);
    }

    // ── SystemManagementDescriptor ──

    #[test]
    fn test_system_management_descriptor() {
        // 0xA5 = 0b10100101: flag=0b10=2, id=0b100101=0x25
        let p = vec![0xA5u8, 0x07];
        let desc = DescriptorBase { tag: 0xFE, length: 2, payload: p, is_valid: true };
        let sm = SystemManagementDescriptor::from_descriptor(&desc).unwrap();
        assert_eq!(sm.broadcasting_flag, 2);
        assert_eq!(sm.broadcasting_id, 0x25);
        assert_eq!(sm.additional_broadcasting_id, 7);
    }

    #[test]
    fn test_system_management_descriptor_wrong_length() {
        let desc = DescriptorBase { tag: 0xFE, length: 3, payload: vec![0,0,0], is_valid: true };
        assert!(SystemManagementDescriptor::from_descriptor(&desc).is_none());
    }

    // ── StreamIDDescriptor ──

    #[test]
    fn test_stream_id_descriptor() {
        let desc = DescriptorBase { tag: 0x52, length: 1, payload: vec![0xAB], is_valid: true };
        let sid = StreamIdDescriptor::from_descriptor(&desc).unwrap();
        assert_eq!(sid.component_tag, 0xAB);
    }

    // ── BroadcasterNameDescriptor ──

    #[test]
    fn test_broadcaster_name_descriptor() {
        // ARIB 生バイトとしてそのまま格納される
        let name = vec![0x41u8, 0x42, 0x43];
        let desc = DescriptorBase {
            tag: 0xD8,
            length: name.len() as u8,
            payload: name.clone(),
            is_valid: true,
        };
        let d = BroadcasterNameDescriptor::from_descriptor(&desc).unwrap();
        assert_eq!(d.broadcaster_name, name);
    }

    #[test]
    fn test_broadcaster_name_descriptor_empty() {
        // length=0 のとき空。
        let desc = DescriptorBase { tag: 0xD8, length: 0, payload: vec![], is_valid: true };
        let d = BroadcasterNameDescriptor::from_descriptor(&desc).unwrap();
        assert!(d.broadcaster_name.is_empty());
    }

    #[test]
    fn test_broadcaster_name_descriptor_wrong_tag() {
        let desc = DescriptorBase { tag: 0xD9, length: 1, payload: vec![0x41], is_valid: true };
        assert!(BroadcasterNameDescriptor::from_descriptor(&desc).is_none());
    }

    // ── ExtendedBroadcasterDescriptor ──

    #[test]
    fn test_extended_broadcaster_terrestrial() {
        // broadcaster_type=1(地上), terrestrial_broadcaster_id=0x1234
        // affiliation_id_loop=2, broadcaster_id_loop=1
        // affiliation: 0x0A,0x0B / broadcaster: onid=0x7E87,bid=0x05
        let payload = vec![
            0x10, // broadcaster_type=1 (上位4bit)
            0x12, 0x34, // terrestrial_broadcaster_id
            0x21, // affiliation=2, broadcaster=1
            0x0A, 0x0B, // affiliation_id_list
            0x7E, 0x87, 0x05, // broadcaster_id_list[0]
        ];
        let desc = DescriptorBase {
            tag: 0xCE,
            length: payload.len() as u8,
            payload,
            is_valid: true,
        };
        let d = ExtendedBroadcasterDescriptor::from_descriptor(&desc).unwrap();
        assert_eq!(d.broadcaster_type, 1);
        let t = d.terrestrial.expect("terrestrial info");
        assert_eq!(t.terrestrial_broadcaster_id, 0x1234);
        assert_eq!(t.affiliation_id_list, vec![0x0A, 0x0B]);
        assert_eq!(t.broadcaster_id_list.len(), 1);
        assert_eq!(t.broadcaster_id_list[0].original_network_id, 0x7E87);
        assert_eq!(t.broadcaster_id_list[0].broadcaster_id, 0x05);
    }

    #[test]
    fn test_extended_broadcaster_non_terrestrial() {
        // broadcaster_type=3(地上以外) のときは terrestrial を解析しない
        let desc = DescriptorBase { tag: 0xCE, length: 1, payload: vec![0x30], is_valid: true };
        let d = ExtendedBroadcasterDescriptor::from_descriptor(&desc).unwrap();
        assert_eq!(d.broadcaster_type, 3);
        assert!(d.terrestrial.is_none());
    }

    #[test]
    fn test_extended_broadcaster_terrestrial_truncated() {
        // 地上だが長さが足りない (loop 宣言に対しペイロード不足)
        let payload = vec![0x10, 0x12, 0x34, 0x21, 0x0A]; // affiliation=2 だが 1 バイトしかない
        let desc = DescriptorBase {
            tag: 0xCE,
            length: payload.len() as u8,
            payload,
            is_valid: true,
        };
        assert!(ExtendedBroadcasterDescriptor::from_descriptor(&desc).is_none());
    }

    #[test]
    fn test_extended_broadcaster_wrong_tag() {
        let desc = DescriptorBase { tag: 0xCF, length: 1, payload: vec![0x10], is_valid: true };
        assert!(ExtendedBroadcasterDescriptor::from_descriptor(&desc).is_none());
    }

    // ── SIParameterDescriptor ──

    #[test]
    fn test_si_parameter_descriptor_nit_and_eit_pf() {
        // parameter_version=0x05, update_time MJD=0xC8AB (適当な有効値)
        // table[0]: table_id=0x40(NIT), desc_len=1, BCD=0x12 -> 12
        // table[1]: table_id=0x4F(EIT_PF_OTHER), desc_len=1, BCD=0x03 -> 3
        let payload = vec![
            0x05, // parameter_version
            0xC8, 0xAB, // update_time (MJD)
            0x40, 0x01, 0x12, // NIT, len=1, cycle BCD 12
            0x4F, 0x01, 0x03, // EIT[p/f other], len=1, cycle BCD 3
        ];
        let desc = DescriptorBase {
            tag: 0xD7,
            length: payload.len() as u8,
            payload,
            is_valid: true,
        };
        let d = SIParameterDescriptor::from_descriptor(&desc).unwrap();
        assert_eq!(d.parameter_version, 0x05);
        assert_eq!(d.table_list.len(), 2);
        assert_eq!(d.table_list[0].table_id, 0x40);
        assert_eq!(d.table_list[0].info, SIParameterTableInfo::Nit { table_cycle: 12 });
        assert_eq!(d.table_list[1].table_id, 0x4F);
        assert_eq!(d.table_list[1].info, SIParameterTableInfo::EitPf { table_cycle: 3 });
    }

    #[test]
    fn test_si_parameter_descriptor_hmleit() {
        // table_id=0x4E(EIT_PF_ACTUAL), desc_len=4 -> HMLEIT
        // HEIT BCD=0x11(11), MEIT BCD=0x22(22), LEIT BCD=0x33(33), p[3]=0x21 -> M=2,L=1
        let payload = vec![
            0x01, 0xC8, 0xAB,
            0x4E, 0x04, 0x11, 0x22, 0x33, 0x21,
        ];
        let desc = DescriptorBase {
            tag: 0xD7,
            length: payload.len() as u8,
            payload,
            is_valid: true,
        };
        let d = SIParameterDescriptor::from_descriptor(&desc).unwrap();
        assert_eq!(d.table_list.len(), 1);
        assert_eq!(
            d.table_list[0].info,
            SIParameterTableInfo::Hmleit {
                heit_table_cycle: 11,
                meit_table_cycle: 22,
                leit_table_cycle: 33,
                num_of_meit_event: 2,
                num_of_leit_event: 1,
            }
        );
    }

    #[test]
    fn test_si_parameter_descriptor_eit_schedule() {
        // table_id=0x50(EIT_SCHEDULE_ACTUAL), desc_len=8
        // media_type_list[0]:
        //   p[0]=0b01_01_1_000=0x58 -> media_type=1, pattern=1, eit_other_flag=true
        //   p[1]=schedule_range BCD=0x07 -> 7
        //   p[2..4]=base_cycle: 3 nibble of [0x01,0x21] -> 0,1,2 = 12
        //     p[3]=0x21, cycle_group_count = 0x21 & 0x03 = 1
        //   cycle_group[0]: p[4]=num_of_segment BCD=0x04 -> 4, p[5]=cycle BCD=0x05 -> 5
        let payload = vec![
            0x01, 0xC8, 0xAB,
            0x50, 0x06,
            0x58, // media_type/pattern/eit_other_flag
            0x07, // schedule_range BCD
            0x01, 0x21, // base_cycle (3 nibble -> 012=12), cycle_group_count = 0x21&0x03 = 1
            0x04, 0x05, // cycle_group[0]: num_of_segment=4, cycle=5
        ];
        let desc = DescriptorBase {
            tag: 0xD7,
            length: payload.len() as u8,
            payload,
            is_valid: true,
        };
        let d = SIParameterDescriptor::from_descriptor(&desc).unwrap();
        assert_eq!(d.table_list.len(), 1);
        match &d.table_list[0].info {
            SIParameterTableInfo::EitSchedule { media_type_list } => {
                assert_eq!(media_type_list.len(), 1);
                let m = &media_type_list[0];
                assert_eq!(m.media_type, 1);
                assert_eq!(m.pattern, 1);
                assert!(m.eit_other_flag);
                assert_eq!(m.schedule_range, 7);
                assert_eq!(m.base_cycle, 12);
                assert_eq!(m.cycle_group.len(), 1);
                assert_eq!(m.cycle_group[0].num_of_segment, 4);
                assert_eq!(m.cycle_group[0].cycle, 5);
            }
            other => panic!("unexpected info: {:?}", other),
        }
    }

    #[test]
    fn test_si_parameter_descriptor_too_short() {
        let desc = DescriptorBase { tag: 0xD7, length: 2, payload: vec![0x01, 0x02], is_valid: true };
        assert!(SIParameterDescriptor::from_descriptor(&desc).is_none());
    }

    #[test]
    fn test_si_parameter_descriptor_wrong_tag() {
        let desc = DescriptorBase { tag: 0xD8, length: 3, payload: vec![0x01, 0x02, 0x03], is_valid: true };
        assert!(SIParameterDescriptor::from_descriptor(&desc).is_none());
    }
}
