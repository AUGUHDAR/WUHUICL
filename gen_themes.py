# 按原版 token 与派生规则生成 6 套 WPF 主题字典。
# 派生规则来自 KAMUCL-1.1.5：dark = luminance(bg) < 0.46；
# accent-2 = 深:72%+白 / 浅:84%+黑；accent-deep = 78%+黑；accent-soft = accent 14%；
# on-accent = 亮度>0.6 -> #1a1208 否则 #ffffff；card 深 88% / 浅 94%；
# bg-2 = sidebarBg 黑紫 88% / 深 26% / 浅 38%；border-strong = border 65% + text 35%；
# scroll = textDim 32%；hover = text 深 7% / 浅 5%；mask/danger/ok/cyan 按深浅两套。
import os

OUT = os.path.join(os.path.dirname(__file__), "Wuhuicl.App", "Themes")


def hx(c):
    c = c.lstrip("#")
    return tuple(int(c[i:i + 2], 16) for i in (0, 2, 4))


def to(a):
    return "#%02x%02x%02x" % a


def mix(a, b, p):
    """color-mix(in srgb, a (1-p), b p) —— p 是第二个参数的权重。"""
    A, B = hx(a), hx(b)
    return to(tuple(round(A[i] * (1 - p) + B[i] * p) for i in range(3)))


def alpha(c, pct):
    return "#%02X%02X%02X%02X" % ((round(pct * 255),) + hx(c))


def lum(c):
    r, g, b = [v / 255 for v in hx(c)]
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


W, K = "#ffffff", "#000000"

# 9 个源 token，逐字取自 KAMUCL src/shared/types.ts:236-336
THEMES = {
    "transparent": dict(name="默认·黑紫", file="Transparent", accent="#9475ed", bg="#212121",
                        card="#292929", text="#f5f5f5", dim="#b4b4b4", border="#414141",
                        sb="#171717", sbt="#c7c7c7", sb_a=0.88, shell_a=0.96),
    "blue-white": dict(name="白蓝", file="Light", accent="#2563eb", bg="#edf0f7", card="#ffffff",
                       text="#1b2437", dim="#68718a", border="#d8dfec", sb="#f5f7fb",
                       sbt="#536078", sb_a=0.38, shell_a=0.42),
    "black-orange": dict(name="橙黑", file="BlackOrange", accent="#f97316", bg="#0b0b0e", card="#18181f",
                         text="#f4f2ee", dim="#aaa69f", border="#35353f", sb="#111116",
                         sbt="#aaa69f", sb_a=0.26, shell_a=0.30),
    "white-pink": dict(name="粉白", file="WhitePink", accent="#ec4899", bg="#fdf2f8", card="#ffffff",
                       text="#4a1d35", dim="#a06b8a", border="#fbcfe8", sb="#fce7f3",
                       sbt="#4a1d35", sb_a=0.38, shell_a=0.42),
    "black-pink": dict(name="粉黑", file="BlackPink", accent="#f472b6", bg="#171019", card="#211623",
                       text="#f5e8f2", dim="#a68ba3", border="#3d2740", sb="#1c1220",
                       sbt="#a68ba3", sb_a=0.26, shell_a=0.30),
}
THEMES["custom"] = dict(THEMES["blue-white"])
THEMES["custom"].update(name="个性化", file="Custom")

