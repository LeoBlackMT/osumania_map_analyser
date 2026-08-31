-- MMA 选歌桥（Etterna / StepMania 系主题）。
--
-- 写入 Save/LeosMmaBridge.txt：选歌/换难度/改速率变化时写（key 门控幂等）。
-- 诊断：Save/LeosMmaBridgeLoaded.txt（脚本加载即写）、
--       Save/LeosMmaBridgeError.txt（运行时错误）。
--
-- 安装：复制到
--   Themes/<你的主题>/BGAnimations/ScreenSelectMusic decorations/
-- 并在同目录 default.lua 的 return t 前插入：
--   t[#t + 1] = LoadActor("mma_bridge.lua")
--
-- 机制（对照 Dan-Overlay 桥实战模式）：每 0.5s 轮询检查（SetUpdateFunction
-- 优先——Etterna 0.7x 支持；不存在则 SetUpdateRate 兜底）+ 全套选歌消息钩子
-- 即时重检。任何 API 异常都 pcall 隔离且不得中断后续注册与循环。

local FILE = "Save/MmaBridge.txt"
local LOADED_FILE = "Save/MmaBridgeLoaded.txt"
local ERROR_FILE = "Save/MmaBridgeError.txt"
local CHECK_INTERVAL = 0.5
local lastKey = ""

local function writeText(path, text)
    pcall(function()
        local f = RageFileUtil.CreateRageFile()
        if f:Open(path, 2) then
            f:Write(text)
            f:Close()
        end
        f:destroy()
    end)
end

local function writeLoadedFlag()
    writeText(LOADED_FILE, "1")
end

local function recordError(msg)
    if msg and msg ~= '' then
        writeText(ERROR_FILE, tostring(msg))
    end
end

local function currentRate()
    local rate = 1.0
    pcall(function()
        if getCurRateValue then
            rate = getCurRateValue()
        end
    end)
    return rate
end

local function safeGet(fn, fallback)
    local ok, v = pcall(fn)
    if ok and v ~= nil then
        return v
    end
    return fallback
end

local function baseName(path)
    local base = path:match("[^/\\]+$")
    if base then
        return base
    end
    return path
end

local function writeState(song, steps)
    local title = safeGet(function() return song:GetDisplayMainTitle() end, "") or ""
    local artist = safeGet(function() return song:GetDisplayArtist() end, "") or ""
    local rate = currentRate()
    local msd = {}
    for i = 1, 8 do
        local ok, v = pcall(function()
            return steps:GetMSD(rate, i)
        end)
        if ok and type(v) == "number" then
            msd[i] = v
        else
            msd[i] = 0
        end
    end
    local lines = {
        "title=" .. title,
        "artist=" .. artist,
        "song_dir=" .. safeGet(function() return song:GetSongDir() end, "") or "",
        "step_file=" .. baseName(safeGet(function() return steps:GetFilename() end, "") or ""),
        "difficulty=" .. tostring(safeGet(function() return steps:GetDifficulty() end, "") or ""),
        "meter=" .. tostring(safeGet(function() return steps:GetMeter() end, 0) or 0),
        "rate=" .. tostring(rate),
    }
    local bg = safeGet(function() return song:GetBackgroundPath() end, nil)
    if bg and bg ~= "" then
        lines[#lines + 1] = "cover=" .. bg
    end
    for i = 1, 8 do
        lines[#lines + 1] = "msd_" .. i .. "=" .. tostring(msd[i])
    end
    writeText(FILE, table.concat(lines, "\n"))
end

local function checkAndUpdate()
    local ok, err = pcall(function()
        local song = GAMESTATE:GetCurrentSong()
        if not song then
            return
        end
        local steps = GAMESTATE:GetCurrentSteps(PLAYER_1) or song:GetAllSteps()[1]
        if not steps then
            return
        end
        -- 快速滚动时 steps 可能属于旧歌：按难度匹配回当前歌的 steps（Dan 桥同款）。
        local songDir = song:GetSongDir() or ""
        local stepFile = steps:GetFilename() or ""
        if songDir ~= "" and (stepFile == "" or not string.find(stepFile, songDir, 1, true)) then
            local allSteps = song:GetAllSteps()
            if allSteps and #allSteps > 0 then
                local curDiff = steps:GetDifficulty()
                local matched = false
                for _, s in ipairs(allSteps) do
                    if curDiff and s:GetDifficulty() == curDiff then
                        steps = s
                        matched = true
                        break
                    end
                end
                if not matched then
                    steps = allSteps[1]
                end
            end
        end
        local key = (song:GetDisplayMainTitle() or "") .. "::" .. baseName(steps:GetFilename() or "")
            .. "::" .. tostring(safeGet(function() return steps:GetDifficulty() end, "") or "")
            .. "::" .. tostring(safeGet(function() return steps:GetMeter() end, 0) or 0)
            .. "::" .. tostring(currentRate())
        if key ~= lastKey then
            lastKey = key
            writeState(song, steps)
        end
    end)
    if not ok then
        recordError(tostring(err))
    end
end

return Def.ActorFrame {
    BeginCommand = function(self)
        writeLoadedFlag()
        pcall(function()
            checkAndUpdate()
        end)
        -- 更新循环：SetUpdateFunction 优先（Etterna 0.7x 支持），SetUpdateRate 兜底。
        local ok = pcall(function()
            self:SetUpdateFunction(function(actor, delta)
                timer = (timer or 0) + delta
                if timer >= CHECK_INTERVAL then
                    timer = 0
                    checkAndUpdate()
                end
            end)
        end)
        if not ok then
            pcall(function()
                self:SetUpdateRate(CHECK_INTERVAL)
            end)
        end
    end,
    UpdateCommand = function(self)
        pcall(function()
            checkAndUpdate()
        end)
    end,
    -- 选歌消息钩子（Dan-Overlay 实战验证过的集合）→ 即时重检
    CurrentSongChangedMessageCommand = function(self) checkAndUpdate() end,
    CurrentStepsChangedMessageCommand = function(self) checkAndUpdate() end,
    DelayedChartUpdateMessageCommand = function(self) checkAndUpdate() end,
    WheelSettledMessageCommand = function(self) checkAndUpdate() end,
    ChangedStepsMessageCommand = function(self) checkAndUpdate() end,
    CurrentRateChangedMessageCommand = function(self) checkAndUpdate() end,
    SortOrderChangedMessageCommand = function(self) checkAndUpdate() end,
    TabChangedMessageCommand = function(self) checkAndUpdate() end,
}