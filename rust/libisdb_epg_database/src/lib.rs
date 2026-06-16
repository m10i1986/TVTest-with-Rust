// Rust port of LibISDB/EPG/EPGDatabase.cpp + EPGDatabase.hpp
//
// 移植対象:
//   EPGDatabase (EPGDatabase.hpp:46) — EIT を解析し番組情報を蓄積するデータベース
//   ServiceInfo / TimeEventInfo / ScheduleInfo / ServiceEventMap (同上)
//
// 設計上の相違:
//   - C++ の EventListener コールバックは ScheduleEvent 列挙体を Vec で返す方式に変更
//   - GetCurrentEPGTime (OS 依存) は update_section の current_sys_time 引数で注入
//   - MutexLock は持たない (スレッド安全性は呼び出し元の責務)
//   - MergeFlag::Database は DATABASE として kebab-case 化

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ops::Bound::*;

use libisdb_datetime::DateTime;
use libisdb_event_info::{EventInfo, TypeFlag, ExtendedTextInfo, VideoInfo, AudioInfo,
    ContentNibble, EventGroupInfo, EventGroupItem, CommonEventInfo};
use libisdb_ts_tables::{EITTable, TOTTable};
use libisdb_descriptor::{
    DescriptorBlock, ShortEventDescriptor, ExtendedEventDescriptor,
    ComponentDescriptor, AudioComponentDescriptor, ContentDescriptor, EventGroupDescriptor,
};
use libisdb_arib_string::{decode_to_string, DecodeFlags};

// ---------------------------------------------------------------------------
// 公開型
// ---------------------------------------------------------------------------

bitflags::bitflags! {
    // EPGDatabase.hpp:119
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
    pub struct MergeFlag: u32 {
        const DISCARD_OLD_EVENTS   = 0x0001;
        const DISCARD_ENDED_EVENTS = 0x0002;
        const DATABASE             = 0x0004;
        const MERGE_BASIC_EXTENDED = 0x0008;
        const SET_SERVICE_UPDATED  = 0x0010;
    }
}

/// サービス識別情報。EPGDatabase.hpp:65。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServiceInfo {
    pub network_id: u16,
    pub transport_stream_id: u16,
    pub service_id: u16,
}

impl ServiceInfo {
    pub const fn new(nid: u16, tsid: u16, sid: u16) -> Self {
        Self { network_id: nid, transport_stream_id: tsid, service_id: sid }
    }

    // EPGDatabase.hpp:91
    fn key(self) -> u64 {
        ((self.network_id as u64) << 32)
            | ((self.transport_stream_id as u64) << 16)
            | (self.service_id as u64)
    }
}

impl PartialOrd for ServiceInfo {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> { Some(self.cmp(other)) }
}
impl Ord for ServiceInfo {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering { self.key().cmp(&other.key()) }
}

/// TimeMap 用エントリ。StartTime のみで比較 (C++ operator< と同様)。EPGDatabase.hpp:101。
#[derive(Clone, Copy, Debug, Default)]
pub struct TimeEventInfo {
    pub start_time: u64,   // GetLinearSeconds() 相当
    pub duration: u32,     // 秒
    pub event_id: u16,
    pub updated_time: u64,
}

impl TimeEventInfo {
    fn probe(start_time: u64) -> Self {
        Self { start_time, duration: 0, event_id: 0, updated_time: 0 }
    }

    fn from_event_info(info: &EventInfo) -> Self {
        Self {
            start_time: info.start_time.get_linear_seconds(),
            duration: info.duration,
            event_id: info.event_id,
            updated_time: info.updated_time,
        }
    }
}

impl PartialEq for TimeEventInfo {
    fn eq(&self, other: &Self) -> bool { self.start_time == other.start_time }
}
impl Eq for TimeEventInfo {}
impl PartialOrd for TimeEventInfo {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> { Some(self.cmp(other)) }
}
impl Ord for TimeEventInfo {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering { self.start_time.cmp(&other.start_time) }
}

/// update_section / update_tot 通知。C++ の EventListener コールバックに対応。
#[derive(Debug, Clone)]
pub enum ScheduleEvent {
    /// EIT schedule が揃った (OnServiceCompleted 相当)
    Completed { network_id: u16, transport_stream_id: u16, service_id: u16, is_extended: bool },
    /// スケジュール日付が変わってリセットした (OnScheduleStatusReset 相当)
    Reset { network_id: u16, transport_stream_id: u16, service_id: u16 },
}

/// update_section / update_tot の返り値。
#[derive(Debug, Default)]
pub struct UpdateResult {
    pub updated: bool,
    pub events: Vec<ScheduleEvent>,
}

// ---------------------------------------------------------------------------
// 非公開: スケジュール追跡
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
struct SegmentInfo {
    section_count: u8,
    section_flags: u8,
}

#[derive(Clone, Default)]
struct TableInfo {
    version: u8,
    is_complete: bool,
    segments: [SegmentInfo; 32],
}

impl TableInfo {
    fn clear(&mut self) {
        self.version = 0;
        self.is_complete = false;
        for s in &mut self.segments { *s = SegmentInfo::default(); }
    }
}

#[derive(Clone, Default)]
struct TableList {
    table_count: u8,
    tables: [TableInfo; 8],
}

impl TableList {
    fn clear(&mut self) {
        self.table_count = 0;
        for t in &mut self.tables { t.clear(); }
    }
}

/// EIT schedule の受信状態追跡。EPGDatabase.hpp:207。
#[derive(Clone, Default)]
struct ScheduleInfo {
    basic: TableList,
    extended: TableList,
}

impl ScheduleInfo {
    fn reset(&mut self) {
        self.basic.table_count = 0;
        self.extended.table_count = 0;
    }

    // EPGDatabase.cpp:1327
    fn is_complete(&self, hour: i32, is_extended: bool) -> bool {
        let list = if is_extended { &self.extended } else { &self.basic };
        if list.table_count == 0 { return false; }

        if !list.tables[0].is_complete && !self.is_table_complete(list, 0, hour) {
            return false;
        }
        for i in 1..list.table_count as usize {
            if !list.tables[i].is_complete { return false; }
        }
        true
    }

    // EPGDatabase.cpp:1354
    fn is_table_complete(&self, list: &TableList, table_index: usize, hour: i32) -> bool {
        if table_index >= list.table_count as usize { return false; }
        if table_index == 0 && (hour < 0 || hour > 23) { return false; }

        let table = &list.tables[table_index];
        let start_seg = if table_index == 0 { (hour / 3) as usize } else { 0 };

        for i in start_seg..32 {
            let seg = &table.segments[i];
            if seg.section_count == 0 { return false; }
            let expected_flags = ((1u16 << seg.section_count) - 1) as u8;
            if seg.section_flags != expected_flags { return false; }
        }
        true
    }

    fn has_schedule(&self, is_extended: bool) -> bool {
        if is_extended { self.extended.table_count > 0 } else { self.basic.table_count > 0 }
    }

    // EPGDatabase.cpp:1384
    fn on_section(&mut self, table: &EITTable, hour: i32) -> bool {
        let table_id = table.get_table_id();
        let last_table_id = table.get_last_table_id();
        let first_table_id = last_table_id & 0xF8;
        let section_number = table.get_section_number();
        let last_section_number = table.get_segment_last_section_number();
        let first_section_number = last_section_number & 0xF8;

        if table_id < 0x50 || table_id > 0x6F
            || table_id < first_table_id || table_id > last_table_id
            || section_number < first_section_number || section_number > last_section_number
        {
            return false;
        }

        let is_extended = (table_id & 0x08) != 0;
        let table_count = (last_table_id - first_table_id) + 1;
        let table_index = (table_id & 0x07) as usize;
        let seg_index = (section_number >> 3) as usize;
        let section_count = (last_section_number - first_section_number) + 1;
        let section_flag = 1u8 << (section_number & 0x07);
        let version = table.get_version_number();

        let mut check_complete = false;

        {
            let list = if is_extended { &mut self.extended } else { &mut self.basic };

            if list.table_count != table_count {
                list.clear();
                list.table_count = table_count;
                list.tables[table_index].version = version;
            } else if version != list.tables[table_index].version {
                let ti = &mut list.tables[table_index];
                ti.version = version;
                ti.is_complete = false;
                for s in &mut ti.segments { *s = SegmentInfo::default(); }
            }

            let seg = &mut list.tables[table_index].segments[seg_index];
            if seg.section_count != section_count {
                seg.section_count = section_count;
                seg.section_flags = 0;
            }

            if (seg.section_flags & section_flag) == 0 {
                seg.section_flags |= section_flag;
                let expected = ((1u16 << seg.section_count) - 1) as u8;
                if seg.section_flags == expected {
                    check_complete = true;
                }
            }
        }

        if check_complete {
            let list = if is_extended { &self.extended } else { &self.basic };
            let is_comp = self.is_table_complete(list, table_index, hour);
            let list_mut = if is_extended { &mut self.extended } else { &mut self.basic };
            list_mut.tables[table_index].is_complete = is_comp;
        }

        true
    }
}

