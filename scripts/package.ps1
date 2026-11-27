# Build release binaries and pack them into dist/youban-desktop-pet-<version>.zip
# (the name must start with ASSET_PREFIX in crates/petcore/src/updater.rs)
# (the same zip the auto-updater downloads from GitHub Releases).
# Layout inside the zip: Youban.exe, YoubanSettings.exe, pets/
# Only the default pet (DEFAULT_PET in crates/petkit/src/config.rs) ships with its frames; every other
# pet ships pet.json + thumb.png + frames.sha256 and downloads dist/pets/pet-<id>.zip from the same
# release when first picked (crates/petkit/src/pet_download.rs).
# If Inno Setup 6.5+ (ISCC.exe) is installed, also builds dist/youban-desktop-pet-setup-<version>.exe
# (installer/youban.iss). Every output gets a .sha256 next to it ("<hash>  <file>", LF, no BOM)
# so `sha256sum -c` works on Linux / Git Bash too.
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

Copy-Item 'target\release\Youban.exe', 'target\release\YoubanSettings.exe' $stage
$defaultPet = (Select-String -Path 'crates\petkit\src\config.rs' -Pattern '^pub const DEFAULT_PET: &str = "(.+)";').Matches[0].Groups[1].Value
$petZips = Join-Path $root 'dist\pets'
if (Test-Path $petZips) { Remove-Item -Recurse -Force $petZips }
New-Item -ItemType Directory -Force $petZips | Out-Null
$utf8 = New-Object System.Text.UTF8Encoding $false
foreach ($pet in Get-ChildItem 'pets' -Directory) {
    $dest = Join-Path $stage "pets\$($pet.Name)"
    if ($pet.Name -eq $defaultPet) {
        Copy-Item -Recurse $pet.FullName $dest
        continue
    }
    $thumb = Join-Path $pet.FullName 'thumb.png'
    if (-not (Test-Path $thumb)) { throw "$($pet.Name) has no thumb.png (python scripts/art/thumb.py pets/$($pet.Name))" }
    New-Item -ItemType Directory -Force $dest | Out-Null
    Copy-Item (Join-Path $pet.FullName 'pet.json'), $thumb $dest
    # Hash of the frames' names + contents: a new release that changes this pet's art changes it,
    # and installed copies re-download. The app only compares it as text.
    $lines = Get-ChildItem (Join-Path $pet.FullName 'frames') -File | Sort-Object Name | ForEach-Object {
        "$($_.Name) $((Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower())"
    }
    $sha = [System.Security.Cryptography.SHA256]::Create()
    $hash = -join ($sha.ComputeHash($utf8.GetBytes($lines -join "`n")) | ForEach-Object { $_.ToString('x2') })
    [System.IO.File]::WriteAllText((Join-Path $dest 'frames.sha256'), $hash, $utf8)
    tar.exe -a -c -f (Join-Path $petZips "pet-$($pet.Name).zip") -C $pet.FullName frames
    if ($LASTEXITCODE -ne 0) { throw "tar failed for $($pet.Name)" }
}

if (Test-Path $zip) { Remove-Item -Force $zip }
# Not Compress-Archive: Windows PowerShell 5.1 writes backslashes into zip entry names,
# which other tools (and the updater's zip reader) treat as part of the file name.
tar.exe -a -c -f $zip -C $stage pets Youban.exe YoubanSettings.exe
if ($LASTEXITCODE -ne 0) { throw 'tar failed' }
$outputs = @($zip)

# $env:ISCC can point at a portable copy (innosetup-6.x.exe /PORTABLE=1 /DIR=...).
$iscc = @($env:ISCC, "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe", "$env:ProgramFiles\Inno Setup 6\ISCC.exe") | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
if ($iscc) {
    & $iscc /Q "/DAppVersion=$version" "/DSourceDir=$stage" 'installer\youban.iss'
    if ($LASTEXITCODE -ne 0) { throw 'ISCC failed' }
    $outputs += Join-Path $root "dist\youban-desktop-pet-setup-$version.exe"
} else {
    Write-Host 'Inno Setup not found; skipped the installer (zip only)'
}
Remove-Item -Recurse -Force $stage

foreach ($file in $outputs) {
    $hash = (Get-FileHash $file -Algorithm SHA256).Hash.ToLower()
    $name = Split-Path -Leaf $file
    # PowerShell's own file writers add CRLF (and 5.1 a BOM); write the bytes ourselves.
    [System.IO.File]::WriteAllText("$file.sha256", "$hash  $name`n", $utf8)
    Write-Host "packed $file"
    Write-Host "sha256 $hash"
}
Get-ChildItem $petZips | ForEach-Object { Write-Host "packed $($_.FullName)" }
