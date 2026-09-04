<#
.SYNOPSIS
    Interactive installer/remover for the ManiaMapAnalyser game bridge files
    (Etterna theme LoadActor injection + Malody V editor plugin).

.DESCRIPTION
    Installs or removes:
      - Etterna :  bridges/etterna/mma_bridge.lua     -> Themes\<theme>\BGAnimations\ScreenSelectMusic decorations\
                  bridges/etterna/mma_gameplay.lua    -> Themes\<theme>\BGAnimations\ScreenGameplay overlay\
                  plus one LoadActor line injected before `return t` in each
                  screen's default.lua.
      - Malody V:  bridges/malody/mma_editor.lua      -> MalodyV\editor\

    Game root directories are auto-detected (running process first, then the
    MMA_ETTERNA_ROOT / MMA_MALODY_ROOT env vars, then Steam libraries found via
    the registry + steamapps/libraryfolders.vdf, then common install paths),
    with a manual entry fallback. The Etterna target theme defaults to Rebirth
    and falls back to a theme picker when Rebirth is missing.

    The installer never touches other scripts' LoadActor lines (e.g. DanOverlay,
    elements, titlesplash): it only adds its own line idempotently and removes
    only its own line on uninstall. Files are written as UTF-8 without BOM so
    LuaJIT (Etterna) never chokes on a BOM.

.PARAMETER Game
    'Etterna' or 'Malody' to skip the game picker. Omit for the menu.

.PARAMETER Uninstall
    Remove instead of install.

.PARAMETER Yes
    Skip confirmation prompts where a safe default exists (advanced usage,
    e.g. automation). Interactive menus are still shown when multiple
    candidates exist.

.PARAMETER Root
    Use this game root directory directly, skipping auto-detection.

.PARAMETER Theme
    Etterna theme name to use, skipping the theme picker.

.PARAMETER ConfigPath
    Path to mma-shell-config.json. Defaults to the plugin root next to the
    bridges\ folder this script lives in (the shell looks for it next to
    mma-shell.exe).

.NOTES
    Target: Windows PowerShell 5.1+ (built into Windows 10/11) and pwsh 7.
    UTF-8 without BOM. English only (no console codepage pitfalls).
#>
[CmdletBinding()]
param(
    [ValidateSet('Etterna', 'Malody')]
    [string]$Game,
    [switch]$Uninstall,
    [switch]$Yes,
    [string]$Root,
    [string]$Theme,
    [string]$ConfigPath
)

$ErrorActionPreference = 'Stop'

# ---------------------------------------------------------------------------
# helpers
# ---------------------------------------------------------------------------

function Write-Step {
    param([string]$Status, [string]$Msg)
    $color = switch ($Status) {
        'OK'   { 'Green' }
        'SKIP' { 'Yellow' }
        'FAIL' { 'Red' }
        'INFO' { 'Cyan' }
        default { 'Gray' }
    }
    Write-Host ("[{0}] {1}" -f $Status, $Msg) -ForegroundColor $color
}

function Show-Banner {
    Write-Host ''
    Write-Host '==================================================' -ForegroundColor Cyan
    Write-Host ' ManiaMapAnalyser - Bridge Installer v1.0.0' -ForegroundColor Cyan
    Write-Host '==================================================' -ForegroundColor Cyan
    Write-Host ' Installs / removes data bridges for mma-shell:' -ForegroundColor Gray
    Write-Host '   - Etterna theme bridge (mma_bridge / mma_gameplay)' -ForegroundColor Gray
    Write-Host '   - Malody V editor plugin (mma_editor)' -ForegroundColor Gray
    Write-Host ''
}

