//! TVTest の `CChannelInput`(`src/ChannelInput.cpp` / `src/ChannelInput.h`)の Rust 移植。
//!
//! チャンネル番号のキー入力を扱う状態機械。数字キー(`0`〜`9`)・テンキー(`VK_NUMPAD0`〜`9`)・
//! ファンクションキー(`VK_F1`〜`F12`)を種別ごとの入力モードに従って蓄積し、確定/継続/取消を判定する。
//!
//! 仮想キーコードは整数値(Win32 `VK_*`)のみを用い、ロジックは純粋なためプラットフォーム非依存。
//! 設定ダイアログ(`CChannelInputOptionsDialog`)は Win32 依存のため対象外。

// 仮想キーコード(Win32 VK_*。整数値は安定しているためここで定義してポータブルに保つ)。
const DIGIT_0: u32 = 0x30;
const DIGIT_9: u32 = 0x39;
const VK_BACK: u32 = 0x08;
const VK_RETURN: u32 = 0x0D;
const VK_ESCAPE: u32 = 0x1B;
const VK_NUMPAD0: u32 = 0x60;
const VK_NUMPAD9: u32 = 0x69;
const VK_F1: u32 = 0x70;
const VK_F12: u32 = 0x7B;

/// キー種別ごとの入力モード(`CChannelInputOptions::KeyInputModeType`、ChannelInput.h:34)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeyInputModeType {
    /// 入力に使わない。
    Disabled,
    /// 1 キーで即確定。
    #[default]
    SingleKey,
    /// 複数キーを連続入力。
    MultipleKeys,
}

/// 入力に使うキーの種別(`CChannelInputOptions::KeyType`、ChannelInput.h:41)。
///
/// 判別値(`as usize`)が [`ChannelInputOptions::key_input_mode`] の添字になる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyType {
    Digit,
    NumPad,
    Function,
}

/// 入力モードを保持できるキー種別数([`KeyType`] の個数)。
pub const NUM_KEY_TYPES: usize = 3;

/// チャンネル入力の設定(`CChannelInputOptions`、ChannelInput.h:31)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelInputOptions {
    /// キー種別ごとの入力モード(添字は [`KeyType`] の判別値)。
    pub key_input_mode: [KeyInputModeType; NUM_KEY_TYPES],
    /// 入力確定までのタイムアウト(ミリ秒)。
    pub key_timeout: u32,
    /// タイムアウト時に確定でなく取消するか。
    pub key_timeout_cancel: bool,
}

impl Default for ChannelInputOptions {
    /// 既定値(全種別 `SingleKey`・タイムアウト 2000ms・タイムアウト取消なし、ChannelInput.cpp:35)。
    fn default() -> Self {
        Self {
            key_input_mode: [KeyInputModeType::SingleKey; NUM_KEY_TYPES],
            key_timeout: 2000,
            key_timeout_cancel: false,
        }
    }
}

/// [`ChannelInput::on_key_down`] の結果(`CChannelInput::KeyDownResult`、ChannelInput.h:58)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyDownResult {
    /// 入力として処理しなかった。
    NotProcessed,
    /// 連続入力を開始した。
    Begin,
    /// 入力が確定した。
    Completed,
    /// 入力を取り消した。
    Cancelled,
    /// 入力を継続中。
    Continue,
}

/// チャンネル番号入力の状態機械(`CChannelInput`、ChannelInput.h:55)。
#[derive(Debug, Clone)]
pub struct ChannelInput {
    options: ChannelInputOptions,
    inputting: bool,
    max_digits: i32,
    cur_digits: i32,
    number: i32,
}

impl ChannelInput {
    /// 設定を与えて生成する(`CChannelInput(const CChannelInputOptions&)`、ChannelInput.cpp:44)。
    pub fn new(options: ChannelInputOptions) -> Self {
        Self {
            options,
            inputting: false,
            max_digits: 0,
            cur_digits: 0,
            number: 0,
        }
    }

    /// 入力を開始する(`BeginInput`、ChannelInput.cpp:50)。桁数上限を指定し状態を初期化する。
    pub fn begin_input(&mut self, max_digits: i32) -> bool {
        self.inputting = true;
        self.max_digits = max_digits;
        self.cur_digits = 0;
        self.number = 0;
        true
    }

    /// 入力を終了する(`EndInput`、ChannelInput.cpp:60)。
    pub fn end_input(&mut self) {
        self.inputting = false;
    }

    /// 入力中か(`IsInputting`、ChannelInput.h:70)。
    pub fn is_inputting(&self) -> bool {
        self.inputting
    }

    /// 桁数上限(`GetMaxDigits`、ChannelInput.h:71)。
    pub fn max_digits(&self) -> i32 {
        self.max_digits
    }

