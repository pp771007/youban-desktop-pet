//! 視窗歸位：記住每個視窗的位置與大小，螢幕關掉再打開（或手動）時放回去。
//! 行為照搬 session-window-manager 已驗證過的做法，
//! 差別是佈局存到磁碟 —— 桌寵核心自動更新會重啟一次，只放記憶體會在那時遺失。
//! 視窗代碼（HWND）在視窗關掉前都不會變，所以核心重啟後拿檔案裡的代碼對回去，再用執行檔路徑確認沒被重用。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use petkit::config::{load_json, write_atomic};
use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED, GetLastError, HWND, LPARAM, MAX_PATH, RECT};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO};
use windows::Win32::System::Threading::{GetCurrentProcessId, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BOOL, PWSTR};

use crate::icons::Icon;
use crate::log;
use crate::modules::{Ctx, Module, ModuleEvent};

/// 螢幕接上時 WM_DISPLAYCHANGE 會在約 1 秒內連發，Windows 也會在那之後約 1 秒內停止搬視窗；
/// 太早恢復會被 Windows 再搬一次（session-window-manager 實測）。
const DISPLAY_SETTLE: Duration = Duration::from_secs(3);
/// 多久掃一次新開的視窗。
const AUTO_RECORD_EVERY: Duration = Duration::from_secs(60);
/// 桌寵說「記住了某某」時，視窗標題最多念幾個字（瀏覽器的標題常常一長串）。
const ANNOUNCE_TITLE_MAX_CHARS: usize = 12;
const MENU_SAVE: u32 = 1;
const MENU_RESTORE: u32 = 2;
use petkit::ipc::{CMD_EDIT_WINDOW as CMD_EDIT, CMD_RESTORE_LAYOUT as CMD_RESTORE, CMD_SAVE_LAYOUT as CMD_SAVE};
/// 系統資料夾下的程式不記（設定、開始選單這類），終端機例外。
const SYSTEM_DIRS: [&str; 4] = ["c:\\windows\\system32\\", "c:\\windows\\systemapps\\", "c:\\windows\\immersivecontrolpanel\\", "c:\\program files\\windowsapps\\"];
const TERMINALS: [&str; 7] = ["conhost.exe", "cmd.exe", "powershell.exe", "pwsh.exe", "wt.exe", "windowsterminal.exe", "openconsole.exe"];
const DESKTOP_TITLE: &str = "Program Manager";
const EXPLORER: &str = "explorer.exe";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub title: String,
    pub exe: String,
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Layout {
    /// 存檔時的螢幕配置（排序過的矩形）；螢幕代號每次重接可能不同，所以只比位置與大小。
    pub monitors: Vec<[i32; 4]>,
    /// 視窗代碼 → 位置。
    pub windows: BTreeMap<i64, Entry>,
}

/// 自動記下新視窗之後桌寵說的話：一個就念標題，好幾個就講數量。
pub fn announce_text(titles: &[String]) -> Option<String> {
    match titles {
        [] => None,
        [title] => {
            let short: String = title.chars().take(ANNOUNCE_TITLE_MAX_CHARS).collect();
            let ellipsis = if title.chars().count() > ANNOUNCE_TITLE_MAX_CHARS { "…" } else { "" };
            Some(format!("「{short}{ellipsis}」的位置，我記住囉～"))
        }
        _ => Some(format!("新來的 {} 個視窗，我都記住囉！", titles.len())),
    }
}

/// 這個視窗要不要排除（純判斷，方便測試）。
pub fn should_exclude(exe_lower: &str, title: &str) -> bool {
    let name = exe_lower.rsplit('\\').next().unwrap_or(exe_lower);
    if name == EXPLORER {
        return title == DESKTOP_TITLE;
    }
    if TERMINALS.contains(&name) {
        return false;
    }
    SYSTEM_DIRS.iter().any(|d| exe_lower.starts_with(d))
}

enum Placed {
    Done,
    /// 系統管理員身分開的視窗，柚伴不是系統管理員就搬不動。
    Denied,
    Failed,
}

