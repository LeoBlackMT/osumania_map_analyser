using System.Globalization;
using System.Reflection;
using System.Text.Json;
using BepInEx.Logging;

namespace MalodyInsightBridge;

/// <summary>
/// Pro judge, judge level and Turbo, read through a three-tier chain that never
/// guesses. A tier that cannot read a value reports null, and null is published
/// as null: the desktop shell turns dynamic OD off until the value is known, so
/// an unreadable field can never be mistaken for the normal window group.
///
/// Tier 1 - the records the game itself keeps.
///   1a. The judge intent the panel was handed: PanelJudge.del, a value type
///       (Malody.Manager.brk+IntentJudge) carrying proJudge and level. It is a
///       copy, so the property is re-read on every observation; a cached copy
///       would freeze Pro for the rest of the session.
///   1b. The local play settings record (Malody.Play.boq): its Level carries the
///       judge level and its Pro flag carries Pro. Both are independent of the
///       panel, so they answer even when the user never opened the panel.
/// Tier 2 - the panel's own controls (the Pro switch, the judge tab indicator).
///   The verified build's controls expose no accessor that says what it holds
///   (UIToggle lists only unnamed booleans), so this tier reports a value only for
///   an accessor named in <see cref="StateAccessors"/>; otherwise it stays null and
///   names the candidates in the log. An unnamed boolean is never taken as Pro.
/// Tier 3 - the game's config.json user_judge_level, for the judge level only. It
///   lags during selection but is at least present. Pro never comes from the file:
///   the file has no Pro key.
///
/// Turbo never comes from a control: it is the committed Turbo record obtained
/// through the local play settings, or null.
///
/// PanelJudge.OnDisable is the commit point. Values read from the other callbacks
/// are in flight and are used to follow as fast as possible; the OnDisable read is
/// final, and it cross-checks both the judge level and Pro against the play
/// settings record. Every change is logged as
/// "Judge &lt;field&gt;=&lt;value&gt; tier=&lt;tier&gt; after &lt;trigger&gt;", one line per field per
/// value change, so the winning tier is readable straight from the log.
/// </summary>
internal sealed class JudgeCapture
{
    private const BindingFlags AllInstance = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance;

    /// <summary>
    /// Control members whose name states what they hold. Names come first so a
    /// renamed or newly discovered accessor is pinned in exactly this one place.
    /// </summary>
    private static readonly string[] StateAccessors =
    {
        "Current", "IsOn", "isOn", "Checked", "On", "Value", "Index", "Selected", "SelectedIndex", "CurrentIndex"
    };

    /// <summary>
    /// Judge level getters on the play settings record (Malody.Play.boq). The named
    /// one is preferred; a renamed build means editing this one list.
    /// </summary>
    private static readonly string[] PlayRecordLevelGetters = { "get_Level" };

    /// <summary>
    /// The play settings record's Pro flag. Measured, not inferred: the T2 real-machine
    /// session moved this member with the panel's Pro switch across four snapshots while
    /// the judge level moved on its own axis (evidence:
    /// .omo/evidence/malody-v-selection-bridge-fork/t2-session-bepinex-log.txt). It is the
    /// only Pro source that does not need the panel, so it is deliberately pinned by name
    /// instead of guessing at "some boolean" on the record; a rename means editing this list.
    /// </summary>
    private static readonly string[] PlayRecordProGetters = { "get_bcgh" };

    /// <summary>Members that describe the interop wrapper rather than the game object.</summary>
    private static readonly string[] Noise = { "ObjectClass", "Pointer", "WasCollected", "m_CachedPtr" };

    private readonly ManualLogSource log;
    private readonly bool dumpPanelFields;
    private readonly Assembly game;
    private readonly MethodInfo playInfo;
    private readonly MethodInfo? turboGetter;
    private readonly string settingsFile;
    private readonly Dictionary<string, string> reported = new();

    private object? panel;
    private bool discovered;
    private MemberInfo? intentMember;
    private PropertyInfo? intentPro;
    private PropertyInfo? intentLevel;
    private MethodInfo? playInfoLevel;
    private MethodInfo? playInfoPro;
    private Type? judgeLevelType;
    private int? judgeLevelMax;

