//! 桌寵講話：什麼時候講、講什麼。純邏輯，不碰 Win32；時間與時鐘由呼叫端餵進來（可以快轉測試）。
//!
//! 三種來源：
//! - 回應：被點、被摸、被甩、睡醒（`on`）
//! - 時間點：打招呼、午餐、下班、深夜……每天各一次（`tick`）
//! - 閒聊與休息提醒：使用者在電腦前時偶爾講一句；連續用電腦太久提醒起來動一動（`tick`）
//!
//! 一律有冷卻時間、最近講過的不重複；使用者不在電腦前時不對著空房間講話。

use std::collections::VecDeque;

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, Timelike, Weekday};
use petkit::config::Talk;

use crate::behavior::{Emote, Happening};

/// 開機後等幾秒再打招呼（桌寵先出現、站穩）。
const GREET_DELAY_SECS: f64 = 3.0;
/// 自己開口（時間點、閒聊、休息提醒）之間至少隔多久。
const MIN_GAP_SECS: f64 = 45.0;
/// 回應使用者動作之間至少隔多久（不然每點一下都講）。
const REACT_GAP_SECS: f64 = 6.0;
const REACT_CHANCE_OFTEN: f32 = 0.6;
const REACT_CHANCE_SOMETIMES: f32 = 0.3;
/// 閒聊間隔（秒，隨機範圍）。
const IDLE_CHAT_OFTEN_SECS: (f32, f32) = (6.0 * 60.0, 12.0 * 60.0);
const IDLE_CHAT_SOMETIMES_SECS: (f32, f32) = (15.0 * 60.0, 30.0 * 60.0);
/// 使用者這麼久沒動電腦就不算「在電腦前」。
const PRESENT_SECS: f32 = 30.0;
/// 睡這麼久以上醒來才說「歡迎回來」。
const WELCOME_BACK_SECS: f32 = 15.0 * 60.0;
/// 戳：這段時間內點這麼多下就抗議。
const POKE_WINDOW_SECS: f64 = 8.0;
const POKES_TO_COMPLAIN: usize = 5;
/// 放手時快過這個（DIP/秒）才算「被甩出去」，輕輕放下不算。
const THROW_SPEED_DIP: f32 = 1200.0;
/// 休息提醒：中間離開這麼久算休息過了，連續使用時間重新算（多久提醒一次在設定裡）。
const BREAK_SECS: f32 = 5.0 * 60.0;
/// 「一天」從清晨四點算起：半夜一點還沒睡算前一天的深夜，不會再打一次招呼。
const DAY_STARTS_HOUR: i64 = 4;
const RECENT_MAX: usize = 8;
/// 泡泡顯示多久：基本時間＋每個字，再設上限。
const SHOW_BASE_SECS: f32 = 2.2;
const SHOW_PER_CHAR_SECS: f32 = 0.12;
const SHOW_MAX_SECS: f32 = 7.0;

const MORNING: &[&str] = &["早安！今天也一起加油吧", "早安～睡得好嗎？", "早！先喝杯水再開始吧"];
const MONDAY_MORNING: &[&str] = &["星期一早安！慢慢來就好", "早安～新的一週開始了"];
const DAY_OFF: &[&str] = &["今天放假！好好休息吧", "放假還來看我，好開心", "假日愉快～"];
const NOON: &[&str] = &["午安！吃飽了嗎？", "午安～"];
const AFTERNOON: &[&str] = &["下午好！", "嗨～下午也要打起精神喔"];
const EVENING: &[&str] = &["晚上好～今天過得怎麼樣？", "晚上好！今天辛苦了"];
const LATE_NIGHT: &[&str] = &["這麼晚了還在忙呀？", "夜貓子你好"];

