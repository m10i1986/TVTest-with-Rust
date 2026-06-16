// Rust port of LibISDB/Filters/AnalyzerFilter.cpp の EIT→EventInfo 構築ロジック。
// AnalyzerFilter.cpp:1263 (GetEventInfo) + EventInfo.hpp
//
// AnalyzerFilter は EIT[p/f] をパースして現在/次の番組情報(EventInfo)を構築する。
// その中核である「EIT イベント + 記述子 → EventInfo」の純粋ロジックを切り出す。
// 入力はパース済みの libisdb_ts_tables::EITTable、出力は libisdb_event_info::EventInfo。
// PID マップ・フィルタグラフ・EITスケジュール管理は対象外。
//
// 移植対象:
//   - EIT イベント基本情報(event_id/start_time/duration/running_status/free_ca_mode)
//   - ShortEventDescriptor(0x4D)  → event_name / event_text
//   - ExtendedEventDescriptor(0x4E) → extended_text (descriptor_number 順に結合)
//   - ContentDescriptor(0x54)     → content_nibble (ジャンル)
//
// 文字列は libisdb_arib_string::decode_to_string で ARIB デコードする。
// ComponentDescriptor / AudioComponentDescriptor 等の video/audio_list は今後の拡張。

use libisdb_event_info::{
    EventInfo, ExtendedTextInfo, ContentNibble, ContentNibbleInfo, TypeFlag,
    VideoInfo, AudioInfo, EventGroupInfo, EventGroupItem,
};
use libisdb_ts_tables::{EITTable, EITEventInfo};
use libisdb_descriptor::{
    ShortEventDescriptor, ExtendedEventDescriptor, ContentDescriptor,
    ComponentDescriptor, AudioComponentDescriptor, EventGroupDescriptor,
};
use libisdb_arib_string::{decode_to_string, DecodeFlags};

/// ARIB 文字列(生バイト列)を UTF-8 にデコードする。空や失敗時は空文字列。
fn decode_aribstr(src: &[u8]) -> String {
    if src.is_empty() {
        return String::new();
    }
    decode_to_string(src, DecodeFlags::default()).unwrap_or_default()
}

