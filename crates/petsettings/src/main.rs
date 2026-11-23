//! 桌寵設定程式：用到才開、關掉就整個結束（瀏覽器核心只在這時候佔記憶體）。
//! 跟核心溝通只有兩條路：寫設定檔（核心每秒看一次有沒有變），以及 WM_COPYDATA 送指令。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod importer;

use std::collections::BTreeMap;
use std::process::Command;

use base64::Engine;
use chrono::NaiveDate;
use petkit::companion::CompanionFile;
use petkit::config::{Config, load_json};
use petkit::holidays::{self, DayKind};
use petkit::paths;
use petkit::reminders::{PreviewItem, Reminder, ReminderFile};
use petkit::{ipc, pack, pack_edit, states};
use serde::Serialize;
use tauri::State;
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, LPARAM, WPARAM};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow, FindWindowW, SW_RESTORE, SendMessageW, SetForegroundWindow, ShowWindow, WM_COPYDATA};
use windows::core::{HSTRING, PCWSTR, w};

/// 也用來找已經開著的設定視窗（只開一個）；跟 `tauri.conf.json` 的視窗 title、`ui/index.html` 的 `<title>` 一致。
const WINDOW_TITLE: &str = "柚伴設定";
const PAGE_FLAG: &str = "--page";
const PREVIEW_COUNT: usize = 5;
const AUTOSTART_FLAG: &str = "--autostart";

struct Init {
    page: Option<String>,
}

#[derive(Serialize)]
struct PetInfo {
    id: String,
    name: String,
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
    title: String,
    exe: String,
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
    serde_json::json!({ "sizes": petkit::config::SIZE_CHOICES_DIP, "version": env!("CARGO_PKG_VERSION") })
}

#[tauri::command]
fn load_config() -> CmdResult<Config> {
    Config::load(&paths::config_file()).map_err(err)
}

#[tauri::command]
fn save_config(config: Config) -> CmdResult<()> {
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

#[tauri::command]
fn preview(reminder: Reminder, overrides: BTreeMap<NaiveDate, DayKind>) -> Vec<PreviewItem> {
    let (cal, _) = holidays::load_calendar(&paths::holiday_cache_dir(), overrides);
    reminder.preview(chrono::Local::now().naive_local(), &cal, PREVIEW_COUNT)
}

#[tauri::command]
fn list_pets() -> Vec<PetInfo> {
    let Some(dir) = paths::pets_dir() else { return Vec::new() };
    let companions = CompanionFile::load(&paths::companion_file()).unwrap_or_default();
    let mut out: Vec<PetInfo> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let id = e.file_name().to_str()?.to_owned();
            let p = pack::load(&e.path()).ok()?;
            let image = data_url(&std::fs::read(&p.states[states::IDLE].variants[0].frames[0]).ok()?);
            let days = companions.days(&id);
            Some(PetInfo { id, name: p.name, image, editable: p.editable, days })
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

fn data_url(png: &[u8]) -> String {
    format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png))
}

fn decode_upload(data: &str) -> CmdResult<Vec<u8>> {
    let raw = data.split_once(',').map_or(data, |(_, b64)| b64);
    base64::engine::general_purpose::STANDARD.decode(raw).map_err(err)
}

/// 使用者剛選的圖整理好（去背、裁切；四格圖會切開）之後的樣子，給畫面預覽。
#[tauri::command]
fn import_preview(data: String) -> CmdResult<Vec<String>> {
    Ok(importer::prepare_all(&decode_upload(&data)?).map_err(err)?.iter().map(|p| data_url(p)).collect())
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
    petkit::config::write_atomic(&dir.join(pack::MANIFEST_FILE), serde_json::to_string_pretty(&manifest).map_err(err)?.as_bytes()).map_err(err)?;
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
            let frames = pack_edit::list(&p.dir, state, &variant).map_err(err)?.iter().filter_map(|f| std::fs::read(f).ok()).map(|b| data_url(&b)).collect();
            groups.push(ActionGroup { state: state.to_owned(), variant, frames });
        }
        let own = p.states.get(state).and_then(|s| s.variants.first()).and_then(|v| v.durations_ms.first()).map(|ms| MS_PER_SEC / *ms as f32);
        fps.insert(state.to_owned(), own.unwrap_or(states::defaults(state).fps));
        default_fps.insert(state.to_owned(), states::defaults(state).fps);
    }
    Ok(PetActions { groups, fps, default_fps })
}