function Read-Option {
    param(
        [string]$Title,
        [string[]]$Options,
        [string]$Extra = $null
    )
    Write-Host ''
    Write-Host $Title -ForegroundColor Cyan
    for ($i = 0; $i -lt $Options.Count; $i++) {
        Write-Host ("  [{0}] {1}" -f ($i + 1), $Options[$i])
    }
    if ($Extra) {
        Write-Host ("  [{0}] {1}" -f ($Options.Count + 1), $Extra)
    }
    while ($true) {
        $input = Read-Host 'Enter choice'
        $n = 0
        if ([int]::TryParse($input, [ref]$n)) {
            if ($n -ge 1 -and $n -le $Options.Count) { return ($n - 1) }
            if ($Extra -and $n -eq $Options.Count + 1) { return $n - 1 }
        }
        Write-Host 'Invalid choice, try again.' -ForegroundColor Yellow
    }
}

function Confirm-YesNo {
    param(
        [string]$Prompt,
        [bool]$DefaultYes = $true
    )
    $hint = if ($DefaultYes) { 'Y/n' } else { 'y/N' }
    while ($true) {
        $input = Read-Host ("{0} [{1}]" -f $Prompt, $hint)
        if ([string]::IsNullOrWhiteSpace($input)) { return $DefaultYes }
        if ($input -match '^(y|yes)$') { return $true }
        if ($input -match '^(n|no)$') { return $false }
        Write-Host 'Please answer y or n.' -ForegroundColor Yellow
    }
}