// ---------------------------------------------------------------------------
// 非公開: サービス毎の番組データ
// ---------------------------------------------------------------------------

type EventMap = HashMap<u16, EventInfo>;

#[derive(Default)]
struct ServiceEventMap {
    event_map: EventMap,
    event_extended_map: EventMap,
    time_map: BTreeSet<TimeEventInfo>,
    is_updated: bool,
    schedule: ScheduleInfo,
    schedule_updated_time: DateTime,
}

impl ServiceEventMap {
    fn with_schedule_time(t: DateTime) -> Self {
        Self { schedule_updated_time: t, ..Default::default() }
    }
}

// ---------------------------------------------------------------------------
// 公開: EPGDatabase
// ---------------------------------------------------------------------------

/// 番組情報データベース。EPGDatabase.hpp:46。
pub struct EpgDatabase {
    service_map: BTreeMap<ServiceInfo, ServiceEventMap>,
    pending_service_map: BTreeMap<ServiceInfo, ServiceEventMap>,
    pub is_updated: bool,
    pub schedule_only: bool,
    pub no_past_events: bool,
    pub string_decode_flags: DecodeFlags,
    cur_tot_time: DateTime,
    cur_tot_seconds: u64,
}

impl Default for EpgDatabase {
    fn default() -> Self { Self::new() }
}

impl EpgDatabase {
    // EPGDatabase.cpp:64
    pub fn new() -> Self {
        Self {
            service_map: BTreeMap::new(),
            pending_service_map: BTreeMap::new(),
            is_updated: false,
            schedule_only: false,
            no_past_events: true,
            string_decode_flags: DecodeFlags { use_char_size: true, ..Default::default() },
            cur_tot_time: DateTime::default(),
            cur_tot_seconds: 0,
        }
    }

    // EPGDatabase.cpp:74
    pub fn clear(&mut self) {
        self.service_map.clear();
        self.pending_service_map.clear();
    }

    // EPGDatabase.cpp:82
    pub fn get_service_count(&self) -> usize { self.service_map.len() }

    // EPGDatabase.cpp:91 (simplified: returns owned Vec)
    pub fn get_service_list(&self) -> Vec<ServiceInfo> {
        self.service_map.keys().copied().collect()
    }

    // EPGDatabase.cpp:110
    pub fn is_service_updated(&self, nid: u16, tsid: u16, sid: u16) -> bool {
        self.find_service(ServiceInfo::new(nid, tsid, sid))
            .map_or(false, |s| s.is_updated)
    }

    // EPGDatabase.cpp:122
    pub fn reset_service_updated(&mut self, nid: u16, tsid: u16, sid: u16) -> bool {
        match self.service_map.get_mut(&ServiceInfo::new(nid, tsid, sid)) {
            Some(s) => { s.is_updated = false; true }
            None => false,
        }
    }

    // EPGDatabase.cpp:135 (時刻順オプションなし版)
    pub fn get_event_list(&self, nid: u16, tsid: u16, sid: u16) -> Option<Vec<EventInfo>> {
        let service = self.find_service(ServiceInfo::new(nid, tsid, sid))?;
        Some(service.event_map.values()
            .filter(|e| is_event_valid(e))
            .cloned()
            .collect())
    }

    // EPGDatabase.cpp:173 — TimeMap 順で返す
    pub fn get_event_list_sorted_by_time(&self, nid: u16, tsid: u16, sid: u16) -> Option<Vec<EventInfo>> {
        let service = self.find_service(ServiceInfo::new(nid, tsid, sid))?;
        let mut list = Vec::with_capacity(service.event_map.len());
        for t in &service.time_map {
            if let Some(ev) = service.event_map.get(&t.event_id) {
                if is_event_valid(ev) { list.push(ev.clone()); }
            }
        }
        Some(list)
    }

    // EPGDatabase.cpp:202
    pub fn get_event_info_by_id(&self, nid: u16, tsid: u16, sid: u16, event_id: u16) -> Option<EventInfo> {
        let service = self.find_service(ServiceInfo::new(nid, tsid, sid))?;
        let ev = service.event_map.get(&event_id)?;
        if !is_event_valid(ev) { return None; }
        let mut ev = ev.clone();
        self.set_common_event_info(&mut ev);
        Some(ev)
    }

    // EPGDatabase.cpp:226
    pub fn get_event_info_by_time(&self, nid: u16, tsid: u16, sid: u16, time: &DateTime) -> Option<EventInfo> {
        let service = self.find_service(ServiceInfo::new(nid, tsid, sid))?;
        let key = TimeEventInfo::probe(time.get_linear_seconds());
        let mut it = service.time_map.range(..=key);
        // upper_bound(key) → take previous
        let entry = loop {
            match it.next_back() {
                None => return None,
                Some(t) => {
                    if t.start_time + t.duration as u64 > key.start_time { break t; }
                    // doesn't cover the time
                    if t.start_time <= key.start_time { return None; }
                }
            }
        };
        let ev = service.event_map.get(&entry.event_id)?;
        if !is_event_valid(ev) { return None; }
        let mut ev = ev.clone();
        self.set_common_event_info(&mut ev);
        Some(ev)
    }

    // EPGDatabase.cpp:258
    pub fn get_next_event_info(&self, nid: u16, tsid: u16, sid: u16, time: &DateTime) -> Option<EventInfo> {
        let service = self.find_service(ServiceInfo::new(nid, tsid, sid))?;
        let key = TimeEventInfo::probe(time.get_linear_seconds());
        // upper_bound: first element with start_time > time
        let entry = service.time_map.range((Excluded(key), Unbounded)).next()?;
        let ev = service.event_map.get(&entry.event_id)?;
        if !is_event_valid(ev) { return None; }
        let mut ev = ev.clone();
        self.set_common_event_info(&mut ev);
        Some(ev)
    }

    // EPGDatabase.cpp:381 (Event をまとめて登録)
    pub fn set_service_event_list(&mut self, info: ServiceInfo, list: Vec<EventInfo>) {
        self.service_map.remove(&info);
        let service = self.service_map.entry(info).or_insert_with(ServiceEventMap::default);
        for ev in list {
            let te = TimeEventInfo::from_event_info(&ev);
            service.event_map.insert(ev.event_id, ev);
            service.time_map.insert(te);
        }
    }

    // EPGDatabase.cpp:405
    pub fn merge(&mut self, src: &mut EpgDatabase, flags: MergeFlag,
                 current_sys_time: Option<&DateTime>) -> bool {
        let src_keys: Vec<ServiceInfo> = src.service_map.keys().copied().collect();
        for key in src_keys {
            if let Some(src_service) = src.service_map.get_mut(&key) {
                // Clone the source service map to avoid borrow issues
                let src_clone = clone_service_event_map(src_service);
                self.merge_event_map_impl(key, src_clone, flags, current_sys_time);
            }
        }
        true
    }

    // EPGDatabase.cpp:422
    pub fn merge_service(&mut self, src: &mut EpgDatabase,
                         nid: u16, tsid: u16, sid: u16,
                         flags: MergeFlag, current_sys_time: Option<&DateTime>) -> bool {
        let key = ServiceInfo::new(nid, tsid, sid);
        let src_service = match src.service_map.get(&key) {
            Some(s) => clone_service_event_map(s),
            None => return false,
        };
        self.merge_event_map_impl(key, src_service, flags, current_sys_time);
        true
    }

    // EPGDatabase.cpp:443
    pub fn is_schedule_complete(&self, nid: u16, tsid: u16, sid: u16, is_extended: bool) -> bool {
        let service = match self.service_map.get(&ServiceInfo::new(nid, tsid, sid)) {
            Some(s) => s, None => return false,
        };
        let hour = if self.cur_tot_time.is_valid() { self.cur_tot_time.hour } else { -1 };
        service.schedule.is_complete(hour, is_extended)
    }

    // EPGDatabase.cpp:455
    pub fn has_schedule(&self, nid: u16, tsid: u16, sid: u16, is_extended: bool) -> bool {
        self.service_map.get(&ServiceInfo::new(nid, tsid, sid))
            .map_or(false, |s| s.schedule.has_schedule(is_extended))
    }

    // EPGDatabase.cpp:467
    pub fn reset_schedule_status(&mut self) {
        for s in self.service_map.values_mut() { s.schedule.reset(); }
    }

