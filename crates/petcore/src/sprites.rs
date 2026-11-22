//! 把桌寵包的圖解碼成 Direct2D 點陣圖：清掉幾乎看不見的殘影、裁到角色外框、縮成顯示大小。
//! 記憶體裡只留顯示大小的圖（`docs/pet-pack-spec.md` 第 5 節）。

use std::collections::HashMap;
use std::path::Path;

use petkit::pack::{PetPack, StateAnim};
use petkit::states;
use windows::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
use windows::Win32::Graphics::Direct2D::{ID2D1Bitmap, ID2D1RenderTarget};
use windows::Win32::Graphics::Gdi::{BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS, DeleteObject};
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, HICON, ICONINFO};
use windows::core::{HSTRING, Result};

/// 透明度低於這個（約 5%）的像素歸零：AI 圖外圍常有一大片 1～3% 的霧，看不到卻會撐大點擊範圍。
const ALPHA_NOISE: u8 = 12;
/// 透明度高於這個的補成完全不透明（AI 圖的角色本體常是 253、254）。
const ALPHA_SOLID: u8 = 248;
const BYTES_PER_PIXEL: u32 = 4;

pub struct Wic {
    factory: IWICImagingFactory,
}

pub struct Frame {
    pub bitmap: ID2D1Bitmap,
    pub width: f32,
    pub height: f32,
    /// 身體重心離左緣多遠（不透明度加權的平均 x）。對齊腳底時用它不用外框中心：
    /// 舉起來的鈴鐺、伸出去的腳會把外框撐寬，用外框中心對齊的話換格時身體會左右跳。
    pub center_x: f32,
}

pub struct LoadedVariant {
    pub weight: u32,
    pub procedural: bool,
    pub durations_ms: Vec<u32>,
    pub frames: Vec<Frame>,
}

pub struct LoadedState {
    pub looping: bool,
    pub variants: Vec<LoadedVariant>,
}

pub struct SpriteSet {
    pub states: HashMap<String, LoadedState>,
    pub dpi: u32,
    /// 站姿的身高（實體像素），其他尺寸都以它為基準。
    pub base_height: f32,
    pub max_width: f32,
    pub max_height: f32,
}

struct Trimmed {
    bitmap: IWICBitmap,
    rect: WICRect,
    /// 重心離外框左緣多遠（原圖像素）。
    center_x: f32,
}

