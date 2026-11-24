//! 改桌寵包裡某個動作的圖：加幾張、刪一張、換順序、調速度。設定程式的「動作編輯」用。
//! 檔名照規範 `<state>[-<variant>]_<nn>.png`，順序就是編號；每次改完都重新編號成 01、02、03…，不留空號。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::config::write_atomic;
use crate::pack::{DEFAULT_VARIANT, FRAMES_DIR, MANIFEST_FILE, parse_frame_name};

const TEMP_PREFIX: &str = "~renumber-";
/// 速度設定寫在 pet.json 的這個欄位（每秒幾張）。
const FPS_KEY: &str = "fps";
const STATES_KEY: &str = "states";

pub fn frame_name(state: &str, variant: &str, index: usize) -> String {
    let n = index + 1;
    if variant == DEFAULT_VARIANT {
        format!("{state}_{n:02}.png")
    } else {
        format!("{state}-{variant}_{n:02}.png")
    }
}

/// 某個動作某一組的圖，依播放順序。
pub fn list(pet_dir: &Path, state: &str, variant: &str) -> io::Result<Vec<PathBuf>> {
    let mut found: Vec<(u32, PathBuf)> = Vec::new();
    for e in fs::read_dir(pet_dir.join(FRAMES_DIR))? {
        let e = e?;
        if let Some((s, v, n)) = parse_frame_name(&e.file_name().to_string_lossy())
            && s == state
            && v == variant
        {
            found.push((n, e.path()));
        }
    }
    found.sort();
    Ok(found.into_iter().map(|(_, p)| p).collect())
}

/// 某個動作有哪幾組（預設那組排第一）。
pub fn variants(pet_dir: &Path, state: &str) -> io::Result<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for e in fs::read_dir(pet_dir.join(FRAMES_DIR))? {
        if let Some((s, v, _)) = parse_frame_name(&e?.file_name().to_string_lossy())
            && s == state
            && !out.contains(&v)
        {
            out.push(v);
        }
    }
    out.sort_by_key(|v| (v != DEFAULT_VARIANT, v.clone()));
    Ok(out)
}

/// 把檔案照 `order` 的順序重新編號（先全部改成暫存名，避免 02 改成 01 時撞到還沒搬走的 01）。
fn renumber(pet_dir: &Path, state: &str, variant: &str, order: &[PathBuf]) -> io::Result<()> {
    let frames = pet_dir.join(FRAMES_DIR);
    let temps: Vec<PathBuf> = order
        .iter()
        .enumerate()
        .map(|(i, _)| frames.join(format!("{TEMP_PREFIX}{i}.png")))
        .collect();
    for (from, tmp) in order.iter().zip(&temps) {
        fs::rename(from, tmp)?;
    }
    for (i, tmp) in temps.iter().enumerate() {
        fs::rename(tmp, frames.join(frame_name(state, variant, i)))?;
    }
    Ok(())
}

/// 接在最後面加幾張。
pub fn append(pet_dir: &Path, state: &str, variant: &str, pngs: &[Vec<u8>]) -> io::Result<()> {
    let frames = pet_dir.join(FRAMES_DIR);
    fs::create_dir_all(&frames)?;
    let existing = list(pet_dir, state, variant)?;
    renumber(pet_dir, state, variant, &existing)?;
    for (i, png) in pngs.iter().enumerate() {
        fs::write(
            frames.join(frame_name(state, variant, existing.len() + i)),
            png,
        )?;
    }
    Ok(())
}

pub fn remove(pet_dir: &Path, state: &str, variant: &str, index: usize) -> io::Result<()> {
    let mut files = list(pet_dir, state, variant)?;
    if index >= files.len() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "沒有這一張"));
    }
    fs::remove_file(files.remove(index))?;
    renumber(pet_dir, state, variant, &files)
}

/// 把第 `from` 張搬到第 `to` 的位置（拖曳排序）。
pub fn move_to(
    pet_dir: &Path,
    state: &str,
    variant: &str,
    from: usize,
    to: usize,
) -> io::Result<()> {
    let mut files = list(pet_dir, state, variant)?;
    if from >= files.len() || to >= files.len() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "沒有這一張"));
    }
    let f = files.remove(from);
    files.insert(to, f);
    renumber(pet_dir, state, variant, &files)
}

/// 讀 pet.json 成可以改的 JSON（沒有這個檔就是空的）。
fn manifest_json(pet_dir: &Path) -> io::Result<serde_json::Value> {
    match fs::read_to_string(pet_dir.join(MANIFEST_FILE)) {
        Ok(text) => serde_json::from_str(&text).map_err(io::Error::other),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(serde_json::json!({})),
        Err(e) => Err(e),
    }
}

