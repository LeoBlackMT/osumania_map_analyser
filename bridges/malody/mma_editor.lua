-- MMA Analyze 编辑器插件（MalodyV，触发类 PluginType=0）。
--
-- 用法：打开谱面编辑器 → 「更多」菜单点击 Analyze → 自动请求分析当前谱面，
-- 分析结果以 AddText 卡片渲染在编辑区。
--
-- 通道：Editor:DoRequest（6.6.43+ 新增，官方文档未记载签名）。本插件内置
-- 「签名自探测」：依次尝试 4 种参数形态，OnResponse 收到非 invalid-url 的
-- 响应即视为成功。任何形态收到壳的正常响应（.mc 原文或 404 JSON）即完成。
--
-- 每步 note() 打点，便于定位卡点。
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
    try = 0,
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

-- 候选签名。用户实测三轮错误证明「第二个参数被当 url」：
--   (url,'POST',body) → invalid url: POST
--   (url, body)      → invalid url: {payload}
-- 即签名 = (method, url, body)——method 在前。放第一位，其余作兜底。
local function tryDoRequest(tryIdx, url, payload)
    local ok, err = pcall(function()
        if tryIdx == 1 then
            Editor:DoRequest('POST', url, payload)      -- (method, url, body) ★ 实测吻合
        elseif tryIdx == 2 then
            Editor:DoRequest(url, payload)             -- (url, body)
        elseif tryIdx == 3 then
            Editor:DoRequest(url, 'POST', payload)      -- (url, method, body)
        elseif tryIdx == 4 then
            Editor:DoRequest({ url = url, method = 'POST', body = payload })  -- 表参数
        end
    end)
    return ok, err
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
            local diag = trim(body, 300)
            if diag == '' then
                diag = '(空响应)'
            end
            note('壳未找到谱面：' .. diag .. '（请求 title=' .. (PENDING.meta.title or '')
                .. ' artist=' .. (PENDING.meta.artist or '')
                .. ' level=' .. (PENDING.meta.level or '') .. '）')
            PENDING.miss = true
        end
    elseif string.find(tostring(body), 'invalid url', 1, true) then
        -- 该签名无效：不设任何标志，让 Run() 继续尝试下一个签名。
        PENDING.badSignature = true
    else
        -- 分析响应：显示结果
        PENDING.done = true
        note(trim(body, 2000))
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
        -- 分析请求也走签名探测（同一 DoRequest 绑定）。
        for t = 1, 4 do
            PENDING.try = t
            PENDING.badSignature = false
            tryDoRequest(t, ENDPOINT, payload)
            -- 等 OnResponse（约 1.5s）；badSignature 则换下一个。
            local waited = 0
            while waited < 30 do
                if os.sleep then
                    pcall(function()
                        os.sleep(0.05)
                    end)
                end
                waited = waited + 1
                if PENDING.done then
                    return
                end
                if PENDING.badSignature then
                    break
                end
            end
            if PENDING.done then
                return
            end
        end
        if not PENDING.done then
            note('分析请求失败（4 种签名均无效）。请检查 mma-shell 是否运行。')
        end
    end)
    if not okA then
        note('分析请求异常：' .. tostring(errA))
    end
end

function Run()
    PENDING = {
        meta = {},
        try = 0,
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
    -- 签名探测：依次尝试 4 种，成功（resolved/miss/done）即停。
    for t = 1, 4 do
        PENDING.try = t
        PENDING.badSignature = false
        local ok, err = tryDoRequest(t, RESOLVE_ENDPOINT, payload)
        if not ok then
            note('DoRequest 调用失败：' .. tostring(err))
            return
        end
        local waited = 0
        while waited < 30 do
            if os.sleep then
                pcall(function()
                    os.sleep(0.05)
                end)
            end
            waited = waited + 1
            if PENDING.resolved or PENDING.miss or PENDING.done then
                return
            end
            if PENDING.badSignature then
                break
            end
        end
        if PENDING.resolved or PENDING.miss or PENDING.done then
            return
        end
    end
    note('未收到壳响应（4 种签名均无有效回执，约 6s）。请确认 mma-shell 正在运行，'
        .. '并检查 MalodyV 日志。')
end
