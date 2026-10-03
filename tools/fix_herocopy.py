import io
p = r"D:\桌面\WUHUICL\wuhuicl-net\Wuhuicl.App\MainWindow.xaml.cs"
s = io.open(p, encoding="utf-8").read()

old = '''        EmptyHint.Visibility = _s.InstalledList.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
        EmptyHintText.Text = "还没有安装任何游戏版本";'''
new = '''        EmptyHint.Visibility = _s.InstalledList.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
        EmptyHintText.Text = "尚未安装游戏实例，点击前往版本管理";'''
assert s.count(old) == 1
s = s.replace(old, new)
io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("ok")
