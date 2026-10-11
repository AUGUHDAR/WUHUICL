using System.Globalization;
using System.IO;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using Wuhuicl.App.Services;
using Wuhuicl.Core.Minecraft;

namespace Wuhuicl.App.Pages;

/// <summary>
/// 默认配置页，复刻 KeysView.vue + DefaultGameOptions.vue，已接真实后端：
/// · 游戏选项（DefaultGameOptions）：root 页 = 子页入口网格；子页行 = 数值输入 / 循环切换 / 下拉，
///   改动即存 default-game-options.json，「跟随游戏」＝存 null；兼容性预览按版本 since/until 折行；
/// · 按键配置（DefaultKeys）：VANILLA_KEYBINDS 全表按分类两列铺开，点键位进入捕获（页面级
///   PreviewKeyDown/PreviewMouseDown，Esc 取消，× 置空 key.keyboard.unknown），单行/全部恢复默认；
/// · 默认材质包（DefaultResourcePacks）：导入（按钮 + 拖放，校验 pack.mcmeta）、启停、↑↓ 优先级、移除。
/// 三个「同步」开关落 settings.json（keySync / resourcePackSync / 游戏选项 enabled），
/// 启动时由 LaunchSession.PlayAsync 按 1.13+/1.6+ 版本门同步进实例 options.txt。
/// </summary>
public partial class KeysPage : UserControl
{
    LaunchSession State => (LaunchSession)DataContext;

    // ---------------- 页面状态 ----------------
    Dictionary<string, string> _keys = [];
    List<DefaultResourcePacks.Pack> _packs = [];
    DefaultGameOptions.GameOptionsState _opts = new();
    string _optPage = "root";
    string _capturing = "";
    bool _loaded;
    bool _busy;

    public KeysPage()
    {
        InitializeComponent();
        // 原版 section 的初值就是 'game' — KeysView.vue:15
        ShowSection(SectionGame);
        IsVisibleChanged += (_, e) =>
        {
            if ((bool)e.NewValue && !_loaded) { _loaded = true; _ = ReloadAsync(); }
        };
        // 捕获：页面级命中即吞（原版在 window 上挂捕获监听，:70-99）
        PreviewKeyDown += OnCaptureKey;
        PreviewMouseDown += OnCaptureMouse;
        // 材质包拖放（模板 :156 的 @dragover/@dragleave/@drop）
        PanelPacks.AllowDrop = true;
        PanelPacks.DragOver += Packs_DragOver;
        PanelPacks.DragLeave += (_, _) => PacksDragActive.Visibility = Visibility.Collapsed;
        PanelPacks.Drop += Packs_Drop;
        Unloaded += (_, _) => StopCapture();
    }

    // ================= 数据装载（onMounted — KeysView.vue:143-152）=================

    async Task ReloadAsync()
    {
        CardLoading.Visibility = Visibility.Visible;
        try
        {
            _keys = await Task.Run(() => DefaultKeys.GetDefaultKeys(State.Paths));
            _packs = await Task.Run(() => DefaultResourcePacks.Get(State.Paths));
            _opts = await Task.Run(() => DefaultGameOptions.Get(State.Paths));
        }
        catch (Exception ex) { State.Status = "读取默认配置失败：" + ex.Message; }
        finally { CardLoading.Visibility = Visibility.Collapsed; }

        SwKeySync.IsChecked = State.Settings.Value.KeySync;
        SwPackSync.IsChecked = State.Settings.Value.ResourcePackSync;
        SwOptionSync.IsChecked = _opts.Enabled;

        try
        {
            BuildPreviewVersions();
            RenderNavCounts();
            RenderOptions();
            RenderKeys();
            RenderPacks();
        }
        catch (Exception ex)
        {
            MainWindow.Diag("keys-reload EX: " + ex);
        }
    }

    /// <summary>versions 计算属性（DefaultGameOptions.vue:10）：26.2 + 已装实例的 MC 版本 + 固定四个锚点。</summary>
    void BuildPreviewVersions()
    {
        var list = new List<string> { "26.2" };
        foreach (var v in State.InstalledList)
        {
            var mc = ExtractMcVersion(v.Id);
            if (mc is not null && !list.Contains(mc)) list.Add(mc);
        }
        foreach (var anchor in new[] { "1.21.11", "1.20.1", "1.16.5", "1.12.2" })
            if (!list.Contains(anchor)) list.Add(anchor);
        CboPreviewVersion.Items.Clear();
        foreach (var v in list) CboPreviewVersion.Items.Add("Minecraft " + v);
        CboPreviewVersion.SelectedIndex = 0;
    }

    /// <summary>从实例 id 里抠 MC 版本（fabric-1.20.1 → 1.20.1；快照 id 原样返回）。</summary>
    static string? ExtractMcVersion(string id)
    {
        var m = System.Text.RegularExpressions.Regex.Match(id, @"\d+\.\d+(\.\d+)?");
        return m.Success ? m.Value : (DefaultKeys.ParseSnapshotId(id) is not null ? id : null);
    }

    string PreviewVersion => CboPreviewVersion.SelectedIndex >= 0
        ? (CboPreviewVersion.SelectedItem as string)?.Replace("Minecraft ", "") ?? "26.2" : "26.2";

    void RenderNavCounts()
    {
        int modified = DefaultKeys.VanillaKeybinds.Count(d => CurrentBind(d) != d.DefaultBind);
        KeysCount.Text = $"{modified} 项自定义绑定";
        PacksCount.Text = $"{_packs.Count} 个材质包";
        OptionsCustomCount.Text = $"{_opts.Values.Count} 项自定义";
        BtnResetAll.IsEnabled = modified > 0;   // :disabled="!keyModifiedCount" — :189
    }

