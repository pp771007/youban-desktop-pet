//! 托盤圖示與右鍵選單。

use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::UI::Shell::{NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW, Shell_NotifyIconW};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::HSTRING;

pub const WM_TRAY: u32 = WM_APP + 1;
const TRAY_ID: u32 = 1;
const TOOLTIP: &str = petkit::APP_NAME;

pub enum MenuItem {
    Item { id: u32, label: String, checked: bool },
    Sub { label: String, items: Vec<MenuItem> },
    Separator,
}

pub struct Tray {
    hwnd: HWND,
    icon: HICON,
}

impl Tray {
    pub fn new(hwnd: HWND, icon: HICON) -> Self {
        let tray = Self { hwnd, icon };
        tray.add();
        tray
    }

    /// 檔案總管重啟後托盤會清空，收到 `TaskbarCreated` 時再加一次。
    pub fn add(&self) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_ADD, &self.data());
        }
    }

    /// 換桌寵時換托盤圖示。
    pub fn set_icon(&mut self, icon: HICON) {
        let old = std::mem::replace(&mut self.icon, icon);
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &self.data());
            let _ = DestroyIcon(old);
        }
    }

    fn data(&self) -> NOTIFYICONDATAW {
        let mut data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: TRAY_ID,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: WM_TRAY,
            hIcon: self.icon,
            ..Default::default()
        };
        for (dst, src) in data.szTip.iter_mut().zip(TOOLTIP.encode_utf16()) {
            *dst = src;
        }
        data
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &self.data());
            let _ = DestroyIcon(self.icon);
        }
    }
}

fn build(items: &[MenuItem]) -> Option<HMENU> {
    unsafe {
        let menu = CreatePopupMenu().ok()?;
        for item in items {
            match item {
                MenuItem::Item { id, label, checked } => {
                    let flags = if *checked { MF_STRING | MF_CHECKED } else { MF_STRING };
                    let _ = AppendMenuW(menu, flags, *id as usize, &HSTRING::from(label.as_str()));
                }
                MenuItem::Sub { label, items } => {
                    if let Some(sub) = build(items) {
                        let _ = AppendMenuW(menu, MF_POPUP, sub.0 as usize, &HSTRING::from(label.as_str()));
                    }
                }
                MenuItem::Separator => {
                    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
                }
            }
        }
        Some(menu)
    }
}

/// 在滑鼠位置彈出選單，回傳選到的項目編號。
pub fn show_menu(hwnd: HWND, items: &[MenuItem]) -> Option<u32> {
    unsafe {
        let menu = build(items)?;
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        // 不先拿到前景，點選單外面時選單不會自己關掉（Win32 TrackPopupMenu 的已知行為）。
        let _ = SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, pt.x, pt.y, None, hwnd, None);
        let _ = PostMessageW(Some(hwnd), WM_NULL, Default::default(), Default::default());
        let _ = DestroyMenu(menu); // 連子選單一起收掉
        (cmd.0 != 0).then_some(cmd.0 as u32)
    }
}
