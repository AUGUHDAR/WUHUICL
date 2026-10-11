using System.ComponentModel;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Media;
using Wuhuicl.App.Services;
using Wuhuicl.Core.Storage;

namespace Wuhuicl.App.Pages;

/// <summary>
/// 资源管理 · 模组页。原版 ModsView.vue:9-15 只是 <code>&lt;FileManager title rel …/&gt;</code> 的 16 行壳，
/// 几何全部来自 components/FileManager.vue，而资源包 / 光影包是同一个组件换 <c>data-resource</c>（FileManager.vue:332），
/// 所以本页做成一套壳：<see cref="ResourceKind"/> 只切列集、图标尺寸、专属单元显隐与三处文案。
/// 文件列表已接真实后端：<see cref="ResourceService"/> 扫 版本资源目录 + 共享目录（同名版本目录优先），
/// 行由 <see cref="BuildRow"/> 按被替换的占位行结构逐项生成（FmRow / Check / Switch / BtnSmGhost … 同一套样式键），
/// 视觉 1:1。启停 = .jar ↔ .jar.disabled 改名；删除 = 移入系统回收站；打开文件夹 = explorer。
/// </summary>
public partial class ModsPage : UserControl
{
    // 表头 + 代码生成的行共用同一套列轨（.fm-table-head,.fm-row 同列宽 — FileManager.vue:719）
    readonly List<Grid> _rowGrids = new();
    bool _ready;
    bool _sessionHooked;
    int _generation;      // 重入保护：只认最后一次 reload 的回包
    bool _ioBusy;         // 启停/删除串行化，避免改名竞态
    bool _suppressToggle; // 回弹开关状态时不触发 Toggle

    string resourceKind = "mods";

    // ---------------- 真实列表状态 ----------------
    List<ResourceEntry> _entries = new();
    string? _loadError;
    bool _loading;
    ResourceEntry? _pendingDelete;

    // ---------------- v-if 门控位 ----------------
    // 这几块在原版都由数据决定渲染与否（currentVersion / updatePanel / batchResults / dupOpen …）。
    bool _hasCurrentVersion = true;   // currentVersion（:341 fm-context / :347 card.empty / :400 fm-controls / :411 fm-card）
    bool _updPanelOpen = false;               // updatePanel.open（:352）
    bool _hasBatchResults = false;            // batchResults.length（:409）
    bool _statusStripOn;              // entries.length && (loading || loadError)（:412）
    bool _statusStripIsError;         // loadError（同一行的两态）
    bool _dupModalOpen = false;               // dupOpen（:473，仅 mods）
    bool _migrationModalOpen = false;         // migrationOpen（:460）
    bool _versionModalOpen = false;           // switchFile 非空（:459）
    bool _confirmModalOpen;           // delModal.open（:463）
    /// <summary>列表是否有真实条目（驱动 .shader-empty，:411 的 !entries.length）。</summary>
    bool _hasListEntries;

    /// <summary>
    /// 资源类型，对应 FileManager.vue 的 props.rel：<c>mods</c>（默认）/ <c>resourcepacks</c> / <c>shaderpacks</c>。
    /// 赋值幂等且只做视觉切换 + 重载列表；非法值一律回落成 mods。
    /// </summary>
    public string ResourceKind
    {
        get => resourceKind;
        set
        {
            string next = (value ?? string.Empty).Trim().ToLowerInvariant() switch
            {
                "resourcepacks" or "resourcepack" or "packs" => "resourcepacks",
                "shaderpacks" or "shaderpack" or "shaders" => "shaderpacks",
                _ => "mods"
            };
            if (next == resourceKind) return;
            resourceKind = next;
            if (_ready)
            {
                ApplyResourceKind();
                _ = ReloadAsync();
            }
        }
    }