    string CurrentBind(DefaultKeys.KeybindDef d) => _keys.GetValueOrDefault(d.Id, d.DefaultBind);

    // ================= 分区导航（selectSection — :16）=================

    void Section_Click(object sender, RoutedEventArgs e)
    {
        if (_capturing.Length > 0) StopCapture();   // selectSection 先 stopCapture — :17
        if (ReferenceEquals(sender, SectionKeys)) ShowSection(SectionKeys);
        else if (ReferenceEquals(sender, SectionPacks)) ShowSection(SectionPacks);
        else ShowSection(SectionGame);
    }

    void ShowSection(Button chosen)
    {
        SetSection(SectionGame, PanelGame, chosen);
        SetSection(SectionKeys, PanelKeys, chosen);
        SetSection(SectionPacks, PanelPacks, chosen);
    }

    static void SetSection(Button tab, FrameworkElement panel, Button chosen)
    {
        bool on = ReferenceEquals(tab, chosen);
        // 选中态整块换样式：background accent-soft + color text（:273 的 button.active）
        tab.Style = (Style)tab.FindResource(on ? "CfgSectionTabActive" : "CfgSectionTab");
        // 三个分区互斥显示，对应模板 :155 / :175 / :156 的 v-show
        panel.Visibility = on ? Visibility.Visible : Visibility.Collapsed;
    }

    // ================= 游戏选项（DefaultGameOptions.vue）=================

    void PreviewVersion_Changed(object sender, SelectionChangedEventArgs e) { if (_loaded) RenderOptions(); }
    void OptionsBack_Click(object sender, RoutedEventArgs e) { _optPage = _optPage == "mouse" ? "controls" : "root"; RenderOptions(); }

    static readonly string[] MousePageIds =
        ["mouseSensitivity", "mouseWheelSensitivity", "discrete_mouse_scroll", "invertXMouse", "invertYMouse", "allowCursorChanges", "touchscreen", "rawMouseInput"];

    /// <summary>rows 计算属性（DefaultGameOptions.vue:12-15）：controls 页并进 mouseSensitivity，video 按原表序。</summary>
    IEnumerable<DefaultGameOptions.GameOptionDef> CurrentRows()
    {
        IEnumerable<DefaultGameOptions.GameOptionDef> list = _optPage switch
        {
            "mouse" => DefaultGameOptions.Unique.Where(d => MousePageIds.Contains(d.Id)),
            "controls" => DefaultGameOptions.Unique.Where(d => d.Page == "controls" || d.Id == "mouseSensitivity"),
            _ => DefaultGameOptions.Unique.Where(d => d.Page == _optPage),
        };
        if (_optPage == "video")
            list = list.OrderBy(d => Array.IndexOf(DefaultGameOptions.GameOptions, d));
        return list;
    }

    void RenderOptions()
    {
        string ver = PreviewVersion;
        TxtOptionsPath.Text = _optPage == "root" ? "选项"
            : _optPage == "mouse" ? "选项 / 控制 / 鼠标设置"
            : "选项 / " + (DefaultGameOptions.Pages.FirstOrDefault(p => p.Id == _optPage).Name ?? _optPage);
        BtnOptionsBack.Visibility = _optPage == "root" ? Visibility.Collapsed : Visibility.Visible;

        bool root = _optPage == "root";
        OptionsGrid.Visibility = root || _optPage == "credits" ? Visibility.Collapsed : Visibility.Visible;
        OptionsEntries.Visibility = root ? Visibility.Visible : Visibility.Collapsed;
        OptionsSubEntrances.Visibility = _optPage == "controls" ? Visibility.Visible : Visibility.Collapsed;
        CreditsNote.Visibility = _optPage == "credits" ? Visibility.Visible : Visibility.Collapsed;

        if (root)
        {
            // root 页格序（DefaultGameOptions.vue:58-67）：fov 行 + 鼠标灵敏度快捷 + 世界选项
            OptionsGrid.Visibility = Visibility.Visible;
            OptionsGrid.Children.Clear();
            OptionsGrid.RowDefinitions.Clear();
            OptionsGrid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
            OptionsGrid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
            var fov = DefaultGameOptions.Unique.First(d => d.Id == "fov");
            var fovCell = BuildOptionCell(fov, ver);
            Grid.SetRow(fovCell, 0);
            Grid.SetColumn(fovCell, 0);
            OptionsGrid.Children.Add(fovCell);

            var shortcut = BuildMouseShortcut();
            Grid.SetRow(shortcut, 0);
            Grid.SetColumn(shortcut, 2);
            OptionsGrid.Children.Add(shortcut);

            var world = BuildWorldOption();
            Grid.SetRow(world, 1);
            Grid.SetColumn(world, 0);
            OptionsGrid.Children.Add(world);
        }
        else if (_optPage != "credits")
        {
            OptionsGrid.Children.Clear();
            OptionsGrid.RowDefinitions.Clear();
            var rows = CurrentRows().ToList();
            int nRows = (rows.Count + 1) / 2;
            for (int i = 0; i < nRows; i++) OptionsGrid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
            for (int i = 0; i < rows.Count; i++)
            {
                var cell = BuildOptionCell(rows[i], ver);
                Grid.SetRow(cell, i / 2);
                Grid.SetColumn(cell, (i % 2) * 2);
                OptionsGrid.Children.Add(cell);
            }
        }

        // root 页入口（:70，两列）；controls 子入口（:56，两枚）
        FillEntrances(OptionsEntries, DefaultGameOptions.Pages.Select(pg => (pg.Id, pg.Name, false)).ToList());
        FillEntrances(OptionsSubEntrances, new List<(string, string, bool)>
        {
            ("mouse", "鼠标设置 · 灵敏度与滚动…", false),
            ("keys", "按键控制…", true),
        });

        // 不可用自定义项警告（:53）
        var unavailable = DefaultGameOptions.Unique
            .Where(d => _opts.Values.ContainsKey(d.Id) && !DefaultGameOptions.Supported(d, ver)).Select(d => d.Label).ToList();
        OptionsWarning.Visibility = unavailable.Count > 0 ? Visibility.Visible : Visibility.Collapsed;
        OptionsWarningText.Text = $"{ver} 不支持：{string.Join("、", unavailable)}。这些项目仅对支持它们的版本生效。";
    }

