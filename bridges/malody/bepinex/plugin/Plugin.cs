using System.Globalization;
using System.Reflection;
using BepInEx;
using BepInEx.Unity.IL2CPP;
using HarmonyLib;

namespace MalodyInsightBridge;

/// <summary>
/// Observes the real selection UI. All native reads happen in Harmony callbacks on
/// the game thread; the worker receives only immutable managed strings and numbers.
/// No gameplay, chart, account, score, or selection value is ever changed.
/// </summary>
[BepInPlugin(Id, "MMA Malody V Selection Bridge", "1.0.0")]
public sealed class Plugin : BasePlugin
{
    public const string Id = "local.mma.malody.selection";
    private static Plugin? instance;
    private readonly object stateLock = new();
    private readonly SceneLifecycle lifecycle = new();
    private readonly CancellationTokenSource stop = new();
    private Harmony? harmony;
    private BridgeClient? bridge;
    private Timer? timer;
    private string gameRoot = "";
    private string chartRoot = "";
    private double debounceSeconds;
    private double heartbeatSeconds;
    private bool verbose;
    private bool traceEvents;
    private object? scene;
    private object? currentChart;
    // SceneChart is loaded independently. Keep selection-entry instances from
    // their real OnEnable callbacks and re-read visibility on every observation.
    // A return may create a new entry instance without ToScene(Inventory).
    private readonly Dictionary<string, object> selectionParents = new();
    private MethodInfo? localPlayInfo;
    private MethodInfo? turboInfoGetter;
    private MethodInfo? resultPlayInfoGetter;
    private MethodInfo? selectionChartGetter;
    private object? judgePanel;
    private JudgeCapture? judgeCapture;
    private Type? chartType;
    private long sequence;
    private Selection snapshot = Selection.Hidden(0, "startup");
    private DateTime due = DateTime.MinValue;
    private DateTime lastSend = DateTime.MinValue;

