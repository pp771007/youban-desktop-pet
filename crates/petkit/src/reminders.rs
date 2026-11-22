//! 提醒的資料格式與排程計算（純邏輯，時間由呼叫端給，方便測任何日期）。
//!
//! 一則提醒有「名目日期」（照重複規則該響的那天）和「實際日期」（套用遇假日規則之後）。
//! 例：每月 28 號、遇假日提前 → 2026-02-28 是星期六、27 號是補假 → 實際在 26 號響。

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use chrono::{Datelike, Days, Duration, NaiveDate, NaiveDateTime, NaiveTime};
use serde::{Deserialize, Serialize};

use crate::holidays::{Calendar, DayKind};

/// 遇假日往前／往後挪最多幾天；名目日期要往回多看這麼多天，才不會漏掉「挪到今天」的那一次。
const SHIFT_WINDOW_DAYS: u64 = 31;
/// 往後找下一次最多看多久（每月規則一定在這之內）。
const HORIZON_DAYS: u64 = 400;
/// 算「錯過幾次」時最多數到多少（每 N 分鐘的提醒關機一個週末會累積很多）。
pub const MAX_MISSED_COUNT: usize = 999;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reminder {
    pub id: String,
    pub title: String,
    #[serde(default = "enabled_default")]
    pub enabled: bool,
    #[serde(with = "hhmm")]
    pub time: NaiveTime,
    pub repeat: Repeat,
    #[serde(default)]
    pub holiday: HolidayPolicy,
    /// 單次提醒的日期；重複提醒從這天開始算。
    pub start_date: NaiveDate,
}

