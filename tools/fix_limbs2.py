import io
p = r"D:\桌面\WUHUICL\wuhuicl-net\Wuhuicl.App\Controls\SkinDoll3D.cs"
s = io.open(p, encoding="utf-8").read()

# 在 Rebuild 的四肢添加前加开关：DOLL_PARTS 环境变量控制渲染到哪一层（head/body/arms/legs/all）
old = '''        // three.js 层级：limb 组在挂点（肩 ±5,-2 / 髋 ±1.9,-12），pivot 在组内 (±pivotX,-4)（臂）或 (0,-6)（腿），
        // mesh 偏移必须留在组内，不能并进平移。'''
new = '''        // three.js 层级：limb 组在挂点（肩 ±5,-2 / 髋 ±1.9,-12），pivot 在组内 (±pivotX,-4)（臂）或 (0,-6)（腿），
        // mesh 偏移必须留在组内，不能并进平移。
        // 排查开关：DOLL_PARTS=head 只渲染头；body=头+身；全缺省=全部'''
assert s.count(old) == 1
s = s.replace(old, new)
io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("ok")
