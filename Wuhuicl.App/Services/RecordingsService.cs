using System.IO;
using System.IO.Compression;
using Microsoft.VisualBasic.FileIO;
using Wuhuicl.Core.Storage;

namespace Wuhuicl.App.Services;

/// <summary>
/// 录像 / 投影列表的一行 = 一个真实文件。kind 录像页是 ReplayMod / Flashback，
/// 投影页是扩展名（litematic / schem / schematic / nbt，不带点，与行 Tag 的筛选口径一致）。
/// source 是「集中收藏 / 游戏目录」两态（原版 RecordingsView.vue:64 的位置筛选项）。
/// </summary>
public sealed class MediaEntry
{
    public required string Name { get; init; }         // 文件名（含扩展名）
    public required string Kind { get; init; }         // ReplayMod / Flashback / litematic / ...
    public required long Size { get; init; }
    public required DateTime Modified { get; init; }
    public required string Directory { get; init; }    // 所在目录（列表第三行的 .path）
    public required string FullPath { get; init; }
    /// <summary>true = 来自集中收藏（&lt;root&gt;\recordings 或 &lt;root&gt;\projections）。</summary>
    public required bool Library { get; init; }
    /// <summary>来自版本目录时的版本 id；集中收藏下为 null。</summary>
    public string? VersionId { get; init; }

    public string Source => Library ? "集中收藏" : "游戏目录";
}

/// <summary>一轮扫描的结果：条目 + 逐目录的读取警告（进原版 :67 的「部分目录未读取」details）。</summary>
public sealed class MediaScanResult
{
    public List<MediaEntry> Entries { get; init; } = [];
    public List<string> Warnings { get; init; } = [];
}

/// <summary>
/// 录像页后端（对齐原版 recordings:list / recordings:import / recordings:open / operate trash 的文件侧语义）：
/// 扫描 &lt;游戏根&gt;\recordings（集中收藏，跨实例共享目录，含其下 library 子目录——原版库路径
/// RecordingsView.vue:58 就是 recordings\library）+ &lt;root&gt;\versions\&lt;id&gt;\replay_recordings\*.mcpr
/// 与 &lt;root&gt;\versions\&lt;id&gt;\flashback\replays\*.zip。
/// ReplayMod = .mcpr（ZIP 内必须同时有 recording.tmcpr + metaData.json，否则不算有效录像）；
/// Flashback = .zip（ZIP 内必须有 metadata.json + chunks/ 条目）。按修改时间降序。
/// </summary>
public sealed class RecordingsService
{
    static GamePaths Paths => GamePaths.DetectDefault();

    /// <summary>集中收藏（导入落点，「打开收藏文件夹」的目标）。</summary>
    public string LibraryDir => Path.Combine(Paths.Root, "recordings");

    /// <summary>录像扫描：集中收藏（递归）+ 各版本目录的两处固定位置。坏目录记警告不炸整轮。</summary>
    public MediaScanResult Scan()
    {
        var warnings = new List<string>();
        var files = new List<string>();
        RecordingsService.WalkFiles(LibraryDir, files, warnings);

        var entries = new List<MediaEntry>();
        foreach (var f in files)
        {
            string ext = Path.GetExtension(f).ToLowerInvariant();
            string? kind = ext switch
            {
                ".mcpr" => IsValidReplayMod(f) ? "ReplayMod" : null,
                ".zip" => IsValidFlashback(f) ? "Flashback" : null,
                _ => null,
            };
            if (kind is null) continue; // 扩展名不符或 ZIP 内容校验失败 → 不是有效录像
            entries.Add(From(f, kind, library: true, versionId: null));
        }

        // 版本目录：versions/<id>/replay_recordings/*.mcpr + versions/<id>/flashback/replays/*.zip（只扫顶层）
        try
        {
            if (Directory.Exists(Paths.Versions))
                foreach (var verDir in Directory.EnumerateDirectories(Paths.Versions))
                {
                    string id = Path.GetFileName(verDir);
                    foreach (var f in TopLevel(Title(verDir, "replay_recordings"), "*.mcpr", warnings))
                        if (IsValidReplayMod(f))
                            entries.Add(From(f, "ReplayMod", library: false, versionId: id));
                    foreach (var f in TopLevel(Title(verDir, "flashback", "replays"), "*.zip", warnings))
                        if (IsValidFlashback(f))
                            entries.Add(From(f, "Flashback", library: false, versionId: id));
                }
        }
        catch (Exception ex)
        {
            warnings.Add($"{Paths.Versions}：{ex.Message}");
        }

        entries.Sort(static (a, b) =>
        {
            int c = b.Modified.CompareTo(a.Modified); // modified 降序
            return c != 0 ? c : string.CompareOrdinal(a.Name, b.Name);
        });
        return new MediaScanResult { Entries = entries, Warnings = warnings };
    }

    /// <summary>导入：复制进集中收藏，同名自动加序号（-1、-2…）。非法内容拒收，返回错误文案。</summary>
    public string Import(string src, out string? error)
    {
        error = null;
        string ext = Path.GetExtension(src).ToLowerInvariant();
        if (ext == ".mcpr" && !IsValidReplayMod(src))
        {
            error = $"{Path.GetFileName(src)}：不是有效的 ReplayMod 录像（缺少 recording.tmcpr 或 metaData.json）";
            return "";
        }
        if (ext == ".zip" && !IsValidFlashback(src))
        {
            error = $"{Path.GetFileName(src)}：不是有效的 Flashback 录像（缺少 metadata.json 或 chunks）";
            return "";
        }
        if (ext is not (".mcpr" or ".zip"))
        {
            error = $"{Path.GetFileName(src)}：只支持 .mcpr / .zip";
            return "";
        }
        try
        {
            Directory.CreateDirectory(LibraryDir);
            string dest = UniqueDest(LibraryDir, Path.GetFileName(src));
            File.Copy(src, dest, overwrite: false);
            return dest;
        }
        catch (Exception ex)
        {
            error = $"{Path.GetFileName(src)}：复制失败（{ex.Message}）";
            return "";
        }
    }

