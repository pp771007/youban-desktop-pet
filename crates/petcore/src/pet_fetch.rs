//! 現在這隻的動作圖還沒下載、或新版改過它的圖時，在背景下載（`petkit::pet_download`），好了通知主執行緒換上。
//! 平常是設定程式在使用者點選時先下載好；這裡補的是設定以外的路：更新後圖改過、
//! 設定檔跟著帳號漫遊到另一台還沒下載過的電腦。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::thread;

use petkit::pet_download;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use crate::log;

pub const WM_PET_DOWNLOADED: u32 = WM_APP + 6;

/// 正在下載的桌寵：使用者來回切換時同一隻不會同時抓兩份。
static IN_FLIGHT: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());

pub fn spawn(dir: PathBuf, notify: HWND) {
    if !IN_FLIGHT
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(dir.clone())
    {
        return;
    }
    let target = notify.0 as isize;
    let worker_dir = dir.clone();
    let spawned = thread::Builder::new()
        .name("pet-download".into())
        .spawn(move || {
            match pet_download::download(&worker_dir) {
                Ok(()) => {
                    log!("pet downloaded: {}", worker_dir.display());
                    unsafe {
                        let _ = PostMessageW(
                            Some(HWND(target as _)),
                            WM_PET_DOWNLOADED,
                            WPARAM(0),
                            LPARAM(0),
                        );
                    }
                }
                // 沒網路就先用手上有的（舊圖或預設那隻），下次啟動再試。
                Err(e) => log!("pet download failed ({}): {e}", worker_dir.display()),
            }
            finish(&worker_dir);
        });
    if let Err(e) = spawned {
        log!("pet download thread failed to start: {e}");
        finish(&dir);
    }
}

fn finish(dir: &Path) {
    IN_FLIGHT
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(dir);
}
