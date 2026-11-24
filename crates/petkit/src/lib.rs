//! 桌寵核心與設定程式共用的資料格式：桌寵包、狀態替代規則、使用者設定。
//! 規範本體在 `docs/pet-pack-spec.md`。

/// 產品名稱（托盤提示、設定視窗標題）。「柚伴」念起來就是「有伴」；水豚頭上那顆是柚子。
/// 執行檔名稱不跟著改；資料夾已改名，舊的會自動搬過來（`paths::migrate_legacy_dirs`）。
pub const APP_NAME: &str = "柚伴";

pub mod anniversary;
pub mod companion;
pub mod config;
pub mod focus_status;
pub mod holidays;
pub mod ipc;
pub mod manifest;
pub mod pack;
pub mod pack_edit;
pub mod paths;
pub mod relay_log;
pub mod reminders;
pub mod sheet;
pub mod states;
pub mod update_status;
