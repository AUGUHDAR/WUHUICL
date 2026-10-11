using System.Globalization;
using System.Windows.Data;

namespace Wuhuicl.App;

/// <summary>fav-btn 的 ToolTip：收藏态换文案（GameView.vue:1147）。</summary>
public sealed class FavTipConverter : IValueConverter
{
    public object Convert(object v, Type t, object p2, CultureInfo c)
        => v is bool b && b ? "取消收藏" : "收藏";

    public object ConvertBack(object v, Type t, object p2, CultureInfo c)
        => Binding.DoNothing;
}
