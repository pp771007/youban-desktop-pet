//! 把一張畫了好幾個姿勢的圖（例如 ChatGPT 的 2×2）切成一隻一隻。
//! 只看透明度，像素格式不拘（核心的預乘 BGRA、設定程式的 RGBA 都一樣，每個像素 4 bytes、第 4 個是透明度）。
//!
//! 不靠固定格線切 —— AI 畫的格子不會剛好對齊。改成找圖上一塊一塊不透明的區域：
//! 靠得很近的碎片（分開畫的葉子、尾巴）併成同一隻，太小的雜點丟掉，再依「由上到下、由左到右」排序，
//! 所以畫的時候照 1、2、3、4 的閱讀順序排，切出來就是播放順序。
//!
//! 每一隻裡沒跟身體連在一起的小碎塊（AI 愛畫的問號、驚嘆號、速度線）會被拿掉：
//! 這些特效引擎自己會畫，留在圖裡會疊兩層，換格時還會閃。大塊的（例如飛起來的橘子）留著。

const BYTES_PER_PIXEL: usize = 4;
const ALPHA: usize = 3;
/// 透明度高於這個才算角色的一部分（濾掉 AI 圖外圍的霧）。
const SOLID_ALPHA: u8 = 40;
/// 先把圖切成這麼大的小方格再找區塊，速度快很多。
const CELL_PX: usize = 4;
/// 相距這麼多格（以小方格計）以內的碎片算同一隻。
const MERGE_CELLS: usize = 4;
/// 比最大那一隻的面積小這麼多比例的區塊當雜點丟掉。
const NOISE_RATIO: f32 = 0.03;
/// 裁切時四周多留的透明邊（像素）。
pub const CROP_PAD: usize = 8;
/// 透明度高於這個才算連在一起（再淡的是邊緣的霧，載入時本來就會清掉）。
const LINK_ALPHA: u8 = 12;
/// 沒連在身體上、而且比身體小這麼多比例的碎塊當成特效符號拿掉。
const MARK_RATIO: f32 = 0.03;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub left: usize,
    pub top: usize,
    pub right: usize,
    pub bottom: usize,
}

impl Rect {
    fn area(&self) -> usize {
        (self.right - self.left) * (self.bottom - self.top)
    }
}

/// 切出來的一隻：像素（格式跟原圖一樣）與大小。
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    pub pixels: Vec<u8>,
    pub width: usize,
    pub height: usize,
    /// 拿掉了幾個特效符號（給工具顯示用）。
    pub marks_removed: usize,
}

/// 找出每一隻、裁下來、清掉特效符號，依閱讀順序回傳。
pub fn split(pixels: &[u8], width: usize, height: usize) -> Vec<Piece> {
    let alpha: Vec<u8> = pixels.as_chunks::<BYTES_PER_PIXEL>().0.iter().map(|p| p[ALPHA]).collect();
    find_figures(&alpha, width, height)
        .into_iter()
        .map(|rect| {
            let (mut crop, cw, ch) = crop(pixels, width, height, rect);
            let marks_removed = strip_marks(&mut crop, cw, ch);
            let (pixels, width, height) = retrim(&crop, cw, ch);
            Piece { pixels, width, height, marks_removed }
        })
        .collect()
}

