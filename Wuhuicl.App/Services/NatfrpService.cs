using System.Diagnostics;
using System.IO;
using System.Net.Http;
using System.Net.Http.Headers;
using System.Text.Json;
using Wuhuicl.Core.Storage;

namespace Wuhuicl.App.Services;

/// <summary>
/// 樱花穿透（natfrp.com）FRP 后端：v4 API 读隧道列表 + frpc 客户端进程管理。
/// 联机页 FriendsPage 的 FRP 面板从这里取全部能力。
/// （NatfrpTunnel 记录类型在 FriendsService.cs 里声明，同一命名空间直接复用。）
///
/// API：GET https://api.natfrp.com/v4/tunnels，鉴权 Authorization: Bearer &lt;访问密钥&gt;（或 ?token=&lt;key&gt;），
/// 返回 {"code":200,"data":[{id,name,node,protocol,local_addr,local_port,remote_addr,remote_port,enabled},...]}。
/// 字段解析全部按 TryGetProperty 防御式处理（个别字段缺失/类型漂移时给默认值，不炸页面）。
///
/// frpc 启动：樱花穿透定制版 frpc 用「访问密钥:隧道ID」做凭证，
/// 即 `frpc -f &lt;访问密钥&gt;:&lt;隧道ID&gt;`（官方启动器同款参数格式）。
/// frpc.exe 由用户从 natfrp.com 下载后放到 &lt;DataDir&gt;\frpc\，或在本页手动选择路径。
/// </summary>
public static class NatfrpService
{
    const string ApiBase = "https://api.natfrp.com/v4";

    static readonly HttpClient Http = CreateHttp();

    static HttpClient CreateHttp()
    {
        var c = new HttpClient { Timeout = TimeSpan.FromSeconds(20) };
        c.DefaultRequestHeaders.UserAgent.ParseAdd("WUHUICL/1.0");
        return c;
    }

    /// <summary>frpc 客户端存放目录（用户把下载的 frpc.exe 放进来，或在本页手动选择）。</summary>
    public static string FrpcDir => Path.Combine(GamePaths.DetectDefault().DataDir, "frpc");

    /// <summary>在 &lt;DataDir&gt;\frpc\ 里找 frpc 可执行文件；没有则 null。</summary>
    public static string? FindFrpc()
    {
        try
        {
            if (!Directory.Exists(FrpcDir)) return null;
            return Directory.EnumerateFiles(FrpcDir, "frpc*.exe")
                .OrderByDescending(File.GetLastWriteTime)
                .FirstOrDefault();
        }
        catch
        {
            return null;
        }
    }

    /// <summary>读取账号下的隧道列表。token 即 natfrp.com 的访问密钥。</summary>
    public static async Task<List<NatfrpTunnel>> GetTunnels(string token)
    {
        if (string.IsNullOrWhiteSpace(token))
            throw new InvalidOperationException("请先填写访问密钥（natfrp.com → 用户面板 → 访问密钥）");

        using var req = new HttpRequestMessage(HttpMethod.Get, $"{ApiBase}/tunnels");
        req.Headers.Authorization = new AuthenticationHeaderValue("Bearer", token.Trim());
        using var resp = await Http.SendAsync(req);
        var body = await resp.Content.ReadAsStringAsync();
        using var doc = JsonDocument.Parse(body);
        var root = doc.RootElement;

        int code = root.TryGetProperty("code", out var c) && c.TryGetInt32(out var i) ? i : (int)resp.StatusCode;
        if (code != 200)
        {
            var msg = root.TryGetProperty("msg", out var m0) ? m0.GetString() :
                      root.TryGetProperty("error", out var e) ? e.GetString() :
                      root.TryGetProperty("message", out var m) ? m.GetString() : null;
            throw new InvalidOperationException($"natfrp 接口返回 code={code}" + (string.IsNullOrEmpty(msg) ? "" : $"：{msg}"));
        }

        var data = root.GetProperty("data");
        var list = new List<NatfrpTunnel>();
        if (data.ValueKind == JsonValueKind.Array)
            foreach (var t in data.EnumerateArray())
                list.Add(ParseTunnel(t));
        return list;
    }

    static NatfrpTunnel ParseTunnel(JsonElement t)
    {
        static string Str(JsonElement e, string name, string fallback = "")
        {
            if (!e.TryGetProperty(name, out var v)) return fallback;
            return v.ValueKind switch
            {
                JsonValueKind.String => v.GetString() ?? fallback,
                JsonValueKind.Number => v.GetRawText(),
                _ => fallback,
            };
        }
        static int Int(JsonElement e, string name)
        {
            return e.TryGetProperty(name, out var v) && v.TryGetInt32(out var i) ? i : 0;
        }
        bool enabled = !t.TryGetProperty("enabled", out var en) || en.ValueKind != JsonValueKind.False;
        var protocol = Str(t, "protocol");
        if (string.IsNullOrEmpty(protocol)) protocol = Str(t, "frp_protocol", "tcp");
        var remoteAddr = Str(t, "remote_addr");
        var remotePort = Int(t, "remote_port");
        // remote_addr 有时只回主机名、有时带端口，统一拆干净再拼
        remoteAddr = remoteAddr.Split(':')[0];
        uint tid = 0;
        if (t.TryGetProperty("id", out var id) && id.TryGetUInt32(out var uid)) tid = uid;
        var name = Str(t, "name");
        if (string.IsNullOrEmpty(name)) name = tid != 0 ? $"隧道 {tid}" : "未命名隧道";
        return new NatfrpTunnel(
            tid,
            name,
            Str(t, "node"),
            protocol,
            Str(t, "local_addr", "127.0.0.1"),
            Int(t, "local_port"),
            remoteAddr,
            remotePort,
            enabled);
    }

    /// <summary>启动 frpc 连接指定隧道（樱花穿透定制版参数：-f 访问密钥:隧道ID）。</summary>
    public static async Task<Process> StartFrpc(string frpcPath, string token, NatfrpTunnel tunnel)
    {
        if (string.IsNullOrWhiteSpace(frpcPath) || !File.Exists(frpcPath))
            throw new InvalidOperationException("未找到 frpc 客户端：请先下载 frpc.exe（natfrp.com → 软件下载），放到 DataDir\\frpc\\ 或点击「浏览…」选择");
        if (tunnel.Id == 0)
            throw new InvalidOperationException("隧道 ID 无效");

        Directory.CreateDirectory(FrpcDir);
        var psi = new ProcessStartInfo
        {
            FileName = frpcPath,
            Arguments = $"-f {token.Trim()}:{tunnel.Id}",
            UseShellExecute = false,
            CreateNoWindow = true,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            WorkingDirectory = Path.GetDirectoryName(frpcPath) ?? FrpcDir,
        };
        var proc = Process.Start(psi) ?? throw new InvalidOperationException("frpc 进程启动失败");
        // 防僵尸：进程退出后回收句柄
        _ = Task.Run(() => { try { proc.WaitForExit(); } catch { } });
        await Task.Delay(80); // 给进程一点起来就崩的时间窗，启动后立刻退出时页面能及时反映
        return proc;
    }

    /// <summary>停止 frpc 进程（含子进程）。</summary>
    public static void StopFrpc(Process proc)
    {
        try
        {
            if (!proc.HasExited) proc.Kill(true);
        }
        catch
        {
            // 进程可能已自行退出
        }
    }
}
