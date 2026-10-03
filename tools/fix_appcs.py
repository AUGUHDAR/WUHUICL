import io

p = r"D:\桌面\WUHUICL\wuhuicl-net\Wuhuicl.App\App.xaml.cs"
s = io.open(p, encoding="utf-8").read()

# 修复被转义破坏的诊断行：真实 TAB / 换行 已经进了字符串字面量
bad = '        System.IO.File.WriteAllText(@"D:' + chr(9) + 'mp' + chr(92) + 'doll_diag.txt", "frame1:' + chr(10) + '" + diag1 + "' + chr(10) + 'frame2:' + chr(10) + '" + diag2);'
good = ('        System.IO.File.WriteAllText(@"D:' + chr(92) + 'tmp' + chr(92) + 'doll_diag.txt", '
        '"frame1:" + diag1 + "  frame2:" + diag2);')
assert bad in s, "bad diag line not found"
s = s.replace(bad, good)

# 同样修复其余含反斜杠的路径字面量（若已被破坏成 TAB 也一并处理）
for name in ("doll_args.txt", "doll_test.png", "doll_test_far.png"):
    for variant in (
        '@"D:' + chr(9) + 'mp' + chr(92) + name + '"',
        '@"D:' + chr(92) + 'tmp' + chr(92) + name + '"',
        '"D:' + chr(9) + 'mp' + chr(92) + name + '"',
        '"D:' + chr(92) + 'tmp' + chr(92) + name + '"',
        '"D:' + chr(9) + 'mp' + name + '"',
    ):
        s = s.replace(variant, '@"D:' + chr(92) + 'tmp' + chr(92) + name + '"')

io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("ok")