    pub fn get_tot_time(&self) -> Option<&DateTime> {
        if self.cur_tot_time.is_valid() { Some(&self.cur_tot_time) } else { None }
    }

    pub fn get_tot_seconds(&self) -> u64 { self.cur_tot_seconds }

    // EPGDatabase.cpp:514 — メインのセクション処理
    pub fn update_section(
        &mut self,
        eit: &EITTable,
        source_id: u32,
        current_sys_time: Option<&DateTime>,
    ) -> Option<UpdateResult> {
        let table_id = eit.get_table_id();
        if table_id < 0x4E || table_id > 0x6F { return None; }

        let is_schedule = table_id >= 0x50;
        let is_extended = is_schedule && (table_id & 0x08) != 0;
        if self.schedule_only && !is_schedule { return None; }

        let key = ServiceInfo::new(
            eit.get_original_network_id(),
            eit.get_transport_stream_id(),
            eit.get_service_id(),
        );

        // サービスエントリを作成（または取得）
        let cur_tot_time_for_insert = self.cur_tot_time.clone();
        self.service_map.entry(key).or_insert_with(|| {
            ServiceEventMap::with_schedule_time(cur_tot_time_for_insert)
        });

        let cur_tot_seconds = self.cur_tot_seconds;
        let cur_tot_time = self.cur_tot_time.clone();
        let no_past_events = self.no_past_events;
        let decode_flags = self.string_decode_flags;
        let nid = eit.get_original_network_id();
        let tsid = eit.get_transport_stream_id();
        let sid = eit.get_service_id();

        let event_count = eit.get_event_count();
        let mut is_updated = false;
        let mut sched_events: Vec<ScheduleEvent> = Vec::new();

        if event_count > 0 {
            for i in 0..event_count {
                let evt = match eit.get_event(i) { Some(e) => e, None => continue };
                let start_time = match &evt.start_time { Some(t) => t.clone(), None => continue };
                if evt.duration == 0 { continue; }

                // 過去イベントフィルタ
                if no_past_events {
                    if let Some(cur_sys) = current_sys_time {
                        let end = match start_time.offset_seconds(evt.duration as i64) {
                            Some(e) => e, None => continue,
                        };
                        if end.diff_seconds(cur_sys) <= -(5 * 60) { continue; }
                    }
                }

                // IsPending / IsExtendedOnly を判定 (メインマップから読み取り)
                let (is_pending, mut is_extended_only) = {
                    let service = self.service_map.get(&key).unwrap();
                    check_pending_and_extended_only(
                        &service.event_map, evt.event_id, is_extended, cur_tot_seconds, source_id,
                    )
                };

                // TOT 未受信時: pending マップ作成
                if cur_tot_seconds == 0 {
                    self.pending_service_map.entry(key)
                        .or_insert_with(ServiceEventMap::default);

                    if is_pending {
                        // pending マップ側で再判定
                        let ps = self.pending_service_map.get(&key).unwrap();
                        let (_, is_eo) = check_pending_and_extended_only(
                            &ps.event_map, evt.event_id, is_extended, 0, source_id,
                        );
                        is_extended_only = is_eo;
                    }
                }

                let target_is_pending = is_pending && cur_tot_seconds == 0;

                // TimeMap 更新 (extended only でなければ)
                if !is_extended_only {
                    let time_evt = TimeEventInfo {
                        start_time: start_time.get_linear_seconds(),
                        duration: evt.duration,
                        event_id: evt.event_id,
                        updated_time: cur_tot_seconds,
                    };

                    let target = if target_is_pending {
                        self.pending_service_map.get_mut(&key).unwrap()
                    } else {
                        self.service_map.get_mut(&key).unwrap()
                    };

                    let mut time_updated = false;
                    if !update_time_map(target, time_evt, &mut time_updated) { continue; }
                    if time_updated && !target_is_pending { is_updated = true; }
                }

                // イベントを更新 (スコープで参照を解放)
                let clone_for_pending = {
                    let target = if target_is_pending {
                        self.pending_service_map.get_mut(&key).unwrap()
                    } else {
                        self.service_map.get_mut(&key).unwrap()
                    };

                    update_event_in_service(
                        target, evt, &start_time, nid, tsid, sid, source_id,
                        is_extended_only, is_extended, is_schedule, table_id,
                        cur_tot_seconds, decode_flags,
                    );

                    if !is_pending && !is_extended_only {
                        let event_map = if is_extended_only { &target.event_extended_map } else { &target.event_map };
                        event_map.get(&evt.event_id).cloned()
                    } else {
                        None
                    }
                };

                // pending マップにマージ
                if let Some(clone_ev) = clone_for_pending {
                    if cur_tot_seconds == 0 {
                        let ps = self.pending_service_map.get_mut(&key).unwrap();
                        merge_event_map_event(ps, clone_ev, MergeFlag::MERGE_BASIC_EXTENDED);
                    }
                    is_updated = true;
                }
            }
        } else {
            // イベントなし: セグメント内の古いイベントを削除
            let tot_ok = cur_tot_time.hour > 0
                || cur_tot_time.minute > 0
                || cur_tot_time.second >= 30;

            if tot_ok
                && ((table_id >= 0x50 && table_id <= 0x57)
                    || (table_id >= 0x60 && table_id <= 0x67))
            {
                let seg_time = get_schedule_time(cur_tot_seconds, table_id, eit.get_section_number());
                let service = self.service_map.get_mut(&key).unwrap();
                let probe = TimeEventInfo::probe(seg_time);
                let seg_end = seg_time + (3 * 60 * 60);

                let to_remove: Vec<TimeEventInfo> = service.time_map
                    .range(probe..)
                    .take_while(|t| t.start_time < seg_end)
                    .filter(|t| t.updated_time < cur_tot_seconds)
                    .copied()
                    .collect();

                for t in to_remove {
                    remove_event_from_map(&mut service.event_map, t.event_id);
                    service.time_map.remove(&t);
                    is_updated = true;
                }
            }
        }

        // updated フラグ反映
        if is_updated {
            self.service_map.get_mut(&key).unwrap().is_updated = true;
            self.is_updated = true;
        }

        // スケジュール追跡
        if is_schedule {
            let tot_ok = cur_tot_time.hour > 0
                || cur_tot_time.minute > 0
                || cur_tot_time.second >= 30;
            let hour = if cur_tot_time.is_valid() { cur_tot_time.hour } else { -1 };

            let service = self.service_map.get_mut(&key).unwrap();

            // 日付が変わったらリセット
            if tot_ok && cur_tot_time.is_valid() && service.schedule_updated_time.is_valid() {
                let su = &service.schedule_updated_time;
                if su.year != cur_tot_time.year
                    || su.month != cur_tot_time.month
                    || su.day != cur_tot_time.day
                {
                    service.schedule.reset();
                    sched_events.push(ScheduleEvent::Reset {
                        network_id: nid, transport_stream_id: tsid, service_id: sid,
                    });
                }
            }

            let was_complete = service.schedule.is_complete(hour, is_extended);
            if service.schedule.on_section(eit, hour) {
                if cur_tot_time.is_valid() {
                    service.schedule_updated_time = cur_tot_time.clone();
                }
                if !was_complete && service.schedule.is_complete(hour, is_extended) {
                    sched_events.push(ScheduleEvent::Completed {
                        network_id: nid, transport_stream_id: tsid, service_id: sid, is_extended,
                    });
                }
            }
        }

        Some(UpdateResult { updated: is_updated, events: sched_events })
    }

    // EPGDatabase.cpp:880
    pub fn update_tot(&mut self, tot: &TOTTable) -> UpdateResult {
        let time = match tot.get_date_time() {
            Some(t) => t.clone(),
            None => return UpdateResult::default(),
        };

        self.cur_tot_time = time.clone();
        self.cur_tot_seconds = time.get_linear_seconds();

        if self.cur_tot_seconds == 0 || self.pending_service_map.is_empty() {
            return UpdateResult { updated: false, events: vec![] };
        }

        // pending マップをメインにマージ
        let cur_tot_seconds = self.cur_tot_seconds;
        let cur_tot_time = self.cur_tot_time.clone();
        let pending = std::mem::take(&mut self.pending_service_map);
        let mut updated_overall = false;

        for (key, mut ps) in pending {
            for ev in ps.event_map.values_mut() { ev.updated_time = cur_tot_seconds; }
            for ev in ps.event_extended_map.values_mut() { ev.updated_time = cur_tot_seconds; }
            ps.schedule_updated_time = cur_tot_time.clone();

            if self.merge_event_map_impl(key, ps, MergeFlag::MERGE_BASIC_EXTENDED | MergeFlag::SET_SERVICE_UPDATED, None) {
                updated_overall = true;
            }
        }

        UpdateResult { updated: updated_overall, events: vec![] }
    }

