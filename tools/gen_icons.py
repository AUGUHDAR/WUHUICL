#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
gen_icons.py — generate Wuhuicl.App/Themes/Icons.xaml from KAMUCL's inline SVG icons.

The SVG markup inside the (READ-ONLY) KAMUCL source tree is the authoritative input:
this script parses it and re-emits WPF Path geometry. No path data is transcribed by hand.

Usage:
    python tools/gen_icons.py            # regenerate Themes/Icons.xaml + self validate
    python tools/gen_icons.py --stdout   # print to stdout, do not write the file

Conversion rules implemented here (see docs/UI_ICONS.md for the icon catalogue):
  * every <svg> child element -> exactly one <Path>
  * <path d>     : tokenised, relative commands expanded to absolute, re-emitted uppercase
  * <circle>     : M cx-r,cy A r,r 0 0 1 cx+r,cy A r,r 0 0 1 cx-r,cy Z
  * <ellipse>    : same pattern with rx/ry
  * <rect>       : rounded-rect path (radius clamped to min(rx, w/2, h/2)), plain otherwise
  * <line>       : M x1,y1 L x2,y2
  * <polyline>   : M ... L ...   (not closed)
  * <polygon>    : M ... L ... Z
  * root <svg> presentation attrs propagate to children that set neither
  * resolved fill=currentColor / stroke=none -> Style IconFill, otherwise Style IconStroke
  * literal hex fills (grass block, Microsoft logo) keep the hex as a local Fill value
  * StrokeThickness is emitted only when the source stroke-width differs from 2
