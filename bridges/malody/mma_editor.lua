-- MMA Analyze 编辑器插件（MalodyV，触发类 PluginType=0）。
--
-- 用法：打开谱面编辑器 → 「更多」菜单点击 Analyze → 自动读取当前谱面并
-- POST 到本地壳（127.0.0.1:24060）→ 分析结果以 AddText 卡片渲染在编辑区。
--
-- 自动读谱：优先走壳的 resolve 通道（POST {"action":"resolve", title, artist}，
-- 壳按 ChartInfo 标题扫描 {malodyRoot}/chart/ 下的 .mc）；失败再尝试
-- ReadFileSelect / chart.mc / 手动输入。所有反馈经 AddText 输出，不弹模态
-- 消息（避免阻塞编辑器交互）。
--
-- 安装：把本文件放到 MalodyV/Editor/ 目录（目录不存在则创建），
-- 客户端重启或插件面板重载后即可在菜单见到。
--
-- 注：Editor:DoRequest / ReadFileSelect 的精确签名无官方文档（真机验证项）；
-- 本脚本对二者均以 pcall 包裹，失败走输入兜底，不会阻断编辑器。

PluginName = 'MMA Analyze'
PluginMode = 0
PluginType = 0
PluginRequire = '5.0.1'

local ENDPOINT = 'http://127.0.0.1:24060/'

local function jsonQuote(s)
    s = tostring(s or '')
    s = s:gsub('\\', '\\\\'):gsub('"', '\\"'):gsub('\n', '\\n'):gsub('\r', '')
    return '"' .. s .. '"'
end

local function trim(s, n)
    s = tostring(s or '')
    if #s > n then
        return s:sub(1, n) .. '…'
    end
    return s
end

local function note(text)
    -- 非模态反馈：写入编辑区文本卡片；不弹窗。
    pcall(function()
        Editor:AddText('mma_result', 'MMA: ' .. text)
    end)
end

local function looksLikeChart(text)
    return text and text ~= '' and text:find('"song"', 1, true) ~= nil
end

local function postAndRender(meta, chartText)
    if not looksLikeChart(chartText) then
        note('未读到谱面（resolve 未命中；可手动输入文件名重试）')
        return
    end
    local payload = '{"meta":{"title":' .. jsonQuote(meta.title)
        .. ',"artist":' .. jsonQuote(meta.artist)
        .. ',"level":' .. jsonQuote(meta.level)
        .. ',"keys":' .. tostring(meta.keys or 0)
        .. '},"chartText":' .. jsonQuote(chartText) .. '}'
    pcall(function()
        Editor:DoRequest(ENDPOINT, 'POST', payload, function(response)
            note(trim(response, 2000))
        end)
    end)
end

local function tryRead(name)
    local chartText = nil
    pcall(function()
        chartText = Editor:ReadFile(name)
    end)
    if looksLikeChart(chartText) then
        return chartText
    end
    return nil
end

local function manualInput(meta)
    -- 1) 文件选择器（签名未证实 → pcall）
    local ok, picked = pcall(function()
        return Editor:ReadFileSelect()
    end)
    if ok and looksLikeChart(picked) then
        postAndRender(meta, picked)
        return
    end
    -- 2) 旧版默认名 / 常见相对路径
    local cand = tryRead('chart.mc')
    if cand then
        postAndRender(meta, cand)
        return
    end
    -- 3) 手动输入（双轨：同步返回值或回调式；输入框由引擎管理，ESC 可取消）
    local got = nil
    local syncOk = pcall(function()
        got = Editor:GetUserInput('chart 目录内 .mc 文件名（如 0/xxx.mc 或 chart.mc）', '')
    end)
    if syncOk and type(got) == 'string' and got ~= '' then
        local chartText = tryRead(got)
        if chartText then
            postAndRender(meta, chartText)
        else
            note('未读到谱面：' .. got)
        end
        return
    end
    pcall(function()
        Editor:GetUserInput('chart 目录内 .mc 文件名（如 0/xxx.mc 或 chart.mc）', '', function(name)
            if name and name ~= '' then
                local chartText = tryRead(name)
                if chartText then
                    postAndRender(meta, chartText)
                else
                    note('未读到谱面：' .. name)
                end
            end
        end)
    end)
end

function Run()
    local meta = {
        title = '',
        artist = '',
        level = '',
        keys = 0,
    }
    pcall(function()
        meta.title = Editor:ChartInfo('title') or ''
        meta.artist = Editor:ChartInfo('artist') or ''
        meta.level = Editor:ChartInfo('level') or ''
        meta.keys = tonumber(Editor:ChartInfo('key')) or 0
    end)

    -- 自动读谱：resolve 通道（壳按标题扫 chart 目录）。
    local dispatched = false
    local payload = '{"action":"resolve","title":' .. jsonQuote(meta.title)
        .. ',"artist":' .. jsonQuote(meta.artist) .. '}'
    pcall(function()
        Editor:DoRequest(ENDPOINT, 'POST', payload, function(response)
            dispatched = true
            if looksLikeChart(response) then
                postAndRender(meta, response)
            else
                manualInput(meta)
            end
        end)
    end)
    if not dispatched then
        manualInput(meta)
    end
end