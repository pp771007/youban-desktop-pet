//! 寫到 `%LOCALAPPDATA%\youban-desktop-pet\logs\petcore.log`。超過上限就換成 `.old`，log 不會無限長大。

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use windows::Win32::System::SystemInformation::GetLocalTime;

const LOG_FILE: &str = "petcore.log";
const MAX_LOG_BYTES: u64 = 1024 * 1024;

struct Sink {
    path: PathBuf,
    file: File,
}

static SINK: Mutex<Option<Sink>> = Mutex::new(None);

pub fn init() {
    let dir = crate::paths::logs_dir();
    let _ = fs::create_dir_all(&dir);
    let path = dir.join(LOG_FILE);
    if let Ok(file) = OpenOptions::new().create(true).append(true).open(&path) {
        *SINK.lock().unwrap() = Some(Sink { path, file });
    }
}

pub fn write(msg: &str) {
    let t = unsafe { GetLocalTime() };
    let line = format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03} {msg}\n",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond, t.wMilliseconds
    );
    #[cfg(debug_assertions)]
    eprint!("{line}");
    let mut guard = SINK.lock().unwrap();
    let Some(sink) = guard.as_mut() else { return };
    let _ = sink.file.write_all(line.as_bytes());
    if sink.file.metadata().map(|m| m.len() > MAX_LOG_BYTES).unwrap_or(false) {
        let old = sink.path.with_extension("log.old");
        let _ = fs::rename(&sink.path, old);
        if let Ok(file) = OpenOptions::new().create(true).append(true).open(&sink.path) {
            sink.file = file;
        }
    }
}

#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => { $crate::logger::write(&format!($($arg)*)) };
}
