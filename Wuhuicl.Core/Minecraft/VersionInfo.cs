using System.Text.Json;

namespace Wuhuicl.Core.Minecraft;

public sealed record DownloadSlot(string Url, string Sha1, long Size);

/// <summary>清单里的一条 os/features 规则。</summary>
public sealed class Rule
{
    public bool AllowAction { get; set; } = true;
    public string? OsName { get; set; }
    public string? OsArch { get; set; }
    public string? OsVersion { get; set; }
    public List<(string Name, bool Value)> Features { get; } = new();

    /// <summary>features 由调用方给出（is_demo_user / has_custom_resolution / is_quickPlayMultiplayer ...）。</summary>
    public bool Matches(HashSet<string> enabledFeatures)
    {
        if (OsName is not null && OsName != "windows") return false;
        if (OsArch is not null && OsArch != (Environment.Is64BitProcess ? "x64" : "x86")) return false;
        foreach (var (name, want) in Features)
            if (enabledFeatures.Contains(name) != want) return false;
        return true;
    }

    public static List<Rule> Parse(JsonElement? arr)
    {
        var list = new List<Rule>();
        if (arr is null || arr.Value.ValueKind != JsonValueKind.Array) return list;
        foreach (var el in arr.Value.EnumerateArray())
        {
            var r = new Rule { AllowAction = el.GetString("action") != "disallow" };
            if (el.TryGetProperty("os", out var os) && os.ValueKind == JsonValueKind.Object)
            {
                r.OsName = os.GetString("name");
                r.OsArch = os.GetString("arch");
                r.OsVersion = os.GetString("version");
            }
            if (el.TryGetProperty("features", out var f) && f.ValueKind == JsonValueKind.Object)
                foreach (var p in f.EnumerateObject())
                    r.Features.Add((p.Name, p.Value.ValueKind == JsonValueKind.True));
            list.Add(r);
        }
        return list;
    }

    /// <summary>无规则=允许；否则按 Mojang 语义：默认拒绝，命中 allow 才允许，命中 deny 则拒绝。</summary>
    public static bool Allow(List<Rule>? rules, HashSet<string> features)
    {
        if (rules is null || rules.Count == 0) return true;
        bool allow = false;
        foreach (var r in rules)
        {
            if (!r.Matches(features)) continue;
            if (!r.AllowAction) return false;
            allow = true;
        }
        return allow;
    }
}

public sealed class Library
{
    public string Name { get; set; } = "";                 // com.google.guava:guava:21.0
    public string? BaseUrl { get; set; }
    public DownloadSlot? Artifact { get; set; }
    public string? NativeClassifier { get; set; }          // 本机需要的 natives-windows 等
    public DownloadSlot? Native { get; set; }
    public bool Extract { get; set; } = true;
    public List<string> Exclude { get; } = new();
    public List<Rule> Rules { get; set; } = new();

    public string MavenPath => Core.Storage.GamePaths.MavenToPath(Name);
    public string? Classifier => Core.Storage.GamePaths.MavenClassifier(Name);
    /// <summary>去重键：同一 group:artifact[:classifier] 只能有一份在 classpath 上。</summary>
    public string DedupeKey
    {
        get
        {
            var p = Name.Split(':');
            if (p.Length < 3) return Name;
            var cls = Core.Storage.GamePaths.MavenClassifier(Name);
            return cls is null ? $"{p[0]}:{p[1]}" : $"{p[0]}:{p[1]}:{cls}";
        }
    }

    public static List<Library> Parse(JsonElement? arr)
    {
        var list = new List<Library>();
        if (arr is null || arr.Value.ValueKind != JsonValueKind.Array) return list;
        foreach (var el in arr.Value.EnumerateArray())
        {
            var name = el.GetString("name");
            if (string.IsNullOrEmpty(name)) continue;      // 老清单里的空条目
            var lib = new Library
            {
                Name = name,
                BaseUrl = el.GetString("url"),
                Rules = Rule.Parse(el.TryGetProperty("rules", out var rr) ? rr : null),
            };
            if (el.TryGetProperty("extract", out var ex) && ex.ValueKind == JsonValueKind.Object)
            {
                lib.Extract = !(ex.TryGetProperty("exclude", out var e0) && e0.ValueKind == JsonValueKind.Array
                                && e0.GetArrayLength() == 0);
                if (ex.TryGetProperty("exclude", out var exs) && exs.ValueKind == JsonValueKind.Array)
                    foreach (var s in exs.EnumerateArray())
                        if (s.GetString() is { } s2) lib.Exclude.Add(s2);
            }
            if (el.TryGetProperty("downloads", out var d) && d.ValueKind == JsonValueKind.Object)
            {
                lib.Artifact = Slot(d, "artifact");
                if (d.TryGetProperty("classifiers", out var cl) && cl.ValueKind == JsonValueKind.Object)
                {
                    var key = "natives-windows";
                    if (cl.TryGetProperty(key, out var n)) lib.Native = Slot(n, null);
                }
                if (d.TryGetProperty("nativePath", out var np) && np.ValueKind == JsonValueKind.String)
                    lib.NativeClassifier = np.GetString();
                foreach (var p in d.EnumerateObject())
                    if (p.Name.StartsWith("natives", StringComparison.Ordinal))
                        lib.NativeClassifier ??= "windows";
            }
            list.Add(lib);
        }
        return list;
    }