/// EIT の 1 イベント(EITEventInfo)から EventInfo を構築する。GetEventInfo (AnalyzerFilter.cpp:1263)。
///
/// `network_id` / `transport_stream_id` / `service_id` は EIT のヘッダ由来を呼び出し側が指定する。
pub fn build_event_info(
    event: &EITEventInfo,
    network_id: u16,
    transport_stream_id: u16,
    service_id: u16,
) -> EventInfo {
    let mut info = EventInfo {
        network_id,
        transport_stream_id,
        service_id,
        event_id: event.event_id,
        start_time: event.start_time.clone().unwrap_or_default(),
        duration: event.duration,
        running_status: event.running_status,
        free_ca_mode: event.free_ca_mode,
        type_flag: TypeFlag::Basic,
        ..Default::default()
    };

    // ShortEventDescriptor(0x4D): 番組名・説明 (1個のみ採用)
    for desc in event.descriptors.iter() {
        if let Some(sed) = ShortEventDescriptor::from_descriptor(desc) {
            info.event_name = decode_aribstr(&sed.event_name);
            info.event_text = decode_aribstr(&sed.event_description);
            break;
        }
    }

    // ExtendedEventDescriptor(0x4E): 拡張テキスト
    // descriptor_number 順に並んだ複数記述子を結合し、同一 description の item をまとめる。
    // 簡易実装: 各 item を (description, item_char) のペアとして順に追加。
    // 同じ description が連続する場合(分割テキスト)は item_char を連結する。
    let mut ext: Vec<ExtendedTextInfo> = Vec::new();
    {
        // descriptor_number 順にソートして処理
        let mut ext_descs: Vec<ExtendedEventDescriptor> = event
            .descriptors
            .iter()
            .filter_map(ExtendedEventDescriptor::from_descriptor)
            .collect();
        ext_descs.sort_by_key(|d| d.descriptor_number);

        for d in &ext_descs {
            for item in &d.item_list {
                let description = decode_aribstr(&item.description);
                let text = decode_aribstr(&item.item_char);
                // description が空(継続行)の場合は直前のテキストに連結
                if description.is_empty() {
                    if let Some(last) = ext.last_mut() {
                        last.text.push_str(&text);
                        continue;
                    }
                }
                ext.push(ExtendedTextInfo { description, text });
            }
        }
    }
    info.extended_text = ext;
    if !info.extended_text.is_empty() {
        info.type_flag |= TypeFlag::Extended;
    }

    // ContentDescriptor(0x54): ジャンル(content_nibble)
    for desc in event.descriptors.iter() {
        if let Some(cd) = ContentDescriptor::from_descriptor(desc) {
            let nibble_list: Vec<ContentNibble> = cd
                .nibble_list
                .iter()
                .map(|n| ContentNibble {
                    content_nibble_level1: n.content_nibble_level1,
                    content_nibble_level2: n.content_nibble_level2,
                    user_nibble1: n.user_nibble1,
                    user_nibble2: n.user_nibble2,
                })
                .collect();
            info.content_nibble = ContentNibbleInfo { nibble_list };
            break;
        }
    }

    // ComponentDescriptor(0x50): 映像コンポーネント (stream_content=0x01)
    for desc in event.descriptors.iter() {
        if let Some(cd) = ComponentDescriptor::from_descriptor(desc) {
            info.video_list.push(VideoInfo {
                stream_content: cd.stream_content,
                component_type: cd.component_type,
                component_tag: cd.component_tag,
                language_code: cd.language_code,
                text: decode_aribstr(&cd.text),
            });
        }
    }

    // AudioComponentDescriptor(0xC4): 音声コンポーネント (stream_content=0x02)
    for desc in event.descriptors.iter() {
        if let Some(ad) = AudioComponentDescriptor::from_descriptor(desc) {
            info.audio_list.push(AudioInfo {
                stream_content: ad.stream_content,
                component_type: ad.component_type,
                component_tag: ad.component_tag,
                simulcast_group_tag: ad.simulcast_group_tag,
                es_multi_lingual_flag: ad.es_multi_lingual_flag,
                main_component_flag: ad.main_component_flag,
                quality_indicator: ad.quality_indicator,
                sampling_rate: ad.sampling_rate,
                language_code: ad.language_code,
                language_code2: ad.language_code2,
                text: decode_aribstr(&ad.text),
            });
        }
    }

    // EventGroupDescriptor(0xD6): イベントグループ (共有/リレー)
    for desc in event.descriptors.iter() {
        if let Some(egd) = EventGroupDescriptor::from_descriptor(desc) {
            let event_list: Vec<EventGroupItem> = egd
                .event_list
                .iter()
                .map(|e| EventGroupItem {
                    service_id: e.service_id,
                    event_id: e.event_id,
                })
                .collect();
            info.event_group_list.push(EventGroupInfo {
                group_type: egd.group_type,
                event_list,
            });
        }
    }

    info
}

