using System.Windows;
using System.Windows.Controls;
using System.Windows.Shapes;
using Wuhuicl.App.Services;

namespace Wuhuicl.App.Pages;

/// <summary>
/// 联机 · 服务器页：KAMUCL ServersView.vue（:356-380 正文）加共用组件层 components/connection/*.vue
/// 的 WPF 版式复刻，本轮接入真实后端（对照 src/main/core/servers.ts 的字段语义）：
/// · 列表 = 游戏根目录 servers.dat（NbtLite 解析；添加/删除/编辑整表写回）；
/// · 在线状态/延迟/人数/版本名/MOTD/favicon = 并发 Server List Ping（3s 超时，离线「未连通」不抛错）；
/// · 搜索过滤名称/地址/MOTD（模板 :363 的 placeholder 原文），收藏优先显示（KAMUCL filteredServers 排序），
///   收藏本体落在启动器数据目录 servers-favorites.json（servers.dat 没有收藏字段）；
/// · 实例关联（快速进入/启动并连接）本轮无数据源（LaunchSession 白名单没有相关字段），保持未关联文案。
/// 事件都留在本页（契约 §1），不引用 MainWindow 的任何方法。
/// </summary>
public partial class ServersPage : UserControl
{
    /// <summary>@media(min-width:1121px) 阈值 — ServersView.vue:510 与 :529 两处同一个断点。</summary>
    const double WideFrom = 1121;

    /// <summary>两列独立滚动的扣高：max-height calc(100vh - 270px) — ServersView.vue:510。</summary>
    const double ScrollDrop = 270;

    /// <summary>窄窗兜底：@max-1120 时 .server-list max-height 360px — ServersView.vue:530。</summary>
    const double NarrowListMax = 360;

    readonly ServersService _service = new();

    /// <summary>主列表 = servers.dat 的当前内容（保存顺序即写回顺序）。</summary>
    List<ServerEntry> _servers = [];

    /// <summary>当前展示的行（收藏优先 + 搜索过滤后的投影）。</summary>
    List<RowVm> _rows = [];

    ServerEntry? _active;

    Window? _host;
    bool _selectMode;
    bool _logOpen;
    bool _ready;
    bool _loading;
    bool _delBatch;
    /// <summary>全选框与行内复选框互写的闸门，避免 Checked 事件来回触发把状态刷成半选。</summary>
    bool _syncing;
    /// <summary>全量刷新的轮次号：单台补 ping 时发现轮次变了就丢弃结果，防止旧值盖新值。</summary>
    int _pingEpoch;

    public ServersPage()
    {
        InitializeComponent();

        Loaded += OnLoaded;
        Unloaded += OnUnloaded;
        IsVisibleChanged += OnIsVisibleChanged;
        _ready = true;
    }

    // ---------------- 断点与两列滚动高度 ----------------
    void OnLoaded(object sender, RoutedEventArgs e)
    {
        _host = Window.GetWindow(this);
        if (_host != null) _host.SizeChanged += Host_SizeChanged;
        ApplyMetrics();
    }

    void OnUnloaded(object sender, RoutedEventArgs e)
    {
        if (_host != null) _host.SizeChanged -= Host_SizeChanged;
        _host = null;
    }

    /// <summary>页面在本体里是 Visibility 切换的（MainWindow 只有一个实例），
    /// 每次进入都重读 servers.dat 并并发探测——对应模板 onMounted 的 load()。</summary>
    void OnIsVisibleChanged(object sender, DependencyPropertyChangedEventArgs e)
    {
        if ((bool)e.NewValue) _ = ReloadAsync();
    }

    void Host_SizeChanged(object? sender, SizeChangedEventArgs e) => ApplyMetrics();

    /// <summary>CSS 的 media query 量的是视口宽；WPF 只知道自己这块的宽度，
    /// 没有宿主窗口时（设计期）按外壳宽度反推：侧栏 208 + 页边距 48（台账 §0，App.vue:1755/styles.css:72）。</summary>
    double ViewportWidth()
    {
        double w = _host?.ActualWidth ?? 0;
        return w > 0 ? w : PageRoot.ActualWidth + 256;
    }

