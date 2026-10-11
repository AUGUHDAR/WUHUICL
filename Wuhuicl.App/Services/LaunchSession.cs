using System.Collections.ObjectModel;
using System.ComponentModel;
using System.IO;
using System.Runtime.CompilerServices;
using System.Windows;
using Wuhuicl.Core;
using Wuhuicl.Core.Minecraft;
using Wuhuicl.Core.Storage;
using Wuhuicl.Core.Stores;

namespace Wuhuicl.App.Services;

public sealed class VersionRow
{
    public string Id { get; init; } = "";
    public string LastPlayed { get; set; } = "";
    public string Label { get; init; } = "";
    public string Kind { get; init; } = "release";
    public bool Installed { get; set; }
    public string Subtitle { get; init; } = "";
    /// <summary>收藏标记（菜单行的星标 v-if isFavorite — HomeView.vue:682），随 ResortInstalledByFavorite 刷新。</summary>
    public bool Favorite { get; set; }
    /// <summary>instance-meta 四段（GameView.vue:1158-1163）：MC 版本 / 加载器 / 目录短名 / 隔离状态。</summary>
    public string McVersion { get; set; } = "";
    public string LoaderLabel { get; set; } = "";
    public string IsolatedText { get; set; } = "已隔离";

    /// <summary>标签文字＝原版 typeText（GameView.vue:344-349）。之前直接把 'release' 显示在标签上。</summary>
    public string KindText => Kind switch
    {
        "release" => "正式版",
        "snapshot" => "快照",
        "old_beta" => "Beta 旧版",
        "old_alpha" => "Alpha 旧版",
        _ => Kind,
    };

    /// <summary>标签配色＝原版 typeTagClass（:352-353）：release 走 tag-gold、snapshot 走 tag-cyan、
    /// 其余是 .tag 基态。给 BrushByKey 当键名用，避免在 XAML 里堆三层 DataTrigger。</summary>
    public string KindTagBg => Kind == "release" ? "AccentSoft" : Kind == "snapshot" ? "CyanSoft" : "ChipBg";
    public string KindTagFg => Kind == "release" ? "AccentHi" : Kind == "snapshot" ? "Cyan" : "Fg2";
    public override string ToString() => Label;
}

public sealed class AccountRow
{
    public Account Model { get; init; } = new();
    /// <summary>头像：微软号用自己的档案皮肤脸（facecache），其余用全局皮肤文件现算。</summary>
    public System.Windows.Media.ImageSource Avatar => SkinRenderer.FaceFor(string.IsNullOrEmpty(Model.Uuid) ? null : Model.Uuid, 64);
    public string Name => Model.Name;
    public string KindText => Model.Kind switch
    {
        "microsoft" => "微软正版",
        "yggdrasil" => "外置登录",
        _ => "离线",
    };
    public override string ToString() => $"{Name}（{KindText}）";
}

/// <summary>
/// 视图与引擎之间的唯一一层。界面只调这里的命令、绑这里的集合，
/// 不直接 new Downloader / Process —— 这样换皮肤或加页面不会把引擎调用散进 XAML。
/// </summary>
public sealed class LaunchSession : INotifyPropertyChanged
{
    public GamePaths Paths { get; }
    public SettingsStore Settings { get; }
    public AccountStore Accounts { get; }
    public InstallService Installer { get; }
    readonly Downloader _dl = new();

    public ObservableCollection<AccountRow> AccountList { get; } = new();
    public ObservableCollection<VersionRow> InstalledList { get; } = new();
    public ObservableCollection<VersionRow> OnlineList { get; } = new();
    public ObservableCollection<string> LogLines { get; } = new();

    AccountRow? _account;
    public AccountRow? SelectedAccount { get => _account; set { if (Set(ref _account, value)) { if (value is not null) Accounts.Select(value.Model.Id); SaveSettings(); } } }

    VersionRow? _installed;
    public VersionRow? SelectedInstalled { get => _installed; set { if (Set(ref _installed, value)) { Settings.Value.LastVersion = value?.Id; SaveSettings(); RaiseRunState(); } } }

    VersionRow? _online;
    public VersionRow? SelectedOnline { get => _online; set => Set(ref _online, value); }

    string _status = "准备就绪";
    public string Status { get => _status; set => Set(ref _status, value); }

