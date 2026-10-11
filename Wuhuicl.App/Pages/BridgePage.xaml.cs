using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Threading;
using Wuhuicl.App.Services;

namespace Wuhuicl.App.Pages;

/// <summary>
/// MOD 面板（BridgeView.vue）：运行期 MOD 实时参数编辑器，已接真实后端 <see cref="BridgeService"/>。
/// 状态机对照原版 poll()（:93-100）：3 秒一轮（页面隐藏时暂停），StatusAsync → 连接后拉 ManifestAsync。
/// 六态文案 / 四个引导动作的显隐（:196-204）、参数两级聚合（modId → group，:102-121）、
/// 四种控件（SWITCH/SLIDER/TEXT/下拉）、apply 标签（APPLY_HINT :132-137）、SERVER 只读（:265/:272）、
/// 单参数恢复默认（resetParam :162）与全部恢复默认（resetAll :173）全部按原版语义。
/// 修改即 SetAsync 下发，以游戏返回的 value/notice/error 为准（applyParam :139-157）。
/// </summary>
public partial class BridgePage : UserControl
{
    LaunchSession State => (LaunchSession)DataContext;

    /// <summary>APPLY_HINT — BridgeView.vue:132-137。INSTANT 是空串不出标签。</summary>
    static string ApplyHint(string apply) => apply switch
    {
        "RELOAD_RESOURCES" => "需重载资源",
        "REJOIN_WORLD" => "需重进世界",
        "RESTART_GAME" => "需重启游戏",
        _ => "",
    };

    List<BridgeService.BridgeParam> _params = [];
    bool _connected;
    bool _loadingManifest;
    bool _installing;
    bool? _present;                 // null=未知（原版 bridgePresent ref）
    int _statusReq, _manifestReq, _presenceReq;   // 重入保护：只认最后一次请求的回包
    DispatcherTimer? _poll;

    public BridgePage()
    {
        InitializeComponent();
        IsVisibleChanged += (_, e) =>
        {
            if ((bool)e.NewValue)
            {
                State.PropertyChanged += State_PropertyChanged;
                _ = RefreshPresenceAsync();
                _ = PollAsync();
                StartPoll();
            }
            else
            {
                State.PropertyChanged -= State_PropertyChanged;
                _poll?.Stop();   // 页面隐藏时暂停轮询，回到页面下一轮自动恢复（:93 注释）
            }
        };
        Unloaded += (_, _) => { _poll?.Stop(); State.PropertyChanged -= State_PropertyChanged; };
    }

    BridgeService Bridge => new(State.Paths.Root);

    void StartPoll()
    {
        _poll ??= new DispatcherTimer { Interval = TimeSpan.FromSeconds(3) };
        _poll.Tick += (_, _) => _ = PollAsync();
        _poll.Start();
    }

    void State_PropertyChanged(object? sender, System.ComponentModel.PropertyChangedEventArgs e)
    {
        if (e.PropertyName != nameof(LaunchSession.SelectedInstalled)) return;
        // watch(currentVersion)（:87-92）：换实例就清状态重来
        System.Threading.Interlocked.Increment(ref _statusReq);
        System.Threading.Interlocked.Increment(ref _manifestReq);
        _params = [];
        _connected = false;
        _present = null;
        SetPresenceError("");
        RenderParams();
        _ = RefreshPresenceAsync();
        _ = PollAsync();
    }

    // ---------------- 状态与清单 ----------------

    /// <summary>refreshBridgePresent（:39-49）：实例 mods 目录里有没有桥接 MOD。</summary>
    async System.Threading.Tasks.Task RefreshPresenceAsync()
    {
        var v = State.SelectedInstalled;
        if (v is null) { _present = null; SetPresenceError(""); return; }
        var req = System.Threading.Interlocked.Increment(ref _presenceReq);
        try
        {
            bool present = await System.Threading.Tasks.Task.Run(() => Bridge.IsInstalled(v.Id));
            if (req == _presenceReq) { _present = present; SetPresenceError(""); }
        }
        catch (Exception ex)
        {
            if (req == _presenceReq) { _present = null; SetPresenceError("无法确认桥接 MOD 是否已安装：" + ex.Message); }
        }
        Render();
    }

