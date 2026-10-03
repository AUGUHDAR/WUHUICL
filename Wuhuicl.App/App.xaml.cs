using System.Runtime.InteropServices;
using System.Windows;

namespace Wuhuicl.App;

public partial class App : Application
{
    protected override void OnStartup(StartupEventArgs e)
    {
        // 必须在任何 HWND 建出来之前声明：从不受信的父进程（脚本 / 计划任务）拉起来时，
        // manifest 可能不被采信，这里显式钉成 system aware，保证渲染缩放取系统 DPI。
        DpiAwareness.EnsureSystemAware();
        base.OnStartup(e);
        // 未捕获异常一律落到界面提示 + 日志，不要静默闪退
        DispatcherUnhandledException += (_, args) =>
        {
            // 崩溃细节必须留下：XAML 运行时异常在弹窗里会被截断成没用的半句话
            try
            {
                var log = System.IO.Path.Combine(System.IO.Path.GetTempPath(), "wuhuicl-crash.log");
                System.IO.File.WriteAllText(log, args.Exception.ToString());
            }
            catch { }
            MessageBox.Show("出错了：" + args.Exception.Message,
                "WUHUICL", MessageBoxButton.OK, MessageBoxImage.Warning);
            args.Handled = true;
        };
        // 原版：主窗先建但不显示，开机碎块动画跑完再 reveal（startupSplash.ts 的 gate.canReveal）
        var win = new MainWindow();
        MainWindow = win;
        var splash = new Services.BootSplashWindow();
        splash.Start(() =>
        {
            win.Show();
            win.Activate();
            splash.Close();
        });
    }
}

/// <summary>
/// 进程级 DPI 感知。已经在 manifest 里声明过时 API 会返回 false（ERROR_ACCESS_DENIED），
/// 那是正常情况；从不受信的父进程拉起来时它就是唯一的兜底。
/// </summary>
static class DpiAwareness
{
    const int SystemAware = -3;

    [DllImport("user32.dll")]
    static extern bool SetProcessDpiAwarenessContext(IntPtr value);

    [DllImport("user32.dll")]
    static extern IntPtr GetProcessDpiAwarenessContext(IntPtr hwnd);

    /// <summary>返回当前进程实际生效的感知句柄，供诊断用。</summary>
    public static IntPtr Current() => GetProcessDpiAwarenessContext(IntPtr.Zero);

    public static void EnsureSystemAware()
    {
        try { SetProcessDpiAwarenessContext(new IntPtr(SystemAware)); }
        catch { /* 老系统没有这个导出，manifest 已经覆盖 */ }
    }
}
