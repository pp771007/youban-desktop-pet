//! 桌寵設定程式：用到才開、關掉就整個結束（瀏覽器核心只在這時候佔記憶體）。
//! 跟核心溝通只有兩條路：寫設定檔（核心每秒看一次有沒有變），以及 WM_COPYDATA 送指令。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod importer;

use std::collections::BTreeMap;
use std::process::Command;

use base64::Engine;
use chrono::NaiveDate;
use petkit::companion::CompanionFile;
use petkit::config::{ColorScheme, Config, load_json};
use petkit::focus_status::FocusStatus;
use petkit::holidays::{self, DayKind};
use petkit::paths;
use petkit::relay_log::RelayLog;
use petkit::reminders::{PreviewItem, Reminder, ReminderFile};
use petkit::update_status::UpdateStatus;
use petkit::{anniversary, ipc, pack, pack_edit, states};
use serde::Serialize;
use tauri::State;
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    ASFW_ANY, AllowSetForegroundWindow, FindWindowW, SW_RESTORE, SetForegroundWindow, ShowWindow,
};
use windows::core::{HSTRING, PCWSTR, w};

/// 也用來找已經開著的設定視窗（只開一個）；跟 `tauri.conf.json` 的視窗 title、`ui/index.html` 的 `<title>` 一致。
const WINDOW_TITLE: &str = "柚伴設定";
const PAGE_FLAG: &str = "--page";
const PREVIEW_COUNT: usize = 5;
const AUTOSTART_FLAG: &str = "--autostart";
/// 畫面讀桌寵圖檔用的網址開頭（WebView2 上自訂的 `pet://` 長這樣）；`tauri.conf.json` 的 CSP 要放行它。
const PET_SCHEME: &str = "pet";
const PET_FILE_ORIGIN: &str = "http://pet.localhost";

struct Init {
    page: Option<String>,
}

#[derive(Serialize)]
struct PetInfo {
    id: String,
    /// 使用者取的名字，沒取就跟 `species` 一樣。
    name: String,
    /// 桌寵包自己的名字（水豚、貓咪…）：名字欄空著時顯示的預設值。
    species: String,
    image: String,
    editable: bool,
    /// 陪伴天數（核心寫的 `companion.json`，這裡只讀）。
    days: u32,
}

/// 動作編輯畫面的一組圖：某個動作的某一組（待機可以有好幾組），依播放順序。
#[derive(Serialize)]
struct ActionGroup {
    state: String,
    variant: String,
    frames: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PetActions {
    groups: Vec<ActionGroup>,
    /// 每個動作現在每秒播幾張（有自訂就是自訂，沒有就是規範預設）。
    fps: BTreeMap<String, f32>,
    /// 規範預設的速度（「慢／普通／快」以它為基準）。
    default_fps: BTreeMap<String, f32>,
}

/// 自己做的桌寵資料夾名稱的開頭（資料夾名稱使用者看不到，名字寫在 pet.json）。
const USER_PET_PREFIX: &str = "my-";
/// 範例圖從這隻內建桌寵拿。
const EXAMPLE_PET: &str = petkit::config::DEFAULT_PET;
/// 範例圖的幾張之間留多少空白（像素）。
const EXAMPLE_GAP: u32 = 24;
const MS_PER_SEC: f32 = 1000.0;

#[derive(Serialize)]
struct WindowInfo {
    id: i64,
    title: String,
    exe: String,
    left: i32,
    top: i32,
    width: i32,
    height: i32,
}

type CmdResult<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

#[tauri::command]
fn initial_page(init: State<Init>) -> Option<String> {
    init.page.clone()
}

#[tauri::command]
fn tokens() -> serde_json::Value {
    serde_json::from_str(include_str!("../../../design/tokens.json")).expect("tokens.json is valid")
}

/// 前端要跟 Rust 同一份的常數。
#[tauri::command]
fn constants() -> serde_json::Value {
    serde_json::json!({
        "sizes": petkit::config::SIZE_CHOICES_DIP,
        "version": env!("CARGO_PKG_VERSION"),
        "lunarMonths": anniversary::LUNAR_MONTHS,
        "lunarDays": anniversary::LUNAR_DAYS,
    })
}

/// 紀念日接下來幾次（列表的倒數、編輯面板的預覽），以及日期的寫法（「農曆三月初五」）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AnniversaryInfo {
    label: String,
    upcoming: Vec<anniversary::Occurrence>,
    /// 離下一次還有幾天（0 = 今天）。
    days_left: Option<i64>,
}

