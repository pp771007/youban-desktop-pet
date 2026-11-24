//! 傳話模組：AI 或其他程式用 `Youban.exe --confirm/--notice` 送來的話，跳確認框或提示框（用詞見 `CONTEXT.md`）。
//!
//! 好幾個來源可能同時送、也可能一直送，所以傳話在核心的卡片佇列裡**最多只佔兩格**：
//! 一格確認框、一格提示框，其他的在這裡排隊，前一張收掉才送下一張。
//! 核心的佇列滿了會丟掉最前面的卡，不這樣限制的話，一直送的程式會把使用者自己排的提醒擠掉。
//!
//! 確認框（排隊中、跳著的、按了稍後的）存在 `relay-state.json`，重開柚伴還會再跳；提示框不存，重開就算了。

use std::path::PathBuf;

use chrono::{Duration, NaiveDateTime};
use petkit::config::{load_json, write_atomic};
use petkit::ipc::{CMD_RELAY, Relay};
use petkit::reminders::Style;
use serde::{Deserialize, Serialize};

use crate::bubble::{Card, CardAction, card_time};
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

    fn notice_card(&self, now: NaiveDateTime) -> Card {
        Card::Notice {
            title: self.title.clone(),
            time: self.time_line(now),
            icon: ICON,
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
    path: PathBuf,
    waiting: Vec<Message>,
    shown: Option<Message>,
    snoozed: Vec<(Message, NaiveDateTime)>,
    notices: Vec<Message>,
    notice_shown: Option<Message>,
    next_seq: u64,
}

impl RelayModule {
    pub fn new(path: PathBuf) -> Self {
        let state: State = load_json(&path).unwrap_or_else(|e| {
            log!("relay state unreadable ({e}); starting fresh");
            State::default()
        });
        Self {
            path,
            waiting: state.waiting,
            shown: None,
            snoozed: state.snoozed,
            notices: Vec::new(),
            notice_shown: None,
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
        let msg = Message {
            key,
            title: relay.title,
            from: relay.from,
            at: ctx.now,
        };
        self.waiting.retain(|m| m.key != msg.key);
        self.snoozed.retain(|(m, _)| m.key != msg.key);
        self.notices.retain(|m| m.key != msg.key);
        let shown_here = self.shown.as_ref().is_some_and(|m| m.key == msg.key);
        let notice_here = self.notice_shown.as_ref().is_some_and(|m| m.key == msg.key);
        match relay.style {
            Style::Confirm => {
                if notice_here {
                    self.notice_shown = None;
                    ctx.out.push(Request::Withdraw {
                        key: NOTICE_KEY.into(),
                    });
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
                if notice_here {
                    ctx.card(NOTICE_KEY, msg.notice_card(ctx.now));
                    self.notice_shown = Some(msg);
                } else {
                    self.notices.push(msg);
                }
            }
        }
        self.pump(ctx);
        self.save();
    }

    /// 空出來的格子補上排最前面的那則。
    fn pump(&mut self, ctx: &mut Ctx) {
        if self.shown.is_none() && !self.waiting.is_empty() {
            let msg = self.waiting.remove(0);
            ctx.card(msg.key.clone(), msg.confirm_card(ctx.now));
            self.shown = Some(msg);
        }
        if self.notice_shown.is_none() && !self.notices.is_empty() {
            let msg = self.notices.remove(0);
            ctx.card(NOTICE_KEY, msg.notice_card(ctx.now));
            self.notice_shown = Some(msg);
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
                if let Err(e) = write_atomic(&self.path, &bytes) {
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
            ModuleEvent::Card { key, action } => {
                if *key == NOTICE_KEY {
                    self.notice_shown = None;
                } else if self.shown.as_ref().is_some_and(|m| m.key == *key) {
                    match action {
                        CardAction::OpenSnooze => return,
                        CardAction::Snooze(minutes) => {
                            let until = ctx.now + Duration::minutes(i64::from(*minutes));
                            self.snoozed.extend(self.shown.take().map(|m| (m, until)));
                        }
                        CardAction::Done | CardAction::Dismiss => self.shown = None,
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
        let mut m = RelayModule::new(state_path());
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
        let mut m = RelayModule::new(state_path());
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
        assert_eq!(titles(&shown), ["要看", "好了"]);
        // 收掉一張才補下一張。
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
        let next = press(&mut m, dt(24, 9, 1), NOTICE_KEY, CardAction::Dismiss);
        assert_eq!(titles(&next), ["好了"]);
    }

    #[test]
    fn same_sender_and_id_updates_instead_of_piling_up() {
        let mut m = RelayModule::new(state_path());
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
        let mut m = RelayModule::new(state_path());
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
        let mut m = RelayModule::new(state_path());
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
        let mut m = RelayModule::new(path.clone());
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
        let mut m = RelayModule::new(path);
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
        let mut m = RelayModule::new(path.clone());
        send(&mut m, dt(24, 9, 0), Style::Confirm, "回信", None, None);
        let mut m = RelayModule::new(path);
        let first = run(&mut m, dt(24, 10, 0), |m, ctx| m.tick(ctx));
        assert_eq!(titles(&first), ["回信"]);
    }
}
