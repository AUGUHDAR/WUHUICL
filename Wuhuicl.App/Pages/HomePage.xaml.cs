using System.Windows.Controls;

namespace Wuhuicl.App.Pages;

public partial class HomePage : UserControl
{
    MainWindow Shell => (MainWindow)System.Windows.Application.Current.MainWindow!;

    public HomePage() => InitializeComponent();

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
