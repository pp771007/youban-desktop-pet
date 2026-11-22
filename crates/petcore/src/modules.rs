//! 功能模組：提醒、視窗歸位，以及之後要加的功能都長這個樣子。
//!
//! 模組不能自己開計時器、執行緒或視窗：只能在 `tick`（每秒一次）裡看時間決定要不要做事，
//! 要顯示東西就往 `Ctx` 丟請求。所以停用一個模組 = 把它丟掉，它的東西一定跟著消失。
//! 模組出錯（panic）只會停掉那一個模組，桌寵本身不受影響。

use std::panic::{AssertUnwindSafe, catch_unwind};

use chrono::NaiveDateTime;

use crate::bubble::{Card, CardAction};
use crate::log;

/// 模組丟給核心的請求。
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    /// 顯示一張卡；`key` 讓模組之後認得是哪一張（例如提醒的 id）。
    Card { key: String, card: Card },
    /// 收掉自己丟出去、還沒被處理的卡（例如提醒被刪掉了）。
    Withdraw { key: String },
}

pub struct Ctx<'a> {
    pub now: NaiveDateTime,
    pub out: &'a mut Vec<Request>,
}

impl Ctx<'_> {
    pub fn card(&mut self, key: impl Into<String>, card: Card) {
        self.out.push(Request::Card { key: key.into(), card });
    }

    pub fn info(&mut self, title: impl Into<String>, detail: Option<String>) {
        self.card("info", Card::success(title, detail));
    }
}

