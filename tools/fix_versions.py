import io
p = r"D:\桌面\WUHUICL\wuhuicl-net\Wuhuicl.App\Pages\VersionsPage.xaml"
s = io.open(p, encoding="utf-8").read()

# ---------- 文件夹卡：单行（label 12px dim + select + path + 管理文件夹右对齐） ----------
old = '''            <!-- .folder-manager：padding 12 16 / gap 8 / border 0（:2171） -->
            <Border Background="{DynamicResource Card}" CornerRadius="14" Padding="16,12" Margin="0,16,0,0">
                <StackPanel>
                    <!-- .folder-manager-main：row align-center gap 16（:2172） -->
                    <Grid>
                        <Grid.ColumnDefinitions>
                            <ColumnDefinition Width="Auto"/>
                            <ColumnDefinition Width="16"/>
                            <ColumnDefinition MinWidth="160" MaxWidth="320" Width="260"/>
                            <ColumnDefinition Width="12"/>
                            <ColumnDefinition Width="*"/>
                        </Grid.ColumnDefinitions>
                        <StackPanel VerticalAlignment="Center">
                            <TextBlock Style="{StaticResource Micro}" Text="游戏目录"/>
                            <TextBlock Style="{StaticResource Lbl14}" Margin="0,2,0,0" Text="当前文件夹"/>
                        </StackPanel>
                        <ComboBox Grid.Column="2" SelectedIndex="0" VerticalAlignment="Center">
                            <ComboBoxItem>.minecraft（启动器托管）</ComboBoxItem>
                            <ComboBoxItem>D:/Games/minecraft</ComboBoxItem>
                        </ComboBox>
                        <!-- .folder-current-path：mono 12px 单行截断（:1545-1551） -->
                        <TextBlock Grid.Column="4" Text="{Binding Paths.Root}" FontFamily="{StaticResource MonoFont}"
                                   FontSize="12" Foreground="{DynamicResource Fg2}" VerticalAlignment="Center"
                                   TextTrimming="CharacterEllipsis"/>
                    </Grid>
                    <!-- .folder-manager-actions：gap 8 + 顶边线（:2176） -->
                    <Border BorderBrush="{DynamicResource Line}" BorderThickness="0,1,0,0" Padding="0,8,0,0"
                            Margin="0,8,0,0">
                        <StackPanel Orientation="Horizontal">
                            <Button Style="{StaticResource BtnSm}" Content="添加目录"
                                    Background="{DynamicResource CardSoft}"/>
                            <Button Style="{StaticResource BtnSm}" Content="打开目录" Margin="8,0,0,0"
                                    Background="{DynamicResource CardSoft}"/>
                            <Button Style="{StaticResource BtnSm}" Content="迁移存档" Margin="8,0,0,0"
                                    Background="{DynamicResource CardSoft}"/>
                        </StackPanel>
                    </Border>
                </StackPanel>
            </Border>'''

