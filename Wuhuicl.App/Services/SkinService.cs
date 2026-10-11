using System.IO;
using System.Security.Cryptography;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Windows.Media.Imaging;
using Wuhuicl.Core;
using Wuhuicl.Core.Storage;

namespace Wuhuicl.App.Services;

/// <summary>skins.json 里的一条历史记录：{id, variant, time, name}（id = 内容 SHA1，去重键）。</summary>
public sealed class SkinHistoryEntry
{
    [JsonPropertyName("id")] public string Id { get; set; } = "";
    /// <summary>classic | slim（skin-render.ts:28-40 的右臂背面判法）。</summary>
    [JsonPropertyName("variant")] public string Variant { get; set; } = "classic";
    [JsonPropertyName("time")] public long Time { get; set; }   // unix ms，最近使用时间
    [JsonPropertyName("name")] public string Name { get; set; } = "";  // 导入时的原始文件名
}

/// <summary>
/// 皮肤页后端：当前皮肤 = exe 旁 Assets\skin.png（SkinRenderer.LoadAsset 对磁盘文件优先于内嵌资源，
/// 所以把导入/换回的文件复制到这个固定路径再 SkinSource(reload:true)，3D 预览与头像全部跟上，无需改 SkinRenderer）。
/// 导入：选 PNG → 校验 64×64 → 复制到 &lt;DataDir&gt;\skins\&lt;sha1&gt;.png → skins.json 历史（上限 30，SHA1 去重）。
/// 换回：从历史文件复制回当前路径并刷新。历史读写用原子替换（坏文件不覆盖好文件）。
/// </summary>
public static class SkinService
{
    const int MaxHistory = 30;

    static GamePaths Paths => GamePaths.DetectDefault();
    public static string SkinsDir => Path.Combine(Paths.DataDir, "skins");
    public static string HistoryFile => Path.Combine(Paths.DataDir, "skins.json");

    /// <summary>当前皮肤的固定落点：SkinRenderer 读的就是它（LoadAsset 的磁盘分支优先）。</summary>
    public static string CurrentSkinPath => Path.Combine(AppContext.BaseDirectory, "Assets", "skin.png");

    // ---------------- 读 ----------------

    /// <summary>读 skins.json；文件不存在返回空表，损坏返回空表并保留坏文件（补 .bad- 时间戳副本）。</summary>
    public static List<SkinHistoryEntry> LoadHistory()
    {
        try
        {
            if (!File.Exists(HistoryFile)) return [];
            var list = JsonSerializer.Deserialize<List<SkinHistoryEntry>>(File.ReadAllText(HistoryFile));
            return list ?? [];
        }
        catch
        {
            try { File.Copy(HistoryFile, HistoryFile + ".bad-" + DateTime.Now.ToString("yyyyMMddHHmmss"), true); }
            catch { /* 尽力而为 */ }
            return [];
        }
    }

    public static string SkinPathOf(string id) => Path.Combine(SkinsDir, id + ".png");

    // ---------------- 写 ----------------

    /// <summary>
    /// 导入一张皮肤：校验 64×64 PNG → 复制到 &lt;DataDir&gt;\skins\&lt;sha1&gt;.png →
    /// 历史去重置顶（重导已有皮肤只刷新时间与名字）→ 设为当前皮肤。
    /// 成功返回 null，失败返回错误文案（给 MessageBox）。
    /// </summary>
    public static string? ImportSkin(string sourcePath)
    {
        if (!File.Exists(sourcePath)) return "文件不存在";
        if (!string.Equals(Path.GetExtension(sourcePath), ".png", StringComparison.OrdinalIgnoreCase))
            return "只支持 PNG 皮肤文件";

        byte[] bytes;
        try { bytes = File.ReadAllBytes(sourcePath); }
        catch (Exception ex) { return $"读取失败（{ex.Message}）"; }

        if (!TryDecode(bytes, out var bmp)) return "不是有效的 PNG 图片";
        if (bmp!.PixelWidth != 64 || bmp.PixelHeight != 64)
            return $"皮肤必须是 64×64（当前 {bmp.PixelWidth}×{bmp.PixelHeight}）";

        string id = Convert.ToHexString(SHA1.HashData(bytes)).ToLowerInvariant();
        try
        {
            Directory.CreateDirectory(SkinsDir);
            string store = SkinPathOf(id);
            if (!File.Exists(store)) File.WriteAllBytes(store, bytes);

            var history = LoadHistory();
            history.RemoveAll(h => h.Id == id); // SHA1 去重：置顶并刷新元数据
            history.Insert(0, new SkinHistoryEntry
            {
                Id = id,
                Variant = DetectVariant(bmp),
                Time = DateTimeOffset.Now.ToUnixTimeMilliseconds(),
                Name = Path.GetFileName(sourcePath),
            });
            if (history.Count > MaxHistory)
            {
                // 超限的条目出列，存储文件没人引用了就一并清掉
                foreach (var dropped in history.Skip(MaxHistory))
                    TryDelete(SkinPathOf(dropped.Id));
                history = history.Take(MaxHistory).ToList();
            }
            SaveHistory(history);
        }
        catch (Exception ex)
        {
            return $"保存失败（{ex.Message}）";
        }
        return SetCurrentFile(SkinPathOf(id));
    }

