//! 傳話紀錄（用詞見 `CONTEXT.md`）：傳過來的話留在 `relay-log.json`，框收掉之後在設定程式裡還找得到。
//!
//! 只有核心寫這個檔；設定程式只讀，要改（按完成、清除、看過了）就送指令給核心，兩邊才不會互相蓋掉。

use std::io;
use std::path::Path;

use chrono::{Duration, NaiveDateTime};
use serde::{Deserialize, Serialize};

use crate::config::{load_json, write_atomic};
use crate::reminders::Style;

/// 留幾天。還沒按完成的確認框不算在內：框還在等使用者按，紀錄裡的「完成」也要在。
pub const KEEP_DAYS: i64 = 7;
/// 最多留幾則，超過先刪最舊的（一樣不刪還沒按完成的確認框）。
pub const MAX_ENTRIES: usize = 200;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    /// 跟核心卡片代號一樣：設定程式按「完成」時靠它認是哪一則。
    pub key: String,
    pub title: String,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub from: Option<String>,
    pub at: NaiveDateTime,
    pub style: Style,
    #[serde(default)]
    pub done: bool,
}

impl LogEntry {
    pub fn waiting_for_done(&self) -> bool {
        self.style == Style::Confirm && !self.done
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RelayLog {
    /// 舊的在前。
    pub entries: Vec<LogEntry>,
    /// 上次打開傳話紀錄的時間：比它晚進來的算還沒看過。
    pub seen_until: Option<NaiveDateTime>,
}

impl RelayLog {
    pub fn load(path: &Path) -> io::Result<Self> {
        load_json(path)
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        write_atomic(
            path,
            &serde_json::to_vec_pretty(self).map_err(io::Error::other)?,
        )
    }

    /// 同一個代號的（同一個來源、同一件事）換成新的這則，排到最後面。
    pub fn record(&mut self, entry: LogEntry) {
        self.entries.retain(|e| e.key != entry.key);
        let now = entry.at;
        self.entries.push(entry);
        self.prune(now);
    }

    pub fn mark_done(&mut self, key: &str) {
        for e in self.entries.iter_mut().filter(|e| e.key == key) {
            e.done = true;
        }
    }

    /// `from`：`None` = 全部，`Some(來源)` = 那一組（`Some(None)` 是沒署名的那組）。還沒按完成的確認框留著。
    pub fn clear(&mut self, from: Option<Option<&str>>) {
        self.entries
            .retain(|e| e.waiting_for_done() || from.is_some_and(|f| e.from.as_deref() != f));
    }

    pub fn unread(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| self.seen_until.is_none_or(|seen| e.at > seen))
            .count()
    }

    fn prune(&mut self, now: NaiveDateTime) {
        let cutoff = now - Duration::days(KEEP_DAYS);
        self.entries
            .retain(|e| e.waiting_for_done() || e.at >= cutoff);
        let mut extra = self.entries.len().saturating_sub(MAX_ENTRIES);
        self.entries.retain(|e| {
            if extra > 0 && !e.waiting_for_done() {
                extra -= 1;
                false
            } else {
                true
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn dt(d: u32, h: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, d)
            .unwrap()
            .and_hms_opt(h, 0, 0)
            .unwrap()
    }

    fn entry(key: &str, from: Option<&str>, style: Style, at: NaiveDateTime) -> LogEntry {
        LogEntry {
            key: key.into(),
            title: key.into(),
            detail: None,
            from: from.map(Into::into),
            at,
            style,
            done: false,
        }
    }

    fn keys(log: &RelayLog) -> Vec<&str> {
        log.entries.iter().map(|e| e.key.as_str()).collect()
    }

    #[test]
    fn old_entries_go_away_but_unfinished_confirms_stay() {
        let mut log = RelayLog::default();
        log.record(entry("old-notice", None, Style::Notice, dt(1, 9)));
        log.record(entry("old-confirm", None, Style::Confirm, dt(1, 9)));
        log.record(entry("new", None, Style::Notice, dt(20, 9)));
        assert_eq!(keys(&log), ["old-confirm", "new"]);
        for i in 0..MAX_ENTRIES {
            log.record(entry(&format!("n{i}"), None, Style::Notice, dt(20, 10)));
        }
        // 超過的兩則刪最舊的 "new"、"n0"，沒按完成的確認框留著。
        assert_eq!(log.entries.len(), MAX_ENTRIES);
        assert_eq!(keys(&log)[..2], ["old-confirm", "n1"]);
    }

    #[test]
    fn clearing_a_sender_keeps_others_and_unfinished_confirms() {
        let mut log = RelayLog::default();
        log.record(entry("a", Some("Claude"), Style::Notice, dt(20, 9)));
        log.record(entry("b", Some("Claude"), Style::Confirm, dt(20, 9)));
        log.record(entry("c", Some("Codex"), Style::Notice, dt(20, 9)));
        log.record(entry("d", None, Style::Notice, dt(20, 9)));
        log.clear(Some(Some("Claude")));
        assert_eq!(keys(&log), ["b", "c", "d"]);
        log.clear(Some(None));
        assert_eq!(keys(&log), ["b", "c"]);
        log.mark_done("b");
        log.clear(None);
        assert!(log.entries.is_empty());
    }

    #[test]
    fn unread_counts_what_came_after_the_last_look() {
        let mut log = RelayLog::default();
        log.record(entry("a", None, Style::Notice, dt(20, 9)));
        assert_eq!(log.unread(), 1);
        log.seen_until = Some(dt(20, 9));
        log.record(entry("b", None, Style::Notice, dt(20, 10)));
        assert_eq!(log.unread(), 1);
    }
}
