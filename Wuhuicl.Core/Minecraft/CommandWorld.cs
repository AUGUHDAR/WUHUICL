using System.IO.Compression;
using System.Security.Cryptography;
using System.Text.Json;

namespace Wuhuicl.Core.Minecraft;

/// <summary>
/// 「启动并创建命令世界」：只新建一个开着作弊的空世界存档，随启动参数直接进世界。
/// 对照 KAMUCL-1.1.10/src/main/core/commandWorld.ts 逐项翻译：
/// 从客户端 jar 读 version.json（world_version/stable/series），≤1.20 前不支持 Quick Play；
/// level.dat 按版本走两套结构（26.1 snapshot 6+ 把世界生成搬进命名空间 SavedData，
/// 以客户端 WorldGenSettings.class 是否含 SavedDataType 字节为准，不看显示 id）；
/// 种子随机，GameType=1 + allowCommands=1；不克隆任何旧存档、不动全局选项。
/// </summary>
public static partial class CommandWorld
{
    public record CreatedWorld(string Id, string Path);

    /// <summary>version.json 里世界数据需要的字段。</summary>
    sealed record ClientVersion(string Id, int WorldVersion, bool Stable, string? SeriesId);

    static ClientVersion? ReadClientVersion(string clientJar)
    {
        using var zip = new ZipArchive(File.OpenRead(clientJar));
        var entry = zip.GetEntry("version.json");
        if (entry is null) return null;
        using var s = entry.Open();
        using var doc = JsonDocument.Parse(s);
        var r = doc.RootElement;
        if (!r.TryGetProperty("world_version", out var wv) || !wv.TryGetInt32(out var worldVersion))
            return null;
        return new ClientVersion(
            r.TryGetProperty("id", out var id) ? id.GetString() ?? "" : "",
            worldVersion,
            r.TryGetProperty("stable", out var st) && st.ValueKind == JsonValueKind.True,
            r.TryGetProperty("series_id", out var se) ? se.GetString() : null);
    }

    /// <summary>Quick Play 单人需要 1.20+（supportsQuickPlayMultiplayer 的版本门）。</summary>
    public static bool Supported(string mcVersion) => VersionCompare.Compare(mcVersion, "1.20") >= 0;

    /// <summary>26.1 snapshot 6+ 检测：看客户端真实字节码，不看显示 id（含快照）。</summary>
    static bool DetectSplitFormat(string clientJar)
    {
        try
        {
            using var zip = new ZipArchive(File.OpenRead(clientJar));
            var entry = zip.GetEntry("net/minecraft/world/level/levelgen/WorldGenSettings.class");
            if (entry is null) return false;
            using var s = entry.Open();
            using var ms = new MemoryStream();
            s.CopyTo(ms);
            return Search(ms.ToArray(), "SavedDataType"u8);
        }
        catch { return false; }
    }

    static bool Search(byte[] hay, System.ReadOnlySpan<byte> needle)
    {
        for (int i = 0; i + needle.Length <= hay.Length; i++)
        {
            bool ok = true;
            for (int j = 0; j < needle.Length; j++)
                if (hay[i + j] != needle[j]) { ok = false; break; }
            if (ok) return true;
        }
        return false;
    }

    /// <summary>创建世界存档目录 + level.dat，返回（世界 id，存档路径）。</summary>
    public static CreatedWorld Create(string gameDirectory, string clientJar)
    {
        var version = ReadClientVersion(clientJar)
            ?? throw new Exception("客户端缺少真实世界数据版本，未创建存档");
        if (!Supported(version.Id)) throw new Exception("此 Minecraft 版本不支持官方单人 Quick Play（需要 1.20+）；请在游戏内新建世界并开启命令");

        var id = $"WUHUICL-Test-{DateTime.Now:yyyy-MM-ddTHH-mm-ss}-{RandomNumberGenerator.GetBytes(3)}";
        var destination = Path.Combine(gameDirectory, "saves", id);
        Directory.CreateDirectory(Path.GetDirectoryName(destination)!);
        Directory.CreateDirectory(destination);   // exclusive：已存在同名会被上面的时间戳+随机避开

        long seed = BitConverter.ToInt64(RandomNumberGenerator.GetBytes(8), 0);
        bool splitFormat = DetectSplitFormat(clientJar);
        string name = $"命令测试 {DateTime.Now:yyyy/MM/dd HH:mm:ss}";

        var root = new NbtWrite.NbtCompound()
            .Set("Data", BuildData(version, name, seed, splitFormat));

        using (var fs = File.Create(Path.Combine(destination, "level.dat")))
        using (var gzip = new GZipStream(fs, CompressionLevel.Optimal))
            NbtWrite.WriteCompound(gzip, root, rootName: "");

        if (splitFormat)
        {
            var dataDir = Path.Combine(destination, "data", "minecraft");
            Directory.CreateDirectory(dataDir);
            var gen = new NbtWrite.NbtCompound().Set("data", WorldGeneration(seed, split: true));
            using (var fs = File.Create(Path.Combine(dataDir, "world_gen_settings.dat")))
            using (var gzip = new GZipStream(fs, CompressionLevel.Optimal))
                NbtWrite.WriteCompound(gzip, gen, rootName: "");
        }
        return new CreatedWorld(id, destination);
    }

