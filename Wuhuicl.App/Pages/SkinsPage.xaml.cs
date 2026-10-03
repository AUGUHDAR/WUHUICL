using System.Windows;
using System.Windows.Controls;

namespace Wuhuicl.App.Pages;

/// <summary>
/// 皮肤与披风页：KAMUCL-1.1.5/src/renderer/src/views/SkinsView.vue 的 WPF 版式复刻。
/// 顶层是 UI_GEOMETRY_PAGES.md §6 的最终 grid（SkinsView.vue:1081-1109 那个后写的 scoped 块），
/// 不是 :602-1080 的 flex 基线。
/// 本轮只铺版式：契约 §5 的 LaunchSession 白名单里没有任何皮肤/披风/历史字段，
/// 所以页内不写 Binding（绑不存在的属性会静默失效，等于假实现），文案全部照原文静态占位。
/// </summary>
public partial class SkinsPage : UserControl
{
    Button _animActive;

    public SkinsPage()
    {
        InitializeComponent();
        Loaded += (_, _) => { ShowSkin(); PlaceAnimBlob(); };
        // 字体/主题变化会让 .seg-btn 宽度变，原版靠 ResizeObserver 重测（:63-87）
        AnimSegGrid.SizeChanged += (_, _) => PlaceAnimBlob();
    }

    /// <summary>3D 视口：SkinDoll3D（skinview3d 的 WPF 等价，参数 1:1），走首页同源的皮肤源。</summary>
    void ShowSkin()
    {
        SkinDoll3D.Skin = Services.SkinRenderer.SkinSource();
        SkinDoll3D.Slim = Services.SkinRenderer.IsSlimSkin();
        // .preview-3d-empty 是 3D 观察器的 v-else（SkinsView.vue:719-728）：人偶挂上后整块就不该存在，
        // 包括那圈 1px dashed 边框和 card-2 底。之前只藏了里面的占位文字，于是虚线框留在画面上。
        PreviewEmptyState.Visibility = System.Windows.Visibility.Collapsed;
    }

    // 分段控件：行走 / 待机（SkinsView.vue:377-392，previewAnim 默认 walk）
    void Seg_Click(object sender, System.Windows.RoutedEventArgs e)
    {
        _animActive = (Button)sender;
        bool walk = ReferenceEquals(_animActive, SegWalk);
        SegWalk.Style = (Style)SegWalk.FindResource(walk ? "SkinSegItemActive" : "SkinSegItem");
        SegIdle.Style = (Style)SegIdle.FindResource(walk ? "SkinSegItem" : "SkinSegItemActive");
        SkinDoll3D.Walk = walk;
        PlaceAnimBlob();
    }

    /// <summary>把 .seg-blob 摆到当前选中段上：量选中按钮相对 seg 内层 Grid 的矩形（等价 offsetLeft/offsetWidth）。
    /// 只定几何不做弹性过渡，过渡留给任务 #13。</summary>
    void PlaceAnimBlob()
    {
        _animActive ??= SegWalk;   // :59 默认 previewAnim = 'walk'
        if (_animActive.ActualWidth <= 0 || AnimSegGrid.ActualWidth <= 0) return;
        var b = _animActive.TransformToVisual(AnimSegGrid)
                    .TransformBounds(new Rect(0, 0, _animActive.ActualWidth, _animActive.ActualHeight));
        SegBlob.Width = b.Width;
        // top/bottom 各内缩 3px（:825-826），Grid 行高已含父 Border 的 padding 3
        SegBlob.Margin = new Thickness(b.X, 3, 0, 3);
        SegBlob.Opacity = 1;
    }

    // 剩余两件靠 JS 才成立的几何，本轮按契约 §6.8「结构留位、动画不做」保留占位：
    //
    // 1) 行走/待机的 blob 位移过渡：原版 left/width 走 190ms ease-out（SkinsView.vue:826-831），
    //    现在只做瞬定，接动效库时把 PlaceAnimBlob 改成补间即可。
    //
    // 2) 拖拽反馈 —— .row-main.drag-over{box-shadow:0 0 0 3px var(--accent-soft)}（:661-663）
    //    + .row-main.drag-over .pane{outline:2px solid var(--accent)}（:1102）。
    //    WPF 没有 outline，需要在外层套一层 Border 或用 Adorner 表达，故本轮不实现，
    //    落点已经写在 SkinsPage.xaml 的 pane-preview / skin-operation-panel 注释里。
}