    public override void Load()
    {
        if (!Config.Bind("Bridge", "Enabled", true, "Observe the song selection UI.").Value)
        {
            Log.LogInfo("Selection bridge disabled by configuration.");
            return;
        }

        int port = Config.Bind("Bridge", "Port", 17653, "Companion service port on 127.0.0.1 only.").Value;
        string route = Config.Bind("Bridge", "SelectionPath", "/selection", "Companion selection endpoint path.").Value;
        bridge = new BridgeClient(port, route, Log);
        debounceSeconds = Math.Clamp(Config.Bind("Bridge", "DebounceSeconds", 0.12, "Wait after a selection change before posting. Existing user values are retained.").Value, 0.05, 2.0);
        heartbeatSeconds = Math.Clamp(Config.Bind("Bridge", "HeartbeatSeconds", 2.0, "Resend current selection; companion should expire stale state.").Value, 0.5, 5.0);
        verbose = Config.Bind("Diagnostics", "Verbose", false, "Log selection event names and chart paths.").Value;
        traceEvents = Config.Bind("Diagnostics", "TraceEvents", false, "Log native callback entry and capture guards while validating this game build.").Value;
        bool dumpPanelFields = Config.Bind("Diagnostics", "DumpPanelFields", false, "Dump the judge panel's fields and the play settings getters with their values; used to discover where a field lives.").Value;
        gameRoot = Path.GetFullPath(Paths.GameRootPath);
        chartRoot = Path.GetFullPath(Path.Combine(gameRoot, "chart")) + Path.DirectorySeparatorChar;


        instance = this;
        harmony = new Harmony(Id);
        Assembly assembly = Assembly.Load("Assembly-CSharp");
        Type item = RequiredType(assembly, "Malody.Scene.Item.ItemChart");
        MethodInfo itemChart = RequiredMethod(item, "get_Chart", 0);
        chartType = itemChart.ReturnType;
        RequiredMethod(chartType, "get_FilePath", 0);
        RequiredMethod(chartType, "get_ChartDir", 0);
        RequiredMethod(chartType, "get_FileName", 0);

        // Structural discovery avoids hard-coding the obfuscated chart/play-info names.
        localPlayInfo = SafeTypes(assembly)
            .Where(t => t.Namespace == "Malody.Play")
            .Select(t => t.GetMethod("get_Local", BindingFlags.Public | BindingFlags.Static))
            .FirstOrDefault(m => m != null &&
                m.GetParameters().Length == 0 &&
                m.ReturnType.GetMethod("get_Chart")?.ReturnType == chartType &&
                m.ReturnType.GetMethod("get_Mod")?.ReturnType.IsEnum == true);
        if (localPlayInfo == null) throw new MissingMethodException("Cannot locate local chart ModMask provider.");

        // Discover the scene dispatcher from its retained API and enum names;
        // neither the obfuscated manager name nor numeric scene IDs are assumed.
        MethodInfo currentScene = SafeTypes(assembly)
            .Where(t => t.Namespace == "Malody.Scene")
            .Select(t => t.GetMethod("get_CurrentScene", BindingFlags.Public | BindingFlags.Static))
            .OfType<MethodInfo>()
            .Single(m => m.ReturnType.IsEnum && new[] { "Song", "Play", "Result" }
                .All(name => Enum.GetNames(m.ReturnType).Contains(name)));
        MethodInfo changeScene = currentScene.DeclaringType!.GetMethods(BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static)
            .Single(m => m.Name == "ToScene" && m.GetParameters().Length == 1 &&
                m.GetParameters()[0].ParameterType == currentScene.ReturnType);
        lifecycle.ChangeScene(currentScene.Invoke(null, null)?.ToString() ?? "Unknown");
        Patch(changeScene, nameof(SceneChanging), prefix: true);

        Patch(RequiredMethod(item, "SetSelected", 0), nameof(ItemSelected));
        Type description = RequiredType(assembly, "Malody.Scene.Panel.PanelSongDesc");
        selectionChartGetter = description.GetMethods(AllInstance)
            .Single(m => m.Name.StartsWith("get_") && m.GetParameters().Length == 0 && m.ReturnType == chartType);
        MethodInfo fill = description.GetMethods(AllInstance).Single(m => m.Name == "FillChartDiff" &&
            m.GetParameters().Length == 1 && m.GetParameters()[0].ParameterType == chartType);
        Patch(fill, nameof(ChartDescriptionFilled));

        Type selection = RequiredType(assembly, "Malody.Scene.SceneChart");
        RequiredMethod(selection, "get_SongDesc", 0);
        RequiredMethod(selection, "get_IsShow", 0);
        foreach (string name in new[] { "SceneSong", "SceneInventory" })
        {
            Type selectionScene = RequiredType(assembly, "Malody.Scene." + name);
            Patch(RequiredMethod(selectionScene, "OnEnable", 0), nameof(SelectionSceneShown));
        }
        Patch(RequiredMethod(selection, "OnEnable", 0), nameof(SelectionShown));
        Patch(RequiredMethod(selection, "Show", 1), nameof(SelectionShown));
        Patch(RequiredMethod(selection, "OnDisable", 0), nameof(SelectionHidden));
        Patch(RequiredMethod(selection, "HideAsync", 0), nameof(SelectionHidden), prefix: true);
        Patch(RequiredMethod(selection, "OnSpeedChanged", 1), nameof(SpeedChanged));
        Patch(RequiredMethod(selection, "ChangeCustomSpeed", 0), nameof(SpeedChanged));
        MethodInfo? speedSetter = chartType.GetMethod("set_Speed", AllInstance);
        if (speedSetter != null) Patch(speedSetter, nameof(ChartSpeedChanged));
        Type modPanel = RequiredType(assembly, "Malody.Scene.Panel.PanelMod");
        Patch(RequiredMethod(modPanel, "OnModSelected", 1), nameof(SpeedChanged));
        Patch(RequiredMethod(modPanel, "Show", 1), nameof(ModPanelChanged));
        Patch(RequiredMethod(modPanel, "OnDisable", 0), nameof(ModPanelChanged));
        Type turboPanel = RequiredType(assembly, "Malody.Scene.Panel.PanelTurbo");
        MethodInfo saveTurbo = RequiredMethod(turboPanel, "SaveToPlayInfo", 0);
        turboInfoGetter = localPlayInfo.ReturnType.GetMethods(AllInstance)
            .Single(m => m.Name.StartsWith("get_") && m.GetParameters().Length == 0 && m.ReturnType == saveTurbo.ReturnType);
        Patch(RequiredMethod(turboPanel, "Setup", 1), nameof(TurboSetup));
        Patch(saveTurbo, nameof(TurboSaved));
        Type judge = RequiredType(assembly, "Malody.Scene.Panel.PanelJudge");
        Patch(RequiredMethod(judge, "Show", 1), nameof(JudgeChanged));
        Patch(RequiredMethod(judge, "OnChangeTurbo", 0), nameof(JudgeChanged));
        Patch(RequiredMethod(judge, "OnDisable", 0), nameof(JudgeChanged));
        // The level control reports its new index through this method. Observing it
        // only re-reads the judge record sooner; a build without it loses that head
        // start and nothing else, so the hook is optional.
        MethodInfo? changeJudge = judge.GetMethods(AllInstance)
            .FirstOrDefault(m => m.Name == "OnChangeJudge" && m.GetParameters().Length == 1 &&
                m.GetParameters()[0].ParameterType == typeof(int));
        if (changeJudge != null) Patch(changeJudge, nameof(JudgeChanged));
        else Log.LogInfo("Judge level change callback is absent in this game build; the level is still read from the record.");

        judgeCapture = new JudgeCapture(Log, dumpPanelFields, assembly, gameRoot, localPlayInfo, turboInfoGetter);

        Type player = RequiredType(assembly, "Malody.Play.ChartPlayer");
        Patch(RequiredMethod(player, "Load", 1), nameof(EnteringPlay), prefix: true);
        RequiredMethod(player, "get_SourcePlayInfo", 0);
        Patch(RequiredMethod(player, "Play", 0), nameof(PlayingStarted));
        Type resultScene = RequiredType(assembly, "Malody.Scene.SceneResult");
        resultPlayInfoGetter = resultScene.GetMethods(AllInstance)
            .Single(m => m.Name.StartsWith("get_") && m.GetParameters().Length == 0 &&
                m.ReturnType == localPlayInfo.ReturnType);
        Patch(RequiredMethod(resultScene, "OnEnable", 0), nameof(ResultShown));
        // Result.Start fills the score asynchronously after the component is
        // enabled. Observe this per-result boundary too, including reused UI.
        // Other Fill overloads belong to historical/multiplayer score views.
        Type resultScore = RequiredType(assembly, "Malody.Scene.Panel.PanelResultScore");
        MethodInfo resultFill = resultScore.GetMethods(AllInstance)
            .Single(m => m.Name == "Fill" && m.GetParameters().Length == 2 &&
                m.GetParameters()[1].ParameterType == localPlayInfo.ReturnType);
        Patch(resultFill, nameof(ResultScoreFilled));


        timer = new Timer(_ => Pump(), null, 50, 50);
        Log.LogInfo($"Selection bridge ready: {bridge.Endpoint}. Chart type: {chartType.FullName}; audio rate provider: {localPlayInfo.DeclaringType!.FullName} (Turbo PlaySpeed, otherwise selected Mod).");
    }

