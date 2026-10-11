using System.Diagnostics;
using System.IO;
using Microsoft.VisualBasic.FileIO;
using Wuhuicl.Core.Storage;

namespace Wuhuicl.App.Services;

/// <summary>资源列表里的一行文件。</summary>
public sealed class ResourceEntry
{
    /// <summary>显示名：mods 的 <c>.jar.disabled</c> 已剥掉 <c>.disabled</c> 后缀。</summary>
    public required string Name { get; init; }
    /// <summary>磁盘全路径（改名 / 删除用它，不是显示名）。</summary>
    public required string FullPath { get; init; }
    public long SizeBytes { get; init; }
    public DateTime LastWriteTime { get; init; }
    /// <summary>仅 mods 有语义：文件名不以 <c>.disabled</c> 结尾＝启用。</summary>
    public bool Enabled { get; init; }
    /// <summary>来自共享目录 <c>&lt;root&gt;\&lt;kind&gt;</c>（同名文件被版本目录优先顶掉，不会出现在列表里）。</summary>
    public bool FromShared { get; init; }
}

/// <summary>
/// 资源管理（模组 / 资源包 / 光影包）的文件层。目录语义对齐 KAMUCL 1.1.10 的隔离：
/// 版本资源目录 <c>&lt;root&gt;\versions\&lt;id&gt;\&lt;kind&gt;</c>（打开 / 写入时不存在则创建），
/// 另扫共享目录 <c>&lt;root&gt;\&lt;kind&gt;</c> 一并显示，同名文件版本目录优先。
/// 普通静态工具类（非单例，无状态）；所有 IO 都走 Task.Run，不持锁；
/// 任何对外路径先过防越界校验（拒绝 <c>..</c>、拒绝逃出游戏根）。
/// </summary>
public static class ResourceService
{
    static GamePaths Paths => GamePaths.DetectDefault();

    static string NormalizeKind(string kind) => kind switch
    {
        "resourcepacks" or "resourcepack" or "packs" => "resourcepacks",
        "shaderpacks" or "shaderpack" or "shaders" => "shaderpacks",
        _ => "mods",
    };

    /// <summary>版本资源目录。versionId 是版本目录名，不接受任何路径分量。</summary>
    static string VersionKindDir(GamePaths paths, string versionId, string kind)
    {
        if (string.IsNullOrWhiteSpace(versionId))
            throw new IOException("未选择游戏版本");
        if (versionId.Split('/', '\\').Any(s => s is "." or "..") || versionId.Contains(':'))
            throw new IOException($"非法的版本目录名：{versionId}");
        return paths.SafeUnder(paths.Versions, Path.Combine(versionId, kind));
    }

    /// <summary>扫一个目录的文件（不递归），折叠进 map；后写者覆盖＝谁后扫谁优先。</summary>
    static void Collect(IDictionary<string, ResourceEntry> map, string dir, bool fromShared)
    {
        if (!Directory.Exists(dir)) return;
        IEnumerable<string> files;
        try { files = Directory.EnumerateFiles(dir); }
        catch (Exception e) { throw new IOException($"读取目录失败：{dir}（{e.Message}）", e); }
        foreach (var path in files)
        {
            FileInfo fi;
            try { fi = new FileInfo(path); } catch { continue; }
            if (!fi.Exists) continue;   // 枚举与取属性之间被删掉：跳过这一条
            string name = fi.Name;
            bool enabled = true;
            if (name.EndsWith(".disabled", StringComparison.OrdinalIgnoreCase))
            {
                enabled = false;
                name = name[..^".disabled".Length];
            }
            map[name] = new ResourceEntry
            {
                Name = name,
                FullPath = fi.FullName,
                SizeBytes = fi.Length,
                LastWriteTime = fi.LastWriteTime,
                Enabled = enabled,
                FromShared = fromShared,
            };
        }
    }

    /// <summary>
    /// 列出某版本某类资源：共享目录 + 版本目录合并去重（同名版本目录优先）。
    /// 目录不存在返回空表（不创建）；文件名的 <c>.disabled</c> 后缀折成 Enabled=false。
    /// </summary>
    public static List<ResourceEntry> List(string versionId, string kind)
    {
        var paths = Paths;
        kind = NormalizeKind(kind);
        var versionDir = VersionKindDir(paths, versionId, kind);
        var sharedDir = Path.Combine(paths.Root, kind);
        var map = new Dictionary<string, ResourceEntry>(StringComparer.OrdinalIgnoreCase);
        Collect(map, sharedDir, fromShared: true);
        Collect(map, versionDir, fromShared: false);   // 版本目录后扫 → 同名覆盖共享
        return map.Values.ToList();
    }

    /// <summary>explorer 打开版本资源目录，不存在先创建。</summary>
    public static Task OpenFolderAsync(string versionId, string kind) => Task.Run(() =>
    {
        var paths = Paths;
        var dir = VersionKindDir(paths, versionId, NormalizeKind(kind));
        Directory.CreateDirectory(dir);
        using var _ = Process.Start(new ProcessStartInfo
        {
            FileName = "explorer.exe",
            Arguments = $"\"{dir}\"",
            UseShellExecute = true,
        });
    });

    /// <summary>仅 mods 语义：<c>.jar</c> ↔ <c>.jar.disabled</c> 改名；对 .disabled 文件调用＝恢复。</summary>
    public static Task ToggleEnabledAsync(string fullPath) => Task.Run(() =>
    {
        var full = Guard(fullPath);
        if (!File.Exists(full)) throw new IOException("文件不存在，可能已被移除或改名");
        string target = full.EndsWith(".disabled", StringComparison.OrdinalIgnoreCase)
            ? full[..^".disabled".Length]
            : full + ".disabled";
        if (File.Exists(target)) throw new IOException("同名目标文件已存在，无法改名");
        File.Move(full, target);
    });

    /// <summary>移入系统回收站（绝不物理删除）。</summary>
    public static Task DeleteToRecycleBinAsync(string fullPath) => Task.Run(() =>
    {
        var full = Guard(fullPath);
        if (!File.Exists(full)) throw new IOException("文件不存在，可能已被移除或改名");
        FileSystem.DeleteFile(full, UIOption.OnlyErrorDialogs, RecycleOption.SendToRecycleBin);
    });

    /// <summary>防越界：解析后必须落在游戏根之内（拒绝 .. 与根外绝对路径）。</summary>
    static string Guard(string path)
    {
        if (string.IsNullOrWhiteSpace(path)) throw new IOException("空路径");
        var full = Path.GetFullPath(path);
        var root = Path.GetFullPath(Paths.Root);
        if (full.Split(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar).Contains(".."))
            throw new IOException($"路径越界，已拒绝：{path}");
        if (!full.StartsWith(root + Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase) &&
            !full.Equals(root, StringComparison.OrdinalIgnoreCase))
            throw new IOException($"路径越界，已拒绝：{path}");
        return full;
    }
}
