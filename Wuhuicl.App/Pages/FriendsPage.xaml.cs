using System.Diagnostics;
using System.IO;
using System.Windows;
using System.Windows.Controls;
using Microsoft.Win32;
using Wuhuicl.App.Services;

namespace Wuhuicl.App.Pages;

/// <summary>
/// 联机（主导航项）：KAMUCL-1.1.5/src/renderer/src/views/FriendConnectView.vue 的 WPF 复刻。
///
/// 页内状态机而不是页签页：type ConnectPage = 'choose' | 'frp' | 'voxlink' | 'terracotta'（:11-12），
/// choose 是方式选择落地页（:54-81），点卡片调 pick(card.key)（:43-45）进方式页（:84-101），
/// 方式页的「← 更换方式」把状态写回 choose（:94）。WPF 用 ChooseView / MethodView 两个兄弟容器互斥显隐表达，
/// 方式页标题与描述取自被点卡片自己那份写死的文案，避免同一段话在 XAML 与 C# 各存一份。
///
/// 三个面板（FrpPanel / VoxLinkPanel / TerracottaPanel，模板 :98-100）本轮只铺小节标题占位，正文另开工。
///
/// 契约 §5 的 LaunchSession 白名单里没有任何联机方式、隧道、房间码字段，所以整页零 Binding；
/// 事件都留在本页（契约 §1），不引用 MainWindow 的任何方法。唯一的代码期工作是
/// @container(min-width:960px) 这条 WPF 表达不了的断点（:161）。
/// </summary>
public partial class FriendsPage : UserControl
{
    /// <summary>container query 阈值：.friend-connect-page 自身 inline-size 960px — FriendConnectView.vue:161。</summary>
    const double WideFrom = 960;

    /// <summary>窄档列表列数：@container 未命中时 .pick-list 是 minmax(0,1fr) 单列（:161）。</summary>
    const int NarrowColumns = 1;

    /// <summary>宽档列表列数：repeat(3, minmax(0,1fr))（:161）。</summary>
    const int WideColumns = 3;

    /// <summary>三张卡与三个面板的对应关系，顺序即 methodCards 的声明顺序（:16 / :25 / :33）。
    /// 元组的元素名必须写死在字段类型上，否则带名字的元组数组无法从字面量隐式转换（CS0029）。</summary>
    (string Key, TextBlock Name, TextBlock Desc, Border Panel)[] _methods = null!;

    Window? _host;

    public FriendsPage()
    {
        InitializeComponent();

        _methods = new[]
        {
            ("frp", PickName1, PickDesc1, MethodFrp),
            ("voxlink", PickName2, PickDesc2, MethodVoxlink),
            ("terracotta", PickName3, PickDesc3, MethodTerracotta),
        };

        Loaded += OnLoaded;
        Unloaded += OnUnloaded;

        // FRP：frpc 客户端路径默认从 DataDir\frpc\ 自动找（用户也可「浏览…」手选）
        TxtFrpcPath.Text = NatfrpService.FindFrpc() ?? "";
    }

    void OnLoaded(object sender, RoutedEventArgs e)
    {
        _host = Window.GetWindow(this);
        if (_host != null) _host.SizeChanged += Host_SizeChanged;
        ShowChoose();
        ApplyMetrics();
    }

    void OnUnloaded(object sender, RoutedEventArgs e)
    {
        if (_host != null) _host.SizeChanged -= Host_SizeChanged;
        _host = null;
        VoxOnUnload(); // 房间靠心跳保活，页面卸载后无法继续发心跳 → 主动退房让服务端回收
    }

    void Host_SizeChanged(object? sender, SizeChangedEventArgs e) => ApplyMetrics();

    /// <summary>container-type:inline-size（:161）让断点量的是本页容器宽度而不是视口，
    /// 所以这里直接用 PageRoot.ActualWidth；设计期没有宿主窗口时按外壳宽反推
    /// （侧栏 208 + 页边距 48，同 ServersPage.xaml.cs 的 ViewportWidth 思路）。</summary>
    double ContainerWidth()
    {
        double w = PageRoot.ActualWidth;
        return w > 0 ? w : (_host?.ActualWidth ?? 1280) - 256;
    }

