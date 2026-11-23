//! 使用者設定（`%APPDATA%\youban-desktop-pet\config.json`）：設定程式寫、核心程式讀（核心只在從托盤換桌寵時寫）。
//! 缺的欄位一律用預設值，舊版設定檔照樣讀得進來。
//! 核心程式自己的執行狀態（桌寵位置等）不放這裡，免得兩支程式搶著寫同一個檔。

use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::reminders::Style;

/// 休息提醒預設連續用多久提醒一次（分鐘）。
pub const DEFAULT_REST_EVERY_MINUTES: u32 = 50;

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
    /// `Roam::Inside` 用的範圍（只有一個框）。
    pub allowed_area: Option<Area>,
    /// `Roam::Avoid` 用的範圍：碰到就爬上去、從頂端走過去。
    pub forbidden_areas: Vec<Area>,
    pub talk: Talk,
    /// 掉下來時可以落在視窗頂端、在上面散步。
    pub stand_on_windows: bool,
    /// 有全螢幕程式（影片、簡報、遊戲）時躲起來，提醒卡先排隊。
    pub hide_in_fullscreen: bool,
    /// 提醒卡跳出來時響一聲（用 Windows 音效配置裡的「提醒」音效）。
    pub reminder_sound: bool,
    /// 全螢幕時桌寵躲起來了，提醒卡照樣跳（放在螢幕右下角）；關著就等離開全螢幕才跳。
    pub remind_in_fullscreen: bool,
    /// 每天看一次有沒有新版，有就自動下載、找空檔換上。
    pub auto_update: bool,
    /// 視窗歸位自動記下新開的視窗時，桌寵說一聲。
    pub announce_new_windows: bool,
    /// 休息提醒：連續用電腦 `rest_every_minutes` 分鐘跳一張卡（中間離開五分鐘算休息過，重新算）。
    pub rest_reminder: bool,
    pub rest_every_minutes: u32,
    /// 休息提醒的卡片要按完成，還是自己消失。
    pub rest_style: Style,
    pub features: Features,
}

/// 哪些功能要開。關掉的功能核心完全不載入（不跑、不佔記憶體）。
/// 每個功能一個欄位；舊設定檔缺的欄位用 `Default` 的值。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Features {
    /// 提醒（連同辦公日曆的背景更新）。
    pub reminders: bool,
    /// 預設關：會自己搬動使用者的視窗，要使用者自己決定打開。
    pub window_restore: bool,
}

impl Default for Features {
    fn default() -> Self {
        Self { reminders: true, window_restore: false }
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
    /// 舊版的「只在附近」拿掉了，讀到就當整個螢幕。
    #[default]
    #[serde(alias = "nearby")]
    Screen,
    /// 只在 `allowed_area` 裡面活動。
    Inside,
    /// 不進 `forbidden_areas`。
    Avoid,
}

/// 使用者在螢幕上畫的一個框（螢幕實體像素，多螢幕時是整個虛擬桌面的座標）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Area {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Area {
    /// 從兩個角（拖曳的起點與終點，順序不拘）組成一個框。
    pub fn from_corners((x0, y0): (i32, i32), (x1, y1): (i32, i32)) -> Self {
        Self { left: x0.min(x1), top: y0.min(y1), right: x0.max(x1), bottom: y0.max(y1) }
    }

    pub fn width(&self) -> i32 {
        self.right - self.left
    }

    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }
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
            allowed_area: None,
            forbidden_areas: Vec::new(),
            talk: Talk::default(),
            stand_on_windows: true,
            hide_in_fullscreen: true,
            reminder_sound: true,
            remind_in_fullscreen: false,
            auto_update: true,
            announce_new_windows: true,
            rest_reminder: true,
            rest_every_minutes: DEFAULT_REST_EVERY_MINUTES,
            rest_style: Style::Auto,
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
        assert_eq!(c.features, Features::default(), "features missing from an old file take defaults");
        assert!(!c.features.window_restore, "window restore starts off");
        let c: Config = serde_json::from_str(r#"{ "features": { "windowRestore": true } }"#).unwrap();
        assert_eq!(c.features, Features { reminders: true, window_restore: true });
        let c: Config = serde_json::from_str(r#"{ "roam": "nearby" }"#).unwrap();
        assert_eq!(c.roam, Roam::Screen, "the removed 'nearby' choice reads as the whole screen");
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
