using System.Text.Json;

namespace Wuhuicl.Core.Minecraft;

/// <summary>
/// 「游戏选项」默认值：定义表 + options.txt 编码 + 合并写入 + 存储。
/// 对照 KAMUCL-1.1.10/src/shared/gameOptions.ts + src/main/core/defaultGameOptions.ts 逐项翻译。
/// 只同步已修改项；编码（fov/percent/sensitivity/json…）与版本兼容性（since/until，快照感知）
/// 与原版一致；启动时按实例真实版本写入 options.txt。
/// </summary>
public static class DefaultGameOptions
{
    public sealed record GameOptionDef(
        string Id, string Label, string Page, object Initial,
        double? Min = null, double? Max = null, double? Step = null, string? Unit = null,
        string? Since = null, string? Until = null,
        (object Value, string Label)[]? Choices = null, string? Encoding = null);

    public static readonly (string Id, string Name)[] Pages =
    [
        ("skin", "皮肤自定义"), ("sounds", "音乐和声音"), ("video", "视频设置"), ("controls", "控制"),
        ("language", "语言"), ("chat", "聊天设置"), ("packs", "资源包"), ("accessibility", "辅助功能"),
        ("telemetry", "遥测数据"), ("credits", "鸣谢和著作权"),
    ];

    // 便捷构造（gameOptions.ts 的 bool/percent/integer）
    static GameOptionDef Bool(string id, string label, string page, object? initial = null, string? since = null)
        => new(id, label, page, initial ?? true, Since: since);
    static GameOptionDef Percent(string id, string label, string page, double initial = 100, string? since = null)
        => new(id, label, page, initial, 0, 100, 1, "%", since, Encoding: "percent");
    static GameOptionDef Integer(string id, string label, string page, double initial, double min, double max, string? since = null)
        => new(id, label, page, initial, min, max, 1, Since: since);

    static readonly string[] ModelParts = ["cape", "jacket", "left_sleeve", "right_sleeve", "left_pants_leg", "right_pants_leg", "hat"];
    static readonly string[] ModelPartLabels = ["披风", "上衣", "左袖", "右袖", "左裤腿", "右裤腿", "帽子"];
    static readonly string[] SoundCats = ["master", "music", "record", "weather", "block", "hostile", "neutral", "player", "ambient", "voice"];
    static readonly string[] SoundCatLabels = ["主音量", "音乐", "唱片 / 音符盒", "天气", "方块", "敌对生物", "友好生物", "玩家", "环境", "声音 / 语音"];
    static readonly (string Id, string Label)[] ToggleIds = [("toggleCrouch", "潜行"), ("toggleSprint", "疾跑"), ("autoJump", "自动跳跃"), ("showSubtitles", "显示字幕")];

