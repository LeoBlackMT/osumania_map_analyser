# ManiaMapAnalyser desktop shell — release 打包（Windows）。
# 用法：desktop\release.ps1
# 产物：cargo build --release → 拷贝 mma-shell.exe 到插件目录 → release/ 下
#       插件 zip（含 exe 与 bridges 安装说明引用）。

$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$pluginDir = Join-Path $root "ManiaMapAnalyser by Leo_Black"
$desktop = Join-Path $root "desktop"
$outDir = Join-Path $root "release"
# 版本号以插件 metadata.txt 为唯一来源（避免与 index.js/metadata 漂移）。
$metadataPath = Join-Path $pluginDir "metadata.txt"
$version = ((Get-Content -LiteralPath $metadataPath) |
    Where-Object { $_ -like 'Version:*' } |
    Select-Object -First 1) -replace '^Version:\s*', ''
if (-not $version) { throw "version not found in $metadataPath" }

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
# ⚠️ `Copy-Item -Recurse bridges` 会把**开发产物**一起装进去：NuGet 包缓存
# （`bepinex/.tools/nuget-packages`，约 97 MB）、本地断言工程（`plugin/tests/`）与
# `plugin/{bin,obj}/`。`.gitignore` 只管 git、**不管拷贝**，所以这里必须显式排除，
# 否则分发包会白白大一倍。用 robocopy 的目录级排除，比逐个 Copy-Item 更好维护。
$stage = Join-Path $env:TEMP "mma-release-stage-$PID"
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
Copy-Item -Recurse $pluginDir $stage
Copy-Item $exeSrc (Join-Path $stage "mma-shell.exe")

$bridgesSrc = Join-Path $root "bridges"
$bridgesDst = Join-Path $stage "bridges"
# /E 全部子目录（含空） /XD 排除这些目录名（任意层级）
& robocopy $bridgesSrc $bridgesDst /E /NFL /NDL /NJH /NJS /NP `
    /XD ".tools" "bin" "obj" "tests" | Out-Null
if ($LASTEXITCODE -ge 8) { throw "robocopy failed with code $LASTEXITCODE" }

# 兜底断言：开发产物一个都不许进包
$forbidden = Get-ChildItem $stage -Recurse -Directory |
    Where-Object { $_.Name -in @('.tools', 'tests') -or ($_.Name -in @('bin', 'obj') -and $_.FullName -like '*bepinex*') }
if ($forbidden) {
    throw ("refusing to package dev artefacts: " + (($forbidden | ForEach-Object { $_.FullName.Substring($stage.Length + 1) }) -join ', '))
}

# 兜底断言：安装素材必须随包分发。加载器与 Unity 参考程序集都是 gitignore 的
# 本地资产，插件 DLL 由 build.ps1 落盘——缺任何一个，安装器都会拒绝安装，
# 用户拿到的就是一个装不上的包。
$required = @(
    'bridges\malody\bepinex\loader\bepinex-il2cpp-788.zip',
    'bridges\malody\bepinex\unity-libs\2022.3.62.zip',
    'bridges\malody\bepinex\plugin\MMAMalodySelection.dll'
)
foreach ($rel in $required) {
    if (-not (Test-Path -LiteralPath (Join-Path $stage $rel) -PathType Leaf)) {
        throw "release package is missing a required asset: $rel"
    }
}

Compress-Archive -Path "$stage\*" -DestinationPath $zip -Force
Remove-Item -Recurse -Force $stage

$mb = [math]::Round((Get-Item $zip).Length / 1MB, 1)
Write-Host "released: $zip ($mb MB)"