//! 主程式：桌寵視窗、提醒卡視窗、訊息迴圈，把行為層、功能模組與繪圖層接起來。

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Instant, SystemTime};

use petkit::companion::{self, CompanionFile};
use petkit::config::{Area, Config, Roam, Talk, load_json, write_atomic};
use petkit::ipc;
use petkit::pack::{self, PetPack};
use petkit::states;
use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, HMONITOR, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::System::Threading::{CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, CreateWaitableTimerExW, INFINITE, SetWaitableTimer, TIMER_ALL_ACCESS};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, GetSystemMetricsForDpi, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetCapture, GetLastInputInfo, LASTINPUTINFO, ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, VK_ESCAPE};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{Error, HSTRING, PCWSTR, Result, w};
use windows_numerics::Vector2;

use crate::behavior::{self, Emote, Env, Happening, Input, ItemView, Pet, Platform, Rect};
use crate::toy::{self, Toy, ToyKind, ToyPainter};
use crate::glow::{GlowPainter, GlowStyle};
use crate::area_editor::{AreaEditor, AreaInput, AreaKind, Outcome};
use crate::chatter::{self, Chatter, Say};
use crate::platforms;
use crate::bubble::{Bubble, Card, CardAction, Pointer};
use crate::effects::Effects;
use crate::holiday_sync::{self, WM_HOLIDAYS_UPDATED};
use crate::updater::{self, WM_UPDATE_READY};
use crate::log;
use crate::memwatch::{self, MemWatch};
use crate::modules::{ModuleEvent, ModuleHost, Request};
use crate::paths;
use crate::reminder_module::{self, ReminderModule};
use crate::render::{Gfx, PetRenderer, Scene};
use crate::soak::{Action, Soak};
use crate::sprites::{SpriteSet, Wic};
use crate::theme::{self, Palette};
use crate::tray::{self, MenuItem, Tray};
use crate::window_restore::{self, WindowRestore};

/// 設定程式靠這個類別名稱找到核心（`petkit::ipc::PET_WINDOW_CLASS`）。
static PET_CLASS_NAME: LazyLock<HSTRING> = LazyLock::new(|| HSTRING::from(ipc::PET_WINDOW_CLASS));
const CARD_CLASS: PCWSTR = w!("YoubanDesktopPet.Card");
const TOY_CLASS: PCWSTR = w!("YoubanDesktopPet.Toy");
const GLOW_CLASS: PCWSTR = w!("YoubanDesktopPet.Glow");
const AREA_CLASS: PCWSTR = w!("YoubanDesktopPet.Area");
/// 畫範圍時，框要至少這麼大才算：禁止範圍（DIP）、活動範圍（站姿寬的幾倍 × 身高的幾倍，要能在裡面走動）。
const MIN_FORBIDDEN_DIP: f32 = 16.0;
const MIN_ALLOWED_WIDTHS: f32 = 2.0;
const MIN_ALLOWED_HEIGHTS: f32 = 1.5;
/// 畫好範圍後桌寵說的話。
const SAY_ALLOWED_SET: &str = "這裡就是我的地盤了～";
const SAY_FORBIDDEN_SET: &str = "知道了，那裡我不去～";
const SAY_AREA_SECS: f32 = 3.0;

pub fn pet_class() -> PCWSTR {
    PCWSTR(PET_CLASS_NAME.as_ptr())
}
const BASE_DPI: u32 = 96;
/// 腳底離視窗底邊留幾個像素，避免最底下一排像素被裁掉。
const FOOT_PAD: f32 = 2.0;
/// 視窗要比桌寵大一圈：被拎起來會左右擺、開心會跳起來、睡覺有 Zzz。
const WINDOW_WIDTH_PER_HEIGHT: f32 = 1.35;
const WINDOW_HEIGHT_PER_HEIGHT: f32 = 1.9;
const CLIMB_MIN_WIDTH_PER_HEIGHT: f32 = 2.2;
/// 有特效或過場在播時一律用這個更新率。
const FPS_ANIMATING: u32 = 60;
/// 全螢幕程式在前面、桌寵躲起來時，只需要偶爾醒來看一下。
const FPS_HIDDEN: u32 = 2;
/// 多久重新找一次可以站的視窗頂端、多久看一次有沒有全螢幕程式。
const PLATFORM_SCAN_SECS: f64 = 0.25;
const BUSY_CHECK_SECS: f64 = 0.5;
/// 視窗頂端露出來的部分要至少這麼寬（站姿寬度的倍數）才站得上去。
const PLATFORM_MIN_WIDTH_RATIO: f32 = 0.8;
/// 一格最多推進多久；電腦睡眠醒來時不要一口氣算幾小時的物理。
const MAX_DT: f32 = 0.1;
/// 預設位置：主螢幕右下角往左幾個桌寵身高。
const DEFAULT_OFFSET_HEIGHTS: f32 = 1.5;
const HUNDRED_NS_PER_SEC: f32 = 10_000_000.0;
/// 功能模組、設定檔變動檢查的頻率。
const MODULE_TICK_SECS: f64 = 1.0;
/// 一般通知卡顯示多久自動收掉。
const INFO_SECS: f64 = 5.0;
/// 排隊中的卡片上限（超過先丟最舊的一般通知）。
const MAX_CARDS: usize = 32;
/// 多久看一次要不要下載新的辦公日曆（真的下載還要看上次是何時）。
const HOLIDAY_CHECK_SECS: f64 = 3600.0;
const UPDATE_CHECK_SECS: f64 = 3600.0;
const UPDATE_CARD_KEY: &str = "update";
/// 換桌寵：舊的縮小消失、新的彈出來。
const SWITCH_OUT_SECS: f64 = 0.18;
const SWITCH_IN_SECS: f64 = 0.36;
/// 講話泡泡最後這段時間淡出。
const SPEECH_FADE_SECS: f64 = 0.25;
/// 點心／球從桌寵旁邊幾個身高的地方、頭上幾個身高的高度掉下來。
const TOY_DROP_SIDE_HEIGHTS: f32 = 1.8;
const TOY_DROP_HEIGHTS: f32 = 2.5;
/// 丟球的初速（DIP/秒）：往旁邊、往上。
const BALL_THROW_X_DIP: f32 = 260.0;
const BALL_THROW_UP_DIP: f32 = 320.0;
/// 球踢這麼多次就玩夠了；玩夠了停穩後再擺這麼久才淡出收掉。
const BALL_MAX_KICKS: u32 = 4;
const TOY_LINGER_SECS: f64 = 4.0;
/// 點心／球這麼久沒人碰（牠搆不到、或沒興趣）就收掉，不會永遠躺在桌面上。
const TOY_UNTOUCHED_MAX_SECS: f64 = 60.0;
const TOY_FADE_SECS: f64 = 0.6;
/// 站著的視窗剛被拖動過：這段時間內用高更新率，跟得才順。
const RIDE_SMOOTH_SECS: f64 = 0.5;
/// 「今天有見到」：使用者這麼久以內碰過滑鼠鍵盤、桌寵在桌面上。
const PRESENT_IDLE_SECS: f32 = 60.0;
/// 第一天、紀念日的話：開機後至少等這麼久（先讓牠打完招呼），泡泡顯示久一點。
const MILESTONE_AFTER_SECS: f64 = 10.0;
const MILESTONE_SAY_SECS: f32 = 6.0;
/// 功能模組請桌寵講的話（例如記住了新視窗）顯示幾秒。
const MODULE_SAY_SECS: f32 = 4.0;

/// `windows` crate 把它放在 Controls 功能底下（為了一個常數不值得開那整包）。
const WM_MOUSELEAVE: u32 = 0x02A3;

const MENU_EXIT: u32 = 1;
const MENU_ADD_REMINDER: u32 = 10;
const MENU_SETTINGS: u32 = 11;
const MENU_DAYS: u32 = 12;
const MENU_FEED: u32 = 13;
const MENU_BALL: u32 = 14;
const MENU_PET_BASE: u32 = 100;
const SETTINGS_PAGE_REMINDERS: &str = "reminders";
const SETTINGS_PAGE_PETS: &str = "pets";

#[derive(Debug, Clone, PartialEq)]
enum Event {
    PointerDown,
    PointerMove,
    PointerUp,
    /// 滑鼠在桌寵身上移動、沒按著（摸摸頭）。
    Hover,
    Menu,
    TaskbarCreated,
    DisplayChanged,
    SettingChanged,
    HolidaysUpdated,
    /// 新版下載解好了，等空檔換上。
    UpdateReady,
    Command(String),
    Card(Pointer, f32, f32),
    /// 使用者點了桌面上的點心／球。
    ToyPoke,
    /// 畫範圍的畫面上的滑鼠與鍵盤。
    Area(AreaInput),
}

/// 桌面上的點心或球，與它自己的視窗。
struct ToySlot {
    toy: Toy,
    /// 上次有人碰它（丟出來、被踢、被點）。
    touched: f64,
    /// 開始淡出的時間。
    fading: Option<f64>,
    /// 玩夠了、停穩的時間（過一陣子淡出）。
    done_at: Option<f64>,
}

