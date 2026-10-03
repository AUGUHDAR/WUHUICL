using System.Windows.Controls;

namespace Wuhuicl.App.Pages;

public partial class VersionsPage : UserControl
{
    MainWindow Shell => (MainWindow)System.Windows.Application.Current.MainWindow!;

    public VersionsPage()
    {
        InitializeComponent();
        Loaded += (_, _) => MoveTabBlob();
        // label.installed-search 里那个 input 的 placeholder（GameView.vue:1129）：
        // WPF 的 TextBox 没有 placeholder，用覆盖文字顶位，有内容就藏。
        InstalledSearch.TextChanged += (_, _) => InstalledSearchHint.Visibility =
            InstalledSearch.Text.Length == 0 ? System.Windows.Visibility.Visible : System.Windows.Visibility.Collapsed;
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

    /// <summary>调试用（--open=install）：把安装弹窗撑开截图。
    /// 只往控件里塞内存假数据，不碰 versions 目录、不写 settings.json。</summary>
    internal void DebugOpenInstall()
    {
        ModalInstall.Visibility = System.Windows.Visibility.Visible;
        InstallTitle.Text = "安装 1.20.1";
        // 选中态是样式键切换（逻辑待办），截图里要让「Fabric」这颗真的是 active，
        // 否则下面的加载器版本／Fabric API 两块与上方磁贴自相矛盾。
        LoaderNone.Style = (System.Windows.Style)FindResource("VpLoaderOption");
        LoaderFabric.Style = (System.Windows.Style)FindResource("VpLoaderOptionOn");
        LoaderDetail.Visibility = System.Windows.Visibility.Visible;
        LoaderVersions.Items.Add("0.15.11");
        LoaderVersions.SelectedIndex = 0;
        FapiBlock.Visibility = System.Windows.Visibility.Visible;
        FapiVersions.Items.Add("0.92.2+1.20.1（2024-05-12）");
        FapiVersions.SelectedIndex = 0;
        InstallInstanceName.Text = "fabric-0.15.11-1.20.1";
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
