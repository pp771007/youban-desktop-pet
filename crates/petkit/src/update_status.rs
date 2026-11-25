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

/// 檔案裡的樣子：進度 ＋ 這是在回應哪一次「檢查更新」（設定程式送指令時帶的編號）。
/// 核心收到指令是下一圈才處理，設定程式送完馬上讀到的會是上一次的結果；要等到編號對上才算數。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateReport {
    #[serde(default)]
    pub request: u64,
    #[serde(flatten)]
    pub status: UpdateStatus,
}

impl UpdateReport {
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
