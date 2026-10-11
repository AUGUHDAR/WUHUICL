using System.IO;
using System.IO.Compression;
using System.Text.Json;

namespace Wuhuicl.Core.Minecraft;

/// <summary>autoMemoryMB（shared/memory.ts）：物理内存 25% 取 0.5GB 整，夹 2~8GB。</summary>
public static class MemoryPolicy
{
    public static int AutoMemoryMB(long totalMemMb)
    {
        var quarter = (long)Math.Floor(totalMemMb * 0.25 / 512) * 512;
        return (int)Math.Min(8192, Math.Max(2048, quarter));
    }
}

public sealed class LaunchOptions
{
    public string VersionId { get; set; } = "";
    public string Username { get; set; } = "";
    public string Uuid { get; set; } = "";
    public string Token { get; set; } = "0";
    public string AccountType { get; set; } = "offline";   // offline | microsoft | yggdrasil
    public string? ServerAddress { get; set; }
    public bool Fullscreen;
    public int Width, Height;
    public int MinHeapMb = 256;
    public int MaxHeapMb = 2048;
    public string? JavaPath { get; set; }
    public List<string> ExtraJvmArgs { get; } = new();
    public List<string> ExtraGameArgs { get; } = new();
    public string? GameDirOverride { get; set; }
    public bool Demo;
    public string? JavaAgent { get; set; }                  // authlib-injector
    public string? PrefetchedMetadata { get; set; }         // authlib-injector base64
    public string? SingleplayerWorld { get; set; }          // 启动并创建命令世界：--quickPlaySingleplayer（1.20+）
}

public sealed class LaunchPlan
{
    public string Java { get; set; } = "";
    public List<string> Args { get; } = new();
    public string WorkingDirectory { get; set; } = "";
    public string GameDir { get; set; } = "";
    public List<string> Classpath { get; } = new();
    public List<string> Warnings { get; } = new();
    public string Token { get; set; } = "";
    public string DisplayCommand(string? hide = null)
    {
        var sb = new System.Text.StringBuilder();
        sb.Append(Path.GetFileNameWithoutExtension(Java));
        foreach (var a in Args)
            sb.Append(' ').Append(hide is not null && a.Contains(hide, StringComparison.Ordinal)
                ? "***" : a.Contains(' ') ? "\"" + a + "\"" : a);
        return sb.ToString();
    }
    public string CommandLine => string.Join(' ', new[] { Java }.Concat(Args)
        .Select(a => a.Contains(' ') ? "\"" + a + "\"" : a));
}