/// 把視窗搬到記下的位置（最大化、最小化的先還原）。
fn place(hwnd: HWND, e: &Entry) -> Placed {
    unsafe {
        if IsIconic(hwnd).as_bool() || IsZoomed(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let mut r = RECT::default();
        let _ = GetWindowRect(hwnd, &mut r);
        if (r.left, r.top, r.right - r.left, r.bottom - r.top) == (e.left, e.top, e.width, e.height) {
            return Placed::Done;
        }
        match SetWindowPos(hwnd, None, e.left, e.top, e.width, e.height, SWP_NOZORDER | SWP_NOACTIVATE) {
            Ok(()) => Placed::Done,
            Err(_) if GetLastError() == ERROR_ACCESS_DENIED => Placed::Denied,
            Err(err) => {
                log!("move '{}' failed: {err}", e.title);
                Placed::Failed
            }
        }
    }
}

pub struct WindowRestore {
    path: PathBuf,
    layout: Layout,
    display_changed: Option<Instant>,
    last_auto: Instant,
}

impl WindowRestore {
    pub fn new(path: PathBuf) -> Self {
        let mut layout: Layout = load_json(&path).unwrap_or_default();
        // 核心重啟後：代碼還在、而且還是同一支程式的視窗才留。
        layout.windows.retain(|hwnd, e| window_exe(HWND(*hwnd as _)).is_some_and(|exe| exe == e.exe));
        let mut m = Self { path, layout, display_changed: None, last_auto: Instant::now() };
        if m.layout.windows.is_empty() {
            m.save_all();
        } else {
            m.record_new();
        }
        m
    }

    fn persist(&self) {
        match serde_json::to_vec_pretty(&self.layout) {
            Ok(bytes) => {
                if let Err(e) = write_atomic(&self.path, &bytes) {
                    log!("save window layout failed: {e}");
                }
            }
            Err(e) => log!("serialize window layout failed: {e}"),
        }
    }

    /// 以目前畫面為準，整份重記。
    fn save_all(&mut self) -> usize {
        self.layout = Layout { monitors: monitor_geometry(), windows: snapshot().into_iter().collect() };
        self.persist();
        self.layout.windows.len()
    }

    /// 只補記新開的視窗（記下之後再拖動不會更新，要按「儲存視窗位置」）。回傳新記下的視窗標題。
    fn record_new(&mut self) -> Vec<String> {
        let mut added = Vec::new();
        for (hwnd, entry) in snapshot() {
            if let std::collections::btree_map::Entry::Vacant(v) = self.layout.windows.entry(hwnd) {
                added.push(entry.title.clone());
                v.insert(entry);
            }
        }
        if !added.is_empty() {
            self.persist();
        }
        added
    }

    /// 回傳（放回幾個、幾個要系統管理員權限才能動）。
    fn restore(&mut self) -> (usize, usize) {
        self.layout.windows.retain(|hwnd, _| unsafe { IsWindow(Some(HWND(*hwnd as _))) }.as_bool());
        let (mut restored, mut denied) = (0, 0);
        for (hwnd, e) in &self.layout.windows {
            match place(HWND(*hwnd as _), e) {
                Placed::Done => restored += 1,
                Placed::Denied => denied += 1,
                Placed::Failed => {}
            }
        }
        self.persist();
        (restored, denied)
    }

    /// 設定裡改了某個視窗的位置：記下來，也馬上把它搬過去。視窗已經關掉的話就把那筆拿掉。
    fn edit(&mut self, v: &serde_json::Value, ctx: &mut Ctx) {
        let int = |k: &str| v[k].as_i64().and_then(|n| i32::try_from(n).ok());
        let (Some(id), Some(left), Some(top), Some(width), Some(height)) = (v["id"].as_i64(), int("left"), int("top"), int("width"), int("height")) else { return };
        if width <= 0 || height <= 0 {
            return;
        }
        let hwnd = HWND(id as _);
        if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
            if self.layout.windows.remove(&id).is_some() {
                self.persist();
            }
            return;
        }
        let Some(e) = self.layout.windows.get_mut(&id) else { return };
        (e.left, e.top, e.width, e.height) = (left, top, width, height);
        if let Placed::Denied = place(hwnd, e) {
            ctx.info("這個視窗太重了，我搬不動耶",Some("要用系統管理員身分開柚伴才行".into()));
        }
        self.persist();
    }

    fn report_restore(&mut self, ctx: &mut Ctx) {
        let (restored, denied) = self.restore();
        let detail = (denied > 0).then(|| format!("有 {denied} 個太重搬不動，要用系統管理員身分開柚伴才行"));
        let title = if restored == 0 { "沒有要搬的視窗喔".to_owned() } else { format!("幫你把 {restored} 個視窗搬回原位囉～") };
        ctx.info(title, detail);
    }

    fn report_save(&mut self, ctx: &mut Ctx) {
        let n = self.save_all();
        let title = if n == 0 { "現在沒有視窗可以記耶".to_owned() } else { format!("{n} 個視窗的位置，我都記好囉！") };
        ctx.info(title, None);
    }
}

/// 模組代號（設定的功能開關、卡片歸屬都用它）。
pub const ID: &str = "windows";