thread_local! {
    static EVENTS: RefCell<Vec<Event>> = const { RefCell::new(Vec::new()) };
    /// 滑鼠在卡片按鈕上（給 WM_SETCURSOR 換成手指游標）。
    static OVER_BUTTON: Cell<bool> = const { Cell::new(false) };
}

static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

fn push(event: Event) {
    EVENTS.with_borrow_mut(|q| {
        // 連續的滑鼠移動只留最後一筆，佇列不會因為滑鼠狂動而長大。
        let is_move = |e: &Event| matches!(e, Event::PointerMove | Event::Hover | Event::Card(Pointer::Move, ..) | Event::Area(AreaInput::Move(..)));
        if is_move(&event) && q.last().is_some_and(|last| std::mem::discriminant(last) == std::mem::discriminant(&event) && is_move(last)) {
            q.pop();
        }
        q.push(event);
    });
}

/// 核心自己的執行狀態（設定程式不碰）。
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct RuntimeState {
    position: Option<(i32, i32)>,
    /// 上次打招呼是哪一天（重開桌寵同一天不再打一次）。
    greeted_on: Option<chrono::NaiveDate>,
}

/// 正在講的話。
struct Speech {
    text: String,
    until: f64,
}

/// 正在播的動畫：哪個狀態、哪個變化款、第幾張。
struct Anim {
    state: &'static str,
    source: String,
    own: bool,
    variant: usize,
    frame: usize,
    elapsed_ms: f32,
}

struct Queued {
    owner: &'static str,
    key: String,
    card: Card,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    Out,
    In,
}

struct Transition {
    phase: Phase,
    start: f64,
    target: String,
    save: bool,
}

struct App {
    hwnd: HWND,
    config: Config,
    config_stamp: Option<SystemTime>,
    pets_dir: PathBuf,
    pack: PetPack,
    wic: Wic,
    renderer: PetRenderer,
    sprites: SpriteSet,
    window_size: (i32, i32),
    pet: Pet,
    anim: Anim,
    started: Instant,
    last_tick: f64,
    last_module_tick: f64,
    last_holiday_check: f64,
    last_update_check: f64,
    /// 新版解好了，還沒問過使用者。
    update_ready: bool,
    /// 這次執行已經問過了（按「下次啟動時」就不要每小時再問）。
    update_offered: bool,
    memwatch: MemWatch,
    tray: Tray,
    bubble: Bubble,
    cards: VecDeque<Queued>,
    shown_card: bool,
    modules: ModuleHost,
    /// 目前的深淺色，再換上這隻桌寵的強調色。
    palette: Palette,
    transition: Option<Transition>,
    effects: Effects,
    platforms: Vec<Platform>,
    last_platform_scan: f64,
    /// 有全螢幕程式在前面，桌寵躲起來了。
    hidden: bool,
    last_busy_check: f64,
    rng: u64,
    soak: Option<Soak>,
    chatter: Chatter,
    speech: Option<Speech>,
    /// (日期, 那天要不要上班)：一天算一次，日曆不常駐記憶體。
    workday: Option<(chrono::NaiveDate, bool)>,
    companions: CompanionFile,
    /// 今天剛好是第一天或紀念日，等牠有空（沒在講別的、沒有提醒卡）再講。
    pending_milestone: Option<u32>,
    /// 上次看到站著的視窗在移動。
    last_ride_move: f64,
    toy: Option<ToySlot>,
    toy_hwnd: HWND,
    toy_painter: ToyPainter,
    glow_hwnd: HWND,
    glow: GlowPainter,
    gfx: Gfx,
    /// 正在畫範圍（設定程式按了「畫範圍」）。
    area_editor: Option<AreaEditor>,
}

pub fn run(soak_secs: Option<u64>, after_update: bool) -> Result<()> {
    let config = load_config(&paths::config_file());
    if let Some(dir) = paths::install_dir() {
        updater::cleanup(&dir);
        updater::discard_stale(&paths::update_dir());
        // 上次按了「下次啟動時」：一啟動就先換，換好讓新的核心接手。
        if config.auto_update && soak_secs.is_none() && updater::enabled_for(&dir) {
            if let Some(staged) = updater::staged(&paths::update_dir()) {
                match updater::apply_and_restart(&staged, &dir) {
                    Ok(()) => {
                        log!("pending update applied at startup; handing over");
                        return Ok(());
                    }
                    Err(e) => log!("pending update failed: {e}"),
                }
            }
        }
    }
    let pets_dir = paths::pets_dir().ok_or_else(|| Error::new(E_FAIL, "找不到 pets 資料夾"))?;
    let pack = load_pack_or_default(&pets_dir, &config.pet)?;
    log!("loaded pet '{}' ({} states)", pack.id, pack.states.len());

    let (hwnd, card_hwnd, toy_hwnd, glow_hwnd) = create_windows()?;
    let gfx = Gfx::new()?;
    let wic = Wic::new()?;
    let renderer = PetRenderer::new(&gfx)?;

    let runtime: RuntimeState = load_json(&paths::state_file()).unwrap_or_default();
    let primary_monitor = unsafe { MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTOPRIMARY) };
    let primary = work_area(primary_monitor);
    let base_height = config.size_dip as f32 * monitor_dpi(primary_monitor) as f32 / BASE_DPI as f32;
    let (x, y) = match runtime.position {
        Some((x, y)) => (x as f32, y as f32),
        None => (primary.right as f32 - base_height * DEFAULT_OFFSET_HEIGHTS, primary.bottom as f32),
    };
    let dpi = monitor_dpi(monitor_at(x, y));
    let sprites = wic.load_pet(&pack, &renderer.canvas.target(), px_height(&config, dpi), dpi)?;
    let tray = Tray::new(hwnd, pet_icon(&wic, &pack, dpi)?);

    let mut modules = ModuleHost::new(Vec::new());
    modules.sync(&enabled_modules(&config), make_module);

    let palette = theme::with_accent(theme::current(), pack.accent.as_deref());
    let started = Instant::now();
    let seed = u64::from(unsafe { GetTickCount() }) ^ std::process::id() as u64;
    let mut app = App {
        hwnd,
        config_stamp: stamp(&paths::config_file()),
        config,
        pets_dir,
        pack,
        wic,
        renderer,
        sprites,
        window_size: (0, 0),
        pet: Pet::new(x, y, 0.0, seed),
        anim: Anim { state: "", source: String::new(), own: false, variant: 0, frame: 0, elapsed_ms: 0.0 },
        started,
        last_tick: 0.0,
        last_module_tick: f64::NEG_INFINITY,
        last_holiday_check: f64::NEG_INFINITY,
        last_update_check: f64::NEG_INFINITY,
        update_ready: false,
        update_offered: false,
        memwatch: MemWatch::new(started),
        tray,
        bubble: Bubble::new(card_hwnd, &gfx)?,
        cards: VecDeque::new(),
        shown_card: false,
        modules,
        palette,
        transition: None,
        effects: Effects::default(),
        platforms: Vec::new(),
        last_platform_scan: f64::NEG_INFINITY,
        hidden: false,
        last_busy_check: f64::NEG_INFINITY,
        // 先把種子打散：種子只有低 32 位元有值，xorshift 前幾次輸出的高位元幾乎都是 0，
        // `rand01`（取高位元）會一直挑到第一種點心／球。
        rng: seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1,
        soak: soak_secs.map(|s| Soak::new(s, 0.0)),
        chatter: Chatter::new(0.0, seed.rotate_left(17), runtime.greeted_on),
        speech: None,
        workday: None,
        companions: load_companions(),
        pending_milestone: None,
        last_ride_move: f64::NEG_INFINITY,
        toy: None,
        toy_hwnd,
        toy_painter: ToyPainter::new(&gfx)?,
        glow_hwnd,
        glow: GlowPainter::new(&gfx)?,
        gfx: gfx.clone(),
        area_editor: None,
    };
    app.resize_window()?;
    if after_update {
        app.enqueue("core", "info".into(), Card::success(format!("已更新到 {}", env!("CARGO_PKG_VERSION")), None));
    }
    app.tick()?;
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
    let result = app.run_loop();
    app.save_state();
    let mem = memwatch::sample();
    log!("exit; private={}KB working_set={}KB", mem.private_bytes / 1024, mem.working_set / 1024);
    result
}

/// 陪伴天數的檔壞掉時不要直接蓋掉（那是一起過的日子）：改名留著，從頭開始。
fn load_companions() -> CompanionFile {
    let path = paths::companion_file();
    CompanionFile::load(&path).unwrap_or_else(|e| {
        let backup = path.with_extension("broken.json");
        log!("companion file unreadable ({e}); kept as {}", backup.display());
        let _ = std::fs::rename(&path, backup);
        CompanionFile::default()
    })
}

