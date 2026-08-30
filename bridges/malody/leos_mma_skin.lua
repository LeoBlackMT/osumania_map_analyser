-- LeosMma 皮肤脚本（MalodyV 基础皮肤，游玩场景）。
--
-- 显示内容：壳写入 skin 目录的 mma_state.txt（由编辑场景/壳侧分析产出）
-- + 当前谱面标题（皮肤侧 Game:ChartInfo 自有信息，不经壳）。
--
-- 安装：
--   1. 在 Malody 皮肤编辑器（Composer）给当前皮肤添加一个 Text 模块，
--      命名为 mma_result（本脚本每帧设置其 Text/Alpha）；
--   2. 把本文件复制到该皮肤目录（skin/<皮肤名>/，文件名任意 .lua）；
--   3. 在皮肤目录创建哨兵文件 mma.txt（空文件）——壳据此识别应向
--      哪些皮肤目录写 mma_state.txt（多个含哨兵目录全部写入，幂等）。
--
-- 身份跟随边界（如实标注）：游玩场景皮肤无外发通道，皮肤只展示壳
-- 最近一次写入的分析结果；精确跟随仅编辑器场景（web post）可用。

local MODULE_NAME = 'mma_result'

function Init()
    local t = Module:Find(MODULE_NAME)
    if t then
        t.Text = ''
        t.Alpha = 0
    end
end

local lastRead = ''

function Update()
    local text = ''
    local ok = pcall(function()
        text = Game:ReadFile('mma_state.txt') or ''
    end)
    if ok and text ~= lastRead then
        lastRead = text
        local t = Module:Find(MODULE_NAME)
        if t then
            t.Text = text
            t.Alpha = 100
        end
    end
    -- 皮肤侧自有元信息补充（标题行前置）
    local title = ''
    pcall(function()
        title = Game:ChartInfo('title') or ''
    end)
    if title ~= '' and lastRead ~= '' then
        local t = Module:Find(MODULE_NAME)
        if t then
            t.Text = title .. '\n' .. lastRead
        end
    end
end