using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Animation;

namespace Wuhuicl.App.Pages;

public partial class HomePage : UserControl
{
    MainWindow Shell => (MainWindow)System.Windows.Application.Current.MainWindow!;

    /// <summary>原版 CreatorCard.vue:4 那 20 个名字是官方 1.1.10 的测试名单；本作只有作者一个人，
    /// 按作者要求只留自己（对外一律只写 WUHUICL）。
    /// 要加人只改这个数组，气泡的错峰与漂移会自己跟上。</summary>
    static readonly string[] Creators = { "物晖" };

    /// <summary>右上角 ↗ 与整张卡都指向作者的 B 站空间（原版链的是官方的 9596327）。</summary>
    const string CreatorUrl = "https://space.bilibili.com/526277131";

    /// <summary>一个气泡的动画参数：reveal 用外层 Border 的平移＋透明度，drift 用内层文字的平移。
    /// 原版是 CSS 自定义属性 --delay / --drift-x / --drift-y / --duration（CreatorCard.vue:6 ＋ :14）。</summary>
    readonly List<(Border Bubble, TranslateTransform Reveal, TranslateTransform Drift,
        double DelayMs, Point DriftXY, double DriftSec)> _bubbles = new();
    readonly Random _drift = new();

    public HomePage()
    {
        InitializeComponent();
        BuildCreatorBubbles();
        // .creator-reveal 由 pointerenter / pointerleave / focusin / focusout 开合（CreatorCard.vue:11）
        CreatorCard.MouseEnter += (_, _) => SetCreatorOpen(true);
        CreatorCard.MouseLeave += (_, _) => SetCreatorOpen(false);
        CreatorCard.LostFocus += (_, _) => SetCreatorOpen(false);
        // .runtime-strip 的关闭挂在整条的 @mouseleave（HomeView.vue:545）：
        // 指针在三格之间转移不关掉浮块，只有离开状态条才归零（脚本 :174 runtimeHover = -1）。
        RuntimeStrip.MouseLeave += (_, _) => HideRuntimeBlob();
        // wideRecent（HomeView.vue:376-377）：窗口宽 ≥1500 取 8 条最近游戏，否则 4 条。
        // 构造时 Shell 尚未就绪，拖到 Loaded 再挂。
        Loaded += (_, _) =>
        {
            var w = Window.GetWindow(this);
            if (w is null) return;
            ApplyRecentWidth(w.ActualWidth);
            w.SizeChanged += (_, se) => ApplyRecentWidth(se.NewSize.Width);
        };
    }

    void ApplyRecentWidth(double width)
    {
        var s = Shell._s;
        int cap = width >= 1500 ? 8 : 4;
        if (s.RecentCap == cap) return;
        s.RecentCap = cap;
        s.RebuildRecent();
    }

    /// <summary>.creator-bubble：12px / radius 999 / 底 card-2 / padding 5 9，层内 gap 8（CreatorCard.vue:24）。
    /// 每个气泡按索引拿一份随机漂移量（:6 的 drift 数组：x ±6px、y ±4px、周期 3.8~6.8s）。</summary>
    void BuildCreatorBubbles()
    {
        for (int i = 0; i < Creators.Length; i++)
        {
            var reveal = new TranslateTransform(0, 4);
            var drift = new TranslateTransform();
            var bubble = new Border
            {
                Padding = new Thickness(9, 5, 9, 5),
                Margin = new Thickness(0, 0, 8, 8),
                Opacity = 0,
                RenderTransform = reveal,
                Child = new TextBlock
                {
                    Text = Creators[i],
                    FontSize = 12,
                    RenderTransform = drift,
                },
            };
            // 画刷必须走动态资源引用（等价 XAML 里的 DynamicResource）：
            // 直接 Application.Current.Resources["CardSoft"] 拿到的是查表当下那一份，
            // 实测在浅色主题下取到的是深色主题的 card-2，气泡会变成一块黑底；
            // 而且换主题时静态取的也不会跟着变。
            bubble.SetResourceReference(Border.BackgroundProperty, "CardSoft");
            ((TextBlock)bubble.Child).SetResourceReference(TextBlock.ForegroundProperty, "Fg");
            // 999 的胶囊半径一律走 CssRadius.Pill：WPF 的 CornerRadius 不会按盒子尺寸收缩
            Controls.CssRadius.SetPill(bubble, true);
            CreatorNames.Children.Add(bubble);
            _bubbles.Add((bubble, reveal, drift, i * 40.0,
                new Point(_drift.NextDouble() * 12 - 6, _drift.NextDouble() * 8 - 4),
                3.8 + _drift.NextDouble() * 3));
        }
    }

