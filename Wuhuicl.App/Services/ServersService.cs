using System.ComponentModel;
using System.IO;
using System.Net;
using System.Net.Sockets;
using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using Wuhuicl.Core;
using Wuhuicl.Core.Minecraft;
using Wuhuicl.Core.Storage;

namespace Wuhuicl.App.Services;

/// <summary>探测状态四态去掉「未检测」（行建出来就立刻并发探测），只留三态。</summary>
public enum SrvPingStatus { Pending, Online, Offline }

/// <summary>
/// 服务器列表条目 = servers.dat 记录（name/ip/icon）+ 最近一次 Server List Ping 结果。
/// 展示串（状态胶囊/延迟/人数/版本行）按 KAMUCL ServerListItem.vue:14 的语义算好，
/// 供 ServersPage 的 DataTemplate 直接绑定；属性变更可在任意线程触发，WPF 绑定自行封送。
/// </summary>
public sealed class ServerEntry : INotifyPropertyChanged
{
    public event PropertyChangedEventHandler? PropertyChanged;
    void Raise(string n) => PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(n));

    string _name = "";
    public string Name { get => _name; set { _name = value; Raise(nameof(Name)); Raise(nameof(Monogram)); } }
    string _ip = "";
    public string Ip { get => _ip; set { _ip = value; Raise(nameof(Ip)); } }
    bool _favorite;
    public bool IsFavorite { get => _favorite; set { _favorite = value; Raise(nameof(IsFavorite)); } }
    /// <summary>servers.dat 内置的 64×64 PNG 头像原字节（原样写回，避免丢图）。</summary>
    public byte[]? IconBytes { get; set; }
    ImageSource? _icon;
    public ImageSource? IconSource { get => _icon; set { _icon = value; Raise(nameof(IconSource)); Raise(nameof(HasIcon)); } }
    public bool HasIcon => IconSource != null;

    public SrvPingStatus Status { get; private set; } = SrvPingStatus.Pending;
    public int LatencyMs { get; private set; }
    public int PlayersOnline { get; private set; } = -1;
    public int PlayersMax { get; private set; } = -1;
    public string VersionName { get; private set; } = "";
    public string Motd { get; private set; } = "";

    // ---------------- 展示串（ServerListItem.vue:14 / ServerDetails.vue 的拼法） ----------------
    public string Monogram => Name.Length == 0 ? "?" : char.ToUpperInvariant(Name[0]).ToString();
    public string StatusText => Status switch
    {
        SrvPingStatus.Online => "在线",
        SrvPingStatus.Offline => "未连通",
        _ => "检测中",
    };
    public string LatencyText => Status == SrvPingStatus.Online ? $"{Math.Max(LatencyMs, 1)} ms" : "—";
    public string PlayersText => Status == SrvPingStatus.Online
        ? $"{Math.Max(PlayersOnline, 0)}/{Math.Max(PlayersMax, 0)} 人" : "—";
    public string PlayersDetail => Status == SrvPingStatus.Online
        ? $"{Math.Max(PlayersOnline, 0)} / {Math.Max(PlayersMax, 0)}" : "— / —";
    /// <summary>行第三行：在线给探测到的版本名；未连通/检测中按未绑定实例的语义显示。</summary>
    public string VersionLine =>
        Status == SrvPingStatus.Online && VersionName.Length > 0 ? VersionName : "尚未关联实例";

    /// <summary>进入探测：清掉上一轮结果并显示「检测中」。</summary>
    public void BeginPing()
    {
        Status = SrvPingStatus.Pending;
        LatencyMs = 0;
        PlayersOnline = PlayersMax = -1;
        VersionName = "";
        Motd = "";
        Raise(nameof(Status));
        Raise(nameof(LatencyText));
        Raise(nameof(PlayersText));
        Raise(nameof(PlayersDetail));
        Raise(nameof(VersionLine));
        Raise(nameof(Motd));
    }

    /// <summary>写入一轮探测结果（任何线程可调）。</summary>
    public void ApplyPing(SrvPingStatus status, int latencyMs, int online, int max, string version, string motd)
    {
        Status = status;
        LatencyMs = latencyMs;
        PlayersOnline = online;
        PlayersMax = max;
        VersionName = version ?? "";
        Motd = motd ?? "";
        Raise(nameof(Status));
        Raise(nameof(LatencyText));
        Raise(nameof(PlayersText));
        Raise(nameof(PlayersDetail));
        Raise(nameof(VersionLine));
        Raise(nameof(Motd));
        Raise(nameof(VersionName));
    }
}

