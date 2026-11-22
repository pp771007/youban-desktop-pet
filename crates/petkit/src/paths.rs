//! 桌寵核心與設定程式共用的檔案位置（兩支程式一定要讀寫同一份）。

use std::env;
use std::path::PathBuf;

const APP_DIR: &str = "youban-desktop-pet";
/// 以前用過的資料夾名（新的在前）：第一次啟動新版時整個搬過來（`migrate_legacy_dirs`），設定、提醒、陪伴天數不會不見。
const LEGACY_APP_DIRS: [&str; 2] = ["youban", "desktop-pet"];
/// 設定與使用者資料放 `%APPDATA%`（跟著帳號漫遊），log、狀態、下載快取放 `%LOCALAPPDATA%`。
const BASE_VARS: [&str; 2] = ["APPDATA", "LOCALAPPDATA"];
const PETS_DIR: &str = "pets";
/// 開發時執行檔在 `target/debug/`，往上找幾層就能碰到 repo 根目錄的 `pets/`。
const PETS_SEARCH_DEPTH: usize = 5;
/// 執行檔名，跟兩個 crate 的 `[[bin]] name` 一致。
pub const CORE_EXE: &str = "Youban.exe";
pub const SETTINGS_EXE: &str = "YoubanSettings.exe";

fn base_dir(var: &str) -> PathBuf {
    env::var_os(var).map(PathBuf::from).unwrap_or_else(env::temp_dir)
}

fn env_dir(var: &str) -> PathBuf {
    base_dir(var).join(APP_DIR)
}

/// 新資料夾還沒有、舊的還在 → 把最新的那個舊資料夾整個改名搬過來。兩支程式啟動時（寫任何檔案之前）都要先呼叫。
/// 回傳搬不動的原因（給呼叫端寫 log）；搬不動時新版從空的開始，舊資料夾原封不動留著。
pub fn migrate_legacy_dirs() -> Vec<String> {
    let mut problems = Vec::new();
    for var in BASE_VARS {
        let new = base_dir(var).join(APP_DIR);
        if new.exists() {
            continue;
        }
        let Some(old) = LEGACY_APP_DIRS.iter().map(|d| base_dir(var).join(d)).find(|d| d.is_dir()) else { continue };
        if let Err(e) = std::fs::rename(&old, &new) {
            problems.push(format!("{} -> {}: {e}", old.display(), new.display()));
        }
    }
    problems
}

/// 使用者設定：`%APPDATA%\youban-desktop-pet\config.json`（跟著帳號漫遊）。
pub fn config_file() -> PathBuf {
    env_dir("APPDATA").join("config.json")
}

/// 提醒清單：設定程式寫、核心讀。
pub fn reminders_file() -> PathBuf {
    env_dir("APPDATA").join("reminders.json")
}

/// 陪伴天數：核心寫、設定程式讀。跟設定放一起（是使用者的資料，不是可以重建的快取）。
pub fn companion_file() -> PathBuf {
    env_dir("APPDATA").join("companion.json")
}

/// 本機資料（log、執行狀態、下載的日曆）：`%LOCALAPPDATA%\youban-desktop-pet\`。
pub fn local_dir() -> PathBuf {
    env_dir("LOCALAPPDATA")
}

pub fn holiday_cache_dir() -> PathBuf {
    local_dir().join("holidays")
}

pub fn window_layout_file() -> PathBuf {
    local_dir().join("window-layout.json")
}

/// `dumpStatus` 指令寫出的目前狀態。
pub fn status_file() -> PathBuf {
    local_dir().join("status.json")
}

/// 桌寵包資料夾：執行檔旁邊的 `pets/`，開發時往上找到 repo 的 `pets/`。
pub fn pets_dir() -> Option<PathBuf> {
    let exe = env::current_exe().ok()?;
    exe.ancestors().skip(1).take(PETS_SEARCH_DEPTH).map(|d| d.join(PETS_DIR)).find(|d| d.is_dir())
}

/// 跟目前這支程式放在同一個資料夾的另一支程式（核心找設定程式、設定程式找核心）。
pub fn sibling_exe(name: &str) -> Option<PathBuf> {
    env::current_exe().ok()?.parent().map(|d| d.join(name)).filter(|p| p.is_file())
}
