using System.Windows.Controls;

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

    /// <summary>展开态：grid-template-rows 0fr→1fr + opacity 1（240ms / 200ms）。
    /// 这里先用 MaxHeight 表达开合，缓动动效归在 UI_MOTION 的待办里。</summary>
    void SetCreatorOpen(bool open)
    {
        CreatorReveal.MaxHeight = open ? 240 : 0;
        CreatorReveal.Opacity = open ? 1 : 0;
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
