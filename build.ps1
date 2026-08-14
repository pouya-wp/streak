# Builds Streak and packages the installer into dist\StreakSetup.exe
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

cargo build --release -p streak
if ($LASTEXITCODE -ne 0) { throw "streak build failed" }

cargo build --release -p streak-setup
if ($LASTEXITCODE -ne 0) { throw "installer build failed" }

New-Item -ItemType Directory -Force dist | Out-Null
Copy-Item target\release\StreakSetup.exe dist\StreakSetup.exe -Force
$size = [math]::Round((Get-Item dist\StreakSetup.exe).Length / 1MB, 1)
Write-Host "dist\StreakSetup.exe ($size MB)" -ForegroundColor Green
