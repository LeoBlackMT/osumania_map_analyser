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
    the registry + steamapps/libraryfolders.vdf, then common install paths).
    If detection finds nothing, you can pick a folder with a file browser,
    type a path (/, \ and \\ are all accepted; quoting and trailing slashes
    are tolerated), or read a short how-to-find-the-path hint.

    The Etterna theme list is pre-checked: only themes whose structure is
    complete (both ScreenSelectMusic decorations\default.lua and
    ScreenGameplay overlay\default.lua exist) are offered for installing into;
    broken themes like _fallback are listed with the reason they were skipped.
    Rebirth is the default when present; installing to ALL themes at once is
    intentionally NOT offered.

    The installer never touches other scripts' LoadActor lines (e.g. DanOverlay,
    elements, titlesplash): it only adds its own line idempotently and removes
    only its own line on uninstall. Files are written as UTF-8 without BOM so
    LuaJIT (Etterna) never chokes on a BOM.

.PARAMETER Game
    'Etterna' or 'Malody' to skip the game picker. Omit for the menu.

.PARAMETER Uninstall
    Remove instead of install.

.PARAMETER Chinese
    Output interface text in Chinese (script file must be UTF-8 with BOM for
    Windows PowerShell 5.1 to parse the strings correctly; install-bridge-zh.bat
    already handles this).

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
    English by default; -Chinese switches the user-facing text to Chinese.
#>
[CmdletBinding()]
param(
    [ValidateSet('Etterna', 'Malody')]
    [string]$Game,
    [switch]$Uninstall,
    [switch]$Chinese,
    [switch]$Yes,
    [string]$Root,
    [string]$Theme,
    [string]$ConfigPath
)

$ErrorActionPreference = 'Stop'

# ---------------------------------------------------------------------------
# localization
# ---------------------------------------------------------------------------

