# 柚伴（Youban Desktop Pet）給 Claude 的規則

產品名稱在 `petkit::APP_NAME`。資料夾是 `%APPDATA%\youban-desktop-pet`、`%LOCALAPPDATA%\youban-desktop-pet`；改名前的舊資料夾在啟動時自動搬過來（`petkit::paths::migrate_legacy_dirs`，要在寫任何檔案之前呼叫）。執行檔是 `Youban.exe`、`YoubanSettings.exe`（crate 名稱還是 `petcore`、`petsettings`，執行檔名由 `[[bin]]` 指定，要跟 `petkit::paths::CORE_EXE` / `SETTINGS_EXE` 一致）；視窗類別、開機啟動的登錄值、環境變數都用 `YoubanDesktopPet` / `YOUBAN_DESKTOP_PET_` 開頭。

`README.md` 只寫給玩家看（怎麼裝、怎麼玩、常見問題），不放程式資訊。指令、資料夾結構、測試跑法、發版在 `docs/DEVELOPMENT.md`；架構與取捨在 `docs/PLAN.md`；畫面規範在 `docs/design-system.md`。這裡只放「讀程式看不出來、做錯了才會發現」的事。

## 這份檔是公開的 —— 這些不能寫進來

repo 公開在 GitHub 上，這份檔、其他文件、程式註解、commit 訊息都一樣。下面這些要寫就寫進 Claude 的本機記憶，不要進 repo：

- 本機路徑、使用者名稱、電腦名稱
- 工作單位的名稱、內網的主機名稱或 IP
- 帳號、權杖、金鑰（commit 過一次就要當成外洩，直接換掉）
- 只存在這台電腦的東西（本機分支、本機資料夾、暫存區）
- 討論過程：誰提了什麼、什麼被否決、誰嫌什麼醜 —— 只寫最後的決定

寫完要推之前，連 commit 訊息一起掃一遍上面這幾類。

## 產品方向

- **不做養成／解鎖機制**（等級、點數、要累積才能用的動作或台詞）。陪伴只做紀念（「陪伴天數」），不鎖任何東西；新動作、新台詞一律一開始就有。
- **桌寵講的話、桌寵跳出的卡片都用牠的口氣**：第一人稱「我」，句尾用「～」「囉」「喔」「耶」，像在跟使用者撒嬌，不寫「已完成…」「已把…」這種系統公告句（例：「幫你把 3 個視窗搬回原位囉～」，不是「已把 3 個視窗放回原位」）。數量是 0 時另外寫一句，不念「0 個」。出錯也用牠的口氣，但一樣要講接下來怎麼辦（「有 2 個太重搬不動，要用系統管理員身分開柚伴才行」）。平常的台詞在 `chatter.rs`，寫新句子前先讀一遍對齊語氣。設定程式的按鈕、提示維持一般說明口吻。
- 圖用 AI 出（提示詞在 `docs/art-prompts.md`，風格 B 平塗）。會放在桌寵旁邊的東西（點心、球、頭上的表情符號）要用同畫風的圖——程式畫的向量小插圖跟桌寵擺在一起會很突兀；功能小圖示（設定、提醒卡）是自己畫的一套，規則見 `design-system.md` 第 2 節。

## 重開柚伴之後還在不在

柚伴會自己重開（自動更新），使用者也會手動關掉再開。新增「會跨時間」的狀態（等待中、今天做過、累計多久）時先想：重開之後該不該還在？

- 要按完成的卡（`Card::needs_done`）由核心統一存在 `state.json`，重開時交回給丟出它的功能決定要不要再跳（`Module::restore_card`，刻意沒有預設實作）。
- 核心自己的狀態放 `app.rs` 的 `RuntimeState`、講話的放 `chatter.rs` 的 `ChatterSaved`；功能模組其他的狀態各自存檔。
- 每補一項就加一條「關掉再開」的測試。

## 改完怎麼驗

