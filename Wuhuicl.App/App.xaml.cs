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
        // 渲染管线选型（2026-10-04 实测，核显机器私内存）：硬件 410MB+（驱动分配，任务管理器可见）
        // vs 软件全开 114MB。用户红线 ≤300MB → 全进程 SoftwareOnly。
        // 娃娃已改为烘焙转台帧（SkinDollTurn），运行时零 3D；动画多为小脏区，软件渲染够用。
        System.Windows.Media.RenderOptions.ProcessRenderMode = System.Windows.Interop.RenderMode.SoftwareOnly;
        // 烘焙模式（开发期一次）：--bake-doll=<目录> 把 3D 娃娃转台帧渲染成 PNG 后退出。
        // 运行时 SkinDollTurn 翻这些帧实现拖拽旋转——零 3D 管线（核显驱动 ~280MB 不可回收）。
        foreach (var a in e.Args)
        {
            if (!a.StartsWith("--bake-doll=", StringComparison.OrdinalIgnoreCase)) continue;
            BakeDolls(a[12..]);
            Shutdown();
            return;
        }
        // 原版：主窗先建但不显示，开机碎块动画跑完再 reveal（startupSplash.ts 的 gate.canReveal）
        var win = new MainWindow();
        MainWindow = win;
        if (Environment.GetEnvironmentVariable("WUHUICL_NOSPLASH") == "1")
        {
            // 内存二分开关：跳过碎玻璃开机动画，主窗直接显
            win.Show();
            win.Activate();
        }
        else
        {
            var splash = new Services.BootSplashWindow();
            splash.Start(() =>
            {
                win.Show();
                win.Activate();
                splash.Close();
            });
        }
    }

    /// <summary>开发期烘焙：3D 娃娃转台帧。3 档俯仰 × 24 转向 + 8 帧行走，
    /// 镜头拉近（zoom 1.7）让娃娃撑满画面——之前娃娃只占帧的 ~40%，卡里显得特别小。</summary>
    static void BakeDolls(string outDir)
    {
        System.IO.Directory.CreateDirectory(outDir);
        var doll = new Controls.SkinDoll3D { Width = 560, Height = 1300, Slim = false };
        doll.Skin = Services.SkinRenderer.SkinSource();
        doll.Measure(new Size(560, 1300));
        doll.Arrange(new Rect(0, 0, 560, 1300));
        double[] pitches = { -0.6, -0.3, 0, 0.3, 0.6 };   // 仰视 / 平视 / 俯视 五档
        for (int row = 0; row < pitches.Length; row++)
            for (int i = 0; i < 24; i++)
            {
                double yaw = -0.6 + i * Math.PI * 2 / 24;
                doll.SetWalkPose(false, 0);
                doll.SetPose(yaw, pitches[row], 1.7);
                SaveFrame(doll, System.IO.Path.Combine(outDir, $"doll_r{row}_{i:00}.png"));
            }
        for (int i = 0; i < 8; i++)
        {
            doll.SetWalkPose(true, i * Math.PI / 4);
            doll.SetPose(-0.6, 0, 1.7);
            SaveFrame(doll, System.IO.Path.Combine(outDir, $"walk_{i}.png"));
        }
        MessageBox.Show("烘焙完成：" + outDir, "WUHUICL");
    }

    static void SaveFrame(Controls.SkinDoll3D doll, string path)
    {
        var rtb = new System.Windows.Media.Imaging.RenderTargetBitmap(560, 1300, 96, 96, System.Windows.Media.PixelFormats.Pbgra32);
        rtb.Render(doll);
        var enc = new System.Windows.Media.Imaging.PngBitmapEncoder();
        enc.Frames.Add(System.Windows.Media.Imaging.BitmapFrame.Create(rtb));
        using var fs = System.IO.File.Create(path);
        enc.Save(fs);
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
