//! 看不見的牆被碰到時亮起來的那一小段紅光：只在桌寵身旁、淡淡的，滑鼠點得穿過去，不擋使用者。

use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos, ShowWindow};
use windows::core::Result;
use windows_numerics::Vector2;

use crate::behavior::Glow;
use crate::render::{Canvas, Gfx, TRANSPARENT};

/// 亮的那一段有多長（桌寵身高的倍數）、光暈多厚（DIP）。
const LENGTH_HEIGHTS: f32 = 2.2;
const THICKNESS_DIP: f32 = 22.0;
/// 淡入、淡出要多久。
const FADE_SECS: f32 = 0.25;
/// 疊三層線：(寬度佔光暈厚度的比例, 不透明度)。外層寬而淡、中間那條細而亮，看起來像牆面微微發光。
const LAYERS: [(f32, f32); 3] = [(1.0, 0.10), (0.5, 0.20), (0.14, 0.55)];

/// 光要畫多大、什麼顏色。
pub struct GlowStyle {
    /// 桌寵站姿身高（實體像素）：亮的那一段跟著牠的大小。
    pub pet_height: f32,
    /// 螢幕縮放（實體像素 / DIP）。
    pub scale: f32,
    pub color: D2D1_COLOR_F,
}

pub struct GlowPainter {
    pub canvas: Canvas,
    /// 最後一次亮的位置：淡出時留在原地慢慢消失。
    last: Option<Glow>,
    opacity: f32,
    shown: bool,
}

impl GlowPainter {
    pub fn new(gfx: &Gfx) -> Result<Self> {
        Ok(Self { canvas: Canvas::new(gfx)?, last: None, opacity: 0.0, shown: false })
    }

    /// 每一格呼叫：`glow` 是這一格桌寵碰著的那段牆（沒有就淡出）。`below` 是桌寵的視窗，光要墊在牠下面。
    pub fn update(&mut self, hwnd: HWND, below: HWND, glow: Option<Glow>, dt: f32, style: &GlowStyle) -> Result<()> {
        let Some((g, opacity)) = self.advance(glow, dt) else {
            self.hide(hwnd);
            return Ok(());
        };
        let origin = self.render(g, style)?;
        self.canvas.present(hwnd, origin, opacity)?;
        if !self.shown {
            self.shown = true;
            unsafe {
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                let _ = SetWindowPos(hwnd, Some(below), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            }
        }
        Ok(())
    }

    /// 推進淡入淡出；回傳這一格要畫的位置與整體透明度（完全淡掉了就是 `None`）。
    pub fn advance(&mut self, glow: Option<Glow>, dt: f32) -> Option<(Glow, f32)> {
        if glow.is_some() {
            self.last = glow;
        }
        let step = dt / FADE_SECS;
        self.opacity = if glow.is_some() { (self.opacity + step).min(1.0) } else { (self.opacity - step).max(0.0) };
        self.last.filter(|_| self.opacity > 0.0).map(|g| (g, self.opacity))
    }

    /// 畫進 `canvas`，回傳它在螢幕上的左上角。
    pub fn render(&mut self, g: Glow, style: &GlowStyle) -> Result<POINT> {
        let (len, thick, color) = (LENGTH_HEIGHTS * style.pet_height, THICKNESS_DIP * style.scale, style.color);
        let (w, h, left, top, start, end) = match g {
            Glow::Floor { x, y } => (len, thick, x - len / 2.0, y - thick / 2.0, Vector2 { X: 0.0, Y: thick / 2.0 }, Vector2 { X: len, Y: thick / 2.0 }),
            Glow::Wall { x, y } => (thick, len, x - thick / 2.0, y - len / 2.0, Vector2 { X: thick / 2.0, Y: 0.0 }, Vector2 { X: thick / 2.0, Y: len }),
        };
        self.canvas.ensure_size(w.ceil() as i32, h.ceil() as i32)?;
        let rt = &self.canvas.rt;
        unsafe {
            // 中間最亮、往兩頭淡掉：看起來是牆的一小段亮起來，不是一條畫上去的線。
            let clear = D2D1_COLOR_F { a: 0.0, ..color };
            let stops = [D2D1_GRADIENT_STOP { position: 0.0, color: clear }, D2D1_GRADIENT_STOP { position: 0.5, color }, D2D1_GRADIENT_STOP { position: 1.0, color: clear }];
            let collection = rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
            let brush = rt.CreateLinearGradientBrush(&D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: start, endPoint: end }, None, &collection)?;
            rt.BeginDraw();
            rt.Clear(Some(&TRANSPARENT));
            for (width, alpha) in LAYERS {
                brush.SetOpacity(alpha);
                rt.DrawLine(start, end, &brush, thick * width, None);
            }
            rt.EndDraw(None, None)?;
        }
        Ok(POINT { x: left.round() as i32, y: top.round() as i32 })
    }

    pub fn hide(&mut self, hwnd: HWND) {
        self.opacity = 0.0;
        self.last = None;
        if std::mem::take(&mut self.shown) {
            unsafe {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
        }
    }
}