    public static readonly GameOptionDef[] GameOptions =
    [
        new("fov", "视场角", "root", 70d, 30, 110, 1, "°", Encoding: "fov"),
        new("mainHand", "主手", "skin", "right", Since: "1.9", Encoding: "text",
            Choices: [(("right", "右手")), (("left", "左手"))]),
        ..ModelParts.Select((part, i) => Bool("modelPart_" + part, ModelPartLabels[i], "skin")),
        ..SoundCats.Select((cat, i) => Percent("soundCategory_" + cat, SoundCatLabels[i], "sounds")),
        Bool("showSubtitles", "显示字幕", "sounds", false, "1.9"),
        Bool("directionalAudio", "定向音频", "sounds", false, "1.19.4"),
        Integer("renderDistance", "渲染距离", "video", 12, 2, 32),
        Integer("simulationDistance", "模拟距离", "video", 12, 5, 32, "1.18"),
        Integer("maxFps", "最大帧率", "video", 120, 10, 260),
        Bool("enableVsync", "垂直同步", "video"),
        Integer("guiScale", "界面尺寸（0 为自动）", "video", 0, 0, 4),
        Percent("gamma", "亮度", "video", 50),
        Bool("bobView", "视角摇晃", "video"),
        new("particles", "粒子效果", "video", 0d, Choices: [((0d, "全部")), ((1d, "减少")), ((2d, "最少"))]),
        new("attackIndicator", "攻击指示器", "video", 1d, Since: "1.9", Choices: [((0d, "关闭")), ((1d, "准星")), ((2d, "快捷栏"))]),
        Bool("entityShadows", "实体阴影", "video", true, "1.8"),
        Integer("mipmapLevels", "Mipmap 级别", "video", 4, 0, 4),
        Bool("showAutosaveIndicator", "显示自动保存指示器", "video", true, "1.18"),
        Bool("ao", "平滑光照", "video", true, "1.20.1"),
        new("renderClouds", "云", "video", "fancy", Since: "1.19", Encoding: "json",
            Choices: [(("off", "关闭")), (("fast", "快速")), (("fancy", "精致"))]),
        new("entityDistanceScaling", "实体渲染距离", "video", 100d, 50, 500, 25, Since: "1.16", Encoding: "percent"),
        Integer("menuBackgroundBlurriness", "菜单背景模糊", "video", 5, 0, 10, "1.20.5"),
        Integer("cloudRange", "云渲染距离", "video", 128, 2, 128, "26.2"),
        Bool("cutoutLeaves", "树叶透明", "video", true, "26.2"),
        Bool("improvedTransparency", "改进透明效果", "video", true, "26.2"),
        Integer("weatherRadius", "天气半径", "video", 10, 3, 10, "26.2"),
        Bool("vignette", "暗角", "video", true, "26.2"),
        new("chunkSectionFadeInTime", "区块淡入时间", "video", 0.75d, 0, 2, 0.05, "秒", "26.2"),
        new("toggleCrouch", "潜行", "controls", false, Since: "1.15", Choices: [((false, "按住")), ((true, "切换"))]),
        new("toggleSprint", "疾跑", "controls", false, Since: "1.15", Choices: [((false, "按住")), ((true, "切换"))]),
        new("toggleAttack", "攻击 / 破坏", "controls", false, Since: "26.2", Choices: [((false, "按住")), ((true, "切换"))]),
        new("toggleUse", "使用 / 放置", "controls", false, Since: "26.2", Choices: [((false, "按住")), ((true, "切换"))]),
        Bool("autoJump", "自动跳跃", "controls", false, "1.10"),
        Integer("sprintWindow", "双击疾跑间隔（0 为关闭）", "controls", 7, 0, 10, "26.2"),
        Bool("operatorItemsTab", "管理员物品选项卡", "controls", false, "1.19.3"),
        new("mouseSensitivity", "鼠标灵敏度", "mouse", 100d, 0, 200, 1, "%", Encoding: "sensitivity"),
        new("mouseWheelSensitivity", "滚动灵敏度", "mouse", 1d, 0.01, 10, 0.01, Since: "1.14"),
        Bool("discrete_mouse_scroll", "离散滚动", "mouse", false, "1.14"),
        Bool("invertXMouse", "反转鼠标 X 轴", "mouse", false, "26.2"),
        Bool("invertYMouse", "反转鼠标 Y 轴", "mouse", false),
        Bool("allowCursorChanges", "允许更改鼠标指针", "mouse", true, "26.2"),
        new("touchscreen", "触屏模式", "mouse", false, Until: "26.1"),
        Bool("rawMouseInput", "原始输入", "mouse", true, "1.14"),
        new("lang", "语言", "language", "zh_cn", Encoding: "text", Choices:
        [
            (("zh_cn", "简体中文")), (("zh_tw", "繁體中文（台灣）")), (("zh_hk", "繁體中文（香港）")),
            (("en_us", "English (US)")), (("en_gb", "English (UK)")), (("ja_jp", "日本語")),
            (("ko_kr", "한국어")), (("de_de", "Deutsch")), (("fr_fr", "Français")),
            (("es_es", "Español")), (("ru_ru", "Русский")),
        ]),
        Bool("forceUnicodeFont", "强制使用 Unicode 字体", "language", false),
        new("chatVisibility", "聊天", "chat", 0d, Choices: [((0d, "显示")), ((1d, "仅命令")), ((2d, "隐藏"))]),
        Bool("chatColors", "聊天颜色", "chat"), Bool("chatLinks", "网页链接", "chat"),
        new("chatOpacity", "聊天不透明度", "chat", 100d, 10, 100, 1, "%", Encoding: "chatOpacity"),
        Bool("chatLinksPrompt", "链接提示", "chat"),
        Percent("chatScale", "聊天缩放", "chat"),
        new("chatWidth", "聊天宽度", "chat", 320d, 40, 320, 1, "px", Encoding: "chatWidth"),
        new("chatHeightFocused", "聊天高度（焦点）", "chat", 180d, 20, 180, 1, "px", Encoding: "chatHeight"),
        new("chatHeightUnfocused", "聊天高度（无焦点）", "chat", 90d, 20, 180, 1, "px", Encoding: "chatHeight"),
        Percent("textBackgroundOpacity", "文字背景不透明度", "chat", 50, "1.14"),
        Percent("chatLineSpacing", "聊天行间距", "chat", 0, "1.16"),
        new("narrator", "旁白", "accessibility", 0d, Since: "1.12", Choices: [((0d, "关闭")), ((1d, "全部")), ((2d, "聊天")), ((3d, "系统"))]),
        Bool("highContrast", "高对比度", "accessibility", false, "1.19.4"),
        ..ToggleIds.Select(t => new GameOptionDef(t.Id, "", "accessibility", false)),
        Percent("screenEffectScale", "失真效果", "accessibility", 100, "1.16.2"),
        Percent("fovEffectScale", "视场角效果", "accessibility", 100, "1.16.2"),
        Percent("darknessEffectScale", "黑暗效果", "accessibility", 100, "1.19"),
        Percent("glintSpeed", "附魔光效速度", "accessibility", 50, "1.19.4"),
        Percent("glintStrength", "附魔光效强度", "accessibility", 75, "1.19.4"),
        Percent("damageTiltStrength", "受伤倾斜", "accessibility", 100, "1.19.4"),
        Bool("hideLightningFlashes", "隐藏闪电闪光", "accessibility", false, "1.19.4"),
        Bool("darkMojangStudiosBackground", "深色加载画面", "accessibility", false, "1.16"),
        Percent("panoramaScrollSpeed", "全景图滚动速度", "accessibility", 100, "1.19.4"),
        Bool("highContrastBlockOutline", "高对比度方块轮廓", "accessibility", false, "1.21.2"),
        Bool("narratorHotkey", "旁白快捷键", "accessibility", true, "1.21.2"),
        Bool("telemetryOptInExtra", "发送可选遥测数据", "telemetry", false, "1.19.3"),
    ];

