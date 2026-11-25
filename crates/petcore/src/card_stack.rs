//! 頭上的框排在哪裡（用詞見 `CONTEXT.md`）。分兩區，從桌寵頭頂往上長：
//!
//! - **下面那區（貼著桌寵）**：要按的框（確認框、錯過的提醒、新版本）。最前面那個完整顯示，其他的疊在後面。
//!   使用者自己設的提醒排在傳話前面，同一層裡舊的在前面；只有使用者按了才會換下一個。
//! - **上面那區**：會自己消失的框（提示框、一般通知）。一次一個，照來的順序；不用等下面那區按掉。
//!
//! 新東西都長在上面，下面那個框的位置不會動，使用者正要按的按鈕就不會跑掉。
//! 只管排序與倒數，不管畫（畫在 `bubble`）；拆出來是為了不開視窗也能測。

use crate::bubble::Card;
use crate::relay_module;

/// 排隊中的框上限（`docs/PLAN.md`：沒有無上限的集合）。滿了先丟最舊的會自己消失的框。
/// 要按的框不靠這裡保存（各功能自己存著），傳話一次也最多放 `relay_module::MAX_SHOWN` 個進來，正常用不會撞到。
const MAX_CARDS: usize = 32;
/// 一般通知留多久。
const INFO_SECS: f64 = 5.0;
/// 提示框留多久：比一般通知久，人轉頭過來還看得到。只算人在、滑鼠沒停在框上的時間。
pub const NOTICE_SECS: f64 = 15.0;

/// 會自己消失的框留幾秒；`None` = 要使用者按掉。
pub fn linger_secs(card: &Card) -> Option<f64> {
    match card {
        Card::Info { .. } => Some(INFO_SECS),
        Card::Notice { .. } | Card::NoticeList { .. } => Some(NOTICE_SECS),
        _ => None,
    }
}

/// 下面那區的先後：使用者自己設的（提醒、休息、新版本）先，別的程式傳來的後。
fn tier(owner: &str) -> u8 {
    u8::from(owner == relay_module::ID)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    /// 進來的順序，也是這一則在畫面上的身分（換成另一則時按鈕要先擋一下）。
    pub seq: u64,
    pub owner: &'static str,
    pub key: String,
    pub card: Card,
}

#[derive(Default)]
pub struct CardStack {
    items: Vec<Item>,
    next_seq: u64,
    /// 使用者在攤開的清單點了哪一則：它當最前面，直到處理掉。
    picked: Option<u64>,
    /// 目前顯示在下面那區最前面的。
    front: Option<u64>,
    /// 目前顯示在上面那區的，與它已經倒數了多久。
    top: Option<u64>,
    top_elapsed: f64,
    /// 下面那區攤開成清單。
    pub expanded: bool,
}

impl CardStack {
    /// 放進一個框；同一個功能的同一個 `key` 原地換掉。回傳 `true` = 有新消息（新的一則、或內容變了），要響。
    pub fn push(&mut self, owner: &'static str, key: String, card: Card) -> bool {
        if let Some(i) = self.position(owner, &key) {
            let item = &mut self.items[i];
            if item.card == card {
                return false;
            }
            // 上面那區的框內容變了（提示清單多一則）：新的那則也要有完整的時間被看到。
            if self.top == Some(item.seq) {
                self.top_elapsed = 0.0;
            }
            item.card = card;
            return true;
        }
        if self.items.len() >= MAX_CARDS {
            let drop_at = self
                .items
                .iter()
                .position(|q| linger_secs(&q.card).is_some())
                .unwrap_or(0);
            let dropped = self.items.remove(drop_at);
            crate::log!("card queue full; dropped {}:{}", dropped.owner, dropped.key);
        }
        self.next_seq += 1;
        self.items.push(Item {
            seq: self.next_seq,
            owner,
            key,
            card,
        });
        true
    }

    pub fn withdraw(&mut self, owner: &str, key: &str) {
        self.items.retain(|q| !(q.owner == owner && q.key == key));
    }

