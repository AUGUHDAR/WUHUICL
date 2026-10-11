using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using Microsoft.Win32;

namespace Wuhuicl.App.Services;

/// <summary>
/// 主题切换：只替换 MergedDictionaries 里最后一个色值字典，控件样式全部走
/// DynamicResource，所以换主题不需要重建窗口。
/// </summary>
public static class ThemeManager
{
    /// <summary>原版共 6 套主题，默认「默认·黑紫」；没有"跟随系统"这一项。</summary>
    public static readonly (string Id, string Name, string File, bool Dark)[] Themes =
    [
        ("transparent", "默认·黑紫", "Transparent", true),
        ("blue-white", "白蓝", "Light", false),
        ("black-orange", "橙黑", "BlackOrange", true),
        ("black-pink", "粉黑", "BlackPink", true),
        ("white-pink", "粉白", "WhitePink", false),
        ("custom", "个性化", "Custom", false),
    ];

    // 主题字典固定在 MergedDictionaries[1]：[0] 是控件样式，绝不能被覆盖
    static int ThemeIndex = 1;

    /// <summary>换主题后广播：DynamicResource 的调用点会自动跟上，但代码里自绘的画刷需要重取。</summary>
    public static event Action? Applied;

    public static void Apply(string id)
    {
        var dict = new ResourceDictionary
        {
            Source = new Uri($"pack://application:,,,/Themes/{FileOf(id)}.xaml", UriKind.Absolute),
        };
        var rd = Application.Current.Resources.MergedDictionaries;
        if (ThemeIndex < rd.Count) rd[ThemeIndex] = dict;
        else { rd.Add(dict); ThemeIndex = rd.Count - 1; }
        // 标题栏与窗体背景要跟着走，否则深色主题下窗口边框仍是白的
        foreach (Window w in Application.Current.Windows)
            Native.SetImmersiveDarkMode(w, IsDark(id));
        Applied?.Invoke();
    }

    static string FileOf(string id) =>
        Themes.FirstOrDefault(t => t.Id == id).File ?? "Transparent";

    public static bool IsDark(string id) =>
        Themes.FirstOrDefault(t => t.Id == id).Dark;

    public static string DisplayName(string id) =>
        Themes.FirstOrDefault(t => t.Id == id).Name ?? "默认·黑紫";
}

static class SystemPref
{
    /// <summary>HKCU 的 AppsUseLightTheme；读不到时按浅色处理。</summary>
    public static bool IsLight()
    {
        try
        {
            using var k = Registry.CurrentUser.OpenSubKey(
                @"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
            var v = k?.GetValue("AppsUseLightTheme");
            return v is null || Convert.ToInt32(v) != 0;
        }
        catch { return true; }
    }
}

static class Native
{
    [DllImport("psapi.dll")]
    static extern bool SetProcessWorkingSetSize(IntPtr hProc, IntPtr min, IntPtr max);

    /// <summary>启动尘埃落定后修剪一次工作集：任务管理器默认「内存」列就是专用工作集，
    /// 启动期的高水位不该让用户一直盯着看。</summary>
    public static void TrimWorkingSet()
    {
        try { SetProcessWorkingSetSize(System.Diagnostics.Process.GetCurrentProcess().Handle, new IntPtr(-1), new IntPtr(-1)); }
        catch { /* 尽力而为 */ }
    }

    [System.Runtime.InteropServices.DllImport("dwmapi.dll")]
    static extern int DwmSetWindowAttribute(IntPtr hwnd, int attr, ref int val, int size);

    /// <summary>开机页要跟随系统光标做力场扰动，而它自己是点击穿透窗口，拿不到鼠标事件。</summary>
    [System.Runtime.InteropServices.DllImport("user32.dll")]
    public static extern bool GetCursorPos(out WinPoint p);

