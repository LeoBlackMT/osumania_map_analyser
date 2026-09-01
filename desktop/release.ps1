# ManiaMapAnalyser desktop shell — release 打包（Windows）。
# 用法：desktop\release.ps1
# 产物：cargo build --release → 拷贝 mma-shell.exe 到插件目录 → release/ 下
#       插件 zip（含 exe 与 bridges 安装说明引用）。

$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$pluginDir = Join-Path $root "ManiaMapAnalyser by Leo_Black"
$desktop = Join-Path $root "desktop"
$outDir = Join-Path $root "release"
$version = "2.1.0"

if (-not (Test-Path (Join-Path $pluginDir "index.html"))) {
    throw "plugin dir not found: $pluginDir"
}

Push-Location $desktop
cargo build --release
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
Pop-Location

$exeSrc = Join-Path $desktop "target\release\mma-shell.exe"
if (-not (Test-Path $exeSrc)) { throw "release exe missing: $exeSrc" }

New-Item -ItemType Directory -Path $outDir -Force | Out-Null
$zip = Join-Path $outDir "ManiaMapAnalyser-by-Leo_Black-v$version-with-shell.zip"
if (Test-Path $zip) { Remove-Item $zip }

# 打包：插件目录 + exe + bridges（桥安装素材）。
$stage = Join-Path $env:TEMP "mma-release-stage-$PID"
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
Copy-Item -Recurse $pluginDir $stage
Copy-Item $exeSrc (Join-Path $stage "mma-shell.exe")
Copy-Item -Recurse (Join-Path $root "bridges") (Join-Path $stage "bridges")
Compress-Archive -Path "$stage\*" -DestinationPath $zip -Force
Remove-Item -Recurse -Force $stage

Write-Host "released: $zip"