    /// <summary>入口网格统一填充。special=keys → 切按键页签（DefaultGameOptions.vue:35 的 emit）。</summary>
    void FillEntrances(Grid grid, List<(string Id, string Name, bool Special)> items)
    {
        grid.Children.Clear();
        grid.RowDefinitions.Clear();
        int rows = (items.Count + 1) / 2;
        for (int i = 0; i < rows; i++) grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        for (int i = 0; i < items.Count; i++)
        {
            var b = new Button
            {
                Style = (Style)FindResource("VpOptionEntry"),
                Content = BuildEntryContent(items[i].Name),
                Cursor = System.Windows.Input.Cursors.Hand,
            };
            var pid = items[i].Id;
            var special = items[i].Special;
            b.Click += (_, _) =>
            {
                if (special) { ShowSection(SectionKeys); return; }
                _optPage = pid;
                RenderOptions();
            };
            Grid.SetRow(b, i / 2);
            Grid.SetColumn(b, (i % 2) * 2);
            grid.Children.Add(b);
        }
    }

    /// <summary>mouse-shortcut（:66）：整宽按钮，label + strong(值) + 右侧「控制 → 鼠标设置 ›」。</summary>
    FrameworkElement BuildMouseShortcut()
    {
        var sens = DefaultGameOptions.Unique.First(d => d.Id == "mouseSensitivity");
        object Value() => _opts.Values.TryGetValue("mouseSensitivity", out var v) ? v : sens.Initial;
        string Label() => Value() is double d ? d.ToString("0", System.Globalization.CultureInfo.InvariantCulture) + "%" : "100%";

        var sp = new StackPanel { Orientation = Orientation.Horizontal };
        sp.Children.Add(new TextBlock { Text = "鼠标灵敏度", FontSize = 14, VerticalAlignment = VerticalAlignment.Center });
        sp.Children.Add(new TextBlock
        {
            Text = Label(), FontSize = 14, FontWeight = FontWeights.Bold,
            Foreground = (Brush)FindResource("AccentHi"),
            VerticalAlignment = VerticalAlignment.Center, Margin = new Thickness(10, 0, 0, 0),
        });
        var hint = new TextBlock
        {
            Text = "控制 → 鼠标设置 ›", FontSize = 12,
            Foreground = (Brush)FindResource("Fg2"),
            VerticalAlignment = VerticalAlignment.Center,
            HorizontalAlignment = HorizontalAlignment.Right,
        };
        var grid = new Grid();
        grid.Children.Add(sp);
        Grid.SetColumn(hint, 1);
        var col = new ColumnDefinition { Width = GridLength.Auto };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(col);
        grid.Children.Add(hint);
        // .btn.btn-ghost.mouse-shortcut（:66）——是按钮，点了去鼠标设置子页
        var btn = new Button { Content = grid, Cursor = System.Windows.Input.Cursors.Hand };
        btn.SetResourceReference(FrameworkElement.StyleProperty, "VpOptionEntry");
        btn.Click += (_, _) => { _optPage = "mouse"; RenderOptions(); };
        return btn;
    }

    /// <summary>world-option（:67）：由单人世界或服务器管理。</summary>
    FrameworkElement BuildWorldOption()
    {
        var sp = new StackPanel();
        sp.Children.Add(new TextBlock { Text = "世界选项 / 难度", FontSize = 14, FontWeight = FontWeights.SemiBold });
        sp.Children.Add(new TextBlock
        {
            Text = "由单人世界或服务器管理，请在游戏中修改。",
            Style = (Style)FindResource("Dim"), FontSize = 12, LineHeight = 20.4,
            TextWrapping = TextWrapping.Wrap, Margin = new Thickness(0, 6, 0, 0),
        });
        return new Border
        {
            Padding = new Thickness(12, 12, 12, 12),
            BorderBrush = (Brush)FindResource("Line"),
            BorderThickness = new Thickness(0, 0, 0, 1),
            Child = sp,
            VerticalAlignment = VerticalAlignment.Center,
        };
    }

    static UIElement BuildEntryContent(string name)
    {
        var g = new Grid();
        g.Children.Add(new TextBlock { Text = name, FontSize = 14, VerticalAlignment = VerticalAlignment.Center });
        g.Children.Add(new TextBlock { Text = "›", FontSize = 14, VerticalAlignment = VerticalAlignment.Center, HorizontalAlignment = HorizontalAlignment.Right });
        return g;
    }

