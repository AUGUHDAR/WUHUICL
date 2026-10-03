using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Animation;
using System.Windows.Media.Imaging;
using System.Windows.Threading;
using Wuhuicl.App.Services;
using Wuhuicl.Core.Stores;

namespace Wuhuicl.App;

public partial class MainWindow : Window
{
    readonly LaunchSession _s = new();
    readonly List<BitmapSource> _banners = new();
    DispatcherTimer? _bannerTimer;
    int _bannerIndex;
    bool _frontA = true;

    public MainWindow()
    {
        InitializeComponent();
        DataContext = _s;
        Diag($"ctor W={Width} H={Height} Min={MinWidth}");
        SourceInitialized += (_, _) =>
        {
            Native.RoundCorners(this);
            Diag($"source-init W={Width} Actual={ActualWidth}x{ActualHeight} " +
                 $"wpfScale={VisualTreeHelper.GetDpi(this).DpiScaleX:0.##}");
            // 混合 DPI 双屏下窗口可能落到屏外（实测见过 5440×3440 压在两屏之外），起窗先夹一次
            Services.ScreenFit.FitToMonitor(this);
        };
        Loaded += OnLoaded;
        StateChanged += (_, _) => UpdateMaxGlyph();
        // 弹窗宽度里的 100vw/vh 减项要跟窗口尺寸走（ApplyViewportCaps）
        SizeChanged += (_, _) => ApplyViewportCaps();
        // 原版 watch(store.logs.length)：日志窗开着时把 .log-body 滚到底（HomeView.vue:255-262）。
        // 要 BeginInvoke：Extents 是排版之后才算得出的，同步调 ScrollToEnd 会滚不过去。
        _s.LogLines.CollectionChanged += (_, _) =>
        {
            if (LogOverlay.Visibility != Visibility.Visible) return;
            Dispatcher.BeginInvoke(() => LogScroll.ScrollToEnd(), System.Windows.Threading.DispatcherPriority.Background);
        };
    }

    async void OnLoaded(object sender, RoutedEventArgs e)
    {
        // XAML 里 NavHome IsChecked=True 触发的 Checked 事件在解析期 PageHome 还没赋值而提前返回，
        // 顶栏按钮显隐（App.vue:1333/1359）要等这里补一次 Show 才生效
        Show("home", sender as RadioButton);
        BindSettings();
        UpdateMaxGlyph();
        ApplyViewportCaps();
        // .input::placeholder 的覆盖层：TextBox 没有 placeholder，初值空就要露出提示字
        MpCustomName.TextChanged += (_, _) => MpCustomNameHint.Visibility =
            MpCustomName.Text.Length == 0 ? Visibility.Visible : Visibility.Collapsed;
        MpCustomNameHint.Visibility = Visibility.Visible;
        ApplyThemeFromSettings();
        LoadBanners();
        ApplyBrandArt();
        SkinDoll.Skin = SkinRenderer.SkinSource();
        SkinDoll.Slim = SkinRenderer.IsSlimSkin();
        PageHome.SkinModelText.Text = SkinDoll.Slim ? "纤细模型" : "经典模型";
        RefreshHero();
        OpenDebugPage();
        InitNavLayers();
        _ = Task.Run(async () =>
        {
            await Task.Delay(8000);
            Dispatcher.Invoke(() =>
            {
                var dpi = VisualTreeHelper.GetDpi(this);
                Diag($"t=8s state={WindowState} W={Width} H={Height} Actual={ActualWidth}x{ActualHeight} " +
                     $"screen={SystemParameters.PrimaryScreenWidth}x{SystemParameters.PrimaryScreenHeight} " +
                     $"dpi={dpi.DpiScaleX}x{dpi.DpiScaleY} ctx={DpiAwareness.Current().ToInt64()}");
            });
        });
        await _s.LoadOnline();
        RefreshHero();
        Diag("after LoadOnline");
    }

    static readonly Stopwatch _clock = Stopwatch.StartNew();

    /// <summary>临时探针：量渲染层级与每张图的真实解码开销，定位完就删。</summary>
    internal static void Diag(string line)
    {
        try
        {
            var p = System.Diagnostics.Process.GetCurrentProcess();
            var mb = (p.PrivateMemorySize64 / (1024 * 1024)).ToString();
            File.AppendAllText(@"D:\tmp\wudiag.txt",
                $"t={_clock.ElapsedMilliseconds}ms priv={mb}MB gc={GC.GetTotalMemory(false) / (1024 * 1024)}MB {line}\n");
        }
        catch { /* 探针不能影响功能 */ }
    }

    /// <summary>原版 hero 位是内置轮播图。缺图就留底色 #17231F，不硬撑。</summary>
    void LoadBanners()
    {
        Diag($"tier={RenderCapability.Tier} dpiScale={PresentationSource.FromVisual(this)?.CompositionTarget?.TransformToDevice.M11} " +
             $"W={Width} H={Height} Actual={ActualWidth}x{ActualHeight} Left={Left} Top={Top}");
        for (int i = 1; i <= 8; i++)
        {
            var img = SkinRenderer.LoadAsset($"hero/banner{i}.png");
            if (img is null) break;
            Diag($"banner{i} {img.PixelWidth}x{img.PixelHeight} dpi={img.DpiX}x{img.DpiY} " +
                 $"raw={img.PixelWidth * img.PixelHeight * 4 / 1048576}MB");
            _banners.Add(img);
        }
        if (_banners.Count == 0) return;
        SetHeroImage(HeroImageA, _banners[0]);
        ApplyHeroOffset();
        if (_banners.Count < 2) return;
        // 间隔默认 6.5s（shared/appearancePolicy.ts:5，夹在 1..120s），交叉淡入 500ms
        _bannerTimer = new DispatcherTimer { Interval = TimeSpan.FromSeconds(6.5) };
        _bannerTimer.Tick += (_, _) => StepBanner();
        _bannerTimer.Start();
    }

    void StepBanner()
    {
        Diag($"step enter slot={HeroStage.ActualWidth}x{HeroStage.ActualHeight}");
        _bannerIndex = (_bannerIndex + 1) % _banners.Count;
        var enter = _frontA ? HeroImageB : HeroImageA;
        var leave = _frontA ? HeroImageA : HeroImageB;
        SetHeroImage(enter, _banners[_bannerIndex]);
        Diag("after source set");
        var d = new Duration(TimeSpan.FromMilliseconds(500));
        enter.BeginAnimation(OpacityProperty, new DoubleAnimation(0, 1, d)
        { FillBehavior = FillBehavior.HoldEnd });
        leave.BeginAnimation(OpacityProperty, new DoubleAnimation(1, 0, d)
        { FillBehavior = FillBehavior.HoldEnd });
        _frontA = !_frontA;
        Diag("after animation begin");
    }

    internal void HeroStage_SizeChanged(object sender, SizeChangedEventArgs e) => ApplyHeroOffset();

    /// <summary>
    /// object-fit:cover + object-position:center 42% 的精确表达。
    /// 用 ImageBrush 的 Viewbox（相对图片归一化）裁出"看得见的那一块"，
    /// 而不是给 Image 挂 RenderTransform —— 后者的基准对齐取决于布局，
    /// 实测会在卡片顶部露出一条与 shift 等高的缝。
    /// </summary>
    void SetHeroImage(Border target, BitmapSource src)
    {
        target.Background = new ImageBrush(src)
        {
            Stretch = Stretch.Fill,
            TileMode = TileMode.None,
            ViewportUnits = BrushMappingMode.Absolute,
        };
        _heroBrushes[target] = (ImageBrush)target.Background;
        _heroSrc[target] = src;
    }

    readonly System.Collections.Generic.Dictionary<Border, ImageBrush> _heroBrushes = new();
    readonly System.Collections.Generic.Dictionary<Border, BitmapSource> _heroSrc = new();

    void ApplyHeroOffset()
    {
        double slotW = HeroStage.ActualWidth, slotH = HeroStage.ActualHeight;
        if (slotW <= 0 || slotH <= 0) return;
        foreach (var (border, brush) in _heroBrushes)
        {
            if (!_heroSrc.TryGetValue(border, out var bs)) continue;
            // cover 后的实际绘制尺寸（用 DIP，位图 DPI 不是 96 时 Pixel 尺寸会偏）
            double s = Math.Max(slotW / bs.Width, slotH / bs.Height);
            double drawnW = bs.Width * s, drawnH = bs.Height * s;
            // object-position: center 42% → 水平居中，垂直取溢出的 42%
            var box = new Rect(
                (drawnW - slotW) * .5 / drawnW,
                Math.Max(0, (drawnH - slotH) * .42) / drawnH,
                slotW / drawnW,
                slotH / drawnH);
            // ViewboxUnits 默认就是 RelativeToBoundingBox（对 Viewbox 而言即相对图片 0..1），不用显式设
            brush.Viewbox = box;
            brush.Viewport = new Rect(0, 0, slotW, slotH);
        }
    }