"""

import os
import re
import sys
import xml.etree.ElementTree as ET

SRC = os.environ.get("KAMUCL_SRC", r"D:/桌面/WUHUICL/KAMUCL-1.1.10/src/renderer/src")
OUT = r"D:/桌面/WUHUICL/wuhuicl-net/Wuhuicl.App/Themes/Icons.xaml"

NS_D = "http://schemas.microsoft.com/winfx/2006/xaml/presentation"
NS_X = "http://schemas.microsoft.com/winfx/2006/xaml"

# --------------------------------------------------------------------------------------
# icon catalogue: style key -> (source file, <svg> start line, locator signature)
# the line anchors come from docs/UI_ICONS.md; the signature is only used to verify that
# the block found at that line is the intended one (it never reaches the output).
# --------------------------------------------------------------------------------------
ICONS = [
    # --- sidebar main navigation (App.vue navItems) -------------------------------------
    ("Icon.Home",           "App.vue",                       125, "M3 10.5 12 3l9 7.5"),
    ("Icon.Game",           "App.vue",                       130, '<rect x="2" y="7"'),
    ("Icon.Friends",        "App.vue",                       135, "M2 21v-3a6 6 0 0 1 12 0v3"),
    ("Icon.Keys",           "App.vue",                       140, "M6 10h.01M10 10h.01"),
    ("Icon.Skin",           "App.vue",                       145, "m9 4-6 3 2 5"),
    ("Icon.Community",      "App.vue",                       150, '<circle cx="12" cy="12" r="9"'),
    ("Icon.Settings",       "App.vue",                       155, "M19.4 15a1.65 1.65"),
    # --- sidebar resource group ---------------------------------------------------------
    ("Icon.Resources",      "App.vue",                      1283, "M3 11h18"),
    ("Icon.CaretRight",     "App.vue",                      1286, "m9 6 6 6-6 6"),
    ("Icon.Mods",           "App.vue",                       178, "M21 8 12 3 3 8v8l9 5 9-5Z"),
    ("Icon.ResourcePacks",  "App.vue",                       183, "M3 7a2 2 0 0 1 2-2"),
    ("Icon.Shaders",        "App.vue",                       188, '<circle cx="12" cy="12" r="4"'),
    # --- topbar / window buttons --------------------------------------------------------
    ("Icon.Download",       "App.vue",                      1344, "M12 3v11"),
    ("Icon.Changelog",      "App.vue",                      1370, "M12 7v5l3.5 2"),
    ("Icon.Bell",           "App.vue",                      1377, "M18 8a6 6 0 1 0-12 0"),
    ("Icon.Back",           "App.vue",                      1338, "m15 18-6-6 6-6"),
    ("Icon.Minimize",       "App.vue",                      1388, "M5 12h14"),
    ("Icon.Maximize",       "App.vue",                      1393, '<rect x="6" y="6"'),
    # source has ONE button for both states (title "最大化/还原"), hence identical geometry
    ("Icon.Restore",        "App.vue",                      1393, '<rect x="6" y="6"'),
    ("Icon.Close",          "App.vue",                      1398, "M6 6l12 12M18 6 6 18"),
    # --- hero / launch area (HomeView.vue) ----------------------------------------------
    ("Icon.Gear",           "views/HomeView.vue",            509, "M19.4 15a1.7 1.7"),
    ("Icon.Ellipsis",       "views/HomeView.vue",            518, '<circle cx="5" cy="12" r="1.8"'),
    ("Icon.Play",           "views/HomeView.vue",            531, "M8 5.5v13l11-6.5Z"),
    ("Icon.ChevronDown",    "views/HomeView.vue",            536, "m6 9 6 6 6-6"),
    # --- runtime strip (HomeView.vue) ---------------------------------------------------
    ("Icon.Java",           "views/HomeView.vue",            546, "M8 2v4M16 2v4"),
    ("Icon.Memory",         "views/HomeView.vue",            551, '<rect x="5" y="5"'),
    ("Icon.Pulse",          "views/HomeView.vue",            556, "M3 12h4l2-7 4 14 2-7h6"),
    ("Icon.ChevronRight",   "views/HomeView.vue",            548, "m9 6 6 6-6 6"),
    # --- instances ----------------------------------------------------------------------
    ("Icon.Apps",           "views/HomeView.vue",            566, '<rect x="3" y="3" width="7"'),
    ("Icon.Star",           "views/GameView.vue",           1157, "m12 2 3.09 6.26"),
    ("Icon.StarOutline",    "views/GameView.vue",           1157, "m12 2 3.09 6.26"),
    # --- common actions -----------------------------------------------------------------
    ("Icon.Search",         "views/GameView.vue",           1013, '<circle cx="11" cy="11" r="7"'),
    ("Icon.Folder",         "views/GameView.vue",           1203, "M3 7a2 2 0 0 1 2-2"),
    ("Icon.Edit",           "views/GameView.vue",           1211, "M17 3a2.85 2.83"),
    ("Icon.Refresh",        "views/GameView.vue",           1034, "M21 12a9 9 0 1 1-2.64-6.36"),
    ("Icon.Check",          "components/SelectMenu.vue",     100, "M20 6 9 17l-5-5"),
    ("Icon.Close2",         "components/UpdateModal.vue",     67, "M18 6 6 18M6 6l12 12"),
    ("Icon.ChevronLeft",    "App.vue",                      1323, "m9 18 6-6-6-6"),
    ("Icon.Upload",         "App.vue",                      1561, "M21 15v4a2 2 0 0 1-2 2"),
    # --- account / skin -----------------------------------------------------------------
    ("Icon.User",           "views/SkinsView.vue",           420, '<circle cx="12" cy="8" r="4"'),
    ("Icon.Sliders",        "views/SettingsView.vue",        682, "M4 21v-7M4 10V3"),
    # --- non-themed multi colour marks --------------------------------------------------
    ("Icon.GrassBlock",     "views/HomeView.vue",            582, '<polygon points="24,5'),
    ("Icon.Microsoft",      "views/HomeView.vue",            621, "M0 0h10v10H0z"),
    # --- sidebar resource group, 1.1.10 additions ----------------------------------------
    ("Icon.Recordings",     "App.vue",                       191, '<rect x="3" y="5" width="14"'),
    ("Icon.Projections",    "App.vue",                       192, "m12 3 9 5v8l-9 5-9-5V8z"),
    # --- MOD 面板 / 服务器 nav icons (were transcribed by hand until 1.1.10) ------------
    ("Icon.Bridge",         "App.vue",                       196, '<circle cx="12" cy="12" r="3.5"'),
    ("Icon.Servers",        "App.vue",                       201, '<rect x="3" y="13" width="18"'),
    # --- 联机方式三卡（stroke-width 1.7, FriendConnectView methodCards） -----------------
    ("Icon.Bars",           "views/FriendConnectView.vue",    22, "M4 20V9m5 11V5m5 15v-8m5 8V8"),
    ("Icon.Gamepad",        "views/FriendConnectView.vue",    30, '<rect x="2.5" y="7"'),
    ("Icon.Cube",           "views/FriendConnectView.vue",    38, "M12 3 4 7v10l8 4 8-4V7l-8-4Z"),
    # --- skin viewer --------------------------------------------------------------------
    ("Icon.ResetView",      "views/SkinsView.vue",           399, '<circle cx="12" cy="12" r="7.5"'),
]

# requested by the port but with no SVG anywhere in the source tree (92 blocks / 85 unique d)
MISSING = [
    ("Icon.DownloadCloud", "no cloud glyph in the tree; download is App.vue:1334/1356/1581 (arrow+tray)"),
    ("Icon.Plus",          "only the text glyphs U+FF0B / U+002B (ServersView.vue:360, HomeView.vue:634)"),
    ("Icon.Warning",       "only the text glyph U+26A0 (SettingsView.vue:1178 and 5 more sites)"),
    ("Icon.Trash",         "deletion is text-only (GameView.vue context menu uses words)"),
    ("Icon.Copy",          "only the text glyph U+29C9 (ServerDetails.vue:15)"),
    ("Icon.Eye",           "no eye SVG; visibility toggles are text buttons"),
    ("Icon.Info",          "no info SVG"),
    ("Icon.Pause",         "pause is the text button 暂停 (App.vue:1455)"),
    ("Icon.Stop",          "stop is text-only"),
    ("Icon.Drag",          "no grip/drag SVG; drag handles are CSS/HTML5 dnd (App.vue:1068)"),
]

# keys whose rendering is forced to one style regardless of the source attrs
FORCE_STYLE = {
    # the source star is fill=currentColor; the unfavourited state in KAMUCL is the text
    # glyph U+2606, so the outline variant reuses the very same silhouette stroked.
    "Icon.StarOutline": "stroke",
}

# explanatory provenance notes that end up in the per-style XAML comment
NOTES = {
    "Icon.Restore": "same square as Icon.Maximize: the source has ONE button "
                    "(title 'maximize/restore', App.vue:1383) and no separate restore glyph",
    "Icon.StarOutline": "source only ships the filled star; this reuses that exact "
                        "silhouette with IconStroke (KAMUCL's empty state is the text glyph U+2606)",
    "Icon.Check": "stroke-width 2.6 from SelectMenu.vue:100; the same d is used with "
                  "stroke-width 3 at SettingsView.vue:668 and AccountsView.vue:577",
    "Icon.ChevronDown": "stroke-width 2.2 hero variant (HomeView.vue:538); SelectMenu.vue:82 "
                        "uses the identical d at stroke-width 2",
    "Icon.Java": "the runtime-strip Java slot is a calendar/Java-cup hybrid "
                 "(HomeView.vue:548); the literal coffee cup is GameView.vue:1216",
    "Icon.Gear": "HomeView simplified 8-tooth gear, distinct from Icon.Settings "
                 "(Feather gear, App.vue:139)",
}

# --------------------------------------------------------------------------------------
# UiGlyph.vue: 1.1.10 keeps its 22 utility glyphs in a JS object literal instead of
# inline SVG. The component template is fixed — one <path>, viewBox 24, fill none,
# stroke currentColor, stroke-width 1.7, round caps and joins — so the table alone is
# parsed here and re-emitted as Icon.G.<name>.
# --------------------------------------------------------------------------------------
UI_GLYPH = "components/UiGlyph.vue"
GLYPH_SW = "1.7"
GLYPH_RE = re.compile(r"^\s{2}([a-z][a-z0-9]*):\s*'([^']*)',\s*$", re.M)


def load_glyphs():
    path = os.path.join(SRC, UI_GLYPH.replace("/", os.sep))
    if not os.path.isfile(path):
        raise IconError(f"UiGlyph source missing: {path}")
    text = open(path, encoding="utf-8").read()
    table = text[text.index("const paths"):text.index("</script>")]
    out = []
    for m in GLYPH_RE.finditer(table):
        line = text[:m.start()].count("\n") + 1
        out.append((f"Icon.G.{m.group(1)}", m.group(2).strip(), line))
    if len(out) < 20:
        raise IconError(f"only {len(out)} glyph entries parsed from {UI_GLYPH}")
    return out


def glyph_info(d, line):
    """A parsed-svg record for one UiGlyph entry, using the component's fixed attrs."""
    return {"viewbox": (24.0, 24.0),
            "root": {"viewBox": "0 0 24 24", "fill": "none", "stroke": "currentColor",
                     "stroke-width": GLYPH_SW, "stroke-linecap": "round",
                     "stroke-linejoin": "round"},
            "els": [("path", {"d": d})],
            "where": f"{UI_GLYPH}:{line}"}


