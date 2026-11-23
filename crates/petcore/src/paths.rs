//! 只有核心用的檔案位置；跟設定程式共用的在 `petkit::paths`。

use std::path::PathBuf;

pub use petkit::paths::*;

pub fn logs_dir() -> PathBuf {
    local_dir().join("logs")
}

/// 核心自己的執行狀態：桌寵位置。
pub fn state_file() -> PathBuf {
    local_dir().join("state.json")
}

/// 核心執行檔所在的資料夾（自動更新換檔的地方）。
pub fn install_dir() -> Option<PathBuf> {
    std::env::current_exe().ok()?.parent().map(PathBuf::from)
}

pub fn reminder_state_file() -> PathBuf {
    local_dir().join("reminder-state.json")
}

pub fn settings_exe() -> Option<PathBuf> {
    sibling_exe(SETTINGS_EXE)
}
