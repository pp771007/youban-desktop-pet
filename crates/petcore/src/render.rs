//! 畫圖：Direct2D（軟體算圖）畫進記憶體點陣圖，再用 `UpdateLayeredWindow` 貼成逐像素透明的視窗。
//! 用分層視窗而不是 DirectComposition：透明的像素滑鼠會自動穿透到底下的視窗，不用自己切換穿透狀態；
//! 桌寵與提醒卡都只有幾百像素見方，軟體算圖的成本很小，也沒有 GPU 裝置遺失要處理。

use windows::Win32::Foundation::{COLORREF, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::{ULW_ALPHA, UpdateLayeredWindow};
use windows::core::{Interface, Result};
use windows_numerics::{Matrix3x2, Vector2};

use crate::behavior::{Pivot, Pose};
use crate::effects::{Anchor, EffectPainter, Effects};
use crate::sprites::Frame;

pub const TRANSPARENT: D2D1_COLOR_F = D2D1_COLOR_F {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.0,
};

/// 全程式共用的繪圖與文字工廠。
#[derive(Clone)]
pub struct Gfx {
    pub d2d: ID2D1Factory,
    pub dwrite: IDWriteFactory,
}

impl Gfx {
    pub fn new() -> Result<Self> {
        unsafe {
            Ok(Self {
                d2d: D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?,
                dwrite: DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?,
            })
        }
    }
}

struct Surface {
    dc: HDC,
    bitmap: HBITMAP,
    bits: *const u8,
    previous: HGDIOBJ,
    width: i32,
    height: i32,
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.previous);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.dc);
        }
    }
}

/// 一個分層視窗的畫布：Direct2D 畫進記憶體點陣圖，再整張貼到螢幕。
pub struct Canvas {
    pub rt: ID2D1DCRenderTarget,
    surface: Option<Surface>,
}

impl Canvas {
    pub fn new(gfx: &Gfx) -> Result<Self> {
        let props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            ..Default::default()
        };
        Ok(Self {
            rt: unsafe { gfx.d2d.CreateDCRenderTarget(&props)? },
            surface: None,
        })
    }

    pub fn target(&self) -> ID2D1RenderTarget {
        self.rt.cast().expect("DC render target is a render target")
    }

    pub fn size(&self) -> (i32, i32) {
        self.surface
            .as_ref()
            .map_or((0, 0), |s| (s.width, s.height))
    }

    /// 大小變了才重建點陣圖。
    pub fn ensure_size(&mut self, width: i32, height: i32) -> Result<()> {
        if self
            .surface
            .as_ref()
            .is_some_and(|s| s.width == width && s.height == height)
        {
            return Ok(());
        }
        self.surface = None;
        unsafe {
            let dc = CreateCompatibleDC(None);
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let bitmap = CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0)?;
            let previous = SelectObject(dc, bitmap.into());
            self.rt.BindDC(
                dc,
                &RECT {
                    left: 0,
                    top: 0,
                    right: width,
                    bottom: height,
                },
            )?;
            self.surface = Some(Surface {
                dc,
                bitmap,
                bits: bits as *const u8,
                previous,
                width,
                height,
            });
        }
        Ok(())
    }

    /// 把畫好的內容貼到螢幕上；`origin` 是視窗左上角，`opacity` 0～1 整體透明度。
    pub fn present(&self, hwnd: HWND, origin: POINT, opacity: f32) -> Result<()> {
        let surface = self.surface.as_ref().expect("ensure_size before present");
        let size = SIZE {
            cx: surface.width,
            cy: surface.height,
        };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: (opacity.clamp(0.0, 1.0) * u8::MAX as f32).round() as u8,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        unsafe {
            UpdateLayeredWindow(
                hwnd,
                None,
                Some(&origin),
                Some(&size),
                Some(surface.dc),
                Some(&POINT { x: 0, y: 0 }),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            )
        }
    }

    /// 目前畫好的內容（預乘 BGRA，由上往下），給截圖工具用。
    pub fn pixels(&self) -> &[u8] {
        let s = self.surface.as_ref().expect("ensure_size before pixels");
        unsafe { std::slice::from_raw_parts(s.bits, (s.width * s.height * 4) as usize) }
    }
}

/// 這一格桌寵要畫什麼。
pub struct Scene<'a> {
    pub frame: &'a Frame,
    pub pose: Pose,
    /// `false` = 這組圖已經夠順，關掉引擎墊底效果（只保留左右翻轉）。
    pub procedural: bool,
    /// 站姿身高（實體像素），墊底效果的位移以它為單位。
    pub base_height: f32,
    /// 換桌寵的過場：0 = 縮成一點、1 = 正常大小。
    pub appear: f32,
    pub effects: &'a Effects,
    pub now: f64,
    /// 用的是這個狀態自己的圖（不是替代來的）。爬牆時有攀爬圖就照原樣畫，沒有就把走路圖轉 90 度。
    pub own_frames: bool,
}

pub struct PetRenderer {
    pub canvas: Canvas,
    target: ID2D1RenderTarget,
    effects: EffectPainter,
}

impl PetRenderer {
    pub fn new(gfx: &Gfx) -> Result<Self> {
        let canvas = Canvas::new(gfx)?;
        Ok(Self {
            target: canvas.target(),
            canvas,
            effects: EffectPainter::new()?,
        })
    }

