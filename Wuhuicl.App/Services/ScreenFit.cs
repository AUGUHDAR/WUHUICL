using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Interop;

namespace Wuhuicl.App.Services;

/// <summary>
/// 起窗落位：把窗口夹进它所在屏的工作区，尺寸不超、位置不出界。
///
/// 为什么要显式做：进程是 system aware（见 app.manifest 的说明），WPF 的渲染缩放不跟屏走，
/// 混合 DPI 双屏时窗口会在往返一次后按系统缩放翻倍，实测出现过 5440×3440 落在两块屏之外、
/// 界面上半截是黑的。夹一次工作区至少保证任何时候都看得见、摸得着。
/// </summary>
static class ScreenFit
{
    const uint SWP_NOZORDER = 0x4, SWP_NOACTIVATE = 0x10;

    [StructLayout(LayoutKind.Sequential)]
    struct Rect { public int Left, Top, Right, Bottom; }

    [StructLayout(LayoutKind.Sequential)]
    struct MonitorInfoEx
    {
        public int cbSize;
        public Rect rcMonitor;
        public Rect rcWork;
        public uint dwFlags;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string szDevice;
    }

    [DllImport("user32.dll")] static extern bool SetWindowPos(IntPtr h, IntPtr after,
        int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out Rect r);
    [DllImport("user32.dll")] static extern IntPtr MonitorFromWindow(IntPtr h, uint flags);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    static extern bool GetMonitorInfoW(IntPtr mon, ref MonitorInfoEx mi);

    public static void FitToMonitor(Window w)
    {
        var hwnd = new WindowInteropHelper(w).Handle;
        if (hwnd == IntPtr.Zero || w.WindowState != WindowState.Normal) return;
        var mi = new MonitorInfoEx { cbSize = Marshal.SizeOf<MonitorInfoEx>() };
        if (!GetMonitorInfoW(MonitorFromWindow(hwnd, 0 /*TONEAREST*/), ref mi)) return;
        GetWindowRect(hwnd, out var r);

        var work = mi.rcWork;
        int cx = Math.Min(r.Right - r.Left, work.Right - work.Left);
        int cy = Math.Min(r.Bottom - r.Top, work.Bottom - work.Top);
        int x = Math.Clamp(r.Left, work.Left, Math.Max(work.Left, work.Right - cx));
        int y = Math.Clamp(r.Top, work.Top, Math.Max(work.Top, work.Bottom - cy));
        if (x == r.Left && y == r.Top && cx == r.Right - r.Left && cy == r.Bottom - r.Top) return;
        SetWindowPos(hwnd, IntPtr.Zero, x, y, cx, cy, SWP_NOZORDER | SWP_NOACTIVATE);
    }
}