fn stamp(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// 用使用者 Windows 音效配置裡的「提醒」（行事曆提醒用的那個）；使用者在系統裡關掉它就不會響。
/// 非同步播放，不會卡住畫面。
fn play_reminder_sound() {
    use windows::Win32::Media::Audio::{PlaySoundW, SND_ALIAS, SND_ASYNC, SND_NODEFAULT};
    if !unsafe { PlaySoundW(w!("Notification.Reminder"), None, SND_ALIAS | SND_ASYNC | SND_NODEFAULT) }.as_bool() {
        log!("reminder sound not played");
    }
}

/// 設定裡開著的功能 → 要載入的模組（也是右鍵選單裡的順序）。
fn enabled_modules(config: &Config) -> Vec<&'static str> {
    let f = config.features;
    [(f.reminders, reminder_module::ID), (f.window_restore, window_restore::ID)].into_iter().filter(|(on, _)| *on).map(|(_, id)| id).collect()
}

fn make_module(id: &str) -> Box<dyn crate::modules::Module> {
    match id {
        reminder_module::ID => Box::new(ReminderModule::new(reminder_module::Paths {
            reminders: paths::reminders_file(),
            state: paths::reminder_state_file(),
            holiday_cache: paths::holiday_cache_dir(),
        })),
        window_restore::ID => Box::new(WindowRestore::new(paths::window_layout_file())),
        _ => unreachable!("unknown module {id}"),
    }
}

fn area_rect(a: Area) -> Rect {
    Rect { left: a.left as f32, top: a.top as f32, right: a.right as f32, bottom: a.bottom as f32 }
}

fn px_height(config: &Config, dpi: u32) -> f32 {
    config.size_dip as f32 * dpi as f32 / BASE_DPI as f32
}

fn load_config(path: &Path) -> Config {
    match Config::load(path) {
        Ok(c) => c,
        Err(e) => {
            // 壞掉的設定檔先留一份，不要被下一次存檔默默蓋掉。
            let backup = path.with_extension("json.bad");
            let _ = std::fs::rename(path, &backup);
            log!("config unreadable ({e}); moved to {} and using defaults", backup.display());
            Config::default()
        }
    }
}

/// 設定裡的那隻讀不進來，就退回預設那隻（不讓一個壞掉的桌寵包害整個程式開不起來）。
fn load_pack_or_default(pets_dir: &Path, id: &str) -> Result<PetPack> {
    pack::load(&pets_dir.join(id))
        .or_else(|e| {
            log!("pet '{id}' unusable ({e}); falling back to default");
            pack::load(&pets_dir.join(petkit::config::DEFAULT_PET))
        })
        .map_err(|e| Error::new(E_FAIL, e.to_string()))
}

/// `pets/` 裡所有讀得進來的桌寵：(資料夾名, 顯示名稱)。
fn list_pets(pets_dir: &Path) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = std::fs::read_dir(pets_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let folder = e.file_name().to_str()?.to_owned();
            let p = pack::load(&e.path()).ok()?;
            Some((folder, p.name))
        })
        .collect();
    out.sort();
    out
}

fn pet_icon(wic: &Wic, pack: &PetPack, dpi: u32) -> Result<HICON> {
    let size = unsafe { GetSystemMetricsForDpi(SM_CXSMICON, dpi) } as u32;
    wic.make_icon(&pack.states[states::IDLE].variants[0].frames[0], size)
}

fn register_class(name: PCWSTR, proc: WNDPROC, cursor: PCWSTR) -> Result<HINSTANCE> {
    unsafe {
        let instance: HINSTANCE = GetModuleHandleW(None)?.into();
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: proc,
            hInstance: instance,
            hCursor: LoadCursorW(None, cursor)?,
            lpszClassName: name,
            ..Default::default()
        };
        if RegisterClassExW(&class) == 0 {
            return Err(Error::from_thread());
        }
        Ok(instance)
    }
}

fn create_layered(class: PCWSTR, title: PCWSTR, instance: HINSTANCE) -> Result<HWND> {
    unsafe { CreateWindowExW(WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, class, title, WS_POPUP, 0, 0, 1, 1, None, None, Some(instance), None) }
}

fn create_windows() -> Result<(HWND, HWND, HWND, HWND)> {
    let instance = register_class(pet_class(), Some(pet_proc), IDC_HAND)?;
    register_class(CARD_CLASS, Some(card_proc), IDC_ARROW)?;
    register_class(TOY_CLASS, Some(toy_proc), IDC_HAND)?;
    register_class(GLOW_CLASS, Some(glow_proc), IDC_ARROW)?;
    register_class(AREA_CLASS, Some(area_proc), IDC_CROSS)?;
    unsafe { TASKBAR_CREATED.store(RegisterWindowMessageW(w!("TaskbarCreated")), Ordering::Relaxed) };
    let pet = create_layered(pet_class(), w!("柚伴"), instance)?;
    let card = create_layered(CARD_CLASS, w!("柚伴提醒"), instance)?;
    let toy = create_layered(TOY_CLASS, w!("柚伴玩具"), instance)?;
    // 牆的紅光：滑鼠點下去要穿過它，點到後面的東西。
    let glow = create_layered(GLOW_CLASS, w!("柚伴牆"), instance)?;
    unsafe {
        SetWindowLongW(glow, GWL_EXSTYLE, GetWindowLongW(glow, GWL_EXSTYLE) | WS_EX_TRANSPARENT.0 as i32);
    }
    // 設定程式（可能不是系統管理員權限）也要能送指令給以系統管理員權限執行的核心。
    unsafe {
        let _ = ChangeWindowMessageFilterEx(pet, WM_COPYDATA, MSGFLT_ALLOW, None);
    }
    Ok((pet, card, toy, glow))
}

extern "system" fn glow_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