SELF_TAGS = {"path", "circle", "ellipse", "rect", "line", "polyline", "polygon"}
SUPPORTED_TAGS = SELF_TAGS
# a source svg using any of these would need geometry work this script does not do:
UNSUPPORTED_ATTRS = ("transform", "stroke-dasharray", "stroke-dashoffset", "clip-path",
                     "mask", "filter", "fill-rule", "stroke-miterlimit", "dx", "dy")

SVG_RE = re.compile(r"<svg\b.*?</svg>", re.S)
ATTR_RE = re.compile(r'([\w:-]+)\s*=\s*"([^"]*)"')
TAG_RE = re.compile(r"<([a-zA-Z]+)\b([^>]*?)/?>")
# one dot at most per number, so SVG's packed "l.06.06" splits into ".06" + ".06"
NUMBER = r"[-+]?(?:\d*\.\d+|\d+(?:\.\d*)?)(?:[eE][-+]?\d+)?"
NUM_TOKEN_RE = re.compile(r"[A-Za-z]|" + NUMBER)


class IconError(Exception):
    pass


# --------------------------------------------------------------------------------------
# number formatting
# --------------------------------------------------------------------------------------
def num(v):
    """Shortest exact decimal, no leading dot, no trailing zeros, no '-0'."""
    r = round(float(v), 6)
    if r == 0:
        r = 0.0
    if r == int(r):
        return str(int(r))
    s = f"{r:.6f}".rstrip("0").rstrip(".")
    return s


def pt(x, y):
    return f"{num(x)},{num(y)}"


