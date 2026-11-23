//! 桌寵頭上的卡片（提醒、錯過的提醒、一般通知）：獨立的分層視窗，尾巴指向桌寵。
//! 尺寸、顏色、圓角、字級、動畫全部來自設計參數（`theme`）。

use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, SW_SHOWNOACTIVATE, ShowWindow};
use windows::core::{HSTRING, Result};
use windows_numerics::{Matrix3x2, Vector2};

use crate::icons::{Icon, Icons};
use crate::render::{Canvas, Gfx, TRANSPARENT};
use crate::theme::{self, Palette, Rgba};

pub const CARD_WIDTH_DIP: f32 = 300.0;
/// 左上角圓形圖示底的直徑，與裡面圖示的大小。
const TILE_DIP: f32 = 40.0;
const GLYPH_DIP: f32 = 22.0;
/// 圖示底色：主題色的淡色版（透明度）。
const TILE_TINT_ALPHA: f32 = 0.16;
const BUTTON_HEIGHT_DIP: f32 = 36.0;
const TAIL_WIDTH_DIP: f32 = 20.0;
const TAIL_HEIGHT_DIP: f32 = 10.0;
/// 尾巴尖端離桌寵頭頂的距離。
const TAIL_GAP_DIP: f32 = 2.0;
const TITLE_MAX_LINES: f32 = 2.0;
const SHADOW_LAYERS: usize = 16;
/// 出現時從這個大小彈到原尺寸。
const APPEAR_FROM_SCALE: f32 = 0.92;
const LOCALE: &str = "zh-tw";
/// 錯過的提醒最多列幾則，其餘收成「還有 N 則」。
pub const MISSED_MAX_ROWS: usize = 4;
/// 講話泡泡：文字最寬多少、最多幾行（超過用刪節號）。
const SPEECH_MAX_TEXT_DIP: f32 = 220.0;
const SPEECH_MAX_LINES: f32 = 3.0;
/// 排平均時每次縮窄多少。
const SPEECH_BALANCE_STEP_DIP: f32 = 6.0;

#[derive(Debug, Clone, PartialEq)]
pub enum Card {
    Reminder { title: String, time: String, snooze_open: bool },
    Missed { rows: Vec<(String, String)>, more: usize },
    Info { title: String, detail: Option<String>, tone: Tone },
    /// 新版下載好了：「現在更新」= `Done`，「下次啟動時」= `Dismiss`。
    Update { version: String },
    /// 桌寵講的話：沒有圖示、沒有按鈕，寬度跟著字數；點一下收掉（`Dismiss`）。
    Speech { text: String },
}

impl Card {
    /// 點卡片任何地方都會收掉（沒有按鈕要按）。
    fn tap_to_dismiss(&self) -> bool {
        matches!(self, Card::Info { .. } | Card::Speech { .. })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Success,
    Warning,
}

impl Card {
    pub fn success(title: impl Into<String>, detail: Option<String>) -> Self {
        Card::Info { title: title.into(), detail, tone: Tone::Success }
    }

