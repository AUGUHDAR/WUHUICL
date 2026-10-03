using System;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Threading;

namespace Wuhuicl.App.Pages;

/// <summary>
/// 资源管理 · 录像页，复刻 KAMUCL-1.1.5/src/renderer/src/views/RecordingsView.vue。
/// 几何一律取 docs/UI_GEOMETRY_PAGES.md §10 的「最终值」，也就是该文件 :92 那个后写的 scoped 块
/// （:91 的基线只在被覆盖处作括号注记），根选择器是 <c>.recordings-page</c> 而不是 <c>.page</c>。
/// 契约 §5 的 LaunchSession 白名单里没有任何录像属性，所以本页不写一个 Binding（绑不存在的属性等于假实现），
/// 四行录像、库路径、分页文案都是照原版格式写死的占位。
/// 页内事件只动本页的可见性 / 文案 / 可用性，不发 IPC、不引用 MainWindow（契约 §1）：
/// 筛选对应 :21 的 filtered 计算属性，勾选对应 :39 的整页反选与 :26 的「改筛选就清空选择」，
/// 读取中对应 :27-32 与 :76 的 loading + inert。
/// </summary>
public partial class RecordingsPage : UserControl
{
    /// <summary>每页 40 条 — RecordingsView.vue:22。本轮只有 4 行占位，页数恒为 1，
    /// 所以分页条按 :83 的 v-if="pages &gt; 1" 常态收起，只保留结构与文案换算。</summary>
    const int PageSize = 40;

    Border[] _rows = Array.Empty<Border>();
    CheckBox[] _checks = Array.Empty<CheckBox>();
    TextBlock[] _names = Array.Empty<TextBlock>();
    TextBlock[] _paths = Array.Empty<TextBlock>();

    // InitializeComponent 里 SelectedIndex="0" 会当场触发 SelectionChanged，那时数组还没装配，必须先挡住
    bool _ready;

    // 只是把「读取中」这套态演一遍给主控看形状：真实 loading 由 recordings:list 的 await 决定（:27-32）
    DispatcherTimer? _busyTimer;

    public RecordingsPage()
    {
        InitializeComponent();
        _rows = new[] { Row1, Row2, Row3, Row4 };
        _checks = new[] { RowCheck1, RowCheck2, RowCheck3, RowCheck4 };
        _names = new[] { RowName1, RowName2, RowName3, RowName4 };
        _paths = new[] { RowPath1, RowPath2, RowPath3, RowPath4 };
        _ready = true;

        ApplyFilter();
        UpdateSelection();
        Unloaded += (_, _) => _busyTimer?.Stop();
    }

    /// <summary>下拉当前项文案，等价于 SelectMenu 的 currentLabel（SelectMenu.vue:25-28）。</summary>
    static string Item(ComboBox box) => (box.SelectedItem as ComboBoxItem)?.Content as string ?? string.Empty;

