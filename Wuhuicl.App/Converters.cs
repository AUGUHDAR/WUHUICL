using System.Globalization;
using System.Windows;
using System.Windows.Data;
using System.Windows.Media;

namespace Wuhuicl.App;

/// <summary>Switch 滑块：true=18（位移到右端），false=0。作为绑定基值，动画只在过渡期覆盖它。</summary>
public sealed class BoolShift : IValueConverter
{
    public object Convert(object v, Type t, object p, CultureInfo c) => v is true ? 18d : 0d;
    public object ConvertBack(object v, Type t, object p, CultureInfo c) => Binding.DoNothing;
}

/// <summary>Switch 选中底：true=1，false=0（同上，基值绑定）。</summary>
public sealed class BoolOne : IValueConverter
{
    public object Convert(object v, Type t, object p, CultureInfo c) => v is true ? 1d : 0d;
    public object ConvertBack(object v, Type t, object p, CultureInfo c) => Binding.DoNothing;
}

public sealed class InvertBool : IValueConverter
{
    public object Convert(object v, Type t, object p, CultureInfo c) => v is not true;
    public object ConvertBack(object v, Type t, object p, CultureInfo c) => v is not true;
}

/// <summary>true → Visible，false → Collapsed（默认），参数 "invert" 时反过来。</summary>
public sealed class BoolVis : IValueConverter
{
    public object Convert(object v, Type t, object p, CultureInfo c)
    {
        bool b = v is true;
        if (p as string == "invert") b = !b;
        return b ? Visibility.Visible : Visibility.Collapsed;
    }
    public object ConvertBack(object v, Type t, object p, CultureInfo c) => v is Visibility.Visible;
}

/// <summary>字符串非空 → Visible。用于"没有账号时才显示引导卡"。</summary>
public sealed class CountVis : IValueConverter
{
    public object Convert(object v, Type t, object p, CultureInfo c)
    {
        int n = v is int i ? i : 0;
        bool wantEmpty = p as string == "empty";
        return (wantEmpty ? n == 0 : n > 0) ? Visibility.Visible : Visibility.Collapsed;
    }
    public object ConvertBack(object v, Type t, object p, CultureInfo c) => Binding.DoNothing;
}

/// <summary>把 "Ok"/"Warn"/"Accent" 这类键名映射成当前主题里的画刷（主题切换后仍正确）。</summary>
public sealed class BrushByKey : IValueConverter
{
    public object Convert(object v, Type t, object p, CultureInfo c)
    {
        var key = v as string ?? "Accent";
        return Application.Current?.TryFindResource(key) as Brush ?? Brushes.Gray;
    }
    public object ConvertBack(object v, Type t, object p, CultureInfo c) => Binding.DoNothing;
}

/// <summary>按键名取出图标样式（"Icon.Home" → ContentControl 的 Style），取不到就留空而不是崩。</summary>
public sealed class StyleByKey : IValueConverter
{
    public object Convert(object v, Type t, object p, CultureInfo c)
    {
        if (v is not string key) return null!;
        return Application.Current?.TryFindResource(key) as Style ?? null!;
    }
    public object ConvertBack(object v, Type t, object p, CultureInfo c) => Binding.DoNothing;
}
