using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Media.Media3D;
using System.Windows.Threading;

namespace Wuhuicl.App.Controls;

/// <summary>
/// 皮肤 3D 纸娃娃，1:1 复刻原版 skinview3d v3.4.2（MIT）+ SkinViewer3D.vue + skinModel.ts：
/// · 几何：头 8×8×8（帽层 9×9×9）中心 y28；身 8×12×4（外套 8.5×12.5×4.5）中心 y18；
///   臂 4×12×4 挂点 (±5,-2)、内枢轴 (∓1,-4)（slim ∓0.5）；腿 4×12×4 挂点 (±1.9,-12,∓0.1)、内枢轴 (0,-6)。
///   皮肤组 y=24，总高 32（skinModel.ts:8、vendor/skinview3d/model.ts:128-260）。
/// · UV：setUVs 六面区域公式（model.ts:31-60），64×64 皮肤。
/// · 动画：angle=cos(t·4.71)·π/4·walk，左臂正右臂负，腿反相（skinModel.ts:24-30）；
///   walk/idle 切换走 blend 渐变（SkinViewer3D.vue:63）。
/// · 相机：fov 45，distance=max(20,10/aspect)/tan(22.5°)，位置 (0,16+sin(pitch)d,cos(pitch)d)
///   看 (0,16,0)，初始 yaw=-0.35（SkinViewer3D.vue:18,95,99）。
/// · 光：环境白 + 平行白自 (-15,30,50)（:103-105）。
/// · 交互：拖拽 yaw+=dx·0.01 / pitch 夹 ±75°，滚轮 zoom 夹 0.5..3，双击回位（:76-98）。
/// 贴图最近邻预放大 16×：WPF 3D 采样固定双线性，放大后采样点落在纹素内部，视觉≈pixelated。
/// </summary>
public sealed class SkinDoll3D : Viewport3D
{
    /// <summary>初始转角（度）。原版 -0.35rad；截图要验六面 UV 时用它转到侧面/背面。</summary>
    static double DefaultYaw =>
        double.TryParse(Environment.GetEnvironmentVariable("DOLL_YAW"), out var deg) ? deg * Math.PI / 180 : -0.35;

    double _yaw = DefaultYaw, _pitch, _zoom = 1, _targetYaw = DefaultYaw, _targetPitch, _targetZoom = 1;
    double _seconds, _walk, _targetWalk = 1;          // blend 从 0 渐入
    readonly DispatcherTimer _clock = new(DispatcherPriority.Render) { Interval = TimeSpan.FromMilliseconds(16) };
    Point _dragLast;
    bool _dragging;

    readonly RotateTransform3D _leftArmRot = ArmRot(), _rightArmRot = ArmRot(),
        _leftLegRot = ArmRot(), _rightLegRot = ArmRot();
    readonly AxisAngleRotation3D _playerAxis = new(new Vector3D(0, 1, 0), 0);
    readonly PerspectiveCamera _cam = new()
    {
        FieldOfView = 45,
        Position = new Point3D(0, 16, 48),
        LookDirection = new Vector3D(0, 0, -1),
        UpDirection = new Vector3D(0, 1, 0),
        NearPlaneDistance = 0.5,
        FarPlaneDistance = 500,
    };
    double _baseDistance = 48;

    /// <summary>调试旋钮：强制相机距离（dolltest 用），正常路径为 null。</summary>
    public static double? DebugDistance { get; set; }
    /// <summary>调试旋钮：锁死摆动角为 0（T-pose 排查几何/UV）。DOLL_FREEZE=1。</summary>
    public static bool DebugFreeze { get; set; } = Environment.GetEnvironmentVariable("DOLL_FREEZE") == "1";

    public static readonly DependencyProperty SkinProperty = DependencyProperty.Register(
        nameof(Skin), typeof(BitmapSource), typeof(SkinDoll3D),
        new PropertyMetadata(null, (d, _) => ((SkinDoll3D)d).Rebuild()));

