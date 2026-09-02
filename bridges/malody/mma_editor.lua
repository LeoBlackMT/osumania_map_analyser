-- MMA Analyze 编辑器插件（MalodyV，触发类 PluginType=0）。
--
-- 用法：打开谱面编辑器 → 「更多」菜单点击 Analyze → 自动请求分析当前谱面，
-- 分析结果以 AddText 卡片渲染在编辑区。
--
-- 通道说明（多轮实测结论）：
--   * Editor:DoRequest 的 POST+body 在 MalodyV 网络层触发
--     `{"jek":-998,"jel":"invalid url: {body}"}`（body 被当 url）——POST 通道不可用。
--   * 因此本插件走**文件通道**（全文档化 API）：
--       1. Run()：ChartInfo 取 title/artist/level/key → Editor:WriteFile('mma_request.json')
--       2. 壳扫描 {malodyRoot}/chart/**/mma_request.json → resolve .mc → 分析
--          → 写回 mma_result.txt（若壳无权限写会提示）
--       3. 本插件轮询 Editor:ReadFile('mma_result.txt') → AddText 显示
--   * 若 WriteFile 失败（游戏无写权限），提示用户。
--
-- 每步 note() 打点，便于定位卡点。
--
-- 安装：把本文件放到 MalodyV/editor/（或 Editor/）目录（目录不存在则创建）。

PluginName = 'MMA Analyze'
PluginMode = 0
PluginType = 0
PluginRequire = '5.0.1'

local REQUEST_FILE = 'mma_request.json'
local RESULT_FILE = 'mma_result.txt'

local PENDING = {
    meta = {},
    sent = false,
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

local function readResult()
    local ok, content = pcall(function()
        return Editor:ReadFile(RESULT_FILE)
    end)
    if ok and content and content ~= '' then
        return content
    end
    return nil
end

-- 上次点击是否显示过残留结果（静态，跨 Run 保留）。
local showedStale = false

function Run()
    PENDING = {
        meta = {},
        sent = false,
        done = false,
    }
    local meta = PENDING.meta
    pcall(function()
        meta.title = Editor:ChartInfo('title') or ''
        meta.artist = Editor:ChartInfo('artist') or ''
        meta.level = Editor:ChartInfo('level') or ''
        meta.keys = tonumber(Editor:ChartInfo('key')) or 0
    end)

    -- 残留结果：上次点击已显示过 → 本次视为「重新分析」跳过残留；
    -- 未显示过（壳已写好）→ 直接展示（二次点击查看结果）。
    local prev = readResult()
    if prev and prev ~= '' and not showedStale then
        PENDING.done = true
        note(trim(prev, 2000) .. '\n（再次点击可重新分析）')
        showedStale = true
        return
    end
    showedStale = false

    note('正在请求分析：' .. (meta.title or '') .. '…')

    local payload = '{"action":"analyze","title":' .. jsonQuote(meta.title)
        .. ',"artist":' .. jsonQuote(meta.artist)
        .. ',"level":' .. jsonQuote(meta.level)
        .. ',"keys":' .. tostring(meta.keys or 0) .. '}'
    local okW, errW = pcall(function()
        Editor:WriteFile(REQUEST_FILE, payload)
    end)
    if not okW then
        note('WriteFile 失败：' .. tostring(errW) .. '（Malody 目录无写权限？请尝试以管理员运行 Malody）')
        return
    end
    -- WriteFile 成功但内容可能没写入（游戏进程写权限未知）——用 ReadFile 验证。
    local verifyOk, verifyContent = pcall(function()
        return Editor:ReadFile(REQUEST_FILE)
    end)
    if not (verifyOk and verifyContent and verifyContent ~= '') then
        note('写入 mma_request.json 失败（目录只读）。请以管理员运行 Malody，或把壳设为管理员运行。')
        return
    end
    PENDING.sent = true

    -- 尽力轮询结果：若 os.sleep 可用则等约 10s；否则提示稍后再点（沙箱无 sleep）。
    if os.sleep then
        note('请求已写入，等待壳分析…')
        local waited = 0
        while waited < 100 do
            pcall(function()
                os.sleep(0.1)
            end)
            waited = waited + 1
            local result = readResult()
            if result and result ~= '' then
                PENDING.done = true
                note(trim(result, 2000))
                return
            end
        end
        note('未收到分析结果（10s 超时）。请确认 mma-shell 正在运行，并查看壳日志。')
    else
        note('请求已写入。壳处理完成后，再次点击「MMA Analyze」即可查看结果（本环境无 sleep，无法自动等待）。')
    end
end
