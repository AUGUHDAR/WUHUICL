import io
p = r"D:\桌面\WUHUICL\wuhuicl-net\Wuhuicl.App\Controls\SkinDoll3D.cs"
s = io.open(p, encoding="utf-8").read()
old = "        var mat = new DiffuseMaterial(Upscaled(Skin));"
new = "        var mat = new DiffuseMaterial(System.Windows.Media.Brushes.Gray); // TODO 排查后换回 Upscaled(Skin)"
assert s.count(old) == 1
s = s.replace(old, new)
io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("ok")