    /// <summary>
    /// option-cell（:59-65 + :81 覆盖后的扁平行）：padding 12 0 + 下边线；
    /// caption（14px，label | 数值输入+单位 或 strong 值 accent-2）→
    /// 滑条（number）/ 下拉（choices&gt;3）/ 循环钮（其余，全宽）→
    /// foot（状态 small 12px + 跟随游戏/应用此值 12px accent-2 无边钮）。
    /// custom 时下边线转 accent（.option-custom 的 color-mix 退化）。
    /// </summary>
    FrameworkElement BuildOptionCell(DefaultGameOptions.GameOptionDef d, string ver)
    {
        bool custom = _opts.Values.ContainsKey(d.Id);
        bool supported = DefaultGameOptions.Supported(d, ver);

        var cell = new Border
        {
            Padding = new Thickness(12, 12, 12, 12),
            Opacity = supported ? 1 : 0.65,
        };
        cell.SetResourceReference(Border.BorderBrushProperty, custom ? "Accent" : "Line");
        cell.BorderThickness = new Thickness(0, 0, 0, 1);
        var sp = new StackPanel();

        // ---- caption ----
        var caption = new Grid { Margin = new Thickness(0, 0, 0, 14) };
        caption.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        caption.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        caption.Children.Add(new TextBlock { Text = d.Label, FontSize = 14, VerticalAlignment = VerticalAlignment.Center, ToolTip = d.Id });
        object Value() => _opts.Values.TryGetValue(d.Id, out var v) ? v : d.Initial;
        string Label() => d.Choices is not null
            ? d.Choices.FirstOrDefault(c => Equals(c.Value, Value())).Label ?? Stringify(Value())
            : Value() is bool b ? (b ? "开启" : "关闭") : Stringify(Value()) + (d.Unit ?? "");

        if (d.Choices is null && d.Initial is double)
        {
            // .option-number：input(76 右对齐) + 单位
            var num = new StackPanel { Orientation = Orientation.Horizontal, VerticalAlignment = VerticalAlignment.Center };
            var box = new System.Windows.Controls.TextBox
            {
                Style = (Style)FindResource("Input"),
                Width = 76, FontSize = 13, TextAlignment = TextAlignment.Right,
                Text = ((double)Value()).ToString("0.##", System.Globalization.CultureInfo.InvariantCulture),
                IsEnabled = supported,
            };
            if (!supported) box.ToolTip = d.Until is not null ? "此版本已移除此选项" : "需要 Minecraft " + d.Since + " 或更新版本";
            void Apply()
            {
                if (!double.TryParse(box.Text.Replace('，', '.'), System.Globalization.NumberStyles.Float, System.Globalization.CultureInfo.InvariantCulture, out var n)
                    || n < d.Min || n > d.Max)
                {
                    box.Text = ((double)Value()).ToString("0.##", System.Globalization.CultureInfo.InvariantCulture);
                    return;
                }
                if (Math.Abs(n - (double)Value()) > 1e-9) _ = SaveOptionAsync(d, n);
            }
            box.LostFocus += (_, _) => Apply();
            box.KeyDown += (_, ke) => { if (ke.Key == Key.Enter) Apply(); };
            num.Children.Add(box);
            num.Children.Add(new TextBlock { Text = d.Unit ?? "", FontSize = 13, VerticalAlignment = VerticalAlignment.Center, Margin = new Thickness(4, 0, 0, 0) });
            Grid.SetColumn(num, 1);
            caption.Children.Add(num);
        }
        else
        {
            caption.Children.Add(new TextBlock
            {
                Text = Label(), FontSize = 14, FontWeight = FontWeights.Bold,
                Foreground = (Brush)FindResource("AccentHi"),
                VerticalAlignment = VerticalAlignment.Center,
            });
            Grid.SetColumn(caption.Children[^1], 1);
        }
        sp.Children.Add(caption);

        // ---- 控件 ----
        if (d.Choices is not null && d.Choices.Length > 3)
        {
            var cb = new ComboBox { FontSize = 13, IsEnabled = supported, HorizontalAlignment = HorizontalAlignment.Stretch };
            foreach (var c in d.Choices) cb.Items.Add(c.Label);
            int idx = Array.FindIndex(d.Choices, c => Equals(c.Value, Value()));
            cb.SelectedIndex = idx >= 0 ? idx : 0;
            cb.SelectionChanged += (_, _) =>
            {
                if (cb.SelectedIndex >= 0 && !Equals(d.Choices[cb.SelectedIndex].Value, Value()))
                    _ = SaveOptionAsync(d, d.Choices[cb.SelectedIndex].Value);
            };
            sp.Children.Add(cb);
        }
        else if (d.Choices is null && d.Initial is double)
        {
            var slider = new Slider
            {
                Style = (Style)FindResource("VpRange"),
                Minimum = d.Min ?? 0,
                Maximum = d.Max ?? 100,
                Height = 22,
                VerticalAlignment = VerticalAlignment.Center,
                IsEnabled = supported,
                IsMoveToPointEnabled = true,
            };
            if (d.Step is { } st && st > 0) slider.TickFrequency = st;
            slider.Value = Math.Clamp((double)Value(), slider.Minimum, slider.Maximum);
            // 原版 @change（松手生效）
            slider.AddHandler(System.Windows.Controls.Primitives.Thumb.DragCompletedEvent,
                new System.Windows.Controls.Primitives.DragCompletedEventHandler((_, _) =>
                    _ = SaveOptionAsync(d, slider.Value)));
            sp.Children.Add(slider);
        }
        else
        {
            // .option-toggle：全宽、space-between、label + ↻
            var toggle = new Button
            {
                IsEnabled = supported,
                Cursor = System.Windows.Input.Cursors.Hand,
            };
            if (!supported) toggle.ToolTip = d.Until is not null ? "此版本已移除此选项" : "需要 Minecraft " + d.Since + " 或更新版本";
            var tg = new Grid();
            tg.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            tg.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            tg.Children.Add(new TextBlock { Text = Label(), FontSize = 13, VerticalAlignment = VerticalAlignment.Center });
            tg.Children.Add(new TextBlock { Text = "↻", FontSize = 13, VerticalAlignment = VerticalAlignment.Center, Margin = new Thickness(8, 0, 0, 0) });
            Grid.SetColumn(tg.Children[^1], 1);
            toggle.Content = tg;
            toggle.SetResourceReference(FrameworkElement.StyleProperty, "VpOptionToggle");
            toggle.Click += (_, _) =>
            {
                var choices = d.Choices is not null
                    ? d.Choices.Select(c => c.Value).Cast<object>().ToArray()
                    : new object[] { false, true };
                int i = choices.ToList().FindIndex(c => Equals(c, Value()));
                _ = SaveOptionAsync(d, choices[(i + 1) % choices.Length]);
            };
            sp.Children.Add(toggle);
        }

        // ---- foot ----
        string status = !supported
            ? (d.Until is not null ? "此版本已移除此选项" : "需要 Minecraft " + d.Since + " 或更新版本")
            : custom ? (_opts.Enabled ? "将应用到游戏" : "已保存 · 同步未开启") : "跟随游戏 · 尚未覆盖";
        var foot = new Grid { Margin = new Thickness(0, 12, 0, 0) };
        foot.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        foot.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        foot.Children.Add(new TextBlock { Text = status, FontSize = 12, Foreground = (Brush)FindResource("Fg2"), VerticalAlignment = VerticalAlignment.Center });
        var footBtn = new Button
        {
            Content = custom ? "跟随游戏" : "应用此值",
            FontSize = 12,
            Cursor = System.Windows.Input.Cursors.Hand,
            VerticalAlignment = VerticalAlignment.Center,
        };
        footBtn.SetResourceReference(FrameworkElement.StyleProperty, "VpFootBtn");
        footBtn.Click += (_, _) => _ = SaveOptionAsync(d, custom ? null : Normalize(d, Value()));
        Grid.SetColumn(footBtn, 1);
        foot.Children.Add(footBtn);
        sp.Children.Add(foot);

        cell.Child = sp;
        return cell;
    }

