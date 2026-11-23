// 正式版不要跳出黑色主控台視窗；開發版留著看 log。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod autostart;
mod behavior;
mod bubble;
mod chatter;
mod effects;
mod holiday_sync;
mod logger;
mod memwatch;
mod modules;
mod paths;
mod platforms;
mod reminder_module;
mod render;
mod slicer;
mod preview;
mod snapshot;
mod soak;
mod sprites;
mod theme;
mod toy;
mod glow;
mod area_editor;
mod countdown;
mod focus_module;
mod icons;
mod popup_menu;
mod tray;
mod updater;
mod window_restore;

use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, LPARAM, WAIT_ABANDONED, WAIT_OBJECT_0, WPARAM};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::Threading::{CreateMutexW, WaitForSingleObject};
use windows::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, SendMessageW, WM_COPYDATA};
use windows::core::{PCWSTR, w};

const SNAPSHOT_FLAG: &str = "--snapshot";
const SNAPSHOT_CARDS_FLAG: &str = "--snapshot-cards";
const FRAMES_FLAG: &str = "--frames";
const SLICE_FLAG: &str = "--slice";
const SEND_FLAG: &str = "--send";
/// 叫執行中的柚伴結束（解除安裝程式在刪檔前用；不用自己組 JSON）。
const QUIT_FLAG: &str = "--quit";
const SOAK_FLAG: &str = "--soak";
const AUTOSTART_FLAG: &str = "--autostart";
const SNAPSHOT_DEFAULT_DPI: u32 = 96;
const SOAK_DEFAULT_SECS: u64 = 120;
/// 更新後的新核心最多等舊的多久才放棄。
const AFTER_UPDATE_WAIT_MS: u32 = 15_000;

fn main() {
    // 要在 log 開檔之前：log 會先建出新資料夾，舊的就搬不過來了。
    let migration = petkit::paths::migrate_legacy_dirs();
    logger::init();
    for problem in migration {
        log!("legacy data folder not moved: {problem}");
    }
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log!("panic: {info}");
        default_hook(info);
    }));

    let args: Vec<String> = std::env::args().collect();
    let arg = |i: usize| args.get(i).map(String::as_str);
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }

    match arg(1) {
        Some(SLICE_FLAG) => {
            let (Some(sheet), Some(pet)) = (arg(2), arg(3)) else {
                eprintln!("用法：Youban.exe --slice <多格圖.png> <桌寵資料夾名> <名稱1> <名稱2> ...");
                std::process::exit(2);
            };
            let names: Vec<String> = args.iter().skip(4).cloned().collect();
            if let Err(e) = slicer::run(std::path::Path::new(sheet), pet, &names) {
                eprintln!("slice failed: {e}");
                std::process::exit(1);
            }
        }
        Some(FRAMES_FLAG) => {
            let out = arg(2).map(std::path::PathBuf::from).unwrap_or_else(|| "frames".into());
            let dpi = arg(3).and_then(|d| d.parse().ok()).unwrap_or(SNAPSHOT_DEFAULT_DPI);
            if let Err(e) = preview::frames(&out, dpi, arg(4)) {
                eprintln!("frames failed: {e}");
                std::process::exit(1);
            }
        }
        Some(SNAPSHOT_FLAG) | Some(SNAPSHOT_CARDS_FLAG) => {
            let out = arg(2).map(std::path::PathBuf::from).unwrap_or_else(|| "snapshot.png".into());
            let dpi = arg(3).and_then(|d| d.parse().ok()).unwrap_or(SNAPSHOT_DEFAULT_DPI);
            let result = if arg(1) == Some(SNAPSHOT_FLAG) { snapshot::pet(&out, dpi) } else { snapshot::cards(&out, dpi) };
            if let Err(e) = result {
                eprintln!("snapshot failed: {e}");
                std::process::exit(1);
            }
        }
        Some(SEND_FLAG) => std::process::exit(if send(arg(2).unwrap_or("{}")) { 0 } else { 1 }),
        Some(QUIT_FLAG) => std::process::exit(if send(&serde_json::json!({ "cmd": petkit::ipc::CMD_QUIT }).to_string()) { 0 } else { 1 }),
        Some(AUTOSTART_FLAG) => std::process::exit(match autostart::cli(arg(2)) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("{e}");
                1
            }
        }),
        other => {
            let soak_secs = (other == Some(SOAK_FLAG)).then(|| arg(2).and_then(|s| s.parse().ok()).unwrap_or(SOAK_DEFAULT_SECS));
            run_app(soak_secs, other == Some(updater::AFTER_UPDATE_FLAG));
        }
    }
}

fn run_app(soak_secs: Option<u64>, after_update: bool) {
    unsafe {
        // 只能有一隻在跑：第二次啟動直接結束。更新後的新核心例外：等舊的結束再接手。
        let guard = CreateMutexW(None, true, w!("Local\\YoubanDesktopPet.Core"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            // 舊核心結束時沒有放開，所以拿到的會是「被放棄」（WAIT_ABANDONED），一樣算接手成功。
            let taken = after_update && guard.as_ref().is_ok_and(|h| matches!(WaitForSingleObject(*h, AFTER_UPDATE_WAIT_MS), WAIT_OBJECT_0 | WAIT_ABANDONED));
            if !taken {
                log!("another instance is running; exiting");
                return;
            }
        }
        std::mem::forget(guard);
    }
    log!("start");
    if let Err(e) = app::run(soak_secs, after_update) {
        log!("fatal: {e}");
        std::process::exit(1);
    }
}

/// 把一段 JSON 指令送給正在跑的桌寵（設定程式與自動測試用）。
fn send(json: &str) -> bool {
    unsafe {
        let Ok(hwnd) = FindWindowW(app::pet_class(), PCWSTR::null()) else {
            eprintln!("desktop pet is not running");
            return false;
        };
        let data = COPYDATASTRUCT { dwData: 0, cbData: json.len() as u32, lpData: json.as_ptr() as *mut _ };
        SendMessageW(hwnd, WM_COPYDATA, Some(WPARAM(0)), Some(LPARAM(&data as *const _ as isize)));
        true
    }
}
