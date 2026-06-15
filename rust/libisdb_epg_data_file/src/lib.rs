// LibISDB の EPG/EPGDataFile.cpp を Rust へ移植したもの。
//
// 移植対象(EPGDataFile.cpp):
//   - EPGData バイナリ構造体定義 (EPGDataFile.cpp:107)
//   - parse_file_header              : EPGDataFile.cpp:449 (Load の FileHeader 読み込み部)
//   - serialize_file_header          : EPGDataFile.cpp:626 (Save の FileHeader 書き込み部)
//   - parse_service_chunk            : LoadService:646
//   - parse_event_chunk              : LoadEvent:675
//   - serialize_service              : SaveService:958
//   - serialize_event                : SaveEvent:980
//
// FileStream / EPGDatabase への依存を排除し、純粋な &[u8] → Vec<EpgService> ↔ Vec<u8> 変換として実装。
// 文字列は原実装の wchar_t → 移植では UTF-16LE u16 スライスを Vec<u16> として保持。

use libisdb_event_info::{
    EventInfo, TypeFlag, ExtendedTextInfo, VideoInfo, AudioInfo,
    ContentNibble, ContentNibbleInfo, EventGroupInfo, EventGroupItem,
    CommonEventInfo,
};
use libisdb_datetime::DateTime;

// ─── バイナリタグ定数 (EPGDataFile.cpp:109) ─────────────────

const TAG_NULL:               u8 = 0x00;
const TAG_END:                u8 = 0x01;
const TAG_SERVICE:            u8 = 0x02;
const TAG_SERVICE_END:        u8 = 0x03;
const TAG_EVENT:              u8 = 0x04;
const TAG_EVENT_END:          u8 = 0x05;
const TAG_EVENT_AUDIO:        u8 = 0x06;
const TAG_EVENT_VIDEO:        u8 = 0x07;
const TAG_EVENT_GENRE:        u8 = 0x08;
const TAG_EVENT_NAME:         u8 = 0x09;
const TAG_EVENT_TEXT:         u8 = 0x0A;
const TAG_EVENT_EXTENDED_TEXT:u8 = 0x0B;
const TAG_EVENT_GROUP:        u8 = 0x0C;

#[allow(dead_code)]
const CHUNK_HEADER_SIZE: usize = 3; // 1(tag) + 2(size u16 LE)
const FILE_HEADER_TYPE: &[u8; 8] = b"EPG-DATA";
const FILE_HEADER_VERSION: u32 = 0;
const MAX_EPG_TEXT_LENGTH: usize = 4096;

// EventInfo flags
const FLAG_RUNNING_STATUS: u16 = 0x0007;
const FLAG_FREE_CA_MODE:   u16 = 0x0008;
const FLAG_BASIC:          u16 = 0x0010;
const FLAG_EXTENDED:       u16 = 0x0020;
const FLAG_PRESENT:        u16 = 0x0040;
const FLAG_FOLLOWING:      u16 = 0x0080;

// AudioInfo flags
const AUDIO_FLAG_MULTI_LINGUAL:  u8 = 0x01;
const AUDIO_FLAG_MAIN_COMPONENT: u8 = 0x02;

// ─── エラー型 ──────────────────────────────────────────────────

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum EpgDataError {
    UnexpectedEnd,
    BadMagic,
    UnsupportedVersion,
    FormatError,
    TextTooLong,
}

impl std::fmt::Display for EpgDataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnexpectedEnd     => write!(f, "unexpected end of data"),
            Self::BadMagic          => write!(f, "bad magic bytes"),
            Self::UnsupportedVersion=> write!(f, "unsupported version"),
            Self::FormatError       => write!(f, "format error"),
            Self::TextTooLong       => write!(f, "text too long"),
        }
    }
}

// ─── サービス情報 ────────────────────────────────────────────

/// サービスキー。EPGDatabase::ServiceInfo 相当。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ServiceKey {
    pub network_id: u16,
    pub transport_stream_id: u16,
    pub service_id: u16,
}

/// サービス+イベントリスト。EPGDataFile::ServiceInfo 相当。
#[derive(Clone, Debug, Default)]
pub struct EpgService {
    pub key: ServiceKey,
    pub events: Vec<EventInfo>,
}

