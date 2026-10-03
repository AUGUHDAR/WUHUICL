import io

# 1) App.xaml.cs: 去掉双 @
p = r"D:\桌面\WUHUICL\wuhuicl-net\Wuhuicl.App\App.xaml.cs"
s = io.open(p, encoding="utf-8").read()
s = s.replace('@@"', '@"')
io.open(p, "w", encoding="utf-8", newline="\n").write(s)

# 2) SkinDoll3D.cs: ApplyFrame 重构时的停表逻辑简化回来
p2 = r"D:\桌面\WUHUICL\wuhuicl-net\Wuhuicl.App\Controls\SkinDoll3D.cs"
s2 = io.open(p2, encoding="utf-8").read()
old = '''        if (settled && !_clock.IsEnabled) _clock.Stop();
        else if (settled) { /* 由真正的 ClockTick（带 sender）停表 */ }
    }'''
new = '''        if (settled) _clock.Stop();
    }'''
assert s2.count(old) == 1
s2 = s2.replace(old, new)
# ClockTick 现在只是转发；停表放在 ApplyFrame 里（带 sender 语义已不需要）
io.open(p2, "w", encoding="utf-8", newline="\n").write(s2)
print("ok")