TPL = """<ResourceDictionary xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
                    xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml">
  <!-- 主题「{name}」({tid})。源 token 逐字取自 KAMUCL types.ts，派生按原版公式计算。 -->
  <Color x:Key="C.Bg">{bg}</Color>
  <Color x:Key="C.Nav">{nav}</Color>
  <Color x:Key="C.Card">{card}</Color>
  <Color x:Key="C.CardSoft">{card2}</Color>
  <Color x:Key="C.Line">{line}</Color>
  <Color x:Key="C.Fg">{fg}</Color>
  <Color x:Key="C.Fg2">{fg2}</Color>
  <Color x:Key="C.Fg3">{fg3}</Color>
  <Color x:Key="C.Accent">{accent}</Color>
  <Color x:Key="C.AccentHi">{accent2}</Color>
  <Color x:Key="C.AccentLo">{accentdeep}</Color>
  <Color x:Key="C.AccentFg">{onaccent}</Color>
  <Color x:Key="C.Ok">{ok}</Color>
  <Color x:Key="C.Warn">{warn}</Color>
  <Color x:Key="C.Danger">{danger}</Color>
  <Color x:Key="C.Cyan">{cyan}</Color>

  <SolidColorBrush x:Key="Bg" Color="{{StaticResource C.Bg}}"/>
  <SolidColorBrush x:Key="NavBg" Color="{{StaticResource C.Nav}}"/>
  <SolidColorBrush x:Key="Card" Color="{{StaticResource C.Card}}"/>
  <SolidColorBrush x:Key="CardSoft" Color="{{StaticResource C.CardSoft}}"/>
  <SolidColorBrush x:Key="Line" Color="{{StaticResource C.Line}}"/>
  <SolidColorBrush x:Key="Fg" Color="{{StaticResource C.Fg}}"/>
  <SolidColorBrush x:Key="Fg2" Color="{{StaticResource C.Fg2}}"/>
  <SolidColorBrush x:Key="Fg3" Color="{{StaticResource C.Fg3}}"/>
  <SolidColorBrush x:Key="Accent" Color="{{StaticResource C.Accent}}"/>
  <SolidColorBrush x:Key="AccentHi" Color="{{StaticResource C.AccentHi}}"/>
  <SolidColorBrush x:Key="AccentLo" Color="{{StaticResource C.AccentLo}}"/>
  <SolidColorBrush x:Key="AccentFg" Color="{{StaticResource C.AccentFg}}"/>
  <SolidColorBrush x:Key="Ok" Color="{{StaticResource C.Ok}}"/>
  <SolidColorBrush x:Key="Warn" Color="{{StaticResource C.Warn}}"/>
  <SolidColorBrush x:Key="Danger" Color="{{StaticResource C.Danger}}"/>
  <SolidColorBrush x:Key="Cyan" Color="{{StaticResource C.Cyan}}"/>
  <SolidColorBrush x:Key="BorderStrong" Color="{bstrong}"/>
  <SolidColorBrush x:Key="AccentSoft" Color="{accentsoft}"/>
  <SolidColorBrush x:Key="Hover" Color="{hover}"/>
  <SolidColorBrush x:Key="ChipBg" Color="{hover}"/>
  <SolidColorBrush x:Key="HeroChipBg" Color="#7A090D0E"/>
  <SolidColorBrush x:Key="LoaderBg" Color="{loadersoft}"/>
  <SolidColorBrush x:Key="LoaderFg" Color="#FFF6FFF8"/>
  <SolidColorBrush x:Key="Overlay" Color="{mask}"/>
  <SolidColorBrush x:Key="Scroll" Color="{scroll}"/>

  <LinearGradientBrush x:Key="WindowGrad" StartPoint="0,0" EndPoint="0.35,1">
    <GradientStop Offset="0" Color="{grad0}"/>
    <GradientStop Offset="1" Color="{grad1}"/>
  </LinearGradientBrush>
  <LinearGradientBrush x:Key="AccentGrad" StartPoint="0,0" EndPoint="1,1">
    <GradientStop Offset="0" Color="{{StaticResource C.AccentHi}}"/>
    <GradientStop Offset="0.55" Color="{{StaticResource C.Accent}}"/>
    <GradientStop Offset="1" Color="{{StaticResource C.AccentLo}}"/>
  </LinearGradientBrush>

  <DropShadowEffect x:Key="CardShadow" BlurRadius="18" ShadowDepth="2" Direction="270"
                    Opacity="{sh}" Color="{shcolor}"/>
  <DropShadowEffect x:Key="PopShadow" BlurRadius="55" ShadowDepth="20" Direction="270"
                    Opacity="{shl}" Color="{shcolor}"/>
</ResourceDictionary>
"""


def build(tid, d):
    dark = lum(d["bg"]) < 0.46
    accent, card, text, dim, border, bg = (d["accent"], d["card"], d["text"],
                                           d["dim"], d["border"], d["bg"])
    return TPL.format(
        name=d["name"], tid=tid,
        bg=mix(bg, card, 0.30 if dark else 0.0),
        nav=mix(bg, d["sb"], d["sb_a"]),
        card=mix(bg, card, 0.90 if dark else 0.97),
        card2=mix(bg, card, 0.78 if dark else 0.90),
        line=mix(bg, border, 0.95),
        fg=text, fg2=dim, fg3=mix(bg, dim, 0.60),
        accent=accent,
        accent2=mix(accent, W, 0.28) if dark else mix(accent, K, 0.16),
        accentdeep=mix(accent, K, 0.22),
        onaccent="#1a1208" if lum(accent) > 0.6 else "#ffffff",
        ok="#68dc88" if dark else "#168a42",
        warn="#f0b55a" if dark else "#b4650a",
        danger="#ff6b73" if dark else "#dc2626",
        cyan="#9ed7e9" if dark else "#0e7490",
        bstrong=mix(border, text, 0.35),
        accentsoft=alpha(accent, 0.14),
        hover=alpha(text, 0.07 if dark else 0.05),
        loadersoft=alpha(accent, 0.30),
        mask="#B8020506" if dark else "#6B1B2437",
        scroll=alpha(dim, 0.32),
        grad0=mix(bg, card, 0.45),
        grad1=mix(bg, accent, 0.06),
        sh="0.16" if dark else "0.05",
        shl="0.42" if dark else "0.18",
        shcolor="#000000" if dark else "#1f3255",
    )


for tid, d in THEMES.items():
    with open(os.path.join(OUT, d["file"] + ".xaml"), "w", encoding="utf-8") as f:
        f.write(build(tid, d))
    print("生成", d["file"] + ".xaml", "深色" if lum(d["bg"]) < 0.46 else "浅色")
