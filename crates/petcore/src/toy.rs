//! 點心與球：從右鍵選單丟到桌面上的小東西。物理（掉落、彈跳、滾動）是純邏輯；
//! 圖在 `assets/toys/`（使用者出的平塗風插畫，跟桌寵同一個畫風），編進執行檔裡，每一隻桌寵都能用。
//! 一次只會有一個。

use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::core::Result;
use windows_numerics::{Matrix3x2, Vector2};

use crate::behavior::Platform;
use crate::render::{Canvas, Gfx, TRANSPARENT};
use crate::sprites::Wic;

const GRAVITY_DIP: f32 = 2600.0;
/// 落地反彈保留多少速度：球彈、點心幾乎不彈。
const BOUNCE_BALL: f32 = 0.55;
const BOUNCE_FOOD: f32 = 0.15;
const WALL_BOUNCE: f32 = 0.6;
/// 在地上滾的時候每秒減速多少（DIP/秒²）。
const ROLL_FRICTION_DIP: f32 = 420.0;
/// 比這個慢（DIP/秒）就停下來。
const REST_SPEED_DIP: f32 = 30.0;
/// 落在平台上的判定距離（像素）。
const SNAP_PX: f32 = 3.0;
/// 直徑是桌寵身高的幾倍。
pub const SIZE_PER_HEIGHT: f32 = 0.3;
/// 被踢一下：水平、往上的初速（DIP/秒）。
const KICK_X_DIP: f32 = 520.0;
const KICK_UP_DIP: f32 = 620.0;
/// 使用者點一下球：往上彈的初速（DIP/秒）、水平隨機的範圍。
const POKE_UP_DIP: f32 = 900.0;
const POKE_X_DIP: f32 = 260.0;
/// 視窗比東西大一圈，畫陰影與外框不會被切掉。
const WINDOW_PER_SIZE: f32 = 1.5;
/// 拿著甩、放手時的速度上限（DIP/秒），跟甩桌寵一樣。
const MAX_THROW_DIP: f32 = 2600.0;
/// 拿著移動時，速度往實際移動速度靠的快慢（每秒）：放手時用平滑過的速度，手一抖才不會亂飛。
const DRAG_VELOCITY_SMOOTHING: f32 = 12.0;

/// 點心與球的圖（數字是陣列裡的第幾張）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToyKind {
    Food(usize),
    Ball(usize),
}

const FOOD_ART: [&[u8]; 10] = [
    include_bytes!("../assets/toys/food_cake.png"),
    include_bytes!("../assets/toys/food_cookie.png"),
    include_bytes!("../assets/toys/food_cupcake.png"),
    include_bytes!("../assets/toys/food_dango.png"),
    include_bytes!("../assets/toys/food_donut.png"),
    include_bytes!("../assets/toys/food_lollipop.png"),
    include_bytes!("../assets/toys/food_macaron.png"),
    include_bytes!("../assets/toys/food_onigiri.png"),
    include_bytes!("../assets/toys/food_pudding.png"),
    include_bytes!("../assets/toys/food_taiyaki.png"),
];
const BALL_ART: [&[u8]; 10] = [
    include_bytes!("../assets/toys/ball_baseball.png"),
    include_bytes!("../assets/toys/ball_basketball.png"),
    include_bytes!("../assets/toys/ball_billiard_black.png"),
    include_bytes!("../assets/toys/ball_billiard_red.png"),
    include_bytes!("../assets/toys/ball_bowling.png"),
    include_bytes!("../assets/toys/ball_golf.png"),
    include_bytes!("../assets/toys/ball_soccer.png"),
    include_bytes!("../assets/toys/ball_teal.png"),
    include_bytes!("../assets/toys/ball_tennis.png"),
    include_bytes!("../assets/toys/ball_volleyball.png"),
];

impl ToyKind {
    /// `pick` 0～1：隨機挑一種點心／球。
    pub fn food(pick: f32) -> Self {
        ToyKind::Food((pick * FOOD_ART.len() as f32) as usize % FOOD_ART.len())
    }

    pub fn ball(pick: f32) -> Self {
        ToyKind::Ball((pick * BALL_ART.len() as f32) as usize % BALL_ART.len())
    }

