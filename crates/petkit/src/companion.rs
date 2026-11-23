//! 陪伴天數：每隻桌寵各自記「你們一起過了幾天」（`%APPDATA%\youban-desktop-pet\companion.json`）。
//! 那天使用者有在電腦前、桌寵在桌面上就算一天。核心寫、設定程式只讀。只是紀念，不鎖任何東西。

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::config::{load_json, write_atomic};

/// 這幾天桌寵會特別紀念一下；之後每滿一年也是。
const MILESTONES: [u32; 6] = [7, 30, 100, 200, 365, 500];
const DAYS_PER_YEAR: u32 = 365;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Companion {
    pub days: u32,
    /// 上一次算進去的是哪一天（同一天不重複算）。
    pub last_day: Option<NaiveDate>,
    /// 第一次見面那天。
    pub since: Option<NaiveDate>,
}

impl Companion {
    /// 今天見到了；回傳 `true` = 這是新的一天（天數剛加一）。
    pub fn visit(&mut self, today: NaiveDate) -> bool {
        if self.last_day == Some(today) {
            return false;
        }
        self.days += 1;
        self.last_day = Some(today);
        self.since.get_or_insert(today);
        true
    }
}

pub fn is_milestone(days: u32) -> bool {
    MILESTONES.contains(&days) || (days > DAYS_PER_YEAR && days.is_multiple_of(DAYS_PER_YEAR))
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CompanionFile {
    /// 桌寵 id → 陪伴天數。
    pub pets: BTreeMap<String, Companion>,
}

impl CompanionFile {
    pub fn load(path: &Path) -> io::Result<Self> {
        load_json(path)
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        write_atomic(path, &serde_json::to_vec_pretty(self).map_err(io::Error::other)?)
    }

    pub fn days(&self, pet: &str) -> u32 {
        self.pets.get(pet).map_or(0, |c| c.days)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
    }

    #[test]
    fn counts_each_day_once_and_remembers_the_first() {
        let mut c = Companion::default();
        assert!(c.visit(day(1)));
        assert!(!c.visit(day(1)), "same day again");
        assert!(c.visit(day(5)), "days apart still count as one more day together");
        assert_eq!((c.days, c.since), (2, Some(day(1))));
    }

    #[test]
    fn milestones() {
        assert!(is_milestone(7) && is_milestone(100) && is_milestone(730));
        assert!(!is_milestone(8) && !is_milestone(366));
    }
}
