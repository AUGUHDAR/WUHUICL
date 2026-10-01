using System.IO;
using System.Net.Http;
using System.Text.Json;

namespace Wuhuicl.Core.Minecraft;

public sealed record MetaVersion(string Id, string Url, string Type, string? ReleaseTime, string? JavaVersion);

/// <summary>版本总表：拉取 + 本地缓存；支持镜像源替换。</summary>
public sealed class ManifestService
{
    public const string OfficialRoot = "https://piston-meta.mojang.com";
    public const string OfficialDataRoot = "https://piston-data.mojang.com";
    public static string Root { get; private set; } = OfficialRoot;
    public static string DataRoot { get; private set; } = OfficialDataRoot;

    /// <summary>镜像只是前缀替换；传 null 恢复官方。</summary>
    public static void UseMirror(string? metaRoot, string? dataRoot = null)
    {
        Root = string.IsNullOrWhiteSpace(metaRoot) ? OfficialRoot : metaRoot.TrimEnd('/');
        DataRoot = string.IsNullOrWhiteSpace(dataRoot) ? Root : dataRoot.TrimEnd('/');
    }

    static readonly HttpClient Http = new(new SocketsHttpHandler { PooledConnectionLifetime = TimeSpan.FromMinutes(5) })
    { Timeout = TimeSpan.FromSeconds(25) };

    public static async Task<List<MetaVersion>> FetchAsync(string cacheFile, bool offline,
                                                           CancellationToken ct = default)
    {
        string? text = null;
        if (File.Exists(cacheFile))
        {
            var age = DateTime.Now - File.GetLastWriteTime(cacheFile);
            if (offline || age < TimeSpan.FromHours(6)) text = await File.ReadAllTextAsync(cacheFile, ct);
        }
        if (text is null)
        {
            if (offline) throw new FileNotFoundException("离线且没有本地清单缓存");
            text = await Http.GetStringAsync($"{Root}/mc/game/version_manifest_v2.json", ct);
            try { Directory.CreateDirectory(Path.GetDirectoryName(cacheFile)!); File.WriteAllText(cacheFile, text); }
            catch { /* 缓存写失败不影响使用 */ }
        }
        var list = new List<MetaVersion>();
        using var doc = JsonDocument.Parse(text);
        foreach (var v in doc.RootElement.GetProperty("versions").EnumerateArray())
        {
            list.Add(new MetaVersion(
                v.GetProperty("id").GetString() ?? "",
                v.GetProperty("url").GetString() ?? "",
                v.GetProperty("type").GetString() ?? "release",
                v.TryGetProperty("releaseTime", out var rt) ? rt.GetString() : null,
                v.TryGetProperty("javaVersion", out var jv) && jv.ValueKind == JsonValueKind.Object
                    ? jv.GetProperty("majorVersion").GetString() : null));
        }
        // URL 换到当前源（镜像）
        return list.Select(Rewrite).ToList();

        static MetaVersion Rewrite(MetaVersion m)
        {
            if (ManifestService.Root == ManifestService.OfficialRoot) return m;
            var url = m.Url.Replace(ManifestService.OfficialRoot, ManifestService.Root);
            return m with { Url = url };
        }
    }

    /// <summary>下载并落盘 versions/&lt;id&gt;/&lt;id&gt;.json；已存在则直接读。</summary>
    public static async Task<VersionInfo> EnsureVersionJsonAsync(Core.Storage.GamePaths paths,
        MetaVersion meta, CancellationToken ct = default)
    {
        var file = paths.VersionJson(meta.Id);
        if (!File.Exists(file))
        {
            var json = await Http.GetStringAsync(meta.Url, ct);
            Directory.CreateDirectory(Path.GetDirectoryName(file)!);
            var tmp = file + ".tmp";
            await File.WriteAllTextAsync(tmp, json, ct);
            File.Move(tmp, file, true);
        }
        return VersionInfo.Parse(await File.ReadAllTextAsync(file, ct));
    }
}

/// <summary>把一个版本装齐：client jar、libraries、assets 索引与对象。</summary>
public sealed class InstallService
{
    readonly Core.Storage.GamePaths Paths;
    public Downloader DL { get; } = new();
    public IProgress<string>? Progress { get; set; }
    /// <summary>国内网络默认先试镜像。</summary>
    public bool UseMirror { get; set; } = true;
    public string MirrorRoot { get; set; } = "https://bmclapi2.bangbang93.com";

    public InstallService(Core.Storage.GamePaths paths) { Paths = paths; }

    string Mirror(string url)
    {
        if (!UseMirror) return url;
        if (url.StartsWith(ManifestService.OfficialDataRoot, StringComparison.Ordinal))
            return MirrorRoot + "/mc/" + url[(ManifestService.OfficialDataRoot.Length + 1)..];
        if (url.StartsWith("https://libraries.minecraft.net/", StringComparison.Ordinal))
            return MirrorRoot + "/maven/" + url["https://libraries.minecraft.net/".Length..];
        if (url.StartsWith("https://resources.download.minecraft.net/", StringComparison.Ordinal))
            return MirrorRoot + "/resources/" + url["https://resources.download.minecraft.net/".Length..];
        return url;
    }

