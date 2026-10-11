using System.Text.RegularExpressions;

namespace Wuhuicl.Core.Minecraft;

/// <summary>
/// MC 原版键位表（options.txt 的 key_* 项）与默认值、启动器级默认键位存储。
/// 对照 KAMUCL-1.1.10/src/shared/keybindings.ts + src/main/core/keybindings.ts 逐项翻译。
/// 用于「默认配置」页：启动时把默认键位同步进实例 options.txt（仅 1.13+）。
/// </summary>
public static partial class DefaultKeys
{
    public sealed record KeybindDef(string Id, string Category, string Label, string DefaultBind);

    public static readonly string[] Categories = ["移动", "游戏", "物品栏", "视角", "界面", "多人游戏", "杂项"];

    public static readonly KeybindDef[] VanillaKeybinds =
    [
        // 移动
        new("key_key.forward", "移动", "前进", "key.keyboard.w"),
        new("key_key.back", "移动", "后退", "key.keyboard.s"),
        new("key_key.left", "移动", "向左移动", "key.keyboard.a"),
        new("key_key.right", "移动", "向右移动", "key.keyboard.d"),
        new("key_key.jump", "移动", "跳跃", "key.keyboard.space"),
        new("key_key.sneak", "移动", "潜行", "key.keyboard.left.shift"),
        new("key_key.sprint", "移动", "疾跑", "key.keyboard.left.control"),
        // 游戏
        new("key_key.attack", "游戏", "攻击 / 破坏", "key.mouse.left"),
        new("key_key.use", "游戏", "使用物品 / 放置方块", "key.mouse.right"),
        new("key_key.pickItem", "游戏", "选取方块", "key.mouse.middle"),
        // 物品栏
        new("key_key.inventory", "物品栏", "打开 / 关闭物品栏", "key.keyboard.e"),
        new("key_key.drop", "物品栏", "丢弃所选物品", "key.keyboard.q"),
        new("key_key.swapOffhand", "物品栏", "与副手交换物品", "key.keyboard.f"),
        new("key_key.hotbar.1", "物品栏", "快捷栏 1", "key.keyboard.1"),
        new("key_key.hotbar.2", "物品栏", "快捷栏 2", "key.keyboard.2"),
        new("key_key.hotbar.3", "物品栏", "快捷栏 3", "key.keyboard.3"),
        new("key_key.hotbar.4", "物品栏", "快捷栏 4", "key.keyboard.4"),
        new("key_key.hotbar.5", "物品栏", "快捷栏 5", "key.keyboard.5"),
        new("key_key.hotbar.6", "物品栏", "快捷栏 6", "key.keyboard.6"),
        new("key_key.hotbar.7", "物品栏", "快捷栏 7", "key.keyboard.7"),
        new("key_key.hotbar.8", "物品栏", "快捷栏 8", "key.keyboard.8"),
        new("key_key.hotbar.9", "物品栏", "快捷栏 9", "key.keyboard.9"),
        // 视角
        new("key_key.togglePerspective", "视角", "切换视角", "key.keyboard.f5"),
        new("key_key.smoothCamera", "视角", "电影视角（平滑运镜）", "key.keyboard.unknown"),
        new("key_key.zoom", "视角", "放大（望远镜）", "key.keyboard.c"),
        // 界面
        new("key_key.chat", "界面", "打开聊天栏", "key.keyboard.t"),
        new("key_key.command", "界面", "输入命令", "key.keyboard.slash"),
        new("key_key.socialInteractions", "界面", "社交屏幕", "key.keyboard.p"),
        new("key_key.advancements", "界面", "进度", "key.keyboard.l"),
        new("key_key.screenshot", "界面", "截图", "key.keyboard.f2"),
        new("key_key.fullscreen", "界面", "全屏切换", "key.keyboard.f11"),
        new("key_key.narrator", "界面", "旁白切换", "key.keyboard.b"),
        // 多人游戏
        new("key_key.playerlist", "多人游戏", "玩家列表", "key.keyboard.tab"),
        // 杂项
        new("key_key.saveToolbarActivator", "杂项", "保存快捷栏（创造模式工具）", "key.keyboard.unknown"),
        new("key_key.loadToolbarActivator", "杂项", "加载快捷栏（创造模式工具）", "key.keyboard.unknown"),
        new("key_key.spectatorOutlines", "杂项", "旁观者模式玩家轮廓", "key.keyboard.unknown"),
    ];

    [GeneratedRegex(@"^key\.(keyboard|mouse)\.[a-z0-9.]+$")]
    public static partial Regex BindRe();

    // ---------------- 存储（userData 等价 = DataDir/default-keys.json）----------------

    static string StoreFile(Storage.GamePaths p) => Path.Combine(p.DataDir, "default-keys.json");

    /// <summary>读取默认键位（无存储时用 MC 原版默认值生成全表 — keybindings.ts:17-29）。</summary>
    public static Dictionary<string, string> GetDefaultKeys(Storage.GamePaths paths)
    {
        Dictionary<string, string> stored = new();
        try
        {
            var j = System.Text.Json.JsonSerializer.Deserialize<Dictionary<string, string>>(File.ReadAllText(StoreFile(paths)));
            if (j is not null) stored = j;
        }
        catch { /* 无存储或损坏：全部回退默认 */ }
        var toLower = stored.ToDictionary(kv => kv.Key, kv => kv.Value, StringComparer.Ordinal);
        var outKeys = new Dictionary<string, string>();
        foreach (var def in VanillaKeybinds)
        {
            toLower.TryGetValue(def.Id, out var value);
            outKeys[def.Id] = value is not null && BindRe().IsMatch(value) ? value : def.DefaultBind;
        }
        return outKeys;
    }

