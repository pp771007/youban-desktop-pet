//! 桌寵的行為與物理：純邏輯，不碰 Win32，時間由呼叫端餵進來（可以快轉測試）。
//!
//! 座標一律是螢幕實體像素，`(x, y)` 是腳底中心點。
//! 所有動作都先用「引擎墊底效果」做（只有一張站姿圖也能動）；桌寵包補上同名狀態的圖之後自動改用圖。

use petkit::config::Activity;
use petkit::states;

/// 拖曳要移動超過這麼多（DIP）才算拖，以內算點一下（點心和球也用同一個）。
pub const DRAG_THRESHOLD_DIP: f32 = 4.0;
const GRAVITY_DIP: f32 = 2600.0;
/// 丟出去：放手時的速度上限（DIP/秒）、撞牆後剩下的水平速度、空氣阻力（每秒衰減比例）。
const MAX_THROW_DIP: f32 = 2600.0;
const WALL_BOUNCE: f32 = 0.55;
const AIR_DRAG: f32 = 0.6;
/// 飛行中依水平速度微微翻滾（度／每 DIP/秒）。
const TUMBLE_DEG_PER_SPEED: f32 = 0.012;
const MAX_TUMBLE_DEG: f32 = 22.0;
/// 腳底離平台（視窗頂端）這麼近就算站在上面。
const PLATFORM_SNAP_PX: f32 = 3.0;
/// 身體跟禁止範圍重疊超過這麼多像素才算碰到（貼著邊站、爬邊時不算）。
const BLOCK_TOUCH_PX: f32 = 0.5;
/// 禁止範圍的頂端當成平台時用的「視窗代碼」：從這裡往下數（真的視窗代碼都是正的）。
const BLOCK_WINDOW_BASE: isize = -1;
/// 撞上看不見的牆之後，那一段牆亮多久。
const BUMP_GLOW_SECS: f64 = 0.6;
/// 站著的視窗一格內移動超過這麼多像素，算「被載著走」（講話系統會想喊一聲）。
const RIDE_NOTICE_PX: f32 = 6.0;
/// 「被載著走」最快多久通知一次。
const RIDE_NOTICE_EVERY_SECS: f64 = 8.0;
/// 走到螢幕邊（或活動範圍的左右邊）時去爬牆的機率（安靜時不爬）。
const CLIMB_CHANCE: f32 = 0.5;
const CLIMB_CHANCE_LIVELY: f32 = 0.8;
/// 爬牆速度是走路速度的幾倍。
/// 爬牆速度是走路速度的幾倍（動畫預覽看過：太慢會很拖沓）。
const CLIMB_SPEED_MUL: f32 = 1.6;
/// 爬到「地面往上 X～螢幕頂往下一個身高」之間的隨機高度（X 以桌寵身高計）。
const CLIMB_MIN_RISE_HEIGHTS: f32 = 1.0;
/// 最多爬多高（以桌寵身高計），大螢幕上才不會爬半分鐘。
const CLIMB_MAX_RISE_HEIGHTS: f32 = 5.0;
const CLING_SECS: (f32, f32) = (1.5, 4.0);
/// 從牆上跳下來的初速（DIP/秒）：往外、往上。
const LEAP_OUT_DIP: f32 = 380.0;
const LEAP_UP_DIP: f32 = 420.0;
const DEFAULT_WALK_SPEED_DIP: f32 = 40.0;
const IDLE_SECS: (f32, f32) = (2.5, 6.0);
const IDLE_SECS_LIVELY: (f32, f32) = (1.5, 4.0);
const IDLE_SECS_CALM: (f32, f32) = (4.0, 9.0);
const WALK_SECS: (f32, f32) = (2.0, 6.0);
/// 散步速度的隨機倍率；偶爾小跑步。
const WALK_SPEED_MUL: (f32, f32) = (0.7, 1.3);
const RUN_CHANCE: f32 = 0.15;
const RUN_CHANCE_LIVELY: f32 = 0.3;
const RUN_SPEED_MUL: f32 = 1.9;
/// 每秒踏幾步（小跑步時跟著速度加快）。
const STEPS_PER_SEC: f32 = 2.2;
/// 落地：先壓扁、再微微彈高、最後停穩（阻尼彈簧），所以比單純壓扁長一點。
const LAND_SECS: f32 = 0.45;
const HAPPY_SECS: f32 = 0.8;
const HOP_SECS: f32 = 0.55;
/// 跳躍的三段比例：先蹲（預備動作）、空中、落地壓扁。
const JUMP_CROUCH: f32 = 0.2;
const JUMP_LAND: f32 = 0.18;
/// 轉身：先壓窄、到一半換面、再撐回來，不要瞬間翻面。
const TURN_SECS: f32 = 0.16;
/// 走路起步加速、停步減速各花多久。
const WALK_EASE_SECS: f32 = 0.3;
/// 從牆上跳下前，最後這段時間身體往牆壓低（預備動作）。
const LEAP_WINDUP_SECS: f32 = 0.25;
const LOOK_SECS: f32 = 1.8;
const STRETCH_SECS: f32 = 1.1;
const DOZE_SECS: f32 = 2.6;
const YAWN_SECS: f32 = 1.6;
/// 坐下來休息多久（秒，隨機範圍）。
const SIT_SECS: (f32, f32) = (6.0, 14.0);
/// 待機時每隔這麼久眨一次眼，每次閉眼這麼久。
const BLINK_EVERY_SECS: f32 = 3.7;
const BLINK_SECS: f32 = 0.14;
const PETTED_SECS: f32 = 1.4;
/// 講話時身體一上一下的頻率（次／秒）與幅度。
const TALK_BOB_HZ: f32 = 2.4;
const TALK_BOB: f32 = 0.025;
/// 轉圈圈：花多久、轉幾圈、順便跳多高（以身高計）。
const SPIN_SECS: f32 = 0.9;
const SPIN_TURNS: f32 = 2.0;
const SPIN_HOP: f32 = 0.12;
const SPIN_MIN_WIDTH: f32 = 0.2;
/// 被點時改成轉圈圈（而不是跳一下）的機率。
const SPIN_ON_CLICK_CHANCE: f32 = 0.4;
/// 翻滾：花多久、滾多遠（以身高計）；翻到橫的時候身體抬高一點，不會陷進地面。
const ROLL_SECS: f32 = 1.0;
const ROLL_DIST_HEIGHTS: f32 = 1.3;
const ROLL_LIFT: f32 = 0.25;
/// 追滑鼠：滑鼠高出地面這麼多個身高就搆不到了。
const CHASE_MAX_ABOVE_HEIGHTS: f32 = 3.0;
/// 逗牠：滑鼠在牠身旁（不在身上，在身上晃是摸頭）這麼多個身高以內快速左右晃，
/// `TEASE_WINDOW_SECS` 內換方向 `TEASE_REVERSALS` 次，牠才跑過來撲。平常工作時滑鼠經過、停在附近都不算 ——
/// 牠不會自己跑去滑鼠旁邊干擾人。
const TEASE_NOTICE_HEIGHTS: f32 = 6.0;
const TEASE_REVERSALS: usize = 3;
const TEASE_WINDOW_SECS: f64 = 1.0;
/// 每一下至少要晃這麼遠（以身高計），手抖不算。
const TEASE_STEP_HEIGHTS: f32 = 0.05;
const CHASE_SPEED_MUL: f32 = 2.4;
const CHASE_SECS: f32 = 6.0;
/// 追到離滑鼠這麼近（以身高計）就撲上去。
const POUNCE_RANGE_HEIGHTS: f32 = 0.6;
const POUNCE_SECS: f32 = 0.7;
const POUNCE_HEIGHT: f32 = 0.55;
const POUNCE_SPEED_HEIGHTS: f32 = 1.2;
/// 去拿點心／球：搆得到的距離（以身高計）、跑多快、吃多久。
const FETCH_REACH_HEIGHTS: f32 = 0.45;
const FETCH_SPEED_MUL: f32 = 2.0;
/// 東西跟牠差這麼多個身高以內算「同一層」（地面或同一個視窗頂）。
const FETCH_SAME_LEVEL_HEIGHTS: f32 = 0.6;
/// 東西落在搆不到的地方：等這麼久就放棄，放棄後這段時間不再理它。
const FETCH_PATIENCE_SECS: f64 = 6.0;
const FETCH_IGNORE_SECS: f64 = 20.0;
const EAT_SECS: f32 = 2.2;
const EAT_BITES_PER_SEC: f32 = 3.0;
/// 甩：這段時間內被甩這麼多次（放手時快過這個速度，DIP/秒），落地後頭暈。
const DIZZY_THROW_SPEED_DIP: f32 = 1200.0;
const DIZZY_THROWS: usize = 3;
const DIZZY_WINDOW_SECS: f64 = 40.0;
/// 頭暈多久、搖晃的角度與頻率；中途再冒一次螺旋（螺旋特效本身只播 1.8 秒）。
const DIZZY_SECS: f32 = 3.4;
const DIZZY_SWAY_DEG: f32 = 11.0;
const DIZZY_SWAY_HZ: f32 = 0.9;
const DIZZY_RESPIN_SECS: f32 = 1.6;
/// 睡著後，系統閒置時間小於這個就醒來（= 使用者剛碰了滑鼠鍵盤）。
const WAKE_INPUT_SECS: f32 = 1.0;
/// 打瞌睡只在使用者有一陣子沒動電腦時才會出現。
const DOZE_AFTER_IDLE_SECS: f32 = 60.0;
const DRAG_MAX_ANGLE: f32 = 28.0;
const NOTIFY_CYCLE_SECS: f32 = 1.6;
/// 拖曳時鐘擺：水平速度換算成目標角度的係數、彈簧與阻尼。
const DRAG_ANGLE_PER_SPEED: f32 = 0.025;
const DRAG_SPRING: f32 = 60.0;
const DRAG_DAMPING: f32 = 9.0;
/// 拖得比這個快（DIP/秒）就冒汗，最快每隔這麼久冒一次。
const SWEAT_SPEED_DIP: f32 = 1400.0;
const SWEAT_EVERY_SECS: f64 = 0.8;
/// 摸摸頭：滑鼠在桌寵身上這段時間內來回換方向這麼多次。
const PET_REVERSALS: usize = 4;
const PET_WINDOW_SECS: f64 = 1.2;
/// 滑鼠移動小於這個（像素）不算換方向，避免手抖誤判。
const PET_MIN_STEP_PX: f32 = 2.0;
/// 滑鼠在這麼多個桌寵身高以內，桌寵會轉頭看它。
const LOOK_AT_CURSOR_HEIGHTS: f32 = 2.5;
const LEAN_DEG: f32 = 3.0;

const FPS_ACTIVE: u32 = 60;
const FPS_IDLE: u32 = 30;
const FPS_SLEEP: u32 = 15;

/// 待機結束後接下來做什麼（權重）。
const NEXT_WALK: f32 = 45.0;
const NEXT_HOP: f32 = 12.0;
const NEXT_LOOK: f32 = 15.0;
const NEXT_STRETCH: f32 = 10.0;
const NEXT_DOZE: f32 = 8.0;
const NEXT_YAWN: f32 = 6.0;
const NEXT_SIT: f32 = 10.0;
/// 安靜模式不散步，坐下的機會多一點。
const SIT_CALM_MUL: f32 = 2.0;
const NEXT_IDLE: f32 = 10.0;
const NEXT_SPIN: f32 = 5.0;
const NEXT_ROLL: f32 = 6.0;
/// 小跳時哼歌、張望時冒問號的機率。
const NOTE_ON_HOP_CHANCE: f32 = 0.4;
const QUESTION_ON_LOOK_CHANCE: f32 = 0.5;

/// 可以站的地方（視窗頂端露出來的那一段）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Platform {
    pub left: f32,
    pub right: f32,
    pub y: f32,
    /// 哪個視窗的頂端（同一個視窗被別的視窗擋住時會切成好幾段，段段同一個值）。
    pub window: isize,
    /// 那個視窗的左邊緣：視窗被拖動時用它算移動了多少（段的左右會因為遮擋改變，視窗左邊緣不會）。
    pub window_left: f32,
}

