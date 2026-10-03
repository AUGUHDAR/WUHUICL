using System;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Media;

namespace Wuhuicl.App.Pages;

/// <summary>
/// 资源管理 · 模组页。原版 ModsView.vue:9-15 只是 <code>&lt;FileManager title rel …/&gt;</code> 的 16 行壳，
/// 几何全部来自 components/FileManager.vue，而资源包 / 光影包是同一个组件换 <c>data-resource</c>（FileManager.vue:332），
/// 所以本页做成一套壳：<see cref="ResourceKind"/> 只切列集、图标尺寸、专属单元显隐与三处文案，
/// 不读盘、不发消息、不引用 MainWindow（契约 §1）。列表数据未接入，四行是占位。
/// </summary>
public partial class ModsPage : UserControl
{
    // 表头 + 4 行共用同一套列轨（.fm-table-head,.fm-row 同列宽 — FileManager.vue:719）
    Grid[] _grids = Array.Empty<Grid>();
    bool _ready;

    string resourceKind = "mods";

    // ---------------- v-if 门控位 ----------------
    // 这几块在原版都由数据决定渲染与否（currentVersion / updatePanel / batchResults / dupOpen …），
    // 而 FileManager.vue 的数据层（fs:list、mods:catalog、mods:checkUpdates）还没接进 LaunchSession，
    // 所以这里给的是「门控布尔位 + 一次刷新」而不是绑定：默认值维持原版空数据下的形态（全部收起），
    // 接线时把对应位改真并调 ApplyGating()，块级可见性就跟着走，不需要动 XAML。
    bool _hasCurrentVersion = true;   // currentVersion（:341 fm-context / :347 card.empty / :400 fm-controls / :411 fm-card）
    bool _updPanelOpen;               // updatePanel.open（:352）
    bool _hasBatchResults;            // batchResults.length（:409）
    bool _statusStripOn;              // entries.length && (loading || loadError)（:412）
    bool _statusStripIsError;         // loadError（同一行的两态）
    bool _dupModalOpen;               // dupOpen（:473，仅 mods）
    bool _migrationModalOpen;         // migrationOpen（:460）
    bool _versionModalOpen;           // switchFile 非空（:459）
    bool _confirmModalOpen;           // delModal.open（:463）

