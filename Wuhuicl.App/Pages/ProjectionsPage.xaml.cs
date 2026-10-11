using System.IO;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Documents;
using System.Windows.Input;
using System.Windows.Media;
using Microsoft.Win32;
using Wuhuicl.App.Services;

namespace Wuhuicl.App.Pages;

/// <summary>
/// 资源管理 · 投影页，复刻 KAMUCL-1.1.10/src/renderer/src/views/ProjectionsView.vue。
/// 与录像页共用同一族样式（:58 的根就挂着 recordings 类），结构、几何、事件形态对齐 RecordingsPage。
/// 列表已接真实后端 <see cref="ProjectionsService"/>：扫 &lt;游戏根&gt;\projections（集中收藏，递归）
/// + &lt;root&gt;\versions\&lt;id&gt;\schematics\*.litematic | *.schem | *.schematic | *.nbt（顶层），按修改时间降序。
/// 行由 <see cref="BuildRow"/> 按占位行结构逐项生成（PrjRow / Check / PrjName / PrjMeta / PrjPath / PrjRowBtn），
/// 筛选/勾选/分页语义与原版一致（:23 filtered、:28 改筛选清空、:24 每页 40）。
/// 操作：定位 = explorer /select；移入回收站 = 确认后 Microsoft.VisualBasic FileSystem；导入 = OpenFileDialog
/// 复制进集中收藏，同名自动加序号；「转换」（ProjectionConvert，:59）本轮未接入，按钮保持原样。
/// </summary>
public partial class ProjectionsPage : UserControl
{
    /// <summary>每页 40 条 — ProjectionsView.vue:24。</summary>
    const int PageSize = 40;

    readonly ProjectionsService _service = new();

    // ---------------- 真实列表状态 ----------------
    List<MediaEntry> _all = [];      // 全量扫描结果（modified 降序）
    List<MediaEntry> _view = [];     // 筛选后的全集（分页的源）
    List<string> _warnings = [];
    string? _loadError;
    List<(Border Row, CheckBox Check, MediaEntry Entry)> _rowItems = [];
    int _page = 1;
    int _generation;   // 重入保护：只认最后一次 reload 的回包
    bool _loading;
    bool _ready;
    bool _everLoaded;
    bool _suppressFilter;
    Run? _countRun;    // 页头「… N 个」的 Run（原版 data.entries.length，:60）

    public ProjectionsPage()
    {
        InitializeComponent();
        // XAML 里的四行是假数据占位：真实列表由 BuildRow 生成，先拆掉
        RecordingList.Children.Clear();

        BtnOpenLibrary.Click += (_, _) => OpenDir(_service.LibraryDir);
        BtnImport.Click += Import_Click;
        HookTrashButton();
        PrevPageBtn.Click += (_, _) => { if (_page > 1) { _page--; ApplyFilter(); } };
        NextPageBtn.Click += (_, _) => { if (_page < PagesOf(_view.Count)) { _page++; ApplyFilter(); } };

        Loaded += (_, _) =>
        {
            if (_everLoaded || _loading) return;
            _everLoaded = true;
            _ = ReloadAsync();
        };
        _ready = true;   // 数组装配完成后放行事件处理器
        ApplyFilter();
        UpdateSelection();
    }

    /// <summary>下拉当前项文案，等价于 SelectMenu 的 currentLabel（SelectMenu.vue:25-28）。</summary>
    static string Item(ComboBox box) => (box.SelectedItem as ComboBoxItem)?.Content as string ?? string.Empty;

    static int PagesOf(int count) => Math.Max(1, (int)Math.Ceiling(count / (double)PageSize));

    // ================= 数据加载 =================