    /// <summary>
    /// <summary>
    /// 两层位置层的初始定位：水滴在没悬停时也停在选中项且不透明
    /// （useNavigationBubble.ts:22 的 target 为空时回落到 active），选条始终在选中项。
    /// 逐屏自测用：`--bubble=N` 把水滴钉在第 N 项、`--expand` 展开子菜单，
    /// 这样静态截图也能验证位移层的测量与几何（动画本身没法截图）。
    /// </summary>
    void InitNavLayers()
    {
        MoveSelection(instant: true);
        MoveBubble(ActiveNavItem(), 1);
        foreach (var a in Environment.GetCommandLineArgs())
        {
            if (a.Equals("--expand", StringComparison.OrdinalIgnoreCase))
            {
                NavResources.IsChecked = true;   // 触发 NavExpand_Changed 展开子菜单
                continue;
            }
            if (!a.StartsWith("--bubble=", StringComparison.OrdinalIgnoreCase)) continue;
            if (!int.TryParse(a[9..], out var i)) continue;
            var items = NavItems;
            if (i < 0 || i >= items.Length) continue;
            MoveBubble(items[i], 1);
            MoveSelection(instant: true);
        }
    }

    void UpdateMaxGlyph() => MaxIcon.Style = (Style)FindResource(
        WindowState == WindowState.Maximized ? "Icon.Restore" : "Icon.Maximize");

    /// <summary>
    /// 逐屏自测用：`Wuhuicl.App.exe --page=accounts` 直接打开某一页，
    /// `--open=log` 之类把只有交互才会出现的浮层直接点开（不然截图永远拍不到它们）。
    /// 只认已知名字，其它一律忽略，不影响正常使用。
    /// </summary>
    void OpenDebugPage()
    {
        foreach (var a in Environment.GetCommandLineArgs())
        {
            // 逐套主题截图核对用：不改动用户 settings.json
            if (a.StartsWith("--theme=", StringComparison.OrdinalIgnoreCase))
            {
                var id = a[8..].ToLowerInvariant();
                if (ThemeManager.Themes.Any(t => t.Id == id))
                {
                    ThemeManager.Apply(id);
                    if (ThemePanel != null) BuildThemeChips(id);
                    SetThemeNote(id);   // 截图核对时说明文字也要跟得上，否则看着像切了没生效
                }
                continue;
            }
            if (a.StartsWith("--open=", StringComparison.OrdinalIgnoreCase))
            {
                OpenDebugLayer(a[7..].ToLowerInvariant());
                continue;
            }
            if (!a.StartsWith("--page=", StringComparison.OrdinalIgnoreCase)) continue;
            var which = a[7..].ToLowerInvariant();
            switch (which)
            {
                case "home": NavHome.IsChecked = true; break;
                case "versions": NavVersions.IsChecked = true; break;
                case "settings": NavSettings.IsChecked = true; break;
                case "accounts": Show("accounts"); break;
                case "skins": NavSkin.IsChecked = true; break;
                case "community": NavCommunity.IsChecked = true; break;
                case "resources": NavResources.IsChecked = true; break;
                case "mods": NavResources.IsChecked = true; NavMods.IsChecked = true; break;
                case "packs": NavResources.IsChecked = true; NavPacks.IsChecked = true; break;
                case "shaders": NavResources.IsChecked = true; NavShaders.IsChecked = true; break;
                case "recordings": NavResources.IsChecked = true; NavRecordings.IsChecked = true; break;
                case "keys": NavDefaults.IsChecked = true; break;
                case "friends": NavOnline.IsChecked = true; break;
                // 服务器与 MOD 面板是资源管理子项：先展开父项再选中，否则子菜单是收起的
                case "servers": NavResources.IsChecked = true; NavServers.IsChecked = true; break;
                case "bridge": NavResources.IsChecked = true; NavBridge.IsChecked = true; break;
            }
        }
    }

    /// <summary>浮层的自测入口：原版这些都要点出来（日志窗是首页「运行状态」那一格，
    /// HomeView.vue:557），截图拍不到，所以给个开关直接开。
    /// log 是空日志态（.log-empty，:722），logfail 再叠上 launchFailed 那条 .log-failure（:717）。</summary>
    void OpenDebugLayer(string which)
    {
        switch (which)
        {
            case "log":
                ToggleLog_Click(this, new RoutedEventArgs());
                break;
            case "logfail":
                _s.LaunchFailed = true;
                foreach (var line in new[]
                {
                    "启动 Java：C:\\Java\\jdk-21\\bin\\javaw.exe",
                    "启动命令：java -Xmx2G -cp <classpath> net.minecraft.client.main.Main --username 测试",
                    "[stderr] Exception in thread \"main\" java.lang.NoClassDefFoundError: com/mojang/bridge/Session",
                    "游戏退出代码 1",
                }) _s.Log(line);
                ToggleLog_Click(this, new RoutedEventArgs());
                break;
            case "installed":
                // 本机一个实例都没装时，已安装行的版式永远拍不到（列表是空的）。
                // 只往内存集合塞两条假行，不动 SelectedInstalled、不写 settings.json，进程一退就没。
                _s.InstalledList.Add(new VersionRow { Id = "1.20.1", Label = "1.20.1", Subtitle = "正式版" });
                _s.InstalledList.Add(new VersionRow
                {
                    Id = "fabric-loader-0.15.11-1.20.4",
                    Label = "Fabric 1.20.4 这个实例名故意取长一点用来验证两行截断",
                    Kind = "loader", Subtitle = "Fabric 0.15.11",
                });
                NavVersions.IsChecked = true;
                PageVersions.TabInstalled.IsChecked = true;
                break;
            case "install":
                // 安装弹窗只能从版本行点出来；截图脚本没有可点的行（本机不一定装了实例），
                // 这里直接把弹窗撑开，数据全在控件内存里，不碰 versions 目录。
                NavVersions.IsChecked = true;
                PageVersions.DebugOpenInstall();
                break;
            case "creator":
                // 创作者卡的展开态只有 hover 才有，截图脚本没有指针，这里直接把它撑开
                PageHome.DebugOpenCreator();
                break;
        }
    }

    // ---------------- 窗口与拖拽 ----------------
    // 原版只有 72px 顶栏是 -webkit-app-region:drag，这里等价：顶栏按下拖动，双击切最大/还原。
    void Topbar_MouseDown(object sender, MouseButtonEventArgs e)
    {
        if (e.ClickCount == 2) { ToggleMax(); return; }
        if (e.ChangedButton == MouseButton.Left) DragMove();
    }

    void LogCard_Down(object sender, MouseButtonEventArgs e) => e.Handled = true;

    void ToggleMax() => WindowState = WindowState == WindowState.Maximized
        ? WindowState.Normal : WindowState.Maximized;

    // ---------------- 导航 ----------------
    /// <summary>访问栈：回退按钮的可见性与目标都从它来（原版是路由历史）。</summary>
    readonly List<(string Key, RadioButton? Item)> _visited = new();
    string _view = "";
    RadioButton? _viewItem;
    bool _suppressHistory;

    void Back_Click(object sender, RoutedEventArgs e)
    {
        if (_visited.Count == 0) return;
        var (key, item) = _visited[^1];
        _visited.RemoveAt(_visited.Count - 1);
        _suppressHistory = true;
        try
        {
            if (item is not null) item.IsChecked = true;   // 触发 Nav_Checked 走正常切换
            else Show(key);
        }
        finally { _suppressHistory = false; }
    }

    void Nav_Checked(object sender, RoutedEventArgs e)
    {
        if (PageHome is null) return;
        MoveSelection(sender as RadioButton);
        if (ReferenceEquals(sender, NavHome)) Show("home", sender as RadioButton);
        else if (ReferenceEquals(sender, NavVersions)) Show("versions", sender as RadioButton);
        else if (ReferenceEquals(sender, NavSettings)) Show("settings", sender as RadioButton);
        else if (ReferenceEquals(sender, NavSkin)) Show("skins", sender as RadioButton);
        else if (ReferenceEquals(sender, NavCommunity)) Show("community", sender as RadioButton);
        else if (ReferenceEquals(sender, NavDefaults)) Show("keys", sender as RadioButton);
        // 三个 view 各自独立：联机=FriendConnectView（主导航）、服务器=ServersView、MOD 面板=BridgeView（都是资源管理子项）
        else if (ReferenceEquals(sender, NavOnline)) Show("friends", sender as RadioButton);
        else if (ReferenceEquals(sender, NavServers)) Show("servers", sender as RadioButton);
        else if (ReferenceEquals(sender, NavBridge)) Show("bridge", sender as RadioButton);
        else if (ReferenceEquals(sender, NavMods)) { PageMods.ResourceKind = "mods"; Show("mods", sender as RadioButton); }
        else if (ReferenceEquals(sender, NavPacks)) { PageMods.ResourceKind = "resourcepacks"; Show("mods", sender as RadioButton); }
        else if (ReferenceEquals(sender, NavShaders)) { PageMods.ResourceKind = "shaders"; Show("mods", sender as RadioButton); }
        else if (ReferenceEquals(sender, NavRecordings)) Show("recordings", sender as RadioButton);
        else ShowStub(sender);
    }

