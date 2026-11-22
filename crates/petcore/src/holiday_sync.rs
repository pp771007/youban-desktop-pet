//! 背景下載人事總處最新的辦公日曆（隔年的每年 6～7 月才公布，公布後也可能修正）。
//! 跑完就結束的一次性執行緒；有更新就通知主執行緒重讀。

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime};

use chrono::Datelike;
use petkit::config::write_atomic;
use petkit::holidays::parse_dgpa_csv;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use crate::log;

pub const WM_HOLIDAYS_UPDATED: u32 = WM_APP + 3;
const DATASET_URL: &str = "https://data.gov.tw/api/v2/rest/dataset/14718";
const TITLE_MARK: &str = "辦公日曆表";
/// 同一份資料另有「Google 行事曆專用」版，欄位不同，不要那份。
const EXCLUDE_MARK: &str = "Google";
const ROC_OFFSET: i32 = 1911;
/// 多久檢查一次。
pub const SYNC_INTERVAL: Duration = Duration::from_secs(7 * 24 * 3600);
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const MARKER: &str = "last-sync";
/// 一年至少要有這麼多天才算完整的日曆（防止下載到一半的檔）。
const MIN_DAYS: usize = 365;

/// 上次檢查超過 `SYNC_INTERVAL` 才真的去下載。
pub fn due(cache: &Path) -> bool {
    let marker = cache.join(MARKER);
    fs::metadata(marker).and_then(|m| m.modified()).ok().and_then(|t| SystemTime::now().duration_since(t).ok()).is_none_or(|age| age >= SYNC_INTERVAL)
}

pub fn spawn(cache: PathBuf, notify: HWND) {
    let target = notify.0 as isize;
    let spawned = thread::Builder::new().name("holiday-sync".into()).spawn(move || match sync(&cache) {
        Ok(changed) => {
            let _ = write_atomic(&cache.join(MARKER), b"");
            log!("holiday sync done; {changed} year(s) updated");
            if changed > 0 {
                unsafe {
                    let _ = PostMessageW(Some(HWND(target as _)), WM_HOLIDAYS_UPDATED, WPARAM(0), LPARAM(0));
                }
            }
        }
        // 沒網路就算了，下次再試；內建的日曆照樣能用。
        Err(e) => log!("holiday sync failed: {e}"),
    });
    if let Err(e) = spawned {
        log!("holiday sync thread failed to start: {e}");
    }
}

/// 憑證用 Windows 的憑證庫驗：有 HTTPS 檢查代理的網路，代理憑證只裝在系統裡，
/// 用內建的 Mozilla 根憑證會回 `UnknownIssuer`（實測）。
fn agent() -> ureq::Agent {
    let tls = ureq::tls::TlsConfig::builder().root_certs(ureq::tls::RootCerts::PlatformVerifier).build();
    ureq::Agent::config_builder().timeout_global(Some(HTTP_TIMEOUT)).tls_config(tls).build().into()
}

fn sync(cache: &Path) -> Result<usize, Box<dyn std::error::Error>> {
    let agent = agent();
    let meta: serde_json::Value = serde_json::from_str(&agent.get(DATASET_URL).call()?.body_mut().read_to_string()?)?;
    let this_year = chrono::Local::now().year();
    let mut changed = 0;
    for dist in meta["result"]["distribution"].as_array().into_iter().flatten() {
        let title = dist["resourceDescription"].as_str().unwrap_or_default();
        let Some(url) = dist["resourceDownloadUrl"].as_str() else { continue };
        let Some(year) = roc_year(title) else { continue };
        if year < this_year {
            continue;
        }
        let text = agent.get(url).call()?.body_mut().read_to_string()?;
        let days = parse_dgpa_csv(&text)?;
        if days.len() < MIN_DAYS || days.iter().any(|d| d.date.year() != year) {
            log!("holiday {year}: unexpected content, skipped");
            continue;
        }
        let path = cache.join(format!("{year}.csv"));
        if fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
            write_atomic(&path, text.as_bytes())?;
            changed += 1;
        }
    }
    Ok(changed)
}

/// 「116年中華民國政府行政機關辦公日曆表」→ 2027；Google 版或其他資料 → `None`。
fn roc_year(title: &str) -> Option<i32> {
    if !title.contains(TITLE_MARK) || title.contains(EXCLUDE_MARK) {
        return None;
    }
    let digits: String = title.chars().take_while(char::is_ascii_digit).collect();
    digits.parse::<i32>().ok().map(|roc| roc + ROC_OFFSET)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_standard_csv_and_converts_roc_year() {
        assert_eq!(roc_year("116年中華民國政府行政機關辦公日曆表"), Some(2027));
        assert_eq!(roc_year("114年中華民國政府行政機關辦公日曆表(修正版)"), Some(2025));
        assert_eq!(roc_year("116年中華民國政府行政機關辦公日曆表_Google行事曆專用"), None);
        assert_eq!(roc_year("其他資料"), None);
    }
}