    /// 現在の桁数(`GetCurDigits`、ChannelInput.h:72)。
    pub fn cur_digits(&self) -> i32 {
        self.cur_digits
    }

    /// 現在の入力番号(`GetNumber`、ChannelInput.h:73)。
    pub fn number(&self) -> i32 {
        self.number
    }

    /// キー押下を処理する(`OnKeyDown`、ChannelInput.cpp:66)。
    ///
    /// 数字系キーは種別の入力モードに従い、即確定(`SingleKey`)/連続入力開始(`MultipleKeys`)/
    /// 蓄積を行う。入力中の `Enter`/`Esc`/`BackSpace` は確定/取消/桁削除として処理する。
    pub fn on_key_down(&mut self, key: u32) -> KeyDownResult {
        let digit = if (DIGIT_0..=DIGIT_9).contains(&key) {
            Some((KeyType::Digit, (key - DIGIT_0) as i32))
        } else if (VK_NUMPAD0..=VK_NUMPAD9).contains(&key) {
            Some((KeyType::NumPad, (key - VK_NUMPAD0) as i32))
        } else if (VK_F1..=VK_F12).contains(&key) {
            Some((KeyType::Function, (key - VK_F1) as i32 + 1))
        } else {
            None
        };

        if let Some((key_type, number)) = digit {
            let mode = self.options.key_input_mode[key_type as usize];
            if mode == KeyInputModeType::Disabled {
                return KeyDownResult::NotProcessed;
            }

            if !self.inputting {
                if mode == KeyInputModeType::SingleKey {
                    self.inputting = true;
                    self.number = if number == 0 { 10 } else { number };
                    self.max_digits = if self.number <= 9 { 1 } else { 2 };
                    self.cur_digits = self.max_digits;
                    return KeyDownResult::Completed;
                }

                // MultipleKeys
                self.inputting = true;
                self.max_digits = 0;
                self.number = if number == 0 { 10 } else { number };
                self.cur_digits = if self.number <= 9 { 1 } else { 2 };
                return KeyDownResult::Begin;
            }

            self.number = self.number * 10 + number;
            self.cur_digits += 1;
            if self.max_digits > 0 && self.cur_digits >= self.max_digits {
                return KeyDownResult::Completed;
            }
            return KeyDownResult::Continue;
        } else if self.inputting {
            match key {
                VK_ESCAPE => return KeyDownResult::Cancelled,
                VK_RETURN => {
                    if self.cur_digits < 1 {
                        return KeyDownResult::Cancelled;
                    }
                    return KeyDownResult::Completed;
                }
                VK_BACK => {
                    if self.cur_digits < 2 {
                        return KeyDownResult::Cancelled;
                    }
                    self.cur_digits -= 1;
                    self.number /= 10;
                    return KeyDownResult::Continue;
                }
                _ => {}
            }
        }

        KeyDownResult::NotProcessed
    }