    /// <summary>页面显隐表驱动：加一页只改这里，不再堆 if-else。
    /// item 是触发本次切换的导航项，回退时要按它恢复选中（mods/packs/shaders 共用一页，只按 key 回不去）。</summary>
    void Show(string which, RadioButton? item = null)
    {
        if (_view != which)
        {
            // 原版 canGoBack 走的是路由历史（App.vue:1321）；回退那一步不产生新历史
            if (_view.Length > 0 && !_suppressHistory) _visited.Add((_view, _viewItem));
            _view = which;
            _viewItem = item;
            if (which == "home") _visited.Clear();   // 首页是根，回到首页就没有可回退的了
        }
        BtnTopBack.Visibility = which != "home" && _visited.Count > 0
            ? Visibility.Visible : Visibility.Collapsed;
        var pages = new FrameworkElement[]
        {
            PageHome, PageVersions, PageSettings, PageAccounts, PageSkins, PageCommunity,
            PageMods, PageKeys, PageRecordings, PageServers, PageBridge, PageFriends, PageStub,
        };
        foreach (var p in pages) p.Visibility = Visibility.Collapsed;
        (which switch
        {
            "home" => (FrameworkElement)PageHome,
            "versions" => PageVersions,
            "settings" => PageSettings,
            "accounts" => PageAccounts,
            "skins" => PageSkins,
            "community" => PageCommunity,
            "mods" => PageMods,
            "keys" => PageKeys,
            "recordings" => PageRecordings,
            "servers" => PageServers,
            "bridge" => PageBridge,
            "friends" => PageFriends,
            _ => PageStub,
        }).Visibility = Visibility.Visible;
        // App.vue:1333/1342 下载与导入都只在非首页出现；:1359 更新日志图标钮只在首页出现；
        // :1351 首页那颗下载图标钮要「首页 且 有活动任务」，跟着 ApplyTaskBadge 一起算
        bool home = which == "home";
        BtnTopDownload.Visibility = home ? Visibility.Collapsed : Visibility.Visible;
        BtnTopImport.Visibility = home ? Visibility.Collapsed : Visibility.Visible;
        BtnTopHistory.Visibility = home ? Visibility.Visible : Visibility.Collapsed;
        ApplyTaskBadge();
        ApplyResourceGroup();
        if (home) RefreshHero();
    }

    /// <summary>Nav_Checked 的兜底：只给"还没接页面的导航项"用。
    /// 曾经在这里挂着的联机/皮肤/社区资源占位文案都已经换成真页面，别再往回退。</summary>
    void ShowStub(object sender)
    {
        Show("stub");
        StubTitle.Text = "该模块尚未接入";
        StubText.Text = "本轮只交付基础链路。";
        StubIcon.Style = (Style)FindResource("Icon.Settings");
    }

    /// <summary>
    /// 资源组联动（App.vue:192-193 的 inResourceGroup ＋ :1267-1285 的绑定）：
    /// 当前视图落在 mods/packs/shaders/bridge/servers/recordings 里时，父项「资源管理」
    /// 既要保持展开（子菜单与尖角的 open 都是 resourceExpanded || inResourceGroup），
    /// 字色还要转 text（.nav-parent.group-active，App.vue:1881）。
    /// 不在组里时要 ClearValue 而不是赋 null：Foreground=null 是「用空笔刷画」，
    /// 图标与文字会整行隐形（实测就是这样把侧栏那一行抹白的），只有 ClearValue 才是「清掉本地值、
    /// 回到样式与模板触发器自己的取色」（hover 转 text 也还在）。
    /// </summary>
    static readonly string[] ResourceGroupViews = { "mods", "packs", "shaders", "bridge", "servers", "recordings" };

    void ApplyResourceGroup()
    {
        if (NavResources is null) return;
        bool inGroup = ResourceGroupViews.Contains(_view);
        if (inGroup) NavResources.Foreground = (Brush)FindResource("Fg");
        else NavResources.ClearValue(Control.ForegroundProperty);
        if (inGroup && NavResources.IsChecked != true) NavResources.IsChecked = true;
    }

    internal void NavVersionsFromButton(object sender, RoutedEventArgs e)
    {
        NavVersions.IsChecked = true;
        Show("versions", sender as RadioButton);
    }

    // ---------------- 导航水滴 / 选条（原版 .nav-bubble + .nav-selection 是两个绝对定位层） ----------------
    /// <summary>导航里所有"可选中页面项"，含资源管理的三个子项（父项是展开开关，不参与选中）。</summary>
    RadioButton[] NavItems => new[]
    {
        NavHome, NavVersions, NavMods, NavPacks, NavShaders, NavRecordings, NavBridge, NavServers,
        NavOnline, NavDefaults, NavSkin, NavCommunity, NavSettings,
    };

    RadioButton? ActiveNavItem() => NavItems.FirstOrDefault(r => r.IsChecked == true);

    void NavItem_Enter(object sender, RoutedEventArgs e) => MoveBubble(sender as FrameworkElement, 1);

    /// <summary>离开时水滴回到选中项而不是淡掉：原版 target 为空时 bounds 回落到 active 项
    /// （useNavigationBubble.ts:22），所以任何时候 active 上都有那块底色。</summary>
    void NavItem_Leave(object sender, RoutedEventArgs e) => MoveBubble(ActiveNavItem(), 1);

    double _subOpenHeight = double.NaN;

    /// <summary>.nav-sub 的 grid-template-rows 0fr→1fr 在 WPF 无对应，按 190ms ease-out 做高度动画。</summary>
    void NavExpand_Changed(object sender, RoutedEventArgs e)
    {
        if (!IsLoaded) return;
        // 原版子菜单的 open 条件是 resourceExpanded || inResourceGroup（App.vue:1285／:1278）：
        // 人已经停在资源子页里时收不起来，这里把父项按回去（会重入一次本函数，走展开分支）
        if (NavResources.IsChecked != true && ResourceGroupViews.Contains(_view))
        {
            NavResources.IsChecked = true;
            return;
        }
        bool open = NavResources.IsChecked == true;
        if (open)
        {
            NavSubList.Visibility = Visibility.Visible;
            if (double.IsNaN(_subOpenHeight))
            {
                // 先放开高度量一次真实内容高，再收回去做动画
                NavSubList.Height = double.NaN;
                NavSubList.UpdateLayout();
                _subOpenHeight = NavSubList.ActualHeight;
                NavSubList.Height = 0;
            }
            NavSubList.BeginAnimation(HeightProperty,
                Motion.To(HeightProperty, 0, _subOpenHeight, Motion.Normal, Motion.EaseOut));
        }
        else
        {
            var to = double.IsNaN(_subOpenHeight) ? 0 : _subOpenHeight;
            var back = Motion.To(HeightProperty, to, 0, Motion.Normal, Motion.EaseOut);
            back.Completed += (_, _) => { if (NavResources.IsChecked != true) NavSubList.Visibility = Visibility.Collapsed; };
            NavSubList.BeginAnimation(HeightProperty, back);
        }
        MoveSelection();
    }

    /// <summary>测量用 TransformToVisual 相对 .nav 容器，等价 getBoundingClientRect 差值（useNavigationBubble.ts:11-27）。</summary>
    void MoveBubble(FrameworkElement? item, double targetOpacity)
    {
        if (item is not null && item.ActualWidth > 0)
        {
            var b = item.TransformToVisual(NavHost)
                        .TransformBounds(new Rect(0, 0, item.ActualWidth, item.ActualHeight));
            SlideTo(NavBubble, b, Motion.NavSlide, Motion.NavCurve);
        }
        NavBubble.BeginAnimation(OpacityProperty,
            Motion.To(OpacityProperty, NavBubble.Opacity, targetOpacity, Motion.Fast, Motion.Ease));
    }

    /// <summary>两层共用同一套位移补间：300ms cubic-bezier(.22,1,.36,1)（App.vue:1846/1857）。
    /// 首次定位时层还没有宽高，from 直接取目标值，等价于原版 measure() 的第一帧不位移。</summary>
    void SlideTo(FrameworkElement layer, Rect b, Duration d, KeySpline s)
    {
        bool sized = layer.Width > 0 && layer.Height > 0 && !double.IsNaN(Canvas.GetLeft(layer));
        Motion.Run(layer,
            (Canvas.LeftProperty, Motion.ToCurrent(Canvas.LeftProperty, b.X, d, s, sized ? Canvas.GetLeft(layer) : b.X)),
            (Canvas.TopProperty, Motion.ToCurrent(Canvas.TopProperty, b.Y, d, s, sized ? Canvas.GetTop(layer) : b.Y)),
            (WidthProperty, Motion.ToCurrent(WidthProperty, b.Width, d, s, sized ? layer.Width : b.Width)),
            (HeightProperty, Motion.ToCurrent(HeightProperty, b.Height, d, s, sized ? layer.Height : b.Height)));
    }

    /// <summary>选中层跟随当前选中项，300ms cubic-bezier(.22,1,.36,1)（App.vue:1846/1857）。</summary>
    void MoveSelection(bool instant = false) => MoveSelection(null, instant);

    /// <summary>优先用点击来源那一项：RadioButton 分组一旦失守（子项与主项不同容器），
    /// 按 IsChecked 找会同时命中两个，选条就会停在旧位置。</summary>
    void MoveSelection(RadioButton? chosen, bool instant = false)
    {
        var item = chosen ?? ActiveNavItem();
        if (item is null || item.ActualWidth <= 0) return;
        var b = item.TransformToVisual(NavHost).TransformBounds(new Rect(0, 0, item.ActualWidth, item.ActualHeight));
        SlideTo(NavSel, b, instant ? new Duration(TimeSpan.Zero) : Motion.NavSlide, Motion.NavCurve);
    }

