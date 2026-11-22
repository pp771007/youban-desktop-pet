# Build release binaries and pack them into dist/youban-desktop-pet-<version>.zip
# (the name must start with ASSET_PREFIX in crates/petcore/src/updater.rs)
# (the same zip the auto-updater downloads from GitHub Releases).
# Layout inside the zip: petcore.exe, petsettings.exe, pets/
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

cargo build --release
if ($LASTEXITCODE -ne 0) { throw 'cargo build --release failed' }

$version = (Select-String -Path 'Cargo.toml' -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$stage = Join-Path $root "dist\youban-desktop-pet"
$zip = Join-Path $root "dist\youban-desktop-pet-$version.zip"
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Force $stage | Out-Null

Copy-Item 'target\release\petcore.exe', 'target\release\petsettings.exe' $stage
Copy-Item -Recurse 'pets' (Join-Path $stage 'pets')

if (Test-Path $zip) { Remove-Item -Force $zip }
# Not Compress-Archive: Windows PowerShell 5.1 writes backslashes into zip entry names,
# which other tools (and the updater's zip reader) treat as part of the file name.
tar.exe -a -c -f $zip -C $stage pets petcore.exe petsettings.exe
if ($LASTEXITCODE -ne 0) { throw 'tar failed' }
Remove-Item -Recurse -Force $stage
Write-Host "packed $zip"
