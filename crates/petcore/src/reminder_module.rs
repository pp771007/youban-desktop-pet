//! 提醒模組：讀 `reminders.json`（設定程式寫的），到點丟提醒卡，處理「稍後」與錯過的提醒。
//!
//! 時間一律用「下一次觸發的絕對時間」排程；電腦睡眠或關機醒來時，tick 之間會出現大空檔，
//! 那段時間該響的不一個一個轟出來，而是整理成一張「錯過的提醒」卡。
//!
//! 要按掉的卡（要按完成的提醒、錯過的提醒）按掉之前記在狀態檔裡：柚伴關掉再開（更新、當掉、手動重開）時重新跳出來，
//! 跟 Outlook、macOS 提醒事項一樣「沒處理的提醒不會因為重開就不見」。會自己消失的提醒不留。

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{Duration, NaiveDateTime};
use petkit::config::{load_json, write_atomic};
use petkit::holidays::{self, Calendar};
use petkit::reminders::{Reminder, ReminderFile, Style};
use serde::{Deserialize, Serialize};

use crate::bubble::{Card, CardAction, MISSED_MAX_ROWS};
use crate::log;
use crate::modules::{Ctx, Module, ModuleEvent};

/// tick 之間隔超過這麼久，就當作電腦睡過一覺（正常是每秒一次）。
const RESUME_GAP_SECS: i64 = 120;
const JUST_DUE_SECS: i64 = 60;
/// 多久存一次「看到哪個時間了」。
const STATE_SAVE_SECS: i64 = 60;
const MISSED_KEY: &str = "missed";
const TIME_FORMAT: &str = "%H:%M";
const DATE_TIME_FORMAT: &str = "%-m/%-d %H:%M";

