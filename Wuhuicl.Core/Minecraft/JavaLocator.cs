using System.Diagnostics;
using System.IO;
using System.Text.RegularExpressions;

namespace Wuhuicl.Core.Minecraft;

public sealed record JavaInstall(string Exe, string Home, int Major, bool Is64, string? Vendor, DateTime Mtime)
{
    public string Display => $"Java {Major}{(Is64 ? "" : " (32位)")}  {Home}";
}

/// <summary>
/// 探测本机 Java：启动器自带 runtime、JAVA_HOME、PATH、常见安装目录。
/// 判定一律用真实执行 `java -version` 的输出来定，不信目录名。
/// </summary>
public static class JavaLocator
{
    static readonly Regex V1 = new(@"""?(\d+)\.(\d+)", RegexOptions.Compiled);
    static readonly Regex V2 = new(@"""?(\d+)(?:\.(\d+))?", RegexOptions.Compiled);

    public static JavaInstall? Probe(string exe, int timeoutMs = 4000)
    {
        if (string.IsNullOrEmpty(exe) || !File.Exists(exe)) return null;
        try
        {
            var psi = new ProcessStartInfo(exe, "-version")
            {
                RedirectStandardError = true, RedirectStandardOutput = true,
                UseShellExecute = false, CreateNoWindow = true,
            };
            using var p = Process.Start(psi);
            if (p is null) return null;
            string err = p.StandardError.ReadToEnd();
            string outp = p.StandardOutput.ReadToEnd();
            if (!p.WaitForExit(timeoutMs)) { try { p.Kill(true); } catch { } return null; }
            var text = (err + outp).Trim();
            int major = ParseMajor(text);
            if (major <= 0) return null;
            var dir = Path.GetDirectoryName(exe) ?? "";
            var home = Path.GetFileName(dir) == "bin" ? Path.GetDirectoryName(dir) ?? dir : dir;
            var vendor = text.Split('\n')[0].Trim();
            bool is64 = !text.Contains("64-Bit", StringComparison.OrdinalIgnoreCase) || true;
            return new JavaInstall(Path.GetFullPath(exe), Path.GetFullPath(home), major,
                                   Environment.Is64BitProcess, vendor,
                                   File.GetLastWriteTimeUtc(exe));
        }
        catch { return null; }
    }

    public static int ParseMajor(string versionText)
    {
        // "openjdk version \"1.8.0_402\"" -> 8 ； "\"11.0.2\"" -> 11 ； "21.0.4" -> 21
        var m = Regex.Match(versionText, @"version\s+""?([0-9][^""\s]*)");
        var s = m.Success ? m.Groups[1].Value : versionText;
        var m1 = V1.Match(s);
        if (m1.Success && m1.Groups[1].Value == "1" && m1.Groups[2].Value.Length == 1)
            return int.Parse(m1.Groups[2].Value);
        var m2 = V2.Match(s);
        return m2.Success ? int.Parse(m2.Groups[1].Value) : 0;
    }

    /// <summary>Mojang 的硬性要求：1.20.5+ 要 21，1.17~1.20.4 要 17，其余 8。</summary>
    public static int RequiredMajor(string mcVersion)
    {
        int[] v = ParseParts(mcVersion);
        int major = v[0], minor = v[1];
        if (major >= 1 && minor >= 21) return 21;
        if (major == 1 && minor == 20)
        {
            int patch = v.Length > 2 ? v[2] : 0;
            return patch >= 5 ? 21 : 17;
        }
        if (major == 1 && minor >= 17) return 17;
        if (major >= 2) return 21;                       // 未来版本号
        return 8;
    }

    static int[] ParseParts(string s) => s.Split('.', '-')
        .Select(x => int.TryParse(new string(x.Where(char.IsDigit).ToArray()), out var n) ? n : 0)
        .ToArray();

    public static List<JavaInstall> Scan(Core.Storage.GamePaths paths)
    {
        var found = new List<JavaInstall>();
        var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        void Add(string exe)
        {
            if (seen.Add(exe))
            {
                var j = Probe(exe);
                if (j is not null) found.Add(j);
            }
        }

        // 1) 启动器自己管的 runtime（最快、最可控）
        foreach (var bin in Directory.Exists(paths.RuntimeDir)
                 ? Directory.EnumerateDirectories(paths.RuntimeDir) : Array.Empty<string>())
        {
            var exe = Path.Combine(bin, "bin", "javaw.exe");
            if (File.Exists(exe)) Add(exe);
            var exe2 = Path.Combine(bin, "bin", "java.exe");
            if (File.Exists(exe2)) Add(exe2);
        }
        // 2) 环境变量
        foreach (var varName in new[] { "WUHUICL_JAVA_HOME", "JAVA_HOME" })
        {
            var h = Environment.GetEnvironmentVariable(varName);
            if (string.IsNullOrWhiteSpace(h)) continue;
            foreach (var name in new[] { "javaw.exe", "java.exe" })
            {
                var exe = Path.Combine(h, "bin", name);
                if (File.Exists(exe)) Add(exe);
            }
        }
        // 3) PATH
        var pathVar = Environment.GetEnvironmentVariable("PATH") ?? "";
        foreach (var dir in pathVar.Split(Path.PathSeparator, StringSplitOptions.RemoveEmptyEntries))
        {
            foreach (var name in new[] { "javaw.exe", "java.exe" })
            {
                try { var exe = Path.Combine(dir.Trim(), name); if (File.Exists(exe)) Add(exe); }
                catch { }
            }
        }
        // 4) 常见安装位置
        foreach (var root in new[]
        {
            Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles),
            Environment.GetFolderPath(Environment.SpecialFolder.ProgramFilesX86),
            Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), ".jdks"),
            Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Programs"),
            @"C:\Program Files\Eclipse Adoptium", @"C:\Program Files\Microsoft",
            @"C:\Program Files\Zulu", @"C:\Program Files\BellSoft",
        })
        {
            if (string.IsNullOrEmpty(root) || !Directory.Exists(root)) continue;
            IEnumerable<string> dirs;
            try { dirs = Directory.EnumerateDirectories(root).SelectMany(v => Directory.EnumerateDirectories(v)).Concat(Directory.EnumerateDirectories(root)); }
            catch { continue; }
            int hit = 0;
            foreach (var d in dirs)
            {
                if (hit++ > 60) break;
                var exe = Path.Combine(d, "bin", "javaw.exe");
                if (File.Exists(exe)) Add(exe);
            }
        }

        return found
            .OrderByDescending(j => j.Major).ThenByDescending(j => j.Mtime)
            .GroupBy(j => j.Exe, StringComparer.OrdinalIgnoreCase)
            .Select(g => g.First()).ToList();
    }

    /// <summary>选出可用于该版本的 Java：优先满足最低要求里"刚好够新"的那个。</summary>
    public static string? Pick(List<JavaInstall> all, int needMajor, string? preferred)
    {
        if (!string.IsNullOrEmpty(preferred) && File.Exists(preferred))
        {
            var j = Probe(preferred);
            if (j is not null && j.Major >= needMajor) return j.Exe;
        }
        var ok = all.Where(j => j.Major >= needMajor).ToList();
        if (ok.Count == 0) return all.OrderByDescending(j => j.Major).FirstOrDefault()?.Exe;
        // 有精确匹配优先用精确匹配（老版本游戏常在新 Java 上崩）
        var exact = ok.Where(j => j.Major == needMajor).OrderBy(j => j.Exe).FirstOrDefault();
        return (exact ?? ok.OrderByDescending(j => j.Major).First()).Exe;
    }
}
