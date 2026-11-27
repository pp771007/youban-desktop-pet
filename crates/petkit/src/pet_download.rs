//! 安裝包裡只有預設那隻帶動作圖；其他內建桌寵只帶名字和選的時候看的那張圖（`pet.json`、`thumb.png`），
//! 動作圖第一次用到才從同一版的 GitHub Release 下載（`scripts/package.ps1` 把每隻打包成 `pet-<id>.zip`）。
//!
//! 安裝包裡的 `frames.sha256` 記著這一版動作圖的內容；下載好的圖旁邊抄一份一樣的。
//! 兩份對不上 = 新版改過這隻的圖（或根本還沒下載），要重抓。沒有 `frames.sha256` 的桌寵
//! （預設那隻、使用者自己做的、開發時 repo 裡的）動作圖本來就在，不用下載。

use std::error::Error;
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::pack::{self, FRAMES_DIR};
use crate::states;

/// 安裝包裡、每隻要下載的桌寵資料夾裡：這一版動作圖的雜湊（`scripts/package.ps1` 寫的）。
pub const HASH_FILE: &str = "frames.sha256";
/// 下載好的 `frames/` 裡：下載時抄下來的雜湊。
const DOWNLOADED_HASH_FILE: &str = "downloaded.sha256";
const RELEASE_URL: &str = "https://github.com/pp771007/youban-desktop-pet/releases/download";
/// Release 附件的檔名：`pet-<id>.zip`，跟 `scripts/package.ps1` 一致；不能用 `youban-desktop-pet-` 開頭（那是自動更新要抓的包）。
const ASSET_PREFIX: &str = "pet-";
const ASSET_EXT: &str = ".zip";
/// 測試用：改從這個網址底下抓 `pet-<id>.zip`（本機假伺服器）。
pub const URL_ENV: &str = "YOUBAN_DESKTOP_PET_PETS_URL";
const HTTP_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_DOWNLOAD_BYTES: u64 = 50 * 1024 * 1024;

pub type DownloadError = Box<dyn Error + Send + Sync>;

