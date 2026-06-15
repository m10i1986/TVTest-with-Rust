// Rust port of LibISDB/Samples/epgdatatojson.cpp
// epgdatatojson.cpp:main
//
// EPGデータファイルを読み込んでJSONに変換する。
// 使い方: epgdatatojson <filename>

use libisdb_epg_data_file::{EpgService, parse};
use libisdb_event_info::EventInfo;
use libisdb_datetime::DateTime;
use std::{env, fs, process};

// ─── JSONエスケープ (epgdatatojson.cpp:EscapeString) ────────────────

fn escape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"'  => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\r' => out.push_str("\\r"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            _    => out.push(c),
        }
    }
    out
}

// ─── JSONフォーマッター (epgdatatojson.cpp:JSONFormatter) ───────────

struct Json {
    out: String,
    comma: bool,
    indent: usize,
}

impl Json {
    fn new() -> Self {
        Self { out: String::new(), comma: false, indent: 0 }
    }

    fn out_value_str(&mut self, key: &str, value: &str) {
        self.pre_value();
        self.out.push('"');
        self.out.push_str(key);
        self.out.push_str("\":\"");
        self.out.push_str(&escape_string(value));
        self.out.push('"');
    }

    fn out_value_datetime(&mut self, key: &str, time: &DateTime) {
        self.pre_value();
        self.out.push('"');
        self.out.push_str(key);
        self.out.push_str("\":\"");
        self.out.push_str(&format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}+09:00",
            time.year, time.month, time.day,
            time.hour, time.minute, time.second,
        ));
        self.out.push('"');
    }

    fn out_value_u8(&mut self, key: &str, value: u8) {
        self.pre_value();
        self.out.push('"');
        self.out.push_str(key);
        self.out.push_str("\":");
        self.out.push_str(&value.to_string());
    }

    fn out_value_u16(&mut self, key: &str, value: u16) {
        self.pre_value();
        self.out.push('"');
        self.out.push_str(key);
        self.out.push_str("\":");
        self.out.push_str(&value.to_string());
    }

    fn out_value_u32(&mut self, key: &str, value: u32) {
        self.pre_value();
        self.out.push('"');
        self.out.push_str(key);
        self.out.push_str("\":");
        self.out.push_str(&value.to_string());
    }

    fn out_value_bool(&mut self, key: &str, value: bool) {
        self.pre_value();
        self.out.push('"');
        self.out.push_str(key);
        self.out.push_str("\":");
        self.out.push_str(if value { "true" } else { "false" });
    }

    fn begin_object(&mut self) {
        if self.comma {
            self.out_comma();
            self.comma = false;
        }
        self.out_indent();
        self.out.push_str("{\n");
        self.indent += 1;
    }

    fn end_object(&mut self) {
        self.out.push('\n');
        self.indent -= 1;
        self.out_indent();
        self.out.push('}');
        self.comma = true;
    }

    fn begin_array(&mut self, key: &str) {
        if self.comma {
            self.out_comma();
            self.comma = false;
        }
        self.out_indent();
        self.out.push('"');
        self.out.push_str(key);
        self.out.push_str("\":[\n");
        self.indent += 1;
    }

    fn end_array(&mut self) {
        self.out.push('\n');
        self.indent -= 1;
        self.out_indent();
        self.out.push(']');
        self.comma = true;
    }

    fn out_comma(&mut self) {
        if self.comma {
            self.out.push_str(",\n");
        } else {
            self.comma = true;
        }
    }

    fn out_indent(&mut self) {
        for _ in 0..self.indent {
            self.out.push('\t');
        }
    }

    fn pre_value(&mut self) {
        self.out_comma();
        self.out_indent();
    }

    fn finish(self) -> String { self.out }
}

// ─── イベント出力 ────────────────────────────────────────────────────

