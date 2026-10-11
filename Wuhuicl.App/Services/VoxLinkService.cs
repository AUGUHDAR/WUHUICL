using System.Net;
using System.Net.Http;
using System.Net.Sockets;
using System.Net.Http.Headers;
using System.Text;
using System.Text.Json;

namespace Wuhuicl.App.Services;

/// <summary>VoxLink 房间信息（join 响应 room 字段的服务端公开视图，roommanager.go :874-897）。</summary>
public record VoxLinkRoomInfo(
    string Name, string Code, int CurrentPlayers, int MaxPlayers,
    bool HasPassword, int HostPort, string GameVersion, string Loader, string Category);

/// <summary>POST /room/create 的 data（app-desktop/app.go CreateRoom 的响应结构 :506-512）。</summary>
public record VoxLinkCreateResult(
    string Code, string HostToken, string Name, string HostIp, int HostPort, int ExpiresIn);

/// <summary>POST /room/join 的 data（app-desktop/app.go JoinRoom 的响应结构 :585-588）。</summary>
public record VoxLinkJoinResult(string ClientToken, string ClientId, VoxLinkRoomInfo? Room);

/// <summary>POST /room/heartbeat 的 data（app-desktop/session.go heartbeatLoop :157-161）。</summary>
public record VoxLinkHeartbeat(int HeartbeatInterval, int CurrentPlayers, string Name);

/// <summary>VoxLink 服务端业务错误。Code 是服务端 error 字段原串（ROOM_NOT_FOUND_OR_WRONG_PASSWORD 等）。</summary>
public sealed class VoxLinkException(string code, string message) : Exception(message)
{
    public string Code { get; } = code;
}

/// <summary>
/// VoxLink 联机（用户自研项目，服务端 D:\桌面\VoxLink-server\server-go，Go）。
///
/// API 形态（app-desktop/api.go buildRequestURL :62-85）：所有调用都是
/// `POST/GET {base}/?route=路由` + JSON body，统一响应信封 {success,data,error,message}（:39-44）。
/// 路由由 server-go 的 RegisterRoute 注册（handlers_room.go :7-16 / handlers_signal.go :10-15）：
///   POST /room/create   body {name,maxPlayers,hostPort,natType,visible,category,gameVersion,loader,
///                             clientType:"app",clientProtocolVersion:7,clientCapabilities[,password]}
///                       → data {code,hostToken,name,hostIp,hostPort,expiresIn}
///   POST /room/join     body {code,clientType:"app",clientProtocolVersion:7,clientCapabilities[,password]}
///                       → data {clientToken,clientId,room{...}}
///   POST /room/leave    body {code,token,isHost}（app-desktop/app.go leaveInternal :204-208）
///   POST /room/heartbeat body {code,token,isHost,seq,currentInterval,mcPlayerCount}
///                       → data {heartbeatInterval,currentPlayers,name}；房间失效 → HTTP 410
/// 房间码：6 位，字符集 ABCDEFGHJKLMNPQRSTUVWXYZ23456789（app-desktop/settings.go validateRoomCode :85-95）。
/// 默认服务器 https://p2p.wuhui.icu（app-desktop/app.go defaultServerURL :21）；自建服务端默认端口 1145
/// （server-go/main.go :94 的 VOXLINK_PORT 环境变量）。
///
/// 说明：create/join/leave/heartbeat 都是纯 HTTP，不需要 WebSocket；P2P 打洞/TURN 中继由
/// VoxLink 桌面端的连接引擎（app-desktop/punch*.go / relay*.go）完成，启动器不做，
/// 房间保活（心跳）在这里实现，否则房间会被服务端按超时回收。
/// </summary>
public static class VoxLinkService
{
    /// <summary>官方默认服务器（app-desktop/app.go defaultServerURL :21）。</summary>
    public const string DefaultServerUrl = "https://p2p.wuhui.icu";

    /// <summary>房间码字符集（settings.go :89：A-Z 去掉 I/O、2-9 去掉 0/1）。</summary>
    public const string CodeCharset = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