# --------------------------------------------------------------------------------------
# <path d> parser: tokenise -> normalise -> expand relatives -> absolute uppercase output
# --------------------------------------------------------------------------------------
ARC_FLAGS = {4, 5}


def tokenize(d):
    toks = NUM_TOKEN_RE.findall(d)
    if not toks:
        raise IconError(f"empty path d: {d!r}")
    out = []
    for t in toks:
        if re.fullmatch(r"[A-Za-z]", t):
            out.append(t)
        else:
            try:
                out.append(float(t))
            except ValueError:
                raise IconError(f"bad token {t!r} in d={d!r}")
    return out


PARAMS = {"M": 2, "L": 2, "H": 1, "V": 1, "C": 6, "S": 4, "Q": 4, "T": 2, "A": 7, "Z": 0}


def parse_path(d):
    """Return [(raw command char, [float args]), ...] with implicit repeats made explicit."""
    toks = tokenize(d)
    segs = []
    i = 0
    cmd = None
    while i < len(toks):
        tok = toks[i]
        if isinstance(tok, str):
            cmd = tok
            if cmd.upper() not in PARAMS:
                raise IconError(f"unsupported path command {tok!r} in d={d!r}")
            i += 1
            if cmd.upper() == "Z":
                segs.append((cmd, []))
                cmd = None
            continue
        if cmd is None:
            raise IconError(f"number before command in d={d!r}")
        n = PARAMS[cmd.upper()]
        chunk = toks[i:i + n]
        if len(chunk) != n or any(isinstance(c, str) for c in chunk):
            raise IconError(f"truncated {cmd} group in d={d!r}")
        segs.append((cmd, [float(c) for c in chunk]))
        i += n
        # implicit repetition: "M x y x2 y2" continues with L / l
        cmd = {"M": "L", "m": "l"}.get(cmd, cmd)
        if cmd and cmd.upper() == "Z":
            cmd = None
    return segs


def absoluteize(cmd, a, cx, cy):
    """Add the current point to a relative command's parameters."""
    c = cmd.upper()
    if c in ("M", "L", "T"):
        return [a[0] + cx, a[1] + cy]
    if c == "H":
        return [a[0] + cx]
    if c == "V":
        return [a[0] + cy]
    if c == "C":
        return [a[0] + cx, a[1] + cy, a[2] + cx, a[3] + cy, a[4] + cx, a[5] + cy]
    if c in ("S", "Q"):
        return [a[0] + cx, a[1] + cy, a[2] + cx, a[3] + cy]
    if c == "A":
        return [a[0], a[1], a[2], a[3], a[4], a[5] + cx, a[6] + cy]
    return list(a)


def path_to_geometry(d):
    """SVG d -> absolute uppercase WPF mini-language, one command letter per group."""
    segs = parse_path(d)
    cx = cy = sx = sy = 0.0          # current point / subpath start
    last_c = None                   # last control point of previous cubic (for S)
    last_q = None                   # last control point of previous quadratic (for T)
    prev = None
    out = []

    for raw, args in segs:
        cmd = raw.upper()
        if cmd == "Z":
            out.append("Z")
            cx, cy = sx, sy
            last_c = last_q = None
            prev = cmd
            continue

        a = absoluteize(raw, args, cx, cy) if raw.islower() else list(args)

        if cmd == "M":
            out.append(f"M{pt(a[0], a[1])}")
            cx, cy = a[0], a[1]
            sx, sy = cx, cy
            last_c = last_q = None
        elif cmd == "L":
            out.append(f"L{pt(a[0], a[1])}")
            cx, cy = a[0], a[1]
            last_c = last_q = None
        elif cmd == "H":
            # SVG "h0"/"v0" no-ops are dropped, but the sub-pixel "h.01" dot trick is kept
            # (round line caps render it as a dot in both SVG and WPF)
            if abs(a[0] - cx) > 1e-9:
                out.append(f"H{num(a[0])}")
            cx = a[0]
            last_c = last_q = None
        elif cmd == "V":
            if abs(a[0] - cy) > 1e-9:
                out.append(f"V{num(a[0])}")
            cy = a[0]
            last_c = last_q = None
        elif cmd == "C":
            out.append(f"C{pt(a[0], a[1])} {pt(a[2], a[3])} {pt(a[4], a[5])}")
            last_c = (a[2], a[3])
            last_q = None
            cx, cy = a[4], a[5]
        elif cmd == "S":
            if prev in ("C", "S") and last_c:
                c1 = (2 * cx - last_c[0], 2 * cy - last_c[1])
            else:
                c1 = (cx, cy)
            out.append(f"C{pt(c1[0], c1[1])} {pt(a[0], a[1])} {pt(a[2], a[3])}")
            last_c = (a[0], a[1])
            last_q = None
            cx, cy = a[2], a[3]
        elif cmd == "Q":
            out.append(f"Q{pt(a[0], a[1])} {pt(a[2], a[3])}")
            last_q = (a[0], a[1])
            last_c = None
            cx, cy = a[2], a[3]
        elif cmd == "T":
            if prev in ("Q", "T") and last_q:
                q = (2 * cx - last_q[0], 2 * cy - last_q[1])
            else:
                q = (cx, cy)
            out.append(f"Q{pt(q[0], q[1])} {pt(a[0], a[1])}")
            last_q = q
            last_c = None
            cx, cy = a[0], a[1]
        elif cmd == "A":
            rx, ry, rot, laf, spf, x, y = a
            out.append(f"A{num(rx)},{num(ry)} {num(rot)} {num(laf)},{num(spf)} {pt(x, y)}")
            last_c = last_q = None
            cx, cy = x, y
        else:
            raise IconError(f"unhandled command {cmd!r} in d={d!r}")

        prev = cmd

    data = " ".join(out)
    if any(ch.islower() for ch in re.sub(r"[^A-Za-z]", "", data)):
        raise IconError(f"lowercase command survived: {data}")
    return data