    /// <summary>把 WPF 表达不了的两档 media query 落成实际数值：
    /// · 页面宽 .connect-page max-width 1120（connection.css:3）→ @min-1121 变 1480（ServersView.vue:529）
    /// · 两列 minmax(260px,1fr) minmax(300px,1.05fr)（:461）→ @min-1121 变 minmax(360px,1.2fr) minmax(320px,1fr)（:510）
    /// · 两列各自 max-height calc(100vh - 270px)（:510）；窄窗列表改 360 上限、详情取消 sticky（:530）</summary>
    void ApplyMetrics()
    {
        bool wide = ViewportWidth() >= WideFrom;
        PageRoot.MaxWidth = wide ? 1480 : 1120;

        var cols = WorkspaceGrid.ColumnDefinitions;
        cols[0].MinWidth = wide ? 360 : 260;
        cols[0].Width = new GridLength(wide ? 1.2 : 1, GridUnitType.Star);
        cols[2].MinWidth = wide ? 320 : 300;
        cols[2].Width = new GridLength(wide ? 1 : 1.05, GridUnitType.Star);

        double vh = _host?.ActualHeight ?? 0;
        ServerListScroll.MaxHeight = wide && vh > 0 ? Math.Max(vh - ScrollDrop, 240) : NarrowListMax;
        // 详情列在窄窗是 position static（:530），不再受 100vh 扣高约束，这里给同样的宽窗上限
        ServerDetailScroll.MaxHeight = wide && vh > 0 ? Math.Max(vh - ScrollDrop, 240) : double.PositiveInfinity;
    }

    // ---------------- 加载与同步（ServersView.vue:37 load / :50 syncNow） ----------------

    /// <summary>重读 servers.dat → 合并（按规范化地址保留收藏与选中）→ 重建行 → 并发探测。</summary>
    async Task ReloadAsync()
    {
    try
    {
        if (_loading) return;
        _loading = true;
        try
        {
            ReplaceServers(_service.LoadFromDat());
            LoadErrorBar.Visibility = Visibility.Collapsed;
            AddLog($"[sync] 已从 servers.dat 读取 {_servers.Count} 条服务器记录");
            RebuildRows();
        }
        catch (Exception ex)
        {
            LoadErrorText.Text = "读取服务器列表失败：" + ex.Message;
            LoadErrorBar.Visibility = Visibility.Visible;
        }
        finally
        {
            _loading = false;
        }
        PingAll();
        await Task.CompletedTask;
    }
    catch (Exception ex) { MainWindow.Diag("servers-ReloadAsync EX: " + ex); }
    }


    /// <summary>用新读到的列表替换主列表；收藏与选中条目按规范化地址继承。</summary>
    void ReplaceServers(List<ServerEntry> loaded)
    {
        var oldFavorites = new Dictionary<string, bool>();
        foreach (var e in _servers) oldFavorites[ServersService.KeyOf(e.Ip)] = e.IsFavorite;
        foreach (var e in loaded)
            if (oldFavorites.TryGetValue(ServersService.KeyOf(e.Ip), out var fav) && fav)
                e.IsFavorite = true;
        _servers = loaded;
        if (_active != null)
        {
            string key = ServersService.KeyOf(_active.Ip);
            _active = _servers.FirstOrDefault(e => ServersService.KeyOf(e.Ip) == key);
        }
        _active ??= _servers.FirstOrDefault();
    }

    // ---------------- 探测（pingAll / pingOne，ServersView.vue:55-75） ----------------

    /// <summary>并发探测全表；每台独立 try/catch（在 PingAsync 内），失败行显示「未连通」。</summary>
    async void PingAll()
    {
        int epoch = ++_pingEpoch;
        var snapshot = _servers.ToList();
        var tasks = snapshot.Select(async entry =>
        {
            string note = await ServersService.PingAsync(entry);
            if (epoch != _pingEpoch) return; // 期间又发起了新一轮刷新，结果作废
            await Dispatcher.BeginInvoke(() =>
            {
                AddLog($"[ping] {entry.Ip} {note}");
                UpdateHeader();
                if (ReferenceEquals(entry, _active)) UpdateDetail();
            });
        });
        try { await Task.WhenAll(tasks); }
        catch { /* 单台异常已在服务层吞掉，这里兜底不让聚合异常冒泡 */ }
    }