    double _progress;
    public double Progress { get => _progress; set => Set(ref _progress, value); }

    bool _busy;
    public bool Busy { get => _busy; set { if (Set(ref _busy, value)) RaiseRunState(); } }

    bool _gameRunning;
    public bool GameRunning { get => _gameRunning; set { if (Set(ref _gameRunning, value)) { Raise(nameof(CanPlay)); Raise(nameof(CanStop)); RaiseRunState(); } } }

    /// <summary>原版 launchFailed（HomeView.vue:161-164）：launchState 为 error，或退出码非 0。
    /// 它同时驱动首页启动按钮文案（:171）、.runtime-state.error 这一档色调（:883-884）
    /// 和日志窗顶部的 .log-failure 条（:717）。</summary>
    bool _launchFailed;
    public bool LaunchFailed { get => _launchFailed; set { if (Set(ref _launchFailed, value)) RaiseRunState(); } }

    public bool CanPlay => !Busy && !GameRunning && SelectedAccount is not null && SelectedInstalled is not null;
    public bool CanStop => GameRunning;
    public bool HasNoAccount => AccountList.Count == 0;

    public string RuntimeSummary => Settings.Value.AutoJava ? "自动选择" :
        (string.IsNullOrEmpty(Settings.Value.JavaPath) ? "自动选择"
            : Path.GetFileNameWithoutExtension(Settings.Value.JavaPath));
    // memoryText（HomeView.vue:310-316）与 memoryValue（SettingsView.vue:426-429）：
    // 自动 → 「自动」/「自动（N GB）」；手动 → N GB（0.5GB 步进不会出现 .5 以外的零头）
    public string MemorySummary => Settings.Value.MemoryAuto
        ? $"自动（{FmtGb(MemoryPolicy.AutoMemoryMB(TotalMemMb))}）"
        : FmtGb(Settings.Value.MemoryMb);
    public string MemoryText => Settings.Value.MemoryAuto ? "自动" : FmtGb(Settings.Value.MemoryMb);
    static string FmtGb(int mb) => mb % 1024 == 0 ? $"{mb / 1024} GB" : $"{mb / 1024.0:0.#} GB";

    /// <summary>本机物理内存 MB（totalMemMB — os.totalmem() 等价，GC 报告的物理上限）。</summary>
    public static long TotalMemMb => GC.GetGCMemoryInfo().TotalAvailableMemoryBytes / 1024 / 1024;
    // 原版 heroStatus 五态（HomeView.vue:192-200），判据顺序也照抄：
    // 没有当前实例 → 等待选择；运行中 → 游戏运行中；正在准备 → 正在准备；
    // launchFailed／实例损坏 → 需要检查；否则就绪。
    // 「没有当前实例」原版查的是 currentVersion（= selectedInstance，:61），不是「一个版本都没装」，
    // 所以这里用 SelectedInstalled 为 null，和 InstalledList.Count==0 只在空列表时等价。
    // 缺的两档（version.failed / version.incomplete，:197）要等实例体检接入才能补，暂时只有 launchFailed。
    public string RunStateText => SelectedInstalled is null ? "等待选择" : GameRunning ? "游戏运行中"
        : Busy ? "正在准备" : LaunchFailed ? "需要检查" : "就绪";

    /// <summary>.runtime-state i 的点色：idle 灰、ready/running 绿、error 红（HomeView.vue:881-884），
    /// 不是 accent —— 之前空状态点成蓝色就是这里错了。error 优先级最低：启动过程中不算需要检查。</summary>
    public string RunStateColor => SelectedInstalled is null ? "Idle"
        : !GameRunning && !Busy && LaunchFailed ? "Danger" : "Ok";

    /// <summary>ready/running 才有 3px 光环 box-shadow 0 0 0 3px ok-soft（:882），
    /// error 换成 danger-soft（:884），idle 没有光环。</summary>
    public string RunStateRing => RunStateColor switch
    {
        "Ok" => "OkSoft",
        "Danger" => "DangerSoft",
        _ => "Transparent",
    };

    /// <summary>.runtime-state strong 的字色：默认 accent-2，error 档转 danger（:880 与 :883）。
    /// 前面的 svg 图标两档都是 accent-2，所以只有文字这一处要跟着 tone 走。</summary>
    public string RunStateFg => RunStateColor == "Danger" ? "Danger" : "AccentHi";

