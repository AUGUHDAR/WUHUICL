#!/usr/bin/env python3
"""把已抽取好的 Pages/*.xaml 接回 MainWindow：
  1) MainWindow.xaml 里的页面块换成 <pages:XxxPage/> 宿主；
  2) 每个页面代码后置生成"薄委托"，把事件转给 Shell（MainWindow）；
  3) MainWindow.xaml.cs 追加一行式元素转发属性，避免改动任何既有处理函数体。

设计约束：逻辑只有一份（仍在 MainWindow.xaml.cs / LaunchSession），页面只是视图。
"""
import re
import sys
from pathlib import Path

APP = Path(__file__).resolve().parent.parent / "Wuhuicl.App"
MAIN = APP / "MainWindow.xaml"
MAIN_CS = APP / "MainWindow.xaml.cs"
PAGES = APP / "Pages"

HOSTS = {"PageHome": "HomePage", "PageVersions": "VersionsPage", "PageSettings": "SettingsPage"}

EVENT_ARGS = {
    "Click": "System.Windows.RoutedEventArgs",
    "Checked": "System.Windows.RoutedEventArgs",
    "Unchecked": "System.Windows.RoutedEventArgs",
    "LostFocus": "System.Windows.RoutedEventArgs",
    "MouseDoubleClick": "System.Windows.Input.MouseButtonEventArgs",
    "MouseLeftButtonDown": "System.Windows.Input.MouseButtonEventArgs",
    "MouseLeftButtonUp": "System.Windows.Input.MouseButtonEventArgs",
    "MouseDown": "System.Windows.Input.MouseButtonEventArgs",
    "SelectionChanged": "System.Windows.Controls.SelectionChangedEventArgs",
    "SizeChanged": "System.Windows.SizeChangedEventArgs",
    "DragOver": "System.Windows.DragEventArgs",
    "Drop": "System.Windows.DragEventArgs",
}
BOUND = re.compile(r'\b(Click|Checked|Unchecked|LostFocus|MouseDoubleClick|SelectionChanged|MouseDown|MouseLeftButtonDown|MouseLeftButtonUp|SizeChanged|DragOver|Drop)="(\w+)"')


def page_element_types(cls):
    """从页面 XAML 里读 x:Name 元素的类型，决定转发属性的类型。"""
    text = (PAGES / f"{cls}.xaml").read_text(encoding="utf-8")
    out = {}
    for m in re.finditer(r'<(\w+)([^>]*?x:Name="(\w+)")', text, re.S):
        out[m.group(3)] = m.group(1)
    return out