    [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
    public struct WinPoint { public int X; public int Y; }

    const int DWMWA_USE_IMMERSIVE_DARK_MODE = 20;
    const int DWMWA_WINDOW_CORNER_PREFERENCE = 33;
    const int DWMWCP_ROUND = 2;

    /// <summary>窗口本体不透明时，圆角只能交给 DWM：原版 Electron 就是 roundedCorners:true。</summary>
    public static void RoundCorners(Window w)
    {
        try
        {
            var h = new System.Windows.Interop.WindowInteropHelper(w).Handle;
            if (h == IntPtr.Zero) return;
            int v = DWMWCP_ROUND;
            DwmSetWindowAttribute(h, DWMWA_WINDOW_CORNER_PREFERENCE, ref v, sizeof(int));
        }
        catch { /* Win10 没有圆角属性，方角即可 */ }
    }

    /// <summary>让系统标题栏/边框跟随主题，避免深色界面配一条白边框。</summary>
    public static void SetImmersiveDarkMode(Window w, bool dark)
    {
        try
        {
            var h = new System.Windows.Interop.WindowInteropHelper(w).Handle;
            if (h == IntPtr.Zero) return;
            int v = dark ? 1 : 0;
            DwmSetWindowAttribute(h, DWMWA_USE_IMMERSIVE_DARK_MODE, ref v, sizeof(int));
        }
        catch { /* 老系统没有这个属性，忽略 */ }
    }
}

/// <summary>
/// 皮肤素材处理：头像与纸娃娃都从用户提供的 skin.png 现算，
/// 不用任何外部图片资源，也不会引入第三方库。
/// </summary>
public static class SkinRenderer
{
    static BitmapSource? _skin;

    /// <summary>读取随包素材（hero.png 等），缺失返回 null 由界面降级处理。</summary>
    /// <summary>资源是否存在（磁盘或内嵌任一），不实际解码。</summary>
    public static bool HasAsset(string name)
    {
        if (File.Exists(Path.Combine(AppContext.BaseDirectory, "Assets", name))) return true;
        var res = "Wuhuicl.App.Assets." + name.Replace('/', '.');
        return typeof(ThemeManager).Assembly.GetManifestResourceStream(res) is not null;
    }

    public static BitmapSource? LoadAsset(string name)
    {
        try
        {
            var path = Path.Combine(AppContext.BaseDirectory, "Assets", name);
            var bmp = new BitmapImage();
            bmp.BeginInit();
            bmp.CacheOption = BitmapCacheOption.OnLoad;
            // hero 源图限宽解码（cover 视口 ~1600 DIP 足够，省一半 WIC 原生位图）；
            // 皮肤文件绝不能限：64x64 的 UV 裁剪坐标系会被放大打散（头像/娃娃全空白的根因）
            if (name.StartsWith("hero/", StringComparison.Ordinal)) bmp.DecodePixelWidth = 960;
            if (File.Exists(path)) bmp.UriSource = new Uri(path);
            else
            {
                // 单文件发布：Assets 以 EmbeddedResource 内嵌，键 = Wuhuicl.App.Assets.<相对路径斜杠转点>
                var asm = typeof(ThemeManager).Assembly;
                var res = "Wuhuicl.App.Assets." + name.Replace('/', '.');
                var stream = asm.GetManifestResourceStream(res);
                if (stream is null) return null;
                bmp.StreamSource = stream;
            }
            bmp.EndInit();
            bmp.Freeze();
            return bmp;
        }
        catch { return null; }
    }

    static BitmapSource Skin()
    {
        if (_skin is not null) return _skin;
        // 必须走 LoadAsset：单文件发布时 Assets 内嵌在程序集里，直接拼磁盘路径会
        // 掉进 MakeFallback 的占位脸（「我的皮肤怎么变了」回归的根因）
        var bmp = LoadAsset("skin.png") ?? MakeFallback();
        if (!bmp.IsFrozen) bmp.Freeze();
        _skin = bmp;
        return bmp;
    }

