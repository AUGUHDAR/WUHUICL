using System.Globalization;
using System.Text.RegularExpressions;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Data;
using System.Windows.Input;
using System.Windows.Threading;
using Wuhuicl.App.Services;

namespace Wuhuicl.App.Pages;

/// <summary>
/// 账号页（复刻 KAMUCL AccountsView.vue，模板 :384-668 / 样式 :671-1094）。
/// 已接真实后端：
/// · 微软设备码登录（BtnMsLogin → MicrosoftAuth）：/devicecode 出码进弹窗（user_code 自动进剪贴板，
///   即 autoCopied 分支 AccountsView.vue:576-579）、verification_uri 一键开浏览器、
///   后台轮询直到完成/过期/取消（MsCancel = cancelMsLogin — :93）；
/// · 离线账号（BtnAddOffline → AccountStore.AddOffline；NAME_RE 校验 = :39）；
/// · 删除（行内危险按钮 → AccountStore.Remove，等价 onRemove — :368）。
/// 探测/角色列表两枚弹窗仍按原版条件收起：外置登录的 probe/list 命令本轮未接。
/// </summary>
public partial class AccountsPage : UserControl
{
    /// <summary>原版用户名正则 NAME_RE，3-16 位字母数字下划线 — AccountsView.vue:39。</summary>
    static readonly Regex NameRe = new("^[A-Za-z0-9_]{3,16}$", RegexOptions.Compiled);

    LaunchSession State => (LaunchSession)DataContext;

    System.Threading.CancellationTokenSource? _msCts;

    public AccountsPage()
    {
        InitializeComponent();
        // 原版 accountMode 的初值就是 microsoft — AccountsView.vue:33
        ApplyTab(TabMicrosoft);
        Unloaded += (_, _) => _msCts?.Cancel();
    }

    // ---------------- 账号类型分段（:717-749）----------------
    void ModeTab_Click(object sender, RoutedEventArgs e)
    {
        if (ReferenceEquals(sender, TabOffline)) ApplyTab(TabOffline);
        else if (ReferenceEquals(sender, TabYgg)) ApplyTab(TabYgg);
        else ApplyTab(TabMicrosoft);
    }

    void ApplyTab(Button chosen)
    {
        SetTab(TabMicrosoft, PanelMicrosoft, chosen);
        SetTab(TabOffline, PanelOffline, chosen);
        SetTab(TabYgg, PanelYgg, chosen);
    }

    static void SetTab(Button tab, FrameworkElement panel, Button chosen)
    {
        bool on = ReferenceEquals(tab, chosen);
        // 实心 accent 渐变选中块：:745-749，靠切 Style 完成（DynamicResource 留在样式里，切主题仍跟着走）
        tab.Style = (Style)tab.FindResource(on ? "AccTabActive" : "AccTab");
        // 三个模式面板互斥显示，对应原版的 v-if / v-else-if / v-else — :416 / :428 / :449
        panel.Visibility = on ? Visibility.Visible : Visibility.Collapsed;
    }

    // ---------------- 离线账号输入（:428-447）----------------
    void Offline_TextChanged(object sender, TextChangedEventArgs e)
    {
        string text = TxtOffline.Text;
        // WPF 无 placeholder，靠覆盖文字藏起来
        PhOffline.Visibility = text.Length == 0 ? Visibility.Visible : Visibility.Collapsed;

        bool valid = NameRe.IsMatch(text);
        bool showErr = text.Length > 0 && !valid;
        // 校验文案照抄 AccountsView.vue:42
        ErrOffline.Text = "用户名需为 3-16 位字母、数字或下划线";
        ErrOffline.Visibility = showErr ? Visibility.Visible : Visibility.Collapsed;
        // .input-error{border-color:var(--danger)} — :769-771；用资源引用以保主题切换
        TxtOffline.SetResourceReference(Control.BorderBrushProperty, showErr ? "Danger" : "Line");
        // :disabled="adding || !newName || !!nameError" — :442
        BtnAddOffline.IsEnabled = valid;
    }

