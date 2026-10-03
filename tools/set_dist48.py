import io
p = r"D:\桌面\WUHUICL\wuhuicl-net\Wuhuicl.App\App.xaml.cs"
s = io.open(p, encoding="utf-8").read()
old = "    Controls.SkinDoll3D.DebugDistance = 150;"
new = "    Controls.SkinDoll3D.DebugDistance = 48;"
assert s.count(old) == 1
s = s.replace(old, new)
io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("ok")
