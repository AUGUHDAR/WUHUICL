using System;
using System.Collections.Generic;
using System.Windows;
using System.Windows.Automation;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Animation;
using System.Windows.Threading;
using Wuhuicl.App.Services;

namespace Wuhuicl.App.Controls;

/// <summary>
/// 顶栏格言：整行居中，指针经过时逐字做有界弹簧位移。
/// 力学与缓动逐参数移植自 components/CreatorMotto.vue：
/// 影响半径 54（:33）、力 18（:33）、rotate = x/7（:36）、
/// transition transform 360ms cubic-bezier(.2,.9,.25,1.3) + color 180ms ease（:66）、
/// hover 整行升到 --text（:67，按特异度它会盖住 .accent 的颜色）。
/// 原版重音落在 'U' 'P'（:57），按需求我们的文案把重音落在"玩家"。
/// </summary>
public sealed class CreatorMotto : Border
{
    /// <summary>
    /// 原版句子是「尝试打造一个方便UP主高频使用的启动器」（CreatorMotto.vue:8），重音在 U、P 两个字符。
    /// 按需求只换受众词：UP主 → 玩家，重音落在"玩家"两字上。
    /// </summary>
    public const string Sentence = "尝试打造一个方便玩家高频使用的启动器";

    const double Radius = 54;         // :33 distance / 54
    const double Strength = 18;       // :33 * 18
    const double RotationDiv = 7;     // :36 rotate(x/7 deg)
    static readonly Duration Nudge = Motion.Ms(360);
    static readonly Duration Recolor = Motion.Ms(180);
    /// <summary>:66 的曲线 y2=1.3 会过冲；CSS 允许，WPF KeySpline 只限制 x 落在 0..1。</summary>
    static readonly KeySpline NudgeCurve = new(new Point(.2, .9), new Point(.25, 1.3));
    static readonly FontFamily TextFont =
        new("Segoe UI Variable Text, Microsoft YaHei, Segoe UI");

    sealed class Glyph
    {
        public TextBlock Tb = new();
        public TranslateTransform Shift = new();
        public RotateTransform Spin = new();
        public SolidColorBrush Brush = new();
        public bool Accent;
        public Point Center;
    }

    readonly List<Glyph> _glyphs = new();
    readonly Border _line = new();
    Window? _window;
    Point? _pointer;
    bool _queued, _hot, _disabled;

    public CreatorMotto()
    {
        // .creator-motto：100%×100% 的 flex 容器，把格言行居中（:63）
        HorizontalAlignment = HorizontalAlignment.Stretch;
        VerticalAlignment = VerticalAlignment.Stretch;
        Child = _line;

        // .motto-line：inline-flex + padding 15px 3px / 12px / 500 / .055em / --text-dim（:64）
        _line.HorizontalAlignment = HorizontalAlignment.Center;
        _line.VerticalAlignment = VerticalAlignment.Center;
        _line.Padding = new Thickness(3, 15, 3, 15);
        // 透明底才拿得到 padding 区域的命中；CSS 的 inline-flex 盒子同样含 padding
        _line.Background = Brushes.Transparent;
        var row = new StackPanel { Orientation = Orientation.Horizontal };
        _line.Child = row;

        foreach (var c in Sentence)
        {
            var g = new Glyph
            {
                Accent = c is '玩' or '家',
                Brush = new SolidColorBrush(ColorFrom("Fg2")),
            };
            g.Tb.Text = c.ToString();
            g.Tb.FontFamily = TextFont;
            g.Tb.FontSize = FontSize;
            g.Tb.FontWeight = g.Accent ? FontWeight.FromOpenTypeWeight(650) : FontWeights.Medium;
            g.Tb.Foreground = g.Brush;
            // letter-spacing .055em：WPF 没有字距属性，逐字补右间距
            g.Tb.Margin = new Thickness(0, 0, Math.Round(g.Tb.FontSize * .055, 2), 0);
            var tg = new TransformGroup();
            tg.Children.Add(g.Shift);
            tg.Children.Add(g.Spin);
            g.Tb.RenderTransformOrigin = new Point(.5, .5);
            g.Tb.RenderTransform = tg;
            row.Children.Add(g.Tb);
            _glyphs.Add(g);
        }

        _line.MouseEnter += (_, _) => { _hot = true; MeasureCenters(); RecolorAll(); };
        _line.MouseLeave += (_, _) => { _hot = false; Reset(); RecolorAll(); };
        _line.MouseMove += Line_MouseMove;
        // .motto-line 是 no-drag 且 dblclick 最大化（:56,:64）：吃掉事件，别冒泡到顶栏拖拽
        _line.MouseLeftButtonDown += (_, e) =>
        {
            e.Handled = true;
            if (e.ClickCount == 2) Maximize();
        };
        // .sr-only 把整句读给屏幕阅读器，逐字 span 只是装饰（:55,:69）
        AutomationProperties.SetName(this, Sentence);

        Loaded += (_, _) => { _window = Window.GetWindow(this); MeasureCenters(); };
        ThemeManager.Applied += RecolorAll;
        Unloaded += (_, _) => ThemeManager.Applied -= RecolorAll;
    }