    /// 每一種（截圖工具列出全部用）。
    pub fn all() -> impl Iterator<Item = ToyKind> {
        (0..FOOD_ART.len()).map(ToyKind::Food).chain((0..BALL_ART.len()).map(ToyKind::Ball))
    }

    pub fn is_food(self) -> bool {
        matches!(self, ToyKind::Food(_))
    }

    fn art(self) -> &'static [u8] {
        match self {
            ToyKind::Food(i) => FOOD_ART[i],
            ToyKind::Ball(i) => BALL_ART[i],
        }
    }
}

/// 物理要看的環境（螢幕實體像素）。
pub struct World<'a> {
    pub ground_y: f32,
    pub wall_left: f32,
    pub wall_right: f32,
    pub platforms: &'a [Platform],
    pub px_per_dip: f32,
}

#[derive(Debug, Clone)]
pub struct Toy {
    pub kind: ToyKind,
    /// 底部中心點。
    pub x: f32,
    pub y: f32,
    vx: f32,
    vy: f32,
    /// 直徑（實體像素）。
    pub size: f32,
    /// 滾動的角度（度）。
    pub spin_deg: f32,
    pub resting: bool,
    /// 桌寵踢了幾次（玩夠了就不追了）。
    pub kicks: u32,
    /// 被使用者拿著：抓住的點離底部中心多遠。
    held: Option<(f32, f32)>,
}

impl Toy {
    pub fn new(kind: ToyKind, x: f32, y: f32, vx: f32, vy: f32, size: f32) -> Self {
        Self { kind, x, y, vx, vy, size, spin_deg: 0.0, resting: false, kicks: 0, held: None }
    }

    /// 使用者在 `(cx, cy)` 抓起來。
    /// 被人拿起來：重丟一次就是再玩一輪，桌寵踢過幾次歸零（踢夠了牠就不理球）。
    pub fn grab(&mut self, cx: f32, cy: f32) {
        self.kicks = 0;
        self.held = Some((self.x - cx, self.y - cy));
        self.resting = false;
        self.vx = 0.0;
        self.vy = 0.0;
    }

    /// 拿著移到滑鼠 `(cx, cy)`，順便記下移動速度（放手時用）。
    pub fn drag_to(&mut self, cx: f32, cy: f32, dt: f32) {
        let Some((dx, dy)) = self.held else { return };
        let (x, y) = (cx + dx, cy + dy);
        let k = (dt * DRAG_VELOCITY_SMOOTHING).min(1.0);
        let dt = dt.max(1e-3);
        self.vx += ((x - self.x) / dt - self.vx) * k;
        self.vy += ((y - self.y) / dt - self.vy) * k;
        self.x = x;
        self.y = y;
    }

    /// 放手：帶著甩的速度飛出去。
    pub fn release(&mut self, px_per_dip: f32) {
        self.held = None;
        let max = MAX_THROW_DIP * px_per_dip;
        self.vx = self.vx.clamp(-max, max);
        self.vy = self.vy.clamp(-max, max);
    }

    pub fn is_held(&self) -> bool {
        self.held.is_some()
    }

    /// 推進一格；回傳 `true` = 這一格剛落地（呼叫端可以趁機出聲音或灰塵）。
    pub fn update(&mut self, dt: f32, w: &World) -> bool {
        if self.held.is_some() {
            return false;
        }
        let s = w.px_per_dip;
        let r = self.size / 2.0;
        if self.resting {
            // 腳下的視窗被拖走或關掉：掉下去。
            let supported = (self.y - w.ground_y).abs() < 0.5 || w.platforms.iter().any(|p| (p.y - self.y).abs() <= SNAP_PX && self.x >= p.left && self.x <= p.right);
            if supported {
                return false;
            }
            self.resting = false;
        }
        let prev_y = self.y;
        self.vy += GRAVITY_DIP * s * dt;
        self.x += self.vx * dt;
        self.y += self.vy * dt;
        if (self.x - r < w.wall_left && self.vx < 0.0) || (self.x + r > w.wall_right && self.vx > 0.0) {
            self.x = self.x.clamp(w.wall_left + r, w.wall_right - r);
            self.vx = -self.vx * WALL_BOUNCE;
        }
        let floor = (self.vy > 0.0).then(|| w.platforms.iter().filter(|p| p.y >= prev_y && p.y <= self.y && self.x >= p.left && self.x <= p.right).map(|p| p.y).min_by(f32::total_cmp)).flatten();
        let mut landed = false;
        if let Some(top) = floor.or((self.y >= w.ground_y).then_some(w.ground_y)) {
            self.y = top;
            let bounce = if self.kind.is_food() { BOUNCE_FOOD } else { BOUNCE_BALL };
            landed = self.vy > REST_SPEED_DIP * s * 4.0;
            self.vy = if self.vy * bounce > REST_SPEED_DIP * s * 3.0 { -self.vy * bounce } else { 0.0 };
            if self.vy == 0.0 {
                // 在地上滾：摩擦減速。
                let slow = ROLL_FRICTION_DIP * s * dt;
                self.vx = if self.vx.abs() <= slow { 0.0 } else { self.vx - slow * self.vx.signum() };
                if self.vx.abs() < REST_SPEED_DIP * s {
                    self.vx = 0.0;
                    self.resting = true;
                }
            }
        }
        // 球滾動時跟著轉（滾過的距離 ÷ 半徑）；點心只在空中翻一點點。
        let turn = if self.kind.is_food() { 0.25 } else { 1.0 };
        self.spin_deg = (self.spin_deg + (self.vx * dt / r).to_degrees() * turn) % 360.0;
        landed
    }