    /// `anchor` 是腳底在視窗裡的位置。
    pub fn draw(&mut self, anchor: Vector2, scene: &Scene) -> Result<()> {
        let rt = &self.canvas.rt;
        unsafe {
            rt.BeginDraw();
            rt.Clear(Some(&TRANSPARENT));
            let f = scene.frame;
            // 站在地上時腳底對齊身體重心；貼在牆上時要的是外框邊緣碰到牆，所以用外框中心。
            let left = if scene.pose.climb.is_some() {
                anchor.X - f.width / 2.0
            } else {
                anchor.X - f.center_x
            };
            let dest = D2D_RECT_F {
                left,
                top: anchor.Y - f.height,
                right: left + f.width,
                bottom: anchor.Y,
            };
            let appear = Matrix3x2::scale_around(scene.appear, scene.appear, anchor);
            let transform = match scene.pose.climb {
                Some(side) => climb_transform(
                    &scene.pose,
                    side,
                    scene.own_frames,
                    anchor,
                    f.width,
                    scene.base_height,
                ),
                None => pose_transform(
                    &scene.pose,
                    scene.procedural,
                    anchor,
                    f.height,
                    scene.base_height,
                ),
            };
            rt.SetTransform(&(transform * appear));
            rt.DrawBitmap(
                &f.bitmap,
                Some(&dest),
                scene.appear.clamp(0.0, 1.0),
                D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                None,
            );
            rt.SetTransform(&Matrix3x2::identity());
            let zzz = match scene.pose.zzz {
                Some(t) => self.effects.draw_zzz(
                    &self.target,
                    t,
                    anchor,
                    f.width,
                    f.height,
                    scene.base_height,
                ),
                None => Ok(()),
            };
            let (scale_y, lift) = if scene.procedural {
                (scene.pose.scale_y, scene.pose.lift)
            } else {
                (1.0, 0.0)
            };
            let head = match scene.pose.climb {
                // 轉 90 度貼在牆上時，頭在離牆一個身高的地方。
                Some(side) if !scene.own_frames => Vector2 {
                    X: anchor.X - side * f.height,
                    Y: anchor.Y - f.width / 2.0,
                },
                _ => Vector2 {
                    X: anchor.X,
                    Y: anchor.Y - f.height * scale_y + lift * scene.base_height,
                },
            };
            let fx_anchor = Anchor {
                head,
                feet: anchor,
                height: scene.base_height,
                facing_left: scene.pose.flip,
            };
            let fx = self
                .effects
                .draw(&self.target, scene.effects, scene.now, fx_anchor);
            let end = rt.EndDraw(None, None);
            zzz.and(fx).and(end)
        }
    }
}

/// 貼在牆上：`side` -1 = 左牆、1 = 右牆，`anchor` 在牆線上。
/// - 有攀爬圖：照原樣直立畫，臉朝牆、身體貼著牆的那一側（攀爬圖照規範面向右畫）。
/// - 沒有：把走路圖以腳底為軸轉 90 度（頭朝離牆的方向、臉朝上），再往上挪半個寬度，
///   整隻落在牆線的螢幕內側、腳底以上 —— 「把牆當地面」走上去。
fn climb_transform(
    pose: &Pose,
    side: f32,
    own_frames: bool,
    anchor: Vector2,
    width: f32,
    base_height: f32,
) -> Matrix3x2 {
    // 爬的時候一步一步離牆一點點。
    let away = -side * (-pose.lift) * base_height;
    let flip = if side < 0.0 { -1.0 } else { 1.0 };
    if own_frames {
        let breathe = Matrix3x2::scale_around(flip, pose.scale_y, anchor);
        return breathe * Matrix3x2::translation(-side * width / 2.0 + away, 0.0);
    }
    // 走路圖面向右：右牆逆時針轉 90 度後臉剛好朝上；左牆要先左右翻再順時針轉。
    // 身體的縱軸轉 90 度後變成離牆的方向：scale_y 小於 1 = 往牆壓低。
    let face_up = Matrix3x2::scale_around(flip, pose.scale_y, anchor);
    let onto_wall = Matrix3x2::rotation_around(-side * 90.0, anchor);
    face_up * onto_wall * Matrix3x2::translation(away, -width / 2.0)
}

fn pose_transform(
    pose: &Pose,
    procedural: bool,
    anchor: Vector2,
    height: f32,
    base_height: f32,
) -> Matrix3x2 {
    let flip = if pose.flip { -1.0 } else { 1.0 };
    if !procedural {
        return Matrix3x2::scale_around(flip, 1.0, anchor);
    }
    let scale = Matrix3x2::scale_around(pose.scale_x * flip, pose.scale_y, anchor);
    let pivot = match pose.pivot {
        Pivot::Feet => anchor,
        Pivot::Top => Vector2 {
            X: anchor.X,
            Y: anchor.Y - height * pose.scale_y,
        },
        Pivot::Center => Vector2 {
            X: anchor.X,
            Y: anchor.Y - height * pose.scale_y / 2.0,
        },
    };
    let rotate = Matrix3x2::rotation_around(pose.rotation_deg, pivot);
    let lift = Matrix3x2::translation(0.0, pose.lift * base_height);
    scale * rotate * lift
}
