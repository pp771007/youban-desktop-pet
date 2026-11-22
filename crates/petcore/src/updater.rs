//! 自動更新：每天看一次 GitHub Releases 有沒有新版。有就在背景下載、解壓到暫存資料夾，
//! 通知主執行緒；主執行緒冒一張卡問使用者「現在更新」還是「下次啟動時」。
//! 現在更新 = 換檔、開新的核心、自己結束（約一秒，桌寵回到原位）；下次啟動時 = 下次核心一啟動就先換。
//!
//! 免安裝版（zip）換檔的做法：Windows 不能覆寫執行中的 exe，但可以把它改名。所以先把舊的 exe 改成 `.old`，
//! 把新檔複製進來，下次啟動再把 `.old` 刪掉。新核心帶 `--after-update` 啟動，會等舊的結束才接手。
//!
//! 安裝版（安裝資料夾裡有 Inno Setup 的解除安裝程式）改下載新版的安裝程式、在背景安靜地跑：
//! 直接換檔的話「應用程式」清單的版本號不會變，新版多出的檔案解除安裝時也刪不掉。
//! 安裝程式裝完會自己把柚伴開回來（`installer/youban.iss` 的 `IsUpdateRelaunch`）。

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime};

use petkit::config::write_atomic;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use crate::log;

pub const WM_UPDATE_READY: u32 = WM_APP + 4;
/// 新核心帶這個參數啟動：等舊的結束、顯示「已更新」。
pub const AFTER_UPDATE_FLAG: &str = "--after-update";
const LATEST_URL: &str = "https://api.github.com/repos/pp771007/youban-desktop-pet/releases/latest";
/// 測試用：把「最新版資訊」改指到別的網址（本機假伺服器）。設了它，從編譯資料夾跑也會更新。
pub const URL_ENV: &str = "YOUBAN_DESKTOP_PET_UPDATE_URL";
pub const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 3600);
const HTTP_TIMEOUT: Duration = Duration::from_secs(60);
const MARKER: &str = "last-check";
const STAGED_DIR: &str = "staged";
/// Release 附件的檔名開頭；`scripts/package.ps1` 打包出來的就是這個名字。
const ASSET_PREFIX: &str = "youban-desktop-pet-";
const ASSET_EXT: &str = ".zip";
const SETUP_ASSET_PREFIX: &str = "youban-desktop-pet-setup-";
const SETUP_ASSET_EXT: &str = ".exe";
/// 有這支 = 用安裝程式裝的。
const UNINSTALLER: &str = "unins000.exe";
/// 下載下來的安裝程式在 staged 裡的名字。
const SETUP_FILE: &str = "setup.exe";
/// 不顯示畫面、不問問題；`/relaunch=1` 讓安裝程式裝完把柚伴開回來；
/// `!desktopicon`：使用者刪掉的桌面捷徑不要每次更新又建回來（VS Code 的背景更新也這樣帶）。
const SETUP_ARGS: [&str; 5] = ["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/relaunch=1", "/MERGETASKS=!desktopicon"];
/// 背景安裝的紀錄（放在 update 資料夾）：裝失敗、柚伴沒回來時查原因用。
const SETUP_LOG: &str = "setup.log";
const OLD_EXT: &str = "old";
/// 解好的新版旁邊記一下版本號（放在 staged 外面，換檔時不會被複製進安裝資料夾）。
const VERSION_FILE: &str = "staged-version";
/// 新版的包裡一定要有這支，沒有就是壞的包。
const CORE_EXE: &str = petkit::paths::CORE_EXE;
const MAX_DOWNLOAD_BYTES: u64 = 100 * 1024 * 1024;

pub fn due(dir: &Path) -> bool {
    fs::metadata(dir.join(MARKER)).and_then(|m| m.modified()).ok().and_then(|t| SystemTime::now().duration_since(t).ok()).is_none_or(|age| age >= CHECK_INTERVAL)
}

/// 從 `cargo build` 的輸出資料夾跑的是開發中的版本，不要被網路上的正式版蓋掉。
pub fn enabled_for(install_dir: &Path) -> bool {
    std::env::var_os(URL_ENV).is_some() || !install_dir.components().any(|c| c.as_os_str() == "target")
}

/// `v1.2.3` / `1.2.3` → 比目前版本新嗎（數字逐段比）。
pub fn newer(tag: &str, current: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> { s.trim_start_matches('v').split('.').map(|p| p.parse().unwrap_or(0)).collect() };
    parse(tag) > parse(current)
}

/// 解好的新版是哪一版（給卡片顯示）。
pub fn staged_version(dir: &Path) -> Option<String> {
    fs::read_to_string(dir.join(VERSION_FILE)).ok().map(|s| s.trim().trim_start_matches('v').to_owned())
}