    /// <summary>按 id 去重（uniqueGameOptions）。</summary>
    public static readonly GameOptionDef[] Unique =
        GameOptions.GroupBy(d => d.Id).Select(g => g.First()).ToArray();

    static readonly string[] VideoOrder =
    [
        "renderDistance", "simulationDistance", "ao", "renderClouds", "particles", "mipmapLevels",
        "entityShadows", "entityDistanceScaling", "menuBackgroundBlurriness", "cloudRange", "cutoutLeaves",
        "improvedTransparency", "weatherRadius", "maxFps", "enableVsync", "guiScale", "gamma", "bobView",
        "showAutosaveIndicator", "vignette", "attackIndicator", "chunkSectionFadeInTime",
    ];

    public static bool Supported(GameOptionDef d, string version)
        => (d.Since is null || DefaultKeys.McVersionAtLeast(version, d.Since))
        && (d.Until is null || !DefaultKeys.McVersionAtLeast(version, d.Until));

    // ---------------- 校验与编码（gameOptions.ts:77-99）----------------

    public static void Validate(GameOptionDef d, object value)
    {
        if (d.Choices is not null)
        {
            if (!d.Choices.Any(c => Equals(c.Value, value))) throw new Exception("游戏选项值无效");
        }
        else if (value.GetType() != d.Initial.GetType()) throw new Exception("游戏选项值无效");
        if (value is double num)
        {
            if (!double.IsFinite(num) || num < d.Min || num > d.Max || (d.Step == 1 && num != Math.Floor(num)))
                throw new Exception("游戏选项超出有效范围");
        }
    }