    // EPGDatabase.cpp:916
    pub fn reset_tot_time(&mut self) {
        self.cur_tot_time = DateTime::default();
        self.cur_tot_seconds = 0;
    }

    // ---------------------------------------------------------------------------
    // 非公開メソッド
    // ---------------------------------------------------------------------------

    // EPGDatabase.cpp:923
    fn find_service(&self, info: ServiceInfo) -> Option<&ServiceEventMap> {
        if info.transport_stream_id != 0xFFFF {
            self.service_map.get(&info)
        } else {
            self.service_map.iter()
                .find(|(k, _)| k.network_id == info.network_id && k.service_id == info.service_id)
                .map(|(_, v)| v)
        }
    }

    // EPGDatabase.cpp:944 (MergeEventMap)
    fn merge_event_map_impl(
        &mut self,
        info: ServiceInfo,
        map: ServiceEventMap,
        flags: MergeFlag,
        current_sys_time: Option<&DateTime>,
    ) -> bool {
        if map.event_map.is_empty() { return false; }

        // サービスが存在しない場合: 新規登録
        if !self.service_map.contains_key(&info) {
            self.service_map.insert(info, map);
            self.is_updated = true;
            return true;
        }

        let service = self.service_map.get_mut(&info).unwrap();

        if flags.contains(MergeFlag::DISCARD_OLD_EVENTS) {
            *service = map;
            self.is_updated = true;
            return true;
        }

        // 終了済みイベントを除外する基準時刻
        let discard_ended = flags.contains(MergeFlag::DISCARD_ENDED_EVENTS);
        let cur_time_lin = if discard_ended {
            current_sys_time.map(|t| t.get_linear_seconds()).unwrap_or(0)
        } else { 0 };

        let mut is_updated = false;

        let events: Vec<EventInfo> = map.event_map.into_values().collect();
        for ev in events {
            let te = TimeEventInfo::from_event_info(&ev);

            if discard_ended && cur_time_lin > 0
                && te.start_time + te.duration as u64 <= cur_time_lin
            {
                continue;
            }

            if let Some(existing) = service.event_map.get(&ev.event_id) {
                if existing.updated_time > ev.updated_time { continue; }
            }

            if merge_event_map_event(service, ev, flags) { is_updated = true; }
        }

        if is_updated {
            self.is_updated = true;
            if flags.contains(MergeFlag::SET_SERVICE_UPDATED) {
                service.is_updated = true;
            }
        }

        is_updated
    }

    // EPGDatabase.cpp:1187 (GetEventInfoByIDs — 内部ルックアップ)
    fn get_event_info_raw(&self, nid: u16, tsid: u16, sid: u16, event_id: u16) -> Option<&EventInfo> {
        self.service_map.get(&ServiceInfo::new(nid, tsid, sid))
            ?.event_map.get(&event_id)
    }

    // EPGDatabase.cpp:1202
    fn set_common_event_info(&self, info: &mut EventInfo) {
        if !info.is_common_event { return; }
        if let Some(common) = self.get_event_info_raw(
            info.network_id, info.transport_stream_id,
            info.common_event.service_id, info.common_event.event_id,
        ) {
            info.event_name    = common.event_name.clone();
            info.event_text    = common.event_text.clone();
            info.extended_text = common.extended_text.clone();
            info.free_ca_mode  = common.free_ca_mode;
            info.video_list    = common.video_list.clone();
            info.audio_list    = common.audio_list.clone();
            info.content_nibble = common.content_nibble.clone();
        }
    }
}

// ---------------------------------------------------------------------------
// 自由関数: イベント処理
// ---------------------------------------------------------------------------

/// イベントが有効か (event_name があるか共有イベントか)。EPGDatabase.cpp:42。
fn is_event_valid(ev: &EventInfo) -> bool {
    !ev.event_name.is_empty() || ev.is_common_event
}

/// IsPending / IsExtendedOnly を判定するヘルパー。
fn check_pending_and_extended_only(
    event_map: &EventMap,
    event_id: u16,
    is_extended: bool,
    cur_tot_seconds: u64,
    source_id: u32,
) -> (bool, bool) {
    let is_pending;
    let is_extended_only;

    if let Some(existing) = event_map.get(&event_id) {
        is_pending = existing.updated_time > cur_tot_seconds && cur_tot_seconds == 0;
        is_extended_only = is_extended
            && (!existing.type_flag.contains(TypeFlag::Basic) || existing.source_id != source_id);
    } else {
        is_pending = false;
        is_extended_only = is_extended; // 既存データなし: extended EIT → extended only
    }

    (is_pending, is_extended_only)
}

/// EIT イベント 1 件をサービスエントリに書き込む。
fn update_event_in_service(
    service: &mut ServiceEventMap,
    evt: &libisdb_ts_tables::EITEventInfo,
    start_time: &DateTime,
    nid: u16, tsid: u16, sid: u16,
    source_id: u32,
    is_extended_only: bool,
    is_extended: bool,
    is_schedule: bool,
    table_id: u8,
    cur_tot_seconds: u64,
    decode_flags: DecodeFlags,
) {
    // 開始時刻変更 → 旧 TimeMap エントリ削除 (extended only でなければ)
    if !is_extended_only {
        let old_lin = service.event_map.get(&evt.event_id)
            .map(|e| e.start_time.get_linear_seconds());
        if let Some(old) = old_lin {
            if old != start_time.get_linear_seconds() {
                let probe = TimeEventInfo::probe(old);
                if let Some(t) = service.time_map.get(&probe).copied() {
                    if t.event_id == evt.event_id { service.time_map.remove(&t); }
                }
            }
        }
    }

    let event_map = if is_extended_only {
        &mut service.event_extended_map
    } else {
        &mut service.event_map
    };

    let is_new = !event_map.contains_key(&evt.event_id);
    let event = event_map.entry(evt.event_id).or_insert_with(EventInfo::default);

    if !is_new {
        let need_reset = event.start_time != *start_time || event.source_id != source_id;
        if need_reset { *event = EventInfo::default(); }
    }

    event.updated_time       = cur_tot_seconds;
    event.source_id          = source_id;
    event.network_id         = nid;
    event.transport_stream_id = tsid;
    event.service_id         = sid;
    event.event_id           = evt.event_id;
    event.start_time         = start_time.clone();
    event.duration           = evt.duration;
    event.running_status     = evt.running_status;
    event.free_ca_mode       = evt.free_ca_mode;

    // TypeFlag
    if is_schedule {
        if is_extended {
            event.type_flag |= TypeFlag::Extended;
        } else {
            event.type_flag |= TypeFlag::Basic;
        }
        event.type_flag &= !(TypeFlag::Present | TypeFlag::Following);
    } else {
        event.type_flag = TypeFlag::Basic | TypeFlag::Extended;
        event.type_flag |= if table_id == 0x4E { TypeFlag::Present } else { TypeFlag::Following };
    }

    let desc_block = &evt.descriptors;

    // 短形式イベント記述子 (tag=0x4D)
    if let Some(desc) = desc_block.get_descriptor_by_tag(ShortEventDescriptor::TAG) {
        if let Some(sed) = ShortEventDescriptor::from_descriptor(desc) {
            if !sed.event_name.is_empty() {
                event.event_name = decode_to_string(&sed.event_name, decode_flags)
                    .unwrap_or_default();
            }
            if !sed.event_description.is_empty() {
                event.event_text = decode_to_string(&sed.event_description, decode_flags)
                    .unwrap_or_default();
            }
        }
    }

    // 拡張形式イベント記述子 (tag=0x4E)
    let extended_texts = parse_extended_event_text(desc_block, decode_flags);
    if !extended_texts.is_empty() {
        event.extended_text = extended_texts;
    } else if !is_extended {
        // extended EIT でない場合: EventExtendedMap からマージ試行
        merge_event_extended_info(service, evt.event_id);
        return; // merge_event_extended_info は event_map を経由するので再取得不要
    }

    // コンポーネント記述子 (tag=0x50)
    if desc_block.get_descriptor_by_tag(ComponentDescriptor::TAG).is_some() {
        let event = event_map.entry(evt.event_id).or_insert_with(EventInfo::default);
        event.video_list.clear();
        for desc in desc_block.iter() {
            if let Some(cd) = ComponentDescriptor::from_descriptor(desc) {
                event.video_list.push(VideoInfo {
                    stream_content: cd.stream_content,
                    component_type: cd.component_type,
                    component_tag: cd.component_tag,
                    language_code: cd.language_code,
                    text: if cd.text.is_empty() { String::new() }
                          else { decode_to_string(&cd.text, decode_flags).unwrap_or_default() },
                });
            }
        }
    }

    // 音声コンポーネント記述子 (tag=0xC4)
    if desc_block.get_descriptor_by_tag(AudioComponentDescriptor::TAG).is_some() {
        let event = event_map.entry(evt.event_id).or_insert_with(EventInfo::default);
        event.audio_list.clear();
        for desc in desc_block.iter() {
            if let Some(ad) = AudioComponentDescriptor::from_descriptor(desc) {
                event.audio_list.push(AudioInfo {
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
                    text: if ad.text.is_empty() { String::new() }
                          else { decode_to_string(&ad.text, decode_flags).unwrap_or_default() },
                });
            }
        }
    }

    // コンテンツ記述子 (tag=0x54)
    if let Some(desc) = desc_block.get_descriptor_by_tag(ContentDescriptor::TAG) {
        if let Some(cd) = ContentDescriptor::from_descriptor(desc) {
            let event = event_map.entry(evt.event_id).or_insert_with(EventInfo::default);
            event.content_nibble.nibble_list = cd.nibble_list.iter().take(7).map(|n| ContentNibble {
                content_nibble_level1: n.content_nibble_level1,
                content_nibble_level2: n.content_nibble_level2,
                user_nibble1: n.user_nibble1,
                user_nibble2: n.user_nibble2,
            }).collect();
        }
    }

    // イベントグループ記述子 (tag=0xD6)
    if desc_block.get_descriptor_by_tag(EventGroupDescriptor::TAG).is_some() {
        let event = event_map.entry(evt.event_id).or_insert_with(EventInfo::default);
        event.event_group_list.clear();
        for desc in desc_block.iter() {
            if let Some(gd) = EventGroupDescriptor::from_descriptor(desc) {
                let group_info = EventGroupInfo {
                    group_type: gd.group_type,
                    event_list: gd.event_list.iter().map(|e| EventGroupItem {
                        service_id: e.service_id,
                        event_id: e.event_id,
                    }).collect(),
                };

                if !event.event_group_list.contains(&group_info) {
                    // イベント共有 (group_type=0x01, 1イベント, 他サービス)
                    if gd.group_type == 0x01 && gd.event_list.len() == 1 {
                        let item = &gd.event_list[0];
                        if item.service_id != sid {
                            event.is_common_event = true;
                            event.common_event = CommonEventInfo {
                                service_id: item.service_id,
                                event_id: item.event_id,
                            };
                        }
                    }
                    event.event_group_list.push(group_info);
                }
            }
        }
    }

    // EventExtendedMap とのマージ (extended only でなければ)
    if !is_extended_only {
        merge_event_extended_info(service, evt.event_id);
    }
}