    private const BindingFlags AllInstance = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance;
    private static Type RequiredType(Assembly assembly, string name) => assembly.GetType(name, throwOnError: true)!;
    private static MethodInfo RequiredMethod(Type type, string name, int count) =>
        type.GetMethods(AllInstance).Single(m => m.Name == name && m.GetParameters().Length == count);
    private static IEnumerable<Type> SafeTypes(Assembly assembly)
    {
        try { return assembly.GetTypes(); }
        catch (ReflectionTypeLoadException ex) { return ex.Types.OfType<Type>(); }
    }

    private void Patch(MethodInfo target, string callback, bool prefix = false)
    {
        var observer = new HarmonyMethod(typeof(Plugin).GetMethod(callback, BindingFlags.NonPublic | BindingFlags.Static)!);
        harmony!.Patch(target, prefix: prefix ? observer : null, postfix: prefix ? null : observer);
        Log.LogInfo($"Observe {target.DeclaringType!.FullName}.{target.Name}");
    }

    // Each callback catches its own failure: an observer must never interrupt the game.
    private static void ItemSelected(object __instance) => Guard(() =>
    {
        if (!instance!.lifecycle.CanSelectCharts) return;
        instance!.Trace($"ItemSelected instance={__instance?.GetType().FullName}");
        instance.Capture(__instance == null ? null : Get(__instance, "Chart"), "ItemChart.SetSelected");
    });
    private static void ChartDescriptionFilled(object __instance, object[] __args) => Guard(() =>
    {
        instance!.Trace("ChartDescriptionFilled args=" + string.Join(",", __args.Select(a => a?.GetType().FullName ?? "null")));
        if (!instance.BindDescriptionPanel(__instance))
        {
            instance.Trace("ChartDescriptionFilled ignored: description has no current active panel owner");
            return;
        }
        instance.Capture(__args.Length > 0 ? __args[0] : null, "PanelSongDesc.FillChartDiff");
    });
    private static void SelectionShown(object __instance, MethodBase __originalMethod) => Guard(() =>
    {
        Plugin self = instance!;
        self.Trace($"SelectionShown callback={__originalMethod.DeclaringType?.FullName}.{__originalMethod.Name}; panel={NativeDescription(__instance)}; destination={self.lifecycle.Destination}");
        self.TraceSelectionOwners("child-" + __originalMethod.Name, __instance);
        self.TracePanelHierarchy(__instance);
        if (!self.BindSelectionPanel(__instance)) return;
        self.CaptureVisiblePanel("SceneChart.Show");
    });
    private static void SelectionSceneShown(object __instance, MethodBase __originalMethod) => Guard(() =>
    {
        Plugin self = instance!;
        string destination = __originalMethod.DeclaringType!.Name == "SceneInventory" ? "Inventory" : "Song";
        // This entry is not SceneChart's Transform parent: in the verified game
        // its ChartCanvas field is null and SceneChart has its own Unity scene.
        self.selectionParents[destination] = __instance;
        self.Trace($"Selection parent recorded destination={destination}; callback={__originalMethod.DeclaringType?.FullName}.{__originalMethod.Name}; parent={NativeDescription(__instance)}; count={self.selectionParents.Count}");
        self.TraceSelectionOwners("parent-OnEnable", null);
        if (!IsComponentActive(__instance)) return;
        if (!self.BindSelectionPanel(self.scene)) return;
        self.Trace($"SelectionSceneShown destination={destination}; visible={IsSelectionPanelVisible(self.scene)}");
        self.CaptureVisiblePanel(destination + ".OnEnable");
    });
    private static void SelectionHidden(object __instance) => Guard(() =>
    {
        Plugin self = instance!;
        self.Trace($"SelectionHidden destination={self.lifecycle.Destination} screen={self.lifecycle.Screen}");
        if (!self.lifecycle.HideSelection(NativeIdentity(__instance))) return;
        self.ClearSelectionReferences();
        self.PublishLifecycle("SceneChart.Hide");
    });
    private static void SceneChanging(object[] __args) => Guard(() =>
    {
        Plugin self = instance!;
        string destination = __args.Length == 1 ? __args[0]?.ToString() ?? "Unknown" : "Unknown";
        self.Trace($"SceneChanging target={destination}; previous={self.lifecycle.Destination}; run={self.lifecycle.Run?.Path}");
        if (!self.lifecycle.ChangeScene(destination)) return;
        self.ClearSelectionReferences();
        self.PublishLifecycle("SceneManager.ToScene." + destination);
    });
    private static void EnteringPlay(object[] __args) => Guard(() =>
    {
        Plugin self = instance!;
        if (self.lifecycle.Destination != "Play") return;
        self.CaptureRun(__args.Length == 1 ? __args[0] : null, initialized: false, "ChartPlayer.Load");
    });
    private static void PlayingStarted(object __instance) => Guard(() =>
    {
        Plugin self = instance!;
        if (self.lifecycle.Destination != "Play") return;
        self.CaptureRun(Get(__instance, "SourcePlayInfo"), initialized: true, "ChartPlayer.Play");
    });
    private static void ResultShown(object __instance) => Guard(() =>
    {
        Plugin self = instance!;
        self.Trace($"ResultShown entered instance={__instance.GetType().FullName}; previous={self.lifecycle.Destination}; run={self.lifecycle.Run?.Path}");
        if (!self.lifecycle.TryEnterResult()) return;
        PlayedChart? fallback = null;
        if (self.lifecycle.Run == null)
            fallback = self.ReadPlayedChart(self.resultPlayInfoGetter!.Invoke(__instance, null), initialized: true);
        self.lifecycle.ShowResult(fallback);
        self.ClearSelectionReferences();
        self.Trace($"ResultShown frozen-path={self.lifecycle.Run?.Path}; rate={self.lifecycle.Run?.Rate}");
        self.PublishLifecycle("SceneResult.OnEnable");
    });
    private static void ResultScoreFilled(object[] __args) => Guard(() =>
    {
        Plugin self = instance!;
        if (!self.lifecycle.TryEnterResult()) return;
        // Keep the initialized run's exact rate. The result's own play-info is
        // the recovery source if Load/Play could not supply a run.
        PlayedChart? fallback = self.lifecycle.Run == null
            ? self.ReadPlayedChart(__args[1], initialized: true) : null;
        self.lifecycle.ShowResult(fallback);
        self.ClearSelectionReferences();
        self.Trace($"ResultScoreFilled frozen-path={self.lifecycle.Run?.Path}; rate={self.lifecycle.Run?.Rate}");
        self.PublishLifecycle("PanelResultScore.Fill");
    });
    private static void SpeedChanged() => Guard(() =>
    {
        if (instance!.currentChart != null) instance.Capture(instance.currentChart, "SceneChart.Speed");
    });
    private static void JudgeChanged(object __instance, MethodBase __originalMethod) => Guard(() =>
    {
        Plugin self = instance!;
        if (!self.lifecycle.CanSelectCharts) return;
        self.judgePanel = __instance;
        // OnDisable is the commit point; Show and the change callbacks are in flight
        // and only let the bridge follow faster. Re-read on every callback: the
        // record is a value type, so a kept copy would freeze Pro for the session.
        self.judgeCapture?.Refresh(__instance, __originalMethod.Name, committed: __originalMethod.Name == "OnDisable");

        self.TraceTurbo(__originalMethod.Name);
        if (self.currentChart != null) self.Capture(self.currentChart, "PanelJudge." + __originalMethod.Name);
    });
    private static void ModPanelChanged(MethodBase __originalMethod) => Guard(() =>
    {
        Plugin self = instance!;
        if (!self.lifecycle.CanSelectCharts) return;

        if (__originalMethod.Name == "OnDisable" && self.currentChart != null)
            self.Capture(self.currentChart, "PanelMod.OnDisable");
    });
    private static void TurboSetup(object __instance) => Guard(() => instance!.TraceTurbo("PanelTurbo.Setup", __instance));
    private static void TurboSaved(object __instance, object __result) => Guard(() =>
    {
        Plugin self = instance!;
        self.TraceTurbo("PanelTurbo.SaveToPlayInfo", __instance, __result);
        // This method builds an option record before PanelJudge commits it. Only
        // PanelJudge.OnDisable observes the new authoritative Local.PlaySpeed.
    });
    private static void ChartSpeedChanged(object __instance) => Guard(() =>
    {
        Plugin self = instance!;
        if (self.currentChart == null || !self.lifecycle.SelectionVisible) return;
        if (StringComparer.OrdinalIgnoreCase.Equals(self.ResolveChartPath(__instance), self.ResolveChartPath(self.currentChart)))
            self.Capture(__instance, "Chart.Speed");
    });
    private static void Guard(Action action)
    {
        if (instance == null) return;
        try { action(); }
        catch (Exception ex)
        {
            Plugin? self = instance;
            if (self == null) return;
            // A failed read on a new selection must not keep the previous result alive.
            self.Publish(Selection.Hidden(++self.sequence, "observation-error"), immediate: true);
            self.Log.LogWarning($"Selection observation failed: {ex.GetBaseException().Message}");
        }
    }