pub struct Paths {
    pub reminders: PathBuf,
    pub state: PathBuf,
    /// 下載回來的官方辦公日曆（`<西元年>.csv`），比內建的新就用它。
    pub holiday_cache: PathBuf,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct State {
    last_checked: Option<NaiveDateTime>,
    snoozes: BTreeMap<String, NaiveDateTime>,
    /// 跳出來、還沒按掉的卡（卡片代號 → 內容）。
    pending: BTreeMap<String, Pending>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum Pending {
    /// 要按完成的提醒，`at` 是響的時間（標題重開時從提醒檔讀，改過名就顯示新的）。
    Reminder { at: NaiveDateTime },
    Missed { rows: Vec<(String, String)>, more: usize },
}

pub struct ReminderModule {
    paths: Paths,
    file: ReminderFile,
    file_stamp: Option<SystemTime>,
    calendar: Calendar,
    next: HashMap<String, NaiveDateTime>,
    state: State,
    last_tick: Option<NaiveDateTime>,
    last_save: Option<NaiveDateTime>,
}

fn stamp(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

impl ReminderModule {
    pub fn new(paths: Paths) -> Self {
        let state: State = load_json(&paths.state).unwrap_or_else(|e| {
            log!("reminder state unreadable ({e}); starting fresh");
            State::default()
        });
        let mut m = Self {
            file: ReminderFile::default(),
            file_stamp: None,
            calendar: Calendar::default(),
            next: HashMap::new(),
            state,
            last_tick: None,
            last_save: None,
            paths,
        };
        m.reload_file();
        m
    }

    fn reload_file(&mut self) {
        self.file_stamp = stamp(&self.paths.reminders);
        match ReminderFile::load(&self.paths.reminders) {
            Ok(f) => self.file = f,
            // 設定程式寫到一半被讀到不會發生（它也是先寫暫存檔再改名）；真的壞了就沿用上一份。
            Err(e) => log!("reminders.json unreadable: {e}"),
        }
        self.reload_calendar();
    }

    fn reload_calendar(&mut self) {
        let (cal, problems) = holidays::load_calendar(&self.paths.holiday_cache, self.file.day_overrides.clone());
        for p in problems {
            log!("holiday calendar unreadable: {p}");
        }
        self.calendar = cal;
    }

    fn reschedule(&mut self, from: NaiveDateTime) {
        self.next = self.file.reminders.iter().filter_map(|r| Some((r.id.clone(), r.next_after(from, &self.calendar)?))).collect();
        self.state.snoozes.retain(|id, _| self.file.reminders.iter().any(|r| &r.id == id));
        self.state.pending.retain(|id, _| id == MISSED_KEY || self.file.reminders.iter().any(|r| &r.id == id));
    }

    /// 時間到：照這則提醒的方式跳要按完成的卡（記進 `pending`），或會自己消失的卡。
    fn fire(&mut self, ctx: &mut Ctx, id: &str, at: NaiveDateTime) {
        let Some(r) = self.file.reminders.iter().find(|r| r.id == id) else { return };
        let (title, time) = (r.title.clone(), at.format(TIME_FORMAT).to_string());
        match r.style {
            Style::Card => {
                ctx.card(id, Card::Reminder { title, time, snooze_open: false });
                self.state.pending.insert(id.to_owned(), Pending::Reminder { at });
            }
            Style::Auto => {
                ctx.card(id, Card::Nudge { title, time });
                self.state.pending.remove(id);
            }
        }
    }

    /// 跳「錯過的提醒」；上一張還沒按掉的話併在一起（新的排前面）。
    fn show_missed(&mut self, ctx: &mut Ctx, rows: Vec<(String, String)>, more: usize) {
        let (mut rows, mut more) = (rows, more);
        if let Some(Pending::Missed { rows: old, more: old_more }) = self.state.pending.remove(MISSED_KEY) {
            rows.extend(old);
            more += old_more;
        }
        more += rows.len().saturating_sub(MISSED_MAX_ROWS);
        rows.truncate(MISSED_MAX_ROWS);
        ctx.card(MISSED_KEY, Card::Missed { rows: rows.clone(), more });
        self.state.pending.insert(MISSED_KEY.to_owned(), Pending::Missed { rows, more });
    }

    /// 剛開起來：上次關掉時還沒按掉的卡重新跳出來（提醒改成自己消失、或被刪掉的就不跳）。
    fn reshow_pending(&mut self, ctx: &mut Ctx) {
        let now = ctx.now;
        self.state.pending.retain(|id, p| match p {
            Pending::Reminder { at } => {
                let Some(r) = self.file.reminders.iter().find(|r| &r.id == id).filter(|r| r.style == Style::Card) else { return false };
                let time = if at.date() == now.date() { at.format(TIME_FORMAT) } else { at.format(DATE_TIME_FORMAT) }.to_string();
                ctx.card(id, Card::Reminder { title: r.title.clone(), time, snooze_open: false });
                true
            }
            Pending::Missed { rows, more } => {
                ctx.card(id, Card::Missed { rows: rows.clone(), more: *more });
                true
            }
        });
    }

    fn find(&self, id: &str) -> Option<&Reminder> {
        self.file.reminders.iter().find(|r| r.id == id)
    }

    /// 這段時間錯過的提醒：(每則一列：標題、最後一次的時間, 列不下的有幾則)。
    fn missed_rows(&self, from: NaiveDateTime, to: NaiveDateTime) -> Option<(Vec<(String, String)>, usize)> {
        let mut rows = Vec::new();
        for r in &self.file.reminders {
            let (count, last) = r.occurrences_between(from, to, &self.calendar);
            let Some(last) = last else { continue };
            let when = if last.date() == to.date() { last.format(TIME_FORMAT) } else { last.format(DATE_TIME_FORMAT) }.to_string();
            let when = if count > 1 { format!("{when}（{count} 次）") } else { when };
            rows.push((last, r.title.clone(), when));
        }
        if rows.is_empty() {
            return None;
        }
        rows.sort_by_key(|r| std::cmp::Reverse(r.0));
        Some((rows.into_iter().map(|(_, t, w)| (t, w)).collect(), 0))
    }

    fn save_state(&mut self, now: NaiveDateTime) {
        self.state.last_checked = Some(now);
        self.last_save = Some(now);
        match serde_json::to_vec_pretty(&self.state) {
            Ok(bytes) => {
                if let Err(e) = write_atomic(&self.paths.state, &bytes) {
                    log!("save reminder state failed: {e}");
                }
            }
            Err(e) => log!("serialize reminder state failed: {e}"),
        }
    }
}

/// 模組代號（設定的功能開關、卡片歸屬都用它）。
pub const ID: &str = "reminders";

impl Module for ReminderModule {
    fn id(&self) -> &'static str {
        ID
    }

    fn name(&self) -> &'static str {
        "提醒"
    }

    fn tick(&mut self, ctx: &mut Ctx) {
        let now = ctx.now;
        if stamp(&self.paths.reminders) != self.file_stamp {
            self.reload_file();
            let before: Vec<String> = self.next.keys().cloned().collect();
            self.reschedule(now);
            for id in before.iter().filter(|id| self.find(id).is_none()) {
                ctx.out.push(crate::modules::Request::Withdraw { key: id.clone() });
            }
        }

        if self.last_tick.is_none() {
            self.reshow_pending(ctx);
        }
        // 第一次 tick：從上次關掉時看到的時間接著算；之後：tick 之間的大空檔 = 睡過一覺。
        let gap_from = match self.last_tick {
            None => self.state.last_checked,
            Some(prev) if now - prev > Duration::seconds(RESUME_GAP_SECS) => Some(prev),
            Some(_) => None,
        };
        // 醒來前一分鐘內該響的照常響，更早的才算錯過。
        let cutoff = now - Duration::seconds(JUST_DUE_SECS);
        let pending_before = self.state.pending.clone();
        if let Some(from) = gap_from.filter(|f| *f < cutoff) {
            if let Some((rows, more)) = self.missed_rows(from, cutoff) {
                self.show_missed(ctx, rows, more);
            }
            self.reschedule(cutoff);
        } else if self.last_tick.is_none() {
            self.reschedule(now);
        }
        self.last_tick = Some(now);

        let due: Vec<String> = self.next.iter().filter(|(_, at)| **at <= now).map(|(id, _)| id.clone()).collect();
        for id in due {
            let fired_at = self.next[&id];
            self.fire(ctx, &id, fired_at);
            match self.find(&id).and_then(|r| r.next_after(now, &self.calendar)) {
                Some(next) => self.next.insert(id, next),
                None => self.next.remove(&id),
            };
        }

        let snoozed: Vec<String> = self.state.snoozes.iter().filter(|(_, at)| **at <= now).map(|(id, _)| id.clone()).collect();
        let snooze_fired = !snoozed.is_empty();
        for id in snoozed {
            self.state.snoozes.remove(&id);
            self.fire(ctx, &id, now);
        }

        // 跳出、按掉的卡馬上存（隨時可能被關掉）；其他照固定間隔存「看到哪個時間了」。
        if snooze_fired || self.state.pending != pending_before || self.last_save.is_none_or(|t| now - t >= Duration::seconds(STATE_SAVE_SECS)) {
            self.save_state(now);
        }
    }

    fn on_event(&mut self, event: &ModuleEvent, ctx: &mut Ctx) {
        match event {
            ModuleEvent::Card { key, action } => {
                if let CardAction::Snooze(minutes) = action {
                    self.state.snoozes.insert(key.to_string(), ctx.now + Duration::minutes(i64::from(*minutes)));
                }
                if *action != CardAction::OpenSnooze {
                    self.state.pending.remove(*key);
                }
                self.save_state(ctx.now);
            }
            ModuleEvent::HolidaysUpdated => {
                self.reload_calendar();
                self.reschedule(ctx.now);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::Request;
    use petkit::config::Config;
    use chrono::{NaiveDate, NaiveTime};
    use petkit::reminders::{HolidayPolicy, Repeat};
    use std::sync::atomic::{AtomicU32, Ordering};

    static N: AtomicU32 = AtomicU32::new(0);

    fn dt(d: u32, h: u32, m: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 3, d).unwrap().and_hms_opt(h, m, 0).unwrap()
    }

    fn setup(reminders: Vec<Reminder>) -> (ReminderModule, PathBuf) {
        let dir = std::env::temp_dir().join(format!("petcore-rem-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let paths = Paths { reminders: dir.join("reminders.json"), state: dir.join("state.json"), holiday_cache: dir.join("holidays") };
        ReminderFile { reminders, ..Default::default() }.save(&paths.reminders).unwrap();
        (ReminderModule::new(paths), dir)
    }

    fn daily(id: &str, h: u32) -> Reminder {
        Reminder {
            id: id.into(),
            title: format!("提醒{id}"),
            enabled: true,
            time: NaiveTime::from_hms_opt(h, 0, 0).unwrap(),
            repeat: Repeat::Daily,
            holiday: HolidayPolicy::Normal,
            start_date: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            style: Style::Card,
        }
    }

    #[test]
    fn an_auto_reminder_shows_a_card_that_goes_away_by_itself() {
        let mut r = daily("a", 9);
        r.style = Style::Auto;
        let (mut m, _) = setup(vec![r]);
        tick(&mut m, dt(2, 8, 59));
        assert_eq!(cards(&tick(&mut m, dt(2, 9, 0))), vec![("a".into(), Card::Nudge { title: "提醒a".into(), time: "09:00".into() })]);
    }

    fn tick(m: &mut ReminderModule, now: NaiveDateTime) -> Vec<Request> {
        let mut out = Vec::new();
        m.tick(&mut Ctx { now, config: &Config::default(), out: &mut out });
        out
    }

    fn cards(reqs: &[Request]) -> Vec<(String, Card)> {
        reqs.iter()
            .filter_map(|r| match r {
                Request::Card { key, card } => Some((key.clone(), card.clone())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn fires_once_at_the_time() {
        let (mut m, _) = setup(vec![daily("a", 9)]);
        assert!(tick(&mut m, dt(2, 8, 59)).is_empty());
        let out = cards(&tick(&mut m, dt(2, 9, 0)));
        assert_eq!(out, vec![("a".into(), Card::Reminder { title: "提醒a".into(), time: "09:00".into(), snooze_open: false })]);
        assert!(cards(&tick(&mut m, dt(2, 9, 0))).is_empty(), "same minute does not fire twice");
    }

    #[test]
    fn snooze_brings_it_back() {
        let (mut m, _) = setup(vec![daily("a", 9)]);
        tick(&mut m, dt(2, 8, 59));
        tick(&mut m, dt(2, 9, 0));
        let mut out = Vec::new();
        m.on_event(&ModuleEvent::Card { key: "a", action: CardAction::Snooze(15) }, &mut Ctx { now: dt(2, 9, 0), config: &Config::default(), out: &mut out });
        assert!(cards(&tick(&mut m, dt(2, 9, 14))).is_empty());
        assert_eq!(cards(&tick(&mut m, dt(2, 9, 15))).len(), 1);
    }

    #[test]
    fn sleeping_through_reminders_gives_one_missed_card() {
        let (mut m, _) = setup(vec![daily("a", 9), daily("b", 10)]);
        tick(&mut m, dt(2, 8, 0));
        // 電腦睡到隔天 11:00：a、b 各錯過兩次，不能一張一張轟出來
        let out = cards(&tick(&mut m, dt(3, 11, 0)));
        assert_eq!(out.len(), 1);
        let (key, card) = &out[0];
        assert_eq!(key, MISSED_KEY);
        let Card::Missed { rows, more } = card else { panic!("expected missed card") };
        assert_eq!(*more, 0);
        assert_eq!(rows[0], ("提醒b".to_string(), "10:00（2 次）".to_string()));
        // 之後照常排：隔天 9:00 響
        assert!(cards(&tick(&mut m, dt(3, 11, 0))).is_empty());
        assert_eq!(cards(&tick(&mut m, dt(4, 9, 0))).len(), 1);
    }

    #[test]
    fn restart_reports_what_was_missed_while_closed() {
        let (mut m, dir) = setup(vec![daily("a", 9)]);
        tick(&mut m, dt(2, 8, 0)); // 存下 lastChecked
        drop(m);
        let paths = Paths { reminders: dir.join("reminders.json"), state: dir.join("state.json"), holiday_cache: dir.join("holidays") };
        let mut m = ReminderModule::new(paths);
        let out = cards(&tick(&mut m, dt(2, 12, 0)));
        assert!(matches!(&out[..], [(k, Card::Missed { .. })] if k == MISSED_KEY));
    }

    fn restart(dir: &Path) -> ReminderModule {
        ReminderModule::new(Paths { reminders: dir.join("reminders.json"), state: dir.join("state.json"), holiday_cache: dir.join("holidays") })
    }

    fn press(m: &mut ReminderModule, key: &str, action: CardAction, now: NaiveDateTime) {
        m.on_event(&ModuleEvent::Card { key, action }, &mut Ctx { now, config: &Config::default(), out: &mut Vec::new() });
    }

    #[test]
    fn unhandled_reminder_card_comes_back_after_restart_until_done() {
        let (mut m, dir) = setup(vec![daily("a", 9)]);
        tick(&mut m, dt(2, 8, 59));
        tick(&mut m, dt(2, 9, 0));
        // 響了還沒按就被關掉：重開時照原本的時間再跳一次。
        let mut m = restart(&dir);
        let shown = vec![("a".to_string(), Card::Reminder { title: "提醒a".into(), time: "09:00".into(), snooze_open: false })];
        assert_eq!(cards(&tick(&mut m, dt(2, 9, 1))), shown);
        // 再重開一次還在；按了完成之後就不再跳。
        let mut m = restart(&dir);
        assert_eq!(cards(&tick(&mut m, dt(2, 9, 2))), shown);
        press(&mut m, "a", CardAction::Done, dt(2, 9, 2));
        let mut m = restart(&dir);
        assert!(cards(&tick(&mut m, dt(2, 9, 3))).is_empty());
    }

    #[test]
    fn auto_reminder_is_not_brought_back_after_restart() {
        let mut r = daily("a", 9);
        r.style = Style::Auto;
        let (mut m, dir) = setup(vec![r]);
        tick(&mut m, dt(2, 8, 59));
        tick(&mut m, dt(2, 9, 0));
        let mut m = restart(&dir);
        assert!(cards(&tick(&mut m, dt(2, 9, 1))).is_empty());
    }

    #[test]
    fn unhandled_missed_card_comes_back_after_restart() {
        let (mut m, dir) = setup(vec![daily("a", 9)]);
        tick(&mut m, dt(2, 8, 0));
        let missed = cards(&tick(&mut m, dt(3, 11, 0)));
        assert!(matches!(&missed[..], [(k, Card::Missed { .. })] if k == MISSED_KEY));
        let mut m = restart(&dir);
        assert_eq!(cards(&tick(&mut m, dt(3, 11, 1))), missed);
        press(&mut m, MISSED_KEY, CardAction::Dismiss, dt(3, 11, 1));
        let mut m = restart(&dir);
        assert!(cards(&tick(&mut m, dt(3, 11, 2))).is_empty());
    }

    #[test]
    fn editing_the_file_reschedules_and_withdraws_deleted() {
        let (mut m, dir) = setup(vec![daily("a", 9), daily("b", 10)]);
        tick(&mut m, dt(2, 8, 0));
        std::thread::sleep(std::time::Duration::from_millis(20));
        ReminderFile { reminders: vec![daily("c", 8)], ..Default::default() }.save(&dir.join("reminders.json")).unwrap();
        let out = tick(&mut m, dt(2, 8, 1));
        assert!(out.contains(&Request::Withdraw { key: "a".into() }));
        assert!(out.contains(&Request::Withdraw { key: "b".into() }));
        assert_eq!(cards(&tick(&mut m, dt(3, 8, 0))), vec![("c".into(), Card::Reminder { title: "提醒c".into(), time: "08:00".into(), snooze_open: false })]);
    }
}