    /// <summary>单台补探测（添加/编辑后立即刷新该行）。</summary>
    async void PingOne(ServerEntry entry)
    {
        int epoch = _pingEpoch;
        string note = await ServersService.PingAsync(entry);
        if (epoch != _pingEpoch) return;
        await Dispatcher.BeginInvoke(() =>
        {
            AddLog($"[ping] {entry.Ip} {note}");
            UpdateHeader();
            UpdateDetail();
        });
    }

    // ---------------- 行构建与空态（模板 :366/:370/:373 的三个 v-if 与 workspace 互斥） ----------------

    /// <summary>按搜索词过滤 + 收藏优先（ServersView.vue filteredServers 的排序语义）重建行。</summary>
    void RebuildRows()
    {
        string kw = TxtSearch.Text.Trim().ToLowerInvariant();
        var selectedKeys = _rows.Where(r => r.IsSelected).Select(r => ServersService.KeyOf(r.Entry.Ip)).ToHashSet();

        IEnumerable<ServerEntry> visible = _servers;
        if (kw.Length > 0)
        {
            visible = _servers.Where(e =>
                e.Name.ToLowerInvariant().Contains(kw) ||
                ServersService.KeyOf(e.Ip).Contains(kw) ||
                e.Ip.ToLowerInvariant().Contains(kw) ||
                e.Motd.ToLowerInvariant().Contains(kw));
        }

        _rows = visible
            .OrderByDescending(e => e.IsFavorite) // 稳定排序：收藏置顶，其余保持 dat 顺序
            .Select(e => new RowVm(e)
            {
                IsSelected = selectedKeys.Contains(ServersService.KeyOf(e.Ip)),
                ShowSelect = _selectMode,
                IsActive = ReferenceEquals(e, _active),
            })
            .ToList();
        ServerListItems.ItemsSource = _rows;

        bool any = _servers.Count > 0, matched = _rows.Count > 0;
        WorkspaceGrid.Visibility = matched ? Visibility.Visible : Visibility.Collapsed;
        EmptyAll.Visibility = any ? Visibility.Collapsed : Visibility.Visible;
        EmptyNoMatch.Visibility = any && !matched ? Visibility.Visible : Visibility.Collapsed;

        UpdateHeader();
        UpdateDetail();
        UpdateBatchCount();
    }

    /// <summary>标题计数：servers.length 与 onlineCount（模板 :359 的拼法）。</summary>
    void UpdateHeader()
    {
        int online = _servers.Count(e => e.Status == SrvPingStatus.Online);
        HeaderCount.Text = $"{_servers.Count} 个 · {online} 个在线";
    }

    // ---------------- 行交互 ----------------

    void ServerRow_Click(object sender, RoutedEventArgs e)
    {
        if ((sender as FrameworkElement)?.DataContext is not RowVm vm) return;
        if (_selectMode) vm.IsSelected = !vm.IsSelected; // 模板 :8：多选态点行是勾选
        else SetActive(vm);
    }

    /// <summary>.server-list-item.active = accent-soft 底 + radius 6（ServersView.vue:515），
    /// 同一条规则让 .server-row-copy strong 与 monogram 变 accent-2（:516，在 DataTrigger 里）。</summary>
    void SetActive(RowVm vm)
    {
        _active = vm.Entry;
        foreach (var r in _rows) r.IsActive = ReferenceEquals(r, vm);
        UpdateDetail();
    }

    // ---------------- 顶部动作（刷新状态 / 同步游戏列表 / 错误条重试） ----------------

    /// <summary>只重跑一轮并发探测（KAMUCL 顶栏 refresh 的语义）。</summary>
    void RefreshPing_Click(object sender, RoutedEventArgs e) => PingAll();

    /// <summary>重读 servers.dat 合并再探测（KAMUCL syncNow 的语义）。</summary>
    void SyncDat_Click(object sender, RoutedEventArgs e) => _ = ReloadAsync();

    void RetryLoad_Click(object sender, RoutedEventArgs e) => _ = ReloadAsync();

    // ---------------- 详情面板（ServerDetails.vue 的字段位） ----------------

