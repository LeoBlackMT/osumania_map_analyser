using System.Net;
using System.Text;
using System.Text.Json;
using BepInEx.Logging;

namespace MalodyInsightBridge;

/// <summary>
/// The network half of the selection bridge: one HttpClient, one loopback
/// destination, one serialised send loop. Selection state stays in
/// <see cref="Plugin"/>; the caller hands over a callback that returns the
/// message that is due, or null when nothing should be sent yet.
///
/// The transport rules are the upstream ones, deliberately unchanged:
///   127.0.0.1 only, POST to the configured path (default port 17653);
///   no proxy, no redirects, 1.5 s timeout;
///   at most one POST in flight, so timer callbacks never pile up;
///   a failing companion is reported at most once every 15 s.
/// </summary>
internal sealed class BridgeClient : IDisposable
{
    private readonly HttpClient http;
    private readonly ManualLogSource log;
    private DateTime lastNetworkWarning = DateTime.MinValue;
    private int sending;
    private bool networkConnected;

    public BridgeClient(int port, string route, ManualLogSource log)
    {
        if (port < 1 || port > 65535 || !route.StartsWith('/') || route.StartsWith("//") || route.Contains('?') || route.Contains('#'))
            throw new ArgumentException("Invalid loopback service port or SelectionPath.");
        this.log = log;
        Endpoint = new UriBuilder(Uri.UriSchemeHttp, IPAddress.Loopback.ToString(), port, route).Uri;
        http = new HttpClient(new HttpClientHandler { UseProxy = false, AllowAutoRedirect = false })
        {
            Timeout = TimeSpan.FromSeconds(1.5)
        };
    }

    /// <summary>The one destination this bridge ever posts to.</summary>
    public Uri Endpoint { get; }

    /// <summary>
    /// Posts the message the caller reports as due. The serialisation guard is
    /// taken before the callback runs, so a pump that collides with an in-flight
    /// POST leaves the caller's pending message untouched for the next tick.
    /// </summary>
    public async Task Pump(Func<Selection?> due, CancellationToken token)
    {
        if (token.IsCancellationRequested || Interlocked.Exchange(ref sending, 1) != 0) return;
        try
        {
            Selection? message = due();
            if (message == null) return;
            using var content = new StringContent(JsonSerializer.Serialize(message), Encoding.UTF8, "application/json");
            using HttpResponseMessage response = await http.PostAsync(Endpoint, content, token).ConfigureAwait(false);
            if (!response.IsSuccessStatusCode) throw new HttpRequestException($"Companion returned HTTP {(int)response.StatusCode}.");
            if (!networkConnected) log.LogInfo("Companion service connected.");
            networkConnected = true;
        }
        catch (OperationCanceledException) when (token.IsCancellationRequested) { }
        catch (Exception ex)
        {
            networkConnected = false;
            if ((DateTime.UtcNow - lastNetworkWarning).TotalSeconds >= 15)
            {
                lastNetworkWarning = DateTime.UtcNow;
                log.LogWarning($"Companion unavailable at {Endpoint}: {ex.GetBaseException().Message}. Will retry.");
            }
        }
        finally { Volatile.Write(ref sending, 0); }
    }

    public void Dispose() => http.Dispose();
}
