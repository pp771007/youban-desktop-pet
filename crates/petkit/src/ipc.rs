//! 設定程式（傳話則是任何程式）→ 核心的指令：用 WM_COPYDATA 送一段 JSON `{"cmd": "...", ...}` 給核心的桌寵視窗。

/// 核心桌寵視窗的類別名稱（設定程式靠它找到核心）。
pub const PET_WINDOW_CLASS: &str = "YoubanDesktopPet.Pet";

pub const CMD_SAVE_LAYOUT: &str = "saveLayout";
pub const CMD_RESTORE_LAYOUT: &str = "restoreLayout";
/// 改一個視窗記下的位置並馬上搬過去 `{"id", "left", "top", "width", "height"}`（`id` 是視窗代碼）。
pub const CMD_EDIT_WINDOW: &str = "editWindow";
pub const CMD_TEST_REMINDER: &str = "testReminder";
pub const CMD_SWITCH_PET: &str = "switchPet";
pub const CMD_OPEN_SETTINGS: &str = "openSettings";
pub const CMD_DUMP_STATUS: &str = "dumpStatus";
pub const CMD_CARD_ACTION: &str = "cardAction";
pub const CMD_QUIT: &str = "quit";
/// 開始／停止專注（設定程式的「開始專注」；右鍵選單走模組自己的選單項目）。
pub const CMD_FOCUS_START: &str = "focusStart";
pub const CMD_FOCUS_STOP: &str = "focusStop";
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
/// 在桌面上框出一個範圍給人看 `{"kind", "area": {"left", "top", "right", "bottom"}}`；不帶 `area` = 收掉。
pub const CMD_PREVIEW_AREA: &str = "previewArea";
pub const AREA_ALLOWED: &str = "allowed";
pub const AREA_FORBIDDEN: &str = "forbidden";
/// 傳話：指令的其他欄位是一個 `Relay`。
pub const CMD_RELAY: &str = "relay";

/// 一則傳話（用詞見 `CONTEXT.md`）。`from` + `id` 一樣的，後來的更新前一則，不另外疊一則。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Relay {
    pub style: crate::reminders::Style,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}
