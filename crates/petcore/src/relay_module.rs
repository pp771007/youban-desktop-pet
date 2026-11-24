//! 傳話模組：AI 或其他程式用 `Youban.exe --confirm/--notice` 送來的話，跳確認框或提示框（用詞見 `CONTEXT.md`）。
//!
//! 好幾個來源可能同時送、也可能一直送，所以傳話在核心的卡片佇列裡**最多只佔兩格**：
//! 一格確認框（其他的在這裡排隊，按掉一張才送下一張）、一格提示框（還沒收掉之前來的都併進去，
//! 兩則以上就變成提示清單，照來源排在一起）。
//! 核心的佇列滿了會丟掉最前面的卡，不這樣限制的話，一直送的程式會把使用者自己排的提醒擠掉。
//!
//! 確認框（排隊中、跳著的、按了稍後的）存在 `relay-state.json`，重開柚伴還會再跳；提示框不存，重開就算了。
//! 每一則另外記進傳話紀錄（`petkit::relay_log`），這個模組是唯一寫它的地方。

use std::path::PathBuf;

use chrono::{Duration, NaiveDateTime};
use petkit::config::{load_json, write_atomic};
use petkit::ipc::{CMD_RELAY, CMD_RELAY_DONE, CMD_RELAY_LOG_CLEAR, CMD_RELAY_LOG_SEEN, Relay};
use petkit::relay_log::{LogEntry, RelayLog};
use petkit::reminders::Style;
use serde::{Deserialize, Serialize};

use crate::bubble::{Card, CardAction, LIST_MAX_ROWS, card_time};
use crate::icons::Icon;
use crate::log;
use crate::modules::{Ctx, Module, ModuleEvent, Request};

pub const ID: &str = "relay";
/// 提示框在核心佇列裡那一格的卡片代號（確認框用每則自己的代號，按鈕才知道按的是哪一則）。
const NOTICE_KEY: &str = "notice";
/// 沒帶 `id` 的傳話，卡片代號是這個前綴 + 流水號（帶 `id` 的是 JSON 陣列，不會撞到）。
const SEQ_KEY_PREFIX: &str = "#";
const ICON: Icon = Icon::MessageDots;
/// 卡片上來源跟時間中間的分隔。
const FROM_SEPARATOR: &str = " · ";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Message {
    key: String,
    title: String,
    from: Option<String>,
    at: NaiveDateTime,
}

impl Message {
    fn time_line(&self, now: NaiveDateTime) -> String {
        let time = card_time(self.at, now.date());
        match &self.from {
            Some(from) => format!("{from}{FROM_SEPARATOR}{time}"),
            None => time,
        }
    }

    fn confirm_card(&self, now: NaiveDateTime) -> Card {
        Card::Confirm {
            title: self.title.clone(),
            time: self.time_line(now),
            snooze_open: false,
            icon: ICON,
        }
    }
}

/// 提示框那一格：一則是提示框，兩則以上是提示清單（同一個來源的排在一起，來源照第一則進來的先後）。
fn notice_slot_card(slot: &[Message], now: NaiveDateTime) -> Option<Card> {
    match slot {
        [] => None,
        [m] => Some(Card::Notice {
            title: m.title.clone(),
            time: m.time_line(now),
            icon: ICON,
        }),
        _ => {
            let mut sources: Vec<&Option<String>> = Vec::new();
            for m in slot {
                if !sources.contains(&&m.from) {
                    sources.push(&m.from);
                }
            }
            let mut rows: Vec<(String, String)> = sources
                .into_iter()
                .flat_map(|from| slot.iter().filter(move |m| &m.from == from))
                .map(|m| (m.title.clone(), m.time_line(now)))
                .collect();
            let more = rows.len().saturating_sub(LIST_MAX_ROWS);
            rows.truncate(LIST_MAX_ROWS);
            Some(Card::NoticeList { rows, more })
        }
    }
}

/// 只存確認框：跳著的那則存檔時放回排隊的最前面（核心有沒有存到那張卡不一定，這裡自己負責再跳）。
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct State {
    waiting: Vec<Message>,
    snoozed: Vec<(Message, NaiveDateTime)>,
    next_seq: u64,
}

