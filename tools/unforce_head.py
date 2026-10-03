import io
p = r"D:\桌面\WUHUICL\wuhuicl-net\Wuhuicl.App\Controls\SkinDoll3D.cs"
s = io.open(p, encoding="utf-8").read()
old = '''        parts = "head"; // TODO 排查完 UV 后移除
'''
assert s.count(old) == 1
s = s.replace(old, "")
io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("ok")