- **commit 前跑 `cargo fmt`**：沒有自動檢查，漏了不會有人發現（原因見 `docs/DEVELOPMENT.md` 的「排版」）。
- **改了 `crates/petsettings/ui/` 一定要重新 `cargo build -p petsettings`**：Tauri 把前端打包進執行檔，只改檔案的話實機看到的還是舊版。換了 `crates/petsettings/icons/icon.ico` 則要先 `cargo clean -p petsettings`，不然執行檔圖示不會更新。
- 程式圖示（柚子）的原圖去背後放在四處：`docs/media/logo.png`（README）、`crates/petsettings/icons/icon.ico`、`crates/petcore/assets/app.ico`（`app.rc` 編進 Youban.exe）、`crates/petsettings/ui/logo.png`（設定側邊欄左下角，64px）。托盤圖示另外用目前這隻桌寵的站姿圖。
- 看畫面不用動到使用者的螢幕：`Youban.exe --snapshot`（各狀態＋頭上特效）、`--snapshot-cards`（提醒卡、講話泡泡、點心與球）、`--frames`（每個動作 25fps PNG 序列）。
- **動作要給影片看，不要給逐格拼圖**：`--frames` 的序列用 ffmpeg 疊到純色底轉 mp4，再裁到桌寵附近。
- 實機端對端：`Youban.exe --send '{"cmd":"..."}'` 送測試指令（`say`、`toy`、`drop`、`dumpStatus`…，名稱在 `petkit::ipc`），`dumpStatus` 寫到 `%LOCALAPPDATA%\youban-desktop-pet\status.json`。`--send` 從 Bash 下；Windows PowerShell 5.1 會吃掉 JSON 的雙引號。
- **使用者不在電腦前時，桌寵會睡著、也不講話**（看滑鼠鍵盤多久沒動）。這時要實測行為，就暫時把 `config.json` 的 `sleepAfterSecs` 調大，測完還原；或用測試指令直接觸發。需要一個能移動又不搶焦點的測試視窗：PowerShell 開 WinForms，`ShowWindow(h, 4)`＋`SetWindowPos(..., SWP_NOACTIVATE)`。

## 安裝程式

- `installer/youban.iss` 與 `ChineseTraditional.isl` 要存成 **UTF-8 with BOM**，不然 Inno Setup 讀中文會變亂碼。左側大圖、右上小圖（`wizard-*.bmp`、`small-*.bmp`）每種螢幕縮放各一張，換圖時整組重做。
- 本機要做安裝程式：Inno Setup 免安裝解到暫存資料夾、`$env:ISCC` 指過去（見 `docs/DEVELOPMENT.md`），用完刪掉。實測安裝畫面不用搶使用者的滑鼠：找 `TWizardForm` 視窗裡 class 是 `TNewButton` 的按鈕，對它 `PostMessage` 空白鍵按下／放開（`BM_CLICK` 在視窗不在前景時會失效），每頁用 `PrintWindow` 截圖；解除安裝用 `unins000.exe /VERYSILENT /SUPPRESSMSGBOXES`。
- **改了更新或安裝程式，兩種版本都要實際更新一次**：做兩個版號的包，`python -m http.server` 放一份假的 `latest.json`（格式照 GitHub 的 `releases/latest`），`YOUBAN_DESKTOP_PET_UPDATE_URL` 指過去再啟動，刪掉 `%LOCALAPPDATA%\youban-desktop-pet\update\last-check` 讓它馬上查。下載好之後用 `cardAction done` 按「現在更新」；卡片要過幾秒才出來，太早送會被忽略，要重送到 log 出現 `update applied`。安裝版要看三件事：「應用程式」清單的版本號有跟著變；資料夾裡只有 `unins000`，出現 `unins001` 就是 AppId 變了，被當成另一個程式；開機啟動沒被打開。安裝版背景更新的安裝紀錄在 `%LOCALAPPDATA%\youban-desktop-pet\update\setup.log`，更新後柚伴沒回來就先看它。

## 圖片素材

- `crates/petcore/assets/` 的圖用 `include_bytes!` 編進執行檔，不用另外打包。
- 使用者給的圖常是洋紅底的多格拼圖（JPEG）：切格後**從格子邊緣往內填色**當背景（圖案都有深色外框包住，裡面的白色高光不會被誤刪），被圈住的飽和洋紅另外清掉，邊緣用「背景色 → 外框色」的混合比例還原透明度，不然會留粉紅邊。縮到長邊 128～160px 存 PNG。

## Git

- GitHub 上是**公開** repo（私人 repo 的 Release 下載要權杖，自動更新用不了）；什麼不能寫見最上面那節。
- 發版照 `docs/DEVELOPMENT.md` 的「發版」：改版號 → 推帶玩家說明的 annotated tag → GitHub Actions 自動測試、打包、發 Release。已發布的 tag 不要再動。
