using System.Globalization;
using System.Text.RegularExpressions;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Data;
using System.Windows.Input;

namespace Wuhuicl.App.Pages;

/// <summary>
/// 账号页（复刻 KAMUCL AccountsView.vue，模板 :384-668 / 样式 :671-1094）。
/// 事件全部留在本页：页签互斥、占位文字、用户名校验、三个弹窗的开关，
/// 都不碰外壳（契约 §1）。写库/登录/探测类按钮不挂事件，因为契约 §5 没暴露对应命令。
/// </summary>
public partial class AccountsPage : UserControl
{
    /// <summary>原版用户名正则 NAME_RE，3-16 位字母数字下划线 — AccountsView.vue:39。</summary>
    static readonly Regex NameRe = new("^[A-Za-z0-9_]{3,16}$", RegexOptions.Compiled);

    public AccountsPage()
    {
        InitializeComponent();
        // 原版 accountMode 的初值就是 microsoft — AccountsView.vue:33
        ApplyTab(TabMicrosoft);
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
        // :disabled="adding || !newName || !!nameError" — :442，adding 属未接入部分，这里只按校验置灰
        BtnAddOffline.IsEnabled = valid;
    }

    // ---------------- 弹窗开关（:565-667 的三个 Teleport 层）----------------
    // 只做到「按原版的 v-if 条件把层显出来」：设备码、探测结果、角色列表都要引擎侧命令
    // （msBeginLogin :78 / probeYggdrasilProvider :181 / listYggdrasilProviders :148），
    // 契约 §5 没暴露，所以内容留空壳，弹窗骨架与几何已经按最终 CSS 建好。

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

    /// <summary>cancelMsLogin() — :593 / :93。真正的 msCancelLogin 要引擎侧命令，先只收层。</summary>
    void MsCancel_Click(object sender, RoutedEventArgs e) => ModalMs.Visibility = Visibility.Collapsed;

    /// <summary>profileModal.open = false — :660。</summary>
    void ProfileCancel_Click(object sender, RoutedEventArgs e) => ModalProfile.Visibility = Visibility.Collapsed;
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
