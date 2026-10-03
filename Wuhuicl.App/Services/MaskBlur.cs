using System;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using System.Windows.Media.Effects;
using System.Windows.Media.Imaging;

namespace Wuhuicl.App.Services;

/// <summary>
/// 遮罩背后的"背景模糊"。WPF 没有 backdrop-filter，而原版这一层是 load-bearing：
/// `.modal-mask` blur(10px) + mask 底色（styles.css:527-538）、`.log-mask` blur(5px)
/// （HomeView.vue:968），亮色主题 mask 只有 42% alpha，不糊就直接看清底下的正文。
///
/// 做法：打开弹窗那一刻把壳层快照成一张降采样位图，铺在 tint 底下并挂 BlurEffect，
/// 关闭即释放。一次性成本换常驻零开销；不做实时采样（原版也不需要，弹窗期间底下是静态的）。
/// </summary>
public static class MaskBlur
{
    /// <summary>快照降采样倍率。模糊结果对分辨率不敏感，砍一半可省 3/4 内存与耗时。</summary>
    const double SnapshotScale = 0.5;

    /// <summary>
    /// 在 host 里、overlay 之前插入一张壳层模糊图。
    /// cssRadius 是原版 blur() 的像素半径；WPF BlurEffect.Radius 是高斯近似，
    /// 按 UI_LAYERS.md G.1 的换算取 N×2（Radius 作用在元素布局空间，与快照倍率无关）。
    /// </summary>
    public static Image? Attach(Grid host, UIElement overlay, FrameworkElement source, double cssRadius)
    {
        var w = source.ActualWidth;
        var h = source.ActualHeight;
        if (w < 1 || h < 1) return null;
        var index = host.Children.IndexOf(overlay);
        if (index < 0) return null;

        RenderTargetBitmap rtb;
        try
        {
            rtb = new RenderTargetBitmap(
                Math.Max(1, (int)(w * SnapshotScale)), Math.Max(1, (int)(h * SnapshotScale)),
                96, 96, PixelFormats.Pbgra32);
            // DrawingContext 没有 DrawVisual；用 VisualBrush 把壳层按倍率画进小画布
            var dv = new DrawingVisual();
            using (var dc = dv.RenderOpen())
            {
                var vb = new VisualBrush(source) { Stretch = Stretch.Fill };
                dc.DrawRectangle(vb, null,
                    new Rect(0, 0, w * SnapshotScale, h * SnapshotScale));
            }
            rtb.Render(dv);
        }
        catch
        {
            // 快照失败（尺寸超限/设备丢失）就退回原来的纯 tint，不能因此开不出弹窗
            return null;
        }

        var img = new Image
        {
            Source = rtb,
            Stretch = Stretch.Fill,
            IsHitTestVisible = false,
            Effect = new BlurEffect { Radius = Math.Max(1, cssRadius * 2) },
        };
        Grid.SetColumnSpan(img, host.ColumnDefinitions.Count);
        host.Children.Insert(index, img);
        return img;
    }

    /// <summary>关闭时摘掉并断开位图引用，让快照立刻回收。</summary>
    public static void Detach(Grid host, Image? img)
    {
        if (img is null) return;
        host.Children.Remove(img);
        img.Source = null;
        img.Effect = null;
    }
}
