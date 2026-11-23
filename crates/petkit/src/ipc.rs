//! 設定程式 → 核心的指令：用 WM_COPYDATA 送一段 JSON `{"cmd": "...", ...}` 給核心的桌寵視窗。

/// 核心桌寵視窗的類別名稱（設定程式靠它找到核心）。
pub const PET_WINDOW_CLASS: &str = "YoubanDesktopPet.Pet";

pub const CMD_SAVE_LAYOUT: &str = "saveLayout";
pub const CMD_RESTORE_LAYOUT: &str = "restoreLayout";
pub const CMD_TEST_REMINDER: &str = "testReminder";
pub const CMD_SWITCH_PET: &str = "switchPet";
pub const CMD_OPEN_SETTINGS: &str = "openSettings";
pub const CMD_DUMP_STATUS: &str = "dumpStatus";
pub const CMD_CARD_ACTION: &str = "cardAction";
pub const CMD_QUIT: &str = "quit";
/// 不等每天的定時檢查，現在就去查新版；進度寫到 `paths::update_status_file`。
pub const CMD_CHECK_UPDATE: &str = "checkUpdate";
/// 換上已經下載好的新版（跟更新卡上的「現在更新」一樣）。
pub const CMD_APPLY_UPDATE: &str = "applyUpdate";
/// 自動測試用：把桌寵拎到 `{"x", "y"}` 放手。
pub const CMD_DROP: &str = "drop";
/// 自動測試用：走一次「記憶體一路往上漲」的提醒流程（存診斷檔＋冒泡）。
pub const CMD_TEST_MEMORY_ALERT: &str = "testMemoryAlert";
/// 讓桌寵講一句 `{"text", "secs"}`（測試講話泡泡用）。
pub const CMD_SAY: &str = "say";
/// 丟一個點心或球 `{"kind": "food" | "ball"}`（跟右鍵選單一樣）。
pub const CMD_TOY: &str = "toy";
/// 在螢幕上畫一個範圍 `{"kind": "allowed" | "forbidden"}`：畫好由核心寫進設定檔。
pub const CMD_DRAW_AREA: &str = "drawArea";
pub const AREA_ALLOWED: &str = "allowed";
pub const AREA_FORBIDDEN: &str = "forbidden";
