# osumania_map_analyser
**[English](README_EN.md) | 中文**
****
本仓库是一个纯AI打造的 [tosu](https://tosu.app) 游戏内叠加界面(ppcounter)，实时在 osu!mania（4/6/7K/Lazer/Stable）及其各种mod下，提供估算难度、分析RC/LN键型、自定义ett版本计算MSD、难度图表和暂停检测功能。

![Features](img/features.gif)

<details>
<summary>主题效果图</summary>
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
- **Rework PP**：提供Rework PP难度表现面板，显示Max PP/Live PP、Proportion及各乘子柱状图，游玩/结算时实时更新。
- **预设系统**：提供系统与自定义预设，一键应用/保存整套配置，支持自动跟随手动修改。
- **SV检测**：检测谱面是否为SV谱面。
- **高度自定义**：提供丰富的自定义选项，满足不同玩家的需求。

## 使用方法
1. 前往[Release](https://github.com/LeoBlackMT/osumania_map_analyser/releases/latest)下载最新版本。
2. 将下载的文件解压到任意位置。
3. 将整个文件夹放置在 tosu 的 `static` 目录下，如果你没有修改`Counters Directory`设置项的话。
4. 启动 tosu，进入 dashborad，即可找到 "ManiaMapAnalyser" 插件，可以点击右侧`Settings`按钮进行相关设置。
5. 游戏内界面以及OBS的使用方法见 tosu 相关文档。

## 难度估计算法基准测试
- 基准测试已迁移至独立仓库 [VSRG-DanEstimation-Benchmark](https://github.com/LeoBlackMT/VSRG-DanEstimation-Benchmark)，测试结果可以在[此处](https://benchmark.leoblack.top/)查看。测试涵盖了多个算法在不同类型谱面上的表现，帮助玩家选择适合自己的算法。
- 需要注意的是，虽然基准测试提供了算法表现的参考，但实际使用中可能会受到谱面特征、mod组合等多种因素的影响，建议玩家结合自己的游玩体验进行判断。
- 你可以在[此处](https://github.com/LeoBlackMT/VSRG-DanEstimation-Benchmark/tree/main/samples/samples.7z)下载用于基准测试的谱面数据，但是请注意阅读免责声明，合理使用这些数据。

## 注意事项
1. 插件需要在 tosu 的 `static` 目录下运行，注意不要嵌套文件夹，确保正确放置。
2. 由于 tosu 不支持中文设置选项，为求统一性，其余所有内容均使用英文。
3. 本插件依赖于谱面数据的正确解析，某些特殊或非标准的谱面可能会导致分析结果不准确。
4. 如果游戏卡顿导致误判，可适当提高暂停检测阈值。
5. 难度估计算法虽然经过调整，但仍然可能存在不准确的情况，请仅将其作为参考。对于4K，一般情况下高难相对比较准确，整体误差不超过半个段位，低难相对没那么准确；在Minijack、Stamina和Anchor等键型中，估计结果可能会有较大的偏差。对于6K和7K，整体表现相对一般。建议玩家结合自己的实际游玩体验进行判断，不要过于依赖估计结果。
6. 该插件的性能可能会受到谱面复杂度和所选功能的影响，在某些情况下可能会出现卡顿或延迟的情况，请根据实际情况调整设置以获得更好的体验。
7. 如果存在问题欢迎提交issue。

## 隐私 / 匿名使用统计
本插件默认开启匿名使用统计，用于统计活跃用户数、在线分布与使用行为。上报内容**不含**用户名、玩家 id、分数、谱面标识（标题/md5）、IP 地址等任何个人信息，数据仅用于生成聚合统计。公开看板见：https://mma-stats.leoblack.top/

如不希望上报，可在 tosu 设置 → Network 分组中关闭 "Anonymous Usage Statistics"。

## 设置说明
注意：推荐直接使用默认设置开始体验，之后再根据个人喜好进行调整。
见 [docs/settings.md](docs/settings.md) 了解详细设置说明。
预设系统教程见 [docs/presets-guide.md](docs/presets-guide.md)。

## Roxy 算法说明
Roxy 是一个 4K RC 元结构估算器，聚焦高难区间（数值难度 11~17，即段位 Alpha 至 Emik Zeta high）。其核心分为两层：第一层对谱面进行 7 个方面结构分析，产出结构化数值难度；第二层通过 Ridge 线性元模型融合 Azusa/Daniel 的参考预测，并在最终输出上与 Azusa 预测按 0.4/0.6 加权平均（降低方差），输出最终难度。元模型按段位 0.5 序数刻度校准（纯内部变换，不依赖谱面之外的信息），使结果更贴近段位判定。
低于 Alpha（< 11）的谱面 Roxy 返回 "< Alpha Low"（不输出数值难度），达到或超过 Emik Zeta high（>= 17）返回 "> Emik Zeta high"；Mixed 算法会自动将低难谱面路由至 Azusa 估算，因此 Roxy 的低难估算不作为最终结果。

## Azusa 算法说明
该算法在谱面本身的基础上，融合了Daniel和Sunny Rework的结果，并针对4K RC谱面进行了特定的调整。如有需要，请前往[此处](docs/azusa_algorithm.md)(英文)查看详细说明。

## 贡献指南
详见 [CONTRIBUTING.md](CONTRIBUTING.md)。

## 参考内容
- [tosu](https://tosu.app): 本插件的运行环境和基础框架。
- [Etterna](https://github.com/etternagame/etterna): 使用了Etterna的MinaCalc进行难度估计和MSD计算。
- [Sunny Rework](https://github.com/sunnyxxy/Star-Rating-Rebirth): 使用了Sunny Rework的算法进行难度估计。
- [Interlude](https://github.com/YAVSRG/YAVSRG): 使用了Interlude的RC键型分析算法并在基础上新增LN检测算法。
- [Daniel](https://thebagelofman.github.io/Daniel/): 使用了Daniel的算法进行难度估计。
- [Companella](https://github.com/Leinadix/companella): 使用了Companella的算法进行难度估计。

## 贡献者
- [inuiyumegan](https://github.com/inuiyumegan): 提供了大量谱面数据用于算法调试和Benchmark。
- [greycsont](https://github.com/greycsont): 提供了部分功能。
- [ZHAO20060708](https://github.com/ZHAO20060708): 提供了精美的Lazer主题和Full模式。
- [SST-03](https://github.com/SST-03) & [AkutaZehy](https://github.com/AkutaZehy): 提供了改进的 Sunny LN 算法。

---------
本页累计访问量，自2026/6/21起统计，感谢大家的支持！
![:maniamapanalyser](https://count.getloli.com/@:maniamapanalyser?name=%3Amaniamapanalyser&theme=rule34&padding=7&offset=0&align=center&scale=1&pixelated=1&darkmode=auto)