    /// <summary>
    /// 无皮肤文件时的占位：逐条搬 fallbackSkin.ts:6-49 的 rect()（64×64 标准 UV，
    /// 未用区域与外层区域保持透明，等价原版"transparent unused/outer-layer regions"）。
    /// 之前这里是随手画的三块矩形（Steve 风格占位），属于自造图形。绘制顺序必须与源一致：
    /// 后画的 rect 覆盖先画的。
    /// </summary>
    static BitmapSource MakeFallback()
    {
        var wb = new WriteableBitmap(64, 64, 96, 96, PixelFormats.Bgra32, null);
        var px = new byte[64 * 64 * 4];
        void Rect(int x, int y, int w, int h, int hex)
        {
            byte r = (byte)(hex >> 16 & 255), g = (byte)(hex >> 8 & 255), b = (byte)(hex & 255);
            for (int yy = y; yy < y + h && yy < 64; yy++)
                for (int xx = x; xx < x + w && xx < 64; xx++)
                {
                    int i = (yy * 64 + xx) * 4;
                    px[i] = b; px[i + 1] = g; px[i + 2] = r; px[i + 3] = 255;
                }
        }
        const int skin = 0xd3a582, shade = 0xbf8b6c, hair = 0x49382f;
        const int jacket = 0x398d8a, seam = 0x286b70, light = 0x69b5ad, pants = 0x38485e, shoe = 0x27303d;
        // 头：发顶、下颌底面与四个侧面（:16-17 起，正面朝向默认预览相机）
        Rect(8, 0, 8, 8, hair); Rect(16, 0, 8, 8, shade);
        Rect(0, 8, 32, 8, skin); Rect(0, 8, 32, 2, hair);
        Rect(24, 8, 8, 7, hair); Rect(0, 8, 6, 5, hair); Rect(18, 8, 6, 5, hair);
        Rect(8, 10, 2, 1, hair); Rect(14, 10, 2, 2, hair);
        Rect(9, 11, 2, 1, 0x675043); Rect(13, 11, 2, 1, 0x675043);
        Rect(9, 12, 2, 1, 0xf2ede2); Rect(13, 12, 2, 1, 0xf2ede2);
        Rect(10, 12, 1, 1, 0x315f62); Rect(13, 12, 1, 1, 0x315f62);
        Rect(11, 13, 2, 1, shade); Rect(11, 15, 2, 1, 0x915d50);
        // 身：青色拉链外套压浅色内衣，含侧面与背面（:26-33）
        Rect(20, 16, 16, 4, jacket); Rect(16, 20, 24, 12, jacket);
        Rect(16, 20, 4, 12, seam); Rect(28, 20, 4, 12, seam);
        Rect(23, 16, 2, 3, skin); Rect(22, 20, 4, 2, 0xe3e5df);
        Rect(23, 22, 2, 10, 0xe3e5df); Rect(20, 22, 1, 8, light);
        Rect(26, 26, 2, 2, seam); Rect(20, 30, 3, 2, seam); Rect(25, 30, 3, 2, seam);
        Rect(32, 22, 8, 1, light); Rect(32, 30, 8, 2, seam);
        void Arm(int x, int y)
        {
            Rect(x + 4, y, 4, 4, jacket); Rect(x + 8, y, 4, 4, skin);
            Rect(x, y + 4, 16, 12, jacket);
            Rect(x, y + 10, 16, 1, light); Rect(x, y + 11, 16, 1, seam);
            Rect(x, y + 12, 16, 4, skin); Rect(x, y + 15, 16, 1, shade);
            Rect(x, y + 4, 4, 6, seam);
        }
        void Leg(int x, int y)
        {
            Rect(x + 4, y, 4, 4, pants); Rect(x + 8, y, 4, 4, shoe);
            Rect(x, y + 4, 16, 12, pants); Rect(x, y + 13, 16, 3, shoe);
            Rect(x + 4, y + 4, 1, 9, 0x506078); Rect(x + 4, y + 14, 4, 1, 0xb4bfc4);
            Rect(x + 12, y + 5, 3, 3, 0x304054);
        }
        Arm(40, 16); Arm(32, 48); Leg(0, 16); Leg(16, 48);
        wb.WritePixels(new Int32Rect(0, 0, 64, 64), px, 64 * 4, 0);
        wb.Freeze();
        return wb;
    }

    /// <summary>
    /// 脸部（皮肤 (8,8) 起 8×8）整数倍放大。逐像素自己铺而不是用 TransformedBitmap：
    /// .NET 的 WPF 没有 BitmapTransformOptions，TransformedBitmap 只能走 WIC 插值，
    /// 像素头像会被抹成一团糊，而原版是 image-rendering:pixelated /
    /// ctx.imageSmoothingEnabled=false（splash.ts:19）。
    /// </summary>
    public static BitmapSource Face(int size = 64) => FaceFrom(Skin(), size);

