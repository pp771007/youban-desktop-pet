//! 檢查更新的進度：核心寫、設定程式讀（設定裡「檢查更新」按鈕顯示結果用）。

use std::{fs, io, path::Path};

use serde::{Deserialize, Serialize};

use crate::config::write_atomic;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum UpdateStatus {
    Checking,
    Downloading {
        version: String,
    },
    UpToDate,
    /// 新版下載解好了，等使用者按「現在更新」（或下次啟動時換上）。
    Ready {
        version: String,
    },
    Failed,
    /// 從編譯資料夾跑的開發版不會被正式版蓋掉。
    Unavailable,
}

impl UpdateStatus {
    /// 檔案不在或讀不懂都當成沒有。
    pub fn load(path: &Path) -> Option<Self> {
        serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        write_atomic(
            path,
            serde_json::to_string(self)
                .map_err(io::Error::other)?
                .as_bytes(),
        )
    }
}
