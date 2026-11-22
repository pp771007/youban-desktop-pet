//! 讀桌寵包資料夾：掃 `frames/` 的檔名、套上 `pet.json`、解析缺圖時的替代規則。
//! 這一層只管「有哪些圖、怎麼播」，不解碼圖片（那是平台層的事）。

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use crate::manifest::{Manifest, StateSpec, SUPPORTED_SPEC, TimingSpec, VariantSpec};
use crate::states::{self, Fallback};

pub const MANIFEST_FILE: &str = "pet.json";
pub const FRAMES_DIR: &str = "frames";
const FRAME_EXT: &str = "png";
pub const DEFAULT_VARIANT: &str = "default";
const DEFAULT_WEIGHT: u32 = 1;
/// 替代鏈的最大長度；規範內建的鏈最長三步（talk → notify → idle），超過代表表格寫出了循環。
const MAX_FALLBACK_HOPS: usize = 8;

#[derive(Debug)]
pub enum PackError {
    Io(PathBuf, std::io::Error),
    Manifest(PathBuf, serde_json::Error),
    SpecTooNew(u32),
    MissingIdle,
    DurationsMismatch { state: String, variant: String, frames: usize, durations: usize },
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(p, e) => write!(f, "讀不到 {}：{e}", p.display()),
            Self::Manifest(p, e) => write!(f, "{} 格式錯誤：{e}", p.display()),
            Self::SpecTooNew(v) => write!(f, "這個桌寵包需要較新版的程式（規範第 {v} 版，目前支援到第 {SUPPORTED_SPEC} 版）"),
            Self::MissingIdle => write!(f, "缺少站姿圖：frames/ 裡至少要有一張 idle_01.png"),
            Self::DurationsMismatch { state, variant, frames, durations } => write!(
                f,
                "{state}（{variant}）有 {frames} 張圖，但 durations 寫了 {durations} 個時間"
            ),
        }
    }
}

impl std::error::Error for PackError {}

#[derive(Debug, Clone)]
pub struct Variant {
    pub id: String,
    pub weight: u32,
    pub frames: Vec<PathBuf>,
    /// 每一張停多久（毫秒），長度與 `frames` 相同。
    pub durations_ms: Vec<u32>,
    pub procedural: bool,
}

#[derive(Debug, Clone)]
pub struct StateAnim {
    pub variants: Vec<Variant>,
    pub looping: bool,
    pub speed: Option<f32>,
    pub height_ratio: f32,
}

#[derive(Debug)]
pub struct PetPack {
    pub id: String,
    pub name: String,
    pub accent: Option<String>,
    pub editable: bool,
    pub dir: PathBuf,
    pub states: BTreeMap<String, StateAnim>,
}

/// `resolve` 的結果：要播哪個狀態的圖。
#[derive(Debug)]
pub struct Resolution<'a> {
    /// 圖實際來自哪個狀態（缺圖時是替代的那個）。
    pub source: &'a str,
    pub anim: &'a StateAnim,
    /// `true` = 用的是這個狀態自己的圖。
    pub own: bool,
}

impl PetPack {
    /// 依替代規則找出 `state` 要用的圖；`None` = 這個過場動作沒有圖，略過不播。
    pub fn resolve(&self, state: &str) -> Option<Resolution<'_>> {
        let mut current: &str = state;
        for _ in 0..MAX_FALLBACK_HOPS {
            if let Some((key, anim)) = self.states.get_key_value(current) {
                return Some(Resolution { source: key, anim, own: current == state });
            }
            match states::defaults(current).fallback {
                Some(Fallback::State(next)) => current = next,
                Some(Fallback::Skip) => return None,
                // 只有 idle 沒有替代，而載入時已保證 idle 存在。
                None => unreachable!("idle 在載入時已驗證存在"),
            }
        }
        unreachable!("狀態表的替代鏈出現循環：{state}")
    }
}

/// 解析 `<state>[-<variant>]_<nn>.png`。
pub fn parse_frame_name(file_name: &str) -> Option<(String, String, u32)> {
    let stem = file_name.strip_suffix(&format!(".{FRAME_EXT}"))?;
    let (name, index) = stem.rsplit_once('_')?;
    let index: u32 = index.parse().ok()?;
    let (state, variant) = match name.split_once('-') {
        Some((s, v)) if !v.is_empty() => (s, v),
        Some(_) => return None,
        None => (name, DEFAULT_VARIANT),
    };
    if state.is_empty() {
        return None;
    }
    Some((state.to_ascii_lowercase(), variant.to_ascii_lowercase(), index))
}