/// <summary>
/// 服务器页后端：servers.dat 读/写（NbtLite）+ Minecraft Server List Ping。
/// 对齐 KAMUCL-1.1.10/src/main/core/servers.ts：地址解析同 serverUtils.parseServerAddress 主干
/// （域名/IPv4/[IPv6]:端口，默认 25565），延迟只计协议往返，离线不抛错返回 Offline。
/// </summary>
public sealed class ServersService
{
    static GamePaths Paths => GamePaths.DetectDefault();
    static string DatPath => Path.Combine(Paths.Root, "servers.dat");
    static string FavPath => Path.Combine(Paths.DataDir, "servers-favorites.json");

    // ---------------- 读 ----------------

    /// <summary>读 servers.dat → 条目列表；文件不存在返回空表；损坏抛异常（页面显示错误条）。
    /// name 缺省回退到 ip，收藏从启动器侧的 servers-favorites.json 恢复（dat 没有收藏字段）。</summary>
    public List<ServerEntry> LoadFromDat()
    {
        if (!File.Exists(DatPath)) return [];
        var root = NbtLite.ParseFile(DatPath)
            ?? throw new InvalidDataException("servers.dat 不是合法的 NBT 文件");
        if (!root.TryGetValue("servers", out var raw) || raw is not List<object?> list)
            throw new InvalidDataException("servers.dat 缺少 servers 列表");
        var favorites = LoadFavorites();
        var result = new List<ServerEntry>();
        foreach (var item in list)
        {
            if (item is not Dictionary<string, object> c) continue;
            string ip = c.TryGetValue("ip", out var iv) && iv is string s ? s : "";
            if (ip.Length == 0) continue;
            string name = c.TryGetValue("name", out var nv) && nv is string ns && ns.Length > 0 ? ns : ip;
            byte[]? icon = c.TryGetValue("icon", out var ic) && ic is byte[] ba ? ba : null;
            result.Add(new ServerEntry
            {
                Name = name,
                Ip = ip,
                IconBytes = icon,
                IconSource = DecodePng(icon),
                IsFavorite = favorites.Contains(KeyOf(ip)),
            });
        }
        return result;
    }

    // ---------------- 写 ----------------

    /// <summary>把整表写回 servers.dat（临时文件 + 替换，避免半截文件）；会保留条目的内置图标。</summary>
    public async Task SaveDatAsync(IReadOnlyList<ServerEntry> entries)
    {
        var list = entries
            .Select(e => new NbtLite.ServersDatEntry(e.Name, ParseAddress(e.Ip).Display is { Length: > 0 } d ? d : e.Ip.Trim(), e.IconBytes))
            .ToList();
        byte[] bytes = NbtLite.BuildServersDat(list);
        Directory.CreateDirectory(Paths.Root);
        string tmp = DatPath + ".tmp";
        await File.WriteAllBytesAsync(tmp, bytes);
        File.Move(tmp, DatPath, overwrite: true);
    }

    // ---------------- 收藏（servers.dat 没有收藏字段，落在启动器自己的数据目录） ----------------

    HashSet<string> LoadFavorites()
    {
        try
        {
            if (!File.Exists(FavPath)) return [];
            return new HashSet<string>(JsonSerializer.Deserialize<string[]>(File.ReadAllText(FavPath)) ?? []);
        }
        catch { return []; }
    }

