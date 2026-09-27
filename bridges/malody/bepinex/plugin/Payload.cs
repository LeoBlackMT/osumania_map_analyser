using System.Text.Json.Serialization;

namespace MalodyInsightBridge;

/// <summary>
/// One selection observation, as posted to the companion service on 127.0.0.1.
///
/// The JSON keys ARE the C# member names: <see cref="BridgeClient"/> serialises
/// with System.Text.Json and no naming policy, so the positional parameter names
/// below are the wire keys and they line up with the desktop shell's serde field
/// names. Renaming a parameter renames the wire field, and the shell only
/// tolerates a missing key for fields it declares optional, so any rename here
/// needs a matching shell change.
///
/// The frame carries exactly eleven keys:
///   path, speed_rate, screen, sequence, event, version, chart_hash, source,
///   judge_level, pro_judge, turbo.
///
/// The last three are read by <see cref="JudgeCapture"/>. They are nullable on
/// purpose: a field no tier could read is published as JSON null, which the shell
/// reads as "unknown". Null is a real answer and is never replaced by a default
/// (the normal window group) or by a guess.
/// </summary>
internal sealed record Selection(string path, double speed_rate, string screen, long sequence,
    [property: JsonPropertyName("event")] string eventName, string version = "", string chart_hash = "")
{
    /// <summary>Identifies the data source: Malody V, read through this IL2CPP bridge.</summary>
    public string source => "malody-v-il2cpp";

    /// <summary>Judge level A-E as 0-4; null when no tier could read it. MAX is never reported.</summary>
    public int? judge_level { get; init; }

    /// <summary>Committed Pro judge flag; null when no tier could read it. Never derived from the settings file.</summary>
    public bool? pro_judge { get; init; }

    /// <summary>The committed Turbo record is non-empty; null when it cannot be read. Never a switch position.</summary>
    public bool? turbo { get; init; }

    /// <summary>The "nothing to show" frame. It still carries all eleven keys.</summary>
    public static Selection Hidden(long sequence, string reason) => new("", 1.0, "other", sequence, reason);
}
