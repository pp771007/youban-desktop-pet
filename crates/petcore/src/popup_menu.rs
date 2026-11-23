//! 右鍵選單（桌寵身上、托盤圖示共用）：自己畫的分層視窗，跟提醒卡同一套長相 ——
//! Windows 內建的選單跟桌寵的畫風搭不起來。項目依功能分組，每組一個小標題；
//! 放哪些項目由呼叫端決定（沒開的功能整組不放）。
//!
//! 有下一層的項目，滑鼠移上去就在旁邊展開第二個視窗（只支援兩層）。第二層不搶焦點：
//! 焦點一直在第一層，點選單外面時第一層失去焦點，整個選單一起收。

use std::collections::HashMap;
use std::path::PathBuf;

use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{VIRTUAL_KEY, VK_BACK, VK_DOWN, VK_ESCAPE, VK_LEFT, VK_RETURN, VK_RIGHT, VK_SPACE, VK_UP};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{HSTRING, PCWSTR, Result, w};
use windows_numerics::{Matrix3x2, Vector2};

use crate::bubble::popup_shadow;
use crate::icons::{Icon, Icons};
use crate::log;
use crate::render::{Canvas, Gfx, TRANSPARENT};
use crate::sprites::Wic;
use crate::theme::{self, Palette};

const MIN_WIDTH_DIP: f32 = 200.0;
const ROW_DIP: f32 = 34.0;
const HEADER_DIP: f32 = 26.0;
const SEPARATOR_DIP: f32 = 9.0;
const ICON_DIP: f32 = 18.0;
/// 桌寵小圖的大小（放在圖示的位置，比圖示大一點才看得出是誰）。
const PICTURE_DIP: f32 = 26.0;
/// 文字跟右邊箭頭（或勾）之間至少留多少。
const TRAILING_GAP_DIP: f32 = 24.0;
/// 滑鼠從展開的那一列移到別列，停這麼久才收掉第二層：斜著往第二層移時會掠過別列。
const SUB_CLOSE_DELAY_SECS: f64 = 0.3;
const LOCALE: &str = "zh-tw";

#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    /// 分組的小標題（點不了）。
    Header(String),
    Item(Item),
    Separator,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub id: u32,
    pub label: String,
    pub icon: Option<Icon>,
    /// 放在圖示位置的圖檔（桌寵的站姿）；有的話不畫 `icon`。
    pub picture: Option<PathBuf>,
    /// 目前選中的那一個：打勾、字變粗。
    pub checked: bool,
    /// 第二層；有的話右邊畫箭頭，滑鼠移上去就展開。
    pub children: Vec<Entry>,
}

