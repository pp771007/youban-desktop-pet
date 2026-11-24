//! 專注進行到哪：核心寫、設定程式讀（設定的專注頁要跟右鍵選單一樣顯示「停止專注」）；
//! 核心重開時也從這裡接著倒數。
//! 選單與設定的按鈕文字都從這裡出，兩邊才不會各寫各的。

use std::{fs, io, path::Path};

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

use crate::config::write_atomic;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FocusStatus {
    /// `true` = 專注中，`false` = 休息中。
    pub focusing: bool,
    /// 這一段在什麼時候結束（本機時間）。
    pub until: NaiveDateTime,
    /// 第幾輪：核心重開時從這裡接著算，長休息才不會跑掉。
    #[serde(default = "first_round")]
    pub round: u32,
}

fn first_round() -> u32 {
    1
}

impl FocusStatus {
    /// 檔案不在、讀不懂、這一段已經過了，都當成沒有在專注。
    pub fn load(path: &Path, now: NaiveDateTime) -> Option<Self> {
        let s: Self = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
        (s.until > now).then_some(s)
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        write_atomic(
            path,
            serde_json::to_string(self)
                .map_err(io::Error::other)?
                .as_bytes(),
        )
    }

    /// 沒在專注：把檔案拿掉。
    pub fn clear(path: &Path) -> io::Result<()> {
        match fs::remove_file(path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    pub fn secs_left(&self, now: NaiveDateTime) -> i64 {
        (self.until - now).num_seconds().max(0)
    }

    /// 無條件進位的分鐘（還剩 30 秒算 1 分）。
    pub fn minutes_left(&self, now: NaiveDateTime) -> i64 {
        (self.secs_left(now) + 59) / 60
    }

    pub fn stop_label(&self, now: NaiveDateTime) -> String {
        format!(
            "停止專注（{}還剩 {} 分）",
            if self.focusing { "" } else { "休息" },
            self.minutes_left(now)
        )
    }
}

pub fn start_label(focus_minutes: u32) -> String {
    format!("開始專注（{focus_minutes} 分）")
}