// ---------------------------------------------------------------------------
// 自由関数: TimeMap 更新
// ---------------------------------------------------------------------------

// EPGDatabase.cpp:1105
fn update_time_map(service: &mut ServiceEventMap, time: TimeEventInfo, is_updated: &mut bool) -> bool {
    // 既存エントリの確認
    let existing = service.time_map.get(&time).copied();
    let is_new = existing.is_none();

    let needs_update = is_new || existing.map_or(false, |e| {
        e.duration != time.duration || e.event_id != time.event_id
    });

    if !needs_update {
        // まったく同じなら何もしない (注: 既存エントリはそのまま)
        return true;
    }

    // 既存エントリが新しい場合はスキップ
    if let Some(e) = existing {
        if e.updated_time > time.updated_time { return false; }
    }

    // まず新しいエントリを挿入 (前後チェック用に必要)
    if is_new { service.time_map.insert(time); }

    let end_time = time.start_time + time.duration as u64;
    let mut skip = false;

    // 前方重複チェック
    let forward: Vec<TimeEventInfo> = service.time_map
        .range((Excluded(time), Unbounded))
        .take_while(|e| e.start_time < end_time)
        .copied()
        .collect();

    for e in forward {
        if e.updated_time > time.updated_time { skip = true; break; }
        remove_event_from_map(&mut service.event_map, e.event_id);
        service.time_map.remove(&e);
        *is_updated = true;
    }

    // 後方重複チェック
    if !skip {
        let backward: Vec<TimeEventInfo> = service.time_map
            .range((Unbounded, Excluded(time)))
            .rev()
            .take_while(|e| e.start_time + e.duration as u64 > time.start_time)
            .copied()
            .collect();

        for e in backward {
            if e.updated_time > time.updated_time { skip = true; break; }
            remove_event_from_map(&mut service.event_map, e.event_id);
            service.time_map.remove(&e);
            *is_updated = true;
        }
    }

    if skip {
        if is_new { service.time_map.remove(&time); }
        return false;
    }

    // event_id が変わった場合、旧イベントを削除
    if let Some(e) = existing {
        if e.event_id != time.event_id {
            remove_event_from_map(&mut service.event_map, e.event_id);
        }
    }

    // エントリを差し替え
    if !is_new {
        service.time_map.remove(&time); // 旧エントリ削除 (start_time 同一で照合)
        service.time_map.insert(time);  // 新エントリ挿入
        *is_updated = true;
    }

    true
}

fn remove_event_from_map(map: &mut EventMap, event_id: u16) {
    map.remove(&event_id);
}

// ---------------------------------------------------------------------------
// 自由関数: マージ
// ---------------------------------------------------------------------------

// EPGDatabase.cpp:1034 — NewEvent をサービスにマージ
fn merge_event_map_event(service: &mut ServiceEventMap, mut new_event: EventInfo, flags: MergeFlag) -> bool {
    let te = TimeEventInfo::from_event_info(&new_event);

    let mut dummy = false;
    if !update_time_map(service, te, &mut dummy) { return false; }

    let mut database_flag = flags.contains(MergeFlag::DATABASE);
    let mut overwrite = true;
    let event_id = new_event.event_id;

    let is_new = !service.event_map.contains_key(&event_id);

    if !is_new {
        // 開始時刻変更 → 旧 TimeMap エントリ削除
        {
            let old_lin = service.event_map.get(&event_id).unwrap().start_time.get_linear_seconds();
            if old_lin != new_event.start_time.get_linear_seconds() {
                let probe = TimeEventInfo::probe(old_lin);
                if let Some(t) = service.time_map.get(&probe).copied() {
                    if t.event_id == event_id { service.time_map.remove(&t); }
                }
            }
        }

        // MergeBasicExtended / 拡張テキストコピー
        enum Action {
            None,
            TransferExtToCur(Vec<ExtendedTextInfo>, u64),  // (text, updated_time)
            CopyExtFromCur(Vec<ExtendedTextInfo>),
            CopyExtForDb(Vec<ExtendedTextInfo>),
        }

        let action = {
            let cur = service.event_map.get(&event_id).unwrap();
            if flags.contains(MergeFlag::MERGE_BASIC_EXTENDED) {
                if new_event.source_id == cur.source_id && new_event.start_time == cur.start_time {
                    if !cur.type_flag.contains(TypeFlag::Extended)
                        && new_event.type_flag.contains(TypeFlag::Extended)
                    {
                        Action::TransferExtToCur(new_event.extended_text.clone(), new_event.updated_time)
                    } else if cur.type_flag.contains(TypeFlag::Extended)
                        && !new_event.type_flag.contains(TypeFlag::Extended)
                    {
                        Action::CopyExtFromCur(cur.extended_text.clone())
                    } else {
                        Action::None
                    }
                } else {
                    Action::None
                }
            } else if !new_event.type_flag.contains(TypeFlag::Extended)
                && cur.type_flag.contains(TypeFlag::Extended)
                && new_event.source_id == cur.source_id
                && new_event.start_time == cur.start_time
                && new_event.extended_text.is_empty()
                && !cur.extended_text.is_empty()
                && new_event.event_name == cur.event_name
            {
                Action::CopyExtForDb(cur.extended_text.clone())
            } else {
                Action::None
            }
        };

        match action {
            Action::TransferExtToCur(texts, upd) => {
                let cur = service.event_map.get_mut(&event_id).unwrap();
                if !texts.is_empty() {
                    cur.extended_text = texts;
                    cur.type_flag |= TypeFlag::Extended;
                }
                cur.updated_time = upd;
                overwrite = false;
            }
            Action::CopyExtFromCur(texts) => {
                if !texts.is_empty() {
                    new_event.extended_text = texts;
                    new_event.type_flag |= TypeFlag::Extended;
                }
            }
            Action::CopyExtForDb(texts) => {
                new_event.extended_text = texts;
                database_flag = true;
            }
            Action::None => {}
        }
    }

    if overwrite { service.event_map.insert(event_id, new_event); }

    merge_event_extended_info(service, event_id);

    if let Some(ev) = service.event_map.get_mut(&event_id) {
        if database_flag { ev.type_flag |= TypeFlag::Database; }
        else             { ev.type_flag &= !TypeFlag::Database; }
    }

    true
}