/// 這隻桌寵每個動作的圖與速度。
#[tauri::command]
fn pet_actions(id: String) -> CmdResult<PetActions> {
    read_actions(&id)
}

/// 接在某個動作最後面加圖；四格圖會切開、依閱讀順序一張張接上去。
#[tauri::command]
fn add_frames(id: String, state: String, variant: String, images: Vec<String>) -> CmdResult<PetActions> {
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
    if state == states::IDLE && variant == pack::DEFAULT_VARIANT && pack_edit::list(&p.dir, &state, &variant).map_err(err)?.len() <= 1 {
        return Err("站著的圖至少要留一張".into());
    }
    pack_edit::remove(&p.dir, &state, &variant, index).map_err(err)?;
    read_actions(&id)
}

/// 拖曳換順序：第 `from` 張搬到第 `to` 的位置。
#[tauri::command]
fn move_frame(id: String, state: String, variant: String, from: usize, to: usize) -> CmdResult<PetActions> {
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
    let Some(dir) = paths::pets_dir().map(|d| d.join(EXAMPLE_PET)) else { return Vec::new() };
    pack_edit::list(&dir, state, pack::DEFAULT_VARIANT).unwrap_or_default().iter().filter_map(|f| std::fs::read(f).ok()).collect()
}

#[tauri::command]
fn example_frames(state: String) -> Vec<String> {
    example_pngs(&state).iter().map(|b| data_url(b)).collect()
}