    public static readonly DependencyProperty SlimProperty = DependencyProperty.Register(
        nameof(Slim), typeof(bool), typeof(SkinDoll3D),
        new PropertyMetadata(true, (d, _) => ((SkinDoll3D)d).Rebuild()));

    public static readonly DependencyProperty WalkProperty = DependencyProperty.Register(
        nameof(Walk), typeof(bool), typeof(SkinDoll3D),
        new PropertyMetadata(true, (d, e) => ((SkinDoll3D)d)._targetWalk = (bool)e.NewValue ? 1 : 0));

    public BitmapSource? Skin { get => (BitmapSource?)GetValue(SkinProperty); set => SetValue(SkinProperty, value); }
    public bool Slim { get => (bool)GetValue(SlimProperty); set => SetValue(SlimProperty, value); }
    public bool Walk { get => (bool)GetValue(WalkProperty); set => SetValue(WalkProperty, value); }

    public SkinDoll3D()
    {
        Children.Add(new ModelVisual3D
        {
            Content = new Model3DGroup
            {
                Children = { new AmbientLight(Colors.White), new DirectionalLight(Colors.White, new Vector3D(-15, 30, 50)) }
            }
        });

        _clock.Tick += ClockTick;
        Loaded += (_, _) => { Fit(); Rebuild(); };
        SizeChanged += (_, _) => Fit();

        // SkinViewer3D.vue:76-98 的指针交互
        MouseLeftButtonDown += (_, e) =>
        {
            _dragging = true; _dragLast = e.GetPosition(this);
            CaptureMouse();
            StartClock();
        };
        MouseMove += (_, e) =>
        {
            if (!_dragging) return;
            var p = e.GetPosition(this);
            _targetYaw += (p.X - _dragLast.X) * 0.01;
            _targetPitch = Math.Clamp(_targetPitch + (p.Y - _dragLast.Y) * 0.01, -Math.PI * 5 / 12, Math.PI * 5 / 12);
            _dragLast = p;
            StartClock();
        };
        MouseLeftButtonUp += (_, _) => { _dragging = false; ReleaseMouseCapture(); };
        MouseWheel += (_, e) =>
        {
            _targetZoom = Math.Clamp(_targetZoom * Math.Exp(-e.Delta * 0.0012), 0.5, 3);
            StartClock();
        };
        // UIElement 没有 MouseDoubleClick（那是 Control 的）→ 用 ClickCount==2 表达双击回位
        MouseDown += (_, e) =>
        {
            if (e.ClickCount == 2) { _targetYaw = DefaultYaw; _targetPitch = 0; _targetZoom = 1; StartClock(); }
        };
    }

    static RotateTransform3D ArmRot() => new(new AxisAngleRotation3D(new Vector3D(1, 0, 0), 0));

    /// <summary>诊断：相机 + 模型包围盒。</summary>
    public string DumpDiag()
    {
        var sb = new System.Text.StringBuilder();
        sb.Append("cam=").Append(((PerspectiveCamera?)Camera)?.Position.ToString() ?? "null");
        Rect3D bounds = Rect3D.Empty;
        foreach (var mv in Children.OfType<ModelVisual3D>())
        {
            if (mv.Content is Model3DGroup group && !group.Bounds.IsEmpty)
                bounds = Rect3D.Union(bounds, group.Bounds);
        }
        sb.Append("  modelBounds=").Append(bounds.IsEmpty ? "none" : bounds.ToString());
        sb.Append("  viewportW=").Append(ActualWidth).Append(" H=").Append(ActualHeight);
        return sb.ToString();
    }

    void StartClock() => _clock.Start();

    // ---------------- 相机（SkinViewer3D.vue fit/render） ----------------
    void Fit()
    {
        double aspect = ActualWidth > 0 && ActualHeight > 0 ? ActualWidth / ActualHeight : 1;
        _baseDistance = Math.Max(20, 10 / aspect) / Math.Tan(45 * Math.PI / 360);
    }

    void ClockTick(object? sender, EventArgs e) => ApplyFrame();

