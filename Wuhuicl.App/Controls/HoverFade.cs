using System;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Animation;

namespace Wuhuicl.App.Controls;

/// <summary>
/// hover 背景淡入的通用附着属性。原版（KAMUCL 1.1.10）几乎所有行/卡/按钮的 hover 底色都带
/// 0.12s～0.2s ease 的过渡（ui-system.css:45 行组 130ms、.btn 0.18s、.fm-row 0.15s、.version-row 0.15s），
/// 而 WPF 的 Trigger Setter 是瞬时换值；逐个样式写 Storyboard 不现实，统一收到这里：
/// <code>
/// ctl:HoverFade.Enabled="True"          // hover 淡入 BrushKey 画刷的颜色（默认 Hover，130ms ease）
/// ctl:HoverFade.BrushKey="CardSoft"     // 换淡入的画刷键
/// ctl:HoverFade.DurationMs="180"        // 换时长
/// ctl:HoverFade.MaxOpacity="0.5"        // 目标画刷太浓时减淡
/// </code>
/// 实现：进入时取「元素当前生效底色 → hover 色」做 ColorAnimation（不从透明起步，免得实色底的行
/// 划过时闪一下）；离开时动画回原底色，再还原背景引用。不直接动画主题画刷——它们是全站单例
/// （DynamicResource），动一处等于染全站。挂了本属性的元素要把同属性的 IsMouseOver 换色
/// Trigger 删掉（本地值优先级高于 Trigger，留着不生效还误导）。
/// </summary>
public static class HoverFade
{
    public static readonly DependencyProperty EnabledProperty = DependencyProperty.RegisterAttached(
        "Enabled", typeof(bool), typeof(HoverFade), new PropertyMetadata(false, OnEnabledChanged));

    public static readonly DependencyProperty BrushKeyProperty = DependencyProperty.RegisterAttached(
        "BrushKey", typeof(string), typeof(HoverFade), new PropertyMetadata("Hover"));

    /// <summary>淡入时长（毫秒），默认 130 ＝ ui-system.css:45 行组的 var(motion-fast)。</summary>
    public static readonly DependencyProperty DurationMsProperty = DependencyProperty.RegisterAttached(
        "DurationMs", typeof(double), typeof(HoverFade), new PropertyMetadata(130.0));

    /// <summary>峰值不透明度：目标画刷键比原版 hover 色更浓时按比例减淡（0-1）。</summary>
    public static readonly DependencyProperty MaxOpacityProperty = DependencyProperty.RegisterAttached(
        "MaxOpacity", typeof(double), typeof(HoverFade), new PropertyMetadata(1.0));

    /// <summary>淡入中的画刷。Leave 时找它继续动画到原底色。</summary>
    static readonly DependencyProperty LiveBrushProperty = DependencyProperty.RegisterAttached(
        "LiveBrush", typeof(SolidColorBrush), typeof(HoverFade), new PropertyMetadata(null));

    /// <summary>进入前的底色（Leave 的动画终点；无底色时是透明）。</summary>
    static readonly DependencyProperty BaseColorProperty = DependencyProperty.RegisterAttached(
        "BaseColor", typeof(Color?), typeof(HoverFade), new PropertyMetadata(null));

    /// <summary>进入前就存在的本地 Background 值（如 FmRow 的 AccentPick 选中演示行），
    /// Leave 动画结束后原样还回，否则会被 ClearValue 顶掉。</summary>
    static readonly DependencyProperty SavedLocalProperty = DependencyProperty.RegisterAttached(
        "SavedLocal", typeof(object), typeof(HoverFade), new PropertyMetadata(null));

    public static void SetEnabled(DependencyObject o, bool v) => o.SetValue(EnabledProperty, v);
    public static bool GetEnabled(DependencyObject o) => (bool)o.GetValue(EnabledProperty);
    public static void SetBrushKey(DependencyObject o, string v) => o.SetValue(BrushKeyProperty, v);
    public static string GetBrushKey(DependencyObject o) => (string)o.GetValue(BrushKeyProperty);
    public static void SetDurationMs(DependencyObject o, double v) => o.SetValue(DurationMsProperty, v);
    public static double GetDurationMs(DependencyObject o) => (double)o.GetValue(DurationMsProperty);
    public static void SetMaxOpacity(DependencyObject o, double v) => o.SetValue(MaxOpacityProperty, v);
    public static double GetMaxOpacity(DependencyObject o) => (double)o.GetValue(MaxOpacityProperty);

    static void OnEnabledChanged(DependencyObject d, DependencyPropertyChangedEventArgs e)
    {
        if (d is not UIElement el) return;
        if ((bool)e.NewValue)
        {
            el.MouseEnter += OnEnter;
            el.MouseLeave += OnLeave;
            el.IsEnabledChanged += OnIsEnabledChanged;
        }
        else
        {
            el.MouseEnter -= OnEnter;
            el.MouseLeave -= OnLeave;
            el.IsEnabledChanged -= OnIsEnabledChanged;
        }
    }

