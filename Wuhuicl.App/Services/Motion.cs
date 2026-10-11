using System;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using System.Windows.Media.Animation;

namespace Wuhuicl.App.Services;

/// <summary>
/// 动效令牌与缓动，全部照 docs/UI_MOTION.md §1 的最终生效值（不是被覆盖前的旧值）：
/// fast 130ms / normal 190ms / enter 220ms / carousel 500ms（ui-system.css:73 覆盖了 styles.css:62-63）。
/// </summary>
static class Motion
{
    public static readonly Duration Fast = Ms(130);
    public static readonly Duration Normal = Ms(190);
    public static readonly Duration Enter = Ms(220);
    public static readonly Duration Carousel = Ms(500);
    public static readonly Duration Press = Ms(140);
    /// <summary>.nav-bubble / .nav-selection 的位移时长（App.vue:1846）。</summary>
    public static readonly Duration NavSlide = Ms(300);

    public static Duration Ms(double v) => new(TimeSpan.FromMilliseconds(v));

    static KeySpline S(double a, double b, double c, double d) => new(new Point(a, b), new Point(c, d));

    /// <summary>CSS ease = cubic-bezier(.25,.1,.25,1)</summary>
    public static readonly KeySpline Ease = S(.25, .1, .25, 1);
    /// <summary>--ease-out = cubic-bezier(.2,.7,.3,1)</summary>
    public static readonly KeySpline EaseOut = S(.2, .7, .3, 1);
    public static readonly KeySpline EaseInOut = S(.42, 0, .58, 1);
    /// <summary>导航水滴用的强减速曲线 cubic-bezier(.22,1,.36,1)，Y 未越界可直接做 KeySpline。</summary>
    public static readonly KeySpline NavCurve = S(.22, 1, .36, 1);

    /// <summary>单段 cubic-bezier 动画。WPF 里 KeySpline 只挂在 Spline*KeyFrame 上
    /// （LinearDoubleKeyFrame / EasingDoubleKeyFrame 都没有），要精确复刻 CSS 的
    /// cubic-bezier 必须用 SplineDoubleKeyFrame。</summary>
    public static DoubleAnimationUsingKeyFrames To(DependencyProperty p, double from, double to,
                                                   Duration d, KeySpline spline, bool hold = true)
    {
        var a = new DoubleAnimationUsingKeyFrames { FillBehavior = hold ? FillBehavior.HoldEnd : FillBehavior.Stop };
        a.KeyFrames.Add(new SplineDoubleKeyFrame(from, KeyTime.FromTimeSpan(TimeSpan.Zero)));
        a.KeyFrames.Add(new SplineDoubleKeyFrame(to, KeyTime.FromTimeSpan(d.TimeSpan), spline));
        return a;
    }

    /// <summary>从当前值起算（避免从 0 跳回）。</summary>
    public static DoubleAnimationUsingKeyFrames ToCurrent(DependencyProperty p, double to,
                                                          Duration d, KeySpline spline, double current)
        => To(p, current, to, d, spline);

    /// <summary>参数用 IAnimatable 而不是 UIElement：Transform 类（TranslateTransform 等）
    /// 也常是补间目标，它们只实现 IAnimatable。</summary>
    public static void Run(IAnimatable target,
                           params (DependencyProperty p, DoubleAnimationUsingKeyFrames a)[] anims)
    {
        foreach (var (p, a) in anims) target.BeginAnimation(p, a);
    }

    /// <summary>
    /// 胶囊/指示块类浮块的位移动画（.capsule-blob / .game-tabs-blob / .seg-blob 家族）。
    /// K 侧是 JS 写内联 left/top/width/height + CSS transition；WPF 在 Canvas 层直接补间同四个属性。
    /// 默认 190ms var(--ease-out)（GameView.vue:1891）；个别页给覆盖值（Community 的 height 0.32s、Skins 的过冲曲线）。
    /// instant 或元素还没量出尺寸时直接落位（等价原版首次 measure 不位移）。
    /// </summary>
    public static void SlideBlob(FrameworkElement blob, double left, double top, double w, double h,
                                 Duration? duration = null, KeySpline? spline = null, bool instant = false)
    {
        if (instant || blob.Width <= 0 || double.IsNaN(blob.Width))
        {
            Canvas.SetLeft(blob, left);
            Canvas.SetTop(blob, top);
            blob.Width = w;
            blob.Height = h;
            return;
        }
        var d = duration ?? Normal;
        var sp = spline ?? EaseOut;
        Run(blob,
            (Canvas.LeftProperty, ToCurrent(Canvas.LeftProperty, left, d, sp, Canvas.GetLeft(blob))),
            (Canvas.TopProperty, ToCurrent(Canvas.TopProperty, top, d, sp, Canvas.GetTop(blob))),
            (FrameworkElement.WidthProperty, ToCurrent(FrameworkElement.WidthProperty, w, d, sp, blob.Width)),
            (FrameworkElement.HeightProperty, ToCurrent(FrameworkElement.HeightProperty, h, d, sp, blob.Height)));
    }
}