    /// <summary>
    /// 资源类型，对应 FileManager.vue 的 props.rel：<c>mods</c>（默认）/ <c>resourcepacks</c> / <c>shaderpacks</c>。
    /// 赋值幂等且只做视觉切换，可反复设置；非法值一律回落成 mods。
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
            if (_ready) ApplyResourceKind();
        }
    }

    public ModsPage()
    {
        InitializeComponent();
        _grids = new[] { FmHead, FmRow1, FmRow2, FmRow3, FmRow4 };
        _ready = true;
    }

    /// <summary>
    /// 列集切换。两套都来自 FileManager.vue:719 的最终块：
    /// mods = <c>18px 32px minmax(0,1fr) 88px 132px 42px 40px</c>（7 列，含复选框与启用开关）；
    /// 资源包 / 光影 = 基础 <c>36px minmax(0,1fr) 88px 132px 40px</c>（5 列）。
    /// 这里用同一套 7 轨表达 5 列：复选框轨与启用轨给 0 宽并把单元收起，图标轨给缩略图 44×44
    /// （:623 区块的 .fm-thumb，mods 的 .fm-file-icon 是 32×32 — :719）。
    /// 残留近似：0 宽轨之间的 12px gap 在 WPF 里不会自动消掉，非 mods 时首列会多 12px 缩进。
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
        // 18/32/88/132/42/40 与 0/44/88/132/0/40 — :719（minmax(0,1fr) 单独给 *）
        double[] w = mods
            ? new[] { 18d, 32d, 88d, 132d, 42d, 40d }
            : new[] { 0d, 44d, 88d, 132d, 0d, 40d };
        string iconKey = resourceKind switch
        {
            "resourcepacks" => "Icon.ResourcePacks",
            "shaderpacks" => "Icon.Shaders",
            _ => "Icon.Mods"
        };

        foreach (Grid g in _grids)
        {
            ColumnDefinitionCollection c = g.ColumnDefinitions;
            c[0].Width = new GridLength(w[0]);
            c[1].Width = new GridLength(w[1]);
            c[2].Width = new GridLength(1, GridUnitType.Star);
            c[3].Width = new GridLength(w[2]);
            c[4].Width = new GridLength(w[3]);
            c[5].Width = new GridLength(w[4]);
            c[6].Width = new GridLength(w[5]);

            foreach (UIElement u in g.Children)
            {
                if (u is not FrameworkElement fe) continue;
                switch (fe.Tag as string)
                {
                    case "mods-only":
                        fe.Visibility = mods ? Visibility.Visible : Visibility.Collapsed;
                        break;
                    case "file-icon":
                        fe.Width = w[1];
                        fe.Height = w[1];
                        // 保持动态引用（契约 §3：图标只认 DynamicResource Icon.Y），切主题不丢
                        fe.SetResourceReference(StyleProperty, iconKey);
                        break;
                }
            }
        }

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

        BtnCheckUpdate.Visibility = mods ? Visibility.Visible : Visibility.Collapsed; // v-if rel==='mods' — :336
        BtnCtxRefresh.Visibility = mods ? Visibility.Collapsed : Visibility.Visible;  // v-if rel!=='mods' — :343
        // 头部那颗「更多」（.file-more，:338）与整条 .fm-controls（搜索 + 两个 SelectMenu + 刷新，:400）
        // 原版都是 v-if rel==='mods'：资源包 / 光影页不渲染，本轮补上门控。
        HeadMoreHost.Visibility = mods ? Visibility.Visible : Visibility.Collapsed;
        FmControls.Visibility = mods ? Visibility.Visible : Visibility.Collapsed;
        EmptyShadersHint.Visibility = resourceKind == "shaderpacks" ? Visibility.Visible : Visibility.Collapsed; // :421

        // 5. .shader-empty（FileManager.vue:411 的 :class ＋ :719）：光影包空列表时这张卡「什么都没有」——
        //    background transparent / border 0 / box-shadow none / backdrop-filter none，
        //    里面的 .empty 再换成 min-height min(380px,45vh)。
        //    原版的完整条件是 shaderpacks && !entries.length && !loading && !loadError，
        //    后三项要等 fs:list；这里先按 resourceKind 落壳，数据接入后把条件补全即可。
        ApplyShaderEmpty();
        ApplyEmptyMinHeight();

        // 只有模组页会出现的两块（更新面板 :352、批量结果 :409）与 DupCleanModal（:473）在切走时强制收起
        ApplyGating();
    }

    /// <summary>.shader-empty 的卡片底：光影包收成透明无边框，其余用主题画刷的动态引用（切主题不丢）。</summary>
    void ApplyShaderEmpty()
    {
        bool on = resourceKind == "shaderpacks";
        if (on)
        {
            FmCard.Background = Brushes.Transparent;
            FmCard.BorderThickness = new Thickness(0);
        }
        else
        {
            // 还原成 .card 的两条 DynamicResource（Panel 样式：Card 底 / Line 描边 1 / radius 14 / 无阴影）。
            // Border.Background / Border.BorderBrush 是 Border 自己注册的 DP，不能用 Control 的同名静态字段。
            FmCard.SetResourceReference(Border.BackgroundProperty, "Card");
            FmCard.SetResourceReference(Border.BorderBrushProperty, "Line");
            FmCard.BorderThickness = new Thickness(1);
        }
    }

    /// <summary>
    /// .shader-empty .empty 的 min-height 是 min(380px,45vh)（:719）。WPF 无视口单位，
    /// 45vh 按本页（.content 里那块 Grid，MainWindow.xaml:260）的实际高度算，窗口缩放时在 Page_SizeChanged 里重算。
    /// 非光影页回到 .content .empty 的 120（ui-system.css:44）。
    /// 同一处顺带把 DupCleanModal 的 max-height 84vh（DupCleanModal.vue:204-208）也算出来。
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
    /// v-if 门控：把上面那几位布尔值映射成块级 Visibility（XAML 里每块都注了原版行号）。
    /// 默认全 false ＝ 原版空数据下的样子；刷新中的 .status-strip 还要按 loadError 换底
    /// （.status-strip.error 是 danger-soft ＋ text 字色，ui-system.css:42）。
    /// </summary>
    void ApplyGating()
    {
        bool mods = resourceKind == "mods";
        // :347 —— 没有可选版本时整页只剩这张卡（.fm-context / .fm-controls / .fm-card 会一起退场，等数据接入再收）
        NoVersionCard.Visibility = _hasCurrentVersion ? Visibility.Collapsed : Visibility.Visible;
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
    /// 「⋯ / 更多」浮层开合：Tag 里放 Popup 的 x:Name，只动本地浮层状态，不做任何业务（契约 §1）。
    /// 原版是 details/summary 的原生开合（:448），WPF 用 Popup 承载 PopupCard + MenuItem2。
    /// </summary>
    void More_Click(object sender, RoutedEventArgs e)
    {
        if (sender is not Button b) return;
        if (FindName(b.Tag as string ?? string.Empty) is not Popup p) return;
        p.IsOpen = !p.IsOpen;
    }

    /// <summary>搜索框 placeholder：WPF 无原生占位，靠覆盖文字模拟（有内容就藏），对应 :401 的 placeholder。</summary>
    void Search_TextChanged(object sender, TextChangedEventArgs e)
        => SearchHint.Visibility = SearchBox.Text.Length == 0 ? Visibility.Visible : Visibility.Collapsed;
}
