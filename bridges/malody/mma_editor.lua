-- MMA Analyze 编辑器插件（MalodyV，触发类 PluginType=0）。
--
-- 用法：打开谱面编辑器 → 「更多」菜单点击 Analyze → 自动读取当前谱面并
-- POST 到本地壳（127.0.0.1:24060）→ 分析结果以 AddText 卡片渲染在编辑区。
--
-- 流程（零输入框、零 ReadFile——不依赖不可靠的输入回调）：
--   1. Editor:ChartInfo 取 title/artist/level/key
--   2. Editor:DoRequest → 壳 /resolve（壳按标题扫 {malodyRoot}/chart/** 返回 .mc 原文）
--   3. OnResponse 收到 .mc → DoRequest POST 壳 / 分析
--   4. OnResponse 收到分析结果 → AddText 显示
-- 每步 note() 打点，便于定位卡点；DoRequest 失败/无响应时提示检查壳是否运行。
--
-- 响应机制（官方 changelog 6.6.42+）：Editor:DoRequest 发起，响应经插件全局
-- 函数 OnResponse 回调（不是 DoRequest 的第四参）。本脚本兼容多参形态。
--
-- 安装：把本文件放到 MalodyV/Editor/（或 editor/）目录（目录不存在则创建）。

PluginName = 'MMA Analyze'
PluginMode = 0
PluginType = 0
PluginRequire = '5.0.1'

local ENDPOINT = 'http://127.0.0.1:24060/'
local RESOLVE_ENDPOINT = 'http://127.0.0.1:24060/resolve'

local PENDING = {
    meta = {},
    resolved = false,
    miss = false,
    done = false,
}

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
        note('分析请求失败：' .. tostring(errA) .. '（请确认壳 mma-shell 已运行）')
    end
end

function OnResponse(...)
    local args = { ... }
    -- 兼容形态：OnResponse(url, status, body) / (status, body) / (body)
    local url, status, body
    for i = 1, #args do
        local a = args[i]
        if type(a) == 'string' then
            if string.find(a, '24060', 1, true) then
                url = a
            elseif body == nil then
                body = a
            end
        elseif type(a) == 'number' then
            status = a
        end
    end
    if not body or body == '' then
        return
    end
    if url and string.find(url, 'resolve', 1, true) then
        -- resolve 响应：.mc 原文 → 分析
        PENDING.resolved = true
        if looksLikeChart(body) then
            note('已找到谱面，分析中…（' .. #body .. ' 字节）')
            postAnalyze(PENDING.meta, body)
        else
            note('壳未在 chart 目录找到该谱面（标题：' .. (PENDING.meta.title or '') .. '）')
            PENDING.miss = true
        end
    else
        -- 分析响应：显示结果
        PENDING.done = true
        note(trim(body, 2000))
    end
end

function Run()
    PENDING = {
        meta = {},
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
    note('正在按标题查找谱面：' .. (meta.title or '') .. '…')

    local payload = '{"action":"resolve","title":' .. jsonQuote(meta.title)
        .. ',"artist":' .. jsonQuote(meta.artist)
        .. ',"level":' .. jsonQuote(meta.level)
        .. ',"keys":' .. tostring(meta.keys or 0) .. '}'
    local ok, err = pcall(function()
        Editor:DoRequest(RESOLVE_ENDPOINT, 'POST', payload)
    end)
    if not ok then
        note('DoRequest 调用失败：' .. tostring(err) .. '（请确认壳已运行、MalodyV 6.6.43+）')
        return
    end
    -- 等待响应（约 3s）；超时提示检查壳。
    local waited = 0
    while waited < 60 and not PENDING.resolved and not PENDING.done and not PENDING.miss do
        if os.sleep then
            pcall(function()
                os.sleep(0.05)
            end)
        end
        waited = waited + 1
    end
    if not PENDING.resolved and not PENDING.done then
        note('未收到壳响应（3s 超时）。请确认 mma-shell 正在运行（exe 旁应有 mma-shell.log）。')
    end
end