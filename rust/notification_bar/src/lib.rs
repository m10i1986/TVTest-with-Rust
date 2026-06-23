//! TVTest の通知バー(src/NotificationBar.cpp / NotificationBar.h)の純粋部分の移植。
//!
//! 画面上部に短時間表示される通知バー(`CNotificationBar`)のうち、ウィンドウや描画から
//! 切り離せる以下のロジックを純粋なモデルとして表現する:
//! - [`MessageType`](情報 / 警告 / エラー)
//! - [`MessageInfo`](表示する 1 件のメッセージ)
//! - [`MessageQueue`](表示中メッセージのキュー操作: 追加時のスキップ可能メッセージ差し替え、
//!   満了による前進、表示状態の遷移)
//! - [`NotificationBarStyle`](余白/アイコン寸法とバー高さ算出)
//! - [`animated_bar_bottom`] / [`bar_position`](表示・フェードアニメーションのバー位置)
//!
//! # 対象外(Win32 / I-O 依存)
//! ウィンドウ生成(`Create`)、描画(`OnMessage`/`WM_PAINT`)、タイマー(`BeginTimer` 等)、
//! テーマ色・フォント実体・アイコン読み込み、スタイル文字列の読み取り(`SetStyle`)。

use std::collections::VecDeque;

/// 通知メッセージの種別(NotificationBar.h 43-47 の `MessageType`)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageType {
    /// 情報(`Info`)。
    Info = 0,
    /// 警告(`Warning`)。
    Warning = 1,
    /// エラー(`Error`)。
    Error = 2,
}

impl MessageType {
    /// 種別を 0/1/2 の索引に変換する(`static_cast<int>(Type)`、色配列 `m_TextColor` 等の添字)。
    pub fn index(self) -> usize {
        self as usize
    }
}

/// 上下左右の余白(Style::Margins 相当)。NotificationBar では Padding / IconMargin / TextMargin。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Margins {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Margins {
    /// 全辺同じ値の余白。
    pub fn all(value: i32) -> Self {
        Self { left: value, top: value, right: value, bottom: value }
    }

    /// 個別指定の余白。
    pub fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self { left, top, right, bottom }
    }

    /// 水平方向の余白合計(`Horz()` = left + right)。
    pub fn horz(&self) -> i32 {
        self.left + self.right
    }

    /// 垂直方向の余白合計(`Vert()` = top + bottom)。
    pub fn vert(&self) -> i32 {
        self.top + self.bottom
    }
}

/// 寸法(Style::Size 相当)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Size {
    pub width: i32,
    pub height: i32,
}

impl Size {
    pub fn new(width: i32, height: i32) -> Self {
        Self { width, height }
    }
}

/// 矩形(RECT 相当)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self { left, top, right, bottom }
    }
}

/// 通知バーのスタイル(NotificationBar.h 79-91 `NotificationBarStyle`)。
///
/// `IconSize` の既定は原実装ではシステムメトリクス(`SM_CXSMICON`/`SM_CYSMICON`)に依存するが、
/// 本移植では一般的な 16x16 を既定とする(実値は呼び出し側で設定可能)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotificationBarStyle {
    /// 外周の余白(`Padding`、既定 `{4,2,4,2}`)。
    pub padding: Margins,
    /// アイコンの寸法(`IconSize`)。
    pub icon_size: Size,
    /// アイコンの余白(`IconMargin`、既定 `{0,0,4,0}`)。
    pub icon_margin: Margins,
    /// テキストの余白(`TextMargin`、既定 0)。
    pub text_margin: Margins,
    /// フォント高さに加える余白(`TextExtraHeight`、既定 4)。
    pub text_extra_height: i32,
}

impl Default for NotificationBarStyle {
    fn default() -> Self {
        // NotificationBar.h 81-85 の既定値。
        Self {
            padding: Margins::new(4, 2, 4, 2),
            icon_size: Size::new(16, 16),
            icon_margin: Margins::new(0, 0, 4, 0),
            text_margin: Margins::all(0),
            text_extra_height: 4,
        }
    }
}

impl NotificationBarStyle {
    /// 既定スタイルを生成する。
    pub fn new() -> Self {
        Self::default()
    }

