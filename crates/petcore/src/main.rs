// 正式版不要跳出黑色主控台視窗；開發版留著看 log。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod area_editor;
mod autostart;
mod behavior;
mod bubble;
mod card_stack;
mod chatter;
mod countdown;
mod effects;
mod focus_module;
mod glow;
mod holiday_sync;
mod icons;
mod logger;
mod memwatch;
mod modules;
mod paths;
mod platforms;
mod popup_menu;
mod preview;
mod relay_module;
mod reminder_module;
mod render;
mod slicer;
mod snapshot;
mod soak;
mod sprites;
mod theme;
mod toy;
mod tray;
mod updater;
mod window_restore;

use petkit::reminders::Style;
use windows::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, GetLastError, WAIT_ABANDONED, WAIT_OBJECT_0,
};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::Threading::{CreateMutexW, WaitForSingleObject};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::core::w;

const SNAPSHOT_FLAG: &str = "--snapshot";
const SNAPSHOT_CARDS_FLAG: &str = "--snapshot-cards";
const FRAMES_FLAG: &str = "--frames";
const SLICE_FLAG: &str = "--slice";
const SEND_FLAG: &str = "--send";
/// 叫執行中的柚伴結束（解除安裝程式在刪檔前用；不用自己組 JSON）。
const QUIT_FLAG: &str = "--quit";
/// 傳話（給 AI 和其他程式用，用詞見 `CONTEXT.md`）：`--confirm` 跳確認框、`--notice` 跳提示框。
const CONFIRM_FLAG: &str = "--confirm";
const NOTICE_FLAG: &str = "--notice";
const RELAY_FROM_OPT: &str = "--from";
const RELAY_ID_OPT: &str = "--id";
const RELAY_DETAIL_OPT: &str = "--detail";
/// 給 AI 看的完整用法（README 只叫人來跑這個，參數與結束碼只寫在這裡）。
const RELAY_HELP_FLAG: &str = "--relay-help";
/// 傳話的結束碼：送程式的那一方靠它知道使用者有沒有收到。
const RELAY_EXIT_SENT: i32 = 0;
const RELAY_EXIT_NOT_RUNNING: i32 = 1;
const RELAY_EXIT_USAGE: i32 = 2;
const RELAY_EXIT_DISABLED: i32 = 3;

/// 傳話的指令印出來的字一律英文（純 ASCII）：正式版沒有主控台，輸出只會進呼叫端的管線，
/// 而繁中 Windows 的 PowerShell 5.1 預設用 cp950 解讀管線，UTF-8 的中文會變亂碼。
fn relay_help() -> String {
    format!(
        r#"Youban desktop pet: relay a message to the user through the pet.

USAGE
  Youban.exe --confirm <message> [--from <name>] [--id <key>] [--detail <text>]
  Youban.exe --notice  <message> [--from <name>] [--id <key>] [--detail <text>]
  Youban.exe --notice  <message> <message> ... [--from <name>]
    Several messages in one call, each its own box (no --id / --detail then).

WHICH ONE
  --confirm  A box with Done / Later buttons. Stays until the user presses Done,
             and comes back if Youban restarts. Use it when the user has to act
             or decide: review a PR, answer a question, a job failed.
  --notice   Disappears by itself after {notice_secs} seconds (the countdown waits
             while the user is away). Use it for FYI: build passed, task finished.
             Notices arriving together merge into one list.
  When unsure, use --notice. Do not send one for every small step.

OPTIONS
  --from <name>    Who is talking, shown on the box. Use the same name every time;
                   the relay log groups messages by it.
  --id <key>       A message with the same --from and --id replaces the earlier one
                   instead of piling up. Use it for progress of one task (--id build).
  --detail <text>  Longer explanation. Not shown on the box, only in the relay log
                   (right-click the pet > relay log).

WRITING THE MESSAGE
  The box shows at most two short lines, about 20 Chinese characters. Keep
  <message> short and put the rest in --detail. Write in the user's language.

EXIT CODES
  {sent}  delivered
  {not_running}  Youban is not running
  {usage}  bad arguments
  {disabled}  relay is turned off in Youban's settings
  On {not_running} or {disabled} the user did not see it; tell them another way.

POWERSHELL
  PowerShell does not wait for Youban.exe (and $LASTEXITCODE stays empty)
  unless the output is piped:
    Youban.exe --notice "Build passed" --from Claude | Out-String; $LASTEXITCODE
"#,
        notice_secs = card_stack::NOTICE_SECS,
        sent = RELAY_EXIT_SENT,
        not_running = RELAY_EXIT_NOT_RUNNING,
        usage = RELAY_EXIT_USAGE,
        disabled = RELAY_EXIT_DISABLED,
    )
}
const SOAK_FLAG: &str = "--soak";
const AUTOSTART_FLAG: &str = "--autostart";
const SNAPSHOT_DEFAULT_DPI: u32 = 96;
const SOAK_DEFAULT_SECS: u64 = 120;
/// 更新後的新核心最多等舊的多久才放棄。
const AFTER_UPDATE_WAIT_MS: u32 = 15_000;

