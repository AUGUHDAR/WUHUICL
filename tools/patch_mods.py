import io
import re

p = r"D:\桌面\WUHUICL\wuhuicl-net\Wuhuicl.App\Pages\ModsPage.xaml"
s = io.open(p, encoding="utf-8").read()

# ---------- (1) head: mods 行内布局（标题左 / 按钮右），资源页保持列流 ----------
old_head = '''            <StackPanel Margin="0,0,0,12">
                <!-- .fm-head-left{flex:none;width:100%} — :509；.page-title 24px/Bold — ui-system.css:12（契约 §2 PageTitle） -->
                <StackPanel Orientation="Horizontal">
                    <TextBlock x:Name="HeadTitle" Style="{StaticResource PageTitle}" Text="模组"/>
                    <!-- .fm-head .page-title small：13px / 400 / text-dim / margin-left 8 — :719。// 占位：等资源管理接入（resourceCount） -->
                    <TextBlock FontSize="13" Foreground="{DynamicResource Fg2}" Margin="8,0,0,0"
                               VerticalAlignment="Bottom" Text="4"/>
                </StackPanel>

                <!-- .fm-actions：flex row / gap 12（space-3 变量）/ flex-wrap / width 100% — :533-539 + :510 -->
                <WrapPanel Margin="0,16,0,0">'''
new_head = '''            <!-- .fm-head：mods 是 row（align-items:flex-end + justify-between，FileManager.vue:507-517），
                 资源包/光影的 .resource-page 才切列流（:506-510）。行/列由 ApplyResourceKind 搬 FmActions。 -->
            <Grid Margin="0,0,0,12">
                <Grid.RowDefinitions>
                    <RowDefinition Height="Auto"/>
                    <RowDefinition Height="Auto"/>
                </Grid.RowDefinitions>
                <Grid.ColumnDefinitions>
                    <ColumnDefinition Width="Auto"/>
                    <ColumnDefinition Width="*"/>
                </Grid.ColumnDefinitions>
                <!-- .fm-head-left{flex:1;min-width:140px}；.page-title 24px/Bold — ui-system.css:12（契约 §2 PageTitle） -->
                <StackPanel Grid.Row="0" Grid.Column="0" Orientation="Horizontal">
                    <TextBlock x:Name="HeadTitle" Style="{StaticResource PageTitle}" Text="模组"/>
                    <!-- .fm-head .page-title small：13px / 400 / text-dim / margin-left 8 — :719。// 占位：等资源管理接入（resourceCount） -->
                    <TextBlock FontSize="13" Foreground="{DynamicResource Fg2}" Margin="8,0,0,0"
                               VerticalAlignment="Bottom" Text="4"/>
                </StackPanel>

                <!-- .fm-actions：flex row / gap 12（space-3 变量）/ flex-wrap — :533-539 -->
                <WrapPanel x:Name="FmActions" Grid.Row="0" Grid.Column="1"
                           HorizontalAlignment="Right" VerticalAlignment="Bottom">'''
assert s.count(old_head) == 1
s = s.replace(old_head, new_head)

old_tail = '''                </WrapPanel>
            </StackPanel>

            <!-- ==================== .fm-context ===================='''
new_tail = '''                </WrapPanel>
            </Grid>

            <!-- ==================== .fm-context ===================='''
assert s.count(old_tail) == 1
s = s.replace(old_tail, new_tail)

# ---------- (2) 实例选择器：SelectMenu 两行卡片 ----------
old_sel = '''                    <!-- :deep(.fm-ver-select)：max-width 240 / min-width 180 — :540-543
                         （:719 在 .fm-context 内改成 width:100%/max-width:none，本轮按任务口径留 240 上限）；
                         36 高 / radius 10 由 ComboBox 默认样式给（契约 §2 Input 同源度量，docs §0 styles.css:304-317） -->
                    <ComboBox x:Name="VerSelect" Margin="0,6,0,0" MinWidth="180" MaxWidth="240"
                              HorizontalAlignment="Left" FontSize="13" SelectedIndex="0">
                        <!-- 占位：等版本列表接入（原版来自 activeInstalled，:48） -->
                        <ComboBoxItem>1.20.1 · Fabric 0.16.4 · 已隔离</ComboBoxItem>
                        <ComboBoxItem>1.21.4 · NeoForge 21.4.60 · 共享</ComboBoxItem>
                        <ComboBoxItem>1.8.9 · Forge 11.15.1 · 共享</ComboBoxItem>
                    </ComboBox>'''
new_sel = '''                    <!-- .fm-ver-select：SelectMenu 两行卡片，宽度撑满 440 的 label 轨
                         （label flex 0 1 440px — :719；截图量得卡宽 ≈436）。label 13px/600，
                         description 11px text-dim，右缘 14px chevron；radius 10 描边。 -->
                    <Button x:Name="VerSelect" Margin="0,6,0,0" HorizontalAlignment="Stretch"
                            HorizontalContentAlignment="Stretch" Cursor="Hand">
                        <Button.Template>
                            <ControlTemplate TargetType="Button">
                                <Border Background="{DynamicResource Card}" CornerRadius="10"
                                        BorderBrush="{DynamicResource Line}" BorderThickness="1" Padding="12,7">
                                    <Grid>
                                        <Grid.ColumnDefinitions>
                                            <ColumnDefinition Width="*"/>
                                            <ColumnDefinition Width="12"/>
                                            <ColumnDefinition Width="14"/>
                                        </Grid.ColumnDefinitions>
                                        <StackPanel>
                                            <TextBlock x:Name="VerName" FontSize="13" FontWeight="SemiBold"
                                                       TextTrimming="CharacterEllipsis" Text="1.20.1 · Fabric 0.16.4"/>
                                            <TextBlock x:Name="VerDesc" FontSize="11" Foreground="{DynamicResource Fg2}"
                                                       Margin="0,2,0,0" TextTrimming="CharacterEllipsis"
                                                       Text="1.20.1 · Fabric · 0.16.4 · 已隔离"/>
                                        </StackPanel>
                                        <ContentControl Grid.Column="2" Width="14" Height="14" VerticalAlignment="Center"
                                                        Style="{DynamicResource Icon.ChevronDown}"/>
                                    </Grid>
                                </Border>
                            </ControlTemplate>
                        </Button.Template>
                        <!-- 占位：等版本列表接入（原版来自 activeInstalled，:48） -->
                    </Button>'''