/// ファイルヘッダー。
#[derive(Clone, Copy, Debug, Default)]
pub struct EpgFileHeader {
    pub version: u32,
    pub service_count: u32,
    pub update_count: u64,
}

// ─── カーソルベースのパーサー ─────────────────────────────

struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Self { Self { data, pos: 0 } }
    #[allow(dead_code)]
    fn remaining(&self) -> usize { self.data.len() - self.pos }

    fn read_bytes(&mut self, n: usize) -> Result<&'a [u8], EpgDataError> {
        if self.pos + n > self.data.len() { return Err(EpgDataError::UnexpectedEnd); }
        let s = &self.data[self.pos..self.pos+n];
        self.pos += n;
        Ok(s)
    }
    fn read_u8(&mut self) -> Result<u8, EpgDataError> {
        let b = self.read_bytes(1)?;
        Ok(b[0])
    }
    fn read_u16_le(&mut self) -> Result<u16, EpgDataError> {
        let b = self.read_bytes(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    fn read_u32_le(&mut self) -> Result<u32, EpgDataError> {
        let b = self.read_bytes(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn read_u64_le(&mut self) -> Result<u64, EpgDataError> {
        let b = self.read_bytes(8)?;
        Ok(u64::from_le_bytes(b.try_into().unwrap()))
    }
    fn skip(&mut self, n: usize) -> Result<(), EpgDataError> {
        if self.pos + n > self.data.len() { return Err(EpgDataError::UnexpectedEnd); }
        self.pos += n;
        Ok(())
    }
    fn read_chunk_header(&mut self) -> Result<(u8, usize), EpgDataError> {
        let tag  = self.read_u8()?;
        let size = self.read_u16_le()? as usize;
        Ok((tag, size))
    }
    // UTF-16LE 文字列: 2バイト length(u16) + length個のu16
    fn read_string_u16(&mut self) -> Result<Vec<u16>, EpgDataError> {
        let len = self.read_u16_le()? as usize;
        if len > MAX_EPG_TEXT_LENGTH { return Err(EpgDataError::TextTooLong); }
        if len == 0 { return Ok(vec![]); }
        let bytes = self.read_bytes(len * 2)?;
        let mut v = Vec::with_capacity(len);
        for i in 0..len {
            v.push(u16::from_le_bytes([bytes[i*2], bytes[i*2+1]]));
        }
        Ok(v)
    }
    fn read_datetime(&mut self) -> Result<DateTime, EpgDataError> {
        // EPGDateTime: Year(u16le), Month(u8), DayOfWeek(u8), Day(u8), Hour(u8), Minute(u8), Second(u8)
        let year        = self.read_u16_le()? as i32;
        let month       = self.read_u8()? as i32;
        let day_of_week = self.read_u8()? as i32;
        let day         = self.read_u8()? as i32;
        let hour        = self.read_u8()? as i32;
        let minute      = self.read_u8()? as i32;
        let second      = self.read_u8()? as i32;
        Ok(DateTime { year, month, day, day_of_week, hour, minute, second, millisecond: 0 })
    }
}

// ─── ライター ────────────────────────────────────────────────

struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    fn new() -> Self { Self { buf: Vec::new() } }
    fn write_u8(&mut self, v: u8) { self.buf.push(v); }
    fn write_u16_le(&mut self, v: u16) { self.buf.extend_from_slice(&v.to_le_bytes()); }
    fn write_u32_le(&mut self, v: u32) { self.buf.extend_from_slice(&v.to_le_bytes()); }
    fn write_u64_le(&mut self, v: u64) { self.buf.extend_from_slice(&v.to_le_bytes()); }
    fn write_bytes(&mut self, b: &[u8]) { self.buf.extend_from_slice(b); }
    fn write_datetime(&mut self, dt: &DateTime) {
        self.write_u16_le(dt.year as u16);
        self.write_u8(dt.month as u8);
        self.write_u8(dt.day_of_week as u8);
        self.write_u8(dt.day as u8);
        self.write_u8(dt.hour as u8);
        self.write_u8(dt.minute as u8);
        self.write_u8(dt.second as u8);
    }
    fn write_string_u16(&mut self, s: &[u16]) {
        let len = s.len().min(MAX_EPG_TEXT_LENGTH) as u16;
        self.write_u16_le(len);
        for &c in &s[..len as usize] {
            self.write_u16_le(c);
        }
    }
    fn write_chunk_header(&mut self, tag: u8, size: usize) {
        self.write_u8(tag);
        self.write_u16_le(size as u16);
    }
    fn write_chunk(&mut self, tag: u8, data: &[u8]) {
        self.write_chunk_header(tag, data.len());
        self.write_bytes(data);
    }
    fn finish(self) -> Vec<u8> { self.buf }
}

// ─── EPGDateTime サイズ定数 ──────────────────────────────────

const EPG_DATETIME_SIZE: usize = 2+1+1+1+1+1+1; // 8 bytes

// EventInfo バイナリヘッダサイズ: EventID(2)+Flags(2)+EPGDateTime(8)+Duration(4)+UpdatedTime(8) = 24
const EVENT_HEADER_SIZE: usize = 2+2+EPG_DATETIME_SIZE+4+8;
// ServiceInfo: NID(2)+TSID(2)+SID(2)+EventCount(2) = 8
const SERVICE_HEADER_SIZE: usize = 8;
// FileHeader: Type(8)+Version(4)+ServiceCount(4)+UpdateCount(8) = 24
#[allow(dead_code)]
const FILE_HEADER_SIZE: usize = 8+4+4+8;

// ─── パース関数 ────────────────────────────────────────────

/// ファイルヘッダーをパースする。EPGDataFile.cpp:449。
fn parse_file_header(cur: &mut Cursor) -> Result<EpgFileHeader, EpgDataError> {
    let magic = cur.read_bytes(8)?;
    if magic != FILE_HEADER_TYPE { return Err(EpgDataError::BadMagic); }
    let version       = cur.read_u32_le()?;
    if version > FILE_HEADER_VERSION { return Err(EpgDataError::UnsupportedVersion); }
    let service_count = cur.read_u32_le()?;
    let update_count  = cur.read_u64_le()?;
    Ok(EpgFileHeader { version, service_count, update_count })
}

/// サービスチャンクをパースする。EPGDataFile.cpp:646。
fn parse_service(cur: &mut Cursor, nid: u16, tsid: u16, sid: u16) -> Result<EpgService, EpgDataError> {
    let mut service = EpgService {
        key: ServiceKey { network_id: nid, transport_stream_id: tsid, service_id: sid },
        events: Vec::new(),
    };
    loop {
        let (tag, size) = cur.read_chunk_header()?;
        match tag {
            TAG_SERVICE_END => break,
            TAG_EVENT if size == EVENT_HEADER_SIZE => {
                let event = parse_event(cur, nid, tsid, sid)?;
                service.events.push(event);
            }
            _ => { cur.skip(size)?; }
        }
    }
    Ok(service)
}

/// イベントチャンクをパースする。EPGDataFile.cpp:675。
fn parse_event(cur: &mut Cursor, nid: u16, tsid: u16, sid: u16) -> Result<EventInfo, EpgDataError> {
    let event_id    = cur.read_u16_le()?;
    let flags       = cur.read_u16_le()?;
    let start_time  = cur.read_datetime()?;
    let duration    = cur.read_u32_le()?;
    let updated_time= cur.read_u64_le()?;

    let mut type_flag = TypeFlag::Database;
    if flags & FLAG_BASIC    != 0 { type_flag |= TypeFlag::Basic; }
    if flags & FLAG_EXTENDED != 0 { type_flag |= TypeFlag::Extended; }
    if flags & FLAG_PRESENT  != 0 { type_flag |= TypeFlag::Present; }
    if flags & FLAG_FOLLOWING!= 0 { type_flag |= TypeFlag::Following; }

    let mut event = EventInfo {
        network_id:           nid,
        transport_stream_id:  tsid,
        service_id:           sid,
        event_id,
        start_time,
        duration,
        running_status:       (flags & FLAG_RUNNING_STATUS) as u8,
        free_ca_mode:         (flags & FLAG_FREE_CA_MODE) != 0,
        type_flag,
        updated_time,
        ..Default::default()
    };

    loop {
        let (tag, size) = cur.read_chunk_header()?;
        match tag {
            TAG_EVENT_END => break,
            TAG_EVENT_AUDIO => {
                let audio_count = cur.read_u8()? as usize;
                let mut list = Vec::with_capacity(audio_count);
                for _ in 0..audio_count {
                    let af              = cur.read_u8()?;
                    let stream_content  = cur.read_u8()?;
                    let component_type  = cur.read_u8()?;
                    let component_tag   = cur.read_u8()?;
                    let simulcast_group_tag = cur.read_u8()?;
                    let quality_indicator= cur.read_u8()?;
                    let sampling_rate   = cur.read_u8()?;
                    let _reserved       = cur.read_u8()?;
                    let language_code   = cur.read_u32_le()?;
                    let language_code2  = cur.read_u32_le()?;
                    let text_u16        = cur.read_string_u16()?;
                    let text = String::from_utf16_lossy(&text_u16).to_string();
                    list.push(AudioInfo {
                        stream_content,
                        component_type,
                        component_tag,
                        simulcast_group_tag,
                        es_multi_lingual_flag:  (af & AUDIO_FLAG_MULTI_LINGUAL) != 0,
                        main_component_flag:    (af & AUDIO_FLAG_MAIN_COMPONENT) != 0,
                        quality_indicator,
                        sampling_rate,
                        language_code,
                        language_code2,
                        text,
                    });
                }
                event.audio_list = list;
            }
            TAG_EVENT_VIDEO => {
                let video_count = cur.read_u8()? as usize;
                let mut list = Vec::with_capacity(video_count);
                for _ in 0..video_count {
                    let stream_content = cur.read_u8()?;
                    let component_type = cur.read_u8()?;
                    let component_tag  = cur.read_u8()?;
                    let _reserved      = cur.read_u8()?;
                    let language_code  = cur.read_u32_le()?;
                    let text_u16       = cur.read_string_u16()?;
                    let text = String::from_utf16_lossy(&text_u16).to_string();
                    list.push(VideoInfo {
                        stream_content, component_type, component_tag, language_code, text,
                    });
                }
                event.video_list = list;
            }
            TAG_EVENT_GENRE => {
                let nibble_count = cur.read_u8()? as usize;
                if nibble_count > 7 { return Err(EpgDataError::FormatError); }
                let mut nibble_list = Vec::with_capacity(nibble_count);
                for _ in 0..nibble_count {
                    let content = cur.read_u8()?;
                    let user    = cur.read_u8()?;
                    nibble_list.push(ContentNibble {
                        content_nibble_level1: content >> 4,
                        content_nibble_level2: content & 0x0F,
                        user_nibble1:          user >> 4,
                        user_nibble2:          user & 0x0F,
                    });
                }
                event.content_nibble = ContentNibbleInfo { nibble_list };
            }
            TAG_EVENT_NAME => {
                let u16s = cur.read_string_u16()?;
                event.event_name = String::from_utf16_lossy(&u16s).to_string();
            }
            TAG_EVENT_TEXT => {
                let u16s = cur.read_string_u16()?;
                event.event_text = String::from_utf16_lossy(&u16s).to_string();
            }
            TAG_EVENT_EXTENDED_TEXT => {
                let text_count = cur.read_u8()? as usize;
                let mut ext = Vec::with_capacity(text_count);
                for _ in 0..text_count {
                    let desc_u16 = cur.read_string_u16()?;
                    let text_u16 = cur.read_string_u16()?;
                    ext.push(ExtendedTextInfo {
                        description: String::from_utf16_lossy(&desc_u16).to_string(),
                        text:        String::from_utf16_lossy(&text_u16).to_string(),
                    });
                }
                event.extended_text = ext;
            }
            TAG_EVENT_GROUP => {
                let group_count = cur.read_u8()? as usize;
                let mut groups = Vec::with_capacity(group_count);
                for _ in 0..group_count {
                    let group_type  = cur.read_u8()?;
                    let event_count = cur.read_u8()? as usize;
                    let mut ev_list = Vec::with_capacity(event_count);
                    for _ in 0..event_count {
                        let service_id          = cur.read_u16_le()?;
                        let ev_event_id         = cur.read_u16_le()?;
                        let ev_network_id       = cur.read_u16_le()?;
                        let ev_ts_id            = cur.read_u16_le()?;
                        ev_list.push(EventGroupItem { service_id, event_id: ev_event_id });
                        // GroupType::Common かつ単一イベントでサービスIDが異なる場合
                        if group_type == 1 && event_count == 1 && service_id != sid {
                            event.is_common_event = true;
                            event.common_event = CommonEventInfo {
                                service_id, event_id: ev_event_id,
                            };
                            let _ = (ev_network_id, ev_ts_id);
                        }
                    }
                    groups.push(EventGroupInfo { group_type, event_list: ev_list });
                }
                event.event_group_list = groups;
            }
            TAG_NULL | TAG_END | TAG_SERVICE | TAG_SERVICE_END => {
                cur.skip(size)?;
            }
            _ => { cur.skip(size)?; }
        }
    }
    Ok(event)
}

// ─── 公開 API: パース ─────────────────────────────────────

/// EPG バイナリデータをパースする。
/// 成功時: (EpgFileHeader, Vec<EpgService>)。
pub fn parse(data: &[u8]) -> Result<(EpgFileHeader, Vec<EpgService>), EpgDataError> {
    let mut cur = Cursor::new(data);
    let header = parse_file_header(&mut cur)?;
    let mut services = Vec::new();
    loop {
        let (tag, size) = cur.read_chunk_header()?;
        match tag {
            TAG_END => break,
            TAG_SERVICE if size == SERVICE_HEADER_SIZE => {
                let nid  = cur.read_u16_le()?;
                let tsid = cur.read_u16_le()?;
                let sid  = cur.read_u16_le()?;
                let _event_count = cur.read_u16_le()?;
                let svc = parse_service(&mut cur, nid, tsid, sid)?;
                if !svc.events.is_empty() {
                    services.push(svc);
                }
            }
            _ => { cur.skip(size)?; }
        }
    }
    Ok((header, services))
}

// ─── 公開 API: シリアライズ ──────────────────────────────────

fn string_to_u16(s: &str) -> Vec<u16> { s.encode_utf16().collect() }

fn serialize_event(w: &mut Writer, event: &EventInfo) {
    let mut flags: u16 = event.running_status as u16 & FLAG_RUNNING_STATUS;
    if event.free_ca_mode                            { flags |= FLAG_FREE_CA_MODE; }
    if event.type_flag.contains(TypeFlag::Basic)     { flags |= FLAG_BASIC; }
    if event.type_flag.contains(TypeFlag::Extended)  { flags |= FLAG_EXTENDED; }
    if event.type_flag.contains(TypeFlag::Present)   { flags |= FLAG_PRESENT; }
    if event.type_flag.contains(TypeFlag::Following) { flags |= FLAG_FOLLOWING; }

    // EventInfo チャンクヘッダー(TAG_EVENT + EVENT_HEADER_SIZE)
    let mut evt_hdr = Writer::new();
    evt_hdr.write_u16_le(event.event_id);
    evt_hdr.write_u16_le(flags);
    evt_hdr.write_datetime(&event.start_time);
    evt_hdr.write_u32_le(event.duration);
    evt_hdr.write_u64_le(event.updated_time);
    w.write_chunk(TAG_EVENT, &evt_hdr.finish());

    // Audio
    if !event.audio_list.is_empty() {
        let mut aw = Writer::new();
        aw.write_u8(event.audio_list.len() as u8);
        for a in &event.audio_list {
            let mut af: u8 = 0;
            if a.es_multi_lingual_flag  { af |= AUDIO_FLAG_MULTI_LINGUAL; }
            if a.main_component_flag    { af |= AUDIO_FLAG_MAIN_COMPONENT; }
            aw.write_u8(af);
            aw.write_u8(a.stream_content);
            aw.write_u8(a.component_type);
            aw.write_u8(a.component_tag);
            aw.write_u8(a.simulcast_group_tag);
            aw.write_u8(a.quality_indicator);
            aw.write_u8(a.sampling_rate);
            aw.write_u8(0); // Reserved
            aw.write_u32_le(a.language_code);
            aw.write_u32_le(a.language_code2);
            aw.write_string_u16(&string_to_u16(&a.text));
        }
        let bytes = aw.finish();
        w.write_chunk(TAG_EVENT_AUDIO, &bytes);
    }

    // Video
    if !event.video_list.is_empty() {
        let mut vw = Writer::new();
        vw.write_u8(event.video_list.len() as u8);
        for v in &event.video_list {
            vw.write_u8(v.stream_content);
            vw.write_u8(v.component_type);
            vw.write_u8(v.component_tag);
            vw.write_u8(0); // Reserved
            vw.write_u32_le(v.language_code);
            vw.write_string_u16(&string_to_u16(&v.text));
        }
        w.write_chunk(TAG_EVENT_VIDEO, &vw.finish());
    }

    // Genre
    let nibbles = &event.content_nibble.nibble_list;
    if !nibbles.is_empty() {
        let mut gw = Writer::new();
        let count = nibbles.len().min(7) as u8;
        gw.write_u8(count);
        for n in &nibbles[..count as usize] {
            gw.write_u8((n.content_nibble_level1 << 4) | n.content_nibble_level2);
            gw.write_u8((n.user_nibble1 << 4) | n.user_nibble2);
        }
        w.write_chunk(TAG_EVENT_GENRE, &gw.finish());
    }

    // Event name/text
    if !event.event_name.is_empty() {
        let mut sw = Writer::new();
        sw.write_string_u16(&string_to_u16(&event.event_name));
        w.write_chunk(TAG_EVENT_NAME, &sw.finish());
    }
    if !event.event_text.is_empty() {
        let mut sw = Writer::new();
        sw.write_string_u16(&string_to_u16(&event.event_text));
        w.write_chunk(TAG_EVENT_TEXT, &sw.finish());
    }

    // Extended text
    if !event.extended_text.is_empty() {
        let mut ew = Writer::new();
        ew.write_u8(event.extended_text.len() as u8);
        for et in &event.extended_text {
            ew.write_string_u16(&string_to_u16(&et.description));
            ew.write_string_u16(&string_to_u16(&et.text));
        }
        w.write_chunk(TAG_EVENT_EXTENDED_TEXT, &ew.finish());
    }

    // Event group
    if !event.event_group_list.is_empty() {
        let mut gw = Writer::new();
        gw.write_u8(event.event_group_list.len() as u8);
        for grp in &event.event_group_list {
            gw.write_u8(grp.group_type);
            gw.write_u8(grp.event_list.len() as u8);
            for ev in &grp.event_list {
                gw.write_u16_le(ev.service_id);
                gw.write_u16_le(ev.event_id);
                gw.write_u16_le(0); // NetworkID (simplified)
                gw.write_u16_le(0); // TransportStreamID (simplified)
            }
        }
        w.write_chunk(TAG_EVENT_GROUP, &gw.finish());
    }

    w.write_chunk_header(TAG_EVENT_END, 0);
}

fn serialize_service(w: &mut Writer, svc: &EpgService) {
    // Service ヘッダー
    let mut sh = Writer::new();
    sh.write_u16_le(svc.key.network_id);
    sh.write_u16_le(svc.key.transport_stream_id);
    sh.write_u16_le(svc.key.service_id);
    sh.write_u16_le(svc.events.len() as u16);
    w.write_chunk(TAG_SERVICE, &sh.finish());

    for event in &svc.events {
        serialize_event(w, event);
    }

    w.write_chunk_header(TAG_SERVICE_END, 0);
}

/// EPG データをバイナリにシリアライズする。
pub fn serialize(services: &[EpgService], update_count: u64) -> Vec<u8> {
    let mut w = Writer::new();

    // FileHeader
    w.write_bytes(FILE_HEADER_TYPE);
    w.write_u32_le(FILE_HEADER_VERSION);
    w.write_u32_le(services.len() as u32);
    w.write_u64_le(update_count);

    for svc in services {
        serialize_service(&mut w, svc);
    }

    w.write_chunk_header(TAG_END, 0);
    w.finish()
}

// ─── テスト ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use libisdb_datetime::get_day_of_week;

    fn make_dt(year: i32, month: i32, day: i32, h: i32, m: i32, s: i32) -> DateTime {
        let dow = get_day_of_week(year, month, day);
        DateTime { year, month, day, day_of_week: dow, hour: h, minute: m, second: s, millisecond: 0 }
    }

    fn make_event(event_id: u16, name: &str) -> EventInfo {
        EventInfo {
            network_id: 1,
            transport_stream_id: 2,
            service_id: 3,
            event_id,
            start_time: make_dt(2024, 4, 1, 20, 0, 0),
            duration: 3600,
            event_name: name.to_string(),
            type_flag: TypeFlag::Basic | TypeFlag::Database,
            ..Default::default()
        }
    }

    #[test]
    fn test_file_header_magic() {
        assert_eq!(FILE_HEADER_TYPE, b"EPG-DATA");
    }

    #[test]
    fn test_serialize_parse_roundtrip_empty() {
        let services: Vec<EpgService> = vec![];
        let bytes = serialize(&services, 1);
        let (hdr, svcs) = parse(&bytes).unwrap();
        assert_eq!(hdr.update_count, 1);
        assert!(svcs.is_empty());
    }

    #[test]
    fn test_serialize_parse_roundtrip_single_event() {
        let evt = make_event(100, "テストイベント");
        let svc = EpgService {
            key: ServiceKey { network_id: 1, transport_stream_id: 2, service_id: 3 },
            events: vec![evt],
        };
        let bytes = serialize(&[svc], 5);
        let (hdr, svcs) = parse(&bytes).unwrap();
        assert_eq!(hdr.update_count, 5);
        assert_eq!(svcs.len(), 1);
        assert_eq!(svcs[0].key.service_id, 3);
        assert_eq!(svcs[0].events.len(), 1);
        assert_eq!(svcs[0].events[0].event_id, 100);
        assert_eq!(svcs[0].events[0].event_name, "テストイベント");
    }

    #[test]
    fn test_serialize_parse_type_flags() {
        let mut evt = make_event(200, "flags test");
        evt.type_flag = TypeFlag::Basic | TypeFlag::Extended | TypeFlag::Database;
        let svc = EpgService {
            key: ServiceKey { network_id: 10, transport_stream_id: 20, service_id: 30 },
            events: vec![evt],
        };
        let bytes = serialize(&[svc], 1);
        let (_, svcs) = parse(&bytes).unwrap();
        let e = &svcs[0].events[0];
        assert!(e.type_flag.contains(TypeFlag::Basic));
        assert!(e.type_flag.contains(TypeFlag::Extended));
        assert!(e.type_flag.contains(TypeFlag::Database));
    }

    #[test]
    fn test_serialize_parse_audio_list() {
        let mut evt = make_event(300, "audio test");
        evt.audio_list = vec![AudioInfo {
            stream_content: 2,
            component_type: 1,
            component_tag: 0,
            simulcast_group_tag: 0xFF,
            es_multi_lingual_flag: false,
            main_component_flag: true,
            quality_indicator: 1,
            sampling_rate: 7,
            language_code: 0x6A706E,
            language_code2: 0,
            text: "日本語".to_string(),
        }];
        let svc = EpgService {
            key: ServiceKey { network_id: 1, transport_stream_id: 1, service_id: 1 },
            events: vec![evt],
        };
        let bytes = serialize(&[svc], 1);
        let (_, svcs) = parse(&bytes).unwrap();
        let a = &svcs[0].events[0].audio_list[0];
        assert_eq!(a.language_code, 0x6A706E);
        assert!(a.main_component_flag);
        assert_eq!(a.text, "日本語");
    }

    #[test]
    fn test_serialize_parse_extended_text() {
        let mut evt = make_event(400, "ext test");
        evt.extended_text = vec![
            ExtendedTextInfo { description: "あらすじ".into(), text: "内容テキスト".into() },
        ];
        let svc = EpgService {
            key: ServiceKey { network_id: 1, transport_stream_id: 1, service_id: 1 },
            events: vec![evt],
        };
        let bytes = serialize(&[svc], 1);
        let (_, svcs) = parse(&bytes).unwrap();
        assert_eq!(svcs[0].events[0].extended_text.len(), 1);
        assert_eq!(svcs[0].events[0].extended_text[0].description, "あらすじ");
        assert_eq!(svcs[0].events[0].extended_text[0].text, "内容テキスト");
    }

    #[test]
    fn test_serialize_parse_content_nibble() {
        let mut evt = make_event(500, "genre test");
        evt.content_nibble.nibble_list = vec![
            ContentNibble { content_nibble_level1: 7, content_nibble_level2: 3, user_nibble1: 0, user_nibble2: 0 },
        ];
        let svc = EpgService {
            key: ServiceKey { network_id: 1, transport_stream_id: 1, service_id: 1 },
            events: vec![evt],
        };
        let bytes = serialize(&[svc], 1);
        let (_, svcs) = parse(&bytes).unwrap();
        let n = &svcs[0].events[0].content_nibble.nibble_list[0];
        assert_eq!(n.content_nibble_level1, 7);
        assert_eq!(n.content_nibble_level2, 3);
    }

    #[test]
    fn test_serialize_parse_multiple_services() {
        let svc1 = EpgService {
            key: ServiceKey { network_id: 1, transport_stream_id: 1, service_id: 1 },
            events: vec![make_event(1, "Svc1 Evt1")],
        };
        let svc2 = EpgService {
            key: ServiceKey { network_id: 2, transport_stream_id: 2, service_id: 2 },
            events: vec![make_event(2, "Svc2 Evt1"), make_event(3, "Svc2 Evt2")],
        };
        let bytes = serialize(&[svc1, svc2], 10);
        let (hdr, svcs) = parse(&bytes).unwrap();
        assert_eq!(hdr.update_count, 10);
        assert_eq!(svcs.len(), 2);
        assert_eq!(svcs[0].events.len(), 1);
        assert_eq!(svcs[1].events.len(), 2);
        assert_eq!(svcs[1].events[1].event_name, "Svc2 Evt2");
    }

    #[test]
    fn test_bad_magic() {
        let mut data = serialize(&[], 1);
        data[0] = b'X'; // 破壊
        assert_eq!(parse(&data).unwrap_err(), EpgDataError::BadMagic);
    }

    #[test]
    fn test_unexpected_end() {
        let data = &[b'E', b'P', b'G', b'-']; // 切れている (8バイト未満)
        assert_eq!(parse(data).unwrap_err(), EpgDataError::UnexpectedEnd);
    }

    #[test]
    fn test_datetime_roundtrip() {
        let dt = make_dt(2024, 12, 31, 23, 59, 59);
        let mut w = Writer::new();
        w.write_datetime(&dt);
        let bytes = w.finish();
        let mut cur = Cursor::new(&bytes);
        let dt2 = cur.read_datetime().unwrap();
        assert_eq!(dt.year, dt2.year);
        assert_eq!(dt.month, dt2.month);
        assert_eq!(dt.day, dt2.day);
        assert_eq!(dt.hour, dt2.hour);
        assert_eq!(dt.minute, dt2.minute);
        assert_eq!(dt.second, dt2.second);
    }

    #[test]
    fn test_serialize_parse_video_list() {
        let mut evt = make_event(600, "video test");
        evt.video_list = vec![VideoInfo {
            stream_content: 1,
            component_type: 0xB3,
            component_tag: 0,
            language_code: 0x6A706E,
            text: String::new(),
        }];
        let svc = EpgService {
            key: ServiceKey { network_id: 1, transport_stream_id: 1, service_id: 1 },
            events: vec![evt],
        };
        let bytes = serialize(&[svc], 1);
        let (_, svcs) = parse(&bytes).unwrap();
        let v = &svcs[0].events[0].video_list[0];
        assert_eq!(v.component_type, 0xB3);
    }

    #[test]
    fn test_serialize_free_ca_mode() {
        let mut evt = make_event(700, "ca test");
        evt.free_ca_mode = true;
        let svc = EpgService {
            key: ServiceKey { network_id: 1, transport_stream_id: 1, service_id: 1 },
            events: vec![evt],
        };
        let bytes = serialize(&[svc], 1);
        let (_, svcs) = parse(&bytes).unwrap();
        assert!(svcs[0].events[0].free_ca_mode);
    }

    #[test]
    fn test_serialize_event_group() {
        let mut evt = make_event(800, "group test");
        evt.event_group_list = vec![EventGroupInfo {
            group_type: 1,
            event_list: vec![EventGroupItem { service_id: 99, event_id: 42 }],
        }];
        let svc = EpgService {
            key: ServiceKey { network_id: 1, transport_stream_id: 1, service_id: 1 },
            events: vec![evt],
        };
        let bytes = serialize(&[svc], 1);
        let (_, svcs) = parse(&bytes).unwrap();
        let g = &svcs[0].events[0].event_group_list[0];
        assert_eq!(g.group_type, 1);
        assert_eq!(g.event_list[0].service_id, 99);
    }
}
