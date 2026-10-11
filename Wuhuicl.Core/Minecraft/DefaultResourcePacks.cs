using System.IO.Compression;
using System.Security.Cryptography;
using System.Text.Json;

namespace Wuhuicl.Core.Minecraft;

/// <summary>
/// 默认材质包：启动器级清单 + 启动时复制/选择性写入实例。
/// 对照 KAMUCL-1.1.10/src/main/core/defaultResourcePacks.ts 逐项翻译：
/// packs.json 清单（sha256 id / basename 安全名 / 启停 / 顺序即优先级），
/// 导入校验 ZIP 里必须有 pack.mcmeta（≤1MB）且能解析出 pack 对象；
/// 同步按客户端 version.json 的 pack_format 判兼容，不兼容的选择进 incompatibleResourcePacks；
/// 管理名单记在实例 .kamucl-default-resourcepacks.json，停用的包也会被持续摘除。
/// </summary>
public static class DefaultResourcePacks
{
    public sealed record Pack(string Id, string Name, long Size, bool Enabled = true);

    static string Root(Storage.GamePaths p) => Path.Combine(p.DataDir, "default-resourcepacks");
    static string ManifestFile(Storage.GamePaths p) => Path.Combine(Root(p), "packs.json");

    static List<Pack> Save(Storage.GamePaths paths, List<Pack> packs)
    {
        Directory.CreateDirectory(Root(paths));
        Storage.AtomicJson.Save(ManifestFile(paths), packs);
        return packs;
    }

    /// <summary>读取清单：id 必须 64 位 hex、name 必须是纯文件名、zip 必须还在盘上（:18-23）。</summary>
    public static List<Pack> Get(Storage.GamePaths paths)
    {
        try
        {
            var packs = Storage.AtomicJson.Load<List<Pack>>(ManifestFile(paths), []);
            return packs.Where(p =>
                p.Id.Length == 64 && p.Id.All(c => Uri.IsHexDigit(c)) &&
                !string.IsNullOrEmpty(p.Name) && p.Name == Path.GetFileName(p.Name) &&
                File.Exists(Path.Combine(Root(paths), p.Id + ".zip")))
                .Select(p => p with { Enabled = p.Enabled })
                .ToList();
        }
        catch { return []; }
    }

    /// <summary>导入一批 ZIP 材质包：先整批校验，任一错误都不改配置（:24-44）。</summary>
    public static List<Pack> Import(Storage.GamePaths paths, IEnumerable<string> files)
    {
        var list = files.ToList();
        if (list.Count == 0) return Get(paths);
        var incoming = new List<(byte[] Data, Pack Pack)>();
        foreach (var file in list)
        {
            if (Path.GetExtension(file).ToLowerInvariant() != ".zip" || !File.Exists(file))
                throw new Exception("请选择 ZIP 格式材质包");
            var data = File.ReadAllBytes(file);
            string metaText;
            using (var ms = new MemoryStream(data))
            using (var zip = new ZipArchive(ms))
            {
                var entry = zip.GetEntry("pack.mcmeta") ?? throw new Exception($"{Path.GetFileName(file)} 缺少有效的 pack.mcmeta");
                if (entry.Length > 1024 * 1024) throw new Exception($"{Path.GetFileName(file)} 缺少有效的 pack.mcmeta");
                using var s = entry.Open();
                using var r = new StreamReader(s);
                metaText = r.ReadToEnd().TrimStart('\uFEFF');
            }
            JsonDocument doc;
            try { doc = JsonDocument.Parse(metaText); }
            catch { throw new Exception($"{Path.GetFileName(file)} 的 pack.mcmeta 无法读取"); }
            using (doc)
            {
                if (!doc.RootElement.TryGetProperty("pack", out var pk) || pk.ValueKind != JsonValueKind.Object)
                    throw new Exception($"{Path.GetFileName(file)} 不是有效材质包");
            }
            var id = Convert.ToHexString(SHA256.HashData(data)).ToLowerInvariant();
            var name = new string(Path.GetFileName(file).Select(c => "<>:\"/\\|?*".Contains(c) ? '_' : c).ToArray());
            incoming.Add((data, new Pack(id, name, data.Length, true)));
        }
        var packs = Get(paths);
        Directory.CreateDirectory(Root(paths));
        foreach (var (data, pack) in incoming)
        {
            if (packs.Any(p => p.Id == pack.Id)) continue;
            File.WriteAllBytes(Path.Combine(Root(paths), pack.Id + ".zip"), data);
            packs.Add(pack);
        }
        return Save(paths, packs);
    }

    /// <summary>仅从默认配置移除；已复制到实例和原始文件均保留（:45-48）。</summary>
    public static List<Pack> Remove(Storage.GamePaths paths, string id)
        => Save(paths, Get(paths).Where(p => p.Id != id).ToList());