    /// <summary>按当前状态推进一帧（也供离线渲染台同步调用）。</summary>
    public void ApplyFrame()
    {
        double dt = 1.0 / 60, k = 1 - Math.Exp(-14 * dt);
        _seconds += dt;
        _walk += (_targetWalk - _walk) * Math.Min(1, dt * 6);
        _yaw += (_targetYaw - _yaw) * k;
        _pitch += (_targetPitch - _pitch) * k;
        _zoom += (_targetZoom - _zoom) * k;

        double angle = DebugFreeze ? 0 : Math.Cos(_seconds * 4.71) * Math.PI / 4 * _walk;
        SetAngle(_leftArmRot, angle); SetAngle(_rightArmRot, -angle);
        SetAngle(_leftLegRot, -angle); SetAngle(_rightLegRot, angle);

        _playerAxis.Angle = _yaw * 180 / Math.PI;
        double d = DebugDistance ?? _baseDistance / _zoom;
        _cam.Position = new Point3D(0, 16 + Math.Sin(_pitch) * d, Math.Cos(_pitch) * d);
        _cam.LookDirection = new Point3D(0, 16, 0) - _cam.Position;
        Camera = _cam;

        // 原版静止时停帧（:69 的 rAF 续帧条件）：非走路 + 交互收敛后停表
        bool settled = !_dragging && _targetWalk == 0
                       && Math.Abs(_targetYaw - _yaw) + Math.Abs(_targetPitch - _pitch) + Math.Abs(_targetZoom - _zoom) < 0.0001
                       && Math.Abs(_walk) < 0.001;
        if (settled) _clock.Stop();
    }

    static void SetAngle(RotateTransform3D t, double rad) => ((AxisAngleRotation3D)t.Rotation).Angle = rad * 180 / Math.PI;

    /// <summary>排查开关：DOLL_PARTS=head/body/arms/legs 控制渲染到哪一层，DOLL_HAT=0 只画内层，
    /// DOLL_YAW=180 定死初始转角（截图要能看到背面/侧面才能验六面 UV）。</summary>
    static string Parts => (Environment.GetEnvironmentVariable("DOLL_PARTS") ?? "all").ToLowerInvariant();
    static bool WantHat => Environment.GetEnvironmentVariable("DOLL_HAT") != "0";