const WELCOME_BACK: &[&str] = &["你回來了！", "歡迎回來～", "等你好久了", "啊，你回來了！我剛剛睡得好香", "好想你喔！"];
const CLICKED: &[&str] = &["嗯？", "找我嗎？", "怎麼了？", "嘿！", "在這裡～"];
const POKED: &[&str] = &["不要一直戳我啦！", "好啦好啦，我在這", "會癢啦～"];
const PETTED: &[&str] = &["好舒服～", "嘿嘿", "再摸一下", "最喜歡你了", "我最喜歡被你摸頭了"];
const THROWN: &[&str] = &["哇啊啊——", "飛起來了！", "呀——"];
const DIZZY: &[&str] = &["頭好暈…", "不要再丟了啦…", "世界在轉圈圈…"];
const ATE: &[&str] = &["好好吃！", "謝謝招待～", "還有嗎？", "嗯～好香"];
const KICKED: &[&str] = &["看我的！", "接球～", "好好玩！"];
const RIDING: &[&str] = &["哇～坐車車！", "要載我去哪裡？", "搖搖晃晃的～", "慢一點啦！"];

/// `{m}` 換成連續使用的分鐘數。
const REST: &[&str] = &["已經連續用電腦 {m} 分鐘了，起來走走吧", "{m} 分鐘沒休息了，喝口水、伸個懶腰吧", "眼睛累了吧？看看遠方休息一下"];
const IDLE_CHAT: &[&str] = &[
    "今天也辛苦了",
    "要記得喝水喔",
    "我在這裡陪你",
    "好想曬太陽",
    "嗯～有點無聊",
    "你在忙什麼呀？",
    "放輕鬆，慢慢來",
    "累了就休息一下吧",
    "哼哼哼～♪",
    "肚子有點餓…",
    "你喜歡吃什麼呀？",
    "如果我會泡茶就好了",
    "跟你在一起好安心",
    "有你在，我一點都不無聊",
    "謝謝你每天陪我",
    "不管多忙，我都在這裡",
];
/// 取了名字才會講、用名字自稱的閒聊；`{name}` 換成名字。
const NAMED_IDLE_CHAT: &[&str] = &["{name}肚子有點餓了～", "{name}想被摸摸頭～", "{name}今天也會乖乖陪你喔", "嘿嘿，{name}在這裡喔～", "{name}最喜歡你了～"];
const NAME_SLOT: &str = "{name}";
/// 閒聊時改用名字自稱的機率：偶爾才講才可愛，太常會膩。
const NAMED_CHAT_CHANCE: f32 = 0.25;

/// 剛被取好名字時說的話。
pub fn named_line(name: &str) -> String {
    format!("我叫{name}囉～謝謝你幫我取名字！")
}

/// 陪伴天數紀念日說的話（第一天是初次見面）；取了名字就報上名字。
pub fn milestone_line(days: u32, name: Option<&str>) -> String {
    match (days, name) {
        (1, None) => "初次見面，請多指教！".to_owned(),
        (1, Some(n)) => format!("我是{n}，初次見面，請多指教～"),
        (_, None) => format!("我們一起過了 {days} 天了！謝謝你一直陪著我"),
        (_, Some(n)) => format!("{n}陪你 {days} 天了！謝謝你一直陪著我～"),
    }
}

/// 每天一次、在某段時間內講的話。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Moment {
    Lunch,
    Snack,
    OffWork,
    Friday,
    Late,
}

struct MomentRule {
    moment: Moment,
    /// (時, 分)；`from` 比 `to` 晚代表跨過午夜。
    from: (u32, u32),
    to: (u32, u32),
    workday_only: bool,
    weekday: Option<Weekday>,
    lines: &'static [&'static str],
}

