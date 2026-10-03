"""原版视图元素清单 + 与 WPF 页面对表。

为什么要这个：用户问"UI 是不是 100% 复刻"，肉眼截图回答不了。
需要一份机器抽出来的数表——原版每个视图有多少个真正带样式的类、
我这边的 XAML 里对应实现了多少、缺的是哪些。

第一阶段：从 <template> 抽「标签 + class（含 :class 绑定）」，并统计该类在本文件样式块
或全局样式里有没有规则。只有"真的有样式规则"的类才算 UI 债，
纯状态字面量（:class="{ active: tab === 'download' }" 里的 download）不计。

第二阶段（--parity）：把这些类拿去我的 XAML 里找。命中判据是类名在页面文件里出现过——
依据是本项目约定：每个还原元素都带注明原版类名的注释，或复用同名 Style 键（.tag → Tag）。
所以"命中"只代表"这个类有人管"，不代表数值一定对；未命中则是确定没做的东西。
原版 Teleport 到 body 的模态层落在壳里，由 TELEPORT_HOST 补进检索范围（见该字典的注释）。

用法：python tools/ui_element_inventory.py            # 每页元素/类汇总
      python tools/ui_element_inventory.py --parity   # 对表台账（markdown）
      python tools/ui_element_inventory.py --json     # 落 docs/ui_inventory.json
"""
import json
import re
import sys
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
# 基准版本：2026-10-03 起从 KAMUCL-1.1.5 换成 KAMUCL-1.1.10（用户把 1.1.10 的源码与成品都放进了工作区）。
# 要看旧基线：python tools/ui_element_inventory.py --base=1.1.5
base = "1.1.10"
for _a in sys.argv[1:]:
    if _a.startswith("--base="):
        base = _a[7:]
SRC = ROOT.parent / f"KAMUCL-{base}" / "src" / "renderer" / "src"
APP = ROOT / "Wuhuicl.App"
OUT = ROOT / "docs" / "ui_inventory.json"

# 贪婪到最后一个 </template>：SFC 只有一个根 template，里面常有 <template v-if> 的闭合标签，
# 非贪婪会在第一处就截断（GameView 曾被数成 9 个元素）
TEMPLATE = re.compile(r"<template>(.*)</template>", re.S)
# 属性可能一个都没有（<div>），所以标签名后的空白不是必需的；整体允许跨行
TAG = re.compile(r"<([a-zA-Z][\w-]*)((?:[^>/]|\"[^\"]*\"|'[^']*')*?)/?>")
ATTR = re.compile(r"([\w:@\-.\[\]]+)(?:=(\"([^\"]*)\"|'([^']*)'|\{\{[^}]*\}\}))?")
STYLE_BLOCK = re.compile(r"<style[^>]*>(.*?)</style>", re.S)
CSS_RULE = re.compile(r"([^{}]+)\{([^{}]*)\}")

