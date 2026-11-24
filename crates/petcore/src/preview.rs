//! `Youban.exe --frames <資料夾> [dpi] [桌寵]`：不開視窗，把每個動作跑一遍、每秒 25 格輸出成 PNG 序列，
//! 放在一個固定大小的「舞台」上（地面在下緣、左右是螢幕邊），用來檢查動態是否自然。
//! 舞台背景是透明的，轉成 GIF 時再墊背景。

use std::fs;
use std::path::Path;

use petkit::config::{Activity, Config};
use petkit::{pack, states};
use windows::Win32::Foundation::E_FAIL;
use windows::core::{Error, Result};

use crate::app::{anchor, window_size};
use crate::behavior::{self, Demo, Env, Input, Pet, Platform, Rect};
use crate::effects::Effects;
use crate::glow::{GlowPainter, GlowStyle};
use crate::paths;
use crate::render::{Gfx, PetRenderer, Scene};
use crate::sprites::Wic;
use crate::theme;

const BASE_DPI: f32 = 96.0;
const FPS: f32 = 25.0;
/// 物理每一格切成幾小步算，跟實機的 60fps 接近。
const SUBSTEPS: usize = 3;
/// 舞台大小（桌寵身高的倍數）。
const STAGE_W_HEIGHTS: f32 = 5.5;
const STAGE_H_HEIGHTS: f32 = 3.0;
const BYTES_PER_PIXEL: usize = 4;
/// 預覽時當作使用者剛離開這麼久：睡著的不會被「有人在用電腦」吵醒，
/// 又還不到會自己打瞌睡的時間（不會干擾其他動作的劇本）。
const IDLE_INPUT_SECS: f32 = 10.0;

/// 「撞上禁止範圍」劇本裡那一塊：從舞台的這個比例開始、寬與高（桌寵身高的倍數），貼著地面。
const WALL_LEFT_RATIO: f32 = 0.42;
const WALL_W_HEIGHTS: f32 = 1.3;
const WALL_H_HEIGHTS: f32 = 0.9;

/// 跳視窗的劇本：舞台上兩個視窗，桌寵站在左邊那個靠右邊緣的地方往右走。
/// (名稱, 亂數種子, 兩個視窗中間的縫（身寬倍數）, 右邊比左邊高多少（身高倍數，負的是比較低）)。
/// 種子決定「只差一點」時硬跳還是停下來看：99 會跳、92 只看。
const LEAP_SCRIPTS: &[(&str, u64, f32, f32)] = &[
    ("21-leap-step", 99, 0.15, 0.0),
    ("22-leap-across", 99, 1.0, 0.0),
    ("23-leap-up", 99, 0.6, 0.35),
    ("24-leap-down", 99, 0.8, -0.6),
    ("25-leap-climb", 99, 0.7, 0.9),
    ("26-leap-miss", 99, 1.8, 0.0),
    ("27-leap-look", 92, 1.8, 0.0),
];
/// 左邊視窗：從舞台左緣到這個比例，頂端在地面往上這麼多個身高；桌寵起點在它右邊緣往內這麼多個身高。
const LEAP_LEFT_WINDOW_RATIO: f32 = 0.38;
const LEAP_WINDOW_TOP_HEIGHTS: f32 = 1.0;
const LEAP_START_INSET_HEIGHTS: f32 = 0.9;
const LEAP_WALK_SECS: f32 = 6.0;
const LEAP_SCRIPT_SECS: f32 = 8.0;
/// 舞台上畫視窗的顏色（BGRA，不透明）。
const WINDOW_FILL: [u8; 4] = [236, 232, 228, 255];
const WINDOW_TOP_EDGE: [u8; 4] = [120, 110, 100, 255];

/// 左、右兩個視窗的頂端（跳視窗劇本用）。
fn leap_windows(stage_w: f32, ground_y: f32, h: f32, half: f32, gap_widths: f32, rise_heights: f32) -> Vec<Platform> {
    let top = ground_y - h * LEAP_WINDOW_TOP_HEIGHTS;
    let left_right = stage_w * LEAP_LEFT_WINDOW_RATIO;
    let right_left = left_right + gap_widths * 2.0 * half;
    vec![
        Platform { left: 0.0, right: left_right, y: top, window: 1, window_left: 0.0, sides: (Some(ground_y), Some(ground_y)), bottom: ground_y },
        Platform { left: right_left, right: stage_w, y: top - rise_heights * h, window: 2, window_left: right_left, sides: (Some(ground_y), Some(ground_y)), bottom: ground_y },
    ]
}