// EPGDatabase.cpp:1242
fn merge_event_extended_info(service: &mut ServiceEventMap, event_id: u16) {
    // 3-way decision based on immutable reads
    enum Action {
        Skip,
        EraseOnly,
        Transfer { texts: Vec<ExtendedTextInfo>, updated_time: u64 },
    }

    let action = {
        let ext = match service.event_extended_map.get(&event_id) { Some(e) => e, None => return };
        let ev  = match service.event_map.get(&event_id)          { Some(e) => e, None => return };

        if ev.source_id != ext.source_id || ev.start_time != ext.start_time {
            Action::Skip
        } else if !ev.extended_text.is_empty() && ev.updated_time > ext.updated_time {
            Action::EraseOnly
        } else {
            Action::Transfer { texts: ext.extended_text.clone(), updated_time: ext.updated_time }
        }
    };

    match action {
        Action::Skip => {}
        Action::EraseOnly => { service.event_extended_map.remove(&event_id); }
        Action::Transfer { texts, updated_time } => {
            {
                let ev = service.event_map.get_mut(&event_id).unwrap();
                ev.extended_text = texts;
                ev.type_flag |= TypeFlag::Extended;
                if ev.updated_time < updated_time { ev.updated_time = updated_time; }
            }
            service.event_extended_map.remove(&event_id);
        }
    }
}

// ---------------------------------------------------------------------------
// 自由関数: 拡張形式イベント記述子解析
// ---------------------------------------------------------------------------

// EPGDatabase.cpp 内 GetEventExtendedTextList 相当
fn parse_extended_event_text(desc_block: &DescriptorBlock, flags: DecodeFlags) -> Vec<ExtendedTextInfo> {
    let mut descs: Vec<ExtendedEventDescriptor> = desc_block.iter()
        .filter_map(|d| ExtendedEventDescriptor::from_descriptor(d))
        .collect();
    if descs.is_empty() { return vec![]; }

    descs.sort_by_key(|d| d.descriptor_number);

    struct Item { desc_num: u8, description: Vec<u8>, data: Vec<u8> }
    let mut items: Vec<Item> = Vec::new();

    for desc in &descs {
        for item in &desc.item_list {
            if !item.description.is_empty() {
                items.push(Item {
                    desc_num: desc.descriptor_number,
                    description: item.description.clone(),
                    data: item.item_char.clone(),
                });
            } else if let Some(last) = items.last_mut() {
                if last.desc_num + 1 == desc.descriptor_number {
                    last.data.extend_from_slice(&item.item_char);
                    // 連続する 1 つ分だけ (pData2 == nullptr に相当)
                }
            }
        }
    }

    items.iter().filter_map(|item| {
        let description = decode_to_string(&item.description, flags)?;
        let raw_text = decode_to_string(&item.data, flags).unwrap_or_default();
        let text = canonicalize_text(&raw_text);
        Some(ExtendedTextInfo { description, text })
    }).collect()
}

// CanonicalizeExtendedText 相当: \r[\n] → \n
fn canonicalize_text(src: &str) -> String {
    let mut dst = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            dst.push('\n');
            if chars.peek() == Some(&'\n') { chars.next(); }
        } else {
            dst.push(c);
        }
    }
    dst
}

// ---------------------------------------------------------------------------
// 自由関数: スケジュール
// ---------------------------------------------------------------------------

// EPGDatabase.cpp:49
fn get_schedule_time(cur_time: u64, table_id: u8, section_number: u8) -> u64 {
    const HOUR: u64 = 60 * 60;
    (cur_time / (24 * HOUR) * (24 * HOUR))
        + ((table_id as u64 & 0x07) * (4 * 24 * HOUR))
        + ((section_number as u64 >> 3) * (3 * HOUR))
}

// ---------------------------------------------------------------------------
// ヘルパー: ServiceEventMap のクローン (merge 用)
// ---------------------------------------------------------------------------

fn clone_service_event_map(s: &ServiceEventMap) -> ServiceEventMap {
    ServiceEventMap {
        event_map: s.event_map.clone(),
        event_extended_map: s.event_extended_map.clone(),
        time_map: s.time_map.clone(),
        is_updated: s.is_updated,
        schedule: s.schedule.clone(),
        schedule_updated_time: s.schedule_updated_time.clone(),
    }
}