    /// <summary>客户端能力组（app-desktop/app.go appCapabilities :30）。</summary>
    static readonly string[] Capabilities = ["relay", "ice_restart", "continuous_retry", "punchAuthV1", "stdTurnV1"];

    static readonly HttpClient Http = CreateHttp();

    static HttpClient CreateHttp()
    {
        var c = new HttpClient { Timeout = TimeSpan.FromSeconds(15) };
        c.DefaultRequestHeaders.UserAgent.ParseAdd("VoxLink-App/1.2.0");
        // /room/list 用 X-VoxLink-Version 决定是否展示互通房（handlers_room.go :171），带上对齐桌面端
        c.DefaultRequestHeaders.Add("X-VoxLink-Version", "1.2.0");
        return c;
    }

    // ---------------- 基础设施 ----------------

    /// <summary>规整服务器地址：补协议、去尾斜杠。参考 app-desktop/api.go buildRequestURL :62-85 的校验口径。</summary>
    public static string NormalizeBaseUrl(string input)
    {
        var s = (input ?? "").Trim();
        if (s.Length == 0) s = DefaultServerUrl;
        if (!s.StartsWith("http://", StringComparison.OrdinalIgnoreCase) &&
            !s.StartsWith("https://", StringComparison.OrdinalIgnoreCase))
            s = "https://" + s;
        if (!Uri.TryCreate(s, UriKind.Absolute, out var uri) ||
            (uri.Scheme != "http" && uri.Scheme != "https") ||
            uri.Host.Length == 0)
            throw new VoxLinkException("NETWORK", "服务器地址无效：需要 http:// 或 https:// 开头的主机地址");
        return s.TrimEnd('/');
    }

    /// <summary>6 位房间码校验（settings.go validateRoomCode :85-95）。</summary>
    public static bool ValidateRoomCode(string code)
    {
        if (string.IsNullOrEmpty(code) || code.Length != 6) return false;
        foreach (var ch in code)
            if (!CodeCharset.Contains(char.ToUpperInvariant(ch)))
                return false;
        return true;
    }

    /// <summary>服务端错误码 → 用户可读文案（错误码逐条来自 server-go 各 handler 的 fail 分支）。</summary>
    static string Friendly(string code, string message)
    {
        var text = code switch
        {
            "ROOM_NOT_FOUND_OR_WRONG_PASSWORD" => "房间不存在或密码错误",
            "ROOM_NOT_FOUND" => "房间不存在（可能已过期）",
            "ROOM_FULL" => "房间已满",
            "ROOM_EVICTED" => "房间已被管理员移出",
            "ROOM_EXPIRED" => "房间已过期",
            "ROOM_CLOSED" => "房间已关闭",
            "CODE_REQUIRED" or "CODE_AND_TOKEN_REQUIRED" => "房间码不能为空",
            "PASSWORD_TOO_LONG" => "密码过长（最多 32 字符）",
            "CONTENT_BLOCKED" => "房间名包含被屏蔽的内容",
            "MAX_ROOMS_REACHED" => "服务器房间数已达上限，请稍后再试",
            "INVALID_TOKEN" => "会话已失效，请重新创建或加入房间",
            "NAME_IN_USE" => "该玩家名已被房间内成员使用",
            "NAME_REQUIRED" => "缺少玩家名",
            "PLAYER_NAME_INVALID" => "玩家名格式不合法",
            "RATE_LIMITED" => "请求过于频繁，请稍后重试",
            "NETWORK" => "网络请求失败",
            _ => "",
        };
        if (text.Length == 0) text = "服务端返回错误 " + code;
        return string.IsNullOrEmpty(message) ? text : $"{text}（{message}）";
    }