    static NbtWrite.NbtCompound BuildData(ClientVersion v, string name, long seed, bool split)
    {
        var data = new NbtWrite.NbtCompound()
            .Set("DataVersion", v.WorldVersion)
            .Set("Version", new NbtWrite.NbtCompound()
                .Set("Name", v.Id)
                .Set("Id", v.WorldVersion)
                .Set("Snapshot", v.Stable ? (byte)0 : (byte)1)
                .Set("Series", v.SeriesId ?? "main"))
            .Set("version", 19133)
            .Set("LevelName", name)
            .Set("GameType", 1)              // 创造
            .Set("allowCommands", (byte)1)   // 开命令
            .Set("hardcore", (byte)0)
            .Set("initialized", (byte)0)
            .Set("LastPlayed", DateTimeOffset.UtcNow.ToUnixTimeMilliseconds())
            .Set("Time", 0L)
            .Set("DayTime", 0L)
            .Set("DataPacks", new NbtWrite.NbtCompound()
                .Set("Enabled", NbtWrite.NbtList.Strings("vanilla"))
                .Set("Disabled", NbtWrite.NbtList.EmptyStrings));

        if (split) data.Set("difficulty_settings", new NbtWrite.NbtCompound()
            .Set("difficulty", (byte)1)
            .Set("locked", (byte)0));
        else
        {
            data.Set("Difficulty", (byte)1);
            data.Set("WorldGenSettings", WorldGeneration(seed, split: false));
            data.Set("DragonFight", new NbtWrite.NbtCompound()
                .Set("NeedsStateScanning", (byte)1)
                .Set("DragonKilled", (byte)0)
                .Set("PreviouslyKilled", (byte)0));
        }
        return data;
    }

    /// <summary>worldGeneration（commandWorld.ts:8-11）：三维度 noise 生成器。</summary>
    static NbtWrite.NbtCompound WorldGeneration(long seed, bool split)
    {
        static NbtWrite.NbtCompound Noise(string dim, string biome)
        {
            var c = new NbtWrite.NbtCompound()
            .Set("type", "minecraft:" + dim)
            .Set("generator", new NbtWrite.NbtCompound()
                .Set("type", "minecraft:noise")
                .Set("settings", "minecraft:" + biome)
                .Set("biome_source", biome == "end"
                    ? new NbtWrite.NbtCompound()
                        .Set("type", "minecraft:the_end")
                    : new NbtWrite.NbtCompound()
                        .Set("type", "minecraft:multi_noise")
                        .Set("preset", "minecraft:" + biome)));
            return c;
        }

        var gen = new NbtWrite.NbtCompound()
            .Set("seed", seed)
            .Set(split ? "generate_structures" : "generate_features", (byte)1)
            .Set("bonus_chest", (byte)0)
            .Set("dimensions", new NbtWrite.NbtCompound()
                .Set("minecraft:overworld", Noise("overworld", "overworld"))
                .Set("minecraft:the_nether", Noise("nether", "nether"))
                .Set("minecraft:the_end", Noise("the_end", "end")));
        return gen;
    }
}

/// <summary>
/// 面向命令世界 level.dat 的最小 NBT 写入端（NbtLite 只有 servers.dat 的专用 Build）：
/// Compound / List / String / Int / Long / Byte，全部大端（NBT 规范）。
/// </summary>
public static class NbtWrite
{
    public sealed class NbtCompound
    {
        public List<(byte Tag, string Name, object Value)> Items = [];
        public NbtWrite.NbtCompound Set(string name, NbtWrite.NbtCompound c) { Items.Add((10, name, c)); return this; }
        public NbtWrite.NbtCompound Set(string name, string s) { Items.Add((8, name, s)); return this; }
        public NbtWrite.NbtCompound Set(string name, int i) { Items.Add((3, name, i)); return this; }
        public NbtWrite.NbtCompound Set(string name, long l) { Items.Add((4, name, l)); return this; }
        public NbtWrite.NbtCompound Set(string name, byte b) { Items.Add((1, name, b)); return this; }
        public NbtWrite.NbtCompound Set(string name, NbtList list) { Items.Add((9, name, list)); return this; }
    }

    public sealed class NbtList
    {
        public byte ElemTag;         // 8=string, 10=compound
        public List<object> Items = [];
        public static NbtList Strings(params string[] ss) => new NbtWrite.NbtList { ElemTag = 8, Items = ss.Cast<object>().ToList() };
        public static NbtList EmptyStrings => new NbtWrite.NbtList { ElemTag = 8 };
    }

    public static void WriteCompound(Stream s, NbtWrite.NbtCompound root, string rootName)
    {
        using var w = new BinaryWriter(s, System.Text.Encoding.UTF8, leaveOpen: true);
        w.Write((byte)10); WStr(w, rootName);
        WritePayload(w, root);
    }

    static void WritePayload(BinaryWriter w, NbtWrite.NbtCompound c)
    {
        foreach (var (tag, name, value) in c.Items)
        {
            w.Write(tag); WStr(w, name);
            switch (tag)
            {
                case 8: WStr(w, (string)value); break;
                case 3: WriteBE(w, ((int)value!) & 0xFFFFFFFF, 4); break;
                case 4: WriteBE(w, (long)value!, 8); break;
                case 1: w.Write((byte)value!); break;
                case 9:
                    var l = (NbtList)value!;
                    w.Write(l.ElemTag); WriteBE(w, l.Items.Count, 4);
                    foreach (var it in l.Items)
                        if (l.ElemTag == 8) WStr(w, (string)it);
                        else WritePayload(w, (NbtCompound)it);
                    break;
                case 10: WritePayload(w, (NbtCompound)value!); break;
            }
        }
        w.Write((byte)0);   // TAG_End
    }

    static void WStr(BinaryWriter w, string s)
    {
        var b = System.Text.Encoding.UTF8.GetBytes(s);
        WriteBE(w, b.Length, 2);
        w.Write(b);
    }

    static void WriteBE(BinaryWriter w, long v, int bytes)
    {
        for (int i = bytes - 1; i >= 0; i--) w.Write((byte)(v >> (8 * i) & 0xFF));
    }
}
