import io
p = r"D:\桌面\WUHUICL\wuhuicl-net\Wuhuicl.App\MainWindow.xaml.cs"
s = io.open(p, encoding="utf-8").read()
old = '''    async void OnLoaded(object sender, RoutedEventArgs e)
    {
        BindSettings();'''
new = '''    async void OnLoaded(object sender, RoutedEventArgs e)
    {
        // XAML 里 NavHome IsChecked=True 触发的 Checked 事件在解析期 PageHome 还没赋值而提前返回，
        // 顶栏按钮显隐（App.vue:1333/1359）要等这里补一次 Show 才生效
        Show("home");
        BindSettings();'''
assert s.count(old) == 1
s = s.replace(old, new)
io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("ok")