    static DownloadSlot? Slot(JsonElement owner, string? field)
    {
        JsonElement el = field is null ? owner : (owner.TryGetProperty(field, out var v) ? v : default);
        if (el.ValueKind != JsonValueKind.Object) return null;
        var url = el.GetString("url") ?? "";
        if (string.IsNullOrEmpty(url)) return null;
        long.TryParse(el.GetString("size"), out var size);
        return new DownloadSlot(url, el.GetString("sha1") ?? "", size);
    }
}

/// <summary>新版 arguments 与旧版 minecraftArguments 的统一表示。</summary>
public sealed class ArgList
{
    public List<(string Value, List<Rule> Rules)> Game { get; } = new();
    public List<(string Value, List<Rule> Rules)> Jvm { get; } = new();

    public static ArgList Parse(JsonElement root)
    {
        var a = new ArgList();
        if (root.TryGetProperty("arguments", out var args) && args.ValueKind == JsonValueKind.Object)
        {
            Read(args, "game", a.Game);
            Read(args, "jvm", a.Jvm);
        }
        else if (root.TryGetProperty("minecraftArguments", out var legacy) &&
                 legacy.ValueKind == JsonValueKind.String)
        {
            foreach (var tok in SplitArgs(legacy.GetString() ?? ""))
                a.Game.Add((tok, new List<Rule>()));
        }
        return a;
    }

    static void Read(JsonElement args, string key, List<(string, List<Rule>)> into)
    {
        if (!args.TryGetProperty(key, out var arr) || arr.ValueKind != JsonValueKind.Array) return;
        foreach (var item in arr.EnumerateArray())
        {
            if (item.ValueKind == JsonValueKind.String)
            {
                var s = item.GetString();
                if (!string.IsNullOrEmpty(s)) into.Add((s, new List<Rule>()));
                continue;
            }
            if (item.ValueKind != JsonValueKind.Object) continue;
            var rules = Rule.Parse(item.TryGetProperty("rules", out var r) ? r : null);
            if (item.TryGetProperty("value", out var v))
            {
                if (v.ValueKind == JsonValueKind.String) into.Add((v.GetString()!, rules));
                else if (v.ValueKind == JsonValueKind.Array)
                    foreach (var s in v.EnumerateArray())
                        if (s.GetString() is { } s2) into.Add((s2, rules));
            }
        }
    }

    public static IEnumerable<string> SplitArgs(string s) =>
        System.Text.RegularExpressions.Regex.Matches(s, "\"[^\"]*\"|[^ ]+")
            .Select(m => m.Value.Trim('"')).ToList();
}

public sealed class VersionInfo
{
    public string Id { get; set; } = "";
    public string? InheritsFrom { get; set; }
    public string? MainClass { get; set; }
    public string Assets { get; set; } = "legacy";
    public DownloadSlot? AssetIndex { get; set; }
    public string Type { get; set; } = "release";
    public string? ReleaseTime { get; set; }
    public int JavaMajor { get; set; }
    public List<Library> Libraries { get; set; } = new();
    public Dictionary<string, DownloadSlot> Downloads { get; set; } = new();
    public ArgList Args { get; set; } = new();
    public JsonElement Raw { get; set; }
    /// <summary>由 loader 注入的额外 javaagent 之类（fabric server-mapper 等）。</summary>
    public string? LoaderKind { get; set; }

