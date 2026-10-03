using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;

namespace Wuhuicl.App.Pages;

/// <summary>
/// 社区资源页（复刻 KAMUCL-1.1.5 CommunityView.vue）。
/// 契约 §5 没有暴露任何检索结果属性，所以结果列表、分页文案全是写死的占位；
/// 本页只做本地交互（分类胶囊互斥 + 指示块定位 + 占位文字），不回调 MainWindow（契约 §1/§6.8）。
/// </summary>
public partial class CommunityPage : UserControl
{
    /// <summary>五个分类按钮，顺序同 kindTabs — CommunityView.vue:53-59。</summary>
    Button[] _capsules = null!;

    /// <summary>当前选中分类，初值是 mod — :157。</summary>
    Button _active = null!;

    public CommunityPage()
    {
        InitializeComponent();

        _capsules = [CapMod, CapModpack, CapResourcePack, CapShader, CapDataPack];
        _active = CapMod;
        // 原版默认高亮第一个（query.kind='mod' — :157），WPF 里初值已写在 XAML，这里只补一次量取
        Loaded += (_, _) => MoveBlob();
        // 字体/主题/换行都会改尺寸 → 原版挂 ResizeObserver 重算 — :76-84
        KindCapsules.SizeChanged += (_, _) => MoveBlob();
    }

    // ---------------- 分类胶囊（:473-485，样式 :734-767 / :1080 / :1083）----------------

    void Capsule_Click(object sender, RoutedEventArgs e)
    {
        if (sender is not Button chosen || ReferenceEquals(chosen, _active)) return;
        foreach (Button b in _capsules)
            // 选中态靠切 Style 完成（AccTab 同款做法），DynamicResource 留在样式里，切主题照样跟着走
            b.Style = (Style)b.FindResource(ReferenceEquals(b, chosen) ? "CapsuleActive" : "Capsule");
        _active = chosen;
        MoveBlob();
        // 原版这里还会 onFilterChange() 重新检索 — :481；检索未接入，本轮只改视觉。
    }

    /// <summary>
    /// 指示块 .capsule-blob 的 left/top/width/height 在原版全是 JS 内联样式（updateKindBlob — :64-74，kindBlobStyle — :89-95）。
    /// WPF 等价：把激活胶囊相对 Canvas 的偏移写成 Canvas.Left/Top + 自身宽高； opacity 初值 0 对应 kindBlob.on=false — :94。
    /// 动画（transition left/top/width/height — :751）等主控的 Storyboard 库，这里只保证结构与位置正确。
    /// </summary>
    void MoveBlob()
    {
        if (_active is null || CapsuleBlob is null || CapsulesHost is null) return;
        if (_active.ActualWidth <= 0 || _active.ActualHeight <= 0) return;

        GeneralTransform toHost = _active.TransformToVisual(CapsulesHost);
        Point origin = toHost.Transform(new Point(0, 0));
        Canvas.SetLeft(CapsuleBlob, origin.X);
        Canvas.SetTop(CapsuleBlob, origin.Y);
        CapsuleBlob.Width = _active.ActualWidth;
        CapsuleBlob.Height = _active.ActualHeight;
        CapsuleBlob.Opacity = 1;
    }

    // ---------------- 搜索行（:487-503）----------------

    void Keyword_TextChanged(object sender, TextChangedEventArgs e) =>
        PhKeyword.Visibility = TxtKeyword.Text.Length == 0 ? Visibility.Visible : Visibility.Collapsed;

    void Keyword_KeyDown(object sender, KeyEventArgs e)
    {
        // @keyup.enter="onSearch" — :492
        if (e.Key == Key.Enter) Search_Click(sender, e);
    }

    void Search_Click(object sender, RoutedEventArgs e)
    {
        // 占位：等社区检索接入 —— communitySearch 是网络功能（:212-228），契约 §5 无可绑属性，这里不假装出结果。
    }

    void Reset_Click(object sender, RoutedEventArgs e)
    {
        // .btn.btn-ghost 重置条件 — :502：本地字段清得掉的先清，其余等检索接入
        TxtKeyword.Clear();
        TxtVersion.Clear();
        foreach (Button b in _capsules)
            b.Style = (Style)b.FindResource(ReferenceEquals(b, CapMod) ? "CapsuleActive" : "Capsule");
        _active = CapMod;
        MoveBlob();
    }

    void Version_TextChanged(object sender, TextChangedEventArgs e) =>
        PhVersion.Visibility = TxtVersion.Text.Length == 0 ? Visibility.Visible : Visibility.Collapsed;

    // ---------------- 以下入口都指向未接入的能力，只做本地占位 ----------------

    void InstanceSelect_Click(object sender, RoutedEventArgs e)
    {
        // 占位：等 SelectMenu 下拉组件（:468 的选项来自已安装实例）
    }

    void UseInstance_Click(object sender, RoutedEventArgs e)
    {
        // 占位：等「使用当前实例」联动 store.selectedInstance — :469
    }

    void FilterSelect_Click(object sender, RoutedEventArgs e)
    {
        // 占位：等 SelectMenu 浮层（来源/加载器/排序，:506-:537）
    }

    void SourcePage_Click(object sender, RoutedEventArgs e)
    {
        // 占位：等外链打开（sourceUrl — :32-35，按钮见 :593-599）
    }

    void Mcmod_Click(object sender, RoutedEventArgs e)
    {
        // 占位：等 MC 百科检索（openMcmod — :37-41，按钮见 :600-606）
    }

    void Download_Click(object sender, RoutedEventArgs e)
    {
        // 下载模态本轮不做（弹窗库由主控统一做），入口先留着 — :608-615
    }

    void Page_Click(object sender, RoutedEventArgs e)
    {
        // 占位：等分页数据（goToPage — :244-248，按钮见 :632）
    }
}