/// 編輯面板預覽幾次。
const ANNIVERSARY_PREVIEW_COUNT: usize = 3;

#[tauri::command]
fn anniversary_info(anniversary: anniversary::Anniversary) -> AnniversaryInfo {
    let today = chrono::Local::now().date_naive();
    let upcoming = anniversary.upcoming(today, ANNIVERSARY_PREVIEW_COUNT);
    let days_left = upcoming.first().map(|o| (o.date - today).num_days());
    AnniversaryInfo {
        label: anniversary.date.label(),
        upcoming,
        days_left,
    }
}

#[tauri::command]
fn load_config() -> CmdResult<Config> {
    Config::load(&paths::config_file()).map_err(err)
}

#[tauri::command]
fn save_config(window: tauri::WebviewWindow, config: Config) -> CmdResult<()> {
    apply_color_scheme(&window, config.color_scheme);
    config.save(&paths::config_file()).map_err(err)
}

#[tauri::command]
fn load_reminders() -> CmdResult<ReminderFile> {
    ReminderFile::load(&paths::reminders_file()).map_err(err)
}

#[tauri::command]
fn save_reminders(file: ReminderFile) -> CmdResult<()> {
    file.save(&paths::reminders_file()).map_err(err)
}

/// 只讀：要改就送指令給核心（`petkit::relay_log` 只有核心寫）。
#[tauri::command]
fn load_relay_log() -> CmdResult<RelayLog> {
    RelayLog::load(&paths::relay_log_file()).map_err(err)
}

#[tauri::command]
fn preview(reminder: Reminder, overrides: BTreeMap<NaiveDate, DayKind>) -> Vec<PreviewItem> {
    let (cal, _) = holidays::load_calendar(&paths::holiday_cache_dir(), overrides);
    reminder.preview(chrono::Local::now().naive_local(), &cal, PREVIEW_COUNT)
}

/// 從今天起官方日曆裡跟平常不一樣的日子（國定假日、補班），給提醒頁一天一天開關。
#[tauri::command]
fn official_days() -> Vec<holidays::CalendarDay> {
    let (cal, _) = holidays::load_calendar(&paths::holiday_cache_dir(), BTreeMap::new());
    cal.official_changes(chrono::Local::now().date_naive())
        .cloned()
        .collect()
}