fn read_hash(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

/// 這隻的動作圖要（重新）下載嗎。
pub fn needs_download(dir: &Path) -> bool {
    let Some(want) = read_hash(&dir.join(HASH_FILE)) else {
        return false;
    };
    read_hash(&dir.join(FRAMES_DIR).join(DOWNLOADED_HASH_FILE)).as_deref() != Some(want.as_str())
}

/// 不下載就用不了（還沒抓過）。圖改過但舊的還在的話照樣能用，背景重抓就好。
pub fn must_download_first(dir: &Path) -> bool {
    needs_download(dir) && pack::load(dir).is_err()
}

/// 下載過的動作圖可以刪（要用時再下載）。跟著程式發的、使用者自己做的不算。
pub fn removable(dir: &Path) -> bool {
    dir.join(HASH_FILE).is_file() && dir.join(FRAMES_DIR).is_dir()
}

/// 刪掉下載的動作圖，桌寵回到「要下載」的樣子（名字和預覽圖留著）。
pub fn remove(dir: &Path) -> std::io::Result<()> {
    if !removable(dir) {
        return Err(std::io::Error::other("這隻桌寵沒有下載過的圖"));
    }
    fs::remove_dir_all(dir.join(FRAMES_DIR))
}

/// 跟這支程式同一版的 Release：桌寵包的格式一定對得上。
fn url(id: &str) -> String {
    let base = std::env::var(URL_ENV)
        .unwrap_or_else(|_| format!("{RELEASE_URL}/v{}", env!("CARGO_PKG_VERSION")));
    format!(
        "{}/{ASSET_PREFIX}{id}{ASSET_EXT}",
        base.trim_end_matches('/')
    )
}

/// 憑證用 Windows 的憑證庫驗：有 HTTPS 檢查代理的網路，代理憑證只裝在系統裡（同 `holiday_sync`）。
fn agent() -> ureq::Agent {
    let tls = ureq::tls::TlsConfig::builder()
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .build();
    ureq::Agent::config_builder()
        .timeout_global(Some(HTTP_TIMEOUT))
        .tls_config(tls)
        .user_agent("youban-desktop-pet")
        .build()
        .into()
}

/// 下載這隻的動作圖、換進 `<dir>/frames/`。會等到下載完才回來，在背景執行緒呼叫。
pub fn download(dir: &Path) -> Result<(), DownloadError> {
    let id = dir
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("桌寵資料夾名稱不對")?;
    let mut bytes = Vec::new();
    agent()
        .get(url(id))
        .call()?
        .body_mut()
        .as_reader()
        .take(MAX_DOWNLOAD_BYTES)
        .read_to_end(&mut bytes)?;
    install(dir, &bytes)
}

/// 把下載的 zip（裡面是 `frames/…`）換進桌寵資料夾。先解到旁邊的暫存資料夾，
/// 確定有站姿圖才換上去：下載到一半斷掉的話，原本的圖（舊版的也好）照樣能用。
pub fn install(dir: &Path, zip: &[u8]) -> Result<(), DownloadError> {
    let want = read_hash(&dir.join(HASH_FILE)).ok_or("這隻桌寵不用下載")?;
    let pid = std::process::id();
    let part = dir.join(format!("download-{pid}.part"));
    let _ = fs::remove_dir_all(&part);
    let result = unpack(&part, zip, &want).and_then(|frames| swap_in(dir, &frames, pid));
    let _ = fs::remove_dir_all(&part);
    result
}

fn unpack(part: &Path, zip: &[u8], hash: &str) -> Result<PathBuf, DownloadError> {
    zip::ZipArchive::new(Cursor::new(zip))?.extract(part)?;
    let frames = part.join(FRAMES_DIR);
    let has_idle = fs::read_dir(&frames)?.flatten().any(|e| {
        e.file_name()
            .to_str()
            .and_then(pack::parse_frame_name)
            .is_some_and(|(state, _, _)| state == states::IDLE)
    });
    if !has_idle {
        return Err("下載的檔案裡沒有站姿圖".into());
    }
    fs::write(frames.join(DOWNLOADED_HASH_FILE), hash)?;
    Ok(frames)
}

fn swap_in(dir: &Path, frames: &Path, pid: u32) -> Result<(), DownloadError> {
    let dest = dir.join(FRAMES_DIR);
    if !dest.exists() {
        fs::rename(frames, &dest)?;
        return Ok(());
    }
    let old = dir.join(format!("frames-{pid}.old"));
    let _ = fs::remove_dir_all(&old);
    fs::rename(&dest, &old)?;
    if let Err(e) = fs::rename(frames, &dest) {
        let _ = fs::rename(&old, &dest);
        return Err(e.into());
    }
    let _ = fs::remove_dir_all(&old);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn stub(name: &str, hash: Option<&str>) -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("petkit-download-{}-{name}", std::process::id()))
            .join("dolphin");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(pack::MANIFEST_FILE), r#"{ "name": "海豚" }"#).unwrap();
        if let Some(h) = hash {
            fs::write(dir.join(HASH_FILE), h).unwrap();
        }
        dir
    }

    fn zip_of(files: &[&str]) -> Vec<u8> {
        let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for f in files {
            out.start_file(
                format!("{FRAMES_DIR}/{f}"),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            out.write_all(b"png").unwrap();
        }
        out.finish().unwrap().into_inner()
    }

    #[test]
    fn pets_shipped_with_frames_never_download() {
        assert!(!needs_download(&stub("bundled", None)));
    }

    #[test]
    fn downloaded_frames_make_the_pet_usable_until_its_art_changes() {
        let dir = stub("cycle", Some("v1"));
        assert!(needs_download(&dir) && must_download_first(&dir));

        install(&dir, &zip_of(&["idle_01.png", "walk_01.png"])).unwrap();
        assert!(!needs_download(&dir));
        assert!(pack::load(&dir).is_ok());

        // 新版改了這隻的圖：要重抓，但舊圖照樣能先用。
        fs::write(dir.join(HASH_FILE), "v2").unwrap();
        assert!(needs_download(&dir) && !must_download_first(&dir));
        install(&dir, &zip_of(&["idle_01.png"])).unwrap();
        assert!(!needs_download(&dir));
        assert!(
            !dir.join(FRAMES_DIR).join("walk_01.png").exists(),
            "舊圖整包換掉"
        );
    }

    #[test]
    fn removing_a_download_brings_back_the_download_button() {
        let dir = stub("remove", Some("v1"));
        assert!(!removable(&dir), "還沒下載");
        install(&dir, &zip_of(&["idle_01.png"])).unwrap();
        assert!(removable(&dir));

        remove(&dir).unwrap();
        assert!(must_download_first(&dir));
        assert!(dir.join(pack::MANIFEST_FILE).is_file(), "名字留著");

        // 從整包都帶動作圖的舊版更新上來：舊圖還在、沒有下載紀錄，但新版放了 frames.sha256。
        let upgraded = stub("remove-upgraded", Some("v1"));
        fs::create_dir_all(upgraded.join(FRAMES_DIR)).unwrap();
        fs::write(upgraded.join(FRAMES_DIR).join("idle_01.png"), b"png").unwrap();
        assert!(removable(&upgraded), "舊使用者也刪得掉");

        let bundled = stub("remove-bundled", None);
        fs::create_dir_all(bundled.join(FRAMES_DIR)).unwrap();
        assert!(!removable(&bundled) && remove(&bundled).is_err());
        assert!(bundled.join(FRAMES_DIR).is_dir(), "跟著程式發的不刪");
    }

    #[test]
    fn broken_download_keeps_the_old_frames() {
        let dir = stub("broken", Some("v1"));
        install(&dir, &zip_of(&["idle_01.png"])).unwrap();
        fs::write(dir.join(HASH_FILE), "v2").unwrap();

        assert!(install(&dir, &zip_of(&["walk_01.png"])).is_err());
        assert!(install(&dir, b"not a zip").is_err());
        assert!(pack::load(&dir).is_ok());
        assert!(needs_download(&dir), "沒換成功就還是要重抓");
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(leftovers.len(), 3, "暫存資料夾要清掉：{leftovers:?}");
    }
}
