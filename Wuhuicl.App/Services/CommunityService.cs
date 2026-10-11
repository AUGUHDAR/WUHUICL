using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Net.Http;
using System.Security.Cryptography;
using System.Text.Json;
using System.Threading.Tasks;

namespace Wuhuicl.App.Services;

public record CommunityResult(string Source, string ProjectId, string Slug, string Title,
    string Author, string Description, string IconUrl, long Downloads, string UpdatedAt);

public record CommunityFile(string FileId, string FileName, string Version, string Url,
    string Sha1, long Size, string ReleaseType, List<string> GameVersions, List<string> Loaders, string Date);

public sealed class CommunityService
{
    static readonly HttpClient Http = new() { Timeout = TimeSpan.FromSeconds(30) };
    static readonly string[] ModrinthHosts = { "https://api.modrinth.com/v2", "https://mod.mcimirror.top/modrinth/v2" };

    public record SearchOptions(string Kind, string Query, int Offset, string? GameVersion, string? Loader);

    static string ModrinthProjectType(string kind) => kind switch
    {
        "shader" => "shader",
        "resourcepack" => "resourcepack",
        "datapack" => "datapack",
        "modpack" => "modpack",
        _ => "mod",
    };

    static int CfClassId(string kind) => kind switch
    {
        "resourcepack" => 12,
        "shader" => 6552,
        "datapack" => 6945,
        "modpack" => 4471,
        _ => 6,
    };

    /// <summary>Modrinth 搜索。</summary>
    public static async Task<List<CommunityResult>> SearchModrinthAsync(SearchOptions o)
    {
        var facets = JsonSerializer.Serialize(BuildFacets(o));
        var url = $"{ModrinthHosts[0]}/search?query={Uri.EscapeDataString(o.Query)}&limit=20&offset={o.Offset}&index=relevance&facets={Uri.EscapeDataString(facets)}";
        using var resp = await Http.GetAsync(url);
        resp.EnsureSuccessStatusCode();
        using var doc = JsonDocument.Parse(await resp.Content.ReadAsStringAsync());
        return doc.RootElement.GetProperty("hits").EnumerateArray().Select(h => new CommunityResult(
            "modrinth",
            h.GetProperty("project_id").GetString() ?? "",
            h.GetProperty("slug").GetString() ?? "",
            h.GetProperty("title").GetString() ?? "",
            h.TryGetProperty("author", out var a) ? a.GetString() ?? "" : "",
            h.TryGetProperty("description", out var d) ? d.GetString() ?? "" : "",
            h.TryGetProperty("icon_url", out var i) ? i.GetString() ?? "" : "",
            h.TryGetProperty("downloads", out var dl) ? dl.GetInt64() : 0,
            h.TryGetProperty("date_modified", out var u) ? u.GetString() ?? "" : ""
        )).ToList();
    }

    static List<List<string>> BuildFacets(SearchOptions o)
    {
        var facets = new List<List<string>>
        {
            new() { $"project_type:{ModrinthProjectType(o.Kind)}" },
        };
        if (!string.IsNullOrEmpty(o.GameVersion)) facets.Add(new() { $"versions:{o.GameVersion}" });
        if (!string.IsNullOrEmpty(o.Loader)) facets.Add(new() { $"categories:{o.Loader}" });
        return facets;
    }

    /// <summary>CurseForge 搜索（镜像免 key）。</summary>
    public static async Task<List<CommunityResult>> SearchCurseForgeAsync(SearchOptions o)
    {
        var classId = CfClassId(o.Kind);
        var url = $"https://mod.mcimirror.top/curseforge/v1/mods/search?gameId=432&classId={classId}&searchFilter={Uri.EscapeDataString(o.Query)}&index={o.Offset}&pageSize=20&sortField=2&sortOrder=desc";
        if (!string.IsNullOrEmpty(o.GameVersion)) url += $"&gameVersion={o.GameVersion}";
        using var resp = await Http.GetAsync(url);
        resp.EnsureSuccessStatusCode();
        using var doc = JsonDocument.Parse(await resp.Content.ReadAsStringAsync());
        var data = doc.RootElement.GetProperty("data");
        return data.EnumerateArray().Select(h => new CommunityResult(
            "curseforge",
            h.GetProperty("id").GetInt64().ToString(),
            h.GetProperty("slug").GetString() ?? "",
            h.GetProperty("name").GetString() ?? "",
            h.TryGetProperty("authors", out var au) && au.GetArrayLength() > 0 ? au[0].GetProperty("name").GetString() ?? "" : "",
            h.TryGetProperty("summary", out var de) ? de.GetString() ?? "" : "",
            h.TryGetProperty("logo", out var lo) && lo.TryGetProperty("thumbnailUrl", out var tu) ? tu.GetString() ?? "" : "",
            h.TryGetProperty("downloadCount", out var dl) ? dl.GetInt64() : 0,
            h.TryGetProperty("dateModified", out var u) ? u.GetString() ?? "" : ""
        )).ToList();
    }

    /// <summary>双源搜索，交错合并。</summary>
    public static async Task<List<CommunityResult>> SearchBothAsync(SearchOptions o)
    {
        var tasks = new List<Task<List<CommunityResult>>>();
        try { tasks.Add(SearchModrinthAsync(o)); } catch { }
        try { tasks.Add(SearchCurseForgeAsync(o)); } catch { }
        var results = await Task.WhenAll(tasks);
        var merged = new List<CommunityResult>();
        int max = results.Length > 0 ? results.Max(r => r.Count) : 0;
        for (int i = 0; i < max; i++)
            foreach (var r in results)
                if (i < r.Count) merged.Add(r[i]);
        return merged;
    }