    /// <summary>poll（:93-100）：状态轮询；刚连上或清单为空时补拉清单。</summary>
    async System.Threading.Tasks.Task PollAsync()
    {
        if (!IsVisible || _installing) return;
        bool wasConnected = _connected;
        var v = State.SelectedInstalled;
        int req = System.Threading.Interlocked.Increment(ref _statusReq);
        if (v is null)
        {
            _connected = false;
            Render();   // status = { connected:false, reason:'未选择实例' }（:61）
            return;
        }
        try
        {
            var st = await Bridge.StatusAsync(v.Id);
            if (req != _statusReq) return;
            _connected = st.Connected;
            _statusModVersion = st.ModVersion;
            _statusReason = st.Reason;
        }
        catch
        {
            if (req != _statusReq) return;
            _connected = false;
            _statusReason = "桥接状态检查失败";   // :65
        }
        Render();
        if (_connected && (!wasConnected || _params.Count == 0))
            await LoadManifestAsync();
    }

    string _statusModVersion = "";
    string _statusReason = "";

    /// <summary>loadManifest（:67-85）。</summary>
    async System.Threading.Tasks.Task LoadManifestAsync()
    {
        var v = State.SelectedInstalled;
        if (v is null || _loadingManifest) return;
        var req = System.Threading.Interlocked.Increment(ref _manifestReq);
        _loadingManifest = true;
        Render();
        try
        {
            var ps = await Bridge.ManifestAsync(v.Id);
            if (req != _manifestReq) return;
            _params = ps.ToList();
        }
        catch (Exception ex)
        {
            if (req != _manifestReq) return;
            _params = [];
            if (_connected) State.Status = "读取参数清单失败：" + ex.Message;
        }
        finally
        {
            if (req == _manifestReq) _loadingManifest = false;
        }
        RenderParams();
        Render();
    }

    // ---------------- 渲染 ----------------

    /// <summary>状态卡的六态文案与四个引导动作（:195-234 的 v-if 链）。</summary>
    void Render()
    {
        var v = State.SelectedInstalled;
        CtxText.Text = v is null ? "尚未选择游戏实例" : v.Label;
        // 状态点：.on 才是 ok 绿（:294）
        BridgeDot.SetResourceReference(System.Windows.Shapes.Shape.FillProperty, _connected ? "Ok" : "Fg2");
        // .bridge-status.connected 的描边（:291 的 color-mix，退到 Accent 描边）
        StatusCard.SetResourceReference(Border.BorderBrushProperty, _connected ? "Accent" : "Line");

        bool gameRunning = State.GameRunning;
        if (_connected)
        {
            TxtStatusTitle.Text = $"已连接桥接 MOD（v{(string.IsNullOrEmpty(_statusModVersion) ? "?" : _statusModVersion)}）";
            TxtStatusDetail.Text = "参数修改即时下发，以游戏返回结果为准。";
        }
        else if (v is null)
        {
            TxtStatusTitle.Text = "先选择游戏实例";
            TxtStatusDetail.Text = "选择已安装加载器的实例后，可安装桥接 MOD。";
        }
        else if (_present == false)
        {
            TxtStatusTitle.Text = "尚未安装桥接 MOD";
            TxtStatusDetail.Text = "安装后启动游戏，即可调整支持的 MOD 参数。";
        }
        else if (_present == null)
        {
            TxtStatusTitle.Text = "尚未确认安装状态";
            TxtStatusDetail.Text = "请刷新确认安装情况，再进行操作。";
        }
        else if (gameRunning)
        {
            TxtStatusTitle.Text = "等待游戏接入";
            TxtStatusDetail.Text = string.IsNullOrEmpty(_statusReason) ? "前往首页启动游戏；此页面会自动检测连接。" : _statusReason;
        }
        else
        {
            TxtStatusTitle.Text = "启动游戏后自动接入";
            TxtStatusDetail.Text = string.IsNullOrEmpty(_statusReason) ? "前往首页启动游戏；此页面会自动检测连接。" : _statusReason;
        }

        // 引导动作显隐（:201-204）
        bool hasLoader = v is not null && (v.Kind == "loader" || v.Subtitle.Contains("fabric", StringComparison.OrdinalIgnoreCase)
            || v.Subtitle.Contains("forge", StringComparison.OrdinalIgnoreCase) || v.Subtitle.Contains("quilt", StringComparison.OrdinalIgnoreCase));
        BtnInstall.Visibility = !_connected && _present == false ? Visibility.Visible : Visibility.Collapsed;
        BtnInstall.IsEnabled = !_installing && v is not null && hasLoader;
        BtnInstall.Content = _installing ? "安装中…" : "安装桥接 MOD";
        TxtPureHint.Visibility = v is not null && !hasLoader && _present == false ? Visibility.Visible : Visibility.Collapsed;
        BtnNav.Visibility = !_connected ? Visibility.Visible : Visibility.Collapsed;
        BtnNav.Content = v is not null ? "前往首页" : "选择游戏实例";
        BtnNav.Style = (Style)FindResource(_present == false ? "BtnGhost" : "BtnGold");
        BtnResetAll.Visibility = _connected && _params.Count > 0 ? Visibility.Visible : Visibility.Collapsed;
        BtnRefresh.IsEnabled = !_installing;

        // 参数区整块（:208 v-if="status?.connected"）
        ParamsSection.Visibility = _connected ? Visibility.Visible : Visibility.Collapsed;
        ToolbarRow.Visibility = _connected && _params.Count > 0 ? Visibility.Visible : Visibility.Collapsed;
        LoadingCard.Visibility = _connected && _loadingManifest ? Visibility.Visible : Visibility.Collapsed;
        bool showEmptyNoParams = _connected && !_loadingManifest && _params.Count == 0;
        EmptyNoParams.Visibility = showEmptyNoParams ? Visibility.Visible : Visibility.Collapsed;
        EmptyNoMatch.Visibility = _connected && !_loadingManifest && _params.Count > 0 && FilteredCount() == 0
            ? Visibility.Visible : Visibility.Collapsed;
        if (EmptyNoMatch.Visibility == Visibility.Visible)
            EmptyNoMatchText.Text = $"没有匹配「{TxtSearch.Text.Trim()}」的参数";
    }