impl Module for WindowRestore {
    fn id(&self) -> &'static str {
        ID
    }

    fn name(&self) -> &'static str {
        "視窗歸位"
    }

    fn tick(&mut self, ctx: &mut Ctx) {
        if let Some(at) = self.display_changed
            && at.elapsed() >= DISPLAY_SETTLE
        {
            self.display_changed = None;
            if monitor_geometry() == self.layout.monitors {
                log!("display reconnected; restoring layout");
                self.report_restore(ctx);
            } else {
                log!("display layout differs from saved; not restoring");
            }
        }
        if self.last_auto.elapsed() >= AUTO_RECORD_EVERY {
            self.last_auto = Instant::now();
            let added = self.record_new();
            if ctx.config.announce_new_windows
                && let Some(text) = announce_text(&added)
            {
                ctx.say(text);
            }
        }
    }

    fn on_event(&mut self, event: &ModuleEvent, ctx: &mut Ctx) {
        match event {
            ModuleEvent::DisplayChanged => self.display_changed = Some(Instant::now()),
            ModuleEvent::Menu(MENU_SAVE) => self.report_save(ctx),
            ModuleEvent::Menu(MENU_RESTORE) => self.report_restore(ctx),
            ModuleEvent::Command(v) => match v["cmd"].as_str() {
                Some(CMD_SAVE) => self.report_save(ctx),
                Some(CMD_RESTORE) => self.report_restore(ctx),
                Some(CMD_EDIT) => self.edit(v, ctx),
                _ => {}
            },
            _ => {}
        }
    }

    /// 跟設定程式那一頁的按鈕同名。
    fn menu(&self, _now: chrono::NaiveDateTime, _config: &petkit::config::Config) -> Vec<(u32, String, Icon)> {
        vec![(MENU_SAVE, "記住目前位置".into(), Icon::Save), (MENU_RESTORE, "恢復位置".into(), Icon::RotateCcw)]
    }
}

fn monitor_geometry() -> Vec<[i32; 4]> {
    unsafe extern "system" fn collect(monitor: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        let out = unsafe { &mut *(data.0 as *mut Vec<[i32; 4]>) };
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        if unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
            let r = info.rcMonitor;
            out.push([r.left, r.top, r.right, r.bottom]);
        }
        true.into()
    }
    let mut out: Vec<[i32; 4]> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(collect), LPARAM(&mut out as *mut _ as isize));
    }
    out.sort();
    out
}

fn window_exe(hwnd: HWND) -> Option<String> {
    unsafe {
        if !IsWindow(Some(hwnd)).as_bool() {
            return None;
        }
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; MAX_PATH as usize];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(process);
        ok.then(|| String::from_utf16_lossy(&buf[..len as usize]))
    }
}

/// 目前所有該記的視窗：看得見、沒縮小、有標題、不是系統程式、不是桌寵自己。
fn snapshot() -> Vec<(i64, Entry)> {
    unsafe extern "system" fn collect(hwnd: HWND, data: LPARAM) -> BOOL {
        let out = unsafe { &mut *(data.0 as *mut Vec<HWND>) };
        out.push(hwnd);
        true.into()
    }
    let mut hwnds: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut hwnds as *mut _ as isize));
    }
    let me = unsafe { GetCurrentProcessId() };
    let mut out = Vec::new();
    for hwnd in hwnds {
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
                continue;
            }
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == me {
                continue;
            }
            let mut title = [0u16; 512];
            let n = GetWindowTextW(hwnd, &mut title);
            if n <= 0 {
                continue;
            }
            let title = String::from_utf16_lossy(&title[..n as usize]);
            let exe = window_exe(hwnd).unwrap_or_default();
            if should_exclude(&exe.to_lowercase(), &title) {
                continue;
            }
            let mut r = RECT::default();
            if GetWindowRect(hwnd, &mut r).is_err() {
                continue;
            }
            out.push((hwnd.0 as i64, Entry { title, exe, left: r.left, top: r.top, width: r.right - r.left, height: r.bottom - r.top }));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclusion_rules_match_session_window_manager() {
        assert!(should_exclude("c:\\windows\\explorer.exe", "Program Manager"), "desktop background");
        assert!(!should_exclude("c:\\windows\\explorer.exe", "下載"), "file explorer windows are kept");
        assert!(!should_exclude("c:\\windows\\system32\\windowspowershell\\v1.0\\powershell.exe", "PowerShell"), "terminals are kept");
        assert!(should_exclude("c:\\windows\\systemapps\\microsoft.windows.startmenuexperiencehost\\x.exe", "開始"));
        assert!(should_exclude("c:\\program files\\windowsapps\\microsoft.windowscalculator\\calc.exe", "小算盤"));
        assert!(!should_exclude("c:\\program files\\google\\chrome\\application\\chrome.exe", "Google"));
        assert!(!should_exclude("", "不知道是誰"), "unknown exe is kept, like the original");
    }

    #[test]
    fn announces_one_window_by_title_and_several_by_count() {
        assert_eq!(announce_text(&[]), None);
        assert_eq!(announce_text(&["記事本".into()]).as_deref(), Some("「記事本」的位置，我記住囉～"));
        assert_eq!(announce_text(&["新分頁 - Google Chrome".into()]).as_deref(), Some("「新分頁 - Google…」的位置，我記住囉～"));
        assert_eq!(announce_text(&["a".into(), "b".into(), "c".into()]).as_deref(), Some("新來的 3 個視窗，我都記住囉！"));
    }
}