fn installed(install_dir: &Path) -> bool {
    install_dir.join(UNINSTALLER).is_file()
}

pub fn spawn(dir: PathBuf, install_dir: &Path, notify: HWND) {
    let target = notify.0 as isize;
    let setup = installed(install_dir);
    let spawned = thread::Builder::new().name("updater".into()).spawn(move || {
        match check_and_stage(&dir, setup) {
            Ok(Some(version)) => {
                log!("update {version} staged");
                unsafe {
                    let _ = PostMessageW(Some(HWND(target as _)), WM_UPDATE_READY, WPARAM(0), LPARAM(0));
                }
            }
            Ok(None) => log!("no update"),
            // 沒網路、GitHub 掛了就下次再試，現在的版本照樣能用。
            Err(e) => log!("update check failed: {e}"),
        }
        let _ = write_atomic(&dir.join(MARKER), b"");
    });
    if let Err(e) = spawned {
        log!("updater thread failed to start: {e}");
    }
}

/// 解好的新版放在哪（主執行緒換檔時用）；有完整的包、而且真的比目前新才回傳
/// （手動裝了更新的版本之後，留著的舊暫存不能把它蓋回去）。
pub fn staged(dir: &Path) -> Option<PathBuf> {
    let p = dir.join(STAGED_DIR);
    let newer_than_me = staged_version(dir).is_some_and(|v| newer(&v, env!("CARGO_PKG_VERSION")));
    ((p.join(CORE_EXE).is_file() || p.join(SETUP_FILE).is_file()) && newer_than_me).then_some(p)
}

/// 已經裝上（或比目前舊）的暫存刪掉：安裝版的安裝程式跑完會留在 staged 裡。
/// 剛更新完時安裝程式可能還沒結束、刪不掉，下次啟動再刪。
pub fn discard_stale(dir: &Path) {
    if staged(dir).is_none() {
        let _ = fs::remove_dir_all(dir.join(STAGED_DIR));
        let _ = fs::remove_file(dir.join(VERSION_FILE));
    }
}

/// 換上新版，再讓新的核心接手。成功的話呼叫端要自己結束。
/// 安裝版：開新版的安裝程式（它會等這支結束再換檔，裝完自己把柚伴開回來）。
/// 免安裝版：換檔，再開新的核心（帶 `--after-update`）。
pub fn apply_and_restart(staged: &Path, install_dir: &Path) -> io::Result<()> {
    let setup = staged.join(SETUP_FILE);
    if setup.is_file() {
        let log = staged.parent().unwrap_or(staged).join(SETUP_LOG);
        std::process::Command::new(setup).args(SETUP_ARGS).arg(format!("/LOG={}", log.display())).spawn()?;
        return Ok(());
    }
    apply(staged, install_dir)?;
    std::process::Command::new(install_dir.join(CORE_EXE)).arg(AFTER_UPDATE_FLAG).spawn()?;
    Ok(())
}

fn agent() -> ureq::Agent {
    let tls = ureq::tls::TlsConfig::builder().root_certs(ureq::tls::RootCerts::PlatformVerifier).build();
    ureq::Agent::config_builder().timeout_global(Some(HTTP_TIMEOUT)).tls_config(tls).user_agent("youban-desktop-pet-updater").build().into()
}

fn check_and_stage(dir: &Path, setup: bool) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let agent = agent();
    let url = std::env::var(URL_ENV).unwrap_or_else(|_| LATEST_URL.to_owned());
    let latest: serde_json::Value = serde_json::from_str(&agent.get(&url).call()?.body_mut().read_to_string()?)?;
    let tag = latest["tag_name"].as_str().ok_or("no tag_name")?;
    if !newer(tag, env!("CARGO_PKG_VERSION")) {
        return Ok(None);
    }
    let asset = asset_url(&latest, setup).ok_or("release has no package for this install")?;
    let mut bytes = Vec::new();
    agent.get(asset).call()?.body_mut().as_reader().take(MAX_DOWNLOAD_BYTES).read_to_end(&mut bytes)?;
    let staged = dir.join(STAGED_DIR);
    if staged.exists() {
        fs::remove_dir_all(&staged)?;
    }
    fs::create_dir_all(&staged)?;
    if setup {
        fs::write(staged.join(SETUP_FILE), &bytes)?;
    } else {
        zip::ZipArchive::new(io::Cursor::new(bytes))?.extract(&staged)?;
    }
    if !setup && !staged.join(CORE_EXE).is_file() {
        fs::remove_dir_all(&staged)?;
        return Err(format!("zip has no {CORE_EXE}").into());
    }
    write_atomic(&dir.join(VERSION_FILE), tag.as_bytes())?;
    Ok(Some(tag.to_owned()))
}