extern "system" fn area_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let (x, y) = client_xy(lparam);
    let (x, y) = (x as i32, y as i32);
    let input = match msg {
        WM_LBUTTONDOWN => Some(AreaInput::Down(x, y)),
        WM_MOUSEMOVE => Some(AreaInput::Move(x, y)),
        WM_LBUTTONUP => Some(AreaInput::Up(x, y)),
        WM_RBUTTONUP => Some(AreaInput::Cancel),
        WM_KEYDOWN if wparam.0 == VK_ESCAPE.0 as usize => Some(AreaInput::Cancel),
        _ => None,
    };
    match input {
        Some(i) => {
            push(Event::Area(i));
            LRESULT(0)
        }
        None => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

extern "system" fn toy_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_LBUTTONUP => {
                push(Event::ToyPoke);
                LRESULT(0)
            }
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

extern "system" fn pet_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_LBUTTONDOWN => {
                SetCapture(hwnd);
                push(Event::PointerDown);
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                push(if GetCapture() == hwnd { Event::PointerMove } else { Event::Hover });
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let _ = ReleaseCapture();
                push(Event::PointerUp);
                LRESULT(0)
            }
            // 拖到一半被別的視窗搶走滑鼠（例如跳出 UAC），當作放開。
            WM_CAPTURECHANGED => {
                push(Event::PointerUp);
                LRESULT(0)
            }
            WM_RBUTTONUP => {
                push(Event::Menu);
                LRESULT(0)
            }
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            tray::WM_TRAY => {
                if matches!(lparam.0 as u32, WM_LBUTTONUP | WM_RBUTTONUP) {
                    push(Event::Menu);
                }
                LRESULT(0)
            }
            WM_DISPLAYCHANGE => {
                push(Event::DisplayChanged);
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_SETTINGCHANGE => {
                push(Event::SettingChanged);
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_HOLIDAYS_UPDATED => {
                push(Event::HolidaysUpdated);
                LRESULT(0)
            }
            WM_UPDATE_READY => {
                push(Event::UpdateReady);
                LRESULT(0)
            }
            WM_COPYDATA => {
                let data = &*(lparam.0 as *const COPYDATASTRUCT);
                if !data.lpData.is_null() {
                    let bytes = std::slice::from_raw_parts(data.lpData as *const u8, data.cbData as usize);
                    push(Event::Command(String::from_utf8_lossy(bytes).into_owned()));
                }
                LRESULT(1)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            m if m != 0 && m == TASKBAR_CREATED.load(Ordering::Relaxed) => {
                push(Event::TaskbarCreated);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

fn client_xy(lparam: LPARAM) -> (f32, f32) {
    let x = (lparam.0 & 0xFFFF) as u16 as i16;
    let y = ((lparam.0 >> 16) & 0xFFFF) as u16 as i16;
    (x as f32, y as f32)
}

extern "system" fn card_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_MOUSEMOVE => {
                let mut track = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                let _ = TrackMouseEvent(&mut track);
                let (x, y) = client_xy(lparam);
                push(Event::Card(Pointer::Move, x, y));
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                let (x, y) = client_xy(lparam);
                push(Event::Card(Pointer::Down, x, y));
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let (x, y) = client_xy(lparam);
                push(Event::Card(Pointer::Up, x, y));
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                push(Event::Card(Pointer::Leave, 0.0, 0.0));
                LRESULT(0)
            }
            WM_SETCURSOR if OVER_BUTTON.get() => {
                if let Ok(c) = LoadCursorW(None, IDC_HAND) {
                    SetCursor(Some(c));
                }
                LRESULT(1)
            }
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

pub fn window_size(s: &SpriteSet) -> (i32, i32) {
    // 爬牆時桌寵橫躺在牆線的一側，視窗半寬至少要一個身高多一點。
    let w = (s.max_width + s.max_height * WINDOW_WIDTH_PER_HEIGHT).max(s.max_height * CLIMB_MIN_WIDTH_PER_HEIGHT).ceil() as i32;
    let h = (s.max_height * WINDOW_HEIGHT_PER_HEIGHT + FOOT_PAD).ceil() as i32;
    (w, h)
}

/// 腳底在視窗裡的位置。
pub fn anchor((w, h): (i32, i32)) -> Vector2 {
    Vector2 { X: w as f32 / 2.0, Y: h as f32 - FOOT_PAD }
}

fn monitor_at(x: f32, y: f32) -> HMONITOR {
    // 腳底在地面那條線上，往上一點才會落在螢幕裡。
    unsafe { MonitorFromPoint(POINT { x: x as i32, y: y as i32 - 1 }, MONITOR_DEFAULTTONEAREST) }
}

fn work_area(monitor: HMONITOR) -> RECT {
    let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe {
        let _ = GetMonitorInfoW(monitor, &mut info);
    }
    info.rcWork
}

fn monitor_dpi(monitor: HMONITOR) -> u32 {
    let (mut x, mut y) = (BASE_DPI, BASE_DPI);
    unsafe {
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y);
    }
    x
}

fn idle_input_secs() -> f32 {
    let mut info = LASTINPUTINFO { cbSize: size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    unsafe {
        if !GetLastInputInfo(&mut info).as_bool() {
            return 0.0;
        }
        GetTickCount().wrapping_sub(info.dwTime) as f32 / 1000.0
    }
}

fn cursor() -> (f32, f32) {
    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }
    (pt.x as f32, pt.y as f32)
}

fn local_now() -> chrono::NaiveDateTime {
    chrono::Local::now().naive_local()
}

impl App {
    fn run_loop(&mut self) -> Result<()> {
        let timer = unsafe { CreateWaitableTimerExW(None, PCWSTR::null(), CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, TIMER_ALL_ACCESS.0)? };
        let result = self.pump(timer);
        unsafe {
            let _ = CloseHandle(timer);
        }
        result
    }

    fn pump(&mut self, timer: HANDLE) -> Result<()> {
        loop {
            let due = -((HUNDRED_NS_PER_SEC / self.desired_fps() as f32) as i64);
            unsafe { SetWaitableTimer(timer, &due, 0, None, None, false)? };
            loop {
                let woke = unsafe { MsgWaitForMultipleObjectsEx(Some(&[timer]), INFINITE, QS_ALLINPUT, MWMO_INPUTAVAILABLE) };
                let mut msg = MSG::default();
                while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
                    if msg.message == WM_QUIT {
                        return Ok(());
                    }
                    unsafe {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
                self.handle_events()?;
                if woke == WAIT_OBJECT_0 {
                    break;
                }
            }
            self.tick()?;
        }
    }

    fn handle_events(&mut self) -> Result<()> {
        let events = EVENTS.with_borrow_mut(std::mem::take);
        for event in events {
            let now = self.now();
            let env = self.env();
            match event {
                Event::PointerDown => {
                    let (x, y) = cursor();
                    self.pet.input(Input::PointerDown { x, y }, now, &env);
                }
                Event::PointerMove => {
                    let (x, y) = cursor();
                    self.pet.input(Input::PointerMove { x, y }, now, &env);
                }
                Event::PointerUp => {
                    self.pet.input(Input::PointerUp, now, &env);
                }
                Event::Hover => {
                    let (x, _) = cursor();
                    self.pet.input(Input::Hover { x }, now, &env);
                }
                Event::Menu => self.show_menu()?,
                Event::TaskbarCreated => self.tray.add(),
                Event::DisplayChanged => self.broadcast(&ModuleEvent::DisplayChanged),
                Event::HolidaysUpdated => {
                    self.workday = None;
                    self.broadcast(&ModuleEvent::HolidaysUpdated);
                }
                Event::UpdateReady => self.update_ready = true,
                Event::SettingChanged => self.palette = theme::with_accent(theme::current(), self.pack.accent.as_deref()),
                Event::Command(text) => self.command(&text)?,
                Event::Area(input) => self.area_input(input)?,
                Event::ToyPoke => {
                    let jitter = self.rand01() * 2.0 - 1.0;
                    let s = self.dpi_scale();
                    if let Some(slot) = self.toy.as_mut() {
                        slot.toy.poke(jitter, s);
                        slot.touched = now;
                    }
                }
                Event::Card(kind, x, y) => {
                    let action = self.bubble.pointer(kind, x, y);
                    OVER_BUTTON.set(self.bubble.is_over_button(x, y));
                    if let Some(action) = action {
                        self.card_action(action);
                    }
                }
            }
        }
        Ok(())
    }

    fn broadcast(&mut self, event: &ModuleEvent) {
        let mut out = Vec::new();
        self.modules.broadcast(event, local_now(), &self.config, &mut out);
        self.apply(out);
    }

    /// 處理模組丟出來的請求。
    fn apply(&mut self, requests: Vec<(&'static str, Request)>) {
        for (owner, request) in requests {
            match request {
                Request::Card { key, card } => self.enqueue(owner, key, card),
                Request::Withdraw { key } => self.cards.retain(|q| !(q.owner == owner && q.key == key)),
                Request::Say { text } => {
                    let now = self.now();
                    let ctx = self.talk_ctx(now);
                    if ctx.can_speak && !ctx.asleep {
                        self.say(Say { text, emote: None, secs: MODULE_SAY_SECS }, now);
                    }
                }
            }
        }
    }

    fn enqueue(&mut self, owner: &'static str, key: String, card: Card) {
        if let Some(q) = self.cards.iter_mut().find(|q| q.owner == owner && q.key == key) {
            q.card = card;
            self.shown_card = false;
            return;
        }
        if self.cards.len() >= MAX_CARDS {
            let drop_at = self.cards.iter().position(|q| matches!(q.card, Card::Info { .. })).unwrap_or(0);
            self.cards.remove(drop_at);
        }
        self.cards.push_back(Queued { owner, key, card });
    }

    fn card_action(&mut self, action: CardAction) {
        if matches!(self.bubble.card(), Some(Card::Speech { .. })) {
            self.speech = None;
            self.pet.stop_talking(self.now());
            return;
        }
        let Some(front) = self.cards.front_mut() else { return };
        if action == CardAction::OpenSnooze {
            if let Card::Reminder { snooze_open, .. } = &mut front.card {
                *snooze_open = true;
                let card = front.card.clone();
                self.bubble.show(card, self.now(), false);
            }
            return;
        }
        let q = self.cards.pop_front().expect("front exists");
        self.shown_card = false;
        if q.owner == "core" && q.key == UPDATE_CARD_KEY {
            // 「下次啟動時」什麼都不用做：解好的新版留著，下次啟動就會先換上。
            if action == CardAction::Done {
                self.update_now();
            }
            return;
        }
        let mut out = Vec::new();
        self.modules.send(q.owner, &ModuleEvent::Card { key: &q.key, action }, local_now(), &self.config, &mut out);
        self.apply(out);
    }

    fn command(&mut self, text: &str) -> Result<()> {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
            log!("bad command: {text}");
            return Ok(());
        };
        log!("command: {text}");
        match v["cmd"].as_str().unwrap_or_default() {
            ipc::CMD_TEST_REMINDER => self.enqueue("core", "test".into(), Card::Reminder { title: v["title"].as_str().unwrap_or("測試提醒").to_owned(), time: local_now().format("%H:%M").to_string(), snooze_open: false }),
            ipc::CMD_CARD_ACTION => {
                let action = match v["action"].as_str().unwrap_or_default() {
                    "done" => Some(CardAction::Done),
                    "openSnooze" => Some(CardAction::OpenSnooze),
                    "dismiss" => Some(CardAction::Dismiss),
                    "snooze" => Some(CardAction::Snooze(v["minutes"].as_u64().unwrap_or(5) as u32)),
                    _ => None,
                };
                if let Some(a) = action {
                    self.card_action(a);
                }
            }
            ipc::CMD_SWITCH_PET => {
                if let Some(id) = v["id"].as_str() {
                    self.begin_switch(id.to_owned(), true);
                }
            }
            ipc::CMD_OPEN_SETTINGS => self.open_settings(v["page"].as_str()),
            ipc::CMD_DUMP_STATUS => self.dump_status(),
            ipc::CMD_TOY => self.spawn_toy(v["kind"].as_str() == Some("ball")),
            ipc::CMD_DRAW_AREA => {
                let kind = if v["kind"].as_str() == Some(ipc::AREA_ALLOWED) { AreaKind::Allowed } else { AreaKind::Forbidden };
                self.open_area_editor(kind)?;
            }
            ipc::CMD_SAY => {
                let text = v["text"].as_str().unwrap_or("測試講話").to_owned();
                let secs = v["secs"].as_f64().unwrap_or(4.0) as f32;
                self.say(Say { text, emote: None, secs }, self.now());
            }
            // 自動測試用：把桌寵拎到 (x, y) 放手（跟真的用滑鼠拖一樣走拖曳流程）。
            ipc::CMD_DROP => {
                if let (Some(x), Some(y)) = (v["x"].as_f64(), v["y"].as_f64()) {
                    let (now, env) = (self.now(), self.env());
                    let (px, py) = (self.pet.x, self.pet.y - self.sprites.base_height / 2.0);
                    self.pet.input(Input::PointerDown { x: px, y: py }, now, &env);
                    self.pet.input(Input::PointerMove { x: x as f32, y: y as f32 - self.sprites.base_height / 2.0 }, now, &env);
                    self.pet.input(Input::PointerUp, now, &env);
                }
            }
            ipc::CMD_TEST_MEMORY_ALERT => {
                const MB: usize = 1024 * 1024;
                let samples: Vec<usize> = (0..memwatch::TREND_SAMPLES).map(|i| 15 * MB + i * 20 * MB / memwatch::TREND_SAMPLES).collect();
                self.memory_alert(&memwatch::Leak { from_bytes: samples[0], to_bytes: samples[samples.len() - 1], samples });
            }
            ipc::CMD_QUIT => self.quit(),
            _ => self.broadcast(&ModuleEvent::Command(&v)),
        }
        Ok(())
    }

    fn dump_status(&self) {
        if let Err(e) = write_atomic(&paths::status_file(), self.status().to_string().as_bytes()) {
            log!("write status failed: {e}");
        }
    }

    /// 記憶體一路往上漲：存一份診斷檔（當下狀態＋這一天的記憶體曲線），再冒泡告訴使用者。
    fn memory_alert(&mut self, leak: &memwatch::Leak) {
        const MB: f64 = 1024.0 * 1024.0;
        log!("memory kept rising: {:.1}MB -> {:.1}MB", leak.from_bytes as f64 / MB, leak.to_bytes as f64 / MB);
        let unix = SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let path = paths::logs_dir().join(format!("diagnostic-{unix}.json"));
        let report = serde_json::json!({ "status": self.status(), "fromBytes": leak.from_bytes, "toBytes": leak.to_bytes, "samples": leak.samples });
        if let Err(e) = write_atomic(&path, report.to_string().as_bytes()) {
            log!("write diagnostic failed: {e}");
        }
        let detail = format!("一天內從 {:.0}MB 漲到 {:.0}MB。重新開啟柚伴就會恢復", leak.from_bytes as f64 / MB, leak.to_bytes as f64 / MB);
        self.enqueue("core", "memory".into(), Card::warning("柚伴的記憶體一直在增加", Some(detail)));
    }

    fn status(&self) -> serde_json::Value {
        let mem = memwatch::sample();
        serde_json::json!({
            "pet": self.pack.id,
            "state": self.pet.state(),
            "x": self.pet.x,
            "y": self.pet.y,
            "card": self.cards.front().map(|q| format!("{}:{}:{:?}", q.owner, q.key, q.card)),
            "speech": self.speech.as_ref().map(|s| s.text.clone()),
            "days": self.companions.days(&self.pack.id),
            "toy": self.toy.as_ref().map(|t| serde_json::json!({ "kind": format!("{:?}", t.toy.kind), "x": t.toy.x, "y": t.toy.y, "resting": t.toy.resting, "kicks": t.toy.kicks })),
            "queued": self.cards.len(),
            "platforms": self.platforms.iter().map(|p| [p.left, p.right, p.y]).collect::<Vec<_>>(),
            "hidden": self.hidden,
            "modules": self.modules.ids(),
            "privateBytes": mem.private_bytes,
            "workingSet": mem.working_set,
        })
    }

    fn show_menu(&mut self) -> Result<()> {
        let pets = list_pets(&self.pets_dir);
        let mut items = Vec::new();
        let days = self.companions.days(&self.pack.id);
        if days > 0 {
            items.push(MenuItem::Item { id: MENU_DAYS, label: format!("♥ 陪你第 {days} 天"), checked: false });
            items.push(MenuItem::Separator);
        }
        items.extend([
            MenuItem::Sub {
                label: "換一隻桌寵".into(),
                items: pets.iter().enumerate().map(|(i, (id, name))| MenuItem::Item { id: MENU_PET_BASE + i as u32, label: name.clone(), checked: *id == self.pack.id }).collect(),
            },
            MenuItem::Item { id: MENU_FEED, label: "餵點心".into(), checked: false },
            MenuItem::Item { id: MENU_BALL, label: "丟球".into(), checked: false },
        ]);
        items.push(MenuItem::Separator);
        for (id, label) in self.modules.menu() {
            items.push(MenuItem::Item { id, label: label.into(), checked: false });
        }
        items.push(MenuItem::Separator);
        if self.config.features.reminders {
            items.push(MenuItem::Item { id: MENU_ADD_REMINDER, label: "新增提醒…".into(), checked: false });
        }
        items.push(MenuItem::Item { id: MENU_SETTINGS, label: "設定…".into(), checked: false });
        items.push(MenuItem::Separator);
        items.push(MenuItem::Item { id: MENU_EXIT, label: "結束".into(), checked: false });

        match tray::show_menu(self.hwnd, &items) {
            Some(MENU_EXIT) => self.quit(),
            Some(MENU_ADD_REMINDER) => self.open_settings(Some(SETTINGS_PAGE_REMINDERS)),
            Some(MENU_SETTINGS) => self.open_settings(None),
            Some(MENU_DAYS) => self.open_settings(Some(SETTINGS_PAGE_PETS)),
            Some(MENU_FEED) => self.spawn_toy(false),
            Some(MENU_BALL) => self.spawn_toy(true),
            Some(id) if (MENU_PET_BASE..crate::modules::MENU_BASE).contains(&id) => {
                if let Some((pet, _)) = pets.get((id - MENU_PET_BASE) as usize) {
                    self.begin_switch(pet.clone(), true);
                }
            }
            Some(id) => {
                let mut out = Vec::new();
                self.modules.menu_clicked(id, local_now(), &self.config, &mut out);
                self.apply(out);
            }
            None => {}
        }
        Ok(())
    }

    /// 用 WM_CLOSE 排進訊息佇列，而不是當場 DestroyWindow：當場關掉的話，
    /// 同一輪接下來的那一格還會拿已經不存在的視窗去畫，結束時噴「無效的視窗控制代碼」。
    fn quit(&self) {
        unsafe {
            let _ = PostMessageW(Some(self.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }

    fn open_settings(&mut self, page: Option<&str>) {
        let Some(exe) = paths::settings_exe() else {
            self.enqueue("core", "info".into(), Card::warning("找不到設定程式", Some(format!("{} 要跟 {} 放在同一個資料夾", paths::SETTINGS_EXE, paths::CORE_EXE))));
            return;
        };
        let mut cmd = std::process::Command::new(exe);
        if let Some(p) = page {
            cmd.args(["--page", p]);
        }
        if let Err(e) = cmd.spawn() {
            log!("open settings failed: {e}");
        }
    }

    fn desired_fps(&self) -> u32 {
        if self.hidden {
            FPS_HIDDEN
        } else if self.effects.active() || self.transition.is_some() || self.now() - self.last_ride_move < RIDE_SMOOTH_SECS || self.toy.as_ref().is_some_and(|t| !t.toy.resting || t.fading.is_some()) {
            FPS_ANIMATING
        } else {
            self.pet.desired_fps()
        }
    }

    /// 全螢幕程式（影片、簡報、遊戲）在前面時躲起來；提醒卡留在佇列裡，離開全螢幕才出現。
    /// 回傳 `true` = 現在是躲起來的狀態，這一格不用畫。
    fn update_hidden(&mut self, now: f64) -> bool {
        if now - self.last_busy_check >= BUSY_CHECK_SECS {
            self.last_busy_check = now;
            let busy = self.config.hide_in_fullscreen && platforms::user_is_busy();
            if busy != self.hidden {
                self.hidden = busy;
                log!("fullscreen app {}; pet {}", if busy { "detected" } else { "gone" }, if busy { "hidden" } else { "shown" });
                unsafe {
                    let _ = ShowWindow(self.hwnd, if busy { SW_HIDE } else { SW_SHOWNOACTIVATE });
                    if self.toy.is_some() {
                        let _ = ShowWindow(self.toy_hwnd, if busy { SW_HIDE } else { SW_SHOWNOACTIVATE });
                    }
                }
                if busy {
                    self.glow.hide(self.glow_hwnd);
                }
                if busy {
                    self.bubble.hide();
                    self.shown_card = false;
                }
                // 躲起來的期間不算時間，回來時動畫不會一口氣跳很多格。
                self.last_tick = now;
            }
        }
        self.hidden
    }

    fn scan_platforms(&mut self, now: f64) {
        if !self.config.stand_on_windows {
            return;
        }
        if now - self.last_platform_scan < PLATFORM_SCAN_SECS {
            self.follow_ridden_window(now);
            return;
        }
        self.last_platform_scan = now;
        let work = work_area(monitor_at(self.pet.x, self.pet.y));
        self.platforms = platforms::scan(work, self.idle_width() * PLATFORM_MIN_WIDTH_RATIO, self.sprites.base_height);
    }

    /// 完整掃描之間，只看桌寵站著的那一個視窗：它被拖動就把它的那幾段一起平移（便宜，可以每一格做）。
    fn follow_ridden_window(&mut self, now: f64) {
        let Some(window) = self.pet.riding_window() else { return };
        let Some(seg) = self.platforms.iter().find(|p| p.window == window) else { return };
        let (old_left, old_top) = (seg.window_left, seg.y);
        match platforms::window_origin(window) {
            Some((left, top)) => {
                let (dx, dy) = (left - old_left, top - old_top);
                if dx == 0.0 && dy == 0.0 {
                    return;
                }
                self.last_ride_move = now;
                for p in self.platforms.iter_mut().filter(|p| p.window == window) {
                    p.left += dx;
                    p.right += dx;
                    p.y += dy;
                    p.window_left = left;
                }
            }
            // 視窗關掉或縮小了：腳下的東西不見，牠會掉下來。
            None => self.platforms.retain(|p| p.window != window),
        }
    }

    fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    fn dpi_scale(&self) -> f32 {
        self.sprites.dpi as f32 / BASE_DPI as f32
    }

    fn env(&self) -> Env {
        let rect = work_area(monitor_at(self.pet.x, self.pet.y));
        let half = self.idle_width() / 2.0;
        let mut env = Env {
            ground_y: rect.bottom as f32,
            min_x: rect.left as f32 + half,
            max_x: (rect.right as f32 - half).max(rect.left as f32 + half),
            px_per_dip: self.dpi_scale(),
            idle_input_secs: idle_input_secs(),
            sleep_after_secs: self.config.sleep_after_secs as f32,
            walk_speed_dip: self.pack.states.get(states::WALK).and_then(|s| s.speed),
            cursor: Some(cursor()),
            pet_height: self.sprites.base_height,
            pet_half_width: half,
            platforms: if self.config.stand_on_windows { self.platforms.clone() } else { Vec::new() },
            activity: self.config.activity,
            wall_left: rect.left as f32,
            wall_right: rect.right as f32,
            ceiling: rect.top as f32,
            item: self.toy.as_ref().filter(|t| t.fading.is_none()).map(|t| ItemView {
                x: t.toy.x,
                y: t.toy.y,
                food: t.toy.kind.is_food(),
                grounded: t.toy.grounded(),
                wanted: t.toy.kind.is_food() || t.toy.kicks < BALL_MAX_KICKS,
            }),
            blocks: Vec::new(),
            invisible_floor: false,
            invisible_walls: (false, false),
        };
        match self.config.roam {
            Roam::Screen => {}
            Roam::Inside => {
                if let Some(area) = self.config.allowed_area {
                    env.confine_to(area_rect(area));
                }
            }
            Roam::Avoid => {
                env.blocks = self.forbidden_rects();
                env.platforms.extend(behavior::block_platforms(&env.blocks, env.ceiling, env.pet_height));
            }
        }
        env
    }

    fn open_area_editor(&mut self, kind: AreaKind) -> Result<()> {
        self.area_editor = None;
        let s = self.dpi_scale();
        let (existing, min, color) = match kind {
            AreaKind::Allowed => (self.config.allowed_area.into_iter().collect(), (self.idle_width() * MIN_ALLOWED_WIDTHS, self.sprites.base_height * MIN_ALLOWED_HEIGHTS), self.palette.accent.d2d()),
            AreaKind::Forbidden => (self.config.forbidden_areas.clone(), (MIN_FORBIDDEN_DIP * s, MIN_FORBIDDEN_DIP * s), self.palette.danger.d2d()),
        };
        let min = (min.0.ceil() as i32, min.1.ceil() as i32);
        self.area_editor = Some(AreaEditor::open(&self.gfx, AREA_CLASS, kind, existing, min, s, color)?);
        Ok(())
    }

    /// 畫範圍的畫面收到滑鼠或鍵盤：畫好就寫進設定檔（設定程式回到前景時會重新讀）。
    fn area_input(&mut self, input: AreaInput) -> Result<()> {
        let Some(editor) = self.area_editor.as_mut() else { return Ok(()) };
        let kind = editor.kind;
        let Some(outcome) = editor.input(input)? else { return Ok(()) };
        self.area_editor = None;
        let Outcome::Done(area) = outcome else { return Ok(()) };
        log!("{kind:?} area drawn: {area:?}");
        let text = match kind {
            AreaKind::Allowed => {
                self.config.allowed_area = Some(area);
                SAY_ALLOWED_SET
            }
            AreaKind::Forbidden => {
                self.config.forbidden_areas.push(area);
                SAY_FORBIDDEN_SET
            }
        };
        let path = paths::config_file();
        if let Err(e) = self.config.save(&path) {
            log!("save config failed: {e}");
        }
        self.config_stamp = stamp(&path);
        if self.config.talk != Talk::Never {
            self.say(Say { text: text.to_owned(), emote: Some(Emote::Note), secs: SAY_AREA_SECS }, self.now());
        }
        Ok(())
    }

    /// 「避開這些地方」開著時的禁止範圍。
    fn forbidden_rects(&self) -> Vec<Rect> {
        if self.config.roam != Roam::Avoid {
            return Vec::new();
        }
        self.config.forbidden_areas.iter().map(|a| area_rect(*a)).collect()
    }

    fn idle_width(&self) -> f32 {
        self.sprites.states[states::IDLE].variants[0].frames[0].width
    }

    fn tick(&mut self) -> Result<()> {
        let now = self.now();
        let dt = ((now - self.last_tick) as f32).clamp(0.0, MAX_DT);
        self.last_tick = now;

        if now - self.last_module_tick >= MODULE_TICK_SECS {
            self.last_module_tick = now;
            self.slow_tick(now);
        }
        self.drive_soak(now)?;
        if self.update_hidden(now) {
            return Ok(());
        }
        self.reload_if_dpi_changed()?;
        let appear = self.advance_transition(now)?;
        self.scan_platforms(now);

        let env = self.env();
        if self.pet.update(now, dt, &env) {
            self.save_state();
        }
        let glow = self.pet.glow(&env, now);
        let style = GlowStyle { pet_height: self.sprites.base_height, scale: self.dpi_scale(), color: self.palette.danger.d2d() };
        self.glow.update(self.glow_hwnd, self.hwnd, glow, dt, &style)?;
        for happening in self.pet.drain_happenings() {
            let ctx = self.talk_ctx(now);
            if let Some(say) = self.chatter.on(happening, &ctx) {
                self.say(say, now);
            }
            match happening {
                Happening::Ate => self.remove_toy(),
                Happening::Kicked { dir } => {
                    let s = self.dpi_scale();
                    if let Some(slot) = self.toy.as_mut() {
                        slot.toy.kick(dir, s);
                        slot.touched = now;
                    }
                }
                _ => {}
            }
        }
        self.update_cards(now)?;
        for emote in self.pet.drain_emotes() {
            self.effects.spawn(emote, now);
        }
        self.effects.prune(now);

        let pose = self.pet.pose(now);
        self.advance_anim(pose.state, dt);
        let variant = &self.sprites.states[&self.anim.source].variants[self.anim.variant];
        let frame = &variant.frames[self.anim.frame];
        let procedural = !self.anim.own || variant.procedural;
        let anchor = anchor(self.window_size);
        let origin = POINT { x: (self.pet.x - anchor.X).round() as i32, y: (self.pet.y - anchor.Y).round() as i32 };
        let scene = Scene {
            frame,
            pose,
            procedural,
            base_height: self.sprites.base_height,
            appear,
            effects: &self.effects,
            now,
            own_frames: self.anim.own,
        };
        self.renderer.draw(anchor, &scene)?;
        self.renderer.canvas.present(self.hwnd, origin, 1.0)?;
        self.update_toy(now, dt)?;
        Ok(())
    }

    fn rand01(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }

    /// 從右鍵選單丟一個點心或球：從桌寵旁邊（往螢幕中間那側）的上方掉下來。已經有一個就換掉。
    fn spawn_toy(&mut self, ball: bool) {
        let h = self.sprites.base_height;
        let s = self.dpi_scale();
        let work = work_area(monitor_at(self.pet.x, self.pet.y));
        let side = if self.pet.x > (work.left + work.right) as f32 / 2.0 { -1.0 } else { 1.0 };
        let size = h * toy::SIZE_PER_HEIGHT;
        let x = (self.pet.x + side * TOY_DROP_SIDE_HEIGHTS * h).clamp(work.left as f32 + size, work.right as f32 - size);
        // 從桌寵頭上的高度掉下來；中間隔著別的視窗頂的話，從那個視窗頂底下開始掉，才會落在牠搆得到的那一層。
        let blocked = self.platforms.iter().filter(|p| x >= p.left && x <= p.right && p.y < self.pet.y - 1.0).map(|p| p.y + 1.0).fold(f32::NEG_INFINITY, f32::max);
        let y = (self.pet.y - TOY_DROP_HEIGHTS * h).max(work.top as f32 + size).max(blocked);
        let toy = if ball {
            Toy::new(ToyKind::ball(self.rand01()), x, y, side * BALL_THROW_X_DIP * s, -BALL_THROW_UP_DIP * s, size)
        } else {
            Toy::new(ToyKind::food(self.rand01()), x, y, 0.0, 0.0, size)
        };
        log!("toy: {:?}", toy.kind);
        self.toy = Some(ToySlot { toy, touched: self.now(), fading: None, done_at: None });
    }

    fn remove_toy(&mut self) {
        if self.toy.take().is_some() {
            unsafe {
                let _ = ShowWindow(self.toy_hwnd, SW_HIDE);
            }
        }
    }

    /// 點心／球的物理、什麼時候收掉、畫出來。
    fn update_toy(&mut self, now: f64, dt: f32) -> Result<()> {
        let blocks = self.forbidden_rects();
        let Some(slot) = self.toy.as_mut() else { return Ok(()) };
        let rect = work_area(monitor_at(slot.toy.x, slot.toy.y - 1.0));
        let mut platforms = if self.config.stand_on_windows { self.platforms.clone() } else { Vec::new() };
        // 禁止範圍的頂端也接得住東西（掉進範圍裡，桌寵就拿不到了）。
        platforms.extend(behavior::block_platforms(&blocks, rect.top as f32, self.sprites.base_height));
        let world = toy::World { ground_y: rect.bottom as f32, wall_left: rect.left as f32, wall_right: rect.right as f32, platforms: &platforms, px_per_dip: self.sprites.dpi as f32 / BASE_DPI as f32 };
        let landed = slot.toy.update(dt, &world);
        if landed {
            self.effects.spawn(Emote::Dust, now);
        }
        let played_out = !slot.toy.kind.is_food() && slot.toy.kicks >= BALL_MAX_KICKS && slot.toy.resting;
        let done = played_out || (slot.toy.resting && now - slot.touched >= TOY_UNTOUCHED_MAX_SECS);
        if done && slot.done_at.is_none() {
            slot.done_at = Some(now);
        }
        if slot.fading.is_none() && slot.done_at.is_some_and(|t| now - t >= TOY_LINGER_SECS) {
            slot.fading = Some(now);
        }
        let opacity = slot.fading.map_or(1.0, |t| 1.0 - ((now - t) / TOY_FADE_SECS) as f32);
        if opacity <= 0.0 {
            self.remove_toy();
            return Ok(());
        }
        let (_, origin) = slot.toy.window();
        self.toy_painter.draw(&slot.toy)?;
        self.toy_painter.present(self.toy_hwnd, origin, opacity)?;
        unsafe {
            if !IsWindowVisible(self.toy_hwnd).as_bool() {
                let _ = ShowWindow(self.toy_hwnd, SW_SHOWNOACTIVATE);
            }
        }
        Ok(())
    }

    /// 每秒一次：功能模組、設定檔變動、辦公日曆下載。
    fn slow_tick(&mut self, now: f64) {
        let mut out = Vec::new();
        self.modules.tick(local_now(), &self.config, &mut out);
        self.apply(out);

        let ctx = self.talk_ctx(now);
        let greeted = self.chatter.greeted_on();
        if let Some(say) = self.chatter.tick(&ctx) {
            self.say(say, now);
        }
        if self.chatter.greeted_on() != greeted {
            self.save_state();
        }
        self.count_day(now);

        if let Some(leak) = self.memwatch.tick(Instant::now()) {
            self.memory_alert(&leak);
        }

        let config_path = paths::config_file();
        let s = stamp(&config_path);
        if s != self.config_stamp {
            self.config_stamp = s;
            self.config_changed(load_config(&config_path));
        }

        if self.config.features.reminders && now - self.last_holiday_check >= HOLIDAY_CHECK_SECS {
            self.last_holiday_check = now;
            let cache = paths::holiday_cache_dir();
            if holiday_sync::due(&cache) {
                holiday_sync::spawn(cache, self.hwnd);
            }
        }

        self.check_update(now);
    }

    /// 每小時看一次要不要去檢查新版（實際一天最多查一次）；新版解好了就挑空檔換上。
    fn check_update(&mut self, now: f64) {
        let Some(install) = paths::install_dir() else { return };
        if !self.config.auto_update || self.soak.is_some() || !updater::enabled_for(&install) {
            return;
        }
        if now - self.last_update_check >= UPDATE_CHECK_SECS {
            self.last_update_check = now;
            let dir = paths::update_dir();
            if updater::staged(&dir).is_some() {
                self.update_ready = true;
            } else if updater::due(&dir) {
                updater::spawn(dir, &install, self.hwnd);
            }
        }
        if !self.update_ready || self.update_offered {
            return;
        }
        self.update_ready = false;
        self.update_offered = true;
        if let Some(version) = updater::staged_version(&paths::update_dir()).filter(|_| updater::staged(&paths::update_dir()).is_some()) {
            self.enqueue("core", UPDATE_CARD_KEY.into(), Card::Update { version });
        }
    }

    /// 使用者在「新版準備好了」卡上按了「現在更新」。
    fn update_now(&mut self) {
        let (Some(install), Some(staged)) = (paths::install_dir(), updater::staged(&paths::update_dir())) else { return };
        match updater::apply_and_restart(&staged, &install) {
            Ok(()) => {
                log!("update applied; restarting");
                self.quit();
            }
            Err(e) => {
                log!("update apply failed: {e}");
                self.enqueue("core", "info".into(), Card::warning("更新沒有成功", Some("下次啟動柚伴時會再試一次".into())));
            }
        }
    }

    fn drive_soak(&mut self, now: f64) -> Result<()> {
        let Some(soak) = self.soak.as_mut() else { return Ok(()) };
        if soak.finished(now) {
            soak.report();
            self.soak = None;
            self.quit();
            return Ok(());
        }
        let Some(action) = soak.next(now) else { return Ok(()) };
        let env = self.env();
        let s = self.dpi_scale();
        match action {
            Action::SwitchPet => {
                let pets = list_pets(&self.pets_dir);
                if let Some((id, _)) = pets.iter().find(|(id, _)| *id != self.pack.id) {
                    self.begin_switch(id.clone(), false);
                }
            }
            Action::ShowCard => self.enqueue("core", "test".into(), Card::Reminder { title: "壓力測試提醒".into(), time: "12:00".into(), snooze_open: false }),
            Action::OpenSnooze => self.card_action(CardAction::OpenSnooze),
            Action::Done => self.card_action(CardAction::Done),
            Action::ShowInfo => self.enqueue("core", "info".into(), Card::success("壓力測試通知", Some("第二行說明".into()))),
            Action::Grab => {
                self.pet.input(Input::PointerDown { x: self.pet.x, y: self.pet.y - 10.0 * s }, now, &env);
            }
            Action::Drag => {
                self.pet.input(Input::PointerMove { x: self.pet.x + 120.0 * s, y: self.pet.y - 200.0 * s }, now, &env);
            }
            Action::Release => {
                self.pet.input(Input::PointerUp, now, &env);
            }
        }
        Ok(())
    }

    fn config_changed(&mut self, new: Config) {
        let old = std::mem::replace(&mut self.config, new);
        if old.features != self.config.features {
            for id in self.modules.sync(&enabled_modules(&self.config), make_module) {
                self.cards.retain(|q| q.owner != id);
            }
            log!("modules now {:?}", self.modules.ids());
        }
        if old.pet != self.config.pet {
            self.begin_switch(self.config.pet.clone(), false);
        } else if old.size_dip != self.config.size_dip {
            // 尺寸變了：用新尺寸重新縮圖（借用換桌寵的過場）。
            self.begin_switch(self.pack.id.clone(), false);
        }
    }

    /// 講話系統要看的環境。
    fn talk_ctx(&mut self, now: f64) -> chatter::Ctx {
        let clock = local_now();
        chatter::Ctx {
            now,
            clock,
            idle_input_secs: idle_input_secs(),
            talk: self.config.talk,
            workday: self.is_workday(clock.date()),
            can_speak: !self.hidden && self.cards.is_empty() && self.transition.is_none() && self.soak.is_none(),
            asleep: self.pet.state() == states::SLEEP,
        }
    }

    /// 看辦公日曆（含使用者在提醒頁手動設的「這天放假／上班」）；同一天只算一次。
    fn is_workday(&mut self, date: chrono::NaiveDate) -> bool {
        if let Some((d, w)) = self.workday
            && d == date
        {
            return w;
        }
        let overrides = petkit::reminders::ReminderFile::load(&paths::reminders_file()).map(|f| f.day_overrides).unwrap_or_default();
        let (calendar, _) = petkit::holidays::load_calendar(&paths::holiday_cache_dir(), overrides);
        let w = calendar.is_workday(date);
        self.workday = Some((date, w));
        w
    }

    /// 今天有見到（使用者在電腦前、牠在桌面上）就把今天算進陪伴天數；第一天與紀念日等牠有空時講一句。
    fn count_day(&mut self, now: f64) {
        if !self.hidden && idle_input_secs() < PRESENT_IDLE_SECS {
            let today = local_now().date();
            let c = self.companions.pets.entry(self.pack.id.clone()).or_default();
            if c.visit(today) {
                let days = c.days;
                log!("days together with {}: {days}", self.pack.id);
                if let Err(e) = self.companions.save(&paths::companion_file()) {
                    log!("save companion failed: {e}");
                }
                if days == 1 || companion::is_milestone(days) {
                    self.pending_milestone = Some(days);
                }
            }
        }
        let free = self.speech.is_none() && self.cards.is_empty() && !self.hidden && self.transition.is_none() && now >= MILESTONE_AFTER_SECS;
        if !free {
            return;
        }
        let Some(days) = self.pending_milestone.take() else { return };
        self.pet.cheer(now);
        self.effects.spawn(Emote::Star, now);
        if self.config.talk != Talk::Never {
            let text = if days == 1 { "初次見面，請多指教！".to_owned() } else { format!("我們一起過了 {days} 天了！謝謝你一直陪著我") };
            self.say(Say { text, emote: Some(Emote::Hearts), secs: MILESTONE_SAY_SECS }, now);
        }
    }

    /// 開口講一句：泡泡＋講話動作（＋表情）。
    fn say(&mut self, say: Say, now: f64) {
        log!("say: {}", say.text);
        self.pet.talk(say.secs, now);
        if let Some(e) = say.emote {
            self.effects.spawn(e, now);
        }
        self.speech = Some(Speech { text: say.text, until: now + say.secs as f64 });
    }

    fn update_cards(&mut self, now: f64) -> Result<()> {
        // 提醒卡優先：卡片一來，講到一半的話直接收掉；講完了也收掉。
        if self.speech.as_ref().is_some_and(|s| !self.cards.is_empty() || now >= s.until) {
            self.speech = None;
            self.pet.stop_talking(now);
        }
        if let Some(front) = self.cards.front() {
            if matches!(front.card, Card::Info { .. }) && self.shown_card && self.bubble.shown_for(now) >= INFO_SECS {
                self.cards.pop_front();
                self.shown_card = false;
            }
        }
        match self.cards.front() {
            Some(q) => {
                if !self.shown_card || self.bubble.card() != Some(&q.card) {
                    let animate = !self.shown_card;
                    if animate && self.config.reminder_sound && matches!(q.card, Card::Reminder { .. } | Card::Missed { .. }) {
                        play_reminder_sound();
                    }
                    self.bubble.show(q.card.clone(), now, animate);
                    self.shown_card = true;
                }
            }
            None => {
                self.shown_card = false;
                match &self.speech {
                    Some(s) => {
                        let card = Card::Speech { text: s.text.clone() };
                        if self.bubble.card() != Some(&card) {
                            self.bubble.show(card, now, true);
                        }
                        self.bubble.set_fade(((s.until - now) / SPEECH_FADE_SECS) as f32);
                    }
                    None => {
                        self.bubble.hide();
                        OVER_BUTTON.set(false);
                    }
                }
            }
        }
        let attention = self.cards.front().is_some_and(|q| matches!(q.card, Card::Reminder { .. } | Card::Missed { .. }));
        self.pet.set_attention(attention, now);

        let head = Vector2 { X: self.pet.x, Y: self.pet.y - self.sprites.base_height };
        let work = work_area(monitor_at(self.pet.x, self.pet.y));
        self.bubble.update(now, head, work, self.dpi_scale(), &self.palette)
    }

    fn begin_switch(&mut self, target: String, save: bool) {
        if self.transition.as_ref().is_some_and(|t| t.target == target) {
            return;
        }
        self.transition = Some(Transition { phase: Phase::Out, start: self.now(), target, save });
    }

    /// 換桌寵的過場；回傳這一格桌寵的大小比例（0～1）。
    fn advance_transition(&mut self, now: f64) -> Result<f32> {
        let Some(t) = &self.transition else { return Ok(1.0) };
        let p = ((now - t.start) / if t.phase == Phase::Out { SWITCH_OUT_SECS } else { SWITCH_IN_SECS }) as f32;
        match t.phase {
            Phase::Out if p < 1.0 => Ok(1.0 - theme::ease(theme::MOTION_EASE_IN, p)),
            Phase::Out => {
                let (target, save) = (t.target.clone(), t.save);
                self.swap_pet(&target, save)?;
                self.transition = Some(Transition { phase: Phase::In, start: now, target, save });
                Ok(0.0)
            }
            Phase::In if p < 1.0 => Ok(theme::ease(theme::MOTION_SPRING, p).max(0.0)),
            Phase::In => {
                self.transition = None;
                Ok(1.0)
            }
        }
    }

    fn swap_pet(&mut self, id: &str, save: bool) -> Result<()> {
        let pack = match pack::load(&self.pets_dir.join(id)) {
            Ok(p) => p,
            Err(e) => {
                self.enqueue("core", "info".into(), Card::warning("這隻桌寵讀不進來", Some(e.to_string())));
                return Ok(());
            }
        };
        let dpi = self.sprites.dpi;
        let height = px_height(&self.config, dpi);
        // 先丟掉舊的圖再載新的，兩隻不會同時佔記憶體；新的圖解碼失敗就把舊的載回來。
        self.sprites.states.clear();
        let target = self.renderer.canvas.target();
        let (pack, sprites) = match self.wic.load_pet(&pack, &target, height, dpi) {
            Ok(s) => (pack, s),
            Err(e) => {
                self.enqueue("core", "info".into(), Card::warning("這隻桌寵的圖讀不進來", Some(e.message())));
                let old = pack::load(&self.pack.dir).map_err(|e| Error::new(E_FAIL, e.to_string()))?;
                let sprites = self.wic.load_pet(&old, &target, height, dpi)?;
                (old, sprites)
            }
        };
        self.sprites = sprites;
        self.tray.set_icon(pet_icon(&self.wic, &pack, dpi)?);
        self.pack = pack;
        self.palette = theme::with_accent(theme::current(), self.pack.accent.as_deref());
        self.anim.state = "";
        self.resize_window()?;
        log!("switched to pet '{}'", self.pack.id);
        if save && self.config.pet != id {
            self.config.pet = id.to_owned();
            let path = paths::config_file();
            if let Err(e) = self.config.save(&path) {
                log!("save config failed: {e}");
            }
            self.config_stamp = stamp(&path);
        }
        Ok(())
    }

    fn advance_anim(&mut self, state: &'static str, dt: f32) {
        if self.anim.state != state {
            let (source, own) = match self.pack.resolve(state) {
                Some(r) => (r.source.to_owned(), r.own),
                None => (states::IDLE.to_owned(), false),
            };
            let variant = self.pick_variant(&source);
            self.anim = Anim { state, source, own, variant, frame: 0, elapsed_ms: 0.0 };
            return;
        }
        let loaded = &self.sprites.states[&self.anim.source];
        let durations = &loaded.variants[self.anim.variant].durations_ms;
        if durations.len() <= 1 {
            return;
        }
        self.anim.elapsed_ms += dt * 1000.0;
        while self.anim.elapsed_ms >= durations[self.anim.frame] as f32 {
            self.anim.elapsed_ms -= durations[self.anim.frame] as f32;
            if self.anim.frame + 1 < durations.len() {
                self.anim.frame += 1;
            } else if loaded.looping {
                self.anim.frame = 0;
            } else {
                self.anim.elapsed_ms = 0.0;
                break;
            }
        }
    }

    /// 依權重隨機挑一個變化款。
    fn pick_variant(&mut self, source: &str) -> usize {
        let variants = &self.sprites.states[source].variants;
        let total: u32 = variants.iter().map(|v| v.weight).sum();
        if total == 0 || variants.len() == 1 {
            return 0;
        }
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        let mut pick = (self.rng % total as u64) as u32;
        for (i, v) in variants.iter().enumerate() {
            if pick < v.weight {
                return i;
            }
            pick -= v.weight;
        }
        0
    }

    /// 桌寵被放到另一個縮放比例的螢幕上 → 用新的比例重新縮圖。
    fn reload_if_dpi_changed(&mut self) -> Result<()> {
        if self.pet.is_dragging() {
            return Ok(());
        }
        let dpi = monitor_dpi(monitor_at(self.pet.x, self.pet.y));
        if dpi == self.sprites.dpi {
            return Ok(());
        }
        self.sprites.states.clear();
        self.sprites = self.wic.load_pet(&self.pack, &self.renderer.canvas.target(), px_height(&self.config, dpi), dpi)?;
        self.anim.state = "";
        log!("dpi changed to {dpi}; sprites reloaded");
        self.resize_window()
    }

    fn resize_window(&mut self) -> Result<()> {
        let (w, h) = window_size(&self.sprites);
        self.window_size = (w, h);
        self.renderer.canvas.ensure_size(w, h)
    }

    fn save_state(&mut self) {
        let state = RuntimeState { position: Some((self.pet.x.round() as i32, self.pet.y.round() as i32)), greeted_on: self.chatter.greeted_on() };
        let result = serde_json::to_vec_pretty(&state).map_err(std::io::Error::other).and_then(|b| write_atomic(&paths::state_file(), &b));
        if let Err(e) = result {
            log!("save state failed: {e}");
        }
    }
}
