//! 設計參數（`design/tokens.json` 產生）與深淺色模式。

#![allow(dead_code)] // token 是整套產生的，沒用到的也留著，跟設定程式那邊保持同一份。

use petkit::config::ColorScheme;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::core::w;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgba {
    pub const fn d2d(self) -> D2D1_COLOR_F {
        D2D1_COLOR_F { r: self.r, g: self.g, b: self.b, a: self.a }
    }

    pub const fn with_alpha(self, a: f32) -> D2D1_COLOR_F {
        D2D1_COLOR_F { r: self.r, g: self.g, b: self.b, a }
    }
}

include!(concat!(env!("OUT_DIR"), "/tokens.rs"));

/// 設定選的深淺色；跟著系統時看 Windows「應用程式模式」。
pub fn current(scheme: ColorScheme) -> &'static Palette {
    match scheme {
        ColorScheme::Light => return &LIGHT,
        ColorScheme::Dark => return &DARK,
        ColorScheme::System => {}
    }
    let mut value: u32 = 1;
    let mut size = size_of::<u32>() as u32;
    let ok = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut _),
            Some(&mut size),
        )
    }
    .is_ok();
    if ok && value == 0 { &DARK } else { &LIGHT }
}

/// 換上桌寵包自己的強調色（`pet.json` 的 `accent`，`#RRGGBB`）；沒給或格式不對就用設計參數的預設。
/// 滑過／按下的顏色照預設那組的深淺差推出來，按鈕上的字挑跟底色對比夠的深色或白色。
pub fn with_accent(base: &Palette, accent: Option<&str>) -> Palette {
    let Some(c) = accent.and_then(parse_hex) else { return *base };
    let dark = luminance(base.surface) < 0.5;
    // 深色模式滑過變亮、按下變暗；淺色模式兩個都變暗（跟 tokens.json 那兩組的方向一致）。
    let (hover, pressed) = if dark { (shade(c, 0.1), shade(c, -0.08)) } else { (shade(c, -0.07), shade(c, -0.16)) };
    let on_accent = if luminance(c) > ON_ACCENT_DARK_ABOVE { base.text } else { Rgba { r: 1.0, g: 1.0, b: 1.0, a: 1.0 } };
    Palette { accent: c, accent_hover: hover, accent_pressed: pressed, on_accent, ..*base }
}

/// 強調色比這個亮，按鈕上的字改用深色（白字會看不清）。
const ON_ACCENT_DARK_ABOVE: f32 = 0.55;

fn parse_hex(s: &str) -> Option<Rgba> {
    let hex = s.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(hex, 16).ok()?;
    let ch = |shift: u32| ((v >> shift) & 0xFF) as f32 / 255.0;
    Some(Rgba { r: ch(16), g: ch(8), b: ch(0), a: 1.0 })
}

/// 相對亮度（WCAG 的算法）。
fn luminance(c: Rgba) -> f32 {
    let lin = |v: f32| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
}

/// 往白（正）或往黑（負）混一點。
fn shade(c: Rgba, amount: f32) -> Rgba {
    let mix = |v: f32| if amount >= 0.0 { v + (1.0 - v) * amount } else { v * (1.0 + amount) };
    Rgba { r: mix(c.r), g: mix(c.g), b: mix(c.b), a: c.a }
}

/// 三次貝茲緩動曲線（CSS `cubic-bezier` 同一種參數），`t` 0～1。
pub fn ease([x1, y1, x2, y2]: [f32; 4], t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    // 用二分法從 x 反求參數 u，再算 y。
    let bez = |a: f32, b: f32, u: f32| 3.0 * a * u * (1.0 - u).powi(2) + 3.0 * b * u * u * (1.0 - u) + u.powi(3);
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if bez(x1, x2, mid) < t { lo = mid } else { hi = mid }
    }
    bez(y1, y2, (lo + hi) / 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ease_hits_endpoints() {
        assert!((ease(MOTION_EASE_OUT, 0.0)).abs() < 1e-3);
        assert!((ease(MOTION_EASE_OUT, 1.0) - 1.0).abs() < 1e-3);
        assert!(ease(MOTION_SPRING, 0.6) > 1.0, "spring overshoots");
    }

    #[test]
    fn pet_accent_replaces_the_default_with_readable_button_text() {
        let blue = with_accent(&LIGHT, Some("#2F6FDE"));
        assert_eq!(blue.accent, Rgba { r: 0x2F as f32 / 255.0, g: 0x6F as f32 / 255.0, b: 0xDE as f32 / 255.0, a: 1.0 });
        assert!(blue.accent_pressed.b < blue.accent.b, "pressed is darker in light mode");
        assert_eq!(blue.on_accent.r, 1.0, "white text on a dark accent");
        let pale = with_accent(&LIGHT, Some("#F7E27A"));
        assert_eq!(pale.on_accent.r, LIGHT.text.r, "dark text on a pale accent");
        assert!(with_accent(&DARK, Some("#2F6FDE")).accent_hover.b > blue.accent.b, "hover is lighter in dark mode");
        for bad in [None, Some("orange"), Some("#12345")] {
            assert_eq!(with_accent(&LIGHT, bad).accent, LIGHT.accent, "{bad:?} keeps the default");
        }
    }
}
