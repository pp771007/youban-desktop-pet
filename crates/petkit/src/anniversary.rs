//! 紀念日：每年同一天（國曆或農曆），一筆可以有好幾個「提前幾天、幾點」的提醒。
//!
//! 對不上的日子照民間「生日緊前不緊後」：國曆 2/29 在平年是 2/28；農曆三十遇到小月是廿九；
//! 閏月的日子在沒有那個閏月的年份，過同名的平常月份（閏六月初十 → 六月初十）。

use chinese_lunisolar_calendar::{LunarMonth, LunisolarDate, LunisolarYear, SolarYear};
use chrono::{Datelike, Days, NaiveDate, NaiveDateTime, NaiveTime};
use serde::{Deserialize, Serialize};

/// 農曆月份、日子的寫法（設定程式的選單與桌寵的卡片共用）。
pub const LUNAR_MONTHS: [&str; 12] = [
    "正月",
    "二月",
    "三月",
    "四月",
    "五月",
    "六月",
    "七月",
    "八月",
    "九月",
    "十月",
    "十一月",
    "十二月",
];
pub const LUNAR_DAYS: [&str; 30] = [
    "初一", "初二", "初三", "初四", "初五", "初六", "初七", "初八", "初九", "初十", "十一", "十二",
    "十三", "十四", "十五", "十六", "十七", "十八", "十九", "二十", "廿一", "廿二", "廿三", "廿四",
    "廿五", "廿六", "廿七", "廿八", "廿九", "三十",
];
/// 往後找下一次時看幾年（農曆的十二月落在隔年國曆，要多看一年）。
const LOOKAHEAD_YEARS: i32 = 2;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Anniversary {
    pub id: String,
    pub title: String,
    #[serde(default = "enabled_default")]
    pub enabled: bool,
    pub date: AnnualDate,
    /// 那一年（國曆年；農曆的是那個農曆年）：有填才算得出「滿幾年」。
    #[serde(default)]
    pub year: Option<i32>,
    pub alerts: Vec<Alert>,
}

fn enabled_default() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "calendar", rename_all = "camelCase")]
pub enum AnnualDate {
    Solar {
        month: u32,
        day: u32,
    },
    Lunar {
        month: u8,
        day: u8,
        #[serde(default)]
        leap: bool,
    },
}

/// 提前 `days_before` 天的 `time` 跳卡片（0 = 當天）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Alert {
    pub days_before: u32,
    #[serde(with = "crate::reminders::hhmm")]
    pub time: NaiveTime,
}

/// 某一年的那一天，以及那是第幾年（沒填年份是 `None`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Occurrence {
    pub date: NaiveDate,
    pub nth: Option<i32>,
}

impl AnnualDate {
    /// `year` 那一年的那一天（農曆的 `year` 是農曆年）；超出農曆資料範圍是 `None`。
    pub fn in_year(&self, year: i32) -> Option<NaiveDate> {
        match *self {
            AnnualDate::Solar { month, day } => {
                NaiveDate::from_ymd_opt(year, month, day).or_else(|| {
                    (month == 2 && day == 29)
                        .then(|| NaiveDate::from_ymd_opt(year, 2, 28))
                        .flatten()
                })
            }
            AnnualDate::Lunar { month, day, leap } => {
                let solar_year = SolarYear::from_u16(u16::try_from(year).ok()?);
                let lunisolar_year = LunisolarYear::from_solar_year(solar_year).ok()?;
                let leap = leap
                    && lunisolar_year
                        .get_leap_lunar_month()
                        .is_some_and(|m| m.to_u8() == month);
                let lunar_month = LunarMonth::from_u8_with_leap(month, leap).ok()?;
                let day = day.min(lunisolar_year.get_total_days_in_a_month(lunar_month)?);
                Some(
                    LunisolarDate::from_ymd(solar_year.to_u16(), month, leap, day)
                        .ok()?
                        .to_naive_date(),
                )
            }
        }
    }

    /// 「國曆 4/20」「農曆閏六月初十」。
    pub fn label(&self) -> String {
        match *self {
            AnnualDate::Solar { month, day } => format!("國曆 {month}/{day}"),
            AnnualDate::Lunar { month, day, leap } => {
                let m = LUNAR_MONTHS
                    .get(usize::from(month).wrapping_sub(1))
                    .copied()
                    .unwrap_or_default();
                let d = LUNAR_DAYS
                    .get(usize::from(day).wrapping_sub(1))
                    .copied()
                    .unwrap_or_default();
                format!("農曆{}{m}{d}", if leap { "閏" } else { "" })
            }
        }
    }
}

impl Anniversary {
    /// `from` 當天或之後的下一次。
    pub fn next_on_or_after(&self, from: NaiveDate) -> Option<Occurrence> {
        (from.year() - 1..=from.year() + LOOKAHEAD_YEARS)
            .filter(|y| self.year.is_none_or(|start| *y >= start))
            .filter_map(|y| {
                Some(Occurrence {
                    date: self.date.in_year(y)?,
                    nth: self.year.map(|start| y - start),
                })
            })
            .filter(|o| o.date >= from)
            .min_by_key(|o| o.date)
    }

    /// 從 `from` 起的接下來 `count` 次。
    pub fn upcoming(&self, from: NaiveDate, count: usize) -> Vec<Occurrence> {
        let mut out = Vec::with_capacity(count);
        let mut cursor = from;
        while out.len() < count {
            let Some(o) = self.next_on_or_after(cursor) else {
                break;
            };
            cursor = o.date.succ_opt().unwrap_or(o.date);
            out.push(o);
        }
        out
    }