const MOMENTS: &[MomentRule] = &[
    MomentRule { moment: Moment::Lunch, from: (11, 45), to: (12, 30), workday_only: false, weekday: None, lines: &["快中午了，今天想吃什麼？", "肚子咕嚕咕嚕叫了，去吃飯吧"] },
    MomentRule { moment: Moment::Snack, from: (15, 0), to: (15, 40), workday_only: false, weekday: None, lines: &["下午三點了，來點點心吧", "休息一下，喝杯飲料？"] },
    MomentRule { moment: Moment::Friday, from: (16, 0), to: (17, 30), workday_only: true, weekday: Some(Weekday::Fri), lines: &["明天就放假了！", "星期五了，再撐一下下"] },
    MomentRule { moment: Moment::OffWork, from: (17, 55), to: (18, 40), workday_only: true, weekday: None, lines: &["下班時間到了！今天辛苦了", "可以下班了嗎？"] },
    MomentRule { moment: Moment::Late, from: (23, 0), to: (3, 0), workday_only: false, weekday: None, lines: &["已經很晚了，早點休息吧", "熬夜對身體不好喔"] },
];

impl MomentRule {
    fn matches(&self, clock: NaiveDateTime, workday: bool) -> bool {
        if (self.workday_only && !workday) || self.weekday.is_some_and(|w| w != clock.weekday()) {
            return false;
        }
        let m = clock.hour() * 60 + clock.minute();
        let (from, to) = (self.from.0 * 60 + self.from.1, self.to.0 * 60 + self.to.1);
        if from <= to { (from..to).contains(&m) } else { m >= from || m < to }
    }
}

/// 要講的一句話。
#[derive(Debug, Clone, PartialEq)]
pub struct Say {
    pub text: String,
    pub emote: Option<Emote>,
    /// 泡泡要顯示幾秒。
    pub secs: f32,
    /// 回應使用者剛做的事（戳、摸、拖、畫好範圍、開始專注）：專注中照講。
    /// `false` = 自己找話講（閒聊、功能的通知），專注中一律不講。桌寵所有的話都從 `App::say` 出去，
    /// 那裡只看這個欄位決定；故意不給預設值，新加的每一句都得自己選一邊。
    pub reply: bool,
}

/// 每次問「要不要講話」時的環境。
pub struct Ctx {
    pub now: f64,
    pub clock: NaiveDateTime,
    /// 整台電腦多久沒有滑鼠鍵盤輸入。
    pub idle_input_secs: f32,
    pub talk: Talk,
    /// 今天要上班（看辦公日曆）。
    pub workday: bool,
    /// 現在方便講話：沒有提醒卡、沒躲起來、沒在換桌寵。
    pub can_speak: bool,
    pub asleep: bool,
    /// 使用者幫牠取的名字（沒取是 `None`）。
    pub name: Option<String>,
}

pub struct Chatter {
    rng: u64,
    started: f64,
    last_said: f64,
    last_react: f64,
    next_idle: Option<f64>,
    recent: VecDeque<&'static str>,
    greeted_on: Option<NaiveDate>,
    moments_day: Option<NaiveDate>,
    moments_done: Vec<Moment>,
    pokes: VecDeque<f64>,
    active_since: Option<f64>,
    last_rest: f64,
}

impl Chatter {
    /// `greeted_on`：上次打招呼是哪一天（存在磁碟上，重開桌寵同一天不會再打一次）。
    pub fn new(now: f64, seed: u64, greeted_on: Option<NaiveDate>) -> Self {
        Self {
            rng: seed | 1,
            started: now,
            last_said: f64::NEG_INFINITY,
            last_react: f64::NEG_INFINITY,
            next_idle: None,
            recent: VecDeque::new(),
            greeted_on,
            moments_day: None,
            moments_done: Vec::new(),
            pokes: VecDeque::new(),
            active_since: None,
            last_rest: f64::NEG_INFINITY,
        }
    }

    pub fn greeted_on(&self) -> Option<NaiveDate> {
        self.greeted_on
    }

