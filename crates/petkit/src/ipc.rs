//! 設定程式（傳話則是任何程式）→ 核心的指令：用 WM_COPYDATA 送一段 JSON `{"cmd": "...", ...}` 給核心的桌寵視窗。
//!
//! 核心收到時只是排進自己的待辦、回一個號碼，下一圈才真的做；所以送的一方一律走 `send`：
//! 它會等到核心做完那一號才回來，送完馬上讀核心寫的檔就是新的結果。

use std::thread;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, IsWindow, SendMessageW, WM_APP, WM_COPYDATA,
};
use windows::core::{HSTRING, PCWSTR};

/// 核心桌寵視窗的類別名稱（設定程式靠它找到核心）。
pub const PET_WINDOW_CLASS: &str = "YoubanDesktopPet.Pet";
/// 問核心「指令做到第幾號了」：回傳值就是號碼（送 WM_COPYDATA 時拿到的是自己那一號）。
pub const WM_COMMANDS_DONE: u32 = WM_APP + 5;
/// 等核心做完時多久問一次、最多等多久（核心躲起來時一秒只轉兩圈；卡住了也不要讓送的一方一直等）。
const WAIT_STEP: Duration = Duration::from_millis(10);
const WAIT_MAX: Duration = Duration::from_secs(3);

/// 核心沒在執行。
#[derive(Debug)]
pub struct NotRunning;

/// 傳話的代號：同一個來源（`from`）＋ `id` 就是同一則（再送一次原地換掉，`--done` 收掉）。
pub fn relay_key(from: Option<&str>, id: &str) -> String {
    serde_json::to_string(&(from, id)).expect("strings serialize")
}

/// 送一個指令給核心，等它做完才回來（核心卡住最多等 `WAIT_MAX`，之後當作送到了）。
pub fn send(json: &str) -> Result<(), NotRunning> {
    let hwnd = unsafe { FindWindowW(&HSTRING::from(PET_WINDOW_CLASS), PCWSTR::null()) }
        .map_err(|_| NotRunning)?;
    let data = COPYDATASTRUCT {
        dwData: 0,
        cbData: json.len() as u32,
        lpData: json.as_ptr() as *mut _,
    };
    let ticket = ask(hwnd, WM_COPYDATA, LPARAM(&data as *const _ as isize));
    let start = Instant::now();
    // 做完那一號就關掉自己的指令（換新版、結束），視窗不在了也算做完。
    while start.elapsed() < WAIT_MAX && unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        if ask(hwnd, WM_COMMANDS_DONE, LPARAM(0)) >= ticket {
            break;
        }
        thread::sleep(WAIT_STEP);
    }
    Ok(())
}

fn ask(hwnd: HWND, msg: u32, lparam: LPARAM) -> u64 {
    unsafe { SendMessageW(hwnd, msg, Some(WPARAM(0)), Some(lparam)) }.0 as u64
}

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
/// 傳話紀錄上按「完成」`{"key"}`：跟在確認框上按一樣。
pub const CMD_RELAY_DONE: &str = "relayDone";
/// 打開了傳話紀錄：到現在為止的都算看過了。
pub const CMD_RELAY_LOG_SEEN: &str = "relayLogSeen";
/// 清掉傳話紀錄 `{"from"?}`：沒有 `from` = 全部，`"from": null` = 沒署名的那組。
pub const CMD_RELAY_LOG_CLEAR: &str = "relayLogClear";

/// 一則傳話（用詞見 `CONTEXT.md`）。`from` + `id` 一樣的，後來的更新前一則，不另外疊一則。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Relay {
    pub style: crate::reminders::Style,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// 補充說明：框上不顯示，傳話紀錄裡看得到。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}
