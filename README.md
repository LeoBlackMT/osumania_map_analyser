# osumania_map_analyser
**[English](README_EN.md) | 中文**
****
本仓库是一个 [tosu](https://tosu.app) 游戏内叠加界面(ppcounter)，实时提供 osu!mania（4/6/7K） 下，lazer与stable的多个mod，提供估算难度、分析RC/LN键型、自定义ett版本计算MSD、难度图表和暂停检测功能。
****
## 主要特性
- **实时分析**：在游戏/选图过程中实时分析当前谱面的各项数据。
- **多mod支持**：兼容lazer与stable的多个mod，支持自定义倍速与改变OD。
- **自定义Ett版本**：允许用户选择不同版本的[Etterna](https://github.com/etternagame/etterna) MinaCalc进行计算。
- **暂停检测**：在游玩过程中检测暂停次数并在图表上显示暂停位置。
- **难度估计**：基于谱面数据估算难度，并提供详细的分析结果。同时提供多种难度估计算法。适配4/6/7K的LN与RC段位。
- **图表可视化**：提供难度变化图，帮助玩家更好地理解谱面难度分布。
- **键型分析**：分析谱面中的RC/LN键型分布，帮助玩家了解谱面结构。
- **SV检测(实验性)**：检测谱面是否为SV谱面。
- **高度自定义**：提供丰富的自定义选项，满足不同玩家的需求。
****
## 使用方法
1. 前往[Release](https://github.com/LeoBlackMT/osumania_map_analyser/releases/latest)下载最新版本。
2. 将下载的文件解压到任意位置。
3. 将整个文件夹放置在 tosu 的 `static` 目录下。
4. 启动 tosu，进入 dashborad，即可找到 "ManiaMapAnalyser" 插件，可以点击右侧`Settings`按钮进行相关设置。
5. 游戏内界面以及OBS的使用方法见 tosu 相关文档。
****
## 设置说明
- **视觉设定**：
    - **Card Body Content**：选择在卡片主体显示的内容。
        - None: 不显示任何内容。即短卡片模式。
        - Auto: 根据谱面LN占比自动选择显示Pattern或Etterna。
        - Pattern: 显示键型分析。
        - Etterna: 显示Etterna 7大键型分。
        - Graph: 显示难度变化图。
    - **Top-left Capsule Text**：选择在卡片左上角胶囊显示的内容。
        - Auto: 根据谱面LN占比自动选择显示ReworkSR或MSD。
        - ReworkSR: 显示[Suuny Rework](https://github.com/sunnyxxy/Star-Rating-Rebirth)星数。
        - MSD: 显示Etterna MSD。*仅适配4/6/7K谱面。
        - InterludeSR: 显示[Interlude](https://github.com/YAVSRG/YAVSRG)星数。
        - Pattern: 显示整体键型。
    - **Top-right Content**: 选择在卡片右上角显示的内容。
        - None: 不显示任何内容。
        - Graph: 显示难度变化图。
        - Difficulty: 显示估计难度。
        - MSD: 显示Etterna MSD。*仅适配4/6/7K谱面。
        - InterludeSR: 显示Interlude星数。
        - ReworkSR: 显示Suuny Rework星数。
        - Pattern: 显示整体键型。
    - **Map Tag Capsule**: 是否显示谱面标签胶囊。
        - 包含HB/RC/LN/Mix/SV标签。
        - 根据谱面特征自动判断。
    - **Metadata Marquee**: 是否启用滚动显示谱面信息功能。
    - **Rainbow Bars**: 是否启用Etterna下的彩虹条。
    - **Numeric Difficulty**: 是否显示数值化难度。
        - 将在RC估计算法下于ESTIMATE DIFFICULTY字样后显示数值化难度。
    - **Hide During Play**: 是否在游玩过程中隐藏卡片。
- **功能性设置**：
    - **Pause Detection**: 是否启用暂停检测功能。
        - 推荐启用：启用后将在难度图表上显示暂停位置，并在卡片右下角显示暂停次数。
    - **Vibro Detection**: 是否启用Vibro检测功能。
        - 推荐启用：启用后将检测谱面是否为Vibro谱面，并在估计难度中显示为Vibro。否则您将看到被极度拉高的难度估计。
    - **Estimator Algorithm**: 选择用于难度估计的算法。
        - Mixed: (推荐)综合下方三个算法的混合算法，准确度相对较高。自动选择适配当前谱面的算法。
        - Suuny: 使用Suuny Rework直接映射段位星数，适配4/6/7K的LN与RC段位。
        - [Daniel](https://thebagelofman.github.io/Daniel/): 使用Daniel算法进行估计，适配4K Reform Alpha及以上段位难度。
        - [Companella](https://github.com/Leinadix/companella): 使用Companella算法进行估计，适用于4K Reform Delta+及以下段位难度。
    - **Global Etterna Version**: 选择用于MSD以及相关计算的Etterna MinaCalc版本。
        - 不同版本的Etterna会有不同的MSD计算结果，可以选择个人喜好的版本。
        - 个人推荐使用默认值0.72.3。
        - 改动该设定将会影响所有依赖于Etterna计算的功能，除了Companella估计算法以外。
    - **Companella Etterna Version**: 选择仅用于Companella估计算法的Etterna MinaCalc版本。
        - 该设定将仅影响Companella算法的计算结果，其他功能仍然使用Global Etterna Version设定的版本。
        - 默认值为0.74.0。推荐将该设定保持在0.74.0，因为Companella是基于Etterna 0.74.0的MinaCalc进行开发和调校的。
        - 你可以切换其他版本来观察不同版本的Etterna在Companella算法下的表现，但请注意可能会出现不准确的情况。
- **网络配置**:
    - **WebSocket Endpoint**: 配置WebSocket服务器的地址和端口。
        - 确保该地址和端口与游戏内设置一致，以用于接收来自游戏内叠加界面的数据。
        - 调整该项可用于在局域网内的其他设备上使用该插件，例如在手机或平板上显示分析结果。
        - 默认值为`localhost:24050`
- **调试内容**:
    - **Use Amount For Category**：是否启用基于谱面Cluster Amount的键型分类逻辑。
        - 启用后将根据谱面物件数量进行键型分类，**可能**会更准确地识别某些谱面。
    - **SV Detection**: 是否启用SV谱面检测功能（实验性）。
        - 启用后将在左下角显示SV标签。
        - 该功能处于实验阶段，会有误判或漏判的情况。
****
## 注意事项
1. 插件需要在 tosu 的 `static` 目录下运行，注意不要嵌套文件夹，确保正确放置。
2. 由于 tosu 不支持中文设置项标题，为求统一性，所有内容均使用英文。
3. 本插件依赖于谱面数据的正确解析，某些特殊或非标准的谱面可能会导致分析结果不准确。
4. 暂停检测功能可能会在游戏卡顿时出现误判或漏判的情况。
5. SV检测功能处于实验阶段，可能会有较高的误判率，请谨慎使用。
6. 难度估计算法虽然经过调整，但仍然可能存在不准确的情况，请仅将其作为参考。
7. 该插件的性能可能会受到谱面复杂度和所选功能的影响，在某些情况下可能会出现卡顿或延迟的情况，请根据实际情况调整设置以获得更好的体验。
8. 如果存在问题欢迎提交issue。
****
## 参考内容
- [tosu](https://tosu.app): 本插件的运行环境和基础框架。
- [Etterna](https://github.com/etternagame/etterna): 使用了Etterna的MinaCalc进行难度估计和MSD计算。
- [Suuny Rework](https://github.com/sunnyxxy/Star-Rating-Rebirth): 使用了Suuny Rework的算法进行难度估计。
- [Interlude](https://github.com/YAVSRG/YAVSRG): 使用了Interlude的RC键型分析算法并在基础上新增LN检测算法。
- [Daniel](https://thebagelofman.github.io/Daniel/): 使用了Daniel的算法进行难度估计。
- [Companella](https://github.com/Leinadix/companella): 使用了Companella的算法进行难度估计。