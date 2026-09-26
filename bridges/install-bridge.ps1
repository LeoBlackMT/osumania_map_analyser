<#
.SYNOPSIS
    Interactive installer/remover for the ManiaMapAnalyser game bridge files
    (Etterna theme LoadActor injection + Malody V editor plugin), plus the
    Malody V in-game song-selection bridge (BepInEx) and the Malody 4 (4.3.7
    native client) source which needs no game files at all.

.DESCRIPTION
    Installs or removes:
      - Etterna :  bridges/etterna/mma_bridge.lua     -> Themes\<theme>\BGAnimations\ScreenSelectMusic decorations\
                  bridges/etterna/mma_gameplay.lua    -> Themes\<theme>\BGAnimations\ScreenGameplay overlay\
                  plus one LoadActor line injected before `return t` in each
                  screen's default.lua.
      - Malody V:  bridges/malody/mma_editor.lua      -> MalodyV\editor\
      - Malody V (in-game song selection, -Game MalodyBridge):
                   the BepInEx 6 IL2CPP be.788 loader archive (winhttp.dll,
                   dotnet\..., BepInEx\core\... - 228 files, SHA256 checked
                   against a cached hash and a vendored manifest) plus
                   bridges/malody/bepinex/MalodyInsightBridge.dll
                   -> MalodyV\BepInEx\plugins\MalodyInsight\
                   Files are extracted into a staging folder inside the game
                   directory, verified there, and only then moved into place.
                   Everything this installer creates/places/keeps is recorded
                   in MalodyV\BepInEx\bepinex-install.json, so uninstall only
                   removes its own files and keeps the loader; a folder with
                   BepInEx files but no such record is never touched.
      - Malody 4:  no files - the 4.3.7 native client is only observed, read
                   only (external process memory, log tail, config.json);
                   installing just records malody4Root in the shell config.

    The Malody V in-game song-selection bridge needs the upstream BepInEx 6
    IL2CPP be.788 release asset (bepinex-il2cpp-788.zip). Put it at
    bridges\malody\bepinex\loader\bepinex-il2cpp-788.zip (that folder is not
    committed) or point the MMA_BEPINEX_LOADER_ZIP environment variable at it;
    its SHA256 is checked before a single byte is written.

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
    'Etterna', 'Malody', 'MalodyBridge' or 'Malody4' to skip the game picker.
    'Malody' is the Lua editor plugin, 'MalodyBridge' the BepInEx in-game
    song-selection bridge. Omit for the menu.

.PARAMETER Uninstall
    Remove instead of install.

.PARAMETER LoaderOnly
    Malody V in-game song-selection bridge only: install (or keep) the BepInEx
    loader and stop before the plugin DLL. Use it when you want the loader in
    place first - BepInEx only writes BepInEx\interop\ after the game has been
    launched once - or when the plugin DLL is not available locally yet.

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
    [ValidateSet('Etterna', 'Malody', 'Malody4', 'MalodyBridge')]
    [string]$Game,
    [switch]$Uninstall,
    [switch]$Chinese,
    [switch]$Yes,
    [switch]$LoaderOnly,
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
    LedeMalody4     = '   - Malody 4 (4.3.7 native client): no game files are written'
    LedeMalody4Zh   = '   - Malody 4（4.3.7 原生客户端）：不向游戏目录写入任何文件'
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
    HeaderMalody4   = '--- Installing Malody 4 (native client) support ---'
    HeaderMalody4Zh = '--- 正在安装 Malody 4（原生客户端）支持 ---'
    RmEtterna       = '--- Removing Etterna bridge ---'
    RmEtternaZh     = '--- 正在卸载 Etterna 桥文件 ---'
    RmMalody        = '--- Removing Malody V bridge ---'
    RmMalodyZh      = '--- 正在卸载 Malody V 桥文件 ---'
    RmMalody4       = '--- Removing Malody 4 (native client) support ---'
    RmMalody4Zh     = '--- 正在卸载 Malody 4（原生客户端）支持 ---'
    UseDetected     = "Use detected {0} folder:`n  {1}"
    UseDetectedZh   = "使用检测到的 {0} 目录：`n  {1}"
    UsingCfgRoot    = "using {0} root from mma-shell-config.json: {1}"
    UsingCfgRootZh  = "使用 mma-shell-config.json 中记录的 {0} 目录：{1}"
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
    HelpMalody4     = @'
How to find your Malody 4 (native client) folder:
  1. Right-click the Malody shortcut you use to launch the game, choose
     'Open file location'.
  2. In the folder that opens, click the address bar and copy the full path,
     e.g. D:\Games\Malody-4.3.7.
We need the folder that directly contains 'malody.exe' and a 'beatmap' folder.
This client is not distributed on Steam, so no Steam library is searched.
'@
    HelpMalody4Zh   = @'
如何找到您的 Malody 4（原生客户端）目录：
  1. 右键点击你用来启动游戏的 Malody 快捷方式，选择「打开文件所在位置」。
  2. 在打开的文件夹中，点击地址栏复制完整路径，例如 D:\Games\Malody-4.3.7。