    pub fn warning(title: impl Into<String>, detail: Option<String>) -> Self {
        Card::Info { title: title.into(), detail, tone: Tone::Warning }
    }
}

#[derive(Debug, Clone, Copy)]
enum Tint {
    Accent,
    Success,
    Warning,
}

impl Tint {
    fn color(self, p: &Palette) -> Rgba {
        match self {
            Tint::Accent => p.accent,
            Tint::Success => p.success,
            Tint::Warning => p.warning,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardAction {
    Done,
    OpenSnooze,
    Snooze(u32),
    Dismiss,
}

/// 「稍後」展開後的選項（分鐘）。
pub const SNOOZE_CHOICES: [(u32, &str); 3] = [(5, "5 分鐘"), (15, "15 分鐘"), (60, "1 小時")];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pointer {
    Move,
    Down,
    Up,
    Leave,
}

struct Button {
    rect: D2D_RECT_F,
    label: String,
    action: CardAction,
    primary: bool,
}

struct Formats {
    scale: f32,
    caption: IDWriteTextFormat,
    body: IDWriteTextFormat,
    body_bold: IDWriteTextFormat,
    subtitle: IDWriteTextFormat,
    title: IDWriteTextFormat,
    button: IDWriteTextFormat,
}

/// 排版結果（實體像素，座標相對於視窗左上角）。
struct Layout {
    width: i32,
    height: i32,
    card: D2D_RECT_F,
    tail_x: f32,
    texts: Vec<(IDWriteTextLayout, Vector2, Ink)>,
    /// 卡片寬度（講話泡泡跟著字數變，其他固定）。
    card_w: f32,
    tile: Option<(D2D_RECT_F, Icon, Tint)>,
    buttons: Vec<Button>,
}

#[derive(Clone, Copy)]
enum Ink {
    Text,
    Muted,
}

/// 只管排版與畫，不管視窗；截圖工具也用它。
pub struct CardPainter {
    pub canvas: Canvas,
    gfx: Gfx,
    brush: ID2D1SolidColorBrush,
    formats: Option<Formats>,
    layout: Option<Layout>,
    icons: Icons,
}

impl CardPainter {
    pub fn new(gfx: &Gfx) -> Result<Self> {
        let canvas = Canvas::new(gfx)?;
        let brush = unsafe { canvas.rt.CreateSolidColorBrush(&TRANSPARENT, None)? };
        let icons = Icons::new(&canvas.rt);
        Ok(Self { canvas, gfx: gfx.clone(), brush, formats: None, layout: None, icons })
    }

    fn formats(&mut self, s: f32) -> Result<&Formats> {
        if self.formats.as_ref().is_none_or(|f| f.scale != s) {
            self.icons.clear();
            let make = |size: f32, bold: bool, center: bool| -> Result<IDWriteTextFormat> {
                unsafe {
                    let f = self.gfx.dwrite.CreateTextFormat(
                        &HSTRING::from(theme::FONT_FAMILY),
                        None,
                        if bold { DWRITE_FONT_WEIGHT_BOLD } else { DWRITE_FONT_WEIGHT_NORMAL },
                        DWRITE_FONT_STYLE_NORMAL,
                        DWRITE_FONT_STRETCH_NORMAL,
                        size * s,
                        &HSTRING::from(LOCALE),
                    )?;
                    if center {
                        f.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
                        f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
                        f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
                    }
                    Ok(f)
                }
            };
            self.formats = Some(Formats {
                scale: s,
                caption: make(theme::FONT_SIZE_CAPTION, false, false)?,
                body: make(theme::FONT_SIZE_BODY, false, false)?,
                body_bold: make(theme::FONT_SIZE_BODY, true, false)?,
                subtitle: make(theme::FONT_SIZE_SUBTITLE, true, false)?,
                title: make(theme::FONT_SIZE_TITLE, true, false)?,
                button: make(theme::FONT_SIZE_BODY, true, true)?,
            });
        }
        Ok(self.formats.as_ref().expect("just set"))
    }

    fn text(&self, text: &str, format: &IDWriteTextFormat, max_w: f32, max_lines: Option<f32>) -> Result<(IDWriteTextLayout, f32, f32)> {
        let wide: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            let line = format.GetFontSize() * 1.4;
            let max_h = max_lines.map_or(f32::MAX, |n| line * n);
            let layout = self.gfx.dwrite.CreateTextLayout(&wide, format, max_w, max_h)?;
            if max_lines.is_some() {
                let trimming = DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, delimiter: 0, delimiterCount: 0 };
                let sign = self.gfx.dwrite.CreateEllipsisTrimmingSign(format)?;
                layout.SetTrimming(&trimming, &sign)?;
            }
            let mut m = DWRITE_TEXT_METRICS::default();
            layout.GetMetrics(&mut m)?;
            Ok((layout, m.widthIncludingTrailingWhitespace, m.height.min(max_h)))
        }
    }

    /// 依卡片內容排版；`tail_x` 是尾巴在卡片寬度裡的位置（0～1，0.5 = 正中間）。
    /// 版型：左上角一個圓形圖示，文字排在它右邊，按鈕橫跨整張卡片寬度。
    fn layout(&mut self, card: &Card, s: f32, tail_frac: f32) -> Result<()> {
        if let Card::Speech { text } = card {
            return self.layout_speech(text, s, tail_frac);
        }
        let margin = (theme::SHADOW_POPUP_BLUR * s).ceil();
        let pad = theme::SPACE_4 * s;
        let card_w = CARD_WIDTH_DIP * s;
        let inner_w = card_w - 2.0 * pad;
        let left = margin + pad;
        let top = margin + pad;
        let tile_size = TILE_DIP * s;
        let text_left = left + tile_size + theme::SPACE_3 * s;
        let text_w = inner_w - tile_size - theme::SPACE_3 * s;
        let tile_rect = D2D_RECT_F { left, top, right: left + tile_size, bottom: top + tile_size };
        let mut y = top;
        let mut texts = Vec::new();
        let mut buttons = Vec::new();
        self.formats(s)?;
        let f = self.formats.as_ref().expect("formats ready");
        let (caption, body, body_bold, subtitle, title_fmt) = (f.caption.clone(), f.body.clone(), f.body_bold.clone(), f.subtitle.clone(), f.title.clone());

        let button_row = |y: f32, items: Vec<(String, CardAction, bool)>, buttons: &mut Vec<Button>| {
            let gap = theme::SPACE_2 * s;
            let n = items.len() as f32;
            let w = (inner_w - gap * (n - 1.0)) / n;
            for (i, (label, action, primary)) in items.into_iter().enumerate() {
                let x = left + i as f32 * (w + gap);
                buttons.push(Button { rect: D2D_RECT_F { left: x, top: y, right: x + w, bottom: y + BUTTON_HEIGHT_DIP * s }, label, action, primary });
            }
        };

        let tile = match card {
            Card::Reminder { title, time, snooze_open } => {
                let (t, _, h) = self.text(time, &caption, text_w, Some(1.0))?;
                texts.push((t, Vector2 { X: text_left, Y: y }, Ink::Muted));
                y += h;
                let (t, _, h) = self.text(title, &title_fmt, text_w, Some(TITLE_MAX_LINES))?;
                texts.push((t, Vector2 { X: text_left, Y: y }, Ink::Text));
                y = (y + h).max(tile_rect.bottom) + theme::SPACE_4 * s;
                let items = if *snooze_open {
                    SNOOZE_CHOICES.iter().map(|(m, l)| (l.to_string(), CardAction::Snooze(*m), false)).collect()
                } else {
                    vec![("完成".to_string(), CardAction::Done, true), ("稍後".to_string(), CardAction::OpenSnooze, false)]
                };
                button_row(y, items, &mut buttons);
                y += BUTTON_HEIGHT_DIP * s;
                (Icon::BellRing, Tint::Accent)
            }
            Card::Missed { rows, more } => {
                let (t, _, h) = self.text("錯過的提醒", &subtitle, text_w, Some(1.0))?;
                texts.push((t, Vector2 { X: text_left, Y: y + (tile_size - h) / 2.0 }, Ink::Text));
                y = tile_rect.bottom + theme::SPACE_3 * s;
                for (name, when) in rows {
                    let (w_layout, w_w, w_h) = self.text(when, &caption, inner_w / 2.0, Some(1.0))?;
                    let (n_layout, _, n_h) = self.text(name, &body, inner_w - w_w - theme::SPACE_3 * s, Some(1.0))?;
                    let row_h = n_h.max(w_h);
                    texts.push((n_layout, Vector2 { X: left, Y: y }, Ink::Text));
                    texts.push((w_layout, Vector2 { X: left + inner_w - w_w, Y: y + (row_h - w_h) / 2.0 }, Ink::Muted));
                    y += row_h + theme::SPACE_1 * s;
                }
                if *more > 0 {
                    let (t, _, h) = self.text(&format!("還有 {more} 則"), &caption, inner_w, Some(1.0))?;
                    texts.push((t, Vector2 { X: left, Y: y }, Ink::Muted));
                    y += h;
                }
                y += theme::SPACE_3 * s;
                button_row(y, vec![("全部完成".to_string(), CardAction::Dismiss, true)], &mut buttons);
                y += BUTTON_HEIGHT_DIP * s;
                (Icon::History, Tint::Accent)
            }
            Card::Update { version } => {
                let (t, _, h) = self.text(&format!("新版 {version} 準備好了"), &body_bold, text_w, Some(TITLE_MAX_LINES))?;
                texts.push((t, Vector2 { X: text_left, Y: y + ((tile_size - h) / 2.0).max(0.0) }, Ink::Text));
                y = (y + h).max(tile_rect.bottom) + theme::SPACE_4 * s;
                button_row(y, vec![("現在更新".to_string(), CardAction::Done, true), ("下次啟動時".to_string(), CardAction::Dismiss, false)], &mut buttons);
                y += BUTTON_HEIGHT_DIP * s;
                (Icon::Download, Tint::Accent)
            }
            Card::Speech { .. } => unreachable!("laid out by layout_speech"),
            Card::Info { title, detail, tone } => {
                let (t, _, title_h) = self.text(title, &body_bold, text_w, Some(TITLE_MAX_LINES))?;
                let detail_layout = match detail {
                    Some(d) => Some(self.text(d, &caption, text_w, Some(TITLE_MAX_LINES))?),
                    None => None,
                };
                let gap = theme::SPACE_1 * s;
                let block_h = title_h + detail_layout.as_ref().map_or(0.0, |(_, _, h)| gap + h);
                // 文字比圖示矮時，整塊文字跟圖示垂直置中。
                let mut ty = y + ((tile_size - block_h) / 2.0).max(0.0);
                texts.push((t, Vector2 { X: text_left, Y: ty }, Ink::Text));
                ty += title_h;
                if let Some((dl, _, dh)) = detail_layout {
                    ty += gap;
                    texts.push((dl, Vector2 { X: text_left, Y: ty }, Ink::Muted));
                    ty += dh;
                }
                y = ty.max(tile_rect.bottom);
                match tone {
                    Tone::Success => (Icon::CircleCheck, Tint::Success),
                    Tone::Warning => (Icon::Alert, Tint::Warning),
                }
            }
        };
        y += pad;
        self.finish_layout(margin, card_w, y, s, tail_frac, texts, Some((tile_rect, tile.0, tile.1)), buttons);
        Ok(())
    }

    /// 講話泡泡：一段字置中，左右留白比上下多一點，寬度貼著字。
    fn layout_speech(&mut self, text: &str, s: f32, tail_frac: f32) -> Result<()> {
        let margin = (theme::SHADOW_POPUP_BLUR * s).ceil();
        let (pad_x, pad_y) = (theme::SPACE_3 * s, theme::SPACE_2 * s);
        self.formats(s)?;
        let body = self.formats.as_ref().expect("formats ready").body.clone();
        let max_w = SPEECH_MAX_TEXT_DIP * s;
        let (mut layout, mut w, mut h) = self.text(text, &body, max_w, Some(SPEECH_MAX_LINES))?;
        // 換行時把每行排平均：一直縮窄到行數會多一行為止，不要第二行只剩兩個字。
        let lines = |l: &IDWriteTextLayout| -> Result<u32> {
            let mut m = DWRITE_TEXT_METRICS::default();
            unsafe { l.GetMetrics(&mut m)? };
            Ok(m.lineCount)
        };
        let n = lines(&layout)?;
        if n > 1 && (n as f32) < SPEECH_MAX_LINES {
            let step = SPEECH_BALANCE_STEP_DIP * s;
            let mut try_w = w - step;
            while try_w > 0.0 {
                let candidate = self.text(text, &body, try_w, Some(SPEECH_MAX_LINES))?;
                if lines(&candidate.0)? > n {
                    break;
                }
                (layout, w, h) = candidate;
                try_w = w - step;
            }
        }
        // 太短的字（「嗯？」）也要寬到放得下兩個圓角和中間的尾巴。
        let min_w = 2.0 * (theme::RADIUS_LG + TAIL_WIDTH_DIP) * s;
        let card_w = (w + 2.0 * pad_x).max(min_w).ceil();
        let at = Vector2 { X: margin + (card_w - w) / 2.0, Y: margin + pad_y };
        self.finish_layout(margin, card_w, margin + pad_y + h + pad_y, s, tail_frac, vec![(layout, at, Ink::Text)], None, Vec::new());
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_layout(&mut self, margin: f32, card_w: f32, bottom: f32, s: f32, tail_frac: f32, texts: Vec<(IDWriteTextLayout, Vector2, Ink)>, tile: Option<(D2D_RECT_F, Icon, Tint)>, buttons: Vec<Button>) {
        let card_rect = D2D_RECT_F { left: margin, top: margin, right: margin + card_w, bottom };
        let corner = theme::RADIUS_LG * s + TAIL_WIDTH_DIP * s / 2.0;
        let tail_x = (margin + card_w * tail_frac).clamp(margin + corner, margin + card_w - corner);
        self.layout = Some(Layout {
            width: (card_w + 2.0 * margin).ceil() as i32,
            height: (bottom + TAIL_HEIGHT_DIP * s + margin).ceil() as i32,
            card: card_rect,
            tail_x,
            texts,
            card_w,
            tile,
            buttons,
        });
    }

    /// 尾巴尖端在視窗裡的位置。
    fn tail_tip(&self, s: f32) -> Vector2 {
        let l = self.layout.as_ref().expect("layout first");
        Vector2 { X: l.tail_x, Y: l.card.bottom + TAIL_HEIGHT_DIP * s }
    }

    fn hit(&self, x: f32, y: f32) -> Option<usize> {
        let l = self.layout.as_ref()?;
        l.buttons.iter().position(|b| x >= b.rect.left && x <= b.rect.right && y >= b.rect.top && y <= b.rect.bottom)
    }

    fn inside_card(&self, x: f32, y: f32) -> bool {
        self.layout.as_ref().is_some_and(|l| x >= l.card.left && x <= l.card.right && y >= l.card.top && y <= l.card.bottom)
    }

    fn paint(&mut self, palette: &Palette, s: f32, hover: Option<usize>, pressed: Option<usize>, appear_scale: f32) -> Result<()> {
        let tip = self.tail_tip(s);
        let tile = self.layout.as_ref().and_then(|l| l.tile);
        let glyph_size = GLYPH_DIP * s;
        let glyph_doc = tile.and_then(|(_, glyph, tint)| self.icons.doc(glyph, tint.color(palette), glyph_size));
        let l = self.layout.as_ref().expect("layout first");
        self.canvas.ensure_size(l.width, l.height)?;
        let rt = &self.canvas.rt;
        let brush = &self.brush;
        let radius = theme::RADIUS_LG * s;
        let tail_path = self.tail_path(l, s)?;
        let button_font = self.formats.as_ref().expect("formats ready").button.clone();
        unsafe {
            rt.BeginDraw();
            rt.Clear(Some(&TRANSPARENT));
            rt.SetTransform(&Matrix3x2::scale_around(appear_scale, appear_scale, tip));

            popup_shadow(rt, brush, l.card, radius, s, palette);

            let card = D2D1_ROUNDED_RECT { rect: l.card, radiusX: radius, radiusY: radius };
            brush.SetColor(&palette.surface_raised.d2d());
            rt.FillRoundedRectangle(&card, brush);
            brush.SetColor(&palette.border.d2d());
            rt.DrawRoundedRectangle(&card, brush, s, None);
            brush.SetColor(&palette.surface_raised.d2d());
            rt.FillGeometry(&tail_path, brush, None);
            brush.SetColor(&palette.border.d2d());
            rt.DrawGeometry(&tail_path, brush, s, None);
            // 蓋掉尾巴與卡片接縫處的那段邊框。
            brush.SetColor(&palette.surface_raised.d2d());
            let tw = TAIL_WIDTH_DIP * s / 2.0 - s;
            rt.FillRectangle(&D2D_RECT_F { left: l.tail_x - tw, top: l.card.bottom - s * 1.5, right: l.tail_x + tw, bottom: l.card.bottom + s * 0.5 }, brush);

            if let Some((rect, _, tint)) = tile {
                let r = (rect.right - rect.left) / 2.0;
                let center = Vector2 { X: rect.left + r, Y: rect.top + r };
                brush.SetColor(&tint.color(palette).with_alpha(TILE_TINT_ALPHA));
                rt.FillEllipse(&D2D1_ELLIPSE { point: center, radiusX: r, radiusY: r }, brush);
                if let Some(doc) = &glyph_doc {
                    let at = Matrix3x2::translation(center.X - glyph_size / 2.0, center.Y - glyph_size / 2.0);
                    self.icons.draw(doc, &(at * Matrix3x2::scale_around(appear_scale, appear_scale, tip)));
                    rt.SetTransform(&Matrix3x2::scale_around(appear_scale, appear_scale, tip));
                }
            }
            for (layout, at, ink) in &l.texts {
                brush.SetColor(&match ink {
                    Ink::Text => palette.text.d2d(),
                    Ink::Muted => palette.text_muted.d2d(),
                });
                rt.DrawTextLayout(*at, layout, brush, D2D1_DRAW_TEXT_OPTIONS_CLIP);
            }
            for (i, b) in l.buttons.iter().enumerate() {
                let (fill, ink) = match (b.primary, pressed == Some(i), hover == Some(i)) {
                    (true, true, _) => (palette.accent_pressed, palette.on_accent),
                    (true, false, true) => (palette.accent_hover, palette.on_accent),
                    (true, false, false) => (palette.accent, palette.on_accent),
                    (false, true, _) => (palette.border_strong, palette.text),
                    (false, false, true) => (palette.border, palette.text),
                    (false, false, false) => (palette.surface_sunken, palette.text),
                };
                let r = theme::RADIUS_SM * s;
                brush.SetColor(&fill.d2d());
                rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: b.rect, radiusX: r, radiusY: r }, brush);
                brush.SetColor(&ink.d2d());
                let label: Vec<u16> = b.label.encode_utf16().collect();
                rt.DrawText(&label, &button_font, &b.rect, brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            }
            rt.SetTransform(&Matrix3x2::identity());
            rt.EndDraw(None, None)
        }
    }

    fn tail_path(&self, l: &Layout, s: f32) -> Result<ID2D1PathGeometry> {
        let half = TAIL_WIDTH_DIP * s / 2.0;
        let top = l.card.bottom - s * 0.5;
        unsafe {
            let path = self.gfx.d2d.CreatePathGeometry()?;
            let sink = path.Open()?;
            sink.BeginFigure(Vector2 { X: l.tail_x - half, Y: top }, D2D1_FIGURE_BEGIN_FILLED);
            sink.AddLine(Vector2 { X: l.tail_x, Y: l.card.bottom + TAIL_HEIGHT_DIP * s });
            sink.AddLine(Vector2 { X: l.tail_x + half, Y: top });
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            sink.Close()?;
            Ok(path)
        }
    }

    /// 截圖工具用：排版並畫一張卡（不開視窗）。
    pub fn render(&mut self, card: &Card, palette: &Palette, s: f32, hover: Option<usize>) -> Result<(i32, i32)> {
        self.layout(card, s, 0.5)?;
        self.paint(palette, s, hover, None, 1.0)?;
        Ok(self.canvas.size())
    }
}

/// 浮在桌面上的東西（卡片、右鍵選單）底下的柔和陰影。畫面要先留 `SHADOW_POPUP_BLUR` 的邊。
/// 一層層往外擴的圓角矩形疊出來。每層的透明度算成「離邊緣 d 的地方，
/// 疊起來的濃度 = A × (1 - d/blur)²」，近邊緣濃、往外平滑淡掉，不會出現一圈硬邊。
pub fn popup_shadow(rt: &ID2D1RenderTarget, brush: &ID2D1SolidColorBrush, rect: D2D_RECT_F, radius: f32, s: f32, palette: &Palette) {
    let blur = theme::SHADOW_POPUP_BLUR * s;
    let offset = theme::SHADOW_POPUP_Y * s * 0.5;
    let density = |d: f32| theme::SHADOW_POPUP_ALPHA * (1.0 - d / blur).max(0.0).powi(2);
    for i in (1..=SHADOW_LAYERS).rev() {
        let grow = blur * i as f32 / SHADOW_LAYERS as f32;
        let inner = blur * (i - 1) as f32 / SHADOW_LAYERS as f32;
        let r = D2D_RECT_F { left: rect.left - grow, top: rect.top - grow * 0.5 + offset, right: rect.right + grow, bottom: rect.bottom + grow + offset };
        unsafe {
            brush.SetColor(&palette.shadow.with_alpha(density(inner) - density(grow)));
            rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r, radiusX: radius + grow, radiusY: radius + grow }, brush);
        }
    }
}