#[tauri::command]
fn list_pets() -> Vec<PetInfo> {
    let Some(dir) = paths::pets_dir() else {
        return Vec::new();
    };
    let companions = CompanionFile::load(&paths::companion_file()).unwrap_or_default();
    let config = Config::load(&paths::config_file()).unwrap_or_default();
    let mut out: Vec<PetInfo> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let id = e.file_name().to_str()?.to_owned();
            let p = pack::load(&e.path()).ok()?;
            let image = pet_file_url(&dir, &p.states[states::IDLE].variants[0].frames[0])?;
            let days = companions.days(&id);
            Some(PetInfo {
                name: config.pet_name(&id, &p.name).to_owned(),
                species: p.name,
                id,
                image,
                editable: p.editable,
                days,
            })
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// 桌寵資料夾裡的圖給畫面用的網址：畫面直接讀檔（`serve_pet_file`），不用轉成 base64 從指令傳過去 ——
/// 五隻桌寵的站姿圖加起來超過 1 MB，轉碼加傳送會讓設定頁晚 0.3 秒以上才畫得出來。
fn pet_file_url(root: &std::path::Path, file: &std::path::Path) -> Option<String> {
    let rel = file.strip_prefix(root).ok()?;
    let parts: Vec<String> = rel
        .components()
        .map(|c| percent_encode(&c.as_os_str().to_string_lossy()))
        .collect();
    Some(format!("{PET_FILE_ORIGIN}/{}", parts.join("/")))
}

fn percent_encode(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

fn percent_decode(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(v) = b
                .get(i + 1..i + 3)
                .and_then(|h| std::str::from_utf8(h).ok())
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

/// `pet://` 的請求：只給桌寵資料夾裡的檔案（`..` 繞出去的一律 404）。
fn serve_pet_file(request: &tauri::http::Request<Vec<u8>>) -> tauri::http::Response<Vec<u8>> {
    let not_found = || {
        tauri::http::Response::builder()
            .status(404)
            .body(Vec::new())
            .expect("static response")
    };
    let Ok(root) = pets_root().and_then(|r| r.canonicalize().map_err(err)) else {
        return not_found();
    };
    let rel = String::from_utf8_lossy(&percent_decode(
        request.uri().path().trim_start_matches('/'),
    ))
    .into_owned();
    let Ok(path) = root.join(rel).canonicalize() else {
        return not_found();
    };
    if !path.starts_with(&root) {
        return not_found();
    }
    match std::fs::read(&path) {
        Ok(bytes) => tauri::http::Response::builder()
            .header("Content-Type", "image/png")
            .body(bytes)
            .expect("static response"),
        Err(_) => not_found(),
    }
}

fn data_url(png: &[u8]) -> String {
    format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png)
    )
}

fn decode_upload(data: &str) -> CmdResult<Vec<u8>> {
    let raw = data.split_once(',').map_or(data, |(_, b64)| b64);
    base64::engine::general_purpose::STANDARD
        .decode(raw)
        .map_err(err)
}

/// 使用者剛選的圖整理好（去背、裁切；四格圖會切開）之後的樣子，給畫面預覽。
#[tauri::command]
fn import_preview(data: String) -> CmdResult<Vec<String>> {
    Ok(importer::prepare_all(&decode_upload(&data)?)
        .map_err(err)?
        .iter()
        .map(|p| data_url(p))
        .collect())
}

/// 用一張圖做一隻新桌寵，回傳它的 id（呼叫端接著把它設成目前的桌寵）。
/// 給的是四格圖的話，四張都當成待機的連續動作。
#[tauri::command]
fn create_pet(name: String, data: String) -> CmdResult<String> {
    let pngs = importer::prepare_all(&decode_upload(&data)?).map_err(err)?;
    let pets = pets_root()?;
    let stamp = chrono::Local::now().format("%Y%m%d%H%M%S");
    let id = format!("{USER_PET_PREFIX}{stamp}");
    let dir = pets.join(&id);
    pack_edit::append(&dir, states::IDLE, pack::DEFAULT_VARIANT, &pngs).map_err(err)?;
    let manifest = serde_json::json!({ "spec": 1, "name": name.trim(), "editable": true });
    petkit::config::write_atomic(
        &dir.join(pack::MANIFEST_FILE),
        serde_json::to_string_pretty(&manifest)
            .map_err(err)?
            .as_bytes(),
    )
    .map_err(err)?;
    Ok(id)
}

/// 自己做的桌寵放在哪。測試時改到暫存資料夾，不會在 repo 的 pets/ 裡留下東西。
fn pets_root() -> CmdResult<std::path::PathBuf> {
    #[cfg(test)]
    if let Some(dir) = tests::PETS_ROOT.with(|r| r.borrow().clone()) {
        return Ok(dir);
    }
    paths::pets_dir().ok_or_else(|| "找不到 pets 資料夾".into())
}

fn editable_pet(id: &str) -> CmdResult<pack::PetPack> {
    let dir = pets_root()?.join(id);
    let p = pack::load(&dir).map_err(err)?;
    if !p.editable {
        return Err("內建的桌寵不能修改".into());
    }
    Ok(p)
}

/// 動作名稱與組名只收規範裡的動作、組名只收英數字（會變成檔名的一部分）。
fn check_action(state: &str, variant: &str) -> CmdResult<()> {
    if !states::ALL.contains(&state) {
        return Err("不認得的動作".into());
    }
    if variant.is_empty() || !variant.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err("組名只能用英文字母或數字".into());
    }
    Ok(())
}

fn read_actions(id: &str) -> CmdResult<PetActions> {
    let p = editable_pet(id)?;
    let mut groups = Vec::new();
    let mut fps = BTreeMap::new();
    let mut default_fps = BTreeMap::new();
    for state in states::ALL {
        for variant in pack_edit::variants(&p.dir, state).map_err(err)? {
            let frames = pack_edit::list(&p.dir, state, &variant)
                .map_err(err)?
                .iter()
                .filter_map(|f| std::fs::read(f).ok())
                .map(|b| data_url(&b))
                .collect();
            groups.push(ActionGroup {
                state: state.to_owned(),
                variant,
                frames,
            });
        }
        let own = p
            .states
            .get(state)
            .and_then(|s| s.variants.first())
            .and_then(|v| v.durations_ms.first())
            .map(|ms| MS_PER_SEC / *ms as f32);
        fps.insert(state.to_owned(), own.unwrap_or(states::defaults(state).fps));
        default_fps.insert(state.to_owned(), states::defaults(state).fps);
    }
    Ok(PetActions {
        groups,
        fps,
        default_fps,
    })
}

