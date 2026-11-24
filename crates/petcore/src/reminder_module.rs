//! 提醒模組：讀 `reminders.json`（設定程式寫的），到點丟提醒卡，處理「稍後」與錯過的提醒。
//! 紀念日也在這裡排：每個「提前幾天、幾點」到了跳一張要按完成的卡（`Entry` 把兩種放在同一條排程上）。
//!
//! 時間一律用「下一次觸發的絕對時間」排程；電腦睡眠或關機醒來時，tick 之間會出現大空檔，
//! 那段時間該響的不一個一個轟出來，而是整理成一張「錯過的提醒」卡。
//!
//! 要按掉的卡（要按完成的提醒、錯過的提醒）沒按就關掉柚伴，重開時會再跳（核心統一存卡片，見 `Module::restore_card`），
//! 跟 Outlook、macOS 提醒事項一樣「沒處理的提醒不會因為重開就不見」。這裡另外記著每張是幾點響的（`pending`），重開時用最新的內容重畫。

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{Datelike, Duration, NaiveDateTime};
use petkit::anniversary::{Anniversary, AnnualDate};
use petkit::config::{load_json, write_atomic};
use petkit::holidays::{self, Calendar};
use petkit::reminders::{Reminder, ReminderFile, Style};
use serde::{Deserialize, Serialize};

use crate::bubble::{Card, CardAction, LIST_MAX_ROWS, card_time};
use crate::icons::Icon;
use crate::log;
use crate::modules::{Ctx, Module, ModuleEvent};

/// tick 之間隔超過這麼久，就當作電腦睡過一覺（正常是每秒一次）。
const RESUME_GAP_SECS: i64 = 120;
const JUST_DUE_SECS: i64 = 60;
/// 多久存一次「看到哪個時間了」。
const STATE_SAVE_SECS: i64 = 60;
const MISSED_KEY: &str = "missed";
/// 紀念日的卡片代號 = 這個前綴 + 紀念日 id（跟提醒的 id 分開，不會撞到）。
const ANNIVERSARY_KEY_PREFIX: &str = "anniversary:";
const WEEKDAYS: [&str; 7] = ["一", "二", "三", "四", "五", "六", "日"];

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
    /// 跳出來、還沒按掉的卡（卡片代號 → 幾點響的）：卡片本身核心會存，重開時靠這個重畫成最新的內容。
    pending: BTreeMap<String, Pending>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum Pending {
    /// 要按完成的提醒，`at` 是響的時間（標題重開時從提醒檔讀，改過名就顯示新的）。
    Reminder {
        at: NaiveDateTime,
    },
    Missed {
        rows: Vec<MissedRow>,
        more: usize,
    },
}

/// 錯過的提醒裡的一列：存時間不存排好的字，卡片放到隔天也能重畫出日期。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
enum MissedRow {
    /// `last` 是最後一次該響的時間，`count` 是這段時間錯過幾次。
    At {
        title: String,
        last: NaiveDateTime,
        count: usize,
    },
    /// 0.6.0 的存檔只有排好的字（標題、時間），讀到就照原樣顯示。
    Text(String, String),
}

impl MissedRow {
    fn shown(&self, today: chrono::NaiveDate) -> (String, String) {
        match self {
            MissedRow::At { title, last, count } => {
                let when = card_time(*last, today);
                (
                    title.clone(),
                    if *count > 1 {
                        format!("{when}（{count} 次）")
                    } else {
                        when
                    },
                )
            }
            MissedRow::Text(title, when) => (title.clone(), when.clone()),
        }
    }
}