pub struct Bubble {
    hwnd: HWND,
    painter: CardPainter,
    card: Option<Card>,
    shown_at: f64,
    /// 上一次畫的時候，卡片出現了多久（毫秒）。
    painted_age_ms: f32,
    hover: Option<usize>,
    pressed: Option<usize>,
    dirty: bool,
    scale: f32,
    last_origin: POINT,
    last_tail_frac: f32,
    /// 收掉前淡出：0～1，乘在整張卡的透明度上。
    fade: f32,
}

impl Bubble {
    pub fn new(hwnd: HWND, gfx: &Gfx) -> Result<Self> {
        Ok(Self {
            hwnd,
            painter: CardPainter::new(gfx)?,
            card: None,
            shown_at: 0.0,
            painted_age_ms: f32::MAX,
            hover: None,
            pressed: None,
            dirty: false,
            scale: 1.0,
            last_origin: POINT { x: i32::MIN, y: i32::MIN },
            last_tail_frac: -1.0,
            fade: 1.0,
        })
    }

    /// 淡出進度（1 = 完全不透明）；講話泡泡快到時間時用。
    pub fn set_fade(&mut self, fade: f32) {
        let fade = fade.clamp(0.0, 1.0);
        if (fade - self.fade).abs() > 1e-3 {
            self.fade = fade;
            self.dirty = true;
        }
    }