/// <summary>
/// 组装启动计划。三处刻意纠正参考实现的缺陷：
///   RUN-3  classpath 按 Maven group:artifact[:classifier] 去重（不是按文件路径），
///          避免 ASM 9.10.1 与 9.6 同时进 classpath；
///   AUTH-8 clientid / auth_xuid 有真实来源，不留空占位；
///   AUTH-9 离线账号 user_type=legacy。
/// </summary>
public static class LaunchPlanBuilder
{
    public static LaunchPlan Build(Core.Storage.GamePaths paths, ResolvedVersion rv, LaunchOptions o)
    {
        var plan = new LaunchPlan { GameDir = o.GameDirOverride ?? paths.Root };
        Directory.CreateDirectory(Path.Combine(plan.GameDir, "logs"));

        // ---- natives：先把 natives jar 解开 ----
        var natives = paths.NativesDir(rv.Id);
        Directory.CreateDirectory(natives);
        ExtractNatives(paths, rv, natives, plan);

        // ---- classpath ----
        var cp = new List<string>();
        foreach (var lib in rv.Libraries)
        {
            if (!Allowed(lib, o)) continue;
            var p = paths.SafeUnder(paths.Libraries, lib.MavenPath);
            if (File.Exists(p)) cp.Add(p);
            else plan.Warnings.Add("缺少库文件：" + lib.Name);
        }
        // 客户端 jar 属于"真正持有 client 下载项"的那一层：loader 实例（fabric/quilt/forge）
        // 自己的目录里只有 profile json，去找 <loaderId>.jar 会永远缺文件。
        var clientId = rv.Chain.FirstOrDefault(c => c.Downloads.ContainsKey("client"))?.Id ?? rv.Id;
        var client = paths.ClientJar(clientId);
        if (File.Exists(client)) cp.Add(client);
        else plan.Warnings.Add($"缺少客户端 jar：{client}（请重新安装 {clientId}）");
        cp = Dedupe(cp, rv, plan);
        plan.Classpath.AddRange(cp);

        // ---- 特性开关 ----
        var features = new HashSet<string>(StringComparer.Ordinal);
        if (o.Demo) features.Add("is_demo_user");
        if (o.Width > 0 && o.Height > 0) features.Add("has_custom_resolution");
        if (!string.IsNullOrEmpty(o.ServerAddress)) features.Add("is_quickPlayMultiplayer");

        var vars = BuildVars(paths, rv, o, natives, cp);

        var args = new List<string>();
        // 1) JVM 参数：清单给的（含 loader 注入的 -p/-DignoreList 等）
        foreach (var (value, rules) in rv.Args.Jvm)
            if (Rule.Allow(rules, features)) args.Add(Expand(value, vars));
        // 2) 用户附加 JVM 参数
        foreach (var extra in o.ExtraJvmArgs) if (!string.IsNullOrWhiteSpace(extra)) args.Add(extra.Trim());
        // 3) 内存
        // 原版 JVM 基线（launch.ts:646-653）：-Xms 取 min(mem,1024)
        args.Add($"-Xms{Math.Min(o.MaxHeapMb, 1024)}m");
        args.Add($"-Xmx{Math.Max(512, o.MaxHeapMb)}m");
        args.Add("-XX:+UseG1GC");
        args.Add("-XX:+ParallelRefProcEnabled");
        args.Add("-XX:MaxGCPauseMillis=200");
        args.Add("-Dfile.encoding=UTF-8");
        // 4) authlib-injector（外置登录）
        if (!string.IsNullOrEmpty(o.JavaAgent))
            args.Add($"-javaagent:{o.JavaAgent}={vars.GetValueOrDefault("yggdrasil_session_server_base_url", "")}");
        if (!string.IsNullOrEmpty(o.PrefetchedMetadata))
            args.Add($"-Dauthlibinjector.yggdrasil.prefetched={o.PrefetchedMetadata}");
        // 5) classpath + 主类
        args.Add("-cp");
        args.Add(string.Join(Path.PathSeparator, cp));
        args.Add(rv.MainClass);
        // 6) 游戏参数
        foreach (var (value, rules) in rv.Args.Game)
            if (Rule.Allow(rules, features)) args.Add(Expand(value, vars));
        if (o.Fullscreen) args.Add("--fullscreen");
        if (!string.IsNullOrEmpty(o.ServerAddress))
        {
            bool supportsQuickPlay = VersionCompare.Compare(rv.Id, "1.20") >= 0;
            if (supportsQuickPlay)
            {
                args.Add("--quickPlayMultiplayer");
                args.Add(o.ServerAddress);
            }
            else plan.Warnings.Add("该版本不支持自动进服，已忽略服务器地址");
        }
        if (!string.IsNullOrEmpty(o.SingleplayerWorld))
        {
            if (VersionCompare.Compare(rv.Id, "1.20") >= 0)
            {
                args.Add("--quickPlaySingleplayer");
                args.Add(o.SingleplayerWorld);
            }
            else plan.Warnings.Add("该版本不支持 Quick Play 单人直进（需要 1.20+），已忽略世界参数");
        }
        foreach (var extra in o.ExtraGameArgs) if (!string.IsNullOrWhiteSpace(extra)) args.Add(extra.Trim());

        plan.Args.AddRange(args.Where(a => !string.IsNullOrEmpty(a)));
        plan.Java = o.JavaPath ?? "";
        plan.WorkingDirectory = plan.GameDir;
        plan.Token = o.Token;
        return plan;
    }

