//! 專注時飄在桌寵頭上的小倒數膠囊（🍅 12:34）：專注用主題色＋番茄，休息用綠色＋小茶杯。
//! 滑鼠點得穿過去；可以一直顯示，或滑鼠移到桌寵上才淡入。

use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, SW_SHOWNOACTIVATE, ShowWindow};
use windows::core::{HSTRING, Result};
use windows_numerics::{Matrix3x2, Vector2};

use crate::bubble::popup_shadow;
use crate::icons::{Icon, Icons};
use crate::render::{Canvas, Gfx, TRANSPARENT};
use crate::theme::{self, Palette};

const HEIGHT_DIP: f32 = 30.0;
const ICON_DIP: f32 = 18.0;
const PAD_DIP: f32 = 11.0;
const GAP_DIP: f32 = 5.0;
/// 膠囊底部離桌寵頭頂多遠。
const ABOVE_HEAD_DIP: f32 = 8.0;
/// 底色疊多少主題色（0～1）、外框粗細。
const TINT_ALPHA: f32 = 0.18;
const BORDER_DIP: f32 = 1.5;
/// 用最寬的數字量寬度：跳秒時膠囊不會一直伸縮。
const WIDEST: &str = "00:00";
/// 滑鼠移過去才顯示時，淡入淡出要多久。
const FADE_SECS: f32 = 0.2;
const LOCALE: &str = "zh-tw";

/// 這一格要顯示什麼。
#[derive(Debug, Clone, PartialEq)]
pub struct Shown {
    /// "12:34"
    pub text: String,
    pub focusing: bool,
    /// 頭頂中心（螢幕實體像素）。
    pub head: Vector2,
}

pub struct Countdown {
    hwnd: HWND,
    canvas: Canvas,
    gfx: Gfx,
    brush: ID2D1SolidColorBrush,
    icons: Icons,
    format: Option<(f32, IDWriteTextFormat)>,
    /// 上一次畫的內容與位置（沒變就不重畫）。
    last: Option<(Shown, f32)>,
    opacity: f32,
    visible: bool,
}

impl Countdown {
    pub fn new(gfx: &Gfx, hwnd: HWND) -> Result<Self> {
        let canvas = Canvas::new(gfx)?;
        let brush = unsafe { canvas.rt.CreateSolidColorBrush(&TRANSPARENT, None)? };
        let icons = Icons::new(&canvas.rt);
        Ok(Self { hwnd, canvas, gfx: gfx.clone(), brush, icons, format: None, last: None, opacity: 0.0, visible: false })
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    fn format(&mut self, s: f32) -> Result<IDWriteTextFormat> {
        if let Some((scale, f)) = &self.format
            && *scale == s
        {
            return Ok(f.clone());
        }
        let f = unsafe {
            let f = self.gfx.dwrite.CreateTextFormat(&HSTRING::from(theme::FONT_FAMILY), None, DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, theme::FONT_SIZE_BODY * s, &HSTRING::from(LOCALE))?;
            f.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
            f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            f
        };
        self.icons.clear();
        self.format = Some((s, f.clone()));
        Ok(f)
    }

    /// 每一格呼叫：`shown` 是這一格要不要顯示、顯示什麼（`None` 就淡出收起來）。
    pub fn update(&mut self, shown: Option<Shown>, dt: f32, s: f32, palette: &Palette) -> Result<()> {
        self.opacity = fade(self.opacity, shown.is_some(), dt);
        if self.opacity <= 0.0 {
            self.last = None;
            if std::mem::take(&mut self.visible) {
                unsafe {
                    let _ = ShowWindow(self.hwnd, SW_HIDE);
                }
            }
            return Ok(());
        }
        // 淡出時停在最後畫的那個樣子。
        let Some(shown) = shown.or_else(|| self.last.as_ref().map(|(sh, _)| sh.clone())) else { return Ok(()) };
        if self.last.as_ref().is_some_and(|(sh, o)| *sh == shown && (*o - self.opacity).abs() < 1e-3) {
            return Ok(());
        }
        let origin = self.paint(&shown, s, palette)?;
        self.canvas.present(self.hwnd, origin, self.opacity)?;
        if !self.visible {
            self.visible = true;
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            }
        }
        self.last = Some((shown, self.opacity));
        Ok(())
    }