    public void SaveFavorites(IEnumerable<ServerEntry> entries)
    {
        try
        {
            Directory.CreateDirectory(Paths.DataDir);
            File.WriteAllText(FavPath,
                JsonSerializer.Serialize(entries.Where(e => e.IsFavorite).Select(e => KeyOf(e.Ip)).ToArray(), Json.Options));
        }
        catch { /* 收藏持久化失败不打断列表 */ }
    }

    // ---------------- Server List Ping（KAMUCL servers.ts:444-602 的 C# 版，超时 3s） ----------------

    /// <summary>对一台服务器做 SLP 探测并写回 entry；任何异常都转成 Offline，不抛出。
    /// 返回日志片段（"32 ms" / "连接超时" / "连接失败"），供页面日志块使用。</summary>
    public static async Task<string> PingAsync(ServerEntry entry, CancellationToken ct = default)
    {
        entry.BeginPing();
        var (host, port, _, err) = ParseAddress(entry.Ip);
        if (err != null)
        {
            entry.ApplyPing(SrvPingStatus.Offline, 0, -1, -1, "", "无法连接（服务器离线或地址错误）");
            return "地址无效";
        }
        try
        {
            using var timeout = CancellationTokenSource.CreateLinkedTokenSource(ct);
            timeout.CancelAfter(TimeSpan.FromSeconds(3));
            using var client = new TcpClient { NoDelay = true };
            await client.ConnectAsync(host, port, timeout.Token);
            await using var stream = client.GetStream();
            // 延迟只计协议往返：发出 Handshake 起表，收到响应首字节止（DNS 与握手不计入）。
            var sw = System.Diagnostics.Stopwatch.StartNew();
            await stream.WriteAsync(Handshake(host, port), timeout.Token);
            await stream.WriteAsync(Frame(0, []), timeout.Token); // Status Request
            byte[] body = await ReadPacketAsync(stream, timeout.Token);
            sw.Stop();
            ApplyStatus(entry, body, (int)sw.ElapsedMilliseconds);
            return $"{Math.Max((int)sw.ElapsedMilliseconds, 1)} ms";
        }
        catch (OperationCanceledException)
        {
            Offline(entry, null);
            return "连接超时";
        }
        catch (Exception)
        {
            Offline(entry, null);
            return "连接失败";
        }
    }

    static void Offline(ServerEntry entry, string? motd)
        => entry.ApplyPing(SrvPingStatus.Offline, 0, -1, -1, "", motd ?? "无法连接（服务器离线或地址错误）");

    /// <summary>解析响应包：varint 包 id(0x00) + varint jsonLen + JSON 状态。</summary>
    static void ApplyStatus(ServerEntry entry, byte[] body, int rttMs)
    {
        int off = 0;
        if (ReadVarInt(body, ref off) != 0) throw new InvalidDataException("响应包类型异常");
        int jsonLen = ReadVarInt(body, ref off);
        string json = Encoding.UTF8.GetString(body, off, Math.Min(jsonLen, body.Length - off));
        using var doc = JsonDocument.Parse(json);
        var root = doc.RootElement;
        int online = 0, max = 0;
        if (root.TryGetProperty("players", out var pl) && pl.ValueKind == JsonValueKind.Object)
        {
            if (pl.TryGetProperty("online", out var o) && o.TryGetInt32(out var oi)) online = oi;
            if (pl.TryGetProperty("max", out var m) && m.TryGetInt32(out var mi)) max = mi;
        }
        string version =
            root.TryGetProperty("version", out var ver) && ver.ValueKind == JsonValueKind.Object
            && ver.TryGetProperty("name", out var vn) && vn.ValueKind == JsonValueKind.String
                ? vn.GetString() ?? "" : "未知";
        string motd = root.TryGetProperty("description", out var d) ? MotdText(d) : "";
        if (motd.Length == 0) motd = "这个服务器没有介绍";
        entry.ApplyPing(SrvPingStatus.Online, Math.Max(rttMs, 1), online, max, version, motd);
        // favicon：data:image/png;base64,... → 解码冻结后可直接跨线程绑定
        if (root.TryGetProperty("favicon", out var fav) && fav.ValueKind == JsonValueKind.String
            && DecodePng(FromDataUri(fav.GetString())) is { } png)
        {
            entry.IconSource = png;
        }
    }