/// 設定某個動作每秒播幾張；`None` = 拿掉設定、回到預設。pet.json 其他內容原封不動。
pub fn set_fps(pet_dir: &Path, state: &str, fps: Option<f32>) -> io::Result<()> {
    let mut m = manifest_json(pet_dir)?;
    let states = m
        .as_object_mut()
        .ok_or_else(|| io::Error::other("pet.json 不是物件"))?
        .entry(STATES_KEY)
        .or_insert_with(|| serde_json::json!({}));
    let entry = states
        .as_object_mut()
        .ok_or_else(|| io::Error::other("states 不是物件"))?
        .entry(state)
        .or_insert_with(|| serde_json::json!({}));
    let obj = entry
        .as_object_mut()
        .ok_or_else(|| io::Error::other("動作設定不是物件"))?;
    match fps {
        Some(f) => {
            obj.insert(FPS_KEY.into(), serde_json::json!(f));
        }
        None => {
            obj.remove(FPS_KEY);
        }
    }
    write_atomic(
        &pet_dir.join(MANIFEST_FILE),
        serde_json::to_string_pretty(&m)
            .map_err(io::Error::other)?
            .as_bytes(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_pet(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("petkit-edit-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(FRAMES_DIR)).unwrap();
        dir
    }

    fn contents(dir: &Path, state: &str, variant: &str) -> Vec<String> {
        list(dir, state, variant)
            .unwrap()
            .iter()
            .map(|p| fs::read_to_string(p).unwrap())
            .collect()
    }

    #[test]
    fn append_remove_and_reorder_keep_numbering_tight() {
        let dir = temp_pet("order");
        append(
            &dir,
            "walk",
            DEFAULT_VARIANT,
            &[b"a".to_vec(), b"b".to_vec(), b"c".to_vec()],
        )
        .unwrap();
        assert_eq!(contents(&dir, "walk", DEFAULT_VARIANT), ["a", "b", "c"]);
        move_to(&dir, "walk", DEFAULT_VARIANT, 2, 0).unwrap();
        assert_eq!(contents(&dir, "walk", DEFAULT_VARIANT), ["c", "a", "b"]);
        remove(&dir, "walk", DEFAULT_VARIANT, 1).unwrap();
        assert_eq!(contents(&dir, "walk", DEFAULT_VARIANT), ["c", "b"]);
        let names: Vec<String> = list(&dir, "walk", DEFAULT_VARIANT)
            .unwrap()
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            ["walk_01.png", "walk_02.png"],
            "no gaps after removing"
        );
        append(&dir, "walk", DEFAULT_VARIANT, &[b"d".to_vec()]).unwrap();
        assert_eq!(contents(&dir, "walk", DEFAULT_VARIANT), ["c", "b", "d"]);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn idle_variants_are_separate_groups() {
        let dir = temp_pet("variants");
        append(&dir, "idle", DEFAULT_VARIANT, &[b"stand".to_vec()]).unwrap();
        append(
            &dir,
            "idle",
            "2",
            &[b"scratch1".to_vec(), b"scratch2".to_vec()],
        )
        .unwrap();
        assert_eq!(variants(&dir, "idle").unwrap(), [DEFAULT_VARIANT, "2"]);
        assert!(dir.join(FRAMES_DIR).join("idle-2_02.png").is_file());
        remove(&dir, "idle", "2", 0).unwrap();
        assert_eq!(contents(&dir, "idle", "2"), ["scratch2"]);
        assert_eq!(
            contents(&dir, "idle", DEFAULT_VARIANT),
            ["stand"],
            "other group untouched"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn speed_is_written_without_touching_other_settings() {
        let dir = temp_pet("fps");
        fs::write(dir.join(MANIFEST_FILE), r#"{ "spec": 1, "name": "小白", "editable": true, "states": { "sleep": { "heightRatio": 0.7 } } }"#).unwrap();
        set_fps(&dir, "sleep", Some(3.0)).unwrap();
        let m = manifest_json(&dir).unwrap();
        assert_eq!(m["name"], "小白");
        assert_eq!(m["states"]["sleep"]["heightRatio"], 0.7);
        assert_eq!(m["states"]["sleep"]["fps"], 3.0);
        set_fps(&dir, "sleep", None).unwrap();
        assert!(
            manifest_json(&dir).unwrap()["states"]["sleep"]
                .get("fps")
                .is_none()
        );
        let _ = fs::remove_dir_all(dir);
    }
}