impl Wic {
    pub fn new() -> Result<Self> {
        let factory = unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)? };
        Ok(Self { factory })
    }

    /// 解碼成預乘透明度的 BGRA，清殘影，回傳角色外框。
    fn load_trimmed(&self, path: &Path) -> Result<Trimmed> {
        unsafe {
            let decoder = self.factory.CreateDecoderFromFilename(
                &HSTRING::from(path.as_os_str()),
                None,
                GENERIC_READ,
                WICDecodeMetadataCacheOnDemand,
            )?;
            let frame = decoder.GetFrame(0)?;
            let converter = self.factory.CreateFormatConverter()?;
            converter.Initialize(&frame, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)?;
            let bitmap = self.factory.CreateBitmapFromSource(&converter, WICBitmapCacheOnLoad)?;
            let (mut w, mut h) = (0, 0);
            bitmap.GetSize(&mut w, &mut h)?;
            let full = WICRect { X: 0, Y: 0, Width: w as i32, Height: h as i32 };
            let rect = {
                let lock = bitmap.Lock(&full, (WICBitmapLockRead.0 | WICBitmapLockWrite.0) as u32)?;
                let stride = lock.GetStride()?;
                let mut size = 0;
                let mut ptr = std::ptr::null_mut();
                lock.GetDataPointer(&mut size, &mut ptr)?;
                let pixels = std::slice::from_raw_parts_mut(ptr, size as usize);
                clean_and_bound(pixels, w, h, stride)
            };
            Ok(match rect {
                Some((rect, center_x)) => Trimmed { bitmap, rect, center_x },
                None => Trimmed { bitmap, rect: full, center_x: w as f32 / 2.0 },
            })
        }
    }

    fn scaled(&self, t: &Trimmed, width: u32, height: u32) -> Result<IWICBitmapScaler> {
        unsafe {
            let clipper = self.factory.CreateBitmapClipper()?;
            clipper.Initialize(&t.bitmap, &t.rect)?;
            let scaler = self.factory.CreateBitmapScaler()?;
            scaler.Initialize(&clipper, width.max(1), height.max(1), WICBitmapInterpolationModeHighQualityCubic)?;
            Ok(scaler)
        }
    }

    /// 載入整隻桌寵，縮成 `base_height`（實體像素）為站姿身高的大小。
    pub fn load_pet(&self, pack: &PetPack, rt: &ID2D1RenderTarget, base_height: f32, dpi: u32) -> Result<SpriteSet> {
        let mut out = HashMap::new();
        let (mut max_w, mut max_h) = (0.0f32, 0.0f32);
        for (state, anim) in &pack.states {
            let loaded = self.load_state(anim, rt, base_height)?;
            for f in loaded.variants.iter().flat_map(|v| &v.frames) {
                max_w = max_w.max(f.width);
                max_h = max_h.max(f.height);
            }
            out.insert(state.clone(), loaded);
        }
        debug_assert!(out.contains_key(states::IDLE));
        Ok(SpriteSet { states: out, dpi, base_height, max_width: max_w, max_height: max_h })
    }

    fn load_state(&self, anim: &StateAnim, rt: &ID2D1RenderTarget, base_height: f32) -> Result<LoadedState> {
        let target_h = base_height * anim.height_ratio;
        let mut variants = Vec::with_capacity(anim.variants.len());
        for v in &anim.variants {
            let trimmed: Vec<Trimmed> = v.frames.iter().map(|p| self.load_trimmed(p)).collect::<Result<_>>()?;
            // 同一組動作用同一個縮放比例，動作之間的高低差（跳起來、蹲下）才會保留。
            let tallest = trimmed.iter().map(|t| t.rect.Height).max().unwrap_or(1).max(1) as f32;
            let scale = target_h / tallest;
            let mut frames = Vec::with_capacity(trimmed.len());
            for t in &trimmed {
                let w = (t.rect.Width as f32 * scale).round() as u32;
                let h = (t.rect.Height as f32 * scale).round() as u32;
                let scaler = self.scaled(t, w, h)?;
                let bitmap = unsafe { rt.CreateBitmapFromWicBitmap(&scaler, None)? };
                frames.push(Frame { bitmap, width: w as f32, height: h as f32, center_x: t.center_x * scale });
            }
            variants.push(LoadedVariant { weight: v.weight, procedural: v.procedural, durations_ms: v.durations_ms.clone(), frames });
        }
        Ok(LoadedState { looping: anim.looping, variants })
    }

    /// 解碼編進執行檔的 PNG，縮到長邊 `max_side`（實體像素）：回傳（點陣圖, 寬, 高）。
    pub fn load_embedded(&self, png: &[u8], rt: &ID2D1RenderTarget, max_side: f32) -> Result<(ID2D1Bitmap, f32, f32)> {
        unsafe {
            let stream = self.factory.CreateStream()?;
            stream.InitializeFromMemory(png)?;
            let decoder = self.factory.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)?;
            let frame = decoder.GetFrame(0)?;
            let converter = self.factory.CreateFormatConverter()?;
            converter.Initialize(&frame, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)?;
            let (mut w, mut h) = (0, 0);
            converter.GetSize(&mut w, &mut h)?;
            let scale = max_side / w.max(h).max(1) as f32;
            let (sw, sh) = (((w as f32 * scale).round() as u32).max(1), ((h as f32 * scale).round() as u32).max(1));
            let scaler = self.factory.CreateBitmapScaler()?;
            scaler.Initialize(&converter, sw, sh, WICBitmapInterpolationModeHighQualityCubic)?;
            Ok((rt.CreateBitmapFromWicBitmap(&scaler, None)?, sw as f32, sh as f32))
        }
    }

    /// 整張解碼成預乘 BGRA（切圖工具用）：回傳（寬, 高, 像素）。
    pub fn decode_premultiplied(&self, path: &Path) -> Result<(usize, usize, Vec<u8>)> {
        unsafe {
            let decoder = self.factory.CreateDecoderFromFilename(&HSTRING::from(path.as_os_str()), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand)?;
            let frame = decoder.GetFrame(0)?;
            let converter = self.factory.CreateFormatConverter()?;
            converter.Initialize(&frame, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)?;
            let (mut w, mut h) = (0, 0);
            converter.GetSize(&mut w, &mut h)?;
            let mut buf = vec![0u8; (w * h * BYTES_PER_PIXEL) as usize];
            converter.CopyPixels(std::ptr::null(), w * BYTES_PER_PIXEL, &mut buf)?;
            Ok((w as usize, h as usize, buf))
        }
    }

    /// 把預乘 BGRA 的像素存成 PNG（截圖工具用）。
    pub fn save_png(&self, path: &Path, width: u32, height: u32, pixels: &[u8]) -> Result<()> {
        unsafe {
            let bitmap = self.factory.CreateBitmapFromMemory(width, height, &GUID_WICPixelFormat32bppPBGRA, width * BYTES_PER_PIXEL, pixels)?;
            let stream = self.factory.CreateStream()?;
            stream.InitializeFromFilename(&HSTRING::from(path.as_os_str()), GENERIC_WRITE.0)?;
            let encoder = self.factory.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null())?;
            encoder.Initialize(&stream, WICBitmapEncoderNoCache)?;
            let mut frame = None;
            let mut props = None;
            encoder.CreateNewFrame(&mut frame, &mut props)?;
            let frame = frame.expect("CreateNewFrame returns a frame");
            frame.Initialize(props.as_ref())?;
            frame.WriteSource(&bitmap, std::ptr::null())?;
            frame.Commit()?;
            encoder.Commit()
        }
    }

    /// 用站姿圖做托盤圖示。
    pub fn make_icon(&self, path: &Path, size: u32) -> Result<HICON> {
        let t = self.load_trimmed(path)?;
        let scale = size as f32 / t.rect.Width.max(t.rect.Height).max(1) as f32;
        let w = ((t.rect.Width as f32 * scale).round() as u32).clamp(1, size);
        let h = ((t.rect.Height as f32 * scale).round() as u32).clamp(1, size);
        unsafe {
            // 圖示要「未預乘」的 BGRA。
            let scaler = self.scaled(&t, w, h)?;
            let converter = self.factory.CreateFormatConverter()?;
            converter.Initialize(&scaler, &GUID_WICPixelFormat32bppBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)?;
            let stride = size * BYTES_PER_PIXEL;
            let mut buf = vec![0u8; (stride * size) as usize];
            let ox = (size - w) / 2;
            let oy = size - h;
            let offset = (oy * stride + ox * BYTES_PER_PIXEL) as usize;
            converter.CopyPixels(std::ptr::null(), stride, &mut buf[offset..])?;

            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: size as i32,
                    biHeight: -(size as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let color = CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0)?;
            std::ptr::copy_nonoverlapping(buf.as_ptr(), bits as *mut u8, buf.len());
            let mask = CreateBitmap(size as i32, size as i32, 1, 1, None);
            let info = ICONINFO { fIcon: true.into(), xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
            let icon = CreateIconIndirect(&info);
            let _ = DeleteObject(color.into());
            let _ = DeleteObject(mask.into());
            icon
        }
    }
}

