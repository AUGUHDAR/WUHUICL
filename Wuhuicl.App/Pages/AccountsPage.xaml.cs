using System.Text.RegularExpressions;
using System.Windows;
using System.Windows.Controls;

namespace Wuhuicl.App.Pages;

/// <summary>
/// 账号页（复刻 KAMUCL AccountsView.vue）。事件全部留在本页：页签互斥、占位文字、用户名校验，
/// 都不碰外壳（契约 §1）。写库/登录相关按钮无事件，因为契约 §5 没暴露对应命令。
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

    // ---------------- 页头「添加账号」入口 ----------------
    void AddEntry_Click(object sender, RoutedEventArgs e)
    {
        // 设备码模态与提供商模态本轮不做（弹窗由主控统一做），入口先把添加卡滚进视野并交给离线输入框。
        AddCard.BringIntoView();
        ApplyTab(TabOffline);
        TxtOffline.Focus();
    }

    // ---------------- 离线账号输入（:428-447）----------------
    void Offline_TextChanged(object sender, TextChangedEventArgs e)
    {
        string text = TxtOffline.Text;
        // WPF 无 placeholder，靠覆盖文字模拟：有内容就藏起来
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
}