    /// バーの高さを算出する(NotificationBar.cpp 200-208 `CalcBarHeight`)。
    ///
    /// `font_height` は原実装の `Style::GetFontHeight(hwnd, font, TextExtraHeight)` の結果
    /// (= フォント高さに `TextExtraHeight` を加味した値)を呼び出し側で渡す。
    pub fn calc_bar_height(&self, font_height: i32) -> i32 {
        let icon_height = self.icon_size.height + self.icon_margin.vert();
        let text_height = font_height + self.text_margin.vert();
        icon_height.max(text_height) + self.padding.vert()
    }
}

/// 表示開始アニメーションのフレーム数(`SHOW_ANIMATION_COUNT`)。
pub const SHOW_ANIMATION_COUNT: i32 = 4;
/// 表示開始アニメーションのフレーム間隔(ms)(`SHOW_ANIMATION_INTERVAL`)。
pub const SHOW_ANIMATION_INTERVAL: u32 = 50;
/// フェードアウトアニメーションのフレーム数(`FADE_ANIMATION_COUNT`)。
pub const FADE_ANIMATION_COUNT: i32 = 4;
/// フェードアウトアニメーションのフレーム間隔(ms)(`FADE_ANIMATION_INTERVAL`)。
pub const FADE_ANIMATION_INTERVAL: u32 = 50;

/// アニメーション中のバー下端を算出する(NotificationBar.cpp 218-222 `GetAnimatedBarPosition`)。
///
/// `bottom = (frame + 1) * bar_height / num_frames`。`frame` は 0 起点。
pub fn animated_bar_bottom(frame: i32, num_frames: i32, bar_height: i32) -> i32 {
    (frame + 1) * bar_height / num_frames
}

/// 親クライアント領域からバーの配置矩形を求める(NotificationBar.cpp 211-215 `GetBarPosition`)。
///
/// 原実装は親のクライアント矩形(left/top/right)をそのまま使い、`bottom` のみ `bar_height` に
/// する(バーは上端に貼り付き、高さ `bar_height`)。
pub fn bar_position(parent_client: Rect, bar_height: i32) -> Rect {
    Rect {
        left: parent_client.left,
        top: parent_client.top,
        right: parent_client.right,
        bottom: bar_height,
    }
}

/// 通知バーに表示する 1 件のメッセージ(NotificationBar.h 71-77 `MessageInfo`)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessageInfo {
    /// 表示文字列(UTF-16。原実装の `String`)。
    pub text: Vec<u16>,
    /// 種別。
    pub message_type: MessageType,
    /// 表示時間(ms)。`0` は自動非表示なし。
    pub timeout: u32,
    /// 後続メッセージで差し替え可能か(`fSkippable`)。
    pub skippable: bool,
}

impl MessageInfo {
    /// メッセージを生成する。
    pub fn new(text: &[u16], message_type: MessageType, timeout: u32, skippable: bool) -> Self {
        Self {
            text: text.to_vec(),
            message_type,
            timeout,
            skippable,
        }
    }
}

/// 通知バーのメッセージキューと表示状態(NotificationBar の `m_MessageQueue`/可視状態)。
///
/// `CNotificationBar::Show`/`Hide` および `WM_TIMER`(`TIMER_ID_HIDE`) のうち、
/// ウィンドウ・タイマー・描画から切り離せるキュー操作と可視状態遷移を表現する。
#[derive(Clone, Debug, Default)]
pub struct MessageQueue {
    queue: VecDeque<MessageInfo>,
    visible: bool,
}

impl MessageQueue {
    /// 空のキューを生成する(非表示状態)。
    pub fn new() -> Self {
        Self::default()
    }

    /// バーが表示中か(`GetVisible` 相当)。
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// キュー内のメッセージ件数。
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// キューが空か。
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// 先頭(表示中)メッセージを返す(`m_MessageQueue.front()`)。空なら `None`。
    pub fn front(&self) -> Option<&MessageInfo> {
        self.queue.front()
    }