fn main() {
    // 要在 log 開檔之前：log 會先建出新資料夾，舊的就搬不過來了。
    let migration = petkit::paths::migrate_legacy_dirs();
    logger::init();
    for problem in migration {
        log!("legacy data folder not moved: {problem}");
    }
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log!("panic: {info}");
        default_hook(info);
    }));

    let args: Vec<String> = std::env::args().collect();
    let arg = |i: usize| args.get(i).map(String::as_str);
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }

    match arg(1) {
        Some(SLICE_FLAG) => {
            let (Some(sheet), Some(pet)) = (arg(2), arg(3)) else {
                eprintln!(
                    "用法：Youban.exe --slice <多格圖.png> <桌寵資料夾名> <名稱1> <名稱2> ..."
                );
                std::process::exit(2);
            };
            let names: Vec<String> = args.iter().skip(4).cloned().collect();
            if let Err(e) = slicer::run(std::path::Path::new(sheet), pet, &names) {
                eprintln!("slice failed: {e}");
                std::process::exit(1);
            }
        }
        Some(FRAMES_FLAG) => {
            let out = arg(2)
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| "frames".into());
            let dpi = arg(3)
                .and_then(|d| d.parse().ok())
                .unwrap_or(SNAPSHOT_DEFAULT_DPI);
            if let Err(e) = preview::frames(&out, dpi, arg(4)) {
                eprintln!("frames failed: {e}");
                std::process::exit(1);
            }
        }
        Some(SNAPSHOT_FLAG) | Some(SNAPSHOT_CARDS_FLAG) => {
            let out = arg(2)
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| "snapshot.png".into());
            let dpi = arg(3)
                .and_then(|d| d.parse().ok())
                .unwrap_or(SNAPSHOT_DEFAULT_DPI);
            let result = if arg(1) == Some(SNAPSHOT_FLAG) {
                snapshot::pet(&out, dpi)
            } else {
                snapshot::cards(&out, dpi)
            };
            if let Err(e) = result {
                eprintln!("snapshot failed: {e}");
                std::process::exit(1);
            }
        }
        Some(SEND_FLAG) => std::process::exit(if send(arg(2).unwrap_or("{}")) { 0 } else { 1 }),
        Some(flag @ (CONFIRM_FLAG | NOTICE_FLAG)) => std::process::exit(relay(flag, &args[2..])),
        // 讀的那端先關掉（`| head`）不算錯，print! 會因此 panic。
        Some(RELAY_HELP_FLAG) => {
            let _ = std::io::Write::write_all(&mut std::io::stdout(), relay_help().as_bytes());
        }
        Some(QUIT_FLAG) => std::process::exit(
            if send(&serde_json::json!({ "cmd": petkit::ipc::CMD_QUIT }).to_string()) {
                0
            } else {
                1
            },
        ),
        Some(AUTOSTART_FLAG) => std::process::exit(match autostart::cli(arg(2)) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("{e}");
                1
            }
        }),
        other => {
            let soak_secs = (other == Some(SOAK_FLAG)).then(|| {
                arg(2)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(SOAK_DEFAULT_SECS)
            });
            run_app(soak_secs, other == Some(updater::AFTER_UPDATE_FLAG));
        }
    }
}

