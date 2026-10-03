import io
p = r"D:\桌面\WUHUICL\wuhuicl-net\Wuhuicl.App\App.xaml.cs"
s = io.open(p, encoding="utf-8").read()

# 移除 dolltest 的参考方块、新相机、双帧渲染与诊断，仅保留最小渲染路径
start = s.find("    void RunDollTest()")
end = s.find("\n}", s.find("host.Show();", start)) + 2
new_block = '''
    void RunDollTest()
    {
        var vp = new Controls.SkinDoll3D
        {
            Width = 400,
            Height = 500,
            Skin = LoadDiskSkin(),
        };
        var host = new Window
        {
            Content = vp, Width = 400, Height = 500, Left = 0, Top = 0,
            ShowActivated = true, Topmost = true,
        };
        host.Loaded += (_, _) =>
        {
            host.UpdateLayout();
            var rtb = new System.Windows.Media.Imaging.RenderTargetBitmap(800, 1000, 192, 192, System.Windows.Media.PixelFormats.Pbgra32);
            rtb.Render(vp);
            var enc = new System.Windows.Media.Imaging.PngBitmapEncoder();
            enc.Frames.Add(System.Windows.Media.Imaging.BitmapFrame.Create(rtb));
            using var fs = System.IO.File.Create(@"D:\\tmp\\doll_test.png");
            enc.Save(fs);
            Shutdown(0);
        };
        host.Show();
    }
'''
s = s[:start] + new_block.rstrip() + s[end:]

# 去掉 OnStartup 里的 args 调试行
marker = '''        System.IO.File.WriteAllText(@"D:\\tmp\\doll_args.txt", string.Join("|", e.Args) + " count=" + e.Args.Length);
'''
s = s.replace(marker, "")
io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("ok")
