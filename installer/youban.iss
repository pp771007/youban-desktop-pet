; 柚伴的安裝程式（Inno Setup 6.5 以上）。由 scripts/package.ps1 呼叫：
;   ISCC /DAppVersion=<版號> /DSourceDir=<打包好的資料夾> installer\youban.iss
; 裝在目前使用者底下（%LOCALAPPDATA%\Programs），不用系統管理員權限、不跳 UAC。

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceDir
  #define SourceDir "..\dist\youban-desktop-pet"
#endif
#define AppName "柚伴"
#define AppExe "Youban.exe"
; 跟 crates/petcore/src/autostart.rs 的 VALUE_NAME 一致：安裝時勾的、設定裡開的是同一筆。
#define RunValue "YoubanDesktopPet"
#define RunKey "Software\Microsoft\Windows\CurrentVersion\Run"
; AppId 決定「這是同一個程式」：重裝、升級會覆蓋同一份，不要改。
#define AppGuid "BE915530-EB05-4723-8486-D27F510796C5"

[Setup]
AppId={{{#AppGuid}}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=pp771007
AppPublisherURL=https://github.com/pp771007/youban-desktop-pet
AppSupportURL=https://github.com/pp771007/youban-desktop-pet/issues
DefaultDirName={localappdata}\Programs\Youban Desktop Pet
DisableProgramGroupPage=yes
; 歡迎頁有左側大圖，像正式軟體；資料夾頁只在第一次裝時出現（之後升級直接沿用）
DisableWelcomePage=no
DisableDirPage=auto
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\dist
OutputBaseFilename=youban-desktop-pet-setup-{#AppVersion}
SetupIconFile=..\crates\petcore\assets\app.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
WizardStyle=modern
; 左側大圖與右上小圖，每種螢幕縮放各一張（Inno 自己挑最接近的，不會糊）
WizardImageFile=wizard-164.bmp,wizard-192.bmp,wizard-246.bmp,wizard-273.bmp,wizard-328.bmp,wizard-355.bmp,wizard-410.bmp
WizardSmallImageFile=small-55.bmp,small-64.bmp,small-83.bmp,small-92.bmp,small-110.bmp,small-119.bmp,small-138.bmp
Compression=lzma2/max
SolidCompression=yes
; 裝新版時柚伴正在跑：自動請它關掉，裝完不自動重開（最後一頁有「啟動柚伴」可以勾）。
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "zh"; MessagesFile: "ChineseTraditional.isl"

[Messages]
WelcomeLabel1=歡迎來到柚伴
WelcomeLabel2=接下來會把柚伴 {#AppVersion} 裝到你的電腦上。%n%n一隻住在桌面上的小水豚：在工作列上散步、陪你上班、提醒你休息。%n%n不需要系統管理員權限，大約幾秒鐘就好。
FinishedHeadingLabel=柚伴裝好了
FinishedLabel=水豚已經準備好搬進你的桌面了。對牠按右鍵可以打開設定。

[Tasks]
; 只在第一次裝時問：之後在設定裡開關的才算數，升級（包括自動更新）不能把使用者關掉的又打開。
Name: "autostart"; Description: "開機時自動啟動柚伴"; Check: not IsUpgrade
Name: "desktopicon"; Description: "在桌面建立捷徑"; Flags: unchecked

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "{#RunKey}"; ValueType: string; ValueName: "{#RunValue}"; ValueData: """{app}\{#AppExe}"""; Tasks: autostart

[Run]
Filename: "{app}\{#AppExe}"; Description: "啟動柚伴"; Flags: nowait postinstall skipifsilent
; 柚伴自動更新時帶 /relaunch=1 安靜地跑安裝程式（crates/petcore/src/updater.rs 的 SETUP_ARGS）：裝完把牠開回來。
Filename: "{app}\{#AppExe}"; Parameters: "--after-update"; Flags: nowait; Check: IsUpdateRelaunch

[UninstallDelete]
; 預設以外的內建桌寵，動作圖是裝好之後才下載的（crates/petkit/src/pet_download.rs），安裝程式不認得，要點名刪。
; 使用者自己做的桌寵沒有 frames.sha256，不會被刪。
#define FindHandle
#define FindResult
#sub AddDownloadedFrames
  #define PetName FindGetFileName(FindHandle)
  #if PetName != "." && PetName != ".." && FileExists(SourceDir + "\pets\" + PetName + "\frames.sha256")
Type: filesandordirs; Name: "{app}\pets\{#PetName}\frames"
  #endif
#endsub
#for {FindHandle = FindResult = FindFirst(SourceDir + "\pets\*", faDirectory); FindResult; FindResult = FindNext(FindHandle)} AddDownloadedFrames
#if FindHandle
  #expr FindClose(FindHandle)
#endif

[UninstallRun]
; 先請執行中的柚伴自己結束，檔案才刪得掉。
Filename: "{app}\{#AppExe}"; Parameters: "--quit"; Flags: runhidden waituntilterminated; RunOnceId: "QuitYouban"

[Code]
function IsUpgrade: Boolean;
begin
  Result := RegKeyExists(HKEY_CURRENT_USER, 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{' + '{#AppGuid}' + '}_is1');
end;

function IsUpdateRelaunch: Boolean;
begin
  Result := ExpandConstant('{param:relaunch|0}') = '1';
end;

// 開機自動啟動可能是安裝時勾的，也可能是之後在設定裡打開的：解除安裝時一律清掉，不留一筆指向已刪檔案的啟動項目。
// 設定、提醒、陪伴天數（%APPDATA%\youban-desktop-pet）刻意留著，重裝回來還在。
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
    RegDeleteValue(HKEY_CURRENT_USER, '{#RunKey}', '{#RunValue}');
end;