    /// <summary>对应 props.disabled（编辑模式）：只停装饰动效，不改版式。</summary>
    public bool Disabled
    {
        get => _disabled;
        set
        {
            if (_disabled == value) return;
            _disabled = value;
            if (value) Reset();
        }
    }

    /// <summary>
    /// clamp(9px, 2.8cqw, 12px)（:64）。cqw 是格言容器宽度的 1%，
    /// 容器要到 429px 以下才会低于 12px，而窗口最小宽 960，所以全部受支持宽度都取上限 12。
    /// </summary>
    const double FontSize = 12;

    static Color ColorFrom(string key) =>
        (Application.Current.TryFindResource(key) as SolidColorBrush)?.Color ?? Colors.Gray;

    /// <summary>:17 取 slot 盒子中心做基准，只在 pointerenter 量一次，避免位移反馈进新基准。</summary>
    void MeasureCenters()
    {
        var win = _window ??= Window.GetWindow(this);
        if (win is null) return;
        foreach (var g in _glyphs)
        {
            if (!g.Tb.IsLoaded || g.Tb.ActualWidth <= 0) continue;
            var bounds = g.Tb.TransformToVisual(win)
                .TransformBounds(new Rect(0, 0, g.Tb.ActualWidth, g.Tb.ActualHeight));
            g.Center = new Point(bounds.X + bounds.Width / 2, bounds.Y + bounds.Height / 2);
        }
    }

    /// <summary>hover 整行变 --text；离开后重音字回 --accent-2、其余回 --text-dim（:67-68）。</summary>
    void RecolorAll()
    {
        var plain = ColorFrom("Fg2");
        var bright = ColorFrom("Fg");
        var accent = ColorFrom("AccentHi");
        foreach (var g in _glyphs)
        {
            var to = _hot ? bright : g.Accent ? accent : plain;
            if (ReducedMotion())
            {
                g.Brush.BeginAnimation(SolidColorBrush.ColorProperty, null);
                g.Brush.Color = to;
            }
            else AnimateColor(g, to);
        }
    }

    static void AnimateColor(Glyph g, Color to)
    {
        var a = new ColorAnimationUsingKeyFrames { FillBehavior = FillBehavior.HoldEnd };
        a.KeyFrames.Add(new SplineColorKeyFrame(g.Brush.Color, KeyTime.FromTimeSpan(TimeSpan.Zero)));
        a.KeyFrames.Add(new SplineColorKeyFrame(to, KeyTime.FromTimeSpan(Recolor.TimeSpan), Motion.Ease));
        g.Brush.BeginAnimation(SolidColorBrush.ColorProperty, a);
    }

    /// <summary>move 只记坐标，一帧内合并处理（:27 requestAnimationFrame）。</summary>
    void Line_MouseMove(object sender, MouseEventArgs e)
    {
        var win = _window ??= Window.GetWindow(this);
        if (win is null || Disabled || ReducedMotion() || win.WindowState == WindowState.Minimized) return;
        _pointer = e.GetPosition(win);
        if (_queued) return;
        _queued = true;
        Dispatcher.BeginInvoke(DispatcherPriority.Render, new Action(() =>
        {
            _queued = false;
            if (_pointer is Point p) Push(p);
        }));
    }

    /// <summary>:30-37 力场：沿"远离指针"方向推开 force 像素，x 分量兼作倾角。</summary>
    void Push(Point pointer)
    {
        foreach (var g in _glyphs)
        {
            var dx = g.Center.X - pointer.X;
            var dy = g.Center.Y - pointer.Y;
            var distance = Math.Sqrt(dx * dx + dy * dy);
            var k = Math.Max(0, 1 - distance / Radius);
            var force = k * k * Strength;
            double x = 0, y = 0;
            if (distance > .1)
            {
                x = dx / distance * force;
                y = dy / distance * force;
            }
            else y = -force;
            // :36 force 为 0 时写回 transform:''，也就是一条回到零位的补间
            Displace(g, force > 0 ? x : 0, force > 0 ? y : 0, force > 0 ? x / RotationDiv : 0);
        }
    }

    void Displace(Glyph g, double x, double y, double rot)
    {
        Motion.Run(g.Shift,
            (TranslateTransform.XProperty, Motion.ToCurrent(TranslateTransform.XProperty, x, Nudge, NudgeCurve, g.Shift.X)),
            (TranslateTransform.YProperty, Motion.ToCurrent(TranslateTransform.YProperty, y, Nudge, NudgeCurve, g.Shift.Y)));
        Motion.Run(g.Spin,
            (RotateTransform.AngleProperty, Motion.ToCurrent(RotateTransform.AngleProperty, rot, Nudge, NudgeCurve, g.Spin.Angle)));
    }

    /// <summary>:19-22 reset：清指针、位移回零。</summary>
    void Reset()
    {
        _pointer = null;
        foreach (var g in _glyphs) Displace(g, 0, 0, 0);
    }

    /// <summary>:24 prefers-reduced-motion 下 transform:none !important，位移与补间都关掉。</summary>
    static bool ReducedMotion() => SystemParameters.ClientAreaAnimation == false;

    void Maximize()
    {
        var win = _window ?? Window.GetWindow(this);
        if (win is null || Disabled) return;
        win.WindowState = win.WindowState == WindowState.Maximized
            ? WindowState.Normal : WindowState.Maximized;
    }
}