pub struct RelayModule {
    state_path: PathBuf,
    log_path: PathBuf,
    log: RelayLog,
    waiting: Vec<Message>,
    shown: Option<Message>,
    snoozed: Vec<(Message, NaiveDateTime)>,
    /// 提示框那一格裡的全部（核心佇列裡有這一格 ⇔ 不是空的）。
    notice_slot: Vec<Message>,
    next_seq: u64,
}

impl RelayModule {
    pub fn new(state_path: PathBuf, log_path: PathBuf) -> Self {
        let state: State = load_json(&state_path).unwrap_or_else(|e| {
            log!("relay state unreadable ({e}); starting fresh");
            State::default()
        });
        let log = RelayLog::load(&log_path).unwrap_or_else(|e| {
            log!("relay log unreadable ({e}); starting fresh");
            RelayLog::default()
        });
        Self {
            state_path,
            log_path,
            log,
            waiting: state.waiting,
            shown: None,
            snoozed: state.snoozed,
            notice_slot: Vec::new(),
            next_seq: state.next_seq,
        }
    }

    fn receive(&mut self, relay: Relay, ctx: &mut Ctx) {
        if relay.title.trim().is_empty() {
            log!("relay without title ignored");
            return;
        }
        let key = match &relay.id {
            Some(id) => serde_json::to_string(&(&relay.from, id)).expect("strings serialize"),
            None => {
                self.next_seq += 1;
                format!("{SEQ_KEY_PREFIX}{}", self.next_seq)
            }
        };
        self.log.record(LogEntry {
            key: key.clone(),
            title: relay.title.clone(),
            detail: relay.detail,
            from: relay.from.clone(),
            at: ctx.now,
            style: relay.style,
            done: false,
        });
        self.save_log();
        let msg = Message {
            key,
            title: relay.title,
            from: relay.from,
            at: ctx.now,
        };
        self.waiting.retain(|m| m.key != msg.key);
        self.snoozed.retain(|(m, _)| m.key != msg.key);
        let shown_here = self.shown.as_ref().is_some_and(|m| m.key == msg.key);
        let in_slot = self.notice_slot.iter().position(|m| m.key == msg.key);
        match relay.style {
            Style::Confirm => {
                if let Some(i) = in_slot {
                    self.notice_slot.remove(i);
                    self.refresh_notice_slot(ctx);
                }
                if shown_here {
                    ctx.card(msg.key.clone(), msg.confirm_card(ctx.now));
                    self.shown = Some(msg);
                } else {
                    self.waiting.push(msg);
                }
            }
            Style::Notice => {
                if shown_here {
                    self.shown = None;
                    ctx.out.push(Request::Withdraw {
                        key: msg.key.clone(),
                    });
                }
                match in_slot {
                    Some(i) => self.notice_slot[i] = msg,
                    None => self.notice_slot.push(msg),
                }
                self.refresh_notice_slot(ctx);
            }
        }
        self.pump(ctx);
        self.save();
    }

    /// 確認框那一格空出來了就補上排最前面的那則。
    fn pump(&mut self, ctx: &mut Ctx) {
        if self.shown.is_none() && !self.waiting.is_empty() {
            let msg = self.waiting.remove(0);
            ctx.card(msg.key.clone(), msg.confirm_card(ctx.now));
            self.shown = Some(msg);
        }
    }

    /// 提示框那一格的內容變了：重送（核心原地換掉，等於重新跳一次），空了就收掉。
    fn refresh_notice_slot(&self, ctx: &mut Ctx) {
        match notice_slot_card(&self.notice_slot, ctx.now) {
            Some(card) => ctx.card(NOTICE_KEY, card),
            None => ctx.out.push(Request::Withdraw {
                key: NOTICE_KEY.into(),
            }),
        }
    }