    void RaiseRunState()
    {
        Raise(nameof(RunStateText));
        Raise(nameof(RunStateColor));
        Raise(nameof(RunStateRing));
        Raise(nameof(RunStateFg));
        Raise(nameof(LogHeadText));
    }

    /// <summary>原版 launchText（HomeView.vue:169-172）四档：launching 用进度文案，这里落回它的
    /// 兜底句「正在启动…」；running 是「再次启动」；launchFailed 才是「重新启动」；都没问题就「开始游戏」。
    /// 之前只有两档，还把 running 写成了「重新启动」。</summary>
    public string LaunchText => Busy ? "正在启动…" : GameRunning ? "再次启动"
        : LaunchFailed ? "重新启动" : "开始游戏";

    /// <summary>日志窗 header 的「N 行 · 运行状态」（HomeView.vue:714）。
    /// WPF 绑不到 ObservableCollection.Count 的变更，所以按原版 computed 的写法做成属性，改日志时手动 Raise。</summary>
    public string LogHeadText => $"{LogLines.Count} 行 · {RunStateText}";

    /// <summary>.log-empty 的 v-if 是 !store.logs.length（:722），footer 清空的 disabled 是同一个条件（:725）。</summary>
    public bool NoLogs => LogLines.Count == 0;
    public bool CanClearLogs => LogLines.Count > 0;

    void RaiseLogs()
    {
        Raise(nameof(NoLogs));
        Raise(nameof(CanClearLogs));
        Raise(nameof(LogHeadText));
    }

    /// <summary>对应原版「清空日志」（HomeView.vue:725 store.logs = []）。</summary>
    public void ClearLogs()
    {
        LogLines.Clear();
        RaiseLogs();
    }

    /// <summary>版本页第二个页签：原版是「已安装」，有实例时才带数量（GameView.vue:937）。</summary>
    public string InstalledTabText => InstalledList.Count == 0 ? "已安装" : $"已安装（{InstalledList.Count}）";

    GameLauncher? _launcher;
    CancellationTokenSource? _cts;

    /// <summary>每个版本的最后启动时间（毫秒）。原版按实例记录，这里同样持久化。</summary>
    readonly Dictionary<string, long> _playTimes = new(StringComparer.Ordinal);
    string PlayTimeFile => Path.Combine(Paths.DataDir, "play-times.json");

    void LoadPlayTimes()
    {
        try
        {
            if (File.Exists(PlayTimeFile))
            {
                var d = System.Text.Json.JsonSerializer.Deserialize<Dictionary<string, long>>(
                    File.ReadAllText(PlayTimeFile));
                if (d is not null) foreach (var kv in d) _playTimes[kv.Key] = kv.Value;
            }
        }
        catch { _playTimes.Clear(); }
    }

    void SavePlayTime(string id)
    {
        _playTimes[id] = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
        try
        {
            AtomicJson.Save(PlayTimeFile, _playTimes);
        }
        catch (Exception ex) { Log("游玩时间保存失败：" + ex.Message); }
        RebuildRecent();   // 刚玩过的排到最近游戏首位
    }

    string LastPlayedText(string id)
    {
        if (!_playTimes.TryGetValue(id, out var ms)) return "未启动过";
        var days = (int)((DateTimeOffset.UtcNow.ToUnixTimeMilliseconds() - ms) / 86400000L);
        if (days <= 0) return "今天玩过";
        if (days == 1) return "昨天玩过";
        return days < 30 ? $"上次游玩：{days}天前" : $"上次游玩：{days / 30}个月前";
    }

    public LaunchSession()
    {
        Paths = GamePaths.DetectDefault();
        Paths.EnsureDirectories();
        Settings = new SettingsStore(Paths);
        Settings.Load();
        // 旧 min/max → memoryMB 一次性迁移（取上限当手动值；memoryAuto 缺省即 true 不用动）
        if (Settings.Value.MemoryMb == 4096 && Settings.Value.MaxHeapMb > 0 && Settings.Value.MaxHeapMb != 2048)
        {
            Settings.Value.MemoryMb = Settings.Value.MaxHeapMb;
        }
        Accounts = new AccountStore(Paths);
        Accounts.Load();
        Installer = new InstallService(Paths)
        {
            UseMirror = Settings.Value.UseMirror,
            MirrorRoot = Settings.Value.MirrorRoot,
            Progress = new Progress<string>(s => Dispatcher(() => Log(s))),
        };
        Installer.DL.PreferMirrorFirst = Settings.Value.UseMirror;
        SkinRenderer.FaceCacheDir = Path.Combine(Paths.DataDir, "facecache");
        LoadPlayTimes();
        ReloadAccounts();
        RefreshInstalled();
    }

