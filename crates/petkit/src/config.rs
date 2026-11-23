//! 使用者設定（`%APPDATA%\youban-desktop-pet\config.json`）：設定程式寫、核心程式讀（核心只在從托盤換桌寵時寫）。
//! 缺的欄位一律用預設值，舊版設定檔照樣讀得進來。
//! 核心程式自己的執行狀態（桌寵位置等）不放這裡，免得兩支程式搶著寫同一個檔。

use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub const DEFAULT_PET: &str = "capybara";
/// 中尺寸：桌寵身高 128 DIP（100% 縮放時 128 像素）。
pub const DEFAULT_SIZE_DIP: u32 = 128;
/// 設定畫面的三種大小。
pub const SIZE_CHOICES_DIP: [u32; 3] = [96, DEFAULT_SIZE_DIP, 176];
/// 多久沒碰電腦就睡著。
pub const DEFAULT_SLEEP_AFTER_SECS: u64 = 300;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Config {
    pub pet: String,
    pub size_dip: u32,
    pub sleep_after_secs: u64,
    pub activity: Activity,
    pub roam: Roam,
    pub talk: Talk,
    /// 掉下來時可以落在視窗頂端、在上面散步。
    pub stand_on_windows: bool,
    /// 有全螢幕程式（影片、簡報、遊戲）時躲起來，提醒卡先排隊。
    pub hide_in_fullscreen: bool,
    /// 提醒卡跳出來時響一聲（用 Windows 音效配置裡的「提醒」音效）。
    pub reminder_sound: bool,
    /// 每天看一次有沒有新版，有就自動下載、找空檔換上。
    pub auto_update: bool,
    /// 視窗歸位自動記下新開的視窗時，桌寵說一聲。
    pub announce_new_windows: bool,
    pub features: Features,
}

/// 哪些功能要開。關掉的功能核心完全不載入（不跑、不佔記憶體）。
/// 新功能一律加一個欄位、預設開，舊設定檔讀進來就是開著的。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Features {
    /// 提醒（連同辦公日曆的背景更新）。
    pub reminders: bool,
    pub window_restore: bool,
}

impl Default for Features {
    fn default() -> Self {
        Self { reminders: true, window_restore: true }
    }
}

/// 活潑程度。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Activity {
    Lively,
    #[default]
    Normal,
    /// 待在原地，只做小動作。
    Calm,
}

/// 活動範圍。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Roam {
    #[default]
    Screen,
    /// 只在使用者最後放下它的位置附近。
    Nearby,
}

/// 講話頻率。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Talk {
    Often,
    #[default]
    Sometimes,
    Never,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            pet: DEFAULT_PET.to_owned(),
            size_dip: DEFAULT_SIZE_DIP,
            sleep_after_secs: DEFAULT_SLEEP_AFTER_SECS,
            activity: Activity::default(),
            roam: Roam::default(),
            talk: Talk::default(),
            stand_on_windows: true,
            hide_in_fullscreen: true,
            reminder_sound: true,
            auto_update: true,
            announce_new_windows: true,
            features: Features::default(),
        }
    }
}

impl Config {
    /// 檔案不存在 → 預設值；檔案壞掉 → 回傳錯誤，讓呼叫端決定（不要默默蓋掉使用者的設定）。
    pub fn load(path: &Path) -> io::Result<Self> {
        load_json(path)
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        write_atomic(path, &serde_json::to_vec_pretty(self).map_err(io::Error::other)?)
    }
}

/// 讀 JSON 檔；不存在就回傳預設值。
pub fn load_json<T: for<'de> Deserialize<'de> + Default>(path: &Path) -> io::Result<T> {
    match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).map_err(io::Error::other),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(e),
    }
}

/// 先寫暫存檔再改名：寫到一半斷電、或另一支程式剛好在讀，都不會看到半份檔案。
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_take_defaults() {
        let c: Config = serde_json::from_str(r#"{ "pet": "cat" }"#).unwrap();
        assert_eq!(c.pet, "cat");
        assert_eq!(c.size_dip, DEFAULT_SIZE_DIP);
        assert_eq!(c.features, Features { reminders: true, window_restore: true }, "features missing from an old file stay on");
        let c: Config = serde_json::from_str(r#"{ "features": { "windowRestore": false } }"#).unwrap();
        assert_eq!(c.features, Features { reminders: true, window_restore: false });
    }

    #[test]
    fn save_then_load_round_trips() {
        let path = std::env::temp_dir().join(format!("petkit-config-{}.json", std::process::id()));
        let c = Config { size_dip: 176, ..Config::default() };
        c.save(&path).unwrap();
        assert_eq!(Config::load(&path).unwrap(), c);
        let _ = fs::remove_file(path);
    }
}