    /// <summary>把 @container(min-width:960px) 落成真实列数（:161 的 minmax(0,1fr) 与 repeat(3,minmax(0,1fr)) 两档）。
    /// 卡片内部只铺宽档的竖排分支（flex-direction column / gap 14），窄档的横排 gap 24（:127）不另开一套，
    /// 这是本轮已知的版式偏差，接线时可以在此处一并换模板。</summary>
    void ApplyMetrics()
    {
        PickList.Columns = ContainerWidth() >= WideFrom ? WideColumns : NarrowColumns;
    }

    // ---------------- choose 与方式页的互斥切换（模板 :54 与 :84 的 v-if/v-else） ----------------
    void Pick_Click(object sender, RoutedEventArgs e)
    {
        if (sender is not Button { Tag: string key }) return;
        foreach (var m in _methods)
        {
            if (m.Key != key) continue;
            // h1 = currentCard.name、p = currentCard.desc（模板 :90-91），直接取卡片自己那份文案
            MethodTitle.Text = m.Name.Text;
            MethodDesc.Text = m.Desc.Text;
            MethodView.Visibility = Visibility.Visible;
            ChooseView.Visibility = Visibility.Collapsed;
            m.Panel.Visibility = Visibility.Visible;
            return;
        }
    }

    void Back_Click(object sender, RoutedEventArgs e) => ShowChoose();

    /// <summary>page 状态回到 choose：卡片列表显示、方式页与三个面板全部收起。</summary>
    void ShowChoose()
    {
        ChooseView.Visibility = Visibility.Visible;
        MethodView.Visibility = Visibility.Collapsed;
        foreach (var m in _methods) m.Panel.Visibility = Visibility.Collapsed;
    }

    // 供主控接线时定位：原版靠 :51 的 Transition name="subpage" :duration="200" 做右滑渐入，
    // 契约 §6.8 本轮不做动画，两个模式容器都留了 x:Name（ChooseView / MethodView）当动画挂点；
    // 卡片 hover 的描边换色在 PickCard 模板里（x:Name=CardBorder），:149/:159 的 overshoot 缓动留在这里接。
    // MethodFrp / MethodVoxlink / MethodTerracotta 是三个面板的占位壳（// 占位：等联机功能接入）。

    // ---------------- FRP（樱花穿透 natfrp.com）：token → 隧道列表 → 启停 frpc ----------------

    /// <summary>最近一次读取的隧道列表。</summary>
    List<NatfrpTunnel> _tunnels = new();

    /// <summary>用户填写的访问密钥（启动 frpc 时要用，不落盘）。</summary>
    string _frpToken = "";

    /// <summary>正在运行的 frpc 进程，key = 隧道 ID（一个隧道同时只跑一个 frpc）。</summary>
    readonly Dictionary<uint, Process> _frpcProcs = new();

    async void FrpRefresh_Click(object sender, RoutedEventArgs e)
    {
        var token = FrpTokenBox.Password.Trim();
        if (token.Length == 0)
        {
            SetFrpStatus("请先填写访问密钥（natfrp.com → 用户面板 → 访问密钥）。", true);
            return;
        }
        _frpToken = token;
        BtnFrpRefresh.IsEnabled = false;
        SetFrpStatus("正在读取隧道列表…", false);
        try
        {
            _tunnels = await NatfrpService.GetTunnels(token);
            RenderTunnels();
            SetFrpStatus(_tunnels.Count == 0
                ? "账号下没有隧道，请先到 natfrp.com 创建。"
                : $"已加载 {_tunnels.Count} 条隧道。", false);
        }
        catch (Exception ex)
        {
            _tunnels = new List<NatfrpTunnel>();
            RenderTunnels();
            SetFrpStatus("读取失败：" + ex.Message, true);
        }
        finally
        {
            BtnFrpRefresh.IsEnabled = true;
        }
    }