    public static void Dispatcher(Action a)
    {
        var app = Application.Current;
        if (app is null || app.Dispatcher.CheckAccess()) a();
        else app.Dispatcher.Invoke(a);
    }

    public void Log(string line)
    {
        Dispatcher(() =>
        {
            LogLines.Add(line);
            while (LogLines.Count > 2000) LogLines.RemoveAt(0);
            RaiseLogs();
        });
    }

    // ---------------- 账号 ----------------
    public void ReloadAccounts()
    {
        AccountList.Clear();
        foreach (var a in Accounts.Accounts) AccountList.Add(new AccountRow { Model = a });
        var sel = AccountList.FirstOrDefault(x => x.Model.Id == Accounts.Selected?.Id) ?? AccountList.FirstOrDefault();
        SelectedAccount = sel;
        Raise(nameof(HasNoAccount));
        Raise(nameof(CanPlay));
    }

    public string? AddOfflineAccount(string name)
    {
        var err = Account.ValidateName(name);
        if (err is not null) return err;
        try
        {
            Accounts.AddOffline(name);
            ReloadAccounts();
            Status = $"已添加离线账号 {name}";
            return null;
        }
        catch (Exception ex) { return ex.Message; }
    }

    // ---------------- 版本 ----------------
    public void RefreshInstalled()
    {
        InstalledList.Clear();
        try
        {
            foreach (var dir in Directory.EnumerateDirectories(Paths.Versions))
            {
                var id = Path.GetFileName(dir);
                var json = Path.Combine(dir, id + ".json");
                if (!File.Exists(json)) continue;
                string kind = "release", inherit = "";
                try
                {
                    using var doc = System.Text.Json.JsonDocument.Parse(File.ReadAllText(json));
                    kind = doc.RootElement.TryGetProperty("type", out var t) ? t.GetString() ?? "release" : "release";
                    inherit = doc.RootElement.TryGetProperty("inheritsFrom", out var i) ? i.GetString() ?? "" : "";
                }
                catch { }
                InstalledList.Add(new VersionRow
                {
                    Id = id,
                    Label = string.IsNullOrEmpty(inherit) ? id : $"{id}",
                    Kind = string.IsNullOrEmpty(inherit) ? kind : "loader",
                    Installed = true,
                    Subtitle = string.IsNullOrEmpty(inherit) ? DescribeKind(kind) : "基于 " + inherit,
                    McVersion = string.IsNullOrEmpty(inherit) ? id : inherit,
                    LoaderLabel = LoaderLabelOf(id, kind),
                    IsolatedText = Settings.Value.GameDirOverride is null && Settings.Value.DefaultIsolation
                        ? "已隔离" : "共享目录",
                });
                InstalledList[^1].LastPlayed = "";
            }
        }
        catch (Exception ex) { Log("读取已安装版本失败：" + ex.Message); }
        // 原地重排：集合实例被界面绑定持有，换掉实例会让 ListBox 失去通知
        foreach (var v in InstalledList) v.LastPlayed = LastPlayedText(v.Id);
        Raise(nameof(InstalledList));
        SelectedInstalled ??= InstalledList.FirstOrDefault();
        ResortInstalledByFavorite();   // GameView.vue:680 sortedInstalled = sortWithFavorite(...)：收藏优先 + 最近游玩降序
        RebuildRecent();
        Raise(nameof(CanPlay));
        RaiseRunState();
        Raise(nameof(InstalledTabText));
    }

    // ---------------- 最近游戏（HomeView.vue:373-381）----------------
    // 原版 recent = sortWithFavorite(activeInstalled).slice(0, wideRecent ? 8 : 4)：
    // 收藏优先，组内按 lastPlayed 时间戳降序；宽度 ≥1500 取 8 条、否则 4 条（HomeView.vue:376）。
    public ObservableCollection<VersionRow> RecentList { get; } = new();

