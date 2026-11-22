//! 「新增桌寵」：使用者給一張圖（PNG、JPG、WebP、GIF、BMP 都收），整理成桌寵包能用的樣子。
//!
//! - 有透明背景就照用；沒有（白底、單色底、JPG）就從四邊往內把背景挖掉，邊緣做一圈漸淡，不會留下鋸齒或白邊。
//! - 清掉幾乎看不見的霧、裁到角色本身、四周留一點邊；太大的圖縮小（引擎本來就會再縮到顯示大小）。
//! - 一張圖上畫了好幾個姿勢（例如 AI 的四格圖）就切成好幾張，順序與清特效符號的規則見 `petkit::sheet`。

use std::io::Cursor;

use image::{ImageFormat, Rgba, RgbaImage, imageops};
use petkit::sheet;

/// 四邊的像素有這麼多比例是透明的，就當作「已經去好背了」。
const TRANSPARENT_BORDER_RATIO: f32 = 0.5;
/// 跟背景顏色差在這個距離以內（RGB 空間）算背景；JPG 的雜訊大約在 20～30。
const BG_TOLERANCE: f32 = 48.0;
/// 背景邊緣往外這段距離做漸淡（比背景多差這麼多才算完全不透明）。
const EDGE_SOFTNESS: f32 = 48.0;
/// 透明度低於這個的霧清掉（跟核心載圖時的門檻一樣）。
const ALPHA_NOISE: u8 = 12;
/// 存檔的最長邊；再大只是浪費空間。
const MAX_SIDE: u32 = 1024;
const OPAQUE: u8 = 255;

#[derive(Debug)]
pub enum ImportError {
    Decode(String),
    Empty,
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Decode(e) => write!(f, "讀不懂這張圖（{e}），換一張 PNG 或 JPG 試試"),
            Self::Empty => write!(f, "去掉背景後什麼都沒剩，換一張角色比較明顯的圖試試"),
        }
    }
}

/// 解碼 → 去背（需要的話）→ 清霧 → 找出每一隻、裁切 → 縮小，回傳整理好的 PNG（依閱讀順序，至少一張）。
pub fn prepare_all(bytes: &[u8]) -> Result<Vec<Vec<u8>>, ImportError> {
    let mut img = image::load_from_memory(bytes).map_err(|e| ImportError::Decode(e.to_string()))?.to_rgba8();
    if !has_transparent_border(&img) {
        remove_background(&mut img);
    }
    for p in img.pixels_mut() {
        if p[3] <= ALPHA_NOISE {
            *p = Rgba([0, 0, 0, 0]);
        }
    }
    let (w, h) = img.dimensions();
    let pieces = sheet::split(img.as_raw(), w as usize, h as usize);
    if pieces.is_empty() {
        return Err(ImportError::Empty);
    }
    pieces
        .into_iter()
        .map(|p| {
            let mut img = RgbaImage::from_raw(p.width as u32, p.height as u32, p.pixels).expect("piece size matches its pixels");
            if img.width().max(img.height()) > MAX_SIDE {
                let scale = MAX_SIDE as f32 / img.width().max(img.height()) as f32;
                img = imageops::resize(&img, (img.width() as f32 * scale) as u32, (img.height() as f32 * scale) as u32, imageops::FilterType::Lanczos3);
            }
            let mut out = Vec::new();
            img.write_to(&mut Cursor::new(&mut out), ImageFormat::Png).map_err(|e| ImportError::Decode(e.to_string()))?;
            Ok(out)
        })
        .collect()
}

fn border(img: &RgbaImage) -> impl Iterator<Item = (u32, u32)> + '_ {
    let (w, h) = img.dimensions();
    (0..w).flat_map(move |x| [(x, 0), (x, h - 1)]).chain((0..h).flat_map(move |y| [(0, y), (w - 1, y)]))
}

fn has_transparent_border(img: &RgbaImage) -> bool {
    let (mut total, mut clear) = (0usize, 0usize);
    for (x, y) in border(img) {
        total += 1;
        if img.get_pixel(x, y)[3] < OPAQUE / 2 {
            clear += 1;
        }
    }
    clear as f32 >= total as f32 * TRANSPARENT_BORDER_RATIO
}

fn distance(a: &Rgba<u8>, b: [f32; 3]) -> f32 {
    let d = |i: usize| a[i] as f32 - b[i];
    (d(0) * d(0) + d(1) * d(1) + d(2) * d(2)).sqrt()
}