fn missed_card(rows: &[MissedRow], more: usize, now: NaiveDateTime) -> Card {
    Card::Missed {
        rows: rows.iter().map(|r| r.shown(now.date())).collect(),
        more,
    }
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

/// 排程上的一筆：一般提醒，或紀念日。
#[derive(Clone, Copy)]
enum Entry<'a> {
    Reminder(&'a Reminder),
    Anniversary(&'a Anniversary),
}

impl Entry<'_> {
    fn key(&self) -> String {
        match self {
            Entry::Reminder(r) => r.id.clone(),
            Entry::Anniversary(a) => format!("{ANNIVERSARY_KEY_PREFIX}{}", a.id),
        }
    }

    fn title(&self) -> &str {
        match self {
            Entry::Reminder(r) => &r.title,
            Entry::Anniversary(a) => &a.title,
        }
    }

    fn next_after(&self, after: NaiveDateTime, cal: &Calendar) -> Option<NaiveDateTime> {
        match self {
            Entry::Reminder(r) => r.next_after(after, cal),
            Entry::Anniversary(a) => a.next_alert_after(after),
        }
    }

    fn occurrences_between(
        &self,
        from: NaiveDateTime,
        to: NaiveDateTime,
        cal: &Calendar,
    ) -> (usize, Option<NaiveDateTime>) {
        match self {
            Entry::Reminder(r) => r.occurrences_between(from, to, cal),
            Entry::Anniversary(a) => a.alerts_between(from, to),
        }
    }

    /// 要按完成才收掉（按掉之前記在 `pending`，重開會再跳）。紀念日一定要，免得沒看到。
    fn needs_done(&self) -> bool {
        match self {
            Entry::Reminder(r) => r.style == Style::Confirm,
            Entry::Anniversary(_) => true,
        }
    }

    /// `at` 響的這一次的卡片；不是今天響的（重開時補跳之前沒按掉的）時間連日期一起寫。
    fn card(&self, at: NaiveDateTime, now: NaiveDateTime) -> Card {
        match self {
            Entry::Reminder(r) => {
                let (title, time) = (r.title.clone(), card_time(at, now.date()));
                match r.style {
                    Style::Confirm => Card::Confirm {
                        title,
                        time,
                        snooze_open: false,
                        icon: Icon::BellRing,
                    },
                    Style::Notice => Card::Notice {
                        title,
                        time,
                        icon: Icon::BellRing,
                    },
                }
            }
            Entry::Anniversary(a) => {
                // 這一張是為哪一次響的；還有幾天照今天算（按「稍後」、隔天重開都對）。
                let (title, time) = match a.next_on_or_after(at.date()) {
                    Some(o) => {
                        let lunar = matches!(a.date, AnnualDate::Lunar { .. })
                            .then(|| format!(" · {}", a.date.label()))
                            .unwrap_or_default();
                        let years = o
                            .nth
                            .filter(|n| *n > 0)
                            .map(|n| format!(" · 滿 {n} 年"))
                            .unwrap_or_default();
                        let day = format!(
                            "{}/{}（{}）{lunar}{years}",
                            o.date.month(),
                            o.date.day(),
                            WEEKDAYS[o.date.weekday().num_days_from_monday() as usize]
                        );
                        (
                            anniversary_line(&a.title, (o.date - now.date()).num_days()),
                            day,
                        )
                    }
                    None => (a.title.clone(), a.date.label()),
                };
                Card::Confirm {
                    title,
                    time,
                    snooze_open: false,
                    icon: Icon::Cake,
                }
            }
        }
    }
}

/// 紀念日卡片上的話（桌寵的口氣）：還有幾天（滿幾年寫在上面那行日期旁邊，這句才短）。
fn anniversary_line(title: &str, days_left: i64) -> String {
    match days_left {
        0 => format!("今天是{title}喔～"),
        1 => format!("明天就是{title}囉～"),
        -1 => format!("昨天是{title}喔～"),
        n if n < 0 => format!("{} 天前是{title}喔～", -n),
        n => format!("再 {n} 天就是{title}囉～"),
    }
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
        let (cal, problems) =
            holidays::load_calendar(&self.paths.holiday_cache, self.file.day_overrides.clone());
        for p in problems {
            log!("holiday calendar unreadable: {p}");
        }
        self.calendar = cal;
    }

    fn entries(&self) -> impl Iterator<Item = Entry<'_>> {
        self.file
            .reminders
            .iter()
            .map(Entry::Reminder)
            .chain(self.file.anniversaries.iter().map(Entry::Anniversary))
    }

    fn entry(&self, key: &str) -> Option<Entry<'_>> {
        self.entries().find(|e| e.key() == key)
    }

    fn reschedule(&mut self, from: NaiveDateTime) {
        self.next = self
            .entries()
            .filter_map(|e| Some((e.key(), e.next_after(from, &self.calendar)?)))
            .collect();
        let keys: HashSet<String> = self.entries().map(|e| e.key()).collect();
        self.state.snoozes.retain(|id, _| keys.contains(id));
        self.state
            .pending
            .retain(|id, _| id == MISSED_KEY || keys.contains(id));
    }

    /// 時間到：跳要按完成的卡（記進 `pending`），或會自己消失的卡。
    fn fire(&mut self, ctx: &mut Ctx, key: &str, at: NaiveDateTime) {
        let Some((card, keep)) = self
            .entry(key)
            .map(|e| (e.card(at, ctx.now), e.needs_done()))
        else {
            return;
        };
        ctx.card(key, card);
        if keep {
            self.state
                .pending
                .insert(key.to_owned(), Pending::Reminder { at });
        } else {
            self.state.pending.remove(key);
        }
    }

    /// 跳「錯過的提醒」；上一張還沒按掉的話併在一起（新的排前面）。
    fn show_missed(&mut self, ctx: &mut Ctx, rows: Vec<MissedRow>) {
        let (mut rows, mut more) = (rows, 0);
        if let Some(Pending::Missed {
            rows: old,
            more: old_more,
        }) = self.state.pending.remove(MISSED_KEY)
        {
            rows.extend(old);
            more += old_more;
        }
        more += rows.len().saturating_sub(LIST_MAX_ROWS);
        rows.truncate(LIST_MAX_ROWS);
        ctx.card(MISSED_KEY, missed_card(&rows, more, ctx.now));
        self.state
            .pending
            .insert(MISSED_KEY.to_owned(), Pending::Missed { rows, more });
    }

    /// 這段時間錯過的提醒，每則一列，最近錯過的排前面。
    fn missed_rows(&self, from: NaiveDateTime, to: NaiveDateTime) -> Vec<MissedRow> {
        let mut rows: Vec<(NaiveDateTime, String, usize)> = self
            .entries()
            .filter_map(|e| {
                let (count, last) = e.occurrences_between(from, to, &self.calendar);
                Some((last?, e.title().to_owned(), count))
            })
            .collect();
        rows.sort_by_key(|r| std::cmp::Reverse(r.0));
        rows.into_iter()
            .map(|(last, title, count)| MissedRow::At { title, last, count })
            .collect()
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
            for id in before.iter().filter(|id| self.entry(id).is_none()) {
                ctx.out
                    .push(crate::modules::Request::Withdraw { key: id.clone() });
            }
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
            let rows = self.missed_rows(from, cutoff);
            if !rows.is_empty() {
                self.show_missed(ctx, rows);
            }
            self.reschedule(cutoff);
        } else if self.last_tick.is_none() {
            self.reschedule(now);
        }
        self.last_tick = Some(now);

        let due: Vec<String> = self
            .next
            .iter()
            .filter(|(_, at)| **at <= now)
            .map(|(id, _)| id.clone())
            .collect();
        for id in due {
            let fired_at = self.next[&id];
            self.fire(ctx, &id, fired_at);
            match self
                .entry(&id)
                .and_then(|e| e.next_after(now, &self.calendar))
            {
                Some(next) => self.next.insert(id, next),
                None => self.next.remove(&id),
            };
        }

        let snoozed: Vec<String> = self
            .state
            .snoozes
            .iter()
            .filter(|(_, at)| **at <= now)
            .map(|(id, _)| id.clone())
            .collect();
        let snooze_fired = !snoozed.is_empty();
        for id in snoozed {
            self.state.snoozes.remove(&id);
            self.fire(ctx, &id, now);
        }

        // 跳出、按掉的卡馬上存（隨時可能被關掉）；其他照固定間隔存「看到哪個時間了」。
        if snooze_fired
            || self.state.pending != pending_before
            || self
                .last_save
                .is_none_or(|t| now - t >= Duration::seconds(STATE_SAVE_SECS))
        {
            self.save_state(now);
        }
    }

    fn on_event(&mut self, event: &ModuleEvent, ctx: &mut Ctx) {
        match event {
            ModuleEvent::Card { key, action } => {
                if let CardAction::Snooze(minutes) = action {
                    self.state.snoozes.insert(
                        key.to_string(),
                        ctx.now + Duration::minutes(i64::from(*minutes)),
                    );
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

    /// 重開或換日時核心還回來的卡：照提醒檔現在的樣子重畫（改過名顯示新的、紀念日的天數與不是今天的時間照今天算）；
    /// 那筆刪掉了、改成自己消失、或已經按掉的不再跳。
    fn restore_card(&mut self, key: &str, _card: Card, now: NaiveDateTime) -> Option<Card> {
        match self.state.pending.get(key)? {
            Pending::Reminder { at } => self
                .entry(key)
                .filter(|e| e.needs_done())
                .map(|e| e.card(*at, now)),
            Pending::Missed { rows, more } => Some(missed_card(rows, *more, now)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::Request;
    use chrono::{NaiveDate, NaiveTime};
    use petkit::config::Config;
    use petkit::reminders::{HolidayPolicy, Repeat};
    use std::sync::atomic::{AtomicU32, Ordering};

    static N: AtomicU32 = AtomicU32::new(0);

    fn dt(d: u32, h: u32, m: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 3, d)
            .unwrap()
            .and_hms_opt(h, m, 0)
            .unwrap()
    }

    fn setup(reminders: Vec<Reminder>) -> (ReminderModule, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "petcore-rem-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            reminders: dir.join("reminders.json"),
            state: dir.join("state.json"),
            holiday_cache: dir.join("holidays"),
        };
        ReminderFile {
            reminders,
            ..Default::default()
        }
        .save(&paths.reminders)
        .unwrap();
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
            style: Style::Confirm,
        }
    }

    #[test]
    fn an_auto_reminder_shows_a_card_that_goes_away_by_itself() {
        let mut r = daily("a", 9);
        r.style = Style::Notice;
        let (mut m, _) = setup(vec![r]);
        tick(&mut m, dt(2, 8, 59));
        assert_eq!(
            cards(&tick(&mut m, dt(2, 9, 0))),
            vec![(
                "a".into(),
                Card::Notice {
                    title: "提醒a".into(),
                    time: "09:00".into(),
                    icon: Icon::BellRing
                }
            )]
        );
    }

    fn tick(m: &mut ReminderModule, now: NaiveDateTime) -> Vec<Request> {
        let mut out = Vec::new();
        m.tick(&mut Ctx {
            now,
            config: &Config::default(),
            out: &mut out,
        });
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
        assert_eq!(
            out,
            vec![(
                "a".into(),
                Card::Confirm {
                    title: "提醒a".into(),
                    time: "09:00".into(),
                    snooze_open: false,
                    icon: Icon::BellRing
                }
            )]
        );
        assert!(
            cards(&tick(&mut m, dt(2, 9, 0))).is_empty(),
            "same minute does not fire twice"
        );
    }

    #[test]
    fn snooze_brings_it_back() {
        let (mut m, _) = setup(vec![daily("a", 9)]);
        tick(&mut m, dt(2, 8, 59));
        tick(&mut m, dt(2, 9, 0));
        let mut out = Vec::new();
        m.on_event(
            &ModuleEvent::Card {
                key: "a",
                action: CardAction::Snooze(15),
            },
            &mut Ctx {
                now: dt(2, 9, 0),
                config: &Config::default(),
                out: &mut out,
            },
        );
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
        let Card::Missed { rows, more } = card else {
            panic!("expected missed card")
        };
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
        let paths = Paths {
            reminders: dir.join("reminders.json"),
            state: dir.join("state.json"),
            holiday_cache: dir.join("holidays"),
        };
        let mut m = ReminderModule::new(paths);
        let out = cards(&tick(&mut m, dt(2, 12, 0)));
        assert!(matches!(&out[..], [(k, Card::Missed { .. })] if k == MISSED_KEY));
    }

    fn restart(dir: &Path) -> ReminderModule {
        ReminderModule::new(Paths {
            reminders: dir.join("reminders.json"),
            state: dir.join("state.json"),
            holiday_cache: dir.join("holidays"),
        })
    }

    fn press(m: &mut ReminderModule, key: &str, action: CardAction, now: NaiveDateTime) {
        m.on_event(
            &ModuleEvent::Card { key, action },
            &mut Ctx {
                now,
                config: &Config::default(),
                out: &mut Vec::new(),
            },
        );
    }

    /// 重開柚伴：核心把上次沒按掉的卡（`needs_done`）還給模組，收集它說要重新跳的。
    fn reopen(
        dir: &Path,
        shown: &[(String, Card)],
        now: NaiveDateTime,
    ) -> (ReminderModule, Vec<(String, Card)>) {
        let mut m = restart(dir);
        let back = shown
            .iter()
            .filter(|(_, c)| c.needs_done())
            .filter_map(|(k, c)| Some((k.clone(), m.restore_card(k, c.clone(), now)?)))
            .collect();
        (m, back)
    }

    #[test]
    fn unhandled_reminder_card_comes_back_after_restart_until_done() {
        let (mut m, dir) = setup(vec![daily("a", 9)]);
        tick(&mut m, dt(2, 8, 59));
        let shown = cards(&tick(&mut m, dt(2, 9, 0)));
        // 響了還沒按就被關掉：重開時照原本的時間再跳一次；再重開一次還在。
        let (_, back) = reopen(&dir, &shown, dt(2, 9, 1));
        assert_eq!(back, shown);
        let (mut m, back) = reopen(&dir, &shown, dt(2, 9, 2));
        assert_eq!(back, shown);
        // 按了完成之後就不再跳；隔天重開的話時間連日期一起寫。
        press(&mut m, "a", CardAction::Done, dt(2, 9, 2));
        assert!(reopen(&dir, &shown, dt(2, 9, 3)).1.is_empty());
        tick(&mut m, dt(3, 9, 0));
        let (_, back) = reopen(&dir, &shown, dt(4, 8, 0));
        assert!(
            matches!(&back[..], [(_, Card::Confirm { time, .. })] if time == "3/3 09:00"),
            "{back:?}"
        );
    }

    #[test]
    fn a_card_is_not_brought_back_once_the_reminder_goes_away_by_itself() {
        let (mut m, dir) = setup(vec![daily("a", 9)]);
        tick(&mut m, dt(2, 8, 59));
        let shown = cards(&tick(&mut m, dt(2, 9, 0)));
        // 關著的時候在設定裡改成「自己消失」：重開不再跳那張要按完成的卡。
        let mut r = daily("a", 9);
        r.style = Style::Notice;
        ReminderFile {
            reminders: vec![r],
            ..Default::default()
        }
        .save(&dir.join("reminders.json"))
        .unwrap();
        assert!(reopen(&dir, &shown, dt(2, 9, 1)).1.is_empty());
    }

    #[test]
    fn unhandled_missed_card_comes_back_after_restart() {
        let (mut m, dir) = setup(vec![daily("a", 9)]);
        tick(&mut m, dt(2, 8, 0));
        let missed = cards(&tick(&mut m, dt(3, 11, 0)));
        assert!(matches!(&missed[..], [(k, Card::Missed { .. })] if k == MISSED_KEY));
        let (mut m, back) = reopen(&dir, &missed, dt(3, 11, 1));
        assert_eq!(back, missed);
        press(&mut m, MISSED_KEY, CardAction::Dismiss, dt(3, 11, 1));
        assert!(reopen(&dir, &missed, dt(3, 11, 2)).1.is_empty());
    }

    #[test]
    fn a_missed_card_left_until_the_next_day_shows_the_date() {
        let (mut m, _) = setup(vec![daily("a", 9)]);
        tick(&mut m, dt(2, 8, 0));
        let missed = cards(&tick(&mut m, dt(3, 11, 0)));
        let row = |c: Option<Card>| match c {
            Some(Card::Missed { rows, .. }) => rows[0].1.clone(),
            other => panic!("expected missed card, got {other:?}"),
        };
        assert_eq!(row(Some(missed[0].1.clone())), "09:00（2 次）");
        assert_eq!(
            row(m.restore_card(MISSED_KEY, missed[0].1.clone(), dt(4, 8, 0))),
            "3/3 09:00（2 次）"
        );
    }

    #[test]
    fn a_missed_card_saved_by_0_6_0_still_comes_back() {
        let (_, dir) = setup(vec![daily("a", 9)]);
        let saved = r#"{ "lastChecked": "2026-03-03T11:00:00", "pending": { "missed": { "kind": "missed", "rows": [["提醒a", "09:00（2 次）"]], "more": 0 } } }"#;
        fs::write(dir.join("state.json"), saved).unwrap();
        let card = Card::Missed {
            rows: vec![("提醒a".into(), "09:00（2 次）".into())],
            more: 0,
        };
        let mut m = restart(&dir);
        assert_eq!(m.state.last_checked, Some(dt(3, 11, 0)));
        assert_eq!(
            m.restore_card(MISSED_KEY, card.clone(), dt(4, 8, 0)),
            Some(card)
        );
    }

    #[test]
    fn anniversary_alerts_say_how_many_days_are_left_and_stay_until_done() {
        use petkit::anniversary::{Alert, Anniversary, AnnualDate};
        let alert = |days_before, h| Alert {
            days_before,
            time: NaiveTime::from_hms_opt(h, 0, 0).unwrap(),
        };
        let a = Anniversary {
            id: "mom".into(),
            title: "媽媽生日".into(),
            enabled: true,
            date: AnnualDate::Solar { month: 3, day: 5 },
            year: Some(1966),
            alerts: vec![alert(3, 9), alert(0, 8)],
        };
        let (_, dir) = setup(Vec::new());
        ReminderFile {
            anniversaries: vec![a],
            ..Default::default()
        }
        .save(&dir.join("reminders.json"))
        .unwrap();
        let mut m = restart(&dir);
        tick(&mut m, dt(2, 8, 0));
        let key = format!("{ANNIVERSARY_KEY_PREFIX}mom");
        let card = |title: &str| {
            vec![(
                key.clone(),
                Card::Confirm {
                    title: title.into(),
                    time: "3/5（四） · 滿 60 年".into(),
                    snooze_open: false,
                    icon: Icon::Cake,
                },
            )]
        };
        let shown = cards(&tick(&mut m, dt(2, 9, 0)));
        assert_eq!(shown, card("再 3 天就是媽媽生日囉～"));
        // 沒按完成就關掉柚伴：隔兩天重開還在，天數照當天算。
        let (mut m, back) = reopen(&dir, &shown, dt(4, 7, 0));
        assert_eq!(back, card("明天就是媽媽生日囉～"));
        tick(&mut m, dt(4, 7, 0));
        press(&mut m, &key, CardAction::Done, dt(4, 7, 0));
        assert_eq!(
            cards(&tick(&mut m, dt(5, 8, 0))),
            card("今天是媽媽生日喔～")
        );
        assert!(cards(&tick(&mut m, dt(5, 8, 1))).is_empty());
    }

    #[test]
    fn editing_the_file_reschedules_and_withdraws_deleted() {
        let (mut m, dir) = setup(vec![daily("a", 9), daily("b", 10)]);
        tick(&mut m, dt(2, 8, 0));
        std::thread::sleep(std::time::Duration::from_millis(20));
        ReminderFile {
            reminders: vec![daily("c", 8)],
            ..Default::default()
        }
        .save(&dir.join("reminders.json"))
        .unwrap();
        let out = tick(&mut m, dt(2, 8, 1));
        assert!(out.contains(&Request::Withdraw { key: "a".into() }));
        assert!(out.contains(&Request::Withdraw { key: "b".into() }));
        assert_eq!(
            cards(&tick(&mut m, dt(3, 8, 0))),
            vec![(
                "c".into(),
                Card::Confirm {
                    title: "提醒c".into(),
                    time: "08:00".into(),
                    snooze_open: false,
                    icon: Icon::BellRing
                }
            )]
        );
    }
}