    public ModsPage()
    {
        InitializeComponent();
        _ready = true;
        // 默认就是 mods，但 ApplyResourceKind 还负责把 packs-only 单元收起、路径文案等
        // 初始状态落位 —— 构造时必须跑一次，否则等 MainWindow 赋同值时 setter 会因幂等直接返回。
        ApplyResourceKind();

        // ---------------- 真实后端接线（都在代码侧挂事件，XAML 不动） ----------------
        BtnOpen.Click += async (_, _) => await OpenFolderAsync();
        BtnRefresh.Click += async (_, _) => await ReloadAsync();
        BtnCtxRefresh.Click += async (_, _) => await ReloadAsync();
        BtnStatusRetry.Click += async (_, _) => await ReloadAsync();
        BtnEmptyRetry.Click += async (_, _) => await ReloadAsync();
        CmbStatus.SelectionChanged += (_, _) => { if (_ready) RenderList(); };
        CmbSort.SelectionChanged += (_, _) => { if (_ready) RenderList(); };
        BtnConfirmCancel.Click += (_, _) => CloseConfirm();
        BtnConfirmDelete.Click += async (_, _) => await DeletePendingAsync();
        Loaded += async (_, _) => { HookSession(); await ReloadAsync(); };
        Unloaded += (_, _) => UnhookSession();
    }

    // ==================== 会话联动：SelectedInstalled 变化 → 重新加载 ====================

    void HookSession()
    {
        if (_sessionHooked || DataContext is not LaunchSession ls) return;
        ls.PropertyChanged += OnSessionPropertyChanged;
        _sessionHooked = true;
    }

    void UnhookSession()
    {
        if (_sessionHooked && DataContext is LaunchSession ls)
            ls.PropertyChanged -= OnSessionPropertyChanged;
        _sessionHooked = false;
    }