我们需要的是直接包含 malody.exe 和 beatmap 文件夹的那个目录。
该客户端不在 Steam 上发行，因此不会搜索 Steam 库。
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
    ConfirmRmTheme  = "Remove bridge from theme '{0}'?"
    ConfirmRmThemeZh = "从主题 '{0}' 卸载桥文件？"

    # malody
    CreatedDir      = 'created {0}'
    CreatedDirZh    = '已创建 {0}'
    MalodySrcNo     = 'bridge source not found next to this script: {0} (expected mma_editor.lua)'
    MalodySrcNoZh   = '未在脚本旁找到桥文件源：{0}（应为 mma_editor.lua）'
    MalodyTip       = 'Use it from the Malody editor: More menu -> MMA Analyze.'
    MalodyTipZh     = '使用方式：打开 Malody 编辑器 → 菜单 → MMA Analyze。'
    Malody4NoCopy   = 'No files are written into the game directory - this client is only observed (read only).'
    Malody4NoCopyZh = '无需向游戏目录写入任何文件——本客户端只被只读观察。'

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
    ConfirmClearM4  = 'Clear malody4Root in mma-shell-config.json?'
    ConfirmClearM4Zh = '是否清空 mma-shell-config.json 中的 malody4Root？'

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
    Malody4Ok       = 'Malody 4 support enabled: malody4Root recorded, no game files written.'
    Malody4OkZh     = 'Malody 4 支持已启用：malody4Root 已记录，未向游戏目录写入任何文件。'
    Malody4Gone     = 'Malody 4 support removed: malody4Root cleared, no game files were touched.'
    Malody4GoneZh   = 'Malody 4 支持已卸载：malody4Root 已清空，未改动任何游戏文件。'
    RemindTheme     = 'Reminder: a theme update wipes these files - re-run this installer to restore.'
    RemindThemeZh   = '提示：主题更新会删除这些文件 - 重新运行本安装器即可恢复。'

    # Malody V in-game song-selection bridge (BepInEx loader + plugin DLL)
    LedeMalodyBridge     = '   - Malody V in-game song-selection bridge (BepInEx loader + plugin)'
    LedeMalodyBridgeZh   = '   - Malody V 游戏内选曲桥（BepInEx 加载器 + 插件）'
    HeaderMalodyBridge   = '--- Installing Malody V song-selection bridge (BepInEx) ---'
    HeaderMalodyBridgeZh = '--- 正在安装 Malody V 游戏内选曲桥（BepInEx） ---'
    RmMalodyBridge       = '--- Removing Malody V song-selection bridge (BepInEx) ---'
    RmMalodyBridgeZh     = '--- 正在卸载 Malody V 游戏内选曲桥（BepInEx） ---'
    ChooseMalodyV        = 'Which Malody V bridge?'
    ChooseMalodyVZh      = '要处理哪个 Malody V 桥？'
    MalodyVLua           = 'Lua editor bridge (MMA Analyze)'
    MalodyVLuaZh         = 'Lua 编辑器桥（MMA Analyze）'
    MalodyVBepInEx       = 'In-game song-selection bridge (BepInEx)'
    MalodyVBepInExZh     = '游戏内选曲桥（BepInEx）'
    MalodyVBoth          = 'Both (Lua + BepInEx)'
    MalodyVBothZh        = '两者（Lua + BepInEx）'
    BridgeRootOk         = 'game folder looks right: Malody V.exe + GameAssembly.dll found'
    BridgeRootOkZh       = '游戏目录校验通过：找到 Malody V.exe 与 GameAssembly.dll'
    BridgeRootMissing    = 'not found in this folder: {0} (continuing - a test/placeholder root is fine)'
    BridgeRootMissingZh  = '该目录中缺少：{0}（继续执行——测试/占位目录属正常情况）'
    ReparsePoint         = 'refusing to write through a junction/symlink: {0}'
    ReparsePointZh       = '拒绝穿过联接/符号链接写入：{0}'
    GameRunning          = 'Malody V is running ({0}) - close the game and run this again'
    GameRunningZh        = 'Malody V 正在运行（{0}）——请先关闭游戏后重新运行'
    GameRunningTip       = 'The loader and the plugin cannot be replaced while the game has them open.'
    GameRunningTipZh     = '游戏运行时加载器与插件文件被占用，无法替换。'
    GameNotRunning       = 'Malody V is not running'
    GameNotRunningZh     = 'Malody V 未在运行'
    LoaderZipFound       = 'loader archive: {0}'
    LoaderZipFoundZh     = '加载器压缩包：{0}'
    LoaderZipEnvBad      = 'MMA_BEPINEX_LOADER_ZIP points at a missing file, falling back to the default path: {0}'
    LoaderZipEnvBadZh    = 'MMA_BEPINEX_LOADER_ZIP 指向的文件不存在，改用默认路径：{0}'
    LoaderZipMissing     = 'loader archive not found. Download the BepInEx 6 IL2CPP be.788 release asset (bepinex-il2cpp-788.zip) and save it as:`n  {0}`nor set the MMA_BEPINEX_LOADER_ZIP environment variable to its path, then run this again.'
    LoaderZipMissingZh   = '未找到加载器压缩包。请从 GitHub Release 下载 BepInEx 6 IL2CPP be.788 的资产（bepinex-il2cpp-788.zip）并保存为：`n  {0}`n或把环境变量 MMA_BEPINEX_LOADER_ZIP 指向该文件，然后重新运行。'
    LoaderZipHashOk      = 'loader archive SHA256 verified: {0}'
    LoaderZipHashOkZh    = '加载器压缩包 SHA256 校验通过：{0}'
    LoaderZipHashBad     = 'loader archive SHA256 mismatch, nothing was written (expected {0}, got {1})'
    LoaderZipHashBadZh   = '加载器压缩包 SHA256 不匹配，未写入任何文件（期望 {0}，实际 {1}）'
    UnityLibsFound       = 'unity reference libraries archive found: {0}'
    UnityLibsFoundZh     = '已找到 Unity 参考程序集压缩包：{0}'
    UnityLibsEnvBad      = 'MMA_MALODY_UNITY_LIBS_ZIP does not exist, falling back to the default path: {0}'
    UnityLibsEnvBadZh    = 'MMA_MALODY_UNITY_LIBS_ZIP 指向的文件不存在，改用默认路径：{0}'
    UnityLibsMissing     = 'unity reference libraries archive not found. BepInEx downloads this file itself on the first launch and then reuses it without checking it ever again, so one truncated download breaks every later launch. Save a copy as:`n  {0}`nor set the MMA_MALODY_UNITY_LIBS_ZIP environment variable to its path, then run this again.'
    UnityLibsMissingZh   = '未找到 Unity 参考程序集压缩包。BepInEx 会在首次启动时自行下载这个文件，此后一直复用且不再校验，因此一次下载不完整就会让之后每次启动都失败。请把它保存为：`n  {0}`n或把环境变量 MMA_MALODY_UNITY_LIBS_ZIP 指向该文件，然后重新运行。'
    UnityLibsHashOk      = 'unity reference libraries SHA256 verified: {0}'
    UnityLibsHashOkZh    = 'Unity 参考程序集 SHA256 校验通过：{0}'
    UnityLibsHashBad     = 'unity reference libraries SHA256 mismatch, nothing was written (expected {0}, got {1})'
    UnityLibsHashBadZh   = 'Unity 参考程序集 SHA256 不匹配，未写入任何文件（期望 {0}，实际 {1}）'
    UnityLibsOk          = 'unity reference libraries: {0}'
    UnityLibsOkZh        = 'Unity 参考程序集：{0}'
    UnityLibsSkip        = 'unity reference libraries already in place with the same hash: {0}'
    UnityLibsSkipZh      = 'Unity 参考程序集已存在且哈希一致：{0}'
    UnityLibsRepair      = '{0} is not a valid copy (expected {1}, got {2}) - replacing it so the next launch does not fail with it'
    UnityLibsRepairZh    = '{0} 不是有效副本（期望 {1}，实际 {2}）——已替换，避免下次启动因此失败'
    UnityLibsBad         = 'could not place the unity reference libraries: {0}'
    UnityLibsBadZh       = '无法放置 Unity 参考程序集：{0}'
    UnityLibsVerifyBad   = 're-verification failed: unity reference libraries mismatch at {0}'
    UnityLibsVerifyBadZh = '复验失败：Unity 参考程序集不一致 {0}'
    ZipOpenFail          = 'could not open {0}: {1}'
    ZipOpenFailZh        = '无法打开 {0}：{1}'
    ManifestNo           = 'loader manifest not found: {0}'
    ManifestNoZh         = '未找到加载器清单：{0}'
    ManifestBad          = 'loader manifest unreadable: {0}'
    ManifestBadZh        = '加载器清单无法解析：{0}'
    ZipEntryBad          = 'archive entry rejected - {0}: {1}'
    ZipEntryBadZh        = '压缩包条目被拒绝 - {0}：{1}'
    ZipReasonEmpty       = 'empty entry name'
    ZipReasonEmptyZh     = '条目名为空'
    ZipReasonDotDot      = 'contains a .. path segment'
    ZipReasonDotDotZh    = '包含 .. 路径段'
    ZipReasonDrive       = 'contains a drive letter'
    ZipReasonDriveZh     = '包含盘符'
    ZipReasonAbsolute    = 'absolute path'
    ZipReasonAbsoluteZh  = '绝对路径'
    ZipReasonControl     = 'contains a control character'
    ZipReasonControlZh   = '包含控制字符'
    ZipReasonDuplicate   = 'duplicate of another entry after normalization (case-insensitive)'
    ZipReasonDuplicateZh = '归一化后与其它条目重名（大小写不敏感）'
    ZipReasonNotListed   = 'not listed in the loader manifest'
    ZipReasonNotListedZh = '不在加载器清单中'
    ZipRejected          = 'archive rejected: {0} bad entry/entries - nothing was written'
    ZipRejectedZh        = '压缩包被拒绝：{0} 个条目不合法——未写入任何文件'
    ZipPlanOk            = 'archive entries match the loader manifest ({0} files)'
    ZipPlanOkZh          = '压缩包条目与加载器清单一致（{0} 个文件）'
    ZipPlanShort         = 'archive holds {0} file(s) but the loader manifest lists {1} - refusing to install an incomplete loader'
    ZipPlanShortZh       = '压缩包内有 {0} 个文件，但加载器清单列出 {1} 个——拒绝安装不完整的加载器'
    ZipEntryGone         = 'archive entry disappeared while reading it: {0}'
    ZipEntryGoneZh       = '读取时压缩包条目消失：{0}'
    StageExtracted       = 'extracted {0} file(s) to the staging folder'
    StageExtractedZh     = '已解包 {0} 个文件到暂存目录'
    StageVerifyBad       = 'staged file does not match the loader manifest: {0}'
    StageVerifyBadZh     = '暂存文件与加载器清单不一致：{0}'
    StageVerifyFail      = 'staging verification failed for {0} file(s) - nothing was moved into the game folder'
    StageVerifyFailZh    = '暂存校验失败 {0} 个文件——未向游戏目录移动任何文件'
    StageVerified        = 'verified size + SHA256 for all {0} staged file(s)'
    StageVerifiedZh      = '已校验全部 {0} 个暂存文件的大小与 SHA256'
    LoaderPlaced         = 'loader: {0}'
    LoaderPlacedZh       = '加载器：{0}'
    LoaderInPlace        = 'loader: {0} (already in place)'
    LoaderInPlaceZh      = '加载器：{0}（已就位）'
    LoaderPlacedCount    = 'loader files: {0} placed, {1} already in place'
    LoaderPlacedCountZh  = '加载器文件：新放置 {0} 个，已就位 {1} 个'
    MoveFail             = 'could not move {0} into place: {1}'
    MoveFailZh           = '无法将 {0} 移动到目标位置：{1}'
    PlanClean            = 'no BepInEx in this folder - clean install'
    PlanCleanZh          = '该目录中没有 BepInEx——全新安装'
    PlanOurs             = 'existing BepInEx is ours (manifest hashes match) - loader stays as it is'
    PlanOursZh           = '已存在的 BepInEx 属于本安装器（清单哈希一致）——加载器保持原样'
    PlanPartial          = 'existing BepInEx is partially ours ({0} file(s) missing, {1} modified) - resuming'
    PlanPartialZh        = '已存在的 BepInEx 是本安装器装的但未完成（缺失 {0} 个，被改动 {1} 个）——继续补齐'
    PlanMismatch         = '{0} does not match our record (expected {1}, got {2}) - it will be restored'
    PlanMismatchZh       = '{0} 与本安装器的记录不一致（期望 {1}，实际 {2}）——将被还原'
    PlanExternal         = 'found BepInEx files here that this installer did not create - refusing to touch them'
    PlanExternalZh       = '该目录中存在并非本安装器创建的 BepInEx 文件——拒绝改动'
    PlanExternalHint     = 'If you installed these paths yourself, delete them and run this again:'
    PlanExternalHintZh   = '如果这些路径是你本人安装的，请先删除它们再重新运行：'
    PlanExternalTip      = 'Nothing was written. These paths are only listed, never deleted automatically.'
    PlanExternalTipZh    = '未写入任何文件。此处只列出这些路径，绝不自动删除。'
    BridgeResumeTip      = 'Re-run this installer to continue: the manifest records what is already in place.'
    BridgeResumeTipZh    = '重新运行本安装器即可续做：清单已记录哪些文件已就位。'
    DllOk                = 'plugin: {0}'
    DllOkZh              = '插件：{0}'
    DllSkip              = 'plugin already in place with the same hash: {0}'
    DllSkipZh            = '插件已存在且哈希一致：{0}'
    DllMissing           = 'bridge plugin DLL not found next to this script: {0}'
    DllMissingZh         = '未在脚本旁找到桥插件 DLL：{0}'
    DllVerifyBad         = 'plugin DLL hash mismatch after copy: {0}'
    DllVerifyBadZh       = '复制后插件 DLL 哈希不一致：{0}'
    UpstreamPresent      = 'another bridge plugin is already installed here: {0}'
    UpstreamPresentZh    = '该目录里已经装着另一个桥插件：{0}'
    UpstreamPresentTip   = 'This fork and that plugin hook the same game methods, so having both would double-hook. Nothing was deleted - remove the file above yourself if you want to switch to this fork.'
    UpstreamPresentTipZh = '本 fork 与它会挂在同一批游戏方法上，两者共存会造成双 Hook。本安装器没有删除任何文件——若要改用本 fork，请自行删除上面那个文件。'
    ManifestWrote        = 'wrote {0}'
    ManifestWroteZh      = '已写入 {0}'
    VerifyLoaderBad      = 're-verification failed for {0} ({1})'
    VerifyLoaderBadZh    = '复验失败：{0}（{1}）'
    VerifyBad            = 're-verification failed: plugin DLL hash mismatch at {0}'
    VerifyBadZh          = '复验失败：插件 DLL 哈希不一致 {0}'
    VerifyOk             = 're-verified: {0} loader file(s) and plugin DLL hash'
    VerifyOkZh           = '复验通过：{0} 个加载器文件与插件 DLL 哈希'
    MalodyBridgeOk       = 'Malody V song-selection bridge installed.'
    MalodyBridgeOkZh     = 'Malody V 游戏内选曲桥安装完成。'
    MalodyBridgeNot      = 'Malody V song-selection bridge was NOT installed (see the failures above).'
    MalodyBridgeNotZh    = 'Malody V 游戏内选曲桥未安装成功（请查看上方错误）。'
    MalodyBridgeTip      = 'Start the game and pick a chart: the selection is sent to mma-shell on 127.0.0.1:17653.'
    MalodyBridgeTipZh    = '启动游戏并选择谱面：选曲结果会通过 127.0.0.1:17653 发送给 mma-shell。'
    MalodyBridgeGone     = 'Malody V song-selection bridge removed (the loader is kept).'
    MalodyBridgeGoneZh   = 'Malody V 游戏内选曲桥已卸载（加载器保留）。'
    LoaderOnlyMode       = 'loader-only pass: the plugin DLL is skipped'
    LoaderOnlyModeZh     = '仅加载器模式：跳过插件 DLL'
    LoaderOnlyNoDll      = 'plugin DLL not installed (-LoaderOnly)'
    LoaderOnlyNoDllZh    = '未安装插件 DLL（-LoaderOnly）'
    VerifyOkLoaderOnly   = 're-verified: {0} loader file(s)'
    VerifyOkLoaderOnlyZh = '复验通过：{0} 个加载器文件'
    LoaderOnlyOk         = 'BepInEx loader installed; the plugin DLL was skipped.'
    LoaderOnlyOkZh       = 'BepInEx 加载器已安装；插件 DLL 已跳过。'
    LoaderOnlyTip        = 'Launch the game once so BepInEx generates BepInEx\interop\, then run this installer again without -LoaderOnly to add the plugin.'
    LoaderOnlyTipZh      = '请先启动游戏一次，让 BepInEx 生成 BepInEx\interop\，然后不带 -LoaderOnly 重新运行本安装器以安装插件。'
    LoaderOnlyN_A        = '-LoaderOnly applies to the Malody V in-game song-selection bridge only, ignoring it for {0}'
    LoaderOnlyN_AZh      = '-LoaderOnly 只对 Malody V 游戏内选曲桥有效，{0} 将忽略该参数'
    UnNoRecord           = 'no install record from this installer at {0} - deleting nothing'
    UnNoRecordZh         = '在 {0} 未找到本安装器的安装记录——不做任何删除'
    UnHashDiff           = 'kept {0}: it does not match the recorded hash, so it is left alone'
    UnHashDiffZh         = '已保留 {0}：与记录中的哈希不一致，不做删除'
    UnKeptNote           = 'loader files kept on purpose ({0} file(s)): BepInEx itself stays installed'
    UnKeptNoteZh         = '按设计保留加载器文件（{0} 个）：BepInEx 本体继续保留'
    UnKeptTip            = 'To remove the loader too, delete winhttp.dll, doorstop_config.ini, .doorstop_version, changelog.txt, dotnet\ and BepInEx\ yourself.'
    UnKeptTipZh          = '若要连同加载器一起移除，请自行删除 winhttp.dll、doorstop_config.ini、.doorstop_version、changelog.txt、dotnet\ 与 BepInEx\。'
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
    Write-Host (Get-Text 'LedeMalodyBridge') -ForegroundColor Gray
    Write-Host (Get-Text 'LedeMalody4') -ForegroundColor Gray
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