    public JudgeCapture(ManualLogSource log, bool dumpPanelFields, Assembly game, string gameRoot,
        MethodInfo playInfo, MethodInfo? turboGetter)
    {
        this.log = log;
        this.dumpPanelFields = dumpPanelFields;
        this.game = game;
        this.playInfo = playInfo;
        this.turboGetter = turboGetter;
        settingsFile = Path.Combine(gameRoot, "config.json");
        // The level enum and the play-settings getters do not need the panel, so the
        // panel-independent tiers work from the first observed selection onwards.
        judgeLevelType = game.GetType("Malody.Play.JudgeLevel");
        judgeLevelMax = MaxMember(judgeLevelType);
        playInfoLevel = FindPlayInfoLevel();
        playInfoPro = FindGetter(PlayRecordProGetters, typeof(bool));
    }

    /// <summary>Judge level A-E as 0-4, or null when no tier could read it.</summary>
    public int? JudgeLevel { get; private set; }

    /// <summary>Committed Pro judge flag, or null when no tier could read it.</summary>
    public bool? ProJudge { get; private set; }

    /// <summary>The committed Turbo record is non-empty, or null when it cannot be read.</summary>
    public bool? Turbo { get; private set; }

    /// <summary>
    /// Re-reads every field. Called from the panel callbacks and before each frame
    /// is built, so a session that never opens the panel still publishes the
    /// panel-independent fields. Logs one line per field per value change, naming
    /// the tier that answered.
    /// </summary>
    internal void Refresh(object? panelInstance, string trigger, bool committed)
    {
        if (panelInstance != null)
        {
            panel = panelInstance;
            if (!discovered) Discover(panelInstance.GetType());
            if (dumpPanelFields && trigger == "Show") Dump(trigger);
        }

        JudgeLevel = ReadLevel(committed, out string? levelTier, out string? levelReason);
        ProJudge = ReadPro(committed, out string? proTier, out string? proReason);
        Turbo = ReadTurbo(out string? turboTier, out string? turboReason);
        Report("judge_level", JudgeLevel?.ToString(CultureInfo.InvariantCulture), levelTier, levelReason, trigger, committed);
        Report("pro_judge", ProJudge?.ToString(), proTier, proReason, trigger, committed);
        Report("turbo", Turbo?.ToString(), turboTier, turboReason, trigger, committed);
    }

    /// <summary>The one line a readable field logs. Pure, so the reported format is asserted offline.</summary>
    internal static string TierLine(string field, string value, string tier, string trigger, bool committed) =>
        $"Judge {field}={value} tier={tier} after {trigger}{(committed ? ", committed" : "")}.";

    /// <summary>The one warning an unreadable field logs.</summary>
    internal static string MissingLine(string field, string trigger, string reason) =>
        $"Judge {field}=null tier=none after {trigger}: {reason}";

    /// <summary>The one line a disagreement between the two records logs.</summary>
    internal static string DisagreementLine(string field, string panelValue, string recordValue) =>
        $"Judge {field} records differ after the commit: the panel's judge record says {panelValue}, " +
        $"the play settings record says {recordValue}. The panel record is reported.";

    private void Report(string field, string? value, string? tier, string? reason, string trigger, bool committed)
    {
        if (!Changed(field, value ?? "null")) return;
        if (value != null) log.LogInfo(TierLine(field, value, tier!, trigger, committed));
        else log.LogWarning(MissingLine(field, trigger, reason ?? "no tier could read it"));
    }

    private bool Changed(string key, string value)
    {
        if (reported.TryGetValue(key, out string? previous) && previous == value) return false;
        reported[key] = value;
        return true;
    }