/// 把視窗畫成實心方塊（頂端一條深色邊），桌寵跳的時候看得出縫在哪。
fn paint_windows(stage: &mut [u8], stage_w: usize, stage_h: usize, windows: &[Platform]) {
    for w in windows {
        let (x0, x1) = (w.left.max(0.0) as usize, (w.right as usize).min(stage_w));
        for y in (w.y.max(0.0) as usize)..(w.bottom as usize).min(stage_h) {
            let color = if y < w.y as usize + 2 { WINDOW_TOP_EDGE } else { WINDOW_FILL };
            for x in x0..x1 {
                let i = (y * stage_w + x) * BYTES_PER_PIXEL;
                stage[i..i + BYTES_PER_PIXEL].copy_from_slice(&color);
            }
        }
    }
}

/// 一個動作的劇本：每一格開始前可以對桌寵做事（按下、移動、放開…）。
struct Script {
    name: &'static str,
    secs: f32,
    start_x: f32,
    setup: fn(&mut Pet, f64, &Env, f32),
    each: fn(&mut Pet, f64, &Env, f32, f32),
}

/// 這個劇本的舞台中間放一塊看不見的禁止範圍（碰到時亮紅光）。
const WALL_SCRIPT: &str = "20-wall";

fn nothing(_: &mut Pet, _: f64, _: &Env, _: f32) {}
fn nothing_each(_: &mut Pet, _: f64, _: &Env, _: f32, _: f32) {}

/// 在身體中間按下滑鼠（拎起來的起點）。
fn grab(p: &mut Pet, now: f64, env: &Env, h: f32) {
    p.input(Input::PointerDown { x: p.x, y: p.y - h / 2.0 }, now, env);
}

fn demo(d: Demo) -> impl Fn(&mut Pet, f64, &Env) {
    move |pet, now, env| pet.start_demo(d, now, env)
}

fn scripts() -> Vec<Script> {
    let leaps = LEAP_SCRIPTS.iter().map(|&(name, ..)| Script {
        name,
        secs: LEAP_SCRIPT_SECS,
        start_x: 0.0,
        setup: |p, n, e, _| demo(Demo::Walk { secs: LEAP_WALK_SECS })(p, n, e),
        each: nothing_each,
    });
    let mut out = vec![
        // 長到看得到一次眨眼。
        Script { name: "01-idle", secs: 4.5, start_x: 0.5, setup: nothing, each: nothing_each },
        Script { name: "02-walk", secs: 3.0, start_x: 0.25, setup: |p, n, e, _| demo(Demo::Walk { secs: 2.6 })(p, n, e), each: nothing_each },
        Script { name: "03-hop", secs: 1.2, start_x: 0.5, setup: |p, n, e, _| demo(Demo::Hop)(p, n, e), each: nothing_each },
        Script {
            name: "04-click",
            secs: 1.4,
            start_x: 0.5,
            setup: |p, n, e, h| {
                p.input(Input::PointerDown { x: p.x, y: p.y - h / 2.0 }, n, e);
                p.input(Input::PointerUp, n, e);
            },
            each: nothing_each,
        },
        Script { name: "05-look", secs: 2.2, start_x: 0.5, setup: |p, n, e, _| demo(Demo::Look)(p, n, e), each: nothing_each },
        Script { name: "06-stretch", secs: 1.5, start_x: 0.5, setup: |p, n, e, _| demo(Demo::Stretch)(p, n, e), each: nothing_each },
        Script { name: "07-doze", secs: 3.0, start_x: 0.5, setup: |p, n, e, _| demo(Demo::Doze)(p, n, e), each: nothing_each },
        Script { name: "08-petted", secs: 1.8, start_x: 0.5, setup: |p, n, e, _| demo(Demo::Petted)(p, n, e), each: nothing_each },
        Script {
            // 拎起來左右甩兩下，放開掉下來。
            name: "09-drag",
            secs: 3.6,
            start_x: 0.5,
            setup: grab,
            each: |p, n, e, h, t| {
                let cx = e.min_x + (e.max_x - e.min_x) / 2.0;
                if t < 2.4 {
                    let lift = (t / 0.3).min(1.0) * h * 1.3;
                    let x = cx + (std::f32::consts::TAU * t / 1.2).sin() * h * 1.2;
                    p.input(Input::PointerMove { x, y: e.ground_y - h / 2.0 - lift }, n, e);
                } else if t < 2.45 {
                    p.input(Input::PointerUp, n, e);
                }
            },
        },
        Script {
            // 往右甩出去：撞牆反彈、落地。
            name: "10-throw",
            secs: 2.6,
            start_x: 0.3,
            setup: grab,
            each: |p, n, e, h, t| {
                if t < 0.4 {
                    let x = e.min_x + h * 0.6 + t * h * 9.0;
                    p.input(Input::PointerMove { x, y: e.ground_y - h * 1.6 - t * h * 2.0 }, n, e);
                } else if t < 0.45 {
                    p.input(Input::PointerUp, n, e);
                }
            },
        },
        Script { name: "11-climb", secs: 9.0, start_x: 0.0, setup: |p, n, e, _| demo(Demo::ClimbLeft)(p, n, e), each: nothing_each },
        Script {
            name: "12-notify",
            secs: 3.4,
            start_x: 0.5,
            setup: |p, n, _, _| p.set_attention(true, n),
            each: nothing_each,
        },
        Script { name: "13-sleep", secs: 4.0, start_x: 0.5, setup: |p, n, e, _| demo(Demo::Sleep)(p, n, e), each: nothing_each },
        Script { name: "14-sit", secs: 3.0, start_x: 0.5, setup: |p, n, e, _| demo(Demo::Sit)(p, n, e), each: nothing_each },
        Script { name: "15-yawn", secs: 2.0, start_x: 0.5, setup: |p, n, e, _| demo(Demo::Yawn)(p, n, e), each: nothing_each },
        Script { name: "16-talk", secs: 3.0, start_x: 0.5, setup: |p, n, e, _| demo(Demo::Talk)(p, n, e), each: nothing_each },
        Script { name: "17-spin", secs: 1.2, start_x: 0.5, setup: |p, n, e, _| demo(Demo::Spin)(p, n, e), each: nothing_each },
        Script { name: "18-roll", secs: 1.4, start_x: 0.3, setup: |p, n, e, _| demo(Demo::Roll)(p, n, e), each: nothing_each },
        Script { name: "19-dizzy", secs: 3.6, start_x: 0.5, setup: |p, n, e, _| demo(Demo::Dizzy)(p, n, e), each: nothing_each },
        Script { name: WALL_SCRIPT, secs: 12.0, start_x: 0.1, setup: |p, n, e, _| demo(Demo::Walk { secs: 4.0 })(p, n, e), each: nothing_each },
    ];
    out.extend(leaps);
    out
}

