//! 桌寵頭上的框：獨立的分層視窗，尾巴指向桌寵。分兩區，從桌寵頭頂往上長（誰放哪裡見 `card_stack`）：
//!
//! - 下面貼著桌寵：要按的框。後面還有別的就露出一兩層邊，右上角「+N」點了攤開成清單。
//! - 上面：會自己消失的框。右上角的 ✕ 外圈是倒數，像時鐘一樣慢慢填滿。
//!
//! 新的東西長在上面，下面那個框的按鈕位置不會跑掉；框剛換上來的一小段時間內按鈕按了不算（防按錯）。
//! 尺寸、顏色、圓角、字級、動畫全部來自設計參數（`theme`）。

use chrono::{NaiveDate, NaiveDateTime};
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
/// 清單型的卡（錯過的提醒、提示清單）最多列幾則，其餘收成「還有 N 則」。
pub const LIST_MAX_ROWS: usize = 4;
/// 講話泡泡：文字最寬多少、最多幾行（超過用刪節號）。
const SPEECH_MAX_TEXT_DIP: f32 = 220.0;
const SPEECH_MAX_LINES: f32 = 3.0;
/// 排平均時每次縮窄多少。
const SPEECH_BALANCE_STEP_DIP: f32 = 6.0;
const CARD_TIME_FORMAT: &str = "%H:%M";
const CARD_DATE_TIME_FORMAT: &str = "%-m/%-d %H:%M";
/// 上下兩區（與攤開的清單）之間的空隙。
const BLOCK_GAP_DIP: f32 = theme::SPACE_2;
/// 會自己消失的框右上角的 ✕：按鈕大小、離框邊多遠、✕ 本身多大、外圈倒數的線寬。
const CLOSE_DIP: f32 = 24.0;
const CLOSE_INSET_DIP: f32 = theme::SPACE_3;
const CLOSE_GLYPH_DIP: f32 = 14.0;
const RING_WIDTH_DIP: f32 = 2.0;
/// 倒數圈一圈分幾格重畫（一格 1/120 圈，15 秒的框每 0.125 秒畫一次，看起來是連續的）。
const RING_STEPS: f32 = 120.0;
/// 下面那區後面還有框時，露出幾層邊、每層往上多少、左右各縮多少。
const EDGES_MAX: usize = 2;
const EDGE_STEP_DIP: f32 = theme::SPACE_2;
const EDGE_INSET_DIP: f32 = theme::SPACE_3;
/// 「+N」小膠囊的高度（騎在框的上緣，一半在框外）。
const BADGE_HEIGHT_DIP: f32 = 20.0;
/// 攤開的清單：最多列幾則、每列多高、列首圖示多大、列尾 ✓ 多大。
const ROWS_MAX: usize = 5;
const ROW_HEIGHT_DIP: f32 = 32.0;
const ROW_GLYPH_DIP: f32 = 16.0;
const ROW_DONE_DIP: f32 = 24.0;
/// 框剛換上來多久內按鈕按了不算：滑鼠正移過去、還沒碰到，框就換掉時，不會按到新的那個。
const PRESS_GUARD_SECS: f64 = 0.5;

/// 卡片上「幾點響的」：今天的只寫時間，不是今天的連日期一起寫（卡片放到隔天才看到也分得出來）。
/// 卡片一跨日就重畫一次（核心在換日時把卡交回給丟出它的功能），所以 `today` 要傳顯示當下的日期。
pub fn card_time(at: NaiveDateTime, today: NaiveDate) -> String {
    at.format(if at.date() == today {
        CARD_TIME_FORMAT
    } else {
        CARD_DATE_TIME_FORMAT
    })
    .to_string()
}

/// 要按完成的卡（`needs_done`）由核心存進狀態檔，重開再跳，所以要能存讀。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Card {
    /// 確認框：「完成」「稍後」兩顆鈕。`icon`：左邊那格的圖示（一般提醒是鈴鐺、休息提醒是杯子）。
    /// 使用者的狀態檔裡可能寫的是 `Reminder`：拿掉 `alias` 那張沒按掉的卡重開就不會再跳。
    #[serde(alias = "Reminder")]
    Confirm {
        title: String,
        time: String,
        snooze_open: bool,
        icon: Icon,
    },
    /// 提示框：跟確認框同樣子但沒有按鈕，過一會兒自己消失、按 ✕ 也收掉。
    Notice {
        title: String,
        time: String,
        icon: Icon,
    },
    Missed {
        rows: Vec<(String, String)>,
        more: usize,
    },
    /// 提示清單：好幾則提示框合成一個，版面同錯過的提醒但沒有按鈕，跟提示框一樣自己消失。
    NoticeList {
        rows: Vec<(String, String)>,
        more: usize,
    },
    Info {
        title: String,
        detail: Option<String>,
        tone: Tone,
    },
    /// 新版下載好了：「現在更新」= `Done`，「下次啟動時」= `Dismiss`。
    Update { version: String },
    /// 桌寵講的話：沒有圖示、沒有按鈕，寬度跟著字數；點一下收掉（`Dismiss`）。
    Speech { text: String },
}

impl Card {
    /// 要使用者按掉才算數的卡（確認框、錯過的提醒）：沒按就關掉柚伴的話，重開再跳。
    /// 新版本的卡由更新那邊每次啟動重新判斷，不在這裡。
    pub fn needs_done(&self) -> bool {
        matches!(self, Card::Confirm { .. } | Card::Missed { .. })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Tone {
    Success,
    Warning,
}

impl Card {
    pub fn success(title: impl Into<String>, detail: Option<String>) -> Self {
        Card::Info {
            title: title.into(),
            detail,
            tone: Tone::Success,
        }
    }

    pub fn warning(title: impl Into<String>, detail: Option<String>) -> Self {
        Card::Info {
            title: title.into(),
            detail,
            tone: Tone::Warning,
        }
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

/// 畫面上的一則：`id` 換了 = 換成另一則（重播出場動畫、按鈕先擋一下）。
#[derive(Debug, Clone, PartialEq)]
pub struct Shown {
    pub id: u64,
    pub card: Card,
}

/// 頭上要畫的東西。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Scene {
    /// 上面那區：會自己消失的框。
    pub top: Option<Shown>,
    /// 上面那區倒數到哪裡（0～1）。
    pub progress: f32,
    /// 下面那區最前面那則（沒有框時放講話泡泡）。
    pub front: Option<Shown>,
    /// 疊在最前面那則後面的，照先後；攤開時列出來的就是這些。
    pub behind: Vec<Card>,
    /// 還在功能那邊排隊、沒放上來的則數（算進「+N」）。
    pub waiting: usize,
    pub expanded: bool,
}

impl Scene {
    fn more(&self) -> usize {
        self.behind.len() + self.waiting
    }

    fn has_stack(&self) -> bool {
        self.front.is_some() && self.more() > 0
    }

    fn is_speech(&self) -> bool {
        matches!(
            self.front,
            Some(Shown {
                card: Card::Speech { .. },
                ..
            })
        )
    }

    /// 除了倒數以外都一樣（只要重畫、不用重排）。
    fn same_layout(&self, other: &Scene) -> bool {
        Scene {
            progress: 0.0,
            ..self.clone()
        } == Scene {
            progress: 0.0,
            ..other.clone()
        }
    }
}

/// 使用者在框上按了什麼。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Press {
    /// 下面那區最前面那則的按鈕（講話泡泡點一下是 `Dismiss`）。
    Front(CardAction),
    /// 上面那區的 ✕。
    Top(CardAction),
    /// 「+N」：攤開／收起。
    Toggle,
    /// 攤開的清單點了第幾則（`Scene::behind` 的位置）：讓它當最前面。
    Pick(usize),
    /// 攤開的清單上第幾則的 ✓。
    Done(usize),
    /// 「還有 N 則」：打開設定看全部。
    More(More),
}

/// 「還有 N 則」要打開哪裡。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum More {
    RelayLog,
    Reminders,
}

/// 三塊各自的出場動畫與防按錯計時。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Zone {
    Top,
    Rows,
    Front,
}

