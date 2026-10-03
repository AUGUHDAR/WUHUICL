import io
p = r"D:\桌面\WUHUICL\wuhuicl-net\Wuhuicl.App\Controls\SkinDoll3D.cs"
s = io.open(p, encoding="utf-8").read()

old = '''        // 右臂：挂点 (-5-pivotX,-6)；左臂镜像（model.ts:177-206）
        skinGroup.Children.Add(Limb(Box(armW, 12, 4, 40, 16), Box(armW + 0.5, 12.5, 4.5, 40, 32), mat,
            new Point3D(-5 - pivotX, -6, 0), _rightArmRot));
        skinGroup.Children.Add(Limb(Box(armW, 12, 4, 32, 48), Box(armW + 0.5, 12.5, 4.5, 48, 48), mat,
            new Point3D(5 + pivotX, -6, 0), _leftArmRot));

        // 腿：挂点 (±1.9,-18,∓0.1)（model.ts:210-260）
        skinGroup.Children.Add(Limb(Box(4, 12, 4, 0, 16), Box(4.5, 12.5, 4.5, 0, 32), mat,
            new Point3D(-1.9, -18, -0.1), _rightLegRot));
        skinGroup.Children.Add(Limb(Box(4, 12, 4, 16, 48), Box(4.5, 12.5, 4.5, 0, 48), mat,
            new Point3D(1.9, -18, 0.1), _leftLegRot));'''

new = '''        // three.js 层级：limb 组在挂点（肩 ±5,-2 / 髋 ±1.9,-12），pivot 在组内 (±pivotX,-4)（臂）或 (0,-6)（腿），
        // mesh 以 pivot 为中心 → 绕「组原点=肩/髋」钟摆摆动。mesh 偏移必须留在组内，不能并进平移。
        skinGroup.Children.Add(Limb(Box(armW, 12, 4, 40, 16), Box(armW + 0.5, 12.5, 4.5, 40, 32), mat,
            new Point3D(-5, -2, 0), new Point3D(-pivotX, -4, 0), _rightArmRot));
        skinGroup.Children.Add(Limb(Box(armW, 12, 4, 32, 48), Box(armW + 0.5, 12.5, 4.5, 48, 48), mat,
            new Point3D(5, -2, 0), new Point3D(pivotX, -4, 0), _leftArmRot));
        skinGroup.Children.Add(Limb(Box(4, 12, 4, 0, 16), Box(4.5, 12.5, 4.5, 0, 32), mat,
            new Point3D(-1.9, -12, -0.1), new Point3D(0, -6, 0), _rightLegRot));
        skinGroup.Children.Add(Limb(Box(4, 12, 4, 16, 48), Box(4.5, 12.5, 4.5, 0, 48), mat,
            new Point3D(1.9, -12, 0.1), new Point3D(0, -6, 0), _leftLegRot));'''
assert s.count(old) == 1
s = s.replace(old, new)

old2 = '''    /// <summary>肢体：旋转组（挂点为轴心）内挂枢轴平移 + 内外两层 mesh（mesh 中心即枢轴）。</summary>
    static Model3DGroup Limb(MeshGeometry3D inner, MeshGeometry3D outer, Material mat,
        Point3D joint, RotateTransform3D rot)
    {
        // 行向量约定：p·M0·M1，先挂的先作用。摆动 = 绕挂点旋转（mesh 原点即挂点），
        // 再把旋转结果放到挂点位置 → Children 顺序 {旋转, 平移}。
        return new Model3DGroup
        {
            Transform = new Transform3DGroup
            {
                Children = { rot, new TranslateTransform3D(joint.X, joint.Y, joint.Z) }
            },
            Children = { Part(inner, mat, Transform3D.Identity), Part(outer, mat, Transform3D.Identity) }
        };
    }'''
new2 = '''    /// <summary>肢体：组 = {绕组原点旋转, 平移到挂点}，mesh 在组内偏移 pivot（three.js 的 pivot 层）。</summary>
    static Model3DGroup Limb(MeshGeometry3D inner, MeshGeometry3D outer, Material mat,
        Point3D joint, Point3D pivot, RotateTransform3D rot)
    {
        return new Model3DGroup
        {
            Transform = new Transform3DGroup
            {
                Children = { rot, new TranslateTransform3D(joint.X, joint.Y, joint.Z) }
            },
            Children =
            {
                Part(inner, mat, new TranslateTransform3D(pivot.X, pivot.Y, pivot.Z)),
                // 外套层比内层厚 0.5：three.js 的 2Box 直接是 12.5 高，中心仍在 pivot
                Part(outer, mat, new TranslateTransform3D(pivot.X, pivot.Y, pivot.Z)),
            }
        };
    }'''
assert s.count(old2) == 1
s = s.replace(old2, new2)
io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("ok")
