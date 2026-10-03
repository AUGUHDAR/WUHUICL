import io
p = r"D:\桌面\WUHUICL\wuhuicl-net\Wuhuicl.App\Pages\SkinsPage.xaml"
s = io.open(p, encoding="utf-8").read()

# 1) xmlns 引入 ctl
old = '<UserControl x:Class="Wuhuicl.App.Pages.SkinsPage"\n             xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"\n             xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml">'
new = '<UserControl x:Class="Wuhuicl.App.Pages.SkinsPage"\n             xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"\n             xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"\n             xmlns:ctl="clr-namespace:Wuhuicl.App.Controls">'
assert s.count(old) == 1
s = s.replace(old, new)

# 2) 纸娃娃位图 → 真 3D；loading 覆盖条收起
old2 = '''                        <Image x:Name="SkinPreviewImg" Stretch="Uniform" Margin="14"
                               RenderOptions.BitmapScalingMode="NearestNeighbor"/>'''
new2 = '''                        <!-- SkinViewer3D 的 WPF 等价（SkinDoll3D.cs）：拖拽 yaw/pitch、滚轮缩放、双击回正，
                             行走/待机切 Walk；几何/相机/动画参数 1:1 抄 skinview3d v3.4.2 + SkinViewer3D.vue -->
                        <Border CornerRadius="10" ClipToBounds="True" Margin="12">
                            <ctl:SkinDoll3D x:Name="SkinDoll3D"/>
                        </Border>'''
assert s.count(old2) == 1
s = s.replace(old2, new2)

old3 = '''                        <Border VerticalAlignment="Bottom" HorizontalAlignment="Stretch" Margin="12"
                                MinHeight="34" Padding="10,7" CornerRadius="9"
                                Background="{DynamicResource Card}"
                                BorderBrush="{DynamicResource Line}" BorderThickness="1">
                            <TextBlock Text="正在加载皮肤…" FontSize="11" FontWeight="Medium"
                                       Foreground="{DynamicResource Fg2}"
                                       HorizontalAlignment="Center" VerticalAlignment="Center"/>
                        </Border>'''
new3 = '''                        <!-- .skin-overlay：原版只在 loading / 未登录 / 出错三态出现（HomeView.vue:649-651），
                             本页皮肤已就绪 → 常收起；结构保留等接入时切 Visible -->
                        <Border VerticalAlignment="Bottom" HorizontalAlignment="Stretch" Margin="12"
                                MinHeight="34" Padding="10,7" CornerRadius="9"
                                Background="{DynamicResource Card}"
                                BorderBrush="{DynamicResource Line}" BorderThickness="1"
                                Visibility="Collapsed">
                            <TextBlock Text="正在加载皮肤…" FontSize="11" FontWeight="Medium"
                                       Foreground="{DynamicResource Fg2}"
                                       HorizontalAlignment="Center" VerticalAlignment="Center"/>
                        </Border>'''
assert s.count(old3) == 1
s = s.replace(old3, new3)
io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("xaml ok")
