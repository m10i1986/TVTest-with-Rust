// LibISDB の BitRateCalculator.cpp を Rust へ移植したもの。
//
// 移植対象:
//   - BitRateCalculator : BitRateCalculator.cpp:35 (ビットレート計算クラス)
//
// C++ の TickClock は OS 時刻依存のため、クロック関数をクロージャとして注入する設計に変更。
// Windows では GetTickCount64() が 1000 clocks/sec (ミリ秒精度)。
// ClocksPerSec = 1000 を定数としてそのまま使用する。

/// クロック単位 (TickClock::ClocksPerSec の相当値)。
/// Windows では 1000 (ミリ秒精度)。BitRateCalculator.hpp:49。
pub const CLOCKS_PER_SEC: u64 = 1000;

/// ビットレート計算器。BitRateCalculator.hpp:38。
///
/// クロック取得関数 `clock_fn` を外部から注入することで
/// OS 依存なしにテスト可能にする。
pub struct BitRateCalculator<F: Fn() -> u64> {
    clock_fn: F,
    last_clock: u64,
    update_interval: u64,
    bytes: u64,
    bit_rate: u64,
}

impl<F: Fn() -> u64> BitRateCalculator<F> {
    /// 指定したクロック関数で初期化する。
    /// update_interval は clocks 単位(デフォルト CLOCKS_PER_SEC)。
    pub fn new(clock_fn: F) -> Self {
        Self {
            clock_fn,
            last_clock: 0,
            update_interval: CLOCKS_PER_SEC,
            bytes: 0,
            bit_rate: 0,
        }
    }

    /// 現在時刻を基点として初期化する。BitRateCalculator.cpp:42。
    pub fn initialize(&mut self) {
        self.last_clock = (self.clock_fn)();
        self.bytes = 0;
        self.bit_rate = 0;
    }

    /// 累積値をリセットする。BitRateCalculator.cpp:50。
    pub fn reset(&mut self) {
        self.last_clock = 0;
        self.bytes = 0;
        self.bit_rate = 0;
    }

    /// バイト数を追加してビットレートを更新する。BitRateCalculator.cpp:58。
    /// 更新間隔を超えた場合に `true` を返す。
    pub fn update(&mut self, bytes: usize) -> bool {
        let now = (self.clock_fn)();
        let mut updated = false;

        if now >= self.last_clock {
            self.bytes += bytes as u64;
            if now - self.last_clock >= self.update_interval {
                // bit_rate = bytes * 8 * ClocksPerSec / elapsed
                self.bit_rate = self.bytes
                    .saturating_mul(8 * CLOCKS_PER_SEC)
                    / (now - self.last_clock);
                self.last_clock = now;
                self.bytes = 0;
                updated = true;
            }
        } else {
            // クロックが巻き戻った場合はリセット
            self.last_clock = now;
            self.bytes = 0;
        }

        updated
    }

    /// 現在のビットレートを返す。BitRateCalculator.cpp:82。
    /// 最終更新から 2 インターバル以上経過していれば 0 を返す。
    pub fn get_bit_rate(&self) -> u64 {
        let now = (self.clock_fn)();
        if now - self.last_clock >= 2 * self.update_interval {
            return 0;
        }
        self.bit_rate
    }

    /// 更新間隔を設定する。BitRateCalculator.cpp:90。
    pub fn set_update_interval(&mut self, interval: u64) -> bool {
        if interval < 1 {
            return false;
        }
        self.update_interval = interval;
        true
    }

