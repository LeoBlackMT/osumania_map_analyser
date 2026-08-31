-- MMA 选歌桥（Etterna / StepMania 系主题）。
--
-- 写入 Save/LeosMmaBridge.txt：换歌 / 换难度 / 改速率时写一次（内容幂等，
-- 避免高频 IO）。插件经壳 2Hz 轮询该文件。另写 Save/LeosMmaBridgeLoaded.txt
-- （脚本被加载即写，用于诊断）与 Save/LeosMmaBridgeError.txt（出错时记录）。
--
-- 安装：把本文件复制到
--   Themes/<你的主题>/BGAnimations/ScreenSelectMusic decorations/
-- 并在同目录 default.lua 的 return t 前插入：
--   t[#t + 1] = LoadActor("mma_bridge.lua")
--
-- 容错：全部运行时调用 pcall 包裹——任何单个 API 异常都不允许中断
-- BeginCommand 的后续注册（SetUpdateRate/UpdateCommand）。

local FILE = "Save/LeosMmaBridge.txt"
local LOADED_FILE = "Save/LeosMmaBridgeLoaded.txt"
local ERROR_FILE = "Save/LeosMmaBridgeError.txt"
local WRITE_INTERVAL = 0.5
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
        writeText(ERROR_FILE, msg)
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

local function write()
    local ok, err = pcall(function()
        local song = GAMESTATE:GetCurrentSong()
        if not song then
            return
        end
        local steps = GAMESTATE:GetCurrentSteps(PLAYER_1) or song:GetAllSteps()[1]
        if not steps then
            return
        end
        local rate = currentRate()
        local msd = {}
        for i = 1, 8 do
            pcall(function()
                msd[i] = steps:GetMSD(rate, i)
            end)
        end
        local title = song:GetDisplayMainTitle() or ""
        local artist = song:GetDisplayArtist() or ""
        local difficulty = nil
        pcall(function()
            difficulty = DifficultyToShortString(steps:GetDifficulty())
        end)
        local meter = 0
        pcall(function()
            meter = steps:GetMeter() or 0
        end)
        local key = title .. "::" .. tostring(steps:GetFilename() or "") .. "::" .. tostring(rate)
        if key == lastKey then
            return
        end
        lastKey = key
        local lines = {
            "title=" .. title,
            "artist=" .. artist,
            "song_dir=" .. tostring(song:GetSongDir() or ""),
            "step_file=" .. tostring(steps:GetFilename() or ""),
            "difficulty=" .. tostring(difficulty or ""),
            "meter=" .. tostring(meter),
            "rate=" .. tostring(rate),
        }
        local bg = nil
        pcall(function()
            bg = song:GetBackgroundPath()
        end)
        if bg and bg ~= "" then
            lines[#lines + 1] = "cover=" .. bg
        end
        for i = 1, 8 do
            lines[#lines + 1] = "msd_" .. i .. "=" .. tostring(msd[i] or 0)
        end
        writeText(FILE, table.concat(lines, "\n"))
    end)
    if not ok then
        recordError(tostring(err))
    end
end

return Def.ActorFrame {
    BeginCommand = function(self)
        -- 加载哨兵 + 首次写入；任何异常不得中断后续注册。
        writeLoadedFlag()
        pcall(function()
            write()
        end)
        -- Etterna/StepMania 标准每帧回调：SetUpdateRate + UpdateCommand
        -- （SetUpdateFunction 是 NotITG 专有 API，在 Etterna 会抛错）。
        self:SetUpdateRate(WRITE_INTERVAL)
    end,
    UpdateCommand = function(self)
        pcall(function()
            write()
        end)
    end,
    CurrentSongChangedMessageCommand = function(self)
        lastKey = ""
    end,
    CurrentStepsChangedMessageCommand = function(self)
        lastKey = ""
    end,
    CurrentRateChangedMessageCommand = function(self)
        lastKey = ""
    end,
}