    /// `after` 之後（不含）下一次要跳卡片的時間。
    pub fn next_alert_after(&self, after: NaiveDateTime) -> Option<NaiveDateTime> {
        if !self.enabled {
            return None;
        }
        // 這一次的提醒可能都過了（例如當天下午才打開），所以連下一次一起看。
        self.upcoming(after.date(), 2)
            .into_iter()
            .flat_map(|o| {
                self.alerts.iter().filter_map(move |a| {
                    Some(
                        o.date
                            .checked_sub_days(Days::new(u64::from(a.days_before)))?
                            .and_time(a.time),
                    )
                })
            })
            .filter(|at| *at > after)
            .min()
    }

    /// `(from, to]` 之間錯過了幾次、最後一次是什麼時候。
    pub fn alerts_between(
        &self,
        from: NaiveDateTime,
        to: NaiveDateTime,
    ) -> (usize, Option<NaiveDateTime>) {
        let mut count = 0;
        let mut last = None;
        let mut cursor = from;
        while let Some(at) = self.next_alert_after(cursor).filter(|at| *at <= to) {
            count += 1;
            last = Some(at);
            cursor = at;
        }
        (count, last)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn at(y: i32, m: u32, day: u32, h: u32) -> NaiveDateTime {
        d(y, m, day).and_hms_opt(h, 0, 0).unwrap()
    }

    fn lunar(month: u8, day: u8) -> AnnualDate {
        AnnualDate::Lunar {
            month,
            day,
            leap: false,
        }
    }

    fn anniversary(date: AnnualDate, alerts: &[(u32, u32)]) -> Anniversary {
        Anniversary {
            id: "a".into(),
            title: "媽媽生日".into(),
            enabled: true,
            date,
            year: None,
            alerts: alerts
                .iter()
                .map(|&(days_before, h)| Alert {
                    days_before,
                    time: NaiveTime::from_hms_opt(h, 0, 0).unwrap(),
                })
                .collect(),
        }
    }

    #[test]
    fn lunar_dates_match_the_official_calendar() {
        // 人事行政總處的辦公日曆（data/holidays）：春節初一、端午、中秋、除夕。
        assert_eq!(lunar(1, 1).in_year(2026), Some(d(2026, 2, 17)));
        assert_eq!(lunar(1, 1).in_year(2027), Some(d(2027, 2, 6)));
        assert_eq!(lunar(5, 5).in_year(2026), Some(d(2026, 6, 19)));
        assert_eq!(lunar(5, 5).in_year(2027), Some(d(2027, 6, 9)));
        assert_eq!(lunar(8, 15).in_year(2026), Some(d(2026, 9, 25)));
        assert_eq!(lunar(8, 15).in_year(2027), Some(d(2027, 9, 15)));
        // 十二月三十：小月就是廿九 —— 兩年都正好落在官方日曆的除夕。
        assert_eq!(lunar(12, 30).in_year(2025), Some(d(2026, 2, 16)));
        assert_eq!(lunar(12, 30).in_year(2026), Some(d(2027, 2, 5)));
    }

    #[test]
    fn days_that_do_not_exist_that_year_come_a_little_early() {
        let feb29 = AnnualDate::Solar { month: 2, day: 29 };
        assert_eq!(
            (feb29.in_year(2028), feb29.in_year(2027)),
            (Some(d(2028, 2, 29)), Some(d(2027, 2, 28)))
        );
        // 2025 年有閏六月：那年過閏六月那天（在六月那天之後一個月），2026 年沒有閏月就過六月。
        let leap = AnnualDate::Lunar {
            month: 6,
            day: 10,
            leap: true,
        };
        let (in_leap, normal) = (
            leap.in_year(2025).unwrap(),
            lunar(6, 10).in_year(2025).unwrap(),
        );
        assert!(
            (29..=30).contains(&(in_leap - normal).num_days()),
            "{normal} → {in_leap}"
        );
        assert_eq!(leap.in_year(2026), lunar(6, 10).in_year(2026));
        assert_eq!(leap.label(), "農曆閏六月初十");
        assert_eq!(AnnualDate::Solar { month: 4, day: 20 }.label(), "國曆 4/20");
    }

    #[test]
    fn next_occurrence_counts_years_and_crosses_into_next_gregorian_year() {
        let mut a = anniversary(lunar(12, 20), &[]);
        a.year = Some(1960);
        // 農曆 2025 年的十二月二十是國曆 2026 年 2 月：一月初看下一次，要找到這一天。
        let next = a.next_on_or_after(d(2026, 1, 1)).unwrap();
        assert_eq!(
            next,
            Occurrence {
                date: lunar(12, 20).in_year(2025).unwrap(),
                nth: Some(65)
            }
        );
        assert_eq!(next.date.year(), 2026);
    }

    #[test]
    fn several_alerts_each_at_their_own_time() {
        let a = anniversary(AnnualDate::Solar { month: 4, day: 20 }, &[(3, 9), (0, 8)]);
        assert_eq!(
            a.next_alert_after(at(2026, 4, 16, 10)),
            Some(at(2026, 4, 17, 9))
        );
        assert_eq!(
            a.next_alert_after(at(2026, 4, 17, 9)),
            Some(at(2026, 4, 20, 8))
        );
        assert_eq!(
            a.next_alert_after(at(2026, 4, 20, 8)),
            Some(at(2027, 4, 17, 9)),
            "this year's are done: next year's first"
        );
        assert_eq!(
            a.alerts_between(at(2026, 4, 1, 0), at(2026, 4, 30, 0)),
            (2, Some(at(2026, 4, 20, 8)))
        );
        assert_eq!(
            Anniversary {
                enabled: false,
                ..a
            }
            .next_alert_after(at(2026, 4, 16, 10)),
            None
        );
    }
}