new = '''            <!-- .folder-manager：padding 12 16（:2171）；主行 grid auto minmax(160,320) minmax(60,1fr)（:2173），
                 单行：label 12px dim + select + 路径 mono 12 + 管理文件夹（btn-ghost btn-sm）贴右。
                 原版没有第二行按钮——目录管理动作收进「管理文件夹」的浮层（对应 InstanceCenter 入口） -->
            <Border Background="{DynamicResource Card}" CornerRadius="14" Padding="16,12" Margin="0,16,0,0">
                <Grid>
                    <Grid.ColumnDefinitions>
                        <ColumnDefinition Width="Auto"/>
                        <ColumnDefinition Width="12"/>
                        <ColumnDefinition MinWidth="160" MaxWidth="320" Width="260"/>
                        <ColumnDefinition Width="12"/>
                        <ColumnDefinition Width="*"/>
                        <ColumnDefinition Width="12"/>
                        <ColumnDefinition Width="Auto"/>
                    </Grid.ColumnDefinitions>
                    <TextBlock Grid.Column="0" Style="{StaticResource Micro}" Text="新版本安装位置"
                               VerticalAlignment="Center"/>
                    <ComboBox Grid.Column="2" SelectedIndex="0" VerticalAlignment="Center">
                        <ComboBoxItem>.minecraft（启动器托管）</ComboBoxItem>
                        <ComboBoxItem>D:/Games/minecraft</ComboBoxItem>
                    </ComboBox>
                    <!-- .folder-current-path：mono 12px 单行截断（:1545-1551） -->
                    <TextBlock Grid.Column="4" Text="{Binding Paths.Root}" FontFamily="{StaticResource MonoFont}"
                               FontSize="12" Foreground="{DynamicResource Fg2}" VerticalAlignment="Center"
                               TextTrimming="CharacterEllipsis"/>
                    <Grid Grid.Column="6">
                        <Button x:Name="BtnFolderMore" Style="{StaticResource BtnGhost}" MinHeight="28"
                                Padding="12,4" FontSize="13" Content="管理文件夹" Click="FolderMore_Click"/>
                        <Popup x:Name="PopFolderMore" PlacementTarget="{x:Reference BtnFolderMore}"
                               Placement="Bottom" AllowsTransparency="True" Focusable="False" StaysOpen="False">
                            <Border Style="{StaticResource PopupCard}" MinWidth="150" Padding="10">
                                <StackPanel>
                                    <Button Style="{StaticResource MenuItem2}" HorizontalAlignment="Stretch" Content="添加目录"/>
                                    <Button Style="{StaticResource MenuItem2}" HorizontalAlignment="Stretch"
                                            Content="打开目录" Margin="0,8,0,0"/>
                                    <Button Style="{StaticResource MenuItem2}" HorizontalAlignment="Stretch"
                                            Content="迁移存档" Margin="0,8,0,0"/>
                                </StackPanel>
                            </Border>
                        </Popup>
                    </Grid>
                </Grid>
            </Border>'''
assert s.count(old) == 1
s = s.replace(old, new)

# ---------- 已安装行：管理 + 启动 + 更多，最近游玩 ----------
old2 = '''                                        <StackPanel Grid.Column="6" Orientation="Horizontal" VerticalAlignment="Center">
                                            <!-- .installed-launch：min-width 78（:2198） -->
                                            <Button Style="{StaticResource BtnGold}" Content="启动"
                                                    MinWidth="78" Padding="12,4" Tag="{Binding}"
                                                    Click="LaunchRow_Click"/>
                                        </StackPanel>'''
new2 = '''                                        <StackPanel Grid.Column="6" VerticalAlignment="Center">
                                            <!-- 右侧按钮组 gap 8（:2196）：管理 + ▶启动（min-width 78，:2198） -->
                                            <StackPanel Orientation="Horizontal">
                                                <Button Style="{StaticResource BtnGhost}" MinHeight="28"
                                                        Padding="12,4" FontSize="13" Content="管理"
                                                        Click="RowMore_Click" Tag="{Binding}"/>
                                                <Button Style="{StaticResource BtnGold}" Content="启动"
                                                        MinWidth="78" Padding="12,4" Margin="8,0,0,0" Tag="{Binding}"
                                                        Click="LaunchRow_Click"/>
                                            </StackPanel>
                                            <!-- .played-text：12px dim（:1926） -->
                                            <TextBlock Text="最近游玩: —" FontSize="12"
                                                       Foreground="{DynamicResource Fg2}" Margin="0,4,0,0"
                                                       HorizontalAlignment="Right"/>
                                        </StackPanel>'''
assert s.count(old2) == 1
s = s.replace(old2, new2)
io.open(p, "w", encoding="utf-8", newline="\n").write(s)

# ---------- code-behind: FolderMore_Click ----------
p2 = r"D:\桌面\WUHUICL\wuhuicl-net\Wuhuicl.App\Pages\VersionsPage.xaml.cs"
s2 = io.open(p2, encoding="utf-8").read()
add = '''

    void FolderMore_Click(object sender, RoutedEventArgs e) => PopFolderMore.IsOpen = !PopFolderMore.IsOpen;
'''
s2 = s2.rstrip() + add
io.open(p2, "w", encoding="utf-8", newline="\n").write(s2)
print("ok")
