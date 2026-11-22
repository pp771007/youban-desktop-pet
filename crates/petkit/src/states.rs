//! 規範內建的狀態 id 與各自的預設值（`docs/pet-pack-spec.md` 第 1 節的狀態表）。

pub const IDLE: &str = "idle";
pub const WALK: &str = "walk";
pub const DRAG: &str = "drag";
pub const FALL: &str = "fall";
pub const SLEEP: &str = "sleep";
pub const NOTIFY: &str = "notify";
pub const BLINK: &str = "blink";
pub const HAPPY: &str = "happy";
pub const SIT: &str = "sit";
pub const LAND: &str = "land";
pub const YAWN: &str = "yawn";
pub const WAKE: &str = "wake";
pub const TALK: &str = "talk";
pub const HOP: &str = "hop";
pub const LOOK: &str = "look";
pub const DOZE: &str = "doze";
/// 爬螢幕邊。沒有攀爬圖時引擎把走路圖轉 90 度、腳貼著牆走上去。
pub const CLIMB: &str = "climb";
/// 被甩暈。沒有頭暈圖時引擎拿站姿圖左右搖晃。
pub const DIZZY: &str = "dizzy";

/// 引擎自己畫的特技：不在規範裡、不用出圖，拿站姿圖轉、翻出來。
pub const SPIN: &str = "spin";
pub const ROLL: &str = "roll";
/// 吃點心（引擎自己畫：低頭一口一口咬）。
pub const EAT: &str = "eat";

/// 規範裡所有的狀態（設定程式驗證「補動作」的對象用）。
pub const ALL: [&str; 18] = [IDLE, WALK, DRAG, FALL, SLEEP, NOTIFY, BLINK, HAPPY, SIT, LAND, YAWN, WAKE, TALK, HOP, LOOK, DOZE, CLIMB, DIZZY];

/// 缺圖時怎麼辦。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fallback {
    /// 改用另一個狀態的圖（那個狀態也缺就繼續往下找）。
    State(&'static str),
    /// 過場動作（眨眼、打哈欠、伸懶腰）：直接略過，不播。
    Skip,
}

#[derive(Debug, Clone, Copy)]
pub struct StateDefaults {
    pub fallback: Option<Fallback>,
    pub looping: bool,
    pub fps: f32,
    pub height_ratio: f32,
}

const fn d(fallback: Option<Fallback>, looping: bool, fps: f32, height_ratio: f32) -> StateDefaults {
    StateDefaults { fallback, looping, fps, height_ratio }
}

const TO_IDLE: Option<Fallback> = Some(Fallback::State(IDLE));

/// 規範之外的自訂狀態（之後功能模組自己加的）一律退回 `idle`。
pub fn defaults(state: &str) -> StateDefaults {
    match state {
        IDLE => d(None, true, 4.0, 1.00),
        WALK => d(TO_IDLE, true, 8.0, 1.00),
        DRAG => d(TO_IDLE, true, 6.0, 1.05),
        FALL => d(TO_IDLE, true, 8.0, 1.00),
        SLEEP => d(TO_IDLE, true, 2.0, 0.55),
        NOTIFY => d(TO_IDLE, true, 6.0, 1.00),
        BLINK => d(Some(Fallback::Skip), false, 8.0, 1.00),
        HAPPY => d(TO_IDLE, true, 6.0, 1.05),
        SIT => d(TO_IDLE, true, 4.0, 0.80),
        LAND => d(Some(Fallback::State(FALL)), false, 8.0, 0.85),
        YAWN => d(Some(Fallback::Skip), false, 6.0, 1.00),
        WAKE => d(Some(Fallback::Skip), false, 6.0, 1.10),
        TALK => d(Some(Fallback::State(NOTIFY)), true, 6.0, 1.00),
        HOP => d(TO_IDLE, false, 8.0, 1.00),
        LOOK => d(TO_IDLE, false, 6.0, 1.00),
        DOZE => d(TO_IDLE, true, 3.0, 0.95),
        CLIMB => d(Some(Fallback::State(WALK)), true, 8.0, 1.00),
        DIZZY => d(TO_IDLE, true, 4.0, 1.00),
        _ => d(TO_IDLE, true, 6.0, 1.00),
    }
}
