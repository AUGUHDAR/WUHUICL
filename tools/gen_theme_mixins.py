"""补齐 6 套主题里"运行时才算出来"的派生色 token。

原版这些颜色是 App.vue 启动时用 color-mix() 现算的，XAML 里没有对应 key：
  NavActiveFg   .nav-item.active 的字色      = color-mix(text 84%, accent)        App.vue:1833
  NavPill       .nav-bubble 的底色            = 135°(accent-soft 80%→transparent → hover)  App.vue:1842
  AccentHiSoft  .loader-badge 的边框          = color-mix(accent-2 34%, transparent) HomeView.vue:831
  BarFill       .dl-bar-fill / .mem-slider-fill = 90°(accent-2 → accent)            App.vue:2281 / SettingsView.vue:1719
  ChipBg        .tag 的底                     = color-mix(text-dim 14%, transparent) styles.css:469
  LoaderBg      .loader-badge 的底            = color-mix(accent 30%, rgba(20,30,24,.46)) HomeView.vue:831
  LogBody       .log-body 的底                = color-mix(bg 42%, transparent)      HomeView.vue:978
  CyanSoft / DangerSoft / DangerBorder        = App.vue:985-990（深浅两套 alpha）

其中 accent-2 就是既有的 AccentHi（accent 72% + white），accent-deep 就是 AccentLo。
混合按 naive sRGB 通道线性插值，与既有 AccentHi/AccentLo 的生成方式保持一致。

用法：python tools/gen_theme_mixins.py          # 就地补齐/刷新
      python tools/gen_theme_mixins.py --check  # 只报告是否已一致
"""
import re
import sys
from pathlib import Path

THEMES = Path(__file__).resolve().parent.parent / "Wuhuicl.App" / "Themes"
FILES = ["Transparent", "Light", "BlackOrange", "WhitePink", "BlackPink", "Custom"]

BEGIN = "  <!-- BEGIN 运行时派生色（tools/gen_theme_mixins.py 生成，勿手改） -->"
END = "  <!-- END 运行时派生色 -->"

FG_MIX = .84        # color-mix(in srgb, text 84%, accent)        App.vue:1833
SOFT_KEEP = .80     # color-mix(in srgb, accent-soft 80%, transparent)  App.vue:1842
BORDER_MIX = .34    # color-mix(in srgb, accent-2 34%, transparent)      HomeView.vue:831

# App.vue:985-1000 的 soft/border 半透明层：深色与浅色各一套 alpha，色相就是同主题的 ok/danger/cyan
SOFT_DARK = .13     # --ok-soft / --cyan-soft / --danger-soft（深色）
SOFT_LIGHT = .10    # 同上（浅色）；--danger-soft 浅色是 .08，单列
DANGER_SOFT_LIGHT = .08
DANGER_BORDER_DARK = .34
DANGER_BORDER_LIGHT = .30
CHIP_MIX = .14      # .tag 底 = color-mix(text-dim 14%, transparent)  styles.css:469
LOADER_MIX = .30    # .loader-badge 底 = color-mix(accent 30%, rgba(20,30,24,.46))  HomeView.vue:831
LOADER_BASE = (20, 30, 24, .46)
LOG_MIX = .42       # .log-body 底 = color-mix(bg 42%, transparent)  HomeView.vue:978


def parse(s):
    """XAML 颜色是 #AARRGGBB，这里统一成 (r,g,b,a) 便于按通道混合。"""
    s = s.lstrip("#")
    if len(s) == 6:
        s = "ff" + s
    return tuple(int(s[i:i + 2], 16) for i in (2, 4, 6, 0))


def hexa(c):
    return "#{3:02X}{0:02X}{1:02X}{2:02X}".format(*[max(0, min(255, round(v))) for v in c])


def rgba(rgb, a):
    """XAML 是 #AARRGGBB。"""
    r, g, b = [max(0, min(255, round(v))) for v in rgb]
    return "#{0:02X}{1:02X}{2:02X}{3:02X}".format(max(0, min(255, round(a * 255))), r, g, b)


def mix(a, b, wa):
    return tuple(a[i] * wa + b[i] * (1 - wa) for i in range(3))


def loader_bg(accent):
    """color-mix(in srgb, accent 30%, rgba(20,30,24,.46))，按 sRGB 预乘混合。"""
    wa, base = LOADER_MIX, LOADER_BASE
    al = wa * 1 + (1 - wa) * base[3]
    rgb = [(wa * accent[i] + (1 - wa) * base[3] * base[i]) / al for i in range(3)]
    return rgba(rgb, al)


def luminance(rgb):
    return (0.299 * rgb[0] + 0.587 * rgb[1] + 0.114 * rgb[2]) / 255


def read(text, pattern, what):
    m = re.search(pattern, text)
    if not m:
        raise SystemExit(f"missing {what}")
    return m.group(1)


