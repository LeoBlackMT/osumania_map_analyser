# ManiaMapAnalyser desktop shell — release 打包（Windows）。
# 用法：.\release.ps1
# 产物：cargo build --release → 拷贝 mma-shell.exe 到插件目录 → release/{timestamp}-{version}/ 下
#       包含：
#       1. ManiaMapAnalyser-by-Leo_Black-v{version}-with-shell.zip（插件 + exe + 偏移表 + bridges）
#       2. ManiaMapAnalyser-by-Leo_Black-v{version}.zip（纯插件本体）
#
# 反馈签名（必填）：先设置 MMA_FEEDBACK_SIGNING_SEED（64 hex）与 MMA_FEEDBACK_KEY_ID，
# 或设置 MMA_FEEDBACK_SEED_FILE 指向一个只含 seed 的本地文件（gitignore）。
# 本地测试可加 -AllowDevSigning 使用内置 dev seed（服务端需 FEEDBACK_ALLOW_DEV_KEY=true）。
# 只检查签名配置不打包：.\release.ps1 -SigningCheckOnly

param(
    [switch]$SkipBuild,
    [switch]$AllowDevSigning,
    [switch]$SigningCheckOnly
)

$ErrorActionPreference = "Stop"
$root = $PSScriptRoot
$pluginDir = Join-Path $root "ManiaMapAnalyser by Leo_Black"
$desktop = Join-Path $root "desktop"

# 反馈签名 seed 只从环境变量或本地 seed 文件读取；绝不写进仓库或日志。
function Resolve-FeedbackSigning {
    param([switch]$AllowDevSigning)

    $seed = $env:MMA_FEEDBACK_SIGNING_SEED
    if (-not $seed -and $env:MMA_FEEDBACK_SEED_FILE) {
        $seedPath = $env:MMA_FEEDBACK_SEED_FILE
        if (-not (Test-Path -LiteralPath $seedPath -PathType Leaf)) {
            throw "MMA_FEEDBACK_SEED_FILE not found: $seedPath"
        }
        $seed = Get-Content -LiteralPath $seedPath -Raw
    }
    $seed = ("$seed").Trim()

    if ($seed) {
        if ($seed -notmatch '^[0-9a-fA-F]{64}$') {
            throw "MMA_FEEDBACK_SIGNING_SEED must be 64 hex chars (32 bytes); got $($seed.Length) chars"
        }
        $keyId = ("$($env:MMA_FEEDBACK_KEY_ID)").Trim()
        if (-not $keyId) { $keyId = "release" }
        if ($keyId -notmatch '^[A-Za-z0-9_-]{1,32}$') {
            throw "MMA_FEEDBACK_KEY_ID must be 1-32 chars of [A-Za-z0-9_-]"
        }
        $env:MMA_FEEDBACK_SIGNING_SEED = $seed.ToLowerInvariant()
        $env:MMA_FEEDBACK_KEY_ID = $keyId
        Write-Host "Feedback signing: key_id=$keyId"

        $keygenDir = Join-Path $root "backend"
        $go = Get-Command go -ErrorAction SilentlyContinue
        if ($go -and (Test-Path -LiteralPath (Join-Path $keygenDir "cmd\feedback-keygen\main.go"))) {
            Push-Location $keygenDir
            try {
                $pub = (& go run ./cmd/feedback-keygen -derive -public-only 2>$null | Select-Object -Last 1)
                $goOk = ($LASTEXITCODE -eq 0)
            } finally {
                Pop-Location
            }
            if ($goOk -and $pub) {
                $pub = $pub.Trim()
                Write-Host "Feedback public key: $pub"
                Write-Host "  -> server FEEDBACK_PUBLIC_KEYS must contain: ${keyId}:${pub}"
            } else {
                Write-Warning "Could not derive the public key (go run failed). Verify it manually."
            }
        } else {
            Write-Warning "Go not found; derive the public key with: cd backend; go run ./cmd/feedback-keygen -derive -public-only"
        }
    } elseif ($AllowDevSigning) {
        Remove-Item Env:MMA_FEEDBACK_SIGNING_SEED -ErrorAction SilentlyContinue
        Remove-Item Env:MMA_FEEDBACK_KEY_ID -ErrorAction SilentlyContinue
        Write-Warning "Feedback signing: DEV SEED (key_id=dev). The server accepts it only with FEEDBACK_ALLOW_DEV_KEY=true."
    } else {
        throw "Feedback signing seed missing. Set MMA_FEEDBACK_SIGNING_SEED (64 hex) or MMA_FEEDBACK_SEED_FILE, or pass -AllowDevSigning for a local test build."
    }
}

Resolve-FeedbackSigning -AllowDevSigning:$AllowDevSigning
if ($SigningCheckOnly) {
    Write-Host "Signing check passed."
    exit 0
}

# 版本号以插件 metadata.txt 为唯一来源（避免与 index.js/metadata 漂移）。
$metadataPath = Join-Path $pluginDir "metadata.txt"
$version = ((Get-Content -LiteralPath $metadataPath) |
    Where-Object { $_ -like 'Version:*' } |
    Select-Object -First 1) -replace '^Version:\s*', ''
if (-not $version) { throw "version not found in $metadataPath" }

$timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
$outDir = Join-Path $root "release\${timestamp}-${version}"
New-Item -ItemType Directory -Path $outDir -Force | Out-Null

if (-not (Test-Path (Join-Path $pluginDir "index.html"))) {
    throw "plugin dir not found: $pluginDir"
}