    /// <summary>首页在 SizeChanged 时按窗口宽更新（≥1500 → 8，否则 4）。</summary>
    public int RecentCap { get; set; } = 8;

    /// <summary>isFavorite（store.ts:315）：无文件夹概念，落 favoriteVersions 支路。</summary>
    public bool IsFavorite(string id) => Settings.Value.FavoriteVersions.Contains(id);

    /// <summary>toggleFavorite（store.ts:318-325）：翻转后落盘并重排（星标与排序即时反馈）。</summary>
    public bool ToggleFavorite(string id)
    {
        var favs = Settings.Value.FavoriteVersions;
        bool now = !favs.Contains(id);
        if (now) favs.Add(id); else favs.Remove(id);
        SaveSettings();
        RebuildRecent();
        ResortInstalledByFavorite();
        return now;
    }

    /// <summary>sortWithFavorite（store.ts:327-334）：收藏优先，组内 lastPlayed 降序（稳定保持原相对序）。</summary>
    IEnumerable<VersionRow> SortWithFavorite(IEnumerable<VersionRow> list) => list
        .Select((v, i) => (v, i))
        .OrderBy(t => IsFavorite(t.v.Id) ? 0 : 1)
        .ThenByDescending(t => _playTimes.GetValueOrDefault(t.v.Id, 0))
        .ThenBy(t => t.i)
        .Select(t => t.v);

    /// <summary>已安装列表就地重排（集合实例被绑定持有，不能换实例）。</summary>
    public void ResortInstalledByFavorite()
    {
        var sorted = SortWithFavorite(InstalledList).ToList();
        InstalledList.Clear();
        foreach (var v in sorted)
        {
            v.Favorite = IsFavorite(v.Id);
            InstalledList.Add(v);
        }
    }

    public void RebuildRecent()
    {
        var recent = SortWithFavorite(InstalledList).Take(RecentCap).ToList();
        RecentList.Clear();
        foreach (var v in recent) RecentList.Add(v);
    }

    /// <summary>loaderLabel（GameView.vue 等价）：从实例 id 抠加载器名与版本。</summary>
    static string LoaderLabelOf(string id, string kind)
    {
        if (kind != "loader") return "原版";
        var m = System.Text.RegularExpressions.Regex.Match(id, @"(?i)(fabric|forge|quilt|optifine)[-_]?loader[-_]?(v?[0-9][^-]*)?");
        if (m.Success)
        {
            var name = System.Globalization.CultureInfo.CurrentCulture.TextInfo.ToTitleCase(m.Groups[1].Value.ToLowerInvariant());
            return m.Groups[2].Success ? $"{name} {m.Groups[2].Value}" : name;
        }
        return "模组加载器";
    }

    static string DescribeKind(string k) => k switch
    {
        "release" => "正式版",
        "snapshot" => "快照",
        "old_beta" => "旧版 Beta",
        "old_alpha" => "旧版 Alpha",
        "loader" => "模组加载器",
        _ => k,
    };

    bool _showSnapshots;
    public bool ShowSnapshots
    {
        get => _showSnapshots;
        set { _showSnapshots = value; Settings.Value.ShowSnapshots = value; SaveSettings(); _ = LoadOnline(); }
    }

    public async Task LoadOnline()
    {
        if (OnlineList.Count > 0) { ApplyFilter(); return; }
        Busy = true; Status = "正在获取版本列表…";
        try
        {
            var metas = await ManifestService.FetchAsync(Paths.VersionManifest, offline: false);
            Dispatcher(() =>
            {
                OnlineList.Clear();
                foreach (var m in metas.Where(m => ShowSnapshots || m.Type == "release"))
                    OnlineList.Add(new VersionRow
                    {
                        Id = m.Id,
                        Label = m.Id,
                        Kind = m.Type,
                        Installed = File.Exists(Paths.VersionJson(m.Id)),
                        // .version-date 只有日期：类型已经由左边的 .tag 表达了（GameView.vue:1070-1071）
                        Subtitle = m.ReleaseTime is null ? "" : m.ReleaseTime[..Math.Min(10, m.ReleaseTime.Length)],
                    });
                SelectedOnline = OnlineList.FirstOrDefault(v => !v.Installed) ?? OnlineList.FirstOrDefault();
                Status = $"版本列表已就绪（{OnlineList.Count} 个）";
                Busy = false;
            });
        }
        catch (Exception ex)
        {
            Dispatcher(() => { Status = "版本列表获取失败：" + ex.Message; Busy = false; });
            Log("版本列表获取失败：" + ex.Message);
        }
    }

