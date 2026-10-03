import io

# ---------- Controls.xaml: ScopeTab / CatPill ----------
p = r"D:\桌面\WUHUICL\wuhuicl-net\Wuhuicl.App\Themes\Controls.xaml"
s = io.open(p, encoding="utf-8").read()

old = '''    <Style x:Key="ScopeTab" TargetType="RadioButton">
        <Setter Property="MinHeight" Value="38"/>
        <Setter Property="Padding" Value="18,8"/>
        <Setter Property="FontSize" Value="15"/>
        <Setter Property="FontWeight" Value="SemiBold"/>
        <Setter Property="Foreground" Value="{DynamicResource Fg2}"/>
        <Setter Property="Cursor" Value="Hand"/>
        <Setter Property="Template">
            <Setter.Value>
                <ControlTemplate TargetType="RadioButton">
                    <Border x:Name="b" Background="Transparent" CornerRadius="10"
                            Padding="{TemplateBinding Padding}">
                        <ContentPresenter HorizontalAlignment="Center" VerticalAlignment="Center"/>
                    </Border>
                    <ControlTemplate.Triggers>
                        <Trigger Property="IsMouseOver" Value="True">
                            <Setter Property="Foreground" Value="{DynamicResource Fg}"/>
                        </Trigger>
                        <Trigger Property="IsChecked" Value="True">
                            <Setter TargetName="b" Property="Background" Value="{DynamicResource AccentSoft}"/>
                            <!-- .settings-scopes [aria-current] 的字色是 accent-2（SettingsView.vue:1840） -->
                            <Setter Property="Foreground" Value="{DynamicResource AccentHi}"/>
                        </Trigger>
                    </ControlTemplate.Triggers>
                </ControlTemplate>
            </Setter.Value>
        </Setter>
    </Style>'''

new = '''    <!-- .settings-scopes .btn：38 高 15px/650 padding 8 18；未选中就是 .btn 默认
         （Card 底 + Line 描边），选中 [aria-current] 换 AccentSoft 底 + accent-2 字、
         边框透明（SettingsView.vue:1839-1840） -->
    <Style x:Key="ScopeTab" TargetType="RadioButton">
        <Setter Property="MinHeight" Value="38"/>
        <Setter Property="Padding" Value="18,8"/>
        <Setter Property="FontSize" Value="15"/>
        <Setter Property="FontWeight" Value="SemiBold"/>
        <Setter Property="Foreground" Value="{DynamicResource Fg2}"/>
        <Setter Property="Cursor" Value="Hand"/>
        <Setter Property="Template">
            <Setter.Value>
                <ControlTemplate TargetType="RadioButton">
                    <Border x:Name="b" Background="{DynamicResource Card}" CornerRadius="10"
                            BorderBrush="{DynamicResource Line}" BorderThickness="1"
                            Padding="{TemplateBinding Padding}">
                        <ContentPresenter HorizontalAlignment="Center" VerticalAlignment="Center"/>
                    </Border>
                    <ControlTemplate.Triggers>
                        <Trigger Property="IsMouseOver" Value="True">
                            <Setter Property="Foreground" Value="{DynamicResource Fg}"/>
                        </Trigger>
                        <Trigger Property="IsChecked" Value="True">
                            <Setter TargetName="b" Property="Background" Value="{DynamicResource AccentSoft}"/>
                            <Setter TargetName="b" Property="BorderBrush" Value="Transparent"/>
                            <!-- .settings-scopes [aria-current] 的字色是 accent-2（SettingsView.vue:1840） -->
                            <Setter Property="Foreground" Value="{DynamicResource AccentHi}"/>
                        </Trigger>
                    </ControlTemplate.Triggers>
                </ControlTemplate>
            </Setter.Value>
        </Setter>
    </Style>'''
assert s.count(old) == 1
s = s.replace(old, new)

old2 = '''                        <Trigger Property="IsChecked" Value="True">
                            <Setter TargetName="b" Property="Background" Value="{DynamicResource AccentSoft}"/>
                            <Setter Property="Foreground" Value="{DynamicResource Fg}"/>
                            <Setter Property="FontWeight" Value="SemiBold"/>
                        </Trigger>'''
new2 = '''                        <Trigger Property="IsChecked" Value="True">
                            <Setter TargetName="b" Property="Background" Value="{DynamicResource AccentSoft}"/>
                            <!-- .settings-categories [aria-current] 字色是 accent（SettingsView.vue:1241 作用域块） -->
                            <Setter Property="Foreground" Value="{DynamicResource Accent}"/>
                            <Setter Property="FontWeight" Value="SemiBold"/>
                        </Trigger>'''
assert s.count(old2) == 1, s.count(old2)
s = s.replace(old2, new2)
io.open(p, "w", encoding="utf-8", newline="\n").write(s)

# ---------- SettingsPage.xaml ----------
p2 = r"D:\桌面\WUHUICL\wuhuicl-net\Wuhuicl.App\Pages\SettingsPage.xaml"
s2 = io.open(p2, encoding="utf-8").read()

# (c) search box: drop the standalone clear button, input fills the column
old3 = '''        <!-- 行1列2：.settings-search flex gap 12，input flex:1（:1234） -->
        <Grid Grid.Row="0" Grid.Column="1" Margin="0,0,0,0">
            <Grid.ColumnDefinitions>
                <ColumnDefinition Width="*"/>
                <ColumnDefinition Width="12"/>
                <ColumnDefinition Width="Auto"/>
            </Grid.ColumnDefinitions>
            <Grid Grid.Column="0">
                <TextBox Style="{StaticResource Input}" FontSize="13"/>
                <StackPanel Orientation="Horizontal" Margin="12,0" VerticalAlignment="Center" IsHitTestVisible="False">
                    <ContentControl Width="14" Height="14" Foreground="{DynamicResource Fg3}"
                                    Style="{DynamicResource Icon.Search}"/>
                    <TextBlock Margin="8,0,0,0" FontSize="13" Foreground="{DynamicResource Fg3}" Text="搜索设置项…"/>
                </StackPanel>
            </Grid>
            <Button Grid.Column="2" Style="{StaticResource IconBtn}" Width="36" Height="36"
                    ToolTip="清除搜索">
                <ContentControl Width="16" Height="16" Style="{DynamicResource Icon.Close2}"/>
            </Button>
        </Grid>'''
new3 = '''        <!-- 行1列2：.settings-search 里只有 input（flex:1），原版没有独立清除按钮（:1234） -->
        <Grid Grid.Row="0" Grid.Column="1" Margin="0,0,0,0">
            <TextBox Style="{StaticResource Input}" FontSize="13"/>
            <StackPanel Orientation="Horizontal" Margin="12,0" VerticalAlignment="Center" IsHitTestVisible="False">
                <ContentControl Width="14" Height="14" Foreground="{DynamicResource Fg3}"
                                Style="{DynamicResource Icon.Search}"/>
                <TextBlock Margin="8,0,0,0" FontSize="13" Foreground="{DynamicResource Fg3}" Text="搜索设置项…"/>
            </StackPanel>
        </Grid>'''
assert s2.count(old3) == 1
s2 = s2.replace(old3, new3)
io.open(p2, "w", encoding="utf-8", newline="\n").write(s2)
print("ok")
