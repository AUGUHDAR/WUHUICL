import io

# ---------- MainWindow.xaml: 顶栏按钮命名 ----------
p = r"D:\桌面\WUHUICL\wuhuicl-net\Wuhuicl.App\MainWindow.xaml"
s = io.open(p, encoding="utf-8").read()
old = '''                            <Button Style="{StaticResource TopBtn}" Click="OpenDl_Click">'''
new = '''                            <!-- App.vue:1333 下载按钮 v-if currentView !== home；首页只有有下载任务时才出图标钮 -->
                            <Button x:Name="BtnTopDownload" Style="{StaticResource TopBtn}" Click="OpenDl_Click">'''
assert s.count(old) == 1
s = s.replace(old, new)
old2 = '''                            <Button Style="{StaticResource IconBtn}" Width="36" Margin="4,0,0,0"
                                    ToolTip="历史启动" Click="History_Click">'''
new2 = '''                            <!-- App.vue:1359 更新日志图标按钮只在首页显示 -->
                            <Button x:Name="BtnTopHistory" Style="{StaticResource IconBtn}" Width="36" Margin="4,0,0,0"
                                    ToolTip="历史启动" Click="History_Click">'''
assert s.count(old2) == 1
s = s.replace(old2, new2)
io.open(p, "w", encoding="utf-8", newline="\n").write(s)

# ---------- MainWindow.xaml.cs: Show 里切顶栏按钮显隐 ----------
p2 = r"D:\桌面\WUHUICL\wuhuicl-net\Wuhuicl.App\MainWindow.xaml.cs"
s2 = io.open(p2, encoding="utf-8").read()
old3 = '''        if (which == "home") RefreshHero();
    }'''
new3 = '''        // App.vue:1333/1359：非首页显示「下载」文字钮，首页显示「历史启动」图标钮
        bool home = which == "home";
        BtnTopDownload.Visibility = home ? Visibility.Collapsed : Visibility.Visible;
        BtnTopHistory.Visibility = home ? Visibility.Visible : Visibility.Collapsed;
        if (home) RefreshHero();
    }'''
assert s2.count(old3) == 1
s2 = s2.replace(old3, new3)
io.open(p2, "w", encoding="utf-8", newline="\n").write(s2)

# ---------- HomePage.xaml: 空态改居中纯文字 + 虚线（WPF Border 不支持 dash → Rectangle 画） ----------
p3 = r"D:\桌面\WUHUICL\wuhuicl-net\Wuhuicl.App\Pages\HomePage.xaml"
s3 = io.open(p3, encoding="utf-8").read()
old4 = '''                                    <!-- 空态：min-height 110，虚线描边，radius 13，整块可点 -->
                                    <Border x:Name="EmptyHint" MinHeight="110" CornerRadius="13"
                                            BorderBrush="{DynamicResource BorderStrong}" BorderThickness="1"
                                            Background="{DynamicResource CardSoft}" Cursor="Hand"
                                            Visibility="Collapsed">
                                        <Grid>
                                            <StackPanel Orientation="Horizontal" VerticalAlignment="Center"
                                                        Margin="16,0">
                                                <!-- 原版空态就是一行纯文字，不配图标 -->
                                                <TextBlock x:Name="EmptyHintText" FontSize="12.5"
                                                           VerticalAlignment="Center"
                                                           Foreground="{DynamicResource Fg2}"
                                                           Text="尚未安装游戏实例，点击前往版本管理"/>
                                            </StackPanel>
                                            <Button Style="{StaticResource BtnGold}" Content="去安装"
                                                    HorizontalAlignment="Right" VerticalAlignment="Center"
                                                    Margin="0,0,16,0"
                                                    Padding="16,6" Click="NavVersionsFromButton"/>
                                        </Grid>
                                    </Border>'''
new4 = '''                                    <!-- 空态 empty-instances（HomeView.vue:913）：min-height 110 / dashed 1px
                                         border-strong / radius 13 / card 底 / 居中纯文字，没有按钮 -->
                                    <Grid x:Name="EmptyHint" MinHeight="110" Cursor="Hand"
                                          Background="{DynamicResource Card}"
                                          Visibility="Collapsed" MouseLeftButtonDown="NavVersionsFromButton">
                                        <Rectangle RadiusX="13" RadiusY="13" StrokeThickness="1"
                                                   Stroke="{DynamicResource BorderStrong}"
                                                   StrokeDashArray="4 2" StrokeDashCap="Flat"/>
                                        <TextBlock x:Name="EmptyHintText" FontSize="12.5"
                                                   HorizontalAlignment="Center" VerticalAlignment="Center"
                                                   Foreground="{DynamicResource Fg2}"
                                                   Text="尚未安装游戏实例，点击前往版本管理"/>
                                    </Grid>'''
assert s3.count(old4) == 1
s3 = s3.replace(old4, new4)
io.open(p3, "w", encoding="utf-8", newline="\n").write(s3)

# ---------- HomePage.xaml: 离线状态字色（原版 .offline-state 是 text-dim）+ 头像外环 ----------
s3 = io.open(p3, encoding="utf-8").read()
old5 = '''                                                <TextBlock Text="{Binding SelectedAccount.KindText, FallbackValue=请选择账户}"
                                                           FontSize="11" Margin="7,0,0,0" VerticalAlignment="Center"
                                                           Foreground="{DynamicResource AccentHi}"/>'''
new5 = '''                                                <!-- .offline-state（未登录）字色 text-dim；已登录才是 accent-2（HomeView.vue:923-925） -->
                                                <TextBlock Text="{Binding SelectedAccount.KindText, FallbackValue=请选择账户}"
                                                           FontSize="11" Margin="7,0,0,0" VerticalAlignment="Center"
                                                           Foreground="{DynamicResource Fg2}"/>'''
assert s3.count(old5) == 1
s3 = s3.replace(old5, new5)

old6 = '''                                        <Border Width="54" Height="54" CornerRadius="12"
                                                Background="{DynamicResource CardSoft}">
                                            <Image x:Name="AvatarImg" Width="54" Height="54"
                                                   RenderOptions.BitmapScalingMode="NearestNeighbor"/>
                                        </Border>'''
new6 = '''                                        <Grid Width="62" Height="62">
                                            <!-- .mc-avatar 外环 box-shadow 0 0 0 4px text 8%（HomeView.vue:920） -->
                                            <Ellipse Fill="Transparent" Stroke="{DynamicResource Fg}" StrokeThickness="4"
                                                     Opacity="0.08"/>
                                            <Border Width="54" Height="54" CornerRadius="12"
                                                    Background="{DynamicResource CardSoft}"
                                                    HorizontalAlignment="Center" VerticalAlignment="Center">
                                                <Image x:Name="AvatarImg" Width="54" Height="54"
                                                       RenderOptions.BitmapScalingMode="NearestNeighbor"/>
                                            </Border>
                                        </Grid>'''
assert s3.count(old6) == 1
s3 = s3.replace(old6, new6)
io.open(p3, "w", encoding="utf-8", newline="\n").write(s3)
print("ok")