    void UpdateDetail()
    {
        var e = _active;
        if (e == null) return;
        DetailTitle.Text = e.Name;
        DetailAddressText.Text = e.Ip + " ⧉";
        DetailMotd.Text = e.Status switch
        {
            SrvPingStatus.Online => e.Motd,
            SrvPingStatus.Offline => "无法连接（服务器离线或地址错误）",
            _ => "正在检测服务器状态…",
        };
        DetailPlayers.Text = e.PlayersDetail;
        DetailLatency.Text = e.LatencyText;
        DetailVersion.Text = "服务器版本：" + (e.Status == SrvPingStatus.Online && e.VersionName.Length > 0 ? e.VersionName : "—");

        (string pillKey, string brushKey, string label) = e.Status switch
        {
            SrvPingStatus.Online => ("SrvStatusOk", "Ok", "在线"),
            SrvPingStatus.Offline => ("SrvStatusDanger", "Danger", "未连通"),
            _ => ("SrvStatusPending", "AccentHi", "检测中"),
        };
        DetailPill.Style = (Style)FindResource(pillKey);
        DetailDot.SetResourceReference(Shape.FillProperty, brushKey);
        DetailStatusText.Text = label;

        if (e.IconSource != null)
        {
            DetailFavicon.Source = e.IconSource;
            DetailFavicon.Visibility = Visibility.Visible;
            DetailMonogram.Visibility = Visibility.Collapsed;
        }
        else
        {
            DetailFavicon.Visibility = Visibility.Collapsed;
            DetailMonogram.Visibility = Visibility.Visible;
            DetailMonogram.Text = e.Monogram;
        }
    }

    /// <summary>地址按钮：复制地址（ServersView.vue copyAddress 的 toast 语义，这里落剪贴板）。</summary>
    void CopyAddress_Click(object sender, RoutedEventArgs e)
    {
        if (_active == null) return;
        try { Clipboard.SetText(_active.Ip); } catch { /* 剪贴板被占用时静默 */ }
    }

    void DetailRefresh_Click(object sender, RoutedEventArgs e)
    {
        if (_active != null) PingOne(_active);
    }

    // ---------------- 批量管理（v-if="selectMode"，模板 :368） ----------------

    void BatchToggle_Click(object sender, RoutedEventArgs e)
    {
        _selectMode = !_selectMode;
        BtnBatch.Content = _selectMode ? "退出多选" : "批量管理";
        BatchBar.Visibility = _selectMode ? Visibility.Visible : Visibility.Collapsed;
        foreach (var r in _rows)
        {
            r.ShowSelect = _selectMode;
            if (!_selectMode) r.IsSelected = false;
        }
        _syncing = true;
        SelectAllCheck.IsChecked = false;
        _syncing = false;
        UpdateBatchCount();
    }

    /// <summary>「全选搜索结果」只作用于当前可见行（模板 :505）。</summary>
    void SelectAll_Changed(object sender, RoutedEventArgs e)
    {
        if (_syncing || !_ready) return;
        bool on = SelectAllCheck.IsChecked == true;
        _syncing = true;
        foreach (var r in _rows) r.IsSelected = on;
        _syncing = false;
        UpdateBatchCount();
    }

    void RowCheck_Changed(object sender, RoutedEventArgs e)
    {
        if (_syncing || !_ready) return;
        // 回写「全选」框时必须有闸门：SelectAllCheck 一变就触发 SelectAll_Changed，
        // 那里又会把所有行框按旧状态刷一遍，两边互相取消，最终停在半选的死状态。
        _syncing = true;
        SelectAllCheck.IsChecked = _rows.Count > 0 && _rows.All(r => r.IsSelected);
        _syncing = false;
        UpdateBatchCount();
    }

    /// <summary>「已选 N 项」+ 删除按钮的 disabled 条件，对应模板 :368 的 :disabled="!selectedCount"。</summary>
    void UpdateBatchCount()
    {
        int picked = _rows.Count(r => r.IsSelected);
        BatchCountText.Text = $"已选 {picked} 项";
        BtnBatchDelete.IsEnabled = picked > 0;
    }

    // ---------------- 搜索框（placeholder 覆盖层 + 过滤，模板 :363） ----------------

    void Search_TextChanged(object sender, TextChangedEventArgs e)
    {
        if (SearchPh != null)
            SearchPh.Visibility = TxtSearch.Text.Length == 0 ? Visibility.Visible : Visibility.Collapsed;
        if (_ready) RebuildRows();
    }