    private static object? Get(object target, string property) =>
        target.GetType().GetMethod("get_" + property, AllInstance)?.Invoke(target, null);
    private static string Text(object target, string property) => Get(target, property)?.ToString() ?? "";
    private static long NativeIdentity(object value) =>
        value is Il2CppInterop.Runtime.InteropTypes.Il2CppObjectBase native ? native.Pointer.ToInt64() : 0;
    private static string NativeDescription(object? value)
    {
        if (value == null) return "null";
        try { return $"{value.GetType().FullName}@0x{NativeIdentity(value):X}"; }
        catch (Exception ex) { return value.GetType().FullName + "@unreadable:" + ex.GetType().Name; }
    }

    internal static string DiagnosticRead(Func<object?> read)
    {
        try
        {
            object? value = read();
            return value is Il2CppInterop.Runtime.InteropTypes.Il2CppObjectBase
                ? NativeDescription(value) : Convert.ToString(value, CultureInfo.InvariantCulture) ?? "null";
        }
        catch (Exception ex)
        {
            Exception cause = ex.GetBaseException();
            return "ERROR " + cause.GetType().Name + ": " + cause.Message.Split('\n')[0].Trim();
        }
    }

    private void TraceSelectionOwners(string reason, object? candidate)
    {
        if (!traceEvents) return;
        Trace($"Selection owner probe reason={reason}; destination={lifecycle.Destination}; count={selectionParents.Count}; candidate={NativeDescription(candidate)}; isShow={(candidate == null ? "n/a" : DiagnosticRead(() => Get(candidate, "IsShow")))}; candidateGameObject={(candidate == null ? "n/a" : DiagnosticRead(() => Get(candidate, "gameObject")))}; candidateActive={(candidate == null ? "n/a" : DiagnosticRead(() => IsComponentActive(candidate)))}");
        foreach ((string destination, object parent) in selectionParents)
        {
            Trace($"Selection owner probe owner={destination}; entry={NativeDescription(parent)}; gameObject={DiagnosticRead(() => Get(parent, "gameObject"))}; active={DiagnosticRead(() => IsComponentActive(parent))}");
        }
    }