/// 把範例圖排成一列存到「下載」資料夾並打開它所在的位置，方便直接丟給 AI。
#[tauri::command]
fn save_example(state: String, label: String) -> CmdResult<()> {
    let frames: Vec<image::RgbaImage> = example_pngs(&state).iter().filter_map(|b| image::load_from_memory(b).ok()).map(|i| i.to_rgba8()).collect();
    if frames.is_empty() {
        return Err("這個動作還沒有範例圖".into());
    }
    let height = frames.iter().map(|f| f.height()).max().unwrap_or(0);
    let width = frames.iter().map(|f| f.width()).sum::<u32>() + EXAMPLE_GAP * (frames.len() as u32 - 1);
    let mut strip = image::RgbaImage::new(width, height);
    let mut x = 0;
    for f in &frames {
        image::imageops::replace(&mut strip, f, x as i64, (height - f.height()) as i64);
        x += f.width() + EXAMPLE_GAP;
    }
    let downloads = std::env::var_os("USERPROFILE").map(|p| std::path::PathBuf::from(p).join("Downloads")).ok_or("找不到下載資料夾")?;
    let path = downloads.join(format!("桌寵範例-{label}.png"));
    strip.save(&path).map_err(err)?;
    Command::new("explorer").arg(format!("/select,{}", path.display())).spawn().map_err(err)?;
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
        .map(|(_, e)| WindowInfo {
            title: e["title"].as_str().unwrap_or_default().to_owned(),
            exe: e["exe"].as_str().unwrap_or_default().rsplit('\\').next().unwrap_or_default().to_owned(),
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

#[tauri::command]
fn send_core(command: serde_json::Value) -> CmdResult<()> {
    let hwnd = core_window().ok_or("柚伴沒有在執行")?;
    let text = command.to_string();
    let data = COPYDATASTRUCT { dwData: 0, cbData: text.len() as u32, lpData: text.as_ptr() as *mut _ };
    unsafe {
        // 核心可能要把自己的畫面拉到最前面（畫範圍要收得到 Esc）：前景權在按按鈕的設定程式手上，先讓出來。
        let _ = AllowSetForegroundWindow(ASFW_ANY);
        SendMessageW(hwnd, WM_COPYDATA, Some(WPARAM(0)), Some(LPARAM(&data as *const _ as isize)));
    }
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
    Command::new("explorer").arg(dir).spawn().map(|_| ()).map_err(err)
}

fn core_cli(arg: &str) -> CmdResult<String> {
    let exe = paths::sibling_exe(paths::CORE_EXE).ok_or(format!("找不到 {}", paths::CORE_EXE))?;
    let out = Command::new(exe).args([AUTOSTART_FLAG, arg]).output().map_err(err)?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if out.status.success() { Ok(text) } else { Err(String::from_utf8_lossy(&out.stderr).trim().to_owned()) }
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
    let page = args.iter().position(|a| a == PAGE_FLAG).and_then(|i| args.get(i + 1)).cloned();
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
            preview,
            list_pets,
            window_layout,
            core_running,
            send_core,
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
        .run(tauri::generate_context!())
        .expect("error while running settings");
}

#[cfg(test)]
mod tests {
    use super::*;

    thread_local! {
        pub static PETS_ROOT: std::cell::RefCell<Option<std::path::PathBuf>> = const { std::cell::RefCell::new(None) };
    }

    /// 白底上畫幾個描邊方塊（大小不同才分得出順序），轉成畫面送進來的 data URL。
    fn sheet(count: u32) -> String {
        let mut img = image::RgbaImage::from_pixel(220 * count, 200, image::Rgba([255, 255, 255, 255]));
        for i in 0..count {
            for y in 40..(120 + i * 10) {
                for x in (20 + i * 220)..(140 + i * 220) {
                    img.put_pixel(x, y, image::Rgba([70, 45, 30, 255]));
                }
            }
        }
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        data_url(&png)
    }

    #[test]
    fn make_a_pet_then_edit_its_actions_through_the_commands() {
        let root = std::env::temp_dir().join(format!("petsettings-pets-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        PETS_ROOT.with(|r| *r.borrow_mut() = Some(root.clone()));
        let id = create_pet("測試".into(), sheet(1)).unwrap();
        let result = (|| -> CmdResult<()> {
            let a = add_frames(id.clone(), states::WALK.into(), pack::DEFAULT_VARIANT.into(), vec![sheet(4)])?;
            let walk = |a: &PetActions| a.groups.iter().find(|g| g.state == states::WALK).map(|g| g.frames.clone()).unwrap_or_default();
            assert_eq!(walk(&a).len(), 4, "a four-pose sheet becomes four walk frames");
            let before = walk(&a);
            let a = move_frame(id.clone(), states::WALK.into(), pack::DEFAULT_VARIANT.into(), 3, 0)?;
            assert_eq!(walk(&a)[0], before[3], "last frame moved to the front");
            let a = remove_frame(id.clone(), states::WALK.into(), pack::DEFAULT_VARIANT.into(), 0)?;
            assert_eq!(walk(&a), before[..3]);
            let a = set_action_speed(id.clone(), states::WALK.into(), Some(12.0))?;
            assert!((a.fps[states::WALK] - 12.0).abs() < 0.5, "custom speed read back");
            assert!(remove_frame(id.clone(), states::IDLE.into(), pack::DEFAULT_VARIANT.into(), 0).is_err(), "the last standing frame cannot be removed");
            assert!(add_frames(id.clone(), "../evil".into(), pack::DEFAULT_VARIANT.into(), vec![sheet(1)]).is_err(), "unknown action rejected");
            assert!(add_frames(id.clone(), states::IDLE.into(), "../x".into(), vec![sheet(1)]).is_err(), "variant cannot escape the folder");
            Ok(())
        })();
        delete_pet(id).unwrap();
        let _ = std::fs::remove_dir_all(&root);
        result.unwrap();
    }
}