#[derive(Debug, Clone)]
pub enum ModuleEvent<'a> {
    /// 使用者點了這個模組提供的選單項目（模組自己的編號）。
    Menu(u32),
    /// 使用者在這個模組丟出的卡片上按了按鈕。
    Card { key: &'a str, action: CardAction },
    /// 螢幕接上／拔掉／改解析度。
    DisplayChanged,
    /// 官方辦公日曆更新了。
    HolidaysUpdated,
    /// 設定程式送來的指令。
    Command(&'a serde_json::Value),
}

pub trait Module {
    fn id(&self) -> &'static str;
    /// 給使用者看的名字（出錯時的通知用）。
    fn name(&self) -> &'static str;
    fn tick(&mut self, _ctx: &mut Ctx) {}
    fn on_event(&mut self, _event: &ModuleEvent, _ctx: &mut Ctx) {}
    /// 這個模組要放進右鍵選單的項目：(模組自己的編號, 文字)。
    fn menu(&self) -> Vec<(u32, &'static str)> {
        Vec::new()
    }
}

/// 選單編號：模組 `i` 的項目是 `MENU_BASE + i * MENU_STRIDE + 自己的編號`。
pub const MENU_BASE: u32 = 1000;
const MENU_STRIDE: u32 = 100;

pub struct ModuleHost {
    modules: Vec<Box<dyn Module>>,
}

impl ModuleHost {
    pub fn new(modules: Vec<Box<dyn Module>>) -> Self {
        Self { modules }
    }

    /// 讓載入的模組剛好是 `wanted`（照這個順序）：沒列到的丟掉，缺的用 `make` 建。
    /// 回傳被丟掉的模組 id，呼叫端要收掉它們留下的卡片。
    pub fn sync(&mut self, wanted: &[&'static str], make: impl Fn(&str) -> Box<dyn Module>) -> Vec<&'static str> {
        let mut old = std::mem::take(&mut self.modules);
        for &id in wanted {
            match old.iter().position(|m| m.id() == id) {
                Some(i) => self.modules.push(old.remove(i)),
                None => self.modules.push(make(id)),
            }
        }
        old.iter().map(|m| m.id()).collect()
    }

    pub fn ids(&self) -> Vec<&'static str> {
        self.modules.iter().map(|m| m.id()).collect()
    }

    pub fn tick(&mut self, now: NaiveDateTime, out: &mut Vec<(&'static str, Request)>) {
        self.each(now, out, |m, ctx| m.tick(ctx));
    }

    /// 送事件給全部模組。
    pub fn broadcast(&mut self, event: &ModuleEvent, now: NaiveDateTime, out: &mut Vec<(&'static str, Request)>) {
        self.each(now, out, |m, ctx| m.on_event(event, ctx));
    }

    /// 送事件給某一個模組。
    pub fn send(&mut self, id: &str, event: &ModuleEvent, now: NaiveDateTime, out: &mut Vec<(&'static str, Request)>) {
        self.each(now, out, |m, ctx| {
            if m.id() == id {
                m.on_event(event, ctx);
            }
        });
    }

    pub fn menu(&self) -> Vec<(u32, &'static str)> {
        let mut items = Vec::new();
        for (i, m) in self.modules.iter().enumerate() {
            for (local, label) in m.menu() {
                items.push((MENU_BASE + i as u32 * MENU_STRIDE + local, label));
            }
        }
        items
    }

    /// 選單編號 → 交給對應的模組。
    pub fn menu_clicked(&mut self, menu_id: u32, now: NaiveDateTime, out: &mut Vec<(&'static str, Request)>) {
        let Some(offset) = menu_id.checked_sub(MENU_BASE) else { return };
        let (index, local) = ((offset / MENU_STRIDE) as usize, offset % MENU_STRIDE);
        let Some(id) = self.modules.get(index).map(|m| m.id()) else { return };
        self.send(id, &ModuleEvent::Menu(local), now, out);
    }

    /// 對每個模組做一件事；出錯的模組直接移除，並發一張通知卡。
    fn each(&mut self, now: NaiveDateTime, out: &mut Vec<(&'static str, Request)>, mut f: impl FnMut(&mut dyn Module, &mut Ctx)) {
        let mut failed = Vec::new();
        for (i, m) in self.modules.iter_mut().enumerate() {
            let mut requests = Vec::new();
            let mut ctx = Ctx { now, out: &mut requests };
            let result = catch_unwind(AssertUnwindSafe(|| f(m.as_mut(), &mut ctx)));
            let id = m.id();
            out.extend(requests.into_iter().map(|r| (id, r)));
            if result.is_err() {
                log!("module '{id}' panicked; disabled");
                failed.push((i, m.name()));
            }
        }
        for (i, name) in failed.into_iter().rev() {
            self.modules.remove(i);
            out.push(("core", Request::Card { key: "info".into(), card: Card::warning(format!("「{name}」出了問題，已暫時停用"), Some("重新啟動柚伴後會再試一次".into())) }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Boom;
    impl Module for Boom {
        fn id(&self) -> &'static str {
            "boom"
        }
        fn name(&self) -> &'static str {
            "會壞的功能"
        }
        fn tick(&mut self, _ctx: &mut Ctx) {
            panic!("broken");
        }
    }

    struct Fine(u32);
    impl Module for Fine {
        fn id(&self) -> &'static str {
            "fine"
        }
        fn name(&self) -> &'static str {
            "正常的功能"
        }
        fn tick(&mut self, ctx: &mut Ctx) {
            self.0 += 1;
            ctx.info("tick", None);
        }
        fn menu(&self) -> Vec<(u32, &'static str)> {
            vec![(3, "做事")]
        }
        fn on_event(&mut self, event: &ModuleEvent, ctx: &mut Ctx) {
            if let ModuleEvent::Menu(3) = event {
                ctx.info("menu", None);
            }
        }
    }

    #[test]
    fn sync_turns_modules_off_and_back_on() {
        let make = |id: &str| -> Box<dyn Module> {
            match id {
                "fine" => Box::new(Fine(0)),
                _ => Box::new(Boom),
            }
        };
        let mut host = ModuleHost::new(Vec::new());
        assert!(host.sync(&["fine"], make).is_empty());
        assert_eq!(host.ids(), ["fine"]);
        assert_eq!(host.menu().len(), 1);
        assert_eq!(host.sync(&[], make), ["fine"], "turned off → reported so its cards can be withdrawn");
        assert!(host.ids().is_empty());
        assert!(host.menu().is_empty(), "a turned-off module leaves nothing in the menu");
        host.sync(&["fine"], make);
        assert_eq!(host.ids(), ["fine"]);
    }

    #[test]
    fn panicking_module_is_removed_and_others_keep_running() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap().and_hms_opt(0, 0, 0).unwrap();
        let mut host = ModuleHost::new(vec![Box::new(Boom), Box::new(Fine(0))]);
        let mut out = Vec::new();
        host.tick(now, &mut out);
        assert!(out.iter().any(|(id, _)| *id == "fine"));
        assert!(out.iter().any(|(id, r)| *id == "core" && matches!(r, Request::Card { card: Card::Info { title, .. }, .. } if title.contains("會壞的功能"))));
        out.clear();
        host.tick(now, &mut out);
        assert_eq!(out.len(), 1, "boom is gone, fine still ticks");
        let menu = host.menu();
        assert_eq!(menu.len(), 1);
        out.clear();
        host.menu_clicked(menu[0].0, now, &mut out);
        assert!(matches!(&out[0].1, Request::Card { card: Card::Info { title, .. }, .. } if title == "menu"));
    }
}