    void SetPresenceError(string msg)
    {
        PresenceError.Visibility = msg.Length == 0 ? Visibility.Collapsed : Visibility.Visible;
        PresenceErrorText.Text = msg;
    }

    /// <summary>groupedParams（:102-121）：按 modId → group 聚合，搜索即时过滤（名称/说明/分组/modId）。</summary>
    System.Collections.Generic.List<(string ModId, System.Collections.Generic.List<(string Group, System.Collections.Generic.List<BridgeService.BridgeParam> Items)>)> GroupedParams()
    {
        var kw = TxtSearch.Text.Trim().ToLowerInvariant();
        bool Match(BridgeService.BridgeParam p) => kw.Length == 0
            || p.Label.ToLowerInvariant().Contains(kw) || p.Description.ToLowerInvariant().Contains(kw)
            || p.Group.ToLowerInvariant().Contains(kw) || p.ModId.ToLowerInvariant().Contains(kw);

        var byMod = new System.Collections.Generic.OrderedDictionary<string, System.Collections.Generic.OrderedDictionary<string, List<BridgeService.BridgeParam>>>();
        foreach (var p in _params.Where(Match))
        {
            if (!byMod.TryGetValue(p.ModId, out var groups)) { groups = new(); byMod[p.ModId] = groups; }
            if (!groups.TryGetValue(p.Group, out var list)) { list = new(); groups[p.Group] = list; }
            list.Add(p);
        }
        return byMod.Select(m => (m.Key, m.Value.Select(g => (g.Key, g.Value)).ToList())).ToList();
    }

    int FilteredCount() => GroupedParams().Sum(g => g.Item2.Count);

    /// <summary>整棵参数树重建（清单变化 / 搜索变化时）。行结构 = BridgeRow 的样板（XAML 注释里的度量）。</summary>
    void RenderParams()
    {
        if (!_connected) { ParamList.Children.Clear(); return; }
        ParamList.Children.Clear();
        foreach (var (modId, groups) in GroupedParams())
            foreach (var (group, items) in groups)
                ParamList.Children.Add(BuildCard(modId, group, items));
    }