# 原版视图 → 我这边的落点。共享壳（App.vue）对应 MainWindow + Controls + Icons。
MAP = {
    "App.vue": ["MainWindow.xaml", "Themes/Controls.xaml", "Themes/Icons.xaml", "App.xaml"],
    "HomeView.vue": ["Pages/HomePage.xaml", "Themes/Controls.xaml"],
    "GameView.vue": ["Pages/VersionsPage.xaml", "Themes/Controls.xaml"],
    "ServersView.vue": ["Pages/ServersPage.xaml", "Themes/Controls.xaml"],
    "BridgeView.vue": ["Pages/BridgePage.xaml", "Themes/Controls.xaml"],
    "FriendConnectView.vue": ["Pages/FriendsPage.xaml", "Themes/Controls.xaml"],
    "KeysView.vue": ["Pages/KeysPage.xaml", "Themes/Controls.xaml"],
    "SkinsView.vue": ["Pages/SkinsPage.xaml", "Themes/Controls.xaml"],
    "CommunityView.vue": ["Pages/CommunityPage.xaml", "Themes/Controls.xaml"],
    "FileManager.vue": ["Pages/ModsPage.xaml", "Themes/Controls.xaml"],
    "RecordingsView.vue": ["Pages/RecordingsPage.xaml", "Themes/Controls.xaml"],
    "SettingsView.vue": ["Pages/SettingsPage.xaml", "Themes/Controls.xaml"],
    "AccountsView.vue": ["Pages/AccountsPage.xaml", "Themes/Controls.xaml"],
    # ↓ 1.1.10 新增的界面。落点写"该放哪儿"，文件还不存在就是 0 命中，缺口会直接显在表上。
    # Mods/Packs/Shaders 三页在原版只是 FileManager 的薄壳（各 500 字节，只换 title/rel/icon），
    # 所以不单独列，统一算在 FileManager.vue 那一行。
    "ProjectionsView.vue": ["Pages/ProjectionsPage.xaml", "Themes/Controls.xaml"],
    "ProjectionConvert.vue": ["Pages/ProjectionsPage.xaml", "Themes/Controls.xaml"],
    "MascotStage.vue": ["MainWindow.xaml", "Themes/Controls.xaml", "Themes/Icons.xaml"],
    "SupplementalModsResult.vue": ["MainWindow.xaml", "Themes/Controls.xaml"],
    "SkinEditor.vue": ["Pages/SkinsPage.xaml", "Themes/Controls.xaml"],
    "SkinColorPalette.vue": ["Pages/SkinsPage.xaml", "Themes/Controls.xaml"],
    "CommunityFavorites.vue": ["Pages/CommunityPage.xaml", "Themes/Controls.xaml"],
    "CommunityModDetails.vue": ["Pages/CommunityPage.xaml", "Themes/Controls.xaml"],
    "FavoriteModsPicker.vue": ["Pages/VersionsPage.xaml", "Themes/Controls.xaml"],
    "MemoryOrganizer.vue": ["Pages/SettingsPage.xaml", "Themes/Controls.xaml"],
    "UiGlyph.vue": ["Themes/Icons.xaml", "Themes/Controls.xaml"],
}

# 有些原版的样式块根本不在对应页面的 XAML 里，硬拿页面文件去找会永远算成缺口：
# 1) 原版用 <Teleport to="body"> 把模态层挂到 body 上，这些层在 WPF 里归壳（MainWindow.xaml）；
# 2) 由 C# 画出来的界面（主题选择卡是 BuildThemeOption 现建的），落点在 MainWindow.xaml.cs。
# 加了这一条，每条命中都要能在指出的文件里搜到注释或 Style 键名。
TELEPORT_HOST = {
    "App.vue": ["MainWindow.xaml", "MainWindow.xaml.cs"],   # .nav-parent.group-active 在 Show() 里按视图算
    "HomeView.vue": ["MainWindow.xaml"],        # .log-mask / .log-dialog / .log-body / .log-empty / .card-float-menu
    "SettingsView.vue": ["MainWindow.xaml.cs"],  # .theme-preview 与整族 tp-*（BuildThemeOption）
}


def parse_template(text):
    """该文件模板里的元素列表：(tag, {attr: value})。"""
    m = TEMPLATE.search(text)
    if not m:
        return []
    body = re.sub(r"<!--.*?-->", " ", m.group(1), flags=re.S)
    out = []
    for tm in TAG.finditer(body):
        attrs = {}
        for am in ATTR.finditer(tm.group(2)):
            val = am.group(3) if am.group(3) is not None else (am.group(4) or "")
            attrs.setdefault(am.group(1), val)
        out.append((tm.group(1), attrs))
    return out


def classes_of(attrs):
    """静态 class + :class 的对象键与字符串字面量，都算该元素可能带上的类。"""
    got = [c for c in (attrs.get("class") or "").split() if c]
    bound = attrs.get(":class") or ""
    got += re.findall(r"['\"]([\w\-]+)['\"]", bound)
    got += re.findall(r"([a-zA-Z][\w\-]*)\s*:", bound)
    return [c for c in got if c and not c.startswith(":")]


def css_defs(text):
    """本文件样式块：类名 → (选择器出现次数, 声明条数)。"""
    used, decl = Counter(), Counter()
    for block in STYLE_BLOCK.finditer(text):
        for sel, body in CSS_RULE.findall(block.group(1)):
            for cls in re.findall(r"\.([A-Za-z][\w\-]*)", sel):
                used[cls] += 1
                decl[cls] += body.count(";")
    return used, decl


