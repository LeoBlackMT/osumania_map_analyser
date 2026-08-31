-- MMA Result 皮肤脚本（MalodyV，游玩场景）。
--
-- 显示内容：壳写入该皮肤目录的 mma_state.txt（由编辑器 Analyze / 壳侧分析产出）。
--
-- 安装（独立皮肤方案——避免覆盖其他皮肤脚本的同名全局钩子）：
--   1. 在 skin/ 下新建独立皮肤目录（如 skin/MMA-Result/），把本文件放入
--      （文件名任意 .lua）；
--   2. 在皮肤 Composer 给该皮肤添加一个 Text 模块，命名 mma_result；
--   3. 在该皮肤目录创建空哨兵文件 mma.txt——壳据此识别写入目标；
--   4. 游玩时选择该皮肤（Base 皮肤不变，选择 MMA-Result 作为皮肤）。
--
-- Malody 皮肤运行时只调用以下全局钩子（参考官方皮肤，如 Elaina）：
--   InitSharedData（开局）/ UpdateSharedData（每帧）/ OnHitSharedData /
--   OnInputSharedData。不要依赖 Init()/Update()（非皮肤 API）。
--
-- 身份跟随边界（如实标注）：游玩场景皮肤无外发通道，只能展示壳最近一次
-- 写入的结果；精确跟随仅编辑器场景（web post）可用。

local MODULE_NAME = 'mma_result'
local lastRead = ''

function UpdateSharedData()
    local text = ''
    local ok = pcall(function()
        text = Game:ReadFile('mma_state.txt') or ''
    end)
    if ok and text ~= lastRead then
        lastRead = text
        local t = Module:Find(MODULE_NAME)
        if t then
            if text == '' then
                -- 空态提示：尚未有任何分析写入（在编辑器触发 Analyze 后更新）。
                t.Text = 'MMA: 尚未分析（请在编辑器触发 Analyze）'
                t.Alpha = 60
            else
                t.Text = text
                t.Alpha = 100
            end
        end
    end
end