    private void TracePanelHierarchy(object panel)
    {
        if (!traceEvents) return;
        Trace($"Selection hierarchy panel={NativeDescription(panel)}; scene={DiagnosticRead(() => { object? gameObject = Get(panel, "gameObject"); object? unityScene = gameObject == null ? null : Get(gameObject, "scene"); return unityScene == null ? null : Get(unityScene, "name"); })}");
        object? current;
        try { current = Get(panel, "transform"); }
        catch (Exception ex) { Trace("Selection hierarchy transform ERROR: " + ex.GetBaseException().Message); return; }
        for (int depth = 0; current != null && depth < 6; depth++)
        {
            object node = current;
            Trace($"Selection hierarchy depth={depth}; transform={NativeDescription(node)}; name={DiagnosticRead(() => Get(node, "name"))}; gameObject={DiagnosticRead(() => Get(node, "gameObject"))}; active={DiagnosticRead(() => IsComponentActive(node))}; parent={DiagnosticRead(() => Get(node, "parent"))}");
            try { current = Get(node, "parent"); }
            catch (Exception ex) { Trace("Selection hierarchy parent ERROR: " + ex.GetBaseException().Message); return; }
        }
    }

    private static bool IsSelectionPanelVisible(object? panel)
    {
        if (panel == null || Get(panel, "IsShow") is not true) return false;
        return IsComponentActive(panel) && IsSelectionPanelScene(panel);
    }

    private static bool IsSelectionPanelScene(object panel)
    {
        object? gameObject = Get(panel, "gameObject");
        object? nativeScene = gameObject == null ? null : Get(gameObject, "scene");
        return nativeScene != null && Text(nativeScene, "name") == "SceneChart";
    }

    private static bool IsComponentActive(object component)
    {
        object? gameObject = Get(component, "gameObject");
        return gameObject != null && Get(gameObject, "activeInHierarchy") is true;
    }

