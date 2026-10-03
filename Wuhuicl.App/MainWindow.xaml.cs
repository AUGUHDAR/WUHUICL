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
    }

    async void OnLoaded(object sender, RoutedEventArgs e)
    {
        // XAML 里 NavHome IsChecked=True 触发的 Checked 事件在解析期 PageHome 还没赋值而提前返回，
        // 顶栏按钮显隐（App.vue:1333/1359）要等这里补一次 Show 才生效
        Show("home", sender as RadioButton);
        BindSettings();
        UpdateMaxGlyph();
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
    /// 逐屏自测用：`Wuhuicl.App.exe --page=accounts` 直接打开某一页。
    /// 只认已知页名，其它一律忽略，不影响正常使用。
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
        // App.vue:1333/1342 下载与导入都只在非首页出现；:1359 更新日志图标钮只在首页出现
        bool home = which == "home";
        BtnTopDownload.Visibility = home ? Visibility.Collapsed : Visibility.Visible;
        BtnTopImport.Visibility = home ? Visibility.Collapsed : Visibility.Visible;
        BtnTopHistory.Visibility = home ? Visibility.Visible : Visibility.Collapsed;
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
        HeroPlayText.Text = _s.GameRunning ? "重新启动" : "开始游戏";
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
        LogList.ScrollIntoView(LogList.Items.Count > 0 ? LogList.Items[^1] : null!);
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

    void FloatMask_Click(object sender, MouseButtonEventArgs e) => ShowFloat(FloatMask);
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
    void CopyCmd_Click(object sender, RoutedEventArgs e)
    {
        if (string.IsNullOrEmpty(_s.LastCommand)) { _s.Status = "还没有启动记录可复制"; return; }
        try { Clipboard.SetText(_s.LastCommand); _s.Status = "启动命令已复制（token 已打码）"; }
        catch (Exception ex) { _s.Status = "复制失败：" + ex.Message; }
    }
    void ClearLog_Click(object sender, RoutedEventArgs e) => _s.LogLines.Clear();

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
                Margin = new Thickness(0, 0, 8, 8),
                Style = (Style)FindResource("ThemeChip"),
            };
            rb.Checked += Theme_Changed;
            ThemePanel.Children.Add(rb);
        }
    }

    static FrameworkElement BuildThemeOption((string Id, string Name, string File, bool Dark) t, bool selected)
    {
        var d = new ResourceDictionary
        {
            Source = new Uri($"pack://application:,,,/Themes/{t.File}.xaml", UriKind.Absolute),
        };
        Brush B(string key) => new SolidColorBrush((Color)d[key]);

        // 预览块：底色 bg，左侧栏 9px，顶栏 6px，内容区一张卡片 + 强调色小按钮 + 4px 圆点
        var preview = new Border
        {
            Width = 46, Height = 32, CornerRadius = new CornerRadius(5),
            Background = B("C.Bg"), ClipToBounds = true,
        };
        var grid = new Grid();
        grid.RowDefinitions.Add(new RowDefinition { Height = new GridLength(6) });
        grid.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(9) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });

        var side = new Border { Background = B("C.Nav") };
        Grid.SetRowSpan(side, 2);
        grid.Children.Add(side);
        var top = new Border { Background = B("C.Nav") };
        Grid.SetColumn(top, 1);
        grid.Children.Add(top);

        var body = new Grid { Margin = new Thickness(4, 3, 4, 3) };
        body.RowDefinitions.Add(new RowDefinition { Height = new GridLength(4) });
        body.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        body.Children.Add(new Border
        {
            Width = 4, Height = 4, CornerRadius = new CornerRadius(2),
            Background = B("C.Fg2"), HorizontalAlignment = HorizontalAlignment.Left,
        });
        var btn = new Border
        {
            Height = 4, CornerRadius = new CornerRadius(2), Background = B("C.Accent"),
            VerticalAlignment = VerticalAlignment.Bottom, Margin = new Thickness(0, 2, 0, 0),
        };
        Grid.SetRow(btn, 1);
        body.Children.Add(btn);
        Grid.SetColumn(body, 1);
        Grid.SetRow(body, 1);
        grid.Children.Add(body);

        // 选中勾：14×14 贴右上角 1px
        var check = new Border
        {
            Width = 14, Height = 14, CornerRadius = new CornerRadius(3),
            Background = B("C.Accent"), HorizontalAlignment = HorizontalAlignment.Right,
            VerticalAlignment = VerticalAlignment.Top, Margin = new Thickness(0, 1, 1, 0),
            Child = new System.Windows.Shapes.Path
            {
                Data = Geometry.Parse("M3,7 L6,10 L11,4"),
                Stroke = B("C.AccentFg"), StrokeThickness = 1.6,
                StrokeStartLineCap = PenLineCap.Round, StrokeEndLineCap = PenLineCap.Round,
                StrokeLineJoin = PenLineJoin.Round,
            },
            Visibility = selected ? Visibility.Visible : Visibility.Collapsed,
        };
        var host = new Grid();
        host.Children.Add(grid);
        host.Children.Add(check);
        preview.Child = host;

        var row = new StackPanel { Orientation = Orientation.Horizontal };
        row.Children.Add(preview);
        row.Children.Add(new TextBlock
        {
            Text = t.Name, FontSize = 13, Margin = new Thickness(10, 0, 0, 0),
            VerticalAlignment = VerticalAlignment.Center,
        });
        return row;
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
