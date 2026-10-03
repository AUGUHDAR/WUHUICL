import io
p = r"D:\桌面\WUHUICL\wuhuicl-net\Wuhuicl.App\Controls\SkinDoll3D.cs"
s = io.open(p, encoding="utf-8").read()
old = '''        var mat = new DiffuseMaterial(new ImageBrush(Skin)); // TODO 排查：原始 64 皮肤直贴'''
new = '''        var mat = new DiffuseMaterial(Upscaled(Skin));'''
assert s.count(old) == 1
s = s.replace(old, new)
old2 = '''        return new ImageBrush(wb)
        {
            ViewportUnits = BrushMappingMode.RelativeToBoundingBox,
            Viewport = new Rect(0, 0, 1, 1),
            Stretch = Stretch.Fill,
        };'''
new2 = '''        // 3D 材质里 ViewportUnits 必须是 Absolute：RelativeToBoundingBox 的"包围盒"
        // 是每个被绘制面的盒子，整张图会被铺到每一个面上，TextureCoordinates 形同虚设
        return new ImageBrush(wb)
        {
            ViewportUnits = BrushMappingMode.Absolute,
            Viewport = new Rect(0, 0, 1, 1),
            Stretch = Stretch.Fill,
        };'''
assert s.count(old2) == 1
s = s.replace(old2, new2)
io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("ok")