    static void Persist(Storage.GamePaths paths, Dictionary<string, string> keys) =>
        Storage.AtomicJson.Save(StoreFile(paths), keys);

    /// <summary>设置单个默认键位（id 必须是原版键位项，value 必须是合法 MC 绑定值 — keybindings.ts:37-46）。</summary>
    public static Dictionary<string, string> SetDefaultKey(Storage.GamePaths paths, string id, string bind)
    {
        if (!VanillaKeybinds.Any(d => d.Id == id)) throw new Exception("未知的键位项");
        if (!BindRe().IsMatch(bind)) throw new Exception("无效的按键值");
        var keys = GetDefaultKeys(paths);
        keys[id] = bind;
        Persist(paths, keys);
        return keys;
    }

    /// <summary>全部恢复 MC 原版默认（keybindings.ts:49-53）。</summary>
    public static Dictionary<string, string> ResetDefaultKeys(Storage.GamePaths paths)
    {
        var keys = VanillaKeybinds.ToDictionary(d => d.Id, d => d.DefaultBind);
        Persist(paths, keys);
        return keys;
    }

    /// <summary>把默认键位合并进 options.txt 文本（纯函数）：覆盖已有 key_* 行，追加缺失项（keybindings.ts:56-76）。</summary>
    public static string MergeKeysIntoOptions(string text, Dictionary<string, string> keys)
    {
        var lines = text.Split(["\r\n", "\n"], StringSplitOptions.None);
        var seen = new HashSet<string>();
        var outLines = new List<string>();
        foreach (var line in lines)
        {
            int i = line.IndexOf(':');
            if (i <= 0)
            {
                if (line.Trim().Length > 0) outLines.Add(line);
                continue;
            }
            var key = line[..i];
            if (keys.TryGetValue(key, out var v))
            {
                outLines.Add($"{key}:{v}");
                seen.Add(key);
            }
            else outLines.Add(line);
        }
        foreach (var (key, value) in keys)
            if (!seen.Contains(key)) outLines.Add($"{key}:{value}");
        return string.Join('\n', outLines).TrimEnd('\n') + "\n";
    }

    /// <summary>启动时同步：把默认键位写进实例 options.txt。返回是否有改动（keybindings.ts:79-86）。</summary>
    public static bool SyncKeysToGameDir(string gameDir, Storage.GamePaths paths)
    {
        var keys = GetDefaultKeys(paths);
        var file = Path.Combine(gameDir, "options.txt");
        var before = File.Exists(file) ? File.ReadAllText(file) : "";
        var after = MergeKeysIntoOptions(before, keys);
        if (after == before) return false;
        File.WriteAllText(file, after);
        return true;
    }

    // ---------------- MC 版本适配（shared/keybindings.ts:136-192）----------------

    static readonly Regex SnapshotRe = new(@"^(\d{2})w(\d{1,2})[a-e]?$", RegexOptions.IgnoreCase | RegexOptions.Compiled);

    /// <summary>快照 id → (年, 周)。无法解析返回 null。</summary>
    public static (int Yy, int Ww)? ParseSnapshotId(string version)
    {
        var m = SnapshotRe.Match(version.Trim());
        return m.Success ? (int.Parse(m.Groups[1].Value), int.Parse(m.Groups[2].Value)) : null;
    }

    /// <summary>任意 MC 版本字符串 → 可比较的数值元组（版本族，注释见源 :141-149）。</summary>
    public static int[] McVersionFamily(string version)
    {
        var v = (version ?? "").Trim();
        var snap = ParseSnapshotId(v);
        if (snap is { } s)
        {
            var (yy, ww) = s;
            if (yy >= 26) return [26, 0];
            if (yy == 25) return ww >= 41 ? [1, 21, 11] : [1, 21, 10];
            if (yy == 24) return [1, 21, 4];
            if (yy == 23) return [1, 20, 4];
            if (yy == 22) return [1, 19, 3];
            if (yy == 21) return [1, 18, 2];
            if (yy == 20) return ww >= 6 ? [1, 16, 5] : [1, 15, 2];
            if (yy == 19) return ww >= 41 ? [1, 15, 2] : [1, 14, 4];
            if (yy == 18) return [1, 14, 4];
            if (yy == 17) return ww >= 43 ? [1, 13] : [1, 12, 2];
            if (yy == 16) return ww >= 20 ? [1, 10, 2] : [1, 9, 4];
            if (yy == 15) return [1, 9];
            return [1, 8];
        }
        var basePart = v.Split([' ', '-'])[0];
        var parts = basePart.Split('.').Select(p => int.TryParse(p, out var n) ? n : (int?)null).ToArray();
        if (parts.Length == 0 || parts.Any(p => p is null)) return [999];
        return parts.Select(p => p!.Value).ToArray();
    }

    /// <summary>版本族比较：-1/0/1。</summary>
    public static int CompareMcVersions(string a, string b)
    {
        var fa = McVersionFamily(a);
        var fb = McVersionFamily(b);
        for (int i = 0; i < Math.Max(fa.Length, fb.Length); i++)
        {
            int d = (i < fa.Length ? fa[i] : 0) - (i < fb.Length ? fb[i] : 0);
            if (d != 0) return Math.Sign(d);
        }
        return 0;
    }

    /// <summary>MC 版本是否 ≥ 目标。空版本按最新处理。</summary>
    public static bool McVersionAtLeast(string mcVersion, string target)
    {
        if (string.IsNullOrEmpty(mcVersion)) return true;
        return CompareMcVersions(mcVersion, target) >= 0;
    }

    /// <summary>≤1.12.2 的 options.txt 是 LWJGL2 数字 keycode，跳过键位同步（keybindings.ts:94-100）。</summary>
    public static bool KeySyncSupportedForVersion(string mcVersion) => McVersionAtLeast(mcVersion, "1.13");
}