    // ---------------- 首页 ----------------
    void RefreshHero()
    {
        var v = _s.SelectedInstalled;
        HeroTitle.Text = v?.Label ?? "还没有安装版本";
        HeroLoaderText.Text = v is null ? "原版" :
            (v.Kind == "loader" ? v.Subtitle : v.Kind switch
            {
                "snapshot" => "快照",
                "release" => "正式版",
                _ => v.Kind,
            });
        HeroSub.Text = v is null
            ? "先在「游戏版本」里安装一个版本，再回来启动。"
            : $"账号 {_s.SelectedAccount?.Name ?? "未选择"} · 内存 {_s.MemorySummary} · Java {_s.RuntimeSummary}";
        EmptyHint.Visibility = _s.InstalledList.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
        EmptyHintText.Text = "尚未安装游戏实例，点击前往版本管理";
        ApplyBrandArt();
    }

    internal async void Play_Click(object sender, RoutedEventArgs e)
    {
        if (_s.SelectedAccount is null)
        {
            NavSettings.IsChecked = true;
            Show("settings", sender as RadioButton);
            TxtAcctErr.Text = "先添加一个离线账号（1-16 位字母、数字或下划线）才能启动。";
            TxtNewName.Focus();
            return;
        }
        if (_s.SelectedInstalled is null)
        {
            NavVersions.IsChecked = true;
            Show("versions", sender as RadioButton);
            _s.Status = "请先选择一个已安装版本";
            return;
        }
        HeroPlayText.Text = "正在启动…";
        await _s.PlayAsync();
        HeroPlayText.Text = _s.LaunchText;
        RefreshHero();
    }

    internal void PlayOptions_Click(object sender, RoutedEventArgs e)
        => _s.Log("启动选项菜单（离线导出、命令世界、快速重启）下一轮接入。");
    internal void VersionSettings_Click(object sender, RoutedEventArgs e)
    {
        NavSettings.IsChecked = true;
        Show("settings", sender as RadioButton);
    }
    internal void MoreVersion_Click(object sender, RoutedEventArgs e)
        => _s.Log("实例菜单：管理实例 / 收藏 / 打开文件夹 / 删除，下一轮接入。");
    internal void PickJava_Click(object sender, RoutedEventArgs e)
    {
        NavSettings.IsChecked = true;
        Show("settings", sender as RadioButton);
        TxtJava.Focus();
    }
    void OpenDl_Click(object sender, RoutedEventArgs e) => ShowFloat(DlPanel);
    internal void GotoMemory_Click(object sender, RoutedEventArgs e)
    {
        NavSettings.IsChecked = true;
        Show("settings", sender as RadioButton);
        TxtMax.Focus();
    }
    // 原版第 3 格「运行状态」点开的是 z:9100 的模态日志窗，不是首页里的一张卡
    internal void ToggleLog_Click(object sender, RoutedEventArgs e)
    {
        // .log-mask 的 blur(5px)（HomeView.vue:968）：先快照再显示，快照里才不含遮罩自己
        _logBlur ??= MaskBlur.Attach(ShellHost, LogOverlay, ShellRoot, 5);
        LogOverlay.Visibility = Visibility.Visible;
        // 原版没有「开窗滚到底」这一条，是 watch 日志条数时判 logOpen（:255-262）；
        // 这里等价：刚显示时先滚一次，之后由 ctor 里那个 CollectionChanged 接手
        Dispatcher.BeginInvoke(() => LogScroll.ScrollToEnd(), System.Windows.Threading.DispatcherPriority.Background);
    }

    void CloseLog_Click(object sender, RoutedEventArgs e)
    {
        LogOverlay.Visibility = Visibility.Collapsed;
        MaskBlur.Detach(ShellHost, _logBlur);
        _logBlur = null;
    }

    System.Windows.Controls.Image? _logBlur;
    void History_Click(object sender, RoutedEventArgs e) => ShowFloat(NotesPanel);

    /// <summary>App.vue:1342 onImportClick：拉起文件选择，把外部 MOD/世界/皮肤导入当前实例。</summary>
    void Import_Click(object sender, RoutedEventArgs e)
        => _s.Log("导入向导下一轮接入。");

    /// <summary>.brand-avatar（1.1.10 新增，App.vue:1247）：点侧栏头像开「卡慕互动彩蛋」，
    /// 原版是 MascotStage 那一整块（11 个类＋mascotFrame/mascotAudio 等 6 个模块），
    /// 端口还没做，所以这里只留入口与提示；打开时原版还会给 .shell 挂 mascots-active
    /// 把全局 backdrop-filter 关掉（:1734）。</summary>
    void OpenMascots_Click(object sender, RoutedEventArgs e)
        => _s.Log("卡慕互动彩蛋（MascotStage）是 1.1.10 新增界面，还没移植。");
    void Notice_Click(object sender, RoutedEventArgs e) => ShowFloat(NoticePanel);

    /// <summary>
    /// 通知 / 更新日志 / 下载中心三个浮层：原版是各自 v-if，配一个 z:9000 的 .notice-mask
    /// 点空白关闭（App.vue:1396-1443）。这里同样互斥，再点一次或点遮罩收起。
    /// </summary>
    void ShowFloat(FrameworkElement panel)
    {
        bool open = panel.Visibility != Visibility.Visible;
        NoticePanel.Visibility = open && ReferenceEquals(panel, NoticePanel) ? Visibility.Visible : Visibility.Collapsed;
        NotesPanel.Visibility = open && ReferenceEquals(panel, NotesPanel) ? Visibility.Visible : Visibility.Collapsed;
        DlPanel.Visibility = open && ReferenceEquals(panel, DlPanel) ? Visibility.Visible : Visibility.Collapsed;
        FloatMask.Visibility = open ? Visibility.Visible : Visibility.Collapsed;
        if (open)
        {
            // .notice-mask 是透明的，只负责点外面关掉面板
            Panel.SetZIndex(FloatMask, 0);
            Panel.SetZIndex(panel, 1);
        }
    }

    /// <summary>收起三个顶栏浮层和它们的遮罩：开弹窗（z 10000／9600）前先退到浮层（z 9001）之下，
    /// 否则 ShowFloat 给面板加的 Panel.ZIndex 会把浮面板压在模态之上。</summary>
    void CloseFloats()
    {
        NoticePanel.Visibility = Visibility.Collapsed;
        NotesPanel.Visibility = Visibility.Collapsed;
        DlPanel.Visibility = Visibility.Collapsed;
        FloatMask.Visibility = Visibility.Collapsed;
    }

    /// <summary>
    /// CSS 的 vw/vh 减项在 WPF 没有对应单位，按窗口实际尺寸写回各弹窗的 MaxWidth/MaxHeight：
    /// .mp-modal 宽 min(620, 100vw 减 40)／最高 88vh（App.vue:2452-2455）；
    /// .modal 宽 min(460, 100vw 减 48)、被 ui-system.css:38 收成 max-width/max-height 减 32，
    /// .launchfail-modal 把宽收成 480（:2314）；
    /// .cfg-mismatch-modal 宽 min(460, 90vw)（:1722），card 基线没有最大高，所以这里也不给它设。
    /// </summary>
    void ApplyViewportCaps()
    {
        double w = ActualWidth, h = ActualHeight;
        if (w < 60 || h < 60) return;
        MpModalCard.MaxWidth = Math.Min(620, w - 40);
        MpModalCard.MaxHeight = h * 0.88;
        LaunchFailCard.MaxWidth = Math.Min(480, w - 32);
        LaunchFailCard.MaxHeight = h - 32;
        CfgMismatchCard.MaxWidth = Math.Min(460, w * 0.9);
        // .log-dialog：宽 min(760, 100vw 减 64)、最高 min(620, 100vh 减 80)（HomeView.vue:969）
        LogCard.MaxWidth = Math.Min(760, w - 64);
        LogCard.MaxHeight = Math.Min(620, h - 80);
    }

    void FloatMask_Click(object sender, MouseButtonEventArgs e) => ShowFloat(FloatMask);

    // ---------------- 顶栏浮层里的行内动作（原版都是面板内按钮，不是页面） ----------------
    /// <summary>通知头部的「清空」（App.vue:1401 clearNotices）。</summary>
    void Notice_ClearAll_Click(object sender, RoutedEventArgs e) => _s.Log("通知中心未接入：清空要等 store.notices 有数据源。");
    /// <summary>通知行里的「查看原因」（App.vue:1409）：原版打开实例中心的诊断页签，需要 n.exitTarget。</summary>
    void Notice_RowDetail_Click(object sender, RoutedEventArgs e) => _s.Log("实例中心（InstanceCenter）下一轮接入，诊断页签从这里进。");
    /// <summary>下载中心头部的「去版本下载」（App.vue:1445）：收面板 ＋ 切到 game/versions 视图。</summary>
    void Dl_GoVersions_Click(object sender, RoutedEventArgs e) { ShowFloat(DlPanel); NavVersions.IsChecked = true; Show("versions", NavVersions); }
    /// <summary>任务行四态动作（App.vue:1455-1465）：暂停／继续／取消／移除记录。</summary>
    void Dl_Pause_Click(object sender, RoutedEventArgs e) => _s.Log("下载任务队列未接入：暂停要等 store.tasks。");
    void Dl_Resume_Click(object sender, RoutedEventArgs e) => _s.Log("下载任务队列未接入：继续要等 store.tasks。");
    void Dl_Cancel_Click(object sender, RoutedEventArgs e) => _s.CancelWork();
    void Dl_Dismiss_Click(object sender, RoutedEventArgs e) => _s.Log("下载任务队列未接入：移除记录要等 store.tasks。");