/// 就地清掉殘影、補滿近乎不透明的像素（預乘 BGRA），回傳角色外框；整張透明回傳 `None`。
/// 清殘影；回傳角色外框，以及重心離外框左緣多遠。
fn clean_and_bound(pixels: &mut [u8], w: u32, h: u32, stride: u32) -> Option<(WICRect, f32)> {
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (u32::MAX, u32::MAX, 0, 0);
    let (mut mass, mut moment) = (0u64, 0u64);
    for y in 0..h {
        let row = &mut pixels[(y * stride) as usize..(y * stride + w * BYTES_PER_PIXEL) as usize];
        for (x, px) in row.chunks_exact_mut(BYTES_PER_PIXEL as usize).enumerate() {
            let a = px[3];
            if a <= ALPHA_NOISE {
                px.fill(0);
                continue;
            }
            if a >= ALPHA_SOLID && a < u8::MAX {
                for c in &mut px[..3] {
                    *c = ((*c as u32 * u8::MAX as u32) / a as u32).min(u8::MAX as u32) as u8;
                }
                px[3] = u8::MAX;
            }
            let x = x as u32;
            mass += px[3] as u64;
            moment += px[3] as u64 * x as u64;
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }
    }
    (min_x <= max_x).then(|| {
        let rect = WICRect { X: min_x as i32, Y: min_y as i32, Width: (max_x - min_x + 1) as i32, Height: (max_y - min_y + 1) as i32 };
        // +0.5：像素的中心在格子中間。
        (rect, moment as f32 / mass as f32 + 0.5 - min_x as f32)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_follows_the_body_not_a_prop_sticking_out() {
        // 一列 20 格：0～9 是身體，19 是舉出去的一個小鈴鐺。外框中心在 10，身體中心在 5。
        let (w, h) = (20u32, 1u32);
        let mut px = vec![0u8; (w * BYTES_PER_PIXEL) as usize];
        for x in (0..10).chain([19]) {
            px[x * 4..x * 4 + 4].copy_from_slice(&[255, 255, 255, 255]);
        }
        let (rect, center) = clean_and_bound(&mut px, w, h, w * BYTES_PER_PIXEL).unwrap();
        assert_eq!(rect.Width, 20);
        // 重心 = (0+…+9+19)/11 + 半格 ≈ 6.3：被鈴鐺拉過去一點點，遠不到外框中心。
        assert!((center - (64.0 / 11.0 + 0.5)).abs() < 0.01, "center {center}");
    }

    #[test]
    fn haze_is_removed_and_body_becomes_opaque() {
        // 4×2：一顆幾乎透明的霧、一顆 253 的本體、其他全透明。
        let (w, h, stride) = (4u32, 2u32, 16u32);
        let mut px = vec![0u8; (stride * h) as usize];
        px[0..4].copy_from_slice(&[3, 3, 3, 3]);
        let body = (stride + 2 * 4) as usize;
        px[body..body + 4].copy_from_slice(&[100, 100, 100, 253]);
        let (rect, _) = clean_and_bound(&mut px, w, h, stride).unwrap();
        assert_eq!(&px[0..4], &[0, 0, 0, 0]);
        assert_eq!(px[body + 3], 255);
        assert_eq!((rect.X, rect.Y, rect.Width, rect.Height), (2, 1, 1, 1));
    }
}