$script:L = @{
    # banner / menus
    AppTitle        = 'ManiaMapAnalyser - Bridge Installer v1.0.0'
    Lede1           = 'Installs / removes data bridges for mma-shell:'
    Lede1Zh         = '安装 / 卸载 mma-shell 数据桥：'
    LedeEtterna     = '   - Etterna theme bridge (mma_bridge / mma_gameplay)'
    LedeMalody      = '   - Malody V editor plugin (mma_editor)'
    MenuAsk         = 'What would you like to do?'
    MenuAskZh       = '你想做什么？'
    MenuInstall     = 'Install bridges'
    MenuInstallZh   = '安装桥文件'
    MenuUninstall   = 'Uninstall bridges'
    MenuUninstallZh = '卸载桥文件'
    MenuExit        = 'Exit'
    MenuExitZh      = '退出'
    ChooseGameIn    = 'Install bridge for:'
    ChooseGameInZh  = '为以下游戏安装桥：'
    ChooseGameRm    = 'Remove bridge for:'
    ChooseGameRmZh  = '为以下游戏卸载桥：'
    BackToMenu      = 'Back to menu'
    BackToMenuZh    = '返回菜单'
    EnterChoice     = 'Enter choice'
    EnterChoiceZh   = '请输入选项'
    InvalidChoice   = 'Invalid choice, try again.'
    InvalidChoiceZh = '无效选项，请重试。'
    AnswerYN        = 'Please answer y or n.'
    AnswerYNZh      = '请输入 y 或 n。'
    ContinueAsk     = 'Continue?'
    ContinueAskZh   = '继续？'
    Done            = 'Done.'
    DoneZh          = '完成。'

    # detection
    HeaderEtterna   = '--- Installing Etterna bridge ---'
    HeaderEtternaZh = '--- 正在安装 Etterna 桥文件 ---'
    HeaderMalody    = '--- Installing Malody V bridge ---'
    HeaderMalodyZh  = '--- 正在安装 Malody V 桥文件 ---'
    RmEtterna       = '--- Removing Etterna bridge ---'
    RmEtternaZh     = '--- 正在卸载 Etterna 桥文件 ---'
    RmMalody        = '--- Removing Malody V bridge ---'
    RmMalodyZh      = '--- 正在卸载 Malody V 桥文件 ---'
    UseDetected     = "Use detected {0} folder:`n  {1}"
    UseDetectedZh   = "使用检测到的 {0} 目录：`n  {1}"
    MultipleDet     = 'Multiple {0} installs detected, pick one:'
    MultipleDetZh   = '检测到多个 {0} 安装位置，请选择：'
    EnterManual     = 'Enter manually...'
    EnterManualZh   = '手动输入……'
    AutoPick        = 'Auto mode: using the first detected {0} folder: {1}'
    AutoPickZh      = '自动模式：使用第一个检测到的 {0} 目录：{1}'
    NoAutoDetect    = 'No {0} install auto-detected'
    NoAutoDetectZh  = '未自动检测到 {0} 安装位置'
    NeedRoot        = 'No {0} detected and interactive input is disabled (-Yes). Please pass -Root <path>.'
    NeedRootZh      = '未检测到 {0} 且已禁用交互输入（-Yes）。请通过 -Root <path> 指定路径。'
    ProvideFolder   = 'How would you like to provide the {0} folder?'
    ProvideFolderZh = '如何提供 {0} 目录？'
    OptBrowse       = 'Browse for the folder...'
    OptBrowseZh     = '浏览选择目录……'
    OptType         = 'Type the path manually'
    OptTypeZh       = '手动输入路径'
    OptHelp         = 'How do I find the path?'
    OptHelpZh       = '如何找到游戏路径？'
    OptAbort        = "Abort (don't install)"
    OptAbortZh      = '取消（不安装）'
    BrowseFail      = 'folder browser unavailable, please type the path instead'
    BrowseFailZh    = '文件夹浏览不可用，请改为手动输入路径'
    TypePathPrompt  = 'Enter the full {0} folder path'
    TypePathPromptZh = '请输入完整的 {0} 目录路径'
    InvalidRoot     = 'Not valid as a {0} folder (path check failed): {1}'
    InvalidRootZh   = '不是有效的 {0} 目录（路径校验失败）：{1}'
    HelpEtterna     = @'
How to find your Etterna folder:
  1. Right-click the Etterna shortcut (desktop / Start menu), choose
     'Open file location'.
  2. In the folder that opens (or its parent if it contains the .exe),
     click the address bar and copy the full path, e.g. D:\Games\Etterna.
The folder we need is the one that directly contains 'Save' and 'Themes'.
'@
    HelpEtternaZh   = @'
如何找到您的 Etterna 目录：
  1. 右键点击 Etterna 快捷方式（桌面 / 开始菜单），选择「打开文件所在位置」。
  2. 在打开的文件夹（或其上一级包含 .exe 的文件夹）中，点击地址栏复制完整路径，例如 D:\Games\Etterna。
我们需要的是直接包含 Save 和 Themes 两个文件夹的那个目录。
'@
    HelpMalody      = @'
How to find your Malody V folder:
  1. Steam users: usually D:\Steam\steamapps\common\MalodyV
     (the Steam library may be on another drive - look for steamapps\common\MalodyV).
  2. Otherwise: right-click the Malody V shortcut, choose 'Open file location'.
We need the folder that directly contains 'chart' and 'skin'.
'@
    HelpMalodyZh    = @'
如何找到您的 Malody V 目录：
  1. Steam 用户：通常在 D:\Steam\steamapps\common\MalodyV
     （Steam 库可能在其它盘符——找到 steamapps\common\MalodyV 即可）。
  2. 其他方式：右键 Malody V 快捷方式，选择「打开文件所在位置」。
我们需要的是直接包含 chart 和 skin 两个文件夹的那个目录。
'@

    # themes
    ThemeConfirm    = "Use theme '{0}'? (installable: {1})"
    ThemeConfirmZh  = "使用主题 '{0}'？（可安装：{1}）"
    ThemePick       = 'Select the Etterna theme to install into:'
    ThemePickZh     = '选择要安装到的 Etterna 主题：'
    ThemePickRm     = 'Select the theme the bridge was installed into:'
    ThemePickRmZh   = '选择桥文件所安装到的主题：'
    ThemeSkipped    = "skipped theme '{0}': {1}"
    ThemeSkippedZh  = "已跳过主题 '{0}'：{1}"
    ReasonNoSel     = 'missing ScreenSelectMusic decorations\default.lua'
    ReasonNoSelZh   = '缺少 ScreenSelectMusic decorations\default.lua'
    ReasonNoGp      = 'missing ScreenGameplay overlay\default.lua'
    ReasonNoGpZh    = '缺少 ScreenGameplay overlay\default.lua'
    NoThemes        = 'No installable theme folders found under {0}\Themes'
    NoThemesZh      = '在 {0}\Themes 下未找到可安装的主题目录'
    NoThemesRm      = 'no theme folders found under {0}\Themes - nothing to remove'
    NoThemesRmZh    = '在 {0}\Themes 下未找到主题目录 - 无需卸载'
    ThemeNotFound   = "Theme '{0}' not found under {1}\Themes"
    ThemeNotFoundZh = "在 {1}\Themes 下未找到主题 '{0}'"
    EtternaRootInfo = 'Etterna root: {0}\Themes\{1}'
    EtternaRootInfoZh = 'Etterna 目录：{0}\Themes\{1}'
    RmThemeFrom     = 'Removing from theme: {0}'
    RmThemeFromZh   = '正在从主题卸载：{0}'
    BridgeSrcNo     = 'bridge sources not found next to this script: {0} (expected mma_bridge.lua / mma_gameplay.lua)'
    BridgeSrcNoZh   = '未在脚本旁找到桥文件源：{0}（应为 mma_bridge.lua / mma_gameplay.lua）'
    ScreenDirNo     = 'screen directory missing: {0} (theme structure incomplete?)'
    ScreenDirNoZh   = '缺少屏幕目录：{0}（主题结构不完整？）'
    LuaMissing      = 'default.lua missing: {0} - cannot inject safely, please install manually'
    LuaMissingZh    = '缺少 default.lua：{0} - 无法安全注入，请手动安装'
    Copied          = 'copied {0} -> {1}'
    CopiedZh        = '已复制 {0} -> {1}'
    Injected        = "injected LoadActor({0}) before 'return t' in {1}"
    InjectedZh      = "已在 {1} 的 'return t' 前注入 LoadActor({0})"
    AlreadyInj      = 'already injected in {0}'
    AlreadyInjZh    = '已注入过：{0}'
    NoReturnT       = "No standalone 'return t' line found in {0} - please install manually"
    NoReturnTZh     = "在 {0} 中未找到独立的 'return t' 行 - 请手动安装"
    InjFail         = 'injection failed for {0}'
    InjFailZh       = '注入失败：{0}'
    RemovedLine     = "removed LoadActor({0}) line from {1}"
    RemovedLineZh   = '已从 {1} 移除 LoadActor({0}) 行'
    NoLineFound     = "no LoadActor({0}) line found in {1}"
    NoLineFoundZh   = "在 {1} 中未找到 LoadActor({0}) 行"
    Deleted         = 'deleted {0}'
    DeletedZh       = '已删除 {0}'
    NotPresent      = 'not present: {0}'
    NotPresentZh    = '不存在：{0}'
    NoDefaultLua    = 'no default.lua at {0}'
    NoDefaultLuaZh  = '{0} 处没有 default.lua'
    BackupDel       = 'deleted backup {0}'
    BackupDelZh     = '已删除备份 {0}'

    # malody
    CreatedDir      = 'created {0}'
    CreatedDirZh    = '已创建 {0}'
    MalodySrcNo     = 'bridge source not found next to this script: {0} (expected mma_editor.lua)'
    MalodySrcNoZh   = '未在脚本旁找到桥文件源：{0}（应为 mma_editor.lua）'
    MalodyTip       = 'Use it from the Malody editor: More menu -> MMA Analyze.'
    MalodyTipZh     = '使用方式：打开 Malody 编辑器 → 菜单 → MMA Analyze。'

    # config
    CfgCreate       = 'creating mma-shell-config.json: {0}'
    CfgCreateZh     = '正在创建 mma-shell-config.json：{0}'
    CfgUnread       = 'mma-shell-config.json unreadable, regenerating: {0}'
    CfgUnreadZh     = 'mma-shell-config.json 无法读取，正在重新生成：{0}'
    CfgWrote        = 'wrote {0} = {1} to {2}'
    CfgWroteZh      = '已写入 {0} = {1} 到 {2}'
    CfgCleared      = 'cleared {0} in {1}'
    CfgClearedZh    = '已清空 {1} 中的 {0}'
    CfgNoFile       = 'no mma-shell-config.json at {0}'
    CfgNoFileZh     = '{0} 处没有 mma-shell-config.json'
    CfgNoKey        = '{0} not present in {1}'
    CfgNoKeyZh      = '{1} 中不存在 {0}'
    CfgReadFail     = 'could not read {0}'
    CfgReadFailZh   = '无法读取 {0}'
    ConfirmClearE   = 'Also clear etternaRoot in mma-shell-config.json?'
    ConfirmClearEZh = '是否同时清空 mma-shell-config.json 中的 etternaRoot？'
    ConfirmClearM   = 'Also clear malodyRoot in mma-shell-config.json?'
    ConfirmClearMZh = '是否同时清空 mma-shell-config.json 中的 malodyRoot？'

    # results
    EtternaOk       = 'Etterna bridge installed.'
    EtternaOkZh     = 'Etterna 桥文件安装完成。'
    EtternaNot      = 'Etterna bridge was NOT installed (see failures above).'
    EtternaNotZh    = 'Etterna 桥文件未安装成功（请查看上方错误）。'
    EtternaGone     = 'Etterna bridge removed.'
    EtternaGoneZh   = 'Etterna 桥文件已卸载。'
    MalodyOk        = 'Malody V bridge installed.'
    MalodyOkZh      = 'Malody V 桥文件安装完成。'
    MalodyGone      = 'Malody V bridge removed.'
    MalodyGoneZh    = 'Malody V 桥文件已卸载。'
    RemindTheme     = 'Reminder: a theme update wipes these files - re-run this installer to restore.'
    RemindThemeZh   = '提示：主题更新会删除这些文件 - 重新运行本安装器即可恢复。'
}