    /// <summary>
    /// .dl-bar-fill 的宽度是进度百分比（App.vue:1482 内联 style），WPF 没有 % 宽度，
    /// 用两根星型列按比例分配。is-indeterminate 时原版强制 35%（:1482 的三元 ＋ :2290 的位移动画），
    /// 那条 translateX(-120%)→(320%) 是相对自身宽度的，只能接线时按 DlRowBarFill.ActualWidth 算。
    /// </summary>
    internal void SetDlRowProgress(double percent, bool indeterminate)
    {
        double p = indeterminate ? 35 : Math.Clamp(percent, 0, 100);
        DlBarFillCol.Width = new GridLength(p, GridUnitType.Star);
        DlBarRestCol.Width = new GridLength(Math.Max(0, 100 - p), GridUnitType.Star);
    }

    /// <summary>.dl-stage .dl-bar 的同款百分比条（App.vue:1491）。</summary>
    internal void SetDlStageProgress(double percent)
    {
        double p = Math.Clamp(percent, 0, 100);
        DlStageBarFillCol.Width = new GridLength(p, GridUnitType.Star);
        DlStageBarRestCol.Width = new GridLength(Math.Max(0, 100 - p), GridUnitType.Star);
    }

    /// <summary>store.noticesUnread → .bell-dot（App.vue:1371 ＋ :2121-2130）。</summary>
    internal void SetNoticesUnread(bool unread) => BellDot.Visibility = unread ? Visibility.Visible : Visibility.Collapsed;

    /// <summary>拖拽遮罩（App.vue:1549 dragActive）。原版还要 blur(24) saturate(130%)，
    /// 这层是 pointer-events:none 的纯视觉层，MaskBlur 得在显示前拍快照才含得住底下的内容。</summary>
    internal void SetDropMask(bool on) => DropMask.Visibility = on ? Visibility.Visible : Visibility.Collapsed;

    /// <summary>
    /// launcherHealth（App.vue:743-750）：最近启动异常→error，有后台任务→busy，否则 ok。
    /// .sidebar-health i 的底色与 3px 光环按 tone 换（:1982-1996）。
    /// 「最近启动异常」这条判定还没接（要看退出码历史），先按 busy/ok 两态走。
    /// </summary>
    internal void SetLauncherHealth(string tone, string text)
    {
        var (dot, ring) = tone switch
        {
            "busy" => ("Accent", "AccentSoft"),
            "error" => ("Danger", "DangerSoft"),
            _ => ("Ok", "OkSoft"),
        };
        HealthDot.Fill = (Brush)FindResource(dot);
        HealthRing.Fill = (Brush)FindResource(ring);
        HealthText.Text = text;
    }

    /// <summary>活动任务数（activeTaskCount）：驱动 .dl-badge、首页那颗图标钮（App.vue:1350-1358）
    /// 与侧栏健康的 busy 态。任务队列没接之前恒为 0，按钮和角标都保持 Collapsed。</summary>
    int _activeTasks;
    internal int ActiveTaskCount
    {
        get => _activeTasks;
        set { _activeTasks = value; ApplyTaskBadge(); }
    }

    void ApplyTaskBadge()
    {
        bool has = _activeTasks > 0;
        var vis = has ? Visibility.Visible : Visibility.Collapsed;
        DlBadge.Visibility = vis;
        DlBadgeText.Text = _activeTasks.ToString();
        DlBadgeCompact.Visibility = vis;
        DlBadgeCompactText.Text = _activeTasks.ToString();
        // 首页的图标钮只在「首页 且 有活动任务」时出现（App.vue:1351）
        BtnTopHomeDl.Visibility = has && _view == "home" ? vis : Visibility.Collapsed;
        SetLauncherHealth(has ? "busy" : "ok",
            has ? $"{_activeTasks} 项后台任务进行中" : "全部系统运行正常");
    }

    // ---------------- 启动失败弹窗（App.vue:1573-1586 ＋ :2313-2325） ----------------
    System.Windows.Controls.Image? _failBlur;
    /// <summary>拉起失败／崩溃时调用：标题＋正文（正文是 pre-wrap，换行照原样保留）。</summary>
    internal void ShowLaunchFailure(string title, string text)
    {
        CloseFloats();
        LaunchFailTitle.Text = title;
        LaunchFailText.Text = text;
        // .modal-mask 的 blur(10)（styles.css:535）：先快照再显示
        _failBlur ??= MaskBlur.Attach(ShellHost, LaunchFailMask, ShellRoot, 10);
        LaunchFailMask.Visibility = Visibility.Visible;
    }

    void CloseLaunchFail()
    {
        LaunchFailMask.Visibility = Visibility.Collapsed;
        MaskBlur.Detach(ShellHost, _failBlur);
        _failBlur = null;
    }

    /// <summary>@pointerdown.self：点遮罩本体才关，点卡片不关（ModalCard_Down 把事件吃掉）。</summary>
    void LaunchFail_Mask_Down(object sender, MouseButtonEventArgs e) => CloseLaunchFail();
    void Mp_Mask_Down(object sender, MouseButtonEventArgs e) => CloseModpackImport();
    void ModalCard_Down(object sender, MouseButtonEventArgs e) => e.Handled = true;

    void LaunchFail_Reason_Click(object sender, RoutedEventArgs e)
    {
        // 原版：openInstanceCenter({id: launchingVersionId, folder: launchingFolder}, 'diagnostics')
        _s.Log("实例中心（InstanceCenter）下一轮接入：这里要带出错实例的 id 与目录。");
        CloseLaunchFail();
    }
    void LaunchFail_Close_Click(object sender, RoutedEventArgs e) => CloseLaunchFail();
    void LaunchFail_Export_Click(object sender, RoutedEventArgs e)
    {
        // launchFail.exporting 期间按钮 disabled（App.vue:1580），文案转「导出中…」
        LaunchFailExportBtn.IsEnabled = false;
        LaunchFailExportText.Text = "导出中…";
        _s.Log("日志导出未接入：要把本次会话日志与崩溃栈打成文件。");
        LaunchFailExportText.Text = "导出错误日志";
        LaunchFailExportBtn.IsEnabled = true;
    }

    // ---------------- 配置不兼容弹窗（App.vue:1532-1541 ＋ :1720-1724） ----------------
    /// <summary>配置版本回退检测（configMismatch）未接入，先给出可见性开关。
    /// 这一层原版是全透明的挡点击层（.menu-overlay 没有底色，.cfg-mismatch-mask 也不写背景），
    /// 所以不挂 Overlay 也不接 MaskBlur，只有居中的 card 自身有底。</summary>
    internal void SetConfigMismatch(bool open)
    {
        CloseFloats();
        CfgMismatchMask.Visibility = open ? Visibility.Visible : Visibility.Collapsed;
    }
    /// <summary>「继续尝试」（App.vue:1537）：只收弹窗，不碰配置文件。</summary>
    void Cfg_Keep_Click(object sender, RoutedEventArgs e) => SetConfigMismatch(false);
    /// <summary>「重置设置」（App.vue:1538 onConfigReset）：备份原配置后写回默认值。</summary>
    void Cfg_Reset_Click(object sender, RoutedEventArgs e)
    {
        _s.Log("配置重置未接入：要先把 settings.json 备份成带时间戳的副本，再落默认值。");
        SetConfigMismatch(false);
    }

    // ---------------- 整合包导入弹窗（App.vue:1591-1712 ＋ :2452-2624） ----------------
    System.Windows.Controls.Image? _mpBlur;
    /// <summary>三态互斥：probing（.mp-loading）／info（主体）／失败（.mp-error ＋ 关闭）。</summary>
    internal enum MpState { Probing, Info, Failed }

    /// <summary>
    /// 拖入或点选整合包后的入口（原版 openModpackImport，App.vue:609）：先开遮罩进「解析中」。
    /// probeModpack 还没移植，所以 info 主体里的字段全部留空壳（各自 Collapsed），
    /// 等解析结果（format / mcVersion / loader / fileName / innerName / existingInstances / hasPresetKeys）就绪再填。
    /// </summary>
    internal void OpenModpackImport(string filePath)
    {
        CloseFloats();
        MpModalCard.MaxWidth = Math.Min(620, Math.Max(240, ActualWidth - 40));
        MpModalCard.MaxHeight = Math.Max(240, ActualHeight * 0.88);
        MpCustomName.Text = "";
        MpFatalError.Text = "";
        SetModpackState(MpState.Probing);
        _mpBlur ??= MaskBlur.Attach(ShellHost, MpMask, ShellRoot, 10);
        MpMask.Visibility = Visibility.Visible;
    }

    /// <summary>切三态可见性。Info 态里各数据块（tags／冲突框／按键同步／错误行）由填数据的代码各自打开。</summary>
    internal void SetModpackState(MpState state)
    {
        MpLoading.Visibility = state == MpState.Probing ? Visibility.Visible : Visibility.Collapsed;
        MpInfoBody.Visibility = state == MpState.Info ? Visibility.Visible : Visibility.Collapsed;
        MpErrorBody.Visibility = state == MpState.Failed ? Visibility.Visible : Visibility.Collapsed;
    }

    void CloseModpackImport()
    {
        MpMask.Visibility = Visibility.Collapsed;
        MaskBlur.Detach(ShellHost, _mpBlur);
        _mpBlur = null;
    }