def main():
    text = MAIN.read_text(encoding="utf-8")
    if "xmlns:pages" not in text:
        text = text.replace('xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"',
                            'xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"\n        xmlns:pages="clr-namespace:Wuhuicl.App.Pages"', 1)

    # 1) 换宿主：页面块已经在 split_pages.py 里核对过边界，这里按同名标记整块替换
    import split_pages as sp
    for name, cls in HOSTS.items():
        s, e = sp.find_element(text, name)
        a, b = sp.line_of(text, s), sp.line_of(text, e)
        indent = len(text.split("\n")[a]) - len(text.split("\n")[a].lstrip())
        lines = text.split("\n")
        # 页面根的显隐由宿主管；拆的时候剥掉了，这里必须补回 Collapsed，
            # 否则所有页面会在启动瞬间叠在一起画。
            host = " " * indent + f'<pages:{cls} x:Name="{name}"' + ("" if name == "PageHome" else ' Visibility="Collapsed"') + "/>"
            lines[a:b + 1] = [host]
        text = "\n".join(lines)
        print(f"MainWindow.xaml: {name} -> <pages:{cls}/>")
    MAIN.write_text(text, encoding="utf-8")

    # 2) 页面薄委托：参数类型按事件本身定，并且要和 Shell 里真实的签名对齐
    shell_cs = MAIN_CS.read_text(encoding="utf-8")
    shell_args = {m.group(1): m.group(2)
                  for m in re.finditer(r'\b(?:internal |public )?(?:async )?void (\w+)\(object sender, ([\w.]+) e\)', shell_cs)}
    used = set()
    for name, cls in HOSTS.items():
        xaml = (PAGES / f"{cls}.xaml").read_text(encoding="utf-8")
        pairs = {}
        for ev, handler in BOUND.findall(xaml):
            pairs.setdefault(handler, set()).add(EVENT_ARGS[ev])
        body = []
        for h, argts in sorted(pairs.items()):
            used.add(h)
            want = shell_args.get(h, "System.Windows.RoutedEventArgs")
            # Shell 里写的是简名（有 using），这里比简名，否则全部判成不兼容
            for argt in sorted(argts):
                if argt.rsplit(".", 1)[-1] == want.rsplit(".", 1)[-1]:
                    body.append(f"    void {h}(object sender, {argt} e) => Shell.{h}(sender, e);")
                elif want.endswith("RoutedEventArgs"):
                    # 同一个函数既挂 Click 又挂 MouseDoubleClick 时，Shell 只认 RoutedEventArgs
                    body.append(f"    void {h}(object sender, {argt} e) => Shell.{h}(sender, new System.Windows.RoutedEventArgs());")
                else:
                    print(f"  ! {cls}: {h} 事件参数 {argt} 与 Shell 签名 {want} 不兼容，需手工处理")
        code = (
            "using System.Windows.Controls;\n\n"
            "namespace Wuhuicl.App.Pages;\n\n"
            f"public partial class {cls} : UserControl\n"
            "{\n"
            "    MainWindow Shell => (MainWindow)System.Windows.Application.Current.MainWindow!;\n\n"
            "    public " + cls + "() => InitializeComponent();\n\n"
            "    // 事件一律转回 Shell：逻辑只有一份，页面不碰状态。\n"
            + "\n".join(body) + "\n}\n"
        )
        (PAGES / f"{cls}.xaml.cs").write_text(code, encoding="utf-8")
        print(f"Pages/{cls}.xaml.cs: {len(pairs)} 个委托")

    # 3) MainWindow 元素转发属性
    cs = MAIN_CS.read_text(encoding="utf-8")
    mark = "    // ---------------- 页面元素转发 ----------------"
    if mark in cs:
        cs = cs[:cs.index(mark)]
    else:
        cs = cs.rstrip().rstrip("}")
    lines = [mark,
             "    // 页面拆成 UserControl 后，既有处理函数体不动，靠这些一行式属性取元素。"]
    for name, cls in HOSTS.items():
        for elem, tag in sorted(page_element_types(cls).items()):
            lines.append(f"    public {sp_wpf_type(tag)} {elem} => {name}.{elem};")
    cs = cs.rstrip() + "\n\n" + "\n".join(lines) + "\n}\n"

    # 4) 被页面调用的处理函数要能被访问到
    for h in sorted(used):
        cs = re.sub(r'(?m)^(\s*)async void ' + h + r'\(', r'\1internal async void ' + h + "(", cs)
        cs = re.sub(r'(?m)^(\s*)void ' + h + r'\(', r'\1internal void ' + h + "(", cs)
    MAIN_CS.write_text(cs, encoding="utf-8")
    print("MainWindow.xaml.cs: 追加转发属性 + 放开 " + str(len(used)) + " 个处理函数")
    return 0


def sp_wpf_type(tag):
    table = {
        "Button": "System.Windows.Controls.Button",
        "TextBox": "System.Windows.Controls.TextBox",
        "CheckBox": "System.Windows.Controls.CheckBox",
        "TextBlock": "System.Windows.Controls.TextBlock",
        "Border": "System.Windows.Controls.Border",
        "ListBox": "System.Windows.Controls.ListBox",
        "RadioButton": "System.Windows.Controls.RadioButton",
        "WrapPanel": "System.Windows.Controls.WrapPanel",
        "Image": "System.Windows.Controls.Image",
        "Grid": "System.Windows.Controls.Grid",
        "ProgressBar": "System.Windows.Controls.ProgressBar",
        "ContentControl": "System.Windows.Controls.ContentControl",
        "ScrollViewer": "System.Windows.Controls.ScrollViewer",
        "TranslateTransform": "System.Windows.Media.TranslateTransform",
        "StackPanel": "System.Windows.Controls.StackPanel",
        "UniformGrid": "System.Windows.Controls.Primitives.UniformGrid",
        "Ellipse": "System.Windows.Shapes.Ellipse",
        "ComboBox": "System.Windows.Controls.ComboBox",
        "Slider": "System.Windows.Controls.Slider",
        "TextBlock": "System.Windows.Controls.TextBlock",
    }
    return table.get(tag, "System.Windows.FrameworkElement")


if __name__ == "__main__":
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    sys.exit(main())