    static GameOptionDef Def(string id) => Unique.FirstOrDefault(d => d.Id == id)
        ?? throw new Exception("未知游戏选项");

    public static string Encode(GameOptionDef d, object value, string version)
    {
        Validate(d, value);
        double Num(object v) => v switch
        {
            bool b => b ? 1 : 0,
            double dd => dd,
            _ => Convert.ToDouble(v, System.Globalization.CultureInfo.InvariantCulture),
        };
        switch (d.Encoding)
        {
            case "fov": return ((Num(value) - 70) / 40).ToString(System.Globalization.CultureInfo.InvariantCulture);
            case "chatOpacity": return ((Num(value) - 10) / 90).ToString(System.Globalization.CultureInfo.InvariantCulture);
            case "chatWidth": return ((Num(value) - 40) / 280).ToString(System.Globalization.CultureInfo.InvariantCulture);
            case "chatHeight": return ((Num(value) - 20) / 160).ToString(System.Globalization.CultureInfo.InvariantCulture);
            case "percent": return (Num(value) / 100).ToString(System.Globalization.CultureInfo.InvariantCulture);
            case "sensitivity": return (Num(value) / 200).ToString(System.Globalization.CultureInfo.InvariantCulture);
            case "json": return JsonSerializer.Serialize(value);
        }
        if (d.Id == "lang" && !DefaultKeys.McVersionAtLeast(version, "1.11"))
        {
            var r = new System.Text.RegularExpressions.Regex("_([a-z]+)");
            return r.Replace(value.ToString()!, m => "_" + m.Groups[1].Value.ToUpperInvariant());
        }
        if (d.Id == "mainHand" && DefaultKeys.McVersionAtLeast(version, "1.19")) return JsonSerializer.Serialize(value);
        return value switch
        {
            bool bb => bb ? "true" : "false",
            double dd => dd.ToString(System.Globalization.CultureInfo.InvariantCulture),
            _ => value.ToString() ?? "",
        };
    }

    // ---------------- 存储（defaultGameOptions.ts:8-33）----------------

    public sealed record GameOptionsState
    {
        [System.Text.Json.Serialization.JsonPropertyName("enabled")] public bool Enabled { get; init; }
        [System.Text.Json.Serialization.JsonPropertyName("values")] public Dictionary<string, object> Values { get; init; } = [];
    }

    static string StoreFile(Storage.GamePaths p) => Path.Combine(p.DataDir, "default-game-options.json");

    static object? Coerce(GameOptionDef d, JsonElement e) => e.ValueKind switch
    {
        JsonValueKind.True or JsonValueKind.False => e.GetBoolean(),
        JsonValueKind.Number => e.GetDouble(),
        JsonValueKind.String => e.GetString() ?? "",
        _ => null,
    };

    public static GameOptionsState Get(Storage.GamePaths paths)
    {
        try
        {
            using var doc = JsonDocument.Parse(File.ReadAllText(StoreFile(paths)));
            var values = new Dictionary<string, object>();
            if (doc.RootElement.TryGetProperty("values", out var vals) && vals.ValueKind == JsonValueKind.Object)
                foreach (var prop in vals.EnumerateObject())
                {
                    var d = Unique.FirstOrDefault(x => x.Id == prop.Name);
                    if (d is null) continue;
                    var v = Coerce(d, prop.Value);
                    if (v is null) continue;
                    try { Validate(d, v); values[prop.Name] = v; } catch { }
                }
            bool enabled = doc.RootElement.TryGetProperty("enabled", out var en) && en.ValueKind == JsonValueKind.True;
            return new GameOptionsState() { Enabled = enabled, Values = values };
        }
        catch { return new GameOptionsState(); }
    }