# --------------------------------------------------------------------------------------
# primitive shapes
# --------------------------------------------------------------------------------------
def circle_geometry(cx, cy, r):
    return (f"M{num(cx - r)},{num(cy)} "
            f"A{num(r)},{num(r)} 0 0,1 {num(cx + r)},{num(cy)} "
            f"A{num(r)},{num(r)} 0 0,1 {num(cx - r)},{num(cy)} Z")


def ellipse_geometry(cx, cy, rx, ry):
    return (f"M{num(cx - rx)},{num(cy)} "
            f"A{num(rx)},{num(ry)} 0 0,1 {num(cx + rx)},{num(cy)} "
            f"A{num(rx)},{num(ry)} 0 0,1 {num(cx - rx)},{num(cy)} Z")


def rect_geometry(x, y, w, h, rx=0.0, ry=None):
    if not rx:
        return f"M{num(x)},{num(y)} H{num(x + w)} V{num(y + h)} H{num(x)} Z"
    if ry is None:
        ry = rx
    r = min(rx, w / 2.0)
    ry = min(ry, h / 2.0)
    if r <= 0 or ry <= 0:
        return f"M{num(x)},{num(y)} H{num(x + w)} V{num(y + h)} H{num(x)} Z"
    right, bottom = x + w, y + h
    arc = f"A{num(r)},{num(ry)} 0 0,1"
    parts = [f"M{num(x + r)},{num(y)}"]
    # edges collapse to nothing when the radius eats the whole side (e.g. rx=5.5 on h=11)
    if right - r - (x + r) > 1e-9:
        parts.append(f"H{num(right - r)}")
    parts.append(f"{arc} {num(right)},{num(y + ry)}")
    if bottom - ry - (y + ry) > 1e-9:
        parts.append(f"V{num(bottom - ry)}")
    parts.append(f"{arc} {num(right - r)},{num(bottom)}")
    if right - r - (x + r) > 1e-9:
        parts.append(f"H{num(x + r)}")
    parts.append(f"{arc} {num(x)},{num(bottom - ry)}")
    if bottom - ry - (y + ry) > 1e-9:
        parts.append(f"V{num(y + ry)}")
    parts.append(f"{arc} {num(x + r)},{num(y)} Z")
    return " ".join(parts)


def points_geometry(raw, close):
    nums = [float(v) for v in re.findall(NUMBER, raw)]
    if len(nums) < 4 or len(nums) % 2:
        raise IconError(f"bad points list: {raw!r}")
    parts = [f"M{pt(nums[0], nums[1])}"] + [f"L{pt(nums[i], nums[i + 1])}"
                                           for i in range(2, len(nums), 2)]
    if close:
        parts.append("Z")
    return " ".join(parts)


def line_geometry(x1, y1, x2, y2):
    return f"M{pt(x1, y1)} L{pt(x2, y2)}"


# --------------------------------------------------------------------------------------
# svg block -> element records
# --------------------------------------------------------------------------------------
def load_blocks(rel):
    path = os.path.join(SRC, rel.replace("/", os.sep))
    if not os.path.isfile(path):
        raise IconError(f"source file not found: {path}")
    text = open(path, encoding="utf-8").read()
    blocks = []
    for m in SVG_RE.finditer(text):
        line = text[:m.start()].count("\n") + 1
        blocks.append((line, m.group(0)))
    return blocks


