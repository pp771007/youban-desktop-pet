//! 專注（番茄鐘）：專注一段、休息一段，每段結束跳一張卡；每幾輪一次長休息。
//! 專注中桌寵安靜陪著、休息提醒暫停（核心看 `session`）。從右鍵選單或設定程式開始、停止。

use chrono::{Duration, NaiveDateTime};
use petkit::config::Config;
use petkit::ipc::{CMD_FOCUS_START, CMD_FOCUS_STOP};
use petkit::reminders::Style;

use crate::bubble::{Card, CardAction};
use crate::icons::Icon;
use crate::modules::{Ctx, Module, ModuleEvent, Session};

/// 模組代號（設定的功能開關、卡片歸屬都用它）。
pub const ID: &str = "focus";
const MENU_START: u32 = 1;
const MENU_STOP: u32 = 2;
const CARD_KEY: &str = "focus";
const TIME_FORMAT: &str = "%H:%M";

/// 台詞：每次隨機挑一句。`{m}` 專注幾分鐘、`{r}` 休息幾分鐘、`{n}` 專注了幾輪。
const START: &[&str] = &["好，我安靜陪你～", "一起加油吧！我在旁邊陪你", "開始專注囉，我不吵你～", "專心時間到，我也坐好了", "你專心，我顧著時間喔"];
const SHORT_BREAK: &[&str] = &["專注 {m} 分鐘了，休息一下吧～", "辛苦了！起來喝口水吧", "做得好～休息 {r} 分鐘喔", "{m} 分鐘到囉，伸個懶腰吧", "好認真！讓眼睛休息一下吧"];
const LONG_BREAK: &[&str] = &["專注 {n} 輪了，好好休息 {r} 分鐘吧～", "連續 {n} 輪好厲害！休息久一點吧", "今天好努力喔，休息 {r} 分鐘再繼續吧"];
const BACK: &[&str] = &["休息結束囉，繼續加油！", "回來囉～下一輪開始！", "充電完畢，一起繼續吧", "休息夠了嗎？我們繼續吧～"];

/// 照時間挑一句（模組沒有亂數；每段結束的時間不同，挑到的就不同）。
fn line(lines: &[&str], now: NaiveDateTime, fill: &[(&str, u32)]) -> String {
    // 每段都在整分鐘結束、時間戳是 60 的倍數：先除掉秒、再打散，不然每次都挑到同一句。
    let minute = now.and_utc().timestamp().unsigned_abs() / 60;
    let i = (minute.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 32) as usize % lines.len();
    fill.iter().fold(lines[i].to_owned(), |s, (key, v)| s.replace(key, &v.to_string()))
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    Idle,
    /// 第 `round` 輪專注到 `until`。
    Focus { until: NaiveDateTime, round: u32 },
    Break { until: NaiveDateTime, round: u32 },
}

pub struct FocusModule {
    phase: Phase,
    /// 剛結束的那一段：卡片上按「稍後」= 那一段再延長幾分鐘。
    before: Phase,
}

impl Default for FocusModule {
    fn default() -> Self {
        Self { phase: Phase::Idle, before: Phase::Idle }
    }
}

fn minutes(m: u32) -> Duration {
    Duration::minutes(i64::from(m.max(1)))
}

impl FocusModule {
    fn start(&mut self, ctx: &mut Ctx) {
        self.phase = Phase::Focus { until: ctx.now + minutes(ctx.config.focus_minutes), round: 1 };
        self.before = Phase::Idle;
        ctx.say(line(START, ctx.now, &[]));
    }

    fn stop(&mut self, ctx: &mut Ctx) {
        self.phase = Phase::Idle;
        self.before = Phase::Idle;
        ctx.out.push(crate::modules::Request::Withdraw { key: CARD_KEY.into() });
    }

    fn card(ctx: &mut Ctx, title: String) {
        let time = ctx.now.format(TIME_FORMAT).to_string();
        let card = match ctx.config.focus_style {
            Style::Card => Card::Reminder { title, time, snooze_open: false },
            Style::Auto => Card::Nudge { title, time },
        };
        ctx.card(CARD_KEY, card);
    }
}

/// 專注幾輪之後的休息是長休息。
fn is_long_break(round: u32, config: &Config) -> bool {
    config.focus_rounds_before_long > 0 && round.is_multiple_of(config.focus_rounds_before_long)
}