    UIElement BuildCard(string modId, string group, List<BridgeService.BridgeParam> items)
    {
        var card = new Border { Style = (Style)FindResource("Panel"), Padding = new Thickness(20, 16, 20, 16), Margin = new Thickness(0, 0, 0, 16) };
        var sp = new StackPanel();

        var head = new Border
        {
            BorderBrush = (System.Windows.Media.Brush)FindResource("Line"),
            BorderThickness = new Thickness(0, 0, 0, 1),
            Padding = new Thickness(0, 0, 0, 12),
        };
        var headSp = new StackPanel { Orientation = Orientation.Horizontal, VerticalAlignment = VerticalAlignment.Bottom };
        headSp.Children.Add(new TextBlock { FontSize = 13, FontWeight = FontWeights.Bold, Text = group });
        headSp.Children.Add(new TextBlock
        {
            Margin = new Thickness(12, 0, 0, 0),
            Style = (Style)FindResource("Micro"),
            FontFamily = (System.Windows.Media.FontFamily)FindResource("MonoFont"),
            Text = modId,
        });
        head.Child = headSp;
        sp.Children.Add(head);

        var rows = new StackPanel { Margin = new Thickness(0, 8, 0, 0) };
        for (int i = 0; i < items.Count; i++)
            rows.Children.Add(BuildRow(items[i], first: i == 0));
        sp.Children.Add(rows);
        card.Child = sp;
        return card;
    }

    /// <summary>一行参数（模板 :228-283）。Tag 里挂 (param, errorText) 供事件处理器取回。</summary>
    FrameworkElement BuildRow(BridgeService.BridgeParam p, bool first)
    {
        bool readOnly = !p.Visible || p.Scope == "SERVER";

        var row = new Border
        {
            Style = (Style)FindResource("BridgeRow"),
            BorderThickness = new Thickness(0, first ? 0 : 1, 0, 0),
            Tag = p,
        };
        var grid = new Grid();
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(16) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        row.Child = grid;

        var info = new StackPanel { VerticalAlignment = VerticalAlignment.Center };
        if (readOnly) info.Opacity = 0.55;   // .bridge-row.disabled — :313

        var labelRow = new StackPanel { Orientation = Orientation.Horizontal };
        labelRow.Children.Add(new TextBlock { FontSize = 13, FontWeight = FontWeights.SemiBold, VerticalAlignment = VerticalAlignment.Center, Text = p.Label });
        string hint = ApplyHint(p.Apply);
        if (hint.Length > 0)
        {
            var tag = new Border { Style = (Style)FindResource("Tag"), Padding = new Thickness(8, 1, 8, 1), Margin = new Thickness(8, 0, 0, 0) };
            tag.Child = new TextBlock { Style = (Style)FindResource("TagText"), Text = hint };
            tag.SetResourceReference(TextBlock.ForegroundProperty, "Fg2");
            labelRow.Children.Add(tag);
        }
        if (p.Scope == "SERVER")
        {
            var tag = new Border
            {
                Style = (Style)FindResource("Tag"),
                Padding = new Thickness(8, 1, 8, 1),
                Margin = new Thickness(8, 0, 0, 0),
                ToolTip = "服务器参数：必须由服务端校验权限，本地接口只读",
            };
            tag.Child = new TextBlock { Style = (Style)FindResource("TagText"), Text = "服务器" };
            tag.SetResourceReference(TextBlock.ForegroundProperty, "Fg2");
            labelRow.Children.Add(tag);
        }
        info.Children.Add(labelRow);

        if (p.Description.Length > 0)
            info.Children.Add(new TextBlock
            {
                Margin = new Thickness(0, 4, 0, 0),
                Style = (Style)FindResource("Micro"),
                TextWrapping = TextWrapping.Wrap,
                Text = p.Description,
            });

        // itemError 行（:228）：修改/恢复失败时在信息区下面出 12px danger 文本
        var errText = new TextBlock
        {
            Margin = new Thickness(0, 4, 0, 0),
            FontSize = 12,
            TextWrapping = TextWrapping.Wrap,
            Visibility = Visibility.Collapsed,
        };
        errText.SetResourceReference(TextBlock.ForegroundProperty, "Danger");
        info.Children.Add(errText);
        row.Tag = (p, errText);
        grid.Children.Add(info);

        var ctl = new StackPanel { Orientation = Orientation.Horizontal, VerticalAlignment = VerticalAlignment.Center };
        Grid.SetColumn(ctl, 2);

        switch (p.Kind)
        {
            case "SWITCH":
            {
                var sw = new CheckBox { Style = (Style)FindResource("Switch"), Width = 40, Height = 22, VerticalAlignment = VerticalAlignment.Center, IsChecked = p.Value is { ValueKind: JsonValueKind.True }, IsEnabled = !readOnly };
                sw.Checked += (_, _) => _ = ApplyAsync(p, true, row);
                sw.Unchecked += (_, _) => _ = ApplyAsync(p, false, row);
                ctl.Children.Add(sw);
                break;
            }
            case "SLIDER":
            {
                double val = p.Value is { ValueKind: JsonValueKind.Number } n ? n.GetDouble() : p.Min ?? 0;
                var slider = new Slider
                {
                    Style = (Style)FindResource("BridgeSlider"),
                    Minimum = p.Min ?? 0,
                    Maximum = p.Max ?? 100,
                };
                slider.Value = Math.Clamp(val, slider.Minimum, slider.Maximum);
                slider.IsEnabled = !readOnly;
                var num = new TextBlock
                {
                    Margin = new Thickness(12, 0, 0, 0),
                    MinWidth = 44,
                    TextAlignment = TextAlignment.Right,
                    FontSize = 12,
                    VerticalAlignment = VerticalAlignment.Center,
                    Text = JsonText(p.Value),
                };
                num.SetResourceReference(TextBlock.ForegroundProperty, "Fg2");
                slider.ValueChanged += (_, _) => num.Text = FormatNum(slider.Value, p);
                // 原版是 @change（松手才下发），WPF 对应 Thumb.DragCompleted；不用 ValueChanged 连打接口
                slider.AddHandler(System.Windows.Controls.Primitives.Thumb.DragCompletedEvent,
                    new System.Windows.Controls.Primitives.DragCompletedEventHandler((_, _) =>
                        _ = ApplyAsync(p, slider.Value, row)));   // 数值参数必须按 JSON number 下发
                ctl.Children.Add(slider);
                ctl.Children.Add(num);
                break;
            }
            case "TEXT":
            {
                var box = new TextBox { Style = (Style)FindResource("Input"), Width = 200, VerticalAlignment = VerticalAlignment.Center, Text = JsonText(p.Value), IsEnabled = !readOnly };
                box.LostFocus += (_, _) => { if (box.Text != JsonText(p.Value)) _ = ApplyAsync(p, box.Text, row); };
                box.KeyDown += (_, ke) => { if (ke.Key == System.Windows.Input.Key.Enter) { if (box.Text != JsonText(p.Value)) _ = ApplyAsync(p, box.Text, row); } };
                ctl.Children.Add(box);
                break;
            }
            default:   // 下拉（SelectMenu 的等价物，:260-267）
            {
                var cb = new ComboBox { Width = 160, VerticalAlignment = VerticalAlignment.Center, IsEnabled = !readOnly };
                foreach (var o in p.Options ?? Array.Empty<string>()) cb.Items.Add(o);
                string cur = JsonText(p.Value);
                int idx = (p.Options ?? Array.Empty<string>()).ToList().IndexOf(cur);
                cb.SelectedIndex = idx >= 0 ? idx : (cb.Items.Count > 0 ? 0 : -1);
                cb.SelectionChanged += (_, _) => { if (cb.SelectedItem is string s && s != cur) _ = ApplyAsync(p, s, row); };
                ctl.Children.Add(cb);
                break;
            }
        }

        // isModified（:179-182）→ 恢复默认按钮（:268-276）；SERVER 只读禁用（:272）
        if (IsModified(p))
        {
            var reset = new Button { Style = (Style)FindResource("BridgeReset"), Margin = new Thickness(12, 0, 0, 0), IsEnabled = p.Scope != "SERVER" };
            reset.Content = new TextBlock { Text = "↺", FontSize = 13, HorizontalAlignment = System.Windows.HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center };
            reset.Click += (_, _) => _ = ResetParamAsync(p, row);
            ctl.Children.Add(reset);
        }

        grid.Children.Add(ctl);
        return row;
    }