    void ClearSearch_Click(object sender, RoutedEventArgs e) => TxtSearch.Clear();

    // ---------------- 收藏（★ / ☆ 字形互换，ServerListItem.vue:10/23；收藏优先显示） ----------------

    void Fav_Click(object sender, RoutedEventArgs e)
    {
        if ((sender as FrameworkElement)?.DataContext is not RowVm vm) return;
        vm.Entry.IsFavorite = !vm.Entry.IsFavorite;
        _service.SaveFavorites(_servers);
        RebuildRows();
    }

    // ---------------- 添加 / 编辑（openAdd + onAdd，ServersView.vue:97-123） ----------------

    ServerEntry? _editing;

    void AddServer_Click(object sender, RoutedEventArgs e) => OpenEditModal(null);

    void EditServer_Click(object sender, RoutedEventArgs e) => OpenEditModal(_active);

    void OpenEditModal(ServerEntry? entry)
    {
        _editing = entry;
        ServerEditTitle.Text = entry == null ? "添加服务器" : "编辑服务器";
        ServerEditName.Text = entry?.Name ?? "";
        ServerEditAddr.Text = entry?.Ip ?? "";
        ServerEditError.Visibility = Visibility.Collapsed;
        ModalServerEdit.Visibility = Visibility.Visible;
    }

    void ShowEditError(string msg)
    {
        ServerEditError.Text = msg;
        ServerEditError.Visibility = Visibility.Visible;
    }

    async void SrvSave_Click(object sender, RoutedEventArgs e)
    {
        string name = ServerEditName.Text.Trim();
        var (host, port, display, err) = ServersService.ParseAddress(ServerEditAddr.Text);
        if (name.Length == 0) { ShowEditError("服务器名称不能为空"); return; }
        if (err != null) { ShowEditError(err); return; }
        string key = ServersService.KeyOf(ServerEditAddr.Text);
        if (_servers.Any(s => !ReferenceEquals(s, _editing) && ServersService.KeyOf(s.Ip) == key))
        { ShowEditError("该服务器已在列表中"); return; }

        if (_editing == null)
        {
            var entry = new ServerEntry { Name = name, Ip = display };
            _servers.Add(entry);
            _active = entry;
            try
            {
                await _service.SaveDatAsync(_servers);
            }
            catch (Exception ex)
            {
                _servers.Remove(entry); // 写失败就回退，内存与磁盘保持一致
                ShowEditError("写入 servers.dat 失败：" + ex.Message);
                return;
            }
        }
        else
        {
            string oldName = _editing.Name, oldIp = _editing.Ip;
            _editing.Name = name;
            _editing.Ip = display;
            try
            {
                await _service.SaveDatAsync(_servers);
            }
            catch (Exception ex)
            {
                _editing.Name = oldName; // 写失败就回退
                _editing.Ip = oldIp;
                ShowEditError("写入 servers.dat 失败：" + ex.Message);
                return;
            }
        }
        CloseServerModals();
        AddLog($"[sync] 已保存「{name}」（{host}:{port}）到 servers.dat");
        RebuildRows();
        if (_active != null) PingOne(_active);
    }

    // ---------------- 删除（requestDelete + onDelete，ServersView.vue:126-177） ----------------

    void RequestDelete_Click(object sender, RoutedEventArgs e)
    {
        if (_active == null) return;
        _delBatch = false;
        ModalServerDelete.Visibility = Visibility.Visible;
    }

    void BatchDelete_Click(object sender, RoutedEventArgs e)
    {
        if (_rows.Count(r => r.IsSelected) == 0) return;
        _delBatch = true;
        ModalServerDelete.Visibility = Visibility.Visible;
    }

    async void SrvDelete_Click(object sender, RoutedEventArgs e)
    {
        var victims = new HashSet<ServerEntry>(
            _delBatch
                ? _rows.Where(r => r.IsSelected).Select(r => r.Entry)
                : _active != null ? [_active] : []);
        if (victims.Count == 0) { CloseServerModals(); return; }
        _servers.RemoveAll(victims.Contains);
        if (_active != null && victims.Contains(_active)) _active = _servers.FirstOrDefault();
        try
        {
            await _service.SaveDatAsync(_servers);
            AddLog($"[sync] 已删除 {victims.Count} 条服务器记录并写回 servers.dat");
        }
        catch (Exception ex)
        {
            AddLog($"[sync] servers.dat 写回失败：{ex.Message}"); // 写失败只记日志，列表照常收缩
        }
        CloseServerModals();
        RebuildRows();
        PingAll();
    }