    /// <summary>执行一次 API 调用并拆信封，成功返回 data 字段。信封结构见 app-desktop/api.go :39-44、:119-167。</summary>
    static async Task<JsonElement> DoAsync(string baseUrl, string route, object? body, HttpMethod? method = null)
    {
        var url = $"{baseUrl}/?route={Uri.EscapeDataString(route)}";
        using var req = new HttpRequestMessage(method ?? HttpMethod.Post, url);
        if (body != null)
        {
            req.Content = new StringContent(JsonSerializer.Serialize(body), Encoding.UTF8);
            req.Content.Headers.ContentType = new MediaTypeHeaderValue("application/json");
        }
        using var resp = await Http.SendAsync(req);
        var raw = await resp.Content.ReadAsStringAsync();

        JsonElement root;
        try
        {
            root = JsonDocument.Parse(raw).RootElement.Clone();
        }
        catch (JsonException)
        {
            throw new VoxLinkException($"HTTP_{(int)resp.StatusCode}",
                $"响应解析失败（HTTP {(int)resp.StatusCode}），请确认服务器地址指向 VoxLink 服务端");
        }

        bool success = root.TryGetProperty("success", out var s) && s.ValueKind == JsonValueKind.True;
        string err = root.TryGetProperty("error", out var e) && e.ValueKind == JsonValueKind.String
            ? e.GetString() ?? "" : "";
        string msg = root.TryGetProperty("message", out var m) && m.ValueKind == JsonValueKind.String
            ? m.GetString() ?? "" : "";

        if (resp.StatusCode == HttpStatusCode.TooManyRequests)
            throw new VoxLinkException(string.IsNullOrEmpty(err) ? "RATE_LIMITED" : err,
                msg.Length == 0 ? "请求过于频繁，请稍后重试" : msg);
        if (!success)
            throw new VoxLinkException(string.IsNullOrEmpty(err) ? $"HTTP_{(int)resp.StatusCode}" : err,
                Friendly(string.IsNullOrEmpty(err) ? $"HTTP_{(int)resp.StatusCode}" : err, msg));

        if (root.TryGetProperty("data", out var data) && data.ValueKind == JsonValueKind.Object)
            return data;
        throw new VoxLinkException("BAD_RESPONSE", "响应缺少 data 字段");
    }

    static string Str(JsonElement e, string name, string fallback = "")
        => e.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.String ? v.GetString() ?? fallback : fallback;

    static int Int(JsonElement e, string name, int fallback = 0)
        => e.TryGetProperty(name, out var v) && v.TryGetInt32(out var i) ? i : fallback;

    static bool Bool(JsonElement e, string name)
        => e.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.True;

    static VoxLinkRoomInfo ParseRoom(JsonElement e) => new(
        Str(e, "name"),
        Str(e, "code"),
        Int(e, "currentPlayers"),
        Int(e, "maxPlayers"),
        Bool(e, "hasPassword"),
        Int(e, "hostPort", 25565),
        Str(e, "gameVersion"),
        Str(e, "loader", "unknown"),
        Str(e, "category", "other"));

    /// <summary>create/join 请求里客户端自报的公共字段（对齐 app-desktop/app.go :473-482、:561-566）。</summary>
    static Dictionary<string, object> ClientFields(string code = "")
    {
        var body = new Dictionary<string, object>
        {
            ["clientType"] = "app",
            ["clientProtocolVersion"] = 7,
            ["clientCapabilities"] = Capabilities,
        };
        if (code.Length > 0) body["code"] = code;
        return body;
    }

    // ---------------- 对外 API（页面只用到这五个） ----------------