    static string Stringify(object v) => v switch
    {
        double d => d.ToString("0.##", CultureInfo.InvariantCulture),
        bool b => b ? "true" : "false",
        _ => v.ToString() ?? "",
    };

    /// <summary>save（DefaultGameOptions.vue:19-25）：busy 防抖；存完重渲染本区。</summary>
    async Task SaveOptionAsync(DefaultGameOptions.GameOptionDef d, object? value)
    {
        if (_busy) return;
        _busy = true;
        try
        {
            _opts = await Task.Run(() => DefaultGameOptions.Set(State.Paths,
                id: d.Id, value: value is null ? null : Normalize(d, value)));
            State.Status = "已保存游戏选项";
        }
        catch (Exception ex) { State.Status = "保存游戏选项失败：" + ex.Message; }
        finally { _busy = false; }
        RenderNavCounts();
        RenderOptions();
    }

    static object Normalize(DefaultGameOptions.GameOptionDef d, object v)
        => d.Initial is double && v is not double ? Convert.ToDouble(v, CultureInfo.InvariantCulture) : v;

    async void OptionSync_Changed(object sender, RoutedEventArgs e)
    {
        if (!_loaded || _busy) return;
        bool on = SwOptionSync.IsChecked == true;
        try { _opts = await Task.Run(() => DefaultGameOptions.Set(State.Paths, enabled: on)); }
        catch (Exception ex) { State.Status = "保存失败：" + ex.Message; SwOptionSync.IsChecked = !on; }
    }

    // ================= 按键配置（KeysView.vue keys 分区）=================

    /// <summary>keyGrouped（:52-62）：分类 → 过滤（名称/id/绑定值标签）。</summary>
    IEnumerable<(string Cat, List<DefaultKeys.KeybindDef> Items)> KeyGroups()
    {
        var kw = TxtKeySearch.Text.Trim().ToLowerInvariant();
        bool Match(DefaultKeys.KeybindDef d) => kw.Length == 0
            || d.Label.ToLowerInvariant().Contains(kw)
            || d.Id.ToLowerInvariant().Contains(kw)
            || McLabel(CurrentBind(d)).ToLowerInvariant().Contains(kw);
        return DefaultKeys.Categories
            .Select(c => (Cat: c, Items: DefaultKeys.VanillaKeybinds.Where(d => d.Category == c && Match(d)).ToList()))
            .Where(g => g.Items.Count > 0);
    }

    void RenderKeys()
    {
        KeysScroll.Children.Clear();
        var empty = KeysEmpty;
        KeysScroll.Children.Remove(empty);
        var groups = KeyGroups().ToList();
        int rows = (groups.Count + 1) / 2;
        for (int i = 0; i < rows; i++) KeysScroll.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        for (int i = 0; i < groups.Count; i++)
        {
            // .cfg-group margin-top 16，first-row 归 0 — :246-247
            var sp = new StackPanel { Margin = i < 2 ? new Thickness(0) : new Thickness(0, 16, 0, 0) };
            sp.Children.Add(new TextBlock { Style = (Style)FindResource("CfgCat"), Text = groups[i].Cat });
            foreach (var def in groups[i].Items) sp.Children.Add(BuildKeyRow(def));
            Grid.SetRow(sp, i / 2);
            Grid.SetColumn(sp, (i % 2) * 2);
            KeysScroll.Children.Add(sp);
        }
        if (groups.Count == 0)
        {
            KeysEmptyText.Text = $"没有匹配「{TxtKeySearch.Text.Trim()}」的按键";
            empty.Visibility = Visibility.Visible;
        }
        else empty.Visibility = Visibility.Collapsed;
        Grid.SetRow(empty, Math.Max(0, rows));
        Grid.SetColumn(empty, 0);
        Grid.SetColumnSpan(empty, 3);
        KeysScroll.Children.Add(empty);
    }

