# 開發

- 規劃與技術選型：[PLAN.md](PLAN.md)
- 視覺與 UI 設計系統：[design-system.md](design-system.md)
- 桌寵包規範：[pet-pack-spec.md](pet-pack-spec.md)
- 出圖提示詞：[art-prompts.md](art-prompts.md)

## 組成

| 資料夾 | 是什麼 |
|---|---|
| `crates/petcore` | 桌寵本體（常駐）：桌寵視窗、提醒卡、功能模組、托盤 |
| `crates/petsettings` | 設定程式（Tauri，用到才開） |
| `crates/petkit` | 兩支共用：桌寵包、提醒排程、辦公日曆、設定檔、檔案位置 |
| `design/tokens.json` | 設計參數唯一來源（顏色、間距、圓角、字級、動畫） |
| `pets/` | 桌寵包，放進一個資料夾就多一隻 |
| `crates/petcore/assets/` | 點心、球、表情符號的圖與程式圖示（編進執行檔） |
| `data/holidays/` | 內建的人事總處辦公日曆（執行時每週自動下載更新） |

## 執行

```
cargo build
target\debug\Youban.exe           # 桌寵（crate 名稱是 petcore）
target\debug\YoubanSettings.exe   # 設定（crate 名稱是 petsettings；也可以從桌寵右鍵選單開）
```

`Youban.exe` 的其他用法：

| 指令 | 用途 |
|---|---|
| `--send '{"cmd":"testReminder"}'` | 送指令給執行中的桌寵（指令名稱見 `petkit::ipc`） |
| `--autostart on\|off\|status` | 開機自動啟動（寫進目前使用者的「執行」登錄，不用系統管理員權限；設定 →「一般」的開關也是這個） |
| `--snapshot <out.png> [dpi]` | 不開視窗，輸出各狀態的桌寵截圖 |
| `--snapshot-cards <out.png> [dpi]` | 不開視窗，輸出各種提醒卡（淺色＋深色）、講話泡泡、點心與球 |
| `--frames <資料夾> [dpi]` | 不開視窗，每個動作輸出 25fps 的 PNG 序列（檢查動態用） |
| `--slice <多格圖.png> <桌寵> <名稱…>` | 把 AI 一次畫的多個姿勢自動切成單張，放進桌寵包（名稱寫 `-` 的那格不要） |
| `--soak <秒>` | 記憶體壓力測試，結果寫到 `%LOCALAPPDATA%\youban-desktop-pet\soak-result.json` |

## 發版

1. `Cargo.toml` 的版本號加一，commit、push
2. 打 tag，訊息寫給玩家看的更新說明（會變成 Release 的說明），推上去：

   ```
   git tag -a v0.2.0 -m "新增：……"
   git push origin v0.2.0
   ```

3. GitHub Actions（`.github/workflows/release.yml`）會在 Windows 機器上測試、打包、發 Release，掛上安裝版 `youban-desktop-pet-setup-<版本>.exe`（`installer/youban.iss`，Inno Setup）、免安裝版 `youban-desktop-pet-<版本>.zip`，各附一個 `.sha256`，說明最後自動附上驗證碼。tag 跟 `Cargo.toml` 版號對不上就直接失敗，不會發。

想先確認建置沒壞：Actions 頁手動 Run workflow，只測試和打包（zip 留在執行紀錄），不發 Release。
本機打包：`powershell -File scripts\package.ps1`（產生 zip；有裝 Inno Setup 6.5 以上才會一起做安裝程式）。不想裝進系統的話，Inno Setup 的安裝檔可以 `/PORTABLE=1 /CURRENTUSER /DIR=<資料夾>` 解到任何地方，再把 `$env:ISCC` 指到裡面的 `ISCC.exe`。
在 Git Bash 裡跑會叫到 GNU tar 而失敗，請從 PowerShell 跑。

柚伴每天看一次最新的 Release，有新版就自動下載、找空檔換上（設定 →「一般」可以關）。安裝版下載安裝程式在背景安靜地裝，免安裝版下載 zip 直接換檔。
設定 →「一般」的「檢查更新」不等每天的定時，馬上查（自動更新關著也能按）；進度由核心寫在 `%LOCALAPPDATA%\youban-desktop-pet\update\status.json`。
從 `target\` 底下跑的開發版不會自動更新，按「檢查更新」會說「開發中的版本不會更新」。

## 測試

```
cargo test                                         # 規則與邏輯（排程、假日、桌寵包、行為、模組）
node crates/petsettings/ui-tests/settings.test.mjs # 設定畫面（無頭瀏覽器）
target\debug\Youban.exe --soak 600                # 記憶體曲線要平
```