# When -Chinese: flip all *Zh keys over their English base names.
if ($Chinese) {
    $map = @{}
    foreach ($k in $script:L.Keys) {
        if ($k.EndsWith('Zh')) {
            $base = $k.Substring(0, $k.Length - 2)
            $map[$base] = $script:L[$k]
        }
    }
    foreach ($k in $map.Keys) { $script:L[$k] = $map[$k] }
    try { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8 } catch { }
}

function Get-Text {
    param([string]$Key)
    return $script:L[$Key]
}

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
        'WARN' { 'Yellow' }
        default { 'Gray' }
    }
    Write-Host ("[{0}] {1}" -f $Status, $Msg) -ForegroundColor $color
}

function Show-Banner {
    Write-Host ''
    Write-Host ('==================================================') -ForegroundColor Cyan
    Write-Host (' {0}' -f (Get-Text 'AppTitle')) -ForegroundColor Cyan
    Write-Host ('==================================================') -ForegroundColor Cyan
    Write-Host (Get-Text 'Lede1') -ForegroundColor Gray
    Write-Host (Get-Text 'LedeEtterna') -ForegroundColor Gray
    Write-Host (Get-Text 'LedeMalody') -ForegroundColor Gray
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
        $choice = Read-Host (Get-Text 'EnterChoice')
        $n = 0
        if ([int]::TryParse($choice, [ref]$n)) {
            if ($n -ge 1 -and $n -le $Options.Count) { return ($n - 1) }
            if ($Extra -and $n -eq $Options.Count + 1) { return $n - 1 }
        }
        Write-Host (Get-Text 'InvalidChoice') -ForegroundColor Yellow
    }
}

