using System.Diagnostics;
using System.IO;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using Wuhuicl.App.Services;
using Wuhuicl.Core.Stores;

namespace Wuhuicl.App;

public partial class MainWindow : Window
{
    readonly LaunchSession _s = new();

    public MainWindow()
    {
        InitializeComponent();
        DataContext = _s;
        Loaded += OnLoaded;
        StateChanged += (_, _) => UpdateMaxGlyph();
    }

    async void OnLoaded(object sender, RoutedEventArgs e)
    {
        BindSettings();
        UpdateMaxGlyph();
        ApplyThemeFromSettings();
        HeroImage.Source = SkinRenderer.LoadAsset("hero.png");
        AvatarImg.Source = SkinRenderer.Face(64);
        SkinDoll.Source = SkinRenderer.PaperDoll(230);
        RefreshHero();
        await _s.LoadOnline();
        RefreshHero();
    }

    void UpdateMaxGlyph() => BtnMax.Content = WindowState == WindowState.Maximized ? "\uE923" : "\uE922";

    // ---------------- 导航 ----------------
    void Nav_Checked(object sender, RoutedEventArgs e)
    {
        if (PageHome is null) return;
        if (ReferenceEquals(sender, NavHome)) Show("home");
        else if (ReferenceEquals(sender, NavVersions)) Show("versions");
        else if (ReferenceEquals(sender, NavSettings)) Show("settings");
        else ShowStub(sender);
    }

    void Show(string which)
    {
        PageHome.Visibility = which == "home" ? Visibility.Visible : Visibility.Collapsed;
        PageVersions.Visibility = which == "versions" ? Visibility.Visible : Visibility.Collapsed;
        PageSettings.Visibility = which == "settings" ? Visibility.Visible : Visibility.Collapsed;
        PageStub.Visibility = which == "stub" ? Visibility.Visible : Visibility.Collapsed;
        if (which == "home") RefreshHero();
    }

    void ShowStub(object sender)
    {
        Show("stub");
        var (title, text, glyph) = sender switch
        {
            _ when ReferenceEquals(sender, NavResources) =>
                ("资源管理", "模组 / 资源包 / 光影包 / 录像的文件管理，下一轮接入。", "\uE8B7"),
            _ when ReferenceEquals(sender, NavOnline) =>
                ("联机", "局域网直连、UPnP 端口映射与内网穿透，下一轮接入。", "\uE8CE"),
            _ when ReferenceEquals(sender, NavDefaults) =>
                ("默认配置", "options.txt 与按键方案的默认写入，下一轮接入。", "\uE90F"),
            _ when ReferenceEquals(sender, NavSkin) =>
                ("皮肤", "皮肤上传与披风切换，下一轮接入。当前右侧已用你的皮肤文件渲染纸娃娃。", "\uE7B0"),
            _ when ReferenceEquals(sender, NavCommunity) =>
                ("社区资源", "整合包与 MOD 检索下载，下一轮接入。", "\uE774"),
            _ => ("该模块尚未接入", "本轮只交付基础链路。", "\uE7BA"),
        };
        StubTitle.Text = title;
        StubText.Text = text;
        StubIcon.Text = glyph;
    }

    void NavVersionsFromButton(object sender, RoutedEventArgs e)
    {
        NavVersions.IsChecked = true;
        Show("versions");
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
        EmptyHintText.Text = "还没有安装任何游戏版本";
        AvatarImg.Source = SkinRenderer.Face(64);
    }

    async void Play_Click(object sender, RoutedEventArgs e)
    {
        if (_s.SelectedAccount is null)
        {
            NavSettings.IsChecked = true;
            Show("settings");
            TxtAcctErr.Text = "先添加一个离线账号（1-16 位字母、数字或下划线）才能启动。";
            TxtNewName.Focus();
            return;
        }
        if (_s.SelectedInstalled is null)
        {
            NavVersions.IsChecked = true;
            Show("versions");
            _s.Status = "请先选择一个已安装版本";
            return;
        }
        HeroPlay.Content = "▶   正在启动…";
        await _s.PlayAsync();
        HeroPlay.Content = _s.GameRunning ? "▶   重新启动" : "▶   开 始 游 戏";
        RefreshHero();
    }