    /// <summary>MOTD 对象转纯文本并去掉 §x 颜色/格式化码（nbt 侧 servers.ts:467 motdText）。</summary>
    static string MotdText(JsonElement node)
    {
        var sb = new StringBuilder();
        void Walk(JsonElement n)
        {
            switch (n.ValueKind)
            {
                case JsonValueKind.String:
                    sb.Append(n.GetString());
                    break;
                case JsonValueKind.Object:
                    if (n.TryGetProperty("text", out var t) && t.ValueKind == JsonValueKind.String)
                        sb.Append(t.GetString());
                    if (n.TryGetProperty("extra", out var ex) && ex.ValueKind == JsonValueKind.Array)
                        foreach (var child in ex.EnumerateArray()) Walk(child);
                    break;
                case JsonValueKind.Array:
                    foreach (var child in n.EnumerateArray()) Walk(child);
                    break;
            }
        }
        Walk(node);
        return Regex.Replace(sb.ToString(), "§[0-9a-fk-or]", "", RegexOptions.IgnoreCase).Trim();
    }

    // ---------------- 协议字节 ----------------

    /// <summary>Handshake(state=1)：协议版本 -1 + 地址 + 端口 + 下一步状态。</summary>
    static byte[] Handshake(string host, int port)
    {
        var addr = Encoding.UTF8.GetBytes(host);
        using var ms = new MemoryStream();
        ms.Write(VarInt(-1));
        ms.Write(VarInt(addr.Length));
        ms.Write(addr);
        ms.WriteByte((byte)(port >> 8));
        ms.WriteByte((byte)port);
        ms.Write(VarInt(1));
        return Frame(0, ms.ToArray());
    }

    /// <summary>组包：varint 长度 + 包 id + 载荷（servers.ts:461 pack）。</summary>
    static byte[] Frame(int id, byte[] payload)
    {
        var body = VarInt(id).Concat(payload).ToArray();
        return VarInt(body.Length).Concat(body).ToArray();
    }

    static byte[] VarInt(int v)
    {
        uint x = unchecked((uint)v); // -1（协议版本未知）按 32 位无符号编码，同 nbt 侧 writeVarInt
        var bytes = new List<byte>(5);
        do
        {
            byte b = (byte)(x & 0x7f);
            x >>= 7;
            if (x != 0) b |= 0x80;
            bytes.Add(b);
        }
        while (x != 0);
        return [.. bytes];
    }

    static async Task<byte[]> ReadPacketAsync(NetworkStream s, CancellationToken ct)
    {
        int len = await ReadVarIntAsync(s, ct);
        if (len <= 0 || len > 2_097_151) throw new InvalidDataException("响应长度异常");
        var body = new byte[len];
        int read = 0;
        while (read < len)
        {
            int n = await s.ReadAsync(body.AsMemory(read, len - read), ct);
            if (n <= 0) break;
            read += n;
        }
        if (read < len) throw new IOException("连接提前关闭");
        return body;
    }

    static async Task<int> ReadVarIntAsync(NetworkStream s, CancellationToken ct)
    {
        int num = 0, shift = 0;
        while (shift <= 28)
        {
            var one = new byte[1];
            int n = await s.ReadAsync(one.AsMemory(), ct);
            if (n != 1) throw new IOException("连接提前关闭");
            num |= (one[0] & 0x7f) << shift;
            if ((one[0] & 0x80) == 0) return num;
            shift += 7;
        }
        throw new InvalidDataException("VarInt 过长");
    }

