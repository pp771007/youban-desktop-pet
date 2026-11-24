//! 功能小圖示：`crates/petsettings/ui/icons/` 的 SVG（跟設定程式共用同一份），換上顏色、拉成指定大小畫在 Direct2D 上。
//! 提醒卡與右鍵選單共用。

use std::collections::HashMap;

use windows::Win32::Graphics::Direct2D::Common::D2D_SIZE_F;
use windows::Win32::Graphics::Direct2D::{ID2D1DeviceContext5, ID2D1RenderTarget, ID2D1SvgDocument};
use windows::Win32::UI::Shell::SHCreateMemStream;
use windows::core::Interface;
use windows_numerics::Matrix3x2;

use crate::log;
use crate::theme::Rgba;

/// 每隻桌寵的強調色不同，換來換去會一直多出新顏色的圖示：滿了就整個重來。
const CACHE_MAX: usize = 32;

/// 會跟著卡片存進狀態檔（重開再跳），所以要能存讀。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Icon {
    BellRing,
    Bell,
    History,
    CircleCheck,
    Alert,
    Download,
    Heart,
    PawPrint,
    Cookie,
    Ball,
    Save,
    RotateCcw,
    Settings,
    Power,
    Check,
    ChevronRight,
    Clock,
    Stop,
    Tomato,
    Cup,
    Cake,
}

impl Icon {
    fn svg(self) -> &'static str {
        match self {
            Icon::BellRing => include_str!("../../petsettings/ui/icons/bell-ring.svg"),
            Icon::Bell => include_str!("../../petsettings/ui/icons/bell.svg"),
            Icon::History => include_str!("../../petsettings/ui/icons/history.svg"),
            Icon::CircleCheck => include_str!("../../petsettings/ui/icons/circle-check.svg"),
            Icon::Alert => include_str!("../../petsettings/ui/icons/triangle-alert.svg"),
            Icon::Download => include_str!("../../petsettings/ui/icons/download.svg"),
            Icon::Heart => include_str!("../../petsettings/ui/icons/heart.svg"),
            Icon::PawPrint => include_str!("../../petsettings/ui/icons/paw-print.svg"),
            Icon::Cookie => include_str!("../../petsettings/ui/icons/cookie.svg"),
            Icon::Ball => include_str!("../../petsettings/ui/icons/ball.svg"),
            Icon::Save => include_str!("../../petsettings/ui/icons/save.svg"),
            Icon::RotateCcw => include_str!("../../petsettings/ui/icons/rotate-ccw.svg"),
            Icon::Settings => include_str!("../../petsettings/ui/icons/settings.svg"),
            Icon::Power => include_str!("../../petsettings/ui/icons/power.svg"),
            Icon::Check => include_str!("../../petsettings/ui/icons/check.svg"),
            Icon::ChevronRight => include_str!("../../petsettings/ui/icons/chevron-right.svg"),
            Icon::Clock => include_str!("../../petsettings/ui/icons/clock.svg"),
            Icon::Stop => include_str!("../../petsettings/ui/icons/x.svg"),
            Icon::Tomato => include_str!("../../petsettings/ui/icons/tomato.svg"),
            Icon::Cup => include_str!("../../petsettings/ui/icons/cup.svg"),
            Icon::Cake => include_str!("../../petsettings/ui/icons/cake.svg"),
        }
    }
}

pub struct Icons {
    /// 畫 SVG 要 Direct2D 1.5 的介面；拿不到（舊版 Windows）就不畫圖示，其他照常。
    target: Option<ID2D1DeviceContext5>,
    /// (圖示, 顏色, 像素大小) → 已解析的 SVG。
    docs: HashMap<(Icon, u32, u32), ID2D1SvgDocument>,
}

impl Icons {
    pub fn new(rt: &ID2D1RenderTarget) -> Self {
        let target = rt.cast::<ID2D1DeviceContext5>().ok();
        if target.is_none() {
            log!("Direct2D SVG unavailable; icons disabled");
        }
        Self { target, docs: HashMap::new() }
    }

    /// 換縮放比例時呼叫（舊大小的不會再用到）。
    pub fn clear(&mut self) {
        self.docs.clear();
    }

    pub fn doc(&mut self, icon: Icon, color: Rgba, size: f32) -> Option<ID2D1SvgDocument> {
        let target = self.target.as_ref()?;
        let to_byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u32;
        let rgb = (to_byte(color.r) << 16) | (to_byte(color.g) << 8) | to_byte(color.b);
        let key = (icon, rgb, size.round() as u32);
        if let Some(doc) = self.docs.get(&key) {
            return Some(doc.clone());
        }
        if self.docs.len() >= CACHE_MAX {
            self.docs.clear();
        }
        // 原檔寫死 24×24 並用 currentColor 上色：改成填滿畫布、換成實際顏色。
        let text = icon.svg().replace("currentColor", &format!("#{rgb:06x}")).replace("width=\"24\"", "width=\"100%\"").replace("height=\"24\"", "height=\"100%\"");
        let doc = unsafe {
            let stream = SHCreateMemStream(Some(text.as_bytes()))?;
            target.CreateSvgDocument(&stream, D2D_SIZE_F { width: size, height: size })
        };
        match doc {
            Ok(doc) => {
                self.docs.insert(key, doc.clone());
                Some(doc)
            }
            Err(e) => {
                log!("svg {icon:?} failed: {e}");
                None
            }
        }
    }

    /// 畫在 `transform` 的位置（左上角）。會改掉畫布的 transform，呼叫端畫別的之前要自己設回去。
    pub fn draw(&self, doc: &ID2D1SvgDocument, transform: &Matrix3x2) {
        if let Some(target) = &self.target {
            unsafe {
                target.SetTransform(transform);
                target.DrawSvgDocument(doc);
            }
        }
    }
}