const ZONES: usize = 3;

impl Zone {
    fn index(self) -> usize {
        self as usize
    }
}

impl Press {
    fn zone(self) -> Zone {
        match self {
            Press::Top(_) => Zone::Top,
            Press::More(More::RelayLog) => Zone::Top,
            Press::Pick(_) | Press::Done(_) => Zone::Rows,
            Press::Front(_) | Press::Toggle | Press::More(More::Reminders) => Zone::Front,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Look {
    Primary,
    Secondary,
    /// 上面那區的 ✕，外圈是倒數。
    Close,
    /// 清單列尾的 ✓。
    Check,
    /// 清單的一整列（滑過時底色變深）。
    Row,
    /// 「還有 N 則」。
    Link,
    /// 「+N」。
    Badge,
}

struct Button {
    rect: D2D_RECT_F,
    label: String,
    press: Press,
    look: Look,
}

struct Formats {
    scale: f32,
    caption: IDWriteTextFormat,
    body: IDWriteTextFormat,
    body_bold: IDWriteTextFormat,
    subtitle: IDWriteTextFormat,
    title: IDWriteTextFormat,
    button: IDWriteTextFormat,
    badge: IDWriteTextFormat,
}

#[derive(Clone, Copy)]
enum Ink {
    Text,
    Muted,
}

/// 一塊框（上面那區、攤開的清單、下面那區各一塊）。座標都是視窗裡的實體像素。
struct Block {
    zone: Zone,
    rect: D2D_RECT_F,
    texts: Vec<(IDWriteTextLayout, Vector2, Ink)>,
    tile: Option<(D2D_RECT_F, Icon, Tint)>,
    /// 清單列首的小圖示（左上角位置）。
    glyphs: Vec<(Vector2, Icon)>,
}

/// 排版結果（實體像素，座標相對於視窗左上角）。
struct Layout {
    width: i32,
    height: i32,
    /// 由上往下；最後一塊帶尾巴。
    blocks: Vec<Block>,
    /// 疊在下面那區後面露出來的邊（遠的先畫）。
    edges: Vec<D2D_RECT_F>,
    tail_x: f32,
    /// 卡片寬度（講話泡泡跟著字數變，其他固定）。
    card_w: f32,
    /// 找按鈕時從前面找起：小按鈕（✕、✓、+N）排在蓋住它的大區塊前面。
    buttons: Vec<Button>,
}

impl Layout {
    fn last(&self) -> &Block {
        self.blocks.last().expect("a layout has at least one block")
    }
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
        Ok(Self {
            canvas,
            gfx: gfx.clone(),
            brush,
            formats: None,
            layout: None,
            icons,
        })
    }

    fn formats(&mut self, s: f32) -> Result<&Formats> {
        if self.formats.as_ref().is_none_or(|f| f.scale != s) {
            self.icons.clear();
            let make = |size: f32, bold: bool, center: bool| -> Result<IDWriteTextFormat> {
                unsafe {
                    let f = self.gfx.dwrite.CreateTextFormat(
                        &HSTRING::from(theme::FONT_FAMILY),
                        None,
                        if bold {
                            DWRITE_FONT_WEIGHT_BOLD
                        } else {
                            DWRITE_FONT_WEIGHT_NORMAL
                        },
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
                badge: make(theme::FONT_SIZE_CAPTION, true, true)?,
            });
        }
        Ok(self.formats.as_ref().expect("just set"))
    }

    fn text(
        &self,
        text: &str,
        format: &IDWriteTextFormat,
        max_w: f32,
        max_lines: Option<f32>,
    ) -> Result<(IDWriteTextLayout, f32, f32)> {
        let wide: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            let line = format.GetFontSize() * 1.4;
            let max_h = max_lines.map_or(f32::MAX, |n| line * n);
            let layout = self
                .gfx
                .dwrite
                .CreateTextLayout(&wide, format, max_w, max_h)?;
            if max_lines.is_some() {
                let trimming = DWRITE_TRIMMING {
                    granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                    delimiter: 0,
                    delimiterCount: 0,
                };
                let sign = self.gfx.dwrite.CreateEllipsisTrimmingSign(format)?;
                layout.SetTrimming(&trimming, &sign)?;
            }
            let mut m = DWRITE_TEXT_METRICS::default();
            layout.GetMetrics(&mut m)?;
            Ok((
                layout,
                m.widthIncludingTrailingWhitespace,
                m.height.min(max_h),
            ))
        }
    }

    /// 排整個畫面；`tail_frac` 是尾巴在卡片寬度裡的位置（0～1，0.5 = 正中間）。
    fn layout_scene(&mut self, scene: &Scene, s: f32, tail_frac: f32) -> Result<()> {
        if let Some(Shown {
            card: Card::Speech { text },
            ..
        }) = &scene.front
        {
            return self.layout_speech(text, s, tail_frac);
        }
        self.formats(s)?;
        let margin = (theme::SHADOW_POPUP_BLUR * s).ceil();
        let card_w = CARD_WIDTH_DIP * s;
        let gap = BLOCK_GAP_DIP * s;
        let mut blocks = Vec::new();
        let mut buttons = Vec::new();
        let mut edges = Vec::new();
        let mut y = margin;

        if let Some(top) = &scene.top {
            let inset = CLOSE_INSET_DIP * s;
            let close = CLOSE_DIP * s;
            // 標題右邊讓出 ✕ 的位置（框內留白之外還要再讓多少）。
            let reserve = (close + inset - theme::SPACE_4 * s + theme::SPACE_1 * s).max(0.0);
            let mut own = Vec::new();
            let block = self.card_block(&top.card, s, margin, y, Zone::Top, reserve, &mut own)?;
            buttons.push(Button {
                rect: D2D_RECT_F {
                    left: block.rect.right - inset - close,
                    top: block.rect.top + inset,
                    right: block.rect.right - inset,
                    bottom: block.rect.top + inset + close,
                },
                label: String::new(),
                press: Press::Top(CardAction::Dismiss),
                look: Look::Close,
            });
            buttons.extend(own);
            y = block.rect.bottom + gap;
            blocks.push(block);
        }

        if let Some(front) = &scene.front {
            let stack = scene.has_stack();
            if stack && scene.expanded {
                let block = self.rows_block(scene, s, margin, y, &mut buttons)?;
                y = block.rect.bottom + gap;
                blocks.push(block);
            }
            let edge_count = if stack && !scene.expanded {
                scene.more().min(EDGES_MAX)
            } else {
                0
            };
            if stack {
                y += (edge_count as f32 * EDGE_STEP_DIP * s).max(BADGE_HEIGHT_DIP * s / 2.0);
            }
            let mut own = Vec::new();
            let block = self.card_block(&front.card, s, margin, y, Zone::Front, 0.0, &mut own)?;
            for k in (1..=edge_count).rev() {
                let (dy, dx) = (k as f32 * EDGE_STEP_DIP * s, k as f32 * EDGE_INSET_DIP * s);
                edges.push(D2D_RECT_F {
                    left: block.rect.left + dx,
                    top: block.rect.top - dy,
                    right: block.rect.right - dx,
                    bottom: block.rect.bottom - dy,
                });
            }
            if stack {
                let label = format!("+{}", scene.more());
                let badge_font = self.formats.as_ref().expect("formats ready").badge.clone();
                let (_, text_w, _) = self.text(&label, &badge_font, card_w, Some(1.0))?;
                let h = BADGE_HEIGHT_DIP * s;
                let w = (text_w + 2.0 * theme::SPACE_2 * s).max(h * 1.5);
                let right = block.rect.right - theme::SPACE_4 * s;
                buttons.push(Button {
                    rect: D2D_RECT_F {
                        left: right - w,
                        top: block.rect.top - h / 2.0,
                        right,
                        bottom: block.rect.top + h / 2.0,
                    },
                    label,
                    press: Press::Toggle,
                    look: Look::Badge,
                });
            }
            buttons.extend(own);
            blocks.push(block);
        }

        let bottom = blocks.last().map_or(margin, |b| b.rect.bottom);
        let corner = theme::RADIUS_LG * s + TAIL_WIDTH_DIP * s / 2.0;
        let tail_x = (margin + card_w * tail_frac).clamp(margin + corner, margin + card_w - corner);
        self.layout = Some(Layout {
            width: (card_w + 2.0 * margin).ceil() as i32,
            height: (bottom + TAIL_HEIGHT_DIP * s + margin).ceil() as i32,
            blocks,
            edges,
            tail_x,
            card_w,
            buttons,
        });
        Ok(())
    }

    /// 一個框；左上角在 (`x0`, `y0`)。版型：左上角一個圓形圖示，文字排在它右邊，按鈕橫跨整個框的寬度。
    /// `reserve`：標題右邊再讓出多寬（給 ✕）。
    #[allow(clippy::too_many_arguments)]
    fn card_block(
        &mut self,
        card: &Card,
        s: f32,
        x0: f32,
        y0: f32,
        zone: Zone,
        reserve: f32,
        buttons: &mut Vec<Button>,
    ) -> Result<Block> {
        let pad = theme::SPACE_4 * s;
        let card_w = CARD_WIDTH_DIP * s;
        let inner_w = card_w - 2.0 * pad;
        let left = x0 + pad;
        let top = y0 + pad;
        let tile_size = TILE_DIP * s;
        let text_left = left + tile_size + theme::SPACE_3 * s;
        let text_w = inner_w - tile_size - theme::SPACE_3 * s - reserve;
        let tile_rect = D2D_RECT_F {
            left,
            top,
            right: left + tile_size,
            bottom: top + tile_size,
        };
        let mut y = top;
        let mut texts = Vec::new();
        let f = self.formats.as_ref().expect("formats ready");
        let (caption, body, body_bold, subtitle, title_fmt) = (
            f.caption.clone(),
            f.body.clone(),
            f.body_bold.clone(),
            f.subtitle.clone(),
            f.title.clone(),
        );
        let wrap = |action: CardAction| match zone {
            Zone::Top => Press::Top(action),
            _ => Press::Front(action),
        };

        let button_row =
            |y: f32, items: Vec<(String, CardAction, bool)>, buttons: &mut Vec<Button>| {
                let gap = theme::SPACE_2 * s;
                let n = items.len() as f32;
                let w = (inner_w - gap * (n - 1.0)) / n;
                for (i, (label, action, primary)) in items.into_iter().enumerate() {
                    let x = left + i as f32 * (w + gap);
                    buttons.push(Button {
                        rect: D2D_RECT_F {
                            left: x,
                            top: y,
                            right: x + w,
                            bottom: y + BUTTON_HEIGHT_DIP * s,
                        },
                        label,
                        press: wrap(action),
                        look: if primary {
                            Look::Primary
                        } else {
                            Look::Secondary
                        },
                    });
                }
            };

        // 清單型的卡：一列一則，左邊名稱、右邊時間，超過的收成「還有 N 則」（點了打開設定看全部）；回傳排到哪裡。
        let list_rows = |rows: &[(String, String)],
                         more: usize,
                         link: More,
                         mut y: f32,
                         texts: &mut Vec<(IDWriteTextLayout, Vector2, Ink)>,
                         buttons: &mut Vec<Button>|
         -> Result<f32> {
            for (name, when) in rows {
                let (w_layout, w_w, w_h) = self.text(when, &caption, inner_w / 2.0, Some(1.0))?;
                let (n_layout, _, n_h) =
                    self.text(name, &body, inner_w - w_w - theme::SPACE_3 * s, Some(1.0))?;
                let row_h = n_h.max(w_h);
                texts.push((n_layout, Vector2 { X: left, Y: y }, Ink::Text));
                texts.push((
                    w_layout,
                    Vector2 {
                        X: left + inner_w - w_w,
                        Y: y + (row_h - w_h) / 2.0,
                    },
                    Ink::Muted,
                ));
                y += row_h + theme::SPACE_1 * s;
            }
            if more > 0 {
                let label = format!("還有 {more} 則");
                let (_, w, h) = self.text(&label, &caption, inner_w, Some(1.0))?;
                buttons.push(Button {
                    rect: D2D_RECT_F {
                        left,
                        top: y,
                        right: left + w,
                        bottom: y + h,
                    },
                    label,
                    press: Press::More(link),
                    look: Look::Link,
                });
                y += h;
            }
            Ok(y)
        };

        let tile = match card {
            Card::Confirm {
                title,
                time,
                snooze_open,
                icon,
            } => {
                let (t, _, h) = self.text(time, &caption, text_w, Some(1.0))?;
                texts.push((t, Vector2 { X: text_left, Y: y }, Ink::Muted));
                y += h;
                let (t, _, h) = self.text(title, &title_fmt, text_w, Some(TITLE_MAX_LINES))?;
                texts.push((t, Vector2 { X: text_left, Y: y }, Ink::Text));
                y = (y + h).max(tile_rect.bottom) + theme::SPACE_4 * s;
                let items = if *snooze_open {
                    SNOOZE_CHOICES
                        .iter()
                        .map(|(m, l)| (l.to_string(), CardAction::Snooze(*m), false))
                        .collect()
                } else {
                    vec![
                        ("完成".to_string(), CardAction::Done, true),
                        ("稍後".to_string(), CardAction::OpenSnooze, false),
                    ]
                };
                button_row(y, items, buttons);
                y += BUTTON_HEIGHT_DIP * s;
                (*icon, Tint::Accent)
            }
            Card::Notice { title, time, icon } => {
                let (t, _, h) = self.text(time, &caption, text_w, Some(1.0))?;
                texts.push((t, Vector2 { X: text_left, Y: y }, Ink::Muted));
                y += h;
                let (t, _, h) = self.text(title, &title_fmt, text_w, Some(TITLE_MAX_LINES))?;
                texts.push((t, Vector2 { X: text_left, Y: y }, Ink::Text));
                y = (y + h).max(tile_rect.bottom);
                (*icon, Tint::Accent)
            }
            Card::Missed { rows, more } => {
                let (t, _, h) = self.text("錯過的提醒", &subtitle, text_w, Some(1.0))?;
                texts.push((
                    t,
                    Vector2 {
                        X: text_left,
                        Y: y + (tile_size - h) / 2.0,
                    },
                    Ink::Text,
                ));
                y = tile_rect.bottom + theme::SPACE_3 * s;
                y = list_rows(rows, *more, More::Reminders, y, &mut texts, buttons)?;
                y += theme::SPACE_3 * s;
                button_row(
                    y,
                    vec![("全部完成".to_string(), CardAction::Dismiss, true)],
                    buttons,
                );
                y += BUTTON_HEIGHT_DIP * s;
                (Icon::History, Tint::Accent)
            }
            Card::NoticeList { rows, more } => {
                let count = rows.len() + more;
                let (t, _, h) = self.text(
                    &format!("有 {count} 件事要跟你說喔～"),
                    &subtitle,
                    text_w,
                    Some(1.0),
                )?;
                texts.push((
                    t,
                    Vector2 {
                        X: text_left,
                        Y: y + (tile_size - h) / 2.0,
                    },
                    Ink::Text,
                ));
                y = tile_rect.bottom + theme::SPACE_3 * s;
                y = list_rows(rows, *more, More::RelayLog, y, &mut texts, buttons)?;
                (Icon::MessageDots, Tint::Accent)
            }
            Card::Update { version } => {
                let (t, _, h) = self.text(
                    &format!("新版 {version} 準備好了"),
                    &body_bold,
                    text_w,
                    Some(TITLE_MAX_LINES),
                )?;
                texts.push((
                    t,
                    Vector2 {
                        X: text_left,
                        Y: y + ((tile_size - h) / 2.0).max(0.0),
                    },
                    Ink::Text,
                ));
                y = (y + h).max(tile_rect.bottom) + theme::SPACE_4 * s;
                button_row(
                    y,
                    vec![
                        ("現在更新".to_string(), CardAction::Done, true),
                        ("下次啟動時".to_string(), CardAction::Dismiss, false),
                    ],
                    buttons,
                );
                y += BUTTON_HEIGHT_DIP * s;
                (Icon::Download, Tint::Accent)
            }
            Card::Speech { .. } => unreachable!("laid out by layout_speech"),
            Card::Info {
                title,
                detail,
                tone,
            } => {
                let (t, _, title_h) =
                    self.text(title, &body_bold, text_w, Some(TITLE_MAX_LINES))?;
                let detail_layout = match detail {
                    Some(d) => Some(self.text(d, &caption, text_w, Some(TITLE_MAX_LINES))?),
                    None => None,
                };
                let gap = theme::SPACE_1 * s;
                let block_h = title_h + detail_layout.as_ref().map_or(0.0, |(_, _, h)| gap + h);
                // 文字比圖示矮時，整塊文字跟圖示垂直置中。
                let mut ty = y + ((tile_size - block_h) / 2.0).max(0.0);
                texts.push((
                    t,
                    Vector2 {
                        X: text_left,
                        Y: ty,
                    },
                    Ink::Text,
                ));
                ty += title_h;
                if let Some((dl, _, dh)) = detail_layout {
                    ty += gap;
                    texts.push((
                        dl,
                        Vector2 {
                            X: text_left,
                            Y: ty,
                        },
                        Ink::Muted,
                    ));
                    ty += dh;
                }
                y = ty.max(tile_rect.bottom);
                match tone {
                    Tone::Success => (Icon::CircleCheck, Tint::Success),
                    Tone::Warning => (Icon::Alert, Tint::Warning),
                }
            }
        };
        Ok(Block {
            zone,
            rect: D2D_RECT_F {
                left: x0,
                top: y0,
                right: x0 + card_w,
                bottom: y + pad,
            },
            texts,
            tile: Some((tile_rect, tile.0, tile.1)),
            glyphs: Vec::new(),
        })
    }

    /// 攤開的清單：疊在後面的每則一列（圖示、標題、時間，確認框列尾有 ✓），點一列讓它當最前面。
    fn rows_block(
        &mut self,
        scene: &Scene,
        s: f32,
        x0: f32,
        y0: f32,
        buttons: &mut Vec<Button>,
    ) -> Result<Block> {
        let pad = theme::SPACE_2 * s;
        let card_w = CARD_WIDTH_DIP * s;
        let (left, right) = (x0 + pad, x0 + card_w - pad);
        let f = self.formats.as_ref().expect("formats ready");
        let (caption, body) = (f.caption.clone(), f.body.clone());
        let row_h = ROW_HEIGHT_DIP * s;
        let glyph = ROW_GLYPH_DIP * s;
        let check = ROW_DONE_DIP * s;
        let inner = theme::SPACE_2 * s;
        let mut texts = Vec::new();
        let mut glyphs = Vec::new();
        let mut y = y0 + pad;
        let shown = scene.behind.len().min(ROWS_MAX);
        for (i, card) in scene.behind.iter().take(shown).enumerate() {
            let (icon, title, time, can_done) = row_parts(card);
            let mut text_right = right - inner;
            if can_done {
                let rect = D2D_RECT_F {
                    left: right - inner / 2.0 - check,
                    top: y + (row_h - check) / 2.0,
                    right: right - inner / 2.0,
                    bottom: y + (row_h + check) / 2.0,
                };
                text_right = rect.left - theme::SPACE_1 * s;
                buttons.push(Button {
                    rect,
                    label: String::new(),
                    press: Press::Done(i),
                    look: Look::Check,
                });
            }
            buttons.push(Button {
                rect: D2D_RECT_F {
                    left,
                    top: y,
                    right,
                    bottom: y + row_h,
                },
                label: String::new(),
                press: Press::Pick(i),
                look: Look::Row,
            });
            glyphs.push((
                Vector2 {
                    X: left + inner,
                    Y: y + (row_h - glyph) / 2.0,
                },
                icon,
            ));
            let text_left = left + inner + glyph + theme::SPACE_2 * s;
            let (t_layout, t_w, t_h) =
                self.text(&time, &caption, (text_right - text_left) / 2.0, Some(1.0))?;
            let gap = if t_w > 0.0 { theme::SPACE_2 * s } else { 0.0 };
            let (n_layout, _, n_h) = self.text(
                &title,
                &body,
                (text_right - text_left - t_w - gap).max(0.0),
                Some(1.0),
            )?;
            texts.push((
                n_layout,
                Vector2 {
                    X: text_left,
                    Y: y + (row_h - n_h) / 2.0,
                },
                Ink::Text,
            ));
            texts.push((
                t_layout,
                Vector2 {
                    X: text_right - t_w,
                    Y: y + (row_h - t_h) / 2.0,
                },
                Ink::Muted,
            ));
            y += row_h;
        }
        let rest = scene.more() - shown;
        if rest > 0 {
            let (t, _, h) = self.text(
                &format!("還有 {rest} 則"),
                &caption,
                right - left - 2.0 * inner,
                Some(1.0),
            )?;
            y += theme::SPACE_1 * s;
            texts.push((
                t,
                Vector2 {
                    X: left + inner,
                    Y: y,
                },
                Ink::Muted,
            ));
            y += h + theme::SPACE_1 * s;
        }
        Ok(Block {
            zone: Zone::Rows,
            rect: D2D_RECT_F {
                left: x0,
                top: y0,
                right: x0 + card_w,
                bottom: y + pad,
            },
            texts,
            tile: None,
            glyphs,
        })
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
        let at = Vector2 {
            X: margin + (card_w - w) / 2.0,
            Y: margin + pad_y,
        };
        let bottom = margin + pad_y + h + pad_y;
        let corner = theme::RADIUS_LG * s + TAIL_WIDTH_DIP * s / 2.0;
        let tail_x = (margin + card_w * tail_frac).clamp(margin + corner, margin + card_w - corner);
        self.layout = Some(Layout {
            width: (card_w + 2.0 * margin).ceil() as i32,
            height: (bottom + TAIL_HEIGHT_DIP * s + margin).ceil() as i32,
            blocks: vec![Block {
                zone: Zone::Front,
                rect: D2D_RECT_F {
                    left: margin,
                    top: margin,
                    right: margin + card_w,
                    bottom,
                },
                texts: vec![(layout, at, Ink::Text)],
                tile: None,
                glyphs: Vec::new(),
            }],
            edges: Vec::new(),
            tail_x,
            card_w,
            buttons: Vec::new(),
        });
        Ok(())
    }

    /// 尾巴尖端在視窗裡的位置。
    fn tail_tip(&self, s: f32) -> Vector2 {
        let l = self.layout.as_ref().expect("layout first");
        Vector2 {
            X: l.tail_x,
            Y: l.last().rect.bottom + TAIL_HEIGHT_DIP * s,
        }
    }

    fn hit(&self, x: f32, y: f32) -> Option<usize> {
        let l = self.layout.as_ref()?;
        l.buttons.iter().position(|b| {
            x >= b.rect.left && x <= b.rect.right && y >= b.rect.top && y <= b.rect.bottom
        })
    }

    fn inside_card(&self, x: f32, y: f32) -> bool {
        self.layout.as_ref().is_some_and(|l| {
            l.blocks.iter().any(|b| {
                x >= b.rect.left && x <= b.rect.right && y >= b.rect.top && y <= b.rect.bottom
            })
        })
    }

    /// `appear`：三塊各自的出場大小（`Zone` 的順序）；`progress`：上面那區的倒數。
    fn paint(
        &mut self,
        palette: &Palette,
        s: f32,
        hover: Option<usize>,
        pressed: Option<usize>,
        appear: [f32; ZONES],
        progress: f32,
    ) -> Result<()> {
        let tip = self.tail_tip(s);
        let Self {
            canvas,
            gfx,
            brush,
            formats,
            layout,
            icons,
        } = self;
        let l = layout.as_ref().expect("layout first");
        let f = formats.as_ref().expect("formats ready");
        let (button_font, badge_font, caption) =
            (f.button.clone(), f.badge.clone(), f.caption.clone());
        canvas.ensure_size(l.width, l.height)?;
        let rt = &canvas.rt;
        let radius = theme::RADIUS_LG * s;
        let tail_path = tail_path(gfx, l, s)?;
        let last = l.blocks.len() - 1;
        // 每一塊從自己的底邊中間彈出來；帶尾巴的那塊從尾巴尖端。
        let anchor = |b: &Block, i: usize| {
            if i == last {
                tip
            } else {
                Vector2 {
                    X: (b.rect.left + b.rect.right) / 2.0,
                    Y: b.rect.bottom,
                }
            }
        };
        let transform_of = |zone: Zone| -> Matrix3x2 {
            l.blocks
                .iter()
                .enumerate()
                .find(|(_, b)| b.zone == zone)
                .map_or(Matrix3x2::identity(), |(i, b)| {
                    let k = appear[zone.index()];
                    Matrix3x2::scale_around(k, k, anchor(b, i))
                })
        };
        let state = |i: usize| (pressed == Some(i), hover == Some(i));
        unsafe {
            rt.BeginDraw();
            rt.Clear(Some(&TRANSPARENT));
            for (bi, block) in l.blocks.iter().enumerate() {
                let transform = transform_of(block.zone);
                rt.SetTransform(&transform);
                if block.zone == Zone::Front {
                    for edge in &l.edges {
                        popup_shadow(rt, brush, *edge, radius, s, palette);
                        let r = D2D1_ROUNDED_RECT {
                            rect: *edge,
                            radiusX: radius,
                            radiusY: radius,
                        };
                        brush.SetColor(&palette.surface_raised.d2d());
                        rt.FillRoundedRectangle(&r, &*brush);
                        brush.SetColor(&palette.border.d2d());
                        rt.DrawRoundedRectangle(&r, &*brush, s, None);
                    }
                }
                popup_shadow(rt, brush, block.rect, radius, s, palette);
                let card = D2D1_ROUNDED_RECT {
                    rect: block.rect,
                    radiusX: radius,
                    radiusY: radius,
                };
                brush.SetColor(&palette.surface_raised.d2d());
                rt.FillRoundedRectangle(&card, &*brush);
                brush.SetColor(&palette.border.d2d());
                rt.DrawRoundedRectangle(&card, &*brush, s, None);
                if bi == last {
                    brush.SetColor(&palette.surface_raised.d2d());
                    rt.FillGeometry(&tail_path, &*brush, None);
                    brush.SetColor(&palette.border.d2d());
                    rt.DrawGeometry(&tail_path, &*brush, s, None);
                    // 蓋掉尾巴與卡片接縫處的那段邊框。
                    brush.SetColor(&palette.surface_raised.d2d());
                    let tw = TAIL_WIDTH_DIP * s / 2.0 - s;
                    rt.FillRectangle(
                        &D2D_RECT_F {
                            left: l.tail_x - tw,
                            top: block.rect.bottom - s * 1.5,
                            right: l.tail_x + tw,
                            bottom: block.rect.bottom + s * 0.5,
                        },
                        &*brush,
                    );
                }
                // 清單列滑過的底色在字的下面。
                for (i, b) in l.buttons.iter().enumerate() {
                    if b.look != Look::Row || b.press.zone() != block.zone {
                        continue;
                    }
                    let fill = match state(i) {
                        (true, _) => Some(palette.border),
                        (false, true) => Some(palette.surface_sunken),
                        _ => None,
                    };
                    if let Some(fill) = fill {
                        let r = theme::RADIUS_SM * s;
                        brush.SetColor(&fill.d2d());
                        rt.FillRoundedRectangle(
                            &D2D1_ROUNDED_RECT {
                                rect: b.rect,
                                radiusX: r,
                                radiusY: r,
                            },
                            &*brush,
                        );
                    }
                }
                if let Some((rect, glyph, tint)) = block.tile {
                    let r = (rect.right - rect.left) / 2.0;
                    let center = Vector2 {
                        X: rect.left + r,
                        Y: rect.top + r,
                    };
                    brush.SetColor(&tint.color(palette).with_alpha(TILE_TINT_ALPHA));
                    rt.FillEllipse(
                        &D2D1_ELLIPSE {
                            point: center,
                            radiusX: r,
                            radiusY: r,
                        },
                        &*brush,
                    );
                    let size = GLYPH_DIP * s;
                    if let Some(doc) = icons.doc(glyph, tint.color(palette), size) {
                        let at =
                            Matrix3x2::translation(center.X - size / 2.0, center.Y - size / 2.0);
                        icons.draw(&doc, &(at * transform));
                        rt.SetTransform(&transform);
                    }
                }
                for (at, glyph) in &block.glyphs {
                    let size = ROW_GLYPH_DIP * s;
                    if let Some(doc) = icons.doc(*glyph, palette.accent, size) {
                        icons.draw(&doc, &(Matrix3x2::translation(at.X, at.Y) * transform));
                        rt.SetTransform(&transform);
                    }
                }
                for (layout, at, ink) in &block.texts {
                    brush.SetColor(&match ink {
                        Ink::Text => palette.text.d2d(),
                        Ink::Muted => palette.text_muted.d2d(),
                    });
                    rt.DrawTextLayout(*at, layout, &*brush, D2D1_DRAW_TEXT_OPTIONS_CLIP);
                }
            }
            for (i, b) in l.buttons.iter().enumerate() {
                let transform = transform_of(b.press.zone());
                rt.SetTransform(&transform);
                let (down, over) = state(i);
                let center = Vector2 {
                    X: (b.rect.left + b.rect.right) / 2.0,
                    Y: (b.rect.top + b.rect.bottom) / 2.0,
                };
                let r = (b.rect.right - b.rect.left) / 2.0;
                let label: Vec<u16> = b.label.encode_utf16().collect();
                match b.look {
                    Look::Primary | Look::Secondary => {
                        let (fill, ink) = match (b.look == Look::Primary, down, over) {
                            (true, true, _) => (palette.accent_pressed, palette.on_accent),
                            (true, false, true) => (palette.accent_hover, palette.on_accent),
                            (true, false, false) => (palette.accent, palette.on_accent),
                            (false, true, _) => (palette.border_strong, palette.text),
                            (false, false, true) => (palette.border, palette.text),
                            (false, false, false) => (palette.surface_sunken, palette.text),
                        };
                        let r = theme::RADIUS_SM * s;
                        brush.SetColor(&fill.d2d());
                        rt.FillRoundedRectangle(
                            &D2D1_ROUNDED_RECT {
                                rect: b.rect,
                                radiusX: r,
                                radiusY: r,
                            },
                            &*brush,
                        );
                        brush.SetColor(&ink.d2d());
                        rt.DrawText(
                            &label,
                            &button_font,
                            &b.rect,
                            &*brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                    }
                    Look::Badge => {
                        let fill = match (down, over) {
                            (true, _) => palette.accent_pressed,
                            (false, true) => palette.accent_hover,
                            _ => palette.accent,
                        };
                        let h = b.rect.bottom - b.rect.top;
                        brush.SetColor(&fill.d2d());
                        rt.FillRoundedRectangle(
                            &D2D1_ROUNDED_RECT {
                                rect: b.rect,
                                radiusX: h / 2.0,
                                radiusY: h / 2.0,
                            },
                            &*brush,
                        );
                        // 框的底色描一圈，跟後面露出的邊分開。
                        brush.SetColor(&palette.surface_raised.d2d());
                        rt.DrawRoundedRectangle(
                            &D2D1_ROUNDED_RECT {
                                rect: b.rect,
                                radiusX: h / 2.0,
                                radiusY: h / 2.0,
                            },
                            &*brush,
                            s * 1.5,
                            None,
                        );
                        brush.SetColor(&palette.on_accent.d2d());
                        rt.DrawText(
                            &label,
                            &badge_font,
                            &b.rect,
                            &*brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                    }
                    Look::Link => {
                        brush.SetColor(
                            &match (down, over) {
                                (true, _) => palette.accent_pressed,
                                (false, true) => palette.accent_hover,
                                _ => palette.accent,
                            }
                            .d2d(),
                        );
                        rt.DrawText(
                            &label,
                            &caption,
                            &b.rect,
                            &*brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                    }
                    Look::Close | Look::Check => {
                        let fill = match (down, over) {
                            (true, _) => Some(palette.border),
                            (false, true) => Some(palette.surface_sunken),
                            _ => None,
                        };
                        if let Some(fill) = fill {
                            brush.SetColor(&fill.d2d());
                            rt.FillEllipse(
                                &D2D1_ELLIPSE {
                                    point: center,
                                    radiusX: r,
                                    radiusY: r,
                                },
                                &*brush,
                            );
                        }
                        let (glyph, size, color) = if b.look == Look::Close {
                            let ring_r = r - RING_WIDTH_DIP * s / 2.0;
                            brush.SetColor(&palette.border.d2d());
                            rt.DrawEllipse(
                                &D2D1_ELLIPSE {
                                    point: center,
                                    radiusX: ring_r,
                                    radiusY: ring_r,
                                },
                                &*brush,
                                RING_WIDTH_DIP * s,
                                None,
                            );
                            if progress > 0.0 {
                                brush.SetColor(&palette.accent.d2d());
                                let arc = ring_arc(gfx, center, ring_r, progress)?;
                                rt.DrawGeometry(&arc, &*brush, RING_WIDTH_DIP * s, None);
                            }
                            let ink = if over {
                                palette.text
                            } else {
                                palette.text_muted
                            };
                            (Icon::Close, CLOSE_GLYPH_DIP * s, ink)
                        } else {
                            (Icon::Check, ROW_GLYPH_DIP * s, palette.accent)
                        };
                        if let Some(doc) = icons.doc(glyph, color, size) {
                            let at = Matrix3x2::translation(
                                center.X - size / 2.0,
                                center.Y - size / 2.0,
                            );
                            icons.draw(&doc, &(at * transform));
                        }
                    }
                    Look::Row => {}
                }
            }
            rt.SetTransform(&Matrix3x2::identity());
            rt.EndDraw(None, None)
        }
    }

    /// 截圖工具用：排版並畫一個畫面（不開視窗）。`hover`：滑過第幾顆按鈕（找按鈕的順序）。
    pub fn render(
        &mut self,
        scene: &Scene,
        palette: &Palette,
        s: f32,
        hover: Option<usize>,
        pressed: Option<usize>,
    ) -> Result<(i32, i32)> {
        self.layout_scene(scene, s, 0.5)?;
        self.paint(palette, s, hover, pressed, [1.0; ZONES], scene.progress)?;
        Ok(self.canvas.size())
    }
}

/// 清單上一列要寫什麼：(圖示, 標題, 時間, 能不能直接按完成)。
fn row_parts(card: &Card) -> (Icon, String, String, bool) {
    match card {
        Card::Confirm {
            title, time, icon, ..
        } => (*icon, title.clone(), time.clone(), true),
        Card::Missed { rows, more } => (
            Icon::History,
            "錯過的提醒".into(),
            format!("{} 則", rows.len() + more),
            false,
        ),
        Card::Update { version } => (
            Icon::Download,
            format!("新版 {version} 準備好了"),
            String::new(),
            false,
        ),
        Card::Notice { title, time, icon } => (*icon, title.clone(), time.clone(), false),
        Card::NoticeList { rows, more } => (
            Icon::MessageDots,
            format!("有 {} 件事要跟你說喔～", rows.len() + more),
            String::new(),
            false,
        ),
        Card::Info { title, .. } => (Icon::CircleCheck, title.clone(), String::new(), false),
        Card::Speech { text } => (Icon::MessageDots, text.clone(), String::new(), false),
    }
}

fn tail_path(gfx: &Gfx, l: &Layout, s: f32) -> Result<ID2D1PathGeometry> {
    let half = TAIL_WIDTH_DIP * s / 2.0;
    let bottom = l.last().rect.bottom;
    let top = bottom - s * 0.5;
    unsafe {
        let path = gfx.d2d.CreatePathGeometry()?;
        let sink = path.Open()?;
        sink.BeginFigure(
            Vector2 {
                X: l.tail_x - half,
                Y: top,
            },
            D2D1_FIGURE_BEGIN_FILLED,
        );
        sink.AddLine(Vector2 {
            X: l.tail_x,
            Y: bottom + TAIL_HEIGHT_DIP * s,
        });
        sink.AddLine(Vector2 {
            X: l.tail_x + half,
            Y: top,
        });
        sink.EndFigure(D2D1_FIGURE_END_OPEN);
        sink.Close()?;
        Ok(path)
    }
}

/// 倒數圈：從 12 點鐘方向順時針畫到 `progress`（0～1）那裡。
fn ring_arc(gfx: &Gfx, center: Vector2, r: f32, progress: f32) -> Result<ID2D1PathGeometry> {
    let angle = progress.min(0.9999) * std::f32::consts::TAU;
    unsafe {
        let path = gfx.d2d.CreatePathGeometry()?;
        let sink = path.Open()?;
        sink.BeginFigure(
            Vector2 {
                X: center.X,
                Y: center.Y - r,
            },
            D2D1_FIGURE_BEGIN_HOLLOW,
        );
        sink.AddArc(&D2D1_ARC_SEGMENT {
            point: Vector2 {
                X: center.X + r * angle.sin(),
                Y: center.Y - r * angle.cos(),
            },
            size: D2D_SIZE_F {
                width: r,
                height: r,
            },
            rotationAngle: 0.0,
            sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
            arcSize: if angle > std::f32::consts::PI {
                D2D1_ARC_SIZE_LARGE
            } else {
                D2D1_ARC_SIZE_SMALL
            },
        });
        sink.EndFigure(D2D1_FIGURE_END_OPEN);
        sink.Close()?;
        Ok(path)
    }
}

/// 浮在桌面上的東西（卡片、右鍵選單）底下的柔和陰影。畫面要先留 `SHADOW_POPUP_BLUR` 的邊。
/// 一層層往外擴的圓角矩形疊出來。每層的透明度算成「離邊緣 d 的地方，
/// 疊起來的濃度 = A × (1 - d/blur)²」，近邊緣濃、往外平滑淡掉，不會出現一圈硬邊。
pub fn popup_shadow(
    rt: &ID2D1RenderTarget,
    brush: &ID2D1SolidColorBrush,
    rect: D2D_RECT_F,
    radius: f32,
    s: f32,
    palette: &Palette,
) {
    let blur = theme::SHADOW_POPUP_BLUR * s;
    let offset = theme::SHADOW_POPUP_Y * s * 0.5;
    let density = |d: f32| theme::SHADOW_POPUP_ALPHA * (1.0 - d / blur).max(0.0).powi(2);
    for i in (1..=SHADOW_LAYERS).rev() {
        let grow = blur * i as f32 / SHADOW_LAYERS as f32;
        let inner = blur * (i - 1) as f32 / SHADOW_LAYERS as f32;
        let r = D2D_RECT_F {
            left: rect.left - grow,
            top: rect.top - grow * 0.5 + offset,
            right: rect.right + grow,
            bottom: rect.bottom + grow + offset,
        };
        unsafe {
            brush.SetColor(&palette.shadow.with_alpha(density(inner) - density(grow)));
            rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: r,
                    radiusX: radius + grow,
                    radiusY: radius + grow,
                },
                brush,
            );
        }
    }
}

pub struct Bubble {
    hwnd: HWND,
    painter: CardPainter,
    scene: Option<Scene>,
    /// 整個視窗什麼時候出現的（淡入用）。
    shown_at: f64,
    /// 三塊各自換上目前這則的時間（出場動畫、防按錯）。
    since: [f64; ZONES],
    /// 最近一次 `update` 的時間（按鈕放開時判斷防按錯用）。
    now: f64,
    /// 上一次畫的時候，最新的那塊出現了多久（毫秒）。
    painted_age_ms: f32,
    hover: Option<usize>,
    pressed: Option<usize>,
    /// 滑鼠在框上（核心據此停住倒數、不換下面那區的框）。
    inside: bool,
    /// 要重新排版。
    dirty: bool,
    /// 只要重畫（倒數圈走了一格、滑過換了按鈕）。
    repaint: bool,
    ring_step: i32,
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
            scene: None,
            shown_at: 0.0,
            since: [0.0; ZONES],
            now: 0.0,
            painted_age_ms: f32::MAX,
            hover: None,
            pressed: None,
            inside: false,
            dirty: false,
            repaint: false,
            ring_step: -1,
            scale: 1.0,
            last_origin: POINT {
                x: i32::MIN,
                y: i32::MIN,
            },
            last_tail_frac: -1.0,
            fade: 1.0,
        })
    }