    /// このキーを入力処理で必要とするか(`IsKeyNeeded`、ChannelInput.cpp:133)。
    ///
    /// 入力中の `Enter`/`Esc`/`BackSpace` のみ真。
    pub fn is_key_needed(&self, key: u32) -> bool {
        self.inputting && matches!(key, VK_RETURN | VK_ESCAPE | VK_BACK)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn single_key_input() -> ChannelInput {
        ChannelInput::new(ChannelInputOptions::default())
    }

    fn multi_key_input() -> ChannelInput {
        ChannelInput::new(ChannelInputOptions {
            key_input_mode: [KeyInputModeType::MultipleKeys; NUM_KEY_TYPES],
            ..ChannelInputOptions::default()
        })
    }

    #[test]
    fn options_default() {
        let opts = ChannelInputOptions::default();
        assert_eq!(opts.key_input_mode, [KeyInputModeType::SingleKey; NUM_KEY_TYPES]);
        assert_eq!(opts.key_timeout, 2000);
        assert!(!opts.key_timeout_cancel);
    }

    #[test]
    fn single_key_digit_completes_immediately() {
        let mut ci = single_key_input();
        assert_eq!(ci.on_key_down(b'5' as u32), KeyDownResult::Completed);
        assert!(ci.is_inputting());
        assert_eq!(ci.number(), 5);
        assert_eq!(ci.max_digits(), 1);
        assert_eq!(ci.cur_digits(), 1);
    }

    #[test]
    fn single_key_zero_becomes_ten() {
        let mut ci = single_key_input();
        assert_eq!(ci.on_key_down(b'0' as u32), KeyDownResult::Completed);
        assert_eq!(ci.number(), 10);
        assert_eq!(ci.max_digits(), 2);
        assert_eq!(ci.cur_digits(), 2);
    }

    #[test]
    fn numpad_and_function_keys() {
        let mut ci = single_key_input();
        // テンキー 3
        assert_eq!(ci.on_key_down(VK_NUMPAD0 + 3), KeyDownResult::Completed);
        assert_eq!(ci.number(), 3);

        // ファンクションキー F1 -> 1
        let mut ci2 = single_key_input();
        assert_eq!(ci2.on_key_down(VK_F1), KeyDownResult::Completed);
        assert_eq!(ci2.number(), 1);

        // F10 -> 10
        let mut ci3 = single_key_input();
        assert_eq!(ci3.on_key_down(VK_F1 + 9), KeyDownResult::Completed);
        assert_eq!(ci3.number(), 10);
        assert_eq!(ci3.max_digits(), 2);
    }

    #[test]
    fn multiple_keys_accumulate_then_enter() {
        let mut ci = multi_key_input();
        // 1 桁目
        assert_eq!(ci.on_key_down(b'1' as u32), KeyDownResult::Begin);
        assert!(ci.is_inputting());
        assert_eq!(ci.number(), 1);
        assert_eq!(ci.max_digits(), 0);
        assert_eq!(ci.cur_digits(), 1);
        // 2 桁目(max_digits=0 なので確定しない)
        assert_eq!(ci.on_key_down(b'2' as u32), KeyDownResult::Continue);
        assert_eq!(ci.number(), 12);
        assert_eq!(ci.cur_digits(), 2);
        // Enter で確定
        assert_eq!(ci.on_key_down(VK_RETURN), KeyDownResult::Completed);
    }

    #[test]
    fn begin_input_with_max_digits_completes_on_limit() {
        let mut ci = multi_key_input();
        ci.begin_input(2);
        assert!(ci.is_inputting());
        assert_eq!(ci.max_digits(), 2);
        assert_eq!(ci.cur_digits(), 0);
        assert_eq!(ci.number(), 0);
        // 1 桁目: 1 < 2 -> 継続
        assert_eq!(ci.on_key_down(b'1' as u32), KeyDownResult::Continue);
        assert_eq!(ci.number(), 1);
        // 2 桁目: 2 >= 2 -> 確定
        assert_eq!(ci.on_key_down(b'2' as u32), KeyDownResult::Completed);
        assert_eq!(ci.number(), 12);
    }

    #[test]
    fn backspace_removes_digit_then_cancels() {
        let mut ci = multi_key_input();
        ci.on_key_down(b'1' as u32); // Begin, cur 1
        ci.on_key_down(b'2' as u32); // Continue, cur 2, number 12
        // BackSpace: cur 2 -> 1, number 1
        assert_eq!(ci.on_key_down(VK_BACK), KeyDownResult::Continue);
        assert_eq!(ci.cur_digits(), 1);
        assert_eq!(ci.number(), 1);
        // さらに BackSpace: cur 1 < 2 -> 取消
        assert_eq!(ci.on_key_down(VK_BACK), KeyDownResult::Cancelled);
    }

    #[test]
    fn escape_cancels_and_enter_needs_digit() {
        let mut ci = multi_key_input();
        ci.begin_input(0);
        // 桁が無い状態の Enter は取消
        assert_eq!(ci.on_key_down(VK_RETURN), KeyDownResult::Cancelled);
        // Esc は取消
        ci.begin_input(0);
        assert_eq!(ci.on_key_down(VK_ESCAPE), KeyDownResult::Cancelled);
    }

    #[test]
    fn disabled_mode_not_processed() {
        let mut opts = ChannelInputOptions::default();
        opts.key_input_mode[KeyType::Digit as usize] = KeyInputModeType::Disabled;
        let mut ci = ChannelInput::new(opts);
        assert_eq!(ci.on_key_down(b'5' as u32), KeyDownResult::NotProcessed);
        assert!(!ci.is_inputting());
    }

    #[test]
    fn unrelated_key_not_processed() {
        let mut ci = single_key_input();
        // 入力中でない状態の他キー
        assert_eq!(ci.on_key_down(b'A' as u32), KeyDownResult::NotProcessed);
        // 入力中の他キー(処理対象外)
        ci.begin_input(0);
        assert_eq!(ci.on_key_down(b'A' as u32), KeyDownResult::NotProcessed);
    }

    #[test]
    fn is_key_needed_only_while_inputting() {
        let mut ci = single_key_input();
        assert!(!ci.is_key_needed(VK_RETURN));
        ci.begin_input(0);
        assert!(ci.is_key_needed(VK_RETURN));
        assert!(ci.is_key_needed(VK_ESCAPE));
        assert!(ci.is_key_needed(VK_BACK));
        assert!(!ci.is_key_needed(b'5' as u32));
    }

    #[test]
    fn end_input_clears_state() {
        let mut ci = single_key_input();
        ci.on_key_down(b'5' as u32);
        assert!(ci.is_inputting());
        ci.end_input();
        assert!(!ci.is_inputting());
    }
}