    /// <summary>按账号取脸：微软登录时下载的档案皮肤在 &lt;DataDir&gt;\facecache\&lt;uuid&gt;.png，
    /// 有就用它（Avatar.vue 按账号渲染的等价物），否则回落全局皮肤。
    /// FaceCacheDir 由 LaunchSession 启动时按 GamePaths.DataDir 指一次。</summary>
    public static string? FaceCacheDir { get; set; }
    static readonly Dictionary<string, BitmapSource> _faceCache = new();
    static readonly HashSet<string> _faceMissing = new();   // 没有缓存文件的也记下，避免每行都去摸磁盘

    public static BitmapSource FaceFor(string? uuid, int size = 64)
    {
        if (string.IsNullOrEmpty(uuid) || FaceCacheDir is null) return Face(size);
        if (_faceCache.TryGetValue(uuid, out var hit)) return hit;
        if (_faceMissing.Contains(uuid)) return Face(size);
        try
        {
            var f = Path.Combine(FaceCacheDir, uuid + ".png");
            if (!File.Exists(f)) { _faceMissing.Add(uuid); return Face(size); }
            var bmp = new BitmapImage();
            bmp.BeginInit();
            bmp.CacheOption = BitmapCacheOption.OnLoad;
            bmp.UriSource = new Uri(f);
            bmp.EndInit();
            bmp.Freeze();
            var face = FaceFrom(bmp, size);
            _faceCache[uuid] = face;
            return face;
        }
        catch { _faceMissing.Add(uuid); return Face(size); }
    }

    /// <summary>微软登录下载了新档案皮肤后清掉该账号的头像缓存。</summary>
    public static void InvalidateFace(string uuid)
    {
        _faceCache.Remove(uuid);
        _faceMissing.Remove(uuid);
    }

    static BitmapSource FaceFrom(BitmapSource skin, int size)
    {
        int cell = Math.Max(1, size / 8), side = cell * 8;
        var src = new FormatConvertedBitmap(skin, PixelFormats.Bgra32, null, 0);
        var px = new byte[src.PixelWidth * src.PixelHeight * 4];
        src.CopyPixels(px, src.PixelWidth * 4, 0);
        var outPx = new byte[side * side * 4];
        for (var y = 0; y < 8; y++)
            for (var x = 0; x < 8; x++)
            {
                var i = ((8 + y) * src.PixelWidth + 8 + x) * 4;
                for (var dy = 0; dy < cell; dy++)
                    for (var dx = 0; dx < cell; dx++)
                    {
                        var o = (((y * cell + dy) * side) + x * cell + dx) * 4;
                        outPx[o] = px[i]; outPx[o + 1] = px[i + 1];
                        outPx[o + 2] = px[i + 2]; outPx[o + 3] = px[i + 3];
                    }
            }
        var wb = new WriteableBitmap(side, side, 96, 96, PixelFormats.Bgra32, null);
        wb.WritePixels(new Int32Rect(0, 0, side, side), outPx, side * 4, 0);
        wb.Freeze();
        return wb;
    }

    /// <summary>3D 查看器用的皮肤源；reload=true 清缓存，之后 Face()/IsSlimSkin() 读到的就是新文件。</summary>
    public static BitmapSource SkinSource(bool reload = false)
    {
        if (reload) _skin = null;
        return Skin();
    }

    /// <summary>slim（Alex）判定：classic 右臂背面右缘一列（x=54,y=20..32）有像素（skin-render.ts:28-40）。</summary>
    public static bool IsSlimSkin()
    {
        try
        {
            var src = new System.Windows.Media.Imaging.FormatConvertedBitmap(Skin(), System.Windows.Media.PixelFormats.Bgra32, null, 0);
            var px = new byte[src.PixelWidth * src.PixelHeight * 4];
            src.CopyPixels(px, src.PixelWidth * 4, 0);
            for (var y = 20; y < 32; y++)
            {
                var i = (y * src.PixelWidth + 54) * 4;
                if (px[i + 3] != 0) return false;
            }
            return true;
        }
        catch { return true; }
    }
}