    /// <summary>展开态。原版两层同时动（CreatorCard.vue:25-30）：
    /// ① .creator-reveal：grid-template-rows 0fr→1fr 用 240ms var(ease-out)，opacity 用 200ms ease；
    ///    WPF 没有 0fr→1fr，用 MaxHeight 0↔240 走同样的时长与曲线。
    /// ② .moving .creator-bubble：reveal 220ms both ＋ 每颗延迟 i×40ms（透明度 0→1、translateY 4→0）；
    ///    内层 span：drift 随机周期 ease-in-out infinite alternate ＋ 同样的延迟。
    /// 收起时把两组动画整个摘掉（BeginAnimation(…, null)），否则 HoldEnd 会把气泡钉在半路。</summary>
    void SetCreatorOpen(bool open)
    {
        CreatorReveal.BeginAnimation(MaxHeightProperty,
            Ramp(CreatorReveal.MaxHeight, open ? 240 : 0, 240, Services.Motion.EaseOut));
        CreatorReveal.BeginAnimation(OpacityProperty,
            Ramp(CreatorReveal.Opacity, open ? 1 : 0, 200, Services.Motion.Ease));

        foreach (var (bubble, reveal, drift, delay, xy, sec) in _bubbles)
        {
            if (!open)
            {
                // 摘掉动画（不是写 0）：HoldEnd 会把气泡钉在半透明/半位移的状态
                bubble.BeginAnimation(OpacityProperty, null);
                reveal.BeginAnimation(TranslateTransform.YProperty, null);
                drift.BeginAnimation(TranslateTransform.XProperty, null);
                drift.BeginAnimation(TranslateTransform.YProperty, null);
                bubble.Opacity = 0;
                reveal.Y = 4;
                drift.X = drift.Y = 0;
                continue;
            }
            bubble.Opacity = 0;
            reveal.Y = 4;
            bubble.BeginAnimation(OpacityProperty, Ramp(0, 1, 220, Services.Motion.Ease, delay));
            reveal.BeginAnimation(TranslateTransform.YProperty, Ramp(4, 0, 220, Services.Motion.Ease, delay));
            // drift：from 是 -xy、to 是 +xy，AutoReverse ＋ Forever，周期就是随机出来的 --duration
            drift.BeginAnimation(TranslateTransform.XProperty, Swing(-xy.X, xy.X, sec, delay));
            drift.BeginAnimation(TranslateTransform.YProperty, Swing(-xy.Y, xy.Y, sec, delay));
        }
    }

    /// <summary>一段带缓动的两帧动画（可选延迟，对应 CSS 的 animation-delay）。
    /// KeySpline 只挂在 SplineDoubleKeyFrame 上（EasingDoubleKeyFrame 没有这个属性），
    /// 这点与 Services/Motion.cs:34-44 同一个写法。</summary>
    static DoubleAnimationUsingKeyFrames Ramp(double from, double to, double ms, KeySpline spline, double delayMs = 0)
    {
        var a = new DoubleAnimationUsingKeyFrames();
        a.KeyFrames.Add(new SplineDoubleKeyFrame(from, KeyTime.FromTimeSpan(TimeSpan.Zero)));
        a.KeyFrames.Add(new SplineDoubleKeyFrame(to, KeyTime.FromTimeSpan(TimeSpan.FromMilliseconds(ms)), spline));
        if (delayMs > 0) a.BeginTime = TimeSpan.FromMilliseconds(delayMs);
        return a;
    }

    /// <summary>CSS 的 `animation: drift Xs ease-in-out infinite alternate`：
    /// WPF 用 AutoReverse ＋ RepeatBehavior.Forever 表达 alternate。</summary>
    static DoubleAnimationUsingKeyFrames Swing(double from, double to, double sec, double delayMs)
    {
        var a = new DoubleAnimationUsingKeyFrames { RepeatBehavior = RepeatBehavior.Forever, AutoReverse = true };
        a.KeyFrames.Add(new SplineDoubleKeyFrame(from, KeyTime.FromTimeSpan(TimeSpan.Zero), Services.Motion.EaseInOut));
        a.KeyFrames.Add(new SplineDoubleKeyFrame(to, KeyTime.FromTimeSpan(TimeSpan.FromSeconds(sec)), Services.Motion.EaseInOut));
        if (delayMs > 0) a.BeginTime = TimeSpan.FromMilliseconds(delayMs);
        return a;
    }

    /// <summary>自测入口：--open=creator 直接展开创作者卡，
    /// 不然 PrintWindow 抓不到 hover 态（截图脚本没有指针）。</summary>
    internal void DebugOpenCreator() => SetCreatorOpen(true);

