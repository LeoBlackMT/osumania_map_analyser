# bridges/ — 游戏侧桥文件

游戏侧注入物（Lua），供桌面壳的数据源通道使用。安装细节详见：

- Etterna：`bridges/etterna/`（选歌桥 + 游玩桥两件套）→ 安装指南见
  `docs/features/desktop-shell.md`（主题目录、LoadActor 注入、主题更新后重装）
- Malody V：`bridges/malody/`（编辑器插件 + 皮肤脚本）→ 安装指南同上
  （编辑器插件放 `MalodyV/Editor/`；皮肤脚本 + `mma.txt` 哨兵 + Composer 内
  `mma_result` Text 模块）

En / 中文：以上文件皆含简短注释；完整说明以 docs 为准。

# bridges/ — game-side bridge files

Lua injection assets for the shell's data-source channels. Follow the bridge
header comments and `docs/features/desktop-shell.md` for installation
(Etterna theme directories with `LoadActor` injection and re-install after
theme updates; Malody Editor/ plugin, skin script with `mma.txt` sentinel and a
`mma_result` Text module in Composer).