# bridges/ — 游戏侧桥文件

游戏侧注入物（Lua），供桌面壳的数据源通道使用。安装细节详见 `docs/shell-guide.md`：

- Etterna：`bridges/etterna/mma_bridge.lua` + `mma_gameplay.lua`（选歌桥 + 游玩桥两件套）——主题目录、LoadActor 注入、主题更新后重装；
- Malody V：`bridges/malody/mma_editor.lua`（编辑器插件）+ `mma_skin.lua`（皮肤脚本）——`MalodyV/Editor/` 放置、皮肤 `mma.txt` 哨兵 + Composer 内 `mma_result` Text 模块。

# bridges/ — game-side bridge files

Lua injection assets for the shell's data-source channels. Follow the bridge header comments and `docs/shell-guide.md` for installation (Etterna theme directories with `LoadActor` injection and re-install after theme updates; Malody Editor/ plugin, skin script with `mma.txt` sentinel and a `mma_result` Text module in Composer).