function Confirm-YesNo {
    param(
        [string]$Prompt,
        [bool]$DefaultYes = $true
    )
    $hint = if ($DefaultYes) { 'Y/n' } else { 'y/N' }
    while ($true) {
        $answer = Read-Host ("{0} [{1}]" -f $Prompt, $hint)
        if ([string]::IsNullOrWhiteSpace($answer)) { return $DefaultYes }
        if ($answer -match '^(y|yes)$') { return $true }
        if ($answer -match '^(n|no)$') { return $false }
        Write-Host (Get-Text 'AnswerYN') -ForegroundColor Yellow
    }
}

function Format-PathInput {
    param([string]$Path)
    if ([string]::IsNullOrWhiteSpace($Path)) { return '' }
    $p = $Path.Trim()
    # strip wrapping quotes (user may have pasted "D:\..." or 'D:\...')
    $p = $p.Trim([char[]]@('"', "'"))
    # fold double backslashes (user may type D:\\Games\\Etterna thinking of JSON escaping)
    $p = $p -replace '\\\\', '\'
    # unify separators to backslash
    $p = $p -replace '/', '\'
    # trim trailing separators
    $p = $p.TrimEnd([char[]]@('\', '/'))
    return $p
}

function Select-FolderBrowser {
    param([string]$Description)
    try {
        Add-Type -AssemblyName System.Windows.Forms
        $dlg = New-Object System.Windows.Forms.FolderBrowserDialog
        $dlg.Description = $Description
        $dlg.ShowNewFolderButton = $false
        if ($dlg.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {
            return $dlg.SelectedPath
        }
    } catch {
        Write-Step WARN (Get-Text 'BrowseFail')
    }
    return $null
}

function Read-CustomPath {
    param([string]$What, [scriptblock]$Validator)
    while ($true) {
        $pick = Read-Option -Title ((Get-Text 'ProvideFolder') -f $What) `
            -Options @(
                (Get-Text 'OptBrowse'),
                (Get-Text 'OptType'),
                (Get-Text 'OptHelp')
            ) -Extra (Get-Text 'OptAbort')
        if ($pick -eq 3) { return $null }
        if ($pick -eq 2) {
            $help = if ($What -eq 'Etterna') { Get-Text 'HelpEtterna' } else { Get-Text 'HelpMalody' }
            Write-Host ''
            Write-Host $help -ForegroundColor Gray
            continue
        }
        if ($pick -eq 0) {
            $chosen = Select-FolderBrowser -Description $What
            if (-not $chosen) { continue }
            $chosen = Format-PathInput -Path $chosen
            if (& $Validator $chosen) { return $chosen }
            Write-Step FAIL ((Get-Text 'InvalidRoot') -f $What, $chosen)
            continue
        }
        # type manually
        $p = Read-Host ((Get-Text 'TypePathPrompt') -f $What)
        if ([string]::IsNullOrWhiteSpace($p)) { continue }
        $p = Format-PathInput -Path $p
        if ($p -and (& $Validator $p)) { return $p }
        Write-Step FAIL ((Get-Text 'InvalidRoot') -f $What, $p)
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
        if ($p) { $p = Format-PathInput -Path $p }
        if ($p -and (Test-EtternaRoot $p) -and -not $cands.Contains($p)) { $cands.Add($p) }
    }
    # running process
    $p = Get-RunningProcessRoot -Names 'Etterna'
    if ($p) { & $add $p }
    # env override
    if ($env:MMA_ETTERNA_ROOT) { & $add $env:MMA_ETTERNA_ROOT }
    # common install paths (Etterna is NOT a Steam app)
    foreach ($c in @('D:/Games/Etterna', 'C:/Games/Etterna', 'D:/Etterna', 'C:/Etterna')) {
        & $add $c
    }
    return @($cands)
}

function Get-MalodyCandidates {
    $cands = New-Object 'System.Collections.Generic.List[string]'
    $add = {
        param($p)
        if ($p) { $p = Format-PathInput -Path $p }
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
        $norm = Format-PathInput -Path $ForceRoot
        if ($norm -and (& $validator $norm)) { return $norm }
        Write-Step FAIL ((Get-Text 'InvalidRoot') -f $Game, $ForceRoot)
        return $null
    }
    if ($Candidates.Count -eq 0) {
        Write-Step INFO ((Get-Text 'NoAutoDetect') -f $Game)
        if ($Yes) {
            Write-Host ((Get-Text 'NeedRoot') -f $Game) -ForegroundColor Yellow
            return $null
        }
        return Read-CustomPath -What $Game -Validator $validator
    }
    if ($Yes) {
        # auto mode: take the highest-priority candidate
        if ($Candidates.Count -gt 1) {
            Write-Step INFO ((Get-Text 'AutoPick') -f $Game, $Candidates[0])
        }
        return $Candidates[0]
    }
    if ($Candidates.Count -eq 1) {
        if (Confirm-YesNo ((Get-Text 'UseDetected') -f $Game, $Candidates[0])) {
            return $Candidates[0]
        }
        return Read-CustomPath -What $Game -Validator $validator
    }
    $idx = Read-Option -Title ((Get-Text 'MultipleDet') -f $Game) -Options $Candidates -Extra (Get-Text 'EnterManual')
    if ($idx -lt $Candidates.Count) { return $Candidates[$idx] }
    return Read-CustomPath -What $Game -Validator $validator
}

# ---------------------------------------------------------------------------
# theme support pre-check: only structurally complete themes are installable
# ---------------------------------------------------------------------------

function Get-ThemeSupport {
    param([string]$EtternaRoot)
    $themes = @()
    $dir = Join-Path $EtternaRoot 'Themes'
    if (Test-Path $dir -PathType Container) {
        $themes = @(Get-ChildItem -LiteralPath $dir -Directory -ErrorAction SilentlyContinue |
            Select-Object -ExpandProperty Name)
    }
    $out = @()
    foreach ($t in $themes) {
        $selDir = Join-Path $EtternaRoot ("Themes\{0}\BGAnimations\ScreenSelectMusic decorations" -f $t)
        $gpDir = Join-Path $EtternaRoot ("Themes\{0}\BGAnimations\ScreenGameplay overlay" -f $t)
        $selLua = Join-Path $selDir 'default.lua'
        $gpLua = Join-Path $gpDir 'default.lua'
        $selOk = Test-Path -LiteralPath $selLua
        $gpOk = Test-Path -LiteralPath $gpLua
        $out += [pscustomobject]@{
            Name          = $t
            SelectDir     = $selDir
            GameplayDir   = $gpDir
            SelectOk      = $selOk
            GameplayOk    = $gpOk
            Installable   = ($selOk -and $gpOk)
            SelectDefault = $selLua
            GameplayDefault = $gpLua
        }
    }
    return $out
}

function Get-InstallableThemes {
    param([string]$EtternaRoot)
    $all = Get-ThemeSupport -EtternaRoot $EtternaRoot
    # report skipped themes (why they are not offered)
    foreach ($t in $all) {
        if (-not $t.Installable) {
            $reason = if (-not $t.SelectOk) { (Get-Text 'ReasonNoSel') } else { (Get-Text 'ReasonNoGp') }
            Write-Step SKIP ((Get-Text 'ThemeSkipped') -f $t.Name, $reason)
        }
    }
    return @($all | Where-Object { $_.Installable })
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
        Write-Step SKIP ((Get-Text 'AlreadyInj') -f $DefaultLua)
        return $true
    }

    # locate the LAST standalone `return t` line (allowing leading whitespace / trailing ';')
    $ms = [regex]::Matches($raw, '(?m)^[ \t]*return t[ \t]*;?[ \t]*\r?$')
    if ($ms.Count -eq 0) {
        Write-Step FAIL ((Get-Text 'NoReturnT') -f $DefaultLua)
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
    Write-Step OK ((Get-Text 'Injected') -f $ActorFile, $DefaultLua)
    return $true
}

function Remove-LoadActorLine {
    param(
        [string]$DefaultLua,
        [string]$ActorFile
    )
    if (-not (Test-Path -LiteralPath $DefaultLua)) {
        Write-Step SKIP ((Get-Text 'NoDefaultLua') -f $DefaultLua)
        return $true
    }
    $raw = [System.IO.File]::ReadAllText($DefaultLua)
    $pattern = '(?m)^[ \t]*t\[#t[ \t]*\+[ \t]*1\][ \t]*=[ \t]*LoadActor\(\s*"' +
        [regex]::Escape($ActorFile) + '"\s*\)[ \t]*\r?\n?'
    $new = [regex]::Replace($raw, $pattern, '')
    if ($new -eq $raw) {
        Write-Step SKIP ((Get-Text 'NoLineFound') -f $ActorFile, $DefaultLua)
        return $true
    }
    [System.IO.File]::WriteAllText($DefaultLua, $new, (New-Object System.Text.UTF8Encoding($false)))
    Write-Step OK ((Get-Text 'RemovedLine') -f $ActorFile, $DefaultLua)
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
            Write-Step INFO ((Get-Text 'CfgUnread') -f $path)
        }
    } else {
        Write-Step INFO ((Get-Text 'CfgCreate') -f $path)
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
    Write-Step OK ((Get-Text 'CfgWrote') -f $Key, $data[$Key], $path)
}

function Clear-ShellConfigValue {
    param([string]$Key)
    $path = Get-ShellConfigPath
    if (-not (Test-Path -LiteralPath $path)) {
        Write-Step SKIP ((Get-Text 'CfgNoFile') -f $path)
        return
    }
    $data = [ordered]@{}
    try {
        $obj = Get-Content -LiteralPath $path -Raw | ConvertFrom-Json
        foreach ($prop in $obj.PSObject.Properties) { $data[$prop.Name] = $prop.Value }
    } catch {
        Write-Step FAIL ((Get-Text 'CfgReadFail') -f $path)
        return
    }
    if (-not $data.Contains($Key)) {
        Write-Step SKIP ((Get-Text 'CfgNoKey') -f $Key, $path)
        return
    }
    $data[$Key] = ''
    $json = $data | ConvertTo-Json -Depth 10
    [System.IO.File]::WriteAllText($path, $json, (New-Object System.Text.UTF8Encoding($false)))
    Write-Step OK ((Get-Text 'CfgCleared') -f $Key, $path)
}

# ---------------------------------------------------------------------------
# Etterna
# ---------------------------------------------------------------------------

function Install-EtternaBridge {
    Write-Host ''
    Write-Host (Get-Text 'HeaderEtterna') -ForegroundColor Cyan
    $cands = Get-EtternaCandidates
    $root = Select-GameRoot -Game 'Etterna' -Candidates $cands -ForceRoot $Root
    if (-not $root) { return }

    # theme pre-check: only structurally complete themes are offered
    $installable = Get-InstallableThemes -EtternaRoot $root
    if ($installable.Count -eq 0) {
        Write-Step FAIL ((Get-Text 'NoThemes') -f $root)
        return
    }
    $theme = Select-Theme -EtternaRoot $root -Installable $installable -ForcedTheme $Theme
    if (-not $theme) { return }
    $info = (Get-ThemeSupport -EtternaRoot $root | Where-Object { $_.Name -eq $theme } | Select-Object -First 1)
    Write-Step INFO ((Get-Text 'EtternaRootInfo') -f $root, $theme)

    # source bridge files live next to this script: bridges\etterna\...
    $src = Join-Path $PSScriptRoot 'etterna'
    if (-not (Test-Path (Join-Path $src 'mma_bridge.lua')) -or
        -not (Test-Path (Join-Path $src 'mma_gameplay.lua'))) {
        Write-Step FAIL ((Get-Text 'BridgeSrcNo') -f $src)
        return
    }

    $targets = @(
        @{ Screen = 'ScreenSelectMusic decorations'; File = 'mma_bridge.lua'; Lua = $info.SelectDefault },
        @{ Screen = 'ScreenGameplay overlay';        File = 'mma_gameplay.lua'; Lua = $info.GameplayDefault }
    )
    $anyOk = $false
    foreach ($t in $targets) {
        $screenDir = Join-Path $root ("Themes\{0}\BGAnimations\{1}" -f $theme, $t.Screen)
        $defaultLua = $t.Lua
        if (-not (Test-Path -LiteralPath $screenDir -PathType Container)) {
            Write-Step FAIL ((Get-Text 'ScreenDirNo') -f $screenDir)
            continue
        }
        if (-not (Test-Path -LiteralPath $defaultLua)) {
            Write-Step FAIL ((Get-Text 'LuaMissing') -f $defaultLua)
            continue
        }
        # copy the bridge file (overwrite on re-run = re-install after theme update)
        Copy-Item -LiteralPath (Join-Path $src $t.File) -Destination (Join-Path $screenDir $t.File) -Force
        Write-Step OK ((Get-Text 'Copied') -f $t.File, $screenDir)
        if (Add-LoadActorLine -DefaultLua $defaultLua -ActorFile $t.File) {
            $anyOk = $true
        }
    }

    if ($anyOk) {
        Set-ShellConfigValue -Key 'etternaRoot' -Value $root
        Write-Host ''
        Write-Step OK (Get-Text 'EtternaOk')
        Write-Host (Get-Text 'RemindTheme') -ForegroundColor Yellow
    } else {
        Write-Host ''
        Write-Step FAIL (Get-Text 'EtternaNot')
    }
}

function Select-Theme {
    param(
        [string]$EtternaRoot,
        [object[]]$Installable,
        [string]$ForcedTheme
    )
    $names = @($Installable | Select-Object -ExpandProperty Name)
    if ($names.Count -eq 0) {
        return $null
    }
    if ($ForcedTheme) {
        if ($names -contains $ForcedTheme) { return $ForcedTheme }
        $which = Get-ThemeSupport -EtternaRoot $EtternaRoot | Where-Object { $_.Name -eq $ForcedTheme } | Select-Object -First 1
        if ($which) {
            # forced theme exists but is not structurally complete
            $reasonKey = if (-not $which.SelectOk) { 'ReasonNoSel' } else { 'ReasonNoGp' }
            Write-Step FAIL ((Get-Text 'ThemeNotFound') -f $ForcedTheme, $EtternaRoot) + ' (' + (Get-Text $reasonKey) + ')'
        } else {
            Write-Step FAIL ((Get-Text 'ThemeNotFound') -f $ForcedTheme, $EtternaRoot)
        }
        return $null
    }
    # default: Rebirth (does NOT offer installing to all themes at once)
    if ($names -contains 'Rebirth') {
        if ($Yes) { return 'Rebirth' }
        if (Confirm-YesNo ((Get-Text 'ThemeConfirm') -f 'Rebirth', ($names -join ', '))) {
            return 'Rebirth'
        }
    }
    $idx = Read-Option -Title (Get-Text 'ThemePick') -Options $names
    return $names[$idx]
}

function Test-ThemeHasBridge {
    param([pscustomobject]$Theme)
    foreach ($dir in @($Theme.SelectDir, $Theme.GameplayDir)) {
        foreach ($f in @('mma_bridge.lua', 'mma_gameplay.lua')) {
            if (Test-Path -LiteralPath (Join-Path $dir $f)) { return $true }
        }
    }
    foreach ($lua in @($Theme.SelectDefault, $Theme.GameplayDefault)) {
        if (Test-Path -LiteralPath $lua) {
            $raw = Get-Content -LiteralPath $lua -Raw -ErrorAction SilentlyContinue
            if ($raw -match 'LoadActor\(\s*"mma_(bridge|gameplay)\.lua"\s*\)') { return $true }
        }
    }
    return $false
}

function Uninstall-EtternaBridge {
    Write-Host ''
    Write-Host (Get-Text 'RmEtterna') -ForegroundColor Cyan
    $cands = Get-EtternaCandidates
    $root = Select-GameRoot -Game 'Etterna' -Candidates $cands -ForceRoot $Root
    if (-not $root) { return }

    # uninstall list = themes with bridge traces (files/injected lines), even if
    # their structure degraded since install; structurally complete ones too.
    $support = Get-ThemeSupport -EtternaRoot $root | Where-Object { $_.Installable -or (Test-ThemeHasBridge $_) }
    $themes = @($support | Select-Object -ExpandProperty Name)
    if ($themes.Count -eq 0) {
        Write-Step FAIL ((Get-Text 'NoThemesRm') -f $root)
        return
    }
    if ($Theme) {
        if ($themes -contains $Theme) {
            Write-Step INFO ((Get-Text 'RmThemeFrom') -f $Theme)
            Uninstall-EtternaTheme -Root $root -Theme $Theme
        } else {
            Write-Step FAIL ((Get-Text 'ThemeNotFound') -f $Theme, $root)
        }
    } elseif ($themes -contains 'Rebirth') {
        Write-Step INFO ((Get-Text 'RmThemeFrom') -f 'Rebirth')
        Uninstall-EtternaTheme -Root $root -Theme 'Rebirth'
    } else {
        $idx = Read-Option -Title (Get-Text 'ThemePickRm') -Options $themes
        Uninstall-EtternaTheme -Root $root -Theme $themes[$idx]
    }
    if ((-not $Yes) -and (Confirm-YesNo (Get-Text 'ConfirmClearE') -DefaultYes $false)) {
        Clear-ShellConfigValue -Key 'etternaRoot'
    }
    Write-Step OK (Get-Text 'EtternaGone')
}

function Uninstall-EtternaTheme {
    param([string]$Root, [string]$Theme)
    foreach ($screen in @('ScreenSelectMusic decorations', 'ScreenGameplay overlay')) {
        $screenDir = Join-Path $Root ("Themes\{0}\BGAnimations\{1}" -f $Theme, $screen)
        $defaultLua = Join-Path $screenDir 'default.lua'
        if (-not (Test-Path -LiteralPath $screenDir -PathType Container)) {
            Write-Step SKIP ((Get-Text 'ScreenDirNo') -f $screenDir)
            continue
        }
        foreach ($f in @('mma_bridge.lua', 'mma_gameplay.lua')) {
            $target = Join-Path $screenDir $f
            if (Test-Path -LiteralPath $target) {
                Remove-Item -LiteralPath $target -Force
                Write-Step OK ((Get-Text 'Deleted') -f $target)
            } else {
                Write-Step SKIP ((Get-Text 'NotPresent') -f $target)
            }
            Remove-LoadActorLine -DefaultLua $defaultLua -ActorFile $f | Out-Null
        }
        # drop the backup file we created (if any)
        $bak = $defaultLua + '.mma-backup'
        if (Test-Path -LiteralPath $bak) {
            Remove-Item -LiteralPath $bak -Force
            Write-Step OK ((Get-Text 'BackupDel') -f $bak)
        }
    }
}

# ---------------------------------------------------------------------------
# Malody V
# ---------------------------------------------------------------------------

function Install-MalodyBridge {
    Write-Host ''
    Write-Host (Get-Text 'HeaderMalody') -ForegroundColor Cyan
    $cands = Get-MalodyCandidates
    $root = Select-GameRoot -Game 'Malody' -Candidates $cands -ForceRoot $Root
    if (-not $root) { return }

    $src = Join-Path $PSScriptRoot 'malody\mma_editor.lua'
    if (-not (Test-Path -LiteralPath $src)) {
        Write-Step FAIL ((Get-Text 'MalodySrcNo') -f $src)
        return
    }
    # Malody V plugin folder (lowercase 'editor' is the real one; NTFS is case-insensitive anyway)
    $editor = Join-Path $root 'editor'
    if (-not (Test-Path -LiteralPath $editor -PathType Container)) {
        New-Item -ItemType Directory -Path $editor -Force | Out-Null
        Write-Step OK ((Get-Text 'CreatedDir') -f $editor)
    }
    Copy-Item -LiteralPath $src -Destination (Join-Path $editor 'mma_editor.lua') -Force
    Write-Step OK ((Get-Text 'Copied') -f 'mma_editor.lua', $editor)

    Set-ShellConfigValue -Key 'malodyRoot' -Value $root
    Write-Host ''
    Write-Step OK (Get-Text 'MalodyOk')
    Write-Host (Get-Text 'MalodyTip') -ForegroundColor Gray
}

function Uninstall-MalodyBridge {
    Write-Host ''
    Write-Host (Get-Text 'RmMalody') -ForegroundColor Cyan
    $cands = Get-MalodyCandidates
    $root = Select-GameRoot -Game 'Malody' -Candidates $cands -ForceRoot $Root
    if (-not $root) { return }

    $editor = Join-Path $root 'editor'
    $target = Join-Path $editor 'mma_editor.lua'
    if (Test-Path -LiteralPath $target) {
        Remove-Item -LiteralPath $target -Force
        Write-Step OK ((Get-Text 'Deleted') -f $target)
    } else {
        Write-Step SKIP ((Get-Text 'NotPresent') -f $target)
    }
    if ((-not $Yes) -and (Confirm-YesNo (Get-Text 'ConfirmClearM') -DefaultYes $false)) {
        Clear-ShellConfigValue -Key 'malodyRoot'
    }
    Write-Step OK (Get-Text 'MalodyGone')
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
        $mode = Read-Option -Title (Get-Text 'MenuAsk') -Options @(
            (Get-Text 'MenuInstall'),
            (Get-Text 'MenuUninstall'),
            (Get-Text 'MenuExit')
        )
        if ($mode -eq 2) { break }
        $removeMode = ($mode -eq 1)
        $gameItem = Read-Option -Title $(if ($removeMode) { Get-Text 'ChooseGameRm' } else { Get-Text 'ChooseGameIn' }) `
            -Options @('Etterna', 'Malody V') -Extra (Get-Text 'BackToMenu')
        if ($gameItem -eq 2) { continue }
        Invoke-GameFlow -G $(if ($gameItem -eq 0) { 'Etterna' } else { 'Malody' }) -Remove $removeMode
        if (-not $Yes) {
            if (-not (Confirm-YesNo (Get-Text 'ContinueAsk') -DefaultYes $false)) { break }
        }
    }
}

Write-Host ''
Write-Host (Get-Text 'Done') -ForegroundColor Green