/// 把一個視窗的內容疊到舞台上（預乘透明度的「疊在上面」），整張再乘上 `opacity`（跟分層視窗的整體透明度一樣）。
// 兩張點陣圖各自的像素與寬高、貼上的位置：每個參數都是獨立的量，包成結構只會讓呼叫端更長。
#[allow(clippy::too_many_arguments)]
fn blit(stage: &mut [u8], stage_w: usize, stage_h: usize, src: &[u8], src_w: usize, src_h: usize, ox: i32, oy: i32, opacity: f32) {
    for y in 0..src_h {
        let sy = oy + y as i32;
        if sy < 0 || sy >= stage_h as i32 {
            continue;
        }
        for x in 0..src_w {
            let sx = ox + x as i32;
            if sx < 0 || sx >= stage_w as i32 {
                continue;
            }
            let s = (y * src_w + x) * BYTES_PER_PIXEL;
            let scaled = |c: usize| (src[s + c] as f32 * opacity).round() as u32;
            let a = scaled(3);
            if a == 0 {
                continue;
            }
            let d = (sy as usize * stage_w + sx as usize) * BYTES_PER_PIXEL;
            for c in 0..BYTES_PER_PIXEL {
                stage[d + c] = (scaled(c) + stage[d + c] as u32 * (255 - a) / 255).min(255) as u8;
            }
        }
    }
}