    // ---------------- details/summary 折叠：同步与检测日志 ----------------

    void DetailsToggle_Click(object sender, RoutedEventArgs e)
    {
        _logOpen = !_logOpen;
        LogBlock.Visibility = _logOpen ? Visibility.Visible : Visibility.Collapsed;
        LogChevron.Style = (Style)LogChevron.FindResource(_logOpen ? "Icon.ChevronDown" : "Icon.ChevronRight");
        if (_logOpen) LogViewport.ScrollToEnd();
    }

    /// <summary>真实同步/探测日志（格式照 FrpPanel.vue:148 的 stream + text 拼法），上限 50 条。</summary>
    void AddLog(string line)
    {
        if (LogLines == null || LogSummaryText == null) return;
        var tb = new TextBlock
        {
            FontSize = 12,
            Margin = new Thickness(0, 0, 0, 2),
            LineHeight = 20,
            TextWrapping = TextWrapping.Wrap,
            Text = line,
        };
        tb.SetResourceReference(TextBlock.FontFamilyProperty, "MonoFont");
        tb.SetResourceReference(TextBlock.ForegroundProperty, "Fg2");
        LogLines.Children.Add(tb);
        if (LogLines.Children.Count > 50) LogLines.Children.RemoveAt(0);
        LogSummaryText.Text = $"同步与检测日志 · {LogLines.Children.Count} 条";
        if (_logOpen) LogViewport.ScrollToEnd();
    }

    // ---------------- 更多浮层（原版 details + absolute 浮块，ServerDetails.vue:24 / scoped :30） ----------------
    // WPF 没有 position absolute，删除项做成同一行的显隐，点击走确认模态。

    void More_Click(object sender, RoutedEventArgs e)
        => BtnDelete.Visibility = BtnDelete.Visibility == Visibility.Visible
            ? Visibility.Collapsed : Visibility.Visible;

    // 供主控接线时定位：LogViewport（TerracottaPanel.vue:225 的 ref，日志自动滚到底）与
    // ServerDetail / ServerListScroll / ServerDetailScroll（ServersView.vue:488 + :510 的 sticky 与两列独立滚动容器）。
    // EmptyAll / EmptyNoMatch / LoadErrorBar 是模板 :366 + :370 + :373 的三个 v-if 分支壳，随数据互斥显隐。

    // ---- 三个模态的开合（ServersView.vue:383 / :406 / :427）----
    // 「进入服务器」需要实例关联数据（LaunchSession 白名单没有相关字段），本轮只收起，不做假写入。
    void CloseServerModals()
    {
        ModalServerEdit.Visibility = Visibility.Collapsed;
        ModalServerDelete.Visibility = Visibility.Collapsed;
        ModalServerJoin.Visibility = Visibility.Collapsed;
    }

    void SrvCancel_Click(object sender, RoutedEventArgs e) => CloseServerModals();
    void SrvJoin_Click(object sender, RoutedEventArgs e) => CloseServerModals();

    /// <summary>行视图模型：Entry 绑数据，IsSelected / IsActive / ShowSelect 绑批量与选中视觉
    /// （DataTemplate.Triggers 按这三个开关和 Entry.Status 切样式）。</summary>
    public sealed class RowVm : System.ComponentModel.INotifyPropertyChanged
    {
        public RowVm(ServerEntry entry) => Entry = entry;

        public ServerEntry Entry { get; }

        public event System.ComponentModel.PropertyChangedEventHandler? PropertyChanged;

        bool _isSelected;
        public bool IsSelected { get => _isSelected; set { if (_isSelected == value) return; _isSelected = value; Raise(); } }

        bool _showSelect;
        public bool ShowSelect { get => _showSelect; set { if (_showSelect == value) return; _showSelect = value; Raise(); } }

        bool _isActive;
        public bool IsActive { get => _isActive; set { if (_isActive == value) return; _isActive = value; Raise(); } }

        void Raise([System.Runtime.CompilerServices.CallerMemberName] string? name = null)
            => PropertyChanged?.Invoke(this, new(name));
    }
}