    void ApplyFilter()
    {
        // 快照/旧版默认收起，避免列表被 3000 条历史版本淹没
        var keep = OnlineList.Where(v => ShowSnapshots || v.Kind == "release").ToList();
        var filtered = OnlineList.Where(v => !keep.Contains(v)).ToList();
        foreach (var v in filtered) OnlineList.Remove(v);
    }

    public async Task InstallAsync(string id)
    {
        if (Busy) return;
        Busy = true; Progress = 0; Status = $"准备安装 {id}…";
        _cts = new CancellationTokenSource();
        try
        {
            ManifestService.UseMirror(Settings.Value.UseMirror ? Settings.Value.MirrorRoot : null);
            Installer.UseMirror = Settings.Value.UseMirror;
            Installer.MirrorRoot = Settings.Value.MirrorRoot;
            // 先确保 versions/<id>/<id>.json 在本地
            var metas = await ManifestService.FetchAsync(Paths.VersionManifest, offline: false, _cts.Token);
            var meta = metas.FirstOrDefault(m => m.Id == id);
            if (meta is not null) await ManifestService.EnsureVersionJsonAsync(Paths, meta, _cts.Token);
            var report = await Installer.InstallAsync(id, _cts.Token);
            Dispatcher(() =>
            {
                Status = report.Ok ? $"{id} 安装完成：{report.Summary}" : $"{id} 安装未完成：{report.Summary}";
                Progress = report.Ok ? 100 : 0;
                RefreshInstalled();
                foreach (var v in OnlineList.Where(x => x.Id == id)) v.Installed = report.Ok;
                Raise(nameof(OnlineList));
                if (report.Ok) SelectedInstalled = InstalledList.FirstOrDefault(v => v.Id == id) ?? SelectedInstalled;
            });
            foreach (var f in report.Failed.Take(20)) Log("失败：" + f);
        }
        catch (OperationCanceledException)
        {
            Status = "已取消安装";
        }
        catch (Exception ex)
        {
            Status = "安装出错：" + ex.Message;
            Log("安装出错：" + ex.Message);
        }
        finally
        {
            Busy = false;
            _cts?.Dispose(); _cts = null;
        }
    }

    /// <summary>memoryOrganizeBeforeLaunch=true 时启动前触发（UI 层执行 EmptyWorkingSet 扫描）。</summary>
    public event Action? OrganizeBeforeLaunch;

    /// <summary>launch.ts:627-629：mem = auto ? autoMemoryMB(total) : clamp(512..total, memoryMB)。</summary>
    int ResolveMemoryMb()
    {
        long total = TotalMemMb;
        return Settings.Value.MemoryAuto
            ? MemoryPolicy.AutoMemoryMB(total)
            : (int)Math.Min(Math.Max(512, Settings.Value.MemoryMb), total);
    }

    // ---------------- 启动 ----------------
    /// <summary>快速重启游戏（卡片菜单 quickRestart）：结束当前进程，等退出后原实例原账号再来一局。</summary>
    public async Task QuickRestartAsync()
    {
        if (!GameRunning || _launcher is null) return;
        Busy = true; Status = "正在重启游戏…";
        var done = new TaskCompletionSource();
        void OnExit(int _) { try { done.TrySetResult(); } catch { } }
        _launcher.Exited += OnExit;
        try
        {
            await Task.Run(() => _launcher.Stop());
            await done.Task;
            GameRunning = false;
            Busy = false;
            await PlayAsync();
        }
        finally
        {
            _launcher.Exited -= OnExit;
        }
    }

