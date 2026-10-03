using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;

namespace Wuhuicl.App.Pages;

/// <summary>
/// 联机 · 服务器页：KAMUCL-1.1.5/src/renderer/src/views/ServersView.vue（:356-380 正文）
/// 加共用组件层 components/connection/*.vue（全部几何在 connection.css）的 WPF 版式复刻。
/// 页顶多了一条 .connect-tabs 页签带（服务器 / 桥接 / 好友联机，配方 connection.css:76-79），
/// 只有「服务器」正文铺开，另两块是 Collapsed 占位壳，正文各归 BridgeView / FriendConnectView。
///
/// 本轮只铺版式：契约 §5 的 LaunchSession 白名单里没有任何服务器条目、.ping、实例关联字段，
/// 所以整页不写 Binding（绑不存在的属性会静默失效＝假实现），行与详情全部是写死的占位文案。
/// 事件都留在本页（契约 §1），不引用 MainWindow 的任何方法：页签互斥、批量多选、选中行高亮、
/// 搜索 placeholder、日志折叠都只是本地视觉状态。
/// </summary>
public partial class ServersPage : UserControl
{
    /// <summary>@media(min-width:1121px) 阈值 — ServersView.vue:510 与 :529 两处同一个断点。</summary>
    const double WideFrom = 1121;

    /// <summary>两列独立滚动的扣高：max-height calc(100vh - 270px) — ServersView.vue:510。</summary>
    const double ScrollDrop = 270;

    /// <summary>窄窗兜底：@max-1120 时 .server-list max-height 360px — ServersView.vue:530。</summary>
    const double NarrowListMax = 360;

    Border[] _rows = null!;
    Button[] _rowButtons = null!;
    TextBlock[] _rowTitles = null!;
    CheckBox[] _rowChecks = null!;

    Window? _host;
    bool _selectMode;
    bool _logOpen;
    /// <summary>全选框与行内复选框互写的闸门，避免 Checked 事件来回触发把状态刷成半选。</summary>
    bool _syncing;

    public ServersPage()
    {
        InitializeComponent();

        _rows = new[] { Row1, Row2, Row3, Row4 };
        _rowButtons = new[] { RowBtn1, RowBtn2, RowBtn3, RowBtn4 };
        _rowTitles = new[] { Title1, Title2, Title3, Title4 };
        _rowChecks = new[] { Chk1, Chk2, Chk3, Chk4 };

        Loaded += OnLoaded;
        Unloaded += OnUnloaded;
    }

    // ---------------- 断点与两列滚动高度 ----------------
    void OnLoaded(object sender, RoutedEventArgs e)
    {
        _host = Window.GetWindow(this);
        if (_host != null) _host.SizeChanged += Host_SizeChanged;
        ApplyActive(RowBtn1);
        ApplyMetrics();
    }

