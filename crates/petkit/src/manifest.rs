//! `pet.json`：每個欄位都是選用的，沒有這個檔也是合法的桌寵包。

use std::collections::BTreeMap;

use serde::Deserialize;

/// 這一版引擎讀得懂的最高規範版本。
pub const SUPPORTED_SPEC: u32 = 1;

#[derive(Debug, Default, Deserialize)]
pub struct Manifest {
    pub spec: Option<u32>,
    pub id: Option<String>,
    pub name: Option<String>,
    pub author: Option<String>,
    pub accent: Option<String>,
    /// 使用者在設定程式裡自己做的桌寵：可以補動作、可以刪。內建的不行（更新時會被蓋回去）。
    #[serde(default)]
    pub editable: bool,
    #[serde(default)]
    pub states: BTreeMap<String, StateSpec>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateSpec {
    #[serde(flatten)]
    pub timing: TimingSpec,
    pub procedural: Option<bool>,
    pub speed: Option<f32>,
    pub height_ratio: Option<f32>,
    #[serde(rename = "loop")]
    pub looping: Option<bool>,
    #[serde(default)]
    pub variants: Vec<VariantSpec>,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct VariantSpec {
    pub id: String,
    pub weight: Option<u32>,
    #[serde(flatten)]
    pub timing: TimingSpec,
    pub procedural: Option<bool>,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct TimingSpec {
    /// 只拿來核對：實際張數以 `frames/` 裡的檔案為準。
    pub frames: Option<usize>,
    pub fps: Option<f32>,
    pub durations: Option<Vec<u32>>,
}
