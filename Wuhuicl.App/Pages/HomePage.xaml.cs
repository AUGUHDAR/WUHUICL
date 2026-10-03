using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;

namespace Wuhuicl.App.Pages;

public partial class HomePage : UserControl
{
    MainWindow Shell => (MainWindow)System.Windows.Application.Current.MainWindow!;

    /// <summary>参与测试及创作者名单，逐字取自 CreatorCard.vue:11 的 names 数组。</summary>
    static readonly string[] Creators =
    {
        "BAI_ZHU", "AeZz", "物晖", "牛肉", "好好", "hun_Bk", "J39", "略略略", "鸦猫", "sheri",
        "书九叶", "yansan", "(x_x;)", "小坎坷", "MuxYang", "八千代", "Henry", "月を見ていた", "Hiro", "摇滚高手"
    };

    public HomePage()
    {
        InitializeComponent();
        BuildCreatorBubbles();
        // .creator-reveal 由 pointerenter / pointerleave / focusin / focusout 开合（CreatorCard.vue:12）
        CreatorCard.MouseEnter += (_, _) => SetCreatorOpen(true);
        CreatorCard.MouseLeave += (_, _) => SetCreatorOpen(false);
        CreatorCard.LostFocus += (_, _) => SetCreatorOpen(false);
        // .runtime-strip 的关闭挂在整条的 @mouseleave（HomeView.vue:545）：
        // 指针在三格之间转移不关掉浮块，只有离开状态条才归零（脚本 :174 runtimeHover = -1）。
        RuntimeStrip.MouseLeave += (_, _) => HideRuntimeBlob();
    }

    /// <summary>.creator-bubble：12px / radius 999 / 底 card-2 / padding 5 9，层内 gap 8（CreatorCard.vue:21-22）。</summary>
    void BuildCreatorBubbles()
    {
        foreach (var name in Creators)
        {
            var bubble = new System.Windows.Controls.Border
            {
                Background = (System.Windows.Media.Brush)System.Windows.Application.Current.Resources["CardSoft"],
                Padding = new System.Windows.Thickness(9, 5, 9, 5),
                Margin = new System.Windows.Thickness(0, 0, 8, 8),
                Child = new System.Windows.Controls.TextBlock
                {
                    Text = name,
                    FontSize = 12,
                    Foreground = (System.Windows.Media.Brush)System.Windows.Application.Current.Resources["Fg"],
                },
            };
            // 999 的胶囊半径一律走 CssRadius.Pill：WPF 的 CornerRadius 不会按盒子尺寸收缩
            Controls.CssRadius.SetPill(bubble, true);
            CreatorNames.Children.Add(bubble);
        }
    }

    /// <summary>展开态：grid-template-rows 0fr→1fr ＋ opacity 1（240ms / 200ms）。
    /// 这里先用 MaxHeight 表达开合，缓动动效归在 UI_MOTION 的待办里。</summary>
    void SetCreatorOpen(bool open)
    {
        CreatorReveal.MaxHeight = open ? 240 : 0;
        CreatorReveal.Opacity = open ? 1 : 0;
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
        RuntimeBlob.Opacity = 1;
        if (RuntimeBlob.RenderTransform is ScaleTransform st) { st.ScaleX = 1; st.ScaleY = 1; }
    }

    /// <summary>基态（:866）：opacity 0 ＋ scale .97，pointer-events none 已经在 XAML 里用 IsHitTestVisible 表达。</summary>
    void HideRuntimeBlob()
    {
        RuntimeBlob.Opacity = 0;
        if (RuntimeBlob.RenderTransform is ScaleTransform st) { st.ScaleX = 0.97; st.ScaleY = 0.97; }
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