    /// 先頭メッセージの表示時間を返す(`SetHideTimer` が参照する `front().Timeout`)。
    ///
    /// 空なら `None`、`Some(0)` は「自動非表示しない」を表す。
    pub fn front_timeout(&self) -> Option<u32> {
        self.queue.front().map(|m| m.timeout)
    }

    /// メッセージを表示する(NotificationBar.cpp 113-163 `Show` のキュー操作部分)。
    ///
    /// - 非表示中だった場合: 追加後、先頭以外の溜まっていたメッセージを全て捨てて新規 1 件のみ
    ///   残し、表示状態にする(`while size>1: pop_front`)。
    /// - 表示中だった場合: 追加前の最後のメッセージがスキップ可能なら、それを取り除いて新規で
    ///   差し替える(`itr = begin + (size-2); if itr->fSkippable: erase(itr)`)。
    pub fn show(&mut self, info: MessageInfo) {
        self.queue.push_back(info);

        if !self.visible {
            while self.queue.len() > 1 {
                self.queue.pop_front();
            }
            self.visible = true;
        } else if self.queue.len() > 1 {
            let index = self.queue.len() - 2;
            if self.queue[index].skippable {
                self.queue.remove(index);
            }
        }
    }

    /// バーを隠す(NotificationBar.cpp 166-188 `Hide` の状態部分)。
    ///
    /// キューを空にして非表示にする(アニメーション・タイマーは対象外)。
    pub fn hide(&mut self) {
        self.visible = false;
        self.queue.clear();
    }