/// Release 附件裡該下載哪一個：安裝版拿安裝程式，免安裝版拿 zip。
fn asset_url(latest: &serde_json::Value, setup: bool) -> Option<&str> {
    let (prefix, ext) = if setup { (SETUP_ASSET_PREFIX, SETUP_ASSET_EXT) } else { (ASSET_PREFIX, ASSET_EXT) };
    latest["assets"].as_array()?.iter().find(|a| a["name"].as_str().is_some_and(|n| n.starts_with(prefix) && n.ends_with(ext)))?["browser_download_url"].as_str()
}

/// 把解好的新版複製進安裝資料夾。執行中的 exe 先改名成 `.old` 讓位；中途失敗就把改名的換回來。
/// 使用者自己放進 `pets` 的桌寵不會被動到（只覆寫包裡有的檔）。
pub fn apply(staged: &Path, install_dir: &Path) -> io::Result<()> {
    let mut moved: Vec<PathBuf> = Vec::new();
    let result = copy_tree(staged, install_dir, &mut moved);
    if result.is_err() {
        for dest in &moved {
            let _ = fs::rename(dest.with_extension(OLD_EXT), dest);
        }
        return result;
    }
    fs::remove_dir_all(staged)
}

fn copy_tree(from: &Path, to: &Path, moved: &mut Vec<PathBuf>) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let dest = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &dest, moved)?;
            continue;
        }
        if dest.extension().is_some_and(|e| e == "exe") && dest.exists() {
            let old = dest.with_extension(OLD_EXT);
            let _ = fs::remove_file(&old);
            fs::rename(&dest, &old)?;
            moved.push(dest.clone());
        }
        fs::copy(entry.path(), &dest)?;
    }
    Ok(())
}

/// 上次更新留下的 `.old`（舊核心已經結束了，這時刪得掉）。
pub fn cleanup(install_dir: &Path) {
    let Ok(entries) = fs::read_dir(install_dir) else { return };
    for e in entries.flatten() {
        if e.path().extension().is_some_and(|x| x == OLD_EXT) {
            let _ = fs::remove_file(e.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions_by_number() {
        assert!(newer("v0.2.0", "0.1.9"));
        assert!(newer("0.10.0", "0.9.0"), "10 > 9, not text order");
        assert!(!newer("v0.1.0", "0.1.0"));
        assert!(!newer("v0.0.9", "0.1.0"));
    }

    #[test]
    fn installed_copy_updates_with_the_installer() {
        let latest = serde_json::json!({ "assets": [
            { "name": "youban-desktop-pet-setup-0.3.0.exe", "browser_download_url": "setup" },
            { "name": "youban-desktop-pet-setup-0.3.0.exe.sha256", "browser_download_url": "setup-hash" },
            { "name": "youban-desktop-pet-0.3.0.zip", "browser_download_url": "zip" },
            { "name": "youban-desktop-pet-0.3.0.zip.sha256", "browser_download_url": "zip-hash" },
        ]});
        assert_eq!(asset_url(&latest, true), Some("setup"));
        assert_eq!(asset_url(&latest, false), Some("zip"));
    }

    #[test]
    fn apply_swaps_files_and_keeps_user_pets() {
        let root = std::env::temp_dir().join(format!("petcore-updater-{}", std::process::id()));
        let (staged, install) = (root.join("staged"), root.join("install"));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(staged.join("pets/capybara")).unwrap();
        fs::create_dir_all(install.join("pets/my-cat")).unwrap();
        fs::write(staged.join(CORE_EXE), b"new").unwrap();
        fs::write(staged.join("pets/capybara/pet.json"), b"{}").unwrap();
        fs::write(install.join(CORE_EXE), b"old").unwrap();
        fs::write(install.join("pets/my-cat/pet.json"), b"cat").unwrap();

        apply(&staged, &install).unwrap();

        assert_eq!(fs::read(install.join(CORE_EXE)).unwrap(), b"new");
        assert_eq!(fs::read(install.join(CORE_EXE).with_extension(OLD_EXT)).unwrap(), b"old", "running exe moved aside, not overwritten");
        assert!(install.join("pets/capybara/pet.json").is_file());
        assert_eq!(fs::read(install.join("pets/my-cat/pet.json")).unwrap(), b"cat", "user's own pet untouched");
        assert!(!staged.exists());
        cleanup(&install);
        assert!(!install.join(CORE_EXE).with_extension(OLD_EXT).exists());
        let _ = fs::remove_dir_all(&root);
    }
}