/// `pet` 沒給就用預設的那一隻。
pub fn frames(out_dir: &Path, dpi: u32, pet: Option<&str>) -> Result<()> {
    let config = Config::default();
    let pets_dir = paths::pets_dir().ok_or_else(|| Error::new(E_FAIL, "找不到 pets 資料夾"))?;
    let pack = pack::load(&pets_dir.join(pet.unwrap_or(&config.pet))).map_err(|e| Error::new(E_FAIL, e.to_string()))?;
    let gfx = Gfx::new()?;
    let wic = Wic::new()?;
    let mut renderer = PetRenderer::new(&gfx)?;
    let px_per_dip = dpi as f32 / BASE_DPI;
    let sprites = wic.load_pet(&pack, &renderer.canvas.target(), config.size_dip as f32 * px_per_dip, dpi)?;
    let size = window_size(&sprites);
    renderer.canvas.ensure_size(size.0, size.1)?;
    let h = sprites.base_height;
    let half = sprites.states[states::IDLE].variants[0].frames[0].width / 2.0;
    let (stage_w, stage_h) = ((h * STAGE_W_HEIGHTS) as usize, (h * STAGE_H_HEIGHTS) as usize);
    let env = Env {
        ground_y: stage_h as f32 - 2.0,
        min_x: half,
        max_x: stage_w as f32 - half,
        px_per_dip,
        idle_input_secs: IDLE_INPUT_SECS,
        sleep_after_secs: f32::MAX,
        walk_speed_dip: None,
        cursor: None,
        pet_height: h,
        pet_half_width: half,
        platforms: Vec::new(),
        activity: Activity::Normal,
        blocks: Vec::new(),
        invisible_floor: false,
        invisible_walls: (false, false),
        invisible_ceiling: false,
        wall_left: 0.0,
        wall_right: stage_w as f32,
        ceiling: 0.0,
        item: None,
    };
    let frame_dt = 1.0 / FPS;
    let anchor = anchor(size);
    let mut glow = GlowPainter::new(&gfx)?;
    let glow_style = GlowStyle { pet_height: h, scale: px_per_dip, color: theme::current(petkit::config::ColorScheme::System).danger.d2d() };
    for script in scripts() {
        let dir = out_dir.join(script.name);
        fs::create_dir_all(&dir).map_err(|e| Error::new(E_FAIL, e.to_string()))?;
        let mut env = env.clone();
        if script.name == WALL_SCRIPT {
            let left = stage_w as f32 * WALL_LEFT_RATIO;
            env.blocks = vec![Rect { left, top: env.ground_y - h * WALL_H_HEIGHTS, right: left + h * WALL_W_HEIGHTS, bottom: env.ground_y }];
            env.platforms = behavior::block_platforms(&env.blocks, env.ceiling, h);
        }
        let x = env.min_x + (env.max_x - env.min_x) * script.start_x;
        let mut pet = Pet::new(x, env.ground_y, 0.0, 99);
        let mut windows = Vec::new();
        if let Some(&(_, seed, gap, rise)) = LEAP_SCRIPTS.iter().find(|s| s.0 == script.name) {
            windows = leap_windows(stage_w as f32, env.ground_y, h, half, gap, rise);
            env.platforms = windows.clone();
            pet = Pet::new(windows[0].right - h * LEAP_START_INSET_HEIGHTS, windows[0].y, 0.0, seed);
        }
        let mut effects = Effects::default();
        let mut now = 0.0f64;
        (script.setup)(&mut pet, now, &env, h);
        let mut last_state = "";
        let mut anim_frame = 0usize;
        let mut anim_elapsed = 0.0f32;
        let mut lit = None;
        for i in 0..(script.secs * FPS) as usize {
            for _ in 0..SUBSTEPS {
                let dt = frame_dt / SUBSTEPS as f32;
                (script.each)(&mut pet, now, &env, h, now as f32);
                now += dt as f64;
                pet.update(now, dt, &env);
                lit = glow.advance(&pet.glows(&env, now), dt);
                for e in pet.drain_emotes() {
                    effects.spawn(e, now);
                }
            }
            effects.prune(now);
            let pose = pet.pose(now);
            let res = pack.resolve(pose.state).or_else(|| pack.resolve(states::IDLE)).ok_or_else(|| Error::new(E_FAIL, "no idle frames"))?;
            let variant = &sprites.states[res.source].variants[0];
            if pose.state != last_state {
                last_state = pose.state;
                anim_frame = 0;
                anim_elapsed = 0.0;
            } else if variant.durations_ms.len() > 1 {
                anim_elapsed += frame_dt * 1000.0;
                while anim_elapsed >= variant.durations_ms[anim_frame] as f32 {
                    anim_elapsed -= variant.durations_ms[anim_frame] as f32;
                    anim_frame = (anim_frame + 1) % variant.durations_ms.len();
                }
            }
            let scene = Scene {
                frame: &variant.frames[anim_frame.min(variant.frames.len() - 1)],
                pose,
                procedural: !res.own || variant.procedural,
                base_height: h,
                appear: 1.0,
                effects: &effects,
                now,
                own_frames: res.own,
            };
            renderer.draw(anchor, &scene)?;
            let mut stage = vec![0u8; stage_w * stage_h * BYTES_PER_PIXEL];
            paint_windows(&mut stage, stage_w, stage_h, &windows);
            if let Some((g, opacity)) = &lit {
                let origin = glow.render(g, &glow_style)?;
                let (gw, gh) = glow.canvas.size();
                blit(&mut stage, stage_w, stage_h, glow.canvas.pixels(), gw as usize, gh as usize, origin.x, origin.y, *opacity);
            }
            let (ox, oy) = ((pet.x - anchor.X).round() as i32, (pet.y - anchor.Y).round() as i32);
            blit(&mut stage, stage_w, stage_h, renderer.canvas.pixels(), size.0 as usize, size.1 as usize, ox, oy, 1.0);
            wic.save_png(&dir.join(format!("{i:03}.png")), stage_w as u32, stage_h as u32, &stage)?;
        }
        println!("{} ({} frames)", script.name, (script.secs * FPS) as usize);
    }
    Ok(())
}