/// 桌面上的點心或球（主程式的物理算好的位置）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ItemView {
    /// 底部中心點。
    pub x: f32,
    pub y: f32,
    pub food: bool,
    /// 落在地上或視窗頂（不在空中）。
    pub grounded: bool,
    /// 桌寵還想去拿（球玩夠了就是 `false`）。
    pub wanted: bool,
}

/// 螢幕上的一個框（實體像素）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

/// 禁止範圍的頂端：可以站、可以走過去（頂端上方留不下一個身高的不算，會被螢幕上緣切掉）。
pub fn block_platforms(blocks: &[Rect], ceiling: f32, pet_height: f32) -> Vec<Platform> {
    blocks
        .iter()
        .enumerate()
        .filter(|(_, b)| b.top - pet_height >= ceiling)
        .map(|(i, b)| Platform { left: b.left, right: b.right, y: b.top, window: BLOCK_WINDOW_BASE - i as isize, window_left: b.left })
        .collect()
}

/// 桌寵正碰著的那段看不見的牆（禁止範圍的邊與頂端、活動範圍的邊）：繪圖層在那裡亮一小段紅光。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Glow {
    /// 腳下的那一段（橫的），`(x, y)` 是腳底。
    Floor { x: f32, y: f32 },
    /// 身旁的那一段（直的），`x` 是牆線、`y` 是那一段的中間。
    Wall { x: f32, y: f32 },
}

/// 每一格由平台層算好的環境。
#[derive(Debug, Clone)]
pub struct Env {
    /// 地面（工作區域底部）。
    pub ground_y: f32,
    /// 腳底中心點能走到的左右範圍。
    pub min_x: f32,
    pub max_x: f32,
    pub px_per_dip: f32,
    /// 整台電腦多久沒有滑鼠鍵盤輸入。
    pub idle_input_secs: f32,
    pub sleep_after_secs: f32,
    /// 走路速度（DIP/秒）；`None` 用預設。
    pub walk_speed_dip: Option<f32>,
    /// 滑鼠位置（螢幕實體像素）。
    pub cursor: Option<(f32, f32)>,
    /// 桌寵站姿身高、半個身寬（實體像素）。
    pub pet_height: f32,
    pub pet_half_width: f32,
    /// 視窗頂端（空的 = 不站到視窗上）。
    pub platforms: Vec<Platform>,
    pub activity: Activity,
    /// 不能進去的地方：走進去前會爬上去、從頂端走過去（頂端要另外放進 `platforms`，見 `block_platforms`）。
    pub blocks: Vec<Rect>,
    /// 地面、左右兩面牆是活動範圍的框（看不見），不是工作列或螢幕邊：碰到時要亮。
    pub invisible_floor: bool,
    pub invisible_walls: (bool, bool),
    /// 工作區域的左右邊與頂端（爬牆用）。
    pub wall_left: f32,
    pub wall_right: f32,
    pub ceiling: f32,
    pub item: Option<ItemView>,
}

impl Env {
    /// 只在 `area` 裡面活動：框的底邊是地板、左右是牆、頂端是天花板，框外的視窗頂不能站。
    pub fn confine_to(&mut self, area: Rect) {
        self.invisible_floor = area.bottom < self.ground_y;
        self.invisible_walls = (area.left > self.wall_left, area.right < self.wall_right);
        self.ground_y = self.ground_y.min(area.bottom);
        self.ceiling = self.ceiling.max(area.top);
        self.wall_left = self.wall_left.max(area.left);
        self.wall_right = self.wall_right.min(area.right);
        self.min_x = self.wall_left + self.pet_half_width;
        self.max_x = (self.wall_right - self.pet_half_width).max(self.min_x);
        let (top, bottom, left, right) = (self.ceiling + self.pet_height, self.ground_y, self.wall_left, self.wall_right);
        self.platforms.retain_mut(|p| {
            p.left = p.left.max(left);
            p.right = p.right.min(right);
            p.y >= top && p.y < bottom && p.right > p.left
        });
    }
}

/// 旋轉的軸心。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pivot {
    Feet,
    /// 被拎起來：以頭頂為軸。
    Top,
    /// 翻滾：以身體中心為軸。
    Center,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Input {
    PointerDown { x: f32, y: f32 },
    PointerMove { x: f32, y: f32 },
    PointerUp,
    /// 滑鼠在桌寵身上移動（沒按著）。
    Hover { x: f32 },
}

/// 頭上冒出來的表情特效（由繪圖層用向量畫）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Emote {
    Hearts,
    Sweat,
    Alert,
    Dust,
    Note,
    Question,
    /// 紀念日的閃亮星星。
    Star,
    /// 被戳太多次生氣。
    Anger,
    /// 被甩到頭暈。
    Dizzy,
}

/// 發生在桌寵身上、講話系統會想回應的事（由 `drain_happenings` 取走）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Happening {
    /// 被點了一下。
    Clicked,
    /// 被摸頭摸到開心。
    Petted,
    /// 被甩出去；放手時的速度（DIP/秒）。
    Thrown { speed_dip: f32 },
    /// 被甩暈了（落地之後）。
    Dizzy,
    /// 睡醒了；睡了多久（秒）。
    Woke { slept_secs: f32 },
    /// 站著的視窗被拖動，牠跟著被載走。
    Riding,
    /// 吃完點心。
    Ate,
    /// 踢了球一腳，往 `dir`（-1 左、1 右）。
    Kicked { dir: f32 },
}

/// 動畫預覽工具可以直接觸發的動作。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Demo {
    Hop,
    Look,
    Stretch,
    Doze,
    Petted,
    Walk { secs: f32 },
    ClimbLeft,
    Sleep,
    Sit,
    Yawn,
    Talk,
    Spin,
    Roll,
    Dizzy,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    Idle { until: f64 },
    Walk { dir: f32, until: f64, speed_mul: f32 },
    Drag,
    Fall { vx: f32, vy: f32 },
    Land,
    Sleep,
    Happy,
    /// 有提醒卡在等使用者處理：站著原地跳。
    Notify,
    Hop,
    Look,
    Stretch,
    Doze,
    Yawn,
    /// 坐下來休息一陣子。
    Sit { until: f64 },
    Petted,
    /// 爬螢幕邊：`side` -1 = 左邊、1 = 右邊。
    Climb { side: f32, target_y: f32 },
    /// 爬到定點，貼在牆上停一下。
    Cling { side: f32, until: f64 },
    /// 爬禁止範圍的邊（`side` 同 `Climb`：範圍在牠的哪一邊），爬到頂端 `top` 就翻上去、往前走過寬 `across` 的頂端。
    ClimbOver { side: f32, top: f32, across: f32 },
    /// 講話（頭上有對話泡泡）。
    Talk { until: f64 },
    Spin,
    /// 往 `dir` 翻滾一段。
    Roll { dir: f32 },
    /// 追著滑鼠跑。
    Chase { until: f64 },
    /// 撲向滑鼠。
    Pounce { dir: f32 },
    /// 被甩暈，搖搖晃晃站不穩；`respun` = 中途那次螺旋已經冒過了。
    Dizzy { respun: bool },
    /// 去拿桌面上的點心或球。
    Fetch { since: f64 },
    Eat,
}

#[derive(Debug, Clone, Copy)]
struct Press {
    start_x: f32,
    start_y: f32,
    grab_dx: f32,
    grab_dy: f32,
    dragging: bool,
}

/// 畫這一格要用的姿勢；比例、角度、位移都是「引擎墊底效果」，圖夠順時由繪圖層忽略。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub state: &'static str,
    pub scale_x: f32,
    pub scale_y: f32,
    pub rotation_deg: f32,
    pub pivot: Pivot,
    /// 往上為負（以桌寵身高 = 1 為單位）。
    pub lift: f32,
    pub flip: bool,
    /// 睡覺時 Zzz 的動畫時間（秒）。
    pub zzz: Option<f32>,
    /// 貼在牆上（-1 左牆、1 右牆）：繪圖層把桌寵轉成腳貼著牆；`lift` 這時代表離牆的距離。
    pub climb: Option<f32>,
}

pub struct Pet {
    pub x: f32,
    pub y: f32,
    mode: Mode,
    mode_start: f64,
    facing_left: bool,
    press: Option<Press>,
    last_pointer_x: f32,
    last_pointer_y: f32,
    drag_vx: f32,
    drag_vy: f32,
    drag_angle: f32,
    drag_angle_vel: f32,
    last_sweat: f64,
    hover_last_x: Option<f32>,
    hover_dir: f32,
    reversals: Vec<f64>,
    /// 逗牠：上一次算進去的滑鼠位置、方向、最近幾次換方向的時間。
    tease_anchor_x: Option<f32>,
    tease_dir: f32,
    tease_reversals: Vec<f64>,
    lean: f32,
    /// 開始張望時面向哪邊（張望完要轉回來）。
    look_origin_left: bool,
    /// 上一次看到的活潑程度（決定待機多久）。
    activity: Activity,
    /// 畫翻滾角度時把速度換回 DIP 用（最近一次的環境值）。
    px_per_dip_hint: f32,
    /// 正在轉身：(轉身前面向左, 開始時間)。
    turn_from: Option<(bool, f64)>,
    last_facing_left: bool,
    emotes: Vec<Emote>,
    happenings: Vec<Happening>,
    /// 站在哪個視窗上：(視窗, 上一格看到的視窗左邊緣)。
    riding: Option<(isize, f32)>,
    last_ride_notice: f64,
    /// 放棄搆不到的東西之後，這個時間以前不再理它。
    ignore_item_until: f64,
    /// 最近幾次被用力甩出去的時間（算頭暈用，只留時間窗內的）。
    throws: Vec<f64>,
    /// 甩夠多次了，下一次落地就頭暈。
    dizzy_pending: bool,
    /// 最近一次撞上看不見的牆：(牆線 x, 撞到的高度, 時間)。
    bump: Option<(f32, f32, f64)>,
    rng: u64,
}

impl Pet {
    pub fn new(x: f32, y: f32, now: f64, seed: u64) -> Self {
        let mut pet = Self {
            x,
            y,
            mode: Mode::Idle { until: now },
            mode_start: now,
            facing_left: false,
            press: None,
            last_pointer_x: x,
            last_pointer_y: y,
            drag_vx: 0.0,
            drag_vy: 0.0,
            drag_angle: 0.0,
            drag_angle_vel: 0.0,
            last_sweat: f64::NEG_INFINITY,
            hover_last_x: None,
            hover_dir: 0.0,
            reversals: Vec::new(),
            tease_anchor_x: None,
            tease_dir: 0.0,
            tease_reversals: Vec::new(),
            lean: 0.0,
            look_origin_left: false,
            activity: Activity::default(),
            px_per_dip_hint: 1.0,
            turn_from: None,
            last_facing_left: false,
            emotes: Vec::new(),
            happenings: Vec::new(),
            riding: None,
            last_ride_notice: f64::NEG_INFINITY,
            ignore_item_until: f64::NEG_INFINITY,
            throws: Vec::new(),
            dizzy_pending: false,
            bump: None,
            rng: seed | 1,
        };
        pet.set_idle(now);
        pet
    }

    pub fn is_dragging(&self) -> bool {
        matches!(self.mode, Mode::Drag)
    }