    private void CaptureVisiblePanel(string reason)
    {
        if (!lifecycle.CanSelectCharts || !IsSelectionPanelVisible(scene)) return;
        object? description = Get(scene!, "SongDesc");
        object? selected = description == null ? null : selectionChartGetter!.Invoke(description, null);
        Trace($"Selection visible chart probe event={reason}; panel={NativeDescription(scene)}; description={NativeDescription(description)}; chart={NativeDescription(selected)}");
        // Read the visible panel's actual selected chart. Neither the old played
        // run nor Local.Chart is a substitute when returning from gameplay.
        if (selected != null) Capture(selected, reason);
    }

    private bool BindSelectionPanel(object? panel)
    {
        if (panel == null || !IsComponentActive(panel) || !IsSelectionPanelScene(panel)) return false;
        var owners = new List<string>();
        foreach ((string destination, object parent) in selectionParents)
        {
            try
            {
                if (IsComponentActive(parent)) owners.Add(destination);
            }
            catch (Exception ex) { Trace($"Selection parent is no longer readable: owner={destination}; parent={NativeDescription(parent)}; " + ex.GetBaseException().Message); }
        }
        string previous = lifecycle.Destination;
        long identity = NativeIdentity(panel);
        bool visible = IsSelectionPanelVisible(panel);
        if (!lifecycle.ObserveSelectionPanel(owners, visible, identity))
        {
            Trace($"Selection panel ignored: active entries={string.Join(",", owners)}; visible={visible}; destination={previous}; panel={identity}");
            return false;
        }
        bool recovered = lifecycle.Destination != previous;
        if (recovered) ClearSelectionReferences();
        // Retain the actual OnEnable/Show instance even before IsShow becomes
        // true, so the later Fill can match this panel's own SongDesc pointer.
        // The lifecycle guard rejects a hidden old panel replacing a live one.
        scene = panel;
        if (recovered)
        {
            Trace($"Selection return recovered from active entry={owners[0]}; previous={previous}; panel={identity}");
            // Do not advertise the old run while waiting for Fill to supply the
            // new panel's chart. No old run or selected chart is substituted.
            Publish(Selection.Hidden(++sequence, "selection-returning"), immediate: true);
        }
        return true;
    }

    private bool BindDescriptionPanel(object description)
    {
        long identity = NativeIdentity(description);
        if (identity == 0 || scene == null) return false;
        Trace($"Selection description probe description={NativeDescription(description)}");
        TraceSelectionOwners("description-Fill", null);
        object panel = scene;
        object? candidate = Get(panel, "SongDesc");
        Trace($"Selection description match panel={NativeDescription(panel)}; panelSongDesc={NativeDescription(candidate)}; callbackDescription={NativeDescription(description)}");
        // Fill must belong to the actual observed SceneChart, not an unrelated
        // PanelSongDesc or a stale panel from the previous Unity scene instance.
        return candidate != null && NativeIdentity(candidate) == identity && BindSelectionPanel(panel);
    }
    private void Trace(string message) { if (traceEvents) Log.LogInfo("TRACE " + message); }

    private void TraceTurbo(string reason, object? panel = null, object? saved = null)
    {
        if (!traceEvents) return;
        object? info = localPlayInfo!.Invoke(null, null);
        object? localTurbo = info == null ? null : turboInfoGetter?.Invoke(info, null);
        panel ??= judgePanel == null ? null : Get(judgePanel, "panelTurbo");
        object? slider = panel == null ? null : Get(panel, "slideSpeed");
        object? toggle = judgePanel == null ? null : Get(judgePanel, "turboToggle");
        Trace($"Turbo event={reason}; ui-enabled={(toggle == null ? null : Get(toggle, "Current"))}; ui-speed={(slider == null ? null : Get(slider, "Current"))}; play-speed={(info == null ? null : Get(info, "PlaySpeed"))}; mod={(info == null ? null : Get(info, "Mod"))}; local-options={TurboScalars(localTurbo)}; saved-options={TurboScalars(saved)}");
    }

    private static string TurboScalars(object? value)
    {
        if (value == null) return "null";
        // Only the small Turbo option record is inspected; no user/configuration objects.
        return string.Join(",", value.GetType().GetMethods(AllInstance | BindingFlags.DeclaredOnly)
            .Where(m => m.Name.StartsWith("get_") && m.GetParameters().Length == 0 &&
                (m.ReturnType == typeof(float) || m.ReturnType == typeof(bool) || m.ReturnType == typeof(int)))
            .Select(m => m.Name[4..] + "=" + Convert.ToString(m.Invoke(value, null), CultureInfo.InvariantCulture)));
    }

    /// <summary>
    /// Builds a frame and stamps it with the current judge fields. Every published
    /// selection goes through here, so a session that never opens the judge panel
    /// still carries the fields that do not need it, and an unreadable field is
    /// published as null instead of being replaced by a default.
    /// </summary>
    private Selection Frame(string path, double rate, string screen, string reason, string version, string hash)
    {
        JudgeCapture? capture = judgeCapture;
        capture?.Refresh(judgePanel, reason, committed: false);
        return new Selection(path, rate, screen, ++sequence, reason, version, hash)
        {
            judge_level = capture?.JudgeLevel,
            pro_judge = capture?.ProJudge,
            turbo = capture?.Turbo
        };
    }