    // -----------------------------------------------------------------------
    // Judge level: panel record, then play settings record, then control, then file.
    // -----------------------------------------------------------------------
    private int? ReadLevel(bool committed, out string? tier, out string? reason)
    {
        var why = new List<string>();
        int? recorded = MapLevel(ReadMember(intentLevel, ReadIntent(why), why, "panel record"));
        if (recorded != null)
        {
            if (committed) CompareRecords(recorded.Value);
            tier = "panel-record";
            reason = null;
            return recorded;
        }

        int? played = ReadPlayInfoLevel(why);
        if (played != null)
        {
            tier = "play-record";
            reason = null;
            return played;
        }

        int? control = MapLevel(ReadState(FindControl("judge", why, "judge control"), why, "judge control"));
        if (control != null)
        {
            tier = "panel-control";
            reason = null;
            return control;
        }

        int? file = ReadSettingsLevel(why);
        if (file != null)
        {
            tier = "settings-file";
            reason = null;
            return file;
        }

        tier = null;
        reason = string.Join("; ", why) + ". The judge level stays unknown rather than defaulting.";
        return null;
    }

    /// <summary>At the commit point, report once when the two level records disagree.</summary>
    private void CompareRecords(int panelLevel)
    {
        int? played = ReadPlayInfoLevel(new List<string>());
        if (played == null || played.Value == panelLevel || !Changed("records", $"{panelLevel}/{played}")) return;
        log.LogInfo(DisagreementLine("level", panelLevel.ToString(CultureInfo.InvariantCulture),
            played.Value.ToString(CultureInfo.InvariantCulture)));
    }

    // -----------------------------------------------------------------------
    // Pro: panel record, then the panel-independent play settings flag, then a
    // named control accessor. Never the settings file.
    // -----------------------------------------------------------------------
    private bool? ReadPro(bool committed, out string? tier, out string? reason)
    {
        var why = new List<string>();
        if (ReadMember(intentPro, ReadIntent(why), why, "panel record") is bool recorded)
        {
            if (committed) CompareProRecords(recorded);
            tier = "panel-record";
            reason = null;
            return recorded;
        }

        if (ReadPlayInfoPro(why) is bool played)
        {
            tier = "play-record";
            reason = null;
            return played;
        }

        if (ReadState(FindControl("pro", why, "Pro control"), why, "Pro control") is bool control)
        {
            tier = "panel-control";
            reason = null;
            return control;
        }

        tier = null;
        reason = string.Join("; ", why) +
            ". Pro stays unknown instead of being taken from the normal window group; opening the JUDGE panel once makes it readable.";
        return null;
    }

    /// <summary>
    /// At the commit point, report once when the panel's Pro flag and the committed
    /// play settings flag disagree. The panel is the live truth and is reported; the
    /// line is what keeps the play record's measured meaning under review.
    /// </summary>
    private void CompareProRecords(bool panelPro)
    {
        bool? played = ReadPlayInfoPro(new List<string>());
        if (played == null || played.Value == panelPro || !Changed("pro-records", $"{panelPro}/{played}")) return;
        log.LogInfo(DisagreementLine("pro", panelPro.ToString(), played.Value.ToString()));
    }

    // -----------------------------------------------------------------------
    // Turbo: the committed Turbo record, never a switch position.
    // -----------------------------------------------------------------------
    private bool? ReadTurbo(out string? tier, out string? reason)
    {
        if (turboGetter == null)
        {
            tier = null;
            reason = "the local play settings expose no Turbo record getter";
            return null;
        }
        try
        {
            object? info = playInfo.Invoke(null, null);
            if (info == null)
            {
                tier = null;
                reason = "the local play settings are not available yet";
                return null;
            }
            bool turbo = turboGetter.Invoke(info, null) != null;
            tier = "turbo-record";
            reason = null;
            return turbo;
        }
        catch (Exception ex)
        {
            tier = null;
            reason = $"reading the committed Turbo record threw {ex.GetBaseException().GetType().Name}";
            return null;
        }
    }

    // -----------------------------------------------------------------------
    // Individual sources
    // -----------------------------------------------------------------------
    private object? ReadIntent(List<string> why)
    {
        object? instance = panel;
        if (instance == null)
        {
            why.Add("no judge panel has been observed in this session");
            return null;
        }
        if (intentMember == null)
        {
            why.Add($"the panel ({instance.GetType().Name}) carries no judge record");
            return null;
        }
        // The record is a value type: read it again here, never from a kept copy.
        return ReadMember(intentMember, instance, why, "panel record");
    }

