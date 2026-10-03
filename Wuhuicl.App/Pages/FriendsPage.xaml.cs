using System.Windows;
using System.Windows.Controls;

namespace Wuhuicl.App.Pages;

/// <summary>
/// 联机（主导航项）：KAMUCL-1.1.5/src/renderer/src/views/FriendConnectView.vue 的 WPF 复刻。
///
/// 页内状态机而不是页签页：type ConnectPage = 'choose' | 'frp' | 'voxlink' | 'terracotta'（:11-12），
/// choose 是方式选择落地页（:54-81），点卡片调 pick(card.key)（:43-45）进方式页（:84-101），
/// 方式页的「← 更换方式」把状态写回 choose（:94）。WPF 用 ChooseView / MethodView 两个兄弟容器互斥显隐表达，
/// 方式页标题与描述取自被点卡片自己那份写死的文案，避免同一段话在 XAML 与 C# 各存一份。
///
/// 三个面板（FrpPanel / VoxLinkPanel / TerracottaPanel，模板 :98-100）本轮只铺小节标题占位，正文另开工。
///
/// 契约 §5 的 LaunchSession 白名单里没有任何联机方式、隧道、房间码字段，所以整页零 Binding；
/// 事件都留在本页（契约 §1），不引用 MainWindow 的任何方法。唯一的代码期工作是
/// @container(min-width:960px) 这条 WPF 表达不了的断点（:161）。
/// </summary>
public partial class FriendsPage : UserControl
{
    /// <summary>container query 阈值：.friend-connect-page 自身 inline-size 960px — FriendConnectView.vue:161。</summary>
    const double WideFrom = 960;

    /// <summary>窄档列表列数：@container 未命中时 .pick-list 是 minmax(0,1fr) 单列（:161）。</summary>
    const int NarrowColumns = 1;

    /// <summary>宽档列表列数：repeat(3, minmax(0,1fr))（:161）。</summary>
    const int WideColumns = 3;

    /// <summary>三张卡与三个面板的对应关系，顺序即 methodCards 的声明顺序（:16 / :25 / :33）。
    /// 元组的元素名必须写死在字段类型上，否则带名字的元组数组无法从字面量隐式转换（CS0029）。</summary>
    (string Key, TextBlock Name, TextBlock Desc, Border Panel)[] _methods = null!;

    Window? _host;

    public FriendsPage()
    {
        InitializeComponent();

        _methods = new[]
        {
            ("frp", PickName1, PickDesc1, MethodFrp),
            ("voxlink", PickName2, PickDesc2, MethodVoxlink),
            ("terracotta", PickName3, PickDesc3, MethodTerracotta),
        };

        Loaded += OnLoaded;
        Unloaded += OnUnloaded;
    }

    void OnLoaded(object sender, RoutedEventArgs e)
    {
        _host = Window.GetWindow(this);
        if (_host != null) _host.SizeChanged += Host_SizeChanged;
        ShowChoose();
        ApplyMetrics();
    }

    void OnUnloaded(object sender, RoutedEventArgs e)
    {
        if (_host != null) _host.SizeChanged -= Host_SizeChanged;
        _host = null;
    }

    void Host_SizeChanged(object? sender, SizeChangedEventArgs e) => ApplyMetrics();

    /// <summary>container-type:inline-size（:161）让断点量的是本页容器宽度而不是视口，
    /// 所以这里直接用 PageRoot.ActualWidth；设计期没有宿主窗口时按外壳宽反推
    /// （侧栏 208 + 页边距 48，同 ServersPage.xaml.cs 的 ViewportWidth 思路）。</summary>
    double ContainerWidth()
    {
        double w = PageRoot.ActualWidth;
        return w > 0 ? w : (_host?.ActualWidth ?? 1280) - 256;
    }

    /// <summary>把 @container(min-width:960px) 落成真实列数（:161 的 minmax(0,1fr) 与 repeat(3,minmax(0,1fr)) 两档）。
    /// 卡片内部只铺宽档的竖排分支（flex-direction column / gap 14），窄档的横排 gap 24（:127）不另开一套，
    /// 这是本轮已知的版式偏差，接线时可以在此处一并换模板。</summary>
    void ApplyMetrics()
    {
        PickList.Columns = ContainerWidth() >= WideFrom ? WideColumns : NarrowColumns;
    }

    // ---------------- choose 与方式页的互斥切换（模板 :54 与 :84 的 v-if/v-else） ----------------
    void Pick_Click(object sender, RoutedEventArgs e)
    {
        if (sender is not Button { Tag: string key }) return;
        foreach (var m in _methods)
        {
            if (m.Key != key) continue;
            // h1 = currentCard.name、p = currentCard.desc（模板 :90-91），直接取卡片自己那份文案
            MethodTitle.Text = m.Name.Text;
            MethodDesc.Text = m.Desc.Text;
            MethodView.Visibility = Visibility.Visible;
            ChooseView.Visibility = Visibility.Collapsed;
            m.Panel.Visibility = Visibility.Visible;
            return;
        }
    }

    void Back_Click(object sender, RoutedEventArgs e) => ShowChoose();

    /// <summary>page 状态回到 choose：卡片列表显示、方式页与三个面板全部收起。</summary>
    void ShowChoose()
    {
        ChooseView.Visibility = Visibility.Visible;
        MethodView.Visibility = Visibility.Collapsed;
        foreach (var m in _methods) m.Panel.Visibility = Visibility.Collapsed;
    }

    // 供主控接线时定位：原版靠 :51 的 Transition name="subpage" :duration="200" 做右滑渐入，
    // 契约 §6.8 本轮不做动画，两个模式容器都留了 x:Name（ChooseView / MethodView）当动画挂点；
    // 卡片 hover 的描边换色在 PickCard 模板里（x:Name=CardBorder），:149/:159 的 overshoot 缓动留在这里接。
    // MethodFrp / MethodVoxlink / MethodTerracotta 是三个面板的占位壳（// 占位：等联机功能接入）。
}