    /// 桌寵身上發生了某件事，要不要回一句。
    pub fn on(&mut self, happening: Happening, ctx: &Ctx) -> Option<Say> {
        if ctx.talk == Talk::Never {
            return None;
        }
        let now = ctx.now;
        match happening {
            Happening::Clicked => {
                if push_within(&mut self.pokes, now, POKE_WINDOW_SECS) >= POKES_TO_COMPLAIN {
                    self.pokes.clear();
                    return self.react(POKED, Some(Emote::Anger), ctx, true);
                }
                self.react(CLICKED, None, ctx, false)
            }
            Happening::Petted => self.react(PETTED, None, ctx, false),
            Happening::Thrown { speed_dip } if speed_dip >= THROW_SPEED_DIP => self.react(THROWN, None, ctx, false),
            Happening::Thrown { .. } => None,
            Happening::Dizzy => self.react(DIZZY, None, ctx, true),
            Happening::Riding => self.react(RIDING, Some(Emote::Note), ctx, false),
            // 點心是使用者特地給的，一定道謝。
            Happening::Ate => self.react(ATE, None, ctx, true),
            Happening::Kicked { .. } => self.react(KICKED, None, ctx, false),
            // 今天還沒打招呼的話，接下來的 `tick` 會打招呼，這裡不搶。
            Happening::Woke { slept_secs } if slept_secs >= WELCOME_BACK_SECS && self.greeted_on == Some(day_of(ctx.clock)) => self.react(WELCOME_BACK, Some(Emote::Hearts), ctx, true),
            Happening::Woke { .. } => None,
        }
    }

    /// 每秒一次：打招呼、時間點、休息提醒、閒聊。
    pub fn tick(&mut self, ctx: &Ctx) -> Option<Say> {
        let now = ctx.now;
        let today = day_of(ctx.clock);
        if self.moments_day != Some(today) {
            self.moments_day = Some(today);
            self.moments_done.clear();
        }
        // 連續使用時間不管能不能講話都要記。
        if ctx.idle_input_secs >= BREAK_SECS {
            self.active_since = None;
        } else if self.active_since.is_none() {
            self.active_since = Some(now - ctx.idle_input_secs as f64);
        }

        if ctx.talk == Talk::Never || !ctx.can_speak || ctx.asleep || ctx.idle_input_secs >= PRESENT_SECS || now - self.started < GREET_DELAY_SECS {
            return None;
        }
        if self.greeted_on != Some(today) {
            self.greeted_on = Some(today);
            return Some(self.say(greeting_lines(ctx.clock, ctx.workday), None, now));
        }
        if now - self.last_said < MIN_GAP_SECS {
            return None;
        }
        if let Some(rule) = MOMENTS.iter().find(|r| !self.moments_done.contains(&r.moment) && r.matches(ctx.clock, ctx.workday)) {
            self.moments_done.push(rule.moment);
            return Some(self.say(rule.lines, None, now));
        }
        let range = if ctx.talk == Talk::Often { IDLE_CHAT_OFTEN_SECS } else { IDLE_CHAT_SOMETIMES_SECS };
        match self.next_idle {
            None => self.next_idle = Some(now + self.rand_range(range) as f64),
            Some(at) if now >= at => {
                self.next_idle = Some(now + self.rand_range(range) as f64);
                if let Some(name) = ctx.name.as_deref()
                    && self.rand01() < NAMED_CHAT_CHANCE
                {
                    let say = self.say(NAMED_IDLE_CHAT, None, now);
                    return Some(Say { text: say.text.replace(NAME_SLOT, name), ..say });
                }
                return Some(self.say(IDLE_CHAT, None, now));
            }
            Some(_) => {}
        }
        None
    }