    private int? ReadPlayInfoLevel(List<string> why)
    {
        if (playInfoLevel == null)
        {
            why.Add("the play settings expose no judge level getter");
            return null;
        }
        try
        {
            object? info = playInfo.Invoke(null, null);
            if (info == null)
            {
                why.Add("the play settings are not available");
                return null;
            }
            int? level = MapLevel(ReadMember(playInfoLevel, info, why, "play record"));
            if (level == null) why.Add("the play settings judge level is outside the level range");
            return level;
        }
        catch (Exception ex)
        {
            why.Add($"the play settings judge level threw {ex.GetBaseException().GetType().Name}");
            return null;
        }
    }

    /// <summary>
    /// The Pro flag on the play settings record. Panel-independent, so it answers in a
    /// session that never opened the panel; it is only consulted while the panel's own
    /// record cannot be read.
    /// </summary>
    private bool? ReadPlayInfoPro(List<string> why)
    {
        if (playInfoPro == null)
        {
            why.Add($"the play settings expose no Pro flag ({string.Join("/", PlayRecordProGetters)})");
            return null;
        }
        try
        {
            object? info = playInfo.Invoke(null, null);
            if (info == null)
            {
                why.Add("the play settings are not available");
                return null;
            }
            if (ReadMember(playInfoPro, info, why, "play record") is bool pro) return pro;
            why.Add("the play settings Pro flag did not read as a boolean");
            return null;
        }
        catch (Exception ex)
        {
            why.Add($"the play settings Pro flag threw {ex.GetBaseException().GetType().Name}");
            return null;
        }
    }

    private int? ReadSettingsLevel(List<string> why)
    {
        try
        {
            if (!File.Exists(settingsFile))
            {
                why.Add($"{settingsFile} does not exist");
                return null;
            }
            int? level = ParseJudgeLevel(File.ReadAllText(settingsFile), judgeLevelMax);
            if (level == null) why.Add("config.json has no usable user_judge_level");
            return level;
        }
        catch (Exception ex)
        {
            why.Add("config.json could not be read (" + ex.GetBaseException().GetType().Name + ")");
            return null;
        }
    }

    /// <summary>Reads user_judge_level from a config.json body. Pure, so the offline assertions can call it.</summary>
    internal static int? ParseJudgeLevel(string? json, int? maxValue)
    {
        if (string.IsNullOrWhiteSpace(json)) return null;
        try
        {
            using JsonDocument document = JsonDocument.Parse(json);
            if (!document.RootElement.TryGetProperty("user_judge_level", out JsonElement value)) return null;
            // TryGetInt32 throws on a non-number, so the kind is checked first.
            if (value.ValueKind != JsonValueKind.Number || !value.TryGetInt32(out int raw)) return null;
            return MapJudgeLevel(raw, maxValue);
        }
        catch (JsonException)
        {
            return null;
        }
    }

    /// <summary>
    /// The payload carries A-E as 0-4. MAX (5 in the verified build) is not a
    /// level, so it is rejected here, together with anything out of range.
    /// </summary>
    internal static int? MapJudgeLevel(int raw, int? maxValue) =>
        raw < 0 || raw > 4 || raw == maxValue ? null : raw;

    private int? MapLevel(object? value)
    {
        if (value == null) return null;
        try
        {
            return MapJudgeLevel(Convert.ToInt32(value, CultureInfo.InvariantCulture), judgeLevelMax);
        }
        catch (Exception)
        {
            return null;
        }
    }