/// 這隻桌寵每個動作的圖與速度。
#[tauri::command]
fn pet_actions(id: String) -> CmdResult<PetActions> {
    read_actions(&id)
}

/// 接在某個動作最後面加圖；四格圖會切開、依閱讀順序一張張接上去。
#[tauri::command]
fn add_frames(
    id: String,
    state: String,
    variant: String,
    images: Vec<String>,
) -> CmdResult<PetActions> {
    check_action(&state, &variant)?;
    let p = editable_pet(&id)?;
    let mut pngs = Vec::new();
    for data in &images {
        pngs.extend(importer::prepare_all(&decode_upload(data)?).map_err(err)?);
    }
    pack_edit::append(&p.dir, &state, &variant, &pngs).map_err(err)?;
    read_actions(&id)
}

#[tauri::command]
fn remove_frame(id: String, state: String, variant: String, index: usize) -> CmdResult<PetActions> {
    check_action(&state, &variant)?;
    let p = editable_pet(&id)?;
    if state == states::IDLE
        && variant == pack::DEFAULT_VARIANT
        && pack_edit::list(&p.dir, &state, &variant)
            .map_err(err)?
            .len()
            <= 1
    {
        return Err("站著的圖至少要留一張".into());
    }
    pack_edit::remove(&p.dir, &state, &variant, index).map_err(err)?;
    read_actions(&id)
}

/// 拖曳換順序：第 `from` 張搬到第 `to` 的位置。
#[tauri::command]
fn move_frame(
    id: String,
    state: String,
    variant: String,
    from: usize,
    to: usize,
) -> CmdResult<PetActions> {
    check_action(&state, &variant)?;
    pack_edit::move_to(&editable_pet(&id)?.dir, &state, &variant, from, to).map_err(err)?;
    read_actions(&id)
}

/// 調某個動作的速度（每秒幾張）；`None` = 回到預設。
#[tauri::command]
fn set_action_speed(id: String, state: String, fps: Option<f32>) -> CmdResult<PetActions> {
    check_action(&state, pack::DEFAULT_VARIANT)?;
    pack_edit::set_fps(&editable_pet(&id)?.dir, &state, fps).map_err(err)?;
    read_actions(&id)
}

/// 內建水豚這個動作的圖，當作「應該長這樣」的範例。
fn example_pngs(state: &str) -> Vec<Vec<u8>> {
    let Some(dir) = paths::pets_dir().map(|d| d.join(EXAMPLE_PET)) else {
        return Vec::new();
    };
    pack_edit::list(&dir, state, pack::DEFAULT_VARIANT)
        .unwrap_or_default()
        .iter()
        .filter_map(|f| std::fs::read(f).ok())
        .collect()
}

#[tauri::command]
fn example_frames(state: String) -> Vec<String> {
    example_pngs(&state).iter().map(|b| data_url(b)).collect()
}

/// 把範例圖排成一列存到「下載」資料夾並打開它所在的位置，方便直接丟給 AI。
#[tauri::command]
fn save_example(state: String, label: String) -> CmdResult<()> {
    let frames: Vec<image::RgbaImage> = example_pngs(&state)
        .iter()
        .filter_map(|b| image::load_from_memory(b).ok())
        .map(|i| i.to_rgba8())
        .collect();
    if frames.is_empty() {
        return Err("這個動作還沒有範例圖".into());
    }
    let height = frames.iter().map(|f| f.height()).max().unwrap_or(0);
    let width =
        frames.iter().map(|f| f.width()).sum::<u32>() + EXAMPLE_GAP * (frames.len() as u32 - 1);
    let mut strip = image::RgbaImage::new(width, height);
    let mut x = 0;
    for f in &frames {
        image::imageops::replace(&mut strip, f, x as i64, (height - f.height()) as i64);
        x += f.width() + EXAMPLE_GAP;
    }
    let downloads = std::env::var_os("USERPROFILE")
        .map(|p| std::path::PathBuf::from(p).join("Downloads"))
        .ok_or("找不到下載資料夾")?;
    let path = downloads.join(format!("桌寵範例-{label}.png"));
    strip.save(&path).map_err(err)?;
    Command::new("explorer")
        .arg(format!("/select,{}", path.display()))
        .spawn()
        .map_err(err)?;
    Ok(())
}

