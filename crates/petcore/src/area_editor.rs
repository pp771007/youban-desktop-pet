//! 在螢幕上畫活動範圍／禁止範圍：整個桌面微微變暗，拖曳框出一個框；Esc 或按右鍵取消。
//! 已經有的範圍也畫出來，畫的人看得到哪裡已經框過了。

use petkit::config::Area;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint};
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{HSTRING, PCWSTR, Result, w};
use windows_numerics::Vector2;

use crate::render::{Canvas, Gfx};
use crate::theme;

/// 整個桌面蓋一層多深的暗色（完全透明的地方點不到，所以一定要有一點）。
const DIM_ALPHA: f32 = 0.28;
/// 範圍裡面的底色、邊框粗細（DIP）。
const FILL_ALPHA: f32 = 0.22;
const BORDER_DIP: f32 = 2.0;
const HINT: &str = "拖曳框出範圍　Esc 取消";
/// 提示放在螢幕上緣往下多少（DIP），字的大小、膠囊的左右留白。
const HINT_TOP_DIP: f32 = 48.0;
const HINT_PAD_DIP: f32 = 18.0;
const HINT_LINE_HEIGHTS: f32 = 2.2;
const HINT_BG_ALPHA: f32 = 0.75;
const LOCALE: &str = "zh-tw";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AreaKind {
    Allowed,
    Forbidden,
}

/// 畫範圍畫面上的滑鼠（視窗內座標，左上角是虛擬桌面的左上角）與取消。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AreaInput {
    Down(i32, i32),
    Move(i32, i32),
    Up(i32, i32),
    Cancel,
}

pub enum Outcome {
    Done(Area),
    Cancelled,
}

pub struct AreaEditor {
    pub kind: AreaKind,
    hwnd: HWND,
    canvas: Canvas,
    brush: ID2D1SolidColorBrush,
    hint: IDWriteTextLayout,
    /// 虛擬桌面（所有螢幕合起來）的左上角。
    origin: POINT,
    existing: Vec<Area>,
    color: D2D1_COLOR_F,
    /// 提示放在哪個螢幕的上緣中間（開的時候滑鼠在的那個螢幕）。
    hint_at: Vector2,
    scale: f32,
    /// 太小的框不算（多半是手滑點了一下）。
    min: (i32, i32),
    start: Option<POINT>,
    current: POINT,
}

impl AreaEditor {
    pub fn open(gfx: &Gfx, class: PCWSTR, kind: AreaKind, existing: Vec<Area>, min: (i32, i32), scale: f32, color: D2D1_COLOR_F) -> Result<Self> {
        let origin = unsafe { POINT { x: GetSystemMetrics(SM_XVIRTUALSCREEN), y: GetSystemMetrics(SM_YVIRTUALSCREEN) } };
        let size = unsafe { (GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN)) };
        let instance = unsafe { windows::Win32::System::LibraryLoader::GetModuleHandleW(None)? };
        let hwnd = unsafe { CreateWindowExW(WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW, class, w!("柚伴範圍"), WS_POPUP, origin.x, origin.y, size.0, size.1, None, None, Some(instance.into()), None)? };
        let mut canvas = Canvas::new(gfx)?;
        canvas.ensure_size(size.0, size.1)?;
        let brush = unsafe { canvas.rt.CreateSolidColorBrush(&color, None)? };
        let hint = unsafe {
            let format = gfx.dwrite.CreateTextFormat(&HSTRING::from(theme::FONT_FAMILY), None, DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, theme::FONT_SIZE_SUBTITLE * scale, &HSTRING::from(LOCALE))?;
            let wide: Vec<u16> = HINT.encode_utf16().collect();
            gfx.dwrite.CreateTextLayout(&wide, &format, f32::MAX, f32::MAX)?
        };
        let mut cursor = POINT::default();
        let hint_at = unsafe {
            let _ = GetCursorPos(&mut cursor);
            let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
            let _ = GetMonitorInfoW(MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST), &mut info);
            let m = info.rcMonitor;
            Vector2 { X: ((m.left + m.right) / 2 - origin.x) as f32, Y: (m.top - origin.y) as f32 + HINT_TOP_DIP * scale }
        };
        let mut editor = Self { kind, hwnd, canvas, brush, hint, origin, existing, color, hint_at, scale, min, start: None, current: cursor };
        editor.draw()?;
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
        }
        Ok(editor)
    }

    pub fn input(&mut self, input: AreaInput) -> Result<Option<Outcome>> {
        let (AreaInput::Down(x, y) | AreaInput::Move(x, y) | AreaInput::Up(x, y)) = input else {
            return Ok(Some(Outcome::Cancelled));
        };
        let p = POINT { x: x + self.origin.x, y: y + self.origin.y };
        self.current = p;
        match input {
            AreaInput::Down(..) => {
                self.start = Some(p);
                unsafe {
                    SetCapture(self.hwnd);
                }
            }
            AreaInput::Move(..) | AreaInput::Cancel => {}
            AreaInput::Up(..) => {
                unsafe {
                    let _ = ReleaseCapture();
                }
                if let Some(start) = self.start.take() {
                    let area = Area::from_corners((start.x, start.y), (p.x, p.y));
                    if area.width() >= self.min.0 && area.height() >= self.min.1 {
                        return Ok(Some(Outcome::Done(area)));
                    }
                }
            }
        }
        self.draw()?;
        Ok(None)
    }

    fn draw(&mut self) -> Result<()> {
        let to_local = |a: &Area| D2D_RECT_F {
            left: (a.left - self.origin.x) as f32,
            top: (a.top - self.origin.y) as f32,
            right: (a.right - self.origin.x) as f32,
            bottom: (a.bottom - self.origin.y) as f32,
        };
        let drawing = self.start.map(|s| Area::from_corners((s.x, s.y), (self.current.x, self.current.y)));
        let border = BORDER_DIP * self.scale;
        let rt = &self.canvas.rt;
        let b = &self.brush;
        unsafe {
            rt.BeginDraw();
            rt.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: DIM_ALPHA }));
            for a in self.existing.iter().chain(drawing.iter()) {
                let r = to_local(a);
                b.SetColor(&D2D1_COLOR_F { a: FILL_ALPHA, ..self.color });
                rt.FillRectangle(&r, b);
                b.SetColor(&self.color);
                rt.DrawRectangle(&r, b, border, None);
            }
            let mut m = DWRITE_TEXT_METRICS::default();
            self.hint.GetMetrics(&mut m)?;
            let (pad, h) = (HINT_PAD_DIP * self.scale, m.height * HINT_LINE_HEIGHTS);
            let pill = D2D_RECT_F { left: self.hint_at.X - m.width / 2.0 - pad, top: self.hint_at.Y, right: self.hint_at.X + m.width / 2.0 + pad, bottom: self.hint_at.Y + h };
            b.SetColor(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: HINT_BG_ALPHA });
            rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: pill, radiusX: h / 2.0, radiusY: h / 2.0 }, b);
            b.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 });
            rt.DrawTextLayout(Vector2 { X: self.hint_at.X - m.width / 2.0, Y: self.hint_at.Y + (h - m.height) / 2.0 }, &self.hint, b, D2D1_DRAW_TEXT_OPTIONS_NONE);
            rt.EndDraw(None, None)?;
        }
        self.canvas.present(self.hwnd, self.origin, 1.0)
    }
}

impl Drop for AreaEditor {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