    // -----------------------------------------------------------------------
    // Control (tier 2) readers
    // -----------------------------------------------------------------------
    private object? FindControl(string hint, List<string> why, string label)
    {
        object? instance = panel;
        if (instance == null)
        {
            why.Add($"{label}: no judge panel has been observed in this session");
            return null;
        }
        foreach (PropertyInfo property in instance.GetType().GetProperties(AllInstance))
        {
            if (property.GetIndexParameters().Length != 0) continue;
            if (!property.Name.Contains(hint, StringComparison.OrdinalIgnoreCase)) continue;
            if (!IsControl(property.PropertyType)) continue;
            object? value = ReadMember(property, instance, why, label);
            if (value != null) return value;
        }
        why.Add($"{label}: the panel exposes no {hint} control");
        return null;
    }

    /// <summary>
    /// Reads a control's state through an accessor whose name states what it holds.
    /// Unnamed members are listed as candidates instead of being guessed at.
    /// </summary>
    private static object? ReadState(object? control, List<string> why, string label)
    {
        if (control == null) return null;
        Type type = control.GetType();
        foreach (string name in StateAccessors)
        {
            MemberInfo? member = type.GetProperty(name, AllInstance) ?? (MemberInfo?)type.GetField(name, AllInstance);
            if (member == null) continue;
            Type memberType = MemberType(member);
            if (memberType != typeof(bool) && memberType != typeof(int) && !memberType.IsEnum) continue;
            return ReadMember(member, control, why, label);
        }
        why.Add($"{label}: it has no state accessor named {string.Join("/", StateAccessors)}; " +
            $"its members are {string.Join("/", LeafMembers(type).Select(m => m.Name))}");
        return null;
    }

    // -----------------------------------------------------------------------
    // Discovery
    // -----------------------------------------------------------------------
    private void Discover(Type panelType)
    {
        discovered = true;
        try
        {
            MemberInfo? member = panelType.GetProperty("del", AllInstance);
            if (member == null || !CarriesIntent(MemberType(member)))
                member = panelType.GetProperties(AllInstance)
                    .Where(p => p.GetIndexParameters().Length == 0 && CarriesIntent(p.PropertyType))
                    .Cast<MemberInfo>().FirstOrDefault();
            if (member != null)
            {
                intentMember = member;
                Type intent = MemberType(member);
                intentPro = intent.GetProperties(AllInstance)
                    .FirstOrDefault(p => p.PropertyType == typeof(bool) && Mentions(p.Name, "projudge"));
                intentLevel = intent.GetProperties(AllInstance)
                    .FirstOrDefault(p => p.PropertyType.IsEnum && Mentions(p.Name, "level"));
                judgeLevelType ??= intentLevel?.PropertyType;
                judgeLevelMax ??= MaxMember(judgeLevelType);
                playInfoLevel ??= FindPlayInfoLevel();
            }
            log.LogInfo($"Judge sources: panel record={Describe(intentMember)}; pro={Describe(intentPro)}; " +
                $"level={Describe(intentLevel)}; play record level={Describe(playInfoLevel)}; play record pro={Describe(playInfoPro)}; " +
                $"level values={judgeLevelType?.Name ?? "unknown"} (MAX={judgeLevelMax?.ToString(CultureInfo.InvariantCulture) ?? "none"}); " +
                $"pro control={Describe(panelType.GetProperties(AllInstance).FirstOrDefault(p => p.Name.Contains("pro", StringComparison.OrdinalIgnoreCase) && IsControl(p.PropertyType)))}; " +
                $"judge control={Describe(panelType.GetProperties(AllInstance).FirstOrDefault(p => p.Name.Contains("judge", StringComparison.OrdinalIgnoreCase) && IsControl(p.PropertyType)))}");
        }
        catch (Exception ex)
        {
            log.LogWarning($"Judge field discovery failed: {ex.GetBaseException().Message}");
        }
    }

    private MethodInfo? FindPlayInfoLevel()
    {
        if (judgeLevelType == null) return null;
        MethodInfo? named = FindGetter(PlayRecordLevelGetters, judgeLevelType);
        if (named != null) return named;
        MethodInfo[] getters = playInfo.ReturnType.GetMethods(AllInstance)
            .Where(m => m.Name.StartsWith("get_", StringComparison.Ordinal) && m.GetParameters().Length == 0 &&
                m.ReturnType == judgeLevelType)
            .ToArray();
        // An obfuscated twin is used only while it is the sole candidate.
        return getters.Length == 1 ? getters[0] : null;
    }