    public static List<Pack> SetEnabled(Storage.GamePaths paths, string id, bool enabled)
    {
        var packs = Get(paths);
        var pack = packs.FirstOrDefault(p => p.Id == id) ?? throw new Exception("材质包不存在，请刷新后重试");
        var idx = packs.IndexOf(pack);
        packs[idx] = pack with { Enabled = enabled };
        return Save(paths, packs);
    }

    public static List<Pack> Move(Storage.GamePaths paths, string id, int direction)
    {
        var packs = Get(paths);
        int i = packs.FindIndex(p => p.Id == id), j = i + (direction < 0 ? -1 : 1);
        if (i >= 0 && j >= 0 && j < packs.Count) (packs[i], packs[j]) = (packs[j], packs[i]);
        return Save(paths, packs);
    }

    // ---------------- options.txt 合并（:62-76）----------------

    public static string MergeResourcePackOptions(string text, List<string> names, List<string> previous, List<string>? incompatible = null)
    {
        incompatible ??= [];
        string[] keys = ["resourcePacks", "incompatibleResourcePacks"];
        var seen = new HashSet<string>();
        var lines = text.Split(["\r\n", "\n"], StringSplitOptions.RemoveEmptyEntries).ToList();
        for (int i = 0; i < lines.Count; i++)
        {
            int c = lines[i].IndexOf(':');
            if (c <= 0) continue;
            var key = lines[i][..c];
            if (!keys.Contains(key)) continue;
            seen.Add(key);
            List<string> existing;
            try
            {
                var parsed = JsonSerializer.Deserialize<JsonElement>(lines[i][(c + 1)..]);
                if (parsed.ValueKind != JsonValueKind.Array) throw new Exception();
                existing = parsed.EnumerateArray().Select(x => x.GetString() ?? "").ToList();
            }
            catch { throw new Exception($"options.txt 中的 {key} 格式无效，未覆盖原配置"); }
            var selected = key == "resourcePacks" ? names : incompatible;
            var merged = existing
                .Where(x => !previous.Contains(x) && !names.Contains(x))
                .Concat(selected)
                .Distinct()
                .ToList();
            lines[i] = key + ":" + JsonSerializer.Serialize(merged);
        }
        foreach (var key in keys)
            if (!seen.Contains(key))
                lines.Add(key + ":" + JsonSerializer.Serialize(key == "resourcePacks" ? new List<string> { "vanilla" }.Concat(names).ToList() : incompatible));
        return string.Join('\n', lines) + "\n";
    }

    // ---------------- pack_format 兼容判定（:78-106）----------------

    static (int, long)? Format(JsonElement v, bool upper = false)
    {
        JsonElement[] parts = v.ValueKind == JsonValueKind.Array
            ? v.EnumerateArray().ToArray()
            : [v];
        if (parts.Length is < 1 or > 2) return null;
        foreach (var p in parts)
        {
            if (p.ValueKind != JsonValueKind.Number) return null;
            var d = p.GetDecimal();
            if (d != Math.Floor(d) || d < 0) return null;
        }
        long minor = parts.Length == 2 ? parts[1].GetInt64() : (upper ? long.MaxValue : 0);
        return (parts[0].GetInt32(), minor);
    }

    static int CompareFormat((int Major, long Minor) a, (int Major, long Minor) b) => a.Major != b.Major ? a.Major - b.Major : a.Minor.CompareTo(b.Minor);

    /// <summary>读客户端 version.json 的 resource pack_format（:87-93）。</summary>
    public static (int, long)? ReadClientResourceFormat(string? clientJar)
    {
        if (string.IsNullOrEmpty(clientJar) || !File.Exists(clientJar)) return null;
        try
        {
            using var zip = new ZipArchive(File.OpenRead(clientJar));
            var entry = zip.GetEntry("version.json");
            if (entry is null) return null;
            using var s = entry.Open();
            using var doc = JsonDocument.Parse(s);
            if (!doc.RootElement.TryGetProperty("pack_version", out var pv)) return null;
            if (pv.TryGetProperty("resource_major", out var major))
            {
                long minor = pv.TryGetProperty("resource_minor", out var mi) ? mi.GetInt64() : 0;
                return (major.GetInt32(), minor);
            }
            if (pv.TryGetProperty("resource", out var res)) return Format(res);
            return null;
        }
        catch { return null; }
    }

