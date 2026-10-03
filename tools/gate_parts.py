import io
p = r"D:\桌面\WUHUICL\wuhuicl-net\Wuhuicl.App\Controls\SkinDoll3D.cs"
s = io.open(p, encoding="utf-8").read()

old = '''        // 头：mesh 中心 (0,4,0)（model.ts:139）
        skinGroup.Children.Add(Part(Box(8, 8, 8, 0, 0), mat, new TranslateTransform3D(0, 4, 0)));
        skinGroup.Children.Add(Part(Box(9, 9, 9, 32, 0), mat, new TranslateTransform3D(0, 4, 0)));
        // 身：中心 (0,-6,0)（:155）
        skinGroup.Children.Add(Part(Box(8, 12, 4, 16, 16), mat, new TranslateTransform3D(0, -6, 0)));
        skinGroup.Children.Add(Part(Box(8.5, 12.5, 4.5, 16, 32), mat, new TranslateTransform3D(0, -6, 0)));

        // three.js 层级：limb 组在挂点（肩 ±5,-2 / 髋 ±1.9,-12），pivot 在组内 (±pivotX,-4)（臂）或 (0,-6)（腿），
        // mesh 以 pivot 为中心 → 绕「组原点=肩/髋」钟摆摆动。mesh 偏移必须留在组内，不能并进平移。
        skinGroup.Children.Add(Limb(Box(armW, 12, 4, 40, 16), Box(armW + 0.5, 12.5, 4.5, 40, 32), mat,
            new Point3D(-5, -2, 0), new Point3D(-pivotX, -4, 0), _rightArmRot));
        skinGroup.Children.Add(Limb(Box(armW, 12, 4, 32, 48), Box(armW + 0.5, 12.5, 4.5, 48, 48), mat,
            new Point3D(5, -2, 0), new Point3D(pivotX, -4, 0), _leftArmRot));
        skinGroup.Children.Add(Limb(Box(4, 12, 4, 0, 16), Box(4.5, 12.5, 4.5, 0, 32), mat,
            new Point3D(-1.9, -12, -0.1), new Point3D(0, -6, 0), _rightLegRot));
        skinGroup.Children.Add(Limb(Box(4, 12, 4, 16, 48), Box(4.5, 12.5, 4.5, 0, 48), mat,
            new Point3D(1.9, -12, 0.1), new Point3D(0, -6, 0), _leftLegRot));'''

new = '''        // 排查开关：DOLL_PARTS=head/body/arms/legs 控制渲染到哪一层，缺省 all
        string parts = (Environment.GetEnvironmentVariable("DOLL_PARTS") ?? "all").ToLowerInvariant();

        // 头：mesh 中心 (0,4,0)（model.ts:139）
        if (parts is "all" or "head" or "body" or "arms" or "legs")
        {
            skinGroup.Children.Add(Part(Box(8, 8, 8, 0, 0), mat, new TranslateTransform3D(0, 4, 0)));
            skinGroup.Children.Add(Part(Box(9, 9, 9, 32, 0), mat, new TranslateTransform3D(0, 4, 0)));
        }
        // 身：中心 (0,-6,0)（:155）
        if (parts is "all" or "body" or "arms" or "legs")
        {
            skinGroup.Children.Add(Part(Box(8, 12, 4, 16, 16), mat, new TranslateTransform3D(0, -6, 0)));
            skinGroup.Children.Add(Part(Box(8.5, 12.5, 4.5, 16, 32), mat, new TranslateTransform3D(0, -6, 0)));
        }
        // three.js 层级：limb 组在挂点（肩 ±5,-2 / 髋 ±1.9,-12），pivot 在组内 (±pivotX,-4)（臂）或 (0,-6)（腿），
        // mesh 以 pivot 为中心 → 绕「组原点=肩/髋」钟摆摆动。mesh 偏移必须留在组内，不能并进平移。
        if (parts is "all" or "arms")
        {
            skinGroup.Children.Add(Limb(Box(armW, 12, 4, 40, 16), Box(armW + 0.5, 12.5, 4.5, 40, 32), mat,
                new Point3D(-5, -2, 0), new Point3D(-pivotX, -4, 0), _rightArmRot));
            skinGroup.Children.Add(Limb(Box(armW, 12, 4, 32, 48), Box(armW + 0.5, 12.5, 4.5, 48, 48), mat,
                new Point3D(5, -2, 0), new Point3D(pivotX, -4, 0), _leftArmRot));
        }
        if (parts is "all" or "legs")
        {
            skinGroup.Children.Add(Limb(Box(4, 12, 4, 0, 16), Box(4.5, 12.5, 4.5, 0, 32), mat,
                new Point3D(-1.9, -12, -0.1), new Point3D(0, -6, 0), _rightLegRot));
            skinGroup.Children.Add(Limb(Box(4, 12, 4, 16, 48), Box(4.5, 12.5, 4.5, 0, 48), mat,
                new Point3D(1.9, -12, 0.1), new Point3D(0, -6, 0), _leftLegRot));
        }'''
assert s.count(old) == 1
s = s.replace(old, new)
io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("ok")