    /// 更新間隔を取得する。BitRateCalculator.hpp:48。
    pub fn get_update_interval(&self) -> u64 {
        self.update_interval
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    fn make_calc(clock: Rc<Cell<u64>>) -> BitRateCalculator<impl Fn() -> u64> {
        let c = clock.clone();
        BitRateCalculator::new(move || c.get())
    }

    #[test]
    fn test_initial_state() {
        let clock = Rc::new(Cell::new(0u64));
        let calc = make_calc(clock.clone());
        assert_eq!(calc.get_bit_rate(), 0);
        assert_eq!(calc.get_update_interval(), CLOCKS_PER_SEC);
    }

    #[test]
    fn test_reset() {
        let clock = Rc::new(Cell::new(500u64));
        let mut calc = make_calc(clock.clone());
        calc.initialize();
        clock.set(1500);
        let updated = calc.update(1000);
        assert!(updated);
        calc.reset();
        assert_eq!(calc.get_bit_rate(), 0);
    }

    #[test]
    fn test_update_no_interval_not_elapsed() {
        let clock = Rc::new(Cell::new(0u64));
        let mut calc = make_calc(clock.clone());
        calc.initialize();
        clock.set(500); // 0.5 sec 経過: interval 未満
        let updated = calc.update(1000);
        assert!(!updated);
        assert_eq!(calc.get_bit_rate(), 0);
    }

    #[test]
    fn test_update_interval_elapsed() {
        let clock = Rc::new(Cell::new(0u64));
        let mut calc = make_calc(clock.clone());
        calc.initialize();
        // 1000 ms (= CLOCKS_PER_SEC) 経過, 1000 バイト送信
        clock.set(1000);
        let updated = calc.update(1000);
        assert!(updated);
        // bit_rate = 1000 * 8 * 1000 / 1000 = 8000 bps
        assert_eq!(calc.get_bit_rate(), 8000);
    }

    #[test]
    fn test_bit_rate_calc() {
        let clock = Rc::new(Cell::new(0u64));
        let mut calc = make_calc(clock.clone());
        calc.initialize();
        // 2000 ms, 500 バイト
        clock.set(2000);
        calc.update(500);
        // bit_rate = 500 * 8 * 1000 / 2000 = 2000 bps
        assert_eq!(calc.get_bit_rate(), 2000);
    }

    #[test]
    fn test_get_bit_rate_stale() {
        let clock = Rc::new(Cell::new(0u64));
        let mut calc = make_calc(clock.clone());
        calc.initialize();
        clock.set(1000);
        calc.update(1000);
        // 直後は 8000 bps
        assert_eq!(calc.get_bit_rate(), 8000);
        // 2 インターバル (2000ms) 以上経過 → 0
        clock.set(3001);
        assert_eq!(calc.get_bit_rate(), 0);
    }

    #[test]
    fn test_set_update_interval_valid() {
        let clock = Rc::new(Cell::new(0u64));
        let mut calc = make_calc(clock.clone());
        assert!(calc.set_update_interval(500));
        assert_eq!(calc.get_update_interval(), 500);
    }

    #[test]
    fn test_set_update_interval_zero() {
        let clock = Rc::new(Cell::new(0u64));
        let mut calc = make_calc(clock.clone());
        assert!(!calc.set_update_interval(0));
        assert_eq!(calc.get_update_interval(), CLOCKS_PER_SEC);
    }

    #[test]
    fn test_clock_rollback() {
        let clock = Rc::new(Cell::new(2000u64));
        let mut calc = make_calc(clock.clone());
        calc.initialize();
        // クロックが 1000 (過去) に戻る
        clock.set(1000);
        let updated = calc.update(1000);
        // 巻き戻しはリセット扱い: updated = false
        assert!(!updated);
        // last_clock が now に更新されているので次の update で正常動作
        clock.set(2001);
        let updated2 = calc.update(500);
        assert!(updated2);
    }

    #[test]
    fn test_multiple_updates_accumulate() {
        let clock = Rc::new(Cell::new(0u64));
        let mut calc = make_calc(clock.clone());
        calc.initialize();
        clock.set(500);
        calc.update(200); // 累積 200
        clock.set(800);
        calc.update(300); // 累積 500
        clock.set(1000);
        let updated = calc.update(0); // elapsed = 1000ms
        assert!(updated);
        // bit_rate = 500 * 8 * 1000 / 1000 = 4000 bps
        assert_eq!(calc.get_bit_rate(), 4000);
    }
}