    public async Task PlayAsync(bool commandWorld = false)
    {
        if (!CanPlay) return;
        var id = SelectedInstalled!.Id;
        var acc = SelectedAccount!.Model;
        Busy = true; Status = "正在准备启动…";
        // 原版 launchState 一旦转成 launching，error 态自然就没了（HomeView.vue:161-165 是按当前 status 算的），
        // 所以重新启动要把「需要检查」清掉，否则失败条会一直挂着。
        LaunchFailed = false;
        try
        {
            // 微软号临期（<5 分钟）自动刷新（accounts.ts getValidAccount）；刷新后要重取 Model
            var valid = await MicrosoftAuth.EnsureValidAsync(Settings, Paths, Accounts, acc, Log);
            if (!ReferenceEquals(valid, acc)) { ReloadAccounts(); acc = SelectedAccount!.Model; }

            VersionInfo Load(string vid)
            {
                var f = Paths.VersionJson(vid);
                return File.Exists(f) ? VersionInfo.Parse(File.ReadAllText(f)) : null!;
            }
            var rv = await Task.Run(() => VersionInfo.Resolve(Load, id));

            int need = rv.JavaMajor > 0 ? rv.JavaMajor : JavaLocator.RequiredMajor(id);
            string? java = Settings.Value.AutoJava ? null : Settings.Value.JavaPath;
            if (string.IsNullOrEmpty(java))
            {
                var all = await Task.Run(() => JavaLocator.Scan(Paths));
                java = JavaLocator.Pick(all, need, Settings.Value.JavaPath);
                if (java is null)
                {
                    Status = $"找不到 Java {need}，请先安装对应 Java 或在设置里指定路径";
                    Log($"未找到满足要求的 Java（需要 ≥{need}）");
                    return;
                }
            }

            var opts = new LaunchOptions
            {
                VersionId = id,
                Username = acc.Name,
                Uuid = acc.Uuid,
                Token = acc.Token ?? Guid.NewGuid().ToString("N"),
                AccountType = acc.Kind,
                MinHeapMb = 0,   // 由内存策略统一算（LaunchPlanBuilder 取 Xms=min(mem,1024)）
                MaxHeapMb = ResolveMemoryMb(),
                JavaPath = java,
                Width = Settings.Value.Fullscreen ? 0 : Settings.Value.Width,
                Height = Settings.Value.Fullscreen ? 0 : Settings.Value.Height,
                Fullscreen = Settings.Value.Fullscreen,
                GameDirOverride = Settings.Value.GameDirOverride
                    ?? (Settings.Value.DefaultIsolation ? Paths.VersionDir(id) : null),
            };
            foreach (var a in ArgSplit(Settings.Value.ExtraJvmArgs)) opts.ExtraJvmArgs.Add(a);
            foreach (var a in ArgSplit(Settings.Value.ExtraGameArgs)) opts.ExtraGameArgs.Add(a);

            // 「启动并创建命令世界」（HomeView 卡片菜单）：先造一个开作弊的空世界，再随参直进
            if (commandWorld)
            {
                var mcVer0 = rv.Chain[^1].Id;
                if (!CommandWorld.Supported(mcVer0))
                    throw new Exception("此 Minecraft 版本不支持官方单人 Quick Play（需要 1.20+）");
                var gameDir0 = Settings.Value.GameDirOverride ?? Paths.Root;
                var created = CommandWorld.Create(gameDir0, Paths.ClientJar(mcVer0));
                opts.SingleplayerWorld = created.Id;
                Log($"已创建命令世界存档：{created.Id}");
            }

            var plan = LaunchPlanBuilder.Build(Paths, rv, opts);
            plan.Java = java!;
            foreach (var w in plan.Warnings) Log("提醒：" + w);

            // 启动前整理内存（launch.ts:622-626）：memoryOrganizeBeforeLaunch 开启且为 win 平台语义
            if (Settings.Value.MemoryOrganizeBeforeLaunch)
                try { OrganizeBeforeLaunch?.Invoke(); }
                catch (Exception orgEx) { Log("内存整理失败，继续正常启动：" + orgEx.Message); }

            // 默认配置同步（KeysView 三件套）：按键 / 游戏选项 / 材质包，都只动已修改项。
            // MC 版本 = 继承链最后一段（加载器实例的 InheritsFrom，原版实例就是 Id 本身）
            var mcVer = rv.Chain[^1].Id;
            var gameDir = plan.GameDir;
            try
            {
                if (Settings.Value.KeySync && DefaultKeys.KeySyncSupportedForVersion(mcVer))
                {
                    if (DefaultKeys.SyncKeysToGameDir(gameDir, Paths))
                        Log("已同步默认按键到 options.txt");
                }
                var (applied, unsupported) = DefaultGameOptions.SyncToGameDir(Paths, gameDir, mcVer);
                if (applied.Count > 0) Log($"已同步 {applied.Count} 项默认游戏选项");
                foreach (var u in unsupported) Log($"此版本不支持同步：{u}");
                if (Settings.Value.ResourcePackSync)
                {
                    var n = DefaultResourcePacks.SyncToGameDir(Paths, gameDir, mcVer, Paths.ClientJar(mcVer));
                    if (n > 0) Log($"已同步 {n} 个默认材质包");
                }
            }
            catch (Exception syncEx) { Log("默认配置同步失败：" + syncEx.Message); }

            _launcher?.Dispose();
            _launcher = new GameLauncher();
            _launcher.LineReceived += (line, err) =>
                Dispatcher(() => Log((err ? "[stderr] " : "") + line));
            _launcher.Exited += code => Dispatcher(() =>
            {
                GameRunning = false;
                // 原版 launchState.status='exited' ＋ code，launchFailed 就是 code 非 0（HomeView.vue:161-164）
                LaunchFailed = code != 0;
                Status = code == 0 ? $"游戏已退出（正常）" : $"游戏退出，代码 {code}";
                Log(code == 0 ? "游戏正常退出" : $"游戏退出代码 {code}");
            });
            _launcher.Start(plan);
            GameRunning = true;
            SavePlayTime(id);
            foreach (var v in InstalledList) v.LastPlayed = LastPlayedText(v.Id);
            Status = $"已启动 {acc.Name} @ {id}";
            Log($"启动 Java：{plan.Java}");
            Log("启动命令：" + plan.DisplayCommand(opts.Token));
            Raise(nameof(CanPlay));
        }
        catch (Exception ex)
        {
            // 原版这里把 launchState 置成 error（HomeView.vue:161-165 的第一个分支），
            // 首页按钮文案转「重新启动」、日志窗出 .log-failure 条
            LaunchFailed = true;
            Status = "启动失败：" + ex.Message;
            Log("启动失败：" + ex.Message);
        }
        finally { Busy = false; }
    }