    pub fn state(&self) -> &'static str {
        match self.mode {
            Mode::Idle { .. } => states::IDLE,
            Mode::Walk { .. } => states::WALK,
            Mode::Drag => states::DRAG,
            Mode::Fall { .. } => states::FALL,
            Mode::Land => states::LAND,
            Mode::Sleep => states::SLEEP,
            Mode::Happy | Mode::Petted => states::HAPPY,
            Mode::Notify => states::NOTIFY,
            Mode::Hop => states::HOP,
            Mode::Look => states::LOOK,
            Mode::Stretch => states::WAKE,
            Mode::Climb { .. } | Mode::Cling { .. } | Mode::ClimbOver { .. } => states::CLIMB,
            Mode::Doze => states::DOZE,
            Mode::Yawn => states::YAWN,
            Mode::Sit { .. } => states::SIT,
            Mode::Talk { .. } => states::TALK,
            Mode::Spin => states::SPIN,
            Mode::Roll { .. } => states::ROLL,
            Mode::Chase { .. } => states::WALK,
            Mode::Pounce { .. } => states::HOP,
            Mode::Fetch { .. } => states::WALK,
            Mode::Eat => states::EAT,
            Mode::Dizzy { .. } => states::DIZZY,
        }
    }

    pub fn desired_fps(&self) -> u32 {
        match self.mode {
            Mode::Sleep => FPS_SLEEP,
            Mode::Idle { .. } => FPS_IDLE,
            _ => FPS_ACTIVE,
        }
    }

    /// 這一格新冒出來的表情（取走後清空）。
    pub fn drain_emotes(&mut self) -> Vec<Emote> {
        std::mem::take(&mut self.emotes)
    }

    /// 正站在哪個視窗頂端上（主程式會每一格追蹤那個視窗，拖動時才跟得緊）。
    pub fn riding_window(&self) -> Option<isize> {
        self.riding.map(|(w, _)| w)
    }

    /// 這一格發生的事（取走後清空）。
    pub fn drain_happenings(&mut self) -> Vec<Happening> {
        std::mem::take(&mut self.happenings)
    }

    fn happen(&mut self, h: Happening) {
        const MAX_PENDING: usize = 8;
        if self.happenings.len() < MAX_PENDING {
            self.happenings.push(h);
        }
    }

    /// 開口講話 `secs` 秒：只有在原地閒著時才換成講話動作（走路、睡覺、被拎著就照原本的動作，泡泡照樣出現）。
    pub fn talk(&mut self, secs: f32, now: f64) {
        if matches!(self.mode, Mode::Idle { .. } | Mode::Sit { .. } | Mode::Look | Mode::Talk { .. }) {
            self.set_mode(Mode::Talk { until: now + secs as f64 }, now);
        }
    }

    /// 開心地跳一下（紀念日）。
    pub fn cheer(&mut self, now: f64) {
        if !matches!(self.mode, Mode::Drag | Mode::Fall { .. } | Mode::Climb { .. } | Mode::Cling { .. } | Mode::ClimbOver { .. }) {
            self.emote(Emote::Hearts);
            self.set_mode(Mode::Happy, now);
        }
    }

    /// 泡泡被收掉（使用者點掉、或被提醒卡蓋過）時不要繼續張嘴。
    pub fn stop_talking(&mut self, now: f64) {
        if matches!(self.mode, Mode::Talk { .. }) {
            self.set_idle(now);
        }
    }

    fn emote(&mut self, e: Emote) {
        // 一格之內最多幾個，表情佇列不會無限長大。
        const MAX_PENDING: usize = 8;
        if self.emotes.len() < MAX_PENDING {
            self.emotes.push(e);
        }
    }

    /// 有沒有卡片在等使用者；每一格都可以呼叫（重複呼叫不會重播動作）。
    pub fn set_attention(&mut self, on: bool, now: f64) {
        let calm = matches!(
            self.mode,
            Mode::Idle { .. }
                | Mode::Walk { .. }
                | Mode::Sleep
                | Mode::Happy
                | Mode::Hop
                | Mode::Look
                | Mode::Stretch
                | Mode::Doze
                | Mode::Yawn
                | Mode::Sit { .. }
                | Mode::Petted
                | Mode::Climb { .. }
                | Mode::Cling { .. }
                | Mode::ClimbOver { .. }
                | Mode::Talk { .. }
                | Mode::Spin
                | Mode::Roll { .. }
                | Mode::Chase { .. }
                | Mode::Pounce { .. }
                | Mode::Fetch { .. }
                | Mode::Eat
                | Mode::Dizzy { .. }
        );
        match (on, self.mode) {
            (true, _) if calm => {
                self.emote(Emote::Alert);
                self.set_mode(Mode::Notify, now);
            }
            (false, Mode::Notify) => self.set_idle(now),
            _ => {}
        }
    }

    /// 回傳 `true` = 剛落地（呼叫端可以趁這時存位置）。
    pub fn input(&mut self, input: Input, now: f64, env: &Env) -> bool {
        match input {
            Input::PointerDown { x, y } => {
                self.press = Some(Press { start_x: x, start_y: y, grab_dx: x - self.x, grab_dy: y - self.y, dragging: false });
                self.last_pointer_x = self.x;
                self.last_pointer_y = self.y;
            }
            Input::PointerMove { x, y } => {
                let Some(mut press) = self.press else { return false };
                if !press.dragging {
                    let threshold = DRAG_THRESHOLD_DIP * env.px_per_dip;
                    if (x - press.start_x).hypot(y - press.start_y) < threshold {
                        return false;
                    }
                    press.dragging = true;
                    self.press = Some(press);
                    self.drag_vx = 0.0;
                    self.drag_vy = 0.0;
                    self.drag_angle = 0.0;
                    self.drag_angle_vel = 0.0;
                    self.set_mode(Mode::Drag, now);
                }
                self.x = x - press.grab_dx;
                self.y = y - press.grab_dy;
            }
            Input::PointerUp => {
                let Some(press) = self.press.take() else { return false };
                if press.dragging {
                    // 甩著放手會帶著速度飛出去。
                    let max = MAX_THROW_DIP * env.px_per_dip;
                    let (vx, vy) = (self.drag_vx.clamp(-max, max), self.drag_vy.clamp(-max, max));
                    let speed_dip = vx.hypot(vy) / env.px_per_dip;
                    self.happen(Happening::Thrown { speed_dip });
                    if speed_dip >= DIZZY_THROW_SPEED_DIP {
                        self.throws.retain(|t| now - t <= DIZZY_WINDOW_SECS);
                        self.throws.push(now);
                        if self.throws.len() >= DIZZY_THROWS {
                            self.throws.clear();
                            self.dizzy_pending = true;
                        }
                    }
                    self.set_mode(Mode::Fall { vx, vy }, now);
                } else {
                    self.emote(Emote::Hearts);
                    self.happen(Happening::Clicked);
                    if self.rand01() < SPIN_ON_CLICK_CHANCE {
                        self.set_mode(Mode::Spin, now);
                    } else {
                        self.set_mode(Mode::Happy, now);
                    }
                }
            }
            Input::Hover { x } => self.hover(x, now),
        }
        false
    }

    /// 摸摸頭：滑鼠在身上來回晃，短時間內換方向夠多次就開心。
    fn hover(&mut self, x: f32, now: f64) {
        if !matches!(self.mode, Mode::Idle { .. } | Mode::Walk { .. } | Mode::Hop | Mode::Look | Mode::Stretch | Mode::Doze | Mode::Yawn | Mode::Sit { .. } | Mode::Talk { .. }) {
            self.hover_last_x = None;
            return;
        }
        let Some(last) = self.hover_last_x.replace(x) else { return };
        let dx = x - last;
        if dx.abs() < PET_MIN_STEP_PX {
            return;
        }
        let dir = dx.signum();
        if self.hover_dir != 0.0 && dir != self.hover_dir {
            self.reversals.push(now);
        }
        self.hover_dir = dir;
        self.reversals.retain(|t| now - t <= PET_WINDOW_SECS);
        if self.reversals.len() >= PET_REVERSALS {
            self.reversals.clear();
            self.emote(Emote::Hearts);
            self.happen(Happening::Petted);
            self.set_mode(Mode::Petted, now);
        }
    }

    /// 推進一格。回傳 `true` = 這一格剛落地。
    pub fn update(&mut self, now: f64, dt: f32, env: &Env) -> bool {
        let mut landed = false;
        if self.notices_item(env, now) {
            self.emote(Emote::Alert);
            self.set_mode(Mode::Fetch { since: now }, now);
        } else if self.teased(env, now) {
            self.emote(Emote::Alert);
            self.set_mode(Mode::Chase { until: now + CHASE_SECS as f64 }, now);
        }
        let t = (now - self.mode_start) as f32;
        match self.mode {
            Mode::Idle { until } => {
                self.stand(env, now);
                self.look_at_cursor(env);
                if matches!(self.mode, Mode::Idle { .. })
                    && let Some(b) = self.block_at(self.x, self.y, env)
                {
                    // 被放進禁止範圍裡：走出去。
                    self.walk_out_of(b, env, now);
                } else if self.should_sleep(env) {
                    self.set_mode(Mode::Sleep, now);
                } else if now >= until {
                    self.pick_next_activity(now, env);
                }
            }
            Mode::Talk { until } => {
                self.stand(env, now);
                self.look_at_cursor(env);
                if self.should_sleep(env) {
                    self.set_mode(Mode::Sleep, now);
                } else if now >= until {
                    self.set_idle(now);
                }
            }
            Mode::Sit { until } => {
                self.stand(env, now);
                if self.should_sleep(env) {
                    self.set_mode(Mode::Sleep, now);
                } else if now >= until {
                    self.set_idle(now);
                }
            }
            Mode::Walk { dir, until, speed_mul } => {
                self.stand(env, now);
                if !matches!(self.mode, Mode::Walk { .. }) {
                    return false;
                }
                let ease = walk_ease(t, (until - now) as f32);
                let speed = env.walk_speed_dip.unwrap_or(DEFAULT_WALK_SPEED_DIP) * env.px_per_dip * speed_mul * ease;
                let prev_x = self.x;
                self.x += dir * speed * dt;
                if let Some(b) = self.entering_block(prev_x, env) {
                    if self.start_climb_over(b, dir, env, now) {
                        return false;
                    }
                    // 頂端上面沒空間翻過去：當成牆，轉頭。
                    let wall = if dir > 0.0 { b.left } else { b.right };
                    self.bump = Some((wall, self.y - env.pet_height / 2.0, now));
                    self.x = prev_x;
                    self.mode = Mode::Walk { dir: -dir, until, speed_mul };
                    self.facing_left = dir > 0.0;
                    return false;
                }
                let (lo, hi) = walk_bounds(env);
                let mut dir = dir;
                let mut hit_wall = None;
                if self.x <= lo {
                    self.x = lo;
                    dir = 1.0;
                    hit_wall = (lo <= env.min_x).then_some(-1.0);
                } else if self.x >= hi {
                    self.x = hi;
                    dir = -1.0;
                    hit_wall = (hi >= env.max_x).then_some(1.0);
                }
                self.facing_left = dir < 0.0;
                self.mode = Mode::Walk { dir, until, speed_mul };
                if let Some(side) = hit_wall
                    && self.may_climb(env)
                {
                    self.start_climb(side, env, now);
                    return false;
                }
                if self.should_sleep(env) {
                    self.set_mode(Mode::Sleep, now);
                } else if now >= until {
                    self.set_idle(now);
                }
            }
            Mode::Drag => {
                let vx = (self.x - self.last_pointer_x) / dt.max(1e-3);
                let vy = (self.y - self.last_pointer_y) / dt.max(1e-3);
                self.last_pointer_x = self.x;
                self.last_pointer_y = self.y;
                let k = (dt * 12.0).min(1.0);
                self.drag_vx += (vx - self.drag_vx) * k;
                self.drag_vy += (vy - self.drag_vy) * k;
                let target = (-self.drag_vx * DRAG_ANGLE_PER_SPEED / env.px_per_dip).clamp(-DRAG_MAX_ANGLE, DRAG_MAX_ANGLE);
                let accel = (target - self.drag_angle) * DRAG_SPRING - self.drag_angle_vel * DRAG_DAMPING;
                self.drag_angle_vel += accel * dt;
                self.drag_angle += self.drag_angle_vel * dt;
                if self.drag_vx.abs() > SWEAT_SPEED_DIP * env.px_per_dip && now - self.last_sweat >= SWEAT_EVERY_SECS {
                    self.last_sweat = now;
                    self.emote(Emote::Sweat);
                }
            }
            Mode::Fall { vx, vy } => {
                let (prev_x, prev_y) = (self.x, self.y);
                let vy = vy + GRAVITY_DIP * env.px_per_dip * dt;
                let mut vx = vx * (1.0 - AIR_DRAG * dt).max(0.0);
                self.y += vy * dt;
                self.x += vx * dt;
                // 從側面撞上禁止範圍就彈回來；從頂端邊緣走下去時腳已經離開頂端、半個身體還壓在上面，
                // 整個推到範圍外再往下掉（正上方掉下來會落在頂端，下面的平台判斷處理）。
                if self.block_at(prev_x, prev_y, env).is_none()
                    && let Some(b) = self.block_at(self.x, self.y, env)
                {
                    if prev_y > b.top {
                        let wall = if self.x > prev_x { b.left } else { b.right };
                        self.bump = Some((wall, self.y - env.pet_height / 2.0, now));
                        self.x = prev_x;
                        vx = -vx * WALL_BOUNCE;
                    } else if self.x < b.left || self.x > b.right {
                        self.x = if self.x < b.left { b.left - env.pet_half_width } else { b.right + env.pet_half_width };
                    }
                }
                // 往螢幕邊飛就反彈；從牆上往外跳時人在邊界外但正在往內飛，不算撞牆。
                if (self.x < env.min_x && vx < 0.0) || (self.x > env.max_x && vx > 0.0) {
                    self.x = self.x.clamp(env.min_x, env.max_x);
                    vx = -vx * WALL_BOUNCE;
                }
                self.facing_left = vx < 0.0 || (vx == 0.0 && self.facing_left);
                // 往下掉時穿過某個平台的高度 → 落在上面（往上飛時不會被擋）。
                let platform = (vy > 0.0).then(|| env.platforms.iter().filter(|p| p.y >= prev_y && p.y <= self.y && self.x >= p.left && self.x <= p.right).min_by(|a, b| a.y.total_cmp(&b.y))).flatten();
                if let Some(top) = platform.map(|p| p.y).or((self.y >= env.ground_y).then_some(env.ground_y)) {
                    // 落在哪就站在哪：起飛前站的視窗還在原地的話，不換掉會被 `stand` 拉回去。
                    self.riding = platform.map(|p| (p.window, p.window_left));
                    self.y = top;
                    self.emote(Emote::Dust);
                    self.set_mode(Mode::Land, now);
                    landed = true;
                } else {
                    self.mode = Mode::Fall { vx, vy };
                }
            }
            Mode::Spin => {
                self.stand(env, now);
                self.finish_after(t, SPIN_SECS, now);
            }
            Mode::Roll { dir } => {
                let (lo, hi) = walk_bounds(env);
                self.slide_to((self.x + dir * ROLL_DIST_HEIGHTS * env.pet_height / ROLL_SECS * dt).clamp(lo, hi), env);
                self.stand(env, now);
                if matches!(self.mode, Mode::Roll { .. }) {
                    self.finish_after(t, ROLL_SECS, now);
                }
            }
            Mode::Chase { until } => {
                self.stand(env, now);
                if !matches!(self.mode, Mode::Chase { .. }) {
                    return false;
                }
                let target = env.cursor.filter(|c| self.cursor_reachable(*c, env, f32::INFINITY));
                match target {
                    Some((cx, _)) if now < until => {
                        let dx = cx - self.x;
                        let dir = dx.signum();
                        self.facing_left = dir < 0.0;
                        if dx.abs() <= POUNCE_RANGE_HEIGHTS * env.pet_height {
                            self.emote(Emote::Note);
                            self.set_mode(Mode::Pounce { dir }, now);
                        } else {
                            let speed = env.walk_speed_dip.unwrap_or(DEFAULT_WALK_SPEED_DIP) * env.px_per_dip * CHASE_SPEED_MUL;
                            let (lo, hi) = walk_bounds(env);
                            let x = (self.x + dir * speed * dt).clamp(lo, hi);
                            if x == self.x || !self.slide_to(x, env) {
                                // 滑鼠在走不到的地方（範圍外、隔著禁止範圍）：放棄。
                                self.emote(Emote::Question);
                                self.set_idle(now);
                            }
                        }
                    }
                    _ => {
                        self.emote(Emote::Question);
                        self.set_idle(now);
                    }
                }
            }
            Mode::Pounce { dir } => {
                let p = t / POUNCE_SECS;
                if (JUMP_CROUCH..1.0 - JUMP_LAND).contains(&p) {
                    let (lo, hi) = walk_bounds(env);
                    self.slide_to((self.x + dir * POUNCE_SPEED_HEIGHTS * env.pet_height * dt).clamp(lo, hi), env);
                }
                self.stand(env, now);
                if matches!(self.mode, Mode::Pounce { .. }) && t >= POUNCE_SECS {
                    self.emote(Emote::Hearts);
                    self.set_idle(now);
                }
            }
            Mode::Fetch { since } => {
                self.stand(env, now);
                if matches!(self.mode, Mode::Fetch { .. }) {
                    self.fetch(since, dt, env, now);
                }
            }
            Mode::Eat => {
                self.stand(env, now);
                if matches!(self.mode, Mode::Eat) && t >= EAT_SECS {
                    self.happen(Happening::Ate);
                    self.emote(Emote::Hearts);
                    self.set_mode(Mode::Happy, now);
                }
            }
            Mode::Land if t >= LAND_SECS && std::mem::take(&mut self.dizzy_pending) => {
                self.emote(Emote::Dizzy);
                self.happen(Happening::Dizzy);
                self.set_mode(Mode::Dizzy { respun: false }, now);
            }
            Mode::Land => self.finish_after(t, LAND_SECS, now),
            Mode::Dizzy { respun } => {
                self.stand(env, now);
                if matches!(self.mode, Mode::Dizzy { .. }) {
                    if !respun && t >= DIZZY_RESPIN_SECS {
                        self.emote(Emote::Dizzy);
                        self.mode = Mode::Dizzy { respun: true };
                    }
                    self.finish_after(t, DIZZY_SECS, now);
                }
            }
            Mode::Happy => self.finish_after(t, HAPPY_SECS, now),
            Mode::Petted => self.finish_after(t, PETTED_SECS, now),
            Mode::Hop | Mode::Look | Mode::Stretch | Mode::Doze | Mode::Yawn => {
                self.stand(env, now);
                let secs = match self.mode {
                    Mode::Hop => HOP_SECS,
                    Mode::Look => LOOK_SECS,
                    Mode::Stretch => STRETCH_SECS,
                    Mode::Yawn => YAWN_SECS,
                    _ => DOZE_SECS,
                };
                if matches!(self.mode, Mode::Look) {
                    // 張望：到一半看另一邊，再轉回來。
                    let p = t / LOOK_SECS;
                    let away = (0.35..0.75).contains(&p);
                    self.facing_left = self.look_origin_left != away;
                }
                if self.should_sleep(env) {
                    self.set_mode(Mode::Sleep, now);
                } else {
                    self.finish_after(t, secs, now);
                }
            }
            Mode::Sleep => {
                self.stand(env, now);
                if env.idle_input_secs < WAKE_INPUT_SECS {
                    self.happen(Happening::Woke { slept_secs: t });
                    self.set_idle(now);
                }
            }
            Mode::Notify => self.stand(env, now),
            Mode::Climb { side, target_y } => {
                self.x = if side < 0.0 { env.wall_left } else { env.wall_right };
                let speed = env.walk_speed_dip.unwrap_or(DEFAULT_WALK_SPEED_DIP) * env.px_per_dip * CLIMB_SPEED_MUL;
                self.y -= speed * dt;
                if self.y <= target_y {
                    self.y = target_y;
                    let until = now + self.rand_range(CLING_SECS) as f64;
                    self.set_mode(Mode::Cling { side, until }, now);
                }
            }
            Mode::ClimbOver { side, top, across } => {
                let speed = env.walk_speed_dip.unwrap_or(DEFAULT_WALK_SPEED_DIP) * env.px_per_dip;
                self.y -= speed * CLIMB_SPEED_MUL * dt;
                if self.y <= top {
                    // 翻上去：站到頂端靠邊的地方，接著往前走過整個頂端（走到另一頭會掉下去 = 翻過去了）。
                    self.y = top;
                    self.x += side * env.pet_half_width;
                    self.riding = None;
                    self.facing_left = side < 0.0;
                    let secs = (across + env.pet_half_width) / speed + 2.0 * WALK_EASE_SECS;
                    self.set_mode(Mode::Walk { dir: side, until: now + secs as f64, speed_mul: 1.0 }, now);
                }
            }
            Mode::Cling { side, until } => {
                self.x = if side < 0.0 { env.wall_left } else { env.wall_right };
                if now >= until {
                    // 往外、往上跳下來。
                    let (vx, vy) = (-side * LEAP_OUT_DIP * env.px_per_dip, -LEAP_UP_DIP * env.px_per_dip);
                    self.facing_left = vx < 0.0;
                    // 從橫躺變回直立時，身體整個移進螢幕裡（不然起跳那一下有半隻在螢幕外）。
                    self.x = if side < 0.0 { env.min_x } else { env.max_x };
                    self.set_mode(Mode::Fall { vx, vy }, now);
                }
            }
        }
        self.track_turn(now);
        landed
    }

    /// 站在地上換方向時記下來，畫成轉身動作；在空中或被拎著時直接換。
    fn track_turn(&mut self, now: f64) {
        if self.facing_left == self.last_facing_left {
            return;
        }
        let grounded = matches!(self.mode, Mode::Idle { .. } | Mode::Walk { .. } | Mode::Look | Mode::Hop | Mode::Stretch | Mode::Doze | Mode::Yawn | Mode::Sit { .. } | Mode::Notify | Mode::Talk { .. } | Mode::Chase { .. } | Mode::Fetch { .. });
        self.turn_from = grounded.then_some((self.last_facing_left, now));
        self.last_facing_left = self.facing_left;
    }

    pub fn pose(&self, now: f64) -> Pose {
        let t = (now - self.mode_start) as f32;
        let tau = std::f32::consts::TAU;
        let pi = std::f32::consts::PI;
        let mut pose = Pose {
            state: self.state(),
            scale_x: 1.0,
            scale_y: 1.0,
            rotation_deg: 0.0,
            pivot: Pivot::Feet,
            lift: 0.0,
            flip: self.facing_left,
            zzz: None,
            climb: None,
        };
        match self.mode {
            Mode::Idle { .. } => {
                let b = (tau * t / 3.0).sin();
                pose.scale_y = 1.0 + 0.015 * b;
                pose.scale_x = 1.0 - 0.008 * b;
                pose.rotation_deg = self.lean;
                if t % BLINK_EVERY_SECS >= BLINK_EVERY_SECS - BLINK_SECS {
                    pose.state = states::BLINK;
                }
            }
            Mode::Spin => {
                // 原地轉兩圈：左右寬度跟著轉的角度變（看得到正面、側面、背面），順便小跳一下。
                let p = (t / SPIN_SECS).min(1.0);
                let turn = ease_in_out(p) * SPIN_TURNS;
                // 側面時不要薄成一條線（跟轉身一樣留一點厚度）。
                let c = (tau * turn).cos();
                pose.scale_x = c.signum() * c.abs().max(SPIN_MIN_WIDTH);
                pose.lift = -SPIN_HOP * (pi * p).sin();
            }
            Mode::Roll { dir } => {
                let p = (t / ROLL_SECS).min(1.0);
                let angle = 360.0 * ease_in_out(p);
                pose.pivot = Pivot::Center;
                pose.rotation_deg = dir * angle;
                pose.lift = -ROLL_LIFT * angle.to_radians().sin().abs();
                pose.flip = dir < 0.0;
            }
            Mode::Chase { .. } => {
                let step = STEPS_PER_SEC * CHASE_SPEED_MUL;
                pose.lift = -0.08 * (pi * t * step).sin().abs();
                pose.rotation_deg = 4.0 * (pi * t * step).sin();
            }
            Mode::Pounce { .. } => jump_pose(&mut pose, t / POUNCE_SECS, POUNCE_HEIGHT, 0.14),
            Mode::Dizzy { .. } => {
                // 站不穩：以腳為軸左右大幅搖晃，快結束時慢慢站穩。
                let settle = (1.0 - (t / DIZZY_SECS - 0.75) / 0.25).clamp(0.0, 1.0);
                let sway = (std::f32::consts::TAU * DIZZY_SWAY_HZ * t).sin();
                pose.rotation_deg = DIZZY_SWAY_DEG * sway * settle;
                pose.scale_y = 1.0 - 0.03 * sway.abs() * settle;
                pose.lift = -0.02 * (std::f32::consts::TAU * DIZZY_SWAY_HZ * 2.0 * t).sin().abs() * settle;
            }
            Mode::Fetch { .. } => {
                let step = STEPS_PER_SEC * FETCH_SPEED_MUL;
                pose.lift = -0.07 * (pi * t * step).sin().abs();
                pose.rotation_deg = 3.5 * (pi * t * step).sin();
            }
            Mode::Eat => {
                // 低頭一口一口咬：往前傾、身體一壓一壓。
                let bite = (pi * t * EAT_BITES_PER_SEC).sin().abs();
                pose.rotation_deg = if pose.flip { -6.0 } else { 6.0 } * (0.5 + 0.5 * bite);
                pose.scale_y = 1.0 - 0.05 * bite;
                pose.scale_x = 1.0 + 0.03 * bite;
            }
            Mode::Talk { .. } => {
                // 講話：身體隨著說話節奏輕輕一上一下。
                let b = (pi * t * TALK_BOB_HZ).sin().abs();
                pose.scale_y = 1.0 + TALK_BOB * b;
                pose.scale_x = 1.0 - TALK_BOB * 0.5 * b;
                pose.rotation_deg = self.lean;
            }
            Mode::Sit { .. } => {
                // 坐著呼吸比站著慢。
                let b = (tau * t / 4.0).sin();
                pose.scale_y = 1.0 + 0.012 * b;
                pose.scale_x = 1.0 - 0.006 * b;
            }
            Mode::Yawn => {
                // 嘴巴張開時身體往上伸一點，結束時落回來。
                let s = (pi * (t / YAWN_SECS).min(1.0)).sin();
                pose.scale_y = 1.0 + 0.05 * s;
                pose.scale_x = 1.0 - 0.025 * s;
            }
            Mode::Walk { speed_mul, until, .. } => {
                let step = STEPS_PER_SEC * speed_mul.max(1.0);
                let bounce = if speed_mul >= RUN_SPEED_MUL { 0.08 } else { 0.05 };
                // 起步、停步時步伐也跟著變小（緩入緩出）。
                let ease = walk_ease(t, (until - now) as f32);
                pose.lift = -bounce * ease * (pi * t * step).sin().abs();
                pose.rotation_deg = 3.5 * ease * (pi * t * step).sin();
            }
            Mode::Drag => {
                pose.pivot = Pivot::Top;
                pose.rotation_deg = self.drag_angle;
                pose.scale_y = 1.05;
                pose.scale_x = 0.97;
            }
            Mode::Fall { vx, .. } => {
                pose.scale_y = 1.08;
                pose.scale_x = 0.94;
                pose.rotation_deg = (vx * TUMBLE_DEG_PER_SPEED / self.px_per_dip_hint).clamp(-MAX_TUMBLE_DEG, MAX_TUMBLE_DEG);
            }
            Mode::Land => {
                // 阻尼彈簧：一落地壓到最扁，接著彈過頭一點點，再停穩（跟隨動作）。
                let decay = (-9.0 * t).exp();
                let squash = 0.2 * decay * (tau * 2.6 * t).cos();
                pose.scale_y = 1.0 - squash;
                pose.scale_x = 1.0 + squash * 0.7;
            }
            Mode::Happy => jump_pose(&mut pose, t / HAPPY_SECS, 0.35, 0.12),
            Mode::Hop => jump_pose(&mut pose, t / HOP_SECS, 0.14, 0.09),
            Mode::Look => {
                pose.rotation_deg = if pose.flip { -LEAN_DEG } else { LEAN_DEG };
            }
            Mode::Stretch => {
                // 先往下蹲、再往上伸長、最後彈回來。
                let p = (t / STRETCH_SECS).min(1.0);
                let s = if p < 0.25 { -(pi * p / 0.25).sin() * 0.4 } else { (pi * (p - 0.25) / 0.75).sin() };
                pose.scale_y = 1.0 + 0.09 * s;
                pose.scale_x = 1.0 - 0.05 * s;
            }
            Mode::Doze => {
                // 打瞌睡：頭慢慢往前點兩下。
                let nod = (pi * t / (DOZE_SECS / 2.0)).sin().max(0.0);
                pose.rotation_deg = if pose.flip { -7.0 } else { 7.0 } * nod;
                pose.scale_y = 1.0 - 0.03 * nod;
            }
            Mode::Petted => {
                let p = (t / PETTED_SECS).min(1.0);
                let fade = 1.0 - p;
                pose.rotation_deg = 6.0 * (tau * t * 3.0).sin() * fade;
                pose.scale_y = 1.0 + 0.04 * (tau * t * 3.0).cos().abs() * fade;
                pose.lift = -0.03 * (tau * t * 3.0).sin().abs() * fade;
            }
            Mode::Sleep => {
                let b = (tau * t / 4.5).sin();
                pose.scale_y = 0.97 + 0.02 * b;
                pose.scale_x = 1.0 - 0.01 * b;
                pose.zzz = Some(t);
            }
            Mode::Notify => {
                // 跳兩下、停一下，重複；每一下都有蹲、跳、落地。
                let cycle = (t / NOTIFY_CYCLE_SECS).fract();
                if cycle < 0.5 {
                    jump_pose(&mut pose, (cycle * 4.0).fract(), 0.08, 0.06);
                }
            }
            Mode::Climb { side, .. } | Mode::ClimbOver { side, .. } => {
                pose.climb = Some(side);
                pose.lift = -0.03 * (pi * t * STEPS_PER_SEC).sin().abs();
            }
            Mode::Cling { side, until } => {
                pose.climb = Some(side);
                let b = (tau * t / 3.0).sin();
                // 跳下來前先往牆壓低身體（預備動作）。
                let windup = (1.0 - (until - now) as f32 / LEAP_WINDUP_SECS).clamp(0.0, 1.0);
                pose.scale_y = 1.0 + 0.012 * b - 0.14 * windup * windup;
            }
        }
        if let Some((old_left, start)) = self.turn_from {
            let p = ((now - start) as f32 / TURN_SECS).clamp(0.0, 1.0);
            if p < 1.0 && pose.climb.is_none() {
                // 先壓窄、到一半換面、再撐回來。
                pose.flip = if p < 0.5 { old_left } else { self.facing_left };
                pose.scale_x *= (pi * p).cos().abs().max(0.15);
            }
        }
        pose
    }

    fn may_climb(&mut self, env: &Env) -> bool {
        let chance = match env.activity {
            Activity::Calm => 0.0,
            Activity::Normal => CLIMB_CHANCE,
            Activity::Lively => CLIMB_CHANCE_LIVELY,
        };
        // 只在地面上的螢幕邊爬（站在視窗上走到邊是掉下去）。
        (self.y - env.ground_y).abs() < 1.0 && self.rand01() < chance
    }

    /// 這一格要亮的那段看不見的牆。
    pub fn glow(&self, env: &Env, now: f64) -> Option<Glow> {
        let wall_mid = self.y - env.pet_half_width;
        let invisible_side = |side: f32| if side < 0.0 { env.invisible_walls.0 } else { env.invisible_walls.1 };
        match self.mode {
            Mode::ClimbOver { .. } => return Some(Glow::Wall { x: self.x, y: wall_mid }),
            Mode::Climb { side, .. } | Mode::Cling { side, .. } if invisible_side(side) => return Some(Glow::Wall { x: self.x, y: wall_mid }),
            Mode::Drag | Mode::Fall { .. } | Mode::Climb { .. } | Mode::Cling { .. } => {}
            _ => {
                let hw = env.pet_half_width;
                let on_block = env.blocks.iter().any(|b| (self.y - b.top).abs() < 1.0 && self.x + hw > b.left && self.x - hw < b.right);
                if on_block || (env.invisible_floor && (self.y - env.ground_y).abs() < 1.0) {
                    return Some(Glow::Floor { x: self.x, y: self.y });
                }
            }
        }
        self.bump.filter(|(.., t)| now - t < BUMP_GLOW_SECS).map(|(x, y, _)| Glow::Wall { x, y })
    }

    /// 身體站在 `(x, y)` 時會碰到的禁止範圍。
    fn block_at(&self, x: f32, y: f32, env: &Env) -> Option<Rect> {
        let (hw, h, e) = (env.pet_half_width, env.pet_height, BLOCK_TOUCH_PX);
        env.blocks.iter().copied().find(|b| x + hw > b.left + e && x - hw < b.right - e && y > b.top + e && y - h < b.bottom - e)
    }

    /// 從 `prev_x` 走到現在的位置，是不是剛走進某個禁止範圍（本來就在裡面的不算，讓牠走得出來）。
    fn entering_block(&self, prev_x: f32, env: &Env) -> Option<Rect> {
        if self.block_at(prev_x, self.y, env).is_some() {
            return None;
        }
        self.block_at(self.x, self.y, env)
    }

    /// 水平移動到 `x`；會走進禁止範圍就不動，回傳 `false`。
    fn slide_to(&mut self, x: f32, env: &Env) -> bool {
        let prev_x = self.x;
        self.x = x;
        if self.entering_block(prev_x, env).is_some() {
            self.x = prev_x;
            return false;
        }
        true
    }

    /// 往 `dir` 走進了禁止範圍 `b`：貼著它的邊往上爬。頂端上方留不下一個身高就爬不過去，回傳 `false`。
    fn start_climb_over(&mut self, b: Rect, dir: f32, env: &Env, now: f64) -> bool {
        if b.top - env.pet_height < env.ceiling {
            return false;
        }
        let side = dir.signum();
        self.x = if side > 0.0 { b.left } else { b.right };
        self.riding = None;
        self.set_mode(Mode::ClimbOver { side, top: b.top, across: b.right - b.left }, now);
        true
    }

    /// 被放進禁止範圍裡：往比較近的那一邊走出去。
    fn walk_out_of(&mut self, b: Rect, env: &Env, now: f64) {
        let (to_left, to_right) = (self.x - b.left, b.right - self.x);
        let (dir, dist) = if to_left <= to_right { (-1.0, to_left) } else { (1.0, to_right) };
        let speed = env.walk_speed_dip.unwrap_or(DEFAULT_WALK_SPEED_DIP) * env.px_per_dip;
        let secs = (dist + env.pet_half_width) / speed + 2.0 * WALK_EASE_SECS;
        self.facing_left = dir < 0.0;
        self.set_mode(Mode::Walk { dir, until: now + secs as f64, speed_mul: 1.0 }, now);
    }

    /// 動畫預覽工具用：直接開始某個動作（平常這些是隨機或由輸入觸發的）。
    pub fn start_demo(&mut self, demo: Demo, now: f64, env: &Env) {
        match demo {
            Demo::Hop => {
                self.emote(Emote::Note);
                self.set_mode(Mode::Hop, now);
            }
            Demo::Look => {
                self.emote(Emote::Question);
                self.look_origin_left = self.facing_left;
                self.set_mode(Mode::Look, now);
            }
            Demo::Stretch => self.set_mode(Mode::Stretch, now),
            Demo::Doze => self.set_mode(Mode::Doze, now),
            Demo::Petted => {
                self.emote(Emote::Hearts);
                self.set_mode(Mode::Petted, now);
            }
            Demo::Walk { secs } => self.set_mode(Mode::Walk { dir: 1.0, until: now + secs as f64, speed_mul: 1.0 }, now),
            Demo::ClimbLeft => self.start_climb(-1.0, env, now),
            Demo::Sleep => self.set_mode(Mode::Sleep, now),
            Demo::Sit => self.set_mode(Mode::Sit { until: now + SIT_SECS.0 as f64 }, now),
            Demo::Yawn => self.set_mode(Mode::Yawn, now),
            Demo::Talk => self.set_mode(Mode::Talk { until: now + 3.0 }, now),
            Demo::Dizzy => {
                self.emote(Emote::Dizzy);
                self.set_mode(Mode::Dizzy { respun: false }, now);
            }
            Demo::Spin => self.set_mode(Mode::Spin, now),
            Demo::Roll => self.set_mode(Mode::Roll { dir: 1.0 }, now),
        }
    }

    fn start_climb(&mut self, side: f32, env: &Env, now: f64) {
        let lowest = env.ground_y - env.pet_height * CLIMB_MIN_RISE_HEIGHTS;
        // 頂端要留一個身高（轉 90 度後桌寵的寬度在牆上往上長）。
        let highest = (env.ceiling + env.pet_height).max(env.ground_y - env.pet_height * CLIMB_MAX_RISE_HEIGHTS).min(lowest);
        let target_y = highest + (lowest - highest) * self.rand01();
        self.x = if side < 0.0 { env.wall_left } else { env.wall_right };
        self.set_mode(Mode::Climb { side, target_y }, now);
    }

    /// 滑鼠靠近時轉頭看它、身體微微往那邊傾。
    fn look_at_cursor(&mut self, env: &Env) {
        let target = match env.cursor {
            Some((cx, cy)) if (cx - self.x).abs() < env.pet_height * LOOK_AT_CURSOR_HEIGHTS && (cy - self.y).abs() < env.pet_height * LOOK_AT_CURSOR_HEIGHTS => {
                // 滑鼠就在正上方時不要左右亂轉。
                if (cx - self.x).abs() > env.pet_height * 0.15 {
                    self.facing_left = cx < self.x;
                }
                if self.facing_left { -LEAN_DEG } else { LEAN_DEG }
            }
            _ => 0.0,
        };
        self.lean += (target - self.lean) * 0.15;
    }

    /// 有人拿滑鼠在牠旁邊快速左右晃（像拿逗貓棒）：牠正閒著、不是安靜模式就要撲過去。
    fn teased(&mut self, env: &Env, now: f64) -> bool {
        let h = env.pet_height;
        let idle = env.activity != Activity::Calm && matches!(self.mode, Mode::Idle { .. } | Mode::Walk { .. } | Mode::Look | Mode::Sit { .. } | Mode::Stretch | Mode::Yawn | Mode::Hop | Mode::Talk { .. });
        let near = env.cursor.filter(|&(cx, cy)| idle && (cx - self.x).abs() > env.pet_half_width && self.cursor_reachable((cx, cy), env, h * TEASE_NOTICE_HEIGHTS));
        let Some((cx, _)) = near else {
            self.tease_anchor_x = None;
            self.tease_reversals.clear();
            return false;
        };
        let anchor = self.tease_anchor_x.get_or_insert(cx);
        let dx = cx - *anchor;
        if dx.abs() < h * TEASE_STEP_HEIGHTS {
            return false;
        }
        *anchor = cx;
        let dir = dx.signum();
        if self.tease_dir != 0.0 && dir != self.tease_dir {
            self.tease_reversals.push(now);
        }
        self.tease_dir = dir;
        self.tease_reversals.retain(|t| now - t <= TEASE_WINDOW_SECS);
        if self.tease_reversals.len() < TEASE_REVERSALS {
            return false;
        }
        self.tease_reversals.clear();
        true
    }

    /// 有人把點心或球拿起來重丟：之前搆不到放棄了也不再賭氣，看到就去拿。
    pub fn item_tossed(&mut self) {
        self.ignore_item_until = f64::NEG_INFINITY;
    }

    /// 有想要的東西、牠正閒著（不在睡覺、沒被拎著、沒在做別的事）就去拿。
    fn notices_item(&self, env: &Env, now: f64) -> bool {
        env.item.is_some_and(|i| i.wanted)
            && now >= self.ignore_item_until
            && matches!(self.mode, Mode::Idle { .. } | Mode::Walk { .. } | Mode::Sit { .. } | Mode::Look | Mode::Stretch | Mode::Yawn | Mode::Doze | Mode::Talk { .. })
    }

    /// 跑去拿東西：同一層就跑過去（東西還在空中就跑到它底下等）；搆到了點心就吃、球就踢；
    /// 東西落在搆不到的地方，等一下就放棄。
    fn fetch(&mut self, since: f64, dt: f32, env: &Env, now: f64) {
        let Some(item) = env.item.filter(|i| i.wanted) else {
            self.set_idle(now);
            return;
        };
        let h = env.pet_height;
        let dx = item.x - self.x;
        let same_level = (item.y - self.y).abs() < h * FETCH_SAME_LEVEL_HEIGHTS;
        let reach = h * FETCH_REACH_HEIGHTS;
        if dx.abs() > 1.0 {
            self.facing_left = dx < 0.0;
        }
        if same_level && item.grounded && dx.abs() <= reach {
            if item.food {
                self.set_mode(Mode::Eat, now);
            } else {
                let dir = if dx.abs() > 1.0 { dx.signum() } else if self.facing_left { -1.0 } else { 1.0 };
                self.happen(Happening::Kicked { dir });
                self.emote(Emote::Note);
                self.set_mode(Mode::Hop, now);
            }
            return;
        }
        if (same_level || !item.grounded) && dx.abs() > reach * 0.5 {
            let speed = env.walk_speed_dip.unwrap_or(DEFAULT_WALK_SPEED_DIP) * env.px_per_dip * FETCH_SPEED_MUL;
            let (lo, hi) = walk_bounds(env);
            // 隔著禁止範圍就過不去，跟搆不到一樣等一下就放棄。
            if self.slide_to((self.x + dx.signum() * (speed * dt).min(dx.abs())).clamp(lo, hi), env) {
                return;
            }
        }
        if item.grounded && now - since >= FETCH_PATIENCE_SECS {
            self.emote(Emote::Question);
            self.ignore_item_until = now + FETCH_IGNORE_SECS;
            self.set_idle(now);
        }
    }

    /// 滑鼠在牠搆得到的地方：在地面（或牠站的視窗頂）附近、不在頭頂老高的地方，水平距離在 `max_dx` 以內。
    fn cursor_reachable(&self, (cx, cy): (f32, f32), env: &Env, max_dx: f32) -> bool {
        let h = env.pet_height;
        let (lo, hi) = walk_bounds(env);
        cy <= self.y + h * 0.5 && cy >= self.y - h * CHASE_MAX_ABOVE_HEIGHTS && (cx - self.x).abs() <= max_dx && cx >= lo - h && cx <= hi + h && self.block_at(cx, self.y, env).is_none()
    }

    fn finish_after(&mut self, t: f32, secs: f32, now: f64) {
        if t >= secs {
            self.set_idle(now);
        }
    }

    fn should_sleep(&self, env: &Env) -> bool {
        env.idle_input_secs >= env.sleep_after_secs
    }

    /// 站在地上或視窗頂端；腳下的東西不見了（走出視窗邊緣、視窗被移走、工作列收起）就掉下去。
    fn stand(&mut self, env: &Env, now: f64) {
        self.px_per_dip_hint = env.px_per_dip;
        // 站著的視窗被拖動：跟著視窗一起移動（上下左右都跟）。
        if let Some((window, last_left)) = self.riding
            && let Some(left) = env.platforms.iter().find(|p| p.window == window).map(|p| p.window_left)
        {
            let dx = left - last_left;
            let x = (self.x + dx).clamp(env.min_x, env.max_x);
            if let Some(p) = env.platforms.iter().find(|p| p.window == window && x >= p.left && x <= p.right) {
                if (dx.abs() + (p.y - self.y).abs()) > RIDE_NOTICE_PX && now - self.last_ride_notice >= RIDE_NOTICE_EVERY_SECS {
                    self.last_ride_notice = now;
                    self.happen(Happening::Riding);
                }
                self.x = x;
                self.y = p.y;
                self.riding = Some((window, left));
                return;
            }
        }
        self.x = self.x.clamp(env.min_x, env.max_x);
        if let Some(p) = env.platforms.iter().find(|p| (p.y - self.y).abs() <= PLATFORM_SNAP_PX && self.x >= p.left && self.x <= p.right) {
            self.y = p.y;
            self.riding = Some((p.window, p.window_left));
        } else if self.y < env.ground_y - 0.5 {
            self.riding = None;
            self.set_mode(Mode::Fall { vx: 0.0, vy: 0.0 }, now);
        } else {
            self.riding = None;
            self.y = env.ground_y;
        }
    }

    fn pick_next_activity(&mut self, now: f64, env: &Env) {
        let doze = if env.idle_input_secs >= DOZE_AFTER_IDLE_SECS { NEXT_DOZE } else { 0.0 };
        let walk = match env.activity {
            Activity::Lively => NEXT_WALK * 1.5,
            Activity::Normal => NEXT_WALK,
            Activity::Calm => 0.0,
        };
        self.activity = env.activity;
        let sit = if env.activity == Activity::Calm { NEXT_SIT * SIT_CALM_MUL } else { NEXT_SIT };
        let moving = env.activity != Activity::Calm;
        let spin = NEXT_SPIN;
        let roll = if moving { NEXT_ROLL } else { 0.0 };
        let choices = [walk, NEXT_HOP, NEXT_LOOK, NEXT_STRETCH, doze, NEXT_YAWN, sit, spin, roll, NEXT_IDLE];
        let total: f32 = choices.iter().sum();
        let mut pick = self.rand01() * total;
        let mut index = choices.len() - 1;
        for (i, w) in choices.iter().enumerate() {
            if pick < *w {
                index = i;
                break;
            }
            pick -= w;
        }
        match index {
            0 => {
                let dir = if self.rand01() < 0.5 { -1.0 } else { 1.0 };
                let until = now + self.rand_range(WALK_SECS) as f64;
                let run_chance = if env.activity == Activity::Lively { RUN_CHANCE_LIVELY } else { RUN_CHANCE };
                let speed_mul = if self.rand01() < run_chance { RUN_SPEED_MUL } else { self.rand_range(WALK_SPEED_MUL) };
                self.set_mode(Mode::Walk { dir, until, speed_mul }, now);
            }
            1 => {
                if self.rand01() < NOTE_ON_HOP_CHANCE {
                    self.emote(Emote::Note);
                }
                self.set_mode(Mode::Hop, now);
            }
            2 => {
                if self.rand01() < QUESTION_ON_LOOK_CHANCE {
                    self.emote(Emote::Question);
                }
                self.look_origin_left = self.facing_left;
                self.set_mode(Mode::Look, now);
            }
            3 => self.set_mode(Mode::Stretch, now),
            4 => self.set_mode(Mode::Doze, now),
            5 => self.set_mode(Mode::Yawn, now),
            6 => {
                let until = now + self.rand_range(SIT_SECS) as f64;
                self.set_mode(Mode::Sit { until }, now);
            }
            7 => self.set_mode(Mode::Spin, now),
            8 => {
                let dir = if self.rand01() < 0.5 { -1.0 } else { 1.0 };
                self.set_mode(Mode::Roll { dir }, now);
            }
            _ => self.set_idle(now),
        }
    }

    fn set_idle(&mut self, now: f64) {
        let range = match self.activity {
            Activity::Lively => IDLE_SECS_LIVELY,
            Activity::Normal => IDLE_SECS,
            Activity::Calm => IDLE_SECS_CALM,
        };
        let until = now + self.rand_range(range) as f64;
        self.set_mode(Mode::Idle { until }, now);
    }

    fn set_mode(&mut self, mode: Mode, now: f64) {
        self.mode = mode;
        self.mode_start = now;
    }

    fn rand01(&mut self) -> f32 {
        // xorshift64：只拿來挑動作與時間，不需要密碼學等級。
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }

    fn rand_range(&mut self, (lo, hi): (f32, f32)) -> f32 {
        lo + (hi - lo) * self.rand01()
    }
}