    // ---------------- 模型构建 ----------------
    public void Rebuild()
    {
        if (Skin is null) return;
        var stage = Children.OfType<ModelVisual3D>().FirstOrDefault(m => m.Content is Model3DGroup);
        if (stage is null) return;
        var group = (Model3DGroup)stage.Content;
        for (int i = group.Children.Count - 1; i >= 0; i--)
            if (group.Children[i] is not Light)
                group.Children.RemoveAt(i);

        var (mat, px, texW, texH) = Upscaled(Skin);
        double armW = Slim ? 3 : 4, pivotX = Slim ? 0.5 : 1;
        int armUv = Slim ? 3 : 4;      // slim 臂贴图区也按 3 宽取（model.ts:160）
        var parts = Parts;
        var hat = WantHat;

        // 皮肤组：WPF 是行向量约定（p·M0·M1，先挂的先作用）。
        // three.js 里 player.rotation 绕足部原点转、skin 组再抬到 y24 → 先 yaw 后平移。
        var skinGroup = new Model3DGroup
        {
            Transform = new Transform3DGroup
            {
                Children = { new RotateTransform3D(_playerAxis), new TranslateTransform3D(0, 24, 0) }
            }
        };

        // 头：mesh 中心 (0,4,0)（model.ts:139）
        if (parts is "all" or "head" or "body" or "arms" or "legs")
        {
            Add(skinGroup, Box(8, 8, 8, 8, 8, 8, 0, 0, px, texW, texH), mat, false, 0, 4, 0);
            if (hat) Add(skinGroup, Box(9, 9, 9, 8, 8, 8, 32, 0, px, texW, texH), mat, true, 0, 4, 0);
        }
        // 身：中心 (0,-6,0)（:155）
        if (parts is "all" or "body" or "arms" or "legs")
        {
            Add(skinGroup, Box(8, 12, 4, 8, 12, 4, 16, 16, px, texW, texH), mat, false, 0, -6, 0);
            if (hat) Add(skinGroup, Box(8.5, 12.5, 4.5, 8, 12, 4, 16, 32, px, texW, texH), mat, true, 0, -6, 0);
        }
        // three.js 层级：limb 组在挂点（肩 ±5,-2 / 髋 ±1.9,-12），pivot 在组内 (±pivotX,-4)（臂）或 (0,-6)（腿），
        // mesh 以 pivot 为中心 → 绕「组原点=肩/髋」钟摆摆动。mesh 偏移必须留在组内，不能并进平移。
        if (parts is "all" or "arms")
        {
            skinGroup.Children.Add(Limb(Box(armW, 12, 4, armUv, 12, 4, 40, 16, px, texW, texH),
                hat ? Box(armW + .5, 12.5, 4.5, armUv, 12, 4, 40, 32, px, texW, texH) : null, mat,
                new Point3D(-5, -2, 0), new Point3D(-pivotX, -4, 0), _rightArmRot));
            skinGroup.Children.Add(Limb(Box(armW, 12, 4, armUv, 12, 4, 32, 48, px, texW, texH),
                hat ? Box(armW + .5, 12.5, 4.5, armUv, 12, 4, 48, 48, px, texW, texH) : null, mat,
                new Point3D(5, -2, 0), new Point3D(pivotX, -4, 0), _leftArmRot));
        }
        if (parts is "all" or "legs")
        {
            skinGroup.Children.Add(Limb(Box(4, 12, 4, 4, 12, 4, 0, 16, px, texW, texH),
                hat ? Box(4.5, 12.5, 4.5, 4, 12, 4, 0, 32, px, texW, texH) : null, mat,
                new Point3D(-1.9, -12, -0.1), new Point3D(0, -6, 0), _rightLegRot));
            skinGroup.Children.Add(Limb(Box(4, 12, 4, 4, 12, 4, 16, 48, px, texW, texH),
                hat ? Box(4.5, 12.5, 4.5, 4, 12, 4, 0, 48, px, texW, texH) : null, mat,
                new Point3D(1.9, -12, 0.1), new Point3D(0, -6, 0), _leftLegRot));
        }

        group.Children.Add(skinGroup);
        StartClock();
    }

    /// <summary>肢体：组 = {绕组原点旋转, 平移到挂点}，mesh 在组内偏移 pivot（three.js 的 pivot 层）。</summary>
    static Model3DGroup Limb(MeshGeometry3D? inner, MeshGeometry3D? outer, Material mat,
        Point3D joint, Point3D pivot, RotateTransform3D rot)
    {
        var g = new Model3DGroup
        {
            Transform = new Transform3DGroup
            {
                Children = { rot, new TranslateTransform3D(joint.X, joint.Y, joint.Z) }
            },
        };
        Add(g, inner, mat, false, pivot.X, pivot.Y, pivot.Z);
        // 外套层比内层厚 0.5：three.js 的 2Box 直接是 12.5 高，中心仍在 pivot
        Add(g, outer, mat, true, pivot.X, pivot.Y, pivot.Z);
        return g;
    }

    /// <summary>空网格必须整块丢掉：透明像素被剔干净后 Positions 为空，
    /// 而 WPF 3D 画空网格不是"什么都不画"，而是一屏乱飞的垃圾三角形。</summary>
    static void Add(Model3DGroup owner, MeshGeometry3D? mesh, Material mat, bool twoSided,
        double x, double y, double z)
    {
        if (mesh is null || mesh.Positions.Count == 0) return;
        owner.Children.Add(new GeometryModel3D
        {
            Geometry = mesh,
            Material = mat,
            // layer1 是 FrontSide、layer2 是 DoubleSide+透明（model.ts:104-110）
            BackMaterial = twoSided ? mat : null,
            Transform = new TranslateTransform3D(x, y, z),
        });
    }