    /// <summary>addOffline()（AccountsView.vue:54 的引擎命令）→ AccountStore.AddOffline + 列表刷新。</summary>
    void AddOffline_Click(object sender, RoutedEventArgs e)
    {
        var err = State.AddOfflineAccount(TxtOffline.Text.Trim());
        if (err is not null)
        {
            ErrOffline.Text = err;
            ErrOffline.Visibility = Visibility.Visible;
            return;
        }
        TxtOffline.Clear();
        ErrOffline.Visibility = Visibility.Collapsed;
    }

    // ---------------- 微软设备码登录（:420 beginMsLogin / :566-596 弹窗）----------------
    async void MsLogin_Click(object sender, RoutedEventArgs e)
    {
        if (_msCts is not null) return;   // ms.starting 为 true 时按钮不重复触发（:420 的 :disabled）
        _msCts = new System.Threading.CancellationTokenSource();
        SetMsStarting(true);
        var s = State;
        s.Status = "正在获取微软设备码…";
        try
        {
            var dc = await MicrosoftAuth.BeginDeviceCodeAsync(s.Settings, _msCts.Token);
            MsUserCodeText.Text = dc.UserCode;
            MsUri.Text = dc.VerificationUri;
            MsError.Visibility = Visibility.Collapsed;
            MsWaiting.Visibility = Visibility.Visible;
            MsWaitingText.Text = $"每 {Math.Max(1, dc.IntervalSec)} 秒自动确认一次，等待授权中…";
            ModalMs.Visibility = Visibility.Visible;
            try { Clipboard.SetText(dc.UserCode); } catch { }   // 原版 autoCopied：出码即复制
            MsCopiedHint.Visibility = Visibility.Visible;
            MsCopyHint.Visibility = Visibility.Collapsed;
            s.Status = "请在浏览器完成微软授权";

            var acc = await MicrosoftAuth.PollAndCompleteAsync(s.Settings, s.Paths, s.Accounts, dc,
                s.Log,
                step => MsWaitingText.Text = step,
                _msCts.Token);
            ModalMs.Visibility = Visibility.Collapsed;
            s.ReloadAccounts();
            s.Status = $"已登录微软账号 {acc.Name}";
            s.Log($"微软登录成功：{acc.Name}（{acc.Uuid[..8]}）");
        }
        catch (OperationCanceledException)
        {
            // 用户取消：只收层，不报错（accounts.ts onDone(null) 的取消分支）
            ModalMs.Visibility = Visibility.Collapsed;
            s.Status = "已取消微软登录";
        }
        catch (Exception ex)
        {
            // 失败：弹窗开着就把原因留在里面，已关就落到状态栏
            MsError.Text = ex.Message;
            MsError.Visibility = Visibility.Visible;
            MsWaiting.Visibility = Visibility.Collapsed;
            s.Status = "微软登录失败：" + ex.Message;
            s.Log("微软登录失败：" + ex.Message);
        }
        finally
        {
            _msCts.Dispose();
            _msCts = null;
            SetMsStarting(false);
        }
    }

    /// <summary>ms.starting（:420-424）：v-if 时 .spin 顶掉 svg，文案不变（「开始微软登录」无 loading 变体）。按钮禁用。</summary>
    void SetMsStarting(bool starting)
    {
        MsLoginSpin.Visibility = starting ? Visibility.Visible : Visibility.Collapsed;
        MsLoginIcon.Visibility = starting ? Visibility.Collapsed : Visibility.Visible;
        BtnMsLogin.IsEnabled = !starting;
    }

    /// <summary>cancelMsLogin() — :93 / :593：中止轮询并收层。</summary>
    void MsCancel_Click(object sender, RoutedEventArgs e)
    {
        _msCts?.Cancel();
        ModalMs.Visibility = Visibility.Collapsed;
    }

