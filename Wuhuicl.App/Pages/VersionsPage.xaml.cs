using System.Windows.Controls;

namespace Wuhuicl.App.Pages;

public partial class VersionsPage : UserControl
{
    MainWindow Shell => (MainWindow)System.Windows.Application.Current.MainWindow!;

    public VersionsPage()
    {
        InitializeComponent();
        Loaded += (_, _) => MoveTabBlob();
    }

    /// <summary>.game-tabs-blob 的 left/width 原版由 JS 量 offsetLeft/offsetWidth 写内联（GameView.vue:120-124）。
    /// 动画等主控件的 Storyboard 库，这里先把几何量对，保证静态截图就是对的。</summary>
    void MoveTabBlob()
    {
        var tab = TabDownload.IsChecked == true ? TabDownload : TabInstalled;
        var b = tab.TransformToVisual(TabsHost).TransformBounds(
            new System.Windows.Rect(0, 0, tab.ActualWidth, tab.ActualHeight));
        System.Windows.Controls.Canvas.SetLeft(TabsBlob, b.X);
        System.Windows.Controls.Canvas.SetTop(TabsBlob, b.Y);
        TabsBlob.Width = b.Width;
        TabsBlob.Height = b.Height;
        TabsBlob.Opacity = 1;
    }

    // 事件一律转回 Shell：逻辑只有一份，页面不碰状态。
    void DeleteVersion_Click(object sender, System.Windows.RoutedEventArgs e) => Shell.DeleteVersion_Click(sender, e);
    void Install_Click(object sender, System.Windows.Input.MouseButtonEventArgs e) => Shell.Install_Click(sender, new System.Windows.RoutedEventArgs());
    void Install_Click(object sender, System.Windows.RoutedEventArgs e) => Shell.Install_Click(sender, e);
    void ShowSnap_Changed(object sender, System.Windows.RoutedEventArgs e) => Shell.ShowSnap_Changed(sender, e);
    void LaunchRow_Click(object sender, System.Windows.RoutedEventArgs e) => Shell.RowPlay_Click(sender, e);
    void RowMore_Click(object sender, System.Windows.RoutedEventArgs e) => Shell.RowMore_Click(sender, e);

    /// <summary>页签切换只切本页可见体，不动全局状态。</summary>
    void Tab_Changed(object sender, System.Windows.RoutedEventArgs e)
    {
        if (BodyDownload is null || BodyInstalled is null) return;
        bool dl = TabDownload.IsChecked == true;
        BodyDownload.Visibility = dl ? System.Windows.Visibility.Visible : System.Windows.Visibility.Collapsed;
        BodyInstalled.Visibility = dl ? System.Windows.Visibility.Collapsed : System.Windows.Visibility.Visible;
    }

    /// <summary>原版 folderToolsOpen 的开合（GameView.vue:956）：触发钮切换 .folder-manager-actions 本身，
    /// 不是弹菜单。</summary>
    void FolderMore_Click(object sender, System.Windows.RoutedEventArgs e)
        => FolderActions.Visibility = FolderActions.Visibility == System.Windows.Visibility.Visible
            ? System.Windows.Visibility.Collapsed : System.Windows.Visibility.Visible;
}
