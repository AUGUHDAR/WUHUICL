using System.IO.Compression;
using System.Text.Json;
using Wuhuicl.Core.Minecraft;

// ============ CommandWorld 自测：造假客户端 jar → 建世界 → NbtLite 回读校验 ============
var root = Path.Combine(Path.GetTempPath(), "wu-cmdworld-" + Guid.NewGuid().ToString("N")[..8]);
Directory.CreateDirectory(root);
int pass = 0, fail = 0;
void Check(bool ok, string name)
{
    if (ok) { pass++; Console.WriteLine("PASS " + name); }
    else { fail++; Console.WriteLine("FAIL " + name); }
}

string MakeJar(string id, int worldVersion, bool stable, string? series, bool splitClass)
{
    var jar = Path.Combine(root, $"client-{id}-{(splitClass ? "split" : "plain")}.jar");
    using (var fs = File.Create(jar))
    using (var zip = new ZipArchive(fs, ZipArchiveMode.Create))
    {
        var e = zip.CreateEntry("version.json");
        using (var w = new StreamWriter(e.Open()))
        {
            var obj = new Dictionary<string, object> { ["id"] = id, ["world_version"] = worldVersion, ["stable"] = stable };
            if (series is not null) obj["series_id"] = series;
            w.Write(JsonSerializer.Serialize(obj));
        }
        // WorldGenSettings.class：split 与否则看里面有没有 SavedDataType 字节
        var c = zip.CreateEntry("net/minecraft/world/level/levelgen/WorldGenSettings.class");
        using (var w = c.Open())
        {
            var bytes = splitClass ? "xxxxSavedDataTypexxxx"u8.ToArray() : "some bytecode"u8.ToArray();
            w.Write(bytes);
        }
    }
    return jar;
}

var gameDir = Path.Combine(root, "gamedir");
Directory.CreateDirectory(gameDir);

// 1.20.1（world_version 3465，非 split）
var jar1 = MakeJar("1.20.1", 3465, true, null, splitClass: false);
var world = CommandWorld.Create(gameDir, jar1);
Check(world.Id.StartsWith("WUHUICL-Test-"), "世界 id 前缀");
var levelPath = Path.Combine(world.Path, "level.dat");
Check(File.Exists(levelPath), "level.dat 已落盘");

var parsed = NbtLite.ParseBytes(File.ReadAllBytes(levelPath));
Check(parsed is not null, "level.dat 可被 NbtLite 解析（gzip + NBT 合法）");
var data = parsed!["Data"] as Dictionary<string, object>;
Check(data is not null, "Data Compound 存在");
Check((int)data!["DataVersion"] == 3465, "DataVersion 回读");
Check(((string)data["LevelName"]).StartsWith("命令测试"), "LevelName 中文");
var version = data["Version"] as Dictionary<string, object>;
Check(version is not null && version["Name"] as string == "1.20.1", "Version.Name 回读");
Check((int)version!["Id"] == 3465, "Version.Id 回读");
Check((int)data["GameType"] == 1, "GameType=1");
Check((byte)data["allowCommands"] == 1, "allowCommands=1");
var gen = data["WorldGenSettings"] as Dictionary<string, object>;
Check(gen is not null && gen.ContainsKey("seed"), "WorldGenSettings.seed（非 split 结构）");
var dims = gen!["dimensions"] as Dictionary<string, object>;
Check(dims is not null && dims.Count == 3, "三维度");
var packs = data["DataPacks"] as Dictionary<string, object>;
Check(packs is not null, "DataPacks 存在");

// split 分支（26.1 snapshot 6+）
var jar2 = MakeJar("26.2", 4440, true, "main", splitClass: true);
var gameDir2 = Path.Combine(root, "gamedir2");
Directory.CreateDirectory(gameDir2);
var world2 = CommandWorld.Create(gameDir2, jar2);
var parsed2 = NbtLite.ParseBytes(File.ReadAllBytes(Path.Combine(world2.Path, "level.dat")));
var data2 = parsed2!["Data"] as Dictionary<string, object>;
Check(data2 is not null && data2.ContainsKey("difficulty_settings"), "split 结构带 difficulty_settings");
Check(data2 is not null && !data2.ContainsKey("WorldGenSettings"), "split 结构无 WorldGenSettings");
var dataDir = Path.Combine(world2.Path, "data", "minecraft", "world_gen_settings.dat");
Check(File.Exists(dataDir), "split 的 world_gen_settings.dat 已落盘");
var gen2 = NbtLite.ParseBytes(File.ReadAllBytes(dataDir));
Check(gen2 is not null && gen2.ContainsKey("data"), "world_gen_settings.dat 回读");

// 1.19.4 应拒绝（<1.20）
var jar3 = MakeJar("1.19.4", 3337, true, null, splitClass: false);
try { CommandWorld.Create(gameDir, jar3); Check(false, "<1.20 应拒绝"); }
catch (Exception ex) { Check(ex.Message.Contains("1.20"), "<1.20 应拒绝：" + ex.Message[..40]); }

// Supported 版本门
Check(CommandWorld.Supported("1.20") && CommandWorld.Supported("1.21.11") && !CommandWorld.Supported("1.19.4"), "Supported 版本门");

Console.WriteLine($"\n== {pass} pass, {fail} fail ==");
try { Directory.Delete(root, true); } catch { }
return fail == 0 ? 0 : 1;