    /// <summary>
    /// A zero-argument getter of the play settings record, looked up by name. Names
    /// only: a boolean picked because it happens to be there could silently mean
    /// something else, which is the one failure this chain exists to prevent.
    /// </summary>
    private MethodInfo? FindGetter(string[] names, Type returnType)
    {
        foreach (string name in names)
        {
            MethodInfo? getter = playInfo.ReturnType.GetMethods(AllInstance)
                .FirstOrDefault(m => string.Equals(m.Name, name, StringComparison.OrdinalIgnoreCase) &&
                    m.GetParameters().Length == 0 && m.ReturnType == returnType);
            if (getter != null) return getter;
        }
        return null;
    }

    private static int? MaxMember(Type? enumType)
    {
        if (enumType == null || !enumType.IsEnum) return null;
        try
        {
            foreach (string name in Enum.GetNames(enumType))
                if (name.Equals("MAX", StringComparison.OrdinalIgnoreCase))
                    return Convert.ToInt32(Enum.Parse(enumType, name), CultureInfo.InvariantCulture);
        }
        catch (Exception)
        {
            // A missing MAX member only means the range guard alone protects the payload.
        }
        return null;
    }

    private static bool CarriesIntent(Type? type)
    {
        if (type == null) return false;
        PropertyInfo[] properties = type.GetProperties(AllInstance);
        return properties.Any(p => p.PropertyType == typeof(bool) && Mentions(p.Name, "projudge")) &&
            properties.Any(p => p.PropertyType.IsEnum && Mentions(p.Name, "level"));
    }

    // -----------------------------------------------------------------------
    // Diagnostics: the field-name discovery instrument (Diagnostics.DumpPanelFields)
    // -----------------------------------------------------------------------
    private void Dump(string trigger)
    {
        try
        {
            object? instance = panel;
            if (instance == null)
            {
                log.LogInfo("Judge panel field dump: no judge panel has been observed in this session.");
            }
            else
            {
                Type type = instance.GetType();
                log.LogInfo($"Judge panel field dump ({trigger}): {type.FullName}; " +
                    $"{type.GetFields(AllInstance).Length} fields, {type.GetProperties(AllInstance).Length} properties");
                foreach (FieldInfo field in type.GetFields(AllInstance))
                    log.LogInfo($"  field {field.Name} : {TypeName(field.FieldType)} = {Plugin.DiagnosticRead(() => field.GetValue(instance))}{IntentState(field, instance)}");
                foreach (PropertyInfo property in type.GetProperties(AllInstance).Where(p => p.GetIndexParameters().Length == 0))
                    log.LogInfo($"  property {property.Name} : {TypeName(property.PropertyType)} = {Plugin.DiagnosticRead(() => property.GetValue(instance))}{IntentState(property, instance)}{ControlState(property, instance)}");
            }
            DumpPlayInfo();
        }
        catch (Exception ex)
        {
            // A diagnostic must never break the bridge.
            log.LogWarning($"Judge panel field dump failed: {ex.GetBaseException().Message}");
        }
    }

    private void DumpPlayInfo()
    {
        object? info;
        try { info = playInfo.Invoke(null, null); }
        catch (Exception ex)
        {
            log.LogInfo("Judge play settings dump: unreadable (" + ex.GetBaseException().GetType().Name + ")");
            return;
        }
        if (info == null)
        {
            log.LogInfo("Judge play settings dump: not available yet.");
            return;
        }
        log.LogInfo($"Judge play settings dump: {info.GetType().FullName} zero-argument getters");
        foreach (MethodInfo getter in playInfo.ReturnType.GetMethods(AllInstance)
            .Where(m => m.Name.StartsWith("get_", StringComparison.Ordinal) && m.GetParameters().Length == 0))
            log.LogInfo($"  play-info {getter.Name[4..]} : {TypeName(getter.ReturnType)} = {Plugin.DiagnosticRead(() => getter.Invoke(info, null))}");
    }