    /// 畫進畫布，回傳視窗在螢幕上的左上角。
    fn paint(&mut self, shown: &Shown, s: f32, palette: &Palette) -> Result<POINT> {
        let format = self.format(s)?;
        let text_w = unsafe {
            let wide: Vec<u16> = WIDEST.encode_utf16().collect();
            let layout = self.gfx.dwrite.CreateTextLayout(&wide, &format, f32::MAX, f32::MAX)?;
            let mut m = DWRITE_TEXT_METRICS::default();
            layout.GetMetrics(&mut m)?;
            m.width.ceil()
        };
        let (h, pad, icon, gap) = (HEIGHT_DIP * s, PAD_DIP * s, ICON_DIP * s, GAP_DIP * s);
        let margin = (theme::SHADOW_POPUP_BLUR * s).ceil();
        let pill_w = pad + icon + gap + text_w + pad;
        let pill = D2D_RECT_F { left: margin, top: margin, right: margin + pill_w, bottom: margin + h };
        self.canvas.ensure_size((pill.right + margin).ceil() as i32, (pill.bottom + margin).ceil() as i32)?;
        let tint = if shown.focusing { palette.accent } else { palette.success };
        let doc = self.icons.doc(if shown.focusing { Icon::Tomato } else { Icon::Cup }, tint, icon);
        let rt = &self.canvas.rt;
        let brush = &self.brush;
        let radius = h / 2.0;
        unsafe {
            rt.BeginDraw();
            rt.Clear(Some(&TRANSPARENT));
            popup_shadow(rt, brush, pill, radius, s * 0.5, palette);
            let rounded = D2D1_ROUNDED_RECT { rect: pill, radiusX: radius, radiusY: radius };
            brush.SetColor(&palette.surface_raised.d2d());
            rt.FillRoundedRectangle(&rounded, brush);
            brush.SetColor(&tint.with_alpha(TINT_ALPHA));
            rt.FillRoundedRectangle(&rounded, brush);
            brush.SetColor(&tint.d2d());
            rt.DrawRoundedRectangle(&rounded, brush, BORDER_DIP * s, None);
            if let Some(doc) = &doc {
                self.icons.draw(doc, &Matrix3x2::translation(pill.left + pad, pill.top + (h - icon) / 2.0));
                rt.SetTransform(&Matrix3x2::identity());
            }
            let wide: Vec<u16> = shown.text.encode_utf16().collect();
            let text_rect = D2D_RECT_F { left: pill.left + pad + icon + gap, top: pill.top, right: pill.right - pad, bottom: pill.bottom };
            brush.SetColor(&palette.text.d2d());
            rt.DrawText(&wide, &format, &text_rect, brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            rt.EndDraw(None, None)?;
        }
        let x = (shown.head.X - margin - pill_w / 2.0).round() as i32;
        let y = (shown.head.Y - ABOVE_HEAD_DIP * s - h - margin).round() as i32;
        Ok(POINT { x, y })
    }

    /// 截圖工具用：畫一個（不開視窗）。
    pub fn render(&mut self, shown: &Shown, s: f32, palette: &Palette) -> Result<(i32, i32)> {
        self.paint(shown, s, palette)?;
        Ok(self.canvas.size())
    }

    pub fn pixels(&self) -> &[u8] {
        self.canvas.pixels()
    }
}

/// 剩幾秒 → "12:34"（超過一小時就是 "75:00" 這樣，番茄鐘不會那麼長）。
pub fn clock_text(secs: i64) -> String {
    let secs = secs.max(0);
    format!("{:02}:{:02}", secs / 60, secs % 60)
}

/// 淡入淡出推進一格：要顯示就往 1 走、到了停住；不顯示就往 0 走。
fn fade(opacity: f32, showing: bool, dt: f32) -> f32 {
    let step = dt / FADE_SECS;
    if showing { (opacity + step).min(1.0) } else { (opacity - step).max(0.0) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stays_fully_shown_instead_of_flickering() {
        let mut o = 0.0;
        let mut seen = Vec::new();
        for _ in 0..30 {
            o = fade(o, true, 0.035);
            seen.push(o);
        }
        assert!(seen[10..].iter().all(|&o| o == 1.0), "once faded in it stays at full opacity: {seen:?}");
        assert_eq!(fade(1.0, false, 1.0), 0.0);
        assert_eq!(clock_text(1498), "24:58");
    }
}