def find_block(rel, line, sig):
    norm = lambda s: re.sub(r"\s+", " ", s).strip()
    blocks = load_blocks(rel)
    for bline, svg in blocks:
        if bline == line and norm(sig) in norm(svg):
            return svg, f"{rel}:{line}"
    # 锚点漂移：换基线（1.1.5 → 1.1.10）之后几乎所有行号都挪了位。
    # 先按签名找；命中多条时取「离原锚点最近」的那条并大声报出所有候选，
    # 因为同一个图标在源码里常被复制成两三份（顶栏、菜单、模态里各一颗），
    # 距离最近的那份才是原来那一个。候选全部打印，方便人工复核。
    hits = [(bline, svg) for bline, svg in blocks if norm(sig) in norm(svg)]
    if len(hits) == 1:
        print(f"  note: {rel} anchor {line} moved, matched block at line {hits[0][0]}")
        return hits[0][1], f"{rel}:{hits[0][0]}"
    if len(hits) > 1:
        near = min(hits, key=lambda h: abs(h[0] - line))
        others = ", ".join(str(h[0]) for h in hits if h is not near)
        print(f"  note: {rel} anchor {line} moved; {len(hits)} 命中（{others}），取最近的 line {near[0]}")
        return near[1], f"{rel}:{near[0]}"
    raise IconError(f"cannot locate svg in {rel} near line {line} ({len(hits)} signature hits)")


def parse_svg(svg, where):
    m = re.match(r"<svg\b([^>]*?)>", svg, re.S)
    if not m:
        raise IconError(f"malformed svg root in {where}")
    root = dict(ATTR_RE.findall(m.group(1)))
    vb = root.get("viewBox") or root.get("viewbox")
    if not vb:
        raise IconError(f"svg without viewBox at {where}")
    vbnums = [float(v) for v in re.findall(NUMBER, vb)]
    if len(vbnums) != 4:
        raise IconError(f"bad viewBox {vb!r} at {where}")
    _, _, vw, vh = vbnums

    body = svg[m.end():svg.rindex("</svg>")]
    els = []
    for t in TAG_RE.finditer(body):
        tag, raw = t.group(1), t.group(2)
        if tag == "svg":
            continue
        if tag not in SUPPORTED_TAGS:
            raise IconError(f"unsupported element <{tag}> at {where}")
        attrs = dict(ATTR_RE.findall(raw))
        for a in attrs:
            if a in UNSUPPORTED_ATTRS:
                raise IconError(f"unsupported attribute {a!r} on <{tag}> at {where}")
        els.append((tag, attrs))
    if not els:
        raise IconError(f"svg has no drawable children at {where}")
    return {"viewbox": (vw, vh), "root": root, "els": els, "where": where}


def resolve(root, child, name, default=None):
    if name in child:
        return child[name]
    if name in root:
        return root[name]
    return default


def geometry_for(tag, a):
    f = lambda k, d=0.0: float(a.get(k, d))
    if tag == "path":
        return path_to_geometry(re.sub(r"\s+", " ", a["d"]).strip())
    if tag == "circle":
        return circle_geometry(f("cx"), f("cy"), f("r"))
    if tag == "ellipse":
        return ellipse_geometry(f("cx"), f("cy"), f("rx"), f("ry"))
    if tag == "rect":
        w, h = f("width"), f("height")
        rx = float(a.get("rx", 0) or 0)
        ry = a.get("ry")
        return rect_geometry(f("x"), f("y"), w, h, min(rx, w / 2, h / 2) if rx else 0.0,
                             (min(float(ry), w / 2, h / 2) if ry else None))
    if tag == "line":
        return line_geometry(f("x1"), f("y1"), f("x2"), f("y2"))
    if tag == "polyline":
        return points_geometry(a["points"], close=False)
    if tag == "polygon":
        return points_geometry(a["points"], close=True)
    raise IconError(f"unhandled tag {tag}")


def build_paths(info, key):
    """Return (list of xaml path strings, diagnostics) for one parsed svg."""
    root = info["root"]
    paths = []
    notes = []
    for tag, child in info["els"]:
        data = geometry_for(tag, child)
        fill = resolve(root, child, "fill", "none")
        stroke = resolve(root, child, "stroke")
        sw = resolve(root, child, "stroke-width")
        swv = float(sw) if sw not in (None, "") else None

        forced = FORCE_STYLE.get(key)
        literal = None
        if fill not in (None, "none"):
            kind = "IconFill"
            if fill != "currentColor":
                literal = fill.upper()          # grass block / Microsoft brand colours
        elif stroke == "currentColor":
            kind = "IconStroke"
        elif stroke and stroke != "none":
            kind = "IconStroke"
            notes.append(f"{key}: stroke={stroke!r} (non-currentColor) on <{tag}>")
        else:
            kind = "IconFill"

        if forced == "stroke":
            kind = "IconStroke"
        if forced == "fill":
            kind = "IconFill"

        props = [f'Style="{{StaticResource {kind}}}"']
        if kind == "IconStroke" and swv is not None and swv != 2:
            props.append(f'StrokeThickness="{num(swv)}"')
        if literal:
            props.append(f'Fill="{literal}"')
        props.append(f'Data="{data}"')
        paths.append(" ".join(props))

        caps = resolve(root, child, "stroke-linecap")
        joins = resolve(root, child, "stroke-linejoin")
        if kind == "IconStroke":
            if caps is None:
                notes.append(f"<{tag}>: no stroke-linecap in source")
            if joins is None:
                notes.append(f"<{tag}>: no stroke-linejoin in source")
    return paths, sorted(set(notes))


