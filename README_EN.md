# osumania_map_analyser
**English | [中文](README.md)**

> **Translation Note**: This document was translated from Chinese to English with the assistance of an AI language model. While efforts have been made to ensure accuracy, please refer to the original Chinese version if any ambiguity arises.
****
This repository is an entirely AI-crafted in-game overlay (ppcounter) for [tosu](https://tosu.app), providing real-time support for osu!mania (4/6/7K/Lazer/Stable) across multiple mods. It offers estimated difficulty, RC/LN pattern analysis, customizable Etterna version for MSD calculation, difficulty graphs, and pause detection.

![Features](img/features.gif)

<details>
<summary>Theme Screenshots</summary>
<img src="img/themeLN.jpg" alt="LN" width="400">
<img src="img/themeRC.jpg" alt="RC" width="400">
<img src="img/full.jpg" alt="Full" width="400">
</details>

## Key Features
- **Real-time Analysis**: Analyzes various data of the current beatmap in real-time during gameplay or beatmap selection.
- **Multi-mod Support**: Compatible with multiple mods in both lazer and stable, supporting custom speed multipliers and OD adjustments.
- **Customizable Etterna Version**: Allows users to select different versions of [Etterna](https://github.com/etternagame/etterna) MinaCalc for calculations.
- **Pause Detection**: Detects pauses during gameplay and marks their positions on the graph.
- **Difficulty Estimation**: Estimates difficulty based on beatmap data and provides detailed analysis results, offering multiple estimation algorithms. Compatible with LN and RC Dans for 4/6/7K.
- **Graph Visualization**: Provides difficulty variation graphs to help players better understand the difficulty distribution of a beatmap.
- **Pattern Analysis**: Analyzes RC/LN pattern distribution in the beatmap to help players understand its structure.
- **Rework PP**: Provides the Rework PP performance panel with Max PP/Live PP, Proportion, and multiplier bars, updating in real-time during play and results.
- **SV Detection**: Detects whether a beatmap is an SV (speed variation) map.
- **Highly Customizable**: Offers a wealth of customization options to meet the needs of different players.

## Usage
1. Go to the [Release](https://github.com/LeoBlackMT/osumania_map_analyser/releases/latest) page and download the latest version.
2. Extract the downloaded file to any location.
3. Place the entire folder in the `static` directory of tosu if you haven't modified the `Counters Directory` setting.
4. Launch tosu, go to the dashboard, and you will find the "ManiaMapAnalyser" plugin. Click the `Settings` button on the right to configure it.
5. For instructions on using the in-game interface and OBS, please refer to the relevant tosu documentation.

## Estimator Algorithm Benchmark
- The benchmark has been migrated to the separate repository [VSRG-DanEstimation-Benchmark](https://github.com/LeoBlackMT/VSRG-DanEstimation-Benchmark), and the results can be viewed [here](https://benchmark.leoblack.top/). The tests cover the performance of multiple algorithms across different types of beatmaps, helping players choose the one that suits them best.
- It is important to note that while the benchmark provides a reference for algorithm performance, actual usage may be influenced by various factors such as beatmap characteristics and mod combinations. Players are encouraged to combine the benchmark results with their own gameplay experience for judgment.
- You can download the beatmap data used for benchmarking [here](https://github.com/LeoBlackMT/VSRG-DanEstimation-Benchmark/tree/main/samples/samples.7z). However, please read the disclaimer and use the data responsibly.

## Notes
1. The plugin needs to run in the `static` directory of tosu. Ensure it is placed directly in that directory, not nested inside another folder.
2. This plugin relies on the correct parsing of beatmap data. Certain special or non-standard beatmaps may lead to inaccurate analysis results.
3. If game lag causes false positives, consider increasing the pause detection threshold.
4. Although the difficulty estimation algorithms have been tuned, inaccuracies may still exist; please use them only as a reference. For 4K, high difficulties are generally more accurate with an overall error of no more than half a Dan, while low difficulties may be less accurate; in specific patterns like Minijack, Stamina, and Anchor, the estimation results may have larger deviations. For 6K and 7K, the overall performance is relatively average. It is recommended that players combine the estimation results with their actual gameplay experience for judgment and not rely too heavily on the estimates.
5. The plugin's performance may be affected by the complexity of the beatmap and the features selected; in some cases, lag or delays may occur. Please adjust the settings according to your actual situation for a better experience.
6. If you encounter any issues, feel free to submit an issue.

## Settings
Note: It is recommended to start with the default settings and then adjust according to personal preference.
See [docs/settings.md](docs/settings.md#english) for detailed settings instructions.

## Roxy Algorithm Explanation
Roxy is a 4K RC meta-structural estimator. Its core consists of two layers: the first layer performs structural analysis on the beatmap across 7 aspects, producing structured numerical difficulty; the second layer blends reference predictions from Azusa/Sunny/Daniel using a GBDT (Gradient Boosted Decision Tree) meta-model to output the final difficulty.
Please note that as a tree-based model, the GBDT meta-model can exhibit boundary discontinuities: a miniscule change in input features (e.g., a 0.01× speed rate difference) may cross a decision tree split threshold and produce a disproportionately large jump in the output difficulty. Users should be aware of this inherent characteristic of tree-based estimators.

## Azusa Algorithm Explanation
This algorithm builds on the beatmap itself, combining the results of Daniel and Sunny Rework, with specific adjustments targeted at 4K RC beatmaps. For more details, please refer to [this document](docs/azusa_algorithm.md).

# Contribution Guide
See [CONTRIBUTING.md](CONTRIBUTING.md) for details.

## References
- [tosu](https://tosu.app): The runtime environment and basic framework for this plugin.
- [Etterna](https://github.com/etternagame/etterna): Etterna's MinaCalc is used for difficulty estimation and MSD calculation.
- [Sunny Rework](https://github.com/sunnyxxy/Star-Rating-Rebirth): Sunny Rework's algorithm is used for difficulty estimation.
- [Interlude](https://github.com/YAVSRG/YAVSRG): Interlude's RC pattern analysis algorithm is used, with LN detection logic added on top.
- [Daniel](https://thebagelofman.github.io/Daniel/): Daniel's algorithm is used for difficulty estimation.
- [Companella](https://github.com/Leinadix/companella): Companella's algorithm is used for difficulty estimation.

## Contributors
- [inuiyumegan](https://github.com/inuiyumegan): Provided a large amount of beatmap data for algorithm debugging and benchmarking.
- [greycsont](https://github.com/greycsont): Contributed several features.
- [ZHAO20060708](https://github.com/ZHAO20060708): Provided the polished Lazer theme and Full mode design.
- [SST-03](https://github.com/SST-03) & [AkutaZehy](https://github.com/AkutaZehy): Provided the improved Sunny LN algorithm.

---------
This page has been viewed since June 21, 2026, thanks for your support!
![:maniamapanalyser](https://count.getloli.com/@:maniamapanalyser?name=%3Amaniamapanalyser&theme=rule34&padding=7&offset=0&align=center&scale=1&pixelated=1&darkmode=auto)
