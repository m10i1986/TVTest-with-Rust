// LibISDB の PIDMap.cpp + PIDMap.hpp を Rust へ移植したもの。
//
// 移植対象:
//   - PIDMapTarget trait: PIDMap.hpp:40
//   - PIDMapManager: PIDMap.cpp:36
//
// C++ の PIDMapTarget は仮想基底クラス。Rust では trait object (Box<dyn PIDMapTarget>) で代替。
// DataStream は未移植のため StorePacketStream は省略。

use libisdb_ts_packet::TsPacket;

/// PID 最大値。TSPacket.hpp の PID_MAX 相当。
pub const PID_MAX: u16 = 0x1FFF;

/// PID マップターゲット trait。PIDMap.hpp:40。
pub trait PIDMapTarget {
    fn store_packet(&mut self, packet: &TsPacket) -> bool;
    fn on_pid_mapped(&mut self, _pid: u16) {}
    fn on_pid_unmapped(&mut self, _pid: u16) {}
}

const PID_TABLE_SIZE: usize = (PID_MAX as usize) + 1;

/// PID マップ管理。PIDMap.cpp:36。
///
/// 8192 エントリの PID テーブルで、各 PID に対してターゲットを登録する。
/// ターゲットは Box<dyn PIDMapTarget> で所有権を管理する。
pub struct PIDMapManager {
    pid_map: Vec<Option<Box<dyn PIDMapTarget + 'static>>>,
    map_count: u16,
}

impl PIDMapManager {
    /// 新規作成。PIDMap.cpp:36。
    pub fn new() -> Self {
        let mut v: Vec<Option<Box<dyn PIDMapTarget + 'static>>> = Vec::with_capacity(PID_TABLE_SIZE);
        for _ in 0..PID_TABLE_SIZE {
            v.push(None);
        }
        Self {
            pid_map: v,
            map_count: 0,
        }
    }

    /// TS パケットを対応するターゲットへルーティング。PIDMap.cpp:49。
    pub fn store_packet(&mut self, packet: &TsPacket) -> bool {
        let pid = packet.get_pid();
        if pid > PID_MAX {
            return false;
        }
        match self.pid_map[pid as usize].as_mut() {
            Some(target) => target.store_packet(packet),
            None => false,
        }
    }

    /// ターゲットを PID に登録。PIDMap.cpp:89。
    pub fn map_target(&mut self, pid: u16, mut target: Box<dyn PIDMapTarget + 'static>) -> bool {
        if pid > PID_MAX {
            return false;
        }
        self.unmap_target(pid);
        target.on_pid_mapped(pid);
        self.pid_map[pid as usize] = Some(target);
        self.map_count += 1;
        true
    }

    /// PID のターゲット登録を解除。PIDMap.cpp:105。
    pub fn unmap_target(&mut self, pid: u16) -> bool {
        if pid > PID_MAX {
            return false;
        }
        if let Some(mut target) = self.pid_map[pid as usize].take() {
            target.on_pid_unmapped(pid);
            self.map_count -= 1;
            true
        } else {
            false
        }
    }

    /// 全 PID のターゲット登録を解除。PIDMap.cpp:124。
    pub fn unmap_all_targets(&mut self) {
        for pid in 0..PID_TABLE_SIZE {
            if let Some(mut target) = self.pid_map[pid].take() {
                target.on_pid_unmapped(pid as u16);
                self.map_count -= 1;
            }
        }
    }

    /// 指定 PID のターゲット参照を取得。PIDMap.cpp:132。
    pub fn get_map_target(&self, pid: u16) -> Option<&(dyn PIDMapTarget + 'static)> {
        if pid > PID_MAX {
            return None;
        }
        self.pid_map[pid as usize].as_deref()
    }

    /// 指定 PID のターゲット可変参照を取得。
    pub fn get_map_target_mut(&mut self, pid: u16) -> Option<&mut (dyn PIDMapTarget + 'static)> {
        if pid > PID_MAX {
            return None;
        }
        self.pid_map[pid as usize].as_deref_mut()
    }

    /// 登録ターゲット数。PIDMap.cpp:141。
    pub fn get_map_count(&self) -> u16 {
        self.map_count
    }
}