    /// <summary>获取版本文件列表（Modrinth）。</summary>
    public static async Task<List<CommunityFile>> GetModrinthVersionsAsync(string projectId, string? gameVersion, string? loader)
    {
        var url = $"{ModrinthHosts[0]}/project/{projectId}/version";
        var q = new List<string>();
        if (!string.IsNullOrEmpty(gameVersion)) q.Add($"game_versions=[\"{gameVersion}\"]");
        if (!string.IsNullOrEmpty(loader)) q.Add($"loaders=[\"{loader}\"]");
        if (q.Count > 0) url += "?" + string.Join("&", q);
        using var resp = await Http.GetAsync(url);
        resp.EnsureSuccessStatusCode();
        using var doc = JsonDocument.Parse(await resp.Content.ReadAsStringAsync());
        return doc.RootElement.EnumerateArray().Select(v =>
        {
            var file = v.GetProperty("files").EnumerateArray().FirstOrDefault(f => f.GetProperty("primary").GetBoolean());
            if (file.ValueKind == JsonValueKind.Undefined && v.GetProperty("files").GetArrayLength() > 0)
                file = v.GetProperty("files")[0];
            return new CommunityFile(
                v.GetProperty("id").GetString() ?? "",
                file.ValueKind != JsonValueKind.Undefined ? file.GetProperty("filename").GetString() ?? "" : "",
                v.TryGetProperty("version_number", out var vn) ? vn.GetString() ?? "" : "",
                file.ValueKind != JsonValueKind.Undefined ? file.GetProperty("url").GetString() ?? "" : "",
                file.ValueKind != JsonValueKind.Undefined && file.TryGetProperty("hashes", out var h) && h.TryGetProperty("sha1", out var s1) ? s1.GetString() ?? "" : "",
                file.ValueKind != JsonValueKind.Undefined && file.TryGetProperty("size", out var sz) ? sz.GetInt64() : 0,
                v.TryGetProperty("release_type", out var rt) ? rt.GetString() ?? "" : "",
                v.TryGetProperty("game_versions", out var gv) ? gv.EnumerateArray().Select(x => x.GetString() ?? "").ToList() : new(),
                v.TryGetProperty("loaders", out var ld) ? ld.EnumerateArray().Select(x => x.GetString() ?? "").ToList() : new(),
                "");
        }).ToList();
    }

    /// <summary>获取 CurseForge 文件列表。</summary>
    public static async Task<List<CommunityFile>> GetCurseForgeFilesAsync(string projectId)
    {
        var url = $"https://mod.mcimirror.top/curseforge/v1/mods/{projectId}/files?pageSize=20";
        using var resp = await Http.GetAsync(url);
        resp.EnsureSuccessStatusCode();
        using var doc = JsonDocument.Parse(await resp.Content.ReadAsStringAsync());
        return doc.RootElement.GetProperty("data").EnumerateArray().Select(f => new CommunityFile(
            f.GetProperty("id").GetInt64().ToString(),
            f.GetProperty("fileName").GetString() ?? "",
            f.TryGetProperty("displayName", out var dn) ? dn.GetString() ?? "" : "",
            f.TryGetProperty("downloadUrl", out var du) && du.ValueKind != JsonValueKind.Null ? du.GetString() ?? "" : "",
            f.TryGetProperty("hashes", out var hs) && hs.GetArrayLength() > 0 ? hs[0].GetProperty("value").GetString() ?? "" : "",
            f.TryGetProperty("fileLength", out var fl) ? fl.GetInt64() : 0,
            f.TryGetProperty("releaseType", out var rt) ? rt.GetInt32() switch { 1 => "release", 2 => "beta", _ => "alpha" } : "",
            f.TryGetProperty("gameVersions", out var gv) ? gv.EnumerateArray().Select(x => x.GetString() ?? "").ToList() : new(),
            new(),
            f.TryGetProperty("fileDate", out var fd) ? fd.GetString() ?? "" : ""
        )).ToList();
    }

    /// <summary>下载文件到目标目录并校验 SHA1。</summary>
    public static async Task<string> DownloadFileAsync(string url, string destDir, string fileName, string? sha1)
    {
        Directory.CreateDirectory(destDir);
        var dest = Path.Combine(destDir, fileName);
        using var resp = await Http.GetAsync(url, HttpCompletionOption.ResponseHeadersRead);
        resp.EnsureSuccessStatusCode();
        using var fs = File.Create(dest + ".part");
        await resp.Content.CopyToAsync(fs);
        fs.Flush();
        fs.Close();

        if (!string.IsNullOrEmpty(sha1))
        {
            using var stream = File.OpenRead(dest + ".part");
            using var sha = SHA1.Create();
            var hash = Convert.ToHexString(sha.ComputeHash(stream)).ToLowerInvariant();
            if (hash != sha1.ToLowerInvariant())
                throw new InvalidOperationException($"SHA1 校验失败：期望 {sha1}，实际 {hash}");
        }

        if (File.Exists(dest)) File.Delete(dest);
        File.Move(dest + ".part", dest);
        return dest;
    }

    /// <summary>获取资源应安装到的目录。</summary>
    public static string GetInstallDir(string gameRoot, string? versionId, string kind)
    {
        var baseDir = string.IsNullOrEmpty(versionId)
            ? gameRoot
            : Path.Combine(gameRoot, "versions", versionId);
        return kind switch
        {
            "mod" => Path.Combine(baseDir, "mods"),
            "resourcepack" => Path.Combine(baseDir, "resourcepacks"),
            "shader" => Path.Combine(baseDir, "shaderpacks"),
            "datapack" => Path.Combine(baseDir, "datapacks"),
            _ => Path.Combine(gameRoot, "downloads"),
        };
    }
}