/// 慢進慢出（smoothstep）。
fn ease_in_out(p: f32) -> f32 {
    let p = p.clamp(0.0, 1.0);
    p * p * (3.0 - 2.0 * p)
}

/// 走路的速度倍率：起步後 `WALK_EASE_SECS` 內加速到全速、停步前同樣時間內減速（smoothstep）。
fn walk_ease(elapsed: f32, remaining: f32) -> f32 {
    let smooth = |x: f32| {
        let x = x.clamp(0.0, 1.0);
        x * x * (3.0 - 2.0 * x)
    };
    (smooth(elapsed / WALK_EASE_SECS) * smooth(remaining / WALK_EASE_SECS)).max(0.05)
}

/// 一次跳躍（`p` 0～1）：先蹲（預備動作）→ 起跳時拉長、到最高點恢復 → 落地壓扁。
/// 寬度跟著高度反向變化，體積看起來不變（壓縮與伸展）。
fn jump_pose(pose: &mut Pose, p: f32, height: f32, squash: f32) {
    use std::f32::consts::PI;
    let p = p.clamp(0.0, 1.0);
    let air_end = 1.0 - JUMP_LAND;
    let scale_y = if p < JUMP_CROUCH {
        1.0 - squash * (PI / 2.0 * p / JUMP_CROUCH).sin()
    } else if p < air_end {
        let q = (p - JUMP_CROUCH) / (air_end - JUMP_CROUCH);
        pose.lift = -height * (PI * q).sin();
        1.0 + squash * 0.6 * (PI * q).cos().abs()
    } else {
        1.0 - squash * (PI * (p - air_end) / JUMP_LAND).sin()
    };
    pose.scale_y = scale_y;
    pose.scale_x = 1.0 + (1.0 - scale_y) * 0.7;
}