    static string JsonText(JsonElement? e) => e switch
    {
        null => "",
        { ValueKind: JsonValueKind.String } s => s.GetString() ?? "",
        { ValueKind: JsonValueKind.Number } n => n.GetRawText(),
        { ValueKind: JsonValueKind.True } => "true",
        { ValueKind: JsonValueKind.False } => "false",
        var o => o!.Value.GetRawText(),
    };

    static string FormatNum(double v, BridgeService.BridgeParam p)
        => p.Step is { } st && st < 1 ? v.ToString("0.#") : ((int)Math.Round(v)).ToString();

    /// <summary>isModified（:179-182）＝ JSON.stringify 比较。Value 未知（null）时按 JSON null 比。</summary>
    static bool IsModified(BridgeService.BridgeParam p)
        => (p.Value?.GetRawText() ?? "null") != p.Default.GetRawText();

    // ---------------- 操作 ----------------

    /// <summary>applyParam（:139-157）：set 成功以返回 value 为准，notice 弹状态栏，失败落到行内 error。</summary>
    async System.Threading.Tasks.Task ApplyAsync(BridgeService.BridgeParam p, object value, Border row)
    {
        var v = State.SelectedInstalled;
        if (v is null) return;
        var (_, errText) = ((BridgeService.BridgeParam, TextBlock))row.Tag;
        errText.Visibility = Visibility.Collapsed;
        try
        {
            var result = await Bridge.SetAsync(v.Id, p.Id, value);
            if (result.ValueKind == JsonValueKind.Object && result.TryGetProperty("ok", out var ok) && !ok.GetBoolean())
            {
                var msg = result.TryGetProperty("error", out var er) ? er.GetString() ?? "修改失败" : "修改失败";
                errText.Text = msg;
                errText.Visibility = Visibility.Visible;
                return;
            }
            if (result.ValueKind == JsonValueKind.Object && result.TryGetProperty("value", out var nv))
                p.Value = nv.Clone();
            if (result.ValueKind == JsonValueKind.Object && result.TryGetProperty("notice", out var no))
            {
                var note = no.GetString();
                if (!string.IsNullOrEmpty(note)) State.Status = note;   // toast('info') 的等价落点
            }
            RenderParams();   // value 变了会加减「恢复默认」钮，整树轻量重建
        }
        catch (Exception ex)
        {
            errText.Text = ex.Message;
            errText.Visibility = Visibility.Visible;
        }
    }