    /// 自動非表示タイマー満了時の前進(NotificationBar.cpp 330-339 `TIMER_ID_HIDE`)。
    ///
    /// 先頭メッセージを取り除き、キューが空になったら非表示にする。次のメッセージが残っていれば
    /// 表示は継続する。「次に設定すべき表示時間」を [`front_timeout`](Self::front_timeout) で
    /// 取得できる。
    pub fn advance(&mut self) {
        if !self.queue.is_empty() {
            self.queue.pop_front();
        }
        if self.queue.is_empty() {
            self.visible = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn message_type_index() {
        assert_eq!(MessageType::Info.index(), 0);
        assert_eq!(MessageType::Warning.index(), 1);
        assert_eq!(MessageType::Error.index(), 2);
    }

    #[test]
    fn bar_height_icon_dominant() {
        // アイコン(16 + 余白0) が テキスト(font 10 + 余白0) より大きい場合。
        let style = NotificationBarStyle::new();
        // icon_height = 16 + 0 = 16, text_height = 10 + 0 = 10, max=16, +padding.vert(2+2=4) = 20。
        assert_eq!(style.calc_bar_height(10), 20);
    }

    #[test]
    fn bar_height_text_dominant() {
        // テキスト高が大きい場合。
        let style = NotificationBarStyle::new();
        // icon_height = 16, text_height = 30, max=30, +4 = 34。
        assert_eq!(style.calc_bar_height(30), 34);
    }

    #[test]
    fn bar_height_uses_margins() {
        let mut style = NotificationBarStyle::new();
        style.icon_margin = Margins::new(0, 3, 4, 3); // vert = 6
        style.text_margin = Margins::new(0, 1, 0, 1); // vert = 2
        // icon_height = 16 + 6 = 22, text_height = font + 2。
        // font=18 → text=20, max=22, +padding.vert(4) = 26。
        assert_eq!(style.calc_bar_height(18), 26);
        // font=30 → text=32, max=32, +4 = 36。
        assert_eq!(style.calc_bar_height(30), 36);
    }

    #[test]
    fn animated_bottom_progression() {
        // 4 フレーム、バー高 40。frame=0..3 で 10,20,30,40。
        assert_eq!(animated_bar_bottom(0, 4, 40), 10);
        assert_eq!(animated_bar_bottom(1, 4, 40), 20);
        assert_eq!(animated_bar_bottom(2, 4, 40), 30);
        assert_eq!(animated_bar_bottom(3, 4, 40), 40);
    }

    #[test]
    fn bar_position_overrides_bottom_only() {
        let parent = Rect::new(0, 0, 800, 600);
        let rc = bar_position(parent, 24);
        assert_eq!(rc, Rect::new(0, 0, 800, 24));
    }

    #[test]
    fn show_when_hidden_keeps_only_latest() {
        let mut q = MessageQueue::new();
        assert!(!q.is_visible());
        // 非表示中に複数件を投入(各 show が 1 件ずつ追加)。
        q.show(MessageInfo::new(&text("A"), MessageType::Info, 1000, true));
        assert!(q.is_visible());
        assert_eq!(q.len(), 1);
        // 既に表示中になるため、以降は表示中ロジックに入る。直接複数溜まった状態を再現するため
        // 一旦 hide してから push を模す代わりに、非表示で複数追加されるケースを検証する。
        let mut q2 = MessageQueue::new();
        // 非表示状態のまま内部キューに複数積む状況: show を連続呼びすると 2 回目は visible 中。
        // C++ の「!visible で size>1」は、前回の Hide 中に積まれた残骸を想定しているため、
        // ここでは hide 後に show して 1 件に保たれることを確認する。
        q2.show(MessageInfo::new(&text("X"), MessageType::Info, 0, false));
        q2.hide();
        assert!(!q2.is_visible());
        assert!(q2.is_empty());
    }

    #[test]
    fn show_while_visible_replaces_skippable() {
        let mut q = MessageQueue::new();
        // 1 件目(timeout=0 で自動非表示なし)を表示。
        q.show(MessageInfo::new(&text("first"), MessageType::Info, 0, false));
        // 2 件目はスキップ可能。
        q.show(MessageInfo::new(&text("second"), MessageType::Warning, 1000, true));
        assert_eq!(q.len(), 2);
        // 3 件目を追加 → 追加前の最後(=second, skippable)が消えて差し替わる。
        q.show(MessageInfo::new(&text("third"), MessageType::Error, 1000, true));
        assert_eq!(q.len(), 2);
        assert_eq!(q.front().unwrap().text, text("first"));
        // 2 番目は third(second は差し替えで消えた)。
        q.advance(); // first を消す
        assert_eq!(q.front().unwrap().text, text("third"));
    }

    #[test]
    fn show_while_visible_keeps_non_skippable() {
        let mut q = MessageQueue::new();
        q.show(MessageInfo::new(&text("first"), MessageType::Info, 0, false));
        // スキップ不可のメッセージ。
        q.show(MessageInfo::new(&text("keep"), MessageType::Error, 1000, false));
        assert_eq!(q.len(), 2);
        // 3 件目 → 追加前の最後(keep)はスキップ不可なので消えず、3 件になる。
        q.show(MessageInfo::new(&text("third"), MessageType::Error, 1000, true));
        assert_eq!(q.len(), 3);
    }

    #[test]
    fn advance_pops_and_hides_when_empty() {
        let mut q = MessageQueue::new();
        q.show(MessageInfo::new(&text("only"), MessageType::Info, 1000, false));
        assert!(q.is_visible());
        // 満了で前進 → 空になり非表示。
        q.advance();
        assert!(q.is_empty());
        assert!(!q.is_visible());
    }

    #[test]
    fn advance_keeps_visible_with_remaining() {
        let mut q = MessageQueue::new();
        q.show(MessageInfo::new(&text("first"), MessageType::Info, 1000, false));
        q.show(MessageInfo::new(&text("second"), MessageType::Warning, 2000, false));
        assert_eq!(q.len(), 2);
        // 1 件目満了 → 2 件目が残り表示継続。
        q.advance();
        assert!(q.is_visible());
        assert_eq!(q.front().unwrap().text, text("second"));
        assert_eq!(q.front_timeout(), Some(2000));
    }

    #[test]
    fn front_timeout_reports_none_when_empty() {
        let q = MessageQueue::new();
        assert_eq!(q.front_timeout(), None);
    }

    #[test]
    fn hide_clears_queue() {
        let mut q = MessageQueue::new();
        q.show(MessageInfo::new(&text("a"), MessageType::Info, 1000, false));
        q.show(MessageInfo::new(&text("b"), MessageType::Warning, 1000, false));
        q.hide();
        assert!(q.is_empty());
        assert!(!q.is_visible());
        assert_eq!(q.front_timeout(), None);
    }
}