    /// <summary>For a member holding the judge record: its Pro flag and level, on the same line.</summary>
    private string IntentState(MemberInfo member, object instance)
    {
        if (!CarriesIntent(MemberType(member))) return "";
        var why = new List<string>();
        object? intent = ReadMember(member, instance, why, "panel record");
        if (intent == null) return why.Count == 0 ? "" : " [record unreadable: " + why[0] + "]";
        return $" [proJudge={Plugin.DiagnosticRead(() => ReadMember(intentPro, intent, why, "pro"))}, " +
            $"level={Plugin.DiagnosticRead(() => ReadMember(intentLevel, intent, why, "level"))}]";
    }

    /// <summary>For a control member: every state candidate it exposes, on the same line.</summary>
    private string ControlState(MemberInfo member, object instance)
    {
        if (!IsControl(MemberType(member))) return "";
        object? control = ReadMember(member, instance, new List<string>(), member.Name);
        if (control == null) return "";
        Type type = control.GetType();
        var parts = new List<string>
        {
            type.GetProperty("Current", AllInstance) == null
                ? "Current=absent"
                : "Current=" + Plugin.DiagnosticRead(() => type.GetProperty("Current", AllInstance)!.GetValue(control))
        };
        foreach (MemberInfo leaf in LeafMembers(type))
            parts.Add($"{leaf.Name}={Plugin.DiagnosticRead(() => ReadMember(leaf, control, new List<string>(), leaf.Name))}");
        return " [" + string.Join(", ", parts) + "]";
    }

    // -----------------------------------------------------------------------
    // Small reflection helpers
    // -----------------------------------------------------------------------
    private static object? ReadMember(MemberInfo? member, object? target, List<string> why, string label)
    {
        if (member == null || target == null) return null;
        try
        {
            // Members arrive both ways: discovered records are properties or fields,
            // while the play settings judge level is a plain getter method.
            return member switch
            {
                PropertyInfo property => property.GetValue(target),
                FieldInfo field => field.GetValue(target),
                MethodInfo method => method.Invoke(target, null),
                _ => null
            };
        }
        catch (Exception ex)
        {
            why.Add($"{label} {member.Name} threw {ex.GetBaseException().GetType().Name}");
            return null;
        }
    }

    private static Type MemberType(MemberInfo member) => member switch
    {
        PropertyInfo property => property.PropertyType,
        FieldInfo field => field.FieldType,
        MethodInfo method => method.ReturnType,
        _ => typeof(object)
    };

    private static bool Mentions(string name, string text) =>
        name.Replace("_", "").Contains(text, StringComparison.OrdinalIgnoreCase);

    private static bool IsControl(Type type)
    {
        for (Type? current = type; current != null; current = current.BaseType)
            if (current.Name is "MonoBehaviour" or "Component") return true;
        return type.Namespace != null &&
            (type.Namespace.StartsWith("Malody.UI", StringComparison.Ordinal) ||
             type.Namespace.StartsWith("Malody.Scene.Widget", StringComparison.Ordinal));
    }

    private static IEnumerable<MemberInfo> LeafMembers(Type type)
    {
        IEnumerable<MemberInfo> members = type.GetProperties(AllInstance).Where(p => p.GetIndexParameters().Length == 0)
            .Cast<MemberInfo>()
            .Concat(type.GetFields(AllInstance));
        return members.Where(m => !Noise.Contains(m.Name) && !m.Name.StartsWith("m_", StringComparison.Ordinal) &&
            !m.Name.StartsWith("Native", StringComparison.Ordinal) && IsLeaf(MemberType(m)));
    }

    private static bool IsLeaf(Type type) =>
        type == typeof(bool) || type == typeof(int) || type == typeof(float) || type == typeof(string) || type.IsEnum;

    private static string TypeName(Type type) => type.FullName ?? type.Name;

    private static string Describe(MemberInfo? member) =>
        member == null ? "absent" : member.DeclaringType!.Name + "." + member.Name;
}