    /// 淡出進度（1 = 完全不透明）；講話泡泡快到時間時用。
    pub fn set_fade(&mut self, fade: f32) {
        let fade = fade.clamp(0.0, 1.0);
        if (fade - self.fade).abs() > 1e-3 {
            self.fade = fade;
            self.repaint = true;
        }
    }

    /// 滑鼠停在框上。
    pub fn hovered(&self) -> bool {
        self.inside && self.scene.is_some()
    }

    /// 換上新的畫面；跟現在一樣就什麼都不做。
    pub fn set_scene(&mut self, scene: Scene, now: f64) {
        let step = (scene.progress * RING_STEPS).floor() as i32;
        if step != self.ring_step {
            self.ring_step = step;
            self.repaint = true;
        }
        match &self.scene {
            Some(old) if old.same_layout(&scene) => {
                self.scene = Some(scene);
                return;
            }
            Some(old) => {
                let id = |s: &Option<Shown>| s.as_ref().map(|x| x.id);
                let rows = |s: &Scene| s.expanded && s.has_stack();
                let changed = [
                    id(&old.top) != id(&scene.top),
                    rows(old) != rows(&scene),
                    id(&old.front) != id(&scene.front),
                ];
                for (z, c) in changed.into_iter().enumerate() {
                    if c {
                        self.since[z] = now;
                        self.painted_age_ms = 0.0;
                    }
                }
                // 講話泡泡淡出到一半換成框：框要完整不透明。
                if !scene.is_speech() {
                    self.fade = 1.0;
                }
            }
            None => {
                self.shown_at = now;
                self.since = [now; ZONES];
                self.painted_age_ms = 0.0;
                self.fade = 1.0;
            }
        }
        self.scene = Some(scene);
        self.hover = None;
        self.pressed = None;
        self.dirty = true;
    }