    /// <summary>整张卡与右上角 ↗ 都开作者的 B 站空间。Button 会吃掉自己的 MouseLeftButtonUp，
    /// 所以点 ↗ 不会冒泡到卡片上开两次。</summary>
    void CreatorCard_Click(object sender, MouseButtonEventArgs e) => OpenCreatorSpace();
    void CreatorLink_Click(object sender, RoutedEventArgs e) => OpenCreatorSpace();

    static void OpenCreatorSpace()
    {
        try
        {
            System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo(CreatorUrl) { UseShellExecute = true });
        }
        catch (Exception ex) { MainWindow.Diag("打开 B 站空间失败：" + ex.Message); }
    }

    // ---------------- .runtime-blob：跟随悬停格滑动（HomeView.vue:546 ＋ :862-870 ＋ 脚本 :177-190） ----------------

    /// <summary>原版 updateRuntimeBlob()：取当前悬停 .runtime-item 的 offsetLeft / offsetWidth，
    /// 写成浮块的内联 left / width。这里等价换算：格子与浮块同属状态条里的那个 Grid，
    /// 所以 TransformToVisual(父 Grid) 拿到的 X 就是 CSS 里 left 的那把尺，直接写 Margin.Left。
    /// 上下各 -4 抵掉状态条 Border 的 padding 8（CSS 的 top/bottom 0 是相对 padding box 量的），
    /// 再加 .runtime-blob 自身的 margin var(space-1) 0 ＝ 4 0 → 高度正好是内高减 8。</summary>
    void RuntimeCell_Enter(object sender, RoutedEventArgs e)
    {
        if (sender is not FrameworkElement cell) return;
        if (RuntimeBlob.Parent is not FrameworkElement stripGrid) return;
        if (cell.ActualWidth <= 0 || cell.ActualHeight <= 0) return;

        var b = cell.TransformToVisual(stripGrid)
            .TransformBounds(new Rect(0, 0, cell.ActualWidth, cell.ActualHeight));
        RuntimeBlob.Width = b.Width;
        RuntimeBlob.Margin = new Thickness(b.Left, -4, 0, -4);
        ShowRuntimeBlob();
    }

    /// <summary>.runtime-blob.on（:870）：opacity 0→1、transform scale(.97)→1。
    /// 位移与缩放在这里先做瞬时切换：left/width 的 var(motion-normal) 与 180/200ms 缓动
    /// 属于 docs/UI_MOTION.md 那一轮（共享样式 RuntimeCell 里的那份格内动画随之作废）。</summary>
    void ShowRuntimeBlob()
    {
        // opacity 180ms ease + scale .97 变 1，200ms ease（HomeView.vue:869-872）
        RuntimeBlob.BeginAnimation(OpacityProperty,
            Services.Motion.To(OpacityProperty, RuntimeBlob.Opacity, 1, Services.Motion.Ms(180), Services.Motion.Ease));
        if (RuntimeBlob.RenderTransform is ScaleTransform st)
        {
            Services.Motion.Run(st,
                (ScaleTransform.ScaleXProperty, Services.Motion.ToCurrent(ScaleTransform.ScaleXProperty, 1, Services.Motion.Ms(200), Services.Motion.Ease, st.ScaleX)),
                (ScaleTransform.ScaleYProperty, Services.Motion.ToCurrent(ScaleTransform.ScaleYProperty, 1, Services.Motion.Ms(200), Services.Motion.Ease, st.ScaleY)));
        }
    }

    /// <summary>基态（:866）：opacity 0 + scale .97，pointer-events none 已经在 XAML 里用 IsHitTestVisible 表达。</summary>
    void HideRuntimeBlob()
    {
        RuntimeBlob.BeginAnimation(OpacityProperty,
            Services.Motion.To(OpacityProperty, RuntimeBlob.Opacity, 0, Services.Motion.Ms(180), Services.Motion.Ease));
        if (RuntimeBlob.RenderTransform is ScaleTransform st)
        {
            Services.Motion.Run(st,
                (ScaleTransform.ScaleXProperty, Services.Motion.ToCurrent(ScaleTransform.ScaleXProperty, 0.97, Services.Motion.Ms(200), Services.Motion.Ease, st.ScaleX)),
                (ScaleTransform.ScaleYProperty, Services.Motion.ToCurrent(ScaleTransform.ScaleYProperty, 0.97, Services.Motion.Ms(200), Services.Motion.Ease, st.ScaleY)));
        }
    }

    // ---------------- 浮层菜单（B.11 / B.12） ----------------

    /// <summary>两个 .menu-overlay（HomeView.vue:957，模板 :662 / :692）：点空白处关掉，
    /// 等价 @click="versionMenu.open = false" 与 @click="cardMenu.id = ''"。
    /// 打开侧要量触发按钮的位置（脚本 :402-409 / :421-426），由主控接线。</summary>
    void MenuOverlay_Click(object sender, MouseButtonEventArgs e)
    {
        VersionMenuOverlay.Visibility = Visibility.Collapsed;
        VersionMenuHost.Visibility = Visibility.Collapsed;
        CardMenuOverlay.Visibility = Visibility.Collapsed;
        CardMenuHost.Visibility = Visibility.Collapsed;
    }

    /// <summary>菜单第一行（模板 :668）：folderListOpen 取反——两支互斥，文案跟着换成
    /// 「‹ 返回版本选择」/「文件夹列表 ›」。文件夹行本身没有数据源（Settings 没有 folders 字段），
    /// 由代码往 VersionMenuFolderList 里加行。</summary>
    void VersionMenuFolders_Click(object sender, RoutedEventArgs e)
    {
        bool openFolders = VersionMenuFolderBranch.Visibility != Visibility.Visible;
        VersionMenuFolderBranch.Visibility = openFolders ? Visibility.Visible : Visibility.Collapsed;
        VersionMenuVersionBranch.Visibility = openFolders ? Visibility.Collapsed : Visibility.Visible;
        VersionMenuToggleFolders.Content = openFolders ? "‹ 返回版本选择" : "文件夹列表 ›";
    }

    // 事件一律转回 Shell：逻辑只有一份，页面不碰状态。
    void CardMenuManage_Click(object sender, System.Windows.RoutedEventArgs e) => Shell.CardMenuManage_Click(sender, e);
    void CardMenuLaunch_Click(object sender, System.Windows.RoutedEventArgs e) => Shell.CardMenuLaunch_Click(sender, e);
    void CardMenuLaunchCommand_Click(object sender, System.Windows.RoutedEventArgs e) => Shell.CardMenuLaunchCommand_Click(sender, e);
    void CardMenuQuickRestart_Click(object sender, System.Windows.RoutedEventArgs e) => Shell.CardMenuQuickRestart_Click(sender, e);
    void CardMenuFavorite_Click(object sender, System.Windows.RoutedEventArgs e) => Shell.CardMenuFavorite_Click(sender, e);
    void CardMenuOpenFolder_Click(object sender, System.Windows.RoutedEventArgs e) => Shell.CardMenuOpenFolder_Click(sender, e);
    void CardMenuDelete_Click(object sender, System.Windows.RoutedEventArgs e) => Shell.CardMenuDelete_Click(sender, e);
    void VersionPick_Click(object sender, System.Windows.RoutedEventArgs e) => Shell.VersionPick_Click(sender, e);
    void AccountMenu_Click(object sender, System.Windows.RoutedEventArgs e) => Shell.AccountMenu_Click(sender, e);
    void AddAccount_Click(object sender, System.Windows.RoutedEventArgs e) => Shell.AddAccount_Click(sender, e);
    void GotoMemory_Click(object sender, System.Windows.RoutedEventArgs e) => Shell.GotoMemory_Click(sender, e);
    void HeroStage_SizeChanged(object sender, System.Windows.SizeChangedEventArgs e) => Shell.HeroStage_SizeChanged(sender, e);
    void MoreVersion_Click(object sender, System.Windows.RoutedEventArgs e) => Shell.MoreVersion_Click(sender, e);
    void NavVersionsFromButton(object sender, System.Windows.RoutedEventArgs e) => Shell.NavVersionsFromButton(sender, e);
    void PickJava_Click(object sender, System.Windows.RoutedEventArgs e) => Shell.PickJava_Click(sender, e);
    void PlayOptions_Click(object sender, System.Windows.RoutedEventArgs e) => Shell.PlayOptions_Click(sender, e);
    void Play_Click(object sender, System.Windows.RoutedEventArgs e) => Shell.Play_Click(sender, e);
    void ReloadSkin_Click(object sender, System.Windows.RoutedEventArgs e) => Shell.ReloadSkin_Click(sender, e);
    void RemoveAccount_Click(object sender, System.Windows.RoutedEventArgs e) => Shell.RemoveAccount_Click(sender, e);
    void RowMore_Click(object sender, System.Windows.RoutedEventArgs e) => Shell.RowMore_Click(sender, e);
    void RowPlay_Click(object sender, System.Windows.RoutedEventArgs e) => Shell.RowPlay_Click(sender, e);
    void ToggleLog_Click(object sender, System.Windows.RoutedEventArgs e) => Shell.ToggleLog_Click(sender, e);
    void VersionSettings_Click(object sender, System.Windows.RoutedEventArgs e) => Shell.VersionSettings_Click(sender, e);
}
