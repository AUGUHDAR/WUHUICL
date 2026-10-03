import io
p = r"D:\桌面\WUHUICL\wuhuicl-net\Wuhuicl.App\Controls\SkinDoll3D.cs"
s = io.open(p, encoding="utf-8").read()
old = '''        string parts = "head"; // TODO UV 核对完恢复 all'''
new = '''        string parts = (Environment.GetEnvironmentVariable("DOLL_PARTS") ?? "all").ToLowerInvariant();'''
assert s.count(old) == 1
s = s.replace(old, new)
io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("ok")