assert s.count(old_sel) == 1
s = s.replace(old_sel, new_sel)

# ---------- (3) 批量条默认隐藏（v-if selection.size，:407） ----------
old_batch = '''            <Border Style="{StaticResource Panel}" Padding="12,16" Margin="0,0,0,16">
                <WrapPanel>
                    <!-- 占位：等批量选择接入（原版是 已选 {{selection.size}} 项，:407） -->'''
new_batch = '''            <Border Style="{StaticResource Panel}" Padding="12,16" Margin="0,0,0,16"
                    Visibility="Collapsed">
                <WrapPanel>
                    <!-- v-if selection.size（:407）默认没有选中项 → 整条隐藏；占位：等批量选择接入 -->'''
assert s.count(old_batch) == 1
s = s.replace(old_batch, new_batch)

# ---------- (4) 分页默认隐藏（v-if pageCount>1，:453） ----------
old_pg = '''            <Grid Margin="0,0,0,12">
                <Grid.ColumnDefinitions>
                    <ColumnDefinition Width="*"/><ColumnDefinition Width="Auto"/>
                </Grid.ColumnDefinitions>'''
new_pg = '''            <Grid Margin="0,0,0,12" Visibility="Collapsed">
                <!-- v-if pageCount>1（:453），单页时整条不渲染 -->
                <Grid.ColumnDefinitions>
                    <ColumnDefinition Width="*"/><ColumnDefinition Width="Auto"/>
                </Grid.ColumnDefinitions>'''
assert s.count(old_pg) == 1
s = s.replace(old_pg, new_pg)

# ---------- (5) 行内主行改文件名，状态进第二行 / 胶囊移除（mods 7 列无状态轨，:441） ----------
pairs = [
    ('<TextBlock Style="{StaticResource FmName}" Text="Fabric API"/>',
     '<TextBlock Style="{StaticResource FmName}" Text="fabric-api-0.98.0+1.20.1.jar"/>'),
    ('<TextBlock Style="{StaticResource FmName}" Text="Sodium 渲染优化"/>',
     '<TextBlock Style="{StaticResource FmName}" Text="sodium-fabric-0.5.8+mc1.20.1.jar"/>'),
    ('''<TextBlock Style="{StaticResource FmName}" TextDecorations="Strikethrough"
                                               Text="OptiFine HD U I6"/>''',
     '<TextBlock Style="{StaticResource FmName}" Text="OptiFine_1.20.1_HD_U_I6.jar"/>'),
    ('<TextBlock Style="{StaticResource FmName}" Text="Iris 光影加载器"/>',
     '<TextBlock Style="{StaticResource FmName}" Text="iris-fabric-1.7.0+1.20.1.jar"/>'),
    ('<TextBlock Style="{StaticResource FmInternal}" Text="OptiFine_1.20.1_HD_U_I6.jar"/>',
     '<TextBlock Style="{StaticResource FmInternal}" Text="已禁用"/>'),
    ('<TextBlock Style="{StaticResource FmInternal}" Text="iris-fabric-1.7.0+1.20.1.jar"/>',
     '<TextBlock Style="{StaticResource FmInternal}" Text="已锁定"/>'),
]
for old, new in pairs:
    assert s.count(old) == 1, old[:60]
    s = s.replace(old, new)

chip_pat = re.compile(
    r'\n\s*<Border Grid\.Column="1" Style="\{StaticResource Tag\}" Background="\{DynamicResource CardSoft\}"\s*\n'
    r'\s*BorderBrush="\{DynamicResource Line\}" BorderThickness="1" Margin="8,0,0,0">\s*\n'
    r'\s*<TextBlock Text="[^"]*" FontSize="12" Foreground="\{DynamicResource Fg3\}"/>\s*\n'
    r'\s*</Border>')
s, n = chip_pat.subn('', s)
assert n == 4, n

io.open(p, "w", encoding="utf-8", newline="\n").write(s)

# ---------- code-behind: head 行/列切换 ----------
p2 = r"D:\桌面\WUHUICL\wuhuicl-net\Wuhuicl.App\Pages\ModsPage.xaml.cs"
s2 = io.open(p2, encoding="utf-8").read()
old_cs = '''    void ApplyResourceKind()
    {
        bool mods = resourceKind == "mods";'''
new_cs = '''    void ApplyResourceKind()
    {
        bool mods = resourceKind == "mods";
        // .resource-page 的 .fm-head 切列流（FileManager.vue:506-510）：按钮行换行并占满
        if (FmActions != null)
        {
            Grid.SetRow(FmActions, mods ? 0 : 1);
            Grid.SetColumn(FmActions, mods ? 1 : 0);
            Grid.SetColumnSpan(FmActions, mods ? 1 : 2);
            FmActions.HorizontalAlignment = mods ? HorizontalAlignment.Right : HorizontalAlignment.Left;
            FmActions.Margin = new Thickness(0, mods ? 0 : 16, 0, 0);
        }'''
assert s2.count(old_cs) == 1
s2 = s2.replace(old_cs, new_cs)
io.open(p2, "w", encoding="utf-8", newline="\n").write(s2)
print("ok")