    /// <summary>
    /// 筛选四件事全在原版 :21 的 filtered 里：文件夹前缀、格式、位置、关键词（名称 + 来源 + 目录拼串后不区分大小写包含）。
    /// 顺手把 :last-child 的无边框（:91）现算在「最后一条可见行」上而不是写死在第四行，
    /// 否则筛掉第 4 行后，剩下的末行会带着不该有的下边线。
    /// </summary>
    void ApplyFilter()
    {
        if (!_ready) return;

        string q = SearchBox.Text.Trim();
        string kind = Item(KindSelect);
        string source = Item(SourceSelect);
        string folder = Item(FolderSelect);
        // folderOptions 的 label 是「name · path」（:16），真正拿来筛的是 path 段
        int split = folder.LastIndexOf(" · ", StringComparison.Ordinal);
        string prefix = split < 0 ? string.Empty : folder[(split + 3)..];

        int visible = 0;
        int lastIndex = -1;
        for (int i = 0; i < _rows.Length; i++)
        {
            string[] meta = (_rows[i].Tag as string ?? string.Empty).Split('|');
            string hay = $"{_names[i].Text} {meta[1]} {_paths[i].Text}";
            bool ok = (kind.Length == 0 || kind == "全部格式" || meta[0] == kind)
                      && (source.Length == 0 || source == "全部位置" || meta[1] == source)
                      && (prefix.Length == 0 || _paths[i].Text.StartsWith(prefix, StringComparison.OrdinalIgnoreCase))
                      && (q.Length == 0 || hay.Contains(q, StringComparison.OrdinalIgnoreCase));

            _rows[i].Visibility = ok ? Visibility.Visible : Visibility.Collapsed;
            if (ok) { lastIndex = i; visible++; }
            // 改筛选就清空勾选 — :26 watch([query, kind, source, folderFilter])
            else if (_checks[i].IsChecked == true) _checks[i].IsChecked = false;
        }

        for (int i = 0; i < _rows.Length; i++)
            _rows[i].BorderThickness = i == lastIndex ? new Thickness(0) : new Thickness(0, 0, 0, 1);

        bool hasRows = visible > 0;
        RecordingList.Visibility = hasRows ? Visibility.Visible : Visibility.Collapsed;
        ControlsRow.Visibility = hasRows ? Visibility.Visible : Visibility.Collapsed; // v-if filtered.length — :68
        EmptyBlock.Visibility = hasRows ? Visibility.Collapsed : Visibility.Visible;   // v-else-if !rows.length && !error — :75

        // :75 的两条文案按 data.entries.length（总量）二选一；静态数据下总量是 4，所以只走「没有匹配」那条，
        // 另一条作为「目录真的为空」的结构占位留着（等录像功能接入）
        bool noData = _rows.Length == 0;
        EmptyTextNoMatch.Visibility = noData ? Visibility.Collapsed : Visibility.Visible;
        EmptyTextNoData.Visibility = noData ? Visibility.Visible : Visibility.Collapsed;
        ClearFilterBtn.Visibility = noData ? Visibility.Collapsed : Visibility.Visible;

        SelectAllBtn.Content = $"全选筛选结果（{visible}）";                             // :69 的 filtered.length
        int pages = Math.Max(1, (int)Math.Ceiling(visible / (double)PageSize));         // :22
        PageText.Text = $"1 / {pages} · {visible} 个录像";                              // :83
        PaginationRow.Visibility = pages > 1 ? Visibility.Visible : Visibility.Collapsed;
        PrevPageBtn.IsEnabled = false;    // :83 的 :disabled="page &lt;= 1"（每页 40，本轮没有第二页可去）
        NextPageBtn.IsEnabled = pages > 1; // :83 的 :disabled="page &gt;= pages"
    }

    /// <summary>
    /// 勾选态只换算成三处：已选计数（:69 的 span）、清空按钮（v-if selected.length — :69）、
    /// 操作行（v-if selected.length — :70）。
    /// </summary>
    void UpdateSelection()
    {
        if (!_ready) return;

        int n = 0;
        foreach (CheckBox c in _checks)
            if (c.IsChecked == true) n++;

        SelectedCount.Text = $"已选 {n} 项";
        OperationRow.Visibility = n > 0 ? Visibility.Visible : Visibility.Collapsed;
        ClearSelectionBtn.Visibility = n > 0 ? Visibility.Visible : Visibility.Collapsed;
    }

    /// <summary>筛选控件的统一出口：SelectionChanged 与 TextChanged 都是 RoutedEventArgs 子类，共用一个处理器签名。</summary>
    void Filter_Changed(object sender, RoutedEventArgs e)
    {
        ApplyFilter();
        UpdateSelection(); // 筛选一变 selected 归零 — :26
    }

    /// <summary>搜索框 placeholder：WPF 无原生占位，用覆盖文字模拟（有内容就藏），对应 :62 的 placeholder。</summary>
    void Search_TextChanged(object sender, TextChangedEventArgs e)
    {
        SearchHint.Visibility = SearchBox.Text.Length == 0 ? Visibility.Visible : Visibility.Collapsed;
        Filter_Changed(sender, e);
    }

    void Row_Check_Changed(object sender, RoutedEventArgs e) => UpdateSelection();

    /// <summary>
    /// 「选择当前页」的整页反选 — :39 selectPage：当前页 ids 全选中就整页取消，否则并集。
    /// 每页 40 条（:22），所以「当前页」就是本页全部可见行。
    /// </summary>
    void SelectPage_Click(object sender, RoutedEventArgs e)
    {
        bool allOn = true;
        foreach (Border row in _rows)
            if (row.Visibility == Visibility.Visible && _checks[Array.IndexOf(_rows, row)].IsChecked != true) allOn = false;

        foreach (Border row in _rows)
        {
            if (row.Visibility != Visibility.Visible) continue;
            _checks[Array.IndexOf(_rows, row)].IsChecked = !allOn;
        }
        UpdateSelection();
    }

