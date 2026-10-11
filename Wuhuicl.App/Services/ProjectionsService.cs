using System.IO;
using Wuhuicl.App.Services;
using Wuhuicl.Core.Storage;

/// <summary>
/// 投影页后端：扫 &lt;游戏根&gt;\projections（集中收藏，递归）+
/// &lt;root&gt;\versions\&lt;id&gt;\schematics\*.litematic | *.schem | *.schematic | *.nbt（顶层）。
/// 投影是纯数据文件，无 ZIP 清单可校验；扩展名即格式（原版 :66 的四档）。
/// </summary>
public sealed class ProjectionsService
{
    static GamePaths Paths => GamePaths.DetectDefault();

    public static readonly string[] Extensions = [".litematic", ".schem", ".schematic", ".nbt"];

    /// <summary>集中收藏（导入落点，「打开收藏文件夹」的目标）。</summary>
    public string LibraryDir => Path.Combine(Paths.Root, "projections");

    public MediaScanResult Scan()
    {
        var warnings = new List<string>();
        var files = new List<string>();
        RecordingsService.WalkFiles(LibraryDir, files, warnings);

        var entries = new List<MediaEntry>();
        foreach (var f in files)
        {
            string ext = Path.GetExtension(f).ToLowerInvariant();
            if (!Extensions.Contains(ext)) continue;
            entries.Add(RecordingsService.From(f, ext.TrimStart('.'), library: true, versionId: null));
        }

        try
        {
            if (Directory.Exists(Paths.Versions))
                foreach (var verDir in Directory.EnumerateDirectories(Paths.Versions))
                {
                    string id = Path.GetFileName(verDir);
                    string schem = Path.Combine(verDir, "schematics");
                    IEnumerable<string> schemFiles = Directory.Exists(schem)
                        ? Directory.EnumerateFiles(schem) : [];
                    foreach (var f in schemFiles)
                    {
                        string ext = Path.GetExtension(f).ToLowerInvariant();
                        if (!Extensions.Contains(ext)) continue;
                        entries.Add(RecordingsService.From(f, ext.TrimStart('.'), library: false, versionId: id));
                    }
                }
        }
        catch (Exception ex)
        {
            warnings.Add($"{Paths.Versions}：{ex.Message}");
        }

        entries.Sort(static (a, b) =>
        {
            int c = b.Modified.CompareTo(a.Modified);
            return c != 0 ? c : string.CompareOrdinal(a.Name, b.Name);
        });
        return new MediaScanResult { Entries = entries, Warnings = warnings };
    }

    /// <summary>导入：复制进集中收藏，同名加序号；扩展名不符拒收。</summary>
    public string Import(string src, out string? error)
    {
        error = null;
        string ext = Path.GetExtension(src).ToLowerInvariant();
        if (!Extensions.Contains(ext))
        {
            error = $"{Path.GetFileName(src)}：只支持 .litematic / .schem / .schematic / .nbt";
            return "";
        }
        try
        {
            Directory.CreateDirectory(LibraryDir);
            string dest = RecordingsService.UniqueDest(LibraryDir, Path.GetFileName(src));
            File.Copy(src, dest, overwrite: false);
            return dest;
        }
        catch (Exception ex)
        {
            error = $"{Path.GetFileName(src)}：复制失败（{ex.Message}）";
            return "";
        }
    }
}
