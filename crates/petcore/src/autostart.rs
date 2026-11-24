//! 開機自動啟動：跟一般程式一樣寫進目前使用者的「執行」登錄機碼
//! （`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`）。
//! 不用系統管理員權限、不跳 UAC，也會出現在工作管理員的「啟動應用程式」裡，使用者在那邊一樣關得掉。
//! 代價：開機自動啟動的柚伴是一般權限，視窗歸位搬不動以系統管理員身分執行的視窗
//! （那張結果卡會寫出有幾個搬不動）；要搬就自己以系統管理員身分開柚伴。

use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use windows::core::{PCWSTR, w};

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const VALUE_NAME: PCWSTR = w!("YoubanDesktopPet");

pub fn cli(arg: Option<&str>) -> Result<(), String> {
    match arg {
        Some("on") => enable(),
        Some("off") => disable(),
        Some("status") => {
            println!("{}", if is_enabled() { "on" } else { "off" });
            Ok(())
        }
        _ => Err("用法：Youban.exe --autostart on|off|status".into()),
    }
}

/// 登錄裡有，而且指的是這一支執行檔（程式被搬走的話，舊的那筆不算）。
pub fn is_enabled() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let mut buf = vec![0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    let ok = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            VALUE_NAME,
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        )
    }
    .is_ok();
    if !ok {
        return false;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
        .trim_matches('"')
        .eq_ignore_ascii_case(&exe.to_string_lossy())
}

fn enable() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let data: Vec<u16> = format!("\"{}\"", exe.display())
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            VALUE_NAME,
            REG_SZ.0,
            Some(data.as_ptr().cast()),
            (data.len() * 2) as u32,
        )
    }
    .ok()
    .map_err(|e| format!("寫不進登錄（{}）", e.message()))
}

fn disable() -> Result<(), String> {
    let r = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, VALUE_NAME) };
    // 本來就沒有也算成功。
    if r.is_ok() || r == windows::Win32::Foundation::ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        Err(format!("刪不掉登錄（{}）", r.to_hresult().message()))
    }
}
