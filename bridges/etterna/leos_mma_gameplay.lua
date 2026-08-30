-- LeosMma 游玩桥（Etterna / StepMania 系主题）。
--
-- 写入 Save/LeosMmaGameplay.txt：进入游玩与离开游玩时各写一次
-- （游玩中零写入，避免 IO 抖动）：
--   playing=1（进入）/ 0（离开）
--   music_seconds=当前位置（进入时）
--   total_seconds=歌曲总长（供壳外推 playing 过期）
--   rate=当前音乐速率
--
-- 安装：把本文件复制到
--   Themes/<你的主题>/BGAnimations/ScreenGameplay overlay/
-- 并在同目录 default.lua 的 return t 前插入：
--   t[#t + 1] = LoadActor("leos_mma_gameplay.lua")

local FILE = "Save/LeosMmaGameplay.txt"

local function currentRate()
    local rate = 1.0
    pcall(function()
        if getCurRateValue then
            rate = getCurRateValue()
        end
    end)
    return rate
end

local function write(playing)
    local pos = 0
    local total = 0
    pcall(function()
        local song = GAMESTATE:GetCurrentSong()
        local sp = GAMESTATE:GetSongPosition()
        pos = sp:GetMusicSeconds() or 0
        total = song:GetMusicSeconds() or 0
    end)
    local rate = currentRate()
    local body = "playing=" .. (playing and 1 or 0)
        .. "\nmusic_seconds=" .. tostring(pos)
        .. "\ntotal_seconds=" .. tostring(total)
        .. "\nrate=" .. tostring(rate)
    pcall(function()
        local f = RageFileUtil.CreateRageFile()
        if f:Open(FILE, 2) then
            f:Write(body)
            f:Close()
        end
        f:destroy()
    end)
end

return Def.Actor {
    InCommand = function(self)
        write(true)
    end,
    OutCommand = function(self)
        write(false)
    end,
    PlayerJoinedMessageCommand = function(self)
        write(true)
    end,
}