    /// <summary>projections:list（:29）：后台扫目录，回包后重建行、库路径、文件夹下拉与告警块。</summary>
    async Task ReloadAsync()
    {
    try
    {
        if (_loading) return;
        _loading = true;
        int gen = ++_generation;
        SetBusy(true);
        try
        {
            var res = await Task.Run(_service.Scan);
            if (gen != _generation) return;
            _loadError = null;
            _all = res.Entries;
            _warnings = res.Warnings;
            _page = 1;
            LibraryPathText.Text = _service.LibraryDir; // data.library（:61）
            UpdateHeaderCount();
            UpdateWarnings();
            PopulateFolderSelect();
            ApplyFilter();
            UpdateSelection();
        }
        catch (Exception ex)
        {
            if (gen != _generation) return;
            _loadError = ex.Message;
            ApplyFilter();
        }
        finally
        {
            if (gen == _generation)
            {
                _loading = false;
                SetBusy(false);
            }
        }
    }
    catch (Exception ex) { MainWindow.Diag("projections-ReloadAsync EX: " + ex); }
    }


    /// <summary>页头计数：data.entries.length（:60 的「4 个」占位 Run）。</summary>
    void UpdateHeaderCount()
    {
        _countRun ??= FindCountRun();
        if (_countRun is not null) _countRun.Text = $" {_all.Count} 个";
    }

    /// <summary>页头标题里那个「 N 个」Run 是 XAML 占位（无 x:Name），靠特征找：双 Inline 且末 Inline 以「 个」结尾。</summary>
    Run? FindCountRun()
    {
        foreach (var tb in Descendants<TextBlock>(this))
            if (tb.Inlines.Count == 2 && tb.Inlines.LastInline is Run r && r.Text.EndsWith(" 个"))
                return r;
        return null;
    }

    /// <summary>部分目录未读取（:70 details）：文案换成真实警告，条数为 0 时整块收起。</summary>
    void UpdateWarnings()
    {
        WarningBody.Children.Clear();
        for (int i = 0; i < _warnings.Count; i++)
        {
            var line = new TextBlock
            {
                FontSize = 14,
                TextWrapping = TextWrapping.Wrap,
                Margin = i == 0 ? new Thickness(0, 0, 0, 0) : new Thickness(0, 4, 0, 0),
                Text = _warnings[i],
            };
            line.SetResourceReference(TextBlock.ForegroundProperty, "Fg2");
            WarningBody.Children.Add(line);
        }
        WarningSummary.Content = $"部分目录未读取（{_warnings.Count}）";
        WarningBlock.Visibility = _warnings.Count > 0 ? Visibility.Visible : Visibility.Collapsed;
    }

    /// <summary>folderOptions（:18）：「名 · 路径」，名按来源（集中收藏 / 版本 id / 游戏目录），筛选用路径段。</summary>
    void PopulateFolderSelect()
    {
        _suppressFilter = true;
        FolderSelect.Items.Clear();
        FolderSelect.Items.Add(new ComboBoxItem { Content = "全部已绑定文件夹" });
        foreach (var dir in _all.Select(e => e.Directory).Distinct())
        {
            var first = _all.First(e => e.Directory == dir);
            string label = first.Library ? "集中收藏" : first.VersionId ?? "游戏目录";
            FolderSelect.Items.Add(new ComboBoxItem { Content = $"{label} · {dir}", ToolTip = dir });
        }
        FolderSelect.SelectedIndex = 0;
        _suppressFilter = false;
    }

    // ================= 列表行 =================

    /// <summary>当前页切片（PageSize=40，:24）→ 逐项生成行。</summary>
    void RebuildRows()
    {
        RecordingList.Children.Clear();
        _rowItems.Clear();
        foreach (var e in _view.Skip((_page - 1) * PageSize).Take(PageSize))
        {
            var (row, check) = BuildRow(e);
            RecordingList.Children.Add(row);
            _rowItems.Add((row, check, e));
        }
    }