    void FrpcBrowse_Click(object sender, RoutedEventArgs e)
    {
        var dlg = new OpenFileDialog
        {
            Title = "选择 frpc 可执行文件",
            Filter = "frpc 可执行文件|frpc*.exe|所有程序|*.exe|所有文件|*.*",
        };
        var dir = NatfrpService.FrpcDir;
        if (Directory.Exists(dir)) dlg.InitialDirectory = dir;
        if (dlg.ShowDialog() == true)
        {
            TxtFrpcPath.Text = dlg.FileName;
            SetFrpStatus("已选择 frpc 客户端。", false);
        }
    }

    void RenderTunnels()
    {
        TunnelList.Children.Clear();
        TunnelEmpty.Visibility = _tunnels.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
        foreach (var t in _tunnels)
            TunnelList.Children.Add(MakeTunnelRow(t));
    }

    Border MakeTunnelRow(NatfrpTunnel t)
    {
        var row = new Border
        {
            BorderBrush = (System.Windows.Media.Brush)FindResource("Line"),
            BorderThickness = new Thickness(0, 0, 0, 1),
            Padding = new Thickness(0, 10, 0, 10),
        };
        var g = new Grid();
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(12) });
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });

        // 左侧：名称 + 节点 / 协议与本地→公网地址
        var info = new StackPanel { MinWidth = 0 };
        Grid.SetColumn(info, 0);
        g.Children.Add(info);

        var head = new StackPanel { Orientation = Orientation.Horizontal };
        var name = new TextBlock { Text = t.Name, FontSize = 14, FontWeight = FontWeights.SemiBold, VerticalAlignment = VerticalAlignment.Center };
        name.SetResourceReference(TextBlock.ForegroundProperty, "Fg");
        head.Children.Add(name);
        if (!string.IsNullOrEmpty(t.Node))
        {
            var nodeTag = new Border
            {
                Style = (Style)FindResource("Tag"),
                Background = (System.Windows.Media.Brush)FindResource("CardSoft"),
                BorderThickness = new Thickness(0),
                Margin = new Thickness(8, 0, 0, 0),
                VerticalAlignment = VerticalAlignment.Center,
            };
            nodeTag.Child = new TextBlock
            {
                Text = "节点 " + t.Node,
                Style = (Style)FindResource("TagTextSm"),
                Foreground = (System.Windows.Media.Brush)FindResource("Fg2"),
            };
            head.Children.Add(nodeTag);
        }
        info.Children.Add(head);

        var addr = new TextBlock
        {
            Style = (Style)FindResource("Micro"),
            LineHeight = 19,
            Margin = new Thickness(0, 4, 0, 0),
            TextWrapping = TextWrapping.Wrap,
            Text = $"{t.Protocol.ToUpperInvariant()} · 本地 {t.LocalAddr}:{t.LocalPort} → 公网 {t.RemoteAddr}:{t.RemotePort}"
                   + (t.Enabled ? "" : "（已停用）"),
        };
        addr.SetResourceReference(TextBlock.ForegroundProperty, "Fg2");
        info.Children.Add(addr);

        // 右侧：启动 / 停止
        var btn = new Button
        {
            Style = (Style)FindResource("BtnGhost"),
            MinWidth = 64,
            VerticalAlignment = VerticalAlignment.Center,
            Content = _frpcProcs.ContainsKey(t.Id) ? "停止" : "启动",
        };
        btn.Click += async (_, _) => await ToggleTunnelAsync(t, btn);
        Grid.SetColumn(btn, 2);
        g.Children.Add(btn);

        row.Child = g;
        return row;
    }

    async Task ToggleTunnelAsync(NatfrpTunnel t, Button btn)
    {
        // 运行中 → 停止
        if (_frpcProcs.TryGetValue(t.Id, out var running))
        {
            NatfrpService.StopFrpc(running);
            _frpcProcs.Remove(t.Id);
            btn.Content = "启动";
            SetFrpStatus($"隧道 {t.Name} 已停止。", false);
            return;
        }

        btn.IsEnabled = false;
        try
        {
            var proc = await NatfrpService.StartFrpc(TxtFrpcPath.Text, _frpToken, t);
            if (proc.HasExited)
                throw new InvalidOperationException("frpc 启动后立即退出，请检查访问密钥是否正确、frpc 是否为樱花穿透定制版");
            proc.EnableRaisingEvents = true;
            proc.Exited += (_, _) => Dispatcher.Invoke(() =>
            {
                if (_frpcProcs.TryGetValue(t.Id, out var p) && ReferenceEquals(p, proc))
                {
                    _frpcProcs.Remove(t.Id);
                    btn.Content = "启动";
                }
            });
            _frpcProcs[t.Id] = proc;
            btn.Content = "停止";
            SetFrpStatus($"隧道 {t.Name} 已启动：游戏内直接连接 {t.RemoteAddr}:{t.RemotePort}", false);
        }
        catch (Exception ex)
        {
            SetFrpStatus("启动失败：" + ex.Message, true);
        }
        finally
        {
            btn.IsEnabled = true;
        }
    }

    void SetFrpStatus(string msg, bool warn)
    {
        FrpStatus.Text = msg;
        FrpStatus.SetResourceReference(TextBlock.ForegroundProperty, warn ? "Danger" : "Fg2");
    }

    // ---------------- VoxLink（自研房间服务，Services/VoxLinkService.cs）：建房间 / 加入 / 心跳保活 / 退出 ----------------

    /// <summary>心跳定时器：房间创建/加入成功后启动，间隔取服务端返回的 heartbeatInterval。</summary>
    System.Windows.Threading.DispatcherTimer? _voxTimer;

    /// <summary>当前会话：创建/加入成功后填充（code + token 是 leave/heartbeat 的凭证）。</summary>
    string _voxCode = "", _voxToken = "";

    bool _voxIsHost;

    /// <summary>心跳序号（服务端 body.seq，桌面端同样递增，session.go heartbeatLoop :156）。</summary>
    long _voxSeq;

    /// <summary>连续心跳失败计数（服务端抖动容忍，连续多次才判失效）。</summary>
    int _voxFails;

    /// <summary>致命错误码：收到即结束会话（对应桌面端 410/ fatal 判定，session.go :171-176）。</summary>
    static readonly HashSet<string> VoxFatalCodes = new()
    {
        "ROOM_EXPIRED", "ROOM_CLOSED", "ROOM_EVICTED", "ROOM_NOT_FOUND",
        "ROOM_NOT_FOUND_OR_WRONG_PASSWORD", "INVALID_TOKEN", "SESSION_ACTIVE_FATAL",
    };

    /// <summary>会话是否存活（心跳回调与页面卸载竞态的护栏）。</summary>
    bool VoxActive => _voxCode.Length > 0 && _voxToken.Length > 0;

    void VoxSetBusy(bool creating, bool joining, bool leaving)
    {
        BtnVoxCreate.IsEnabled = !creating && !leaving && !VoxActive;
        BtnVoxJoin.IsEnabled = !joining && !leaving && !VoxActive;
        BtnVoxLeave.IsEnabled = !creating && !joining;
    }

    void SetVoxStatus(string msg, bool warn)
    {
        VoxStatus.Text = msg;
        VoxStatus.SetResourceReference(TextBlock.ForegroundProperty, warn ? "Danger" : "Fg2");
    }

    async void VoxCreate_Click(object sender, RoutedEventArgs e)
    {
        if (VoxActive)
        {
            SetVoxStatus("已有进行中的房间，请先退出再操作。", true);
            return;
        }

        string baseUrl, name, password;
        int port;
        try
        {
            baseUrl = VoxLinkService.NormalizeBaseUrl(TxtVoxServer.Text);
            name = TxtVoxName.Text.Trim();
            password = VoxCreatePwdBox.Password;
            port = int.TryParse(TxtVoxPort.Text.Trim(), out var p) ? p : 25565;
            if (port < 1 || port > 65535)
                throw new InvalidOperationException("端口必须是 1-65535 的数字");
        }
        catch (Exception ex)
        {
            SetVoxStatus(ex.Message, true);
            return;
        }

        // 与桌面端一致：本地端口没人在听就不建房（app-desktop/app.go PORT_NOT_LISTENING :466-469）
        VoxSetBusy(creating: true, joining: false, leaving: false);
        SetVoxStatus($"正在检查端口 {port} 是否有服务在监听…", false);
        if (!await VoxLinkService.TestTcpAsync("127.0.0.1", port))
        {
            VoxSetBusy(creating: false, joining: false, leaving: false);
            SetVoxStatus($"端口 {port} 没有服务在监听：请先在游戏里对局域网开放世界（开放后聊天栏会显示真实端口），或启动服务端后再试。", true);
            return;
        }

        SetVoxStatus("正在创建房间…", false);
        try
        {
            var room = await VoxLinkService.CreateRoomAsync(baseUrl, name, port, password);
            VoxStartSession(room.Code, room.HostToken, isHost: true);
            VoxShowRoom(room.Code,
                $"房主视图 · 本机地址 127.0.0.1:{room.HostPort}（在游戏内对局域网开放的就是它）· 有效期约 {room.ExpiresIn} 秒（自动续期）");
            SetVoxStatus($"房间已创建，把房间码 {room.Code} 发给好友即可。", false);
        }
        catch (VoxLinkException vex)
        {
            SetVoxStatus("创建失败：" + vex.Message, true);
        }
        catch (Exception ex)
        {
            SetVoxStatus("创建失败：" + ex.Message, true);
        }
        finally
        {
            VoxSetBusy(creating: false, joining: false, leaving: false);
        }
    }

    async void VoxJoin_Click(object sender, RoutedEventArgs e)
    {
        if (VoxActive)
        {
            SetVoxStatus("已在房间中，请先退出再操作。", true);
            return;
        }

        string baseUrl, code, password;
        try
        {
            baseUrl = VoxLinkService.NormalizeBaseUrl(TxtVoxServer.Text);
            code = TxtVoxCode.Text.Trim().ToUpperInvariant();
            password = TxtVoxJoinPwd.Password;
            if (!VoxLinkService.ValidateRoomCode(code))
                throw new InvalidOperationException("房间码必须为 6 位（A-Z 去掉 I/O、2-9 去掉 0/1）");
        }
        catch (Exception ex)
        {
            SetVoxStatus(ex.Message, true);
            return;
        }

        VoxSetBusy(joining: true, creating: false, leaving: false);
        SetVoxStatus($"正在加入房间 {code}…", false);
        try
        {
            var result = await VoxLinkService.JoinRoomAsync(baseUrl, code, password);
            VoxStartSession(code, result.ClientToken, isHost: false);
            var room = result.Room;
            string info = room == null
                ? "已加入房间"
                : $"房间「{room.Name}」· {room.CurrentPlayers}/{room.MaxPlayers} 人"
                  + (room.HasPassword ? " · 有密码" : "")
                  + (room.HostPort > 0 ? $" · 主机端口 {room.HostPort}" : "");
            VoxShowRoom(code, info + " · 请向房主索取游戏内直连地址（房主面板显示 127.0.0.1:端口）。");
            SetVoxStatus($"已加入房间 {code}。", false);
        }
        catch (VoxLinkException vex)
        {
            SetVoxStatus("加入失败：" + vex.Message, true);
        }
        catch (Exception ex)
        {
            SetVoxStatus("加入失败：" + ex.Message, true);
        }
        finally
        {
            VoxSetBusy(joining: false, creating: false, leaving: false);
        }
    }

    async void VoxLeave_Click(object sender, RoutedEventArgs e)
    {
        if (!VoxActive) return;
        VoxSetBusy(leaving: true, creating: false, joining: false);
        var (code, token, isHost) = (_voxCode, _voxToken, _voxIsHost);
        VoxStopHeartbeat();
        _voxCode = _voxToken = "";
        VoxHideRoom();
        await VoxLinkService.LeaveAsync(TxtVoxServer.Text, code, token, isHost);
        SetVoxStatus($"已退出房间 {code}。", false);
        VoxSetBusy(leaving: false, creating: false, joining: false);
    }

    /// <summary>会话建立：填状态 + 启动心跳（房间不心跳会被服务端按超时回收）。</summary>
    void VoxStartSession(string code, string token, bool isHost)
    {
        _voxCode = code;
        _voxToken = token;
        _voxIsHost = isHost;

        _voxSeq = 0;
        _voxFails = 0;

        _voxTimer = new System.Windows.Threading.DispatcherTimer { Interval = TimeSpan.FromSeconds(5) };
        _voxTimer.Tick += async (_, _) => await VoxHeartbeatTickAsync();
        _voxTimer.Start();
    }

    void VoxStopHeartbeat()
    {
        _voxTimer?.Stop();
        _voxTimer = null;
    }

    void VoxShowRoom(string code, string info)
    {
        VoxRoomCode.Text = code;
        VoxRoomInfo.Text = info;
        VoxRoomCard.Visibility = Visibility.Visible;
        BtnVoxLeave.Visibility = Visibility.Visible;
    }

    void VoxHideRoom()
    {
        VoxRoomCard.Visibility = Visibility.Collapsed;
        BtnVoxLeave.Visibility = Visibility.Collapsed;
        VoxRoomCode.Text = "——";
        VoxRoomInfo.Text = "";
    }

    async Task VoxHeartbeatTickAsync()
    {
        if (!VoxActive || _voxTimer == null) return;
        _voxSeq++;
        try
        {
            var hb = await VoxLinkService.HeartbeatAsync(TxtVoxServer.Text, _voxCode, _voxToken, _voxIsHost, _voxSeq);
            _voxFails = 0;
            if (hb.CurrentPlayers > 0 && VoxRoomInfo.Text.Length > 0)
            {
                // 把房间信息行里的人数刷成服务端口径（简单起见整行重建仅当有变化的关键字）
                var text = VoxRoomInfo.Text;
                var updated = System.Text.RegularExpressions.Regex.Replace(
                    text, @"\d+/\d+ 人", $"{hb.CurrentPlayers}/{(ParseMax(text))} 人");
                if (updated != text) VoxRoomInfo.Text = updated;
            }
            if (hb.HeartbeatInterval > 0 && _voxTimer != null)
                _voxTimer.Interval = TimeSpan.FromSeconds(Math.Clamp(hb.HeartbeatInterval, 3, 60));
        }
        catch (VoxLinkException vex) when (VoxFatalCodes.Contains(vex.Code))
        {
            VoxEndSession("房间已失效：" + vex.Message, warn: true);
        }
        catch (Exception)
        {
            // 网络抖动容忍（桌面端同款退避重试，session.go heartbeatBackoff），连续多次才放弃
            if (++_voxFails >= 5)
                VoxEndSession("与服务器失联（连续多次心跳失败），房间可能已被回收。", warn: true);
        }
    }

    /// <summary>从信息行里抠回 maxPlayers（房间信息行格式「x/y 人」由 join 响应写入）。</summary>
    static int ParseMax(string info)
    {
        var m = System.Text.RegularExpressions.Regex.Match(info, @"\d+/(\d+) 人");
        return m.Success && int.TryParse(m.Groups[1].Value, out var v) ? v : 20;
    }

    /// <summary>会话终止（房间失效/失联）：停心跳、清状态、收起房间卡。</summary>
    void VoxEndSession(string message, bool warn)
    {
        VoxStopHeartbeat();
        _voxCode = _voxToken = "";
        VoxHideRoom();
        SetVoxStatus(message, warn);
        VoxSetBusy(creating: false, joining: false, leaving: false);
    }

    /// <summary>页面卸载：停心跳并尽力退房（无心跳的房间服务端也会按超时回收，leave 只是加速）。</summary>
    async void VoxOnUnload()
    {
        if (!VoxActive) return;
        var (code, token, isHost) = (_voxCode, _voxToken, _voxIsHost);
        VoxStopHeartbeat();
        _voxCode = _voxToken = "";
        VoxHideRoom();
        await VoxLinkService.LeaveAsync(TxtVoxServer.Text, code, token, isHost);
    }
}