/// 刪掉自己做的桌寵（設定畫面已經用「再按一次確認」擋過）。
#[tauri::command]
fn delete_pet(id: String) -> CmdResult<()> {
    let p = editable_pet(&id)?;
    std::fs::remove_dir_all(&p.dir).map_err(err)
}

#[tauri::command]
fn window_layout() -> Vec<WindowInfo> {
    let v: serde_json::Value = load_json(&paths::window_layout_file()).unwrap_or_default();
    let mut out: Vec<WindowInfo> = v["windows"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(id, e)| Some((id.parse().ok()?, e)))
        .map(|(id, e)| WindowInfo {
            id,
            title: e["title"].as_str().unwrap_or_default().to_owned(),
            exe: e["exe"]
                .as_str()
                .unwrap_or_default()
                .rsplit('\\')
                .next()
                .unwrap_or_default()
                .to_owned(),
            left: e["left"].as_i64().unwrap_or(0) as i32,
            top: e["top"].as_i64().unwrap_or(0) as i32,
            width: e["width"].as_i64().unwrap_or(0) as i32,
            height: e["height"].as_i64().unwrap_or(0) as i32,
        })
        .collect();
    out.sort_by(|a, b| a.title.cmp(&b.title));
    out
}

fn core_window() -> Option<windows::Win32::Foundation::HWND> {
    unsafe { FindWindowW(&HSTRING::from(ipc::PET_WINDOW_CLASS), PCWSTR::null()).ok() }
}

#[tauri::command]
fn core_running() -> bool {
    core_window().is_some()
}

/// 送指令給核心；等它做完才回來（在背景等，畫面不會卡），前端 `await` 之後讀到的就是新的結果。
#[tauri::command]
async fn send_core(command: serde_json::Value) -> CmdResult<()> {
    let text = command.to_string();
    tauri::async_runtime::spawn_blocking(move || send_core_now(&text))
        .await
        .map_err(|e| e.to_string())?
}

fn send_core_now(text: &str) -> CmdResult<()> {
    // 核心可能要把自己的畫面拉到最前面（畫範圍要收得到 Esc）：前景權在按按鈕的設定程式手上，先讓出來。
    unsafe {
        let _ = AllowSetForegroundWindow(ASFW_ANY);
    }
    ipc::send(text).map_err(|_| "柚伴沒有在執行".to_string())
}

/// 正在專注／休息時，「停止專注」按鈕的文字（跟右鍵選單同一句）；沒在專注是 `None`。
/// 柚伴沒在跑就不算（當掉時留下的檔還寫著沒結束的時間）。
#[tauri::command]
fn focus_stop_label() -> Option<String> {
    let now = chrono::Local::now().naive_local();
    let status = FocusStatus::load(&paths::focus_status_file(), now).filter(|_| core_running())?;
    Some(status.stop_label(now))
}

#[tauri::command]
fn update_status() -> Option<UpdateStatus> {
    UpdateStatus::load(&paths::update_status_file())
}

/// 換新版時設定程式也會被換掉：送出指令後自己先關，不要開著舊版的畫面。
#[tauri::command]
fn apply_update(app: tauri::AppHandle) -> CmdResult<()> {
    send_core_now(&serde_json::json!({ "cmd": ipc::CMD_APPLY_UPDATE }).to_string())?;
    app.exit(0);
    Ok(())
}

#[tauri::command]
fn start_core() -> CmdResult<()> {
    let exe = paths::sibling_exe(paths::CORE_EXE).ok_or(format!("找不到 {}", paths::CORE_EXE))?;
    Command::new(exe).spawn().map(|_| ()).map_err(err)
}

#[tauri::command]
fn open_pets_dir() -> CmdResult<()> {
    let dir = paths::pets_dir().ok_or("找不到 pets 資料夾")?;
    Command::new("explorer")
        .arg(dir)
        .spawn()
        .map(|_| ())
        .map_err(err)
}

