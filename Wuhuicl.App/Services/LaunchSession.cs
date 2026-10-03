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
    public override string ToString() => Label;
}

public sealed class AccountRow
{
    public Account Model { get; init; } = new();
    /// <summary>头像一律由玩家皮肤文件现算，不用任何外部图片。</summary>
    public System.Windows.Media.ImageSource Avatar => SkinRenderer.Face(64);
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
    public VersionRow? SelectedInstalled { get => _installed; set { if (Set(ref _installed, value)) { Settings.Value.LastVersion = value?.Id; SaveSettings(); } } }

    VersionRow? _online;
    public VersionRow? SelectedOnline { get => _online; set => Set(ref _online, value); }

    string _status = "准备就绪";
    public string Status { get => _status; set => Set(ref _status, value); }

    double _progress;
    public double Progress { get => _progress; set => Set(ref _progress, value); }

    bool _busy;
    public bool Busy { get => _busy; set { if (Set(ref _busy, value)) { Raise(nameof(RunStateText)); Raise(nameof(RunStateColor)); } } }

    bool _gameRunning;
    public bool GameRunning { get => _gameRunning; set { if (Set(ref _gameRunning, value)) { Raise(nameof(CanPlay)); Raise(nameof(CanStop)); Raise(nameof(RunStateText)); Raise(nameof(RunStateColor)); } } }

    public bool CanPlay => !Busy && !GameRunning && SelectedAccount is not null && SelectedInstalled is not null;
    public bool CanStop => GameRunning;
    public bool HasNoAccount => AccountList.Count == 0;

    public string RuntimeSummary => Settings.Value.AutoJava ? "自动选择" :
        (string.IsNullOrEmpty(Settings.Value.JavaPath) ? "自动选择"
            : Path.GetFileNameWithoutExtension(Settings.Value.JavaPath));
    public string MemorySummary => Settings.Value.MaxHeapMb >= 1024
        ? $"{Settings.Value.MaxHeapMb / 1024.0:0.#} GB"
        : $"{Settings.Value.MaxHeapMb} MB";
    // 原版 heroStatus 四态（HomeView.vue:192-200）：等待选择 / 正在准备 / 游戏运行中 / 需要检查 / 就绪
    public string RunStateText => GameRunning ? "游戏运行中" : Busy ? "正在准备"
        : InstalledList.Count == 0 ? "等待选择" : "就绪";

    /// <summary>.runtime-state i 的点色：idle 灰、ready/running 绿（HomeView.vue:881-882），
    /// 不是 accent —— 之前空状态点成蓝色就是这里错了。</summary>
    public string RunStateColor => !GameRunning && !Busy && InstalledList.Count == 0 ? "Idle" : "Ok";

    /// <summary>ready/running 才有 3px 光环 box-shadow 0 0 0 3px --ok-soft（:882）。</summary>
    public string RunStateRing => RunStateColor == "Ok" ? "OkSoft" : "Transparent";

    /// <summary>版本页第二个页签：原版是「已安装」，有实例时才带数量（GameView.vue:937）。</summary>
    public string InstalledTabText => InstalledList.Count == 0 ? "已安装" : $"已安装（{InstalledList.Count}）";

    string _lastCommand = "";
    public string LastCommand { get => _lastCommand; set => Set(ref _lastCommand, value); }

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
        Accounts = new AccountStore(Paths);
        Accounts.Load();
        Installer = new InstallService(Paths)
        {
            UseMirror = Settings.Value.UseMirror,
            MirrorRoot = Settings.Value.MirrorRoot,
            Progress = new Progress<string>(s => Dispatcher(() => Log(s))),
        };
        Installer.DL.PreferMirrorFirst = Settings.Value.UseMirror;
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
        });
    }

    // ---------------- 账号 ----------------
    void ReloadAccounts()
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
                });
                InstalledList[^1].LastPlayed = "";
            }
        }
        catch (Exception ex) { Log("读取已安装版本失败：" + ex.Message); }
        // 原地重排：集合实例被界面绑定持有，换掉实例会让 ListBox 失去通知
        foreach (var v in InstalledList) v.LastPlayed = LastPlayedText(v.Id);
        Raise(nameof(InstalledList));
        var sorted = InstalledList.OrderByDescending(v => v.Id, Comparer<string>.Create(VersionCompare.Compare)).ToList();
        InstalledList.Clear();
        foreach (var v in sorted) InstalledList.Add(v);
        SelectedInstalled ??= InstalledList.FirstOrDefault();
        Raise(nameof(CanPlay));
        Raise(nameof(RunStateText));
        Raise(nameof(RunStateColor));
        Raise(nameof(RunStateRing));
        Raise(nameof(InstalledTabText));
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
                        Subtitle = DescribeKind(m.Type) + (m.ReleaseTime is null ? "" : " · " + m.ReleaseTime[..Math.Min(10, m.ReleaseTime.Length)]),
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

    // ---------------- 启动 ----------------
    public async Task PlayAsync()
    {
        if (!CanPlay) return;
        var id = SelectedInstalled!.Id;
        var acc = SelectedAccount!.Model;
        Busy = true; Status = "正在准备启动…";
        try
        {
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
                MinHeapMb = Settings.Value.MinHeapMb,
                MaxHeapMb = Settings.Value.MaxHeapMb,
                JavaPath = java,
                Width = Settings.Value.Fullscreen ? 0 : Settings.Value.Width,
                Height = Settings.Value.Fullscreen ? 0 : Settings.Value.Height,
                Fullscreen = Settings.Value.Fullscreen,
                GameDirOverride = Settings.Value.GameDirOverride,
            };
            foreach (var a in ArgSplit(Settings.Value.ExtraJvmArgs)) opts.ExtraJvmArgs.Add(a);
            foreach (var a in ArgSplit(Settings.Value.ExtraGameArgs)) opts.ExtraGameArgs.Add(a);

            var plan = LaunchPlanBuilder.Build(Paths, rv, opts);
            plan.Java = java!;
            LastCommand = plan.DisplayCommand(opts.Token);
            foreach (var w in plan.Warnings) Log("提醒：" + w);

            _launcher?.Dispose();
            _launcher = new GameLauncher();
            _launcher.LineReceived += (line, err) =>
                Dispatcher(() => Log((err ? "[stderr] " : "") + line));
            _launcher.Exited += code => Dispatcher(() =>
            {
                GameRunning = false;
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

    public string DataDirSummary => $"{Paths.Root}\n启动器数据：{Paths.DataDir}";

    public event PropertyChangedEventHandler? PropertyChanged;
    void Raise([CallerMemberName] string? n = null) => PropertyChanged?.Invoke(this, new(n));
    bool Set<T>(ref T field, T value, [CallerMemberName] string? n = null)
    {
        if (EqualityComparer<T>.Default.Equals(field, value)) return false;
        field = value; Raise(n); return true;
    }
}
