import io
p = r"D:\桌面\WUHUICL\wuhuicl-net\Wuhuicl.App\App.xaml.cs"
s = io.open(p, encoding="utf-8").read()
old = '''    host.Loaded += (_, _) =>
    {'''
new = '''    host.Loaded += (_, _) =>
    {
        // 参考方块：无纹理纯红，判定 3D 管线本身是否工作
        var cube = new System.Windows.Media.Media3D.ModelVisual3D
        {
            Content = new System.Windows.Media.Media3D.GeometryModel3D
            {
                Geometry = new System.Windows.Media.Media3D.MeshGeometry3D
                {
                    Positions = new System.Windows.Media.Media3D.Point3DCollection
                    {
                        new(-5, 30, 5), new(5, 30, 5), new(5, 40, 5), new(-5, 40, 5),
                        new(5, 30, -5), new(-5, 30, -5), new(-5, 40, -5), new(5, 40, -5),
                    },
                    TriangleIndices = new System.Windows.Media.Media3D.Int32Collection
                        { 0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7 },
                },
                Material = System.Windows.Media.Media3D.DiffuseMaterial.FromColor(System.Windows.Media.Colors.Red),
            },
        };
        vp.Children.Add(cube);'''
assert s.count(old) == 1
s = s.replace(old, new)
io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("ok")