    /// 回應使用者的動作；`always` = 一定回（抗議、頭暈），否則看機率。
    fn react(&mut self, lines: &[&'static str], emote: Option<Emote>, ctx: &Ctx, always: bool) -> Option<Say> {
        if !ctx.can_speak || (!always && ctx.now - self.last_react < REACT_GAP_SECS) {
            return None;
        }
        let chance = if ctx.talk == Talk::Often { REACT_CHANCE_OFTEN } else { REACT_CHANCE_SOMETIMES };
        if !always && self.rand01() >= chance {
            return None;
        }
        self.last_react = ctx.now;
        Some(Say { reply: true, ..self.say(lines, emote, ctx.now) })
    }

    /// 休息提醒（跟「講話」設定無關，每秒問一次）：連續用電腦滿 `every_secs` 而且離上次提醒也滿了，
    /// 回傳卡片上那句話。人離開五分鐘就重新算（`tick` 記的連續使用時間）。
    pub fn rest_due(&mut self, now: f64, every_secs: f64) -> Option<String> {
        let since = self.active_since?;
        if now - since < every_secs || now - self.last_rest < every_secs {
            return None;
        }
        self.last_rest = now;
        let minutes = ((now - since) / 60.0).round() as i64;
        Some(self.say(REST, None, now).text.replace("{m}", &minutes.to_string()))
    }

    /// 休息提醒按了「稍後」：`minutes` 分鐘後再提醒一次。
    pub fn snooze_rest(&mut self, now: f64, minutes: u32, every_secs: f64) {
        self.last_rest = now - every_secs + f64::from(minutes) * 60.0;
    }

    fn say(&mut self, lines: &[&'static str], emote: Option<Emote>, now: f64) -> Say {
        let fresh: Vec<&'static str> = lines.iter().copied().filter(|l| !self.recent.contains(l)).collect();
        let pool = if fresh.is_empty() { lines.to_vec() } else { fresh };
        let line = pool[(self.rand01() * pool.len() as f32) as usize % pool.len()];
        if self.recent.len() >= RECENT_MAX {
            self.recent.pop_front();
        }
        self.recent.push_back(line);
        self.last_said = now;
        let chars = line.chars().count() as f32;
        Say { text: line.to_owned(), emote, secs: (SHOW_BASE_SECS + SHOW_PER_CHAR_SECS * chars).min(SHOW_MAX_SECS), reply: false }
    }

    fn rand01(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }

    fn rand_range(&mut self, (lo, hi): (f32, f32)) -> f32 {
        lo + (hi - lo) * self.rand01()
    }
}

/// 記一次、丟掉時間窗外的，回傳窗內次數。
fn push_within(times: &mut VecDeque<f64>, now: f64, window: f64) -> usize {
    times.retain(|t| now - t <= window);
    times.push_back(now);
    times.len()
}

fn day_of(clock: NaiveDateTime) -> NaiveDate {
    (clock - Duration::hours(DAY_STARTS_HOUR)).date()
}

fn greeting_lines(clock: NaiveDateTime, workday: bool) -> &'static [&'static str] {
    match clock.hour() {
        5..=10 if !workday => DAY_OFF,
        5..=10 if clock.weekday() == Weekday::Mon => MONDAY_MORNING,
        5..=10 => MORNING,
        11..=13 => NOON,
        14..=17 => AFTERNOON,
        18..=22 => EVENING,
        _ => LATE_NIGHT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(h, min, 0).unwrap()
    }

    /// 2026-11-23 是星期一。
    fn ctx(now: f64, clock: NaiveDateTime) -> Ctx {
        Ctx { now, clock, idle_input_secs: 0.0, talk: Talk::Sometimes, workday: true, can_speak: true, asleep: false, name: None }
    }

    /// 從 `now` 開始每秒 tick 一次，跑 `secs` 秒（時鐘跟著走），收集講過的話。
    fn run(c: &mut Chatter, now: &mut f64, clock: &mut NaiveDateTime, secs: u32, edit: impl Fn(&mut Ctx)) -> Vec<String> {
        let mut said = Vec::new();
        for _ in 0..secs {
            *now += 1.0;
            *clock += Duration::seconds(1);
            let mut x = ctx(*now, *clock);
            edit(&mut x);
            if let Some(s) = c.tick(&x) {
                said.push(s.text);
            }
        }
        said
    }

    #[test]
    fn greets_once_a_day_and_not_again_after_a_restart() {
        let (mut now, mut clock) = (0.0, at(2026, 11, 23, 9, 0));
        let mut c = Chatter::new(now, 7, None);
        let said = run(&mut c, &mut now, &mut clock, 10, |_| {});
        assert_eq!(said.len(), 1);
        assert!(MONDAY_MORNING.contains(&said[0].as_str()), "Monday morning greeting, got {said:?}");

        let mut restarted = Chatter::new(now, 8, c.greeted_on());
        assert!(run(&mut restarted, &mut now, &mut clock, 10, |_| {}).is_empty(), "same day after restart: no second greeting");

        let mut next_day = at(2026, 11, 24, 9, 0);
        let said = run(&mut restarted, &mut now, &mut next_day, 10, |_| {});
        assert!(said.iter().any(|s| MORNING.contains(&s.as_str())), "next morning greets again, got {said:?}");
    }

    #[test]
    fn stays_quiet_when_nobody_is_there_or_talk_is_off() {
        let (mut now, mut clock) = (0.0, at(2026, 11, 23, 12, 0));
        let mut c = Chatter::new(now, 7, None);
        assert!(run(&mut c, &mut now, &mut clock, 120, |x| x.idle_input_secs = 600.0).is_empty(), "away from the computer");
        assert!(run(&mut c, &mut now, &mut clock, 120, |x| x.talk = Talk::Never).is_empty(), "talk off");
        assert!(run(&mut c, &mut now, &mut clock, 120, |x| x.can_speak = false).is_empty(), "a reminder card is up");
        assert_eq!(run(&mut c, &mut now, &mut clock, 120, |_| {}).len(), 2, "back: greeting, then lunch after the gap");
    }

    #[test]
    fn says_each_moment_once_inside_its_window() {
        let (mut now, mut clock) = (0.0, at(2026, 11, 23, 11, 40));
        let mut c = Chatter::new(now, 7, Some(NaiveDate::from_ymd_opt(2026, 11, 23).unwrap()));
        let said = run(&mut c, &mut now, &mut clock, 60 * 60, |_| {});
        let lunch = MOMENTS.iter().find(|r| r.moment == Moment::Lunch).unwrap().lines;
        assert_eq!(said.iter().filter(|s| lunch.contains(&s.as_str())).count(), 1, "lunch exactly once, got {said:?}");
    }

    #[test]
    fn off_work_only_on_workdays() {
        let off = MOMENTS.iter().find(|r| r.moment == Moment::OffWork).unwrap();
        assert!(off.matches(at(2026, 11, 23, 18, 0), true));
        assert!(!off.matches(at(2026, 11, 23, 18, 0), false), "national holiday on a weekday");
        let late = MOMENTS.iter().find(|r| r.moment == Moment::Late).unwrap();
        assert!(late.matches(at(2026, 11, 23, 1, 0), true), "late-night window crosses midnight");
    }

    #[test]
    fn rest_reminder_after_long_use_even_when_not_talking_and_a_break_resets_it() {
        const EVERY: f64 = 30.0 * 60.0;
        let now = std::cell::Cell::new(0.0);
        let mut clock = at(2026, 11, 23, 13, 0);
        let mut c = Chatter::new(now.get(), 7, Some(NaiveDate::from_ymd_opt(2026, 11, 23).unwrap()));
        // 一秒一秒跑：`tick` 記連續使用時間，`rest_due` 問要不要提醒。
        let mut run_rest = |c: &mut Chatter, secs: u32, idle: f32| -> Vec<String> {
            let mut out = Vec::new();
            for _ in 0..secs {
                now.set(now.get() + 1.0);
                clock += Duration::seconds(1);
                let mut x = ctx(now.get(), clock);
                x.talk = Talk::Never;
                x.idle_input_secs = idle;
                c.tick(&x);
                out.extend(c.rest_due(now.get(), EVERY));
            }
            out
        };
        assert!(run_rest(&mut c, 29 * 60, 0.0).is_empty());
        let due = run_rest(&mut c, 2 * 60, 0.0);
        assert_eq!(due.len(), 1, "rest reminder at 30 minutes even with talking off, got {due:?}");
        assert!(due[0].contains("30") || due[0].contains("遠方"), "{due:?}");

        run_rest(&mut c, 6 * 60, 360.0);
        assert!(run_rest(&mut c, 25 * 60, 0.0).is_empty(), "a 6-minute break restarts the count");

        // 稍後 5 分鐘：5 分鐘後再一次。
        let due = run_rest(&mut c, 6 * 60, 0.0);
        assert_eq!(due.len(), 1);
        c.snooze_rest(now.get(), 5, EVERY);
        assert!(run_rest(&mut c, 4 * 60, 0.0).is_empty());
        assert_eq!(run_rest(&mut c, 2 * 60, 0.0).len(), 1, "snoozed rest reminder comes back after 5 minutes");
    }

    #[test]
    fn complains_when_poked() {
        let clock = at(2026, 11, 23, 10, 0);
        let mut c = Chatter::new(0.0, 7, Some(NaiveDate::from_ymd_opt(2026, 11, 23).unwrap()));
        let mut said = Vec::new();
        for i in 0..5 {
            said.extend(c.on(Happening::Clicked, &ctx(100.0 + i as f64, clock)));
        }
        assert!(said.last().is_some_and(|s| POKED.contains(&s.text.as_str())), "fifth poke complains, got {said:?}");
        assert!(said.iter().all(|s| s.reply), "reacting to a poke is a reply: it still speaks while focusing");
        assert_eq!(c.on(Happening::Thrown { speed_dip: 300.0 }, &ctx(300.0, clock)), None, "a gentle drop is not a throw");
    }

    #[test]
    fn calls_itself_by_its_name_now_and_then_only_once_named() {
        let day = Some(NaiveDate::from_ymd_opt(2026, 11, 23).unwrap());
        // 晚上七點到十點半：沒有時間點的話，只剩閒聊。
        let chat = |name: Option<&str>| {
            let (mut now, mut clock) = (0.0, at(2026, 11, 23, 19, 0));
            let mut c = Chatter::new(now, 7, day);
            run(&mut c, &mut now, &mut clock, 3 * 3600 + 30 * 60, |x| {
                x.talk = Talk::Often;
                x.name = name.map(str::to_owned);
            })
        };
        let named = chat(Some("圓圓"));
        let with_name = named.iter().filter(|s| s.contains("圓圓")).count();
        assert!(with_name > 0 && with_name < named.len(), "sometimes, not always: {named:?}");
        assert!(named.iter().all(|s| !s.contains(NAME_SLOT)), "{named:?}");
        let unnamed = chat(None);
        assert!(unnamed.iter().all(|s| IDLE_CHAT.contains(&s.as_str())), "{unnamed:?}");
        assert_eq!(milestone_line(100, Some("圓圓")), "圓圓陪你 100 天了！謝謝你一直陪著我～");
        assert_eq!(milestone_line(100, None), "我們一起過了 100 天了！謝謝你一直陪著我");
    }

    #[test]
    fn idle_chat_does_not_repeat_recent_lines() {
        let mut c = Chatter::new(0.0, 7, None);
        let lines: Vec<String> = (0..IDLE_CHAT.len()).map(|i| c.say(IDLE_CHAT, None, i as f64).text).collect();
        let mut unique = lines.clone();
        unique.sort();
        unique.dedup();
        assert!(unique.len() >= RECENT_MAX.min(IDLE_CHAT.len()), "{lines:?}");
    }
}
