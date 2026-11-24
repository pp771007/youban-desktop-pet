//! 不開視窗的截圖工具（檢查畫面用，不需要把東西放到使用者桌面上、也不用擷取螢幕）：
//! - `Youban.exe --snapshot <out.png> [dpi]`：每個狀態各畫一格桌寵，排成一張
//! - `Youban.exe --snapshot-cards <out.png> [dpi]`：每種卡片的淺色與深色版本，排成一張（最後附上點心與球）

use std::path::Path;

use petkit::config::Config;
use petkit::pack;
use petkit::states;
use windows::Win32::Foundation::E_FAIL;
use windows::core::{Error, Result};

use crate::app::{anchor, window_size};
use crate::behavior::{Emote, Env, Input, Pet, Pose};
use crate::bubble::{Card, CardPainter};
use crate::effects::Effects;
use crate::icons::Icon;
use crate::paths;
use crate::popup_menu::{Entry, Item, MenuPainter};
use crate::render::{Gfx, PetRenderer, Scene};
use crate::sprites::Wic;
use crate::theme;
use crate::toy::{self, Toy, ToyKind, ToyPainter};

const BASE_DPI: f32 = 96.0;
const DT: f32 = 1.0 / 60.0;
/// 模擬「使用者離開一陣子」，打瞌睡才會出現（但還沒到睡著）。
const DOZE_IDLE_SECS: f32 = 90.0;
/// 模擬時最多推進幾秒去等某個狀態出現（散步是隨機的）。
const MAX_WAIT_SECS: f32 = 600.0;
const BYTES_PER_PIXEL: usize = 4;

struct Sim {
    pet: Pet,
    now: f64,
    env: Env,
}

impl Sim {
    fn step(&mut self, secs: f32) {
        for _ in 0..(secs / DT).round() as usize {
            self.now += DT as f64;
            self.pet.update(self.now, DT, &self.env);
        }
    }

    fn until(&mut self, state: &str) {
        let mut waited = 0.0;
        while self.pet.state() != state && waited < MAX_WAIT_SECS {
            self.step(DT);
            waited += DT;
        }
    }

    fn input(&mut self, input: Input) {
        self.pet.input(input, self.now, &self.env);
    }

    fn pose(&self) -> Pose {
        self.pet.pose(self.now)
    }
}

/// 把幾張同高或不同高的圖橫向排成一張（不足的高度留透明）。
struct Sheet {
    tiles: Vec<(usize, usize, Vec<u8>)>,
}

impl Sheet {
    fn push(&mut self, w: i32, h: i32, px: &[u8]) {
        self.tiles.push((w as usize, h as usize, px.to_vec()));
    }

    fn save(&self, wic: &Wic, out: &Path) -> Result<()> {
        let width: usize = self.tiles.iter().map(|t| t.0).sum();
        let height = self.tiles.iter().map(|t| t.1).max().unwrap_or(1);
        let row = width * BYTES_PER_PIXEL;
        let mut sheet = vec![0u8; row * height];
        let mut x0 = 0;
        for (w, h, px) in &self.tiles {
            let tile_row = w * BYTES_PER_PIXEL;
            for y in 0..*h {
                let dst = y * row + x0 * BYTES_PER_PIXEL;
                sheet[dst..dst + tile_row].copy_from_slice(&px[y * tile_row..(y + 1) * tile_row]);
            }
            x0 += w;
        }
        wic.save_png(out, width as u32, height as u32, &sheet)
    }
}