/// 散步能走的左右範圍（腳底中心點）。
fn walk_bounds(env: &Env) -> (f32, f32) {
    (env.min_x, env.max_x)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn env() -> Env {
        Env {
            ground_y: 1000.0,
            min_x: 100.0,
            max_x: 1800.0,
            px_per_dip: 1.0,
            idle_input_secs: 0.0,
            sleep_after_secs: 300.0,
            walk_speed_dip: None,
            cursor: None,
            pet_height: 128.0,
            pet_half_width: 50.0,
            platforms: Vec::new(),
            activity: Activity::Normal,
            blocks: Vec::new(),
            invisible_floor: false,
            invisible_walls: (false, false),
            wall_left: 36.0,
            wall_right: 1864.0,
            ceiling: 0.0,
            item: None,
        }
    }

    fn run(pet: &mut Pet, now: &mut f64, secs: f32, env: &Env) -> bool {
        let mut landed = false;
        for _ in 0..(secs / DT) as usize {
            *now += DT as f64;
            landed |= pet.update(*now, DT, env);
        }
        landed
    }

    #[test]
    fn dropped_pet_falls_to_ground_and_settles() {
        let env = env();
        let mut now = 0.0;
        let mut pet = Pet::new(500.0, 1000.0, now, 42);
        pet.input(Input::PointerDown { x: 500.0, y: 950.0 }, now, &env);
        pet.input(Input::PointerMove { x: 600.0, y: 400.0 }, now, &env);
        assert_eq!(pet.state(), states::DRAG);
        assert_eq!((pet.x, pet.y), (600.0, 450.0));
        pet.input(Input::PointerUp, now, &env);
        assert_eq!(pet.state(), states::FALL);
        assert!(run(&mut pet, &mut now, 2.0, &env), "should land within 2s");
        assert_eq!(pet.y, env.ground_y);
        assert_ne!(pet.state(), states::FALL);
        assert!(pet.drain_emotes().contains(&Emote::Dust), "landing kicks up dust");
    }

    #[test]
    fn small_press_is_a_click_with_hearts() {
        let env = env();
        let mut pet = Pet::new(500.0, 1000.0, 0.0, 1);
        pet.input(Input::PointerDown { x: 500.0, y: 950.0 }, 0.0, &env);
        pet.input(Input::PointerMove { x: 502.0, y: 951.0 }, 0.0, &env);
        pet.input(Input::PointerUp, 0.0, &env);
        assert!([states::HAPPY, states::SPIN].contains(&pet.state()), "a click hops or spins, got {}", pet.state());
        assert_eq!((pet.x, pet.y), (500.0, 1000.0));
        assert_eq!(pet.drain_emotes(), vec![Emote::Hearts]);
    }

    #[test]
    fn never_leaves_the_screen() {
        let env = env();
        let mut now = 0.0;
        let mut pet = Pet::new(500.0, 1000.0, now, 7);
        for _ in 0..20_000 {
            now += DT as f64;
            pet.update(now, DT, &env);
            // 爬牆時腳貼著螢幕邊、跳下來時在空中；其餘時間都站在地上、身體在螢幕裡。
            assert!((env.wall_left..=env.wall_right).contains(&pet.x));
            assert!(pet.y <= env.ground_y);
            if !matches!(pet.state(), states::CLIMB | states::FALL | states::LAND) {
                assert!((env.min_x..=env.max_x).contains(&pet.x), "x = {} in {}", pet.x, pet.state());
                assert_eq!(pet.y, env.ground_y);
            }
        }
    }

    #[test]
    fn idle_life_has_variety() {
        let mut env = env();
        env.idle_input_secs = DOZE_AFTER_IDLE_SECS + 1.0;
        let mut now = 0.0;
        let mut pet = Pet::new(500.0, 1000.0, now, 11);
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..(600.0 / DT) as usize {
            now += DT as f64;
            pet.update(now, DT, &env);
            seen.insert(pet.state());
            // 眨眼只是待機中閃一下的畫面，看 pose 才看得到。
            seen.insert(pet.pose(now).state);
        }
        for s in [states::IDLE, states::WALK, states::HOP, states::LOOK, states::WAKE, states::DOZE, states::YAWN, states::SIT, states::BLINK] {
            assert!(seen.contains(s), "never saw {s} in 10 minutes: {seen:?}");
        }
    }

    #[test]
    fn rubbing_the_pet_makes_it_happy() {
        let env = env();
        let mut pet = Pet::new(500.0, 1000.0, 0.0, 5);
        let mut now = 0.0;
        for i in 0..8 {
            now += 0.1;
            let x = if i % 2 == 0 { 480.0 } else { 520.0 };
            pet.input(Input::Hover { x }, now, &env);
        }
        assert_eq!(pet.state(), states::HAPPY);
        assert!(pet.drain_emotes().contains(&Emote::Hearts));
    }

    #[test]
    fn slow_hover_is_not_petting() {
        let env = env();
        let mut pet = Pet::new(500.0, 1000.0, 0.0, 5);
        for i in 0..8 {
            let x = if i % 2 == 0 { 480.0 } else { 520.0 };
            pet.input(Input::Hover { x }, i as f64 * 1.0, &env);
        }
        assert_ne!(pet.state(), states::HAPPY);
    }

    #[test]
    fn fast_drag_sweats() {
        let env = env();
        let mut now = 0.0;
        let mut pet = Pet::new(500.0, 1000.0, now, 3);
        pet.input(Input::PointerDown { x: 500.0, y: 950.0 }, now, &env);
        for i in 0..30 {
            now += DT as f64;
            pet.input(Input::PointerMove { x: 500.0 + i as f32 * 60.0, y: 700.0 }, now, &env);
            pet.update(now, DT, &env);
        }
        assert!(pet.drain_emotes().contains(&Emote::Sweat));
    }

    #[test]
    fn looks_toward_a_nearby_cursor() {
        let mut env = env();
        let mut now = 0.0;
        let mut pet = Pet::new(500.0, 1000.0, now, 9);
        env.cursor = Some((300.0, 950.0));
        pet.update(now, DT, &env);
        now += DT as f64;
        pet.update(now, DT, &env);
        // 轉身是一個動作：剛開始還面向原本那邊、身體壓窄，轉完才面向滑鼠。
        let mid = pet.pose(now);
        assert!(mid.scale_x < 1.0, "turning squeezes the body");
        run(&mut pet, &mut now, TURN_SECS + 0.05, &env);
        assert!(pet.pose(now).flip, "faces left toward the cursor");
    }

    #[test]
    fn sleeps_when_idle_and_wakes_on_input() {
        let mut env = env();
        let mut now = 0.0;
        let mut pet = Pet::new(500.0, 1000.0, now, 3);
        env.idle_input_secs = 301.0;
        run(&mut pet, &mut now, 0.1, &env);
        assert_eq!(pet.state(), states::SLEEP);
        assert!(pet.pose(now).zzz.is_some());
        env.idle_input_secs = 0.2;
        run(&mut pet, &mut now, 0.1, &env);
        assert_eq!(pet.state(), states::IDLE);
    }

    #[test]
    fn attention_pops_an_alert_once() {
        let env = env();
        let mut pet = Pet::new(500.0, 1000.0, 0.0, 4);
        pet.set_attention(true, 0.0);
        pet.set_attention(true, 0.1);
        assert_eq!(pet.state(), states::NOTIFY);
        assert_eq!(pet.drain_emotes(), vec![Emote::Alert]);
        pet.set_attention(false, 0.2);
        let _ = env;
        assert_eq!(pet.state(), states::IDLE);
    }

    #[test]
    fn falls_when_ground_drops_away() {
        let mut env = env();
        let mut now = 0.0;
        let mut pet = Pet::new(500.0, 1000.0, now, 9);
        env.ground_y = 1040.0; // 工作列收起來了
        run(&mut pet, &mut now, 0.05, &env);
        assert_eq!(pet.state(), states::FALL);
        run(&mut pet, &mut now, 1.0, &env);
        assert_eq!(pet.y, 1040.0);
    }

    /// 拖著往右甩、放手：會往右飛一段，撞到右邊反彈，最後落地。
    fn throw_right(pet: &mut Pet, now: &mut f64, env: &Env) {
        pet.input(Input::PointerDown { x: pet.x, y: pet.y - 50.0 }, *now, env);
        for _ in 0..10 {
            *now += DT as f64;
            let (x, y) = (pet.x + 30.0, pet.y - 50.0 - 10.0);
            pet.input(Input::PointerMove { x, y }, *now, env);
            pet.update(*now, DT, env);
        }
        pet.input(Input::PointerUp, *now, env);
    }

    #[test]
    fn thrown_pet_flies_with_momentum_and_bounces_off_walls() {
        let env = env();
        let mut now = 0.0;
        let mut pet = Pet::new(1500.0, 1000.0, now, 2);
        throw_right(&mut pet, &mut now, &env);
        let released_at = pet.x;
        let mut hit_wall = false;
        for _ in 0..(3.0 / DT) as usize {
            now += DT as f64;
            pet.update(now, DT, &env);
            hit_wall |= pet.x >= env.max_x;
        }
        assert!(hit_wall, "flew right into the wall");
        assert!(pet.x < env.max_x && pet.x > released_at - 400.0, "bounced back a little");
        assert_eq!(pet.y, env.ground_y);
    }

    #[test]
    fn lands_on_a_window_top_and_walks_off_its_edge() {
        let mut env = env();
        env.platforms = vec![Platform { left: 400.0, right: 600.0, y: 700.0, window: 1, window_left: 400.0 }];
        let mut now = 0.0;
        let mut pet = Pet::new(500.0, 1000.0, now, 6);
        pet.input(Input::PointerDown { x: 500.0, y: 950.0 }, now, &env);
        pet.input(Input::PointerMove { x: 500.0, y: 450.0 }, now, &env);
        pet.input(Input::PointerUp, now, &env);
        run(&mut pet, &mut now, 1.0, &env);
        assert_eq!(pet.y, 700.0, "landed on the window top, not the floor");
        // 一直待在上面散步，遲早會走出邊緣掉到地上。
        let mut fell = false;
        for _ in 0..(120.0 / DT) as usize {
            now += DT as f64;
            pet.update(now, DT, &env);
            if pet.y == env.ground_y {
                fell = true;
                break;
            }
        }
        assert!(fell, "walked off the edge and fell to the floor");
    }

    #[test]
    fn thrown_from_one_window_onto_another_stays_there() {
        let mut env = env();
        env.platforms = vec![
            Platform { left: 400.0, right: 900.0, y: 700.0, window: 1, window_left: 400.0 },
            Platform { left: 400.0, right: 900.0, y: 400.0, window: 2, window_left: 400.0 },
        ];
        let mut now = 0.0;
        let mut pet = Pet::new(500.0, 700.0, now, 6);
        run(&mut pet, &mut now, 0.1, &env);
        assert_eq!(pet.riding_window(), Some(1));
        pet.input(Input::PointerDown { x: 500.0, y: 650.0 }, now, &env);
        pet.input(Input::PointerMove { x: 500.0, y: 300.0 }, now, &env);
        pet.input(Input::PointerUp, now, &env);
        run(&mut pet, &mut now, 2.0, &env);
        assert_eq!(pet.y, 400.0, "stays on the upper window instead of snapping back to the one it was picked from");
        assert_eq!(pet.riding_window(), Some(2));
    }

    #[test]
    fn window_moving_away_drops_the_pet() {
        let mut env = env();
        env.platforms = vec![Platform { left: 400.0, right: 600.0, y: 700.0, window: 1, window_left: 400.0 }];
        let mut now = 0.0;
        let mut pet = Pet::new(500.0, 700.0, now, 6);
        run(&mut pet, &mut now, 0.1, &env);
        assert_eq!(pet.y, 700.0);
        env.platforms.clear();
        run(&mut pet, &mut now, 1.0, &env);
        assert_eq!(pet.y, env.ground_y);
    }

    #[test]
    fn calm_pet_stays_put() {
        let mut env = env();
        env.activity = Activity::Calm;
        let mut now = 0.0;
        let mut pet = Pet::new(500.0, 1000.0, now, 13);
        for _ in 0..(300.0 / DT) as usize {
            now += DT as f64;
            pet.update(now, DT, &env);
            assert_ne!(pet.state(), states::WALK);
        }
        assert_eq!(pet.x, 500.0);
    }

    #[test]
    fn climbs_the_screen_edge_then_leaps_off() {
        let mut env = env();
        env.activity = Activity::Lively;
        let mut now = 0.0;
        let mut pet = Pet::new(150.0, 1000.0, now, 17);
        let (mut climbed, mut highest, mut leapt_and_landed) = (false, f32::MAX, false);
        for _ in 0..(900.0 / DT) as usize {
            now += DT as f64;
            pet.update(now, DT, &env);
            if pet.state() == states::CLIMB {
                climbed = true;
                assert!(pet.x == env.wall_left || pet.x == env.wall_right, "feet stay on the wall");
                assert!(pet.pose(now).climb.is_some());
                highest = highest.min(pet.y);
            } else if climbed && pet.y == env.ground_y && (env.min_x..=env.max_x).contains(&pet.x) {
                leapt_and_landed = true;
                break;
            }
        }
        assert!(climbed, "walked into a wall and climbed it at least once in 15 minutes");
        assert!(highest <= env.ground_y - env.pet_height, "climbed at least one body height");
        assert!(highest >= env.ceiling + env.pet_height, "kept room below the top of the screen");
        assert!(leapt_and_landed, "leapt off and landed back on the floor inside the screen");
    }

    #[test]
    fn calm_never_climbs() {
        let mut env = env();
        env.activity = Activity::Calm;
        let mut now = 0.0;
        let mut pet = Pet::new(150.0, 1000.0, now, 17);
        for _ in 0..(600.0 / DT) as usize {
            now += DT as f64;
            pet.update(now, DT, &env);
            assert_ne!(pet.state(), states::CLIMB);
        }
    }

    #[test]
    fn stays_inside_an_allowed_area_standing_on_its_bottom_edge() {
        let mut env = env();
        env.activity = Activity::Lively;
        env.platforms = vec![
            Platform { left: 500.0, right: 900.0, y: 600.0, window: 1, window_left: 500.0 },
            Platform { left: 1300.0, right: 1600.0, y: 600.0, window: 2, window_left: 1300.0 },
        ];
        let area = Rect { left: 400.0, top: 300.0, right: 1000.0, bottom: 800.0 };
        env.confine_to(area);
        assert_eq!(env.platforms.len(), 1, "window tops outside the area are not offered");
        let mut now = 0.0;
        let mut pet = Pet::new(600.0, 800.0, now, 21);
        for _ in 0..(600.0 / DT) as usize {
            now += DT as f64;
            pet.update(now, DT, &env);
            assert!((area.left..=area.right).contains(&pet.x) && pet.y <= area.bottom, "left the area at ({}, {})", pet.x, pet.y);
        }
        assert!(pet.y == area.bottom || pet.y == 600.0, "stands on the area's bottom edge or the window top inside it, y = {}", pet.y);
    }

    /// 地上一塊禁止範圍：散步走過去會爬上去、從頂端走過，身體從來不會跑進去。
    #[test]
    fn climbs_over_a_forbidden_area_instead_of_walking_into_it() {
        let mut env = env();
        let block = Rect { left: 600.0, top: 850.0, right: 800.0, bottom: 1000.0 };
        env.blocks = vec![block];
        env.platforms = block_platforms(&env.blocks, env.ceiling, env.pet_height);
        let mut now = 0.0;
        let mut pet = Pet::new(450.0, 1000.0, now, 3);
        pet.start_demo(Demo::Walk { secs: 30.0 }, now, &env);
        let (mut climbed, mut on_top, mut beyond) = (false, false, false);
        for _ in 0..(40.0 / DT) as usize {
            now += DT as f64;
            pet.update(now, DT, &env);
            assert!(pet.block_at(pet.x, pet.y, &env).is_none() || pet.state() == states::CLIMB, "inside the forbidden area at ({}, {}) while {}", pet.x, pet.y, pet.state());
            assert_eq!(pet.glow(&env, now).is_some(), pet.state() == states::CLIMB || (pet.y == block.top && pet.state() != states::FALL),"glows only along the forbidden area's edge and top (state {}, y {})", pet.state(), pet.y);
            climbed |= pet.state() == states::CLIMB;
            on_top |= pet.y == block.top;
            beyond |= on_top && pet.x > block.right && pet.y == env.ground_y;
        }
        assert!(climbed && on_top && beyond, "climbed {climbed}, walked on top {on_top}, got down the far side {beyond}");
    }

    #[test]
    fn dropped_inside_a_forbidden_area_walks_out() {
        let mut env = env();
        env.blocks = vec![Rect { left: 400.0, top: 700.0, right: 900.0, bottom: 1000.0 }];
        let mut now = 0.0;
        let mut pet = Pet::new(500.0, 1000.0, now, 8);
        run(&mut pet, &mut now, 10.0, &env);
        assert!(pet.block_at(pet.x, pet.y, &env).is_none(), "still inside at x = {}", pet.x);
    }

    /// 滑鼠在 `cx` 附近左右晃（每 0.15 秒換一次邊）。
    fn wiggle(cx: f32, now: f64) -> f32 {
        if (now / 0.15) as i64 % 2 == 0 { cx - 40.0 } else { cx + 40.0 }
    }

    #[test]
    fn only_comes_to_the_cursor_when_teased() {
        let mut env = env();
        env.cursor = Some((1000.0, 980.0));
        let (mut now, mut pet) = (0.0, Pet::new(700.0, 1000.0, 0.0, 5));
        // 滑鼠停在附近工作十分鐘：不會自己跑過去。
        for _ in 0..(600.0 / DT) as usize {
            now += DT as f64;
            pet.update(now, DT, &env);
            assert!(!matches!(pet.mode, Mode::Chase { .. } | Mode::Pounce { .. }), "came to a resting cursor at {now:.1}s");
        }
        // 在牠旁邊晃：跑過去撲。
        let mut pounced = false;
        for _ in 0..(30.0 / DT) as usize {
            now += DT as f64;
            let x = (pet.x + 300.0).min(env.max_x - 100.0);
            env.cursor = Some((wiggle(x, now), 980.0));
            pet.update(now, DT, &env);
            pounced |= matches!(pet.mode, Mode::Pounce { .. });
        }
        assert!(pounced, "teased → ran to the cursor and pounced");
        // 晃在頭頂老高的地方：搆不到，不追。
        let mut pet = Pet::new(700.0, 1000.0, now, 5);
        for _ in 0..(30.0 / DT) as usize {
            now += DT as f64;
            env.cursor = Some((wiggle(1000.0, now), 300.0));
            pet.update(now, DT, &env);
            assert!(!matches!(pet.mode, Mode::Chase { .. }), "a cursor high up the screen is out of reach");
        }
    }

    #[test]
    fn rides_along_when_its_window_is_dragged() {
        let mut env = env();
        env.activity = Activity::Calm;
        let window = |dx: f32, dy: f32| vec![Platform { left: 400.0 + dx, right: 900.0 + dx, y: 700.0 + dy, window: 7, window_left: 400.0 + dx }];
        env.platforms = window(0.0, 0.0);
        let mut now = 0.0;
        let mut pet = Pet::new(500.0, 700.0, now, 6);
        run(&mut pet, &mut now, 0.1, &env);
        let start_x = pet.x;
        // 視窗往右上拖了 200、往上 150，分好幾格移動。
        for i in 1..=10 {
            env.platforms = window(20.0 * i as f32, -15.0 * i as f32);
            run(&mut pet, &mut now, DT, &env);
        }
        assert_eq!(pet.y, 550.0, "stays on the window top as it rises");
        assert!((pet.x - (start_x + 200.0)).abs() < 1.0, "carried 200px to the right, x = {}", pet.x);
        assert!(pet.drain_happenings().contains(&Happening::Riding));
    }

    #[test]
    fn runs_to_food_and_eats_it() {
        let mut env = env();
        env.item = Some(ItemView { x: 1200.0, y: 1000.0, food: true, grounded: true, wanted: true });
        let (mut now, mut pet) = (0.0, Pet::new(500.0, 1000.0, 0.0, 4));
        let mut ate = false;
        for _ in 0..(60.0 / DT) as usize {
            now += DT as f64;
            pet.update(now, DT, &env);
            ate |= pet.drain_happenings().contains(&Happening::Ate);
            if ate {
                break;
            }
        }
        assert!(ate, "ate the food");
        assert!((pet.x - 1200.0).abs() <= env.pet_height * FETCH_REACH_HEIGHTS, "ate it where it lay, x = {}", pet.x);
    }

    #[test]
    fn kicks_a_ball_but_ignores_one_it_is_done_with() {
        let mut env = env();
        env.item = Some(ItemView { x: 700.0, y: 1000.0, food: false, grounded: true, wanted: true });
        let (mut now, mut pet) = (0.0, Pet::new(500.0, 1000.0, 0.0, 4));
        let mut kicked = None;
        for _ in 0..(30.0 / DT) as usize {
            now += DT as f64;
            pet.update(now, DT, &env);
            kicked = kicked.or(pet.drain_happenings().into_iter().find(|h| matches!(h, Happening::Kicked { .. })));
        }
        assert_eq!(kicked, Some(Happening::Kicked { dir: 1.0 }), "kicked it away from itself");

        env.item = Some(ItemView { wanted: false, ..env.item.unwrap() });
        let mut pet = Pet::new(500.0, 1000.0, now, 4);
        for _ in 0..(30.0 / DT) as usize {
            now += DT as f64;
            pet.update(now, DT, &env);
            assert!(!matches!(pet.mode, Mode::Fetch { .. }));
        }
    }

    #[test]
    fn gives_up_on_food_it_cannot_reach() {
        let mut env = env();
        // 落在高高的視窗頂上（這個測試環境裡沒有那個平台，牠上不去）。
        env.item = Some(ItemView { x: 600.0, y: 400.0, food: true, grounded: true, wanted: true });
        let (mut now, mut pet) = (0.0, Pet::new(500.0, 1000.0, 0.0, 4));
        run(&mut pet, &mut now, FETCH_PATIENCE_SECS as f32 + 1.0, &env);
        assert!(!matches!(pet.mode, Mode::Fetch { .. } | Mode::Eat), "gave up");
        assert!(pet.ignore_item_until > now, "and stops trying for a while");
        // 有人把它拿起來丟到牠旁邊：馬上又去拿。
        pet.item_tossed();
        env.item = Some(ItemView { x: 600.0, y: 1000.0, food: true, grounded: true, wanted: true });
        let mut went = false;
        for _ in 0..(3.0 / DT) as usize {
            now += DT as f64;
            pet.update(now, DT, &env);
            went |= matches!(pet.mode, Mode::Fetch { .. } | Mode::Eat);
        }
        assert!(went, "went for the re-thrown food");
    }

    #[test]
    fn three_hard_throws_make_it_dizzy_after_landing() {
        let env = env();
        let mut now = 0.0;
        let mut pet = Pet::new(500.0, 1000.0, now, 6);
        let mut dizzy_after = Vec::new();
        for _ in 0..3 {
            throw_right(&mut pet, &mut now, &env);
            let mut dizzy = false;
            for _ in 0..(3.0 / DT) as usize {
                now += DT as f64;
                pet.update(now, DT, &env);
                dizzy |= matches!(pet.mode, Mode::Dizzy { .. });
            }
            dizzy_after.push(dizzy);
        }
        assert_eq!(dizzy_after, vec![false, false, true]);
        assert!(pet.drain_happenings().contains(&Happening::Dizzy));
    }

    #[test]
    fn a_click_sometimes_spins_instead_of_hopping() {
        let click = |pet: &mut Pet, env: &Env, now: f64| {
            pet.input(Input::PointerDown { x: pet.x, y: 950.0 }, now, env);
            pet.input(Input::PointerUp, now, env);
            pet.state()
        };
        let env = env();
        let mut pet = Pet::new(500.0, 1000.0, 0.0, 3);
        let states: Vec<&str> = (0..50).map(|i| click(&mut pet, &env, i as f64)).collect();
        assert!(states.contains(&states::SPIN) && states.contains(&states::HAPPY), "{states:?}");
    }
}