pub fn load(dir: &Path) -> Result<PetPack, PackError> {
    let manifest = read_manifest(dir)?;
    if let Some(spec) = manifest.spec
        && spec > SUPPORTED_SPEC
    {
        return Err(PackError::SpecTooNew(spec));
    }

    let mut found: BTreeMap<String, BTreeMap<String, Vec<(u32, PathBuf)>>> = BTreeMap::new();
    let frames_dir = dir.join(FRAMES_DIR);
    let entries = fs::read_dir(&frames_dir).map_err(|e| PackError::Io(frames_dir.clone(), e))?;
    for entry in entries {
        let entry = entry.map_err(|e| PackError::Io(frames_dir.clone(), e))?;
        let file_name = entry.file_name();
        let Some((state, variant, index)) = file_name.to_str().and_then(parse_frame_name) else {
            continue;
        };
        found.entry(state).or_default().entry(variant).or_default().push((index, entry.path()));
    }
    if !found.contains_key(states::IDLE) {
        return Err(PackError::MissingIdle);
    }

    let mut anims = BTreeMap::new();
    for (state, variants) in found {
        let spec = manifest.states.get(&state).cloned().unwrap_or_default();
        anims.insert(state.clone(), build_state(&state, &spec, variants)?);
    }

    let folder = dir.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_owned();
    Ok(PetPack {
        id: manifest.id.unwrap_or_else(|| folder.clone()),
        name: manifest.name.unwrap_or(folder),
        accent: manifest.accent,
        editable: manifest.editable,
        dir: dir.to_owned(),
        states: anims,
    })
}

fn read_manifest(dir: &Path) -> Result<Manifest, PackError> {
    let path = dir.join(MANIFEST_FILE);
    match fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| PackError::Manifest(path, e)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Manifest::default()),
        Err(e) => Err(PackError::Io(path, e)),
    }
}

fn build_state(
    state: &str,
    spec: &StateSpec,
    variants: BTreeMap<String, Vec<(u32, PathBuf)>>,
) -> Result<StateAnim, PackError> {
    let defaults = states::defaults(state);
    let mut out = Vec::with_capacity(variants.len());
    for (id, mut frames) in variants {
        frames.sort_by_key(|(index, _)| *index);
        let frames: Vec<PathBuf> = frames.into_iter().map(|(_, p)| p).collect();
        let vspec = find_variant_spec(spec, &id);
        // 變化款自己的設定優先，沒寫的沿用狀態層的設定。
        let timing = vspec.map(|v| &v.timing).filter(|t| t.fps.is_some() || t.durations.is_some()).unwrap_or(&spec.timing);
        let durations_ms = durations(state, &id, timing, frames.len(), defaults.fps)?;
        out.push(Variant {
            weight: vspec.and_then(|v| v.weight).unwrap_or(DEFAULT_WEIGHT),
            procedural: vspec.and_then(|v| v.procedural).or(spec.procedural).unwrap_or(true),
            id,
            frames,
            durations_ms,
        });
    }
    Ok(StateAnim {
        variants: out,
        looping: spec.looping.unwrap_or(defaults.looping),
        speed: spec.speed,
        height_ratio: spec.height_ratio.unwrap_or(defaults.height_ratio),
    })
}

fn find_variant_spec<'a>(spec: &'a StateSpec, id: &str) -> Option<&'a VariantSpec> {
    spec.variants.iter().find(|v| v.id.eq_ignore_ascii_case(id))
}