/// EITTable の全イベントから EventInfo のリストを構築する。
///
/// network_id / transport_stream_id / service_id は EITTable のヘッダから取得する。
pub fn build_event_list(eit: &EITTable) -> Vec<EventInfo> {
    let nid = eit.get_original_network_id();
    let tsid = eit.get_transport_stream_id();
    let sid = eit.get_service_id();

    let mut list = Vec::with_capacity(eit.get_event_count());
    for i in 0..eit.get_event_count() {
        if let Some(ev) = eit.get_event(i) {
            list.push(build_event_info(ev, nid, tsid, sid));
        }
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;
    use libisdb_ts_tables::EITTable;
    use libisdb_ts_packet::{TsPacket, TS_PACKET_SIZE};

    // CRC32-MPEG2 (init=0xFFFFFFFF, no final xor) — Tables.cpp と同じ
    fn crc32_mpeg2_calc(data: &[u8]) -> u32 {
        const POLY: u32 = 0x04C1_1DB7;
        let mut crc: u32 = 0xFFFF_FFFF;
        for &b in data {
            crc ^= (b as u32) << 24;
            for _ in 0..8 {
                if crc & 0x8000_0000 != 0 {
                    crc = (crc << 1) ^ POLY;
                } else {
                    crc <<= 1;
                }
            }
        }
        crc
    }

    /// ShortEventDescriptor(0x4D) を組み立てる。
    fn build_short_event_desc(lang: u32, name: &[u8], desc: &[u8]) -> Vec<u8> {
        let mut p = Vec::new();
        p.extend_from_slice(&lang.to_be_bytes()[1..4]); // 24bit language_code
        p.push(name.len() as u8);
        p.extend_from_slice(name);
        p.push(desc.len() as u8);
        p.extend_from_slice(desc);
        let mut d = vec![0x4D, p.len() as u8];
        d.extend_from_slice(&p);
        d
    }

    /// ContentDescriptor(0x54) を組み立てる(1ニブル)。
    fn build_content_desc(level1: u8, level2: u8) -> Vec<u8> {
        // content_nibble: level1<<4|level2, user_nibble1<<4|user_nibble2
        let body = [(level1 << 4) | (level2 & 0x0F), 0xFF];
        let mut d = vec![0x54, body.len() as u8];
        d.extend_from_slice(&body);
        d
    }

    /// ComponentDescriptor(0x50) を組み立てる。stream_content=0x01 固定。
    fn build_component_desc(component_type: u8, component_tag: u8, text: &[u8]) -> Vec<u8> {
        let mut p = Vec::new();
        p.push(0x01); // reserved(4) + stream_content(4)=0x01
        p.push(component_type);
        p.push(component_tag);
        p.extend_from_slice(&0x6A706Eu32.to_be_bytes()[1..4]); // language_code jpn
        p.extend_from_slice(text);
        let mut d = vec![0x50, p.len() as u8];
        d.extend_from_slice(&p);
        d
    }

    /// AudioComponentDescriptor(0xC4) を組み立てる。stream_content=0x02 固定、多言語なし。
    fn build_audio_component_desc(
        component_type: u8,
        component_tag: u8,
        main_component: bool,
        text: &[u8],
    ) -> Vec<u8> {
        let mut p = Vec::new();
        p.push(0x02); // reserved(4) + stream_content(4)=0x02
        p.push(component_type);
        p.push(component_tag);
        p.push(0x00); // stream_type
        p.push(0xFF); // simulcast_group_tag
        // ES_multi_lingual_flag(1)=0 + main_component_flag(1) + quality(2) + sampling(3) + reserved(1)
        let flags = (if main_component { 0x40 } else { 0x00 }) | (0x01 << 4) | (0x07 << 1);
        p.push(flags);
        p.extend_from_slice(&0x6A706Eu32.to_be_bytes()[1..4]); // language_code jpn
        p.extend_from_slice(text);
        let mut d = vec![0xC4, p.len() as u8];
        d.extend_from_slice(&p);
        d
    }

    /// EventGroupDescriptor(0xD6) を組み立てる。共有(group_type=1)、events: (service_id, event_id)。
    fn build_event_group_desc(group_type: u8, events: &[(u16, u16)]) -> Vec<u8> {
        let mut p = Vec::new();
        p.push((group_type << 4) | (events.len() as u8 & 0x0F));
        for &(sid, eid) in events {
            p.extend_from_slice(&sid.to_be_bytes());
            p.extend_from_slice(&eid.to_be_bytes());
        }
        let mut d = vec![0xD6, p.len() as u8];
        d.extend_from_slice(&p);
        d
    }

    /// EIT[p/f] セクションを 1 イベント分組み立てて EITTable に store する。
    /// mjd_date/bcd_time/dur はそのままバイト列で渡す。
    fn make_eit_table(
        service_id: u16,
        tsid: u16,
        onid: u16,
        event_id: u16,
        start_time: [u8; 5],
        duration: [u8; 3],
        running_status: u8,
        free_ca_mode: bool,
        descriptors: &[u8],
    ) -> EITTable {
        // イベントループ本体
        let mut events = Vec::new();
        events.extend_from_slice(&event_id.to_be_bytes());
        events.extend_from_slice(&start_time);
        events.extend_from_slice(&duration);
        let dll = descriptors.len();
        let b = ((running_status & 0x07) << 5)
            | (if free_ca_mode { 0x10 } else { 0x00 })
            | ((dll >> 8) as u8 & 0x0F);
        events.push(b);
        events.push((dll & 0xFF) as u8);
        events.extend_from_slice(descriptors);

        // body: transport_stream_id(2) + original_network_id(2) +
        //       segment_last_section_number(1) + last_table_id(1) + events
        let mut body = Vec::new();
        body.extend_from_slice(&tsid.to_be_bytes());
        body.extend_from_slice(&onid.to_be_bytes());
        body.push(0x00); // segment_last_section_number
        body.push(0x4E); // last_table_id
        body.extend_from_slice(&events);

        // section header (table_id = 0x4E EIT p/f actual)
        let table_id = 0x4Eu8;
        let section_length = 5 + body.len() + 4;
        let mut sec = Vec::new();
        sec.push(table_id);
        sec.push(0xB0 | ((section_length >> 8) as u8));
        sec.push((section_length & 0xFF) as u8);
        sec.extend_from_slice(&service_id.to_be_bytes()); // table_id_extension = service_id
        sec.push(0xC1); // version=0, current_next=1
        sec.push(0x00); // section_number
        sec.push(0x00); // last_section_number
        sec.extend_from_slice(&body);
        let crc = crc32_mpeg2_calc(&sec);
        sec.extend_from_slice(&crc.to_be_bytes());

        // TS パケットに載せる (PID = 0x0012 HEIT)
        let pid = 0x0012u16;
        let mut data = [0xFFu8; TS_PACKET_SIZE];
        data[0] = 0x47;
        data[1] = 0x40 | ((pid >> 8) as u8 & 0x1F);
        data[2] = (pid & 0xFF) as u8;
        data[3] = 0x10;
        data[4] = 0x00;
        data[5..5 + sec.len()].copy_from_slice(&sec);

        let mut pkt = TsPacket::new(&data);
        pkt.parse_packet(None);

        let mut table = EITTable::new();
        table.store_packet(&pkt);
        table
    }

    // MJD 2024-04-01 = 0xE0CB あたり。正確な値はテストで日時検証しないので固定。
    const START_TIME: [u8; 5] = [0xE0, 0xCB, 0x20, 0x00, 0x00]; // MJD + BCD 20:00:00
    const DURATION: [u8; 3] = [0x01, 0x30, 0x00]; // BCD 01:30:00

    #[test]
    fn test_build_event_info_basic() {
        // ShortEventDescriptor: 番組名 + 説明。ARIB G0=Alphanumeric に切替えて ASCII。
        let name: &[u8] = &[0x1B, 0x28, 0x4A, 0x41]; // 'Ａ'
        let desc: &[u8] = &[0x1B, 0x28, 0x4A, 0x42]; // 'Ｂ'
        let sed = build_short_event_desc(0x6A706E, name, desc);

        let eit = make_eit_table(
            0x0400, 0x7FE0, 0x0004, 0x1234, START_TIME, DURATION, 4, false, &sed,
        );
        assert_eq!(eit.get_event_count(), 1);

        let list = build_event_list(&eit);
        assert_eq!(list.len(), 1);
        let ev = &list[0];
        assert_eq!(ev.service_id, 0x0400);
        assert_eq!(ev.transport_stream_id, 0x7FE0);
        assert_eq!(ev.network_id, 0x0004);
        assert_eq!(ev.event_id, 0x1234);
        assert_eq!(ev.running_status, 4);
        assert!(!ev.free_ca_mode);
        assert!(!ev.event_name.is_empty());
        assert!(!ev.event_text.is_empty());
        assert!(ev.type_flag.contains(TypeFlag::Basic));
    }

    #[test]
    fn test_build_event_info_content_nibble() {
        let name: &[u8] = &[0x1B, 0x28, 0x4A, 0x41];
        let mut descs = build_short_event_desc(0x6A706E, name, &[]);
        // ジャンル: level1=7(バラエティ), level2=3
        descs.extend_from_slice(&build_content_desc(7, 3));

        let eit = make_eit_table(
            0x0400, 0x7FE0, 0x0004, 0x1234, START_TIME, DURATION, 4, false, &descs,
        );
        let list = build_event_list(&eit);
        let ev = &list[0];
        assert_eq!(ev.content_nibble.nibble_list.len(), 1);
        assert_eq!(ev.content_nibble.nibble_list[0].content_nibble_level1, 7);
        assert_eq!(ev.content_nibble.nibble_list[0].content_nibble_level2, 3);
    }

    #[test]
    fn test_build_event_info_free_ca_mode() {
        let name: &[u8] = &[0x1B, 0x28, 0x4A, 0x41];
        let sed = build_short_event_desc(0x6A706E, name, &[]);
        let eit = make_eit_table(
            0x0400, 0x7FE0, 0x0004, 0x1234, START_TIME, DURATION, 2, true, &sed,
        );
        let list = build_event_list(&eit);
        assert!(list[0].free_ca_mode);
        assert_eq!(list[0].running_status, 2);
    }

    #[test]
    fn test_build_event_info_no_descriptors() {
        let eit = make_eit_table(
            0x0400, 0x7FE0, 0x0004, 0x1234, START_TIME, DURATION, 0, false, &[],
        );
        let list = build_event_list(&eit);
        assert_eq!(list.len(), 1);
        assert!(list[0].event_name.is_empty());
        assert!(list[0].content_nibble.nibble_list.is_empty());
    }

    #[test]
    fn test_build_event_info_event_id_preserved() {
        let name: &[u8] = &[0x1B, 0x28, 0x4A, 0x41];
        let sed = build_short_event_desc(0x6A706E, name, &[]);
        let eit = make_eit_table(
            0x0500, 0x1234, 0x0007, 0xABCD, START_TIME, DURATION, 1, false, &sed,
        );
        let list = build_event_list(&eit);
        assert_eq!(list[0].event_id, 0xABCD);
        assert_eq!(list[0].service_id, 0x0500);
        assert_eq!(list[0].network_id, 0x0007);
    }

    #[test]
    fn test_build_event_info_video_component() {
        let name: &[u8] = &[0x1B, 0x28, 0x4A, 0x41];
        let mut descs = build_short_event_desc(0x6A706E, name, &[]);
        // 映像コンポーネント: component_type=0xB3(1080i), tag=0x00
        descs.extend_from_slice(&build_component_desc(0xB3, 0x00, &[]));

        let eit = make_eit_table(
            0x0400, 0x7FE0, 0x0004, 0x1234, START_TIME, DURATION, 4, false, &descs,
        );
        let list = build_event_list(&eit);
        let ev = &list[0];
        assert_eq!(ev.video_list.len(), 1);
        assert_eq!(ev.video_list[0].stream_content, 0x01);
        assert_eq!(ev.video_list[0].component_type, 0xB3);
        assert_eq!(ev.video_list[0].component_tag, 0x00);
        assert_eq!(ev.video_list[0].language_code, 0x6A706E);
    }

    #[test]
    fn test_build_event_info_audio_component() {
        let name: &[u8] = &[0x1B, 0x28, 0x4A, 0x41];
        let mut descs = build_short_event_desc(0x6A706E, name, &[]);
        // 音声コンポーネント: component_type=0x03(ステレオ), tag=0x10, main
        descs.extend_from_slice(&build_audio_component_desc(0x03, 0x10, true, &[]));

        let eit = make_eit_table(
            0x0400, 0x7FE0, 0x0004, 0x1234, START_TIME, DURATION, 4, false, &descs,
        );
        let list = build_event_list(&eit);
        let ev = &list[0];
        assert_eq!(ev.audio_list.len(), 1);
        assert_eq!(ev.audio_list[0].stream_content, 0x02);
        assert_eq!(ev.audio_list[0].component_type, 0x03);
        assert_eq!(ev.audio_list[0].component_tag, 0x10);
        assert!(ev.audio_list[0].main_component_flag);
        assert!(!ev.audio_list[0].es_multi_lingual_flag);
        assert_eq!(ev.audio_list[0].language_code, 0x6A706E);
    }

    #[test]
    fn test_build_event_info_video_and_audio() {
        let name: &[u8] = &[0x1B, 0x28, 0x4A, 0x41];
        let mut descs = build_short_event_desc(0x6A706E, name, &[]);
        descs.extend_from_slice(&build_component_desc(0xB3, 0x00, &[]));
        descs.extend_from_slice(&build_audio_component_desc(0x03, 0x10, true, &[]));

        let eit = make_eit_table(
            0x0400, 0x7FE0, 0x0004, 0x1234, START_TIME, DURATION, 4, false, &descs,
        );
        let list = build_event_list(&eit);
        let ev = &list[0];
        assert_eq!(ev.video_list.len(), 1);
        assert_eq!(ev.audio_list.len(), 1);
    }

    #[test]
    fn test_build_event_info_event_group() {
        let name: &[u8] = &[0x1B, 0x28, 0x4A, 0x41];
        let mut descs = build_short_event_desc(0x6A706E, name, &[]);
        // 共有(group_type=1): 2サービスの共有イベント
        descs.extend_from_slice(&build_event_group_desc(1, &[(0x0401, 0x1111), (0x0402, 0x2222)]));

        let eit = make_eit_table(
            0x0400, 0x7FE0, 0x0004, 0x1234, START_TIME, DURATION, 4, false, &descs,
        );
        let list = build_event_list(&eit);
        let ev = &list[0];
        assert_eq!(ev.event_group_list.len(), 1);
        assert_eq!(ev.event_group_list[0].group_type, 1);
        assert_eq!(ev.event_group_list[0].event_list.len(), 2);
        assert_eq!(ev.event_group_list[0].event_list[0].service_id, 0x0401);
        assert_eq!(ev.event_group_list[0].event_list[0].event_id, 0x1111);
        assert_eq!(ev.event_group_list[0].event_list[1].service_id, 0x0402);
        assert_eq!(ev.event_group_list[0].event_list[1].event_id, 0x2222);
    }

    #[test]
    fn test_decode_aribstr_empty() {
        assert_eq!(decode_aribstr(&[]), "");
    }
}
