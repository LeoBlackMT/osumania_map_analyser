-- MMA 游玩状态桥：装载于常驻的 ScreenSystemLayer overlay。
-- 每 0.25s 检查当前场景，只在进入/离开游玩时写 Save/MmaGameplay.txt。
-- 即使游戏直接返回选歌而未发送 Off/Out 命令，也会清除 playing。
-- 将本文件放在系统层 overlay 脚本旁，在其 return t 前加入：
--   t[#t + 1] = LoadActor("mma_gameplay.lua")

local FILE = "Save/MmaGameplay.txt"
local CHECK_INTERVAL = 0.25
local lastPlaying = nil

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
        pos = GAMESTATE:GetSongPosition():GetMusicSeconds() or 0
        total = song:MusicLengthSeconds() or 0
    end)
    if playing and total <= 0 then
        return false -- 场景已切换但歌曲尚未就绪，下一次检查再发布。
    end
    local body = "playing=" .. (playing and 1 or 0)
        .. "\nmusic_seconds=" .. tostring(pos)
        .. "\ntotal_seconds=" .. tostring(total)
        .. "\nrate=" .. tostring(currentRate())
    local wrote = false
    pcall(function()
        local f = RageFileUtil.CreateRageFile()
        if f:Open(FILE, 2) then
            f:Write(body)
            f:Close()
            wrote = true
        end
        f:destroy()
    end)
    return wrote
end

local function check()
    local ok, playing = pcall(function()
        local screen = SCREENMAN:GetTopScreen()
        return screen ~= nil and screen:GetName():match("^ScreenGameplay") ~= nil
    end)
    if ok and playing ~= lastPlaying and write(playing) then
        lastPlaying = playing
    end
end

return Def.ActorFrame {
    InitCommand = function(self)
        local elapsed = 0
        self:SetUpdateFunction(function(_, delta)
            elapsed = elapsed + delta
            if elapsed >= CHECK_INTERVAL then
                elapsed = 0
                check()
            end
        end)
        check()
    end,
}