fn write_event(json: &mut Json, event: &EventInfo) {
    json.begin_object();

    json.out_value_u16("eventId", event.event_id);
    json.out_value_str("eventName", &event.event_name);
    json.out_value_str("eventText", &event.event_text);

    // extendedText (epgdatatojson.cpp:249)
    json.begin_array("extendedText");
    for et in &event.extended_text {
        json.begin_object();
        json.out_value_str("description", &et.description);
        json.out_value_str("text", &et.text);
        json.end_object();
    }
    json.end_array();

    json.out_value_datetime("startTime", &event.start_time);
    json.out_value_u32("duration", event.duration);
    json.out_value_bool("freeCaMode", event.free_ca_mode);

    // videoList (epgdatatojson.cpp:259)
    if !event.video_list.is_empty() {
        json.begin_array("videoList");
        for v in &event.video_list {
            json.begin_object();
            json.out_value_u8("streamContent", v.stream_content);
            json.out_value_u8("componentType", v.component_type);
            json.out_value_u8("componentTag", v.component_tag);
            json.out_value_u32("languageCode", v.language_code);
            json.out_value_str("text", &v.text);
            json.end_object();
        }
        json.end_array();
    }

    // audioList (epgdatatojson.cpp:271)
    if !event.audio_list.is_empty() {
        json.begin_array("audioList");
        for a in &event.audio_list {
            json.begin_object();
            json.out_value_u8("streamContent", a.stream_content);
            json.out_value_u8("componentType", a.component_type);
            json.out_value_u8("componentTag", a.component_tag);
            json.out_value_bool("multiLingual", a.es_multi_lingual_flag);
            json.out_value_bool("mainComponent", a.main_component_flag);
            json.out_value_u32("languageCode", a.language_code);
            json.out_value_u32("languageCode2", a.language_code2);
            json.out_value_str("text", &a.text);
            json.end_object();
        }
        json.end_array();
    }

    // contentNibble (epgdatatojson.cpp:286)
    let nibbles = &event.content_nibble.nibble_list;
    if !nibbles.is_empty() {
        json.begin_array("contentNibble");
        for n in nibbles {
            json.begin_object();
            json.out_value_u8("level1", n.content_nibble_level1);
            json.out_value_u8("level2", n.content_nibble_level2);
            json.out_value_u8("user1", n.user_nibble1);
            json.out_value_u8("user2", n.user_nibble2);
            json.end_object();
        }
        json.end_array();
    }

    // eventGroup (epgdatatojson.cpp:299)
    if !event.event_group_list.is_empty() {
        json.begin_array("eventGroup");
        for grp in &event.event_group_list {
            json.begin_object();
            json.out_value_u8("groupType", grp.group_type);
            if !grp.event_list.is_empty() {
                json.begin_array("eventList");
                for ev in &grp.event_list {
                    json.begin_object();
                    json.out_value_u16("serviceId", ev.service_id);
                    json.out_value_u16("eventId", ev.event_id);
                    // networkId / transportStreamId は epgdatatojson.cpp:313 に含まれるが、
                    // EventGroupItem には格納されていないため省略 (epg_data_file での保存時に0)
                    json.end_object();
                }
                json.end_array();
            }
            json.end_object();
        }
        json.end_array();
    }

    // commonEvent (epgdatatojson.cpp:325)
    if event.is_common_event {
        json.out_value_u16("commonServiceId", event.common_event.service_id);
        json.out_value_u16("commonEventId", event.common_event.event_id);
    }

    json.end_object();
}

// ─── サービス出力 ─────────────────────────────────────────────────────

fn write_service(json: &mut Json, svc: &EpgService) {
    json.begin_object();

    json.out_value_u16("serviceId", svc.key.service_id);
    json.out_value_u16("networkId", svc.key.network_id);
    json.out_value_u16("transportStreamId", svc.key.transport_stream_id);

    // eventList (epgdatatojson.cpp:238)
    json.begin_array("eventList");
    for event in &svc.events {
        write_event(json, event);
    }
    json.end_array();

    json.end_object();
}

// ─── エントリーポイント (epgdatatojson.cpp:main) ─────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Need filename");
        process::exit(1);
    }

    let path = &args[1];
    let data = match fs::read(path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Failed to open file : {} ({})", path, e);
            process::exit(1);
        }
    };

    let (_, services) = match parse(&data) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Load error: {}", e);
            process::exit(1);
        }
    };

    let mut json = Json::new();

    json.begin_object();
    json.begin_array("serviceList");

    for svc in &services {
        write_service(&mut json, svc);
    }

    json.end_array();
    json.end_object();

    println!("{}", json.finish());
}