    static void OnIsEnabledChanged(object sender, DependencyPropertyChangedEventArgs e)
    {
        if (e.NewValue is false) Fade((UIElement)sender, false);
    }

    static void OnEnter(object sender, MouseEventArgs e) => Fade((UIElement)sender, true);
    static void OnLeave(object sender, MouseEventArgs e) => Fade((UIElement)sender, false);

    static Brush? ReadBackground(UIElement el) => el switch
    {
        Border b => b.Background,
        Control c => c.Background,
        _ => null,
    };

    static void WriteBackground(UIElement el, Brush? value)
    {
        switch (el)
        {
            case Border b: b.Background = value; break;
            case Control c: c.Background = value; break;
        }
    }

    static void Fade(UIElement el, bool on)
    {
        if (!el.IsVisible) return;
        var brush = (SolidColorBrush?)el.GetValue(LiveBrushProperty);

        if (!on)
        {
            if (brush is null) return;
            var ms = Math.Max(1, GetDurationMs(el));
            var baseC = (Color?)el.GetValue(BaseColorProperty)
                        ?? Color.FromArgb(0, brush.Color.R, brush.Color.G, brush.Color.B);
            var back = new ColorAnimation(baseC, TimeSpan.FromMilliseconds(ms))
            {
                EasingFunction = new QuadraticEase { EasingMode = EasingMode.EaseOut },
            };
            back.Completed += (_, _) =>
            {
                brush.BeginAnimation(SolidColorBrush.ColorProperty, null);
                if (ReferenceEquals(ReadBackground(el), brush))
                {
                    if (el.GetValue(SavedLocalProperty) is Brush restored) WriteBackground(el, restored);
                    else WriteBackground(el, null);  // ClearValue → 回样式/主题取值
                }
                el.SetValue(LiveBrushProperty, null);
                el.SetValue(BaseColorProperty, null);
                el.SetValue(SavedLocalProperty, null);
            };
            brush.BeginAnimation(SolidColorBrush.ColorProperty, back);
            return;
        }

        if (brush is not null)
        {
            // 上一次离场动画还没走完又进来了：直接改道去 hover 色
            var key2 = GetBrushKey(el);
            var src2 = ((el as FrameworkElement)?.TryFindResource(key2) ?? Application.Current.TryFindResource(key2)) as SolidColorBrush;
            if (src2 is null) return;
            AnimateTo(brush, TargetColor(src2.Color, GetMaxOpacity(el)), GetDurationMs(el));
            return;
        }

        var key = GetBrushKey(el);
        var src = ((el as FrameworkElement)?.TryFindResource(key) ?? Application.Current.TryFindResource(key)) as SolidColorBrush;
        if (src is null) return;
        var target = TargetColor(src.Color, GetMaxOpacity(el));

        // 进入前的底色既是动画起点也是离场终点；本地持久底（选中演示行等）要记住离开时还回去
        var current = ReadBackground(el);
        var baseColor = (current as SolidColorBrush)?.Color;
        var prop = el is Border ? Border.BackgroundProperty : Control.BackgroundProperty;
        var local = el.ReadLocalValue(prop);
        if (local is Brush && !ReferenceEquals(local, DependencyProperty.UnsetValue))
            el.SetValue(SavedLocalProperty, local);

        brush = new SolidColorBrush(baseColor ?? Color.FromArgb(0, target.R, target.G, target.B));
        el.SetValue(LiveBrushProperty, brush);
        el.SetValue(BaseColorProperty, baseColor);
        WriteBackground(el, brush);
        AnimateTo(brush, target, GetDurationMs(el));
    }

    /// <summary>目标色＝源画刷的颜色乘 MaxOpacity。柔和画刷（Hover #0D1B2437、AccentSoft #24…）
    /// 的「淡」全编在 alpha 里，必须按比例保留源 alpha，不能想当然写满 255。</summary>
    static Color TargetColor(Color src, double maxOpacity)
        => Color.FromArgb(
            (byte)Math.Clamp(Math.Round(src.A * Math.Clamp(maxOpacity, 0, 1)), 0, 255),
            src.R, src.G, src.B);

    static void AnimateTo(SolidColorBrush brush, Color to, double ms)
    {
        var anim = new ColorAnimation(to, TimeSpan.FromMilliseconds(Math.Max(1, ms)))
        {
            EasingFunction = new QuadraticEase { EasingMode = EasingMode.EaseOut },
        };
        brush.BeginAnimation(SolidColorBrush.ColorProperty, anim);
    }
}