def build_inv():
    files = [SRC / "App.vue"] + sorted((SRC / "views").glob("*.vue")) \
        + sorted((SRC / "components").glob("**/*.vue"))
    glob_src = "".join((SRC / n).read_text(encoding="utf-8")
                       for n in ("styles.css", "ui-system.css") if (SRC / n).exists())
    glob_used = set(re.findall(r"\.([A-Za-z][\w\-]*)", glob_src))
    inv = {}
    for f in files:
        text = f.read_text(encoding="utf-8")
        els = parse_template(text)
        if not els:
            continue
        counter = Counter()
        for _, attrs in els:
            for c in classes_of(attrs):
                counter[c] += 1
        used, decl = css_defs(text)
        rows = [{"class": c, "uses": n, "ownRule": used.get(c, 0),
                 "ownDecls": decl.get(c, 0), "global": c in glob_used}
                for c, n in counter.most_common()]
        inv[f.name] = {"elements": len(els), "classes": len(counter), "rows": rows}
    return inv


def parity(inv):
    """markdown 台账 + [(视图, 缺失类)] 明细。"""
    rows_out, tot_all, tot_hit, misses = [], 0, 0, []
    for view, targets in MAP.items():
        d = inv.get(view)
        if not d:
            continue
        real = [r for r in d["rows"] if r["ownRule"] or r["global"]]
        blob = "".join((APP / rel).read_text(encoding="utf-8")
                       for rel in targets + TELEPORT_HOST.get(view, [])
                       if (APP / rel).exists())
        # 归一化后再比：CSS 用 kebab、XAML 用 Pascal（.switch-ui ↔ SwitchUi、.page-title ↔ PageTitle），
        # 区分大小写的子串匹配会把已实现的东西算成缺口（首版台账 54.5% 就是这么虚低的）
        hay = re.sub(r"[^0-9a-z]", "", blob.lower())
        missing, hit = [], 0
        for r in real:
            needle = re.sub(r"[^0-9a-z]", "", r["class"].lower())
            if needle and needle in hay:
                hit += 1
            else:
                missing.append((r["class"], r["ownDecls"] + (1 if r["global"] else 0)))
        missing.sort(key=lambda x: -x[1])
        misses += [(view, c) for c, _ in missing]
        tot_all += len(real)
        tot_hit += hit
        rows_out.append("| {} | {} | {} | {:.1f}% | {} |".format(
            view, len(real), hit, 100.0 * hit / max(1, len(real)),
            (", ".join("`%s`" % c for c, _ in missing[:12])
             + (" …(+%d)" % (len(missing) - 12) if len(missing) > 12 else ""))
            if missing else "—"))
    lines = ["| 原版视图 | 有样式的类 | XAML 命中 | 覆盖率 | 未命中（按声明数排序） |",
             "|---|---|---|---|---|"] + rows_out
    lines.append("| **合计** | **{}** | **{}** | **{:.1f}%** | |".format(
        tot_all, tot_hit, 100.0 * tot_hit / max(1, tot_all)))
    return "\n".join(lines), misses


def misses(inv):
    """只打印未命中的类名（--parity 的表格里每组只留 12 个，这里给全量）。"""
    for view, targets in MAP.items():
        d = inv.get(view)
        if not d:
            continue
        real = [r for r in d["rows"] if r["ownRule"] or r["global"]]
        blob = "".join((APP / rel).read_text(encoding="utf-8")
                       for rel in targets + TELEPORT_HOST.get(view, [])
                       if (APP / rel).exists())
        hay = re.sub(r"[^0-9a-z]", "", blob.lower())
        out = [r["class"] for r in real
               if re.sub(r"[^0-9a-z]", "", r["class"].lower()) not in hay]
        print("%s (%d): %s" % (view, len(out), ", ".join(sorted(out))))


def main():
    inv = build_inv()
    if "--misses" in sys.argv:
        misses(inv)
        return 0
    if "--parity" in sys.argv:
        table, _ = parity(inv)
        print(table)
        return 0
    if "--json" in sys.argv:
        OUT.write_text(json.dumps(inv, ensure_ascii=False, indent=1), encoding="utf-8")
        print("wrote %s" % OUT)
        return 0
    total = 0
    for name, d in sorted(inv.items(), key=lambda kv: -kv[1]["classes"]):
        orphan = [r["class"] for r in d["rows"] if not r["ownRule"] and not r["global"]]
        total += d["classes"]
        print("{:26} 元素 {:4}  类 {:3}  无样式定义 {}".format(
            name, d["elements"], d["classes"], len(orphan))
            + ("  <- " + ", ".join(orphan[:6]) if orphan else ""))
    print("\n合计类名 %d" % total)
    return 0


sys.exit(main())