    /// <summary>取消／关闭（App.vue:1701／:1710）。</summary>
    void Mp_Cancel_Click(object sender, RoutedEventArgs e) => CloseModpackImport();

    /// <summary>确认导入（App.vue:1702 confirmModpackImport）：整合包安装链路未移植，先记录缺什么。</summary>
    void Mp_Confirm_Click(object sender, RoutedEventArgs e)
    {
        _s.Log("整合包导入未接入：要 mrpack/curseforge 解析、资产下载与实例冲突处理（"
            + (MpActUpdate.IsChecked == true ? "更新" : MpActOverwrite.IsChecked == true ? "覆盖"
               : MpActNew.IsChecked == true ? "新实例" : "重新命名") + "）。");
        CloseModpackImport();
    }

    /// <summary>
    /// conflictAction 变化（App.vue:599 onMpConflictActionChange）：
    /// update／overwrite 才需要选实例＋看影响＋勾确认（mpNeedsReplaceConfirm，:574）；
    /// 重新命名／新实例则收起这三块。确认勾选在换动作时要清掉。
    /// </summary>
    void MpConflictAction_Changed(object sender, RoutedEventArgs e)
    {
        // XAML 解析期 MpActRename 的 IsChecked="True" 就会抛一次 Checked，
        // 那时下面这些元素还没建出来，必须等加载完再动它们
        if (!IsLoaded || MpExistingSelect is null) return;
        bool needConfirm = MpActUpdate.IsChecked == true || MpActOverwrite.IsChecked == true;
        MpExistingSelect.Visibility = needConfirm ? Visibility.Visible : Visibility.Collapsed;
        MpImpact.Visibility = needConfirm ? Visibility.Visible : Visibility.Collapsed;
        MpReplaceConfirm.Visibility = needConfirm ? Visibility.Visible : Visibility.Collapsed;
        if (needConfirm)
        {
            MpReplaceConfirm.IsChecked = false;
            bool update = MpActUpdate.IsChecked == true;
            // 两段影响说明照 App.vue:1676／:1679 原文；.mp-impact.danger 只改字色（:2610-2612），不画边框
            MpImpactText.Text = update
                ? "将重建包管理文件并恢复用户存档、配置及非包管理文件；同名的新包 MOD 优先。操作失败会恢复完整备份。"
                : "将重建实例包文件；存档、配置、截图、资源包及可识别的用户 MOD 会保留，其他未知顶层内容可能被移除。操作失败会恢复完整备份。";
            MpImpactText.Foreground = (Brush)FindResource(update ? "Fg2" : "Danger");
            MpReplaceConfirmText.Text = update
                ? "我已确认上述影响范围，并同意更新所选实例。"
                : "我已确认上述影响范围，并同意覆盖所选实例。";
        }
    }
    internal void AccountMenu_Click(object sender, RoutedEventArgs e)
    {
        // 原版账号卡上的 ⋯ 是切到 accounts 视图，不是跳设置
        Show("accounts");
    }
    internal void ReloadSkin_Click(object sender, RoutedEventArgs e)
    {
        // PaperDoll(reload) 会清掉 SkinRenderer 的缓存，之后 Face() 读到的就是新文件
        SkinDoll.Skin = SkinRenderer.SkinSource(reload: true);
        SkinDoll.Slim = SkinRenderer.IsSlimSkin();
        PageHome.SkinModelText.Text = SkinDoll.Slim ? "纤细模型" : "经典模型";
        ApplyBrandArt();
        _s.Log("已重新读取皮肤文件 Assets/skin.png");
    }

    /// <summary>
    /// 皮肤脸的三个落点：侧栏 logo、账号卡头像、开机碎块。原版侧栏就是 splash-face.png
    /// （App.vue:1239 .brand-head，42×42 / pixelated / radius 10 / 1px rgba(255,255,255,.26)）。
    /// </summary>
    void ApplyBrandArt()
    {
        BrandHead.Background = new ImageBrush(SkinRenderer.Face(256)) { Stretch = Stretch.Fill };
        RenderOptions.SetBitmapScalingMode(BrandHead, BitmapScalingMode.NearestNeighbor);
        AvatarImg.Source = SkinRenderer.Face(64);
    }
    internal void RowPlay_Click(object sender, RoutedEventArgs e)
    {
        if ((sender as FrameworkElement)?.Tag is VersionRow v)
        {
            _s.SelectedInstalled = v;
            Play_Click(sender, e);
        }
    }
    internal void RowMore_Click(object sender, RoutedEventArgs e)
    {
        if ((sender as FrameworkElement)?.Tag is VersionRow v)
            _s.Log($"{v.Id} 的实例菜单下一轮接入（管理 / 收藏 / 打开文件夹 / 删除）。");
    }
    /// <summary>.log-dialog footer 的「清空日志」（HomeView.vue:725 store.logs = []）。</summary>
    void ClearLog_Click(object sender, RoutedEventArgs e) => _s.ClearLogs();

    /// <summary>.log-failure 里的「导出错误日志」（HomeView.vue:264-275 exportFailureLogs）：
    /// 导出期间按钮 disabled ＋ 文案转「导出中…」，结束再切回来。真正的导出要等 launch 日志落盘接口。</summary>
    async void ExportFailureLogs_Click(object sender, RoutedEventArgs e)
    {
        var btn = (Button)sender;
        btn.IsEnabled = false;
        ExportFailureText.Text = "导出中…";
        await Task.Delay(400);
        ExportFailureText.Text = "导出错误日志";
        btn.IsEnabled = true;
        _s.Log("导出错误日志下一轮接入：要等 crash-report 与 latest.log 的落盘路径。");
    }

    // ---------------- 版本 ----------------
    internal async void Install_Click(object sender, RoutedEventArgs e)
    {
        var v = _s.SelectedOnline;
        if (v is null) { _s.Status = "先在右侧选一个版本"; return; }
        DlHint.Text = $"正在安装 {v.Id}：客户端 + 库 + 资源";
        await _s.InstallAsync(v.Id);
        DlHint.Text = "安装结束，可在左侧「本地已安装」选择启动";
        RefreshHero();
    }

    void Cancel_Click(object sender, RoutedEventArgs e) => _s.CancelWork();

    internal void DeleteVersion_Click(object sender, RoutedEventArgs e)
    {
        if ((sender as FrameworkElement)?.Tag is not VersionRow v) return;
        if (MessageBox.Show($"删除版本 {v.Id} 的目录？存档与模组文件不会被碰。", "WUHUICL",
                MessageBoxButton.YesNo, MessageBoxImage.Question) != MessageBoxResult.Yes) return;
        try
        {
            Directory.Delete(_s.Paths.VersionDir(v.Id), true);
            _s.RefreshInstalled();
            RefreshHero();
            _s.Status = $"已删除 {v.Id}";
        }
        catch (Exception ex) { _s.Status = "删除失败：" + ex.Message; }
    }

    internal void ShowSnap_Changed(object sender, RoutedEventArgs e)
    {
        if (!IsLoaded) return;
        _s.ShowSnapshots = ChkSnap.IsChecked == true;
    }

    // ---------------- 账号 ----------------
    internal void AddAccount_Click(object sender, RoutedEventArgs e)
    {
        var name = TxtNewName?.Text.Trim() ?? "";
        if (string.IsNullOrEmpty(name))
        {
            NavSettings.IsChecked = true;
            Show("settings", sender as RadioButton);
            TxtNewName.Focus();
            TxtAcctErr.Text = "输入玩家名后点「添加离线账号」。";
            return;
        }
        var err = _s.AddOfflineAccount(name);
        TxtAcctErr.Text = err ?? "";
        if (err is null)
        {
            TxtNewName.Clear();
            RefreshHero();
            _ = _s.PlayAsync();
        }
    }

    internal void RemoveAccount_Click(object sender, RoutedEventArgs e)
    {
        if ((sender as FrameworkElement)?.Tag is not AccountRow row) return;
        _s.Accounts.Remove(row.Model.Id);
        _s.AccountList.Remove(row);
        if (_s.AccountList.Count > 0) _s.SelectedAccount = _s.AccountList[0];
        RefreshHero();
    }

    // ---------------- 设置 ----------------
    void BindSettings()
    {
        var st = _s.Settings.Value;
        TxtMin.Text = st.MinHeapMb.ToString();
        TxtMax.Text = st.MaxHeapMb.ToString();
        TxtJava.Text = st.JavaPath ?? "";
        ChkAutoJava.IsChecked = st.AutoJava;
        TxtW.Text = st.Width.ToString();
        TxtH.Text = st.Height.ToString();
        ChkFull.IsChecked = st.Fullscreen;
        ChkMirror.IsChecked = st.UseMirror;
        TxtMirror.Text = st.MirrorRoot;
        TxtJvmArgs.Text = st.ExtraJvmArgs;
        TxtGameArgs.Text = st.ExtraGameArgs;
        BuildThemeChips(st.Theme);
        SetThemeNote(st.Theme);
    }

    /// <summary>
    /// .theme-option：行 gap 10 / min-height 48 / padding 6 8 / radius 6（SettingsView.vue:1863）
    /// .theme-preview：46×32 radius 5（:1865），内部 .tp-side 9px、.tp-top 6px、.tp-dot 4×4、.tp-btn 4px（:1869）
    /// .tp-check：14×14 贴右上（:1869）——原版靠 JS 换选中态，这里在重建列表时按当前主题给可见性。
    /// </summary>
    const double ChipMin = 145, ChipGap = 8;   // .theme-options：auto-fit minmax(145px,1fr) gap 8（SettingsView.vue:1862）

