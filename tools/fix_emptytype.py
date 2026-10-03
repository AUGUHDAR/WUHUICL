import io
p = r"D:\桌面\WUHUICL\wuhuicl-net\Wuhuicl.App\MainWindow.xaml.cs"
s = io.open(p, encoding="utf-8").read()
old = "    public System.Windows.Controls.Border EmptyHint => PageHome.EmptyHint;"
new = "    public System.Windows.Controls.Grid EmptyHint => PageHome.EmptyHint;"
assert s.count(old) == 1
s = s.replace(old, new)
io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("ok")
