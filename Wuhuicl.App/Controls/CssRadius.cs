using System.Windows;
using System.Windows.Controls;

namespace Wuhuicl.App.Controls;

/// <summary>
/// 复现浏览器对 border-radius 的收缩规则。
///
/// CSS 里 radius 之和超出盒子尺寸时，浏览器会把所有 radius 同比例缩小
/// （均匀 radius 时等效于 min(高,宽)/2），所以原版写 border-radius:999px 是个"胶囊惯用法"，
/// 36 高的元素实际圆角是 18。WPF 的 CornerRadius 不收缩，按字面值画弧：
/// 同一个 999 在 36 高的 Border 上会画成一片浅透镜（上下两条平缓弧线），也就是用户说的"椭圆"。
///
/// 用法：在 Border 上（或 Border 样式里）设 ctl:CssRadius.Pill=True，圆角改由实际尺寸算出。
/// 注意它会写 CornerRadius 的本地值，样式/模板里的 CornerRadius Setter 与 Trigger 都会失效。
/// </summary>
public static class CssRadius
{
    public static readonly DependencyProperty PillProperty =
        DependencyProperty.RegisterAttached(
            "Pill", typeof(bool), typeof(CssRadius), new PropertyMetadata(false, OnPill));

    public static bool GetPill(DependencyObject d) => (bool)d.GetValue(PillProperty);
    public static void SetPill(DependencyObject d, bool v) => d.SetValue(PillProperty, v);

    /// <summary>
    /// 按钮模板圆角。原版 .btn 是 radius-md(10)，.btn-sm 是 radius-sm(6)，.capsule/.loader-option
    /// 是 999（= 18）；模板里写死一个值会让这三种长得一样，所以圆角由宿主样式设进来。
    /// 默认 10 ＝ .btn 基线，未显式设置的按钮外观不变。
    /// </summary>
    public static readonly DependencyProperty RoundProperty =
        DependencyProperty.RegisterAttached(
            "Round", typeof(CornerRadius), typeof(CssRadius),
            new PropertyMetadata(new CornerRadius(10)));

    public static CornerRadius GetRound(DependencyObject d) => (CornerRadius)d.GetValue(RoundProperty);
    public static void SetRound(DependencyObject d, CornerRadius v) => d.SetValue(RoundProperty, v);

    static void OnPill(DependencyObject d, DependencyPropertyChangedEventArgs e)
    {
        if (d is not FrameworkElement el) return;
        el.SizeChanged -= OnResize;
        if (e.NewValue is not true) return;
        el.SizeChanged += OnResize;
        Clamp(el);
    }

    static void OnResize(object sender, SizeChangedEventArgs e) => Clamp((FrameworkElement)sender);

    static void Clamp(FrameworkElement el)
    {
        if (el is not Border b) return;
        double r = Math.Min(el.ActualHeight, el.ActualWidth) / 2;
        // 0 尺寸（还没排版，或动画从 0 长起来的 blob）不动，避免把圆角清零后又被裁掉
        if (r <= 0) return;
        if (Math.Abs(b.CornerRadius.TopLeft - r) > .2) b.CornerRadius = new CornerRadius(r);
    }
}