impl Module for FocusModule {
    fn id(&self) -> &'static str {
        ID
    }

    fn name(&self) -> &'static str {
        "專注"
    }

    fn tick(&mut self, ctx: &mut Ctx) {
        let now = ctx.now;
        match self.phase {
            Phase::Focus { until, round } if now >= until => {
                let long = is_long_break(round, ctx.config);
                let rest = if long { ctx.config.long_break_minutes } else { ctx.config.short_break_minutes };
                self.before = self.phase;
                self.phase = Phase::Break { until: now + minutes(rest), round };
                let fill = [("{m}", ctx.config.focus_minutes), ("{r}", rest), ("{n}", round)];
                Self::card(ctx, line(if long { LONG_BREAK } else { SHORT_BREAK }, now, &fill));
            }
            Phase::Break { until, round } if now >= until => {
                self.before = self.phase;
                self.phase = if ctx.config.focus_auto_continue { Phase::Focus { until: now + minutes(ctx.config.focus_minutes), round: round + 1 } } else { Phase::Idle };
                Self::card(ctx, line(BACK, now, &[]));
            }
            _ => {}
        }
    }

    fn on_event(&mut self, event: &ModuleEvent, ctx: &mut Ctx) {
        match event {
            ModuleEvent::Menu(MENU_START) => self.start(ctx),
            ModuleEvent::Menu(MENU_STOP) => self.stop(ctx),
            ModuleEvent::Command(v) => match v["cmd"].as_str() {
                Some(CMD_FOCUS_START) if self.phase == Phase::Idle => self.start(ctx),
                Some(CMD_FOCUS_STOP) => self.stop(ctx),
                _ => {}
            },
            // 卡片上按「稍後」：剛結束的那一段再延長幾分鐘（再專注一下、多休息一下）。
            ModuleEvent::Card { key: CARD_KEY, action: CardAction::Snooze(m) } => {
                let until = ctx.now + minutes(*m);
                self.phase = match self.before {
                    Phase::Focus { round, .. } => Phase::Focus { until, round },
                    Phase::Break { round, .. } => Phase::Break { until, round },
                    Phase::Idle => self.phase,
                };
                self.before = Phase::Idle;
            }
            _ => {}
        }
    }

    fn menu(&self, now: NaiveDateTime) -> Vec<(u32, String, Icon)> {
        match self.session(now) {
            None => vec![(MENU_START, "開始專注".into(), Icon::Clock)],
            Some(s) => vec![(MENU_STOP, format!("停止專注（{}還剩 {} 分）", if s.focusing { "" } else { "休息" }, s.minutes_left), Icon::Stop)],
        }
    }

    fn session(&self, now: NaiveDateTime) -> Option<Session> {
        let (until, focusing) = match self.phase {
            Phase::Idle => return None,
            Phase::Focus { until, .. } => (until, true),
            Phase::Break { until, .. } => (until, false),
        };
        let secs = (until - now).num_seconds().max(0);
        Some(Session { focusing, secs_left: secs, minutes_left: (secs + 59) / 60 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::Request;
    use chrono::NaiveDate;

    fn at(h: u32, m: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, 23).unwrap().and_hms_opt(h, m, 0).unwrap()
    }

    fn step(f: &mut FocusModule, config: &Config, now: NaiveDateTime, event: Option<ModuleEvent>) -> Vec<Request> {
        let mut out = Vec::new();
        let mut ctx = Ctx { now, config, out: &mut out };
        match event {
            Some(e) => f.on_event(&e, &mut ctx),
            None => f.tick(&mut ctx),
        }
        out
    }

    fn card_titles(reqs: &[Request]) -> Vec<String> {
        reqs.iter()
            .filter_map(|r| match r {
                Request::Card { card: Card::Nudge { title, .. } | Card::Reminder { title, .. }, .. } => Some(title.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn focus_break_rounds_with_a_long_break_every_four() {
        let config = Config { focus_minutes: 25, short_break_minutes: 5, long_break_minutes: 15, focus_rounds_before_long: 4, ..Config::default() };
        let mut f = FocusModule::default();
        let mut now = at(9, 0);
        step(&mut f, &config, now, Some(ModuleEvent::Menu(MENU_START)));
        assert!(f.session(now).is_some_and(|s| s.focusing && s.minutes_left == 25));
        let mut titles = Vec::new();
        // 四輪：25+5、25+5、25+5、25+15。
        for _ in 0..(4 * 30 + 5) {
            now += Duration::minutes(1);
            titles.extend(card_titles(&step(&mut f, &config, now, None)));
        }
        // 這張卡是不是 `lines` 裡的某一句（休息 `rest` 分鐘、第 4 輪）。
        let is = |lines: &[&str], rest: &str, t: &String| lines.iter().any(|l| l.replace("{m}", "25").replace("{r}", rest).replace("{n}", "4") == *t);
        assert_eq!(titles.iter().filter(|t| is(SHORT_BREAK, "5", t)).count(), 3, "{titles:?}");
        assert_eq!(titles.iter().filter(|t| is(LONG_BREAK, "15", t)).count(), 1, "the 4th break is long: {titles:?}");
        assert_eq!(titles.iter().filter(|t| is(BACK, "", t)).count(), 3, "{titles:?}");
        assert!(!titles.iter().any(|t| t.contains('{')), "every placeholder is filled: {titles:?}");
        assert!(f.session(now).is_some_and(|s| !s.focusing && s.minutes_left == 5), "still in the long break at 11:05");

        step(&mut f, &config, now, Some(ModuleEvent::Menu(MENU_STOP)));
        assert!(f.session(now).is_none());
    }

    #[test]
    fn snooze_on_the_card_extends_the_part_that_just_ended() {
        let config = Config { focus_minutes: 25, short_break_minutes: 5, focus_style: Style::Card, ..Config::default() };
        let mut f = FocusModule::default();
        step(&mut f, &config, at(9, 0), Some(ModuleEvent::Menu(MENU_START)));
        let out = step(&mut f, &config, at(9, 25), None);
        assert!(matches!(&out[..], [Request::Card { card: Card::Reminder { .. }, .. }]), "{out:?}");
        // 「再專注一下」：稍後 5 分鐘 → 回到專注，9:30 才休息。
        step(&mut f, &config, at(9, 25), Some(ModuleEvent::Card { key: CARD_KEY, action: CardAction::Snooze(5) }));
        assert!(f.session(at(9, 26)).is_some_and(|s| s.focusing && s.minutes_left == 4));
        assert_eq!(card_titles(&step(&mut f, &config, at(9, 30), None)).len(), 1);
        assert!(f.session(at(9, 31)).is_some_and(|s| !s.focusing));
    }
}
