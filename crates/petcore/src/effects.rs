//! 桌寵頭上的表情特效（愛心、汗滴、驚嘆號、灰塵、音符、問號、星星、生氣、頭暈、睡覺的 Z）：
//! 圖在 `assets/emotes/`（跟桌寵同一個平塗畫風），編進執行檔；動畫（彈出、飄、轉）用程式做。
//! 每一隻桌寵都有。特效數量有上限，舊的會被擠掉，不會越積越多。

use std::f32::consts::{PI, TAU};

use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::core::Result;
use windows_numerics::{Matrix3x2, Vector2};

use crate::behavior::Emote;
use crate::sprites::Wic;
use crate::theme;

/// 同時存在的特效上限。
const MAX_EFFECTS: usize = 16;
/// 圖解碼成多大（長邊，以桌寵身高計）：畫的時候只縮不放，才不會糊。
const ART_PER_HEIGHT: f32 = 0.3;
/// 睡覺的 Z：同時幾個、多久飄完一輪。
const ZZZ_COUNT: usize = 3;
const ZZZ_CYCLE_SECS: f32 = 2.4;

fn lifetime(kind: Emote) -> f32 {
    match kind {
        Emote::Hearts => 1.3,
        Emote::Sweat => 0.8,
        Emote::Alert => 1.2,
        Emote::Dust => 0.5,
        Emote::Note => 1.6,
        Emote::Question => 1.3,
        Emote::Star => 1.4,
        Emote::Anger => 1.2,
        Emote::Dizzy => 1.8,
    }
}

#[derive(Debug, Clone, Copy)]
enum Art {
    Heart,
    Sweat,
    Alert,
    Question,
    Note,
    Dust,
    Z,
    Star,
    Anger,
    Dizzy,
}

/// 跟 `Art` 同順序。
const ART_FILES: [&[u8]; 10] = [
    include_bytes!("../assets/emotes/heart.png"),
    include_bytes!("../assets/emotes/sweat.png"),
    include_bytes!("../assets/emotes/alert.png"),
    include_bytes!("../assets/emotes/question.png"),
    include_bytes!("../assets/emotes/note.png"),
    include_bytes!("../assets/emotes/dust.png"),
    include_bytes!("../assets/emotes/z.png"),
    include_bytes!("../assets/emotes/star.png"),
    include_bytes!("../assets/emotes/anger.png"),
    include_bytes!("../assets/emotes/dizzy.png"),
];

struct Effect {
    kind: Emote,
    born: f64,
    /// 每個特效一點點不一樣（飄的方向、晃的節奏）。
    seed: f32,
}

#[derive(Default)]
pub struct Effects {
    list: Vec<Effect>,
    counter: u32,
}

impl Effects {
    pub fn spawn(&mut self, kind: Emote, now: f64) {
        if self.list.len() >= MAX_EFFECTS {
            self.list.remove(0);
        }
        self.counter = self.counter.wrapping_add(1);
        let seed = (self.counter.wrapping_mul(2_654_435_761) >> 16) as f32 / 65_536.0;
        self.list.push(Effect { kind, born: now, seed });
    }

    pub fn prune(&mut self, now: f64) {
        self.list.retain(|e| ((now - e.born) as f32) < lifetime(e.kind));
    }

    pub fn active(&self) -> bool {
        !self.list.is_empty()
    }
}

/// 特效要貼在桌寵身上的哪裡（視窗內的實體像素）。
#[derive(Clone, Copy)]
pub struct Anchor {
    /// 頭頂中心。
    pub head: Vector2,
    /// 腳底中心。
    pub feet: Vector2,
    /// 站姿身高（實體像素），特效大小以它為單位。
    pub height: f32,
    /// 桌寵面向左。
    pub facing_left: bool,
}

struct Sprite {
    bitmap: ID2D1Bitmap,
    width: f32,
    height: f32,
}

pub struct EffectPainter {
    wic: Wic,
    /// (解碼時的長邊像素, 十張圖)：桌寵大小或縮放比例變了才重新解碼。
    sprites: Option<(u32, Vec<Sprite>)>,
}

impl EffectPainter {
    pub fn new() -> Result<Self> {
        Ok(Self { wic: Wic::new()?, sprites: None })
    }

