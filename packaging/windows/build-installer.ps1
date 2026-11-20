# Builds the Windows installer: release binary, then Inno Setup (ISCC.exe) on mathcraft.iss.
#
#   packaging\windows\build-installer.ps1               version read from Cargo.toml
#   packaging\windows\build-installer.ps1 -Version 0.2.0
#
# Output: target\packaging\Mathcraft-<version>-windows-x64-setup.exe
# Requires Inno Setup 6 (https://jrsoftware.org/isinfo.php); ISCC.exe is looked up on PATH and in
# the default install folder.
param([string]$Version = "")

$ErrorActionPreference = "Stop"
$Root = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
Push-Location $Root
try {
    if (-not $Version) {
        $line = Select-String -Path "Cargo.toml" -Pattern '^version = "(.*)"' | Select-Object -First 1
        $Version = $line.Matches[0].Groups[1].Value
    }
    # Both binaries (the window and mathcraft-mcp) carry the offscreen renderer for the render tools.
    cargo build --release --locked -p mathcraft-app --features render
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

    $iscc = (Get-Command ISCC.exe -ErrorAction SilentlyContinue).Source
    if (-not $iscc) {
        $iscc = Join-Path ${env:ProgramFiles(x86)} "Inno Setup 6\ISCC.exe"
    }
    if (-not (Test-Path $iscc)) { throw "ISCC.exe not found; install Inno Setup 6" }

    & $iscc "/DMyAppVersion=$Version" "packaging\windows\mathcraft.iss"
    if ($LASTEXITCODE -ne 0) { throw "ISCC failed" }
    Write-Host "wrote target\packaging\Mathcraft-$Version-windows-x64-setup.exe"
}
finally {
    Pop-Location
}
