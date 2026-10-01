using System.Windows;

namespace Wuhuicl.App;

public partial class App : Application
{
    protected override void OnStartup(StartupEventArgs e)
    {
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
        var win = new MainWindow();
        MainWindow = win;
        win.Show();
    }
}