    /// <summary>
    /// setUVs（model.ts:31-60）六面区块 → WPF 盒子。
    /// 几何尺寸与贴图区块尺寸必须分开传：帽层几何 9³ / 8.5³ 但贴图区仍是 8³ / 4×12×4
    /// （model.ts:132-133、:148-149），混用会把贴图拉伸错位。
    /// 每个贴图像素单独成一个面片并丢掉全透明像素：WPF 3D 没有 alpha test，
    /// 透明片元照样写深度，帽层的透明区会把内层"挖"出洞（原版 layer2 用 alphaTest 1e-5）。
    /// </summary>
    static MeshGeometry3D Box(double gw, double gh, double gd,
        int uw, int uh, int ud, int u, int v, byte[] px, int texW, int texH)
    {
        var pos = new Point3DCollection();
        var uv = new PointCollection();
        var idx = new Int32Collection();

        double hw = gw / 2, hh = gh / 2, hd = gd / 2;
        // (bl, br, tr, tl) 四角 + 该面的贴图区块左上角与像素尺寸
        void Face(Point3D bl, Point3D br, Point3D tr, Point3D tl, int rx, int ry, int rw, int rh)
        {
            if (rw <= 0 || rh <= 0) return;
            // 整面不透明时退化成 1 个面片，省掉绝大多数三角形
            var cells = OpaqueCount(px, texW, rx, ry, rw, rh);
            if (cells == rw * rh) { Quad(bl, br, tr, tl, rx, ry, rw, rh, 0, 0, rw, rh); return; }
            for (var j = 0; j < rh; j++)
                for (var i = 0; i < rw; i++)
                    if (Alpha(px, texW, rx + i, ry + j) != 0)
                        // 第 7/8 个参数是"这一面共几个纹素"（几何分格的分母），不是格子尺寸
                        Quad(bl, br, tr, tl, rx, ry, rw, rh, i, j, 1, 1);
        }

        void Quad(Point3D bl, Point3D br, Point3D tr, Point3D tl,
            int rx, int ry, int rw, int rh, int cx, int cy, int cw, int ch)
        {
            // 贴图坐标：x 左→右，y 上→下。预放大是整数倍，0..1 的归一化与放大倍数无关，
            // 直接按源皮肤尺寸除即可（按放大后的尺寸除会把整张图缩到左上角 1/16，全是透明像素）
            double Tu(int k) => (double)k / texW;
            double Tv(int k) => (double)k / texH;
            // 面内插值：L(s,t)，t=0 是上缘（tl/tr），t=1 是下缘（bl/br）
            Point3D L(double s, double t)
            {
                var top = new Point3D(tl.X + (tr.X - tl.X) * s, tl.Y + (tr.Y - tl.Y) * s, tl.Z + (tr.Z - tl.Z) * s);
                var bot = new Point3D(bl.X + (br.X - bl.X) * s, bl.Y + (br.Y - bl.Y) * s, bl.Z + (br.Z - bl.Z) * s);
                return new Point3D(top.X + (bot.X - top.X) * t, top.Y + (bot.Y - top.Y) * t, top.Z + (bot.Z - top.Z) * t);
            }
            double x0 = (double)cx / rw, x1 = (double)(cx + cw) / rw;
            double y0 = (double)cy / rh, y1 = (double)(cy + ch) / rh;
            int vi = pos.Count;
            pos.Add(L(x0, y1)); pos.Add(L(x1, y1)); pos.Add(L(x1, y0)); pos.Add(L(x0, y0));
            uv.Add(new Point(Tu(rx + cx), Tv(ry + cy + ch)));
            uv.Add(new Point(Tu(rx + cx + cw), Tv(ry + cy + ch)));
            uv.Add(new Point(Tu(rx + cx + cw), Tv(ry + cy)));
            uv.Add(new Point(Tu(rx + cx), Tv(ry + cy)));
            idx.Add(vi); idx.Add(vi + 1); idx.Add(vi + 2);
            idx.Add(vi); idx.Add(vi + 2); idx.Add(vi + 3);
        }

        // 六面区块（setUVs:38-45）
        Face(new(-hw, -hh, hd), new(hw, -hh, hd), new(hw, hh, hd), new(-hw, hh, hd), u + ud, v + ud, uw, uh);            // front +Z
        Face(new(hw, -hh, -hd), new(-hw, -hh, -hd), new(-hw, hh, -hd), new(hw, hh, -hd), u + uw + 2 * ud, v + ud, uw, uh); // back -Z
        Face(new(hw, -hh, hd), new(hw, -hh, -hd), new(hw, hh, -hd), new(hw, hh, hd), u + uw + ud, v + ud, ud, uh);        // right +X
        Face(new(-hw, -hh, -hd), new(-hw, -hh, hd), new(-hw, hh, hd), new(-hw, hh, -hd), u, v + ud, ud, uh);              // left -X
        Face(new(-hw, hh, hd), new(hw, hh, hd), new(hw, hh, -hd), new(-hw, hh, -hd), u + ud, v, uw, ud);                  // top +Y
        Face(new(-hw, -hh, -hd), new(hw, -hh, -hd), new(hw, -hh, hd), new(-hw, -hh, hd), u + uw + ud, v, uw, ud);         // bottom -Y

        return new MeshGeometry3D { Positions = pos, TextureCoordinates = uv, TriangleIndices = idx };
    }