function Test-PathRootAvailable {
    <#
        Drive-letter pre-check. Join-Path throws DriveNotFoundException when the
        first path segment names a drive that does not exist (e.g. the common-path
        candidates probe D:\ while the user has no D: drive) - a terminating error
        under $ErrorActionPreference = 'Stop' that would kill the installer.
        Detection must never crash: probe the drive root first and skip.
    #>
    param([string]$p)
    if (-not $p) { return $false }
    $root = [System.IO.Path]::GetPathRoot($p)
    if (-not $root) { return $true }  # relative path: no drive to validate
    if ($root -notmatch '^[A-Za-z]:') { return $true }  # UNC / device path
    try {
        return [System.IO.DriveInfo]::new($root.Substring(0, 1)).IsReady
    } catch {
        return $false
    }
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
            $help = switch ($What) {
                'Etterna'      { Get-Text 'HelpEtterna'; break }
                'Malody'       { Get-Text 'HelpMalody'; break }
                'MalodyBridge' { Get-Text 'HelpMalody'; break }
                'Malody4'      { Get-Text 'HelpMalody4'; break }
            }
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
        if (-not (Test-PathRootAvailable $r)) { continue }
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
    if (-not (Test-PathRootAvailable $p)) { return $false }
    $save = Join-Path $p 'Save'
    $themes = Join-Path $p 'Themes'
    return (Test-Path $save -PathType Container) -and (Test-Path $themes -PathType Container)
}

function Test-MalodyRoot {
    param([string]$p)
    if (-not $p) { return $false }
    if (-not (Test-PathRootAvailable $p)) { return $false }
    return (Test-Path (Join-Path $p 'chart') -PathType Container) -and
           (Test-Path (Join-Path $p 'skin') -PathType Container)
}

