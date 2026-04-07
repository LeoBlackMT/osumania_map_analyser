# osumania_map_analyser
**[English](README_EN.md) | 中文**

> **Translation Note**: This document was translated from Chinese to English with the assistance of an AI language model. While efforts have been made to ensure accuracy, please refer to the original Chinese version if any ambiguity arises.
****
This repository is an in-game overlay (pp counter) for [tosu](https://tosu.app), providing real-time support for osu!mania (4/6/7K) across both lazer and stable with multiple mods. It offers estimated difficulty analysis, RC/LN pattern analysis, customizable Etterna version for MSD calculation, difficulty charts, and pause detection.
![Features](img/features.gif)

## Key Features
- **Real-time Analysis**: Analyzes various data of the current beatmap in real-time during gameplay/selection.
- **Multi-mod Support**: Compatible with multiple mods in both lazer and stable, supporting custom speed changes and OD adjustments.
- **Customizable Etterna Version**: Allows users to select different versions of [Etterna](https://github.com/etternagame/etterna) MinaCalc for calculations.
- **Pause Detection**: Detects pauses during gameplay and marks their positions on the chart.
- **Difficulty Estimation**: Estimates difficulty based on beatmap data and provides detailed analysis results, offering multiple difficulty estimation algorithms. Compatible with LN and RC Dans for 4/6/7K.
- **Chart Visualization**: Provides difficulty variation charts to help players better understand the difficulty distribution of a beatmap.
- **Pattern Analysis**: Analyzes RC/LN pattern distribution in the beatmap to help players understand its structure.
- **SV Detection (Experimental)**: Detects whether a beatmap is an SV map.
- **Highly Customizable**: Offers a wealth of customization options to meet the needs of different players.

## Usage
1. Go to the [Release](https://github.com/LeoBlackMT/osumania_map_analyser/releases/latest) page and download the latest version.
2. Extract the downloaded file to any location.
3. Place the entire folder in the `static` directory of tosu.
4. Launch tosu, go to the dashboard, and you will find the "ManiaMapAnalyser" plugin. Click the `Settings` button on the right to configure it.
5. For instructions on using the in-game interface and OBS, please refer to the relevant tosu documentation.

## Notes
1. The plugin needs to run in the `static` directory of tosu. Ensure it is placed directly in that directory, not nested inside another folder.
2. This plugin relies on the correct parsing of beatmap data. Certain special or non-standard beatmaps may lead to inaccurate analysis results.
3. The pause detection feature may produce false positives or misses during game lag.
4. The SV detection feature is experimental and may have a high rate of false positives; use it with caution.
5. Although the difficulty estimation algorithms have been tuned, inaccuracies may still exist; please use them only as a reference. For 4K, high difficulties are generally more accurate with an overall error of no more than half a Dan, while low difficulties may be less accurate; in specific patterns like Minijack, Stamina, and Anchor, the estimation results may have larger deviations. For 6K and 7K, the overall performance is relatively average. It is recommended that players combine the estimation results with their actual gameplay experience for judgment and not rely too heavily on the estimates.
6. The plugin's performance may be affected by the complexity of the beatmap and the features selected; in some cases, lag or delays may occur. Please adjust the settings according to your actual situation for a better experience.
7. If you encounter any issues, feel free to submit an issue.

## Settings
Note: It is recommended to start with the default settings and then adjust according to personal preference.
- **Visual Settings**:
    - **Card Body Content**: Select what to display in the main body of the card.
        - None: Displays nothing, i.e., short card mode.
        - Auto: Automatically selects Pattern or Etterna based on the LN ratio of the beatmap.
        - Pattern: Displays pattern analysis.
        - Etterna: Displays Etterna's 7 major skill set breakdowns.
        - Graph: Displays the difficulty variation chart.
        - Note: For non-4/6/7K beatmaps, body content automatically falls back to Pattern.
    - **Top-left Capsule Text**: Select what to display in the top-left capsule.
        - Auto: Automatically selects ReworkSR or MSD based on the LN ratio of the beatmap.
        - ReworkSR: Displays [Suuny Rework](https://github.com/sunnyxxy/Star-Rating-Rebirth) star rating.
        - MSD: Displays Etterna MSD. *Only compatible with 4/6/7K beatmaps.
        - InterludeSR: Displays [Interlude](https://github.com/YAVSRG/YAVSRG) star rating.
        - Pattern: Displays the overall pattern.
    - **Top-right Content**: Select what to display in the top-right.
        - None: Displays nothing.
        - Graph: Displays the difficulty variation chart.
        - Difficulty: Displays the estimated difficulty.
        - MSD: Displays Etterna MSD. *Only compatible with 4/6/7K beatmaps.
        - InterludeSR: Displays Interlude star rating.
        - ReworkSR: Displays Suuny Rework star rating.
        - Pattern: Displays the overall pattern.
    - **Map Tag Capsule**: Whether to display the beatmap tag capsule.
        - Includes HB/RC/LN/Mix/SV tags.
        - Automatically determined based on beatmap characteristics.
    - **Metadata Marquee**: Whether to enable scrolling for beatmap metadata.
    - **Rainbow Bars**: Whether to enable rainbow bars under the Etterna display.
    - **Numeric Difficulty**: Whether to display numerical difficulty.
        - When enabled, numerical difficulty will be shown after the "ESTIMATE DIFFICULTY" label under the RC estimation algorithm.
    - **Hide During Play**: Whether to hide the card during gameplay.
    - **Card Opacity**: Set overall card opacity.
        - Available values: 100% / 95% / 90% / 80% / 70%.
    - **Card Blur**: Set card background blur strength.
        - Off: Disable blur.
        - Soft: Light blur (default).
        - Strong: Strong blur.
    - **Card Radius**: Set card corner roundness.
        - Small / Medium / Large.
    - **Show Title Icon**: Whether to show the icon on the left side of the status line.
    - **Reverse Card Extension**: Reverse card expansion direction.
        - When enabled, the card stays anchored at the bottom and grows upward; when disabled, it expands downward by default.
- **Functional Settings**:
    - **Pause Detection**: Whether to enable pause detection.
        - Recommended: When enabled, pause positions will be displayed on the difficulty chart, and the number of pauses will be shown in the bottom right corner.
    - **Vibro Detection**: Whether to enable vibro detection.
        - Recommended: When enabled, the plugin will detect if a beatmap is a vibro map and display it as "Vibro" in the estimated difficulty; otherwise, you will see an extremely inflated difficulty estimate.
    - **Estimator Algorithm**: Choose the algorithm used for difficulty estimation.
        - Mixed: (Recommended) A hybrid algorithm combining the three below, offering relatively higher accuracy. Automatically selects the algorithm best suited for the current beatmap.
        - Suuny: Maps directly to Dan star ratings using Suuny Rework, compatible with LN and RC Dans for 4/6/7K.
        - [Daniel](https://thebagelofman.github.io/Daniel/): Uses the Daniel algorithm for estimation, suitable for 4K Reform Alpha and above Dan difficulties.
        - [Companella](https://github.com/Leinadix/companella): Uses the Companella algorithm, suitable for 4K Reform Delta+ and below Dan difficulties.
    - **Global Etterna Version**: Select the Etterna MinaCalc version used for MSD and related calculations.
        - Different versions of Etterna may yield different MSD results; you can choose your preferred version.
        - The default value 0.72.3 is recommended.
        - Changing this setting will affect all features that depend on Etterna calculations, except for the Companella estimation algorithm.
        - 4K uses the selected version directly; 6K/7K prefer 0.74.0 for stability.
        - If the current version is unavailable or does not support the key mode, it will automatically fall back to an available version.
    - **Companella Etterna Version**: Select the Etterna MinaCalc version used exclusively for the Companella estimation algorithm.
        - This setting only affects the calculations of the Companella algorithm; other features will continue to use the version set in Global Etterna Version.
        - The default value is 0.74.0. It is recommended to keep this setting at 0.74.0, as Companella was developed and calibrated based on Etterna 0.74.0's MinaCalc.
        - You can switch to other versions to observe their performance with the Companella algorithm, but please be aware that results may be inaccurate.
- **Network Configuration**:
    - **WebSocket Endpoint**: Configure the address and port of the WebSocket server.
        - Ensure this address and port match those set in-game to receive data from the in-game overlay.
        - Adjusting this setting allows you to use the plugin on other devices on the same local network, such as displaying analysis results on a mobile phone or tablet.
        - The default value is `localhost:24050`
- **Debug Settings**:
    - **Use Amount For Category**: Whether to enable pattern classification logic based on the beatmap's cluster amount.
        - When enabled, pattern classification will be based on the number of objects in the beatmap, which **may** more accurately identify certain beatmaps.
    - **SV Detection**: Whether to enable SV beatmap detection (experimental).
        - When enabled, an SV tag will be displayed in the bottom left corner.
        - This feature is experimental and may produce false positives or misses.

## References
- [tosu](https://tosu.app): The runtime environment and basic framework for this plugin.
- [Etterna](https://github.com/etternagame/etterna): Etterna's MinaCalc is used for difficulty estimation and MSD calculation.
- [Suuny Rework](https://github.com/sunnyxxy/Star-Rating-Rebirth): Suuny Rework's algorithm is used for difficulty estimation.
- [Interlude](https://github.com/YAVSRG/YAVSRG): Interlude's RC pattern analysis algorithm is used, with new LN detection added.
- [Daniel](https://thebagelofman.github.io/Daniel/): Daniel's algorithm is used for difficulty estimation.
- [Companella](https://github.com/Leinadix/companella): Companella's algorithm is used for difficulty estimation.