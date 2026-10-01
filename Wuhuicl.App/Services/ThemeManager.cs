using System.IO;
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
        ("white-pink", "粉白", "WhitePink", false),
        ("black-pink", "粉黑", "BlackPink", true),
        ("custom", "个性化", "Custom", false),
    ];

    // 主题字典固定在 MergedDictionaries[1]：[0] 是控件样式，绝不能被覆盖
    static int ThemeIndex = 1;

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
    [System.Runtime.InteropServices.DllImport("dwmapi.dll")]
    static extern int DwmSetWindowAttribute(IntPtr hwnd, int attr, ref int val, int size);

    const int DWMWA_USE_IMMERSIVE_DARK_MODE = 20;

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
    public static BitmapSource? LoadAsset(string name)
    {
        try
        {
            var path = Path.Combine(AppContext.BaseDirectory, "Assets", name);
            if (!File.Exists(path)) return null;
            var bmp = new BitmapImage();
            bmp.BeginInit();
            bmp.CacheOption = BitmapCacheOption.OnLoad;
            bmp.UriSource = new Uri(path);
            bmp.EndInit();
            bmp.Freeze();
            return bmp;
        }
        catch { return null; }
    }

    static BitmapSource Skin()
    {
        if (_skin is not null) return _skin;
        var path = Path.Combine(AppContext.BaseDirectory, "Assets", "skin.png");
        var bmp = new BitmapImage();
        if (File.Exists(path))
        {
            bmp.BeginInit();
            bmp.CacheOption = BitmapCacheOption.OnLoad;
            bmp.UriSource = new Uri(path);
            bmp.EndInit();
        }
        else
        {
            return MakeFallback();
        }
        bmp.Freeze();
        _skin = bmp;
        return bmp;
    }

    static BitmapSource MakeFallback()
    {
        // 没有皮肤文件时给一张 Steve 风格的占位，保证界面不空
        var wb = new WriteableBitmap(64, 64, 96, 96, PixelFormats.Bgra32, null);
        var px = new byte[64 * 64 * 4];
        void Fill(int x, int y, int w, int h, byte r, byte g, byte b)
        {
            for (int yy = y; yy < y + h && yy < 64; yy++)
                for (int xx = x; xx < x + w && xx < 64; xx++)
                {
                    int i = (yy * 64 + xx) * 4;
                    px[i] = b; px[i + 1] = g; px[i + 2] = r; px[i + 3] = 255;
                }
        }
        Fill(8, 8, 8, 8, 0xB0, 0x84, 0x5C);        // 脸
        Fill(8, 0, 8, 8, 0x40, 0x33, 0x2B);        // 头发
        Fill(16, 16, 8, 12, 0x3E, 0x8E, 0x3E);     // 上身
        wb.WritePixels(new Int32Rect(0, 0, 64, 64), px, 64 * 4, 0);
        wb.Freeze();
        return wb;
    }

    /// <summary>脸部（皮肤 (8,8) 起 8×8），最近邻放大，保持像素风。</summary>
    public static BitmapSource Face(int size = 64)
    {
        var crop = new CroppedBitmap(Skin(), new Int32Rect(8, 8, 8, 8));
        var scaled = new TransformedBitmap(crop, new ScaleTransform(size / 8.0, size / 8.0));
        scaled.Freeze();
        return scaled;
    }

    /// <summary>
    /// 正面纸娃娃：头/身/双臂/双腿按 64×64 皮肤布局拼出来。
    /// 真正的 3D 预览下一轮做，这里先保证"用的是玩家自己的皮肤"。
    /// </summary>
    public static BitmapSource PaperDoll(int height = 190, bool reload = false)
    {
        if (reload) _skin = null;
        var s = Skin();
        double scale = height / 32.0;                 // 头 8 + 身 12 + 腿 12
        int w = (int)Math.Round(16 * scale), h = (int)Math.Round(32 * scale);
        var dv = new DrawingVisual();
        using (var dc = dv.RenderOpen())
        {
            void Part(int sx, int sy, int sw, int sh, double dx, double dy)
                => dc.DrawImage(new CroppedBitmap(s, new Int32Rect(sx, sy, sw, sh)),
                                new Rect(dx, dy, sw * scale, sh * scale));
            double cx = w / 2.0;
            Part(8, 8, 8, 8, cx - 4 * scale, 0);                       // 头
            Part(20, 20, 8, 12, cx - 4 * scale, 8 * scale);            // 身
            Part(44, 16, 8, 4, cx - 8 * scale, 8 * scale);             // 左臂
            Part(52, 16, 4, 4, cx - 8 * scale, 12 * scale);            // 左臂下
            Part(40, 16, 8, 4, cx + 4 * scale, 8 * scale);             // 右臂
            Part(44, 16, 4, 4, cx + 8 * scale, 12 * scale);            // 右臂下
            Part(16, 16, 4, 12, cx - 4 * scale, 20 * scale);           // 左腿
            Part(20, 16, 4, 12, cx, 20 * scale);                       // 右腿
        }
        var rtb = new RenderTargetBitmap(w, h, 96, 96, PixelFormats.Pbgra32);
        rtb.Render(dv);
        rtb.Freeze();
        return rtb;
    }
}
