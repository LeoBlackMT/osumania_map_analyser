namespace MalodyInsightBridge;

/// <summary>Managed-only scene policy. A run never depends on later selection callbacks.</summary>
internal sealed record PlayedChart(string Path, double Rate, string Version, string Hash);

internal sealed class SceneLifecycle
{
    public string Destination { get; private set; } = "Unknown";
    public string Screen { get; private set; } = "other";
    public bool SelectionVisible { get; private set; }
    public long SelectionPanel { get; private set; }
    public PlayedChart? Run { get; private set; }
    // Inventory is the expanded all-songs browser. It hosts the same SceneChart
    // detail panel as Song and must use the same selection/mod/rate observers.
    public bool CanSelectCharts => Destination is "Song" or "Inventory";

    public bool ChangeScene(string destination)
    {
        if (Destination == destination) return false;
        Destination = destination;
        SelectionVisible = false;
        SelectionPanel = 0;
        if (destination == "Result")
            Screen = Run == null ? "other" : "result";
        else
        {
            // Each Play transition starts a fresh run; Load/Play will supply its
            // real chart. Home, editor, replay browsing, etc. discard the run.
            Run = null;
            Screen = "other";
        }
        return true;
    }

    public bool ShowSelection(long panel = 0)
    {
        if (!CanSelectCharts) return false;
        SelectionVisible = true;
        SelectionPanel = panel;
        Screen = "selection";
        return true;
    }

    public bool RestoreSelection(bool panelVisible, long panel)
        => panelVisible && ShowSelection(panel);

    public bool CanBindSelectionPanel(bool panelVisible, long panel)
        => CanSelectCharts && (panelVisible || !SelectionVisible || SelectionPanel == 0 || SelectionPanel == panel);

    public bool ObserveSelectionPanel(IReadOnlyList<string> activeScenes, bool panelVisible, long panel)
    {
        // SceneChart is a separate Unity scene, not a child of the selection
        // scene. Its live visibility plus one active selection entry identifies
        // a return even when the game's dispatcher omits ToScene(Inventory).
        if (activeScenes.Count != 1 || activeScenes[0] is not ("Song" or "Inventory") || panel == 0)
            return false;
        string destination = activeScenes[0];
        if (CanSelectCharts)
            return Destination == destination && CanBindSelectionPanel(panelVisible, panel);
        // OnEnable precedes Show/Fill: the observer may retain this real panel,
        // but an initially hidden panel must not replace a played/result run.
        return !panelVisible || RecoverSelectionScene(destination, true, true, panel);
    }

    public bool RecoverSelectionScene(string destination, bool parentActive, bool panelVisible, long panel)
    {
        // Inventory can recreate its independent UI without ToScene(Inventory).
        // A live selection scene and visible SceneChart are the return signal;
        // an old hidden callback alone is never sufficient.
        if (destination is not ("Song" or "Inventory") || !parentActive || !panelVisible || panel == 0)
            return false;
        ChangeScene(destination);
        return ShowSelection(panel);
    }

    public bool HideSelection(long panel = 0)
    {
        // A reused scene can finish hiding an old chart panel after another
        // panel has already supplied the current selection.
        if (panel != 0 && SelectionPanel != 0 && panel != SelectionPanel) return false;
        SelectionVisible = false;
        SelectionPanel = 0;
        if (Screen != "selection") return false;
        Screen = "other";
        return true;
    }

    public bool CaptureRun(PlayedChart chart)
    {
        if (Destination != "Play") return false;
        Run = chart;
        SelectionVisible = false;
        SelectionPanel = 0;
        Screen = "playing";
        return true;
    }

    public bool ShowResult(PlayedChart? fallback = null)
    {
        if (Destination != "Result") return false;
        Run ??= fallback;
        SelectionVisible = false;
        SelectionPanel = 0;
        Screen = Run == null ? "other" : "result";
        return true;
    }

    public bool TryEnterResult()
    {
        // Natural completion may omit ToScene(Result), but a delayed result
        // callback must not pull an already-returned selection back into play.
        if (Destination is not ("Play" or "Result")) return false;
        ChangeScene("Result");
        return true;
    }

    public void InvalidateRun()
    {
        Run = null;
        Screen = "other";
    }
}