    fn ensure_sprites(&mut self, rt: &ID2D1RenderTarget, height: f32) -> Result<()> {
        let side = (height * ART_PER_HEIGHT).round().max(1.0) as u32;
        if self.sprites.as_ref().is_some_and(|(s, _)| *s == side) {
            return Ok(());
        }
        let list = ART_FILES
            .iter()
            .map(|png| self.wic.load_embedded(png, rt, side as f32).map(|(bitmap, width, height)| Sprite { bitmap, width, height }))
            .collect::<Result<Vec<_>>>()?;
        self.sprites = Some((side, list));
        Ok(())
    }

    /// 以 `c` 為中心畫一張圖，長邊 `size`；`flip` 左右翻，`turn` 旋轉角度。
    // 每個特效的呼叫都是一行寫完位置、大小、透明度、翻轉、角度；包成結構只會讓十幾個呼叫端更長。
    #[allow(clippy::too_many_arguments)]
    fn put(&self, rt: &ID2D1RenderTarget, art: Art, c: Vector2, size: f32, alpha: f32, flip: bool, turn: f32) {
        let Some((_, list)) = &self.sprites else { return };
        if size <= 0.0 || alpha <= 0.0 {
            return;
        }
        let s = &list[art as usize];
        let k = size / s.width.max(s.height);
        let (w, h) = (s.width * k, s.height * k);
        let dest = D2D_RECT_F { left: c.X - w / 2.0, top: c.Y - h / 2.0, right: c.X + w / 2.0, bottom: c.Y + h / 2.0 };
        let mirror = Matrix3x2::scale_around(if flip { -1.0 } else { 1.0 }, 1.0, c);
        unsafe {
            let mut before = Matrix3x2::identity();
            rt.GetTransform(&mut before);
            rt.SetTransform(&(mirror * Matrix3x2::rotation_around(turn, c) * before));
            rt.DrawBitmap(&s.bitmap, Some(&dest), alpha.clamp(0.0, 1.0), D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
            rt.SetTransform(&before);
        }
    }

    pub fn draw(&mut self, rt: &ID2D1RenderTarget, effects: &Effects, now: f64, a: Anchor) -> Result<()> {
        if effects.list.is_empty() {
            return Ok(());
        }
        self.ensure_sprites(rt, a.height)?;
        // 「前面」= 桌寵面向的那一邊。
        let front = if a.facing_left { -1.0 } else { 1.0 };
        let h = a.height;
        let at = |x: f32, y: f32| Vector2 { X: x, Y: y };
        for e in &effects.list {
            let age = (now - e.born) as f32;
            let p = (age / lifetime(e.kind)).clamp(0.0, 1.0);
            let fade = 1.0 - ((p - 0.65) / 0.35).clamp(0.0, 1.0);
            let pop = theme::ease(theme::MOTION_SPRING, (p * 4.0).min(1.0));
            let sway = (p * 9.0 + e.seed * 6.0).sin();
            match e.kind {
                Emote::Hearts => {
                    for i in 0..3 {
                        let q = ((age - i as f32 * 0.1) / (lifetime(e.kind) - 0.3)).clamp(0.0, 1.0);
                        if q <= 0.0 {
                            continue;
                        }
                        let x = a.head.X + (i as f32 - 1.0) * 0.24 * h + (q * 7.0 + e.seed * 6.0).sin() * 0.04 * h;
                        let y = a.head.Y - 0.04 * h - q * 0.42 * h;
                        let size = 0.17 * h * theme::ease(theme::MOTION_SPRING, (q * 4.0).min(1.0)) * (1.0 - 0.15 * i as f32);
                        self.put(rt, Art::Heart, at(x, y), size, 1.0 - ((q - 0.6) / 0.4).clamp(0.0, 1.0), false, (i as f32 - 1.0) * 12.0);
                    }
                }
                Emote::Sweat => {
                    // 汗滴從頭的後面往外甩。
                    let side = -front;
                    for i in 0..2 {
                        let x = a.head.X + side * (0.3 + 0.1 * i as f32) * h + side * p * 0.14 * h;
                        let y = a.head.Y + (0.12 + 0.12 * i as f32) * h - (PI * p).sin() * 0.08 * h + p * 0.06 * h;
                        self.put(rt, Art::Sweat, at(x, y), 0.13 * h * pop, fade, false, side * 25.0);
                    }
                }
                Emote::Dust => {
                    // 落地時從腳邊往兩側噴出、邊飄高邊散開（不放在身體正下方，那裡會被視窗底邊切掉）。
                    // 圖的尾巴朝左下 = 往右上飄：右邊照原樣，左邊左右翻。
                    for side in [-1.0f32, 1.0] {
                        let x = a.feet.X + side * (0.4 + p * 0.22) * h;
                        let y = a.feet.Y - (0.1 + p * 0.1) * h;
                        self.put(rt, Art::Dust, at(x, y), 0.2 * h * (0.8 + 0.6 * p), 1.0 - p, side < 0.0, 0.0);
                    }
                }
                Emote::Alert => {
                    let c = at(a.head.X + front * 0.3 * h, a.head.Y - 0.1 * h);
                    self.put(rt, Art::Alert, c, 0.28 * h * pop, fade, false, front * 8.0 * (TAU * p * 2.0).sin() * (1.0 - p));
                }
                Emote::Question => {
                    let c = at(a.head.X + front * 0.32 * h + sway * 0.04 * h, a.head.Y - 0.12 * h);
                    self.put(rt, Art::Question, c, 0.26 * h * pop, fade, false, sway * 10.0);
                }
                Emote::Note => {
                    let c = at(a.head.X + front * 0.32 * h + sway * 0.05 * h, a.head.Y - 0.12 * h - p * 0.4 * h);
                    self.put(rt, Art::Note, c, 0.24 * h * pop, fade, false, sway * 15.0);
                }
                Emote::Star => {
                    // 頭的周圍三顆星星輪流一閃一閃。
                    for (i, (dx, dy)) in [(-0.38, -0.05), (0.0, -0.3), (0.38, -0.08)].into_iter().enumerate() {
                        let q = ((age - i as f32 * 0.15) / 0.8).clamp(0.0, 1.0);
                        let twinkle = (PI * q).sin();
                        self.put(rt, Art::Star, at(a.head.X + dx * h, a.head.Y + dy * h), 0.18 * h * twinkle, fade, false, q * 40.0);
                    }
                }
                Emote::Anger => {
                    // 太陽穴上的青筋，跟著心跳一鼓一鼓。
                    let beat = 1.0 + 0.18 * (TAU * p * 3.0).sin().abs();
                    let c = at(a.head.X + front * 0.26 * h, a.head.Y + 0.05 * h);
                    self.put(rt, Art::Anger, c, 0.2 * h * pop * beat, fade, false, 0.0);
                }
                Emote::Dizzy => {
                    let c = at(a.head.X, a.head.Y - 0.12 * h);
                    self.put(rt, Art::Dizzy, c, 0.3 * h * pop, fade, false, 720.0 * p);
                }
            }
        }
        Ok(())
    }

    /// 睡覺：頭的前上方一個接一個飄出 Z，越飄越大、越淡。
    pub fn draw_zzz(&mut self, rt: &ID2D1RenderTarget, t: f32, anchor: Vector2, frame_width: f32, frame_height: f32, base_height: f32) -> Result<()> {
        self.ensure_sprites(rt, base_height)?;
        for i in 0..ZZZ_COUNT {
            let phase = (t / ZZZ_CYCLE_SECS + i as f32 / ZZZ_COUNT as f32).fract();
            let c = Vector2 { X: anchor.X + frame_width * 0.3 + phase * base_height * 0.25, Y: anchor.Y - frame_height * 0.85 - phase * base_height * 0.35 };
            self.put(rt, Art::Z, c, base_height * (0.1 + 0.08 * phase), (PI * phase).sin(), false, -10.0 + 20.0 * phase);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effects_are_capped_and_expire() {
        let mut fx = Effects::default();
        for i in 0..100 {
            fx.spawn(Emote::Hearts, i as f64 * 0.01);
        }
        assert_eq!(fx.list.len(), MAX_EFFECTS);
        fx.prune(10.0);
        assert!(!fx.active());
    }
}
