#!/usr/bin/env python3
"""把 MainWindow.xaml 里的页面块拆成独立 UserControl。

标签定位用字符级 tokenizer：XAML 的开标签经常把属性换行写，
按行正则匹配会漏掉开标签，导致深度算错、切早。
"""
import re
import sys
from pathlib import Path

APP = Path(__file__).resolve().parent.parent / "Wuhuicl.App"
MAIN = APP / "MainWindow.xaml"
PAGES = APP / "Pages"

TARGETS = {
    "PageHome": "HomePage",
    "PageVersions": "VersionsPage",
    "PageSettings": "SettingsPage",
}

HEADER = """<UserControl x:Class="Wuhuicl.App.Pages.{cls}"
               xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
               xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml">
{body}
</UserControl>
"""

CODE = """using System.Windows.Controls;

namespace Wuhuicl.App.Pages;

public partial class {cls} : UserControl
{{
    public {cls}() => InitializeComponent();
}}
"""


def tokens(text):
    """产出 (kind, tag, raw, start, end)，kind in {open, close, self}。"""
    i, n = 0, len(text)
    while i < n:
        lt = text.find("<", i)
        if lt < 0:
            return
        # 注释 / CDATA / 声明整体跳过
        if text.startswith("<!--", lt):
            j = text.find("-->", lt)
            i = (j + 3) if j >= 0 else n
            continue
        if text.startswith("<!", lt) or text.startswith("<?", lt):
            j = text.find(">", lt)
            i = (j + 1) if j >= 0 else n
            continue
        # 找到标签结束处：> 且不在引号内
        j, quote = lt + 1, None
        while j < n:
            c = text[j]
            if quote:
                if c == quote:
                    quote = None
            elif c in "\"'":
                quote = c
            elif c == ">":
                break
            j += 1
        raw = text[lt:j + 1]
        body = raw[1:-1].strip()
        if body.startswith("/"):
            kind, tag = "close", body[1:].strip()
        elif raw.rstrip().endswith("/>"):
            kind, tag = "self", body.split()[0]
        else:
            kind, tag = "open", body.split()[0]
        yield kind, tag, raw, lt, j + 1
        i = j + 1


def find_element(text, name):
    """返回 (起, 止) 字符偏移，覆盖整个元素且含其前后缩进空白。"""
    stack = []
    for kind, tag, raw, s, e in tokens(text):
        if kind == "open":
            stack.append((s, e, raw))
        elif kind == "close":
            if not stack:
                continue
            os_, _, oraw = stack.pop()
            # 目标元素可能嵌在好几层网格里，判据是"刚弹出的这个元素"带这个名字
            if f'x:Name="{name}"' in oraw:
                return text.rfind("\n", 0, os_) + 1, e
        elif kind == "self" and f'x:Name="{name}"' in raw:
            return text.rfind("\n", 0, s) + 1, e
    raise SystemExit(f"未找到元素 {name}")


def line_of(text, off):
    return text.count("\n", 0, off)


def main():
    PAGES.mkdir(exist_ok=True)
    text = MAIN.read_text(encoding="utf-8")
    lines = text.split("\n")
    for name, cls in TARGETS.items():
        s, e = find_element(text, name)
        a, b = line_of(text, s), line_of(text, e)
        body = lines[a:b + 1]
        head = body[0]
        head = re.sub(r'\s*x:Name="%s"' % name, "", head)
        head = re.sub(r'\s*Visibility="[^"]*"', "", head, count=1)
        body[0] = head
        # 整块缩进减 4：原来嵌在内容网格里，现在自己是根
        body = [l[4:] if l.startswith("    ") else l for l in body]
        (PAGES / f"{cls}.xaml").write_text(
            HEADER.format(cls=cls, body="\n".join(body)), encoding="utf-8")
        (PAGES / f"{cls}.xaml.cs").write_text(CODE.format(cls=cls), encoding="utf-8")
        print(f"{name} -> Pages/{cls}.xaml  行 {a + 1}..{b + 1}（{b - a + 1} 行）")
    return 0


if __name__ == "__main__":
    sys.exit(main())
