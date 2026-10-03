using System;
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

    /// <summary>
    /// 脸部（皮肤 (8,8) 起 8×8）整数倍放大。逐像素自己铺而不是用 TransformedBitmap：
    /// .NET 的 WPF 没有 BitmapTransformOptions，TransformedBitmap 只能走 WIC 插值，
    /// 像素头像会被抹成一团糊，而原版是 image-rendering:pixelated /
    /// ctx.imageSmoothingEnabled=false（splash.ts:19）。
    /// </summary>
    public static BitmapSource Face(int size = 64)
    {
        int cell = Math.Max(1, size / 8), side = cell * 8;
        var src = new FormatConvertedBitmap(Skin(), PixelFormats.Bgra32, null, 0);
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