impl Item {
    /// 一般的一列（有圖示、沒打勾、沒有下一層）。
    pub fn entry(id: u32, label: impl Into<String>, icon: Icon) -> Entry {
        Entry::Item(Self { id, label: label.into(), icon: Some(icon), picture: None, checked: false, children: Vec::new() })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MenuInput {
    Move(f32, f32),
    Leave,
    Down(f32, f32),
    Up(f32, f32),
    Key(u16),
    /// 點了選單外面（第一層失去焦點）。
    Deactivate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Pick(u32),
    Close,
}

struct Row {
    rect: D2D_RECT_F,
    text: Option<(IDWriteTextLayout, Vector2)>,
}

/// 只管排版與畫，不管視窗；截圖工具也用它。
pub struct MenuPainter {
    pub canvas: Canvas,
    gfx: Gfx,
    brush: ID2D1SolidColorBrush,
    icons: Icons,
    /// 圖檔 → 縮好的點陣圖（屬於這張畫布，要先 `load_pictures`）。
    pictures: HashMap<PathBuf, (ID2D1Bitmap, f32, f32)>,
    entries: Vec<Entry>,
    rows: Vec<Row>,
    card: D2D_RECT_F,
    /// 圖示（或桌寵小圖）那一欄的寬度。
    lead: f32,
    scale: f32,
}

impl MenuPainter {
    pub fn new(gfx: &Gfx) -> Result<Self> {
        let canvas = Canvas::new(gfx)?;
        let brush = unsafe { canvas.rt.CreateSolidColorBrush(&TRANSPARENT, None)? };
        let icons = Icons::new(&canvas.rt);
        Ok(Self { canvas, gfx: gfx.clone(), brush, icons, pictures: HashMap::new(), entries: Vec::new(), rows: Vec::new(), card: D2D_RECT_F::default(), lead: 0.0, scale: 0.0 })
    }

    /// 把這些項目要用的圖檔先讀進來（讀不到就只是沒有小圖）。
    pub fn load_pictures(&mut self, wic: &Wic, entries: &[Entry], s: f32) {
        let size = PICTURE_DIP * s;
        for e in entries {
            let Entry::Item(Item { picture: Some(path), .. }) = e else { continue };
            if self.pictures.contains_key(path) {
                continue;
            }
            match wic.load_file(path, &self.canvas.target(), size) {
                Ok(bitmap) => {
                    self.pictures.insert(path.clone(), bitmap);
                }
                Err(err) => log!("menu picture {} failed: {err}", path.display()),
            }
        }
    }

    fn format(&self, size: f32, bold: bool) -> Result<IDWriteTextFormat> {
        unsafe {
            let f = self.gfx.dwrite.CreateTextFormat(
                &HSTRING::from(theme::FONT_FAMILY),
                None,
                if bold { DWRITE_FONT_WEIGHT_BOLD } else { DWRITE_FONT_WEIGHT_NORMAL },
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size * self.scale,
                &HSTRING::from(LOCALE),
            )?;
            f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            Ok(f)
        }
    }

    /// 排版；回傳視窗大小（含陰影留的邊）。
    pub fn layout(&mut self, entries: Vec<Entry>, s: f32) -> Result<(i32, i32)> {
        if s != self.scale {
            self.icons.clear();
            self.scale = s;
        }
        let (body, bold, caption) = (self.format(theme::FONT_SIZE_BODY, false)?, self.format(theme::FONT_SIZE_BODY, true)?, self.format(theme::FONT_SIZE_CAPTION, true)?);
        let side = theme::SPACE_3 * s;
        let icon = ICON_DIP * s;
        let has_picture = entries.iter().any(|e| matches!(e, Entry::Item(Item { picture: Some(_), .. })));
        self.lead = if has_picture { PICTURE_DIP } else { ICON_DIP } * s;
        let text_left = side + self.lead + side;
        let mut texts = Vec::new();
        let mut width = MIN_WIDTH_DIP * s;
        for e in &entries {
            let text = match e {
                Entry::Header(label) => Some((label.as_str(), &caption, side + side)),
                Entry::Item(item) => {
                    let trailing = if trailing_icon(item).is_some() { TRAILING_GAP_DIP * s + icon + side } else { side };
                    Some((item.label.as_str(), if item.checked { &bold } else { &body }, text_left + trailing))
                }
                Entry::Separator => None,
            };
            texts.push(match text {
                Some((label, format, extra)) => {
                    let wide: Vec<u16> = label.encode_utf16().collect();
                    let layout = unsafe { self.gfx.dwrite.CreateTextLayout(&wide, format, f32::MAX, f32::MAX)? };
                    let mut m = DWRITE_TEXT_METRICS::default();
                    unsafe { layout.GetMetrics(&mut m)? };
                    width = width.max((m.widthIncludingTrailingWhitespace + extra).ceil());
                    Some((layout, m.height))
                }
                None => None,
            });
        }
        let margin = (theme::SHADOW_POPUP_BLUR * s).ceil();
        let pad = theme::SPACE_1 * s;
        let left = margin + pad;
        let mut y = margin + pad;
        self.rows.clear();
        for (e, text) in entries.iter().zip(texts) {
            let h = match e {
                Entry::Header(_) => HEADER_DIP,
                Entry::Item(_) => ROW_DIP,
                Entry::Separator => SEPARATOR_DIP,
            } * s;
            let rect = D2D_RECT_F { left, top: y, right: left + width, bottom: y + h };
            let text = text.map(|(layout, th)| {
                let x = if matches!(e, Entry::Header(_)) { left + side } else { left + text_left };
                // 小標題貼著下面那組，不置中。
                let ty = if matches!(e, Entry::Header(_)) { rect.bottom - th - theme::SPACE_1 * s } else { y + (h - th) / 2.0 };
                (layout, Vector2 { X: x, Y: ty })
            });
            self.rows.push(Row { rect, text });
            y += h;
        }
        self.card = D2D_RECT_F { left: margin, top: margin, right: left + width + pad, bottom: y + pad };
        self.entries = entries;
        let w = (self.card.right + margin).ceil() as i32;
        let h = (self.card.bottom + margin).ceil() as i32;
        self.canvas.ensure_size(w, h)?;
        Ok((w, h))
    }

    /// 視窗內座標落在哪一個能點的項目上。
    fn hit(&self, x: f32, y: f32) -> Option<usize> {
        self.rows.iter().zip(&self.entries).position(|(r, e)| matches!(e, Entry::Item(_)) && x >= r.rect.left && x < r.rect.right && y >= r.rect.top && y < r.rect.bottom)
    }

    pub fn paint(&mut self, palette: &Palette, hover: Option<usize>, pressed: Option<usize>) -> Result<()> {
        let s = self.scale;
        let icon = ICON_DIP * s;
        let side = theme::SPACE_3 * s;
        // 先把要畫的圖示都準備好（拿圖示要可變借用，畫的時候不行）。
        let mut docs = Vec::new();
        for e in &self.entries {
            let Entry::Item(item) = e else {
                docs.push((None, None));
                continue;
            };
            let lead = if item.picture.is_some() {
                None
            } else if item.checked {
                Some(Icon::Check)
            } else {
                item.icon
            };
            let lead = lead.and_then(|ic| self.icons.doc(ic, palette.accent, icon));
            let trail = trailing_icon(item).and_then(|(ic, accent)| self.icons.doc(ic, if accent { palette.accent } else { palette.text_muted }, icon));
            docs.push((lead, trail));
        }
        let rt = &self.canvas.rt;
        let brush = &self.brush;
        let radius = theme::RADIUS_MD * s;
        unsafe {
            rt.BeginDraw();
            rt.Clear(Some(&TRANSPARENT));
            popup_shadow(rt, brush, self.card, radius, s, palette);
            let card = D2D1_ROUNDED_RECT { rect: self.card, radiusX: radius, radiusY: radius };
            brush.SetColor(&palette.surface_raised.d2d());
            rt.FillRoundedRectangle(&card, brush);
            brush.SetColor(&palette.border.d2d());
            rt.DrawRoundedRectangle(&card, brush, s, None);

            for (i, ((row, e), (lead, trail))) in self.rows.iter().zip(&self.entries).zip(&docs).enumerate() {
                let r = row.rect;
                match e {
                    Entry::Separator => {
                        let mid = ((r.top + r.bottom) / 2.0).floor() + 0.5;
                        brush.SetColor(&palette.border.d2d());
                        rt.DrawLine(Vector2 { X: r.left + side, Y: mid }, Vector2 { X: r.right - side, Y: mid }, brush, s, None);
                    }
                    Entry::Header(_) => {
                        if let Some((layout, at)) = &row.text {
                            brush.SetColor(&palette.text_muted.d2d());
                            rt.DrawTextLayout(*at, layout, brush, D2D1_DRAW_TEXT_OPTIONS_NONE);
                        }
                    }
                    Entry::Item(item) => {
                        if hover == Some(i) || pressed == Some(i) {
                            let fill = if pressed == Some(i) { palette.border } else { palette.accent_soft };
                            brush.SetColor(&fill.d2d());
                            let rr = theme::RADIUS_SM * s;
                            rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r, radiusX: rr, radiusY: rr }, brush);
                        }
                        let mid_y = (r.top + r.bottom) / 2.0;
                        if let Some((bitmap, bw, bh)) = item.picture.as_ref().and_then(|p| self.pictures.get(p)) {
                            let cx = r.left + side + self.lead / 2.0;
                            let dest = D2D_RECT_F { left: cx - bw / 2.0, top: mid_y - bh / 2.0, right: cx + bw / 2.0, bottom: mid_y + bh / 2.0 };
                            rt.DrawBitmap(bitmap, Some(&dest), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
                        }
                        let icon_y = mid_y - icon / 2.0;
                        if let Some(doc) = lead {
                            self.icons.draw(doc, &Matrix3x2::translation(r.left + side + (self.lead - icon) / 2.0, icon_y));
                        }
                        if let Some(doc) = trail {
                            self.icons.draw(doc, &Matrix3x2::translation(r.right - side - icon, icon_y));
                        }
                        rt.SetTransform(&Matrix3x2::identity());
                        if let Some((layout, at)) = &row.text {
                            brush.SetColor(&palette.text.d2d());
                            rt.DrawTextLayout(*at, layout, brush, D2D1_DRAW_TEXT_OPTIONS_NONE);
                        }
                    }
                }
            }
            rt.EndDraw(None, None)
        }
    }

    fn item(&self, index: usize) -> Option<&Item> {
        match self.entries.get(index) {
            Some(Entry::Item(item)) => Some(item),
            _ => None,
        }
    }

    /// 能點的項目裡，從 `from` 往上或往下一個（鍵盤）。
    fn step(&self, from: Option<usize>, down: bool) -> Option<usize> {
        let items: Vec<usize> = (0..self.entries.len()).filter(|&i| self.item(i).is_some()).collect();
        let pos = from.and_then(|h| items.iter().position(|&i| i == h));
        let next = match (pos, down) {
            (None, true) => 0,
            (None, false) => items.len().checked_sub(1)?,
            (Some(p), true) => (p + 1) % items.len(),
            (Some(p), false) => (p + items.len() - 1) % items.len(),
        };
        items.get(next).copied()
    }

    /// 截圖工具用：排版並畫一個選單（不開視窗）。
    pub fn render(&mut self, entries: Vec<Entry>, palette: &Palette, s: f32, hover: Option<usize>) -> Result<(i32, i32)> {
        let size = self.layout(entries, s)?;
        self.paint(palette, hover, None)?;
        Ok(size)
    }
}

/// 右邊要畫什麼：有下一層畫箭頭；用小圖的那種打勾畫在右邊（左邊被小圖佔了）。(圖示, 用強調色)
fn trailing_icon(item: &Item) -> Option<(Icon, bool)> {
    if !item.children.is_empty() {
        Some((Icon::ChevronRight, false))
    } else if item.checked && item.picture.is_some() {
        Some((Icon::Check, true))
    } else {
        None
    }
}

/// 一層選單的視窗（畫布另外放：第二層的畫布收起來後留著，下次展開不用重讀小圖）。
struct Level {
    hwnd: HWND,
    hover: Option<usize>,
    pressed: Option<usize>,
    /// 視窗左上角（螢幕座標）。
    origin: POINT,
}

impl Level {
    fn new(class: PCWSTR, ex_style: WINDOW_EX_STYLE) -> Result<Self> {
        let instance = unsafe { GetModuleHandleW(None)? };
        let hwnd = unsafe { CreateWindowExW(WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | ex_style, class, w!("柚伴選單"), WS_POPUP, 0, 0, 1, 1, None, None, Some(instance.into()), None)? };
        Ok(Self { hwnd, hover: None, pressed: None, origin: POINT::default() })
    }

