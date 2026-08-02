# osumania_map_analyser
**[English](README_EN.md) | 中文**
****
本仓库是一个纯AI打造的 [tosu](https://tosu.app) 游戏内叠加界面(ppcounter)，实时在 osu!mania（4/6/7K/Lazer/Stable）及其各种mod下，提供估算难度、分析RC/LN键型、自定义ett版本计算MSD、难度图表和暂停检测功能。

![Features](img/features.gif)

<details>
<summary>更新: 新主题效果图</summary>
<img src="img/themeLN.jpg" alt="LN" width="400">
<img src="img/themeRC.jpg" alt="RC" width="400">
<img src="img/full.jpg" alt="Full" width="400">
</details>

## 主要特性
- **实时分析**：在游戏/选图过程中实时分析当前谱面的各项数据。
- **多mod支持**：兼容lazer与stable的多个mod，支持自定义倍速与改变OD。
- **自定义Ett版本**：允许用户选择不同版本的[Etterna](https://github.com/etternagame/etterna) MinaCalc进行计算。
- **暂停检测**：在游玩过程中检测暂停次数并在图表上显示暂停位置。
- **难度估计**：基于谱面数据估算难度，并提供详细的分析结果。同时提供多种难度估计算法。适配4/6/7K的LN与RC段位。
- **图表可视化**：提供难度变化图，帮助玩家更好地理解谱面难度分布。
- **键型分析**：分析谱面中的RC/LN键型分布，帮助玩家了解谱面结构。
- **SV检测**：检测谱面是否为SV谱面。
- **高度自定义**：提供丰富的自定义选项，满足不同玩家的需求。

## 使用方法
1. 前往[Release](https://github.com/LeoBlackMT/osumania_map_analyser/releases/latest)下载最新版本。
2. 将下载的文件解压到任意位置。
3. 将整个文件夹放置在 tosu 的 `static` 目录下。
4. 启动 tosu，进入 dashborad，即可找到 "ManiaMapAnalyser" 插件，可以点击右侧`Settings`按钮进行相关设置。
5. 游戏内界面以及OBS的使用方法见 tosu 相关文档。

## 难度估计算法基准测试
- 该插件的难度估计算法经过了基于真实谱面数据的基准测试，测试结果可以在[此处](https://leoblackmt.github.io/osumania_map_analyser/)查看。测试涵盖了多个算法在不同类型谱面上的表现，帮助玩家选择适合自己的算法。
- 需要注意的是，虽然基准测试提供了算法表现的参考，但实际使用中可能会受到谱面特征、mod组合等多种因素的影响，建议玩家结合自己的游玩体验进行判断。
- 你可以在[此处](https://github.com/LeoBlackMT/osumania_map_analyser/tree/main/docs/data/files.7z)下载用于基准测试的谱面数据，但是请注意阅读免责声明，合理使用这些数据。

## 注意事项
1. 插件需要在 tosu 的 `static` 目录下运行，注意不要嵌套文件夹，确保正确放置。
2. 由于 tosu 不支持中文设置选项，为求统一性，其余所有内容均使用英文。
3. 本插件依赖于谱面数据的正确解析，某些特殊或非标准的谱面可能会导致分析结果不准确。
4. 如果游戏卡顿导致误判，可适当提高暂停检测阈值。
5. 难度估计算法虽然经过调整，但仍然可能存在不准确的情况，请仅将其作为参考。对于4K，一般情况下高难相对比较准确，整体误差不超过半个段位，低难相对没那么准确；在Minijack、Stamina和Anchor等键型中，估计结果可能会有较大的偏差。对于6K和7K，整体表现相对一般。建议玩家结合自己的实际游玩体验进行判断，不要过于依赖估计结果。
6. 该插件的性能可能会受到谱面复杂度和所选功能的影响，在某些情况下可能会出现卡顿或延迟的情况，请根据实际情况调整设置以获得更好的体验。
7. 如果存在问题欢迎提交issue。

## 设置说明
注意：推荐直接使用默认设置开始体验，之后再根据个人喜好进行调整。
- **模块设定**：
    - **Card Body Content**：选择在卡片主体显示的内容。
        - None: 不显示任何内容。即短卡片模式。
        - Auto: 根据谱面LN占比自动选择显示Pattern或Etterna。
        - Pattern: 显示键型分析。
        - Etterna: 显示Etterna 7大键型分。
        - Graph: 显示难度变化图。
        - Full: 显示完整内容，包括键型分析、难度图表和Etterna分数。不推荐日常使用，可能会比较拥挤。
        - 注：对于非4/6/7K谱面，主体内容将自动回退为Pattern显示。
    - **Top-left Capsule Text**：选择在卡片左上角胶囊显示的内容。
        - Auto: 根据谱面LN占比自动选择显示ReworkSR或MSD。
        - ReworkSR: 显示[Sunny Rework](https://github.com/sunnyxxy/Star-Rating-Rebirth)星数。
        - MSD: 显示Etterna MSD。*仅适配4/6/7K谱面。
        - InterludeSR: 显示[Interlude](https://github.com/YAVSRG/YAVSRG)星数。
        - Pattern: 显示整体键型。
    - **Top-right Content**: 选择在卡片右上角显示的内容。
        - None: 不显示任何内容。
        - Graph: 显示难度变化图。
        - Difficulty: 显示估计难度。
        - MSD: 显示Etterna MSD。*仅适配4/6/7K谱面。
        - InterludeSR: 显示Interlude星数。
        - ReworkSR: 显示Sunny Rework星数。
        - Pattern: 显示整体键型。
    - **Map Tag Capsule**: 是否显示谱面标签胶囊。
        - 包含HB/RC/LN/Mix/SV标签。
        - 根据谱面特征自动判断。
- **主题与效果**：
    - **osu!Lazer Card Theme**: 是否启用Lazer风格的卡片主题。
        - 启用后将使用类似于osu!lazer的卡片设计风格，并启用部分仅在Lazer主题下可用的设置项。
    - **osu! Font**: 是否仅在卡片内使用内置 Torus 字体。
    - **Floating Triangles Animation**: 是否启用卡片背景的浮动三角形动画效果。
        - 该选项仅在启用Lazer Card Theme时生效。
    - **Cover Art Background**: 是否将谱面背景图作为卡片背景。
        - 该选项仅在启用Lazer Card Theme时生效。
        - 启用后卡片颜色主题将从谱面背景图中提取，增强视觉效果；禁用后将使用纯色背景，建议配合Custom Background Color设置使用。
    - **Custom Background Color**: 设置卡片的自定义背景颜色。
        - 该选项仅在启用Lazer Card Theme时生效。
        - 当使用谱面背景图作为卡片背景时，也可以通过该选项设置一个自定义颜色。
        - 设置为纯黑色（#000000）将禁用该功能并从谱面背景采样颜色或使用深黑色背景色。
    - **Rainbow Bars**: 是否启用Etterna下的彩虹条
        - 建议在启用Lazer Card Theme时禁用该选项，以获得更统一的视觉效果。
    - **Metadata Marquee**: 是否启用滚动显示谱面信息功能。
    - **Numeric Difficulty**: 是否显示数值化难度。
        - 将在RC估计算法下于ESTIMATE DIFFICULTY字样后显示数值化难度。
    - **Card Visibility**: 控制卡片的显示时机。
        - 可选值：`DuringPlay`（仅在游玩时显示）、`OutsidePlay`（仅在非游玩时显示）、`Always`（始终显示）。默认值为 `Always`。
    - **Reverse Card Extension**: 是否反转卡片延展方向。
        - 启用后卡片底边保持锚定，扩展时向上生长；关闭时默认向下扩展。
    - **Card Opacity**: 设置卡片整体透明度。
        - 可选范围：100% / 95% / 90% / 80% / 70%。
    - **Content Background Blur**: 是否启用背景图片模糊效果。
         - 启用后卡片内容区域的背景将会有模糊效果，增强内容的可读性和视觉层次感。
    - **Card Radius**: 设置卡片圆角大小。
        - Small / Medium / Large。
- **功能性设置**：
    - **Pause Detection**: 是否启用暂停检测功能。
        - 推荐启用：启用后将在难度图表上显示暂停位置，并在卡片右下角显示暂停次数。
    - **Enable Update Check**: 是否启用版本更新检查。
        - 启用后默认每天最多检查一次 GitHub latest release。
        - 当从“关闭”切换到“开启”时，会立即额外触发一次检查。
        - 当发现新版本时，状态栏左侧星形图标会显示。
    - **Vibro Detection**: 是否启用Vibro检测功能。
        - 推荐启用：启用后将检测谱面是否为Vibro谱面，并在估计难度中显示为Vibro。否则您将看到被极度拉高的难度估计。
    - **SV Detection**: 是否启用SV谱面检测功能。
        - 启用后当检测到变速时，将在左下角显示SV标签。
        - 注意：如果未开启显示谱面标签胶囊，SV标签将不会显示。
    - **Pause Detection Threshold**: 设置暂停检测的时间阈值（毫秒）。
        - 只有当游戏时间冻结超过该时长后，才会被判定为一次暂停。
        - 默认值为500ms。如果游戏卡顿导致误判，可适当提高该值。
    - **Estimator Algorithm**: 选择用于难度估计的算法。
        - Mixed: (推荐)综合下方四个算法的混合算法，准确度相对较高。自动选择适配当前谱面的算法。
        - Azusa: 面向4K RC的融合算法，综合了下方算法并进行了针对调整，在RC场景下表现较好，但不适用于LN主体的谱面。
        - Roxy: 面向4K RC的元结构估算器。使用结构分析对谱面进行建模，再通过GBDT元模型融合Azusa/Sunny/Daniel的参考预测。
        - Sunny: 使用Sunny Rework直接映射段位星数，适配4/6/7K的LN与RC段位。
        - [Daniel](https://thebagelofman.github.io/Daniel/): 使用Daniel算法进行估计，适配4K Reform Alpha及以上段位难度。
        - [Companella](https://github.com/Leinadix/companella): 使用Companella算法进行估计，适用于4K Reform Delta+及以下段位难度。
    - **Global Etterna Version**: 选择用于MSD以及相关计算的Etterna MinaCalc版本。
        - 不同版本的Etterna会有不同的MSD计算结果，可以选择个人喜好的版本。
        - 个人推荐使用默认值0.72.3。
        - 改动该设定将会影响所有依赖于Etterna计算的功能，除了Companella估计算法以外。
        - 4K将按所选版本计算；6K/7K为保证稳定性会优先使用0.74.0。
        - 若当前版本不可用或不支持当前键数，将自动回退到可用版本。
    - **Companella Etterna Version**: 选择仅用于Companella估计算法的Etterna MinaCalc版本。
        - 该设定将仅影响Companella算法的计算结果，其他功能仍然使用Global Etterna Version设定的版本。
        - 默认值为0.74.0。推荐将该设定保持在0.74.0，因为Companella是基于Etterna 0.74.0的MinaCalc进行开发和调校的。
        - 你可以切换其他版本来观察不同版本的Etterna在Companella算法下的表现，但请注意可能会出现不准确的情况。
- **网络配置**:
    - **WebSocket Endpoint**: 配置WebSocket服务器的地址和端口。
        - 确保该地址和端口与 tosu 内设置一致，以用于接收来自 tosu 的数据。
        - 同时用于拼接谱面文件请求地址：`http://{host:port}/files/beatmap/file`。
        - 调整该项可用于在局域网内的其他设备上使用该插件，例如在手机或平板上显示分析结果。
        - 默认值为`localhost:24050`
- **调试内容**:
    - **Use Amount For Category**：是否启用基于谱面Cluster Amount的键型分类逻辑。
        - 启用后将根据谱面物件数量进行键型分类，**可能**会更准确地识别某些谱面。
    - **Azusa Sunny Reference Force HO**
        - 启用后将强制Azusa算法将谱面视为纯米。
        - 默认启用，请不要随意关闭。

## Roxy 算法说明
Roxy 是一个 4K RC 元结构估算器。其核心分为两层：第一层对谱面进行 7 个方面结构分析，产出结构化数值难度；第二层通过 GBDT（梯度提升决策树）元模型融合 Azusa/Sunny/Daniel 的参考预测值，输出最终难度。
请注意，由于使用了树模型进行决策，GBDT 元模型可能会存在边界不连续的情况：输入特征的微小变化（例如倍速相差0.01）可能会跨过决策树的分割阈值，导致输出难度出现不成比例的大跳跃。用户在使用时应当注意到这是树模型固有的特性。

## Azusa 算法说明
该算法在谱面本身的基础上，融合了Daniel和Sunny Rework的结果，并针对4K RC谱面进行了特定的调整。如有需要，请前往[此处](azusa_algorithm.md)(英文)查看详细说明。

## 参考内容
- [tosu](https://tosu.app): 本插件的运行环境和基础框架。
- [Etterna](https://github.com/etternagame/etterna): 使用了Etterna的MinaCalc进行难度估计和MSD计算。
- [Sunny Rework](https://github.com/sunnyxxy/Star-Rating-Rebirth): 使用了Suuny Rework的算法进行难度估计。
- [Interlude](https://github.com/YAVSRG/YAVSRG): 使用了Interlude的RC键型分析算法并在基础上新增LN检测算法。
- [Daniel](https://thebagelofman.github.io/Daniel/): 使用了Daniel的算法进行难度估计。
- [Companella](https://github.com/Leinadix/companella): 使用了Companella的算法进行难度估计。

## 特别感谢
- [inuiyumegan](https://github.com/inuiyumegan): 提供了大量谱面数据用于算法调试和Benchmark。
- [greycsont](https://github.com/greycsont): 提供了部分功能。
- [ZHAO20060708](https://github.com/ZHAO20060708): 提供了精美的Lazer主题和Full模式。

---------
本页累计访问量，自2026/6/21起统计，感谢大家的支持！
![:maniamapanalyser](https://count.getloli.com/@:maniamapanalyser?name=%3Amaniamapanalyser&theme=rule34&padding=7&offset=0&align=center&scale=1&pixelated=1&darkmode=auto)