    /// <summary>copyUserCode()（:84）：点代码再复制一次，切换到「已复制」提示 2 秒（原版 setTimeout 2000）。</summary>
    void UserCode_Click(object sender, RoutedEventArgs e)
    {
        if (MsUserCodeText.Text.Length == 0) return;
        try { Clipboard.SetText(MsUserCodeText.Text); } catch { }
        MsCopiedHint.Visibility = Visibility.Visible;
        MsCopyHint.Visibility = Visibility.Collapsed;
        var t = new DispatcherTimer { Interval = TimeSpan.FromSeconds(2) };
        t.Tick += (_, _) => { t.Stop(); if (_msCts is not null) { MsCopiedHint.Visibility = Visibility.Collapsed; MsCopyHint.Visibility = Visibility.Visible; } };
        t.Start();
    }

    /// <summary>打开验证页面（:584 的 window.open 等价）。</summary>
    void OpenVerify_Click(object sender, RoutedEventArgs e)
    {
        if (string.IsNullOrEmpty(MsUri.Text)) return;
        try { System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo(MsUri.Text) { UseShellExecute = true }); }
        catch (Exception ex) { MsError.Text = "打开浏览器失败：" + ex.Message; MsError.Visibility = Visibility.Visible; }
    }

    // ---------------- 账号行内操作 ----------------
    /// <summary>onRemove(acc)（:368）→ AccountStore.Remove。原版直接删（无确认框），这里按桌面惯例加一道确认。</summary>
    void RowDelete_Click(object sender, RoutedEventArgs e)
    {
        if ((sender as FrameworkElement)?.DataContext is not AccountRow row) return;
        if (MessageBox.Show($"删除账号 {row.Name}？", "WUHUICL",
                MessageBoxButton.YesNo, MessageBoxImage.Question) != MessageBoxResult.Yes) return;
        State.Accounts.Remove(row.Model.Id);
        State.ReloadAccounts();
        State.Status = $"已删除账号 {row.Name}";
    }

    // ---------------- 外置登录（本轮仅弹窗骨架）----------------
    /// <summary>「添加提供商」= openProviderImport() — :459 / :157。</summary>
    void AddProvider_Click(object sender, RoutedEventArgs e) => ModalProvider.Visibility = Visibility.Visible;

    /// <summary>providerModal.open = false — :628。</summary>
    void ProviderCancel_Click(object sender, RoutedEventArgs e) => ModalProvider.Visibility = Visibility.Collapsed;

    /// <summary>蒙层上的 @pointerdown.self — :599：只有点在 .modal 之外才关。
    /// WPF 的 MouseDown 只报左键，且 e.Source 是命中的最上层元素，点在蒙层空白处时就是蒙层本身。
    /// 另外两个弹窗（:566 微软码、:644 角色选择）原版没有这条，所以不接。</summary>
    void ProviderMask_MouseDown(object sender, MouseButtonEventArgs e)
    {
        if (ReferenceEquals(e.Source, ModalProvider))
            ModalProvider.Visibility = Visibility.Collapsed;
    }

    /// <summary>profileModal.open = false — :660。</summary>
    void ProfileCancel_Click(object sender, RoutedEventArgs e) => ModalProfile.Visibility = Visibility.Collapsed;

    /// <summary>逐屏自测入口：--open=mslogin 直接走一遍真实设备码申请（打到微软端点），
    /// 弹窗里就是刚拿到的 user_code，不需要指针交互。</summary>
    internal void DebugOpenMsLogin() => MsLogin_Click(BtnMsLogin, new RoutedEventArgs());
}

/// <summary>
/// span.muted.uuid 的显示值是 acc.uuid.slice(0, 8) — AccountsView.vue:541。
/// AccountRow（Services/LaunchSession.cs:39）只透传 Model，不许在那边加派生属性，
/// 截断就落在这一层：绑的真实存在的 Model.Uuid（32 位带横线的 GUID，Json.OfflineUuid）。
/// </summary>
public sealed class UuidPrefix : IValueConverter
{
    public object Convert(object v, Type t, object p, CultureInfo c)
        => v is string s && s.Length > 0 ? (s.Length > 8 ? s[..8] : s) : "";

    public object ConvertBack(object v, Type t, object p, CultureInfo c) => Binding.DoNothing;
}
