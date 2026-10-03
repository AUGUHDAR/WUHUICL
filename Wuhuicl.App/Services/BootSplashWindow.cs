using System;
using System.Collections.Generic;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Shapes;
using System.Windows.Threading;

namespace Wuhuicl.App.Services;

/// <summary>
/// 开机启动动画窗口：把皮肤头像切成 72 块玻璃碎片，先各自漂浮并躲指针，
/// 再汇聚拼成完整的脸，停留后淡出交给主窗。
/// 逐参数移植自 KAMUCL-1.1.5：src/main/startupSplash.ts（窗口属性）、
/// src/renderer/src/splash.ts（渲染循环）、src/renderer/src/splash.css（胶囊与退场）、
/// src/shared/startup.ts（几何与物理，见 BootGlass）。
/// </summary>
public sealed class BootSplashWindow : Window
{
    /// <summary>原版标签文案（splash.ts:16），按我们的启动阶段对应改写文案本身不改节奏。</summary>
    static readonly string[] Labels =
    {
        "读取配置", "加载账户", "扫描游戏实例", "加载首页图片、Java 与皮肤", "准备首帧",
    };

    readonly Canvas _stage = new();
    Border? _caption;
    TextBlock _stageText = new();
    readonly List<(GlassShard shard, Canvas root, Path image, Path tint, Path edge, Path bevel)> _items = new();
    GlassGeometry _geo = new();
    Image? _finalFace;
    BitmapSource? _face;
    Stopwatch2 _clock = new();
    double _lastFrame, _convergeAt = double.NaN, _nextFrame;
    bool _assembled, _closing;
    GlassPoint? _pointer;
    DispatcherTimer? _pointerTimer, _stageTimer;
    int _stageIndex;
    Action? _onDone;

    public BootSplashWindow()
    {
        // startupSplash.ts:24-27：frame:false / transparent / hasShadow:false /
        // resizable:movable:focusable=false / skipTaskbar / alwaysOnTop / 点击穿透
        WindowStyle = WindowStyle.None;
        AllowsTransparency = true;
        Background = Brushes.Transparent;
        ResizeMode = ResizeMode.NoResize;
        ShowInTaskbar = false;
        Focusable = false;
        IsHitTestVisible = false;
        Topmost = true;
        Title = "WUHUICL · 正在启动";
        Left = SystemParameters.PrimaryScreenWidth;
        Top = SystemParameters.PrimaryScreenHeight;
        Width = SystemParameters.PrimaryScreenWidth;
        Height = SystemParameters.PrimaryScreenHeight;
        Left = Top = 0;
        WindowStartupLocation = WindowStartupLocation.Manual;

        _stage.ClipToBounds = true;
        Content = _stage;
        BuildCaption();
    }

    void BuildCaption()
    {
        // splash.css:17-19：left 50% / top calc(50% + 135px) / 220×86 / border #ffffff24 /
        // bg #172021b8 / radius 14 / 文字 12px，strong 20px/700
        var card = new Border
        {
            Width = 220,
            Height = 86,
            CornerRadius = new CornerRadius(14),
            BorderBrush = new SolidColorBrush(Color.FromArgb(0x24, 0xff, 0xff, 0xff)),
            BorderThickness = new Thickness(1),
            Background = new SolidColorBrush(Color.FromArgb(0xb8, 0x17, 0x20, 0x21)),
            Child = new StackPanel
            {
                Orientation = Orientation.Vertical,
                HorizontalAlignment = HorizontalAlignment.Center,
                VerticalAlignment = VerticalAlignment.Center,
            },
        };
        ((StackPanel)card.Child).Children.Add(new TextBlock
        {
            Text = "WUHUICL",
            FontSize = 20,
            FontWeight = FontWeights.Bold,
            Foreground = new SolidColorBrush(Color.FromRgb(0xf5, 0xf8, 0xf6)),
            HorizontalAlignment = HorizontalAlignment.Center,
        });
        ((StackPanel)card.Child).Children.Add(new TextBlock
        {
            Margin = new Thickness(0, 8, 0, 0),
            FontSize = 12,
            Foreground = new SolidColorBrush(Color.FromRgb(0xf5, 0xf8, 0xf6)),
            HorizontalAlignment = HorizontalAlignment.Center,
            Text = Labels[0] + "…",
        });
        _stageText = (TextBlock)((StackPanel)card.Child).Children[1];
        _caption = card;
        _stage.Children.Add(card);
        Canvas.SetLeft(card, SystemParameters.PrimaryScreenWidth / 2 - 110);
        Canvas.SetTop(card, SystemParameters.PrimaryScreenHeight / 2 + 135);
    }

