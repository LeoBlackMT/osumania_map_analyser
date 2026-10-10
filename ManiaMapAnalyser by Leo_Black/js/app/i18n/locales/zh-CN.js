/**
 * Simplified Chinese (zh-CN) localization dictionary for ManiaMapAnalyser settings page.
 * Keeps all proper nouns untranslated per project conventions.
 */

export default {
    localeName: "简体中文",
    nav: {
        shellConfig: "客户端配置 (Shell)",
        cardSettings: "卡片设置 (Card)",
        presetsManager: "预设管理器 (Presets)"
    },
    status: {
        loading: "正在载入设置…",
        shellUnavailable: "无法连接至桌面端壳服务 (24061)",
        shellOnline: "tosu 已连接 —— 当前页面为只读状态（配置由 tosu 统一托管）。",
        shellOnlinePresets: "tosu 已连接 —— 设置在此为只读状态。请在 tosu 的预设页面中管理预设。",
        originNotice: "请通过桌面壳打开本页面: http://127.0.0.1:24061/settings.html",
        saving: "正在保存…",
        saved: "已保存。",
        ready: "就绪",
        saveFailed: "保存失败: ",
        unreadableNotice: "磁盘上的配置文件损坏或格式不正确 (HTTP 400)；已禁用修改。",
        refreshingPaths: "正在刷新生效的路径…",
        tosuDashboard: "tosu 仪表盘：",
        cardSettingsSub: "当 tosu 离线时，桌面端壳服务将这些配置保存在 mma-settings.json 中；Overlay 会立即生效所有更改。"
    },
    headers: {
        hLinks: "快捷链接 / Links",
        hPresets: "预设方案 / Presets",
        hModules: "模块自定义 / Modules Customization",
        hTheme: "主题与视觉效果 / Theme & Effects",
        hFunctions: "功能性选项 / Functionality Options",
        hNetwork: "网络与遥测 / Network Configuration",
        hDebug: "调试选项 / Debug Options"
    },
    shell: {
        title: "桌面壳配置 (Shell Configuration)",
        subtitle: "这些配置仅供 mma-shell 桌面运行环境读取 (保存在程序同级的 mma-shell-config.json)。",
        gameClient: {
            title: "目标游戏客户端",
            description: "选择 mma-shell 挂载的目标客户端。Auto 自动在运行中的 osu! (原生内存直读)、Etterna、Malody V 和 Malody 4.3.7 之间切换；指定特定客户端则禁用自动探测。"
        },
        roots: {
            etternaRoot: {
                title: "Etterna 根目录",
                description: "Etterna 游戏安装目录。留空表示由壳体自动扫描定位。"
            },
            malodyRoot: {
                title: "Malody V 根目录",
                description: "Malody V 游戏安装目录。留空表示由壳体自动扫描定位。"
            },
            malody4Root: {
                title: "Malody 4.3.7 根目录",
                description: "Malody 4.3.7 游戏安装目录。留空表示由壳体自动扫描定位。"
            }
        },
        hotkeys: {
            title: "全局快捷键",
            description: "按键格式：修饰键+按键 (如 Ctrl+Shift+T)。留空则禁用对应快捷键。",
            topmost: "窗口置顶切换 (Topmost)",
            clickThrough: "鼠标点击穿透 (Click-through)",
            close: "关闭悬浮窗",
            settings: "打开设置窗口"
        },
        logLevel: {
            title: "日志记录级别",
            description: "程序运行日志详细程度 (输出至程序同级 logs/ 目录下)。"
        },
        window: {
            title: "窗口位置与尺寸",
            description: "悬浮窗口当前的屏幕坐标与宽高尺寸。启动时加载，修改保存后即刻生效。",
            width: "宽度",
            height: "高度",
            x: "X 坐标",
            y: "Y 坐标"
        },
        offsets: {
            title: "内存偏移表管理 (Memory Offsets)",
            description: "用于 osu! (lazer & stable) 原生内存读取的布局偏移表运行状态。",
            stableLayout: "Stable 内存表状态: ",
            lazerLayout: "Lazer 内存表状态: ",
            actionsTitle: "偏移表操作",
            actionsDesc: "为 osu! 客户端更新或生成内存结构映射表。",
            liveGenerator: "活体生成器 (Live Generator)",
            checkRemote: "检查远程更新",
            genReadyTitle: "免 .NET SDK 直接从运行中的 osu! 提取并自校验偏移表",
            genDisabledTitle: "未检测到 gen.exe",
            updateTitle: "验证 Ed25519 签名清单并同步更新官方偏移表",
            generatingMsg: "正在扫描运行中的 osu! 并生成偏移表…",
            updatingMsg: "正在检查远程清单并验证签名…",
            genSuccess: "偏移表已成功生成并校验通过。",
            updateSuccessZero: "偏移表已是最新状态。",
            updateSuccessMulti: "已成功更新 {count} 份偏移表。"
        },
        shadow: {
            title: "对比影子诊断 (Native vs Tosu)",
            descEmpty: "等待对比数据帧… (在 osu! 中游玩谱面，且原生读取与 tosu 均处于活跃状态)",
            descPopulated: "{rate}% 匹配率 ({total} 帧中匹配 {matches} 帧，不匹配 {mismatches} 帧)",
            resetBtn: "重置诊断统计数据"
        }
    },
    cardSettings: {
        GuideButton: {
            title: "设置说明文档",
            description: "前往 GitHub 仓库查看各项设置的使用说明与背景信息。"
        },
        PresetGuideButton: {
            title: "预设说明文档",
            description: "前往 GitHub 仓库查看预设系统的详细用法与进阶配置指南。"
        },
        IssueButton: {
            title: "反馈问题 (Report Issue)",
            description: "前往 GitHub 提交 Bug 反馈或提出新功能建议。"
        },
        BenchmarkButton: {
            title: "基准测试结果 (Benchmark)",
            description: "查看算法难度估算的 Benchmark 跑分与表现数据。"
        },
        preset: {
            title: "快速预设 (Preset)",
            description: "快速应用覆盖下方设置的内置预设方案。选择 LastSavedPreset 沿用手动修改。可在 presets.html 页面管理与分享自定义预设。"
        },
        PresetButton: {
            title: "预设管理页面",
            description: "前往 presets.html 页面管理、导入与分享自定义预设。"
        },
        contentBar: {
            title: "卡片主体内容",
            description: "选择卡片主体展示的模块。Full 为同时展示 Pattern、Etterna 与 Graph 折线图。",
            options: {
                "None": "无 (None)",
                "Auto": "自动 (Auto)",
                "Pattern": "Pattern (键型分析)",
                "Etterna": "Etterna (MSD 雷达)",
                "Graph": "Graph (难度折线图)",
                "ReworkPP": "Rework PP",
                "Full": "完整展示 (Full)"
            }
        },
        srText: {
            title: "左上角胶囊文字",
            description: "选择卡片左上角胶囊显示的数值指标。",
            options: {
                "Auto": "自动 (Auto)",
                "ReworkSR": "Rework SR",
                "ReworkPP": "Rework PP",
                "InterludeSR": "Interlude SR",
                "MSD": "MSD",
                "Pattern": "Pattern"
            }
        },
        diffText: {
            title: "右上角展示模块",
            description: "选择卡片右上角展示的模块。",
            options: {
                "None": "无 (None)",
                "Graph": "Graph 折线图",
                "Difficulty": "Difficulty (难度评级)",
                "MSD": "MSD",
                "Pattern": "Pattern",
                "ReworkSR": "Rework SR",
                "InterludeSR": "Interlude SR"
            }
        },
        showModeTagCapsule: {
            title: "谱面属性胶囊",
            description: "在卡片左下角显示谱面属性标签胶囊 (HB/RC/LN/Mix/SV)。"
        },
        enableOsuTheme: {
            title: "osu!lazer 卡片主题",
            description: "使用 osu!lazer 风格的视觉主题卡片。"
        },
        useOsuFont: {
            title: "osu! 字体 (Torus)",
            description: "仅在分析卡片上启用 Osu Torus 官方英文字体。"
        },
        enableFloatingTriangles: {
            title: "漂浮三角形动效",
            description: "[需开启 Lazer 卡片主题] 在卡片背景后方呈现缓缓漂浮上升的三角形粒子。"
        },
        enableCoverArt: {
            title: "谱面背景曲绘",
            description: "[需开启 Lazer 卡片主题] 在内容区块后方铺设谱面背景。若关闭建议自定义背景颜色。"
        },
        customBackgroundColor: {
            title: "自定义背景颜色",
            description: "[需开启 Lazer 卡片主题] 使用固定自定义颜色代替谱面背景取色。设为纯黑 (#000000) 即为关闭。"
        },
        enableEtternaRainbowBars: {
            title: "彩虹条柱",
            description: "在 Etterna 模式下启用多彩条柱。若启用了 Lazer 主题建议关闭以保持视觉协调。"
        },
        enableStatusMarquee: {
            title: "元数据跑马灯",
            description: "当歌曲标题或艺术家文本过长时启用横向跑马灯平滑滚动。"
        },
        enableNumericDifficulty: {
            title: "数值难度评级",
            description: "在 RC 算法模式下显示精确数字难度分值。"
        },
        enableLNDifficulty: {
            title: "LN 星级评定",
            description: "[需开启 Improve Sunny LN Estimation] 在 LN 算法模式下显示 Rework 星级评定。"
        },
        reverseCardExtendDirection: {
            title: "反向展开卡片",
            description: "锚定卡片底部，在卡片展开时向上伸展（适用于卡片摆放在屏幕下方的场景）。"
        },
        cardVisibility: {
            title: "卡片可见性时机",
            description: "控制分析卡片的显隐时机。",
            options: {
                "DuringPlay": "仅游玩时显示 (DuringPlay)",
                "OutsidePlay": "仅非游玩时显示 (OutsidePlay)",
                "Always": "始终显示 (Always)"
            }
        },
        cardOpacity: {
            title: "卡片整体不透明度",
            description: "设置整张卡片的不透明度。"
        },
        cardBgBlur: {
            title: "内容背景高斯模糊",
            description: "Pattern、Etterna 和 Graph 区块背后曲绘的高斯模糊半径（需开启曲绘背景）。",
            options: {
                "Off": "关闭 (Off)",
                "4px": "4px (微弱)",
                "8px": "8px (柔和)",
                "12px": "12px (适中)",
                "16px": "16px (强烈)",
                "20px": "20px (深度)"
            }
        },
        cardRadius: {
            title: "卡片边角圆角",
            description: "设置卡片边角的圆润程度。",
            options: {
                "Small": "小圆角 (Small)",
                "Medium": "中圆角 (Medium)",
                "Large": "大圆角 (Large)"
            }
        },
        enableUpdateCheck: {
            title: "自动检测更新",
            description: "每天检测一次 GitHub 最新 Release。仅在发现新版本时在界面呈现星号提醒。"
        },
        enableResultCache: {
            title: "启用结果缓存 (Result Cache)",
            description: "缓存曲目分析结果，切回已分析谱面时瞬间恢复快照。若怀疑缓存陈旧可临时关闭。"
        },
        enablePauseDetection: {
            title: "暂停检测 (Pause Detection)",
            description: "推荐开启：游玩期间统计暂停次数并在 Graph 折线图上绘制暂停红线标记。"
        },
        VibroDetection: {
            title: "Vibro 震音检测",
            description: "推荐开启：识别超高频单键颤音/震音谱面 (Vibro)，并在需要时应用降级保护处理。"
        },
        useSvDetection: {
            title: "SV 变速检测",
            description: "开启基于 SV 的分类检测。启用后存在剧烈变速或停顿的谱面将被标记为 SV。"
        },
        display6kLevel: {
            title: "显示 6K 定数段位",
            description: "针对 6K 谱面在星级胶囊上叠加显示对应的段位定数。"
        },
        extendedEstimationRange: {
            title: "扩展估算范围 (Extended Range)",
            description: "仅在部分情况下对 Sunny 估算器有效。将启用扩展等级表，超出常规段位区间的精度不作保证。"
        },
        forceSunnyWindow: {
            title: "改进 Sunny LN 估算",
            description: "在分析前剥离谱面中的单键 (Rice) 部分，显著提升对纯面条长按 (LN) 难度的估算准确度。"
        },
        enableAnalyzeLN: {
            title: "细化分析 LN 组成",
            description: "[需开启 Improve Sunny LN Estimation] 分析谱面中各个小节的 LN/HB/Mix/RC 占比。"
        },
        estimatorAlgorithm: {
            title: "难度估算算法",
            description: "选择用于难度与段位计算的核心估算算法。",
            options: {
                "Mixed": "Mixed (自动推荐最佳算法)",
                "Azusa": "Azusa (4K RC 专用算法)",
                "Roxy": "Roxy (4K GBDT 元模型)",
                "Sunny": "Sunny (全键位 LN/RC 通用)",
                "Daniel": "Daniel (4K Reform Alpha+)",
                "Companella": "Companella (4K Etterna/ONNX 模型)"
            }
        },
        etternaVersion: {
            title: "全局 Etterna 版本",
            description: "选择 MSD 及相关技术指标所调用的 Etterna MinaCalc WASM 版本。"
        },
        companellaEtternaVersion: {
            title: "Companella 专用 Etterna 版本",
            description: "专门为 Companella 估算器指定的 MinaCalc 计算引擎版本。"
        },
        wsEndpoint: {
            title: "WebSocket 服务端地址",
            description: "设置 tosu / mma-shell 的通信地址 (host:port)，用于接收即时数据与谱面文件。"
        },
        enableTelemetry: {
            title: "匿名遥测统计 (Telemetry)",
            description: "向项目遥测服务器发送匿名统计（不包含用户名、无谱面信息、不记录 IP）。可随时在此关闭。"
        },
        DebugButton: {
            title: "调试控制台",
            description: "前往 debug.html 页面查看与管理调试信息。"
        },
        debugUseAmount: {
            title: "按数量确定主要形态",
            description: "按各键型元素数量进行降序排序，并使用主导键型数量作为分类兜底依据。"
        },
        azusaSunnyReferenceHo: {
            title: "Azusa 强制 HO 基准",
            description: "开启后，Azusa 始终使用 cvtFlag=HO 调用 Sunny 基准，消除含少量面条谱面的 LN 虚高影响。"
        },
        enableAlwaysShowLNDifficulty: {
            title: "始终显示 LN 难度",
            description: "即使 LN 占比极低或面条过少，也始终强制显示 LN 难度指标。"
        },
        presetStorage: {
            title: "预设内部存储 (JSON)",
            description: "自定义预设的底层 JSON 存储。除非了解内部格式，否则请勿手动修改。"
        }
    },
    presets: {
        title: "预设管理器",
        systemCategory: "系统内置预设",
        customCategory: "我的自定义预设",
        createBtn: "新建预设",
        importBtn: "导入预设",
        exportAllBtn: "导出全部",
        applyBtn: "应用此预设",
        saveBtn: "保存修改",
        deleteBtn: "删除预设",
        exportBtn: "导出当前预设",
        nameLabel: "预设名称",
        descLabel: "预设描述",
        versionLabel: "版本号",
        fieldsIncludedLabel: "快照包含的设置字段"
    }
};