    /// <summary>「全选筛选结果」是无条件全选可见行 — :69 的 selected = filtered.map(e =&gt; e.id)。</summary>
    void SelectAll_Click(object sender, RoutedEventArgs e)
    {
        foreach (Border row in _rows)
            if (row.Visibility == Visibility.Visible) _checks[Array.IndexOf(_rows, row)].IsChecked = true;
        UpdateSelection();
    }

    /// <summary>清空选择 — :69 的 selected = []。</summary>
    void ClearSelection_Click(object sender, RoutedEventArgs e)
    {
        foreach (CheckBox c in _checks) c.IsChecked = false;
        UpdateSelection();
    }

    /// <summary>清除筛选（空态那颗按钮，:75）：搜索框清空 + 三个下拉回首项，列表与空态由 ApplyFilter 重算。</summary>
    void ClearFilter_Click(object sender, RoutedEventArgs e)
    {
        SearchBox.Text = string.Empty;
        FolderSelect.SelectedIndex = 0;
        KindSelect.SelectedIndex = 0;
        SourceSelect.SelectedIndex = 0;
        ApplyFilter();
        UpdateSelection();
    }

    /// <summary>「复制到实例」只有选了目标实例才可点 — :70 的 :disabled="busy || target === ''"。</summary>
    void Target_Changed(object sender, SelectionChangedEventArgs e)
    {
        if (!_ready) return;
        CopyToInstanceBtn.IsEnabled = TargetSelect.SelectedIndex > 0;
    }

    /// <summary>
    /// details 的开合：原版三处 summary 都是原生 details（使用说明 :58、部分目录未读取 :67、上次操作结果 :85），
    /// WPF 没有对应控件，所以做成 RecSummary 按钮 + Tag 指正文块名字，只切 Visibility。
    /// 原生 marker 的 90° 旋转属动效，本轮不做（契约 §6.8），模板里已留 Tag="SummaryChevron" 的独立元素待接。
    /// </summary>
    void Disclosure_Toggle_Click(object sender, RoutedEventArgs e)
    {
        if (sender is not Button b) return;
        if (FindName(b.Tag as string ?? string.Empty) is not UIElement body) return;
        body.Visibility = body.Visibility == Visibility.Visible ? Visibility.Collapsed : Visibility.Visible;
    }

    /// <summary>
    /// 刷新：原版调 recordings:list（:27），按钮 :disabled="busy || loading" 且文案随 loading 变「读取中…」（:57），
    /// 列表还带 inert（:76），全局 [inert] 规则给到 opacity .65（ui-system.css:43）。
    /// 本轮没有 IPC 可调，就把这套视觉态演 1.2 秒后自行还原；错误卡（:66）的「重试」共用这条路径。
    /// </summary>
    void Refresh_Click(object sender, RoutedEventArgs e)
    {
        if (_busyTimer is null)
        {
            _busyTimer = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(1200) };
            _busyTimer.Tick += (_, _) => { _busyTimer!.Stop(); SetBusy(false); };
        }
        SetBusy(true);
        _busyTimer.Stop();
        _busyTimer.Start();
    }

    /// <summary>读取中 / 空闲两套视觉态：按钮文案与禁用、status-strip（:73）、列表 inert（:76 + ui-system.css:43）。</summary>
    void SetBusy(bool busy)
    {
        BtnRefresh.Content = busy ? "读取中…" : "↻ 刷新"; // :57
        BtnRefresh.IsEnabled = !busy;                     // :57 的 :disabled="busy || loading"
        BtnImport.IsEnabled = !busy;                      // :57 的 :disabled="busy"
        ErrorCard.IsEnabled = !busy;                      // :66 重试的 :disabled
        StatusStrip.Visibility = busy ? Visibility.Visible : Visibility.Collapsed;
        RecordingList.Opacity = busy ? 0.65 : 1;
        foreach (CheckBox c in _checks) c.IsEnabled = !busy; // :78 的 :disabled="busy"
    }
}