    /// <summary>按占位行的结构建一行：checkbox / .recording-info（名称+元信息+目录）/ 定位文件 + 转换双按钮（:83）。</summary>
    (Border Row, CheckBox Check) BuildRow(MediaEntry e)
    {
        var check = new CheckBox
        {
            Style = (Style)FindResource("Check"),
            VerticalAlignment = VerticalAlignment.Center,
            ToolTip = $"选择 {e.Name}",
        };
        check.Checked += Row_Check_Changed;
        check.Unchecked += Row_Check_Changed;

        var name = new TextBlock { Style = (Style)FindResource("PrjName"), Text = e.Name, ToolTip = e.Name };
        var meta = new TextBlock
        {
            Style = (Style)FindResource("PrjMeta"),
            Margin = new Thickness(0, 6, 0, 0),
            // 元信息口径照 :82 的主干：.kind · 来源 · 大小 · 修改时间（方块数/游戏版本占位段没有真实数据源，略）
            Text = $".{e.Kind} · {e.Source} · {RecordingsService.FormatSize(e.Size)} · {e.Modified:yyyy/M/d HH:mm:ss}",
        };
        var path = new TextBlock
        {
            Style = (Style)FindResource("PrjPath"),
            Margin = new Thickness(0, 6, 0, 0),
            Text = e.Directory,
            ToolTip = e.FullPath,
        };
        var info = new StackPanel
        {
            Margin = new Thickness(16, 0, 16, 0),
            Cursor = Cursors.SizeAll, // :94 draggable → WPF 无 grab 光标，取 SizeAll（与占位一致）
            ToolTip = "按住拖出投影文件（拖放需 projections:drag，:38 未接入）",
        };
        info.Children.Add(name);
        info.Children.Add(meta);
        info.Children.Add(path);

        var reveal = new Button
        {
            Style = (Style)FindResource("PrjRowBtn"),
            Content = "定位文件",
            ToolTip = "在资源管理器中定位该投影文件",
            Tag = e,
            Margin = new Thickness(0, 0, 8, 0),
        };
        reveal.Click += Reveal_Click;
        // 「转换」= ProjectionConvert（:59），本轮未接入，保留占位按钮
        var convert = new Button { Style = (Style)FindResource("PrjRowBtn"), Content = "转换" };
        var buttons = new StackPanel { Orientation = Orientation.Horizontal, VerticalAlignment = VerticalAlignment.Center };
        buttons.Children.Add(reveal);
        buttons.Children.Add(convert);

        var grid = new Grid();
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        Grid.SetColumn(info, 1);
        Grid.SetColumn(buttons, 2);
        grid.Children.Add(check);
        grid.Children.Add(info);
        grid.Children.Add(buttons);

        return (new Border { Style = (Style)FindResource("PrjRow"), Tag = $"{e.Kind}|{e.Source}", Child = grid }, check);
    }

    // ================= 筛选 / 选择 =================

    /// <summary>
    /// 筛选四件事全在原版 :23 的 filtered 里：文件夹前缀、格式、位置、关键词。
    /// 格式下拉显示 .litematic（:66 的 '.'+v），条目 kind 存的是不带点的扩展名，比较前把点去掉。
    /// </summary>
    void ApplyFilter()
    {
        if (!_ready) return;

        string q = SearchBox.Text.Trim();
        string kind = Item(KindSelect).TrimStart('.'); // :66 的四扩展名档去掉前导点
        string source = Item(SourceSelect);
        string folder = Item(FolderSelect);
        int split = folder.LastIndexOf(" · ", StringComparison.Ordinal);
        string prefix = split < 0 ? string.Empty : folder[(split + 3)..];

        _view = _all.Where(en =>
            (kind.Length == 0 || kind == "全部格式" || en.Kind == kind)
            && (source.Length == 0 || source == "全部位置" || en.Source == source)
            && (prefix.Length == 0 || en.Directory.StartsWith(prefix, StringComparison.OrdinalIgnoreCase))
            && (q.Length == 0 || $"{en.Name} {en.Source} {en.Directory}".Contains(q, StringComparison.OrdinalIgnoreCase)))
            .ToList();

        int pages = PagesOf(_view.Count);
        if (_page > pages) _page = pages;
        RebuildRows();

        for (int i = 0; i < _rowItems.Count; i++) // :last-child 无边框由最后一条渲染行承担
            _rowItems[i].Row.BorderThickness = i == _rowItems.Count - 1 ? new Thickness(0, 0, 0, 0) : new Thickness(0, 0, 0, 1);

        bool hasRows = _rowItems.Count > 0;
        bool hasData = _all.Count > 0;
        RecordingList.Visibility = hasRows ? Visibility.Visible : Visibility.Collapsed;
        ControlsRow.Visibility = hasRows ? Visibility.Visible : Visibility.Collapsed; // v-if filtered.length — :71
        ErrorCard.Visibility = _loadError is not null ? Visibility.Visible : Visibility.Collapsed; // v-if error — :69
        EmptyBlock.Visibility = !hasRows && _loadError is null ? Visibility.Visible : Visibility.Collapsed; // :78

        // :78 的两条文案按 data.entries.length（总量）二选一
        EmptyTextNoMatch.Visibility = hasData ? Visibility.Visible : Visibility.Collapsed;
        EmptyTextNoData.Visibility = hasData ? Visibility.Collapsed : Visibility.Visible;
        ClearFilterBtn.Visibility = hasData ? Visibility.Visible : Visibility.Collapsed;

        SelectAllBtn.Content = $"全选筛选结果（{_view.Count}）"; // :72 的 filtered.length
        PageText.Text = $"{_page} / {pages} · {_view.Count} 个投影"; // :86
        PaginationRow.Visibility = pages > 1 ? Visibility.Visible : Visibility.Collapsed;
        PrevPageBtn.IsEnabled = _page > 1 && !_loading;   // :86 的 :disabled="page &lt;= 1"
        NextPageBtn.IsEnabled = _page < pages && !_loading; // :86 的 :disabled="page &gt;= pages"
    }