/// 找出圖上的每一隻角色（像素座標的外框），依由上到下、由左到右排序。
pub fn find_figures(alpha: &[u8], width: usize, height: usize) -> Vec<Rect> {
    let (cw, ch) = (width.div_ceil(CELL_PX), height.div_ceil(CELL_PX));
    let mut solid = vec![false; cw * ch];
    for y in 0..height {
        for x in 0..width {
            if alpha[y * width + x] > SOLID_ALPHA {
                solid[(y / CELL_PX) * cw + x / CELL_PX] = true;
            }
        }
    }
    // 把有內容的小方格往外擴，靠得近的碎片就會連在一起。
    let mut grown = vec![false; cw * ch];
    for cy in 0..ch {
        for cx in 0..cw {
            if !solid[cy * cw + cx] {
                continue;
            }
            for gy in cy.saturating_sub(MERGE_CELLS)..(cy + MERGE_CELLS + 1).min(ch) {
                for gx in cx.saturating_sub(MERGE_CELLS)..(cx + MERGE_CELLS + 1).min(cw) {
                    grown[gy * cw + gx] = true;
                }
            }
        }
    }
    let mut label = vec![usize::MAX; cw * ch];
    let mut boxes: Vec<Rect> = Vec::new();
    for start in 0..cw * ch {
        if !grown[start] || label[start] != usize::MAX {
            continue;
        }
        let id = boxes.len();
        let mut b = Rect { left: usize::MAX, top: usize::MAX, right: 0, bottom: 0 };
        let mut stack = vec![start];
        label[start] = id;
        while let Some(i) = stack.pop() {
            let (cx, cy) = (i % cw, i / cw);
            // 外框只算真的有內容的格子；算進擴出來的那圈，每一隻都會被撐大。
            if solid[i] {
                b.left = b.left.min(cx);
                b.top = b.top.min(cy);
                b.right = b.right.max(cx + 1);
                b.bottom = b.bottom.max(cy + 1);
            }
            for ny in cy.saturating_sub(1)..(cy + 2).min(ch) {
                for nx in cx.saturating_sub(1)..(cx + 2).min(cw) {
                    let n = ny * cw + nx;
                    if grown[n] && label[n] == usize::MAX {
                        label[n] = id;
                        stack.push(n);
                    }
                }
            }
        }
        if b.left != usize::MAX {
            boxes.push(Rect { left: b.left * CELL_PX, top: b.top * CELL_PX, right: (b.right * CELL_PX).min(width), bottom: (b.bottom * CELL_PX).min(height) });
        }
    }
    let largest = boxes.iter().map(Rect::area).max().unwrap_or(0) as f32;
    boxes.retain(|b| b.area() as f32 >= largest * NOISE_RATIO);
    order_reading(boxes)
}

/// 由上到下分成一列一列（中心高度差不到半個身高就算同一列），每列由左到右。
fn order_reading(mut boxes: Vec<Rect>) -> Vec<Rect> {
    let center_y = |b: &Rect| (b.top + b.bottom) / 2;
    boxes.sort_by_key(center_y);
    let mut rows: Vec<Vec<Rect>> = Vec::new();
    for b in boxes {
        match rows.last_mut() {
            Some(row) if center_y(&b) < center_y(&row[0]) + (row[0].bottom - row[0].top) / 2 => row.push(b),
            _ => rows.push(vec![b]),
        }
    }
    rows.into_iter()
        .flat_map(|mut r| {
            r.sort_by_key(|b| b.left);
            r
        })
        .collect()
}

fn crop(pixels: &[u8], w: usize, h: usize, rect: Rect) -> (Vec<u8>, usize, usize) {
    let left = rect.left.saturating_sub(CROP_PAD);
    let top = rect.top.saturating_sub(CROP_PAD);
    let right = (rect.right + CROP_PAD).min(w);
    let bottom = (rect.bottom + CROP_PAD).min(h);
    let (cw, ch) = (right - left, bottom - top);
    let mut out = vec![0u8; cw * ch * BYTES_PER_PIXEL];
    for y in 0..ch {
        let src = ((top + y) * w + left) * BYTES_PER_PIXEL;
        out[y * cw * BYTES_PER_PIXEL..(y + 1) * cw * BYTES_PER_PIXEL].copy_from_slice(&pixels[src..src + cw * BYTES_PER_PIXEL]);
    }
    (out, cw, ch)
}

/// 把沒連在身體上的小碎塊清成透明，回傳拿掉幾塊。
pub fn strip_marks(px: &mut [u8], w: usize, h: usize) -> usize {
    let mut label = vec![usize::MAX; w * h];
    let mut sizes: Vec<usize> = Vec::new();
    for start in 0..w * h {
        if px[start * BYTES_PER_PIXEL + ALPHA] <= LINK_ALPHA || label[start] != usize::MAX {
            continue;
        }
        let id = sizes.len();
        let mut count = 0;
        let mut stack = vec![start];
        label[start] = id;
        while let Some(i) = stack.pop() {
            count += 1;
            let (x, y) = (i % w, i / w);
            for ny in y.saturating_sub(1)..(y + 2).min(h) {
                for nx in x.saturating_sub(1)..(x + 2).min(w) {
                    let n = ny * w + nx;
                    if label[n] == usize::MAX && px[n * BYTES_PER_PIXEL + ALPHA] > LINK_ALPHA {
                        label[n] = id;
                        stack.push(n);
                    }
                }
            }
        }
        sizes.push(count);
    }
    let largest = sizes.iter().copied().max().unwrap_or(0) as f32;
    let is_mark: Vec<bool> = sizes.iter().map(|&s| (s as f32) < largest * MARK_RATIO).collect();
    for (i, l) in label.iter().enumerate() {
        // 沒標到的（極淡的霧）也一起清掉，免得符號留下一圈淡淡的影子。
        if *l == usize::MAX || is_mark[*l] {
            px[i * BYTES_PER_PIXEL..(i + 1) * BYTES_PER_PIXEL].fill(0);
        }
    }
    is_mark.iter().filter(|m| **m).count()
}

