-- MMA Analyze 编辑器插件（MalodyV，触发类 PluginType=0）。
--
-- 用法：打开谱面编辑器 → 「更多」菜单点击 Analyze → 自动读取当前谱面并
-- POST 到本地壳（127.0.0.1:24060）→ 分析结果以 AddText 卡片渲染在编辑区。
--
-- 响应机制（官方 changelog 6.6.42+ 证实）：Editor:DoRequest 发起请求，
-- **响应经插件全局函数 OnResponse 回调**（不是 DoRequest 的第四参；Lua 对
-- 多余参数静默丢弃 → 之前的 callback 形参被忽略）。本脚本：
--   * 定义 OnResponse(...)，按 url 区分 resolve（…/resolve）与 analyze（…/）；
--   * DoRequest 用 3 参（url, method, body）发起；pcall 失败再试 4 参兼容；
--   * 发起后短等待响应，超时进入手动输入兜底（不阻塞编辑器交互，全部
--     反馈经 AddText 非模态输出）。
--
-- 安装：把本文件放到 MalodyV/Editor/（或 editor/）目录（目录不存在则创建）。

PluginName = 'MMA Analyze'
PluginMode = 0
PluginType = 0
PluginRequire = '5.0.1'

local ENDPOINT = 'http://127.0.0.1:24060/'
local RESOLVE_ENDPOINT = 'http://127.0.0.1:24060/resolve'

local PENDING = {}

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
    pcall(function()
        Editor:AddText('mma_result', 'MMA: ' .. text)
    end)
end

local function looksLikeChart(text)
    return text and text ~= '' and text:find('"song"', 1, true) ~= nil
end

local function shortWait(seconds)
    local slept = pcall(function()
        os.sleep(seconds)
    end)
    if not slept then
        local ok, t0 = pcall(function()
            return os.time()
        end)
        if ok then
            while os.time() - t0 < seconds * 1000 do
                -- 忙等兜底（os.sleep 不可用时）
            end
        end
    end
end

local function postAnalyze(meta, chartText)
    local payload = '{"meta":{"title":' .. jsonQuote(meta.title)
        .. ',"artist":' .. jsonQuote(meta.artist)
        .. ',"level":' .. jsonQuote(meta.level)
        .. ',"keys":' .. tostring(meta.keys or 0)
        .. '},"chartText":' .. jsonQuote(chartText) .. '}'
    PENDING.mode = 'analyze'
    local okA, errA = pcall(function()
        Editor:DoRequest(ENDPOINT, 'POST', payload)
    end)
    if not okA then
        pcall(function()
            Editor:DoRequest(ENDPOINT, 'POST', payload, function(response)
                note(trim(response, 2000))
            end)
        end)
        note('DoRequest 调用失败：' .. tostring(errA))
    end
end

function OnResponse(...)
    local a1, a2, a3 = ...
    local url
    local status
    local body
    if type(a1) == 'string' and string.find(a1, '24060', 1, true) then
        url = a1
        status = a2
        body = a3
    elseif type(a1) == 'number' then
        status = a1
        body = a2
    else
        body = a1
    end
    if not body or body == '' then
        return
    end
    if url and string.find(url, 'resolve', 1, true) then
        -- resolve 响应：拿到 .mc 原文 → 继续分析
        PENDING.resolved = true
        if looksLikeChart(body) then
            postAnalyze(PENDING.meta or {}, body)
        else
            note('未在 chart 目录找到该谱面文件')
            PENDING.miss = true
        end
    else
        note(trim(body, 2000))
        PENDING.done = true
    end
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
    local ok, picked = pcall(function()
        return Editor:ReadFileSelect()
    end)
    if ok and looksLikeChart(picked) then
        postAnalyze(meta, picked)
        return
    end
    local cand = tryRead('chart.mc')
    if cand then
        postAnalyze(meta, cand)
        return
    end
    local got = nil
    local syncOk = pcall(function()
        got = Editor:GetUserInput('chart 目录内 .mc 文件名（如 0/xxx.mc 或 chart.mc）', '')
    end)
    if not (syncOk and type(got) == 'string' and got ~= '') then
        pcall(function()
            Editor:GetUserInput('chart 目录内 .mc 文件名', '', function(name)
                if name and name ~= '' then
                    got = name
                end
            end)
        end)
    end
    if not (got and got ~= '') then
        note('未输入文件名')
        return
    end
    -- 候选矩阵：全名 / 0/名 / chart/名 / 去扩展名 / chart.mc——每项结果都显示，供真机定位。
    local noExt = (got:gsub('%.mc$', ''))
    local candidates = { got, '0/' .. got, 'chart/' .. got, noExt, 'chart.mc' }
    local report = {}
    for _, name in ipairs(candidates) do
        local chartText = nil
        pcall(function()
            chartText = Editor:ReadFile(name)
        end)
        if looksLikeChart(chartText) then
            report[#report + 1] = name .. '=[OK ' .. #chartText .. 'b]'
            picked = chartText
        else
            report[#report + 1] = name .. '=[x]'
        end
    end
    note('ReadFile 尝试：' .. table.concat(report, ' '))
    if picked then
        postAnalyze(meta, picked)
    end
end

function Run()
    PENDING = {
        meta = {},
        mode = nil,
        resolved = false,
        miss = false,
        done = false,
    }
    local meta = PENDING.meta
    pcall(function()
        meta.title = Editor:ChartInfo('title') or ''
        meta.artist = Editor:ChartInfo('artist') or ''
        meta.level = Editor:ChartInfo('level') or ''
        meta.keys = tonumber(Editor:ChartInfo('key')) or 0
    end)

    -- 自动读谱：resolve 通道（壳按标题扫 chart 目录）→ OnResponse 收响应。
    local payload = '{"action":"resolve","title":' .. jsonQuote(meta.title)
        .. ',"artist":' .. jsonQuote(meta.artist) .. '}'
    local ok, err = pcall(function()
        Editor:DoRequest(RESOLVE_ENDPOINT, 'POST', payload)
    end)
    if not ok then
        note('DoRequest 调用失败（请反馈此信息）：' .. tostring(err))
        manualInput(meta)
        return
    end
    -- 等待响应（约 2s）；超时 → 手动输入兜底。
    local waited = 0
    while waited < 40 and not PENDING.resolved and not PENDING.done and not PENDING.miss do
        shortWait(0.05)
        waited = waited + 1
    end
    if not PENDING.resolved and not PENDING.done then
        manualInput(meta)
    end
end