    public static VersionInfo Parse(string json)
    {
        using var doc = JsonDocument.Parse(json);
        var root = doc.RootElement.Clone();
        var v = new VersionInfo
        {
            Id = root.GetString("id") ?? "",
            InheritsFrom = root.GetString("inheritsFrom"),
            MainClass = root.GetString("mainClass"),
            Assets = root.GetString("assets") ?? "legacy",
            Type = root.GetString("type") ?? "release",
            ReleaseTime = root.GetString("releaseTime"),
            Libraries = Library.Parse(root.TryGetProperty("libraries", out var l) ? l : null),
            Args = ArgList.Parse(root),
            Raw = root,
        };
        if (root.TryGetProperty("javaVersion", out var jv) && jv.ValueKind == JsonValueKind.Object)
        {
            var s = jv.GetString("majorVersion");
            if (string.IsNullOrEmpty(s)) s = jv.GetString("component");
            if (s is not null && s.AsSpan().IndexOfAny("0123456789") >= 0)
            {
                var digits = new string(s.Where(char.IsDigit).ToArray());
                int.TryParse(digits, out var major);
                v.JavaMajor = major;
            }
        }
        if (root.TryGetProperty("assetIndex", out var ai) && ai.ValueKind == JsonValueKind.Object)
        {
            var url = ai.GetString("url") ?? "";
            long.TryParse(ai.GetString("totalSize"), out var size);
            if (!string.IsNullOrEmpty(url))
                v.AssetIndex = new DownloadSlot(url, ai.GetString("sha1") ?? "", size);
        }
        if (root.TryGetProperty("downloads", out var d) && d.ValueKind == JsonValueKind.Object)
            foreach (var p in d.EnumerateObject())
                if (p.Value.ValueKind == JsonValueKind.Object)
                {
                    var url = p.Value.GetString("url");
                    if (string.IsNullOrEmpty(url)) continue;
                    long.TryParse(p.Value.GetString("size"), out var size);
                    v.Downloads[p.Name] = new DownloadSlot(url, p.Value.GetString("sha1") ?? "", size);
                }
        return v;
    }

    /// <summary>
    /// 沿 inheritsFrom 链合并：子覆盖父。库按 Maven 去重键合并（子优先），
    /// 参数按"父的在前、子的在后"拼接——这是 KAMUCL 用 flatMap 合链踩过的坑（RUN-3）。
    /// </summary>
    public static ResolvedVersion Resolve(Func<string, VersionInfo?> load, string id, int maxDepth = 16)
    {
        var chain = new List<VersionInfo>();
        var cursor = id;
        var guard = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        for (int i = 0; i < maxDepth && cursor is not null; i++)
        {
            if (!guard.Add(cursor))
                throw new InvalidOperationException($"版本继承链出现环：{cursor}");
            var info = load(cursor) ?? throw new FileNotFoundException($"缺少版本文件 {cursor}");
            chain.Add(info);
            cursor = info.InheritsFrom;
        }
        // chain[0] 是叶子（最具体），最后一个是官方基版本
        var leaf = chain[0];
        var libs = new List<Library>();
        var seen = new HashSet<string>(StringComparer.Ordinal);
        foreach (var v in chain)                       // 叶子优先
            foreach (var l in v.Libraries)
                if (seen.Add(l.DedupeKey)) libs.Add(l);

        var args = new ArgList();
        foreach (var v in chain)                       // 基版本在前，符合 Mojang 语义
            foreach (var a in v.Args.Jvm) args.Jvm.Add(a);
        foreach (var v in chain.AsEnumerable().Reverse())
            foreach (var a in v.Args.Game) args.Game.Add(a);

        var downloads = new Dictionary<string, DownloadSlot>(StringComparer.Ordinal);
        foreach (var v in chain.AsEnumerable().Reverse())
            foreach (var p in v.Downloads) downloads[p.Key] = p.Value;

        return new ResolvedVersion(id, leaf, chain, libs, args, downloads);
    }
}

public sealed record ResolvedVersion(
    string Id, VersionInfo Leaf, IReadOnlyList<VersionInfo> Chain,
    IReadOnlyList<Library> Libraries, ArgList Args,
    IReadOnlyDictionary<string, DownloadSlot> Downloads)
{
    public string MainClass => Leaf.MainClass ?? Chain.First(c => c.MainClass is not null).MainClass!;
    public string Assets => Leaf.Assets;
    public DownloadSlot? AssetIndex => Leaf.AssetIndex ?? Chain.FirstOrDefault(c => c.AssetIndex is not null)?.AssetIndex;
    public int JavaMajor => Chain.First(c => c.JavaMajor > 0).JavaMajor;
    public DownloadSlot? Client => Downloads.TryGetValue("client", out var c) ? c : null;
    public bool NeedsNatives => Libraries.Any(l => l.Native is not null);
}

static file class JsonExt
{
    public static string? GetString(this JsonElement el, string name) =>
        el.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.String ? v.GetString() : null;
}
