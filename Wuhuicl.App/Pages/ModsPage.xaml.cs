using System;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;

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