fn durations(state: &str, variant: &str, timing: &TimingSpec, frames: usize, default_fps: f32) -> Result<Vec<u32>, PackError> {
    if let Some(list) = &timing.durations {
        if list.len() != frames {
            return Err(PackError::DurationsMismatch {
                state: state.to_owned(),
                variant: variant.to_owned(),
                frames,
                durations: list.len(),
            });
        }
        return Ok(list.clone());
    }
    let fps = timing.fps.filter(|f| *f > 0.0).unwrap_or(default_fps);
    Ok(vec![(1000.0 / fps).round() as u32; frames])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn temp_pack(files: &[&str], manifest: Option<&str>) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("petkit-test-{}-{n}", std::process::id())).join("mypet");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(FRAMES_DIR)).unwrap();
        for f in files {
            fs::write(dir.join(FRAMES_DIR).join(f), b"").unwrap();
        }
        if let Some(m) = manifest {
            fs::write(dir.join(MANIFEST_FILE), m).unwrap();
        }
        dir
    }

    #[test]
    fn parses_frame_names() {
        assert_eq!(parse_frame_name("idle_01.png"), Some(("idle".into(), "default".into(), 1)));
        assert_eq!(parse_frame_name("idle-look_03.png"), Some(("idle".into(), "look".into(), 3)));
        assert_eq!(parse_frame_name("Walk_10.png"), Some(("walk".into(), "default".into(), 10)));
        assert_eq!(parse_frame_name("reference.png"), None);
        assert_eq!(parse_frame_name("idle_01.jpg"), None);
        assert_eq!(parse_frame_name("idle-_01.png"), None);
    }

    #[test]
    fn single_image_pack_covers_every_state() {
        let pack = load(&temp_pack(&["idle_01.png"], None)).unwrap();
        assert_eq!(pack.name, "mypet");
        for state in [states::WALK, states::DRAG, states::FALL, states::SLEEP, states::NOTIFY, states::LAND, states::TALK, "eat"] {
            let r = pack.resolve(state).unwrap();
            assert_eq!(r.source, states::IDLE, "{state}");
            assert!(!r.own);
        }
        assert!(pack.resolve(states::BLINK).is_none());
        assert!(pack.resolve(states::YAWN).is_none());
    }

    #[test]
    fn own_frames_win_over_fallback_and_chain_follows_table() {
        let pack = load(&temp_pack(&["idle_01.png", "notify_01.png", "fall_01.png"], None)).unwrap();
        let talk = pack.resolve(states::TALK).unwrap();
        assert_eq!(talk.source, states::NOTIFY);
        let land = pack.resolve(states::LAND).unwrap();
        assert_eq!(land.source, states::FALL);
        assert!(pack.resolve(states::NOTIFY).unwrap().own);
    }

    #[test]
    fn missing_idle_is_rejected() {
        assert!(matches!(load(&temp_pack(&["walk_01.png"], None)), Err(PackError::MissingIdle)));
    }

    #[test]
    fn manifest_timing_and_variants() {
        let manifest = r#"{
            "name": "水豚",
            "states": {
                "idle": { "fps": 2, "variants": [ { "id": "look", "weight": 3, "durations": [100, 200], "procedural": false } ] },
                "walk": { "fps": 10, "procedural": false, "speed": 55 }
            }
        }"#;
        let files = ["idle_01.png", "idle_02.png", "idle-look_02.png", "idle-look_01.png", "walk_01.png"];
        let pack = load(&temp_pack(&files, Some(manifest))).unwrap();
        assert_eq!(pack.name, "水豚");
        let idle = &pack.states[states::IDLE];
        let default = idle.variants.iter().find(|v| v.id == DEFAULT_VARIANT).unwrap();
        assert_eq!(default.durations_ms, vec![500, 500]);
        assert!(default.procedural);
        let look = idle.variants.iter().find(|v| v.id == "look").unwrap();
        assert_eq!(look.weight, 3);
        assert_eq!(look.durations_ms, vec![100, 200]);
        assert!(!look.procedural);
        assert!(look.frames[0].ends_with("idle-look_01.png"));
        let walk = &pack.states[states::WALK];
        assert_eq!(walk.speed, Some(55.0));
        assert!(!walk.variants[0].procedural);
        assert_eq!(walk.variants[0].durations_ms, vec![100]);
    }

    #[test]
    fn durations_must_match_frame_count() {
        let manifest = r#"{ "states": { "idle": { "durations": [100, 200, 300] } } }"#;
        let err = load(&temp_pack(&["idle_01.png"], Some(manifest))).unwrap_err();
        assert!(matches!(err, PackError::DurationsMismatch { frames: 1, durations: 3, .. }));
    }

    #[test]
    fn newer_spec_is_rejected() {
        let err = load(&temp_pack(&["idle_01.png"], Some(r#"{ "spec": 99 }"#))).unwrap_err();
        assert!(matches!(err, PackError::SpecTooNew(99)));
    }
}