// ─── テスト ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use libisdb_epg_data_file::{EpgService, ServiceKey, serialize};
    use libisdb_event_info::{EventInfo, TypeFlag};
    use libisdb_datetime::{DateTime, get_day_of_week};

    fn make_dt(year: i32, month: i32, day: i32, h: i32, m: i32, s: i32) -> DateTime {
        let dow = get_day_of_week(year, month, day);
        DateTime { year, month, day, day_of_week: dow, hour: h, minute: m, second: s, millisecond: 0 }
    }

    fn make_event(id: u16, name: &str) -> EventInfo {
        EventInfo {
            network_id: 1,
            transport_stream_id: 2,
            service_id: 3,
            event_id: id,
            start_time: make_dt(2024, 4, 1, 20, 0, 0),
            duration: 1800,
            event_name: name.to_string(),
            type_flag: TypeFlag::Basic | TypeFlag::Database,
            ..Default::default()
        }
    }

    #[test]
    fn test_escape_string_basic() {
        assert_eq!(escape_string("hello"), "hello");
        assert_eq!(escape_string("a\"b"), "a\\\"b");
        assert_eq!(escape_string("a\\b"), "a\\\\b");
        assert_eq!(escape_string("a\nb"), "a\\nb");
        assert_eq!(escape_string("a\rb"), "a\\rb");
        assert_eq!(escape_string("a\tb"), "a\\tb");
    }

    #[test]
    fn test_escape_string_japanese() {
        let s = "テスト番組";
        assert_eq!(escape_string(s), s);
    }

    #[test]
    fn test_json_empty_service_list() {
        let mut json = Json::new();
        json.begin_object();
        json.begin_array("serviceList");
        json.end_array();
        json.end_object();
        let out = json.finish();
        assert!(out.contains("\"serviceList\""));
        assert!(out.starts_with('{'));
        assert!(out.ends_with('}'));
    }

    #[test]
    fn test_json_single_service() {
        let svc = EpgService {
            key: ServiceKey { network_id: 32736, transport_stream_id: 1032, service_id: 1024 },
            events: vec![make_event(100, "ニュース")],
        };
        let mut json = Json::new();
        json.begin_object();
        json.begin_array("serviceList");
        write_service(&mut json, &svc);
        json.end_array();
        json.end_object();
        let out = json.finish();
        assert!(out.contains("\"serviceId\":1024"));
        assert!(out.contains("\"networkId\":32736"));
        assert!(out.contains("\"eventId\":100"));
        assert!(out.contains("\"eventName\":\"ニュース\""));
    }

    #[test]
    fn test_json_event_extended_text() {
        use libisdb_event_info::ExtendedTextInfo;
        let mut evt = make_event(200, "ドラマ");
        evt.extended_text = vec![ExtendedTextInfo {
            description: "あらすじ".into(),
            text: "詳細内容".into(),
        }];
        let svc = EpgService {
            key: ServiceKey { network_id: 1, transport_stream_id: 1, service_id: 1 },
            events: vec![evt],
        };
        let mut json = Json::new();
        write_service(&mut json, &svc);
        let out = json.finish();
        assert!(out.contains("\"description\":\"あらすじ\""));
        assert!(out.contains("\"text\":\"詳細内容\""));
    }

    #[test]
    fn test_json_start_time_format() {
        let dt = make_dt(2024, 7, 15, 19, 30, 0);
        let mut json = Json::new();
        json.begin_object();
        json.out_value_datetime("startTime", &dt);
        json.end_object();
        let out = json.finish();
        assert!(out.contains("\"startTime\":\"2024-07-15T19:30:00+09:00\""));
    }

    #[test]
    fn test_json_free_ca_mode() {
        let mut evt = make_event(300, "有料放送");
        evt.free_ca_mode = true;
        let svc = EpgService {
            key: ServiceKey { network_id: 1, transport_stream_id: 1, service_id: 1 },
            events: vec![evt],
        };
        let mut json = Json::new();
        write_service(&mut json, &svc);
        let out = json.finish();
        assert!(out.contains("\"freeCaMode\":true"));
    }

    #[test]
    fn test_json_content_nibble() {
        use libisdb_event_info::ContentNibble;
        let mut evt = make_event(400, "スポーツ");
        evt.content_nibble.nibble_list = vec![ContentNibble {
            content_nibble_level1: 6,
            content_nibble_level2: 1,
            user_nibble1: 0,
            user_nibble2: 0,
        }];
        let svc = EpgService {
            key: ServiceKey { network_id: 1, transport_stream_id: 1, service_id: 1 },
            events: vec![evt],
        };
        let mut json = Json::new();
        write_service(&mut json, &svc);
        let out = json.finish();
        assert!(out.contains("\"contentNibble\""));
        assert!(out.contains("\"level1\":6"));
        assert!(out.contains("\"level2\":1"));
    }

    #[test]
    fn test_json_escape_in_title() {
        let evt = make_event(500, "タイトル\"特集\"");
        let svc = EpgService {
            key: ServiceKey { network_id: 1, transport_stream_id: 1, service_id: 1 },
            events: vec![evt],
        };
        let mut json = Json::new();
        write_service(&mut json, &svc);
        let out = json.finish();
        assert!(out.contains("\\\"特集\\\""));
    }

    #[test]
    fn test_roundtrip_serialize_then_json() {
        let svc = EpgService {
            key: ServiceKey { network_id: 32736, transport_stream_id: 1032, service_id: 1024 },
            events: vec![make_event(1, "番組A"), make_event(2, "番組B")],
        };
        let bytes = serialize(&[svc], 42);
        let (_, services) = parse(&bytes).unwrap();
        let mut json = Json::new();
        json.begin_object();
        json.begin_array("serviceList");
        for s in &services { write_service(&mut json, s); }
        json.end_array();
        json.end_object();
        let out = json.finish();
        assert!(out.contains("\"番組A\""));
        assert!(out.contains("\"番組B\""));
    }
}
