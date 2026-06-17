// Rust port of LibISDB/Filters/EPGDatabaseFilter.cpp + EPGDatabaseFilter.hpp
//
// 番組情報フィルタ。SingleIOFilter として、入力 TS パケットから H-EIT / L-EIT / TOT を
// 取り出して EPGDatabase を更新しつつ、入力をそのまま下流へ渡す(パススルー)。
//
// 原実装との対応:
//   - C++ の SingleIOFilter 継承 → FilterBase + FilterSink を実装(ProcessData 後にパススルー出力)
//   - C++ の PIDMapManager + PSITableBase::CreateWithHandler<EITPfScheduleTable>(OnEITSection)
//     による自己参照コールバックを避け、本フィルタが H-EIT/L-EIT 用の EITPfScheduleTable と
//     TOTTable を直接保持し、PID でルーティングする(他フィルタ移植と同じ平坦化方式)。
//   - C++ の EPGDatabase::EventListener::OnScheduleStatusReset(m_ResetTable を立てる)は、
//     EpgDatabase::update_section が返す ScheduleEvent::Reset で表現される。Reset を受け取ったら
//     原実装どおり H-EIT/L-EIT 両方の EITPfScheduleTable に対し該当サービスを reset する。
//
// 重要な挙動(原実装由来):
//   - OnEITSection はセクションが更新(内容差分)されるたびに発火し、その都度 UpdateSection を
//     呼ぶ。1 つの TS パケットで複数 EIT セクションが完成しうるため、store_packet は更新された
//     EITTable のリストを返し、本フィルタはその全てを処理する(取りこぼし防止)。
//   - EPGDatabase 未設定でもテーブルへの格納(状態更新)は行う(C++ も StorePacketStream は実行)。

use std::cell::RefCell;
use std::rc::Rc;

use libisdb_datetime::DateTime;
use libisdb_epg_database::{EpgDatabase, ScheduleEvent};
use libisdb_filter_base::{DataStream, FilterBase, FilterSink, OutputSlot};
use libisdb_ts_info::{PID_HEIT, PID_LEIT, PID_TOT};
use libisdb_ts_packet::{TsPacket, TS_PACKET_SIZE};
use libisdb_ts_tables::{EITPfScheduleTable, TOTTable};

/// EPGDatabase の共有ハンドル。C++ の `EPGDatabase *`(外部所有・共有)に相当。
pub type EpgDatabaseHandle = Rc<RefCell<EpgDatabase>>;

/// イベント情報のソース ID 型。EventInfo::SourceIDType 相当。
pub type SourceIdType = u32;

/// 番組情報フィルタ。EPGDatabaseFilter.hpp:42。
pub struct EpgDatabaseFilter {
    /// H-EIT (PID 0x0012) 用スケジュールテーブル
    heit_table: EITPfScheduleTable,
    /// L-EIT (PID 0x0027) 用スケジュールテーブル
    leit_table: EITPfScheduleTable,
    /// TOT (PID 0x0014)
    tot_table: TOTTable,

    epg_database: Option<EpgDatabaseHandle>,
    source_id: SourceIdType,

    /// EPGDatabase::UpdateSection に渡す現在時刻(C++ の GetCurrentEPGTime 注入)。
    /// None の場合は過去イベントのフィルタリングを行わない。
    current_time: Option<DateTime>,

    /// 出力スロット(SingleIOFilter のパススルー出力)
    output: OutputSlot,
}

impl EpgDatabaseFilter {
    /// EPGDatabaseFilter::EPGDatabaseFilter (cpp:38)。コンストラクタは Reset() を呼ぶ。
    pub fn new() -> Self {
        let mut s = Self {
            heit_table: EITPfScheduleTable::new(),
            leit_table: EITPfScheduleTable::new(),
            tot_table: TOTTable::new(),
            epg_database: None,
            source_id: 0,
            current_time: None,
            output: OutputSlot::new(),
        };
        s.reset_impl();
        s
    }

    // ── 出力スロット接続 ───────────────────────────────────────

    pub fn connect_output(&mut self, sink: Box<dyn FilterSink>) {
        self.output.connect(sink);
    }
    pub fn disconnect_output(&mut self) {
        self.output.disconnect();
    }
    pub fn is_output_connected(&self) -> bool {
        self.output.is_connected()
    }

    // ── 設定 / 取得 (EPGDatabaseFilter.cpp:75-) ─────────────────

    /// EPGDatabaseFilter::SetEPGDatabase (cpp:75)。
    ///
    /// C++ は旧 DB から EventListener を外し新 DB へ登録するが、本移植では
    /// スケジュールリセットを update_section の返り値で受け取るためリスナー登録は不要。
    pub fn set_epg_database(&mut self, database: Option<EpgDatabaseHandle>) {
        self.epg_database = database;
    }

    /// EPGDatabaseFilter::GetEPGDatabase (cpp:89)
    pub fn get_epg_database(&self) -> Option<EpgDatabaseHandle> {
        self.epg_database.clone()
    }

