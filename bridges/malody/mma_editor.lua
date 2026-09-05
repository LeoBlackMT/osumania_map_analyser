-- MMA Analyze 编辑器插件（MalodyV，触发类 PluginType=0）。
--
-- 用法：打开谱面编辑器 → 菜单点击 Analyze → 请求分析当前谱面。
--
-- 通道（文件，全文档化 API）：
--   1. Run()：ChartInfo 取 title/artist/level/key → Editor:WriteFile('mma_request.json')
--      （Malody 自动加谱面名前缀 → <谱面>_mma_request.json）
--   2. 壳扫描到后：谱面本体 = 同目录 <谱面base名>.mc|.osu（base 精确锁定）→
--      分析 → 结果展示在壳窗口卡片（壳即展示端，不回写 txt 到游戏内）。
--   3. 本插件 note 仅提示请求状态；结果请看壳窗口。
--
-- 每步 note() 打点，便于定位卡点。note 内容可换行、不被截断。
--
-- 安装：把本文件放到 MalodyV/editor/（或 Editor/）目录（目录不存在则创建）。

PluginName = 'MMA Analyze'
PluginMode = 0
PluginType = 0
PluginRequire = '5.0.1'

local REQUEST_FILE = 'mma_request.json'

local function jsonQuote(s)
    s = tostring(s or '')
    s = s:gsub('\\', '\\\\'):gsub('"', '\\"'):gsub('\n', '\\n'):gsub('\r', '')
    return '"' .. s .. '"'
end

local function note(text)
    pcall(function()
        Editor:AddText('mma_result', 'MMA: ' .. text)
    end)
end

function Run()
    local meta = {}
    pcall(function()
        meta.title = Editor:ChartInfo('title') or ''
        meta.artist = Editor:ChartInfo('artist') or ''
        meta.level = Editor:ChartInfo('level') or ''
        meta.keys = tonumber(Editor:ChartInfo('key')) or 0
    end)

    local payload = '{"action":"analyze","title":' .. jsonQuote(meta.title)
        .. ',"artist":' .. jsonQuote(meta.artist)
        .. ',"level":' .. jsonQuote(meta.level)
        .. ',"keys":' .. tostring(meta.keys or 0) .. '}'
    local okW, errW = pcall(function()
        Editor:WriteFile(REQUEST_FILE, payload)
    end)
    if not okW then
        note('WriteFile failed: ' .. tostring(errW)
            .. '\nPossible cause: Malody dir not writable (run Malody as admin).')
        return
    end
    -- 验证写入（ReadFile 读回；Malody 加前缀规则同 WriteFile）。
    local verifyOk, verifyContent = pcall(function()
        return Editor:ReadFile(REQUEST_FILE)
    end)
    if not (verifyOk and verifyContent and verifyContent ~= '') then
        note('Request write not effective (dir read-only?).\nRun Malody as admin, or check MalodyV/editor permissions.')
        return
    end
    note('Analysis requested: ' .. (meta.title or '')
        .. '\nResult will appear on the mma-shell window card.')
end