    private void Capture(object? chart, string reason)
    {
        Trace($"Capture event={reason} chart={chart?.GetType().FullName ?? "null"} expected={chartType?.FullName} visible={lifecycle.SelectionVisible}");
        if (!lifecycle.CanSelectCharts) return;
        if (chart == null || chartType == null || !chartType.IsInstanceOfType(chart)) return;
        currentChart = chart;
        if (scene == null || !lifecycle.RestoreSelection(IsSelectionPanelVisible(scene), NativeIdentity(scene)))
        {
            Trace("Capture stopped: SceneChart is not visible in the active hierarchy");
            if (scene != null && lifecycle.HideSelection(NativeIdentity(scene)))
                PublishLifecycle("selection-not-visible");
            return;
        }

        // Chart.Mode is a FileMode bitmask (MCKey=64, OsuMania=1), not MC meta.mode.
        // PlayModeType.Key covers every key count; it is independent of chart format.
        object? playMode = Get(chart, "PlayMode");
        Trace($"Capture file-mode={Get(chart, "Mode")} play-mode={playMode} scene={scene?.GetType().FullName}");
        if (playMode == null || !string.Equals(playMode.ToString(), "Key", StringComparison.Ordinal))
        {
            Publish(Selection.Hidden(++sequence, "unsupported-mode"), immediate: true);
            return;
        }
        string? path = ResolveChartPath(chart);
        if (path == null)
        {
            Publish(Selection.Hidden(++sequence, "chart-path-unresolved"), immediate: true);
            Log.LogWarning("Selected chart could not be resolved to one existing file under the game's chart directory.");
            return;
        }

        double rate = ReadAudioRate();
        Trace($"Capture accepted path={path} reported-speed={rate}");
        Selection next = Frame(path, rate, "selection", reason, Text(chart, "DisplayVersion"), Text(chart, "Hash"));
        Publish(next, immediate: false);
        if (verbose) Log.LogInfo($"Selection {next.sequence}: {reason} | {path} | rate {rate:0.###}");
    }

    private string? ResolveChartPath(object chart)
    {
        string filePath = Text(chart, "FilePath");
        string directory = Text(chart, "ChartDir");
        string filename = Text(chart, "FileName");
        var candidates = new List<string>();
        if (!string.IsNullOrWhiteSpace(filePath))
        {
            candidates.Add(Path.GetFullPath(filePath, gameRoot));
            candidates.Add(Path.GetFullPath(filePath, chartRoot));
        }
        if (!string.IsNullOrWhiteSpace(directory) && !string.IsNullOrWhiteSpace(filename))
        {
            candidates.Add(Path.GetFullPath(Path.Combine(directory, filename), gameRoot));
            candidates.Add(Path.GetFullPath(Path.Combine(directory, filename), chartRoot));
        }
        // If two valid locations disagree, stop rather than guess from title or mtime.
        string[] existing = candidates.Distinct(StringComparer.OrdinalIgnoreCase)
            .Where(p => p.StartsWith(chartRoot, StringComparison.OrdinalIgnoreCase) && File.Exists(p)).ToArray();
        return existing.Length == 1 ? existing[0] : null;
    }

    private double ReadAudioRate(object? playInfo = null, bool initialized = false)
    {
        object? info = playInfo ?? localPlayInfo!.Invoke(null, null);
        if (info == null) throw new InvalidOperationException("Cannot read the local play settings.");
        if (initialized)
        {
            // SourcePlayInfo after ChartPlayer.Play has its actual ModForPlay,
            // including replay settings. Never substitute the current Local UI.
            double playedRate = ReadFinitePlaySpeed(info);
            Trace($"Played audio rate: initialized PlaySpeed={playedRate:0.00}");
            return playedRate;
        }
        object? turbo = turboInfoGetter!.Invoke(info, null);
        if (turbo != null)
        {
            // The verified native getter returns the Turbo record's audio rate
            // before testing fixed-rate Mods. A null record means Turbo is off.
            object value = Get(info, "PlaySpeed") ?? throw new InvalidOperationException("Cannot read Turbo audio speed.");
            double raw = Convert.ToDouble(value, CultureInfo.InvariantCulture);
            if (!double.IsFinite(raw) || raw < 0.05 || raw > 10)
                throw new InvalidOperationException("Turbo audio speed is outside the supported range.");
            // The game saves the slider to two decimal places. Remove Single
            // representation noise (e.g. 1.230000019) without losing 0.01 steps.
            double customRate = Math.Round(raw, 2, MidpointRounding.AwayFromZero);
            Trace($"Audio rate: Turbo PlaySpeed={raw:0.#########} => {customRate:0.00}; selected Mod={Get(info, "Mod")}");
            return customRate;
        }
        object? mod = info == null ? null : Get(info, "Mod");
        if (mod == null || !mod.GetType().IsEnum)
            throw new InvalidOperationException("Cannot read the selected audio-rate modifiers.");
        long mask = Convert.ToInt64(mod, CultureInfo.InvariantCulture);
        double rate = 1;
        // Match the native PlaySpeed getter's fixed-Mod precedence. During song
        // selection its ModForPlay can be stale, so read the selected Mod mask.
        foreach ((string name, double multiplier) in new[] { ("Rush", 1.5), ("Dash", 1.2), ("Slow", 0.8) })
        {
            long flag = Convert.ToInt64(Enum.Parse(mod.GetType(), name), CultureInfo.InvariantCulture);
            if ((mask & flag) != 0)
            {
                rate = multiplier;
                break;
            }
        }
        // Falling-note speed is not an audio multiplier. In the verified build,
        // Local.PlaySpeed alone also stayed at 1 with Dash enabled, so use ModMask.
        Trace($"Audio rate: ModMask={mask} => {rate:0.###}; observed Local.PlaySpeed={Get(info!, "PlaySpeed")}");
        return rate;
    }