def build_block(text):
    bg = parse(read(text, r'<Color x:Key="C\.Bg">#(\w{6,8})</Color>', "C.Bg"))
    fg = parse(read(text, r'<Color x:Key="C\.Fg">#([0-9a-fA-F]{6})</Color>', "C.Fg"))
    fg2 = parse(read(text, r'<Color x:Key="C\.Fg2">#([0-9a-fA-F]{6})</Color>', "C.Fg2"))
    accent = parse(read(text, r'<Color x:Key="C\.Accent">#([0-9a-fA-F]{6})</Color>', "C.Accent"))
    accent_hi = parse(read(text, r'<Color x:Key="C\.AccentHi">#([0-9a-fA-F]{6})</Color>', "C.AccentHi"))
    soft = parse(read(text, r'<SolidColorBrush x:Key="AccentSoft" Color="(#\w{8})"', "AccentSoft"))
    hover = read(text, r'<SolidColorBrush x:Key="Hover" Color="(#\w{8})"', "Hover")
    ok = parse(read(text, r'<Color x:Key="C\.Ok">#(\w{6,8})</Color>', "C.Ok"))
    danger = parse(read(text, r'<Color x:Key="C\.Danger">#(\w{6,8})</Color>', "C.Danger"))
    cyan = parse(read(text, r'<Color x:Key="C\.Cyan">#(\w{6,8})</Color>', "C.Cyan"))

    # App.vue:970 的深色判据就是 bg 的相对亮度 < .46，soft 层的双套 alpha 全靠它
    dark = luminance(bg) < .46
    soft_a = SOFT_DARK if dark else SOFT_LIGHT
    danger_soft_a = SOFT_DARK if dark else DANGER_SOFT_LIGHT
    danger_border_a = DANGER_BORDER_DARK if dark else DANGER_BORDER_LIGHT

    nav_fg = "#{0:02X}{1:02X}{2:02X}".format(*[round(v) for v in mix(fg, accent, FG_MIX)])
    pill_top = hexa(soft[:3] + (soft[3] * SOFT_KEEP,))
    border = hexa(accent_hi[:3] + (255 * BORDER_MIX,))

    return (
        f"{BEGIN}\n"
        f"  <Color x:Key=\"C.NavActiveFg\">{nav_fg}</Color>\n"
        f"  <SolidColorBrush x:Key=\"NavActiveFg\" Color=\"{{StaticResource C.NavActiveFg}}\"/>\n"
        f"  <LinearGradientBrush x:Key=\"NavPill\" StartPoint=\"0,0\" EndPoint=\"1,1\">\n"
        f"    <GradientStop Offset=\"0\" Color=\"{pill_top}\"/>\n"
        f"    <GradientStop Offset=\"1\" Color=\"{hover}\"/>\n"
        f"  </LinearGradientBrush>\n"
        f"  <SolidColorBrush x:Key=\"AccentHiSoft\" Color=\"{border}\"/>\n"
        f"  <SolidColorBrush x:Key=\"OkSoft\" Color=\"{rgba(ok[:3], soft_a)}\"/>\n"
        f"  <SolidColorBrush x:Key=\"CyanSoft\" Color=\"{rgba(cyan[:3], soft_a)}\"/>\n"
        f"  <SolidColorBrush x:Key=\"DangerSoft\" Color=\"{rgba(danger[:3], danger_soft_a)}\"/>\n"
        f"  <SolidColorBrush x:Key=\"DangerBorder\" Color=\"{rgba(danger[:3], danger_border_a)}\"/>\n"
        f"  <SolidColorBrush x:Key=\"ChipBg\" Color=\"{rgba(fg2[:3], CHIP_MIX)}\"/>\n"
        f"  <SolidColorBrush x:Key=\"LoaderBg\" Color=\"{loader_bg(accent[:3])}\"/>\n"
        f"  <SolidColorBrush x:Key=\"LogBody\" Color=\"{rgba(bg[:3], LOG_MIX)}\"/>\n"
        f"  <SolidColorBrush x:Key=\"Idle\" Color=\"#FF9CA3AF\"/>\n"
        f"  <LinearGradientBrush x:Key=\"BarFill\" StartPoint=\"0,0\" EndPoint=\"1,0\">\n"
        f"    <GradientStop Offset=\"0\" Color=\"{{StaticResource C.AccentHi}}\"/>\n"
        f"    <GradientStop Offset=\"1\" Color=\"{{StaticResource C.Accent}}\"/>\n"
        f"  </LinearGradientBrush>\n"
        f"{END}\n"
    )


LEGACY = re.compile(r"  <!-- 导航选中态派生色.*?</LinearGradientBrush>\n", re.S)
# ChipBg / LoaderBg 早期是手抄的（ChipBg 抄成了 Hover 的 5~7% alpha），改由脚本按公式出
HANDWRITTEN = re.compile(r'^  <SolidColorBrush x:Key="(?:ChipBg|LoaderBg)" Color="[^"]+"/>\n', re.M)


def sync(name):
    path = THEMES / f"{name}.xaml"
    text = path.read_text(encoding="utf-8")
    # 上一版脚本按 token 逐条插过，先摘掉再统一写带 BEGIN/END 标记的块，避免重复 key
    text = LEGACY.sub("", text)
    # 只摘 BEGIN 标记之前的手抄行；块内的同名 key 属于生成结果，摘掉会让 --check 永远 stale
    head, sep, tail = text.partition(BEGIN)
    text = HANDWRITTEN.sub("", head) + sep + tail
    block = build_block(text)
    old = re.search(re.escape(BEGIN) + r".*?" + re.escape(END) + r"\n", text, re.S)
    if old and old.group(0) == block:
        return path, "ok"
    if "--check" in sys.argv:
        return path, "stale" if old else "missing"
    if old:
        text = text.replace(old.group(0), block)
    else:
        anchor = '  <SolidColorBrush x:Key="Scroll"'
        text = text[:text.index(anchor)] + block + text[text.index(anchor):]
    path.write_text(text, encoding="utf-8")
    return path, ("updated" if old else "added")


def main():
    for name in FILES:
        path, state = sync(name)
        print(f"{name:14} {state:8} {path.name}")


main()