    /// EPGDatabaseFilter::SetSourceID (cpp:95)
    pub fn set_source_id(&mut self, id: SourceIdType) {
        self.source_id = id;
    }

    /// EPGDatabaseFilter::GetSourceID (cpp:103)
    pub fn get_source_id(&self) -> SourceIdType {
        self.source_id
    }

    /// EPGDatabase::UpdateSection に渡す現在時刻を設定する(GetCurrentEPGTime 注入)。
    pub fn set_current_time(&mut self, time: Option<DateTime>) {
        self.current_time = time;
    }

    // ── 内部処理 ───────────────────────────────────────────────

    /// EPGDatabaseFilter::Reset (cpp:46)
    fn reset_impl(&mut self) {
        // m_PIDMapManager.UnmapAllTargets() + H-EIT/L-EIT/TOT 再マップ相当
        self.heit_table.reset();
        self.leit_table.reset();
        self.tot_table.reset();

        if let Some(db) = &self.epg_database {
            db.borrow_mut().reset_tot_time();
        }
    }

    /// EPGDatabaseFilter::ProcessData (cpp:66)。TS パケット列を PID でルーティングする。
    fn process_data(&mut self, stream: &mut dyn DataStream) {
        if !stream.is_ts_packet() {
            return;
        }
        loop {
            let data = stream.data();
            if data.len() >= TS_PACKET_SIZE {
                let mut arr = [0u8; TS_PACKET_SIZE];
                arr.copy_from_slice(&data[..TS_PACKET_SIZE]);
                let mut pkt = TsPacket::new(&arr);
                pkt.parse_packet(None);
                self.store_packet(&pkt);
            }
            if !stream.next() {
                break;
            }
        }
    }

    /// PIDMapManager::StorePacketStream 相当。PID で各テーブルへルーティングする。
    fn store_packet(&mut self, pkt: &TsPacket) {
        match pkt.get_pid() {
            PID_HEIT => self.process_eit(pkt, true),
            PID_LEIT => self.process_eit(pkt, false),
            PID_TOT => self.process_tot(pkt),
            _ => {}
        }
    }

    /// EPGDatabaseFilter::OnEITSection (cpp:117) 相当。
    ///
    /// 更新された各 EIT セクションについて UpdateSection を呼び、スケジュールリセットが
    /// 通知されたら H-EIT/L-EIT 両テーブルの該当サービスを reset する。
    fn process_eit(&mut self, pkt: &TsPacket, is_heit: bool) {
        // EPGDatabase 未設定でもテーブルへの格納は行う(C++ も StorePacketStream を実行)。
        let updated = if is_heit {
            self.heit_table.store_packet(pkt)
        } else {
            self.leit_table.store_packet(pkt)
        };

        let db = match &self.epg_database {
            Some(d) => d.clone(),
            None => return,
        };

        for eit in &updated {
            let result = {
                let mut db_ref = db.borrow_mut();
                db_ref.update_section(eit, self.source_id, self.current_time.as_ref())
            };
            if let Some(result) = result {
                for ev in &result.events {
                    if let ScheduleEvent::Reset {
                        network_id,
                        transport_stream_id,
                        service_id,
                    } = ev
                    {
                        // C++: m_ResetTable が立ったら H-EIT/L-EIT 両方を ResetScheduleService
                        self.heit_table
                            .reset_schedule_service(*network_id, *transport_stream_id, *service_id);
                        self.leit_table
                            .reset_schedule_service(*network_id, *transport_stream_id, *service_id);
                    }
                }
            }
        }
    }

    /// EPGDatabaseFilter::OnTOTSection (cpp:149) 相当。
    fn process_tot(&mut self, pkt: &TsPacket) {
        if !self.tot_table.store_packet(pkt) {
            return;
        }
        if let Some(db) = &self.epg_database {
            db.borrow_mut().update_tot(&self.tot_table);
        }
    }
}

impl Default for EpgDatabaseFilter {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// FilterBase 実装
// ---------------------------------------------------------------------------

impl FilterBase for EpgDatabaseFilter {
    fn input_count(&self) -> usize {
        1
    }
    fn output_count(&self) -> usize {
        1
    }

    /// EPGDatabaseFilter::Reset (cpp:46)
    fn reset(&mut self) {
        self.reset_impl();
    }
}

// ---------------------------------------------------------------------------
// FilterSink 実装(SingleIOFilter: ProcessData → OutputData パススルー)
// ---------------------------------------------------------------------------

impl FilterSink for EpgDatabaseFilter {
    fn receive_data(&mut self, stream: &mut dyn DataStream) -> bool {
        // SingleIOFilter::ReceiveData = ProcessData(pData) → OutputData(pData)
        self.process_data(stream);
        // パススルー出力(OutputData は内部で rewind してから下流へ渡す)
        self.output.send(stream);
        true
    }
}

#[cfg(test)]
mod tests;