fn run_app(soak_secs: Option<u64>, after_update: bool) {
    unsafe {
        // 只能有一隻在跑：第二次啟動直接結束。更新後的新核心例外：等舊的結束再接手。
        let guard = CreateMutexW(None, true, w!("Local\\YoubanDesktopPet.Core"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            // 舊核心結束時沒有放開，所以拿到的會是「被放棄」（WAIT_ABANDONED），一樣算接手成功。
            let taken = after_update
                && guard.as_ref().is_ok_and(|h| {
                    matches!(
                        WaitForSingleObject(*h, AFTER_UPDATE_WAIT_MS),
                        WAIT_OBJECT_0 | WAIT_ABANDONED
                    )
                });
            if !taken {
                log!("another instance is running; exiting");
                return;
            }
        }
        std::mem::forget(guard);
    }
    log!("start");
    if let Err(e) = app::run(soak_secs, after_update) {
        log!("fatal: {e}");
        std::process::exit(1);
    }
}

/// `--confirm` / `--notice` 的參數拆成要送的傳話：沒帶選項的參數各是一則；`None` = 參數不對。
/// 一次好幾則時不收 `--id`、`--detail`（看不出是給哪一則的）。
fn parse_relay(flag: &str, rest: &[String]) -> Option<Vec<petkit::ipc::Relay>> {
    let (mut titles, mut from, mut id, mut detail) = (Vec::new(), None, None, None);
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        let slot = match a.as_str() {
            RELAY_FROM_OPT => &mut from,
            RELAY_ID_OPT => &mut id,
            RELAY_DETAIL_OPT => &mut detail,
            _ => {
                titles.push(a.clone());
                continue;
            }
        };
        *slot = Some(it.next()?.clone());
    }
    let several = titles.len() > 1;
    if titles.is_empty()
        || titles.iter().any(|t| t.trim().is_empty())
        || several && (id.is_some() || detail.is_some())
    {
        return None;
    }
    let style = if flag == CONFIRM_FLAG {
        Style::Confirm
    } else {
        Style::Notice
    };
    Some(
        titles
            .into_iter()
            .map(|title| petkit::ipc::Relay {
                style,
                title,
                from: from.clone(),
                id: id.clone(),
                detail: detail.clone(),
            })
            .collect(),
    )
}

/// `--confirm` / `--notice` 的參數組成傳話送給桌寵，回傳結束碼。
fn relay(flag: &str, rest: &[String]) -> i32 {
    let Some(relays) = parse_relay(flag, rest) else {
        eprintln!(
            "usage: Youban.exe --confirm|--notice <message>... [--from <name>] [--id <key>] [--detail <text>]\nfull guide: Youban.exe {RELAY_HELP_FLAG}"
        );
        return RELAY_EXIT_USAGE;
    };
    let config = petkit::config::Config::load(&petkit::paths::config_file()).unwrap_or_default();
    if !config.features.relay {
        eprintln!("relay is turned off in Youban's settings; the user did not see this");
        return RELAY_EXIT_DISABLED;
    }
    for r in relays {
        let mut v = serde_json::to_value(r).expect("relay serializes");
        v["cmd"] = petkit::ipc::CMD_RELAY.into();
        if !send(&v.to_string()) {
            return RELAY_EXIT_NOT_RUNNING;
        }
    }
    RELAY_EXIT_SENT
}

/// 把一段 JSON 指令送給正在跑的桌寵，等它做完才回來（傳話、自動測試用）。
fn send(json: &str) -> bool {
    let sent = petkit::ipc::send(json).is_ok();
    if !sent {
        eprintln!("desktop pet is not running");
    }
    sent
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn several_messages_in_one_call_become_separate_relays() {
        let relays = parse_relay(
            NOTICE_FLAG,
            &args(&["建置好了", "測試通過", "--from", "Claude"]),
        )
        .unwrap();
        let got: Vec<_> = relays
            .iter()
            .map(|r| (r.title.as_str(), r.from.as_deref()))
            .collect();
        assert_eq!(
            got,
            [("建置好了", Some("Claude")), ("測試通過", Some("Claude"))]
        );
        // 好幾則時 --id 不知道是給哪一則的。
        assert!(parse_relay(NOTICE_FLAG, &args(&["a", "b", "--id", "x"])).is_none());
    }
}
