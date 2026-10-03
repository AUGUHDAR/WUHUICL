using System.Windows;
using System.Windows.Controls;

namespace Wuhuicl.App.Pages;

/// <summary>
/// 默认配置页：KAMUCL-1.1.5/src/renderer/src/views/KeysView.vue 的 WPF 版式复刻（options.txt / 默认按键预设）。
/// 几何按 docs/UI_GEOMETRY_PAGES.md §9 的最终值：分类导航取 :273 覆盖块（flex + gap 8 + 下边框 + 靠底色选中），
/// 不取 :229 基线里被覆盖掉的 grid 三列 / padding 18 20 / radius 14；内容网格 :244；行 :249；值胶囊 :254-256；
/// 重置钮 :262-267；清除钮 :270。
/// 本轮只铺版式：契约 §5 的 LaunchSession 白名单里没有任何 options / keybinding 字段，
/// 所以页内一条 Binding 都没写（绑不存在的属性会静默失效＝假实现），行内容全是静态占位文案。
/// 事件也不碰外壳（契约 §1）：分类切换只是本地换样式 + 切显隐。
/// </summary>
public partial class KeysPage : UserControl
{
    public KeysPage()
    {
        InitializeComponent();
        // 原版 section 的初值就是 'game' — KeysView.vue:15
        ShowSection(SectionGame);
    }

    // ---------------- 分类导航（selectSection，KeysView.vue:16）----------------
    void Section_Click(object sender, RoutedEventArgs e)
    {
        if (ReferenceEquals(sender, SectionKeys)) ShowSection(SectionKeys);
        else if (ReferenceEquals(sender, SectionPacks)) ShowSection(SectionPacks);
        else ShowSection(SectionGame);
    }

    void ShowSection(Button chosen)
    {
        SetSection(SectionGame, PanelGame, chosen);
        SetSection(SectionKeys, PanelKeys, chosen);
        SetSection(SectionPacks, PanelPacks, chosen);
    }

    static void SetSection(Button tab, FrameworkElement panel, Button chosen)
    {
        bool on = ReferenceEquals(tab, chosen);
        // 选中态整块换样式：background accent-soft + color text（:273 的 button.active，没有指示条）。
        // DynamicResource 留在样式 setter 里，所以切主题仍然即时生效（契约 §4）。
        tab.Style = (Style)tab.FindResource(on ? "CfgSectionTabActive" : "CfgSectionTab");
        // 三个分区互斥，对应模板 :155 / :175 / :156 的 v-show
        panel.Visibility = on ? Visibility.Visible : Visibility.Collapsed;
    }

    // ---------------- 搜索框（.cfg-search，:243）----------------
    // 只做 placeholder 覆盖文字的收放；按 keySearch 过滤 keyGrouped 的逻辑（:57-65）属未接入部分。
    void Search_TextChanged(object sender, TextChangedEventArgs e)
        => SearchHint.Visibility = TxtKeySearch.Text.Length == 0
            ? Visibility.Visible
            : Visibility.Collapsed;

    // ---------------- 原版靠 JS / CSS 才成立、本轮留位的几处 ----------------
    // 1) 捕获态 —— startCapture()/onCaptureKey()/onCaptureMouse()（:70-99）在 window 上挂 keydown/mousedown
    //    监听并把 capturing 写到具体那一行。XAML 里已留三个独立命名节点：
    //    CaptureRow（那一行）、CaptureBind（.cfg-bind.capturing，z-index 9001 — :271）、
    //    BtnClearCapture（.cfg-clear 30×30 — :270）。接线时把这三件套在行间搬动即可。
    //
    // 2) CaptureMask —— .cfg-capture-mask 是 position:fixed / inset:0 / z-index:9000 的全屏遮罩
    //    （模板 :221 + 样式 :269），WPF 的 fixed 定位与跨容器 z 轴 stacking 都表达不了：
    //    Panel.ZIndex 只在本 Panel 的子项之间排序，压不到 ScrollViewer 之外的窗口层。
    //    所以本页只留一个 0 尺寸锚点，真遮罩要由主控放进窗口的浮层（与 Toasts/模态同一层）。
    //
    // 3) 断点 —— §9 记的三个媒体查询（@max-850 内容网格收单列 :245；@max-760 行 wrap + label flex-basis 100%
    //    + small 隐藏；@max-700 导航 gap 8 + small 隐藏 :230）在 WPF 里没有对应机制。
    //    需要宿主在 SizeChanged 时改 ColumnDefinition 集合与分组容器的 Orientation。
    //
    // 4) 另外几处 CSS 特性缺口，均在 XAML 注释里就地标注：
    //    color-mix(in srgb, accent 55%, border)（:260）→ 退到 Accent 描边；
    //    box-shadow 0 0 0 3px accent-soft（:261）→ 需 Adorner；
    //    outline 2px + background accent-soft 的拖拽高亮（:232）→ WPF 无 outline，需外套一层 Border；
    //    font-weight 650（:248）→ WPF 只有 600/700 两档，取 SemiBold。
}