    /// 功能被關掉：它留下的框全部收掉。
    pub fn remove_owner(&mut self, owner: &str) {
        self.items.retain(|q| q.owner != owner);
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Item> {
        self.items.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Item> {
        self.items.iter_mut()
    }

    /// 每一格呼叫一次，決定兩區各放哪一則。`hovered` = 滑鼠在框上：這時下面那區不換人（換了會按錯）。
    pub fn refresh(&mut self, hovered: bool) {
        if self.top.is_none_or(|seq| self.find(seq).is_none()) {
            self.top = self
                .items
                .iter()
                .find(|q| linger_secs(&q.card).is_some())
                .map(|q| q.seq);
            self.top_elapsed = 0.0;
        }
        if self.picked.is_some_and(|seq| self.find(seq).is_none()) {
            self.picked = None;
        }
        let keep = hovered && self.front.is_some_and(|seq| self.find(seq).is_some());
        if !keep {
            self.front = self
                .picked
                .or_else(|| self.pending().first().map(|q| q.seq));
        }
        if self.pending().len() <= 1 {
            self.expanded = false;
        }
    }

    /// 上面那區的倒數往前走 `dt` 秒（`frozen` = 滑鼠在框上或人不在，停住）。時間到就拿出來交給呼叫端收掉。
    pub fn tick(&mut self, dt: f64, frozen: bool) -> Option<Item> {
        let item = self.top()?;
        let (seq, linger) = (item.seq, linger_secs(&item.card)?);
        if !frozen {
            self.top_elapsed += dt;
        }
        if self.top_elapsed < linger {
            return None;
        }
        self.top = None;
        self.take(seq)
    }

    pub fn top(&self) -> Option<&Item> {
        self.top.and_then(|seq| self.find(seq))
    }

    /// 上面那區倒數到哪裡（0 = 剛出現，1 = 要收了）。
    pub fn top_progress(&self) -> f32 {
        self.top()
            .and_then(|q| linger_secs(&q.card))
            .map_or(0.0, |linger| {
                (self.top_elapsed / linger).clamp(0.0, 1.0) as f32
            })
    }

    pub fn front(&self) -> Option<&Item> {
        self.front.and_then(|seq| self.find(seq))
    }

    pub fn front_mut(&mut self) -> Option<&mut Item> {
        let seq = self.front?;
        self.items.iter_mut().find(|q| q.seq == seq)
    }

    /// 下面那區疊在最前面那則後面的，照先後。
    pub fn behind(&self) -> Vec<&Item> {
        self.pending()
            .into_iter()
            .filter(|q| Some(q.seq) != self.front)
            .collect()
    }

    /// 在攤開的清單點了某一則：讓它當最前面。
    pub fn pick(&mut self, seq: u64) {
        if self.find(seq).is_some() {
            self.picked = Some(seq);
            self.front = Some(seq);
        }
    }

    pub fn take(&mut self, seq: u64) -> Option<Item> {
        let i = self.items.iter().position(|q| q.seq == seq)?;
        Some(self.items.remove(i))
    }

    fn pending(&self) -> Vec<&Item> {
        let mut v: Vec<&Item> = self
            .items
            .iter()
            .filter(|q| linger_secs(&q.card).is_none())
            .collect();
        v.sort_by_key(|q| (tier(q.owner), q.seq));
        v
    }

    fn find(&self, seq: u64) -> Option<&Item> {
        self.items.iter().find(|q| q.seq == seq)
    }

    fn position(&self, owner: &str, key: &str) -> Option<usize> {
        self.items
            .iter()
            .position(|q| q.owner == owner && q.key == key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::icons::Icon;
    use crate::reminder_module;

    fn confirm(title: &str) -> Card {
        Card::Confirm {
            title: title.into(),
            time: "14:00".into(),
            snooze_open: false,
            icon: Icon::BellRing,
        }
    }

    fn notice(title: &str) -> Card {
        Card::Notice {
            title: title.into(),
            time: "14:00".into(),
            icon: Icon::MessageDots,
        }
    }

    fn title(item: Option<&Item>) -> Option<&str> {
        match &item?.card {
            Card::Confirm { title, .. } | Card::Notice { title, .. } => Some(title),
            _ => None,
        }
    }

    #[test]
    fn a_notice_shows_right_away_even_when_a_confirm_is_waiting() {
        let mut s = CardStack::default();
        s.push(relay_module::ID, "a".into(), confirm("審 PR"));
        s.push(relay_module::ID, "n".into(), notice("建置好了"));
        s.refresh(false);
        assert_eq!(title(s.front()), Some("審 PR"));
        assert_eq!(title(s.top()), Some("建置好了"));
    }

    #[test]
    fn countdown_stops_while_hovered_or_away() {
        let mut s = CardStack::default();
        s.push(relay_module::ID, "n".into(), notice("建置好了"));
        s.refresh(false);
        assert!(s.tick(NOTICE_SECS - 1.0, false).is_none());
        // 停住的時候再久都不會收掉。
        assert!(s.tick(1000.0, true).is_none());
        assert!(s.tick(0.5, false).is_none());
        assert_eq!(title(s.tick(0.5, false).as_ref()), Some("建置好了"));
        assert!(s.is_empty());
    }

    #[test]
    fn own_reminders_come_before_relayed_confirms_and_older_first() {
        let mut s = CardStack::default();
        s.push(relay_module::ID, "a".into(), confirm("AI 1"));
        s.push(relay_module::ID, "b".into(), confirm("AI 2"));
        s.push(reminder_module::ID, "r".into(), confirm("繳電話費"));
        s.refresh(false);
        assert_eq!(title(s.front()), Some("繳電話費"));
        let behind: Vec<_> = s.behind().into_iter().map(|q| title(Some(q))).collect();
        assert_eq!(behind, [Some("AI 1"), Some("AI 2")]);
    }

    #[test]
    fn the_front_card_does_not_change_under_the_mouse() {
        let mut s = CardStack::default();
        s.push(relay_module::ID, "a".into(), confirm("AI 1"));
        s.refresh(false);
        s.push(reminder_module::ID, "r".into(), confirm("繳電話費"));
        s.refresh(true);
        assert_eq!(title(s.front()), Some("AI 1"));
        s.refresh(false);
        assert_eq!(title(s.front()), Some("繳電話費"));
    }

    #[test]
    fn a_full_queue_drops_cards_that_would_vanish_anyway() {
        let mut s = CardStack::default();
        s.push(relay_module::ID, "n".into(), notice("提示"));
        for i in 0..MAX_CARDS {
            s.push(reminder_module::ID, i.to_string(), confirm("提醒"));
        }
        assert_eq!(s.len(), MAX_CARDS);
        assert!(s.iter().all(|q| linger_secs(&q.card).is_none()));
    }
}
