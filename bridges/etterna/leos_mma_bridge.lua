-- LeosMma 选歌桥（Etterna / StepMania 系主题）。
--
-- 写入 Save/LeosMmaBridge.txt：换歌 / 换难度 / 改速率时写一次（内容幂等，
-- 避免高频 IO）。插件经壳 2Hz 轮询该文件。
--
-- 安装：把本文件复制到
--   Themes/<你的主题>/BGAnimations/ScreenSelectMusic decorations/
-- 并在同目录 default.lua 的 return t 前插入：
--   t[#t + 1] = LoadActor("leos_mma_bridge.lua")

local FILE = "Save/LeosMmaBridge.txt"
local WRITE_INTERVAL = 0.5
local lastKey = ""
local updateTimer = 0

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
    local difficulty = DifficultyToShortString(steps:GetDifficulty())
    local meter = steps:GetMeter() or 0
    local key = title .. "::" .. (steps:GetFilename() or "") .. "::" .. tostring(rate)
    if key == lastKey then
        return
    end
    lastKey = key
    local lines = {
        "title=" .. title,
        "artist=" .. artist,
        "song_dir=" .. (song:GetSongDir() or ""),
        "step_file=" .. (steps:GetFilename() or ""),
        "difficulty=" .. tostring(difficulty),
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
    pcall(function()
        local f = RageFileUtil.CreateRageFile()
        if f:Open(FILE, 2) then
            f:Write(table.concat(lines, "\n"))
            f:Close()
        end
        f:destroy()
    end)
end

return Def.ActorFrame {
    BeginCommand = function(self)
        write()
        self:SetUpdateFunction(function(actor, delta)
            updateTimer = updateTimer + delta
            if updateTimer >= WRITE_INTERVAL then
                updateTimer = 0
                write()
            end
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
    CurrentBeatMessageCommand = function(self)
        -- 保持 key 门控可响应（部分主题切歌无 SongChanged 广播）
        if updateTimer >= WRITE_INTERVAL then
            updateTimer = 0
            write()
        end
    end,
}