    fn redraw(&self, painter: &mut MenuPainter, palette: &Palette) -> Result<()> {
        painter.paint(palette, self.hover, self.pressed)?;
        painter.canvas.present(self.hwnd, self.origin, 1.0)
    }
}

impl Drop for Level {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

/// 卡片（不含陰影）在螢幕上的範圍。
fn card_on_screen(level: &Level, painter: &MenuPainter) -> RECT {
    let (o, c) = (level.origin, painter.card);
    RECT { left: o.x + c.left as i32, top: o.y + c.top as i32, right: o.x + c.right as i32, bottom: o.y + c.bottom as i32 }
}

/// 打開中的選單。收掉（`drop`）時連視窗一起關。
pub struct PopupMenu {
    main: Level,
    painter: MenuPainter,
    /// 第二層，與它是第一層的哪一列展開的。
    sub: Option<(usize, Level)>,
    /// 第一層每個有下一層的列各一張畫布：打開時就把小圖讀好，展開時直接用。
    sub_painters: HashMap<usize, MenuPainter>,
    /// 滑鼠移到第一層的別列了：到這個時間還沒回來就收掉第二層。
    close_sub_at: Option<f64>,
    /// 方向鍵目前在第二層裡（焦點還是在第一層的視窗，只是按鍵改給第二層）。
    keys_in_sub: bool,
    class: PCWSTR,
    palette: Palette,
    work: RECT,
}

impl PopupMenu {
    pub fn open(gfx: &Gfx, wic: &Wic, class: PCWSTR, entries: Vec<Entry>, palette: Palette, dpi_scale: f32) -> Result<Self> {
        let mut anchor = POINT::default();
        let work = unsafe {
            let _ = GetCursorPos(&mut anchor);
            let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
            let _ = GetMonitorInfoW(MonitorFromPoint(anchor, MONITOR_DEFAULTTONEAREST), &mut info);
            info.rcWork
        };
        let mut painter = MenuPainter::new(gfx)?;
        painter.load_pictures(wic, &entries, dpi_scale);
        let mut sub_painters = HashMap::new();
        for (i, e) in entries.iter().enumerate() {
            if let Entry::Item(item) = e
                && !item.children.is_empty()
            {
                let mut p = MenuPainter::new(gfx)?;
                p.load_pictures(wic, &item.children, dpi_scale);
                p.layout(item.children.clone(), dpi_scale)?;
                sub_painters.insert(i, p);
            }
        }
        let (w, h) = painter.layout(entries, dpi_scale)?;
        let margin = painter.card.left as i32;
        let (cw, ch) = (w - 2 * margin, h - 2 * margin);
        // 貼著滑鼠開，太靠螢幕邊緣就往左、往上翻。
        let x = if anchor.x + cw > work.right { anchor.x - cw } else { anchor.x };
        let y = if anchor.y + ch > work.bottom { anchor.y - ch } else { anchor.y };
        let mut main = Level::new(class, WINDOW_EX_STYLE(0))?;
        main.origin = POINT { x: x.max(work.left) - margin, y: y.max(work.top) - margin };
        main.redraw(&mut painter, &palette)?;
        unsafe {
            let _ = ShowWindow(main.hwnd, SW_SHOW);
            let _ = SetWindowPos(main.hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
            // 拿到焦點才收得到 Esc、方向鍵，點選單外面時也才會失去焦點而收起來。
            let _ = SetForegroundWindow(main.hwnd);
        }
        Ok(Self { main, painter, sub: None, sub_painters, close_sub_at: None, keys_in_sub: false, class, palette, work })
    }

    /// 兩層的視窗（定時重新放到最上層時用，後面的在上面）。
    pub fn hwnds(&self) -> Vec<HWND> {
        std::iter::once(self.main.hwnd).chain(self.sub.as_ref().map(|(_, l)| l.hwnd)).collect()
    }

    fn sub_index(&self) -> Option<usize> {
        self.sub.as_ref().map(|(i, _)| *i)
    }

    fn redraw_main(&mut self) -> Result<()> {
        self.main.redraw(&mut self.painter, &self.palette)
    }

    fn redraw_sub(&mut self) -> Result<()> {
        let Some((i, level)) = &self.sub else { return Ok(()) };
        let Some(p) = self.sub_painters.get_mut(i) else { return Ok(()) };
        level.redraw(p, &self.palette)
    }

    /// 展開第一層第 `index` 列的第二層（已經是它就不動）。
    fn open_sub(&mut self, index: usize) -> Result<()> {
        self.close_sub_at = None;
        if self.sub_index() == Some(index) {
            return Ok(());
        }
        self.close_sub();
        let Some(p) = self.sub_painters.get_mut(&index) else { return Ok(()) };
        let margin = p.card.left as i32;
        let (cw, ch) = ((p.card.right - p.card.left) as i32, (p.card.bottom - p.card.top) as i32);
        let parent = card_on_screen(&self.main, &self.painter);
        let row_top = self.main.origin.y + self.painter.rows[index].rect.top as i32;
        let pad = (theme::SPACE_1 * self.painter.scale) as i32;
        // 接在第一層右邊（第一列跟展開的那一列對齊）；右邊放不下就放左邊。
        let x = if parent.right + cw <= self.work.right { parent.right - pad } else { parent.left - cw + pad };
        let y = (row_top - pad).min(self.work.bottom - ch).max(self.work.top);
        let mut level = Level::new(self.class, WS_EX_NOACTIVATE)?;
        level.origin = POINT { x: x - margin, y: y - margin };
        level.redraw(p, &self.palette)?;
        unsafe {
            let _ = ShowWindow(level.hwnd, SW_SHOWNOACTIVATE);
            let _ = SetWindowPos(level.hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
        self.sub = Some((index, level));
        Ok(())
    }

    fn close_sub(&mut self) {
        self.close_sub_at = None;
        self.keys_in_sub = false;
        self.sub = None;
    }

    /// 每一格呼叫：滑鼠離開展開的那一列一陣子了就收掉第二層。
    pub fn tick(&mut self, now: f64) -> Result<()> {
        if self.close_sub_at.is_some_and(|at| now >= at) {
            self.close_sub();
            self.redraw_main()?;
        }
        Ok(())
    }

    fn has_children(&self, index: usize) -> bool {
        self.painter.item(index).is_some_and(|it| !it.children.is_empty())
    }

    /// 第一層第 `index` 列被選了：有下一層就展開，沒有就交給呼叫端。
    fn choose_main(&mut self, index: usize) -> Result<Option<Outcome>> {
        let Some(item) = self.painter.item(index) else { return Ok(None) };
        if item.children.is_empty() {
            return Ok(Some(Outcome::Pick(item.id)));
        }
        self.open_sub(index)?;
        Ok(None)
    }

    fn choose_sub(&self, index: usize) -> Option<Outcome> {
        let p = self.sub_painters.get(&self.sub_index()?)?;
        p.item(index).filter(|it| it.children.is_empty()).map(|it| Outcome::Pick(it.id))
    }

    /// `hwnd` 是收到這個輸入的視窗（第一層或第二層）。
    pub fn input(&mut self, hwnd: HWND, input: MenuInput, now: f64) -> Result<Option<Outcome>> {
        if self.sub.as_ref().is_some_and(|(_, l)| l.hwnd == hwnd) {
            return self.sub_input(input);
        }
        if hwnd != self.main.hwnd {
            return Ok(None);
        }
        match input {
            MenuInput::Move(x, y) => {
                let hit = self.painter.hit(x, y);
                if let Some(i) = hit {
                    if self.has_children(i) {
                        self.open_sub(i)?;
                    } else if self.sub.is_some() && self.close_sub_at.is_none() {
                        self.close_sub_at = Some(now + SUB_CLOSE_DELAY_SECS);
                    }
                }
                if hit.is_some() && hit != self.main.hover {
                    self.main.hover = hit;
                    self.keys_in_sub = false;
                    self.redraw_main()?;
                }
            }
            MenuInput::Leave => {
                // 移到第二層去了：展開的那一列保持反白。
                let keep = self.sub_index();
                if self.main.hover != keep || self.main.pressed.is_some() {
                    self.main.hover = keep;
                    self.main.pressed = None;
                    self.redraw_main()?;
                }
            }
            MenuInput::Down(x, y) => {
                self.main.pressed = self.painter.hit(x, y);
                self.redraw_main()?;
            }
            MenuInput::Up(x, y) => {
                self.main.pressed = None;
                self.redraw_main()?;
                if let Some(i) = self.painter.hit(x, y) {
                    return self.choose_main(i);
                }
            }
            MenuInput::Key(vk) => return self.key(vk),
            MenuInput::Deactivate => return Ok(Some(Outcome::Close)),
        }
        Ok(None)
    }

    fn sub_input(&mut self, input: MenuInput) -> Result<Option<Outcome>> {
        self.close_sub_at = None;
        let Some(index) = self.sub_index() else { return Ok(None) };
        if self.main.hover != Some(index) {
            self.main.hover = Some(index);
            self.redraw_main()?;
        }
        let Some((_, level)) = self.sub.as_mut() else { return Ok(None) };
        let Some(p) = self.sub_painters.get(&index) else { return Ok(None) };
        let (hover, pressed) = (level.hover, level.pressed);
        match input {
            MenuInput::Move(x, y) => level.hover = p.hit(x, y),
            MenuInput::Leave => (level.hover, level.pressed) = (None, None),
            MenuInput::Down(x, y) => level.pressed = p.hit(x, y),
            MenuInput::Up(x, y) => {
                level.pressed = None;
                if let Some(i) = p.hit(x, y) {
                    return Ok(self.choose_sub(i));
                }
            }
            // 第二層拿不到焦點，按鍵與「點了外面」都是第一層收到的。
            MenuInput::Key(_) | MenuInput::Deactivate => {}
        }
        if (level.hover, level.pressed) != (hover, pressed) {
            self.redraw_sub()?;
        }
        Ok(None)
    }

    fn key(&mut self, vk: u16) -> Result<Option<Outcome>> {
        let is = |k: VIRTUAL_KEY| vk == k.0;
        if self.keys_in_sub
            && let Some(index) = self.sub_index()
        {
            if is(VK_DOWN) || is(VK_UP) {
                let from = self.sub.as_ref().and_then(|(_, l)| l.hover);
                let next = self.sub_painters.get(&index).and_then(|p| p.step(from, is(VK_DOWN)));
                if let Some((_, level)) = self.sub.as_mut() {
                    level.hover = next;
                }
                self.redraw_sub()?;
            } else if is(VK_RETURN) || is(VK_SPACE) {
                if let Some(i) = self.sub.as_ref().and_then(|(_, l)| l.hover) {
                    return Ok(self.choose_sub(i));
                }
            } else if is(VK_LEFT) || is(VK_BACK) || is(VK_ESCAPE) {
                self.close_sub();
                self.redraw_main()?;
            }
            return Ok(None);
        }
        if is(VK_ESCAPE) {
            return Ok(Some(Outcome::Close));
        }
        if is(VK_DOWN) || is(VK_UP) {
            self.close_sub();
            self.main.hover = self.painter.step(self.main.hover, is(VK_DOWN));
            self.redraw_main()?;
        } else if is(VK_RETURN) || is(VK_SPACE) || is(VK_RIGHT) {
            let Some(i) = self.main.hover else { return Ok(None) };
            if self.has_children(i) {
                self.open_sub(i)?;
                self.keys_in_sub = true;
                let first = self.sub_painters.get(&i).and_then(|p| p.step(None, true));
                if let Some((_, level)) = self.sub.as_mut() {
                    level.hover = first;
                }
                self.redraw_sub()?;
            } else if !is(VK_RIGHT) {
                return self.choose_main(i);
            }
        }
        Ok(None)
    }
}