    /// 傳話紀錄上按了「完成」：跳著的就收掉，還在排隊或稍後的就不用再跳了。
    fn done(&mut self, key: &str, ctx: &mut Ctx) {
        if self.shown.as_ref().is_some_and(|m| m.key == key) {
            self.shown = None;
            ctx.out.push(Request::Withdraw { key: key.into() });
        }
        self.waiting.retain(|m| m.key != key);
        self.snoozed.retain(|(m, _)| m.key != key);
        self.log.mark_done(key);
        self.save_log();
        self.pump(ctx);
        self.save();
    }

    fn save_log(&self) {
        if let Err(e) = self.log.save(&self.log_path) {
            log!("save relay log failed: {e}");
        }
    }

    fn save(&self) {
        let state = State {
            waiting: self.shown.iter().chain(&self.waiting).cloned().collect(),
            snoozed: self.snoozed.clone(),
            next_seq: self.next_seq,
        };
        match serde_json::to_vec_pretty(&state) {
            Ok(bytes) => {
                if let Err(e) = write_atomic(&self.state_path, &bytes) {
                    log!("save relay state failed: {e}");
                }
            }
            Err(e) => log!("serialize relay state failed: {e}"),
        }
    }
}

impl Module for RelayModule {
    fn id(&self) -> &'static str {
        ID
    }

    fn name(&self) -> &'static str {
        "傳話"
    }

    fn tick(&mut self, ctx: &mut Ctx) {
        let any_due = self.snoozed.iter().any(|(_, until)| *until <= ctx.now);
        if any_due {
            let (mut due, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.snoozed)
                .into_iter()
                .partition(|(_, until)| *until <= ctx.now);
            self.snoozed = rest;
            due.sort_by_key(|(_, until)| *until);
            for (i, (m, _)) in due.into_iter().enumerate() {
                self.waiting.insert(i, m);
            }
        }
        // 重開後排隊裡的確認框（核心沒存到跳著的那張時）也是在這裡補上。
        let idle = self.shown.is_none() && !self.waiting.is_empty();
        if any_due || idle {
            self.pump(ctx);
            self.save();
        }
    }

    fn on_event(&mut self, event: &ModuleEvent, ctx: &mut Ctx) {
        match event {
            ModuleEvent::Command(v) if v["cmd"].as_str() == Some(CMD_RELAY) => {
                match serde_json::from_value::<Relay>((*v).clone()) {
                    Ok(relay) => self.receive(relay, ctx),
                    Err(e) => log!("bad relay ({e})"),
                }
            }
            ModuleEvent::Command(v) => match v["cmd"].as_str() {
                Some(CMD_RELAY_DONE) => {
                    if let Some(key) = v["key"].as_str() {
                        self.done(key, ctx);
                    }
                }
                Some(CMD_RELAY_LOG_SEEN) => {
                    self.log.seen_until = Some(ctx.now);
                    self.save_log();
                }
                Some(CMD_RELAY_LOG_CLEAR) => {
                    let from = v.get("from").map(|f| f.as_str());
                    self.log.clear(from);
                    self.save_log();
                }
                _ => {}
            },
            ModuleEvent::Card { key, action } => {
                if *key == NOTICE_KEY {
                    self.notice_slot.clear();
                } else if self.shown.as_ref().is_some_and(|m| m.key == *key) {
                    match action {
                        CardAction::OpenSnooze => return,
                        CardAction::Snooze(minutes) => {
                            let until = ctx.now + Duration::minutes(i64::from(*minutes));
                            self.snoozed.extend(self.shown.take().map(|m| (m, until)));
                        }
                        CardAction::Done | CardAction::Dismiss => {
                            self.shown = None;
                            self.log.mark_done(key);
                            self.save_log();
                        }
                    }
                }
                self.pump(ctx);
                self.save();
            }
            _ => {}
        }
    }

    /// 重開時核心還回來的確認框：還在排隊就讓它當跳著的那則；換日時跳著的那則照今天重畫（時間加上日期）。
    /// 核心沒存到的，第一次 tick 會從排隊裡自己補上，所以這裡說不要也不會漏。
    fn restore_card(&mut self, key: &str, _card: Card, now: NaiveDateTime) -> Option<Card> {
        if self.shown.as_ref().is_some_and(|m| m.key == key) {
            return self.shown.as_ref().map(|m| m.confirm_card(now));
        }
        if self.shown.is_some() {
            return None;
        }
        let i = self.waiting.iter().position(|m| m.key == key)?;
        let msg = self.waiting.remove(i);
        let card = msg.confirm_card(now);
        self.shown = Some(msg);
        Some(card)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use petkit::config::Config;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn dt(d: u32, h: u32, m: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, d)
            .unwrap()
            .and_hms_opt(h, m, 0)
            .unwrap()
    }

    fn state_path() -> PathBuf {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "petcore-relay-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("relay-state.json")
    }

    fn open(state: &std::path::Path) -> RelayModule {
        RelayModule::new(state.to_path_buf(), state.with_file_name("relay-log.json"))
    }

    #[test]
    fn done_in_the_log_closes_the_card_and_shows_the_next() {
        let path = state_path();
        let mut m = open(&path);
        send(
            &mut m,
            dt(24, 9, 0),
            Style::Confirm,
            "回信",
            Some("Claude"),
            None,
        );
        send(
            &mut m,
            dt(24, 9, 0),
            Style::Confirm,
            "審 PR",
            Some("Claude"),
            None,
        );
        let v = serde_json::json!({ "cmd": CMD_RELAY_DONE, "key": "#1" });
        let out = run(&mut m, dt(24, 9, 5), |m, ctx| {
            m.on_event(&ModuleEvent::Command(&v), ctx)
        });
        assert!(out.contains(&Request::Withdraw { key: "#1".into() }));
        assert_eq!(titles(&out), ["審 PR"]);
        let log = RelayLog::load(&path.with_file_name("relay-log.json")).unwrap();
        let done: Vec<bool> = log.entries.iter().map(|e| e.done).collect();
        assert_eq!(done, [true, false]);
    }

    fn run(
        m: &mut RelayModule,
        now: NaiveDateTime,
        f: impl FnOnce(&mut RelayModule, &mut Ctx),
    ) -> Vec<Request> {
        let mut out = Vec::new();
        f(
            m,
            &mut Ctx {
                now,
                config: &Config::default(),
                out: &mut out,
            },
        );
        out
    }

    fn send(
        m: &mut RelayModule,
        now: NaiveDateTime,
        style: Style,
        title: &str,
        from: Option<&str>,
        id: Option<&str>,
    ) -> Vec<Request> {
        let mut v = serde_json::to_value(Relay {
            style,
            title: title.into(),
            from: from.map(Into::into),
            id: id.map(Into::into),
            detail: None,
        })
        .unwrap();
        v["cmd"] = CMD_RELAY.into();
        run(m, now, |m, ctx| m.on_event(&ModuleEvent::Command(&v), ctx))
    }

    fn press(
        m: &mut RelayModule,
        now: NaiveDateTime,
        key: &str,
        action: CardAction,
    ) -> Vec<Request> {
        run(m, now, |m, ctx| {
            m.on_event(&ModuleEvent::Card { key, action }, ctx)
        })
    }

    fn cards(reqs: &[Request]) -> Vec<(String, Card)> {
        reqs.iter()
            .filter_map(|r| match r {
                Request::Card { key, card } => Some((key.clone(), card.clone())),
                _ => None,
            })
            .collect()
    }

    fn titles(reqs: &[Request]) -> Vec<String> {
        cards(reqs)
            .into_iter()
            .map(|(_, c)| match c {
                Card::Confirm { title, .. } | Card::Notice { title, .. } => title,
                other => panic!("unexpected card {other:?}"),
            })
            .collect()
    }

    #[test]
    fn shows_the_sender_and_time_on_the_card() {
        let mut m = open(&state_path());
        let out = send(
            &mut m,
            dt(24, 14, 2),
            Style::Confirm,
            "建置失敗",
            Some("Claude"),
            None,
        );
        assert_eq!(
            cards(&out),
            vec![(
                "#1".into(),
                Card::Confirm {
                    title: "建置失敗".into(),
                    time: "Claude · 14:02".into(),
                    snooze_open: false,
                    icon: ICON,
                }
            )]
        );
    }

    #[test]
    fn many_senders_take_at_most_one_confirm_and_one_notice_slot() {
        let mut m = open(&state_path());
        let mut shown = Vec::new();
        for i in 0..50 {
            let from = format!("AI{i}");
            shown.extend(send(
                &mut m,
                dt(24, 9, 0),
                Style::Confirm,
                "要看",
                Some(&from),
                None,
            ));
            shown.extend(send(
                &mut m,
                dt(24, 9, 0),
                Style::Notice,
                "好了",
                Some(&from),
                None,
            ));
        }
        let mut keys: Vec<String> = cards(&shown).into_iter().map(|(k, _)| k).collect();
        keys.dedup();
        assert_eq!(keys, ["#1", NOTICE_KEY]);
        // 提示框那一格一直原地更新成提示清單：列 4 則，其餘「還有 N 則」。
        let Some((_, Card::NoticeList { rows, more })) = cards(&shown).pop() else {
            panic!("expected a notice list");
        };
        assert_eq!((rows.len(), more), (LIST_MAX_ROWS, 50 - LIST_MAX_ROWS));
        // 確認框按掉一張才補下一張。
        let next = press(&mut m, dt(24, 9, 1), "#1", CardAction::Done);
        assert_eq!(
            cards(&next)[0].1,
            Card::Confirm {
                title: "要看".into(),
                time: "AI1 · 09:00".into(),
                snooze_open: false,
                icon: ICON,
            }
        );
        // 提示清單收掉就全部看過了，不會再跳。
        let next = press(&mut m, dt(24, 9, 1), NOTICE_KEY, CardAction::Dismiss);
        assert!(cards(&next).is_empty());
    }

    #[test]
    fn notices_from_the_same_sender_are_listed_together() {
        let mut m = open(&state_path());
        send(
            &mut m,
            dt(24, 9, 0),
            Style::Notice,
            "建置完成",
            Some("Claude"),
            None,
        );
        send(
            &mut m,
            dt(24, 9, 1),
            Style::Notice,
            "部署好了",
            Some("Codex"),
            None,
        );
        let out = send(
            &mut m,
            dt(24, 9, 2),
            Style::Notice,
            "測試通過",
            Some("Claude"),
            None,
        );
        assert_eq!(
            cards(&out),
            vec![(
                NOTICE_KEY.into(),
                Card::NoticeList {
                    rows: vec![
                        ("建置完成".into(), "Claude · 09:00".into()),
                        ("測試通過".into(), "Claude · 09:02".into()),
                        ("部署好了".into(), "Codex · 09:01".into()),
                    ],
                    more: 0,
                }
            )]
        );
    }

    #[test]
    fn same_sender_and_id_updates_instead_of_piling_up() {
        let mut m = open(&state_path());
        send(
            &mut m,
            dt(24, 9, 0),
            Style::Confirm,
            "佔住確認框",
            None,
            None,
        );
        send(
            &mut m,
            dt(24, 9, 0),
            Style::Confirm,
            "建置中",
            Some("Claude"),
            Some("build"),
        );
        send(
            &mut m,
            dt(24, 9, 1),
            Style::Confirm,
            "建置失敗",
            Some("Claude"),
            Some("build"),
        );
        // 別的來源用同一個 id 是另一則。
        send(
            &mut m,
            dt(24, 9, 2),
            Style::Confirm,
            "部署好了",
            Some("Codex"),
            Some("build"),
        );
        let a = press(&mut m, dt(24, 9, 3), "#1", CardAction::Done);
        let key = cards(&a)[0].0.clone();
        assert_eq!(titles(&a), ["建置失敗"]);
        assert_eq!(
            titles(&press(&mut m, dt(24, 9, 3), &key, CardAction::Done)),
            ["部署好了"]
        );
    }

    #[test]
    fn updating_the_shown_card_replaces_it_in_place() {
        let mut m = open(&state_path());
        send(
            &mut m,
            dt(24, 9, 0),
            Style::Notice,
            "建置中",
            Some("Claude"),
            Some("build"),
        );
        let out = send(
            &mut m,
            dt(24, 9, 1),
            Style::Notice,
            "建置完成",
            Some("Claude"),
            Some("build"),
        );
        assert_eq!(cards(&out)[0].0, NOTICE_KEY);
        assert_eq!(titles(&out), ["建置完成"]);
        // 從提示框改成確認框：提示框收掉，改跳確認框。
        let out = send(
            &mut m,
            dt(24, 9, 2),
            Style::Confirm,
            "建置失敗",
            Some("Claude"),
            Some("build"),
        );
        assert!(out.contains(&Request::Withdraw {
            key: NOTICE_KEY.into()
        }));
        assert_eq!(titles(&out), ["建置失敗"]);
    }

    #[test]
    fn snoozed_confirm_comes_back_later() {
        let mut m = open(&state_path());
        send(&mut m, dt(24, 9, 0), Style::Confirm, "回信", None, None);
        assert!(cards(&press(&mut m, dt(24, 9, 1), "#1", CardAction::Snooze(5))).is_empty());
        assert!(cards(&run(&mut m, dt(24, 9, 5), |m, ctx| m.tick(ctx))).is_empty());
        assert_eq!(
            titles(&run(&mut m, dt(24, 9, 6), |m, ctx| m.tick(ctx))),
            ["回信"]
        );
    }

    #[test]
    fn confirms_survive_a_restart_but_notices_do_not() {
        let path = state_path();
        let mut m = open(&path);
        send(&mut m, dt(24, 9, 0), Style::Confirm, "跳著的", None, None);
        send(&mut m, dt(24, 9, 0), Style::Confirm, "排隊的", None, None);
        send(&mut m, dt(24, 9, 0), Style::Notice, "提示", None, None);
        send(
            &mut m,
            dt(24, 9, 0),
            Style::Confirm,
            "稍後的",
            Some("Claude"),
            Some("x"),
        );
        press(&mut m, dt(24, 9, 1), "#1", CardAction::Done);
        let snoozed_key = serde_json::to_string(&(Some("Claude"), "x")).unwrap();
        // 「排隊的」現在跳著；按「稍後的」之前先讓它輪到。
        press(&mut m, dt(24, 9, 1), "#2", CardAction::Done);
        press(&mut m, dt(24, 9, 1), &snoozed_key, CardAction::Snooze(60));
        send(&mut m, dt(24, 9, 2), Style::Confirm, "跳著的2", None, None);
        send(&mut m, dt(24, 9, 2), Style::Confirm, "排隊的2", None, None);

        // 關掉再開：核心還回跳著的那張，排隊的、稍後的也都還在；提示框不會再跳。
        let mut m = open(&path);
        let card = Card::Confirm {
            title: String::new(),
            time: String::new(),
            snooze_open: false,
            icon: ICON,
        };
        let back = m.restore_card("#4", card, dt(25, 8, 0)).unwrap();
        assert!(
            matches!(back, Card::Confirm { title, time, .. } if title == "跳著的2" && time == "11/24 09:02")
        );
        // 稍後的時間已經過了：排到最前面，等跳著的那張按掉。
        assert!(cards(&run(&mut m, dt(25, 8, 0), |m, ctx| m.tick(ctx))).is_empty());
        assert_eq!(
            titles(&press(&mut m, dt(25, 8, 1), "#4", CardAction::Done)),
            ["稍後的"]
        );
        let next = press(&mut m, dt(25, 8, 1), &snoozed_key, CardAction::Done);
        assert_eq!(titles(&next), ["排隊的2"]);
    }

    #[test]
    fn a_shown_confirm_the_core_did_not_save_still_comes_back() {
        let path = state_path();
        let mut m = open(&path);
        send(&mut m, dt(24, 9, 0), Style::Confirm, "回信", None, None);
        let mut m = open(&path);
        let first = run(&mut m, dt(24, 10, 0), |m, ctx| m.tick(ctx));
        assert_eq!(titles(&first), ["回信"]);
    }
}