function Test-Malody4Root {
    # the 4.3.7 native client root: malody.exe next to the beatmap folder.
    # No version check here - mma-shell validates the executable itself.
    param([string]$p)
    if (-not $p) { return $false }
    if (-not (Test-PathRootAvailable $p)) { return $false }
    return (Test-Path (Join-Path $p 'beatmap') -PathType Container) -and
           (Test-Path (Join-Path $p 'malody.exe') -PathType Leaf)
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
        # probing is best-effort: never let an unexpected provider error
        # terminate the installer ($ErrorActionPreference = 'Stop')
        try {
            if ($p) { $p = Format-PathInput -Path $p }
            if ($p -and (Test-EtternaRoot $p) -and -not $cands.Contains($p)) { $cands.Add($p) }
        } catch { }
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
        # probing is best-effort: never let an unexpected provider error
        # terminate the installer ($ErrorActionPreference = 'Stop')
        try {
            if ($p) { $p = Format-PathInput -Path $p }
            if ($p -and (Test-MalodyRoot $p) -and -not $cands.Contains($p)) { $cands.Add($p) }
        } catch { }
    }
    $p = Get-RunningProcessRoot -Names 'Malody V', 'MalodyV'
    if ($p) { & $add $p }
    if ($env:MMA_MALODY_ROOT) { & $add $env:MMA_MALODY_ROOT }
    foreach ($lib in (Get-SteamLibraryRoots)) {
        if (-not (Test-PathRootAvailable $lib)) { continue }
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

function Get-Malody4Candidates {
    $cands = New-Object 'System.Collections.Generic.List[string]'
    $add = {
        param($p)
        # probing is best-effort: never let an unexpected provider error
        # terminate the installer ($ErrorActionPreference = 'Stop')
        try {
            if ($p) { $p = Format-PathInput -Path $p }
            if ($p -and (Test-Malody4Root $p) -and -not $cands.Contains($p)) { $cands.Add($p) }
        } catch { }
    }
    # running process (the native client's binary is malody.exe)
    $p = Get-RunningProcessRoot -Names 'malody'
    if ($p) { & $add $p }
    if ($env:MMA_MALODY4_ROOT) { & $add $env:MMA_MALODY4_ROOT }
    # common install paths (the 4.3.7 native client was NOT distributed on
    # Steam, so Steam libraries are deliberately not consulted here)
    foreach ($c in @(
        'D:/Games/Malody-4.3.7',
        'C:/Games/Malody-4.3.7',
        'D:/Malody-4.3.7',
        'D:/Games/Malody',
        'C:/Malody-4.3.7'
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
    $validator = switch ($Game) {
        'Etterna'      { { param($p) Test-EtternaRoot $p } }
        'Malody'       { { param($p) Test-MalodyRoot $p } }
        'MalodyBridge' { { param($p) Test-MalodyRoot $p } }
        'Malody4'      { { param($p) Test-Malody4Root $p } }
    }
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

function Get-ShellConfigRoot {
    param([string]$Key)
    $path = Get-ShellConfigPath
    if (-not (Test-Path -LiteralPath $path)) { return '' }
    try {
        $obj = Get-Content -LiteralPath $path -Raw | ConvertFrom-Json
        $v = $obj.PSObject.Properties[$Key]
        if ($v) { return ([string]$v.Value).Trim() }
    } catch { }
    return ''
}

function Select-ExistingRoot {
    <#
    .SYNOPSIS
        Pick a game root for UNINSTALL: explicit -Root first, then the root we
        recorded in mma-shell-config.json at install time (no re-probing), then
        fall back to the normal detection flow only when neither is available.
    #>
    param(
        [string]$Game,
        [string]$ConfigKey,
        [scriptblock]$Validator
    )
    if ($Root) {
        $norm = Format-PathInput -Path $Root
        if ($norm -and (& $Validator $norm)) { return $norm }
        Write-Step FAIL ((Get-Text 'InvalidRoot') -f $Game, $Root)
        return $null
    }
    $cfg = Get-ShellConfigRoot -Key $ConfigKey
    if ($cfg -and (& $Validator $cfg)) {
        $cfg = Format-PathInput -Path $cfg
        Write-Step INFO ((Get-Text 'UsingCfgRoot') -f $Game, $cfg)
        return $cfg
    }
    $cands = @(switch ($Game) {
        'Etterna'      { Get-EtternaCandidates }
        'Malody'       { Get-MalodyCandidates }
        'MalodyBridge' { Get-MalodyCandidates }
        'Malody4'      { Get-Malody4Candidates }
    })
    return Select-GameRoot -Game $Game -Candidates $cands -ForceRoot ''
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
    if (-not $data.Contains('malody4Root')) { $data['malody4Root'] = '' }
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
    # use the recorded root from mma-shell-config.json when available (no re-probing)
    $root = Select-ExistingRoot -Game 'Etterna' -ConfigKey 'etternaRoot' -Validator { param($p) Test-EtternaRoot $p }
    if (-not $root) { return }

    # uninstall list = themes with bridge traces (files/injected lines), even if
    # their structure degraded since install; structurally complete ones too.
    $support = Get-ThemeSupport -EtternaRoot $root | Where-Object { $_.Installable -or (Test-ThemeHasBridge $_) }
    $themes = @($support | Select-Object -ExpandProperty Name)
    if ($themes.Count -eq 0) {
        Write-Step FAIL ((Get-Text 'NoThemesRm') -f $root)
        return
    }
    $theme = $null
    if ($Theme) {
        if ($themes -contains $Theme) {
            $theme = $Theme
        } else {
            Write-Step FAIL ((Get-Text 'ThemeNotFound') -f $Theme, $root)
            return
        }
    } elseif ($Yes) {
        # auto mode: highest-priority theme
        if ($themes -contains 'Rebirth') { $theme = 'Rebirth' } else { $theme = $themes[0] }
    } elseif ($themes.Count -eq 1) {
        # single candidate: still ask before removing
        if (-not (Confirm-YesNo ((Get-Text 'ConfirmRmTheme') -f $themes[0]) -DefaultYes $false)) { return }
        $theme = $themes[0]
    } else {
        $idx = Read-Option -Title (Get-Text 'ThemePickRm') -Options $themes
        $theme = $themes[$idx]
    }
    Write-Step INFO ((Get-Text 'RmThemeFrom') -f $theme)
    Uninstall-EtternaTheme -Root $root -Theme $theme
    if ((-not $Yes) -and (Confirm-YesNo (Get-Text 'ConfirmClearE') -DefaultYes $false)) {
        Clear-ShellConfigValue -Key 'etternaRoot'
    }
    Write-Step OK (Get-Text 'EtternaGone')
}

function Uninstall-EtternaTheme {
    param([string]$Root, [string]$Theme)
    # screen -> file pairing, mirrors the install targets exactly
    # (each screen only ever holds its own bridge file)
    $pairs = @(
        @{ Screen = 'ScreenSelectMusic decorations'; File = 'mma_bridge.lua' },
        @{ Screen = 'ScreenGameplay overlay';        File = 'mma_gameplay.lua' }
    )
    foreach ($pair in $pairs) {
        $screenDir = Join-Path $Root ("Themes\{0}\BGAnimations\{1}" -f $Theme, $pair.Screen)
        $defaultLua = Join-Path $screenDir 'default.lua'
        if (-not (Test-Path -LiteralPath $screenDir -PathType Container)) {
            Write-Step SKIP ((Get-Text 'ScreenDirNo') -f $screenDir)
            continue
        }
        $target = Join-Path $screenDir $pair.File
        if (Test-Path -LiteralPath $target) {
            Remove-Item -LiteralPath $target -Force
            Write-Step OK ((Get-Text 'Deleted') -f $target)
        } else {
            Write-Step SKIP ((Get-Text 'NotPresent') -f $target)
        }
        Remove-LoadActorLine -DefaultLua $defaultLua -ActorFile $pair.File | Out-Null
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
    # use the recorded root from mma-shell-config.json when available (no re-probing)
    $root = Select-ExistingRoot -Game 'Malody' -ConfigKey 'malodyRoot' -Validator { param($p) Test-MalodyRoot $p }
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
# Malody 4 (4.3.7 native client) - observed read-only, config only
# ---------------------------------------------------------------------------

function Install-Malody4Bridge {
    Write-Host ''
    Write-Host (Get-Text 'HeaderMalody4') -ForegroundColor Cyan
    $cands = Get-Malody4Candidates
    $root = Select-GameRoot -Game 'Malody4' -Candidates $cands -ForceRoot $Root
    if (-not $root) { return }

    # This source needs no bridge files at all: mma-shell only observes the
    # client (external process memory, its log and config.json), read-only.
    # So the install is exactly one thing - record the root in the config.
    Write-Step INFO (Get-Text 'Malody4NoCopy')
    Set-ShellConfigValue -Key 'malody4Root' -Value $root
    Write-Host ''
    Write-Step OK (Get-Text 'Malody4Ok')
}

function Uninstall-Malody4Bridge {
    Write-Host ''
    Write-Host (Get-Text 'RmMalody4') -ForegroundColor Cyan
    # use the recorded root from mma-shell-config.json when available (no re-probing)
    $root = Select-ExistingRoot -Game 'Malody4' -ConfigKey 'malody4Root' -Validator { param($p) Test-Malody4Root $p }
    if (-not $root) { return }

    Write-Step INFO (Get-Text 'Malody4NoCopy')
    if ((-not $Yes) -and (-not (Confirm-YesNo (Get-Text 'ConfirmClearM4') -DefaultYes $false))) { return }
    # nothing was ever written into the game directory, so only the config key goes
    Clear-ShellConfigValue -Key 'malody4Root'
    Write-Step OK (Get-Text 'Malody4Gone')
}

# ---------------------------------------------------------------------------
# Malody V in-game song-selection bridge (BepInEx 6 IL2CPP loader + plugin DLL)
#
# Everything below writes inside the game directory, so every write/delete
# target is gated twice: a hard reparse-point gate (a junction or symlink that
# already exists in the game folder must never redirect our writes outside it)
# and the install manifest {root}\BepInEx\bepinex-install.json, which records
# exactly what this installer created / placed / kept.
# ---------------------------------------------------------------------------

$script:LoaderZipSha256   = 'F4CC496BD098A0DF4164B81E3737297707F13A47C2478DBA2F60EEFAB784817A'
$script:LoaderZipRel      = 'malody\bepinex\loader\bepinex-il2cpp-788.zip'
$script:LoaderManifestRel = 'malody\bepinex\loader-manifest.bepinex-6.0.0-be.788.json'
# Unity reference assemblies for Malody V's Unity version (2022.3.62). BepInEx
# downloads this one file itself on its first launch
# (https://unity.bepinex.dev/libraries/2022.3.62.zip) and afterwards reuses
# whatever sits in BepInEx\unity-libs\ without ever validating it again, so a
# single truncated download makes every later launch fail with
# "End of Central Directory record could not be found" and no plugin ever loads.
# We vendor the byte-exact copy and install it before the first launch instead.
$script:UnityLibsZipSha256 = '575E7D600F69DE8200CCF4DB700B3AE6252366C22E8C3434C860E428974518D1'
$script:UnityLibsZipRel    = 'malody\bepinex\unity-libs\2022.3.62.zip'
$script:UnityLibsTarget    = 'BepInEx/unity-libs/2022.3.62.zip'
$script:BridgeDllRel      = 'malody\bepinex\plugin\MMAMalodySelection.dll'
$script:BridgeManifestRel = 'BepInEx\bepinex-install.json'
$script:BridgeDllTarget   = 'BepInEx/plugins/MalodyInsight/MMAMalodySelection.dll'
# The plugin's own BepInEx config; BepInEx writes it on first load (name follows the
# plugin GUID, `local.mma.malody.selection`). Never created by this installer.
$script:BridgeCfgTarget   = 'BepInEx/config/local.mma.malody.selection.cfg'
# The upstream bridge this fork replaces. It must NOT be co-installed: two plugins
# patching the same methods would double-hook. Only ever reported, never deleted.
$script:BridgeUpstreamRel = 'BepInEx/plugins/MalodyInsight/MalodyInsightBridge.dll'
# a loader we did not install: these show up in a folder with no manifest
$script:BridgeForeignRel  = @('winhttp.dll', 'doorstop_config.ini', '.doorstop_version', 'changelog.txt', 'dotnet', 'BepInEx')

function Initialize-ZipAssembly {
    # System.IO.Compression.ZipFile ships with Windows PowerShell 5.1 but is not
    # loaded until asked for; pwsh 7 already has the type. Add-Type is a no-op
    # once the assembly is in the AppDomain.
    if (-not ('System.IO.Compression.ZipFile' -as [type])) {
        Add-Type -AssemblyName System.IO.Compression.FileSystem
    }
}

function Join-BridgeTarget {
    # manifest/hash records use forward slashes; the filesystem wants backslashes
    param([string]$Root, [string]$Rel)
    return (Join-Path $Root ($Rel -replace '/', '\'))
}

function Get-BridgeFileState {
    # 'ok' | 'missing' | 'mismatch' for one file recorded in a manifest.
    # $Size -gt 0 adds the explicit size check (the hash alone implies it).
    param([string]$Path, [string]$Sha256, [long]$Size = 0)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return 'missing' }
    try {
        if ($Size -gt 0) {
            if ((Get-Item -LiteralPath $Path -Force).Length -ne $Size) { return 'mismatch' }
        }
        if ((Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash -ne $Sha256) { return 'mismatch' }
    } catch {
        return 'missing'
    }
    return 'ok'
}

function Assert-NoReparsePoint {
    # Walk every component of $Path (down to $StopAt) and fail on the
    # ReparsePoint attribute / LinkType: a junction or symlink already present
    # inside the game directory must never redirect a write or a delete to a
    # path outside it. A component that does not exist yet redirects nothing.
    param([string]$Path, [string]$StopAt = '')
    if (-not $Path) { return $true }
    $full = $Path
    try { $full = [System.IO.Path]::GetFullPath($Path) } catch { return $true }
    $stop = ''
    if ($StopAt) {
        try { $stop = [System.IO.Path]::GetFullPath($StopAt) } catch { $stop = $StopAt }
        $stop = $stop.TrimEnd([char[]]@('\', '/'))
    }
    $cur = $full.TrimEnd([char[]]@('\', '/'))
    while ($cur) {
        $item = Get-Item -LiteralPath $cur -Force -ErrorAction SilentlyContinue
        if ($item) {
            $isLink = (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0)
            if (-not $isLink) {
                $linkType = $null
                try { $linkType = $item.LinkType } catch { }
                if ($linkType) { $isLink = $true }
            }
            if ($isLink) {
                Write-Step FAIL ((Get-Text 'ReparsePoint') -f $cur)
                return $false
            }
        }
        if ($stop -and ($cur -ieq $stop)) { break }
        if ($cur -match '^[A-Za-z]:$') { break }   # drive root reached
        $parent = Split-Path -Path $cur -Parent -ErrorAction SilentlyContinue
        if (-not $parent -or $parent -eq $cur) { break }
        $cur = $parent.TrimEnd([char[]]@('\', '/'))
    }
    return $true
}

function Test-MalodyBridgeRoot {
    # Is this really a Malody V folder? Reported as OK/SKIP and never throwing,
    # so a test/dummy root still exercises the rest of the install. The hard
    # gates are the reparse-point check and the running-game check.
    param([string]$Root)
    $missing = @()
    foreach ($f in @('Malody V.exe', 'GameAssembly.dll')) {
        if (-not (Test-Path -LiteralPath (Join-Path $Root $f) -PathType Leaf)) { $missing += $f }
    }
    if ($missing.Count -eq 0) {
        Write-Step OK (Get-Text 'BridgeRootOk')
        return $true
    }
    Write-Step SKIP ((Get-Text 'BridgeRootMissing') -f ($missing -join ', '))
    return $false
}

function Resolve-LoaderZip {
    # Locate the BepInEx 6 IL2CPP be.788 archive: MMA_BEPINEX_LOADER_ZIP first,
    # then the vendored local slot next to this script
    # (bridges\malody\bepinex\loader\, not committed). No third source.
    if ($env:MMA_BEPINEX_LOADER_ZIP) {
        if (Test-Path -LiteralPath $env:MMA_BEPINEX_LOADER_ZIP -PathType Leaf) {
            $p = (Get-Item -LiteralPath $env:MMA_BEPINEX_LOADER_ZIP -Force).FullName
            Write-Step INFO ((Get-Text 'LoaderZipFound') -f $p)
            return $p
        }
        Write-Step WARN ((Get-Text 'LoaderZipEnvBad') -f $env:MMA_BEPINEX_LOADER_ZIP)
    }
    $def = Join-Path $PSScriptRoot $script:LoaderZipRel
    if (Test-Path -LiteralPath $def -PathType Leaf) {
        $p = (Get-Item -LiteralPath $def -Force).FullName
        Write-Step INFO ((Get-Text 'LoaderZipFound') -f $p)
        return $p
    }
    Write-Step FAIL ((Get-Text 'LoaderZipMissing') -f $def)
    return $null
}

function Assert-LoaderZipHash {
    param([string]$Path)
    $actual = ''
    try { $actual = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash } catch {
        Write-Step FAIL ((Get-Text 'ZipOpenFail') -f $Path, $_.Exception.Message)
        return $false
    }
    if ($actual -ne $script:LoaderZipSha256) {
        Write-Step FAIL ((Get-Text 'LoaderZipHashBad') -f $script:LoaderZipSha256, $actual)
        return $false
    }
    Write-Step OK ((Get-Text 'LoaderZipHashOk') -f $actual)
    return $true
}

function Resolve-UnityLibsZip {
    # Locate the vendored Unity reference assemblies for Malody V's Unity
    # version: MMA_MALODY_UNITY_LIBS_ZIP first, then the vendored local slot
    # next to this script (bridges\malody\bepinex\unity-libs\, not committed).
    # No third source: this file is what keeps the loader's first launch offline
    # and deterministic, so it ships with the installer like the loader archive.
    if ($env:MMA_MALODY_UNITY_LIBS_ZIP) {
        if (Test-Path -LiteralPath $env:MMA_MALODY_UNITY_LIBS_ZIP -PathType Leaf) {
            $p = (Get-Item -LiteralPath $env:MMA_MALODY_UNITY_LIBS_ZIP -Force).FullName
            Write-Step INFO ((Get-Text 'UnityLibsFound') -f $p)
            return $p
        }
        Write-Step WARN ((Get-Text 'UnityLibsEnvBad') -f $env:MMA_MALODY_UNITY_LIBS_ZIP)
    }
    $def = Join-Path $PSScriptRoot $script:UnityLibsZipRel
    if (Test-Path -LiteralPath $def -PathType Leaf) {
        $p = (Get-Item -LiteralPath $def -Force).FullName
        Write-Step INFO ((Get-Text 'UnityLibsFound') -f $p)
        return $p
    }
    Write-Step FAIL ((Get-Text 'UnityLibsMissing') -f $def)
    return $null
}

function Assert-UnityLibsZipHash {
    param([string]$Path)
    $actual = ''
    try { $actual = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash } catch {
        Write-Step FAIL ((Get-Text 'ZipOpenFail') -f $Path, $_.Exception.Message)
        return $false
    }
    if ($actual -ne $script:UnityLibsZipSha256) {
        Write-Step FAIL ((Get-Text 'UnityLibsHashBad') -f $script:UnityLibsZipSha256, $actual)
        return $false
    }
    Write-Step OK ((Get-Text 'UnityLibsHashOk') -f $actual)
    return $true
}

function Install-UnityLibsZip {
    <# Place the vendored archive at {root}\BepInEx\unity-libs\2022.3.62.zip.
       A file already there is kept only while size + SHA256 match; anything else
       - a truncated download, a half-written copy - is replaced, which is what
       repairs an install whose every launch dies in InteropManager. #>
    param([string]$Root, [string]$ZipPath, [long]$Size, [string]$Sha256)
    $dst = Join-BridgeTarget -Root $Root -Rel $script:UnityLibsTarget
    if ((Get-BridgeFileState -Path $dst -Sha256 $Sha256 -Size $Size) -eq 'ok') {
        Write-Step SKIP ((Get-Text 'UnityLibsSkip') -f $script:UnityLibsTarget)
        return $true
    }
    if (-not (Assert-NoReparsePoint -Path $dst -StopAt $Root)) { return $false }
    $dir = Split-Path -Path $dst -Parent
    if (-not (Test-Path -LiteralPath $dir -PathType Container)) {
        New-Item -ItemType Directory -Path $dir -Force | Out-Null
    }
    $had = Test-Path -LiteralPath $dst -PathType Leaf
    if ($had) {
        $old = '?'
        try {
            $oldItem = Get-Item -LiteralPath $dst -Force
            $old = '{0} bytes, {1}' -f $oldItem.Length, (Get-FileHash -LiteralPath $dst -Algorithm SHA256).Hash
        } catch { }
        Write-Step WARN ((Get-Text 'UnityLibsRepair') -f $script:UnityLibsTarget, ('{0} bytes, {1}' -f $Size, $Sha256), $old)
    }
    Copy-Item -LiteralPath $ZipPath -Destination $dst -Force
    if ((Get-BridgeFileState -Path $dst -Sha256 $Sha256 -Size $Size) -ne 'ok') {
        Write-Step FAIL ((Get-Text 'UnityLibsBad') -f $script:UnityLibsTarget)
        return $false
    }
    if (-not $had) { Write-Step OK ((Get-Text 'UnityLibsOk') -f $script:UnityLibsTarget) }
    return $true
}

function Assert-GameNotRunning {
    <# The loader and plugin files are held open by the game; never kill it. #>
    $procs = @()
    try { $procs = @(Get-Process -Name 'Malody V', 'MalodyV' -ErrorAction SilentlyContinue) } catch { $procs = @() }
    if ($procs.Count -gt 0) {
        $what = ($procs | ForEach-Object { '{0}({1})' -f $_.ProcessName, $_.Id }) -join ', '
        Write-Step FAIL ((Get-Text 'GameRunning') -f $what)
        Write-Host (Get-Text 'GameRunningTip') -ForegroundColor Gray
        return $false
    }
    Write-Step OK (Get-Text 'GameNotRunning')
    return $true
}

function Get-ZipEntryPlan {
    # Read-only enumeration of the loader archive. The vendored
    # loader-manifest.bepinex-6.0.0-be.788.json is the only whitelist: a file
    # entry that is not listed there is rejected. Independently of the manifest,
    # names are rejected when empty, or when they carry a ".." segment, a drive
    # letter, an absolute path, a control character, or collide with an earlier
    # entry after normalization (\ -> /, lowercase, trailing slash dropped) -
    # on Windows winhttp.dll and WINHTTP.DLL would otherwise overwrite each
    # other. Directory entries are markers only (extraction creates directories
    # on demand, the manifest lists files), so they are validated then ignored;
    # BepInEx/plugins/** is never in the manifest and is rejected by
    # construction (our own plugin DLL is copied separately). Every violation
    # is reported, then $null is returned.
    param([string]$ZipPath)
    Initialize-ZipAssembly
    $manifestPath = Join-Path $PSScriptRoot $script:LoaderManifestRel
    if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
        Write-Step FAIL ((Get-Text 'ManifestNo') -f $manifestPath)
        return $null
    }
    $manifest = @()
    try {
        # foreach, not @(... | ConvertFrom-Json): Windows PowerShell 5.1 hands a
        # JSON array back as a single object, so @() would keep one nested array
        $parsed = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
        foreach ($m in $parsed) { $manifest += $m }
    } catch {
        Write-Step FAIL ((Get-Text 'ManifestBad') -f $manifestPath)
        return $null
    }
    $allowed = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::Ordinal)
    $records = [System.Collections.Generic.Dictionary[string, object]]::new([System.StringComparer]::Ordinal)
    foreach ($m in $manifest) {
        $rel = [string]$m.path
        [void]$allowed.Add($rel)
        $records[$rel] = $m
    }

    $zip = $null
    try { $zip = [System.IO.Compression.ZipFile]::OpenRead($ZipPath) } catch {
        Write-Step FAIL ((Get-Text 'ZipOpenFail') -f $ZipPath, $_.Exception.Message)
        return $null
    }
    $plan = New-Object 'System.Collections.Generic.List[object]'
    $seen = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
    $bad = 0
    try {
        foreach ($entry in $zip.Entries) {
            $name = [string]$entry.FullName
            $reasonKey = ''
            if ([string]::IsNullOrWhiteSpace($name)) {
                $reasonKey = 'ZipReasonEmpty'
            } elseif ($name -match '(^|[\\/])\.\.([\\/]|$)') {
                $reasonKey = 'ZipReasonDotDot'
            } elseif ($name -match '^[A-Za-z]:') {
                $reasonKey = 'ZipReasonDrive'
            } elseif ($name.StartsWith('/') -or $name.StartsWith('\')) {
                $reasonKey = 'ZipReasonAbsolute'
            } elseif ($name -match '[\x00-\x1f]') {
                $reasonKey = 'ZipReasonControl'
            }
            if ($reasonKey) {
                Write-Step FAIL ((Get-Text 'ZipEntryBad') -f $name, (Get-Text $reasonKey))
                $bad++
                continue
            }
            if ($name.EndsWith('/')) { continue }   # directory marker
            $norm = $name.Replace('\', '/').ToLowerInvariant().TrimEnd('/')
            if (-not $seen.Add($norm)) {
                Write-Step FAIL ((Get-Text 'ZipEntryBad') -f $name, (Get-Text 'ZipReasonDuplicate'))
                $bad++
                continue
            }
            if (-not $allowed.Contains($name)) {
                Write-Step FAIL ((Get-Text 'ZipEntryBad') -f $name, (Get-Text 'ZipReasonNotListed'))
                $bad++
                continue
            }
            $plan.Add([pscustomobject]@{
                Path   = $name
                Size   = [long]$records[$name].size
                Sha256 = [string]$records[$name].sha256
            })
        }
    } finally {
        $zip.Dispose()
    }
    if ($bad -gt 0) {
        Write-Step FAIL ((Get-Text 'ZipRejected') -f $bad)
        return $null
    }
    if ($plan.Count -ne $manifest.Count) {
        Write-Step FAIL ((Get-Text 'ZipPlanShort') -f $plan.Count, $manifest.Count)
        return $null
    }
    Write-Step OK ((Get-Text 'ZipPlanOk') -f $plan.Count)
    return $plan.ToArray()
}

function Assert-InstallPlan {
    # Four-state decision from the installer's own manifest:
    #   clean   - no BepInEx\ and no winhttp.dll            -> install normally
    #   ours    - manifest present, every kept file matches -> loader all SKIP
    #   partial - manifest present, files missing/modified  -> resume
    #   foreign - BepInEx\ or winhttp.dll, no manifest      -> FAIL, touch nothing
    # Returns $null for 'foreign', else a hashtable with the files already in
    # place and the paths the old manifest claims as created by us.
    param([string]$Root, [object[]]$Plan)
    $hasBepInEx = Test-Path -LiteralPath (Join-Path $Root 'BepInEx') -PathType Container
    $hasWinhttp = Test-Path -LiteralPath (Join-Path $Root 'winhttp.dll') -PathType Leaf
    $mf = Read-BridgeManifest -Root $Root
    if (-not $mf) {
        if (-not $hasBepInEx -and -not $hasWinhttp) {
            Write-Step OK (Get-Text 'PlanClean')
            return @{ State = 'clean'; InPlace = @(); CreatedPaths = @() }
        }
        Write-Step FAIL (Get-Text 'PlanExternal')
        Write-Host (Get-Text 'PlanExternalHint') -ForegroundColor Yellow
        foreach ($rel in $script:BridgeForeignRel) {
            $p = Join-BridgeTarget -Root $Root -Rel $rel
            if (Test-Path -LiteralPath $p) { Write-Host ('  ' + $p) -ForegroundColor Yellow }
        }
        Write-Host (Get-Text 'PlanExternalTip') -ForegroundColor Gray
        return $null
    }
    $missing = 0
    $mismatch = 0
    $inPlace = New-Object 'System.Collections.Generic.List[object]'
    foreach ($e in $Plan) {
        $p = Join-BridgeTarget -Root $Root -Rel $e.Path
        $state = Get-BridgeFileState -Path $p -Sha256 $e.Sha256 -Size $e.Size
        if ($state -eq 'ok') {
            $inPlace.Add([pscustomobject]@{ Path = $e.Path; Sha256 = $e.Sha256 })
            continue
        }
        if ($state -eq 'missing') { $missing++; continue }
        $actual = '?'
        try { $actual = (Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash } catch { }
        Write-Step WARN ((Get-Text 'PlanMismatch') -f $e.Path, $e.Sha256, $actual)
        $mismatch++
    }
    $createdPaths = @(@($mf.created) | ForEach-Object { [string]$_.path })
    $inPlaceArray = @($inPlace.ToArray())
    if ($missing -eq 0 -and $mismatch -eq 0) {
        Write-Step OK (Get-Text 'PlanOurs')
        return @{ State = 'ours'; InPlace = $inPlaceArray; CreatedPaths = $createdPaths }
    }
    Write-Step SKIP ((Get-Text 'PlanPartial') -f $missing, $mismatch)
    return @{ State = 'partial'; InPlace = $inPlaceArray; CreatedPaths = $createdPaths }
}

function Read-BridgeManifest {
    <# $null when there is no (or no readable) manifest of ours in that root. #>
    param([string]$Root)
    $path = Join-Path $Root $script:BridgeManifestRel
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { return $null }
    try { $obj = Get-Content -LiteralPath $path -Raw | ConvertFrom-Json } catch { return $null }
    if (-not $obj -or -not $obj.PSObject.Properties['kept']) { return $null }
    return $obj
}

function Write-BridgeManifest {
    # Write {root}\BepInEx\bepinex-install.json. The section mapping is fixed:
    # created = text files we created (by path, no hash), installed = plugin
    # files we placed (hash), kept = everything else the loader needs (hash; the
    # archive entries plus the vendored Unity reference assemblies - part of our
    # install but never deleted on uninstall).
    param([string]$Root, [object[]]$Created, [object[]]$Installed, [object[]]$Kept)
    $path = Join-Path $Root $script:BridgeManifestRel
    $dir = Split-Path -Path $path -Parent
    if (-not (Test-Path -LiteralPath $dir -PathType Container)) {
        New-Item -ItemType Directory -Path $dir -Force | Out-Null
    }
    $data = [ordered]@{
        installed_at = (Get-Date).ToString('yyyy-MM-ddTHH:mm:sszzz')
        created      = @($Created)
        installed    = @($Installed)
        kept         = @($Kept)
    }
    $json = $data | ConvertTo-Json -Depth 6
    [System.IO.File]::WriteAllText($path, $json, (New-Object System.Text.UTF8Encoding($false)))
    Write-Step OK ((Get-Text 'ManifestWrote') -f $path)
}

function Expand-BridgeLoader {
    # Extract the loader archive into {root}\.mma-bepinex-stage-<pid>\ (inside
    # the game directory, so nothing crosses a volume boundary), verify size +
    # SHA256 of every extracted file against the manifest, and only then move
    # each file into place: a failure before the first move leaves the game
    # directory as it was. Files listed in $InPlace are skipped (resume). The
    # caller writes the manifest before calling this, so a failure during the
    # moving phase is recognised as "partially ours" on the next run.
    param(
        [string]$Root,
        [string]$ZipPath,
        [object[]]$Plan,
        [object[]]$InPlace
    )
    Initialize-ZipAssembly
    $stage = Join-Path $Root ('.mma-bepinex-stage-{0}' -f $PID)
    if (-not (Assert-NoReparsePoint -Path $stage -StopAt $Root)) { return $false }
    try {
        New-Item -ItemType Directory -Path $stage -Force | Out-Null
        $zip = $null
        try {
            $zip = [System.IO.Compression.ZipFile]::OpenRead($ZipPath)
            foreach ($e in $Plan) {
                $entry = $zip.GetEntry($e.Path)
                if (-not $entry) {
                    Write-Step FAIL ((Get-Text 'ZipEntryGone') -f $e.Path)
                    return $false
                }
                $staged = Join-BridgeTarget -Root $stage -Rel $e.Path
                $dir = Split-Path -Path $staged -Parent
                if (-not (Test-Path -LiteralPath $dir -PathType Container)) {
                    New-Item -ItemType Directory -Path $dir -Force | Out-Null
                }
                $in = $entry.Open()
                try {
                    $out = [System.IO.File]::Create($staged)
                    try { $in.CopyTo($out) } finally { $out.Dispose() }
                } finally { $in.Dispose() }
            }
        } catch {
            Write-Step FAIL ((Get-Text 'ZipOpenFail') -f $ZipPath, $_.Exception.Message)
            return $false
        } finally {
            if ($zip) { $zip.Dispose() }
        }
        Write-Step OK ((Get-Text 'StageExtracted') -f $Plan.Count)

        $bad = 0
        foreach ($e in $Plan) {
            $staged = Join-BridgeTarget -Root $stage -Rel $e.Path
            if ((Get-BridgeFileState -Path $staged -Sha256 $e.Sha256 -Size $e.Size) -ne 'ok') {
                Write-Step FAIL ((Get-Text 'StageVerifyBad') -f $e.Path)
                $bad++
            }
        }
        if ($bad -gt 0) {
            Write-Step FAIL ((Get-Text 'StageVerifyFail') -f $bad)
            return $false
        }
        Write-Step OK ((Get-Text 'StageVerified') -f $Plan.Count)

        $skip = @{}
        foreach ($s in $InPlace) { $skip[[string]$s.Path] = $true }
        $placed = 0
        $keptCount = 0
        foreach ($e in $Plan) {
            if ($skip.ContainsKey($e.Path)) {
                Write-Step SKIP ((Get-Text 'LoaderInPlace') -f $e.Path)
                $keptCount++
                continue
            }
            $target = Join-BridgeTarget -Root $Root -Rel $e.Path
            $dir = Split-Path -Path $target -Parent
            if (-not (Test-Path -LiteralPath $dir -PathType Container)) {
                New-Item -ItemType Directory -Path $dir -Force | Out-Null
            }
            try {
                Move-Item -LiteralPath (Join-BridgeTarget -Root $stage -Rel $e.Path) -Destination $target -Force
            } catch {
                Write-Step FAIL ((Get-Text 'MoveFail') -f $e.Path, $_.Exception.Message)
                return $false
            }
            Write-Step OK ((Get-Text 'LoaderPlaced') -f $e.Path)
            $placed++
        }
        Write-Step OK ((Get-Text 'LoaderPlacedCount') -f $placed, $keptCount)
        return $true
    } finally {
        if (Test-Path -LiteralPath $stage -PathType Container) {
            # our own staging folder, a uniquely named direct child of the game root
            Remove-Item -LiteralPath $stage -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
}

function Install-MalodyBridgeDll {
    <# Copy our plugin into BepInEx\plugins\MalodyInsight\ and re-verify it. #>
    param([string]$Root)
    $src = Join-Path $PSScriptRoot $script:BridgeDllRel
    if (-not (Test-Path -LiteralPath $src -PathType Leaf)) {
        Write-Step FAIL ((Get-Text 'DllMissing') -f $src)
        return $null
    }
    $srcHash = (Get-FileHash -LiteralPath $src -Algorithm SHA256).Hash
    $srcSize = (Get-Item -LiteralPath $src -Force).Length
    $rec = [pscustomobject]@{ path = $script:BridgeDllTarget; size = [long]$srcSize; sha256 = $srcHash }
    $dst = Join-BridgeTarget -Root $Root -Rel $script:BridgeDllTarget
    if ((Get-BridgeFileState -Path $dst -Sha256 $srcHash -Size $srcSize) -eq 'ok') {
        Write-Step SKIP ((Get-Text 'DllSkip') -f $script:BridgeDllTarget)
        return $rec
    }
    if (-not (Assert-NoReparsePoint -Path $dst -StopAt $Root)) { return $null }
    $dir = Split-Path -Path $dst -Parent
    if (-not (Test-Path -LiteralPath $dir -PathType Container)) {
        New-Item -ItemType Directory -Path $dir -Force | Out-Null
    }
    Copy-Item -LiteralPath $src -Destination $dst -Force
    if ((Get-BridgeFileState -Path $dst -Sha256 $srcHash -Size $srcSize) -ne 'ok') {
        Write-Step FAIL ((Get-Text 'DllVerifyBad') -f $script:BridgeDllTarget)
        return $null
    }
    Write-Step OK ((Get-Text 'DllOk') -f $script:BridgeDllTarget)
    return $rec
}

function Test-BridgeUpstreamPresent {
    # The upstream bridge (a different plugin, GUID local.malody.insight.selection)
    # lives at the same folder as ours. Both hook the same methods, so having both
    # installed double-hooks the game. We only ever REPORT it: deleting somebody
    # else's file is the user's call, never ours. Returns $true when it is present.
    param([string]$Root)
    $rel = $script:BridgeUpstreamRel
    $full = Join-BridgeTarget -Root $Root -Rel $rel
    if (-not (Test-Path -LiteralPath $full -PathType Leaf)) { return $false }
    Write-Step FAIL ((Get-Text 'UpstreamPresent') -f $rel)
    Write-Host (Get-Text 'UpstreamPresentTip') -ForegroundColor Yellow
    Write-Host ("         " + $full) -ForegroundColor Gray
    return $true
}

function Install-MalodyBepInExBridge {
    <# -LoaderOnly installs (or leaves) the BepInEx loader and stops before the
       plugin DLL, so the game can be launched once to generate
       BepInEx\interop\ (which the plugin is compiled against) and so the loader
       is present before the plugin exists locally. #>
    param([switch]$LoaderOnly)
    Write-Host ''
    Write-Host (Get-Text 'HeaderMalodyBridge') -ForegroundColor Cyan
    if ($LoaderOnly) { Write-Host (Get-Text 'LoaderOnlyMode') -ForegroundColor Gray }
    $cands = Get-MalodyCandidates
    $root = Select-GameRoot -Game 'MalodyBridge' -Candidates $cands -ForceRoot $Root
    if (-not $root) { return }

    # hard gate 1: no junction/symlink on the root path itself
    if (-not (Assert-NoReparsePoint -Path $root)) {
        Write-Step FAIL (Get-Text 'MalodyBridgeNot')
        return
    }
    # informational: is this really a Malody V folder? (a dummy root still works)
    [void](Test-MalodyBridgeRoot -Root $root)
    # hard gate 2: the game must be closed
    if (-not (Assert-GameNotRunning)) {
        Write-Step FAIL (Get-Text 'MalodyBridgeNot')
        return
    }
    # read-only: archive location, archive hash, entry plan
    $zip = Resolve-LoaderZip
    if (-not $zip) {
        Write-Step FAIL (Get-Text 'MalodyBridgeNot')
        return
    }
    if (-not (Assert-LoaderZipHash -Path $zip)) {
        Write-Step FAIL (Get-Text 'MalodyBridgeNot')
        return
    }
    $plan = Get-ZipEntryPlan -ZipPath $zip
    if (-not $plan) {
        Write-Step FAIL (Get-Text 'MalodyBridgeNot')
        return
    }
    # read-only: the vendored Unity reference assemblies, so the loader's first
    # launch never has to download them (see $script:UnityLibsZipRel)
    $ulZip = Resolve-UnityLibsZip
    if (-not $ulZip) {
        Write-Step FAIL (Get-Text 'MalodyBridgeNot')
        return
    }
    if (-not (Assert-UnityLibsZipHash -Path $ulZip)) {
        Write-Step FAIL (Get-Text 'MalodyBridgeNot')
        return
    }
    $ulSize = (Get-Item -LiteralPath $ulZip -Force).Length
    $ulHash = (Get-FileHash -LiteralPath $ulZip -Algorithm SHA256).Hash
    # hard gate 3: every path we are about to write, checked before the first write
    $targets = @($script:BridgeManifestRel, $script:BridgeDllTarget, $script:BridgeCfgTarget, $script:UnityLibsTarget)
    foreach ($e in $plan) { $targets += ($e.Path -replace '/', '\') }
    foreach ($rel in $targets) {
        if (-not (Assert-NoReparsePoint -Path (Join-BridgeTarget -Root $root -Rel $rel) -StopAt $root)) {
            Write-Step FAIL (Get-Text 'MalodyBridgeNot')
            return
        }
    }
    # four-state decision against our own manifest
    $state = Assert-InstallPlan -Root $root -Plan $plan
    if (-not $state) {
        Write-Step FAIL (Get-Text 'MalodyBridgeNot')
        return
    }

    $dllSrc = Join-Path $PSScriptRoot $script:BridgeDllRel
    $dllHash = $null
    if (-not $LoaderOnly) {
        if (-not (Test-Path -LiteralPath $dllSrc -PathType Leaf)) {
            Write-Step FAIL ((Get-Text 'DllMissing') -f $dllSrc)
            return
        }
        $dllHash = (Get-FileHash -LiteralPath $dllSrc -Algorithm SHA256).Hash
        $dllSize = (Get-Item -LiteralPath $dllSrc -Force).Length
    }
    $kept = @($plan | ForEach-Object {
        [pscustomobject]@{ path = $_.Path; size = [long]$_.Size; sha256 = [string]$_.Sha256 }
    })
    $kept += [pscustomobject]@{ path = $script:UnityLibsTarget; size = [long]$ulSize; sha256 = $ulHash }
    if ($LoaderOnly) {
        # nothing of ours beyond the loader: an empty installed/created pair
        # keeps the record honest, and -Uninstall then removes nothing but the
        # loader-free residual manifest
        $installed = @()
        $created = @()
    } else {
        $installed = @([pscustomobject]@{ path = $script:BridgeDllTarget; size = [long]$dllSize; sha256 = $dllHash })
        $created = @()
    }

    # manifest first, then the moves: an interrupted run is recognised as
    # "partially ours" and resumed instead of being mistaken for a foreign install
    Write-BridgeManifest -Root $root -Created $created -Installed $installed -Kept $kept

    if (-not (Expand-BridgeLoader -Root $root -ZipPath $zip -Plan $plan -InPlace $state.InPlace)) {
        Write-Step FAIL (Get-Text 'MalodyBridgeNot')
        Write-Host (Get-Text 'BridgeResumeTip') -ForegroundColor Gray
        return
    }
    if (-not (Install-UnityLibsZip -Root $root -ZipPath $ulZip -Size $ulSize -Sha256 $ulHash)) {
        Write-Step FAIL (Get-Text 'MalodyBridgeNot')
        return
    }
    if ($LoaderOnly) {
        Write-Step SKIP (Get-Text 'LoaderOnlyNoDll')
    } else {
        # report a previously installed upstream bridge before touching anything:
        # two plugins hooking the same methods would double-hook the game
        [void](Test-BridgeUpstreamPresent -Root $root)
        $dllRec = Install-MalodyBridgeDll -Root $root
        if (-not $dllRec) {
            Write-Step FAIL (Get-Text 'MalodyBridgeNot')
            return
        }
        # no cfg is written any more: the fork has no overlay section, and BepInEx
        # generates its own config on first load (BridgeCfgTarget is never created)
        Write-BridgeManifest -Root $root -Created @() -Installed @($dllRec) -Kept $kept
    }

    # final re-verification before reporting success
    $bad = 0
    foreach ($e in $plan) {
        $st = Get-BridgeFileState -Path (Join-BridgeTarget -Root $root -Rel $e.Path) -Sha256 $e.Sha256 -Size $e.Size
        if ($st -ne 'ok') {
            Write-Step FAIL ((Get-Text 'VerifyLoaderBad') -f $e.Path, $st)
            $bad++
        }
    }
    if ($bad -gt 0) {
        Write-Step FAIL (Get-Text 'MalodyBridgeNot')
        return
    }
    if ((Get-BridgeFileState -Path (Join-BridgeTarget -Root $root -Rel $script:UnityLibsTarget) -Sha256 $ulHash -Size $ulSize) -ne 'ok') {
        Write-Step FAIL ((Get-Text 'UnityLibsVerifyBad') -f $script:UnityLibsTarget)
        Write-Step FAIL (Get-Text 'MalodyBridgeNot')
        return
    }
    if ($LoaderOnly) {
        Write-Step OK ((Get-Text 'VerifyOkLoaderOnly') -f $plan.Count)
        Write-Step OK (Get-Text 'LoaderOnlyOk')
        Write-Host (Get-Text 'LoaderOnlyTip') -ForegroundColor Gray
        return
    }
    if ((Get-BridgeFileState -Path (Join-BridgeTarget -Root $root -Rel $script:BridgeDllTarget) -Sha256 $dllHash) -ne 'ok') {
        Write-Step FAIL ((Get-Text 'VerifyBad') -f $script:BridgeDllTarget)
        Write-Step FAIL (Get-Text 'MalodyBridgeNot')
        return
    }
    Write-Step OK ((Get-Text 'VerifyOk') -f $plan.Count)
    Write-Host ''
    Write-Step OK (Get-Text 'MalodyBridgeOk')
    Write-Host (Get-Text 'MalodyBridgeTip') -ForegroundColor Gray
}

function Uninstall-MalodyBepInExBridge {
    Write-Host ''
    Write-Host (Get-Text 'RmMalodyBridge') -ForegroundColor Cyan
    $root = Select-ExistingRoot -Game 'MalodyBridge' -ConfigKey 'malodyRoot' -Validator { param($p) Test-MalodyRoot $p }
    if (-not $root) { return }

    $mf = Read-BridgeManifest -Root $root
    if (-not $mf) {
        Write-Step SKIP ((Get-Text 'UnNoRecord') -f (Join-Path $root $script:BridgeManifestRel))
        return
    }
    if (-not (Assert-NoReparsePoint -Path $root)) {
        Write-Step FAIL (Get-Text 'MalodyBridgeNot')
        return
    }

    # installed: plugin files, deleted only while the hash still matches
    foreach ($rec in @($mf.installed)) {
        if (-not $rec) { continue }
        $rel = [string]$rec.path
        if (-not $rel) { continue }
        $full = Join-BridgeTarget -Root $root -Rel $rel
        if (-not (Assert-NoReparsePoint -Path $full -StopAt $root)) {
            Write-Step FAIL (Get-Text 'MalodyBridgeNot')
            return
        }
        switch (Get-BridgeFileState -Path $full -Sha256 ([string]$rec.sha256)) {
            'missing'  { Write-Step SKIP ((Get-Text 'NotPresent') -f $rel) }
            'mismatch' { Write-Step SKIP ((Get-Text 'UnHashDiff') -f $rel) }
            'ok'       {
                Remove-Item -LiteralPath $full -Force
                Write-Step OK ((Get-Text 'Deleted') -f $rel)
            }
        }
    }

    # created: text files of ours, deleted by path (never hash-compared -
    # BepInEx rewrites that cfg itself, so its hash always differs)
    foreach ($rec in @($mf.created)) {
        if (-not $rec) { continue }
        $rel = [string]$rec.path
        if (-not $rel) { continue }
        $full = Join-BridgeTarget -Root $root -Rel $rel
        if (-not (Assert-NoReparsePoint -Path $full -StopAt $root)) {
            Write-Step FAIL (Get-Text 'MalodyBridgeNot')
            return
        }
        if (Test-Path -LiteralPath $full -PathType Leaf) {
            Remove-Item -LiteralPath $full -Force
            Write-Step OK ((Get-Text 'Deleted') -f $rel)
        } else {
            Write-Step SKIP ((Get-Text 'NotPresent') -f $rel)
        }
    }

    # kept: the loader itself stays; the residual manifest keeps recognising it
    $kept = @($mf.kept)
    Write-Step SKIP ((Get-Text 'UnKeptNote') -f $kept.Count)
    Write-Host (Get-Text 'UnKeptTip') -ForegroundColor Gray
    Write-BridgeManifest -Root $root -Created @() -Installed @() -Kept $kept
    Write-Step OK (Get-Text 'MalodyBridgeGone')
}

# ---------------------------------------------------------------------------
# main flow
# ---------------------------------------------------------------------------

function Invoke-GameFlow {
    param([string]$G, [bool]$Remove, [bool]$Loader)
    switch ($G) {
        'Etterna'      { if ($Remove) { Uninstall-EtternaBridge } else { Install-EtternaBridge } }
        'Malody'       { if ($Remove) { Uninstall-MalodyBridge } else { Install-MalodyBridge } }
        'Malody4'      { if ($Remove) { Uninstall-Malody4Bridge } else { Install-Malody4Bridge } }
        'MalodyBridge' { if ($Remove) { Uninstall-MalodyBepInExBridge } else { Install-MalodyBepInExBridge -LoaderOnly:$Loader } }
        'Both'         {
            if ($Remove) {
                Uninstall-MalodyBridge
                Uninstall-MalodyBepInExBridge
            } else {
                Install-MalodyBridge
                Install-MalodyBepInExBridge -LoaderOnly:$Loader
            }
        }
    }
}

# A test harness may dot-source this file for the functions above; running it
# normally (install-bridge.bat uses -File) still enters the menu below.
if ($MyInvocation.InvocationName -eq '.') { return }

Show-Banner

if ($Game) {
    # -LoaderOnly only means something for the BepInEx-based Malody V bridge;
    # drop it elsewhere instead of silently ignoring it
    if ($LoaderOnly -and ($Game -ne 'MalodyBridge') -and ($Game -ne 'Both')) {
        Write-Step SKIP ((Get-Text 'LoaderOnlyN_A') -f $Game)
        $LoaderOnly = $false
    }
    Invoke-GameFlow -G $Game -Remove ([bool]$Uninstall) -Loader ([bool]$LoaderOnly)
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
            -Options @('Etterna', 'Malody V', 'Malody 4') -Extra (Get-Text 'BackToMenu')
        if ($gameItem -eq 3) { continue }
        if ($gameItem -eq 1) {
            # Malody V carries two independent bridges: the Lua editor plugin and
            # the BepInEx in-game song-selection bridge
            $vPick = Read-Option -Title (Get-Text 'ChooseMalodyV') -Options @(
                (Get-Text 'MalodyVLua'),
                (Get-Text 'MalodyVBepInEx'),
                (Get-Text 'MalodyVBoth')
            )
            # not $game: that name is the -Game parameter (PowerShell variable
            # names are case-insensitive) and 'Both' is deliberately not one of
            # its ValidateSet values
            $gameKey = switch ($vPick) {
                0 { 'Malody' }
                1 { 'MalodyBridge' }
                2 { 'Both' }
            }
        } else {
            $gameKey = switch ($gameItem) {
                0 { 'Etterna' }
                2 { 'Malody4' }
            }
        }
        Invoke-GameFlow -G $gameKey -Remove $removeMode -Loader ([bool]$LoaderOnly)
        if (-not $Yes) {
            if (-not (Confirm-YesNo (Get-Text 'ContinueAsk') -DefaultYes $false)) { break }
        }
    }
}

Write-Host ''
Write-Host (Get-Text 'Done') -ForegroundColor Green