/// 拿掉符號後外框可能變小：重新裁到內容，四周留 `CROP_PAD`。
pub fn retrim(px: &[u8], w: usize, h: usize) -> (Vec<u8>, usize, usize) {
    let (mut l, mut t, mut r, mut b) = (w, h, 0, 0);
    for y in 0..h {
        for x in 0..w {
            if px[(y * w + x) * BYTES_PER_PIXEL + ALPHA] > 0 {
                (l, t, r, b) = (l.min(x), t.min(y), r.max(x + 1), b.max(y + 1));
            }
        }
    }
    if l >= r {
        return (px.to_vec(), w, h);
    }
    let (l, t, r, b) = (l.saturating_sub(CROP_PAD), t.saturating_sub(CROP_PAD), (r + CROP_PAD).min(w), (b + CROP_PAD).min(h));
    let (nw, nh) = (r - l, b - t);
    let mut out = vec![0u8; nw * nh * BYTES_PER_PIXEL];
    for y in 0..nh {
        let src = ((t + y) * w + l) * BYTES_PER_PIXEL;
        out[y * nw * BYTES_PER_PIXEL..(y + 1) * nw * BYTES_PER_PIXEL].copy_from_slice(&px[src..src + nw * BYTES_PER_PIXEL]);
    }
    (out, nw, nh)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paint(alpha: &mut [u8], w: usize, x0: usize, y0: usize, x1: usize, y1: usize) {
        for y in y0..y1 {
            for x in x0..x1 {
                alpha[y * w + x] = 255;
            }
        }
    }

    #[test]
    fn finds_four_figures_in_reading_order_and_merges_detached_bits() {
        let (w, h) = (400, 400);
        let mut a = vec![0u8; w * h];
        // 2×2 四隻，故意不對齊格線、大小不一。
        paint(&mut a, w, 30, 40, 150, 170); // 左上
        paint(&mut a, w, 230, 30, 360, 160); // 右上（比左上高一點）
        paint(&mut a, w, 40, 240, 160, 380); // 左下
        paint(&mut a, w, 250, 250, 370, 370); // 右下
        paint(&mut a, w, 90, 25, 100, 34); // 左上那隻頭上分開畫的葉子（離本體幾個像素）
        a[5 * w + 395] = 255; // 角落一個雜點
        let found = find_figures(&a, w, h);
        assert_eq!(found.len(), 4, "{found:?}");
        assert!(found[0].left < 200 && found[0].top < 200, "top-left first");
        assert!(found[0].top <= 25, "leaf merged into the top-left figure");
        assert!(found[1].left > 200 && found[1].top < 200, "then top-right");
        assert!(found[2].left < 200 && found[2].top > 200, "then bottom-left");
        assert!(found[3].left > 200 && found[3].top > 200, "then bottom-right");
    }

    #[test]
    fn detached_small_marks_are_removed_but_big_parts_stay() {
        let (w, h) = (100, 100);
        let mut px = vec![0u8; w * h * 4];
        let mut fill = |x0: usize, y0: usize, x1: usize, y1: usize| {
            for y in y0..y1 {
                for x in x0..x1 {
                    px[(y * w + x) * 4..(y * w + x) * 4 + 4].fill(255);
                }
            }
        };
        fill(20, 40, 80, 100); // 身體 60×60
        fill(40, 20, 52, 32); // 飛起來的橘子 12×12：沒連著，但夠大
        fill(90, 10, 92, 14); // 問號：小碎塊
        let removed = strip_marks(&mut px, w, h);
        assert_eq!(removed, 1);
        assert_eq!(px[(12 * w + 91) * 4 + 3], 0, "mark cleared");
        assert_eq!(px[(25 * w + 45) * 4 + 3], 255, "orange kept");
        let (_, nw, _) = retrim(&px, w, h);
        assert!(nw < w, "frame shrinks back to the body after the mark is gone");
    }
}
