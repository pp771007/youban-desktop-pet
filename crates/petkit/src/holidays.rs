//! 上班日／放假日判斷。資料來源：人事行政總處「中華民國政府行政機關辦公日曆表」
//! （政府開放資料 data.gov.tw 14718，每年 6～7 月公布隔年）。
//!
//! 判斷順序：使用者手動覆寫（颱風假、請假、補班之類自己的特別日子）> 官方日曆 > 週末放假。
//! 官方日曆會把補班的星期六標成上班日，所以「工作日」提醒在補班日照樣響。

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::Path;

use chrono::{Datelike, Days, NaiveDate, Weekday};
use serde::{Deserialize, Serialize};

/// 隨程式附帶的官方日曆（沒網路也能用）；更新的版本由核心程式每月下載蓋過去。
pub const BUNDLED: &[(i32, &str)] = &[
    (2026, include_str!("../../../data/holidays/2026.csv")),
    (2027, include_str!("../../../data/holidays/2027.csv")),
];

/// CSV「是否放假」欄位：0 = 上班、2 = 放假。
const DGPA_WORKDAY: &str = "0";
const DGPA_HOLIDAY: &str = "2";
const DGPA_DATE_FORMAT: &str = "%Y%m%d";
/// 找下一個／上一個上班日時最多看幾天；連假最長也不會超過兩週。
const MAX_WORKDAY_SEARCH_DAYS: u64 = 31;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DayKind {
    Workday,
    Holiday,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarDay {
    pub date: NaiveDate,
    pub kind: DayKind,
    pub note: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub content: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "辦公日曆第 {} 行看不懂：{}", self.line, self.content)
    }
}

impl std::error::Error for ParseError {}

/// 解析人事總處的 CSV（`西元日期,星期,是否放假,備註`），會略過 UTF-8 BOM 與標題列。
pub fn parse_dgpa_csv(text: &str) -> Result<Vec<CalendarDay>, ParseError> {
    let text = text.trim_start_matches('\u{feff}');
    let mut days = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || i == 0 && !line.starts_with(|c: char| c.is_ascii_digit()) {
            continue;
        }
        let bad = || ParseError { line: i + 1, content: line.to_owned() };
        let mut cols = line.splitn(4, ',');
        let date = cols.next().and_then(|d| NaiveDate::parse_from_str(d.trim(), DGPA_DATE_FORMAT).ok()).ok_or_else(bad)?;
        let _weekday = cols.next().ok_or_else(bad)?;
        let kind = match cols.next().map(str::trim) {
            Some(DGPA_WORKDAY) => DayKind::Workday,
            Some(DGPA_HOLIDAY) => DayKind::Holiday,
            _ => return Err(bad()),
        };
        let note = cols.next().unwrap_or_default().trim().to_owned();
        days.push(CalendarDay { date, kind, note });
    }
    Ok(days)
}

/// 沒有官方日曆時的規則：週末放假、平日上班。
fn weekend_rule(date: NaiveDate) -> DayKind {
    match date.weekday() {
        Weekday::Sat | Weekday::Sun => DayKind::Holiday,
        _ => DayKind::Workday,
    }
}

#[derive(Debug, Default, Clone)]
pub struct Calendar {
    official: BTreeMap<NaiveDate, CalendarDay>,
    overrides: BTreeMap<NaiveDate, DayKind>,
    years: BTreeSet<i32>,
}

impl Calendar {
    pub fn add_official(&mut self, days: Vec<CalendarDay>) {
        for d in days {
            self.years.insert(d.date.year());
            self.official.insert(d.date, d);
        }
    }

    pub fn set_overrides(&mut self, overrides: BTreeMap<NaiveDate, DayKind>) {
        self.overrides = overrides;
    }

    /// 這一年有沒有官方日曆；沒有的話只能用「週末放假」猜。
    pub fn has_year(&self, year: i32) -> bool {
        self.years.contains(&year)
    }

    pub fn kind(&self, date: NaiveDate) -> DayKind {
        if let Some(k) = self.overrides.get(&date) {
            return *k;
        }
        self.official.get(&date).map_or_else(|| weekend_rule(date), |d| d.kind)
    }

    /// `from` 當天起，官方日曆跟「週末放假」不一樣的日子（平日放的國定假日、要上班的週末），照日期排。
    /// 設定程式列給使用者一天一天開關（關掉 = 覆寫成週末規則那樣）。
    pub fn official_changes(&self, from: NaiveDate) -> impl Iterator<Item = &CalendarDay> {
        self.official.range(from..).map(|(_, d)| d).filter(|d| d.kind != weekend_rule(d.date))
    }

    pub fn is_workday(&self, date: NaiveDate) -> bool {
        self.kind(date) == DayKind::Workday
    }

    /// 這一天放假的原因（國定假日名稱）；週末或沒有註記回傳 `None`。
    pub fn note(&self, date: NaiveDate) -> Option<&str> {
        self.official.get(&date).map(|d| d.note.as_str()).filter(|n| !n.is_empty())
    }

