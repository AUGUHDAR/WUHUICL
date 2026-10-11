using System.Buffers.Binary;
using System.IO.Compression;
using System.Text;

namespace Wuhuicl.Core.Minecraft;

/// <summary>
/// 最小 NBT（Named Binary Tag）读写器，只服务服务器页的 servers.dat：读侧覆盖 0..12 全部标签
/// （gzip 头自动解压兜底），写侧只做 servers.dat 需要的子集。字段语义对照
/// KAMUCL-1.1.10/src/main/core/nbt.ts（parseNbt / buildServersDat）。
/// </summary>
public static class NbtLite
{
    const byte TagEnd = 0;
    const byte TagCompound = 10;

    /// <summary>解析 NBT 文件为根 Compound；文件不存在、损坏或根不是 Compound 返回 null。值类型映射：
    /// 基元→对应类型、String→string、ByteArray→byte[]、IntArray→int[]、LongArray→long[]、
    /// List→List&lt;object?&gt;、Compound→Dictionary&lt;string,object&gt;。</summary>
    public static Dictionary<string, object>? ParseFile(string path)
    {
        try { return File.Exists(path) ? ParseBytes(File.ReadAllBytes(path)) : null; }
        catch { return null; }
    }

    /// <summary>解析 NBT 字节；gzip 自动解压，根必须是 Compound，否则抛 InvalidDataException。</summary>
    public static Dictionary<string, object> ParseBytes(byte[] raw)
    {
        if (raw.Length > 2 && raw[0] == 0x1f && raw[1] == 0x8b)
            using (var gz = new GZipStream(new MemoryStream(raw), CompressionMode.Decompress))
            using (var outMs = new MemoryStream()) { gz.CopyTo(outMs); raw = outMs.ToArray(); }
        var r = new Reader(raw);
        if (r.ReadByte() != TagCompound) throw new InvalidDataException("NBT 根节点不是 Compound");
        r.String(); // 根名，servers.dat 恒为空串
        return r.Payload(TagCompound) as Dictionary<string, object>
            ?? throw new InvalidDataException("NBT 根节点不是 Compound");
    }

    /// <summary>servers.dat 的一个条目；icon 是原版缓存的 64×64 头像 PNG（可空）。</summary>
    public sealed record ServersDatEntry(string Name, string Ip, byte[]? Icon = null);

    /// <summary>序列化 servers.dat：根 Compound 只含 servers 一个键，即 List[Compound{name,ip,icon?}]，
    /// 与 KAMUCL 的 buildServersDat 相同；条目顺序即列表顺序。</summary>
    public static byte[] BuildServersDat(IReadOnlyList<ServersDatEntry> list)
    {
        using var ms = new MemoryStream();
        using var w = new BinaryWriter(ms, Encoding.UTF8, leaveOpen: true);
        w.Write((byte)TagCompound); WStr(w, "");               // 根 Compound + 空根名
        w.Write((byte)9); WStr(w, "servers");                  // servers: TAG_List
        w.Write((byte)TagCompound); WInt32(w, list.Count);     // 元素类型 Compound + 长度
        foreach (var e in list)
        {
            w.Write((byte)8); WStr(w, "name"); WStr(w, e.Name);
            w.Write((byte)8); WStr(w, "ip"); WStr(w, e.Ip);
            if (e.Icon is { Length: > 0 } icon)
                { w.Write((byte)7); WStr(w, "icon"); WInt32(w, icon.Length); w.Write(icon); }
            w.Write((byte)TagEnd);                             // 条目 Compound 结束
        }
        w.Write((byte)TagEnd);                                 // 根 Compound 结束
        return ms.ToArray();
    }

    /// <summary>NBT 字符串是 u16 大端字节长 + UTF-8；int 同理逐字节大端（BinaryWriter 默认小端不能用）。</summary>
    static void WStr(BinaryWriter w, string s)
    {
        var b = Encoding.UTF8.GetBytes(s);
        w.Write((byte)(b.Length >> 8)); w.Write((byte)b.Length); w.Write(b);
    }
    static void WInt32(BinaryWriter w, int v)
        { w.Write((byte)(v >> 24)); w.Write((byte)(v >> 16)); w.Write((byte)(v >> 8)); w.Write((byte)v); }
    /// <summary>大端序流式读取器；任何越界都抛 InvalidDataException，不静默截断。</summary>
    sealed class Reader(byte[] buf)
    {
        int _off;

        public byte ReadByte() { Ensure(1); return buf[_off++]; }

        public string String()
        {
            Ensure(2);
            int len = BinaryPrimitives.ReadUInt16BigEndian(buf.AsSpan(_off)); _off += 2;
            Ensure(len);
            var s = Encoding.UTF8.GetString(buf, _off, len); _off += len;
            return s;
        }

        /// <summary>按标签号读载荷（tag 0 是 Compound 内的结束符，只在 ReadCompound 处理）。</summary>
        public object Payload(byte t) => t switch
        {
            1 => ReadByte(),
            2 => Read(2, s => BinaryPrimitives.ReadInt16BigEndian(s)),
            3 => Read(4, s => BinaryPrimitives.ReadInt32BigEndian(s)),
            4 => Read(8, s => BinaryPrimitives.ReadInt64BigEndian(s)),
            5 => Read(4, s => BinaryPrimitives.ReadSingleBigEndian(s)),
            6 => Read(8, s => BinaryPrimitives.ReadDoubleBigEndian(s)),
            7 => PrimArray(1, p => p[0]),
            8 => String(),
            9 => ReadList(),
            10 => ReadCompound(),
            11 => PrimArray(4, s => BinaryPrimitives.ReadInt32BigEndian(s)),
            12 => PrimArray(8, s => BinaryPrimitives.ReadInt64BigEndian(s)),
            _ => throw new InvalidDataException($"不支持的 NBT 类型 {t}"),
        };

        object Read<T>(int size, Func<ReadOnlySpan<byte>, T> parse)
            { Ensure(size); var v = parse(buf.AsSpan(_off, size)); _off += size; return v!; }

        // TAG_Byte/Int/Long_Array：int 元素数 + 定长载荷
        object PrimArray<T>(int elemSize, Func<ReadOnlySpan<byte>, T> parse)
        {
            int count = Len(elemSize);
            Ensure(count * elemSize);
            var a = new T[count];
            for (int i = 0; i < count; i++) a[i] = parse(buf.AsSpan(_off + i * elemSize));
            _off += count * elemSize;
            return a;
        }
        object ReadList()
        {
            byte itemType = ReadByte();
            int count = Len(1);
            var list = new List<object?>(Math.Min(count, 1024));
            for (int i = 0; i < count; i++) list.Add(itemType == TagEnd ? null : Payload(itemType));
            return list;
        }

        object ReadCompound()
        {
            var dict = new Dictionary<string, object>();
            while (true)
            {
                byte t = ReadByte();
                if (t == TagEnd) return dict;
                dict[String()] = Payload(t);
            }
        }

        /// <summary>TAG_List / TAG_Int 长度头：负数或超 100 万视为坏文件（nbt.ts 的 collectionLength）。</summary>
        int Len(int elemSize)
        {
            Ensure(4);
            int count = BinaryPrimitives.ReadInt32BigEndian(buf.AsSpan(_off));
            _off += 4;
            if (count < 0 || (long)count * elemSize > 1_000_000) throw new InvalidDataException($"NBT 集合长度异常: {count}");
            return count;
        }

        void Ensure(int n) { if (n < 0 || _off + n > buf.Length) throw new InvalidDataException("NBT 数据被截断或长度无效"); }
    }
}
