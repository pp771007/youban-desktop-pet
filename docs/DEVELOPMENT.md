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
target\debug\petcore.exe          # 桌寵
target\debug\petsettings.exe      # 設定（也可以從桌寵右鍵選單開）
```

`petcore.exe` 的其他用法：

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

```
powershell -File scripts\package.ps1     # 產生 dist\youban-desktop-pet-<版本>.zip
```

把 `Cargo.toml` 的版本號加一、跑上面那行，再把 zip 掛到 GitHub Release（tag `v<版本>`）。
柚伴每天看一次最新的 Release，有新版就自動下載、找空檔換上（設定 →「一般」可以關）。
從 `target\` 底下跑的開發版不會自動更新。

## 測試

```
cargo test                                         # 規則與邏輯（排程、假日、桌寵包、行為、模組）
node crates/petsettings/ui-tests/settings.test.mjs # 設定畫面（無頭瀏覽器）
target\debug\petcore.exe --soak 600                # 記憶體曲線要平
```