if (-not $env:CARGO_TARGET_DIR) {
    $env:CARGO_TARGET_DIR = Join-Path $env:TEMP "cargo-target"
}

if (-not $SkipBuild) {
    Push-Location $desktop
    try {
        Write-Host "Building release binary..."
        cargo build --release
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    } finally {
        Pop-Location
    }
}

$exeSrc = if ($env:CARGO_TARGET_DIR -and (Test-Path (Join-Path $env:CARGO_TARGET_DIR "release\mma-shell.exe"))) {
    Join-Path $env:CARGO_TARGET_DIR "release\mma-shell.exe"
} else {
    Join-Path $desktop "target\release\mma-shell.exe"
}
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
# 排除开发依赖（如 companella ORT 的 node_modules）
Get-ChildItem -Path $stage -Recurse -Directory -Filter "node_modules" | Remove-Item -Recurse -Force
Copy-Item $exeSrc (Join-Path $stage "mma-shell.exe")

# 拷贝统一偏移表（desktop/offsets，包含 stable 与 lazer 原生内存读取必需的纯数据表与 README）
$offsetsSrc = Join-Path $desktop "offsets"
if (Test-Path $offsetsSrc) {
    Copy-Item -Recurse $offsetsSrc (Join-Path $stage "offsets")
}

# 编译随包分发的点击即用偏移生成器 gen.exe（放入 offsets/ 目录）
$genSrc = Join-Path $root "tools\lazer-offsets-gen\main.rs"
if (Test-Path $genSrc) {
    Write-Host "Building gen.exe (offsets generator)..."
    $offsetsStage = Join-Path $stage "offsets"
    if (-not (Test-Path $offsetsStage)) {
        New-Item -ItemType Directory -Path $offsetsStage -Force | Out-Null
    }
    $genDst = Join-Path $offsetsStage "gen.exe"
    & rustc --edition 2021 -O -A dead_code "-Clink-arg=/MANIFEST:EMBED" "-Clink-arg=/MANIFESTUAC:level='requireAdministrator' uiAccess='false'" -o $genDst $genSrc
    if ($LASTEXITCODE -ne 0) { throw "rustc build of gen.exe failed" }
}

$bridgesSrc = Join-Path $root "bridges"
$bridgesDst = Join-Path $stage "bridges"
# /E 全部子目录（含空） /XD 排除这些目录名（任意层级）
& robocopy $bridgesSrc $bridgesDst /E /NFL /NDL /NJH /NJS /NP `
    /XD ".tools" "bin" "obj" "tests" | Out-Null
if ($LASTEXITCODE -ge 8) { throw "robocopy failed with code $LASTEXITCODE" }

# `bepinex\plugin\` 是插件的**开发树**（5 个 .cs、.csproj、NuGet.Config、build.ps1）。
# 安装器只读 `MMAMalodySelection.dll`（install-bridge.ps1 的 $BridgeDllRel）；LICENSE 是
# 该 DLL 的 MIT 归属声明、NOTICES.md 是它的来源与复现说明，两者必须随二进制分发。
# 其余文件对用户没有任何用途，只留这三份。
$pluginStage = Join-Path $bridgesDst "malody\bepinex\plugin"
$pluginShip = @('MMAMalodySelection.dll', 'LICENSE', 'NOTICES.md')
Get-ChildItem -LiteralPath $pluginStage -File |
    Where-Object { $_.Name -notin $pluginShip } |
    Remove-Item -Force

# 兜底断言：开发产物一个都不许进包
$forbidden = Get-ChildItem $stage -Recurse -Directory |
    Where-Object { $_.Name -in @('.tools', 'tests', 'node_modules') -or ($_.Name -in @('bin', 'obj') -and $_.FullName -like '*bepinex*') }
if ($forbidden) {
    throw ("refusing to package dev artefacts: " + (($forbidden | ForEach-Object { $_.FullName.Substring($stage.Length + 1) }) -join ', '))
}

# 兜底断言：插件目录里只该有那三份安装素材。将来若有人往 plugin/ 里加了新文件又被拷进包，
# 这里会直接失败，而不是把开发文件悄悄发给用户。
$shippedPluginFiles = @(Get-ChildItem -LiteralPath $pluginStage -File | Select-Object -ExpandProperty Name | Sort-Object)
if (($shippedPluginFiles -join ',') -ne (($pluginShip | Sort-Object) -join ',')) {
    throw ("unexpected files in the packaged plugin folder: " + ($shippedPluginFiles -join ', '))
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
Write-Host "released (with shell):  $zip ($mb MB)"

# 纯插件本体包（供独立 tosu 用户使用）
$pluginOnlyZip = Join-Path $outDir "ManiaMapAnalyser-by-Leo_Black-v$version.zip"
if (Test-Path $pluginOnlyZip) { Remove-Item $pluginOnlyZip }
$pluginStage = Join-Path $env:TEMP "mma-plugin-stage-$PID"
if (Test-Path $pluginStage) { Remove-Item -Recurse -Force $pluginStage }
Copy-Item -Recurse $pluginDir $pluginStage
Get-ChildItem -Path $pluginStage -Recurse -Directory -Filter "node_modules" | Remove-Item -Recurse -Force
Compress-Archive -Path "$pluginStage\*" -DestinationPath $pluginOnlyZip -Force
Remove-Item -Recurse -Force $pluginStage

$pluginMb = [math]::Round((Get-Item $pluginOnlyZip).Length / 1MB, 1)
Write-Host "released (plugin only): $pluginOnlyZip ($pluginMb MB)"