# --------------------------------------------------------------------------------------
# emit xaml
# --------------------------------------------------------------------------------------
IND = "    "


def render(key, info, paths, provenance, sw_note, extra=""):
    vw, vh = info["viewbox"]
    lines = []
    lines.append(f"{IND * 0}<!-- {key}  <-  {provenance}{sw_note}{extra} -->")
    lines.append(f"{IND * 0}<Style x:Key=\"{key}\" TargetType=\"ContentControl\">")
    lines.append(f"{IND * 1}<Setter Property=\"Template\">")
    lines.append(f"{IND * 2}<Setter.Value>")
    lines.append(f"{IND * 3}<ControlTemplate TargetType=\"ContentControl\">")
    lines.append(f"{IND * 4}<Viewbox Stretch=\"Uniform\">")
    lines.append(f"{IND * 5}<Canvas Width=\"{num(vw)}\" Height=\"{num(vh)}\">")
    for p in paths:
        lines.append(f"{IND * 6}<Path {p} />")
    lines.append(f"{IND * 5}</Canvas>")
    lines.append(f"{IND * 4}</Viewbox>")
    lines.append(f"{IND * 3}</ControlTemplate>")
    lines.append(f"{IND * 2}</Setter.Value>")
    lines.append(f"{IND * 1}</Setter>")
    lines.append(f"{IND * 0}</Style>")
    return "\n".join(lines)


def sw_annotation(info):
    sw = info["root"].get("stroke-width")
    fills = [c.get("fill", info["root"].get("fill")) for _, c in info["els"]]
    if sw in (None, ""):
        if any(v == "currentColor" for v in fills):
            return "  fill=currentColor"
        return ""
    return f"  stroke-width {sw}"


def generate():
    chunks = []
    stats = {"keys": 0, "svg_children": 0, "paths": 0, "viewboxes": {}}
    report = []
    records = []
    for key, rel, line, sig in ICONS:
        svg, provenance = find_block(rel, line, sig)
        records.append((key, parse_svg(svg, provenance)))
    for key, d, line in load_glyphs():
        records.append((key, glyph_info(d, line)))
    for key, info in records:
        provenance = info["where"]
        paths, notes = build_paths(info, key)
        if not paths:
            raise IconError(f"{key}: no geometry produced")
        stats["keys"] += 1
        stats["svg_children"] += len(info["els"])
        stats["paths"] += len(paths)
        stats["viewboxes"][key] = (info["viewbox"][0], info["viewbox"][1])
        note = NOTES.get(key, "")
        extra = f"  |  {note}" if note else ""
        chunks.append(render(key, info, paths, provenance, sw_annotation(info), extra))
        report.append((key, provenance, len(info["els"]), notes))
    body = ("\n\n".join(chunks)).rstrip("\n")

    header = (
        f'<ResourceDictionary xmlns="{NS_D}"\n'
        f'                    xmlns:x="{NS_X}">\n\n'
        "    <!-- GENERATED FILE - DO NOT EDIT BY HAND.\n"
        f"         Rewritten by tools/gen_icons.py from {SRC} (read-only source of truth):\n"
        "         the inline SVGs listed in the ICONS catalogue, plus every entry of\n"
        "         components/UiGlyph.vue's JS path table as Icon.G.<name> (stroke-width 1.7).\n"
        "         Each Style is a ContentControl template: Viewbox > Canvas(viewBox size) > Path.\n"
        "         Path colour comes from Themes/Controls.xaml IconStroke / IconFill styles;\n"
        "         StrokeThickness is set only where the source stroke-width differs from 2. -->\n"
    )
    text = header + "\n\n" + body + "\n\n</ResourceDictionary>\n"
    return text, stats, report


# --------------------------------------------------------------------------------------
# validation
# --------------------------------------------------------------------------------------
LEGAL = set("MLHVCSQTAZ")


def check_geometry(data, key):
    """Structural well-formedness of one emitted geometry string (arity, flags, subpaths)."""
    toks = NUM_TOKEN_RE.findall(data)
    i = 0
    cmd = None
    started = False
    while i < len(toks):
        t = toks[i]
        if re.fullmatch(r"[A-Za-z]", t):
            cmd = t
            if cmd not in PARAMS:
                raise IconError(f"{key}: unknown command {cmd!r} in {data}")
            if cmd == "Z":
                if not started:
                    raise IconError(f"{key}: Z without an open figure in {data}")
                cmd = None
            i += 1
            continue
        if cmd is None:
            raise IconError(f"{key}: coordinate before a command in {data}")
        n = PARAMS[cmd]
        group = toks[i:i + n]
        if len(group) != n or any(re.fullmatch(r"[A-Za-z]", g) for g in group):
            raise IconError(f"{key}: truncated {cmd} group in {data}")
        vals = [float(g) for g in group]
        if cmd == "M" and not started:
            started = True
        if cmd == "A":
            if vals[0] < 0 or vals[1] < 0:
                raise IconError(f"{key}: negative arc radius in {data}")
            if vals[3] not in (0.0, 1.0) or vals[4] not in (0.0, 1.0):
                raise IconError(f"{key}: arc flags must be 0/1, got {vals[3]},{vals[4]}")
            if abs(vals[2]) >= 360:
                raise IconError(f"{key}: absurd arc rotation {vals[2]}")
        if any(v != v or abs(v) > 1e6 for v in vals):
            raise IconError(f"{key}: non-finite/absurd coordinate in {data}")
        i += n
        cmd = {"M": "L"}.get(cmd, cmd)
    if not started:
        raise IconError(f"{key}: geometry has no figure: {data}")