    /// <summary>创建房间（POST /room/create）。成功返回房间码 + hostToken（leave/heartbeat 都要用）。</summary>
    public static async Task<VoxLinkCreateResult> CreateRoomAsync(string baseUrl, string name, int hostPort,
        string? password, string? gameVersion = null, string? loader = null, string? category = null)
    {
        var body = ClientFields();
        body["name"] = string.IsNullOrWhiteSpace(name) ? "VoxLink Room" : name.Trim();
        body["maxPlayers"] = 20;                       // 桌面端固定 20（app.go :477）
        body["hostPort"] = hostPort;
        body["natType"] = "unknown";                   // 不做 STUN 探测，服务端对 unknown 宽容（roommanager.go :348）
        body["visible"] = true;
        body["category"] = string.IsNullOrWhiteSpace(category) ? "other" : category.Trim();
        body["gameVersion"] = string.IsNullOrWhiteSpace(gameVersion) ? "unknown" : gameVersion.Trim();
        body["loader"] = string.IsNullOrWhiteSpace(loader) ? "unknown" : loader.Trim().ToLowerInvariant();
        if (!string.IsNullOrEmpty(password)) body["password"] = password;

        var data = await DoAsync(NormalizeBaseUrl(baseUrl), "/room/create", body);
        return new VoxLinkCreateResult(
            Str(data, "code"),
            Str(data, "hostToken"),
            Str(data, "name"),
            Str(data, "hostIp"),
            Int(data, "hostPort", hostPort),
            Int(data, "expiresIn"));
    }

    /// <summary>加入房间（POST /room/join）。成功返回 clientToken/clientId 与房间公开信息。</summary>
    public static async Task<VoxLinkJoinResult> JoinRoomAsync(string baseUrl, string code, string? password)
    {
        var upper = (code ?? "").Trim().ToUpperInvariant();
        if (!ValidateRoomCode(upper))
            throw new VoxLinkException("INVALID_PARAMS", "房间码必须为 6 位（A-Z 去掉 I/O、2-9 去掉 0/1）");

        var body = ClientFields(upper);
        if (!string.IsNullOrEmpty(password)) body["password"] = password;

        var data = await DoAsync(NormalizeBaseUrl(baseUrl), "/room/join", body);
        VoxLinkRoomInfo? room = null;
        if (data.TryGetProperty("room", out var r) && r.ValueKind == JsonValueKind.Object)
            room = ParseRoom(r);
        return new VoxLinkJoinResult(
            Str(data, "clientToken"),
            Str(data, "clientId"),
            room);
    }

    /// <summary>离开房间（POST /room/leave，body 见 app-desktop/app.go :204-208）。尽力而为，失败不抛。</summary>
    public static async Task LeaveAsync(string baseUrl, string code, string token, bool isHost)
    {
        if (string.IsNullOrEmpty(code) || string.IsNullOrEmpty(token)) return;
        try
        {
            await DoAsync(NormalizeBaseUrl(baseUrl), "/room/leave",
                new Dictionary<string, object> { ["code"] = code, ["token"] = token, ["isHost"] = isHost });
        }
        catch
        {
            // 房间会因心跳超时被服务端自动回收（app.go leaveInternal :207 同款容错）
        }
    }

    /// <summary>房间心跳（POST /room/heartbeat）。房间失效/会话失效时抛 VoxLinkException（HTTP 410 → 410/ROOM_*）。</summary>
    public static async Task<VoxLinkHeartbeat> HeartbeatAsync(string baseUrl, string code, string token,
        bool isHost, long seq)
    {
        var data = await DoAsync(NormalizeBaseUrl(baseUrl), "/room/heartbeat", new Dictionary<string, object>
        {
            ["code"] = code,
            ["token"] = token,
            ["isHost"] = isHost,
            ["seq"] = seq,
            ["currentInterval"] = 5,
            ["mcPlayerCount"] = 0,
        });
        return new VoxLinkHeartbeat(
            Int(data, "heartbeatInterval", 5),
            Int(data, "currentPlayers"),
            Str(data, "name"));
    }

    /// <summary>TCP 连通性测试（连得上 = 有服务在监听）。host 传 127.0.0.1 测本机、传公网地址测远端。</summary>
    public static async Task<bool> TestTcpAsync(string host, int port, int timeoutMs = 1500)
    {
        try
        {
            using var client = new TcpClient();
            using var cts = new CancellationTokenSource(timeoutMs);
            await client.ConnectAsync(host, port, cts.Token);
            return client.Connected;
        }
        catch
        {
            return false;
        }
    }
}
