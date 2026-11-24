//! `Youban.exe --slice <多格圖.png> <桌寵資料夾名> <名稱1> <名稱2> ...`：
//! 把 ChatGPT 一次畫好的多個姿勢（例如 2×2）自動切成單張，存進 `pets/<桌寵>/frames/<名稱>.png`。
//! 名稱寫 `-` 的那一格不存（同一組畫了好幾張、各挑最好的幾格拼起來時用）。
//! 怎麼找出每一隻、怎麼排順序、怎麼清掉特效符號，在 `petkit::sheet`（設定程式的「補動作」也用同一套）。

use std::path::Path;

use petkit::sheet;
use windows::Win32::Foundation::E_FAIL;
use windows::core::{Error, Result};

use crate::paths;
use crate::sprites::Wic;

/// 這一格不要。
const SKIP_NAME: &str = "-";

pub fn run(sheet_path: &Path, pet: &str, names: &[String]) -> Result<()> {
    let pets_dir = paths::pets_dir().ok_or_else(|| Error::new(E_FAIL, "找不到 pets 資料夾"))?;
    let frames_dir = pets_dir.join(pet).join(petkit::pack::FRAMES_DIR);
    std::fs::create_dir_all(&frames_dir).map_err(|e| Error::new(E_FAIL, e.to_string()))?;
    let wic = Wic::new()?;
    let (w, h, pixels) = wic.decode_premultiplied(sheet_path)?;
    let pieces = sheet::split(&pixels, w, h);
    if pieces.len() != names.len() {
        return Err(Error::new(
            E_FAIL,
            format!(
                "圖上找到 {} 隻，但給了 {} 個名稱",
                pieces.len(),
                names.len()
            ),
        ));
    }
    for (piece, name) in pieces.iter().zip(names) {
        if name == SKIP_NAME {
            continue;
        }
        let file = frames_dir.join(format!("{name}.png"));
        wic.save_png(
            &file,
            piece.width as u32,
            piece.height as u32,
            &piece.pixels,
        )?;
        let note = if piece.marks_removed > 0 {
            format!("（拿掉 {} 個符號）", piece.marks_removed)
        } else {
            String::new()
        };
        println!(
            "{name}: {}x{}{note} → {}",
            piece.width,
            piece.height,
            file.display()
        );
    }
    Ok(())
}