def validate(text, stats):
    root = ET.fromstring(text)
    if root.tag != f"{{{NS_D}}}ResourceDictionary":
        raise IconError("root is not ResourceDictionary")
    styles = root.findall(f"{{{NS_D}}}Style")
    keys = [s.get(f"{{{NS_X}}}Key") for s in styles]
    if len(keys) != stats["keys"]:
        raise IconError(f"style count {len(keys)} != expected {stats['keys']}")
    if len(set(keys)) != len(keys):
        dup = [k for k in set(keys) if keys.count(k) > 1]
        raise IconError(f"duplicate x:Key: {dup}")
    if set(keys) != {k for k, *_ in ICONS} | {k for k, _, _ in load_glyphs()}:
        raise IconError("generated keys differ from catalogue")

    paths = root.iter(f"{{{NS_D}}}Path")
    n = 0
    for p in paths:
        n += 1
        data = (p.get("Data") or "").strip()
        if not data:
            raise IconError("empty Path Data")
        letters = set(re.sub(r"[^A-Za-z]", "", data))
        if not letters <= LEGAL:
            raise IconError(f"illegal letters {sorted(letters - LEGAL)} in {data[:60]}")
        if any(ch.islower() for ch in letters):
            raise IconError(f"lowercase command in {data[:60]}")
        style = p.get("Style") or ""
        if "StaticResource IconStroke" not in style and "StaticResource IconFill" not in style:
            raise IconError(f"Path without IconStroke/IconFill: {data[:60]}")
        if "{DynamicResource" in data or "currentColor" in data:
            raise IconError("unconverted currentColor inside Data")
        check_geometry(data, style)
    if n != stats["paths"]:
        raise IconError(f"Path count {n} != generated {stats['paths']}")

    # every style must wrap exactly one Viewbox > Canvas sized to the source viewBox
    for s in styles:
        key = s.get(f"{{{NS_X}}}Key")
        cv = s.find(f".//{{{NS_D}}}Canvas")
        vb = s.find(f".//{{{NS_D}}}Viewbox")
        if cv is None or vb is None:
            raise IconError(f"{key}: missing Viewbox/Canvas")
        want = stats["viewboxes"][key]
        got = (float(cv.get("Width")), float(cv.get("Height")))
        if got != want:
            raise IconError(f"{key}: Canvas {got} != source viewBox {want}")
        if vb.get("Stretch") != "Uniform":
            raise IconError(f"{key}: Viewbox must Stretch=Uniform")
    # no leftover currentColor in any attribute value (provenance comments may mention it)
    for el in root.iter():
        for name, value in el.attrib.items():
            if "currentColor" in value:
                raise IconError(f"currentColor survived in {name}={value!r}")
    return n, len(keys)


def main():
    try:
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    except Exception:
        pass
    if not os.path.isdir(SRC):
        raise IconError(f"source tree missing: {SRC}")
    text, stats, report = generate()
    n_paths, n_keys = validate(text, stats)

    print(f"source tree   : {SRC}")
    print(f"icon keys     : {n_keys}")
    print(f"svg children  : {stats['svg_children']}")
    print(f"paths emitted : {n_paths}  (1:1 with svg children: "
          f"{'OK' if n_paths == stats['svg_children'] else 'MISMATCH'})")
    widths = sorted({re.search(r'StrokeThickness="([\d.]+)"', l).group(1)
                     for l in text.splitlines() if 'StrokeThickness="' in l},
                    key=float)
    print(f"non-2 stroke widths: {', '.join(widths)}")

    dev = [(k, p, notes) for k, p, _, notes in report if notes]
    if dev:
        print("source-attribute deviations (Icons.xaml keeps the shared round cap/join recipe):")
        for k, p, notes in dev:
            print(f"  {k} ({p}): " + "; ".join(notes))

    if "--stdout" in sys.argv:
        sys.stdout.write(text)
        return
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with open(OUT, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(text)
    print(f"wrote         : {OUT} ({len(text.encode('utf-8'))} bytes)")
    print("omitted (no SVG in source):")
    for k, why in MISSING:
        print(f"  {k}: {why}")


if __name__ == "__main__":
    main()