pub fn pet(out: &Path, dpi: u32) -> Result<()> {
    let config = Config::default();
    let pets_dir = paths::pets_dir().ok_or_else(|| Error::new(E_FAIL, "找不到 pets 資料夾"))?;
    let pack =
        pack::load(&pets_dir.join(&config.pet)).map_err(|e| Error::new(E_FAIL, e.to_string()))?;
    let gfx = Gfx::new()?;
    let wic = Wic::new()?;
    let mut renderer = PetRenderer::new(&gfx)?;
    let px_per_dip = dpi as f32 / BASE_DPI;
    let sprites = wic.load_pet(
        &pack,
        &renderer.canvas.target(),
        config.size_dip as f32 * px_per_dip,
        dpi,
    )?;
    let size = window_size(&sprites);
    renderer.canvas.ensure_size(size.0, size.1)?;

    let ground = 2000.0;
    let env = Env {
        ground_y: ground,
        min_x: 0.0,
        max_x: 10_000.0,
        px_per_dip,
        idle_input_secs: DOZE_IDLE_SECS,
        sleep_after_secs: config.sleep_after_secs as f32,
        walk_speed_dip: None,
        cursor: None,
        pet_height: sprites.base_height,
        pet_half_width: sprites.states[petkit::states::IDLE].variants[0].frames[0].width / 2.0,
        platforms: Vec::new(),
        activity: petkit::config::Activity::Normal,
        blocks: Vec::new(),
        invisible_floor: false,
        invisible_walls: (false, false),
        invisible_ceiling: false,
        wall_left: 0.0,
        wall_right: 10_000.0,
        ceiling: 0.0,
        item: None,
    };
    let mut sim = Sim {
        pet: Pet::new(5000.0, ground, 0.0, 12345),
        now: 0.0,
        env,
    };
    // (姿勢, 要一起畫的特效與它已經播了幾秒)
    let mut shots: Vec<(Pose, Option<(Emote, f32)>)> = Vec::new();

    sim.step(0.75);
    shots.push((sim.pose(), None));
    sim.until(states::WALK);
    sim.step(0.12);
    shots.push((sim.pose(), None));
    for (state, at, emote) in [
        (states::HOP, 0.22, Some((Emote::Note, 0.5))),
        (states::LOOK, 0.9, Some((Emote::Question, 0.4))),
        (states::WAKE, 0.6, None),
        (states::DOZE, 0.6, None),
    ] {
        sim.until(state);
        sim.step(at);
        shots.push((sim.pose(), emote));
    }
    sim.until(states::IDLE);
    sim.input(Input::PointerDown {
        x: 5000.0,
        y: ground - 50.0,
    });
    sim.input(Input::PointerUp);
    sim.step(0.3);
    shots.push((sim.pose(), Some((Emote::Hearts, 0.45))));
    sim.until(states::IDLE);
    sim.input(Input::PointerDown {
        x: sim.pet.x,
        y: ground - 50.0,
    });
    for i in 0..15 {
        let x = 5000.0 + i as f32 * 25.0 * px_per_dip;
        sim.input(Input::PointerMove {
            x,
            y: ground - 400.0,
        });
        sim.step(DT);
    }
    shots.push((sim.pose(), Some((Emote::Sweat, 0.3))));
    sim.input(Input::PointerUp);
    sim.until(states::LAND);
    sim.step(0.1);
    shots.push((sim.pose(), Some((Emote::Dust, 0.15))));
    sim.until(states::IDLE);
    sim.pet.set_attention(true, sim.now);
    sim.step(0.2);
    shots.push((sim.pose(), Some((Emote::Alert, 0.35))));
    sim.pet.set_attention(false, sim.now);
    sim.until(states::IDLE);
    for (emote, age) in [(Emote::Star, 0.5), (Emote::Anger, 0.3), (Emote::Dizzy, 0.5)] {
        shots.push((sim.pose(), Some((emote, age))));
    }
    // 爬牆（左牆、右牆）：直接造姿勢，不用等它隨機走到螢幕邊。
    for side in [-1.0, 1.0] {
        let mut climb = sim.pose();
        climb.state = states::CLIMB;
        climb.climb = Some(side);
        climb.flip = false;
        shots.push((climb, None));
    }
    sim.env.idle_input_secs = sim.env.sleep_after_secs + 1.0;
    sim.step(1.0);
    shots.push((sim.pose(), None));

    let mut sheet = Sheet { tiles: Vec::new() };
    for (pose, emote) in &shots {
        // 過場動作沒有圖時直接用站姿（跟主程式一樣）。
        let res = pack
            .resolve(pose.state)
            .or_else(|| pack.resolve(states::IDLE))
            .ok_or_else(|| Error::new(E_FAIL, "no idle frames"))?;
        let variant = &sprites.states[res.source].variants[0];
        let mut effects = Effects::default();
        let now = 100.0;
        if let Some((kind, age)) = emote {
            effects.spawn(*kind, now - *age as f64);
        }
        let scene = Scene {
            frame: &variant.frames[0],
            pose: *pose,
            procedural: !res.own || variant.procedural,
            base_height: sprites.base_height,
            appear: 1.0,
            effects: &effects,
            now,
            own_frames: res.own,
        };
        renderer.draw(anchor(size), &scene)?;
        sheet.push(size.0, size.1, renderer.canvas.pixels());
        println!("{}", pose.state);
    }
    sheet.save(&wic, out)
}

