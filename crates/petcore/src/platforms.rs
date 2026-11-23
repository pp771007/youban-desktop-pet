//! 找出桌寵可以站的地方：每個視窗「頂端露出來的那一段」，以及有沒有全螢幕程式在前面。

use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Shell::{QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::BOOL;

use crate::behavior::Platform;

/// 比桌寵身高的這麼多倍還細的視窗不算擋住別的視窗頂端：有些程式（LINE 這類 Qt 程式）在主視窗四周
/// 另外放一圈只有十來像素寬的視窗畫陰影，貼在頂端上面那條會把整個頂端扣光，桌寵就站不上去。
/// 這麼細的東西本來也遮不住牠的腳。
const OCCLUDER_MIN_THICKNESS_HEIGHTS: f32 = 0.1;

/// 由上往下的一個視窗（`scan` 從系統讀出來的；拆出來是為了不碰 Windows 也能測）。
struct Shown {
    id: isize,
    rect: RECT,
    /// 有標題、不是工具視窗、沒有最大化：可以站上去。
    standable: bool,
}

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
    let mut shown = Vec::new();
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
            let Some(rect) = frame_bounds(hwnd) else { continue };
            let standable = !IsZoomed(hwnd).as_bool() && ex & WS_EX_TOOLWINDOW.0 == 0 && GetWindowTextLengthW(hwnd) > 0;
            shown.push(Shown { id: hwnd.0 as isize, rect, standable });
        }
    }
    platforms_of(&shown, work, min_width, pet_height)
}

/// `shown` 由上往下排（EnumWindows 的順序）：上面的視窗會蓋住下面視窗的頂端。
fn platforms_of(shown: &[Shown], work: RECT, min_width: f32, pet_height: f32) -> Vec<Platform> {
    let min_thickness = (pet_height * OCCLUDER_MIN_THICKNESS_HEIGHTS) as i32;
    let mut above: Vec<RECT> = Vec::new();
    let mut out = Vec::new();
    for w in shown {
        let r = w.rect;
        if w.standable && r.top as f32 > work.top as f32 + pet_height && r.top < work.bottom {
            let mut segments = vec![(r.left.max(work.left) as f32, r.right.min(work.right) as f32)];
            for o in above.iter().filter(|o| o.top <= r.top && o.bottom >= r.top) {
                segments = subtract(segments, o.left as f32, o.right as f32);
            }
            out.extend(segments.into_iter().filter(|(l, rr)| rr - l >= min_width).map(|(left, right)| {
                // 段的端點就是視窗真正的邊（不是被別的視窗、螢幕邊切出來的）才爬得上來。
                let sides = (left == r.left as f32, right == r.right as f32);
                Platform { left, right, y: r.top as f32, window: w.id, window_left: r.left as f32, sides, bottom: r.bottom as f32 }
            }));
        }
        if (r.right - r.left).min(r.bottom - r.top) >= min_thickness {
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
    fn a_thin_shadow_window_hugging_the_top_does_not_hide_it() {
        let rect = |left, top, right, bottom| RECT { left, top, right, bottom };
        let work = rect(0, 0, 3840, 2112);
        // LINE：主視窗上面貼一條 11 像素的陰影視窗（排在主視窗前面）。
        let shown = [
            Shown { id: 1, rect: rect(2793, 1050, 3789, 1061), standable: false },
            Shown { id: 2, rect: rect(2793, 1061, 3789, 1951), standable: true },
        ];
        let p = platforms_of(&shown, work, 100.0, 256.0);
        assert_eq!(p.len(), 1, "{p:?}");
        assert_eq!((p[0].left, p[0].right, p[0].y, p[0].sides), (2793.0, 3789.0, 1061.0, (true, true)));
        // 真的蓋在上面的視窗照樣擋住那一段。
        let shown = [Shown { id: 3, rect: rect(3000, 900, 3400, 1500), standable: true }, Shown { id: 2, rect: rect(2793, 1061, 3789, 1951), standable: true }];
        let p: Vec<_> = platforms_of(&shown, work, 100.0, 256.0).into_iter().filter(|p| p.window == 2).map(|p| (p.left, p.right)).collect();
        assert_eq!(p, vec![(2793.0, 3000.0), (3400.0, 3789.0)]);
    }

    #[test]
    fn covered_part_of_a_window_top_is_removed() {
        assert_eq!(subtract(vec![(0.0, 100.0)], 30.0, 60.0), vec![(0.0, 30.0), (60.0, 100.0)]);
        assert_eq!(subtract(vec![(0.0, 100.0)], -10.0, 40.0), vec![(40.0, 100.0)]);
        assert_eq!(subtract(vec![(0.0, 100.0)], -10.0, 140.0), vec![]);
        assert_eq!(subtract(vec![(0.0, 100.0)], 200.0, 300.0), vec![(0.0, 100.0)]);
    }
}