fn enabled_default() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Repeat {
    Once,
    Daily,
    /// `days`：1 = 星期一 … 7 = 星期日。
    Weekly { days: Vec<u8> },
    Monthly { rule: MonthlyRule },
    /// 照官方辦公日曆的上班日（含補班的星期六）。
    Workdays,
    /// 從 `time` 到 `until` 每 `every_minutes` 分鐘一次。
    #[serde(rename_all = "camelCase")]
    Interval {
        every_minutes: u32,
        #[serde(with = "hhmm")]
        until: NaiveTime,
        #[serde(default)]
        workdays_only: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum MonthlyRule {
    /// 第幾號；那個月沒有這一天（例如 2 月 30 號）就在月底響。
    Day { day: u8 },
    LastDay,
    /// 第 `nth` 個星期 `weekday`（1～4，5 = 最後一個）。
    NthWeekday { nth: u8, weekday: u8 },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HolidayPolicy {
    #[default]
    Normal,
    Skip,
    NextWorkday,
    PrevWorkday,
}

/// `reminders.json`：設定程式寫、核心程式讀。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ReminderFile {
    pub reminders: Vec<Reminder>,
    /// 手動覆寫：颱風假、請假（視同假日），或平常放假但這天要上班的日子（視同上班）。
    pub day_overrides: BTreeMap<NaiveDate, DayKind>,
}

impl ReminderFile {
    pub fn load(path: &Path) -> io::Result<Self> {
        crate::config::load_json(path)
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        crate::config::write_atomic(path, &serde_json::to_vec_pretty(self).map_err(io::Error::other)?)
    }
}

mod hhmm {
    use chrono::NaiveTime;
    use serde::{Deserialize, Deserializer, Serializer};

    const FORMAT: &str = "%H:%M";

    pub fn serialize<S: Serializer>(t: &NaiveTime, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&t.format(FORMAT).to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<NaiveTime, D::Error> {
        let s = String::deserialize(d)?;
        NaiveTime::parse_from_str(&s, FORMAT).map_err(serde::de::Error::custom)
    }
}

impl Reminder {
    /// 名目上（套用假日規則之前）這天該不該響。
    fn matches(&self, date: NaiveDate, cal: &Calendar) -> bool {
        if date < self.start_date {
            return false;
        }
        match &self.repeat {
            Repeat::Once => date == self.start_date,
            Repeat::Daily => true,
            Repeat::Weekly { days } => days.contains(&(date.weekday().number_from_monday() as u8)),
            Repeat::Monthly { rule } => monthly_matches(rule, date),
            Repeat::Workdays => cal.is_workday(date),
            Repeat::Interval { workdays_only, .. } => !workdays_only || cal.is_workday(date),
        }
    }

    /// 自己就照上班日走的規則不再套假日規則。
    fn policy(&self) -> HolidayPolicy {
        match self.repeat {
            Repeat::Workdays | Repeat::Interval { workdays_only: true, .. } => HolidayPolicy::Normal,
            _ => self.holiday,
        }
    }

    fn times(&self) -> Vec<NaiveTime> {
        match &self.repeat {
            Repeat::Interval { every_minutes, until, .. } if *every_minutes > 0 => {
                let step = Duration::minutes(i64::from(*every_minutes));
                let mut out = Vec::new();
                let mut t = self.time;
                while t <= *until {
                    out.push(t);
                    let (next, wrapped) = t.overflowing_add_signed(step);
                    if wrapped != 0 {
                        break;
                    }
                    t = next;
                }
                out
            }
            _ => vec![self.time],
        }
    }

    /// `after` 之後（不含）下一次實際要響的時間。
    pub fn next_after(&self, after: NaiveDateTime, cal: &Calendar) -> Option<NaiveDateTime> {
        if !self.enabled {
            return None;
        }
        let times = self.times();
        let policy = self.policy();
        let first = after.date().checked_sub_days(Days::new(SHIFT_WINDOW_DAYS))?.max(self.start_date);
        let mut best: Option<NaiveDateTime> = None;
        for i in 0..=HORIZON_DAYS + SHIFT_WINDOW_DAYS {
            let nominal = first.checked_add_days(Days::new(i))?;
            if let Some(b) = best
                && nominal > b.date().checked_add_days(Days::new(SHIFT_WINDOW_DAYS))?
            {
                break;
            }
            if !self.matches(nominal, cal) {
                continue;
            }
            let Some(actual) = apply_policy(policy, nominal, cal) else { continue };
            if let Some(at) = times.iter().map(|t| actual.and_time(*t)).find(|at| *at > after) {
                best = Some(best.map_or(at, |b| b.min(at)));
            }
            if matches!(self.repeat, Repeat::Once) {
                break;
            }
        }
        best
    }

    /// `(from, to]` 之間錯過了幾次、最後一次是什麼時候。
    pub fn occurrences_between(&self, from: NaiveDateTime, to: NaiveDateTime, cal: &Calendar) -> (usize, Option<NaiveDateTime>) {
        let mut count = 0;
        let mut last = None;
        let mut cursor = from;
        while count < MAX_MISSED_COUNT {
            match self.next_after(cursor, cal) {
                Some(at) if at <= to => {
                    count += 1;
                    last = Some(at);
                    cursor = at;
                }
                _ => break,
            }
        }
        (count, last)
    }

    /// 設定畫面的「接下來幾次」預覽：會響的日子，以及因為假日被跳過或挪動的日子。
    pub fn preview(&self, from: NaiveDateTime, cal: &Calendar, count: usize) -> Vec<PreviewItem> {
        let mut out = Vec::new();
        let mut fires = 0;
        if !self.enabled {
            return out;
        }
        let times = self.times();
        let policy = self.policy();
        let mut last_actual: Option<NaiveDate> = None;
        for i in 0..=HORIZON_DAYS {
            if fires >= count {
                break;
            }
            let Some(nominal) = from.date().checked_add_days(Days::new(i)) else { break };
            if !self.matches(nominal, cal) {
                continue;
            }
            match apply_policy(policy, nominal, cal) {
                None => out.push(PreviewItem::Skipped { date: nominal, note: cal.note(nominal).map(str::to_owned) }),
                Some(actual) => {
                    // 兩個名目日期挪到同一天（週六、週日都順延到週一）只響一次。
                    if last_actual == Some(actual) {
                        continue;
                    }
                    last_actual = Some(actual);
                    let moved_from = (actual != nominal).then_some(nominal);
                    for t in &times {
                        let at = actual.and_time(*t);
                        if at > from && fires < count {
                            out.push(PreviewItem::Fire { at, moved_from, note: moved_from.and_then(|d| cal.note(d)).map(str::to_owned) });
                            fires += 1;
                        }
                    }
                }
            }
            if matches!(self.repeat, Repeat::Once) {
                break;
            }
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PreviewItem {
    #[serde(rename_all = "camelCase")]
    Fire { at: NaiveDateTime, moved_from: Option<NaiveDate>, note: Option<String> },
    Skipped { date: NaiveDate, note: Option<String> },
}

fn apply_policy(policy: HolidayPolicy, date: NaiveDate, cal: &Calendar) -> Option<NaiveDate> {
    match policy {
        HolidayPolicy::Normal => Some(date),
        HolidayPolicy::Skip => cal.is_workday(date).then_some(date),
        HolidayPolicy::NextWorkday => cal.workday_on_or_after(date),
        HolidayPolicy::PrevWorkday => cal.workday_on_or_before(date),
    }
}

fn last_day_of_month(date: NaiveDate) -> u32 {
    let (y, m) = if date.month() == 12 { (date.year() + 1, 1) } else { (date.year(), date.month() + 1) };
    NaiveDate::from_ymd_opt(y, m, 1).and_then(|d| d.pred_opt()).map_or(28, |d| d.day())
}

fn monthly_matches(rule: &MonthlyRule, date: NaiveDate) -> bool {
    let last = last_day_of_month(date);
    match *rule {
        MonthlyRule::Day { day } => date.day() == u32::from(day).min(last),
        MonthlyRule::LastDay => date.day() == last,
        MonthlyRule::NthWeekday { nth, weekday } => {
            if date.weekday().number_from_monday() != u32::from(weekday) {
                return false;
            }
            const LAST: u8 = 5;
            if nth >= LAST { date.day() + 7 > last } else { (date.day() - 1) / 7 + 1 == u32::from(nth) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::holidays::{BUNDLED, parse_dgpa_csv};

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn dt(y: i32, m: u32, day: u32, h: u32, min: u32) -> NaiveDateTime {
        d(y, m, day).and_hms_opt(h, min, 0).unwrap()
    }

    fn cal() -> Calendar {
        let mut c = Calendar::default();
        for &(_, text) in BUNDLED {
            c.add_official(parse_dgpa_csv(text).unwrap());
        }
        c
    }

    fn reminder(repeat: Repeat, holiday: HolidayPolicy) -> Reminder {
        Reminder {
            id: "r".into(),
            title: "喝水".into(),
            enabled: true,
            time: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
            repeat,
            holiday,
            start_date: d(2026, 1, 1),
        }
    }

    #[test]
    fn daily_fires_today_then_tomorrow() {
        let r = reminder(Repeat::Daily, HolidayPolicy::Normal);
        let c = cal();
        assert_eq!(r.next_after(dt(2026, 3, 2, 8, 0), &c), Some(dt(2026, 3, 2, 9, 0)));
        assert_eq!(r.next_after(dt(2026, 3, 2, 9, 0), &c), Some(dt(2026, 3, 3, 9, 0)));
    }

    #[test]
    fn weekly_picks_listed_days() {
        let r = reminder(Repeat::Weekly { days: vec![1, 3] }, HolidayPolicy::Normal); // 一、三
        let c = cal();
        assert_eq!(r.next_after(dt(2026, 3, 3, 10, 0), &c), Some(dt(2026, 3, 4, 9, 0))); // 週二 → 週三
        assert_eq!(r.next_after(dt(2026, 3, 4, 10, 0), &c), Some(dt(2026, 3, 9, 9, 0))); // 週三 → 下週一
    }

    #[test]
    fn monthly_day_clamps_to_month_end() {
        let r = reminder(Repeat::Monthly { rule: MonthlyRule::Day { day: 31 } }, HolidayPolicy::Normal);
        assert_eq!(r.next_after(dt(2026, 2, 1, 0, 0), &cal()), Some(dt(2026, 2, 28, 9, 0)));
        let last = reminder(Repeat::Monthly { rule: MonthlyRule::LastDay }, HolidayPolicy::Normal);
        assert_eq!(last.next_after(dt(2026, 4, 1, 0, 0), &cal()), Some(dt(2026, 4, 30, 9, 0)));
    }

    #[test]
    fn monthly_nth_and_last_weekday() {
        let second_tue = reminder(Repeat::Monthly { rule: MonthlyRule::NthWeekday { nth: 2, weekday: 2 } }, HolidayPolicy::Normal);
        assert_eq!(second_tue.next_after(dt(2026, 3, 1, 0, 0), &cal()), Some(dt(2026, 3, 10, 9, 0)));
        let last_fri = reminder(Repeat::Monthly { rule: MonthlyRule::NthWeekday { nth: 5, weekday: 5 } }, HolidayPolicy::Normal);
        assert_eq!(last_fri.next_after(dt(2026, 3, 1, 0, 0), &cal()), Some(dt(2026, 3, 27, 9, 0)));
    }

    #[test]
    fn workdays_skip_lunar_new_year() {
        let r = reminder(Repeat::Workdays, HolidayPolicy::Normal);
        // 2026 春節：2/16（一）～2/20（五）放假
        assert_eq!(r.next_after(dt(2026, 2, 13, 10, 0), &cal()), Some(dt(2026, 2, 23, 9, 0)));
    }

    #[test]
    fn workdays_include_makeup_saturday() {
        let mut c = Calendar::default();
        c.add_official(parse_dgpa_csv("西元日期,星期,是否放假,備註\n20230107,六,0,補行上班\n").unwrap());
        let mut r = reminder(Repeat::Workdays, HolidayPolicy::Normal);
        r.start_date = d(2023, 1, 1);
        assert_eq!(r.next_after(dt(2023, 1, 6, 10, 0), &c), Some(dt(2023, 1, 7, 9, 0)));
    }

    #[test]
    fn skip_policy_drops_holidays() {
        let r = reminder(Repeat::Daily, HolidayPolicy::Skip);
        assert_eq!(r.next_after(dt(2026, 2, 13, 10, 0), &cal()), Some(dt(2026, 2, 23, 9, 0)));
    }

    #[test]
    fn next_workday_policy_moves_forward_once() {
        let r = reminder(Repeat::Monthly { rule: MonthlyRule::Day { day: 1 } }, HolidayPolicy::NextWorkday);
        // 2026-01-01 開國紀念日（四）→ 1/2（五）
        assert_eq!(r.next_after(dt(2025, 12, 31, 0, 0), &cal()), Some(dt(2026, 1, 2, 9, 0)));
        let daily = reminder(Repeat::Daily, HolidayPolicy::NextWorkday);
        // 週五 9:00 之後 → 週六、週日都挪到週一，週一只響一次
        let mon = daily.next_after(dt(2026, 3, 6, 10, 0), &cal()).unwrap();
        assert_eq!(mon, dt(2026, 3, 9, 9, 0));
        assert_eq!(daily.next_after(mon, &cal()), Some(dt(2026, 3, 10, 9, 0)));
    }

    #[test]
    fn prev_workday_policy_moves_back_past_consecutive_holidays() {
        let r = reminder(Repeat::Monthly { rule: MonthlyRule::Day { day: 28 } }, HolidayPolicy::PrevWorkday);
        // 2/28 週六、2/27 補假 → 2/26（四）
        assert_eq!(r.next_after(dt(2026, 2, 1, 0, 0), &cal()), Some(dt(2026, 2, 26, 9, 0)));
        // 2/26 已過 → 3/28 週六 → 3/27（五）
        assert_eq!(r.next_after(dt(2026, 2, 26, 10, 0), &cal()), Some(dt(2026, 3, 27, 9, 0)));
    }

    #[test]
    fn interval_within_window_on_workdays() {
        let mut r = reminder(
            Repeat::Interval { every_minutes: 60, until: NaiveTime::from_hms_opt(11, 30, 0).unwrap(), workdays_only: true },
            HolidayPolicy::Normal,
        );
        r.time = NaiveTime::from_hms_opt(9, 0, 0).unwrap();
        let c = cal();
        assert_eq!(r.next_after(dt(2026, 3, 6, 9, 0), &c), Some(dt(2026, 3, 6, 10, 0)));
        assert_eq!(r.next_after(dt(2026, 3, 6, 11, 0), &c), Some(dt(2026, 3, 9, 9, 0))); // 11:30 前最後一次是 11:00
        assert_eq!(r.occurrences_between(dt(2026, 3, 6, 0, 0), dt(2026, 3, 9, 23, 0), &c).0, 6);
    }

    #[test]
    fn once_and_disabled() {
        let mut r = reminder(Repeat::Once, HolidayPolicy::Normal);
        r.start_date = d(2026, 5, 5);
        assert_eq!(r.next_after(dt(2026, 5, 1, 0, 0), &cal()), Some(dt(2026, 5, 5, 9, 0)));
        assert_eq!(r.next_after(dt(2026, 5, 5, 9, 0), &cal()), None);
        r.enabled = false;
        assert_eq!(r.next_after(dt(2026, 5, 1, 0, 0), &cal()), None);
    }

    #[test]
    fn preview_marks_skipped_and_moved_days() {
        let skip = reminder(Repeat::Monthly { rule: MonthlyRule::Day { day: 1 } }, HolidayPolicy::Skip);
        // 1/1 國定假日、2/1 與 3/1 是星期日 → 都跳過，第一次響在 4/1（三）
        let items = skip.preview(dt(2025, 12, 31, 0, 0), &cal(), 1);
        assert_eq!(
            items,
            vec![
                PreviewItem::Skipped { date: d(2026, 1, 1), note: Some("開國紀念日".into()) },
                PreviewItem::Skipped { date: d(2026, 2, 1), note: None },
                PreviewItem::Skipped { date: d(2026, 3, 1), note: None },
                PreviewItem::Fire { at: dt(2026, 4, 1, 9, 0), moved_from: None, note: None },
            ]
        );

        let next = reminder(Repeat::Monthly { rule: MonthlyRule::Day { day: 1 } }, HolidayPolicy::NextWorkday);
        let items = next.preview(dt(2025, 12, 31, 0, 0), &cal(), 1);
        assert_eq!(items[0], PreviewItem::Fire { at: dt(2026, 1, 2, 9, 0), moved_from: Some(d(2026, 1, 1)), note: Some("開國紀念日".into()) });
    }

    #[test]
    fn json_round_trip() {
        let json = r#"{
            "reminders": [{
                "id": "a", "title": "繳卡費", "time": "10:30", "startDate": "2026-01-01",
                "repeat": { "kind": "monthly", "rule": { "kind": "day", "day": 25 } },
                "holiday": "prevWorkday"
            }, {
                "id": "b", "title": "喝水", "time": "09:00", "startDate": "2026-01-01",
                "repeat": { "kind": "interval", "everyMinutes": 60, "until": "18:00", "workdaysOnly": true }
            }],
            "dayOverrides": { "2026-07-29": "holiday" }
        }"#;
        let file: ReminderFile = serde_json::from_str(json).unwrap();
        assert_eq!(file.reminders[0].holiday, HolidayPolicy::PrevWorkday);
        assert!(file.reminders[1].enabled);
        assert_eq!(file.day_overrides[&d(2026, 7, 29)], DayKind::Holiday);
        let again: ReminderFile = serde_json::from_str(&serde_json::to_string(&file).unwrap()).unwrap();
        assert_eq!(again, file);
    }
}