    pub fn hide(&mut self) {
        self.inside = false;
        if self.scene.take().is_some() {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
        }
    }

    /// 每一格呼叫：跟著桌寵移動、播出現動畫。`head` 是桌寵頭頂中心（螢幕實體像素）。
    pub fn update(
        &mut self,
        now: f64,
        head: Vector2,
        work: RECT,
        dpi_scale: f32,
        palette: &Palette,
    ) -> Result<()> {
        self.now = now;
        let Some(scene) = self.scene.clone() else {
            return Ok(());
        };
        let s = dpi_scale;
        let newest = self.since.iter().copied().fold(self.shown_at, f64::max);
        let age_ms = ((now - newest) * 1000.0) as f32;
        // 更新得很慢的時候（桌寵躲起來，一秒只更新兩次）出場動畫可能整段被跳過：
        // 動畫結束後一定再畫一次最後那一格，不然會停在剛出現、幾乎全透明的那格。
        let animating = age_ms < theme::MOTION_MEDIUM || self.painted_age_ms < theme::MOTION_MEDIUM;
        // 先用正中間排一次拿到寬度，再依螢幕邊緣決定尾巴要偏哪邊。
        if self.dirty || s != self.scale {
            self.painter.layout_scene(&scene, s, 0.5)?;
        }
        let (w, h, card_w) = {
            let l = self.painter.layout.as_ref().expect("layout");
            (l.width, l.height, l.card_w)
        };
        let margin = (theme::SHADOW_POPUP_BLUR * s).ceil() as i32;
        let x = ((head.X - w as f32 / 2.0).round() as i32).clamp(
            work.left - margin,
            (work.right - w + margin).max(work.left - margin),
        );
        let tail_frac = ((head.X - x as f32 - margin as f32) / card_w).clamp(0.0, 1.0);
        let tip_y = head.Y - TAIL_GAP_DIP * s;
        let y = (tip_y - (h - margin) as f32).round() as i32;
        let origin = POINT {
            x,
            y: y.max(work.top - margin),
        };

        let moved = origin.x != self.last_origin.x
            || origin.y != self.last_origin.y
            || (tail_frac - self.last_tail_frac).abs() > 1e-3;
        if !(self.dirty || self.repaint || animating || moved || s != self.scale) {
            return Ok(());
        }
        if (tail_frac - 0.5).abs() > 1e-3 || self.dirty || s != self.scale {
            self.painter.layout_scene(&scene, s, tail_frac)?;
        }
        let appear = self.since.map(|since| {
            let p = (((now - since) * 1000.0) as f32 / theme::MOTION_MEDIUM).clamp(0.0, 1.0);
            APPEAR_FROM_SCALE + (1.0 - APPEAR_FROM_SCALE) * theme::ease(theme::MOTION_SPRING, p)
        });
        let opacity = theme::ease(
            theme::MOTION_EASE_OUT,
            (((now - self.shown_at) * 1000.0) as f32 / theme::MOTION_FAST).clamp(0.0, 1.0),
        ) * self.fade;
        self.painter
            .paint(palette, s, self.hover, self.pressed, appear, scene.progress)?;
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
        self.repaint = false;
        Ok(())
    }

