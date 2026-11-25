# 桌面壳 v13：窗口操作与 lazer 读取状态 / Desktop shell v13: gestures and lazer reader status

## 中文

1. **改了什么。** 壳与页面的 WebSocket 契约由 v12 升至 v13：新增带八方向的 `control.resizeStart`；`sources.osu` 增加 `lazerLaneAccReady`，以及可选的 `lazerScoreReady` 和 `lazerLaneAccNotice`。HTTP 接口 `/offsets/status` 的 `lazer.loaded` 现在表示读取器已附着；`lazer.bundled` 表示随包表。
2. **为什么改。** tosu 可以提供谱面和游戏状态，但只有读到游戏判定的壳才能提供 LaneAcc。把两种就绪状态分开，设置页就能告诉用户应该启动游戏、检查权限还是更新偏移表。独立的窗口把手让状态行和标签保持可点，也让桌面壳在桥重连时仍可拖动。
3. **影响谁。** Windows 和 Linux 共用新窗口热区。Linux x86-64 可从原生 osu!lazer 采集 mania 判定；KWin 6 + XWayland 支持全屏置顶。普通 tosu 页面保持原有布局。Etterna 安装器会迁移旧 gameplay 钩子；Malody V 的选曲和编辑器均可解析 Wine 盘符。
4. **如何升级。** 将桌面壳和前端资源一起更新。新版页面兼容契约 3–13；完整的八向缩放和 Linux LaneAcc 使用 v13 壳。读取状态的客户端用 `lazer.loaded` 判断当前表，用 `lazer.bundled` 展示随包版本。Linux 用户可运行 `./run-lazer.sh`；自装游戏时，`MMA_LAZER_EXECUTABLE` 可指定可执行文件、AppImage 或 `osu!.dll`。
5. **如何验证。** 在 320px 和 440px 宽的窗口中，状态圆点、移动把手与八个缩放区各自命中。真实 Linux 壳在桥连通和断开时均可移动，连通时可改变窗口宽度。osu!lazer 2026.1117 的原生 4K 测试谱产生 10 个判定和 20 个回放帧；原生 standard 谱完成回放时两者均为零。

## English

1. **What changed.** Shell–page contract v13 adds eight-direction `control.resizeStart` and `sources.osu.lazerLaneAccReady`, with optional `lazerScoreReady` and `lazerLaneAccNotice`. In the HTTP endpoint `/offsets/status`, `lazer.loaded` means the reader is attached; `lazer.bundled` identifies the packaged reference table.
2. **Why.** A working tosu connection can supply the beatmap and game state while LaneAcc is still waiting for the game, permission, or matching offsets. Separate fields let the page show the right action. Dedicated window gutters keep card controls clickable, and native dragging works while the bridge reconnects.
3. **Who is affected.** Windows and Linux share the window controls. Linux x86-64 can collect judgments from native mania maps; KWin 6 with XWayland supports fullscreen overlays. Ordinary tosu browser pages retain their layout. The Etterna installer migrates the legacy gameplay hook; both Malody V endpoints can resolve Wine drive paths.
4. **How to upgrade.** Ship the shell and frontend assets together. The new page accepts contracts 3–13; full eight-direction resizing and Linux LaneAcc require v13. Status consumers should use `lazer.loaded` for the active reader and `lazer.bundled` for the shipped reference. Start with `./run-lazer.sh` on Linux. For a custom install, point `MMA_LAZER_EXECUTABLE` at the executable, AppImage, or `osu!.dll`.
5. **Verification.** At window widths of 320 and 440 pixels the source dot, drag handle and all eight resize targets received input separately. A Linux shell moved with and without its bridge connection and resized while connected. A native 4K Autoplay chart in osu!lazer 2026.1117 produced ten judgments and twenty replay frames; the native standard replay reached results with zero of either.
