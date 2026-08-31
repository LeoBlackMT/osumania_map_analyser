-- MMA Analyze 编辑器插件（MalodyV，触发类 PluginType=0）。
--
-- 用法：打开谱面编辑器 → 「更多」菜单点击 Analyze → 读取当前谱面并
-- POST 到本地壳（127.0.0.1:24060）→ 分析结果以短消息与 AddText 卡片
-- 渲染在编辑区。
--
-- 安装：把本文件放到 MalodyV/Editor/ 目录（目录不存在则创建），
-- 客户端重启或插件面板重载后即可在菜单见到。
--
-- 注：Editor:DoRequest / ReadFileSelect 的精确签名无官方文档（真机
-- 验证项）；本脚本对二者均以 pcall 包裹，失败走 GetUserInput+ReadFile
-- 兜底，不会阻断编辑器。

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

local function postAndRender(meta, chartText)
    if not chartText or chartText == '' then
        Editor:ShowMessage('MMA: 未能读取谱面文件，请检查文件名（chart/<曲目目录>/ 内的 .mc，通常为数字名）')
        return
    end
    local payload = '{"meta":{"title":' .. jsonQuote(meta.title)
        .. ',"artist":' .. jsonQuote(meta.artist)
        .. ',"level":' .. jsonQuote(meta.level)
        .. ',"keys":' .. tostring(meta.keys or 0)
        .. '},"chartText":' .. jsonQuote(chartText) .. '}'
    pcall(function()
        Editor:DoRequest(ENDPOINT, 'POST', payload, function(response)
            local text = trim(response, 2000)
            Editor:ShowMessage('LeosMma: ' .. text)
            pcall(function()
                Editor:AddText('mma_result', 'LeosMma: ' .. text)
            end)
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

    -- 尝试自动读取：1) 文件选择器（签名未证实 → pcall）；2) 旧版默认名 chart.mc。
    local ok, picked = pcall(function()
        return Editor:ReadFileSelect()
    end)
    if ok and picked and picked ~= '' then
        postAndRender(meta, picked)
        return
    end

    local candidates = { 'chart.mc' }
    for _, name in ipairs(candidates) do
        local chartText = nil
        pcall(function()
            chartText = Editor:ReadFile(name)
        end)
        if chartText and chartText ~= '' then
            postAndRender(meta, chartText)
            return
        end
    end

    -- 兜底：手动输入。路径 = chart/<曲目目录>/<文件名>.mc（如 0/Various Artists - xxx.mc 或纯文件名）。
    pcall(function()
        Editor:GetUserInput('chart 目录内的 .mc 文件名（若自动读取失败，请输入如 0/xxxx.mc 或 chart.mc）', '', function(name)
            if name and name ~= '' then
                local chartText = nil
                pcall(function()
                    chartText = Editor:ReadFile(name)
                end)
                postAndRender(meta, chartText)
            end
        end)
    end)
end