pub fn cards(out: &Path, dpi: u32) -> Result<()> {
    let gfx = Gfx::new()?;
    let wic = Wic::new()?;
    let mut painter = CardPainter::new(&gfx)?;
    let s = dpi as f32 / BASE_DPI;
    let samples = [
        (Card::Confirm { title: "喝水時間".into(), time: "14:00".into(), snooze_open: false, icon: Icon::BellRing }, Some(0)),
        (Card::Notice { title: "看看遠方，讓眼睛休息".into(), time: "14:30".into(), icon: Icon::BellRing }, None),
        (Card::Notice { title: "已經連續用電腦 50 分鐘了，起來走走吧".into(), time: "15:20".into(), icon: Icon::Cup }, None),
        (Card::Confirm { title: "再 3 天就是媽媽生日囉～".into(), time: "5/21（四） · 農曆四月初五 · 滿 60 年".into(), snooze_open: false, icon: Icon::Cake }, None),
        (Card::Confirm { title: "繳信用卡費：這個月記得用網銀轉帳，順便確認自動扣款有沒有設定成功".into(), time: "10:30".into(), snooze_open: true, icon: Icon::BellRing }, None),
        (Card::Confirm { title: "PR 有 2 則新留言，要你看一下".into(), time: "Claude・柚伴 · 14:02".into(), snooze_open: false, icon: Icon::MessageDots }, None),
        (Card::Notice { title: "測試全部通過了".into(), time: "Codex・網站 · 14:05".into(), icon: Icon::MessageDots }, None),
        (
            Card::Missed {
                rows: vec![("喝水".into(), "14:00（3 次）".into()), ("站起來走走".into(), "15:30".into()), ("週報".into(), "11/20 17:00".into())],
                more: 2,
            },
            None,
        ),
        (Card::success("已把 12 個視窗放回原位", Some("2 個需要以系統管理員身分執行柚伴才能移動".into())), None),
        (Card::warning("這隻桌寵讀不進來", Some("缺少站姿圖：frames/ 裡至少要有一張 idle_01.png".into())), None),
        (Card::Update { version: "0.2.0".into() }, None),
        (Card::Speech { text: "嗯？".into() }, None),
        (Card::Speech { text: "已經連續用電腦 50 分鐘了，起來走走吧".into() }, None),
        (Card::Speech { text: "早安！今天也一起加油吧。昨天睡得好嗎？我做了一個很長很長的夢，夢到一大片橘子園，還有溫泉可以泡，真想再睡一次".into() }, None),
    ];
    let mut sheet = Sheet { tiles: Vec::new() };
    for palette in [&theme::LIGHT, &theme::DARK] {
        for (card, hover) in &samples {
            let (w, h) = painter.render(card, palette, s, *hover)?;
            sheet.push(w, h, painter.canvas.pixels());
        }
    }
    let mut toys = ToyPainter::new(&gfx)?;
    let size = Config::default().size_dip as f32 * s * toy::SIZE_PER_HEIGHT;
    let kinds = ToyKind::all()
        .map(|k| (k, 0.0, true))
        .chain([(ToyKind::Ball(0), 50.0, false)]);
    for (kind, spin, resting) in kinds {
        let mut t = Toy::new(kind, 0.0, 0.0, 0.0, 0.0, size);
        t.spin_deg = spin;
        t.resting = resting;
        toys.draw(&t)?;
        let (w, h) = toys.canvas.size();
        sheet.push(w, h, toys.canvas.pixels());
    }
    // 第二層用 repo 裡真的桌寵（小圖讀圖檔）。
    let mut pets: Vec<_> = paths::pets_dir()
        .and_then(|d| std::fs::read_dir(d).ok())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = pack::load(&e.path()).ok()?;
            let picture = p
                .states
                .get(states::IDLE)?
                .variants
                .first()?
                .frames
                .first()?
                .clone();
            Some((p.name, picture))
        })
        .collect();
    pets.sort();
    let children: Vec<Entry> = pets
        .into_iter()
        .enumerate()
        .map(|(i, (name, picture))| {
            Entry::Item(Item {
                id: 10 + i as u32,
                label: name,
                icon: None,
                picture: Some(picture),
                checked: i == 1,
                children: Vec::new(),
            })
        })
        .collect();
    let switch = Item {
        id: 2,
        label: "換一隻桌寵".into(),
        icon: Some(Icon::PawPrint),
        picture: None,
        checked: false,
        children: children.clone(),
    };
    let main = vec![
        Entry::Header("桌寵".into()),
        Item::entry(1, "陪你第 12 天", Icon::Heart),
        Entry::Item(switch.clone()),
        Item::entry(3, "餵點心", Icon::Cookie),
        Item::entry(4, "丟球", Icon::Ball),
        Entry::Header("提醒".into()),
        Item::entry(5, "新增提醒…", Icon::Bell),
        Entry::Header("視窗歸位".into()),
        Item::entry(6, "記住目前位置", Icon::Save),
        Item::entry(7, "恢復位置", Icon::RotateCcw),
        Entry::Separator,
        Item::entry(8, "設定…", Icon::Settings),
        Item::entry(9, "結束", Icon::Power),
    ];
    let mut chip =
        crate::countdown::Countdown::new(&gfx, windows::Win32::Foundation::HWND::default())?;
    for palette in [&theme::LIGHT, &theme::DARK] {
        for (text, focusing) in [("24:59", true), ("03:07", false)] {
            let shown = crate::countdown::Shown {
                text: text.into(),
                focusing,
                head: windows_numerics::Vector2 { X: 0.0, Y: 0.0 },
            };
            let (w, h) = chip.render(&shown, s, palette)?;
            sheet.push(w, h, chip.pixels());
        }
    }
    let mut menus = MenuPainter::new(&gfx)?;
    menus.load_pictures(&wic, &children, s);
    for palette in [&theme::LIGHT, &theme::DARK] {
        for (entries, hover) in [(main.clone(), Some(2)), (children.clone(), Some(3))] {
            let (w, h) = menus.render(entries, palette, s, hover)?;
            sheet.push(w, h, menus.canvas.pixels());
        }
    }
    sheet.save(&wic, out)
}
