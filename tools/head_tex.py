import io

p = r"D:\桌面\WUHUICL\wuhuicl-net\Wuhuicl.App\Controls\SkinDoll3D.cs"
s = io.open(p, encoding="utf-8").read()
# 恢复贴图材质
old = "        var mat = new DiffuseMaterial(System.Windows.Media.Brushes.Gray); // TODO 排查后换回 Upscaled(Skin)"
new = "        var mat = new DiffuseMaterial(Upscaled(Skin));"
assert s.count(old) == 1
s = s.replace(old, new)
# 只渲染头部（含帽层）便于核对六面 UV
old2 = '''        string parts = (Environment.GetEnvironmentVariable("DOLL_PARTS") ?? "all").ToLowerInvariant();'''
new2 = '''        string parts = "head"; // TODO UV 核对完恢复 all'''
assert s.count(old2) == 1
s = s.replace(old2, new2)
io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("ok")
