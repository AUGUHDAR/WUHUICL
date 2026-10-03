"""查某个原版 class 的证据：模板里挂在什么元素上（行号）＋ 全文件所有规则（行号）。
用法：python tools/cls.py SkinsView.vue drag-hint [more classes...] [--base=1.1.10]
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
# 默认对齐 1.1.10（2026-10-03 换的基线）；要看旧版：--base=1.1.5
_base = next((a[7:] for a in sys.argv[1:] if a.startswith("--base=")), "1.1.10")
SRC = ROOT.parent / f"KAMUCL-{_base}" / "src" / "renderer" / "src"


def find(rel):
    p = SRC / rel
    if not p.exists():
        for cand in SRC.rglob(rel):
            p = cand
            break
    return p


def main():
    rel = sys.argv[1]
    want = sys.argv[2:]
    text = find(rel).read_text(encoding="utf-8")
    lines = text.split("\n")
    for i, line in enumerate(lines, 1):
        for cls in want:
            if re.search(r"class=\"[^\"]*\b%s\b" % re.escape(cls), line) or \
               re.search(r":class=\"[^\"]*\b%s\b" % re.escape(cls), line):
                frag = line.strip()
                m = re.search(r"<[a-zA-Z][^>]{0,200}%s[^>]*>" % re.escape(cls), frag)
                print("TPL %5d  %s" % (i, (m.group(0) if m else frag)[:200]))
    print("--- rules ---")
    pat = re.compile(r"([^{};]+)\{([^{}]*)\}")
    for m in pat.finditer(text):
        raw, body = m.group(1), m.group(2).strip()
        sel = " ".join(raw.split())
        if re.search(r"\.(%s)\b" % "|".join(re.escape(c) for c in want), sel):
            # 行号要算到选择器第一个非空白字符：group(1) 以「上一条 }」后面的换行开头，
            # 直接按 m.start() 数会整体偏小 1（子 Agent 就是靠这个偏差发现问题的）
            lead = len(raw) - len(raw.lstrip())
            ln = text[:m.start(1) + lead].count("\n") + 1
            print("CSS %5d  %s { %s }" % (ln, sel[:110], " ".join(body.split())[:300]))


main()
