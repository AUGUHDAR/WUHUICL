using System.IO;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace Wuhuicl.Core.Storage;

/// <summary>
/// 全项目唯一的路径事实源。其他任何地方不得自己拼 .minecraft 结构。
/// </summary>
public sealed class GamePaths
{
    public string Root { get; }          // .minecraft 等价物（游戏根）
    public string DataDir { get; }       // 启动器自己的数据目录
    public string VersionManifest => Path.Combine(Root, "version_manifest_v2.json");
    public string Versions => Path.Combine(Root, "versions");
    public string Libraries => Path.Combine(Root, "libraries");
    public string Assets => Path.Combine(Root, "assets");
    public string Logs => Path.Combine(Root, "logs");
    public string LauncherLogs => Path.Combine(DataDir, "logs");
    public string RuntimeDir => Path.Combine(DataDir, "runtime");
    public string AccountsFile => Path.Combine(DataDir, "accounts.json");
    public string SettingsFile => Path.Combine(DataDir, "settings.json");

    public GamePaths(string root, string dataDir)
    {
        Root = Path.GetFullPath(root);
        DataDir = Path.GetFullPath(dataDir);
    }

    /// <summary>portable.txt 在 exe 目录旁，或环境变量 WUHUICL_HOME 指定。</summary>
    public static GamePaths DetectDefault()
    {
        var exeDir = AppContext.BaseDirectory;
        var env = Environment.GetEnvironmentVariable("WUHUICL_HOME");
        if (!string.IsNullOrWhiteSpace(env))
            return new GamePaths(env, env);

        if (File.Exists(Path.Combine(exeDir, "portable.txt")))
        {
            var data = Path.Combine(exeDir, "WUHUICL-数据");
            return new GamePaths(Path.Combine(data, "minecraft"), data);
        }
        var appData = Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData);
        var dir = Path.Combine(appData, "WUHUICL");
        return new GamePaths(Path.Combine(dir, "minecraft"), dir);
    }

    public string VersionDir(string id) => Path.Combine(Versions, id);
    public string VersionJson(string id) => Path.Combine(Versions, id, id + ".json");
    public string ClientJar(string id) => Path.Combine(Versions, id, id + ".jar");
    public string NativesDir(string id) => Path.Combine(VersionDir(id), "natives-wu");
    public string AssetIndexFile(string name) => Path.Combine(Assets, "indexes", name + ".json");
    public string AssetObjectPath(string sha1) =>
        Path.Combine(Assets, "objects", sha1[..2], sha1);
    public string LibraryPath(string mavenPath) => Path.Combine(Libraries, mavenPath);

    /// <summary>
    /// maven 坐标转仓库相对路径。形式为 group:artifact:version[:classifier]，
    /// 少数老条目是 group:artifact:version:packaging:classifier（第 3 段是打包类型时跳过）。
    /// </summary>
    public static string MavenToPath(string coords)
    {
        var parts = coords.Split(':');
        if (parts.Length is < 3 or > 5) return coords.Replace(':', '/');
        var group = parts[0].Replace('.', '/');
        var name = parts[1];
        var ver = parts[2];
        string classifier = parts.Length switch
        {
            4 => "-" + parts[3],                                  // 最常见：分类器
            5 => "-" + parts[4],                                  // group:artifact:version:packaging:classifier
            _ => "",
        };
        return $"{group}/{name}/{ver}/{name}-{ver}{classifier}.jar";
    }

    /// <summary>坐标里的分类器（natives-windows 等），无则 null。</summary>
    public static string? MavenClassifier(string coords)
    {
        var parts = coords.Split(':');
        if (parts.Length == 4) return parts[3];
        if (parts.Length == 5) return parts[4];
        return null;
    }

    /// <summary>拒绝任何越出 Root 的目标（zip-slip / 清单里的绝对路径）。</summary>
    public string SafeUnder(string baseDir, string relative)
    {
        if (string.IsNullOrEmpty(relative))
            throw new IOException("空路径");
        if (Path.IsPathRooted(relative) || relative.Contains("..", StringComparison.Ordinal))
            throw new IOException($"路径越界，已拒绝：{relative}");
        var full = Path.GetFullPath(Path.Combine(baseDir, relative));
        var prefix = Path.GetFullPath(baseDir) + Path.DirectorySeparatorChar;
        if (!full.StartsWith(prefix, StringComparison.OrdinalIgnoreCase))
            throw new IOException($"路径逃逸出 {baseDir}，已拒绝：{full}");
        return full;
    }

    public void EnsureDirectories()
    {
        foreach (var d in new[] { Root, Versions, Libraries, Assets, Logs,
                     Path.Combine(Assets, "indexes"), Path.Combine(Assets, "objects"),
                     LauncherLogs, RuntimeDir })
            Directory.CreateDirectory(d);
    }
}

/// <summary>JSON 读写：一律临时文件 + 原子替换，坏文件不覆盖好文件。</summary>
public static class AtomicJson
{
    public static T Load<T>(string path, T fallback) where T : class
    {
        try
        {
            if (!File.Exists(path)) return fallback;
            var text = File.ReadAllText(path);
            if (string.IsNullOrWhiteSpace(text)) return fallback;
            return JsonSerializer.Deserialize<T>(text) ?? fallback;
        }
        catch
        {
            // 保留坏文件供排查，绝不静默清空用户数据
            try { File.Copy(path, path + ".bad-" + DateTime.Now.ToString("yyyyMMddHHmmss"), true); }
            catch { /* 尽力而为 */ }
            return fallback;
        }
    }

    public static void Save<T>(string path, T value)
    {
        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
        var tmp = path + ".tmp-" + Guid.NewGuid().ToString("N")[..8];
        var text = JsonSerializer.Serialize(value, JsonOpts);
        File.WriteAllText(tmp, text);
        try
        {
            if (File.Exists(path)) File.Replace(tmp, path, path + ".bak");
            else File.Move(tmp, path);
        }
        catch (IOException)
        {
            File.Delete(path);
            File.Move(tmp, path);
        }
    }

    public static readonly System.Text.Json.JsonSerializerOptions JsonOpts = new()
    {
        WriteIndented = true,
        Encoder = System.Text.Encodings.Web.JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
        PropertyNameCaseInsensitive = true,
    };
}