    void OnUnloaded(object sender, RoutedEventArgs e)
    {
        if (_host != null) _host.SizeChanged -= Host_SizeChanged;
        _host = null;
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

    // ---------------- 联机页签（服务器 / 桥接 / 好友联机） ----------------
    // ---------------- 列表选中行（active 态） ----------------
    void ServerRow_Click(object sender, RoutedEventArgs e)
    {
        if (sender is Button b) ApplyActive(b);
    }

    /// <summary>.server-list-item.active = accent-soft 底 + radius 6（ServersView.vue:515），
    /// 同一条规则还让 .server-row-copy strong 变 accent-2（:516）。</summary>
    void ApplyActive(Button chosen)
    {
        for (int i = 0; i < _rowButtons.Length; i++)
        {
            bool on = ReferenceEquals(_rowButtons[i], chosen);
            _rows[i].Style = (Style)_rows[i].FindResource(on ? "SrvRowActive" : "SrvRow");
            _rowTitles[i].SetResourceReference(TextBlock.ForegroundProperty, on ? "AccentHi" : "Fg");
        }
    }

    // ---------------- 批量管理（v-if="selectMode"，模板 :368） ----------------
    void BatchToggle_Click(object sender, RoutedEventArgs e)
    {
        _selectMode = !_selectMode;
        BtnBatch.Content = _selectMode ? "退出多选" : "批量管理";
        BatchBar.Visibility = _selectMode ? Visibility.Visible : Visibility.Collapsed;
        _syncing = true;
        foreach (var chk in _rowChecks)
        {
            chk.Visibility = _selectMode ? Visibility.Visible : Visibility.Collapsed;
            if (!_selectMode) chk.IsChecked = false;
        }
        SelectAllCheck.IsChecked = false;
        _syncing = false;
        UpdateBatchCount();
    }

    void SelectAll_Changed(object sender, RoutedEventArgs e)
    {
        if (_syncing) return;
        bool on = SelectAllCheck.IsChecked == true;
        _syncing = true;
        foreach (var chk in _rowChecks) chk.IsChecked = on;
        _syncing = false;
        UpdateBatchCount();
    }

    void RowCheck_Changed(object sender, RoutedEventArgs e)
    {
        if (_syncing) return;
        // 回写「全选」框时必须有闸门：SelectAllCheck 一变就触发 SelectAll_Changed，
        // 那里又会把所有行框按旧状态刷一遍，两边互相取消，最终停在半选的死状态。
        _syncing = true;
        SelectAllCheck.IsChecked = CountSelected() == _rowChecks.Length;
        _syncing = false;
        UpdateBatchCount();
    }

    int CountSelected()
    {
        int picked = 0;
        foreach (var chk in _rowChecks) if (chk.IsChecked == true) picked++;
        return picked;
    }

    /// <summary>「已选 N 项」+ 删除按钮的 disabled 条件，对应模板 :368 的 :disabled="!selectedCount"。
    /// // 占位：等联机功能接入（计数只统计本页占位行）</summary>
    void UpdateBatchCount()
    {
        int picked = CountSelected();
        BatchCountText.Text = $"已选 {picked} 项";
        BtnBatchDelete.IsEnabled = picked > 0;
    }

    // ---------------- 搜索框（placeholder 覆盖层，模板 :363） ----------------
    void Search_TextChanged(object sender, TextChangedEventArgs e)
        => SearchPh.Visibility = TxtSearch.Text.Length == 0 ? Visibility.Visible : Visibility.Collapsed;

    void ClearSearch_Click(object sender, RoutedEventArgs e) => TxtSearch.Clear();

    // ---------------- 收藏（★ / ☆ 字形互换，ServerListItem.vue:10） ----------------
    void Fav_Click(object sender, RoutedEventArgs e)
    {
        if (sender is not Button b) return;
        bool starred = (string)b.Content == "★";
        b.Content = starred ? "☆" : "★";
        // starred 类的字色是 accent（ServerListItem.vue:23），未收藏回落 text-dim
        b.SetResourceReference(Control.ForegroundProperty, starred ? "Fg2" : "Accent");
    }

    // ---------------- details/summary 折叠：同步与检测日志 ----------------
    void DetailsToggle_Click(object sender, RoutedEventArgs e)
    {
        _logOpen = !_logOpen;
        LogBlock.Visibility = _logOpen ? Visibility.Visible : Visibility.Collapsed;
        LogChevron.Style = (Style)LogChevron.FindResource(_logOpen ? "Icon.ChevronDown" : "Icon.ChevronRight");
    }

    // ---------------- 更多浮层（原版 details + absolute 浮块，ServerDetails.vue:24 / scoped :30） ----------------
    // WPF 没有 position absolute，这里先把删除项做成同一行的显隐；接线时可换成 Popup。
    void More_Click(object sender, RoutedEventArgs e)
        => BtnDelete.Visibility = BtnDelete.Visibility == Visibility.Visible
            ? Visibility.Collapsed : Visibility.Visible;

    // 供主控接线时定位：原版靠 JS 才成立的两处几何，这里都留了独立命名元素——
    // LogViewport（TerracottaPanel.vue:225 的 ref，日志要自动滚到底）与
    // ServerDetail / ServerListScroll / ServerDetailScroll（ServersView.vue:488 + :510 的 sticky 与两列独立滚动容器）。
    // 另外 EmptyAll / EmptyNoMatch / LoadErrorBar 是模板 :366 + :370 + :373 的三个 v-if 分支壳，
    // 数据接入后与 WorkspaceGrid 互斥显隐。// 占位：等联机功能接入
}
