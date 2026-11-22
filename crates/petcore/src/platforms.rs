//! 找出桌寵可以站的地方：每個視窗「頂端露出來的那一段」，以及有沒有全螢幕程式在前面。

use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Shell::{QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::BOOL;

use crate::behavior::Platform;

/// Windows 自己判斷「現在不該打擾使用者」：全螢幕程式、D3D 全螢幕遊戲、簡報模式。
/// 跟 Windows 通知在這些情況下暫停的規則一致。
pub fn user_is_busy() -> bool {
    match unsafe { SHQueryUserNotificationState() } {
        Ok(state) => matches!(state, QUNS_BUSY | QUNS_RUNNING_D3D_FULL_SCREEN | QUNS_PRESENTATION_MODE),
        Err(_) => false,
    }
}

fn frame_bounds(hwnd: HWND) -> Option<RECT> {
    let mut r = RECT::default();
    // 用 DWM 的實際外框：GetWindowRect 會多算一圈看不見的調整大小邊框（約 7 像素），桌寵會浮在半空中。
    let ok = unsafe { DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS, &mut r as *mut _ as *mut _, size_of::<RECT>() as u32) }.is_ok();
    if ok || unsafe { GetWindowRect(hwnd, &mut r) }.is_ok() { Some(r) } else { None }
}

fn is_cloaked(hwnd: HWND) -> bool {
    let mut cloaked: u32 = 0;
    unsafe { DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut cloaked as *mut _ as *mut _, size_of::<u32>() as u32) }.is_ok() && cloaked != 0
}

/// 這個螢幕工作區域裡、可以站的視窗頂端（實體像素）。
/// `min_width` 以下的窄段不算；頂端上方留不下一個桌寵高度的也不算（會被螢幕上緣切掉）。
pub fn scan(work: RECT, min_width: f32, pet_height: f32) -> Vec<Platform> {
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
    // EnumWindows 由上往下列出；上面的視窗會蓋住下面視窗的頂端。
    let mut above: Vec<RECT> = Vec::new();
    let mut out = Vec::new();
    for hwnd in hwnds {
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() || is_cloaked(hwnd) {
                continue;
            }
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == me {
                continue;
            }
            let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            let Some(r) = frame_bounds(hwnd) else { continue };
            let usable = !IsZoomed(hwnd).as_bool() && ex & WS_EX_TOOLWINDOW.0 == 0 && GetWindowTextLengthW(hwnd) > 0;
            if usable && r.top as f32 > work.top as f32 + pet_height && r.top < work.bottom {
                let mut segments = vec![(r.left.max(work.left) as f32, r.right.min(work.right) as f32)];
                for o in above.iter().filter(|o| o.top <= r.top && o.bottom >= r.top) {
                    segments = subtract(segments, o.left as f32, o.right as f32);
                }
                let (window, window_left) = (hwnd.0 as isize, r.left as f32);
                out.extend(segments.into_iter().filter(|(l, rr)| rr - l >= min_width).map(|(left, right)| Platform { left, right, y: r.top as f32, window, window_left }));
            }
            above.push(r);
        }
    }
    out
}

/// 單獨看一個視窗現在的左上角（桌寵站在它上面時每一格都看，拖動時才跟得緊）；
/// 視窗關掉、縮小、藏起來了回傳 `None`。
pub fn window_origin(window: isize) -> Option<(f32, f32)> {
    let hwnd = HWND(window as *mut _);
    unsafe {
        if !IsWindow(Some(hwnd)).as_bool() || !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() || is_cloaked(hwnd) {
            return None;
        }
    }
    frame_bounds(hwnd).map(|r| (r.left as f32, r.top as f32))
}

/// 從一組區段裡挖掉 `[lo, hi]`。
fn subtract(segments: Vec<(f32, f32)>, lo: f32, hi: f32) -> Vec<(f32, f32)> {
    let mut out = Vec::with_capacity(segments.len() + 1);
    for (l, r) in segments {
        if hi <= l || lo >= r {
            out.push((l, r));
            continue;
        }
        if lo > l {
            out.push((l, lo));
        }
        if hi < r {
            out.push((hi, r));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covered_part_of_a_window_top_is_removed() {
        assert_eq!(subtract(vec![(0.0, 100.0)], 30.0, 60.0), vec![(0.0, 30.0), (60.0, 100.0)]);
        assert_eq!(subtract(vec![(0.0, 100.0)], -10.0, 40.0), vec![(40.0, 100.0)]);
        assert_eq!(subtract(vec![(0.0, 100.0)], -10.0, 140.0), vec![]);
        assert_eq!(subtract(vec![(0.0, 100.0)], 200.0, 300.0), vec![(0.0, 100.0)]);
    }
}