    /// <summary>
    /// 勾选态只换算成三处：已选计数（:72 的 span）、清空按钮（v-if selected.length）、
    /// 操作行（v-if selected.length — :73）。
    /// </summary>
    void UpdateSelection()
    {
        if (!_ready) return;

        int n = _rowItems.Count(r => r.Check.IsChecked == true);
        SelectedCount.Text = $"已选 {n} 项";
        OperationRow.Visibility = n > 0 ? Visibility.Visible : Visibility.Collapsed;
        ClearSelectionBtn.Visibility = n > 0 ? Visibility.Visible : Visibility.Collapsed;
    }

    /// <summary>筛选控件的统一出口（SelectionChanged 与 TextChanged 共用）；下拉重建期间不触发。</summary>
    void Filter_Changed(object sender, RoutedEventArgs e)
    {
        if (_suppressFilter) return;
        ApplyFilter();
        UpdateSelection(); // 筛选一变 selected 归零 — :28
    }

    /// <summary>搜索框 placeholder：WPF 无原生占位，用覆盖文字模拟（有内容就藏），对应 :65 的 placeholder。</summary>
    void Search_TextChanged(object sender, TextChangedEventArgs e)
    {
        SearchHint.Visibility = SearchBox.Text.Length == 0 ? Visibility.Visible : Visibility.Collapsed;
        Filter_Changed(sender, e);
    }

    void Row_Check_Changed(object sender, RoutedEventArgs e) => UpdateSelection();

    /// <summary>「选择当前页」的整页反选 — :41 selectPage：当前页全部选中就整页取消，否则并集。</summary>
    void SelectPage_Click(object sender, RoutedEventArgs e)
    {
        bool allOn = _rowItems.All(r => r.Check.IsChecked == true);
        foreach (var r in _rowItems) r.Check.IsChecked = !allOn;
        UpdateSelection();
    }

    /// <summary>「全选筛选结果」全选可见行（行只渲染当前页）— :72 的 selected = filtered.map(e =&gt; e.id)。</summary>
    void SelectAll_Click(object sender, RoutedEventArgs e)
    {
        foreach (var r in _rowItems) r.Check.IsChecked = true;
        UpdateSelection();
    }

    /// <summary>清空选择 — :72 的 selected = []。</summary>
    void ClearSelection_Click(object sender, RoutedEventArgs e)
    {
        foreach (var r in _rowItems) r.Check.IsChecked = false;
        UpdateSelection();
    }