    public static bool Incompatible(JsonElement pack, (int, long)? target)
    {
        if (target is null) return false;   // 没证据不发明不兼容（:95）
        var t = target.Value;
        ((int, long)? min, (int, long)? max) ranges;
        if (t.Item1 >= 65 && pack.TryGetProperty("min_format", out var mf) && pack.TryGetProperty("max_format", out var xf))
        {
            ranges = (Format(mf), Format(xf, upper: true));
        }
        else
        {
            JsonElement? supported = pack.TryGetProperty("supported_formats", out var sf) && t.Item1 >= 18 ? sf : (JsonElement?)null;
            JsonElement? low, high;
            if (supported is { ValueKind: JsonValueKind.Array } arr)
            {
                var a = arr.EnumerateArray().ToArray();
                low = a.Length > 0 ? a[0] : null;
                high = a.Length > 1 ? a[1] : null;
            }
            else if (supported is { ValueKind: JsonValueKind.Object } obj)
            {
                low = obj.TryGetProperty("min_inclusive", out var lo) ? lo : (JsonElement?)null;
                high = obj.TryGetProperty("max_inclusive", out var hi) ? hi : (JsonElement?)null;
            }
            else
            {
                low = supported;
                high = supported;
            }
            if (low is null && pack.TryGetProperty("pack_format", out var pf)) low = pf;
            if (high is null && pack.TryGetProperty("pack_format", out var pf2)) high = pf2;
            ranges = (low is null ? null : Format(low.Value), high is null ? null : Format(high.Value, upper: true));
        }
        return ranges.min is { } mn && ranges.max is { } mx
            && (CompareFormat(t, mn) < 0 || CompareFormat(t, mx) > 0);
    }

    /// <summary>启动时同步（:108-134）：复制 zip 进实例 resourcepacks（EXCL）+ 合并 options.txt 选择 + 记管理名单。</summary>
    public static int SyncToGameDir(Storage.GamePaths paths, string gameDir, string mcVersion, string? clientJar)
    {
        if (!DefaultKeys.McVersionAtLeast(mcVersion, "1.6")) return 0;
        var allPacks = Get(paths);
        var packs = allPacks.Where(p => p.Enabled).ToList();
        var stateFile = Path.Combine(gameDir, ".kamucl-default-resourcepacks.json");
        var previous = new List<string>();
        try
        {
            var raw = JsonSerializer.Deserialize<JsonElement>(File.ReadAllText(stateFile));
            if (raw.ValueKind == JsonValueKind.Array)
                previous = raw.EnumerateArray().Where(x => x.ValueKind == JsonValueKind.String).Select(x => x.GetString()!).ToList();
        }
        catch { /* 首次同步 */ }
        if (allPacks.Count == 0 && previous.Count == 0) return 0;
        string Prefix() => DefaultKeys.McVersionAtLeast(mcVersion, "1.13") ? "file/" : "";
        string ManagedName(Pack p) => $"WUHUICL-default-{p.Id}-{p.Name}";   // 物理文件名，不带 file/ 前缀
        string SelectedName(Pack p) => Prefix() + ManagedName(p);            // options.txt 里的选择串
        // 停用的默认包也是受管：反复启动仍会摘除其选择（:114-115）
        var managed = allPacks.Select(SelectedName).ToList();
        var names = packs.Select(SelectedName).ToList();
        var targetFormat = ReadClientResourceFormat(clientJar);
        var incompatible = new List<string>();
        for (int i = 0; i < packs.Count; i++)
        {
            string meta;
            using (var zip = new ZipArchive(File.OpenRead(Path.Combine(Root(paths), packs[i].Id + ".zip"))))
            {
                var entry = zip.GetEntry("pack.mcmeta");
                if (entry is null) continue;
                using var s = entry.Open();
                using var r = new StreamReader(s);
                meta = r.ReadToEnd().TrimStart('\uFEFF');
            }
            using var doc = JsonDocument.Parse(meta);
            if (doc.RootElement.TryGetProperty("pack", out var pk) && Incompatible(pk, targetFormat))
                incompatible.Add(names[i]);
        }
        var options = Path.Combine(gameDir, "options.txt");
        var before = File.Exists(options) ? File.ReadAllText(options) : "";
        var after = MergeResourcePackOptions(before, names, previous.Union(managed).ToList(), incompatible);
        var target = Path.Combine(gameDir, "resourcepacks");
        Directory.CreateDirectory(target);
        foreach (var p in packs)
        {
            var dest = Path.Combine(target, ManagedName(p));
            var source = Path.Combine(Root(paths), p.Id + ".zip");
            if (File.Exists(dest))
            {
                if (Convert.ToHexString(SHA256.HashData(File.ReadAllBytes(dest))).ToLowerInvariant() != p.Id)
                    throw new Exception($"默认材质包副本被修改，未覆盖：{p.Name}");
            }
            else if (!File.Exists(dest)) File.Copy(source, dest);
        }
        if (after != before)
        {
            if (before.Length > 0) File.WriteAllText(options + ".before-default-packs", before);
            File.WriteAllText(options, after);
        }
        File.WriteAllText(stateFile, JsonSerializer.Serialize(names));
        return packs.Count;
    }
}