    void BuildThemeChips(string current)
    {
        ThemePanel.Children.Clear();
        foreach (var t in ThemeManager.Themes)
        {
            var rb = new RadioButton
            {
                Content = BuildThemeOption(t, current == t.Id),
                GroupName = "theme",
                Tag = t.Id,
                IsChecked = current == t.Id,
                FontSize = 13,
                Margin = new Thickness(0, 0, ChipGap, ChipGap),
                Style = (Style)FindResource("ThemeChip"),
            };
            rb.Checked += Theme_Changed;
            ThemePanel.Children.Add(rb);
        }
        LayoutThemeChips();
        ThemePanel.SizeChanged += (_, _) => LayoutThemeChips();
    }

    /// <summary>WPF 的 WrapPanel 没有 auto-fit：列数按可用宽度取，列宽再等分回去，
    /// 否则卡片会缩成内容宽度（原版是一排等宽铺满）。</summary>
    void LayoutThemeChips()
    {
        double w = ThemePanel.ActualWidth;
        if (w <= 0 || ThemePanel.Children.Count == 0) return;
        int cols = Math.Max(1, (int)((w + ChipGap) / (ChipMin + ChipGap)));
        double col = (w - ChipGap * (cols - 1)) / cols;
        foreach (var c in ThemePanel.Children.OfType<RadioButton>())
            c.Width = col;
    }