function Read-CustomPath {
    param([string]$What, [scriptblock]$Validator)
    while ($true) {
        $p = Read-Host ("Enter the full {0} folder path" -f $What)
        if ([string]::IsNullOrWhiteSpace($p)) { return $null }
        $p = $p.Trim().TrimEnd('\', '/')
        if ($p -and (& $Validator $p)) { return $p }
        Write-Step FAIL "Not a valid $What folder (path check failed): $p"
    }
}

function Get-SteamLibraryRoots {
    $roots = New-Object 'System.Collections.Generic.List[string]'
    # 1. registry
    try {
        $v = (Get-ItemProperty -Path 'HKCU:\Software\Valve\Steam' -Name SteamPath -ErrorAction Stop).SteamPath
        if ($v) { $roots.Add($v) }
    } catch { }
    foreach ($key in @('HKLM:\SOFTWARE\Valve\Steam', 'HKLM:\SOFTWARE\WOW6432Node\Valve\Steam')) {
        try {
            $v = (Get-ItemProperty -Path $key -Name InstallPath -ErrorAction Stop).InstallPath
            if ($v) { $roots.Add($v) }
        } catch { }
    }
    # 2. libraryfolders.vdf inside every known root
    $snapshot = @($roots)
    foreach ($r in $snapshot) {
        $vdf = Join-Path $r 'steamapps\libraryfolders.vdf'
        if (Test-Path $vdf) {
            $text = Get-Content -LiteralPath $vdf -Raw -ErrorAction SilentlyContinue
            if ($text) {
                foreach ($m in [regex]::Matches($text, '"path"\s+"([^"]+)"')) {
                    $p = $m.Groups[1].Value -replace '\\\\', '\'
                    if ($p -and -not $roots.Contains($p)) { $roots.Add($p) }
                }
            }
        }
    }
    return $roots
}

function Test-EtternaRoot {
    param([string]$p)
    if (-not $p) { return $false }
    $save = Join-Path $p 'Save'
    $themes = Join-Path $p 'Themes'
    return (Test-Path $save -PathType Container) -and (Test-Path $themes -PathType Container)
}

function Test-MalodyRoot {
    param([string]$p)
    if (-not $p) { return $false }
    return (Test-Path (Join-Path $p 'chart') -PathType Container) -and
           (Test-Path (Join-Path $p 'skin') -PathType Container)
}

function Get-RunningProcessRoot {
    param([string[]]$Names)
    foreach ($name in $Names) {
        try {
            $proc = Get-Process -Name $name -ErrorAction SilentlyContinue | Select-Object -First 1
            if ($proc -and $proc.Path) {
                return (Split-Path $proc.Path -Parent)
            }
        } catch { }
    }
    return $null
}

function Get-EtternaCandidates {
    $cands = New-Object 'System.Collections.Generic.List[string]'
    $add = {
        param($p)
        if ($p -and (Test-EtternaRoot $p) -and -not $cands.Contains($p)) { $cands.Add($p) }
    }
    # running process
    $p = Get-RunningProcessRoot -Names 'Etterna'
    if ($p) { & $add $p }
    # env override
    if ($env:MMA_ETTERNA_ROOT) { & $add $env:MMA_ETTERNA_ROOT }
    # steam libraries
    foreach ($lib in (Get-SteamLibraryRoots)) {
        & $add (Join-Path $lib 'steamapps\common\Etterna')
    }
    # common install paths
    foreach ($c in @('D:/Games/Etterna', 'C:/Games/Etterna', 'D:/Etterna', 'C:/Etterna')) {
        & $add $c
    }
    return @($cands)
}

function Get-MalodyCandidates {
    $cands = New-Object 'System.Collections.Generic.List[string]'
    $add = {
        param($p)
        if ($p -and (Test-MalodyRoot $p) -and -not $cands.Contains($p)) { $cands.Add($p) }
    }
    $p = Get-RunningProcessRoot -Names 'Malody V', 'MalodyV'
    if ($p) { & $add $p }
    if ($env:MMA_MALODY_ROOT) { & $add $env:MMA_MALODY_ROOT }
    foreach ($lib in (Get-SteamLibraryRoots)) {
        & $add (Join-Path $lib 'steamapps\common\MalodyV')
    }
    foreach ($c in @(
        'D:/Steam/steamapps/common/MalodyV',
        'D:/SteamLibrary/steamapps/common/MalodyV',
        'C:/Program Files (x86)/Steam/steamapps/common/MalodyV',
        'C:/SteamLibrary/steamapps/common/MalodyV'
    )) {
        & $add $c
    }
    return @($cands)
}

function Select-GameRoot {
    param(
        [string]$Game,
        [string[]]$Candidates,
        [string]$ForceRoot
    )
    $validator = if ($Game -eq 'Etterna') { { param($p) Test-EtternaRoot $p } } else { { param($p) Test-MalodyRoot $p } }
    if ($ForceRoot) {
        if (& $validator $ForceRoot) { return $ForceRoot.TrimEnd('\', '/') }
        Write-Step FAIL "Provided -Root is not a valid $Game folder: $ForceRoot"
        return $null
    }
    if ($Candidates.Count -eq 1) {
        if ($Yes) { return $Candidates[0] }
        if (Confirm-YesNo ("Use detected {0} folder:{1}  {2}" -f $Game, [Environment]::NewLine, $Candidates[0])) {
            return $Candidates[0]
        }
    } elseif ($Candidates.Count -gt 1) {
        $idx = Read-Option -Title "Multiple $Game installs detected, pick one:" -Options $Candidates -Extra 'Enter manually...'
        if ($idx -lt $Candidates.Count) { return $Candidates[$idx] }
    } else {
        Write-Step INFO "No $Game install auto-detected"
    }
    return Read-CustomPath -What $Game -Validator $validator
}

function Get-Themes {
    param([string]$EtternaRoot)
    $dir = Join-Path $EtternaRoot 'Themes'
    if (-not (Test-Path $dir -PathType Container)) { return @() }
    return @(Get-ChildItem -LiteralPath $dir -Directory -ErrorAction SilentlyContinue |
        Select-Object -ExpandProperty Name)
}

function Select-Theme {
    param(
        [string]$EtternaRoot,
        [string]$ForcedTheme
    )
    $themes = Get-Themes -EtternaRoot $EtternaRoot
    if ($themes.Count -eq 0) {
        Write-Step FAIL "No theme folders found under $EtternaRoot\Themes"
        return $null
    }
    if ($ForcedTheme) {
        if ($themes -contains $ForcedTheme) { return $ForcedTheme }
        Write-Step FAIL "Theme '$ForcedTheme' not found under $EtternaRoot\Themes"
        return $null
    }
    # default: Rebirth (does NOT offer installing to all themes at once)
    if ($themes -contains 'Rebirth') {
        if ($Yes) { return 'Rebirth' }
        if (Confirm-YesNo ("Use theme 'Rebirth'? (available themes: {0})" -f ($themes -join ', '))) {
            return 'Rebirth'
        }
    }
    $idx = Read-Option -Title 'Select the Etterna theme to install into:' -Options $themes
    return $themes[$idx]
}

# ---------------------------------------------------------------------------
# default.lua injection (idempotent, coexists with other scripts)
# ---------------------------------------------------------------------------

function Get-FileNewline {
    param([string]$Text)
    if ($Text.Contains("`r`n")) { return "`r`n" }
    if ($Text.Contains("`n")) { return "`n" }
    return [Environment]::NewLine
}

function Add-LoadActorLine {
    param(
        [string]$DefaultLua,
        [string]$ActorFile
    )
    $raw = [System.IO.File]::ReadAllText($DefaultLua)
    $nl = Get-FileNewline -Text $raw

    # already injected? (any existing LoadActor for this file -> leave alone)
    if ($raw -match ('LoadActor\(\s*"' + [regex]::Escape($ActorFile) + '"\s*\)')) {
        Write-Step SKIP "already injected in $DefaultLua"
        return $true
    }

    # locate the LAST standalone `return t` line (allowing leading whitespace / trailing ';')
    $ms = [regex]::Matches($raw, '(?m)^[ \t]*return t[ \t]*;?[ \t]*\r?$')
    if ($ms.Count -eq 0) {
        Write-Step FAIL "No standalone 'return t' line found in $DefaultLua - please install manually"
        return $false
    }
    $m = $ms[$ms.Count - 1]
    $indent = [regex]::Match($m.Value, '^[ \t]*').Value

    # backup once (a theme update wipes it; that is fine, uninstall removes the line)
    $bak = $DefaultLua + '.mma-backup'
    if (-not (Test-Path -LiteralPath $bak)) {
        Copy-Item -LiteralPath $DefaultLua -Destination $bak -Force
    }

    $insert = $indent + 't[#t + 1] = LoadActor("' + $ActorFile + '")' + $nl
    $new = $raw.Substring(0, $m.Index) + $insert + $raw.Substring($m.Index)
    [System.IO.File]::WriteAllText($DefaultLua, $new, (New-Object System.Text.UTF8Encoding($false)))
    Write-Step OK ("injected LoadActor({0}) before 'return t' in {1}" -f $ActorFile, $DefaultLua)
    return $true
}

function Remove-LoadActorLine {
    param(
        [string]$DefaultLua,
        [string]$ActorFile
    )
    if (-not (Test-Path -LiteralPath $DefaultLua)) {
        Write-Step SKIP "no default.lua at $DefaultLua"
        return $true
    }
    $raw = [System.IO.File]::ReadAllText($DefaultLua)
    $pattern = '(?m)^[ \t]*t\[#t[ \t]*\+[ \t]*1\][ \t]*=[ \t]*LoadActor\(\s*"' +
        [regex]::Escape($ActorFile) + '"\s*\)[ \t]*\r?\n?'
    $new = [regex]::Replace($raw, $pattern, '')
    if ($new -eq $raw) {
        Write-Step SKIP "no LoadActor($ActorFile) line found in $DefaultLua"
        return $true
    }
    [System.IO.File]::WriteAllText($DefaultLua, $new, (New-Object System.Text.UTF8Encoding($false)))
    Write-Step OK ("removed LoadActor({0}) line from {1}" -f $ActorFile, $DefaultLua)
    return $true
}

# ---------------------------------------------------------------------------
# shell config (mma-shell-config.json next to the plugin root)
# ---------------------------------------------------------------------------

function Get-ShellConfigPath {
    if ($ConfigPath) { return $ConfigPath }
    $root = Split-Path $PSScriptRoot -Parent   # bridges\.. == plugin root (exe side)
    return (Join-Path $root 'mma-shell-config.json')
}

function Set-ShellConfigValue {
    param([string]$Key, [string]$Value)
    $path = Get-ShellConfigPath
    $data = [ordered]@{}
    if (Test-Path -LiteralPath $path) {
        try {
            $obj = Get-Content -LiteralPath $path -Raw | ConvertFrom-Json
            foreach ($prop in $obj.PSObject.Properties) { $data[$prop.Name] = $prop.Value }
        } catch {
            Write-Step INFO "mma-shell-config.json unreadable, regenerating: $path"
        }
    } else {
        Write-Step INFO "creating mma-shell-config.json: $path"
    }
    # skeleton defaults (match the shell's ensure_shell_config)
    if (-not $data.Contains('gameClient')) { $data['gameClient'] = 'Auto' }
    if (-not $data.Contains('etternaRoot')) { $data['etternaRoot'] = '' }
    if (-not $data.Contains('malodyRoot')) { $data['malodyRoot'] = '' }
    if (-not $data.Contains('hotkeys')) {
        $data['hotkeys'] = [ordered]@{ topmost = 'Ctrl+Shift+T'; clickThrough = 'Ctrl+Shift+C'; close = 'Ctrl+Q' }
    }
    if (-not $data.Contains('logLevel')) { $data['logLevel'] = 'info' }

    # forward slashes: the shell's normalize_path handles / and \ mixed input
    $data[$Key] = $Value.Replace('\', '/')
    $json = $data | ConvertTo-Json -Depth 10
    [System.IO.File]::WriteAllText($path, $json, (New-Object System.Text.UTF8Encoding($false)))
    Write-Step OK ("wrote {0} = {1} to {2}" -f $Key, $data[$Key], $path)
}

function Clear-ShellConfigValue {
    param([string]$Key)
    $path = Get-ShellConfigPath
    if (-not (Test-Path -LiteralPath $path)) {
        Write-Step SKIP "no mma-shell-config.json at $path"
        return
    }
    $data = [ordered]@{}
    try {
        $obj = Get-Content -LiteralPath $path -Raw | ConvertFrom-Json
        foreach ($prop in $obj.PSObject.Properties) { $data[$prop.Name] = $prop.Value }
    } catch {
        Write-Step FAIL "could not read $path"
        return
    }
    if (-not $data.Contains($Key)) {
        Write-Step SKIP "$Key not present in $path"
        return
    }
    $data[$Key] = ''
    $json = $data | ConvertTo-Json -Depth 10
    [System.IO.File]::WriteAllText($path, $json, (New-Object System.Text.UTF8Encoding($false)))
    Write-Step OK ("cleared {0} in {1}" -f $Key, $path)
}

# ---------------------------------------------------------------------------
# Etterna
# ---------------------------------------------------------------------------

function Install-EtternaBridge {
    Write-Host ''
    Write-Host '--- Installing Etterna bridge ---' -ForegroundColor Cyan
    $cands = Get-EtternaCandidates
    $root = Select-GameRoot -Game 'Etterna' -Candidates $cands -ForceRoot $Root
    if (-not $root) { return }
    $theme = Select-Theme -EtternaRoot $root -ForcedTheme $Theme
    if (-not $theme) { return }
    Write-Step INFO ("Etterna root: {0}\Themes\{1}" -f $root, $theme)

    # source bridge files live next to this script: bridges\etterna\...
    $src = Join-Path $PSScriptRoot 'etterna'
    if (-not (Test-Path (Join-Path $src 'mma_bridge.lua')) -or
        -not (Test-Path (Join-Path $src 'mma_gameplay.lua'))) {
        Write-Step FAIL "bridge sources not found next to this script: $src (expected mma_bridge.lua / mma_gameplay.lua)"
        return
    }

    $targets = @(
        @{ Screen = 'ScreenSelectMusic decorations'; File = 'mma_bridge.lua' },
        @{ Screen = 'ScreenGameplay overlay';        File = 'mma_gameplay.lua' }
    )
    $anyOk = $false
    foreach ($t in $targets) {
        $screenDir = Join-Path $root ("Themes\{0}\BGAnimations\{1}" -f $theme, $t.Screen)
        $defaultLua = Join-Path $screenDir 'default.lua'
        if (-not (Test-Path -LiteralPath $screenDir -PathType Container)) {
            Write-Step FAIL ("screen directory missing: {0} (theme structure incomplete?)" -f $screenDir)
            continue
        }
        if (-not (Test-Path -LiteralPath $defaultLua)) {
            Write-Step FAIL ("default.lua missing: {0} - cannot inject safely, please install manually" -f $defaultLua)
            continue
        }
        # copy the bridge file (overwrite on re-run = re-install after theme update)
        Copy-Item -LiteralPath (Join-Path $src $t.File) -Destination (Join-Path $screenDir $t.File) -Force
        Write-Step OK ("copied {0} -> {1}" -f $t.File, $screenDir)
        if (Add-LoadActorLine -DefaultLua $defaultLua -ActorFile $t.File) {
            $anyOk = $true
        }
    }

    if ($anyOk) {
        Set-ShellConfigValue -Key 'etternaRoot' -Value $root
        Write-Host ''
        Write-Step OK 'Etterna bridge installed.'
        Write-Host 'Reminder: a theme update wipes these files - re-run this installer to restore.' -ForegroundColor Yellow
    } else {
        Write-Host ''
        Write-Step FAIL 'Etterna bridge was NOT installed (see failures above).'
    }
}

function Uninstall-EtternaBridge {
    Write-Host ''
    Write-Host '--- Removing Etterna bridge ---' -ForegroundColor Cyan
    $cands = Get-EtternaCandidates
    $root = Select-GameRoot -Game 'Etterna' -Candidates $cands -ForceRoot $Root
    if (-not $root) { return }
    $themes = Get-Themes -EtternaRoot $root
    if ($themes.Count -eq 0) {
        Write-Step FAIL "no theme folders found under $root\Themes - nothing to remove"
        return
    }
    if ($Theme) {
        if ($themes -contains $Theme) {
            Write-Step INFO ("Removing from theme: {0}" -f $Theme)
            Uninstall-EtternaTheme -Root $root -Theme $Theme
        } else {
            Write-Step FAIL "Theme '$Theme' not found under $root\Themes"
        }
    } elseif ($themes -contains 'Rebirth') {
        Write-Step INFO ('Removing from theme: Rebirth')
        Uninstall-EtternaTheme -Root $root -Theme 'Rebirth'
    } else {
        $idx = Read-Option -Title 'Select the theme the bridge was installed into:' -Options $themes
        Uninstall-EtternaTheme -Root $root -Theme $themes[$idx]
    }
    if ((-not $Yes) -and (Confirm-YesNo 'Also clear etternaRoot in mma-shell-config.json?' -DefaultYes $false)) {
        Clear-ShellConfigValue -Key 'etternaRoot'
    }
    Write-Step OK 'Etterna bridge removed.'
}

function Uninstall-EtternaTheme {
    param([string]$Root, [string]$Theme)
    foreach ($screen in @('ScreenSelectMusic decorations', 'ScreenGameplay overlay')) {
        $screenDir = Join-Path $Root ("Themes\{0}\BGAnimations\{1}" -f $Theme, $screen)
        $defaultLua = Join-Path $screenDir 'default.lua'
        if (-not (Test-Path -LiteralPath $screenDir -PathType Container)) {
            Write-Step SKIP "screen directory missing: $screenDir"
            continue
        }
        foreach ($f in @('mma_bridge.lua', 'mma_gameplay.lua')) {
            $target = Join-Path $screenDir $f
            if (Test-Path -LiteralPath $target) {
                Remove-Item -LiteralPath $target -Force
                Write-Step OK "deleted $target"
            } else {
                Write-Step SKIP "not present: $target"
            }
            Remove-LoadActorLine -DefaultLua $defaultLua -ActorFile $f | Out-Null
        }
        # drop the backup file we created (if any)
        $bak = $defaultLua + '.mma-backup'
        if (Test-Path -LiteralPath $bak) {
            Remove-Item -LiteralPath $bak -Force
            Write-Step OK "deleted backup $bak"
        }
    }
}

# ---------------------------------------------------------------------------
# Malody V
# ---------------------------------------------------------------------------

function Install-MalodyBridge {
    Write-Host ''
    Write-Host '--- Installing Malody V bridge ---' -ForegroundColor Cyan
    $cands = Get-MalodyCandidates
    $root = Select-GameRoot -Game 'Malody' -Candidates $cands -ForceRoot $Root
    if (-not $root) { return }

    $src = Join-Path $PSScriptRoot 'malody\mma_editor.lua'
    if (-not (Test-Path -LiteralPath $src)) {
        Write-Step FAIL "bridge source not found next to this script: $src (expected mma_editor.lua)"
        return
    }
    # Malody V plugin folder (lowercase 'editor' is the real one; NTFS is case-insensitive anyway)
    $editor = Join-Path $root 'editor'
    if (-not (Test-Path -LiteralPath $editor -PathType Container)) {
        New-Item -ItemType Directory -Path $editor -Force | Out-Null
        Write-Step OK "created $editor"
    }
    Copy-Item -LiteralPath $src -Destination (Join-Path $editor 'mma_editor.lua') -Force
    Write-Step OK "copied mma_editor.lua -> $editor"

    Set-ShellConfigValue -Key 'malodyRoot' -Value $root
    Write-Host ''
    Write-Step OK 'Malody V bridge installed.'
    Write-Host 'Use it from the Malody editor: More menu -> MMA Analyze.' -ForegroundColor Gray
}

function Uninstall-MalodyBridge {
    Write-Host ''
    Write-Host '--- Removing Malody V bridge ---' -ForegroundColor Cyan
    $cands = Get-MalodyCandidates
    $root = Select-GameRoot -Game 'Malody' -Candidates $cands -ForceRoot $Root
    if (-not $root) { return }

    $editor = Join-Path $root 'editor'
    $target = Join-Path $editor 'mma_editor.lua'
    if (Test-Path -LiteralPath $target) {
        Remove-Item -LiteralPath $target -Force
        Write-Step OK "deleted $target"
    } else {
        Write-Step SKIP "not present: $target"
    }
    if ((-not $Yes) -and (Confirm-YesNo 'Also clear malodyRoot in mma-shell-config.json?' -DefaultYes $false)) {
        Clear-ShellConfigValue -Key 'malodyRoot'
    }
    Write-Step OK 'Malody V bridge removed.'
}

# ---------------------------------------------------------------------------
# main flow
# ---------------------------------------------------------------------------

function Invoke-GameFlow {
    param([string]$G, [bool]$Remove)
    if ($Remove) {
        if ($G -eq 'Etterna') { Uninstall-EtternaBridge } else { Uninstall-MalodyBridge }
    } else {
        if ($G -eq 'Etterna') { Install-EtternaBridge } else { Install-MalodyBridge }
    }
}

Show-Banner

if ($Game) {
    Invoke-GameFlow -G $Game -Remove ([bool]$Uninstall)
} else {
    while ($true) {
        $mode = Read-Option -Title 'What would you like to do?' -Options @('Install bridges', 'Uninstall bridges', 'Exit')
        if ($mode -eq 2) { break }
        $removeMode = ($mode -eq 1)
        $gameItem = Read-Option -Title $(if ($removeMode) { 'Remove bridge for:' } else { 'Install bridge for:' }) `
            -Options @('Etterna', 'Malody V') -Extra 'Back to menu'
        if ($gameItem -eq 2) { continue }
        Invoke-GameFlow -G $(if ($gameItem -eq 0) { 'Etterna' } else { 'Malody' }) -Remove $removeMode
        if (-not $Yes) {
            if (-not (Confirm-YesNo 'Continue?' -DefaultYes $false)) { break }
        }
    }
}

Write-Host ''
Write-Host 'Done.' -ForegroundColor Green