    /// <summary>resetParam（:162-176）。</summary>
    async System.Threading.Tasks.Task ResetParamAsync(BridgeService.BridgeParam p, Border row)
    {
        var v = State.SelectedInstalled;
        if (v is null) return;
        var (_, errText) = ((BridgeService.BridgeParam, TextBlock))row.Tag;
        errText.Visibility = Visibility.Collapsed;
        try
        {
            await Bridge.ResetAsync(v.Id, p.Id);
            p.Value = p.Default.Clone();
            State.Status = $"「{p.Label}」已恢复默认";
            RenderParams();
        }
        catch (Exception ex)
        {
            errText.Text = "恢复失败：" + ex.Message;
            errText.Visibility = Visibility.Visible;
        }
    }

    /// <summary>resetAll（:173-184）：全恢复后重拉清单。</summary>
    async void ResetAll_Click(object sender, RoutedEventArgs e)
    {
        var v = State.SelectedInstalled;
        if (v is null) return;
        try
        {
            await Bridge.ResetAsync(v.Id);
            await LoadManifestAsync();
            State.Status = "已恢复全部默认配置";
        }
        catch (Exception ex) { State.Status = "恢复失败：" + ex.Message; }
    }

    /// <summary>onInstallBridge（:51-69）：内嵌 jar 复制进实例 mods；成败都给状态栏反馈。</summary>
    async void Install_Click(object sender, RoutedEventArgs e)
    {
        var v = State.SelectedInstalled;
        if (v is null || _installing) return;
        _installing = true;
        Render();
        try
        {
            await System.Threading.Tasks.Task.Run(() => Bridge.Install(v.Id));
            _present = true;
            SetPresenceError("");
            State.Status = "桥接 MOD 已装入实例，启动游戏后自动接入";
            State.Log($"桥接 MOD 已装入 {v.Id}");
        }
        catch (Exception ex)
        {
            State.Status = "安装失败：" + ex.Message;
        }
        finally
        {
            _installing = false;
            Render();
            _ = PollAsync();
        }
    }

    /// <summary>store.currentView = currentVersion ? 'home' : 'game'（:203）。</summary>
    void Nav_Click(object sender, RoutedEventArgs e)
        => (Application.Current.MainWindow as MainWindow)?.Show(State.SelectedInstalled is not null ? "home" : "versions");

    /// <summary>「刷新状态」（:193）：presence + 状态一起刷。</summary>
    async void Refresh_Click(object sender, RoutedEventArgs e)
    {
        await RefreshPresenceAsync();
        await PollAsync();
    }

    void Search_TextChanged(object sender, TextChangedEventArgs e)
    {
        SearchPh.Visibility = TxtSearch.Text.Length == 0 ? Visibility.Visible : Visibility.Collapsed;
        if (_connected) { RenderParams(); Render(); }
    }
}