    public async Task<InstallReport> InstallAsync(string id, CancellationToken ct = default)
    {
        Paths.EnsureDirectories();
        var report = new InstallReport(id);
        var resolved = VersionInfo.Resolve(LoadLocal, id);

        var items = new List<DownloadItem>();

        if (resolved.Client is { } client)
            items.Add(new DownloadItem(client.Url, Paths.ClientJar(id), client.Sha1, client.Size,
                                       $"{id} 客户端"));

        foreach (var lib in resolved.Libraries.Where(l => Allowed(l)))
        {
            var baseU = lib.BaseUrl is null ? "https://libraries.minecraft.net/"
                                            : lib.BaseUrl.TrimEnd('/') + "/";
            if (lib.Artifact is { } art)
            {
                var url = art.Url.StartsWith("http", StringComparison.Ordinal) ? art.Url : baseU + lib.MavenPath;
                items.Add(new DownloadItem(url, Paths.SafeUnder(Paths.Libraries, lib.MavenPath),
                                           NullIfEmpty(art.Sha1), art.Size, lib.Name));
            }
            if (lib.Native is { } nat)
            {
                var path = lib.MavenPath.Replace("${arch}", Environment.Is64BitProcess ? "64" : "32");
                items.Add(new DownloadItem(nat.Url.StartsWith("http") ? nat.Url : baseU + path,
                                           Paths.SafeUnder(Paths.Libraries, path),
                                           NullIfEmpty(nat.Sha1), nat.Size, lib.Name + " (natives)"));
            }
        }

        // assets
        string assetName = resolved.Assets;
        string? indexJsonPath = null;
        if (resolved.AssetIndex is { } idx)
        {
            indexJsonPath = Paths.AssetIndexFile(assetName);
            if (!File.Exists(indexJsonPath) || !await Downloader.VerifyAsync(indexJsonPath, idx.Sha1, ct))
            {
                var r = await DL.OneAsync(new DownloadItem(idx.Url, indexJsonPath,
                    NullIfEmpty(idx.Sha1), idx.Size, $"资源索引 {assetName}"), ct);
                if (!r.Ok) report.Failed.Add($"资源索引：{r.Error}");
            }
        }
        else if (assetName != "legacy" && !string.IsNullOrEmpty(assetName) &&
                 File.Exists(Paths.AssetIndexFile(assetName)))
        {
            indexJsonPath = Paths.AssetIndexFile(assetName);
        }

        if (indexJsonPath is not null && File.Exists(indexJsonPath))
            items.AddRange(AssetObjects(indexJsonPath));

        if (items.Count > 0)
        {
            var results = await DL.RunAsync(items, Progress, ct);
            foreach (var bad in results.Where(r => !r.Ok))
                report.Failed.Add($"{bad.Item.Label ?? Path.GetFileName(bad.Item.Dest)}：{bad.Error}");
            report.Downloaded = results.Count(r => !r.Skipped && r.Ok);
            report.Skipped = results.Count(r => r.Skipped);
        }
        report.Ok = report.Failed.Count == 0;
        return report;

        VersionInfo? LoadLocal(string vid)
        {
            var f = Paths.VersionJson(vid);
            return File.Exists(f) ? VersionInfo.Parse(File.ReadAllText(f)) : null;
        }

        IEnumerable<DownloadItem> AssetObjects(string indexPath)
        {
            using var doc = JsonDocument.Parse(File.ReadAllText(indexPath));
            if (!doc.RootElement.TryGetProperty("objects", out var objs) ||
                objs.ValueKind != JsonValueKind.Object) yield break;
            foreach (var p in objs.EnumerateObject())
            {
                var sha1 = p.Value.GetProperty("hash").GetString();
                if (string.IsNullOrEmpty(sha1)) continue;
                var size = p.Value.TryGetProperty("size", out var sz) ? sz.GetInt64() : 0;
                var dest = Paths.AssetObjectPath(sha1);
                yield return new DownloadItem(
                    $"https://resources.download.minecraft.net/{sha1[..2]}/{sha1}",
                    dest, sha1, size, "资源 " + p.Name)
                { };
            }
        }
    }

    static bool Allowed(Library lib) => Rule.Allow(lib.Rules, InstalledFeatures);
    static readonly HashSet<string> InstalledFeatures = new(StringComparer.Ordinal);
    static string? NullIfEmpty(string s) => string.IsNullOrWhiteSpace(s) ? null : s;
}

public sealed class InstallReport
{
    public InstallReport(string id) { Id = id; }
    public string Id { get; }
    public bool Ok { get; set; }
    public int Downloaded { get; set; }
    public int Skipped { get; set; }
    public List<string> Failed { get; } = new();
    public string Summary => Ok ? $"已就绪（新下载 {Downloaded}，已存在 {Skipped}）"
                                : $"{Failed.Count} 个文件失败：{string.Join("；", Failed.Take(3))}";
}