    FrameworkElement BuildKeyRow(DefaultKeys.KeybindDef def)
    {
        string bind = CurrentBind(def);
        bool modified = bind != def.DefaultBind;
        bool capturing = _capturing == def.Id;

        var box = new Border { Style = (Style)FindResource("CfgRowBox"), Tag = def };
        var g = new Grid();
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(12) });
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(12) });
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(12) });
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        box.Child = g;

        g.Children.Add(new TextBlock { Style = (Style)FindResource("CfgLabel"), ToolTip = def.Id, Text = def.Label, VerticalAlignment = VerticalAlignment.Center });

        var bindBtn = new Button
        {
            Style = (Style)FindResource(capturing ? "CfgValueCapturing" : modified ? "CfgValueModified" : "CfgValue"),
            Content = capturing ? "按任意键…" : McLabel(bind),
            ToolTip = capturing ? "按任意键设置，Esc 取消" : "点击后按任意键修改",
        };
        if (capturing) bindBtn.SetValue(Panel.ZIndexProperty, 9001);
        Grid.SetColumn(bindBtn, 2);
        var defId = def.Id;
        bindBtn.Click += (_, _) => StartCapture(defId);
        g.Children.Add(bindBtn);

        if (capturing)
        {
            var clear = new Button { Style = (Style)FindResource("CfgClear"), Content = "×", ToolTip = "设为未指定" };
            clear.SetValue(Panel.ZIndexProperty, 9001);
            Grid.SetColumn(clear, 4);
            clear.Click += (_, _) => _ = ApplyKeyAsync(def.Id, "key.keyboard.unknown", cleared: true);
            g.Children.Add(clear);
        }

        if (modified && !capturing)
        {
            var reset = new Button
            {
                Style = (Style)FindResource("CfgReset"),
                ToolTip = "恢复此项默认",
                Content = new TextBlock { Text = "↺", FontSize = 13, HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center },
            };
            reset.Click += (_, _) => _ = ApplyKeyAsync(def.Id, def.DefaultBind);
            Grid.SetColumn(reset, 6);
            g.Children.Add(reset);
        }
        else
        {
            // .cfg-reset.invisible：visibility hidden 占位（:268）
            var reset = new Button { Style = (Style)FindResource("CfgReset"), Visibility = Visibility.Hidden, IsHitTestVisible = false };
            Grid.SetColumn(reset, 6);
            g.Children.Add(reset);
        }
        return box;
    }

    // ---------------- 键位捕获（startCapture/onCaptureKey/onCaptureMouse — :70-99）----------------

    void StartCapture(string id)
    {
        if (_capturing == id) { StopCapture(); return; }
        _capturing = id;
        RenderKeys();
    }

    void StopCapture()
    {
        if (_capturing.Length == 0) return;
        _capturing = "";
        RenderKeys();
    }

    void OnCaptureKey(object sender, KeyEventArgs e)
    {
        if (_capturing.Length == 0) return;
        e.Handled = true;
        if (e.Key == Key.Escape) { StopCapture(); return; }
        var bind = KeyToMc(e.Key);
        if (bind is null) return;
        var id = _capturing;
        StopCapture();
        _ = ApplyKeyAsync(id, bind);
    }

    void OnCaptureMouse(object sender, MouseButtonEventArgs e)
    {
        if (_capturing.Length == 0) return;
        // × 按钮自身的点击不算鼠标键（:100 的 data-key-clear 判定）
        if (e.OriginalSource is DependencyObject d && FindAncestor<Button>(d)?.ToolTip as string == "设为未指定") return;
        e.Handled = true;
        var bind = e.ChangedButton switch
        {
            MouseButton.Left => "key.mouse.left",
            MouseButton.Middle => "key.mouse.middle",
            MouseButton.Right => "key.mouse.right",
            MouseButton.XButton1 => "key.mouse.4",
            MouseButton.XButton2 => "key.mouse.5",
            _ => null,
        };
        if (bind is null) return;
        var id = _capturing;
        StopCapture();
        _ = ApplyKeyAsync(id, bind);
    }

    static T? FindAncestor<T>(DependencyObject d) where T : DependencyObject
    {
        while (d is not null and not T) d = System.Windows.Media.VisualTreeHelper.GetParent(d);
        return d as T;
    }

    /// <summary>applyKey（:81-90）。</summary>
    async Task ApplyKeyAsync(string id, string bind, bool cleared = false)
    {
        try
        {
            _keys = await Task.Run(() => DefaultKeys.SetDefaultKey(State.Paths, id, bind));
            State.Status = cleared ? "已将该按键设置为空" : "已更新默认按键";
        }
        catch (Exception ex) { State.Status = "设置失败：" + ex.Message; }
        RenderNavCounts();
        RenderKeys();
    }

    async void ResetAllKeys_Click(object sender, RoutedEventArgs e)
    {
        try
        {
            _keys = await Task.Run(() => DefaultKeys.ResetDefaultKeys(State.Paths));
            State.Status = "按键已全部恢复为 MC 原版默认";
        }
        catch (Exception ex) { State.Status = "重置失败：" + ex.Message; }
        RenderNavCounts();
        RenderKeys();
    }

    void KeySync_Changed(object sender, RoutedEventArgs e)
    {
        if (!_loaded) return;
        bool on = SwKeySync.IsChecked == true;
        State.Settings.Value.KeySync = on;
        State.SaveSettings();
        State.Status = on ? "已开启按键设置同步" : "已关闭按键设置同步";
    }

    /// <summary>WPF Key → MC 绑定值（shared/keybindings.ts CODE_TO_MC 的 WPF 等价表）。</summary>
    static string? KeyToMc(Key k) => k switch
    {
        >= Key.A and <= Key.Z => "key.keyboard." + k.ToString().ToLowerInvariant(),
        >= Key.D0 and <= Key.D9 => "key.keyboard." + (char)('0' + (int)(k - Key.D0)),
        Key.Space => "key.keyboard.space",
        Key.Tab => "key.keyboard.tab",
        Key.Enter => "key.keyboard.enter",
        Key.Escape => "key.keyboard.escape",
        Key.Back => "key.keyboard.backspace",
        Key.Delete => "key.keyboard.delete",
        Key.Insert => "key.keyboard.insert",
        Key.Home => "key.keyboard.home",
        Key.End => "key.keyboard.end",
        Key.Prior => "key.keyboard.page.up",
        Key.Next => "key.keyboard.page.down",
        Key.Up => "key.keyboard.up",
        Key.Down => "key.keyboard.down",
        Key.Left => "key.keyboard.left",
        Key.Right => "key.keyboard.right",
        Key.LeftShift => "key.keyboard.left.shift",
        Key.RightShift => "key.keyboard.right.shift",
        Key.LeftCtrl => "key.keyboard.left.control",
        Key.RightCtrl => "key.keyboard.right.control",
        Key.LeftAlt => "key.keyboard.left.alt",
        Key.RightAlt => "key.keyboard.right.alt",
        Key.LWin => "key.keyboard.left.win",
        Key.RWin => "key.keyboard.right.win",
        Key.Capital => "key.keyboard.caps.lock",
        Key.NumLock => "key.keyboard.num.lock",
        Key.OemMinus => "key.keyboard.minus",
        Key.OemPlus => "key.keyboard.equal",
        Key.OemOpenBrackets => "key.keyboard.left.bracket",
        Key.OemCloseBrackets => "key.keyboard.right.bracket",
        Key.OemBackslash => "key.keyboard.backslash",
        Key.OemSemicolon => "key.keyboard.semicolon",
        Key.OemQuotes => "key.keyboard.apostrophe",
        Key.OemComma => "key.keyboard.comma",
        Key.OemPeriod => "key.keyboard.period",
        Key.OemQuestion => "key.keyboard.slash",
        Key.OemTilde => "key.keyboard.grave.accent",
        >= Key.F1 and <= Key.F12 => "key.keyboard.f" + (int)(k - Key.F1 + 1),
        >= Key.NumPad0 and <= Key.NumPad9 => "key.keyboard.keypad." + (int)(k - Key.NumPad0),
        Key.Add => "key.keyboard.keypad.add",
        Key.Subtract => "key.keyboard.keypad.subtract",
        Key.Decimal => "key.keyboard.keypad.decimal",
        Key.Divide => "key.keyboard.keypad.divide",
        Key.Multiply => "key.keyboard.keypad.multiply",
        _ => null,
    };

    /// <summary>mcKeyLabel（keybindings.ts:113-134）。</summary>
    static string McLabel(string bind)
    {
        if (string.IsNullOrEmpty(bind) || bind == "key.keyboard.unknown") return "未指定";
        if (bind.StartsWith("key.mouse.", StringComparison.Ordinal))
        {
            var tail = bind["key.mouse.".Length..];
            return tail switch
            {
                "left" => "鼠标左键",
                "middle" => "鼠标中键",
                "right" => "鼠标右键",
                "4" => "鼠标侧键4",
                "5" => "鼠标侧键5",
                _ => bind,
            };
        }
        var key = bind.StartsWith("key.keyboard.", StringComparison.Ordinal) ? bind["key.keyboard.".Length..] : bind;
        var pretty = new Dictionary<string, string>
        {
            ["space"] = "空格", ["tab"] = "Tab", ["enter"] = "回车", ["escape"] = "Esc", ["backspace"] = "退格",
            ["delete"] = "Del", ["left.shift"] = "左Shift", ["right.shift"] = "右Shift",
            ["left.control"] = "左Ctrl", ["right.control"] = "右Ctrl", ["left.alt"] = "左Alt", ["right.alt"] = "右Alt",
            ["left.win"] = "左Win", ["right.win"] = "右Win", ["up"] = "↑", ["down"] = "↓", ["left"] = "←", ["right"] = "→",
            ["page.up"] = "PageUp", ["page.down"] = "PageDown", ["caps.lock"] = "CapsLock", ["num.lock"] = "NumLock",
            ["grave.accent"] = "`", ["apostrophe"] = "'", ["slash"] = "/", ["backslash"] = "\\",
            ["minus"] = "-", ["equal"] = "=", ["comma"] = ",", ["period"] = ".",
            ["left.bracket"] = "[", ["right.bracket"] = "]", ["semicolon"] = ";",
            ["home"] = "Home", ["end"] = "End", ["insert"] = "Ins",
        };
        if (pretty.TryGetValue(key, out var p)) return p;
        if (key.StartsWith("keypad.", StringComparison.Ordinal)) return "小键盘 " + key["keypad.".Length..];
        return key.Length == 1 ? key.ToUpperInvariant() : key;
    }

    // ================= 默认材质包（KeysView.vue packs 分区）=================

    void RenderPacks()
    {
        PacksHost.Children.Clear();
        for (int i = 0; i < _packs.Count; i++)
            PacksHost.Children.Add(BuildPackRow(_packs[i], i));
        PacksEmpty.Visibility = _packs.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
        RenderNavCounts();
    }

    FrameworkElement BuildPackRow(DefaultResourcePacks.Pack pack, int index)
    {
        var box = new Border { Style = (Style)FindResource("CfgPackRowBox"), Tag = pack };
        var g = new Grid();
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(12) });
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(12) });
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(12) });
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(12) });
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(12) });
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        box.Child = g;

        g.Children.Add(new TextBlock { Style = (Style)FindResource("CfgLabel"), ToolTip = pack.Name, Text = pack.Name });

        var stateText = new TextBlock
        {
            Style = (Style)FindResource("CfgSyncText"),
            Text = pack.Enabled ? "已启用" : "未启用",
            Margin = new Thickness(0, 0, 8, 0),
            VerticalAlignment = VerticalAlignment.Center,
        };
        Grid.SetColumn(stateText, 2);
        g.Children.Add(stateText);

        var sw = new CheckBox { Style = (Style)FindResource("Switch"), IsChecked = pack.Enabled, VerticalAlignment = VerticalAlignment.Center };
        Grid.SetColumn(sw, 4);
        g.Children.Add(sw);

        var up = new Button { Style = (Style)FindResource("BtnGhost"), Content = "↑", MinHeight = 32, Padding = new Thickness(12, 4, 12, 4), FontSize = 13, IsEnabled = index > 0, ToolTip = index == 0 ? "降低优先级（已是首项）" : "降低优先级" };
        Grid.SetColumn(up, 6);
        g.Children.Add(up);
        var down = new Button { Style = (Style)FindResource("BtnGhost"), Content = "↓", MinHeight = 32, Padding = new Thickness(12, 4, 12, 4), FontSize = 13, IsEnabled = index < _packs.Count - 1, ToolTip = index == _packs.Count - 1 ? "已是最后一项" : "提高优先级" };
        Grid.SetColumn(down, 8);
        g.Children.Add(down);
        var remove = new Button { Style = (Style)FindResource("BtnGhost"), Content = "移除", MinHeight = 32, Padding = new Thickness(12, 4, 12, 4), FontSize = 13 };
        Grid.SetColumn(remove, 10);
        g.Children.Add(remove);

        var packId = pack.Id;
        sw.Checked += (_, _) => _ = PackActionAsync(() => DefaultResourcePacks.SetEnabled(State.Paths, packId, true));
        sw.Unchecked += (_, _) => _ = PackActionAsync(() => DefaultResourcePacks.SetEnabled(State.Paths, packId, false));
        up.Click += (_, _) => _ = PackActionAsync(() => DefaultResourcePacks.Move(State.Paths, packId, -1));
        down.Click += (_, _) => _ = PackActionAsync(() => DefaultResourcePacks.Move(State.Paths, packId, 1));
        remove.Click += (_, _) => _ = PackActionAsync(() => DefaultResourcePacks.Remove(State.Paths, packId));
        return box;
    }

    /// <summary>editPacks（KeysView.vue:25-37）：busy 防抖 + 重载 + 刷导航计数；enable=true 时顺带开总同步。</summary>
    async Task PackActionAsync(Func<List<DefaultResourcePacks.Pack>> action, bool enable = false)
    {
        if (_busy) return;
        _busy = true;
        BtnAddPack.Content = "正在导入…";
        BtnAddPack.IsEnabled = false;
        try
        {
            _packs = await Task.Run(action);
            if (enable && _packs.Count > 0 && !State.Settings.Value.ResourcePackSync)
            {
                State.Settings.Value.ResourcePackSync = true;
                State.SaveSettings();
                SwPackSync.IsChecked = true;   // 添加后自动开启同步 — :24
            }
            State.Status = "已更新默认材质包";
        }
        catch (Exception ex) { State.Status = "材质包配置失败：" + ex.Message; }
        finally
        {
            _busy = false;
            BtnAddPack.Content = "添加材质包…";
            BtnAddPack.IsEnabled = true;
        }
        RenderNavCounts();
        RenderPacks();
    }

    async void AddPack_Click(object sender, RoutedEventArgs e)
    {
        var dlg = new Microsoft.Win32.OpenFileDialog
        {
            Title = "选择材质包",
            Filter = "材质包 (*.zip)|*.zip",
            Multiselect = true,
        };
        if (dlg.ShowDialog() != true) return;
        await PackActionAsync(() => DefaultResourcePacks.Import(State.Paths, dlg.FileNames), enable: true);
    }

    void Packs_DragOver(object sender, DragEventArgs e)
    {
        PacksDragActive.Visibility = Visibility.Visible;   // .drag-active — :231
        e.Effects = e.Data.GetDataPresent(DataFormats.FileDrop) ? DragDropEffects.Copy : DragDropEffects.None;
        e.Handled = true;
    }

    async void Packs_Drop(object sender, DragEventArgs e)
    {
        PacksDragActive.Visibility = Visibility.Collapsed;
        if (e.Data.GetData(DataFormats.FileDrop) is not string[] files || files.Length == 0) return;
        // dropPacks（:29-34）：拖入即导入并自动开启同步
        ShowSection(SectionPacks);
        await PackActionAsync(() => DefaultResourcePacks.Import(State.Paths, files.Where(File.Exists)), enable: true);
    }

    void PackSync_Changed(object sender, RoutedEventArgs e)
    {
        if (!_loaded) return;
        bool on = SwPackSync.IsChecked == true;
        State.Settings.Value.ResourcePackSync = on;
        State.SaveSettings();
        State.Status = on ? "已开启材质包同步" : "已关闭材质包同步";
    }

    // ---------------- 搜索框 ----------------
    void Search_TextChanged(object sender, TextChangedEventArgs e)
    {
        SearchHint.Visibility = TxtKeySearch.Text.Length == 0 ? Visibility.Visible : Visibility.Collapsed;
        if (_loaded) RenderKeys();
    }
}