    static FrameworkElement BuildThemeOption((string Id, string Name, string File, bool Dark) t, bool selected)
    {
        var d = new ResourceDictionary
        {
            Source = new Uri($"pack://application:,,,/Themes/{t.File}.xaml", UriKind.Absolute),
        };
        Brush B(string key) => new SolidColorBrush((Color)d[key]);
        Brush K(string key) => (Brush)d[key];

        // .theme-preview（模板 :628-631，样式 :1551 被尾部 :1864 收成 46×32 / radius 5 / flex:none）
        // ＋ border 1.5px border ＋ overflow hidden  position relative。
        // hover 只把描边转 border-strong（:1561 的位移被 :1865 取消），
        // 选中态描边转 accent 且光环被 :1866 取消。原版靠 :hover 伪类，这里在行元素上挂事件。
        var preview = new Border
        {
            Width = 46, Height = 32, CornerRadius = new CornerRadius(5),
            Background = B("C.Bg"), BorderThickness = new Thickness(1.5),
            BorderBrush = selected ? B("C.Accent") : K("Line"),
            ClipToBounds = true,
        };

        // 迷你界面（.tp-side ＋ .tp-main）对所有主题都建：原版模板 :634-660 没有 v-if，
        // 自定义主题只是再往上盖一层 .tp-custom-grad（:1637，inset 0 的 135° 彩虹、opacity .85）
        // 加一个 20×20 的白色调色盘 .tp-palette（:1643 ＋ 尾部 :1869，带 drop-shadow），
        // 并把 .preview-custom（:1631）的底色换成写死的 #16161d。
        var grid = new Grid();
        {
            grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(9) });   // .tp-side（:1581→尾部 9 宽）
            grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) }); // .tp-main（:1594）

            // .tp-side：display flex / justify center / padding-top 4，里面才是 4×4 的 .tp-dot
            // （:1589→尾部 4×4，圆点底色是这套主题自己的 accent（:640 内联），不是 text-dim）；
            // 右缘那条 1px 分隔线也是内联写的 borderRight: 1px solid theme.border
            var side = new Border
            {
                Background = B("C.Nav"),
                BorderBrush = K("Line"), BorderThickness = new Thickness(0, 0, 1, 0),
                Padding = new Thickness(0, 4, 0, 0),
                Child = new Border
                {
                    Width = 4, Height = 4, CornerRadius = new CornerRadius(2),
                    Background = B("C.Accent"), HorizontalAlignment = HorizontalAlignment.Center,
                },
            };
            grid.Children.Add(side);

            var main = new StackPanel();
            // .tp-top：高 6（:1600→尾部 6）、底 card、border-bottom 1px border（:645 内联）
            main.Children.Add(new Border
            {
                Height = 6, Background = B("C.Card"),
                BorderBrush = K("Line"), BorderThickness = new Thickness(0, 0, 0, 1),
            });
            // .tp-body：flex 1 / column / gap 2 / padding 3（:1604→尾部 :1869）。
            // 里面是 block（flex 1）＋ btn（高 4），WPF 用两行栅格表达 star＋auto
            var body = new Grid { Margin = new Thickness(3) };
            body.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
            body.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
            // .tp-block：flex 1 / radius 4 / 底 card / border 1px border（:1611 ＋ :652 内联）
            body.Children.Add(new Border
            {
                CornerRadius = new CornerRadius(4), Background = B("C.Card"),
                BorderBrush = K("Line"), BorderThickness = new Thickness(1),
            });
            // .tp-btn：高 4 / radius 4（4 高即胶囊）/ 底 accent（:1615→尾部 4），与上面隔 gap 2
            var btn = new Border
            {
                Height = 4, CornerRadius = new CornerRadius(2), Background = B("C.Accent"),
                Margin = new Thickness(0, 2, 0, 0),
            };
            Grid.SetRow(btn, 1);
            body.Children.Add(btn);
            main.Children.Add(body);
            Grid.SetColumn(main, 1);
            grid.Children.Add(main);
        }

        // 自定义主题：盖一层彩虹渐变（opacity .85，所以底下的迷你界面还会透出 15%），
        // 再放居中的白色调色盘；.preview-custom 的底色是写死的 #16161d（:1631）
        var host = new Grid();
        host.Children.Add(grid);
        if (t.Id == "custom")
        {
            preview.Background = new SolidColorBrush(Color.FromRgb(0x16, 0x16, 0x1d));
            host.Children.Add(new Border
            {
                Opacity = 0.85,
                Background = new LinearGradientBrush
                {
                    // linear-gradient(135deg, ...) → 左上到右下
                    StartPoint = new Point(0, 0), EndPoint = new Point(1, 1),
                    GradientStops = new GradientStopCollection
                    {
                        new(Color.FromRgb(0xf4, 0x3f, 0x5e), 0.00),
                        new(Color.FromRgb(0xf9, 0x73, 0x16), 0.25),
                        new(Color.FromRgb(0xea, 0xb3, 0x08), 0.45),
                        new(Color.FromRgb(0x22, 0xc5, 0x5e), 0.65),
                        new(Color.FromRgb(0x3b, 0x82, 0xf6), 0.85),
                        new(Color.FromRgb(0xa8, 0x55, 0xf7), 1.00),
                    },
                },
            });
            host.Children.Add(new Viewbox
            {
                Width = 20, Height = 20, Stretch = Stretch.Uniform,
                // .tp-palette 的 color 是写死的 #fff，滤镜 drop-shadow(0 1px 3px rgba(0,0,0,.45))
                Child = PaletteIcon(),
                Effect = new System.Windows.Media.Effects.DropShadowEffect
                { BlurRadius = 3, ShadowDepth = 1, Direction = 270, Color = Colors.Black, Opacity = 0.45 },
            });
        }
        preview.Child = host;

        // .tp-check（模板 :670，样式 :1620→尾部 :1869）：14×14 圆（radius 50%）、padding 2、
        // 底 accent、字色 on-accent，里面是那个 viewBox 24、stroke-width 3 的圆头对勾；
        // 位置是 absolute top 1 / right 1。
        var check = new Border
        {
            Width = 14, Height = 14, CornerRadius = new CornerRadius(7),
            Background = B("C.Accent"), Padding = new Thickness(2),
            HorizontalAlignment = HorizontalAlignment.Right,
            VerticalAlignment = VerticalAlignment.Top, Margin = new Thickness(0, 1, 1, 0),
            Child = new System.Windows.Shapes.Path
            {
                // viewBox 24 → 10（14 减两侧 padding 2），所以整条路径按 10/24 缩过
                Data = Geometry.Parse("M8.33,2.5 L3.75,7.08 L1.67,5.0"),
                Stroke = B("C.AccentFg"), StrokeThickness = 1.25,
                StrokeStartLineCap = PenLineCap.Round, StrokeEndLineCap = PenLineCap.Round,
                StrokeLineJoin = PenLineJoin.Round,
            },
            Visibility = selected ? Visibility.Visible : Visibility.Collapsed,
        };
        if (preview.Child is Grid hostGrid) hostGrid.Children.Add(check);

        var row = new StackPanel { Orientation = Orientation.Horizontal };
        row.Children.Add(preview);
        row.Children.Add(new TextBlock
        {
            Text = t.Name, FontSize = 13, Margin = new Thickness(10, 0, 0, 0),
            VerticalAlignment = VerticalAlignment.Center,
        });
        // .theme-option:hover .theme-preview { border-color: border-strong }（:1561）
        var idleBrush = preview.BorderBrush;
        row.MouseEnter += (_, _) => preview.BorderBrush = K("BorderStrong");
        row.MouseLeave += (_, _) => preview.BorderBrush = idleBrush;
        return row;
    }

    /// <summary>自定义主题预览里的那个调色盘图标（SettingsView.vue:663-668）：
    /// 一条 24 viewBox 的描边轮廓 ＋ 三颗 fill=currentColor 的 r=1 圆点，颜色 #fff。</summary>
    static FrameworkElement PaletteIcon()
    {
        var canvas = new Canvas { Width = 24, Height = 24 };
        canvas.Children.Add(new System.Windows.Shapes.Path
        {
            Data = Geometry.Parse(
                "M12,22 C6.49,22 2,17.51 2,12 S6.49,2 12,2 s10,4.04 10,9 c0,3.31 -2.69,6 -6,6 h-1.77 " +
                "c-0.28,0 -0.5,0.22 -0.5,0.5 c0,0.12 0.05,0.23 0.13,0.33 c0.41,0.47 0.64,1.06 0.64,1.67 " +
                "A2.5,2.5 0 0 1 12,22 Z"),
            Stroke = Brushes.White, StrokeThickness = 1.8,
            StrokeStartLineCap = PenLineCap.Round, StrokeEndLineCap = PenLineCap.Round,
            StrokeLineJoin = PenLineJoin.Round,
        });
        foreach (var (cx, cy) in new[] { (7.5, 11.5), (12.0, 7.5), (16.5, 11.5) })
        {
            var dot = new System.Windows.Shapes.Ellipse { Width = 2, Height = 2, Fill = Brushes.White };
            Canvas.SetLeft(dot, cx - 1);
            Canvas.SetTop(dot, cy - 1);
            canvas.Children.Add(dot);
        }
        return canvas;
    }

    void SetThemeNote(string id) => TxtThemeNote.Text =
        $"当前主题：{ThemeManager.DisplayName(id)}（原版共 {ThemeManager.Themes.Length} 套）";

    void Theme_Changed(object sender, RoutedEventArgs e)
    {
        if (!IsLoaded || sender is not RadioButton { Tag: string id }) return;
        _s.Settings.Value.Theme = id;
        _s.SaveSettings();
        ThemeManager.Apply(id);
        BuildThemeChips(id);   // 勾标是静态可见性，换主题后要重建才跟得上
        ApplyResourceGroup();  // 侧栏父项的字色是 FindResource 取的本地值，换主题后要按新主题重取
        SetThemeNote(id);
    }

    void ApplyThemeFromSettings()
    {
        var id = _s.Settings.Value.Theme;
        if (ThemeManager.Themes.All(t => t.Id != id)) id = "transparent";
        ThemeManager.Apply(id);
    }

    internal void Setting_Changed(object sender, RoutedEventArgs e)
    {
        if (!IsLoaded || TxtMin is null) return;
        var st = _s.Settings.Value;
        st.MinHeapMb = Clamp(TxtMin.Text, 128, 4096, st.MinHeapMb);
        st.MaxHeapMb = Math.Max(Clamp(TxtMax.Text, 256, 32768, st.MaxHeapMb), st.MinHeapMb);
        st.JavaPath = string.IsNullOrWhiteSpace(TxtJava.Text) ? null : TxtJava.Text.Trim();
        st.AutoJava = ChkAutoJava.IsChecked == true;
        st.Width = Clamp(TxtW.Text, 640, 7680, st.Width);
        st.Height = Clamp(TxtH.Text, 480, 4320, st.Height);
        st.Fullscreen = ChkFull.IsChecked == true;
        st.UseMirror = ChkMirror.IsChecked == true;
        st.MirrorRoot = string.IsNullOrWhiteSpace(TxtMirror.Text)
            ? "https://bmclapi2.bangbang93.com" : TxtMirror.Text.Trim();
        st.ExtraJvmArgs = TxtJvmArgs.Text.Trim();
        st.ExtraGameArgs = TxtGameArgs.Text.Trim();
        _s.SaveSettings();
        _s.Installer.UseMirror = st.UseMirror;
        _s.Installer.MirrorRoot = st.MirrorRoot;
        _s.Installer.DL.PreferMirrorFirst = st.UseMirror;
    }

    static int Clamp(string text, int min, int max, int fallback) =>
        int.TryParse(text?.Trim(), out var n) ? Math.Clamp(n, min, max) : fallback;

    internal void BrowseJava_Click(object sender, RoutedEventArgs e)
    {
        var dlg = new Microsoft.Win32.OpenFileDialog
        { Title = "选择 javaw.exe", Filter = "Java 可执行文件|javaw.exe;java.exe|所有文件|*.*" };
        if (dlg.ShowDialog(this) == true)
        {
            TxtJava.Text = dlg.FileName;
            Setting_Changed(sender, e);
        }
    }

    internal void OpenDir_Click(object sender, RoutedEventArgs e)
    {
        try
        {
            Directory.CreateDirectory(_s.Paths.Root);
            Process.Start(new ProcessStartInfo("explorer.exe", _s.Paths.Root) { UseShellExecute = true });
        }
        catch (Exception ex) { _s.Status = "打开目录失败：" + ex.Message; }
    }

    internal void ChangeDir_Click(object sender, RoutedEventArgs e)
    {
        var dlg = new Microsoft.Win32.OpenFolderDialog { Title = "选择游戏根目录" };
        if (dlg.ShowDialog(this) != true) return;
        _s.Settings.Value.GameDirOverride = dlg.FolderName;
        _s.SaveSettings();
        _s.RefreshInstalled();
        RefreshHero();
        _s.Status = "游戏目录已切换：" + dlg.FolderName;
    }

    // ---------------- 窗口 ----------------
    void Min_Click(object sender, RoutedEventArgs e) => WindowState = WindowState.Minimized;
    void Max_Click(object sender, RoutedEventArgs e) =>
        WindowState = WindowState == WindowState.Maximized ? WindowState.Normal : WindowState.Maximized;
    void Close_Click(object sender, RoutedEventArgs e) => Close();

    protected override void OnKeyDown(KeyEventArgs e)
    {
        base.OnKeyDown(e);
        if (e.Key == Key.Escape && _s.Busy) _s.CancelWork();
    }

    // ---------------- 页面元素转发 ----------------
    // 页面拆成 UserControl 后，既有处理函数体不动，靠这些一行式属性取元素。
    public System.Windows.Controls.ListBox AcctList => PageHome.AcctList;
    public System.Windows.Controls.Image AvatarImg => PageHome.AvatarImg;
    public System.Windows.Controls.Grid EmptyHint => PageHome.EmptyHint;
    public System.Windows.Controls.TextBlock EmptyHintText => PageHome.EmptyHintText;
    public System.Windows.Controls.Button HeroArrow => PageHome.HeroArrow;
    public System.Windows.Controls.TextBlock HeroGameVersion => PageHome.HeroGameVersion;
    public System.Windows.Controls.Border HeroImageA => PageHome.HeroImageA;
    public System.Windows.Controls.Border HeroImageB => PageHome.HeroImageB;
    public System.Windows.Controls.Border HeroLoader => PageHome.HeroLoader;
    public System.Windows.Controls.TextBlock HeroLoaderText => PageHome.HeroLoaderText;
    public System.Windows.Controls.Button HeroMore => PageHome.HeroMore;
    public System.Windows.Controls.Button HeroPlay => PageHome.HeroPlay;
    public System.Windows.Controls.TextBlock HeroPlayText => PageHome.HeroPlayText;
    public System.Windows.Controls.Button HeroSettings => PageHome.HeroSettings;
    public System.Windows.Controls.Grid HeroStage => PageHome.HeroStage;
    public System.Windows.Controls.TextBlock HeroSub => PageHome.HeroSub;
    public System.Windows.Controls.TextBlock HeroTitle => PageHome.HeroTitle;
    public System.Windows.Controls.ListBox HomeInstalled => PageHome.HomeInstalled;
    public Controls.SkinDoll3D SkinDoll => PageHome.SkinDoll;
    public System.Windows.Controls.CheckBox ChkSnap => PageVersions.ChkSnap;
    public System.Windows.Controls.TextBlock DlHint => PageVersions.DlHint;
    public System.Windows.Controls.CheckBox ChkAutoJava => PageSettings.ChkAutoJava;
    public System.Windows.Controls.CheckBox ChkFull => PageSettings.ChkFull;
    public System.Windows.Controls.CheckBox ChkMirror => PageSettings.ChkMirror;
    public System.Windows.Controls.WrapPanel ThemePanel => PageSettings.ThemePanel;
    public System.Windows.Controls.TextBlock TxtAcctErr => PageSettings.TxtAcctErr;
    public System.Windows.Controls.TextBox TxtGameArgs => PageSettings.TxtGameArgs;
    public System.Windows.Controls.TextBox TxtH => PageSettings.TxtH;
    public System.Windows.Controls.TextBox TxtJava => PageSettings.TxtJava;
    public System.Windows.Controls.TextBox TxtJvmArgs => PageSettings.TxtJvmArgs;
    public System.Windows.Controls.TextBox TxtMax => PageSettings.TxtMax;
    public System.Windows.Controls.TextBox TxtMin => PageSettings.TxtMin;
    public System.Windows.Controls.TextBox TxtMirror => PageSettings.TxtMirror;
    public System.Windows.Controls.TextBox TxtNewName => PageSettings.TxtNewName;
    public System.Windows.Controls.TextBlock TxtThemeNote => PageSettings.TxtThemeNote;
    public System.Windows.Controls.TextBox TxtW => PageSettings.TxtW;
}
