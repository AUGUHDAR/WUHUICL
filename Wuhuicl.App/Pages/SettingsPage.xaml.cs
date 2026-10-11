using System;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;

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

    // ==================== 窗口背景（HomeLayoutEditor.vue:221-359 的背景段） ====================
    // 控件态在本页（x:Name 在本页名字树里），补丁一律走 Shell.ApplyBackgroundPatch → LaunchSession.PatchBackground
    // → BackgroundChanged → MainWindow 重涂背景层。UI 态刷新读 Shell.CurrentBackground 快照。

    /// <summary>预设色板：K 的原生取色器在 WPF 没有对应控件，落成常用色快选 + hex 文本。</summary>
    static readonly (string Hex, string Name)[] Swatches =
    [
        ("#1b2437", "默认深蓝"),
        ("#0f172a", "午夜"),
        ("#17231f", "墨绿"),
        ("#3b0764", "深紫"),
        ("#7c2d12", "赭石"),
        ("#0e7490", "青"),
        ("#2563eb", "蓝"),
        ("#dc2626", "红"),
        ("#f8fafc", "近白"),
        ("#111827", "炭黑"),
    ];

    bool _bgReady;

    void SettingsPage_Loaded(object sender, RoutedEventArgs e)
    {
        // 色板只搭一次（Loaded 可能多次触发）
        if (BgColorSwatches.Children.Count == 0)
        {
            foreach (var (hex, name) in Swatches)
            {
                var chip = new Button
                {
                    Style = (Style)FindResource("StCapsule"),
                    Width = 34,
                    Height = 22,
                    MinHeight = 22,
                    Padding = new Thickness(0),
                    Margin = new Thickness(0, 0, 8, 0),
                    Background = SwatchBrush(hex),
                    BorderBrush = FindResource("Line") as Brush,
                    Tag = hex,
                    ToolTip = name,
                    Cursor = System.Windows.Input.Cursors.Hand,
                };
                chip.Click += (o, a) => { BgColorHex.Text = hex; Shell.ApplyBackgroundPatch(color: hex); };
                BgColorSwatches.Children.Add(chip);
            }
        }
        _bgReady = true;
        UpdateBackgroundUi();
    }

    static SolidColorBrush SwatchBrush(string hex)
    {
        try { return new SolidColorBrush((Color)ColorConverter.ConvertFromString(hex)); }
        catch { return new SolidColorBrush(Colors.Gray); }
    }

    void BgMode_Click(object sender, RoutedEventArgs e)
    {
        if ((sender as FrameworkElement)?.Tag is not string mode) return;
        Shell.ApplyBackgroundPatch(mode: mode);
        UpdateBackgroundUi();
    }

    void BgFit_Click(object sender, RoutedEventArgs e)
    {
        if ((sender as FrameworkElement)?.Tag is not string fit) return;
        Shell.ApplyBackgroundPatch(fit: fit);
        UpdateBackgroundUi();
    }

    void BgPick_Click(object sender, RoutedEventArgs e)
    {
        var dlg = new Microsoft.Win32.OpenFileDialog
        {
            Title = "选择背景图片",
            Filter = "图片|*.png;*.jpg;*.jpeg;*.bmp;*.webp|所有文件|*.*",
        };
        if (dlg.ShowDialog() != true) return;
        Shell.ApplyBackgroundPatch(image: dlg.FileName);
        UpdateBackgroundUi();
    }

    void BgReset_Click(object sender, RoutedEventArgs e)
    {
        Shell.ApplyBackgroundPatch(reset: true);
        UpdateBackgroundUi();
    }

    // 滑条拖动期间连续触发：补丁是轻量 JSON 写盘 + 事件，直接跟手即可
    void BgOpacity_Changed(object sender, RoutedPropertyChangedEventArgs<double> e)
    {
        if (!_bgReady) return; // XAML 初值也会触发一次
        BgOpacityVal.Text = $"{Math.Round((1 - BgOpacitySlider.Value) * 100)}%";
        Shell.ApplyBackgroundPatch(opacity: 1 - BgOpacitySlider.Value);
    }

    void BgBlur_Changed(object sender, RoutedPropertyChangedEventArgs<double> e)
    {
        if (!_bgReady) return;
        BgBlurVal.Text = $"{BgBlurSlider.Value:0}px";
        Shell.ApplyBackgroundPatch(blur: BgBlurSlider.Value);
    }

    /// <summary>按当前设置快照刷胶囊选中态、分区显隐、滑条与文案（等价 Vue 侧的响应式绑定）。</summary>
    void UpdateBackgroundUi()
    {
        var bg = Shell.CurrentBackground;
        string mode = bg.Mode is "color" or "image" ? bg.Mode : "none";

        SetCapsule(BgModeSystem, mode == "none");
        SetCapsule(BgModeColor, mode == "color");
        SetCapsule(BgModeImage, mode == "image");
        BgColorRow.Visibility = mode == "color" ? Visibility.Visible : Visibility.Collapsed;
        BgImageSection.Visibility = mode == "image" ? Visibility.Visible : Visibility.Collapsed;

        BgColorHex.Text = bg.Color;

        bool hasImage = !string.IsNullOrEmpty(bg.Image) && System.IO.File.Exists(bg.Image);
        BgPreviewImage.Visibility = hasImage ? Visibility.Visible : Visibility.Collapsed;
        BgPreviewEmpty.Text = hasImage ? "" : (string.IsNullOrEmpty(bg.Image) ? "尚未导入背景图片" : "背景图片不可用，已回退默认背景");
        BgPreviewEmpty.Visibility = hasImage ? Visibility.Collapsed : Visibility.Visible;
        if (hasImage)
        {
            try
            {
                BgPreviewImage.Source = new System.Windows.Media.Imaging.BitmapImage(new Uri(bg.Image));
                BgPreviewImage.Stretch = bg.Fit switch
                {
                    "fill" => Stretch.Fill,
                    "fit" => Stretch.Uniform,
                    _ => Stretch.UniformToFill,
                };
            }
            catch { BgPreviewImage.Source = null; }
        }
        BgPathText.Text = string.IsNullOrEmpty(bg.Image) ? "未选择" : System.IO.Path.GetFileName(bg.Image);

        SetCapsule(BgFitFill, bg.Fit == "fill");
        SetCapsule(BgFitFit, bg.Fit == "fit");
        SetCapsule(BgFitCrop, bg.Fit != "fill" && bg.Fit != "fit");

        // 改值走 -/+ 订阅，避免回写时把补丁再发一遍
        BgOpacitySlider.ValueChanged -= BgOpacity_Changed;
        BgOpacitySlider.Value = Math.Clamp(1 - bg.Opacity, 0, 1);
        BgOpacitySlider.ValueChanged += BgOpacity_Changed;
        BgOpacityVal.Text = $"{Math.Round((1 - bg.Opacity) * 100)}%";

        BgBlurSlider.ValueChanged -= BgBlur_Changed;
        BgBlurSlider.Value = Math.Clamp(bg.Blur, 0, 40);
        BgBlurSlider.ValueChanged += BgBlur_Changed;
        BgBlurVal.Text = $"{bg.Blur:0}px";
    }

    void SetCapsule(Button b, bool on) => b.Style = (Style)FindResource(on ? "StCapsuleOn" : "StCapsule");

    // ---------------- 内存分配接线（SettingsView.vue 内存区 + memoryOrganize） ----------------

    /// <summary>刷新内存信息行（memoryInfoText — :431-434）与自动分配说明里的本机数值。</summary>
    public void SetMemInfo(string infoText, string autoDesc)
    {
        MemInfoText.Text = infoText;
        AutoMemDesc.Text = autoDesc;
    }

    void MemInfoRefresh_Click(object sender, System.Windows.RoutedEventArgs e)
        => (Application.Current.MainWindow as MainWindow)?.RefreshMemInfo();

    void OrganizeMem_Click(object sender, System.Windows.RoutedEventArgs e)
    {
        if (Application.Current.MainWindow is MainWindow w) w.OrganizeSystemMemory();
    }

    void OrganizeBefore_Changed(object sender, System.Windows.RoutedEventArgs e)
    {
        if (Application.Current.MainWindow is not MainWindow w) return;
        w._s.Settings.Value.MemoryOrganizeBeforeLaunch = ChkOrganizeBefore.IsChecked == true;
        w._s.SaveSettings();
    }

    /// <summary>设置页装载时的内存区初值（MainWindow.BindSettings 调用）。</summary>
    public void BindMemory()
    {
        ChkOrganizeBefore.IsChecked = (Application.Current.MainWindow as MainWindow)?._s.Settings.Value.MemoryOrganizeBeforeLaunch == true;
    }
}
