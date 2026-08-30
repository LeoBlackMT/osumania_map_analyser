# 桌面壳（desktop shell）

> 给人类的使用/安装文档（中英双语节选）；开发与协议细节见
> `docs/features/multi-source.md` 与 `desktop/README.md`、`desktop/docs/CONTRACT.md`。

## 这是什么

一个可选的桌面小窗口（Tauri v2，Windows/Linux）：加载同插件页面，并作为
「本地聚合桥」——接收 Malody 编辑器分析请求（24060）、向页面推送 Etterna
数据（24061 WS）、离线时静态服务插件页面（24061）、写 Malody 皮肤状态文件、
提供封面白名单服务。**浏览器模式不受影响**；壳只是另一种展示形态（可置顶/
透明/无边框，盖在其他窗口含全屏游戏之上）。

## 在线 / 离线

- 壳启动时在可执行文件上级目录（2–3 层）找 `tosu.env`，读 `SERVER_PORT`；
  `GET /` 健康探测成功（tosu 运行中）→ 直接打开 `http://127.0.0.1:{port}/{插件目录}/`
  （设置/WS/谱面文件全部照常走 tosu，零适配）；
- tosu 未运行 → 打开 `http://127.0.0.1:24061/`（壳静态服务插件目录），
  Etterna/Malody 数据源可用，osu! 数据源显示离线。

## 安装（Windows 示例）

1. 把 `mma-shell.exe` 放到插件目录（`tosu/static/ManiaMapAnalyser by Leo_Black/`）；
2. 双击运行。（Linux：构建后在插件目录运行 `mma-shell`。）

## 平台限制（如实标注）

- 置顶/无边框/透明：Windows 与 Linux（合成器）可用；
- Windows 点击穿透：尽力（`set_ignore_cursor_events`），失败退回置顶模式；
- Linux 点击穿透：不支持（不作穿透）；
- 透明白闪（Windows）：`noRedirectionBitmap` 配置；如仍有白闪属已知抖动。

## 数据源准备（桥）

- Etterna：安装 `bridges/etterna/` 两个 Lua 到主题目录（`ScreenSelectMusic
  decorations/` 与 `ScreenGameplay overlay/` 的 `default.lua` 引用 `LoadActor`），
  **主题更新后需重装**；壳自动/设置 `etternaRoot` 探测 Etterna 根目录。
- Malody：`bridges/malody/mma_editor.lua` 放 `MalodyV/Editor/`（目录不存在
  则创建），编辑器「更多」菜单触发分析；`bridges/malody/mma_skin.lua` 放入
  皮肤目录并在皮肤 Composer 添加名为 `mma_result` 的 Text 模块、目录内创建空
  哨兵文件 `mma.txt`（壳识别写入目标）。
- 设置：`gameClient`（Auto/Osu!/Etterna/Malody）+ `etternaRoot`/`malodyRoot`
  （壳自动探测不到时才需手填）。在线模式设置走 tosu；离线模式壳自有存储
  （页面侧持久化接线为已知待办，见 multi-source.md）。

## 构建

```
cd desktop
cargo build --release        # 产出 target/release/mma-shell(.exe)
```
发布：`desktop/release.ps1`（Windows）构建 release、拷贝 exe 到插件目录并打 zip。

# Desktop shell

Optional Tauri v2 window (Windows/Linux) hosting the same plugin page and acting
as the local aggregation bridge: Malody editor POST (24060), Etterna poll push
(24061 WS), offline static serving, Malody skin state writer, cover whitelist.
Browser mode is unaffected.

- Online (tosu.env found & alive): opens `http://127.0.0.1:{port}/{plugin}/`
  (settings/WS/files stay on tosu, zero adaptation).
- Offline: opens `http://127.0.0.1:24061/` (shell serves plugin dir; external
  sources available, osu! marked offline).
- Install: copy `mma-shell.exe` next to the plugin folder; Linux binary likewise.
- Platform notes: always-on-top/borderless/transparent OK; Windows click-through
  best-effort (`set_ignore_cursor_events`), fallback always-on-top; Linux no
  click-through; Windows transparency white flash mitigated via
  `noRedirectionBitmap` config.
- Bridges: see `bridges/` READMEs (Etterna select/gameplay Lua pair; Malody
  editor plugin + skin script with `mma.txt` sentinel). Theme updates require
  re-installing the Etterna Lua pair.
- Build: `cargo build --release`; `desktop/release.ps1` packages exe into the
  plugin dir as zip.