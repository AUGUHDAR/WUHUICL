using System.IO;
using Wuhuicl.Core.Minecraft;
using Wuhuicl.Core.Stores;

namespace Wuhuicl.Core;

/// <summary>
/// 离线自检：只跑不依赖网络的部分，重点覆盖台账里登记过的坑。
/// 每条断言对应一条真实缺陷，改引擎时若回归会立刻红。
/// </summary>
public static class SelfTest
{
    public static async Task<int> RunAsync(TextWriter w)
    {
        int pass = 0, fail = 0;
        void Check(string name, bool ok, string? detail = null)
        {
            if (ok) { pass++; w.WriteLine($"  ok   {name}"); }
            else { fail++; w.WriteLine($"  FAIL {name}{(detail is null ? "" : "  -> " + detail)}"); }
        }

        // --- 离线 UUID：同名必须稳定，且带版本位 3 ---
        var u1 = Json.OfflineUuid("Steve");
        var u2 = Json.OfflineUuid("Steve");
        var u3 = Json.OfflineUuid("steve");
        Check("离线 UUID 同名稳定", u1 == u2, u1 + " vs " + u2);
        Check("离线 UUID 大小写敏感", u1 != u3);
        Check("离线 UUID 形如 8-4-4-4-12 且版本位为 3",
            System.Text.RegularExpressions.Regex.IsMatch(u1, @"^[0-9a-f]{8}-[0-9a-f]{4}-3[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$"), u1);

        // --- 玩家名校验 ---
        Check("玩家名接受中文被拒绝", Account.ValidateName("张三") is not null);
        Check("玩家名 16 位上限", Account.ValidateName(new string('a', 17)) is not null);
        Check("玩家名合法通过", Account.ValidateName("Notch_9") is null);

        // --- Java 版本矩阵 ---
        Check("1.20.1 需要 Java 17", JavaLocator.RequiredMajor("1.20.1") == 17,
            JavaLocator.RequiredMajor("1.20.1").ToString());
        Check("1.20.5 需要 Java 21", JavaLocator.RequiredMajor("1.20.5") == 21);
        Check("1.21 需要 Java 21", JavaLocator.RequiredMajor("1.21") == 21);
        Check("1.16.5 需要 Java 8", JavaLocator.RequiredMajor("1.16.5") == 8);
        Check("1.12.2 需要 Java 8", JavaLocator.RequiredMajor("1.12.2") == 8);

        // --- -version 解析 ---
        Check("解析 1.8.0_402 -> 8", JavaLocator.ParseMajor("java version \"1.8.0_402\"") == 8);
        Check("解析 11.0.2 -> 11", JavaLocator.ParseMajor("openjdk version \"11.0.2\" 2019-01-15") == 11);
        Check("解析 21.0.4 -> 21", JavaLocator.ParseMajor("openjdk version \"21.0.4\"") == 21);

        // --- 版本比较：数字段不能被当字典序 ---
        Check("1.20.10 > 1.20.4", VersionCompare.Compare("1.20.10", "1.20.4") > 0);
        Check("1.16 > 1.12", VersionCompare.Compare("1.16", "1.12") > 0);
        Check("1.21 > 1.20.5", VersionCompare.Compare("1.21", "1.20.5") > 0);
        Check("相等版本为 0", VersionCompare.Compare("1.20.1", "1.20.1") == 0);

        // --- maven 坐标转路径（含 classifier）---
        Check("普通坐标转路径",
            Core.Storage.GamePaths.MavenToPath("com.mojang:logging:1.1.0") == "com/mojang/logging/1.1.0/logging-1.1.0.jar",
            Core.Storage.GamePaths.MavenToPath("com.mojang:logging:1.1.0"));
        Check("带 classifier 转路径",
            Core.Storage.GamePaths.MavenToPath("org.lwjgl:lwjgl:3.3.3:natives-windows") ==
            "org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3-natives-windows.jar",
            Core.Storage.GamePaths.MavenToPath("org.lwjgl:lwjgl:3.3.3:natives-windows"));

        // --- rules 求值语义 ---
        using (var doc = System.Text.Json.JsonDocument.Parse(
                   """[{"action":"allow"},{"action":"disallow","os":{"name":"osx"}}]"""))
        {
            var rules = Minecraft.Rule.Parse(doc.RootElement);
            Check("Windows 下 allow+非本机 deny 组合放行", Minecraft.Rule.Allow(rules, new()));
        }
        using (var doc2 = System.Text.Json.JsonDocument.Parse(
                   """[{"action":"allow","os":{"name":"osx"}}]"""))
        {
            var onlyOsx = Minecraft.Rule.Parse(doc2.RootElement);
            Check("仅 osx 的规则在 Windows 不成立", !Minecraft.Rule.Allow(onlyOsx, new()));
        }

        // --- classpath 去重键（台账 RUN-3：ASM 9.10.1 与 9.6 不能同时进）---
        var libA = new Minecraft.Library { Name = "org.ow2.asm:asm:9.6" };
        var libB = new Minecraft.Library { Name = "org.ow2.asm:asm:9.10.1" };
        Check("同 group:artifact 视为冲突", libA.DedupeKey == libB.DedupeKey, libA.DedupeKey);
        var n1 = new Minecraft.Library { Name = "org.lwjgl:lwjgl:3.3.3:natives-windows" };
        var n2 = new Minecraft.Library { Name = "org.lwjgl:lwjgl:3.3.3" };
        Check("classifier 与主件不算同一件", n1.DedupeKey != n2.DedupeKey, n1.DedupeKey + " / " + n2.DedupeKey);

        // --- 版本链解析与继承（本地构造，不联网）---
        var tmp = Path.Combine(Path.GetTempPath(), "wuhuicl-selftest-" + Guid.NewGuid().ToString("N")[..8]);
        try
        {
            var paths = new Core.Storage.GamePaths(tmp, tmp);
            paths.EnsureDirectories();
            Write(paths.VersionJson("1.20.1"), """
{
  "id": "1.20.1", "type": "release",
  "mainClass": "net.minecraft.client.main.Main",
  "assets": "5", "javaVersion": { "majorVersion": "17" },
  "arguments": {
    "game": [
      "--username", "${auth_player_name}",
      "--version", "${version_name}",
      "--gameDir", "${game_directory}",
      "--assetsDir", "${assets_root}",
      "--assetIndex", "${assets_index_name}",
      "--uuid", "${auth_uuid}",
      "--accessToken", "${auth_access_token}",
      "--userType", "${user_type}",
      "--versionType", "${version_type}",
      {
        "rules": [ { "action": "allow", "features": { "has_custom_resolution": true } } ],
        "value": [ "--width", "${resolution_width}", "--height", "${resolution_height}" ]
      }
    ],
    "jvm": [
      "-Djava.library.path=${natives_directory}",
      "-cp", "${classpath}"
    ]
  },
  "libraries": [
    { "name": "com.google.guava:guava:21.0",
      "downloads": { "artifact": {
        "url": "https://libraries.minecraft.net/com/google/guava/guava/21.0/guava-21.0.jar",
        "sha1": "aa", "size": 5 } } }
  ],
  "downloads": { "client": { "url": "https://x/client.jar", "sha1": "cc", "size": 6 } }
}
""");
            Write(paths.VersionJson("fabric-1.20.1"), """
            {"id":"fabric-1.20.1","inheritsFrom":"1.20.1","type":"release",
             "mainClass":"net.fabricmc.loader.impl.launch.knot.KnotClient",
             "libraries":[{"name":"net.fabricmc:fabric-loader:0.15.0"},
                          {"name":"com.google.guava:guava:21.0"}]}
            """);
            var guava = paths.LibraryPath("com/google/guava/guava/21.0/guava-21.0.jar");
            Directory.CreateDirectory(Path.GetDirectoryName(guava)!);
            File.WriteAllText(guava, "guava");
            File.WriteAllText(paths.ClientJar("1.20.1"), "client");
            Minecraft.VersionInfo Load(string id)
            {
                var f = paths.VersionJson(id);
                return File.Exists(f) ? Minecraft.VersionInfo.Parse(File.ReadAllText(f)) : null!;
            }
            var rv = Minecraft.VersionInfo.Resolve(Load, "fabric-1.20.1");
            Check("继承链找到两个版本", rv.Chain.Count == 2, rv.Chain.Count.ToString());
            Check("主类取叶子（loader 的）", rv.MainClass.Contains("KnotClient"), rv.MainClass);
            Check("Java 需求继承自基版本", rv.JavaMajor == 17, rv.JavaMajor.ToString());
            Check("重复库被去重", rv.Libraries.Count(l => l.Name.StartsWith("com.google.guava")) == 1,
                rv.Libraries.Count.ToString());
            Check("基版本的 JVM 参数被继承", rv.Args.Jvm.Any(a => a.Value == "-cp"));

            var plan = LaunchPlanBuilder.Build(paths, rv, new LaunchOptions
            {
                VersionId = "fabric-1.20.1", Username = "Steve",
                Uuid = Json.OfflineUuid("Steve"), Token = "tok",
                AccountType = "offline", Width = 1280, Height = 720,
            });
            var ti = plan.Args.IndexOf("--userType");
            Check("离线 user_type 必须是 legacy",
                ti >= 0 && plan.Args[ti + 1] == "legacy",
                ti >= 0 ? plan.Args[ti + 1] : "缺 --userType");
            Check("占位符已全部展开", !plan.Args.Any(a => a.Contains("${", StringComparison.Ordinal)),
                plan.Args.FirstOrDefault(a => a.Contains("${", StringComparison.Ordinal)) ?? "");
            Check("resolution 特性触发自定义分辨率参数",
                plan.Args.Contains("--width") && plan.Args.Contains("1280"));
            Check("classpath 里含客户端 jar",
                plan.Args[plan.Args.IndexOf("-cp") + 1].Contains("1.20.1.jar"));

            // 账号存储：原子写 + 密文往返
            var store = new AccountStore(paths);
            store.Load();
            store.AddOffline("Notch");
            store.AddOffline("Lee");
            var store2 = new AccountStore(paths);
            store2.Load();
            Check("账号落盘后可读回", store2.Accounts.Count == 2, store2.Accounts.Count.ToString());
            Check("后加的账号成为选中项", store2.Selected?.Name == "Lee", store2.Selected?.Name);
            Check("token 未明文落盘",
                !File.ReadAllText(paths.AccountsFile).Contains(store2.Selected!.Token!),
                "accounts.json 里能搜到明文 token");
            var (back, damaged) = SecretProtect.Unprotect(store2.Accounts[0].TokenEnc);
            Check("DPAPI 往返一致", back == store.Accounts[0].Token && !damaged, $"back={back} damaged={damaged}");

            // 下载校验：坏文件必须被判定不通过（台账 PACK-1）
            var bad = Path.Combine(tmp, "bad.bin");
            File.WriteAllText(bad, "错误的正文内容");
            Check("sha1 不匹配判定为失败",
                !await Minecraft.Downloader.VerifyAsync(bad, "0000000000000000000000000000000000000000", default));
            var good = File.ReadAllBytes(bad);
            var sha = Json.Sha1Hex(good);
            Check("sha1 匹配判定为通过", await Minecraft.Downloader.VerifyAsync(bad, sha, default));

            // 路径逃逸防护
            bool threw = false;
            try { paths.SafeUnder(paths.Libraries, "..\\..\\evil.jar"); } catch (IOException) { threw = true; }
            Check("拒绝越出根目录的相对路径", threw);
        }
        finally
        {
            try { Directory.Delete(tmp, true); } catch { }
        }

        w.WriteLine($"\n自检：{pass} 通过，{fail} 失败");
        return fail;
    }

    static void Write(string path, string text)
    {
        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
        File.WriteAllText(path, text);
    }
}
