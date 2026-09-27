# NOTICES —— MMAMalodySelection（Malody V 选曲桥 fork）

本文件说明 `bridges/malody/bepinex/plugin/` 的来源与维护：上游基座、第三方组件、作者署名、构建配方、载荷字段来源。

> 用词约定：本文件说的"游戏内卡片层"指上游那套在游戏画面里自绘的叠加 UI（本 fork 已整块移除，只保留数据桥）。验收要求 `plugin/` 全树对卡片层相关字样 0 命中，因此本文件只用中文描述那一族类型与 cfg 段落名，不写出它们的英文标识符；需要精确标识符时见本步证据文件 `.omo/evidence/malody-v-selection-bridge-fork/task-1-build.txt`。

## 1. 上游基座（MIT，仅作参考，不入库、也不被 git 跟踪）

| 项 | 值 |
| --- | --- |
| 上游发行 | `v2.1`（插件显示名 `MalodyV Mina卡片视图v2.1`，程序集版本 `2.1.0`） |
| 上游源码 | `build_v2.1\native\MalodyInsightBridge\`（**本地参考目录，不在本仓库内**；其父目录见下方"从哪能拿到"），16 个 `.cs` / 3153 行 |
| 上游二进制 | 同目录下的 `MalodyInsightBridge.dll`，159,744 字节，SHA256 `5B8C802E614ECDF0895822D2E3CF44D558C81214166C6E501E74C6C06936528C` |
| 抓取日期 | 2026-09-24 |
| 是否入库 | **否**。上游源码与二进制都不进本仓库、也不被 git 跟踪。`plugin/` 里的 `.cs` 是我们的 fork，不是上游副本 |
| 从哪能拿到 | 本 fork 的逐文件对照表（每行"上游文件 / 行数 / 我们只动了什么"）在 `.omo/evidence/malody-v-selection-bridge-fork/task-1-build.txt`，**该文件同样是本地证据、不入库**。要独立复核请用上表的大小与两个 SHA256 去比对拿到的文件：一致即同一份基座 |

本 fork 的组成（T2b 实测行数）：

| 文件 | 行数 | 与上游的关系 |
| --- | --- | --- |
| `Plugin.cs` | 719 | 上游 `Plugin.cs`（737 行）逐一删除全部游戏内卡片层接触点后的结果；T2 在此接上判定字段与载荷 |
| `SceneLifecycle.cs` | 122 | 上游原样（只把行尾统一为 CRLF） |
| `BridgeClient.cs` | 75 | 由上游 `Plugin.cs` 抽出的 HTTP 客户端与发送循环 |
| `Payload.cs` | 41 | 由上游 `Plugin.cs` 抽出的载荷记录 `Selection`；T2 扩到 11 字段 |
| `JudgeCapture.cs` | 711 | **T2 新增，T2b 扩到面板无关的 Pro**：`pro_judge` / `judge_level` / `turbo` 的三级读取链、字段发现诊断、逐字段变更日志与三条纯函数日志行 |

T2b 源码哈希（后续步骤改动这些文件后需重算）：

| 文件 | 字节 | SHA256 |
| --- | --- | --- |
| `Plugin.cs` | 40,689 | `B4D5AE423E281C25F187F80938381B9C8F053E99B32E63D9CC9DB9CE7C7BC5A2` |
| `SceneLifecycle.cs` | 4,814 | `F8483747D5CCE50C6FC621DC131B1D406DB10B9A603558AFC047A4E8CA649EF8` |
| `BridgeClient.cs` | 3,406 | `5FC35DC5C77B2A83A9BCFAA4D523903B419FBF2AC0C4AF21484FC3745F3DAC57` |
| `Payload.cs` | 2,143 | `AB6E6B3BD71D8DD9E5E31AE9F96ED13057D63F0B42CE3B5D39336B82D60D0147` |
| `JudgeCapture.cs` | 32,235 | `7A98D9877C9300C99B6F3168510C6B9A70E3272DB4F26BFEEE66D292A53BF85B` |
| `MMAMalodySelection.csproj` | 2,316 | `F22397612A79CD8481B33078FECBA2FB8D17B8900D26A373C782D61DB09043CA` |
| `NuGet.Config` | 302 | `0D4D1B3C7892CB6FA84B1899AA5FF40D4E34AF0101D533AD3AF486BE9D41433D` |
| `build.ps1` | 8,697 | `DE30864C7F8000E194290A51D279E74BC839B158986919B2DAA60C64C95A24B0` |
| `LICENSE` | 1,105 | `8D106A38B156260185AA8956111EDC283B06D2A3D274C8A57348379EDDFD0E1A` |

### 上游 `Plugin.cs` 里删掉了什么

共 20 处接触点：卡片层字段；两个抑制标志字段；卡片层那段 cfg 开关（段落名即"卡片层"一词、键为 `Enabled`、默认 `true`）以及紧随其后的组件挂载与初始化；`PanelJudge` 回调里的抑制标志赋值与刷新调用；`PanelMod` 回调里的抑制标志赋值与刷新调用；`Publish` 内的刷新调用；两个卡片层刷新方法（取快照后重绘、以及把场景与路径推给卡片层）；`ClearSelectionReferences()` 里的两行标志复位；`Unload()` 里的两行（关闭与销毁）；以及被删方法在 `PanelJudge`/`PanelMod` 两个回调里的两处 `self.Refresh…();` 调用点。

保留不动的部分：**23 处 `Patch(...)` 观察挂点与 `harmony!.Patch(...)` 那一行逐字节与上游一致**（`Plugin.cs` 中 `Patch(` 出现 25 次，与上游相同）；结构发现、场景分发、倍率读取、防抖与心跳、HTTP 收发、`Unload` 顺序全部保持上游行为。

程序集引用同步收敛：删掉 `UnityEngine` 的 IMGUI 与 TextRendering 两条模块引用（它们只服务卡片层绘制），`CheckBepInEx` 的第二条 `Error` 改为检查 `UnityEngine.CoreModule.dll`，否则构建会被永远拦住。

命名空间保留上游的 `MalodyInsightBridge`，这是有意的：计划要求 `SceneLifecycle.cs` 原样保留，改命名空间就必须同时改它，diff 会无谓变大；`RootNamespace` 属性按计划设为 `MMA.MalodySelection`。

### 曾同仓库存在过的另一份上游存档（已移除，与本 fork 无关）

`bridges/malody/bepinex/MalodyInsightBridge.dll` 与 `bridges/malody/bepinex/src/`（连同 `PROVENANCE.md` 与那份 `LICENSE`）曾在本仓库里，是上游 **`v1.01`** 的旧存档（DLL 115,712 字节，SHA256 `12DE9C2C4BE5FFFEDBAAA1F8B8B7CD256F22A04C28BE99E6A59AD3470E1ADF55`）。其源码 8 个 `.cs` 与本 fork 的基座（`v2.1`）**不是同一版**，两者不可互推。

**它们已从仓库移除**，因为：fork 分发的是我们自己的产物，留着那份 DLL 会让克隆者以为要安装它（两份插件共存会双 Hook）；且 MIT 只要求保留著作权与许可声明，该要求已由本目录的 `LICENSE` + 本文档承担，无需第二份。原件已归档到本地 `backup/20260924-malodyv-t8-asset-removal/`（不入库）。**全仓库只应有一个插件 DLL，即本 fork 编译出的 `MMAMalodySelection.dll`。**

## 2. BepInEx（LGPL-2.1）

| 项 | 值 |
| --- | --- |
| 版本 | `6.0.0-be.788`（发行名 `BepInEx-Unity.IL2CPP-win-x64-6.0.0-be.788`） |
| 源码提交 | `5b766a3b7f6c164d4798924a93f3acf4db769d06` |
| 许可 | GNU LGPL 2.1 |
| 源码 | <https://github.com/BepInEx/BepInEx/tree/5b766a3b7f6c164d4798924a93f3acf4db769d06> |

本插件只以程序集引用方式使用 `core\` 下的 `BepInEx.Core.dll`、`BepInEx.Unity.IL2CPP.dll`、`0Harmony.dll`、`Il2CppInterop.Runtime.dll`（`<Private>false</Private>`：不复制、不静态嵌入、不随本仓库分发其字节）。运行期由用户自行安装的 loader 提供这些程序集；loader 是外部可替换组件。

## 3. 上游作者

- **Entitley** —— <https://space.bilibili.com/3546767926758173>
- 上游**未托管于任何代码平台**（没有 GitHub / Gitee 之类的仓库地址可引），发行与说明只在 B 站发布，因此本文件以本地发行包路径与哈希作为可复现引用。
- 著作权与许可声明见同目录 `LICENSE`（MIT，`Copyright (c) 2026 Malody Insight contributors`）。

## 4. 构建配方

前置（缺一即失败，`build.ps1` 不静默降级）：

| 前置 | 说明 |
| --- | --- |
| .NET SDK | 目标框架 `net6.0`。脚本先找 PATH 上的 `dotnet`，再找 `C:\Program Files\dotnet\dotnet.exe`；都找不到则退出码 2 并打印下载地址 |
| `-BepInExDir` | BepInEx 6 Unity IL2CPP 的根目录（其下有 `core\`）。本机：`D:\Steam\steamapps\common\MalodyV\BepInEx`。缺失 ⇒ 退出码 3；指向错误目录 ⇒ 退出码 4，并逐条列出缺哪个 DLL |
| `-GameInteropDir` | 本机游戏生成的 interop 目录。本机：`D:\Steam\steamapps\common\MalodyV\BepInEx\interop`（134 个文件）。**interop 不能离线生成**：必须装好 loader 后至少启动该游戏一次，BepInEx 才会生成 `BepInEx\interop\`；换游戏版本或换机器都要重新生成 |
| 包源 | `NuGet.Config` 只留 nuget.org；包缓存固定落在 `bridges/malody/bepinex/.tools/nuget-packages`（`bridges/malody/bepinex/.tools/` 与另一个可能的 `bridges/malody/.tools/` 都是本地缓存，需要被 gitignore 覆盖） |

命令：

```powershell
pwsh -File bridges/malody/bepinex/plugin/build.ps1 `
    -BepInExDir "D:\Steam\steamapps\common\MalodyV\BepInEx" `
    -GameInteropDir "D:\Steam\steamapps\common\MalodyV\BepInEx\interop"
```

产物：`plugin/bin/Release/net6.0/MMAMalodySelection.dll`。脚本最后打印路径、字节数与 SHA256，并在出现任何 warning 时以退出码 5 失败（本插件要求 0 error / 0 warning）。

**脚本还会把产物复制成 `plugin/MMAMalodySelection.dll`（安装器读的就是这一份）**，并校验副本哈希与产物一致（不一致 ⇒ 退出码 7）。这一步不可省：`bin/` 是 gitignore 的，没有它的话新克隆的仓库会缺产物，安装器只能装到**上一版残留的 DLL**。若手改过产物请重新跑构建，不要只改副本。

### 可复现构建（为什么哈希是硬指标）

`.csproj` 里同时设了 `Deterministic`（SDK 默认）、**`ContinuousIntegrationBuild=true`** 与 **`PathMap`**。三者缺一不可：

- 只靠 `Deterministic=true` 时，编译器仍会把**构建机器的绝对源码/PDB 路径**写进程序集，且注入的 **MVID 每次构建都会变** —— 同一份源码两次构建得到**不同的 SHA256**，于是"产物与源码一致"只能是口头断言，无法核对。
- 加上 `ContinuousIntegrationBuild=true` 与 `PathMap`（把项目目录映射成固定前缀）后，**同一份源码的干净构建是稳定的**：实测**连续五次**从清空的 `obj/`+`bin/` 重建（其中一次是在跑过离线断言工程之后）**得到同一哈希** —— **72,704 字节 / `36542CB095726B5F975ACB41A14BED7FF255971BD3581542BB67A40EB8B18DD9`**，且与仓库里那份 `plugin/MMAMalodySelection.dll` 逐字节相同。

所以核对方式很直接：清空 `obj/`、`bin/` 后重新构建，哈希应与下表一致；不一致即说明源码或构建环境变了。

历史值（**均已不适用于当前源码**，留作记录）：T1 为 50,688 字节 / `DB9DAA598FF629390618311CE6CB9272C591E54E4BDC59468318458268A2DD57`（8 字段）；T2 为 71,168 字节 / `C3DD44165C5ECA19B041E93F27C2D4237F4A439E030294C28412ACDD977BF438`（11 字段）；T2b 为 72,704 字节 / `800DCDE39335DBB697F29E5C381F4C43CD40709D50382375E237230DEC5C8C99`（**这几次构建都在开启 `ContinuousIntegrationBuild` 之前，因此不可复现**）。<br>**当前（可复现）**：72,704 字节 / `36542CB095726B5F975ACB41A14BED7FF255971BD3581542BB67A40EB8B18DD9`。

### 源码 → 产物一致性的复算方法

1. **源码侧**：重算本节 §1 表格里各文件的行数与 SHA256，与记录比对。不一致说明源码被改过，产物不能沿用。
2. **产物侧**：连续跑两次上面的命令，比较两次产物的 SHA256（同一 SDK、同一绝对路径下应一致；换 SDK 或换路径不保证逐字节相同）。
3. **不依赖字节相等的交叉核对**（对产物做 IL/元数据与原始字符串扫描）：
   - 被删掉的那一族游戏内卡片层类型名（卡片视图类、私有字体光栅化类、卡片文字渲染类、卡片纹理探针类）——作为 TypeDef / TypeRef / MemberRef 与原始字符串都必须 0 命中；
   - `plugin/` 内不存在任何以卡片层一词命名的 `.cs` 源文件，且全树对卡片层相关字样（该词本身、其任意大小写、以及视图类的完整类型名）0 命中；
   - 11 个载荷键名（`path` / `speed_rate` / `screen` / `sequence` / `event` / `version` / `chart_hash` / `source` / `judge_level` / `pro_judge` / `turbo`）必须各命中一次。
4. **与上游二进制的关系**：上游 `v2.1` 的 `MalodyInsightBridge.dll`（159,744 字节）与本产物**不是同一版、也不等价**：它含游戏内卡片层与那三个 GDI+ 私有字体相关类型，本产物按设计全部没有。两者只能比"数据桥行为"，不能比字节。

### 离线断言（本地，不入库）

`plugin/tests/` 用 `<Compile Include="../Plugin.cs" />` 直接编译 fork 的源码，跑纯托管断言。T1 实测 **160 条全绿**；T2 为 198 条；**T2b 实测 206 条全绿**，覆盖：11 字段载荷的键名逐一断言（含空帧仍带全部 11 键、三个判定字段读不到时是 `null` 而不是默认值）、`SceneLifecycle` 的场景分类（selection / playing / result / other 四态及其边界）、判定档取值范围与 `MAX` 排除、`config.json` fallback 的解析、**用同名字段的替身把三级读取链整体跑一遍**（面板记录优先于播放记录、值类型每次重读、面板 Pro 关但记录 Pro 开时报告面板值、无名控件 bool 不被当成 Pro、无 Pro 标志的记录 ⇒ `null`、Turbo 只认已提交记录、文件只供判定档），以及**三条日志行文本**（`TierLine` / `MissingLine` / `DisagreementLine`）的逐字断言。原生取值本身需要游戏，属 T7 实机矩阵，本步不伪造。该目录**不入库**：它引用本机 BepInEx / interop 路径，属开发期脚手架。两组软降级断言（致命组 / 特性组）属后续步骤，本步不伪造。

## 5. 载荷字段来源与维护

`Selection`（`Payload.cs`）序列化时不设命名策略，**C# 成员名就是 JSON 键名**，与桌面壳的 `serde` 字段名一一对应：

| JSON 键 | 原生来源 | 读取点 |
| --- | --- | --- |
| `path` | `ItemChart.FilePath`，或 `ChartDir + FileName`；相对 `{game}\chart\` 归一化；候选不唯一即判失败（载荷退回空串） | `Plugin.ResolveChartPath` |
| `speed_rate` | 选曲期：Turbo 记录非空 ⇒ `Local.PlaySpeed`（两位小数）；否则按固定 Mod 掩码 Rush 1.5 / Dash 1.2 / Slow 0.8；进入游玩后改用 `SourcePlayInfo.PlaySpeed`（已提交值） | `Plugin.ReadAudioRate` / `Plugin.ReadFinitePlaySpeed` |
| `screen` | `SceneLifecycle.Screen`，闭集 `selection` / `playing` / `result` / `other` | `SceneLifecycle.cs` |
| `sequence` | 进程内单调递增序号，每次发布前自增 | `Plugin.sequence` |
| `event` | 触发本次发布的回调名（`ItemChart.SetSelected`、`SceneChart.Show`、`SceneManager.ToScene.Play` 等） | 各 Harmony 回调传入的 reason |
| `version` | `ItemChart.DisplayVersion`（谱面难度显示名） | `Plugin.Capture` / `Plugin.ReadPlayedChart` |
| `chart_hash` | `ItemChart.Hash` | 同上 |
| `source` | 常量 `malody-v-il2cpp` | `Payload.cs` |
| `judge_level` | 判定档 A~E 记为 0~4（枚举里的 `MAX`=5 **永不**作为档位上报）。三级读取链：① 判定记录 `PanelJudge.del` 的 `level`；② 与面板无关的 `Malody.Play.boq.Level`；③ 面板判定控件的具名状态；④ `config.json` 的 `user_judge_level`（仅 fallback，选曲期滞后但至少有值）。都读不到 ⇒ `null` | `JudgeCapture.ReadLevel`（含 `ReadPlayInfoLevel` / `FindControl` / `ReadSettingsLevel`） |
| `pro_judge` | Pro（严格组）开关，三级读取链：① 判定记录 `PanelJudge.del` 的 `proJudge`；② **与面板无关的播放设置记录 `Malody.Play.boq` 的 Pro 标志（`get_bcgh`）**，本局一次没开过面板也能读到；③ 面板 Pro 控件的**具名**状态访问器（控件只暴露无名 bool 时不取值——宁可为 `null` 也不猜）。**绝不来自 `config.json`**（文件里没有该键） | `JudgeCapture.ReadPro`（含 `ReadPlayInfoPro` / `FindControl`） |
| `turbo` | **已提交的 Turbo 记录非空**：`PanelTurbo.SaveToPlayInfo` 的返回类型在 `localPlayInfo` 上的对应 getter（与倍率读取同源），返回 `null` 即非 Turbo。**不得用 UI 开关 bool 判定**（上游实测教训：那个标志不是 Turbo 启用位） | `JudgeCapture.ReadTurbo` |

### 实测字段与生效级别（T2 + T2 实机会话）

T2 用只读元数据探针（`temp/t2-interop-probe/`，本地临时、不入库）复核了 interop 里的**真实成员名**，其中 Pro 的两处语义由随后的实机会话确认；以下为实测值，不再是推断：

| 事实 | 实测值 |
| --- | --- |
| 判定记录类型 | `Malody.Manager.brk+IntentJudge`，**值类型**；成员 `proJudge : Boolean`、`level : Malody.Play.JudgeLevel`、`speed : Single`、`allowSpeed`、`allowCustom`、`customJudge : Malody.Play.boa` |
| 面板上的记录入口 | `Malody.Scene.Panel.PanelJudge.del`（属性 `get_del`），类型即上面那个值类型 |
| 判定档枚举 | `Malody.Play.JudgeLevel`：`A=0 … E=4`、`MAX=5`（`MAX` 不是档位，`MapJudgeLevel` 与 `MaxMember` 一起把它挡掉） |
| 与面板无关的记录入口 | `Malody.Play.boq`（静态 `get_Local`，即本 fork 的 `localPlayInfo`）上的 `get_Level` → `JudgeLevel`（判定档）、`get_bcgh` → `Boolean`（**Pro**）；同类型 `get_bcgi` → `Malody.Play.boa` 即 Turbo 记录 |
| **Pro 的面板无关来源** | `boq.get_bcgh`。**实测确立**（2026-09-24 实机会话，证据 `.omo/evidence/malody-v-selection-bridge-fork/t2-session-bepinex-log.txt`）：四次快照里 `bcgh` 随面板 Pro 勾选精确变化（False → 面板开 Pro 后 True → 保持 True → 面板关 Pro 后 False），同期 `Level` 沿另一条轴变化（C → D → E → B），面板日志的 `pro_judge` 变换与之逐次吻合。**语义是由这四次相关性推断的**，因此 `OnDisable` 提交点会再各读一次面板记录与 `bcgh`，不一致时打一条 `Judge pro records differ after the commit` 并以面板值为准——这条日志就是它的复核机制 |
| Turbo 记录 | `Malody.Play.boa`（`PanelTurbo.SaveToPlayInfo` 的返回类型） |
| Pro 控件 | `PanelJudge.proToggle : Malody.UI.UIToggle`；该类型**没有 `Current`**，6 个成员里 4 个是无名 bool（`bajb` / `iml` / `muz` / `ctfx`） |
| 判定档控件 | `PanelJudge.judgeGroup : Malody.Scene.Widget.UITabIndicatorJudge`；只有无名 int（`dlf`） |
| 档位变更回调 | `PanelJudge.OnChangeJudge(Int32)`：可选挂点，只用来更早重读记录；该挂点缺失只少一点跟手速度，不影响取值 |

**生效级别（哪一级真正赢）**：

- `judge_level` —— 首选 ① 面板记录 `del.level`；面板没打开过时用 ② `Local.Level`（**与面板无关**，因此"本局一次没开过面板"也能拿到判定档）；再退 ③ 控件、④ 文件。
- `pro_judge` —— 首选 ① 面板记录 `proJudge`（面板是实时真相）；面板没打开过时用 ② `bcgh`（**与面板无关**，实测已确认，因此"本局一次没开过面板"同样能拿到 Pro）；再退 ③ 具名控件访问器。只有三级都读不到才是 `null`（fail-visible：壳侧据此关闭动态 OD 并在状态行明示，**绝不按常态组冒充**）。
- `turbo` —— 与面板无关，任何时刻都可读（读不到 `null`）。

**实机会话的结论（2026-09-24，哪一级真的赢）**：在**一次面板都没打开**的选曲段落里，日志给出

```
Judge judge_level=1 tier=play-record after PanelSongDesc.FillChartDiff.
Judge pro_judge=False tier=play-record after PanelSongDesc.FillChartDiff.
Judge turbo=False  tier=turbo-record after PanelSongDesc.FillChartDiff.
```

即 `judge_level` 与 `pro_judge` 都走了**与面板无关的播放设置记录**（②），`turbo` 走已提交 Turbo 记录。随后打开面板勾选 Pro，两条都切换成 `tier=panel-record` 并随勾选变化（`True` → 取消后 `False`），说明**面板在打开时确实是权威**、而**没打开时也不缺值**——这正是本项目"选曲界面直接出正确结果"的前提。证据：`.omo/evidence/malody-v-selection-bridge-fork/t2-session-bepinex-log.txt`。

**日志格式（每字段每次取值变化一行，可直接 grep `tier=`）**：

```
Judge <字段>=<值> tier=<级别> after <触发回调>[, committed].
Judge <字段>=null tier=none after <触发回调>: <为什么读不到>.
Judge level|pro records differ after the commit: the panel's judge record says <面板值>, the play settings record says <记录值>. The panel record is reported.
```

`tier` 取值：`panel-record`（面板判定记录） / `play-record`（与面板无关的播放设置记录） / `panel-control`（面板控件具名状态） / `settings-file`（`config.json` 判定档） / `turbo-record`（已提交 Turbo 记录） / `none`（都读不到）。行文本由 `JudgeCapture.TierLine` / `MissingLine` / `DisagreementLine` 三个纯函数生成，`plugin/tests/` 的 `judge-chain` 组逐一断言，改格式即会红灯。

⚠️ **尚未实机验证的对照项**：

1. `del` 拿到的是"面板提交后的记录"还是"面板被传入的那份意图"，即提交点 `del.level` 与 `Local.Level` 是否一致。插件在 `OnDisable`（提交点）会各读一次，不一致就打一条 `Judge level records differ`，以日志为准决定是否交换优先级。
2. `Malody.Play.boq` 上 `get_bcgg`（另一个 `JudgeLevel` getter）与 `get_Level` 谁是"当前值"；插件优先用具名的 `get_Level`，只有它是唯一候选时才用别的。
3. 若将来把某个无名控件访问器（例如 `bajb`）验证为 Pro 状态，把它加进 `JudgeCapture.StateAccessors` 即可生效（当前 Pro 已由 ①② 覆盖，控件路径只作后备）。

诊断开关：`Diagnostics.DumpPanelFields`（默认 `false`，落在 `BepInEx/config/local.mma.malody.selection.cfg`）。置 `true` 后每次打开判定面板会在日志里逐行打印：面板的全部字段/属性名与类型（控件成员附带其可读状态，`UIToggle` 会明确打印 `Current=absent`）、以及 play-info 类型上**全部无参 getter** 的返回类型与当前值——`get_bcgh` 就是这样被发现的。**保留**。

维护定位（字段改名时**只改 `JudgeCapture.cs` 一处**）：

- 判定记录与 `proJudge` / `level` 的成员名 ⇒ `JudgeCapture.Discover`（`GetProperty("del")` 与两处 `Mentions`；找不到具名成员时退化为按"类型里同时有 proJudge 布尔与 level 枚举"的结构发现）；
- 控件状态访问器的具名清单 ⇒ `JudgeCapture.StateAccessors`（**唯一一处**；实测到无名访问器的真实语义后加进这个数组即可）；
- 判定档枚举与 `MAX` ⇒ 不硬编码：`MaxMember` 从枚举里找 `MAX`，越界与 `MAX` 一律判 `null`（`MapJudgeLevel`）；
- 与面板无关的记录入口 ⇒ `JudgeCapture.PlayRecordLevelGetters` / `PlayRecordProGetters`（**各一处**：`get_Level` 与实测确立的 `get_bcgh`；`FindGetter` 只按名字取，不做"随便找个 bool"的猜测）；
- 载荷键名与字段增减 ⇒ 仍然只改 `Payload.cs`，并同步壳侧 `desktop/src/server/bridge.rs`。

其余文件各自的定位：

- 谱面原生属性改名 ⇒ 只改 `Plugin.cs` 里 `Text(chart, "<Property>")` 与结构发现处的 `get_*` 名字（判定字段不在此列，见上）；
- 场景状态机行为 ⇒ 只改 `SceneLifecycle.cs`（纯托管，`plugin/tests/` 可直接断言）；
- HTTP 目的地、超时、串行发送与告警节流 ⇒ 只改 `BridgeClient.cs`。

字段演进：T1 的载荷是 8 字段；T2 加入 `judge_level` / `pro_judge` / `turbo`，总数 **11**，全部可空（读不到就是 JSON `null`，壳侧三个字段都是 `#[serde(default)]` 的 `Option`）。