    /// 卡片視窗的滑鼠事件（視窗內座標）；按下又在同一顆按鈕上放開才算點。
    pub fn pointer(&mut self, kind: Pointer, x: f32, y: f32) -> Option<Press> {
        let scene = self.scene.as_ref()?;
        let hit = self.painter.hit(x, y);
        match kind {
            Pointer::Move => {
                self.inside = true;
                if hit != self.hover {
                    self.hover = hit;
                    self.repaint = true;
                }
                None
            }
            Pointer::Leave => {
                self.inside = false;
                if self.hover.is_some() || self.pressed.is_some() {
                    self.hover = None;
                    self.pressed = None;
                    self.repaint = true;
                }
                None
            }
            Pointer::Down => {
                self.pressed = hit;
                self.repaint = true;
                None
            }
            Pointer::Up => {
                let pressed = self.pressed.take();
                self.repaint = true;
                if scene.is_speech() && self.painter.inside_card(x, y) {
                    return Some(Press::Front(CardAction::Dismiss));
                }
                let (Some(p), Some(h)) = (pressed, hit) else {
                    return None;
                };
                if p != h {
                    return None;
                }
                let press = self.painter.layout.as_ref()?.buttons[h].press;
                if self.now - self.since[press.zone().index()] < PRESS_GUARD_SECS {
                    return None;
                }
                Some(press)
            }
        }
    }

    pub fn is_over_button(&self, x: f32, y: f32) -> bool {
        self.painter.hit(x, y).is_some()
            || self.scene.as_ref().is_some_and(Scene::is_speech) && self.painter.inside_card(x, y)
    }
}
