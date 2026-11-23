//! 托盤圖示（點了開的選單在 `popup_menu`）。

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::{NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW, Shell_NotifyIconW};
use windows::Win32::UI::WindowsAndMessaging::*;

pub const WM_TRAY: u32 = WM_APP + 1;
const TRAY_ID: u32 = 1;
const TOOLTIP: &str = petkit::APP_NAME;

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