    void PlayOptions_Click(object sender, RoutedEventArgs e)
        => _s.Log("启动选项菜单（离线导出、命令世界、快速重启）下一轮接入。");
    void VersionSettings_Click(object sender, RoutedEventArgs e)
    {
        NavSettings.IsChecked = true;
        Show("settings");
    }
    void MoreVersion_Click(object sender, RoutedEventArgs e)
        => _s.Log("实例菜单：管理实例 / 收藏 / 打开文件夹 / 删除，下一轮接入。");
    void GotoMemory_Click(object sender, RoutedEventArgs e)
    {
        NavSettings.IsChecked = true;
        Show("settings");
        TxtMax.Focus();
    }
    void ToggleLog_Click(object sender, RoutedEventArgs e)
    {
        Show("home");
        NavHome.IsChecked = true;
        LogList.ScrollIntoView(LogList.Items.Count > 0 ? LogList.Items[^1] : null!);
    }
    void History_Click(object sender, RoutedEventArgs e)
        => _s.Log("历史与录像列表下一轮接入。");
    void Notice_Click(object sender, RoutedEventArgs e)
        => _s.Log("通知中心下一轮接入。");
    void AccountMenu_Click(object sender, RoutedEventArgs e)
    {
        NavSettings.IsChecked = true;
        Show("settings");
    }
    void ReloadSkin_Click(object sender, RoutedEventArgs e)
    {
        SkinDoll.Source = SkinRenderer.PaperDoll(230, reload: true);
        AvatarImg.Source = SkinRenderer.Face(64);
        _s.Log("已重新读取皮肤文件 Assets/skin.png");
    }
    void RowPlay_Click(object sender, RoutedEventArgs e)
    {
        if ((sender as FrameworkElement)?.Tag is VersionRow v)
        {
            _s.SelectedInstalled = v;
            Play_Click(sender, e);
        }
    }
    void RowMore_Click(object sender, RoutedEventArgs e)
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
    async void Install_Click(object sender, RoutedEventArgs e)
    {
        var v = _s.SelectedOnline;
        if (v is null) { _s.Status = "先在右侧选一个版本"; return; }
        DlHint.Text = $"正在安装 {v.Id}：客户端 + 库 + 资源";
        await _s.InstallAsync(v.Id);
        DlHint.Text = "安装结束，可在左侧「本地已安装」选择启动";
        RefreshHero();
    }

    void Cancel_Click(object sender, RoutedEventArgs e) => _s.CancelWork();

    void DeleteVersion_Click(object sender, RoutedEventArgs e)
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

    void ShowSnap_Changed(object sender, RoutedEventArgs e)
    {
        if (!IsLoaded) return;
        _s.ShowSnapshots = ChkSnap.IsChecked == true;
    }

    // ---------------- 账号 ----------------
    void AddAccount_Click(object sender, RoutedEventArgs e)
    {
        var name = TxtNewName?.Text.Trim() ?? "";
        if (string.IsNullOrEmpty(name))
        {
            NavSettings.IsChecked = true;
            Show("settings");
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

    void RemoveAccount_Click(object sender, RoutedEventArgs e)
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
        foreach (var t in ThemeManager.Themes)
        {
            var rb = new RadioButton
            {
                Content = t.Name,
                GroupName = "theme",
                Tag = t.Id,
                IsChecked = st.Theme == t.Id,
                FontSize = 13,
                Margin = new Thickness(0, 0, 10, 0),
                Style = (Style)FindResource("ThemeChip"),
            };
            rb.Checked += Theme_Changed;
            ThemePanel.Children.Add(rb);
        }
        TxtThemeNote.Text = $"当前主题：{ThemeManager.DisplayName(st.Theme)}（原版共 {ThemeManager.Themes.Length} 套）";
    }

    void Theme_Changed(object sender, RoutedEventArgs e)
    {
        if (!IsLoaded || sender is not RadioButton { Tag: string id }) return;
        _s.Settings.Value.Theme = id;
        _s.SaveSettings();
        ThemeManager.Apply(id);
        TxtThemeNote.Text = $"当前主题：{ThemeManager.DisplayName(id)}（原版共 {ThemeManager.Themes.Length} 套）";
    }

    void ApplyThemeFromSettings()
    {
        var id = _s.Settings.Value.Theme;
        if (ThemeManager.Themes.All(t => t.Id != id)) id = "transparent";
        ThemeManager.Apply(id);
    }

    void Setting_Changed(object sender, RoutedEventArgs e)
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

    void BrowseJava_Click(object sender, RoutedEventArgs e)
    {
        var dlg = new Microsoft.Win32.OpenFileDialog
        { Title = "选择 javaw.exe", Filter = "Java 可执行文件|javaw.exe;java.exe|所有文件|*.*" };
        if (dlg.ShowDialog(this) == true)
        {
            TxtJava.Text = dlg.FileName;
            Setting_Changed(sender, e);
        }
    }

    void OpenDir_Click(object sender, RoutedEventArgs e)
    {
        try
        {
            Directory.CreateDirectory(_s.Paths.Root);
            Process.Start(new ProcessStartInfo("explorer.exe", _s.Paths.Root) { UseShellExecute = true });
        }
        catch (Exception ex) { _s.Status = "打开目录失败：" + ex.Message; }
    }

    void ChangeDir_Click(object sender, RoutedEventArgs e)
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
}