    void OnSessionPropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(LaunchSession.SelectedInstalled))
            _ = ReloadAsync();
    }

    // ==================== 列表加载 ====================

    /// <summary>重新扫目录并重渲染。任何 IO 失败都落到状态文本（状态条 / 空态），不抛出、不崩。</summary>
    async Task ReloadAsync()
    {
        if (!_ready) return;
        int gen = ++_generation;
        string? id = (DataContext as LaunchSession)?.SelectedInstalled?.Id;
        if (string.IsNullOrEmpty(id))
        {
            // 没选实例：整页退场只留 NoVersionCard（:347 的 currentVersion 门）
            _hasCurrentVersion = false;
            _entries = new List<ResourceEntry>();
            _loadError = null;
            _loading = false;
            RenderList();
            return;
        }
        _hasCurrentVersion = true;
        _loading = true;
        _loadError = null;
        RenderList();   // 刷新中：保留旧行（原版语义，status-strip 压在列表上方）
        try
        {
            var list = await Task.Run(() => ResourceService.List(id, resourceKind));
            if (gen != _generation) return;
            _entries = list;
            _loadError = null;
        }
        catch (Exception ex)
        {
            if (gen != _generation) return;
            _loadError = ex.Message;   // 旧列表保留，错误走状态条 / 空态
        }
        finally
        {
            if (gen == _generation)
            {
                _loading = false;
                RenderList();
            }
        }
    }

    /// <summary>按当前 搜索 / 状态筛选 / 排序 生成视图。</summary>
    List<ResourceEntry> ApplyFilterSort()
    {
        IEnumerable<ResourceEntry> q = _entries;
        string? needle = SearchBox?.Text?.Trim();
        if (!string.IsNullOrEmpty(needle))
            q = q.Where(e => e.Name.Contains(needle, StringComparison.OrdinalIgnoreCase));
        q = CmbStatus?.SelectedIndex switch
        {
            1 => q.Where(e => e.Enabled),
            2 => q.Where(e => !e.Enabled),
            3 => Enumerable.Empty<ResourceEntry>(),   // 已锁定：锁定目录尚未接入
            _ => q,
        };
        return CmbSort?.SelectedIndex switch
        {
            1 => q.OrderByDescending(e => e.LastWriteTime).ToList(),   // 最近修改
            2 => q.OrderByDescending(e => e.SizeBytes).ToList(),       // 文件大小
            _ => q.OrderBy(e => e.Name, StringComparer.OrdinalIgnoreCase).ToList(),  // 名称排序
        };
    }

    /// <summary>
    /// 重渲染：真实行 / 计数 / 实例卡与路径 / 空态三选一 / 状态条 / shader-empty。
    /// </summary>
    void RenderList()
    {
        if (!_ready) return;
        var view = ApplyFilterSort();
        RebuildRows(view);
        _hasListEntries = _entries.Count > 0;

        // 标题旁计数（resourceCount）跟当前显示行数
        HeadCount.Text = view.Count.ToString();

        UpdateContextTexts();

        // 卡内空态三选一（:414 / :418 / :423）：读取失败 / 真空 / 搜索无匹配
        bool totalEmpty = _entries.Count == 0;
        bool viewEmpty = view.Count == 0 && !totalEmpty;
        bool loadErrorShown = _loadError is not null && totalEmpty;
        EmptyLoadError.Visibility = loadErrorShown ? Visibility.Visible : Visibility.Collapsed;
        if (loadErrorShown) EmptyLoadErrorText.Text = $"读取失败：{_loadError}";
        EmptyBlock.Visibility = !loadErrorShown && totalEmpty ? Visibility.Visible : Visibility.Collapsed;
        EmptyNoMatch.Visibility = !loadErrorShown && viewEmpty ? Visibility.Visible : Visibility.Collapsed;

        // 状态条：列表还有旧数据时，刷新中 / 刷新失败都压在列表上方（:412）
        _statusStripOn = _entries.Count > 0 && (_loading || _loadError is not null);
        _statusStripIsError = _loadError is not null;
        StatusStripText.Text = _loading ? "正在刷新，暂时保留上次的文件列表…" : $"刷新失败：{_loadError}";
        BtnStatusRetry.Visibility = _statusStripIsError && !_loading ? Visibility.Visible : Visibility.Collapsed;

        ApplyGating();
        ApplyShaderEmpty();
        ApplyEmptyMinHeight();
    }

    /// <summary>清掉旧行、按视图插新行。插入点＝空态三兄弟之前（表头之后）。</summary>
    void RebuildRows(List<ResourceEntry> view)
    {
        if (EmptyLoadError.Parent is not StackPanel host) return;
        foreach (var g in _rowGrids) host.Children.Remove(g);
        _rowGrids.Clear();
        int at = host.Children.IndexOf(EmptyLoadError);
        if (at < 0) at = host.Children.Count;
        foreach (var e in view)
        {
            var row = BuildRow(e);
            host.Children.Insert(at, row);
            at++;
            if (row is Border { Child: Grid g }) _rowGrids.Add(g);
        }
    }

    /// <summary>实例卡两行文字与 .fm-path 显示路径（游戏根 + '/' + rel，正斜杠，>80 中间省略 — FileManager.vue:116-117）。</summary>
    void UpdateContextTexts()
    {
        var ls = DataContext as LaunchSession;
        string? id = ls?.SelectedInstalled?.Id;
        if (VerSelect.Template?.FindName("VerName", VerSelect) is TextBlock name)
            name.Text = string.IsNullOrEmpty(id) ? "未选择版本" : id;
        if (VerSelect.Template?.FindName("VerDesc", VerSelect) is TextBlock desc)
            desc.Text = string.IsNullOrEmpty(id) ? "到「游戏版本」页安装或选择版本" : $"{id} · 已隔离";

        string root = ls?.Paths.Root ?? GamePaths.DetectDefault().Root;
        string p = root.Replace('\\', '/') + "/" + resourceKind;
        if (p.Length > 80) p = p[..40] + "…" + p[^40..];
        PathText.Text = p;
    }

    // ==================== 行生成（视觉逐项对齐被替换的占位行） ====================

    /// <summary>
    /// 生成一行。结构＝占位行 XAML：FmRow 外框 ＋ 13 轨 Grid ＋ Check 复选框 / FmFile 图标（mods 32、
    /// 其余 36）/ FmName 名称（禁用加删除线，第二行 FmInternal 状态） / FmMeta 大小·日期 /
    /// Switch 启用开关 / ⋯ 菜单（锁定·版本为占位项，删除接回收站）。
    /// </summary>
    Border BuildRow(ResourceEntry entry)
    {
        bool mods = resourceKind == "mods";
        var row = new Border { Style = S("FmRow") };
        var grid = new Grid();
        AddColumns(grid, mods);

        // 复选框（mods-only；批量选择未接入，视觉占位与原占位行一致）
        var cb = new CheckBox
        {
            Style = S("Check"),
            VerticalAlignment = VerticalAlignment.Center,
            Tag = "mods-only",
        };
        grid.Children.Add(cb);

        // 文件图标：file 形不随资源类型换（原版内联 file SVG；cube/folder/sun 只在空态）
        var icon = new ContentControl
        {
            Width = mods ? 32 : 36,
            Height = mods ? 32 : 36,
            VerticalAlignment = VerticalAlignment.Center,
            Tag = "file-icon",
        };
        icon.SetResourceReference(FrameworkElement.StyleProperty, "Icon.FmFile");
        icon.SetResourceReference(Control.ForegroundProperty, "Accent");
        Grid.SetColumn(icon, 2);
        grid.Children.Add(icon);

        // 名称列（* 轨）：FmName + 可选 FmInternal 第二行（已禁用 / 共享目录 / 两者）
        var nameStack = new StackPanel { VerticalAlignment = VerticalAlignment.Center };
        var name = new TextBlock { Style = S("FmName"), Text = entry.Name };
        if (!entry.Enabled) name.TextDecorations = TextDecorations.Strikethrough;   // .fm-row-disabled — :608-611
        nameStack.Children.Add(name);
        string sub = (entry.Enabled ? "" : "已禁用") + (entry.FromShared ? (entry.Enabled ? "共享目录" : " · 共享目录") : "");
        if (sub.Length > 0)
            nameStack.Children.Add(new TextBlock { Style = S("FmInternal"), Text = sub });
        var nameCell = new Grid();
        nameCell.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        nameCell.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        nameCell.Children.Add(nameStack);
        Grid.SetColumn(nameCell, 4);
        grid.Children.Add(nameCell);

        // 大小 / 修改时间（.fm-meta：12px、大小右对齐、日期左对齐 — :719）
        var size = new TextBlock { Style = S("FmMeta"), Text = FormatSize(entry.SizeBytes) };
        Grid.SetColumn(size, 6);
        grid.Children.Add(size);
        var date = new TextBlock
        {
            Style = S("FmMeta"),
            TextAlignment = TextAlignment.Left,
            Text = entry.LastWriteTime.ToString("yyyy/M/d"),
        };
        Grid.SetColumn(date, 8);
        grid.Children.Add(date);

        // 启用开关（mods-only；.switch 40×22）
        var toggle = new CheckBox
        {
            Style = S("Switch"),
            HorizontalAlignment = HorizontalAlignment.Left,
            VerticalAlignment = VerticalAlignment.Center,
            Tag = "mods-only",
            ToolTip = entry.Enabled ? "禁用模组" : "启用模组",
        };
        toggle.IsChecked = entry.Enabled;   // 先置值再挂事件，避免伪触发
        toggle.Checked += (_, _) => OnToggle(entry, toggle);
        toggle.Unchecked += (_, _) => OnToggle(entry, toggle);
        Grid.SetColumn(toggle, 10);
        grid.Children.Add(toggle);

        // ⋯ 菜单（.file-more：BtnSmGhost + PopupCard）
        var moreHost = new Grid();
        var moreBtn = new Button
        {
            Style = S("BtnSmGhost"),
            Width = 44,
            Height = 32,
            Padding = new Thickness(0),
            HorizontalAlignment = HorizontalAlignment.Left,
            FontSize = 18,
            Content = "⋯",
            ToolTip = "更多操作",
        };
        var popup = new Popup
        {
            PlacementTarget = moreBtn,
            Placement = PlacementMode.Bottom,
            HorizontalOffset = -110,
            AllowsTransparency = true,
            Focusable = false,
            StaysOpen = false,
        };
        var menu = new StackPanel();
        var bLock = MenuItem("锁定", null);          // 锁定目录未接入：与占位期一致的可视项
        var bVer = MenuItem("版本", new Thickness(0, 8, 0, 0));
        var bDel = MenuItem("删除", new Thickness(0, 4, 0, 0));
        bDel.SetResourceReference(Control.ForegroundProperty, "Danger");
        bDel.Click += (_, _) => { popup.IsOpen = false; AskDelete(entry); };
        menu.Children.Add(bLock);
        menu.Children.Add(bVer);
        menu.Children.Add(new Border   // .file-more-actions .fm-remove 分隔线 — :489
        {
            Height = 1,
            Background = TryFindResource("Line") as Brush,
            Margin = new Thickness(0, 4, 0, 0),
        });
        menu.Children.Add(bDel);
        var card = new Border { Style = S("PopupCard"), MinWidth = 150, Padding = new Thickness(10), Child = menu };
        popup.Child = card;
        moreHost.Children.Add(moreBtn);
        moreHost.Children.Add(popup);
        Grid.SetColumn(moreHost, 12);
        grid.Children.Add(moreHost);

        // 列轨与 Tag 显隐（mods-only / packs-only / file-icon 尺寸）按当前资源类型落位
        ApplyKindToGrid(grid, mods, KindWidths(mods));

        row.Child = grid;
        return row;
    }

    /// <summary>列轨宽 [复选框, gap, 图标, gap, 大小, 日期, 启用, 操作]；名称轨恒 *（FileManager.vue:719）。</summary>
    static double[] KindWidths(bool mods) => mods
        ? new[] { 18d, 12d, 32d, 12d, 88d, 132d, 42d, 40d }
        : new[] { 0d, 0d, 36d, 12d, 88d, 132d, 0d, 40d };

    /// <summary>行 Grid 的 13 轨（与表头一致）。</summary>
    static void AddColumns(Grid g, bool mods)
    {
        double[] w = KindWidths(mods);
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(w[0]) });   // 复选框
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(w[1]) });   // gap
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(w[2]) });   // 图标
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(w[3]) });   // gap
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) }); // 名称
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(12) });     // gap
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(w[4]) });   // 大小
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(12) });     // gap
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(w[5]) });   // 修改时间
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(mods ? 12 : 0) });
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(w[6]) });   // 启用
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(mods ? 12 : 0) });
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(w[7]) });   // 操作
    }

    Button MenuItem(string content, Thickness? margin)
    {
        var b = new Button
        {
            Style = S("MenuItem2"),
            HorizontalAlignment = HorizontalAlignment.Stretch,
            Content = content,
        };
        if (margin is { } m) b.Margin = m;
        return b;
    }

    /// <summary>页内 / 应用级样式键查找；找不到就落默认样式（不抛）。</summary>
    Style? S(string key) => TryFindResource(key) as Style;

    // ==================== 操作：启停 / 删除 / 打开文件夹 ====================

    async void OnToggle(ResourceEntry entry, CheckBox toggle)
    {
        if (_suppressToggle) return;
        bool enable = toggle.IsChecked == true;
        if (enable == entry.Enabled) return;   // 回弹后的同态事件不重复操作
        if (_ioBusy)
        {
            SetToggle(toggle, entry.Enabled);  // 忙：回弹到磁盘状态
            return;
        }
        _ioBusy = true;
        try
        {
            await ResourceService.ToggleEnabledAsync(entry.FullPath);
            await ReloadAsync();               // 成功：整表重扫（.disabled 改名落地）
        }
        catch (Exception ex)
        {
            SetToggle(toggle, entry.Enabled);  // 失败：回弹
            ShowStatusError($"更改启用状态失败：{ex.Message}");
        }
        finally
        {
            _ioBusy = false;
        }
    }

    void SetToggle(CheckBox cb, bool value)
    {
        _suppressToggle = true;
        try { cb.IsChecked = value; }
        finally { _suppressToggle = false; }
    }

    void AskDelete(ResourceEntry entry)
    {
        _pendingDelete = entry;
        ConfirmText.Text = $"确定要删除「{entry.Name}」吗？文件将移入系统回收站，可从回收站恢复。";
        _confirmModalOpen = true;
        ApplyGating();
    }

    void CloseConfirm()
    {
        _pendingDelete = null;
        _confirmModalOpen = false;
        ApplyGating();
    }

    async Task DeletePendingAsync()
    {
        var entry = _pendingDelete;
        CloseConfirm();
        if (entry is null || _ioBusy) return;
        _ioBusy = true;
        try
        {
            await ResourceService.DeleteToRecycleBinAsync(entry.FullPath);
        }
        catch (Exception ex)
        {
            ShowStatusError($"删除失败：{ex.Message}");
        }
        finally
        {
            _ioBusy = false;
        }
        await ReloadAsync();
    }

    async Task OpenFolderAsync()
    {
        string? id = (DataContext as LaunchSession)?.SelectedInstalled?.Id;
        if (string.IsNullOrEmpty(id))
        {
            ShowStatusError("未选择游戏版本，无法打开文件夹");
            return;
        }
        try
        {
            await ResourceService.OpenFolderAsync(id, resourceKind);
        }
        catch (Exception ex)
        {
            ShowStatusError($"打开文件夹失败：{ex.Message}");
        }
    }

    /// <summary>操作失败的状态条（error 档；重试钮由 ApplyGating 按 _statusStripIsError 落，点击＝重扫列表）。</summary>
    void ShowStatusError(string message)
    {
        _statusStripOn = true;
        _statusStripIsError = true;
        StatusStripText.Text = message;
        ApplyGating();
    }

    /// <summary>fmtSize：与占位行同档的一位小数 MB / KB / GB / B。</summary>
    static string FormatSize(long bytes) => bytes switch
    {
        >= 1L << 30 => $"{bytes / (double)(1L << 30):0.##} GB",
        >= 1L << 20 => $"{bytes / (double)(1L << 20):0.#} MB",
        >= 1L << 10 => $"{bytes / (double)(1L << 10):0} KB",
        _ => $"{bytes} B",
    };

    // ==================== 视觉门控（与原实现一致，只把行轨换成「表头 + 生成行」） ====================

    /// <summary>
    /// 列集切换。两套都来自 FileManager.vue:719 的最终块：
    /// mods = <c>18px 32px minmax(0,1fr) 88px 132px 42px 40px</c>（7 轨，含复选框与启用开关）；
    /// 资源包 / 光影 = 基础 <c>36px minmax(0,1fr) 88px 132px 40px</c>（5 轨）。
    /// </summary>
    void ApplyResourceKind()
    {
        bool mods = resourceKind == "mods";
        // .resource-page 的 .fm-head 切列流（FileManager.vue:506-510）：按钮行换行并占满
        if (FmActions != null)
        {
            Grid.SetRow(FmActions, mods ? 0 : 1);
            Grid.SetColumn(FmActions, mods ? 1 : 0);
            Grid.SetColumnSpan(FmActions, mods ? 1 : 2);
            FmActions.HorizontalAlignment = mods ? HorizontalAlignment.Right : HorizontalAlignment.Left;
            FmActions.Margin = new Thickness(0, mods ? 0 : 16, 0, 0);
        }
        // 轨宽按 [复选框, gap, 图标, gap, 大小, 日期, 启用, 操作] 给；名称轨恒 *
        double[] w = KindWidths(mods);
        string emptyIconKey = resourceKind switch
        {
            "resourcepacks" => "Icon.ResourcePacks",
            "shaderpacks" => "Icon.Shaders",
            _ => "Icon.Mods"
        };

        ApplyKindToGrid(FmHead, mods, w);
        foreach (var g in _rowGrids) ApplyKindToGrid(g, mods, w);

        // 三个壳的文案由 props 决定：ModsView.vue:10-13 / PacksView.vue:10-13 / ShadersView.vue:10-13
        switch (resourceKind)
        {
            case "resourcepacks":
                HeadTitle.Text = "资源包";
                BtnOpen.Content = "打开资源包文件夹";
                EmptyText.Text = "暂无资源包，将 .zip 文件放入 resourcepacks 文件夹";
                break;
            case "shaderpacks":
                HeadTitle.Text = "光影包";
                BtnOpen.Content = "打开光影包文件夹";
                EmptyText.Text = "暂无光影包，将 .zip 文件放入 shaderpacks 文件夹";
                break;
            default:
                HeadTitle.Text = "模组";
                BtnOpen.Content = "打开模组文件夹";
                EmptyText.Text = "暂无模组，将 .jar 文件放入 mods 文件夹";
                break;
        }
        // .fm-actions 第二颗是 `button.btn` ＋（shaderpacks → btn-gold，否则 btn-ghost）— :337
        BtnOpen.SetResourceReference(StyleProperty,
            resourceKind == "shaderpacks" ? "BtnGold" : "BtnGhost");
        EmptyIcon.SetResourceReference(StyleProperty, emptyIconKey);

        BtnCheckUpdate.Visibility = mods ? Visibility.Visible : Visibility.Collapsed; // v-if rel==='mods' — :336
        BtnCtxRefresh.Visibility = mods ? Visibility.Collapsed : Visibility.Visible;  // v-if rel!=='mods' — :343
        // 头部那颗「更多」（.file-more，:338）：资源包 / 光影页不渲染
        HeadMoreHost.Visibility = mods ? Visibility.Visible : Visibility.Collapsed;
        // 资源包 / 光影没有 .fm-controls（搜索 + SelectMenu，:400）——刷新由上面的 BtnCtxRefresh 承担，
        // FmControls 的显隐（还叠加 currentVersion 门）统一在 ApplyGating 落。
        // 光影包空态提示行 — :421
        EmptyShadersHint.Visibility = resourceKind == "shaderpacks" ? Visibility.Visible : Visibility.Collapsed; // :421

        // 5. .shader-empty（FileManager.vue:411 的 :class ＋ :719）
        ApplyShaderEmpty();
        ApplyEmptyMinHeight();

        // 只有模组页会出现的两块（更新面板 :352、批量结果 :409）与 DupCleanModal（:473）在切走时强制收起
        ApplyGating();
    }

    /// <summary>对一个行/表头 Grid 落列轨与 Tag 显隐（原实现按 _grids 循环的那段，抽出来给生成行复用）。</summary>
    void ApplyKindToGrid(Grid g, bool mods, double[] w)
    {
        ColumnDefinitionCollection c = g.ColumnDefinitions;
        if (c.Count < 13) return;
        c[0].Width = new GridLength(w[0]);   // 复选框
        c[1].Width = new GridLength(w[1]);   // gap
        c[2].Width = new GridLength(w[2]);   // 图标
        c[3].Width = new GridLength(w[3]);   // gap
        c[4].Width = new GridLength(1, GridUnitType.Star);  // 名称
        c[5].Width = new GridLength(12);     // gap
        c[6].Width = new GridLength(w[4]);   // 大小
        c[7].Width = new GridLength(12);     // gap
        c[8].Width = new GridLength(w[5]);   // 修改时间
        c[9].Width = new GridLength(mods ? 12 : 0);  // gap
        c[10].Width = new GridLength(w[6]);  // 启用
        c[11].Width = new GridLength(mods ? 12 : 0); // gap
        c[12].Width = new GridLength(w[7]);  // 操作

        foreach (UIElement u in g.Children)
        {
            if (u is not FrameworkElement fe) continue;
            switch (fe.Tag as string)
            {
                case "mods-only":
                    fe.Visibility = mods ? Visibility.Visible : Visibility.Collapsed;
                    break;
                case "packs-only":
                    fe.Visibility = mods ? Visibility.Collapsed : Visibility.Visible;
                    break;
                case "file-icon":
                    fe.Width = w[2];
                    fe.Height = w[2];
                    fe.SetResourceReference(StyleProperty, "Icon.FmFile");
                    break;
            }
        }
    }

    /// <summary>
    /// .shader-empty 的卡片底：光影包且列表为空（且不在读取、无错误）时才收成透明无边框（FileManager.vue:411）。
    /// </summary>
    void ApplyShaderEmpty()
    {
        bool on = resourceKind == "shaderpacks" && !_hasListEntries && !_loading && _loadError is null;
        if (on)
        {
            FmCard.Background = Brushes.Transparent;
            FmCard.BorderThickness = new Thickness(0);
        }
        else
        {
            // 还原成 .card 的两条 DynamicResource（Panel 样式：Card 底 / Line 描边 1 / radius 14 / 无阴影）。
            FmCard.SetResourceReference(Border.BackgroundProperty, "Card");
            FmCard.SetResourceReference(Border.BorderBrushProperty, "Line");
            FmCard.BorderThickness = new Thickness(1);
        }
    }

    /// <summary>
    /// .shader-empty .empty 的 min-height 是 min(380px,45vh)（:719）；非光影页回到 120。
    /// 同一处顺带把 DupCleanModal 的 max-height 84vh 也算出来。
    /// </summary>
    void ApplyEmptyMinHeight()
    {
        double h = ActualHeight;
        EmptyBlock.MinHeight = resourceKind == "shaderpacks" && h > 1
            ? Math.Min(380, h * 0.45)
            : 120;
        if (h > 1) DupModal.MaxHeight = h * 0.84;
    }

    /// <summary>
    /// v-if 门控：布尔位 → 块级 Visibility。_hasCurrentVersion 为假时 fm-context / fm-controls / fm-card
    /// 一起退场，整页只剩 NoVersionCard（:347 的 currentVersion 门控，:237 注释）。
    /// </summary>
    void ApplyGating()
    {
        bool mods = resourceKind == "mods";
        bool hasVer = _hasCurrentVersion;
        // :347 —— 没有可选版本时整页只剩这张卡
        NoVersionCard.Visibility = hasVer ? Visibility.Collapsed : Visibility.Visible;
        FmContext.Visibility = hasVer ? Visibility.Visible : Visibility.Collapsed;
        FmControls.Visibility = mods && hasVer ? Visibility.Visible : Visibility.Collapsed;         // :400 + rel 门
        FmCard.Visibility = hasVer ? Visibility.Visible : Visibility.Collapsed;                     // :411
        UpdPanel.Visibility = mods && _updPanelOpen ? Visibility.Visible : Visibility.Collapsed;      // :352
        FmResults.Visibility = mods && _hasBatchResults ? Visibility.Visible : Visibility.Collapsed;   // :409
        StatusStrip.Visibility = _statusStripOn ? Visibility.Visible : Visibility.Collapsed;           // :412
        BtnStatusRetry.Visibility = _statusStripIsError ? Visibility.Visible : Visibility.Collapsed;   // :412 的 v-if loadError
        if (_statusStripIsError) StatusStrip.SetResourceReference(Border.BackgroundProperty, "DangerSoft");
        else StatusStrip.SetResourceReference(Border.BackgroundProperty, "AccentSoft");                 // ui-system.css:41-42
        // 四个弹窗（:459/:460/:462/:473）；DupCleanModal 的 v-if 还带 props.rel==='mods'
        ModVersionModalHost.Visibility = _versionModalOpen ? Visibility.Visible : Visibility.Collapsed;
        ModMigrationModalHost.Visibility = _migrationModalOpen ? Visibility.Visible : Visibility.Collapsed;
        ConfirmModalHost.Visibility = _confirmModalOpen ? Visibility.Visible : Visibility.Collapsed;
        DupCleanModalHost.Visibility = mods && _dupModalOpen ? Visibility.Visible : Visibility.Collapsed;
    }

    /// <summary>窗口/面板尺寸变化时重算 .shader-empty 的 min(380px,45vh)。</summary>
    void Page_SizeChanged(object sender, SizeChangedEventArgs e)
    {
        if (_ready) ApplyEmptyMinHeight();
    }

    /// <summary>
    /// 「⋯ / 更多」浮层开合（头部那颗仍走 XAML 的 Tag→FindName 路径；行内菜单由生成代码直挂事件）。
    /// </summary>
    void More_Click(object sender, RoutedEventArgs e)
    {
        if (sender is not Button b) return;
        if (FindName(b.Tag as string ?? string.Empty) is not Popup p) return;
        p.IsOpen = !p.IsOpen;
    }

    /// <summary>搜索框 placeholder ＋ 按输入重渲染（:401）。</summary>
    void Search_TextChanged(object sender, TextChangedEventArgs e)
    {
        SearchHint.Visibility = SearchBox.Text.Length == 0 ? Visibility.Visible : Visibility.Collapsed;
        if (_ready) RenderList();
    }
}