    /// <summary>换回历史皮肤：把 &lt;DataDir&gt;\skins\&lt;id&gt;.png 复制到当前皮肤路径并刷新缓存；成功 null。</summary>
    public static string? ApplySkin(string id)
    {
        string file = SkinPathOf(id);
        if (!File.Exists(file)) return $"皮肤文件已不存在：{id}";
        var history = LoadHistory();
        var hit = history.FirstOrDefault(h => h.Id == id);
        if (hit is not null)
        {
            history.RemoveAll(h => h.Id == id);
            hit.Time = DateTimeOffset.Now.ToUnixTimeMilliseconds();
            history.Insert(0, hit);
            SaveHistory(history);
        }
        return SetCurrentFile(file);
    }

    /// <summary>删除历史条目与对应存储文件；当前正用的皮肤也允许删（文件已复制到当前路径，不受影响）。成功 null。</summary>
    public static string? RemoveSkin(string id)
    {
        try
        {
            var history = LoadHistory();
            history.RemoveAll(h => h.Id == id);
            SaveHistory(history);
            TryDelete(SkinPathOf(id));
            return null;
        }
        catch (Exception ex)
        {
            return $"删除失败（{ex.Message}）";
        }
    }

    /// <summary>把皮肤文件复制到 exe 旁的固定当前路径，并让 SkinRenderer 重读（3D 预览/头像全部跟上）。</summary>
    static string? SetCurrentFile(string file)
    {
        try
        {
            Directory.CreateDirectory(Path.GetDirectoryName(CurrentSkinPath)!);
            File.Copy(file, CurrentSkinPath, overwrite: true);
            SkinRenderer.SkinSource(reload: true);
            return null;
        }
        catch (Exception ex)
        {
            return $"应用皮肤失败（{ex.Message}）";
        }
    }

    static void SaveHistory(List<SkinHistoryEntry> history)
    {
        Directory.CreateDirectory(Path.GetDirectoryName(HistoryFile)!);
        string tmp = HistoryFile + ".tmp-" + Guid.NewGuid().ToString("N")[..8];
        File.WriteAllText(tmp, JsonSerializer.Serialize(history, Json.Options));
        if (File.Exists(HistoryFile)) File.Replace(tmp, HistoryFile, HistoryFile + ".bak");
        else File.Move(tmp, HistoryFile);
    }

    // ---------------- 小件 ----------------

    /// <summary>解码 PNG 字节，坏图返回 false。必须 CacheOption.OnLoad，流关掉后位图仍可用。</summary>
    static bool TryDecode(byte[] png, out BitmapImage? bmp)
    {
        bmp = null;
        try
        {
            var img = new BitmapImage();
            using var ms = new MemoryStream(png);
            img.BeginInit();
            img.CacheOption = BitmapCacheOption.OnLoad;
            img.StreamSource = ms;
            img.EndInit();
            img.Freeze();
            bmp = img;
            return true;
        }
        catch { return false; }
    }

    /// <summary>slim（Alex）判定：classic 右臂背面右缘一列（x=54, y=20..31）有像素 → classic（skin-render.ts:28-40，
    /// 与 SkinRenderer.IsSlimSkin 同一判法；这里是对任意文件判，不动 SkinRenderer 的当前皮肤缓存）。</summary>
    public static string DetectVariant(BitmapSource skin)
    {
        try
        {
            var src = new FormatConvertedBitmap(skin, System.Windows.Media.PixelFormats.Bgra32, null, 0);
            var px = new byte[src.PixelWidth * src.PixelHeight * 4];
            src.CopyPixels(px, src.PixelWidth * 4, 0);
            for (var y = 20; y < 32; y++)
            {
                var i = (y * src.PixelWidth + 54) * 4;
                if (px[i + 3] != 0) return "classic";
            }
            return "slim";
        }
        catch { return "classic"; }
    }

    /// <summary>相对时间（历史条目的 fmtTime）：刚刚 / n 分钟前 / n 小时前 / n 天前 / 落到日期。</summary>
    public static string FmtTime(long ms)
    {
        var t = DateTimeOffset.FromUnixTimeMilliseconds(ms).LocalDateTime;
        var d = DateTime.Now - t;
        if (d.TotalMinutes < 1) return "刚刚";
        if (d.TotalMinutes < 60) return $"{(int)d.TotalMinutes} 分钟前";
        if (d.TotalHours < 24) return $"{(int)d.TotalHours} 小时前";
        if (d.TotalDays < 30) return $"{(int)d.TotalDays} 天前";
        return t.ToString("yyyy/M/d");
    }

    static void TryDelete(string path)
    {
        try { if (File.Exists(path)) File.Delete(path); } catch { /* 尽力而为 */ }
    }
}