    private static double ReadFinitePlaySpeed(object info)
    {
        object value = Get(info, "PlaySpeed") ?? throw new InvalidOperationException("Cannot read the played audio speed.");
        double rate = Convert.ToDouble(value, CultureInfo.InvariantCulture);
        if (!double.IsFinite(rate) || rate < 0.05 || rate > 10)
            throw new InvalidOperationException("Played audio speed is outside the supported range.");
        return Math.Round(rate, 2, MidpointRounding.AwayFromZero);
    }

    private PlayedChart ReadPlayedChart(object? playInfo, bool initialized)
    {
        if (playInfo == null || !localPlayInfo!.ReturnType.IsInstanceOfType(playInfo))
            throw new InvalidOperationException("The active player did not supply a valid play-info record.");
        object? chart = Get(playInfo, "Chart");
        if (chart == null || !chartType!.IsInstanceOfType(chart) || Text(chart, "PlayMode") != "Key")
            throw new InvalidOperationException("The active player is not a supported Key chart.");
        string path = ResolveChartPath(chart) ?? throw new InvalidOperationException("Cannot resolve the active player's chart file.");
        return new PlayedChart(path, ReadAudioRate(playInfo, initialized), Text(chart, "DisplayVersion"), Text(chart, "Hash"));
    }

    private void CaptureRun(object? playInfo, bool initialized, string reason)
    {
        lifecycle.InvalidateRun();
        PlayedChart chart = ReadPlayedChart(playInfo, initialized);
        lifecycle.CaptureRun(chart);
        ClearSelectionReferences();
        Trace($"Run captured event={reason}; path={chart.Path}; rate={chart.Rate:0.00}; initialized={initialized}");
        PublishLifecycle(reason);
    }

    private void ClearSelectionReferences()
    {

        scene = null;
        currentChart = null;
    }

    private void PublishLifecycle(string reason)
    {
        PlayedChart? chart = lifecycle.Run;
        Selection next = chart != null && lifecycle.Screen is "playing" or "result"
            ? Frame(chart.Path, chart.Rate, lifecycle.Screen, reason, chart.Version, chart.Hash)
            : Selection.Hidden(++sequence, reason);
        Publish(next, immediate: true);
    }

    private void Publish(Selection value, bool immediate)
    {
        lock (stateLock)
        {
            // A judge change alone is still a change: the companion's own content test
            // treats Pro and Turbo as news, so the frame must be scheduled even while
            // path, rate and screen stay as they are.
            bool same = value.path == snapshot.path && Math.Abs(value.speed_rate - snapshot.speed_rate) < 0.00001 &&
                value.screen == snapshot.screen && value.judge_level == snapshot.judge_level &&
                value.pro_judge == snapshot.pro_judge && value.turbo == snapshot.turbo;
            snapshot = value;
            // Repeated game redraws must not postpone a selection indefinitely.
            if (!same) due = DateTime.UtcNow.AddSeconds(immediate ? 0 : debounceSeconds);
        }
    }

    // The client owns the socket and the send guard. This only decides whether a
    // message is due; it is never called while another send is in flight, so the
    // pending message is not consumed by a skipped pump.
    private Selection? NextMessage()
    {
        DateTime now = DateTime.UtcNow;
        lock (stateLock)
        {
            if ((due != DateTime.MaxValue && now < due) ||
                (due == DateTime.MaxValue && (now - lastSend).TotalSeconds < heartbeatSeconds)) return null;
            lastSend = now;
            due = DateTime.MaxValue;
            return snapshot;
        }
    }

    private async void Pump()
    {
        BridgeClient? client = bridge;
        if (client == null) return;
        await client.Pump(NextMessage, stop.Token).ConfigureAwait(false);
    }

    public override bool Unload()
    {
        timer?.Dispose();
        stop.Cancel();
        harmony?.UnpatchSelf();
        instance = null;
        bridge?.Dispose();
        // The companion expires heartbeat state if the game exits or unloads.
        return true;
    }

}
