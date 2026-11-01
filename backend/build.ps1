<#
.SYNOPSIS
  Build the telemetry backend.

.DESCRIPTION
  Defaults to a Windows binary for local testing. Use -Target Linux to
  cross-compile the static amd64 binary deployed to the server. Output goes to
  backend/bin/ (gitignored), or to -OutDir when given.

.EXAMPLE
  .\build.ps1
  .\build.ps1 -Target Linux
  .\build.ps1 -Version 2.2.0 -OutDir ..\temp\backend-build
#>
param(
    [ValidateSet("Windows", "Linux")]
    [string]$Target = "Windows",
    [string]$Version = "3.0.0",
    [string]$OutDir
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
if (-not $OutDir) { $OutDir = Join-Path $root "bin" }
New-Item -ItemType Directory -Path $OutDir -Force | Out-Null

# modernc.org/sqlite is pure Go: CGO is never needed, and disabling it keeps the
# Linux binary static enough to run on the server without a matching libc.
$env:CGO_ENABLED = "0"
if ($Target -eq "Linux") {
    $env:GOOS = "linux"
    $env:GOARCH = "amd64"
    $name = "telemetry-server-linux-amd64"
} else {
    $env:GOOS = "windows"
    $env:GOARCH = "amd64"
    $name = "telemetry-server.exe"
}

$out = Join-Path $OutDir $name
Push-Location $root
try {
    go build -trimpath -ldflags "-s -w -X main.version=$Version" -o $out ./cmd/server
    if ($LASTEXITCODE -ne 0) { throw "go build failed ($LASTEXITCODE)" }
} finally {
    Pop-Location
}

$size = [math]::Round((Get-Item $out).Length / 1MB, 1)
Write-Host "built $Target $Version -> $out ($size MB)"