impl Default for PIDMapManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libisdb_ts_packet::{TS_PACKET_SIZE, TsPacket};

    struct CountingTarget {
        pub count: usize,
    }

    impl CountingTarget {
        fn new() -> Self {
            Self { count: 0 }
        }
    }

    impl PIDMapTarget for CountingTarget {
        fn store_packet(&mut self, _packet: &TsPacket) -> bool {
            self.count += 1;
            true
        }
    }

    fn make_packet(pid: u16) -> TsPacket {
        let mut data = [0u8; TS_PACKET_SIZE];
        data[0] = 0x47;
        data[1] = ((pid >> 8) & 0x1F) as u8;
        data[2] = (pid & 0xFF) as u8;
        data[3] = 0x10; // adaptation_field_control=0b01 (payload only)
        let mut pkt = TsPacket::new(&data);
        pkt.parse_packet(None);
        pkt
    }

    #[test]
    fn test_new_empty() {
        let mgr = PIDMapManager::new();
        assert_eq!(mgr.get_map_count(), 0);
    }

    #[test]
    fn test_map_and_store() {
        let mut mgr = PIDMapManager::new();
        assert!(mgr.map_target(0x0100, Box::new(CountingTarget::new())));
        assert_eq!(mgr.get_map_count(), 1);

        let pkt = make_packet(0x0100);
        assert!(mgr.store_packet(&pkt));
    }

    #[test]
    fn test_store_unmapped_pid() {
        let mut mgr = PIDMapManager::new();
        let pkt = make_packet(0x0100);
        assert!(!mgr.store_packet(&pkt));
    }

    #[test]
    fn test_unmap() {
        let mut mgr = PIDMapManager::new();
        mgr.map_target(0x0010, Box::new(CountingTarget::new()));
        assert_eq!(mgr.get_map_count(), 1);
        assert!(mgr.unmap_target(0x0010));
        assert_eq!(mgr.get_map_count(), 0);
    }

    #[test]
    fn test_unmap_nonexistent() {
        let mut mgr = PIDMapManager::new();
        assert!(!mgr.unmap_target(0x0100));
    }

    #[test]
    fn test_remap_replaces_old_target() {
        let mut mgr = PIDMapManager::new();
        mgr.map_target(0x0100, Box::new(CountingTarget::new()));
        assert_eq!(mgr.get_map_count(), 1);
        // 再登録 → 古いターゲットが解除されカウントは 1 のまま
        mgr.map_target(0x0100, Box::new(CountingTarget::new()));
        assert_eq!(mgr.get_map_count(), 1);
    }

    #[test]
    fn test_pid_max_boundary() {
        let mut mgr = PIDMapManager::new();
        // PID_MAX(0x1FFF) は有効
        assert!(mgr.map_target(PID_MAX, Box::new(CountingTarget::new())));
        // PID_MAX+1 は無効
        assert!(!mgr.map_target(PID_MAX + 1, Box::new(CountingTarget::new())));
        assert_eq!(mgr.get_map_count(), 1);
    }

    #[test]
    fn test_unmap_all() {
        let mut mgr = PIDMapManager::new();
        mgr.map_target(0x0010, Box::new(CountingTarget::new()));
        mgr.map_target(0x0011, Box::new(CountingTarget::new()));
        mgr.map_target(0x0012, Box::new(CountingTarget::new()));
        assert_eq!(mgr.get_map_count(), 3);
        mgr.unmap_all_targets();
        assert_eq!(mgr.get_map_count(), 0);
    }

    #[test]
    fn test_get_map_target_absent() {
        let mgr = PIDMapManager::new();
        assert!(mgr.get_map_target(0x0100).is_none());
    }

    #[test]
    fn test_get_map_target_invalid_pid() {
        let mgr = PIDMapManager::new();
        assert!(mgr.get_map_target(0xFFFF).is_none());
    }

    #[test]
    fn test_multiple_pids() {
        let mut mgr = PIDMapManager::new();
        for &pid in &[0x0010_u16, 0x0020, 0x0030] {
            mgr.map_target(pid, Box::new(CountingTarget::new()));
        }
        assert_eq!(mgr.get_map_count(), 3);

        for &pid in &[0x0010_u16, 0x0020, 0x0030] {
            let pkt = make_packet(pid);
            assert!(mgr.store_packet(&pkt));
        }

        // 無関係 PID はルーティングされない
        let pkt = make_packet(0x0040);
        assert!(!mgr.store_packet(&pkt));
    }

    #[test]
    fn test_default() {
        let mgr = PIDMapManager::default();
        assert_eq!(mgr.get_map_count(), 0);
    }

    #[test]
    fn test_store_packet_routing_count() {
        let mut mgr = PIDMapManager::new();
        mgr.map_target(0x0100, Box::new(CountingTarget::new()));
        mgr.map_target(0x0200, Box::new(CountingTarget::new()));

        // 0x0100 に 3 回送る
        for _ in 0..3 {
            mgr.store_packet(&make_packet(0x0100));
        }
        // 0x0200 に 2 回送る
        for _ in 0..2 {
            mgr.store_packet(&make_packet(0x0200));
        }

        // get_map_target_mut でカウントを確認
        let t100 = mgr.get_map_target_mut(0x0100).unwrap();
        let ct100 = unsafe { &*(t100 as *mut dyn PIDMapTarget as *mut CountingTarget) };
        assert_eq!(ct100.count, 3);

        let t200 = mgr.get_map_target_mut(0x0200).unwrap();
        let ct200 = unsafe { &*(t200 as *mut dyn PIDMapTarget as *mut CountingTarget) };
        assert_eq!(ct200.count, 2);
    }
}