    public static GameOptionsState Set(Storage.GamePaths paths, bool? enabled = null, string? id = null, object? value = null)
    {
        var state = Get(paths);
        if (enabled is not null) state = state with { Enabled = enabled.Value };
        if (id is not null)
        {
            var values = new Dictionary<string, object>(state.Values);
            if (value is null) values.Remove(id);
            else { Validate(Def(id), value); values[id] = value; }
            state = state with { Values = values };
        }
        Storage.AtomicJson.Save(StoreFile(paths), state);
        return state;
    }

    // ---------------- 合并写入（defaultGameOptions.ts mergeGameOptions）----------------

    public static (string Text, List<string> Applied, List<string> Unsupported) Merge(
        string text, Dictionary<string, object> values, string version)
    {
        if (values.Count > 0 && (string.IsNullOrEmpty(version) || DefaultKeys.McVersionFamily(version)[0] == 999))
            throw new Exception("无法确认 Minecraft 实际版本，未写入默认游戏选项");
        var writes = new System.Collections.Generic.OrderedDictionary<string, string>();
        var unsupported = new List<string>();
        foreach (var (id, value) in values)
        {
            var d = Def(id);
            if (!Supported(d, version)) { unsupported.Add(d.Label); continue; }
            writes[id] = Encode(d, value, version);
        }
        // 游戏内的高对比度开关会同时选择内置资源包，仅写 highContrast:true 视觉包不会生效
        if (writes.IndexOf("highContrast") >= 0)
        {
            var source = text.Split(["\r\n", "\n"], StringSplitOptions.None)
                .FirstOrDefault(line => line.StartsWith("resourcePacks:", StringComparison.Ordinal))?["resourcePacks:".Length..];
            var packs = new List<string>();
            if (source is not null)
            {
                var parsed = JsonSerializer.Deserialize<JsonElement>(source);
                if (parsed.ValueKind != JsonValueKind.Array) throw new Exception("现有资源包配置无效，未覆盖 options.txt");
                packs = parsed.EnumerateArray().Select(x => x.GetString() ?? "").ToList();
            }
            packs.RemoveAll(x => x == "high_contrast");
            if (Equals(values.GetValueOrDefault("highContrast"), true)) packs.Add("high_contrast");
            writes["resourcePacks"] = JsonSerializer.Serialize(packs);
        }
        string newline = text.Contains("\r\n") ? "\r\n" : "\n";
        var seen = new HashSet<string>();
        var lines = text.Split(["\r\n", "\n"], StringSplitOptions.None);
        if (lines.Length > 0 && lines[^1].Length == 0) lines = lines[..^1];
        var outLines = new List<string>();
        foreach (var line in lines)
        {
            int i = line.IndexOf(':');
            var id = i > 0 ? line[..i] : "";
            if (id.Length == 0 || writes.IndexOf(id) < 0) { outLines.Add(line); continue; }
            if (seen.Contains(id)) continue;
            seen.Add(id);
            outLines.Add(id + ":" + writes[id]);
        }
        foreach (var (id, v) in writes)
            if (!seen.Contains(id)) outLines.Add(id + ":" + v);
        return (string.Join(newline, outLines) + (outLines.Count > 0 ? newline : ""), [.. writes.Keys], unsupported);
    }

    /// <summary>启动时同步（defaultGameOptions.ts syncDefaultGameOptions）：启用才写；原子写 + 回读校验。</summary>
    public static (List<string> Applied, List<string> Unsupported) SyncToGameDir(Storage.GamePaths paths, string gameDir, string version)
    {
        var state = Get(paths);
        if (!state.Enabled) return ([], []);
        var target = Path.Combine(gameDir, "options.txt");
        var before = File.Exists(target) ? File.ReadAllText(target) : "";
        var (text, applied, unsupported) = Merge(before, state.Values, version);
        if (before != text)
        {
            Directory.CreateDirectory(gameDir);
            var tmp = target + $".wu-{Environment.ProcessId}.tmp";
            File.WriteAllText(tmp, text);
            File.Move(tmp, target, overwrite: true);
            if (File.ReadAllText(target) != text) throw new Exception("默认游戏选项写入校验失败");
        }
        return (applied, unsupported);
    }
}