    const int Upsample = 16;

    static int Alpha(byte[] px, int texW, int x, int y) =>
        x < 0 || y < 0 || x >= texW || y * texW >= px.Length / 4 ? 0 : px[(y * texW + x) * 4 + 3];

    static int OpaqueCount(byte[] px, int texW, int rx, int ry, int rw, int rh)
    {
        var n = 0;
        for (var j = 0; j < rh; j++)
            for (var i = 0; i < rw; i++)
                if (Alpha(px, texW, rx + i, ry + j) == 255) n++;
        return n;
    }

    /// <summary>
    /// 最近邻 16× 预放大：WPF 3D 固定双线性采样，不放大时纹素边界会糊成一片（原版 NearestFilter）。
    /// 顺带把原始 BGRA 交给调用方做逐纹素 alpha 判定。同一张皮肤只算一次。
    /// </summary>
    static (DiffuseMaterial Mat, byte[] Px, int TexW, int TexH) Upscaled(BitmapSource skin)
    {
        var src = new FormatConvertedBitmap(skin, PixelFormats.Bgra32, null, 0);
        int sw = src.PixelWidth, sh = src.PixelHeight;
        var px = new byte[sw * sh * 4];
        src.CopyPixels(px, sw * 4, 0);
        var key = (skin, sw, sh);
        if (_brush is null || _brushKey != key)
        {
            int dw = sw * Upsample, dh = sh * Upsample;
            var outPx = new byte[dw * dh * 4];
            for (int y = 0; y < dh; y++)
            {
                int sy = y / Upsample * sw;
                for (int x = 0; x < dw; x++)
                {
                    int si = (sy + x / Upsample) * 4;
                    int di = (y * dw + x) * 4;
                    outPx[di] = px[si]; outPx[di + 1] = px[si + 1];
                    outPx[di + 2] = px[si + 2]; outPx[di + 3] = px[si + 3];
                }
            }
            var wb = new WriteableBitmap(dw, dh, 96, 96, PixelFormats.Bgra32, null);
            wb.WritePixels(new Int32Rect(0, 0, dw, dh), outPx, dw * 4, 0);
            wb.Freeze();
            // 3D 材质里 ViewportUnits 必须是 Absolute：RelativeToBoundingBox 的"包围盒"
            // 是每个被绘制面的盒子，整张图会被铺到每一个面上，TextureCoordinates 形同虚设
            _brush = new DiffuseMaterial(new ImageBrush(wb)
            {
                ViewportUnits = BrushMappingMode.Absolute,
                Viewport = new Rect(0, 0, 1, 1),
                Stretch = Stretch.Fill,
            });
            _brush.Freeze();
            _brushKey = key;
        }
        return (_brush, px, sw, sh);
    }

    static DiffuseMaterial? _brush;
    static (BitmapSource, int, int)? _brushKey;
}