    static IEnumerable<string> ArgSplit(string s) =>
        string.IsNullOrWhiteSpace(s) ? Array.Empty<string>() : ArgList.SplitArgs(s);

    public void StopGame()
    {
        _launcher?.Stop(6000);
        Status = "已请求退出游戏";
    }

    public void CancelWork() => _cts?.Cancel();

    public void SaveSettings()
    {
        try { Settings.Save(); } catch (Exception ex) { Log("设置保存失败：" + ex.Message); }
    }

    /// <summary>窗口背景（壁纸）变更事件：设置页补丁保存后通知 MainWindow 重涂背景层
    /// （背景层在窗口壳上，页面按契约 §1 不引用 MainWindow，走这条 sanctioned 桥）。</summary>
    public event Action? BackgroundChanged;

    /// <summary>对 background 段做一次补丁并持久化 + 广播。null 字段表示「这一项不动」。</summary>
    public void PatchBackground(string? mode = null, string? color = null, string? image = null,
        double? opacity = null, double? blur = null, string? fit = null, bool reset = false)
    {
        var bg = Settings.Value.Background;
        if (reset) Settings.Value.Background = new BackgroundSettings();
        if (mode is not null) bg.Mode = mode;
        if (color is not null) bg.Color = color;
        if (image is not null) bg.Image = image;
        if (opacity is not null) bg.Opacity = Math.Clamp(opacity.Value, 0, 1);
        if (blur is not null) bg.Blur = Math.Clamp(blur.Value, 0, 40);
        if (fit is not null) bg.Fit = fit;
        SaveSettings();
        BackgroundChanged?.Invoke();
    }

    public string DataDirSummary => $"{Paths.Root}\n启动器数据：{Paths.DataDir}";

    public event PropertyChangedEventHandler? PropertyChanged;
    public void Raise([CallerMemberName] string? n = null) => PropertyChanged?.Invoke(this, new(n));
    bool Set<T>(ref T field, T value, [CallerMemberName] string? n = null)
    {
        if (EqualityComparer<T>.Default.Equals(field, value)) return false;
        field = value; Raise(n); return true;
    }
}