    /// <summary>清除筛选（空态那颗按钮，:78）：搜索框清空 + 三个下拉回首项，列表与空态由 ApplyFilter 重算。</summary>
    void ClearFilter_Click(object sender, RoutedEventArgs e)
    {
        SearchBox.Text = string.Empty;
        FolderSelect.SelectedIndex = 0;
        KindSelect.SelectedIndex = 0;
        SourceSelect.SelectedIndex = 0;
        ApplyFilter();
        UpdateSelection();
    }

    /// <summary>「复制到实例」只有选了目标实例才可点 — :73 的 :disabled="busy || target === ''"。</summary>
    void Target_Changed(object sender, SelectionChangedEventArgs e)
    {
        if (!_ready) return;
        CopyToInstanceBtn.IsEnabled = TargetSelect.SelectedIndex > 0;
    }

    /// <summary>details 的开合（使用说明 :61、部分目录未读取 :70、上次操作结果 :88 的 WPF 等价）。</summary>
    void Disclosure_Toggle_Click(object sender, RoutedEventArgs e)
    {
        if (sender is not Button b) return;
        if (FindName(b.Tag as string ?? string.Empty) is not UIElement body) return;
        body.Visibility = body.Visibility == Visibility.Visible ? Visibility.Collapsed : Visibility.Visible;
    }

    // ================= 操作 =================

    /// <summary>projections:open（:42）：定位单行文件（explorer /select）。</summary>
    void Reveal_Click(object sender, RoutedEventArgs e)
    {
        if (sender is Button { Tag: MediaEntry entry }) RecordingsService.Reveal(entry.FullPath);
    }

