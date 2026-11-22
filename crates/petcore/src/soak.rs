//! `Youban.exe --soak <秒數>`：記憶體壓力測試。用真的程式、真的視窗，自動且快速地重複
//! 換桌寵、出提醒卡、按按鈕、拖曳放開，每秒記一次私有記憶體；暖機後的曲線要是平的。
//! 結果寫到 `logs/soak.csv` 與 `soak-result.json`，跑完自己結束。

use std::fs;
use std::io::Write;

use petkit::config::write_atomic;
use serde::Serialize;

use windows::Win32::System::Threading::{GR_GDIOBJECTS, GR_USEROBJECTS, GetCurrentProcess, GetGuiResources, GetProcessHandleCount};

use crate::log;
use crate::memwatch;
use crate::paths;

/// 只跑某一類動作（找是哪一類在漲用）：`switch`、`cards`、`drag`；沒設就全跑。
const ONLY_ENV: &str = "YOUBAN_DESKTOP_PET_SOAK_ONLY";

/// 每隔多久做一個動作。
const ACTION_EVERY_SECS: f64 = 0.15;
const SAMPLE_EVERY_SECS: f64 = 1.0;
/// 暖機：前面這段不算（第一次載入字型、快取，以及記憶體配置器把堆積撐到穩定大小）。
const WARMUP_SECS: f64 = 20.0;
/// 暖機後切三段，比較「中間那段」與「最後那段」的中位數。記憶體會隨動作上下擺盪約 1～2MB，
/// 用中位數、而且不拿剛暖機完還在爬坡的第一段，才分得出「擺盪後趨平」與「一直在漏」。
const SEGMENTS: f64 = 3.0;
const MIN_SEGMENT_SECS: f64 = 10.0;
/// 中間段到最後段的中位數差超過這麼多就判定記憶體在漲。
const MAX_GROWTH_BYTES: f64 = 1024.0 * 1024.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    SwitchPet,
    ShowCard,
    OpenSnooze,
    Done,
    ShowInfo,
    Grab,
    Drag,
    Release,
}

const SCRIPT: [Action; 10] = [
    Action::ShowCard,
    Action::OpenSnooze,
    Action::Done,
    Action::Grab,
    Action::Drag,
    Action::Drag,
    Action::Release,
    Action::ShowInfo,
    Action::SwitchPet,
    Action::ShowCard,
];

pub struct Soak {
    duration: f64,
    start: f64,
    next_action: f64,
    next_sample: f64,
    step: usize,
    samples: Vec<Sample>,
    script: Vec<Action>,
    pub actions_done: u64,
}

/// 除了記憶體也記系統資源數量：GDI／USER 物件或核心物件代碼一直變多，就是有東西沒放掉。
struct Sample {
    secs: f64,
    private_bytes: usize,
    gdi: u32,
    user: u32,
    handles: u32,
}

fn group(a: Action) -> &'static str {
    match a {
        Action::SwitchPet => "switch",
        Action::ShowCard | Action::OpenSnooze | Action::Done | Action::ShowInfo => "cards",
        Action::Grab | Action::Drag | Action::Release => "drag",
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    pass: bool,
    seconds: f64,
    actions: u64,
    first_avg_bytes: f64,
    last_avg_bytes: f64,
    growth_bytes: f64,
    max_bytes: usize,
}

impl Soak {
    pub fn new(duration_secs: u64, now: f64) -> Self {
        let only = std::env::var(ONLY_ENV).ok();
        let script: Vec<Action> = SCRIPT.iter().copied().filter(|a| only.as_deref().is_none_or(|g| group(*a) == g)).collect();
        let script = if script.is_empty() { SCRIPT.to_vec() } else { script };
        Self { duration: duration_secs as f64, start: now, next_action: now + 1.0, next_sample: now, step: 0, samples: Vec::new(), script, actions_done: 0 }
    }

    pub fn finished(&self, now: f64) -> bool {
        now - self.start >= self.duration
    }

    /// 這一格要做的動作（每格最多一個）。
    pub fn next(&mut self, now: f64) -> Option<Action> {
        if now >= self.next_sample {
            self.next_sample += SAMPLE_EVERY_SECS;
            let (gdi, user, mut handles) = unsafe {
                let p = GetCurrentProcess();
                (GetGuiResources(p, GR_GDIOBJECTS), GetGuiResources(p, GR_USEROBJECTS), 0u32)
            };
            unsafe {
                let _ = GetProcessHandleCount(GetCurrentProcess(), &mut handles);
            }
            self.samples.push(Sample { secs: now - self.start, private_bytes: memwatch::sample().private_bytes, gdi, user, handles });
        }
        if now < self.next_action {
            return None;
        }
        self.next_action = now + ACTION_EVERY_SECS;
        let a = self.script[self.step % self.script.len()];
        self.step += 1;
        self.actions_done += 1;
        Some(a)
    }

    pub fn report(&self) -> bool {
        let median = |from: f64, to: f64| {
            let mut v: Vec<f64> = self.samples.iter().filter(|s| s.secs >= from && s.secs < to).map(|s| s.private_bytes as f64).collect();
            v.sort_by(f64::total_cmp);
            v.get(v.len() / 2).copied().unwrap_or(0.0)
        };
        let segment = ((self.duration - WARMUP_SECS) / SEGMENTS).max(MIN_SEGMENT_SECS);
        let first = median(self.duration - 2.0 * segment, self.duration - segment);
        let last = median(self.duration - segment, self.duration + 1.0);
        let growth = last - first;
        let report = Report {
            pass: growth < MAX_GROWTH_BYTES,
            seconds: self.duration,
            actions: self.actions_done,
            first_avg_bytes: first,
            last_avg_bytes: last,
            growth_bytes: growth,
            max_bytes: self.samples.iter().map(|s| s.private_bytes).max().unwrap_or(0),
        };
        if let Ok(mut f) = fs::File::create(paths::logs_dir().join("soak.csv")) {
            let _ = writeln!(f, "secs,private_bytes,gdi,user,handles");
            for s in &self.samples {
                let _ = writeln!(f, "{:.1},{},{},{},{}", s.secs, s.private_bytes, s.gdi, s.user, s.handles);
            }
        }
        let json = serde_json::to_vec_pretty(&report).unwrap_or_default();
        let _ = write_atomic(&paths::local_dir().join("soak-result.json"), &json);
        log!("soak done: pass={} actions={} first={:.0}KB last={:.0}KB growth={:.0}KB", report.pass, report.actions, first / 1024.0, last / 1024.0, growth / 1024.0);
        report.pass
    }
}