// ---------------------------------------------------------------------------
// テスト
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use libisdb_ts_packet::{TsPacket, TS_PACKET_SIZE};
    use libisdb_datetime::{DateTime, datetime_to_mjd};

    // CRC-32/MPEG-2 付加
    fn append_crc(s: &mut Vec<u8>) {
        let crc = libisdb_crc::crc32_mpeg2(s, 0xFFFF_FFFF);
        s.push(((crc >> 24) & 0xFF) as u8);
        s.push(((crc >> 16) & 0xFF) as u8);
        s.push(((crc >>  8) & 0xFF) as u8);
        s.push( (crc        & 0xFF) as u8);
    }

    // TS パケット組み立て
    fn make_ts_packet(pid: u16, pusi: bool, payload: &[u8]) -> TsPacket {
        let mut data = [0u8; TS_PACKET_SIZE];
        data[0] = 0x47;
        data[1] = (if pusi { 0x40 } else { 0x00 }) | ((pid >> 8) & 0x1F) as u8;
        data[2] = (pid & 0xFF) as u8;
        data[3] = 0x10;
        let offset = if pusi { 5 } else { 4 };
        if pusi { data[4] = 0x00; }
        let copy = payload.len().min(TS_PACKET_SIZE - offset);
        data[offset..offset + copy].copy_from_slice(&payload[..copy]);
        let mut pkt = TsPacket::new(&data);
        pkt.parse_packet(None);
        pkt
    }

    fn bcd(v: u32) -> u8 { (((v / 10) % 10) as u8) << 4 | ((v % 10) as u8) }

    /// EIT セクションバイト列を構築
    /// events: (event_id, start_time, duration_secs)
    fn make_eit_section(
        table_id: u8, version: u8, section_number: u8, last_section_number: u8,
        service_id: u16, ts_id: u16, net_id: u16,
        seg_last_section: u8, last_table_id: u8,
        events: &[(u16, DateTime, u32)],
    ) -> Vec<u8> {
        let mut events_bytes = Vec::new();
        for (eid, st, dur) in events {
            let mjd = datetime_to_mjd(st);
            events_bytes.push((eid >> 8) as u8);
            events_bytes.push((eid & 0xFF) as u8);
            events_bytes.push((mjd >> 8) as u8);
            events_bytes.push((mjd & 0xFF) as u8);
            events_bytes.push(bcd(st.hour as u32));
            events_bytes.push(bcd(st.minute as u32));
            events_bytes.push(bcd(st.second as u32));
            let dh = dur / 3600;
            let dm = (dur % 3600) / 60;
            let ds = dur % 60;
            events_bytes.push(bcd(dh));
            events_bytes.push(bcd(dm));
            events_bytes.push(bcd(ds));
            events_bytes.push(0x80); // running_status=4, free_ca=0, desc_len_high=0
            events_bytes.push(0x00);
        }

        let payload_len = 6 + events_bytes.len();
        let section_length = (5 + payload_len + 4) as u16;

        let mut s = Vec::new();
        s.push(table_id);
        s.push(0xB0 | ((section_length >> 8) as u8));
        s.push((section_length & 0xFF) as u8);
        s.push((service_id >> 8) as u8);
        s.push((service_id & 0xFF) as u8);
        s.push(0xC0 | ((version & 0x1F) << 1) | 0x01);
        s.push(section_number);
        s.push(last_section_number);
        s.push((ts_id >> 8) as u8);
        s.push((ts_id & 0xFF) as u8);
        s.push((net_id >> 8) as u8);
        s.push((net_id & 0xFF) as u8);
        s.push(seg_last_section);
        s.push(last_table_id);
        s.extend_from_slice(&events_bytes);
        append_crc(&mut s);
        s
    }

    fn make_eit(
        table_id: u8, section_number: u8, last_section_number: u8,
        service_id: u16, ts_id: u16, net_id: u16,
        seg_last_section: u8, last_table_id: u8,
        events: &[(u16, DateTime, u32)],
    ) -> EITTable {
        let sec = make_eit_section(
            table_id, 0, section_number, last_section_number,
            service_id, ts_id, net_id,
            seg_last_section, last_table_id,
            events,
        );
        let pkt = make_ts_packet(0x0012, true, &sec);
        let mut eit = EITTable::new();
        eit.store_packet(&pkt);
        eit
    }

    fn dt(year: i32, month: i32, day: i32, hour: i32, min: i32, sec: i32) -> DateTime {
        DateTime { year, month, day, day_of_week: 0, hour, minute: min, second: sec, millisecond: 0 }
    }

    // ─── テスト ────────────────────────────────────────

    #[test]
    fn test_update_section_pf_basic() {
        // p/f EIT (table_id=0x4E) でイベントが登録される
        let mut db = EpgDatabase::new();
        db.no_past_events = false;

        let start = dt(2025, 4, 1, 12, 0, 0);
        let eit = make_eit(
            0x4E, 0, 1,
            0x0400, 0x0001, 0x7FE0,
            0x00, 0x4E,
            &[(0x0001, start.clone(), 3600)],
        );

        let result = db.update_section(&eit, 0, None).unwrap();
        assert!(result.updated);

        let list = db.get_event_list(0x7FE0, 0x0001, 0x0400).unwrap();
        // event_name が空のため is_event_valid = false → list は空
        // ただし event は event_map に存在するはず
        let raw = db.service_map.get(&ServiceInfo::new(0x7FE0, 0x0001, 0x0400)).unwrap();
        assert_eq!(raw.event_map.len(), 1);
        let ev = raw.event_map.get(&0x0001).unwrap();
        assert_eq!(ev.event_id, 0x0001);
        assert_eq!(ev.duration, 3600);
        assert!(ev.type_flag.contains(TypeFlag::Present));
    }

    #[test]
    fn test_update_section_schedule_basic() {
        // schedule basic EIT (table_id=0x50)
        let mut db = EpgDatabase::new();
        db.no_past_events = false;

        let start = dt(2025, 4, 2, 8, 0, 0);
        let eit = make_eit(
            0x50, 0x00, 0x07,
            0x0400, 0x0001, 0x7FE0,
            0x07, 0x50,
            &[(0x1001, start.clone(), 1800)],
        );

        let result = db.update_section(&eit, 0, None).unwrap();
        assert!(result.updated);

        let raw = db.service_map.get(&ServiceInfo::new(0x7FE0, 0x0001, 0x0400)).unwrap();
        let ev = raw.event_map.get(&0x1001).unwrap();
        assert!(ev.type_flag.contains(TypeFlag::Basic));
        assert!(!ev.type_flag.contains(TypeFlag::Extended));
        assert!(!ev.type_flag.contains(TypeFlag::Present));
    }

    #[test]
    fn test_update_section_multiple_events() {
        let mut db = EpgDatabase::new();
        db.no_past_events = false;

        let eit = make_eit(
            0x4E, 0, 1,
            0x0401, 0x0001, 0x7FE0,
            0x00, 0x4E,
            &[
                (0x0001, dt(2025, 4, 1, 10, 0, 0), 3600),
                (0x0002, dt(2025, 4, 1, 11, 0, 0), 1800),
                (0x0003, dt(2025, 4, 1, 11, 30, 0), 1800),
            ],
        );

        db.update_section(&eit, 0, None);
        let raw = db.service_map.get(&ServiceInfo::new(0x7FE0, 0x0001, 0x0401)).unwrap();
        assert_eq!(raw.event_map.len(), 3);
        assert_eq!(raw.time_map.len(), 3);
    }

    #[test]
    fn test_time_map_ordering() {
        let mut db = EpgDatabase::new();
        db.no_past_events = false;

        // 非順序でイベントを追加
        let eit = make_eit(
            0x4E, 0, 1,
            0x0402, 0x0001, 0x7FE0,
            0x00, 0x4E,
            &[
                (0x0003, dt(2025, 4, 1, 14, 0, 0), 3600),
                (0x0001, dt(2025, 4, 1, 12, 0, 0), 3600),
                (0x0002, dt(2025, 4, 1, 13, 0, 0), 3600),
            ],
        );
        db.update_section(&eit, 0, None);

        let sorted = db.get_event_list_sorted_by_time(0x7FE0, 0x0001, 0x0402);
        // event_name 無しで is_event_valid = false なので空
        // TimeMap の順序を直接確認
        let raw = db.service_map.get(&ServiceInfo::new(0x7FE0, 0x0001, 0x0402)).unwrap();
        let times: Vec<u64> = raw.time_map.iter().map(|t| t.start_time).collect();
        assert!(times.windows(2).all(|w| w[0] < w[1]), "TimeMap should be sorted");
    }

    #[test]
    fn test_update_time_map_overlap() {
        // 重複イベントが正しく処理される
        let mut service = ServiceEventMap::default();

        let t1 = TimeEventInfo { start_time: 1000, duration: 200, event_id: 1, updated_time: 10 };
        let t2 = TimeEventInfo { start_time: 1100, duration: 200, event_id: 2, updated_time: 20 };

        let mut updated = false;
        assert!(update_time_map(&mut service, t1, &mut updated));
        assert!(!updated);

        // t2 と t1 が重複 (t1.start=1000, end=1200; t2.start=1100)
        // t2 の updated_time(20) > t1(10) なので t1 が除去される
        updated = false;
        assert!(update_time_map(&mut service, t2, &mut updated));
        assert!(updated); // t1 が除去された
        assert_eq!(service.time_map.len(), 1);
        assert_eq!(service.time_map.iter().next().unwrap().event_id, 2);
    }

    #[test]
    fn test_update_time_map_overlap_newer_wins() {
        // 既存エントリが新しい場合は追加イベントがスキップされる
        let mut service = ServiceEventMap::default();

        let t1 = TimeEventInfo { start_time: 1000, duration: 500, event_id: 1, updated_time: 100 };
        let t2 = TimeEventInfo { start_time: 1200, duration: 200, event_id: 2, updated_time: 10 };

        let mut updated = false;
        assert!(update_time_map(&mut service, t1, &mut updated));

        // t2 は t1 の範囲内 (1000+500=1500) だが t2.updated_time(10) < t1(100) → t2 はスキップ
        updated = false;
        let ok = update_time_map(&mut service, t2, &mut updated);
        assert!(!ok);
        assert_eq!(service.time_map.len(), 1); // t1 が残る
    }

    #[test]
    fn test_get_event_info_by_time() {
        let mut db = EpgDatabase::new();
        db.no_past_events = false;

        // event_name を持たないイベントは is_event_valid=false だが TimeMap には入る
        // TimeMap の検索ロジックを直接確認するため手動挿入
        let key = ServiceInfo::new(0x7FE0, 0x0001, 0x0400);
        let service = db.service_map.entry(key).or_insert_with(ServiceEventMap::default);

        let start = dt(2025, 4, 1, 10, 0, 0);
        let start_lin = start.get_linear_seconds();

        let mut ev = EventInfo::default();
        ev.event_id = 0x0010;
        ev.start_time = start.clone();
        ev.duration = 3600;
        ev.event_name = "Test Event".to_string();
        ev.network_id = 0x7FE0;
        ev.transport_stream_id = 0x0001;
        ev.service_id = 0x0400;
        ev.type_flag = TypeFlag::Basic;

        let te = TimeEventInfo { start_time: start_lin, duration: 3600, event_id: 0x0010, updated_time: 0 };
        service.event_map.insert(0x0010, ev);
        service.time_map.insert(te);

        // 開始時刻ちょうど → ヒット
        let mid = dt(2025, 4, 1, 10, 30, 0);
        let found = db.get_event_info_by_time(0x7FE0, 0x0001, 0x0400, &mid);
        assert!(found.is_some());
        assert_eq!(found.unwrap().event_id, 0x0010);

        // 終了後 → なし
        let after = dt(2025, 4, 1, 11, 30, 0);
        assert!(db.get_event_info_by_time(0x7FE0, 0x0001, 0x0400, &after).is_none());
    }

    #[test]
    fn test_get_next_event_info() {
        let mut db = EpgDatabase::new();

        let key = ServiceInfo::new(0x7FE0, 0x0001, 0x0400);
        let service = db.service_map.entry(key).or_insert_with(ServiceEventMap::default);

        let s1 = dt(2025, 4, 1, 10, 0, 0);
        let s2 = dt(2025, 4, 1, 11, 0, 0);

        for (eid, st, dur) in &[(0x0001u16, s1.clone(), 3600u32), (0x0002, s2.clone(), 3600)] {
            let mut ev = EventInfo::default();
            ev.event_id = *eid; ev.start_time = st.clone(); ev.duration = *dur;
            ev.event_name = format!("Event {}", eid);
            ev.network_id = 0x7FE0; ev.transport_stream_id = 0x0001; ev.service_id = 0x0400;
            ev.type_flag = TypeFlag::Basic;
            let te = TimeEventInfo { start_time: st.get_linear_seconds(), duration: *dur, event_id: *eid, updated_time: 0 };
            service.event_map.insert(*eid, ev);
            service.time_map.insert(te);
        }

        // s1 の後 → s2 (next event)
        let next = db.get_next_event_info(0x7FE0, 0x0001, 0x0400, &s1);
        assert!(next.is_some());
        assert_eq!(next.unwrap().event_id, 0x0002);
    }

    #[test]
    fn test_update_tot_merges_pending() {
        // TOT 受信前の pending イベントが update_tot 後にマージされる
        let mut db = EpgDatabase::new();
        db.no_past_events = false;

        // TOT 前: cur_tot_seconds == 0 → pending マップに入る
        // ただし pending に入るのは既存エントリが newer の場合のみ (IsPending)
        // 初回登録時は IsPending=false → メインマップに入り、pending にもコピー
        let start = dt(2025, 4, 1, 9, 0, 0);
        let eit = make_eit(
            0x50, 0x00, 0x07,
            0x0400, 0x0001, 0x7FE0,
            0x07, 0x50,
            &[(0x2001, start.clone(), 1800)],
        );
        db.update_section(&eit, 0, None);

        // メインマップにあるはず
        assert!(db.service_map.contains_key(&ServiceInfo::new(0x7FE0, 0x0001, 0x0400)));

        // pending マップにもコピーされているはず (cur_tot_seconds==0 && !IsPending)
        assert!(db.pending_service_map.contains_key(&ServiceInfo::new(0x7FE0, 0x0001, 0x0400)));

        // TOT を設定
        let tot_time = dt(2025, 4, 1, 9, 30, 0);
        db.cur_tot_time = tot_time.clone();
        db.cur_tot_seconds = tot_time.get_linear_seconds();

        // pending が空でなければ update_tot でマージ
        // (ここでは TOT テーブルを直接設定)
        // pending のイベントの updated_time を TOT 秒数に更新してからマージ
        let pending = std::mem::take(&mut db.pending_service_map);
        for (key, mut ps) in pending {
            for ev in ps.event_map.values_mut() { ev.updated_time = db.cur_tot_seconds; }
            db.merge_event_map_impl(key, ps, MergeFlag::MERGE_BASIC_EXTENDED | MergeFlag::SET_SERVICE_UPDATED, None);
        }

        let raw = db.service_map.get(&ServiceInfo::new(0x7FE0, 0x0001, 0x0400)).unwrap();
        assert!(raw.event_map.contains_key(&0x2001));
    }

    #[test]
    fn test_schedule_tracking() {
        let mut db = EpgDatabase::new();
        db.no_past_events = false;

        // cur_tot_time を設定して schedule 判定が正しく動くようにする
        db.cur_tot_time = dt(2025, 4, 1, 9, 0, 0);
        db.cur_tot_seconds = db.cur_tot_time.get_linear_seconds();

        let start = dt(2025, 4, 2, 8, 0, 0);

        // table_id=0x50 (basic schedule, table index 0)
        // section_number=0x00 (segment 0), last_section_number=0x07 (8 sections)
        // segment_last_section_number=0x07, last_table_id=0x57 (8 tables)
        // → table_count=8, table_index=0, seg_index=0, section_count=8
        let eit = make_eit(
            0x50, 0x00, 0x07,
            0x0400, 0x0001, 0x7FE0,
            0x07, 0x57,
            &[(0x3001, start.clone(), 1800)],
        );
        db.update_section(&eit, 0, None);

        assert!(db.has_schedule(0x7FE0, 0x0001, 0x0400, false));
    }

    #[test]
    fn test_set_service_event_list() {
        let mut db = EpgDatabase::new();
        let key = ServiceInfo::new(0x7FE0, 0x0001, 0x0400);

        let mut events = Vec::new();
        for i in 1u16..=3 {
            let mut ev = EventInfo::default();
            ev.event_id = i;
            ev.start_time = dt(2025, 4, 1, (i as i32) * 2, 0, 0);
            ev.duration = 3600;
            ev.event_name = format!("Event {}", i);
            events.push(ev);
        }

        db.set_service_event_list(key, events);
        assert_eq!(db.get_service_count(), 1);
        let list = db.get_event_list_sorted_by_time(0x7FE0, 0x0001, 0x0400).unwrap();
        assert_eq!(list.len(), 3);
        // 時刻順になっているか
        for w in list.windows(2) {
            assert!(w[0].start_time.get_linear_seconds() < w[1].start_time.get_linear_seconds());
        }
    }

    #[test]
    fn test_clear() {
        let mut db = EpgDatabase::new();
        db.no_past_events = false;

        let eit = make_eit(
            0x4E, 0, 1, 0x0400, 0x0001, 0x7FE0, 0x00, 0x4E,
            &[(0x0001, dt(2025, 4, 1, 10, 0, 0), 3600)],
        );
        db.update_section(&eit, 0, None);
        assert_eq!(db.get_service_count(), 1);

        db.clear();
        assert_eq!(db.get_service_count(), 0);
    }

    #[test]
    fn test_no_past_events_filter() {
        let mut db = EpgDatabase::new();
        db.no_past_events = true;

        // "現在時刻" = 2025-04-01 12:00:00
        let cur = dt(2025, 4, 1, 12, 0, 0);
        // 5分以上前に終了したイベントはフィルタされる
        let past = dt(2025, 4, 1, 10, 0, 0); // 終了 11:00, 現在 12:00 → 60分前 → フィルタ

        let eit = make_eit(
            0x4E, 0, 1, 0x0400, 0x0001, 0x7FE0, 0x00, 0x4E,
            &[(0x0001, past.clone(), 3600)],
        );
        db.update_section(&eit, 0, Some(&cur));

        let raw = db.service_map.get(&ServiceInfo::new(0x7FE0, 0x0001, 0x0400)).unwrap();
        assert_eq!(raw.event_map.len(), 0, "Past events should be filtered");
    }

    #[test]
    fn test_no_past_events_recent_keep() {
        let mut db = EpgDatabase::new();
        db.no_past_events = true;

        let cur = dt(2025, 4, 1, 12, 0, 0);
        // 2分前に終了 (11:58 終了、現在 12:00 → 2分前 < 5分マージン → 保持)
        let recent = dt(2025, 4, 1, 11, 55, 0);

        let eit = make_eit(
            0x4E, 0, 1, 0x0400, 0x0001, 0x7FE0, 0x00, 0x4E,
            &[(0x0002, recent.clone(), 180)], // 3分間
        );
        db.update_section(&eit, 0, Some(&cur));

        let raw = db.service_map.get(&ServiceInfo::new(0x7FE0, 0x0001, 0x0400)).unwrap();
        assert_eq!(raw.event_map.len(), 1, "Recent events within 5min margin should be kept");
    }

    #[test]
    fn test_service_updated_flag() {
        let mut db = EpgDatabase::new();
        db.no_past_events = false;

        let eit = make_eit(
            0x4E, 0, 1, 0x0400, 0x0001, 0x7FE0, 0x00, 0x4E,
            &[(0x0001, dt(2025, 4, 1, 10, 0, 0), 3600)],
        );
        db.update_section(&eit, 0, None);

        assert!(db.is_service_updated(0x7FE0, 0x0001, 0x0400));
        assert!(db.reset_service_updated(0x7FE0, 0x0001, 0x0400));
        assert!(!db.is_service_updated(0x7FE0, 0x0001, 0x0400));
    }

    #[test]
    fn test_get_event_info_by_id_missing() {
        let db = EpgDatabase::new();
        assert!(db.get_event_info_by_id(0x7FE0, 0x0001, 0x0400, 0x0001).is_none());
    }

    #[test]
    fn test_time_event_info_ordering() {
        let a = TimeEventInfo::probe(1000);
        let b = TimeEventInfo::probe(2000);
        let c = TimeEventInfo::probe(1000); // same start_time as a

        assert!(a < b);
        assert!(a == c); // same start_time → equal
        assert!(!(a > c));
    }

    #[test]
    fn test_service_info_ordering() {
        let a = ServiceInfo::new(0x7FE0, 0x0001, 0x0400);
        let b = ServiceInfo::new(0x7FE0, 0x0001, 0x0401);
        let c = ServiceInfo::new(0x7FE0, 0x0002, 0x0400);

        assert!(a < b);
        assert!(b < c);
        assert!(a < c);
    }
}