fn core_cli(arg: &str) -> CmdResult<String> {
    let exe = paths::sibling_exe(paths::CORE_EXE).ok_or(format!("找不到 {}", paths::CORE_EXE))?;
    let out = Command::new(exe)
        .args([AUTOSTART_FLAG, arg])
        .output()
        .map_err(err)?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if out.status.success() {
        Ok(text)
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
    }
}

#[tauri::command]
fn autostart_status() -> CmdResult<bool> {
    core_cli("status").map(|s| s == "on")
}

#[tauri::command]
fn set_autostart(on: bool) -> CmdResult<()> {
    core_cli(if on { "on" } else { "off" }).map(|_| ())
}

/// 已經開著就把那個視窗叫到前面，不開第二個。
fn focus_existing() -> bool {
    unsafe {
        let _guard = CreateMutexW(None, true, w!("Local\\YoubanDesktopPet.Settings"));
        if GetLastError() != ERROR_ALREADY_EXISTS {
            std::mem::forget(_guard);
            return false;
        }
        if let Ok(hwnd) = FindWindowW(PCWSTR::null(), &HSTRING::from(WINDOW_TITLE)) {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(hwnd);
        }
        true
    }
}

fn main() {
    if focus_existing() {
        return;
    }
    for problem in paths::migrate_legacy_dirs() {
        eprintln!("legacy data folder not moved: {problem}");
    }
    let args: Vec<String> = std::env::args().collect();
    let page = args
        .iter()
        .position(|a| a == PAGE_FLAG)
        .and_then(|i| args.get(i + 1))
        .cloned();
    tauri::Builder::default()
        .manage(Init { page })
        .invoke_handler(tauri::generate_handler![
            initial_page,
            tokens,
            constants,
            load_config,
            save_config,
            load_reminders,
            save_reminders,
            load_relay_log,
            preview,
            official_days,
            anniversary_info,
            list_pets,
            window_layout,
            core_running,
            send_core,
            focus_stop_label,
            update_status,
            apply_update,
            start_core,
            open_pets_dir,
            import_preview,
            create_pet,
            pet_actions,
            add_frames,
            remove_frame,
            move_frame,
            set_action_speed,
            example_frames,
            save_example,
            delete_pet,
            autostart_status,
            set_autostart,
        ])
        .register_uri_scheme_protocol(PET_SCHEME, |_, request| serve_pet_file(&request))
        .setup(|app| {
            use tauri::Manager;
            if let Some(window) = app.get_webview_window("main") {
                apply_color_scheme(
                    &window,
                    Config::load(&paths::config_file())
                        .unwrap_or_default()
                        .color_scheme,
                );
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running settings");
}

/// 標題列跟著設定的深淺色（畫面裡面由 `ui/app.js` 看 `colorScheme` 切），再塗底色。
fn apply_color_scheme(window: &tauri::WebviewWindow, scheme: ColorScheme) {
    let _ = window.set_theme(match scheme {
        ColorScheme::System => None,
        ColorScheme::Light => Some(tauri::Theme::Light),
        ColorScheme::Dark => Some(tauri::Theme::Dark),
    });
    paint_background(window);
}

/// 視窗底色先塗成畫面的底色：瀏覽器核心還沒畫出東西前，不會先閃一片白（深色模式下特別刺眼）。
fn paint_background(window: &tauri::WebviewWindow) {
    let dark = window.theme().is_ok_and(|t| t == tauri::Theme::Dark);
    let tokens = tokens();
    let hex = tokens["color"][if dark { "dark" } else { "light" }]["surface"]
        .as_str()
        .unwrap_or_default()
        .trim_start_matches('#')
        .to_owned();
    let Ok(rgb) = u32::from_str_radix(&hex, 16) else {
        return;
    };
    let _ = window.set_background_color(Some(tauri::window::Color(
        (rgb >> 16) as u8,
        (rgb >> 8) as u8,
        rgb as u8,
        255,
    )));
}

#[cfg(test)]
mod tests {
    use super::*;

    thread_local! {
        pub static PETS_ROOT: std::cell::RefCell<Option<std::path::PathBuf>> = const { std::cell::RefCell::new(None) };
    }

    #[test]
    fn pet_files_are_served_only_from_the_pets_folder() {
        let base = std::env::temp_dir().join(format!("petsettings-serve-{}", std::process::id()));
        let root = base.join("pets");
        std::fs::create_dir_all(root.join("my cat")).unwrap();
        std::fs::write(root.join("my cat/idle_01.png"), b"png").unwrap();
        std::fs::write(base.join("secret.txt"), b"secret").unwrap();
        PETS_ROOT.with(|r| *r.borrow_mut() = Some(root.clone()));
        let get = |url: &str| {
            serve_pet_file(
                &tauri::http::Request::builder()
                    .uri(url)
                    .body(Vec::new())
                    .unwrap(),
            )
        };

        let url = pet_file_url(&root, &root.join("my cat/idle_01.png")).unwrap();
        let ok = get(&url);
        assert_eq!(
            (ok.status().as_u16(), ok.body().as_slice()),
            (200, &b"png"[..]),
            "{url}"
        );
        assert_eq!(
            get(&format!("{PET_FILE_ORIGIN}/..%2Fsecret.txt"))
                .status()
                .as_u16(),
            404
        );
        assert_eq!(
            get(&format!("{PET_FILE_ORIGIN}/%2E%2E/secret.txt"))
                .status()
                .as_u16(),
            404
        );

        PETS_ROOT.with(|r| *r.borrow_mut() = None);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// 白底上畫幾個描邊方塊（大小不同才分得出順序），轉成畫面送進來的 data URL。
    fn sheet(count: u32) -> String {
        let mut img =
            image::RgbaImage::from_pixel(220 * count, 200, image::Rgba([255, 255, 255, 255]));
        for i in 0..count {
            for y in 40..(120 + i * 10) {
                for x in (20 + i * 220)..(140 + i * 220) {
                    img.put_pixel(x, y, image::Rgba([70, 45, 30, 255]));
                }
            }
        }
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        data_url(&png)
    }

    #[test]
    fn make_a_pet_then_edit_its_actions_through_the_commands() {
        let root = std::env::temp_dir().join(format!("petsettings-pets-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        PETS_ROOT.with(|r| *r.borrow_mut() = Some(root.clone()));
        let id = create_pet("測試".into(), sheet(1)).unwrap();
        let result = (|| -> CmdResult<()> {
            let a = add_frames(
                id.clone(),
                states::WALK.into(),
                pack::DEFAULT_VARIANT.into(),
                vec![sheet(4)],
            )?;
            let walk = |a: &PetActions| {
                a.groups
                    .iter()
                    .find(|g| g.state == states::WALK)
                    .map(|g| g.frames.clone())
                    .unwrap_or_default()
            };
            assert_eq!(
                walk(&a).len(),
                4,
                "a four-pose sheet becomes four walk frames"
            );
            let before = walk(&a);
            let a = move_frame(
                id.clone(),
                states::WALK.into(),
                pack::DEFAULT_VARIANT.into(),
                3,
                0,
            )?;
            assert_eq!(walk(&a)[0], before[3], "last frame moved to the front");
            let a = remove_frame(
                id.clone(),
                states::WALK.into(),
                pack::DEFAULT_VARIANT.into(),
                0,
            )?;
            assert_eq!(walk(&a), before[..3]);
            let a = set_action_speed(id.clone(), states::WALK.into(), Some(12.0))?;
            assert!(
                (a.fps[states::WALK] - 12.0).abs() < 0.5,
                "custom speed read back"
            );
            assert!(
                remove_frame(
                    id.clone(),
                    states::IDLE.into(),
                    pack::DEFAULT_VARIANT.into(),
                    0
                )
                .is_err(),
                "the last standing frame cannot be removed"
            );
            assert!(
                add_frames(
                    id.clone(),
                    "../evil".into(),
                    pack::DEFAULT_VARIANT.into(),
                    vec![sheet(1)]
                )
                .is_err(),
                "unknown action rejected"
            );
            assert!(
                add_frames(
                    id.clone(),
                    states::IDLE.into(),
                    "../x".into(),
                    vec![sheet(1)]
                )
                .is_err(),
                "variant cannot escape the folder"
            );
            Ok(())
        })();
        delete_pet(id).unwrap();
        let _ = std::fs::remove_dir_all(&root);
        result.unwrap();
    }
}
