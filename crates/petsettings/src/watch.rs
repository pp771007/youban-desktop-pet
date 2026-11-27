//! 畫面上的資料，有些不是設定程式自己寫的：核心畫好範圍、從托盤換桌寵、傳話進來、記住視窗位置、查新版，
//! 玩家也會直接往桌寵資料夾丟東西。這裡定時看那些檔的修改時間，變了就發 `DATA_CHANGED` 給畫面，
//! 畫面重讀那一份（`ui/app.js` 的 `RELOADERS`）—— 不用每個功能各自猜什麼時候該重讀。
//! 跟核心看設定檔是同一招（`petcore` 的 `config_stamp`）。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use petkit::paths;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

pub const DATA_CHANGED: &str = "data-changed";
/// 看一次幾個檔的修改時間很便宜；半秒內跟上，畫完範圍回到設定視窗時已經是新的。
const INTERVAL: Duration = Duration::from_millis(500);

/// 畫面收到的名字；`ui/app.js` 的 `RELOADERS` 每一個都要有（開啟時會檢查）。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Config,
    Reminders,
    Pets,
    Windows,
    Focus,
    Relay,
    Update,
}

pub const KINDS: [Kind; 7] = [
    Kind::Config,
    Kind::Reminders,
    Kind::Pets,
    Kind::Windows,
    Kind::Focus,
    Kind::Relay,
    Kind::Update,
];

impl Kind {
    fn files(self) -> Vec<PathBuf> {
        match self {
            Kind::Config => vec![paths::config_file()],
            Kind::Reminders => vec![paths::reminders_file()],
            // 資料夾本身的修改時間：裡面多一隻、少一隻桌寵時會變。陪伴天數也顯示在桌寵清單上。
            Kind::Pets => paths::pets_dir()
                .into_iter()
                .chain([paths::companion_file()])
                .collect(),
            Kind::Windows => vec![paths::window_layout_file()],
            Kind::Focus => vec![paths::focus_status_file()],
            Kind::Relay => vec![paths::relay_log_file()],
            Kind::Update => vec![paths::update_status_file()],
        }
    }

    fn stamps(self) -> Stamps {
        self.files()
            .iter()
            .map(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
            .collect()
    }
}

type Stamps = Vec<Option<SystemTime>>;

pub struct DataWatch {
    seen: Mutex<HashMap<Kind, Stamps>>,
}

impl DataWatch {
    pub fn new() -> Self {
        Self {
            seen: Mutex::new(KINDS.iter().map(|&k| (k, k.stamps())).collect()),
        }
    }

    /// 設定程式自己寫的檔：寫完當場記下新的修改時間，不會回頭通知自己。
    /// 不然畫面重讀時可能讀到上一次存的值，蓋掉使用者剛改的。
    pub fn write_own<T>(&self, kind: Kind, write: impl FnOnce() -> T) -> T {
        let mut seen = self.seen.lock().unwrap();
        let out = write();
        seen.insert(kind, kind.stamps());
        out
    }

    fn changed(&self) -> Vec<Kind> {
        let mut seen = self.seen.lock().unwrap();
        KINDS
            .into_iter()
            .filter(|&k| {
                let now = k.stamps();
                seen.insert(k, now.clone()).as_ref() != Some(&now)
            })
            .collect()
    }
}

pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(INTERVAL);
            for kind in app.state::<DataWatch>().changed() {
                let _ = app.emit(DATA_CHANGED, kind);
            }
        }
    });
}