/// 背景色 = 四邊像素各色的中位數（角落有一點別的東西也不會被帶偏）；
/// 從四邊往內灌水，顏色接近背景的挖掉，碰到角色的描邊就停。
fn remove_background(img: &mut RgbaImage) {
    let (w, h) = img.dimensions();
    let mut channels: [Vec<u8>; 3] = Default::default();
    for (x, y) in border(img) {
        let p = img.get_pixel(x, y);
        for (c, v) in channels.iter_mut().enumerate() {
            v.push(p[c]);
        }
    }
    let bg = channels.map(|mut v| {
        v.sort_unstable();
        v[v.len() / 2] as f32
    });
    let idx = |x: u32, y: u32| (y * w + x) as usize;
    let mut is_bg = vec![false; (w * h) as usize];
    let mut stack: Vec<(u32, u32)> = border(img).filter(|&(x, y)| distance(img.get_pixel(x, y), bg) <= BG_TOLERANCE).collect();
    for &(x, y) in &stack {
        is_bg[idx(x, y)] = true;
    }
    while let Some((x, y)) = stack.pop() {
        for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
            let (nx, ny) = (x as i32 + dx, y as i32 + dy);
            if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                continue;
            }
            let (nx, ny) = (nx as u32, ny as u32);
            if !is_bg[idx(nx, ny)] && distance(img.get_pixel(nx, ny), bg) <= BG_TOLERANCE {
                is_bg[idx(nx, ny)] = true;
                stack.push((nx, ny));
            }
        }
    }
    for y in 0..h {
        for x in 0..w {
            if is_bg[idx(x, y)] {
                img.put_pixel(x, y, Rgba([0, 0, 0, 0]));
                continue;
            }
            // 緊貼背景的那一圈：越像背景越透明，邊緣才不會有一圈白邊。
            let touches_bg = [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)].iter().any(|(dx, dy)| {
                let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                nx >= 0 && ny >= 0 && nx < w as i32 && ny < h as i32 && is_bg[idx(nx as u32, ny as u32)]
            });
            if touches_bg {
                let p = img.get_pixel_mut(x, y);
                let t = ((distance(p, bg) - BG_TOLERANCE) / EDGE_SOFTNESS).clamp(0.0, 1.0);
                p[3] = (p[3] as f32 * t) as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAD: u32 = sheet::CROP_PAD as u32;

    fn png(img: &RgbaImage) -> Vec<u8> {
        let mut out = Vec::new();
        img.write_to(&mut Cursor::new(&mut out), ImageFormat::Png).unwrap();
        out
    }

    fn prepare(bytes: &[u8]) -> Result<Vec<u8>, ImportError> {
        prepare_all(bytes).map(|mut v| v.remove(0))
    }

    #[test]
    fn a_four_pose_sheet_on_white_becomes_four_frames_in_reading_order() {
        let mut img = RgbaImage::from_pixel(400, 400, Rgba([255, 255, 255, 255]));
        // 四隻大小不同，切出來才分得出誰是誰。
        for (i, (x0, y0)) in [(30u32, 30u32), (230, 40), (40, 230), (240, 240)].into_iter().enumerate() {
            let size = 100 + i as u32 * 10;
            for y in y0..y0 + size.min(150) {
                for x in x0..x0 + 100 {
                    img.put_pixel(x, y, Rgba([60, 40, 30, 255]));
                }
            }
        }
        let frames = prepare_all(&png(&img)).unwrap();
        let heights: Vec<u32> = frames.iter().map(|f| image::load_from_memory(f).unwrap().height()).collect();
        assert_eq!(heights, [100 + 2 * PAD, 110 + 2 * PAD, 120 + 2 * PAD, 130 + 2 * PAD]);
    }

    #[test]
    fn white_background_is_removed_and_character_kept() {
        // 白底中間一個描黑邊的橘色方塊（像角色），角色身上也有白色（眼睛），不能被挖掉。
        let mut img = RgbaImage::from_pixel(100, 100, Rgba([255, 255, 255, 255]));
        for y in 30..80 {
            for x in 20..70 {
                let edge = x == 20 || x == 69 || y == 30 || y == 79;
                img.put_pixel(x, y, if edge { Rgba([40, 25, 20, 255]) } else { Rgba([230, 150, 80, 255]) });
            }
        }
        img.put_pixel(40, 50, Rgba([255, 255, 255, 255]));
        let out = image::load_from_memory(&prepare(&png(&img)).unwrap()).unwrap().to_rgba8();
        assert_eq!(out.dimensions(), (50 + 2 * PAD, 50 + 2 * PAD), "trimmed to the character plus padding");
        assert_eq!(out.get_pixel(0, 0)[3], 0, "background gone");
        assert_eq!(out.get_pixel(PAD + 20, PAD + 20)[3], 255, "white eye inside the outline stays");
    }

    #[test]
    fn already_transparent_image_is_only_trimmed() {
        let mut img = RgbaImage::new(80, 60);
        for y in 10..50 {
            for x in 10..40 {
                img.put_pixel(x, y, Rgba([250, 250, 250, 255])); // 白色的角色：不能當成背景挖掉
            }
        }
        let out = image::load_from_memory(&prepare(&png(&img)).unwrap()).unwrap().to_rgba8();
        assert_eq!(out.dimensions(), (30 + 2 * PAD, 40 + 2 * PAD));
        assert_eq!(out.get_pixel(PAD + 5, PAD + 5)[3], 255);
    }

    #[test]
    fn blank_image_is_rejected() {
        let img = RgbaImage::from_pixel(20, 20, Rgba([255, 255, 255, 255]));
        assert!(matches!(prepare(&png(&img)), Err(ImportError::Empty)));
    }
}