    /// 在地上或視窗頂（滾動中也算），不在空中、沒被拿著。
    pub fn grounded(&self) -> bool {
        self.held.is_none() && (self.resting || self.vy == 0.0)
    }

    /// 桌寵踢球：往 `dir` 飛出去。
    pub fn kick(&mut self, dir: f32, px_per_dip: f32) {
        self.kicks += 1;
        self.resting = false;
        self.vx = dir * KICK_X_DIP * px_per_dip;
        self.vy = -KICK_UP_DIP * px_per_dip;
    }

    /// 使用者點了一下：往上彈，水平方向 `jitter` -1～1。
    pub fn poke(&mut self, jitter: f32, px_per_dip: f32) {
        self.resting = false;
        self.vx += jitter * POKE_X_DIP * px_per_dip;
        self.vy = -POKE_UP_DIP * px_per_dip;
    }

    /// 視窗大小（正方形）與左上角位置。
    pub fn window(&self) -> (i32, POINT) {
        let side = (self.size * WINDOW_PER_SIZE).ceil() as i32;
        let pad = (side as f32 - self.size) / 2.0;
        (side, POINT { x: (self.x - side as f32 / 2.0).round() as i32, y: (self.y + pad - side as f32).round() as i32 })
    }
}

/// 一個分層視窗＋畫點心／球。
pub struct ToyPainter {
    pub canvas: Canvas,
    wic: Wic,
    brush: ID2D1SolidColorBrush,
    /// 目前這一個的圖（縮成顯示大小）：(哪一種, 直徑, 點陣圖, 寬, 高)。換一個或換大小才重新解碼，只留一張。
    art: Option<(ToyKind, u32, ID2D1Bitmap, f32, f32)>,
}

const SHADOW_ALPHA: f32 = 0.18;

impl ToyPainter {
    pub fn new(gfx: &Gfx) -> Result<Self> {
        let canvas = Canvas::new(gfx)?;
        let brush = unsafe { canvas.rt.CreateSolidColorBrush(&TRANSPARENT, None)? };
        Ok(Self { canvas, wic: Wic::new()?, brush, art: None })
    }