    /// <summary>「打开收藏文件夹」= projections:open 无参分支（:42）：explorer 打开集中收藏。</summary>
    void OpenDir(string dir)
    {
        try
        {
            Directory.CreateDirectory(dir);
            System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo("explorer.exe", dir)
            {
                UseShellExecute = true,
            });
        }
        catch (Exception ex) { MessageBox.Show($"打开文件夹失败（{ex.Message}）", "投影", MessageBoxButton.OK, MessageBoxImage.Warning); }
    }

    /// <summary>projections:import（:48）：OpenFileDialog 选四扩展名投影，复制进集中收藏。</summary>
    void Import_Click(object sender, RoutedEventArgs e)
    {
        if (_loading) return;
        var dlg = new OpenFileDialog
        {
            Title = "导入投影",
            Multiselect = true,
            Filter = "Litematic|*.litematic|Schem|*.schem|Schematic|*.schematic|结构方块导出|*.nbt|全部文件|*.*",
        };
        if (dlg.ShowDialog() != true) return;
        _ = ImportAsync(dlg.FileNames);
    }

    async Task ImportAsync(string[] files)
    {
        _loading = true;
        int gen = ++_generation;
        SetBusy(true);
        var results = new List<(string Name, bool Ok, string Detail)>();
        try
        {
            MediaScanResult res = await Task.Run(() =>
            {
                foreach (var f in files)
                {
                    string dest = _service.Import(f, out var err);
                    results.Add(dest.Length > 0
                        ? (Path.GetFileName(f), true, dest)
                        : (Path.GetFileName(f), false, err ?? "导入失败"));
                }
                return _service.Scan();
            });
            if (gen != _generation) return;
            _loadError = null;
            _all = res.Entries;
            _warnings = res.Warnings;
            _page = 1;
            UpdateHeaderCount();
            UpdateWarnings();
            PopulateFolderSelect();
            ApplyFilter();
            UpdateSelection();
            ShowResults(results);
        }
        finally
        {
            if (gen == _generation)
            {
                _loading = false;
                SetBusy(false);
            }
        }
    }

    /// <summary>上次操作结果（:88）：「name：已完成 → path」，失败行套 .failed 的 danger 色。</summary>
    void ShowResults(List<(string Name, bool Ok, string Detail)> results)
    {
        ResultsBody.Children.Clear();
        for (int i = 0; i < results.Count; i++)
        {
            var (name, ok, detail) = results[i];
            var line = new TextBlock { FontSize = 14, TextWrapping = TextWrapping.Wrap, Margin = i == 0 ? new Thickness(0, 0, 0, 0) : new Thickness(0, 4, 0, 0) };
            if (ok)
            {
                line.Inlines.Add(new Run($"{name}：已完成"));
                line.Inlines.Add(new Run($" → {detail}") { FontSize = 12 });
                line.SetResourceReference(TextBlock.ForegroundProperty, "Fg2");
            }
            else
            {
                line.Text = $"{name}：{detail}";
                line.SetResourceReference(TextBlock.ForegroundProperty, "Danger");
            }
            ResultsBody.Children.Add(line);
        }
        ResultsBlock.Visibility = results.Count > 0 ? Visibility.Visible : Visibility.Collapsed;
    }

    /// <summary>「移入回收站」按钮在 XAML 里没挂事件（原版等确认弹窗），代码里找到它接上。</summary>
    void HookTrashButton()
    {
        foreach (var child in OperationRow.Children)
            if (child is Button b && b.Content as string == "移入回收站")
            {
                b.Click += Trash_Click;
                break;
            }
    }

    /// <summary>projections:operate trash（:73/:89）：确认后把勾选行逐个移入系统回收站，再重扫。</summary>
    void Trash_Click(object sender, RoutedEventArgs e)
    {
        var picked = _rowItems.Where(r => r.Check.IsChecked == true).Select(r => r.Entry).ToList();
        if (picked.Count == 0) return;
        if (MessageBox.Show($"确定把选中的 {picked.Count} 个投影移入回收站？", "移入回收站",
                MessageBoxButton.YesNo, MessageBoxImage.Question) != MessageBoxResult.Yes) return;

        var errors = new List<string>();
        foreach (var en in picked)
        {
            try
            {
                if (File.Exists(en.FullPath)) RecordingsService.Recycle(en.FullPath);
            }
            catch (Exception ex) { errors.Add($"{en.Name}：{ex.Message}"); }
        }
        if (errors.Count > 0)
            MessageBox.Show(string.Join("\n", errors), "部分投影删除失败", MessageBoxButton.OK, MessageBoxImage.Warning);
        _ = ReloadAsync();
    }

    /// <summary>刷新（:29 projections:list / :69 的重试共用）：真实重扫，「读取中」态由 _loading 驱动。</summary>
    void Refresh_Click(object sender, RoutedEventArgs e)
    {
        if (_loading) return;
        _loadError = null;
        _ = ReloadAsync();
    }

    /// <summary>读取中 / 空闲两套视觉态：按钮文案与禁用、status-strip（:76）、列表 inert（:79 + ui-system.css:43）。</summary>
    void SetBusy(bool busy)
    {
        BtnRefresh.Content = busy ? "读取中…" : "↻ 刷新"; // :60
        BtnRefresh.IsEnabled = !busy;                     // :60 的 :disabled="busy || loading"
        BtnImport.IsEnabled = !busy;                      // :60 的 :disabled="busy"
        BtnOpenLibrary.IsEnabled = !busy;
        ErrorCard.IsEnabled = !busy;                      // :69 重试的 :disabled
        StatusStrip.Visibility = busy ? Visibility.Visible : Visibility.Collapsed;
        RecordingList.Opacity = busy ? 0.65 : 1;          // 全局 [inert] 的 opacity .65
        PrevPageBtn.IsEnabled = _page > 1 && !busy;
        NextPageBtn.IsEnabled = _page < PagesOf(_view.Count) && !busy;
        foreach (var r in _rowItems) r.Check.IsEnabled = !busy; // :81 的 :disabled="busy"
    }

    // ---------------- 树遍历（页头计数 Run 定位用） ----------------

    static IEnumerable<T> Descendants<T>(DependencyObject root) where T : DependencyObject
    {
        int n = VisualTreeHelper.GetChildrenCount(root);
        for (int i = 0; i < n; i++)
        {
            var child = VisualTreeHelper.GetChild(root, i);
            if (child is T hit) yield return hit;
            foreach (var sub in Descendants<T>(child)) yield return sub;
        }
    }
}