    /// <summary>头像用玩家自己的皮肤文件现算（用户要求：二创头像一律基于这张皮肤）。</summary>
    public void Start(Action onDone)
    {
        _onDone = onDone;
        try { _face = SkinRenderer.Face(256); } catch { _face = null; }
        if (_face is null)
        {
            // 对应 splash.ts:67 face.onerror：头像资源不可用就不该卡在启动页，直接放行主窗
            onDone();
            return;
        }
        Build();
        _clock.Start();
        CompositionTarget.Rendering += OnRendering;
        // startupSplash.ts:33-37：指针每 32ms 采一次屏幕坐标（窗口本身点击穿透）
        _pointerTimer = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(32) };
        _pointerTimer.Tick += (_, _) =>
        {
            // 窗口点击穿透，Mouse.GetPosition 不可靠；原版轮询 screen.getCursorPoint()
            if (Native.GetCursorPos(out var pt)) _pointer = new GlassPoint(pt.X, pt.Y);
        };
        _pointerTimer.Start();
        // 阶段文案：真实启动进度接好前，按原版节奏推进文案，不假装在读盘
        _stageTimer = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(420) };
        _stageTimer.Tick += (_, _) =>
        {
            if (_stageIndex >= Labels.Length - 1) { _stageText.Text = "准备就绪"; _stageTimer!.Stop(); return; }
            _stageIndex++;
            _stageText.Text = Labels[_stageIndex] + "…";
        };
        _stageTimer.Start();
        Show();
        Activate();
    }

    void Build()
    {
        _geo = BootGlass.Make(SystemParameters.PrimaryScreenWidth, SystemParameters.PrimaryScreenHeight);
        _items.Clear();
        foreach (var s in _geo.Shards)
        {
            var fig = new PathGeometry();
            var f = new PathFigure { StartPoint = new Point(s.Vertices[0].X, s.Vertices[0].Y), IsClosed = true };
            for (var i = 1; i < s.Vertices.Count; i++) f.Segments.Add(new LineSegment(new Point(s.Vertices[i].X, s.Vertices[i].Y), true));
            fig.Figures.Add(f);

            // 整张脸按 board 尺寸铺在 (-sourceX,-sourceY)：三角形本来就落在这一块里，
            // 所以 TileMode=None 足够，不需要平铺。
            // ViewportUnits 必须是 Absolute：默认是 RelativeToBoundingBox，那时 board 会被
            // 当成"bbox 的倍数"，每块碎片只采样到一个像素，看起来就是一块纯灰玻璃。
            var brush = new ImageBrush(_face!)
            {
                Stretch = Stretch.Fill,
                TileMode = TileMode.None,
                ViewportUnits = BrushMappingMode.Absolute,
                Viewport = new Rect(-s.SourceX, -s.SourceY, _geo.Board, _geo.Board),
            };
            var img = new Path { Data = fig, Fill = brush };
            // splash.ts:19 ctx.imageSmoothingEnabled=false，像素头像不能糊
            RenderOptions.SetBitmapScalingMode(img, BitmapScalingMode.NearestNeighbor);
            // 玻璃高光渐变（splash.ts:50-52）：渐变端点是碎片本地坐标，不是包围盒比例
            var tint = new Path
            {
                Data = fig,
                Stroke = new SolidColorBrush(Color.FromArgb(0x45, 0x25, 0x33, 0x49)),
                StrokeThickness = 2.4,
                Fill = new LinearGradientBrush
                {
                    MappingMode = BrushMappingMode.Absolute,
                    StartPoint = new Point(-28, -30),
                    EndPoint = new Point(32, 35),
                    GradientStops = new GradientStopCollection
                    {
                        new(Color.FromArgb(0x80, 0xef, 0xfa, 0xff), 0),
                        new(Color.FromArgb(0x20, 0xba, 0xdf, 0xf2), .40),
                        new(Color.FromArgb(0x58, 0xff, 0xff, 0xff), .52),
                        new(Color.FromArgb(0x28, 0xbc, 0xb0, 0xfa), 1),
                    },
                },
            };
            var edge = new Path
            {
                Data = fig,
                Stroke = new SolidColorBrush(Color.FromArgb(0xb8, 0xea, 0xf7, 0xff)),
                StrokeThickness = .85,
            };
            // 一条亮边随碎片转动反光（splash.ts:62-65）
            var bevel = new Path
            {
                Stroke = new SolidColorBrush(Color.FromArgb(0xff, 0xff, 0xff, 0xff)),
                StrokeThickness = 1.4,
                StrokeStartLineCap = PenLineCap.Round,
                StrokeEndLineCap = PenLineCap.Round,
                Data = new LineGeometry(new Point(s.Vertices[0].X, s.Vertices[0].Y),
                                        new Point(s.Vertices[1].X, s.Vertices[1].Y)),
            };

            var root = new Canvas { RenderTransformOrigin = new Point(0, 0) };
            var tg = new TransformGroup();
            tg.Children.Add(new TranslateTransform());
            tg.Children.Add(new RotateTransform());
            tg.Children.Add(new ScaleTransform());
            root.RenderTransform = tg;
            root.Children.Add(img);
            root.Children.Add(tint);
            root.Children.Add(edge);
            root.Children.Add(bevel);
            _stage.Children.Add(root);
            _items.Add((s, root, img, tint, edge, bevel));
        }
        // splash.ts:41 汇聚完成后不再画碎片，整脸直绘 drawImage(face,left,top,board,board)
        _finalFace = new Image
        {
            Source = _face,
            Width = _geo.Board,
            Height = _geo.Board,
            Stretch = Stretch.Fill,
            Visibility = Visibility.Collapsed,
        };
        RenderOptions.SetBitmapScalingMode(_finalFace, BitmapScalingMode.NearestNeighbor);
        _stage.Children.Add(_finalFace);
        Canvas.SetLeft(_finalFace, _geo.Left);
        Canvas.SetTop(_finalFace, _geo.Top);
        // 碎片后建，胶囊要始终盖在最上层
        if (_caption is not null) { _stage.Children.Remove(_caption); _stage.Children.Add(_caption); }
    }

    void OnRendering(object? sender, EventArgs e)
    {
        var elapsed = _clock.ElapsedMs;
        // splash.ts:30-33：渲染循环自己按 60fps 掐表，高刷屏上不做限速会让汇聚节奏变快
        if (_nextFrame != 0 && elapsed + .5 < _nextFrame) return;
        if (_nextFrame == 0) _nextFrame = elapsed;
        _nextFrame += BootGlass.BootFrameMs;
        if (_nextFrame <= elapsed) _nextFrame = elapsed + BootGlass.BootFrameMs;

        var delta = _lastFrame == 0 ? BootGlass.BootFrameMs : elapsed - _lastFrame;
        _lastFrame = elapsed;

        var reduced = SystemParameters.ClientAreaAnimation == false;
        if (double.IsNaN(_convergeAt) && (reduced || elapsed >= BootGlass.GlassFloatMinMs)) _convergeAt = elapsed;
        if (double.IsNaN(_convergeAt) && !reduced) BootGlass.Advance(_geo, elapsed, delta, _pointer);

        var complete = reduced || (elapsed - _convergeAt >= BootGlass.ConvergeDuration);
        if (_finalFace is not null) _finalFace.Visibility = complete ? Visibility.Visible : Visibility.Collapsed;
        foreach (var (shard, root, image, tint, _, bevel) in _items)
        {
            var p = BootGlass.Pose(shard, elapsed, double.IsNaN(_convergeAt) ? null : _convergeAt);
            var tg = (TransformGroup)root.RenderTransform;
            ((TranslateTransform)tg.Children[0]).X = p.x;
            ((TranslateTransform)tg.Children[0]).Y = p.y;
            ((RotateTransform)tg.Children[1]).Angle = p.rot * 180 / Math.PI;
            ((ScaleTransform)tg.Children[2]).ScaleX = ((ScaleTransform)tg.Children[2]).ScaleY = p.scale;
            root.Visibility = complete ? Visibility.Collapsed : Visibility.Visible;
            var glass = 1 - p.progress;
            tint.Opacity = glass;
            image.Opacity = .12 + .88 * p.progress;
            var b = (SolidColorBrush)bevel.Stroke;
            b.Opacity = .55 + Math.Sin(elapsed / 1100 + shard.Phase) * .25;
        }
        if (complete && !_assembled && elapsed - _convergeAt >= BootGlass.ConvergeDuration + BootGlass.AssembledHoldMs)
        {
            _assembled = true;
            Leave();
        }
        else if (complete && !_assembled)
        {
            // 汇聚完成后整脸静止停留，等够 ASSEMBLED_HOLD_MS 再退场
        }
    }

    void Leave()
    {
        if (_closing) return;
        _closing = true;
        CompositionTarget.Rendering -= OnRendering;
        _pointerTimer?.Stop();
        _stageTimer?.Stop();
        // splash.css:20 body.leaving { opacity:0; transition: opacity 320ms ease }
        var fade = new System.Windows.Media.Animation.DoubleAnimation(1, 0, TimeSpan.FromMilliseconds(320));
        fade.Completed += (_, _) =>
        {
            _onDone?.Invoke();
            Close();
        };
        _stage.BeginAnimation(OpacityProperty, fade);
    }
}

/// <summary>只用毫秒的简易计时器，避免到处引 System.Diagnostics。</summary>
public sealed class Stopwatch2
{
    readonly DateTime _t0 = DateTime.UtcNow;
    public double ElapsedMs => (DateTime.UtcNow - _t0).TotalMilliseconds;
    public void Start() { }
}