    pub fn draw(&mut self, toy: &Toy) -> Result<()> {
        let (side, _) = toy.window();
        self.canvas.ensure_size(side, side)?;
        let size_key = toy.size.round() as u32;
        if !self.art.as_ref().is_some_and(|(k, s, ..)| *k == toy.kind && *s == size_key) {
            let (bitmap, w, h) = self.wic.load_embedded(toy.kind.art(), &self.canvas.target(), toy.size)?;
            self.art = Some((toy.kind, size_key, bitmap, w, h));
        }
        let (_, _, bitmap, w, h) = self.art.as_ref().expect("just loaded");
        let rt = &self.canvas.rt;
        let b = &self.brush;
        let r = toy.size / 2.0;
        let pad = (side as f32 - toy.size) / 2.0;
        let c = Vector2 { X: side as f32 / 2.0, Y: side as f32 - pad - r };
        // 底部貼地、左右置中；扁的（鯛魚燒、馬卡龍）也是坐在地上，不會浮在半空中。
        let dest = D2D_RECT_F { left: c.X - w / 2.0, top: c.Y + r - h, right: c.X + w / 2.0, bottom: c.Y + r };
        unsafe {
            rt.BeginDraw();
            rt.Clear(Some(&TRANSPARENT));
            if toy.resting {
                b.SetColor(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: SHADOW_ALPHA });
                rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: c.X, Y: c.Y + r * 0.95 }, radiusX: w * 0.4, radiusY: r * 0.16 }, b);
            }
            let pivot = Vector2 { X: c.X, Y: c.Y + r - h / 2.0 };
            rt.SetTransform(&Matrix3x2::rotation_around(toy.spin_deg, pivot));
            rt.DrawBitmap(bitmap, Some(&dest), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
            rt.SetTransform(&Matrix3x2::identity());
            rt.EndDraw(None, None)
        }
    }

    pub fn present(&self, hwnd: HWND, origin: POINT, opacity: f32) -> Result<()> {
        self.canvas.present(hwnd, origin, opacity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn world(platforms: &[Platform]) -> World<'_> {
        World { ground_y: 1000.0, wall_left: 0.0, wall_right: 1920.0, platforms, px_per_dip: 1.0 }
    }

    fn settle(t: &mut Toy, w: &World, secs: f32) {
        for _ in 0..(secs / DT) as usize {
            t.update(DT, w);
        }
    }

    #[test]
    fn food_drops_and_stays_put() {
        let w = world(&[]);
        let mut t = Toy::new(ToyKind::Food(0), 500.0, 300.0, 0.0, 0.0, 40.0);
        settle(&mut t, &w, 3.0);
        assert!(t.resting);
        assert_eq!((t.x, t.y), (500.0, 1000.0));
    }

    #[test]
    fn kicked_ball_bounces_rolls_and_stops_inside_the_screen() {
        let w = world(&[]);
        let mut t = Toy::new(ToyKind::Ball(0), 1800.0, 1000.0, 0.0, 0.0, 40.0);
        t.kick(1.0, 1.0);
        let mut bounced = 0;
        for _ in 0..(10.0 / DT) as usize {
            bounced += t.update(DT, &w) as u32;
        }
        assert!(bounced >= 2, "a ball bounces, got {bounced}");
        assert!(t.resting, "comes to rest");
        assert!(t.x + t.size / 2.0 <= 1920.0, "bounced off the right edge, x = {}", t.x);
        assert_eq!(t.kicks, 1);
    }

    #[test]
    fn grabbed_and_flung_it_flies_with_momentum_then_lands() {
        let w = world(&[]);
        let mut t = Toy::new(ToyKind::ball(0.0), 500.0, 1000.0, 0.0, 0.0, 40.0);
        settle(&mut t, &w, 1.0);
        t.kicks = 5;
        t.grab(500.0, 980.0);
        assert!(!t.grounded(), "held in the hand, not on the floor");
        assert_eq!(t.kicks, 0, "a fresh throw is a new round of play");
        for i in 1..=10 {
            t.drag_to(500.0 + 25.0 * i as f32, 980.0 - 30.0 * i as f32, DT);
            assert!(!t.update(DT, &w), "no physics while held");
        }
        assert_eq!((t.x, t.y), (750.0, 700.0), "follows the hand");
        t.release(1.0);
        settle(&mut t, &w, 0.3);
        assert!(t.x > 950.0 && t.y < 700.0, "flung up and to the right, at ({}, {})", t.x, t.y);
        settle(&mut t, &w, 5.0);
        assert_eq!(t.y, 1000.0, "fell back to the floor");
    }

    #[test]
    fn lands_on_a_window_and_falls_when_it_goes_away() {
        let platforms = [Platform { left: 400.0, right: 700.0, y: 600.0, window: 1, window_left: 400.0, sides: (None, None), bottom: 1000.0 }];
        let mut t = Toy::new(ToyKind::Food(1), 500.0, 100.0, 0.0, 0.0, 40.0);
        settle(&mut t, &world(&platforms), 3.0);
        assert_eq!(t.y, 600.0);
        settle(&mut t, &world(&[]), 3.0);
        assert_eq!(t.y, 1000.0);
    }
}