    static int ReadVarInt(byte[] buf, ref int off)
    {
        int num = 0, shift = 0;
        while (shift <= 28)
        {
            if (off >= buf.Length) throw new InvalidDataException("VarInt 越界");
            byte b = buf[off++];
            num |= (b & 0x7f) << shift;
            if ((b & 0x80) == 0) return num;
            shift += 7;
        }
        throw new InvalidDataException("VarInt 过长");
    }

    // ---------------- 地址解析（serverUtils.parseServerAddress 主干） ----------------

    /// <summary>解析 host[:port] / [IPv6]:port / 裸 IPv6；默认端口 25565。
    /// Display 是写回 servers.dat 的规范化地址（默认端口省略、IPv6 带方括号）；port 越界或格式无效返回 Error。</summary>
    public static (string Host, int Port, string Display, string? Error) ParseAddress(string? raw)
    {
        string s = (raw ?? "").Trim();
        if (s.StartsWith("minecraft://", StringComparison.OrdinalIgnoreCase)) s = s["minecraft://".Length..];
        if (s.Length == 0) return ("", 0, "", "服务器地址不能为空");
        string hostPart;
        string? portPart = null;
        if (s.StartsWith('['))
        {
            int close = s.IndexOf(']');
            if (close < 0) return ("", 0, "", "IPv6 地址缺少右方括号");
            hostPart = s[1..close];
            string rest = s[(close + 1)..];
            if (rest.Length > 0)
            {
                if (!rest.StartsWith(':')) return ("", 0, "", "服务器地址格式无效");
                portPart = rest[1..];
            }
        }
        else
        {
            int colons = s.Count(c => c == ':');
            if (colons == 1)
            {
                int at = s.LastIndexOf(':');
                hostPart = s[..at];
                portPart = s[(at + 1)..];
            }
            else hostPart = s; // 0 个冒号是普通域名；多个冒号按裸 IPv6 整体交给 IPAddress 校验
        }
        hostPart = hostPart.Trim().TrimEnd('.');
        if (hostPart.Length == 0 || hostPart.IndexOfAny([' ', '/', '?', '#', '@']) >= 0)
            return ("", 0, "", "服务器地址格式无效");
        int port = 25565;
        if (!string.IsNullOrEmpty(portPart))
        {
            if (!int.TryParse(portPart, out port) || port is < 1 or > 65535)
                return ("", 0, "", "服务器端口必须是 1 到 65535 的整数");
        }
        bool isV6 = IPAddress.TryParse(hostPart, out var ip) && ip!.AddressFamily == AddressFamily.InterNetworkV6;
        string shown = isV6 ? $"[{hostPart}]" : hostPart;
        string display = port != 25565 ? $"{shown}:{port}" : shown;
        return (hostPart, port, display, null);
    }

    /// <summary>去重/收藏键：始终显式带端口，IPv6 带方括号（normalizedAddress 语义）。</summary>
    public static string KeyOf(string ip)
    {
        var (host, port, _, _) = ParseAddress(ip);
        return $"{(host.Contains(':') ? $"[{host}]" : host)}:{port}";
    }

    // ---------------- 图标 ----------------

    static byte[]? FromDataUri(string? uri)
    {
        if (string.IsNullOrEmpty(uri)) return null;
        int comma = uri.IndexOf(',');
        if (comma < 0) return null;
        try { return Convert.FromBase64String(uri[(comma + 1)..]); }
        catch { return null; }
    }

    /// <summary>PNG 字节 → 冻结的 BitmapImage；坏图返回 null（monogram 回落）。</summary>
    internal static BitmapImage? DecodePng(byte[]? png)
    {
        if (png is not { Length: > 0 }) return null;
        try
        {
            var img = new BitmapImage();
            using var ms = new MemoryStream(png);
            img.BeginInit();
            img.CacheOption = BitmapCacheOption.OnLoad;
            img.StreamSource = ms;
            img.EndInit();
            img.Freeze();
            return img;
        }
        catch { return null; }
    }
}
