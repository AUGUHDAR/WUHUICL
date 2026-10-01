using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace Wuhuicl.Core;

public static class Json
{
    public static readonly JsonSerializerOptions Options = new()
    {
        WriteIndented = true,
        Encoder = System.Text.Encodings.Web.JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
        PropertyNameCaseInsensitive = true,
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
    };

    public static string Sha1Hex(byte[] data)
    {
        var hash = SHA1.HashData(data);
        return Convert.ToHexString(hash).ToLowerInvariant();
    }

    public static async Task<string> Sha1HexOfFileAsync(string path, CancellationToken ct)
    {
        await using var fs = File.OpenRead(path);
        var hash = await SHA1.HashDataAsync(fs, ct);
        return Convert.ToHexString(hash).ToLowerInvariant();
    }

    public static string Md5Hex(string text) =>
        Convert.ToHexString(MD5.HashData(Encoding.UTF8.GetBytes(text))).ToLowerInvariant();

    /// <summary>
    /// 离线模式 UUID：MD5("OfflinePlayer:" + name)，置版本位 3 与变体位 8。
    /// 与原版服务端算法一致，保证同名玩家的存档归属不变。
    /// </summary>
    public static string OfflineUuid(string name)
    {
        var hex = Md5Hex("OfflinePlayer:" + name);
        var chars = hex.ToCharArray();
        chars[12] = '3';
        // 变体位必须由 MD5 自身派生：随机化会让同名玩家每次启动换 UUID，
        // 原版服务端按 UUID 认人，结果就是"背包和位置全没了"。
        chars[16] = "89ab"[Convert.ToInt32(chars[16].ToString(), 16) & 3];
        return $"{new string(chars, 0, 8)}-{new string(chars, 8, 4)}-" +
               $"{new string(chars, 12, 4)}-{new string(chars, 16, 4)}-{new string(chars, 20, 12)}";
    }

    /// <summary>去掉横线，启动参数里 yggdrasil 类型要 32 位形式。</summary>
    public static string StripDashes(string uuid) => uuid.Replace("-", "");
}

/// <summary>版本号比较：1.20.10 大于 1.20.4；预发布串排在正式版之后。</summary>
public static class VersionCompare
{
    public static int Compare(string? a, string? b)
    {
        if (string.Equals(a, b, StringComparison.Ordinal)) return 0;
        if (string.IsNullOrEmpty(a)) return -1;
        if (string.IsNullOrEmpty(b)) return 1;

        var pa = Split(a);
        var pb = Split(b);
        for (int i = 0; i < Math.Max(pa.Count, pb.Count); i++)
        {
            var sa = i < pa.Count ? pa[i] : "";
            var sb = i < pb.Count ? pb[i] : "";
            bool na = long.TryParse(sa, out var va) && sa.All(char.IsDigit);
            bool nb = long.TryParse(sb, out var vb) && sb.All(char.IsDigit);
            int r;
            if (na && nb) r = va.CompareTo(vb);
            else if (na) r = 1;              // 数字段比字母段"新"
            else if (nb) r = -1;
            else r = string.CompareOrdinal(sa, sb);
            if (r != 0) return r;
        }
        return 0;
    }

    static List<string> Split(string v)
    {
        var list = new List<string>();
        var buf = new StringBuilder();
        foreach (var ch in v)
        {
            if (ch is '.' or '-' or '_' or '+')
            {
                if (buf.Length > 0) { list.Add(buf.ToString()); buf.Clear(); }
            }
            else buf.Append(ch);
        }
        if (buf.Length > 0) list.Add(buf.ToString());
        return list;
    }

    public static IEnumerable<string> SortDescending(IEnumerable<string> ids) =>
        ids.OrderByDescending(x => x, Comparer<string>.Create(Compare));
}