    // ---------------- 校验（ZIP 内清单） ----------------

    /// <summary>ReplayMod .mcpr：ZIP 根必须同时有 recording.tmcpr 与 metaData.json。</summary>
    public static bool IsValidReplayMod(string path)
    {
        try
        {
            using var zip = ZipFile.OpenRead(path);
            bool hasRec = false, hasMeta = false;
            foreach (var e in zip.Entries)
            {
                string name = e.FullName.Replace('\\', '/').TrimStart('/');
                if (name.Equals("recording.tmcpr", StringComparison.OrdinalIgnoreCase)) hasRec = true;
                if (name.Equals("metaData.json", StringComparison.OrdinalIgnoreCase)) hasMeta = true;
                if (hasRec && hasMeta) return true;
            }
            return false;
        }
        catch { return false; }
    }

    /// <summary>Flashback .zip：ZIP 内必须有 metadata.json 与 chunks/ 前缀条目。</summary>
    public static bool IsValidFlashback(string path)
    {
        try
        {
            using var zip = ZipFile.OpenRead(path);
            bool hasMeta = false, hasChunks = false;
            foreach (var e in zip.Entries)
            {
                string name = e.FullName.Replace('\\', '/').TrimStart('/');
                if (name.Equals("metadata.json", StringComparison.OrdinalIgnoreCase)) hasMeta = true;
                if (name.StartsWith("chunks/", StringComparison.OrdinalIgnoreCase)) hasChunks = true;
                if (hasMeta && hasChunks) return true;
            }
            return false;
        }
        catch { return false; }
    }

    // ---------------- 通用小件（投影页共用） ----------------

    internal static MediaEntry From(string fullPath, string kind, bool library, string? versionId) =>
        new()
        {
            Name = Path.GetFileName(fullPath),
            Kind = kind,
            Size = TrySize(fullPath),
            Modified = TryModified(fullPath),
            Directory = Path.GetDirectoryName(fullPath) ?? fullPath,
            FullPath = fullPath,
            Library = library,
            VersionId = versionId,
        };

    /// <summary>移入系统回收站（原版 operate trash 的落点；Microsoft.VisualBasic 框架自带）。</summary>
    public static void Recycle(string path) =>
        FileSystem.DeleteFile(path, UIOption.OnlyErrorDialogs, RecycleOption.SendToRecycleBin);

    /// <summary>explorer /select 定位单个文件；目录不存在时退化为打开所在目录。</summary>
    public static void Reveal(string fullPath)
    {
        string arg = File.Exists(fullPath)
            ? $"/select,\"{fullPath}\""
            : $"/select,\"{Path.GetDirectoryName(fullPath) ?? fullPath}\"";
        System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo("explorer.exe", arg)
        {
            UseShellExecute = true,
        });
    }

    public static string FormatSize(long bytes)
    {
        double v = bytes;
        string[] units = ["B", "KB", "MB", "GB", "TB"];
        int i = 0;
        while (v >= 1024 && i < units.Length - 1) { v /= 1024; i++; }
        return i == 0 ? $"{v:0} B" : v >= 100 ? $"{v:0.#} {units[i]}" : $"{v:0.0#} {units[i]}";
    }

    /// <summary>目标目录里同名文件自动加序号：name-1.ext、name-2.ext…</summary>
    internal static string UniqueDest(string dir, string fileName)
    {
        string name = Path.GetFileNameWithoutExtension(fileName);
        string ext = Path.GetExtension(fileName);
        string dest = Path.Combine(dir, fileName);
        for (int i = 1; File.Exists(dest); i++)
            dest = Path.Combine(dir, $"{name}-{i}{ext}");
        return dest;
    }

    /// <summary>递归收集目录下全部文件；任何一层打不开都记一条警告继续扫（不炸整轮）。</summary>
    internal static void WalkFiles(string dir, List<string> into, List<string> warnings)
    {
        if (!Directory.Exists(dir)) return;
        try { foreach (var f in Directory.EnumerateFiles(dir)) into.Add(f); }
        catch (Exception ex) { warnings.Add($"{dir}：{ex.Message}"); }
        try
        {
            foreach (var d in Directory.EnumerateDirectories(dir))
                WalkFiles(d, into, warnings);
        }
        catch (Exception ex) { warnings.Add($"{dir}：{ex.Message}"); }
    }

    static IEnumerable<string> TopLevel(string dir, string pattern, List<string> warnings)
    {
        try { return Directory.Exists(dir) ? Directory.EnumerateFiles(dir, pattern) : []; }
        catch (Exception ex) { warnings.Add($"{dir}：{ex.Message}"); return []; }
    }

    static string Title(string baseDir, params string[] parts)
    {
        string p = baseDir;
        foreach (var x in parts) p = Path.Combine(p, x);
        return p;
    }

    static long TrySize(string path) { try { return new FileInfo(path).Length; } catch { return 0; } }
    static DateTime TryModified(string path)
    {
        try { return File.GetLastWriteTime(path); } catch { return DateTime.MinValue; }
    }
}
