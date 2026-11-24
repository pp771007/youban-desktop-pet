//! 記憶體看門狗：定時把私有記憶體與工作集寫進 `logs/memory.csv`，用來畫長時間的記憶體曲線；
//! 最近 24 小時一路往上漲就回報，讓核心冒泡提醒並存診斷檔（`docs/PLAN.md` 第 3 節第 4 點）。

use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use windows::Win32::System::ProcessStatus::{
    GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
};
use windows::Win32::System::Threading::GetCurrentProcess;

const CSV_FILE: &str = "memory.csv";
const CSV_HEADER: &str = "unix_secs,private_bytes,working_set_bytes\n";
const DEFAULT_INTERVAL_SECS: u64 = 600;
/// 每 10 分鐘一行約 40 bytes，1MB 大約是半年份。
const MAX_CSV_BYTES: u64 = 1024 * 1024;
/// 壓力測試時縮短取樣間隔用。
const INTERVAL_ENV: &str = "YOUBAN_DESKTOP_PET_MEM_INTERVAL_SECS";
/// 看多長的趨勢：24 小時份的取樣（每 10 分鐘一筆）。
pub const TREND_SAMPLES: usize = 144;
/// 把這段切成幾塊比中位數：每一塊都比前一塊高，才算「一路往上」（中間持平或降回來就是正常擺盪）。
const TREND_BLOCKS: usize = 6;
/// 而且頭尾差超過這麼多才回報（平常用量約 7～20MB）。
const LEAK_ALERT_BYTES: usize = 10 * 1024 * 1024;

pub struct MemWatch {
    path: PathBuf,
    interval: Duration,
    next: Instant,
    recent: VecDeque<usize>,
}

/// 記憶體一路往上漲：從多少漲到多少（各取頭尾那一塊的中位數），以及這段的每一筆取樣。
#[derive(Debug, Clone, PartialEq)]
pub struct Leak {
    pub from_bytes: usize,
    pub to_bytes: usize,
    pub samples: Vec<usize>,
}

#[derive(Debug, Clone, Copy)]
pub struct Sample {
    pub private_bytes: usize,
    pub working_set: usize,
}

pub fn sample() -> Sample {
    let mut c = PROCESS_MEMORY_COUNTERS_EX {
        cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        ..Default::default()
    };
    unsafe {
        let _ = GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut c as *mut _ as *mut PROCESS_MEMORY_COUNTERS,
            c.cb,
        );
    }
    Sample {
        private_bytes: c.PrivateUsage,
        working_set: c.WorkingSetSize,
    }
}

impl MemWatch {
    pub fn new(now: Instant) -> Self {
        let secs = std::env::var(INTERVAL_ENV)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_INTERVAL_SECS);
        Self {
            path: crate::paths::logs_dir().join(CSV_FILE),
            interval: Duration::from_secs(secs.max(1)),
            next: now,
            recent: VecDeque::with_capacity(TREND_SAMPLES),
        }
    }

    /// 到了取樣時間就記一筆；最近 24 小時一路往上漲時回傳 `Leak`（回報一次就重新累積，不會一直響）。
    pub fn tick(&mut self, now: Instant) -> Option<Leak> {
        if now < self.next {
            return None;
        }
        self.next = now + self.interval;
        let s = sample();
        self.write_csv(s);
        self.record(s.private_bytes)
    }

    fn record(&mut self, private_bytes: usize) -> Option<Leak> {
        if self.recent.len() == TREND_SAMPLES {
            self.recent.pop_front();
        }
        self.recent.push_back(private_bytes);
        let samples: Vec<usize> = self.recent.iter().copied().collect();
        let leak = rising(&samples)?;
        self.recent.clear();
        Some(leak)
    }

    fn write_csv(&self, s: Sample) {
        let unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if fs::metadata(&self.path)
            .map(|m| m.len() > MAX_CSV_BYTES)
            .unwrap_or(false)
        {
            let _ = fs::rename(&self.path, self.path.with_extension("csv.old"));
        }
        let new_file = !self.path.exists();
        if let Some(dir) = self.path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        if let Ok(mut f) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            if new_file {
                let _ = f.write_all(CSV_HEADER.as_bytes());
            }
            let _ = writeln!(f, "{unix},{},{}", s.private_bytes, s.working_set);
        }
    }
}

/// 滿 24 小時份、每一塊的中位數都比前一塊高、頭尾差夠大 → 在漏。
fn rising(samples: &[usize]) -> Option<Leak> {
    if samples.len() < TREND_SAMPLES {
        return None;
    }
    let medians: Vec<usize> = samples
        .chunks(samples.len() / TREND_BLOCKS)
        .take(TREND_BLOCKS)
        .map(|block| {
            let mut b = block.to_vec();
            b.sort_unstable();
            b[b.len() / 2]
        })
        .collect();
    // 嚴格往上：平平的一段最後突然變高（剛好在忙）不算漏。
    let climbing = medians.windows(2).all(|w| w[1] > w[0]);
    let (from, to) = (medians[0], medians[TREND_BLOCKS - 1]);
    (climbing && to >= from + LEAK_ALERT_BYTES).then(|| Leak {
        from_bytes: from,
        to_bytes: to,
        samples: samples.to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MB: usize = 1024 * 1024;

    fn watch() -> MemWatch {
        MemWatch {
            path: PathBuf::new(),
            interval: Duration::from_secs(1),
            next: Instant::now(),
            recent: VecDeque::new(),
        }
    }

    #[test]
    fn steady_climb_over_a_day_is_reported_once() {
        let mut w = watch();
        let mut reports = 0;
        for i in 0..TREND_SAMPLES * 2 {
            // 一天漲 20MB，加上上下 2MB 的擺盪。
            let wobble = if i % 3 == 0 { 2 * MB } else { 0 };
            if w.record(15 * MB + i * 20 * MB / TREND_SAMPLES + wobble)
                .is_some()
            {
                reports += 1;
            }
        }
        assert_eq!(
            reports, 2,
            "once per full day of climbing, not every sample"
        );
    }

    #[test]
    fn normal_swings_are_not_a_leak() {
        let mut w = watch();
        for i in 0..TREND_SAMPLES * 3 {
            // 用量在 16～22MB 之間來回（開提醒卡、換桌寵），而且有一段時間偏高後又降回來。
            let busy = if (TREND_SAMPLES..TREND_SAMPLES + 30).contains(&i) {
                12 * MB
            } else {
                0
            };
            let swing = [16, 22, 18, 20][i % 4] * MB;
            assert!(w.record(swing + busy).is_none(), "sample {i}");
        }
    }
}
