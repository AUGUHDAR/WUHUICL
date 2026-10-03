import io
p = r"D:\桌面\WUHUICL\wuhuicl-net\Wuhuicl.App\App.xaml.cs"
s = io.open(p, encoding="utf-8").read()
old = '''        vp.Children.Add(cube);'''
new = '''        vp.Children.Add(cube);
        // 二分排查：直接挂全新相机，绕开 SkinDoll3D 的 _cam/ApplyFrame
        vp.Camera = new System.Windows.Media.Media3D.PerspectiveCamera(
            new System.Windows.Media.Media3D.Point3D(0, 16, 48),
            new System.Windows.Media.Media3D.Vector3D(0, 0, -1),
            new System.Windows.Media.Media3D.Vector3D(0, 1, 0), 45);'''
assert s.count(old) == 1
s = s.replace(old, new)
io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("ok")