    static Dictionary<string, string> BuildVars(Core.Storage.GamePaths paths, ResolvedVersion rv,
        LaunchOptions o, string natives, List<string> cp)
    {
        bool isYgg = o.AccountType == "yggdrasil";
        string uuid = isYgg ? Json.StripDashes(o.Uuid) : o.Uuid;
        var userType = o.AccountType switch
        {
            "microsoft" => "msa",
            "yggdrasil" => "mojang",
            _ => "legacy",              // 离线账号必须是 legacy（原版服务端按此判定正版校验）
        };
        return new(StringComparer.Ordinal)
        {
            ["auth_player_name"] = o.Username,
            ["version_name"] = rv.Id,
            ["game_directory"] = o.GameDirOverride ?? paths.Root,
            ["assets_root"] = paths.Assets,
            ["game_assets"] = Path.Combine(paths.Assets, "virtual", rv.Assets),
            ["assets_index_name"] = rv.Assets,
            ["auth_uuid"] = uuid,
            ["auth_access_token"] = o.Token,
            ["auth_session"] = o.Token,
            ["clientid"] = "",
            ["auth_xuid"] = "",
            ["user_type"] = userType,
            ["version_type"] = rv.Leaf.Type.Length > 0 ? rv.Leaf.Type : "release",
            ["natives_directory"] = natives,
            ["launcher_name"] = "WUHUICL",
            ["launcher_version"] = "0.1.0",
            ["classpath"] = string.Join(Path.PathSeparator, cp),
            ["classpath_separator"] = Path.PathSeparator.ToString(),
            ["library_directory"] = paths.Libraries,
            ["user_properties"] = "{}",
            ["resolution_width"] = o.Width > 0 ? o.Width.ToString() : "854",
            ["resolution_height"] = o.Height > 0 ? o.Height.ToString() : "480",
        };
    }

    static string Expand(string template, Dictionary<string, string> vars)
    {
        var s = template;
        foreach (var kv in vars) s = s.Replace("${" + kv.Key + "}", kv.Value);
        return s;
    }

    static bool Allowed(Library lib, LaunchOptions o)
    {
        var features = new HashSet<string>(StringComparer.Ordinal);
        if (o.Demo) features.Add("is_demo_user");
        return Rule.Allow(lib.Rules, features);
    }

    /// <summary>
    /// 同一个 group:artifact[:classifier] 只允许出现一次，保留链上更具体的那份。
    /// 路径重复也顺手去掉（forge 与 vanilla 会引用同一份库）。
    /// </summary>
    static List<string> Dedupe(List<string> cp, ResolvedVersion rv, LaunchPlan plan)
    {
        var byKey = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
        var order = new List<string>();
        var libByKey = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
        foreach (var lib in rv.Libraries) libByKey[lib.DedupeKey] = lib.Name;

        foreach (var path in cp)
        {
            var key = KeyOf(path);
            if (byKey.ContainsKey(key))
            {
                plan.Warnings.Add($"库冲突，已保留先出现的一份：{key}");
                continue;
            }
            byKey[key] = path;
            order.Add(path);
        }
        return order;
    }

    static string KeyOf(string jarPath)
    {
        // .../com/mojang/logging/1.1.0/logging-1.1.0[-natives-windows].jar -> com.mojang:logging
        var parts = jarPath.Replace('\\', '/').Split('/');
        if (parts.Length >= 3)
        {
            var ver = parts[^2];
            var name = parts[^3];
            var group = string.Join('.', parts[..^3]);
            return string.IsNullOrEmpty(group) ? $"{name}:{ver}" : $"{group}:{name}";
        }
        return parts[^1];
    }

    static void ExtractNatives(Core.Storage.GamePaths paths, ResolvedVersion rv, string natives, LaunchPlan plan)
    {
        foreach (var lib in rv.Libraries)
        {
            if (lib.Native is null) continue;
            var jar = paths.SafeUnder(paths.Libraries,
                lib.MavenPath.Replace("${arch}", Environment.Is64BitProcess ? "64" : "32"));
            if (!File.Exists(jar)) { plan.Warnings.Add("缺少 natives 包：" + lib.Name); continue; }
            try
            {
                using var zip = System.IO.Compression.ZipFile.OpenRead(jar);
                foreach (var entry in zip.Entries)
                {
                    if (string.IsNullOrEmpty(entry.Name)) continue;            // 目录条目
                    if (entry.FullName.StartsWith("META-INF", StringComparison.OrdinalIgnoreCase)) continue;
                    bool excluded = lib.Exclude.Any(x => entry.FullName.StartsWith(x, StringComparison.OrdinalIgnoreCase));
                    if (excluded) continue;
                    var dest = paths.SafeUnder(natives, entry.FullName);
                    Directory.CreateDirectory(Path.GetDirectoryName(dest)!);
                    entry.ExtractToFile(dest, true);
                }
            }
            catch (Exception ex) { plan.Warnings.Add($"解压 natives 失败 {lib.Name}：{ex.Message}"); }
        }
    }
}