    /// `date` 當天或之後的第一個上班日。
    pub fn workday_on_or_after(&self, date: NaiveDate) -> Option<NaiveDate> {
        (0..MAX_WORKDAY_SEARCH_DAYS).filter_map(|i| date.checked_add_days(Days::new(i))).find(|d| self.is_workday(*d))
    }

    /// `date` 當天或之前的最後一個上班日。
    pub fn workday_on_or_before(&self, date: NaiveDate) -> Option<NaiveDate> {
        (0..MAX_WORKDAY_SEARCH_DAYS).filter_map(|i| date.checked_sub_days(Days::new(i))).find(|d| self.is_workday(*d))
    }
}

/// 內建日曆 ＋ 下載回來的較新版本（`<cache_dir>/<西元年>.csv`，同一年以下載的為準）＋ 手動覆寫。
/// 回傳日曆與讀不懂的檔案清單（給呼叫端寫 log；壞掉的那一年退回週末規則）。
pub fn load_calendar(cache_dir: &Path, overrides: BTreeMap<NaiveDate, DayKind>) -> (Calendar, Vec<String>) {
    let mut years: BTreeMap<i32, String> = BUNDLED.iter().map(|(y, t)| (*y, t.to_string())).collect();
    if let Ok(entries) = fs::read_dir(cache_dir) {
        for e in entries.flatten() {
            let path = e.path();
            let year = path.file_stem().and_then(|s| s.to_str()).and_then(|s| s.parse::<i32>().ok());
            if let (Some(year), Ok(text)) = (year, fs::read_to_string(&path)) {
                years.insert(year, text);
            }
        }
    }
    let mut cal = Calendar::default();
    let mut problems = Vec::new();
    for (year, text) in years {
        match parse_dgpa_csv(&text) {
            Ok(days) => cal.add_official(days),
            Err(e) => problems.push(format!("{year}: {e}")),
        }
    }
    cal.set_overrides(overrides);
    (cal, problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    const SAMPLE: &str = "\u{feff}西元日期,星期,是否放假,備註\r\n20230102,一,2,補假\r\n20230107,六,0,補行上班\r\n20230108,日,2,\r\n";

    #[test]
    fn parses_official_csv_with_bom_and_crlf() {
        let days = parse_dgpa_csv(SAMPLE).unwrap();
        assert_eq!(days.len(), 3);
        assert_eq!(days[0], CalendarDay { date: d(2023, 1, 2), kind: DayKind::Holiday, note: "補假".into() });
        assert_eq!(days[1].kind, DayKind::Workday);
    }

    #[test]
    fn bad_row_reports_line_number() {
        let err = parse_dgpa_csv("西元日期,星期,是否放假,備註\n20230102,一,9,\n").unwrap_err();
        assert_eq!(err.line, 2);
    }

    #[test]
    fn makeup_saturday_is_a_workday_and_override_wins() {
        let mut cal = Calendar::default();
        cal.add_official(parse_dgpa_csv(SAMPLE).unwrap());
        assert!(!cal.is_workday(d(2023, 1, 2)), "weekday holiday");
        assert!(cal.is_workday(d(2023, 1, 7)), "make-up Saturday");
        assert!(cal.is_workday(d(2023, 1, 3)), "ordinary Tuesday");
        assert!(!cal.is_workday(d(2023, 1, 14)), "ordinary Saturday");
        cal.set_overrides(BTreeMap::from([(d(2023, 1, 3), DayKind::Holiday)])); // 颱風假
        assert!(!cal.is_workday(d(2023, 1, 3)));
        assert_eq!(cal.workday_on_or_after(d(2023, 1, 2)), Some(d(2023, 1, 4)));
        assert_eq!(cal.workday_on_or_before(d(2023, 1, 8)), Some(d(2023, 1, 7)));
    }

    #[test]
    fn lists_only_days_that_differ_from_the_weekend_rule_from_a_date_on() {
        let mut cal = Calendar::default();
        cal.add_official(parse_dgpa_csv(SAMPLE).unwrap());
        let dates = |from| cal.official_changes(from).map(|d| d.date).collect::<Vec<_>>();
        // 1/8 是本來就放的星期日，不列。
        assert_eq!(dates(d(2023, 1, 1)), vec![d(2023, 1, 2), d(2023, 1, 7)]);
        assert_eq!(dates(d(2023, 1, 3)), vec![d(2023, 1, 7)]);
    }

    #[test]
    fn bundled_calendars_parse() {
        for &(year, text) in BUNDLED {
            let days = parse_dgpa_csv(text).unwrap();
            assert!(days.len() >= 365, "{year}");
            let mut cal = Calendar::default();
            cal.add_official(days);
            assert!(cal.has_year(year));
            assert!(!cal.is_workday(d(year, 1, 1)), "{year} 開國紀念日");
        }
    }
}
