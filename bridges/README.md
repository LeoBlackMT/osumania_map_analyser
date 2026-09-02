# bridges/ — 游戏侧桥文件

游戏侧注入物（Lua），供桌面壳的数据源通道使用。安装方法详见 [docs/shell-guide.md](https://github.com/LeoBlackMT/osumania_map_analyser/blob/main/docs/shell-guide.md)：

- Etterna：`bridges/etterna/mma_bridge.lua` + `mma_gameplay.lua` —— 主题目录、LoadActor 注入、主题更新后重装；
- Malody V：`bridges/malody/mma_editor.lua`（编辑器插件）—— 在 `MalodyV/Editor/` 放置，编辑器菜单 MMA Analyze 按钮触发分析。

# bridges/ — game-side bridge files

Lua injection assets for the shell's data-source channels. Follow the bridge header comments and `docs/shell-guide.md` for installation (Etterna theme directories with `LoadActor` injection and re-install after theme updates; Malody Editor/ plugin triggered from the editor More menu — the in-game skin display was removed).