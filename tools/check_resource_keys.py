#!/usr/bin/env python3
"""资源键名撞车检查：主题画刷键 与 样式/图标键 不能同名。

WPF 的 StaticResource 从 Application.Resources 往回找时会先命中后合并的字典，
所以 x:Key="Card" 的画刷会盖掉 x:Key="Card" 的样式，把 Brush 赋给 Style，
在解析期抛 "设置属性 Style 时引发了异常"。这类问题只在运行时爆，编译期看不出来。

用法：python tools/check_resource_keys.py    （非 0 退出码 = 有撞车）
"""
import re
import sys
from pathlib import Path

APP = Path(__file__).resolve().parent.parent / "Wuhuicl.App"
THEME_GLOBS = ("Themes/*.xaml",)
STYLE_FILES = [APP / "Themes" / "Controls.xaml", APP / "Themes" / "Icons.xaml"]
KEY = re.compile(r'x:Key="([^"]+)"')


def page_files():
    d = APP / "Pages"
    return sorted(d.glob("*.xaml")) if d.exists() else []


def keys_of(path: Path) -> set[str]:
    return set(KEY.findall(path.read_text(encoding="utf-8")))


COMMENT = re.compile(r'<!--(.*?)-->', re.S)


def check_comments(paths) -> int:
    """XAML 注释体里不能出现连续两个减号，也不能以减号结尾（MC3000，只有编译才报）。"""
    bad = 0
    for f in paths:
        if not f.exists():
            continue
        text = f.read_text(encoding="utf-8")
        for m in COMMENT.finditer(text):
            body = m.group(1)
            if "--" in body or body.rstrip().endswith("-"):
                line = text[:m.start()].count("\n") + 1
                print(f"{f.name}:{line} 注释里有连续减号 -> {body.strip()[:60]}")
                bad += 1
    return bad


FWD = re.compile(r'\{StaticResource ([^}]+)\}')


def check_forward_refs(paths) -> int:
    """同一份字典里 StaticResource 只能向前引用（后定义的键在解析时还不存在）。

    BasedOn="{StaticResource TagText}" 写在 TagText 之前，编译能过、启动才炸：
    "在 StaticResourceHolder 上提供值时引发了异常"，整个窗口起不来。
    """
    bad = 0
    for f in paths:
        if not f.exists():
            continue
        text = COMMENT.sub('', f.read_text(encoding='utf-8'))
        defs: dict[str, int] = {}
        for m in KEY.finditer(text):
            defs.setdefault(m.group(1), text[:m.start()].count('\n') + 1)
        for m in FWD.finditer(text):
            k = m.group(1).strip()
            d = defs.get(k)
            if d is None:
                continue                      # 键来自 App 级字典，本文件管不着
            line = text[:m.start()].count('\n') + 1
            if line < d:
                print(f"{f.name}:{line} 向前引用 {k}（本文件第 {d} 行才定义）")
                bad += 1
    return bad


def main() -> int:
    theme: set[str] = set()
    for pattern in THEME_GLOBS:
        for f in sorted(APP.glob(pattern)):
            if f.name in {"Controls.xaml", "Icons.xaml"}:
                continue
            theme |= keys_of(f)

    bad = 0
    for f in STYLE_FILES:
        if not f.exists():
            print(f"missing {f}")
            bad += 1
            continue
        hit = sorted(theme & keys_of(f))
        if hit:
            print(f"{f.name}: 与主题键撞车 -> {', '.join(hit)}")
            bad += 1

    both = sorted(keys_of(STYLE_FILES[0]) & keys_of(STYLE_FILES[1])) \
        if all(f.exists() for f in STYLE_FILES) else []
    if both:
        print(f"Controls/Icons 互相撞车 -> {', '.join(both)}")
        bad += 1

    # 页面里的局部样式同样不能与主题画刷同名（Card 那次事故就是这条）
    for f in page_files():
        hit = sorted(theme & keys_of(f))
        if hit:
            print(f"Pages/{f.name}: 与主题键撞车 -> {', '.join(hit)}")
            bad += 1

    bad += check_comments([*STYLE_FILES, *page_files(), APP / "MainWindow.xaml", APP / "App.xaml"])
    bad += check_forward_refs([*STYLE_FILES, *page_files(), APP / "MainWindow.xaml", APP / "App.xaml"])

    if bad == 0:
        print(f"OK 主题 {len(theme)} 键，样式/图标/{len(page_files())} 个页面无撞车，注释与引用顺序合法")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