    pub fn card(&self) -> Option<&Card> {
        self.card.as_ref()
    }

    pub fn shown_for(&self, now: f64) -> f64 {
        now - self.shown_at
    }

    /// 換一張卡（`animate` = 從小彈出來；同一張卡改內容，例如展開「稍後」，就不重播動畫）。
    pub fn show(&mut self, card: Card, now: f64, animate: bool) {
        if animate {
            self.shown_at = now;
            self.painted_age_ms = 0.0;
        }
        self.card = Some(card);
        self.hover = None;
        self.pressed = None;
        self.fade = 1.0;
        self.dirty = true;
    }

    pub fn hide(&mut self) {
        if self.card.take().is_some() {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
        }
    }

    /// 每一格呼叫：跟著桌寵移動、播出現動畫。`head` 是桌寵頭頂中心（螢幕實體像素）。
    pub fn update(&mut self, now: f64, head: Vector2, work: RECT, dpi_scale: f32, palette: &Palette) -> Result<()> {
        let Some(card) = self.card.clone() else { return Ok(()) };
        let s = dpi_scale;
        let age_ms = ((now - self.shown_at) * 1000.0) as f32;
        // 更新得很慢的時候（桌寵躲起來，一秒只更新兩次）出場動畫可能整段被跳過：
        // 動畫結束後一定再畫一次最後那一格，不然會停在剛出現、幾乎全透明的那格。
        let animating = age_ms < theme::MOTION_MEDIUM || self.painted_age_ms < theme::MOTION_MEDIUM;
        // 先用正中間排一次拿到寬度，再依螢幕邊緣決定尾巴要偏哪邊。
        if self.dirty || s != self.scale {
            self.painter.layout(&card, s, 0.5)?;
        }
        let (w, h) = {
            let l = self.painter.layout.as_ref().expect("layout");
            (l.width, l.height)
        };
        let margin = (theme::SHADOW_POPUP_BLUR * s).ceil() as i32;
        let x = ((head.X - w as f32 / 2.0).round() as i32).clamp(work.left - margin, (work.right - w + margin).max(work.left - margin));
        let card_w = self.painter.layout.as_ref().expect("layout").card_w;
        let tail_frac = ((head.X - x as f32 - margin as f32) / card_w).clamp(0.0, 1.0);
        let tip_y = head.Y - TAIL_GAP_DIP * s;
        let y = (tip_y - (h - margin) as f32).round() as i32;
        let origin = POINT { x, y: y.max(work.top - margin) };

        let moved = origin.x != self.last_origin.x || origin.y != self.last_origin.y || (tail_frac - self.last_tail_frac).abs() > 1e-3;
        if !(self.dirty || animating || moved || s != self.scale) {
            return Ok(());
        }
        if (tail_frac - 0.5).abs() > 1e-3 || self.dirty || s != self.scale {
            self.painter.layout(&card, s, tail_frac)?;
        }
        let p = (age_ms / theme::MOTION_MEDIUM).clamp(0.0, 1.0);
        let scale = APPEAR_FROM_SCALE + (1.0 - APPEAR_FROM_SCALE) * theme::ease(theme::MOTION_SPRING, p);
        let opacity = theme::ease(theme::MOTION_EASE_OUT, (age_ms / theme::MOTION_FAST).clamp(0.0, 1.0)) * self.fade;
        self.painter.paint(palette, s, self.hover, self.pressed, scale)?;
        self.painter.canvas.present(self.hwnd, origin, opacity)?;
        self.painted_age_ms = age_ms;
        if self.last_origin.x == i32::MIN || self.dirty {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            }
        }
        self.last_origin = origin;
        self.last_tail_frac = tail_frac;
        self.scale = s;
        self.dirty = false;
        Ok(())
    }

    /// 卡片視窗的滑鼠事件（視窗內座標）；按下又在同一顆按鈕上放開才算點。
    pub fn pointer(&mut self, kind: Pointer, x: f32, y: f32) -> Option<CardAction> {
        let card = self.card.as_ref()?;
        let hit = self.painter.hit(x, y);
        match kind {
            Pointer::Move => {
                if hit != self.hover {
                    self.hover = hit;
                    self.dirty = true;
                }
                None
            }
            Pointer::Leave => {
                if self.hover.is_some() || self.pressed.is_some() {
                    self.hover = None;
                    self.pressed = None;
                    self.dirty = true;
                }
                None
            }
            Pointer::Down => {
                self.pressed = hit;
                self.dirty = true;
                None
            }
            Pointer::Up => {
                let pressed = self.pressed.take();
                self.dirty = true;
                if card.tap_to_dismiss() && self.painter.inside_card(x, y) {
                    return Some(CardAction::Dismiss);
                }
                match (pressed, hit) {
                    (Some(p), Some(h)) if p == h => self.painter.layout.as_ref().map(|l| l.buttons[h].action),
                    _ => None,
                }
            }
        }
    }

    pub fn is_over_button(&self, x: f32, y: f32) -> bool {
        self.painter.hit(x, y).is_some() || self.card.as_ref().is_some_and(Card::tap_to_dismiss) && self.painter.inside_card(x, y)
    }
}
