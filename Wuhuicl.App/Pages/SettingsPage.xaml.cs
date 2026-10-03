using System.Windows.Controls;

namespace Wuhuicl.App.Pages;

public partial class SettingsPage : UserControl
{
    MainWindow Shell => (MainWindow)System.Windows.Application.Current.MainWindow!;

    public SettingsPage() => InitializeComponent();

    // 事件一律转回 Shell：逻辑只有一份，页面不碰状态。
    void AddAccount_Click(object sender, System.Windows.RoutedEventArgs e) => Shell.AddAccount_Click(sender, e);
    void BrowseJava_Click(object sender, System.Windows.RoutedEventArgs e) => Shell.BrowseJava_Click(sender, e);
    void ChangeDir_Click(object sender, System.Windows.RoutedEventArgs e) => Shell.ChangeDir_Click(sender, e);
    void OpenDir_Click(object sender, System.Windows.RoutedEventArgs e) => Shell.OpenDir_Click(sender, e);
    void Setting_Changed(object sender, System.Windows.RoutedEventArgs e) => Shell.Setting_Changed(sender, e);
}
