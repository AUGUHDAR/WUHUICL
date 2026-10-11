using System;
using System.IO;
using System.Net.Http;
using System.Text.Json;
using System.Threading.Tasks;

namespace Wuhuicl.App.Services;

/// <summary>桥接 MOD 面板后端（KAMUCL modBridge.ts 翻译）。</summary>
public sealed class BridgeService
{
    static readonly HttpClient Http = new() { Timeout = TimeSpan.FromSeconds(3) };
    readonly string _root;
    public BridgeService(string gameRoot) => _root = gameRoot;

    string ModsDir(string vid) => Path.Combine(_root, "versions", vid, "mods");

    public bool IsInstalled(string vid)
    {
        var dir = ModsDir(vid);
        return Directory.Exists(dir) && Directory.EnumerateFiles(dir, "kamucl-bridge*.jar").Any();
    }

    public void Install(string vid)
    {
        var dir = ModsDir(vid);
        Directory.CreateDirectory(dir);
        using var s = typeof(BridgeService).Assembly.GetManifestResourceStream("Wuhuicl.App.Assets.kamucl-bridge.jar")
            ?? throw new InvalidOperationException("内嵌桥接 JAR 缺失");
        using var f = File.Create(Path.Combine(dir, "kamucl-bridge-1.0.1.jar"));
        s.CopyTo(f);
    }

    public record BridgeStatus(bool Connected, string ModVersion, string Reason);
    public record BridgeParam(string Id, string ModId, string Label, string Description, string Group,
        string Kind, string Apply, string Scope, JsonElement Default,
        double? Min, double? Max, double? Step, string[]? Options, bool Visible)
    {
        /// <summary>当前值：set/reset 后以游戏回包为准（applyParam p.value = result.value）。</summary>
        public JsonElement? Value { get; set; }
    }

    record Disc(int Port, string Token, string ModVersion, int Pid);
    Disc? ReadDisc(string vid)
    {
        try
        {
            var p = Path.Combine(_root, "versions", vid, ".kamucl-bridge.json");
            if (!File.Exists(p)) return null;
            using var d = JsonDocument.Parse(File.ReadAllText(p));
            var r = d.RootElement;
            return new(r.GetProperty("port").GetInt32(), r.GetProperty("token").GetString() ?? "",
                r.GetProperty("modVersion").GetString() ?? "", r.GetProperty("pid").GetInt32());
        }
        catch { return null; }
    }

    static HttpRequestMessage Req(Disc d, HttpMethod m, string path, string? json = null)
    {
        var msg = new HttpRequestMessage(m, $"http://127.0.0.1:{d.Port}/kamucl/v1/{path}");
        msg.Headers.Add("X-Kamucl-Token", d.Token);
        if (json is not null) msg.Content = new StringContent(json, System.Text.Encoding.UTF8, "application/json");
        return msg;
    }

    public async Task<BridgeStatus> StatusAsync(string vid)
    {
        var d = ReadDisc(vid);
        if (d is null) return new(false, "", "未找到发现文件");
        if (!IsInstalled(vid)) return new(false, d.ModVersion, "桥接 MOD 未安装");
        try { using var p = System.Diagnostics.Process.GetProcessById(d.Pid); if (p.HasExited) return new(false, d.ModVersion, "进程已退出"); }
        catch { return new(false, d.ModVersion, "进程已退出"); }
        try
        {
            using var r = await Http.SendAsync(Req(d, HttpMethod.Get, "ping"));
            return r.IsSuccessStatusCode ? new(true, d.ModVersion, "已连接") : new(false, d.ModVersion, $"HTTP {r.StatusCode}");
        }
        catch { return new(false, d.ModVersion, "无法连接"); }
    }

    public async Task<BridgeParam[]> ManifestAsync(string vid)
    {
        var d = ReadDisc(vid) ?? throw new InvalidOperationException("未找到发现文件");
        using var r = await Http.SendAsync(Req(d, HttpMethod.Get, "manifest"));
        r.EnsureSuccessStatusCode();
        using var doc = JsonDocument.Parse(await r.Content.ReadAsStringAsync());
        return doc.RootElement.GetProperty("params").EnumerateArray().Select(p => new BridgeParam(
            p.GetProperty("id").GetString() ?? "", p.GetProperty("modId").GetString() ?? "",
            p.GetProperty("label").GetString() ?? "",
            p.TryGetProperty("description", out var de) ? de.GetString() ?? "" : "",
            p.TryGetProperty("group", out var gr) ? gr.GetString() ?? "" : "",
            p.GetProperty("kind").GetString() ?? "",
            p.TryGetProperty("apply", out var ap) ? ap.GetString() ?? "INSTANT" : "INSTANT",
            p.TryGetProperty("scope", out var sc) ? sc.GetString() ?? "CLIENT" : "CLIENT",
            p.GetProperty("defaultValue").Clone(),
            p.TryGetProperty("min", out var mn) ? mn.GetDouble() : null,
            p.TryGetProperty("max", out var mx) ? mx.GetDouble() : null,
            p.TryGetProperty("step", out var st) ? st.GetDouble() : null,
            p.TryGetProperty("options", out var op) ? op.EnumerateArray().Select(o => o.GetString() ?? "").ToArray() : null,
            !p.TryGetProperty("visible", out var vi) || vi.GetBoolean())
        {
            Value = p.TryGetProperty("value", out var v) ? v.Clone() : null,
        }).ToArray();
    }

    public async Task<JsonElement> SetAsync(string vid, string id, object value)
    {
        var d = ReadDisc(vid) ?? throw new InvalidOperationException("未找到发现文件");
        using var r = await Http.SendAsync(Req(d, HttpMethod.Post, "set", JsonSerializer.Serialize(new { id, value })));
        r.EnsureSuccessStatusCode();
        using var doc = JsonDocument.Parse(await r.Content.ReadAsStringAsync());
        return doc.RootElement.Clone();
    }

    public async Task ResetAsync(string vid, string? id = null)
    {
        var d = ReadDisc(vid) ?? throw new InvalidOperationException("未找到发现文件");
        using var r = await Http.SendAsync(Req(d, HttpMethod.Post, "reset", JsonSerializer.Serialize(new { id })));
        r.EnsureSuccessStatusCode();
    }
}
