using System.IO;
using System.IO.Compression;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using Wuhuicl.App.Services;
using Wuhuicl.Core.Minecraft;
using Wuhuicl.Core.Storage;

namespace Wuhuicl.App;

/// <summary>
/// 实例管理中心（InstanceCenter.vue 的 WPF 等价物，代码构建 UI）：
/// 概览（目录 + 四枚入口 + 复制实例）/ 存档（saves 扫描 + 打开/导出 ZIP/备份）/
/// 截图（screenshots 网格 + 另存为）/ 备份（DataDir\backups 清单 + 恢复/备份实例）/
/// 诊断（Java 扫描 + 为此实例应用 + 导出诊断日志）。
/// 目录语义与启动链一致：DefaultIsolation=true → versions\&lt;id&gt;（独立游戏目录），否则共享根。
/// </summary>
public sealed class InstanceCenter
{
    readonly MainWindow _shell;
    readonly LaunchSession _s;
    Grid _host = null!;
    Border _mask = null!;
    Border _modal = null!;
    TextBlock _title = null!, _sub = null!;
    StackPanel _tabsPanel = null!;
    ContentControl _body = null!;
    Button[] _tabButtons = null!;

    string _id = "";
    string _mcVersion = "";
    string _tab = "overview";
    string Dir => _s.Settings.Value.DefaultIsolation ? _s.Paths.VersionDir(_id) : _s.Paths.Root;
    bool Shared => !(_s.Settings.Value.DefaultIsolation && _s.Settings.Value.GameDirOverride is null);

    static string BackupsDir => Path.Combine(GamePaths.DetectDefault().DataDir, "backups");

    public InstanceCenter(MainWindow shell, LaunchSession s) { _shell = shell; _s = s; }

    // ---------------- 壳 ----------------

    public FrameworkElement BuildHost()
    {
        _host = new Grid { Visibility = Visibility.Collapsed };
        System.Windows.Controls.Panel.SetZIndex(_host, 9500);
        _mask = new Border
        {
            Background = new SolidColorBrush(Color.FromArgb(0x99, 0, 0, 0)),
        };
        // backdrop blur 用 MaskBlur 不可行（快照成本），原版 blur(8) 退化为纯暗罩（与端口其它模态一致）
        _modal = new Border
        {
            MaxWidth = 1120,
            MaxHeight = 860,
            Margin = new Thickness(28),
            BorderThickness = new Thickness(1),
            CornerRadius = new CornerRadius(24),
        };
        _host.Children.Add(_mask);
        _host.Children.Add(_modal);
        _mask.MouseLeftButtonDown += (_, _) => Close();
        return _host;
    }

    public void Open(string versionId)
    {
        _id = versionId;
        var row = _s.InstalledList.FirstOrDefault(v => v.Id == versionId);
        _mcVersion = row?.McVersion ?? versionId;

        var root = new StackPanel();
        // ---- header ----
        var head = new Grid { Margin = new Thickness(30, 26, 30, 4) };
        head.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        head.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        var headLeft = new StackPanel();
        headLeft.Children.Add(new TextBlock { Text = "实例管理中心", FontSize = 12, Foreground = (Brush)_shell.FindResource("Fg2") });
        _title = new TextBlock { Text = row?.Label ?? versionId, FontSize = 26, FontWeight = FontWeights.Bold, Margin = new Thickness(0, 6, 0, 0), TextTrimming = TextTrimming.CharacterEllipsis };
        headLeft.Children.Add(_title);
        _sub = new TextBlock
        {
            FontSize = 13,
            Foreground = (Brush)_shell.FindResource("Fg2"),
            Margin = new Thickness(0, 4, 0, 0),
        };
        headLeft.Children.Add(_sub);
        Grid.SetColumn(headLeft, 0);
        head.Children.Add(headLeft);
        var close = MakeButton("✕", "BtnGhost", null);
        close.Click += (_, _) => Close();
        Grid.SetColumn(close, 1);
        head.Children.Add(close);
        root.Children.Add(head);

        // ---- tabs ----
        _tabsPanel = new StackPanel { Orientation = Orientation.Horizontal, Margin = new Thickness(30, 10, 30, 0) };
        string[][] tabs = [["overview", "概览"], ["worlds", "存档"], ["screenshots", "截图"], ["backups", "备份"], ["diagnostics", "诊断"]];
        _tabButtons = tabs.Select(t =>
        {
            var b = new Button { Content = t[1], Cursor = System.Windows.Input.Cursors.Hand };
            b.Click += (_, _) => ChooseTab(t[0]);
            return b;
        }).ToArray();
        for (int i = 0; i < _tabButtons.Length; i++)
        {
            _tabButtons[i].Margin = new Thickness(0, 0, 6, 0);
            _tabsPanel.Children.Add(_tabButtons[i]);
        }
        root.Children.Add(_tabsPanel);

        var line = new Border { Height = 1, Background = (Brush)_shell.FindResource("Line"), Margin = new Thickness(0, 14, 0, 0) };
        root.Children.Add(line);

        _body = new ContentControl { Margin = new Thickness(30, 20, 30, 26) };
        root.Children.Add(_body);

        // 主题在 OnLoaded 才应用，壳的画刷必须等 Open 时再取（ctor 取到的是默认主题快照）
        _modal.Background = (Brush)_shell.FindResource("Card");
        _modal.BorderBrush = (Brush)_shell.FindResource("Line");
        var scroll = new ScrollViewer { Content = root, VerticalScrollBarVisibility = ScrollBarVisibility.Auto };
        _modal.Child = scroll;
        _host.Visibility = Visibility.Visible;
        ChooseTab("overview");
    }

    public void Close() => _host.Visibility = Visibility.Collapsed;

    void ChooseTab(string tab)
    {
        _tab = tab;
        string[] keys = ["overview", "worlds", "screenshots", "backups", "diagnostics"];
        for (int i = 0; i < keys.Length; i++)
            _tabButtons[i].Style = (Style)_shell.FindResource(keys[i] == tab ? "IcTabOn" : "IcTab");
        _body.Content = tab switch
        {
            "overview" => BuildOverview(),
            "worlds" => BuildWorlds(),
            "screenshots" => BuildShots(),
            "backups" => BuildBackups(),
            _ => BuildDiagnostics(),
        };
    }

    // ---------------- 概览 ----------------

    UIElement BuildOverview()
    {
        var sp = new StackPanel();

        var sum = new Grid { Margin = new Thickness(0, 0, 0, 22) };
        sum.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        sum.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        var sumLeft = new StackPanel();
        sumLeft.Children.Add(new TextBlock { Text = "你的游戏，独立管理", FontSize = 16, FontWeight = FontWeights.Bold });
        var path = new TextBlock { Text = Dir, FontSize = 12, Foreground = (Brush)_shell.FindResource("Fg2"), Margin = new Thickness(0, 10, 0, 0), TextTrimming = TextTrimming.CharacterEllipsis };
        sumLeft.Children.Add(path);
        Grid.SetColumn(sumLeft, 0);
        sum.Children.Add(sumLeft);
        var open = MakeButton("打开目录", "BtnGhost", null);
        open.Click += (_, _) => TryOpenDir(Dir);
        Grid.SetColumn(open, 1);
        sum.Children.Add(open);
        sp.Children.Add(sum);

        // .ic-grid：repeat(2,minmax(0,1fr)) gap 14（:109）——四枚：模组/资源包/光影/版本设置
        var grid = new Grid { Margin = new Thickness(0, 0, 0, 22) };
        for (int i = 0; i < 3; i++)
        {
            grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(14) });
        }
        grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        (string Title, string Sub, string Nav)[] cards =
        [
            ("模组管理 →", "版本、启停与兼容性", "mods"),
            ("资源包 →", "管理材质与界面资源", "packs"),
            ("光影包 →", "管理游戏光影", "shaders"),
            ("版本设置 →", "Java、窗口与隔离设置", "versions"),
        ];
        for (int i = 0; i < cards.Length; i++)
        {
            var card = MakeNavCard(cards[i].Title, cards[i].Sub, cards[i].Nav);
            Grid.SetRow(card, i / 2);
            Grid.SetColumn(card, (i % 2) * 2);
            if (i / 2 == 1) card.Margin = new Thickness(0, 14, 0, 0);
            grid.Children.Add(card);
        }
        sp.Children.Add(grid);

        // 复制实例卡（:61）：.ic-card 描边卡
        var cloneCard = new Border
        {
            Padding = new Thickness(20),
            CornerRadius = new CornerRadius(16),
            Margin = new Thickness(0, 0, 0, 14),
        };
        cloneCard.SetResourceReference(Border.BackgroundProperty, "Card");
        cloneCard.SetResourceReference(Border.BorderBrushProperty, "Line");
        var cardSp = new StackPanel();
        cloneCard.Child = cardSp;
        cardSp.Children.Add(new TextBlock { Text = "复制一个独立实例", FontSize = 15, FontWeight = FontWeights.Bold });
        var cloneDesc = new TextBlock
        {
            Text = "保留模组、配置与锁定状态，原实例保持完整。可在副本里尝试新的模组组合。",
            FontSize = 13,
            TextWrapping = TextWrapping.Wrap, Margin = new Thickness(0, 8, 0, 0),
        };
        cloneDesc.SetResourceReference(TextBlock.ForegroundProperty, "Fg2");
        cardSp.Children.Add(cloneDesc);
        var cloneBtn = MakeButton("复制实例", "BtnGold", null);
        cloneBtn.Margin = new Thickness(0, 12, 0, 0);
        cloneBtn.HorizontalAlignment = HorizontalAlignment.Left;   // 原版按钮内联，不整宽
        cloneBtn.MinWidth = 120;
        cloneBtn.Click += (_, _) => CloneInstance();
        cardSp.Children.Add(cloneBtn);
        sp.Children.Add(cloneCard);
        return sp;
    }

    void CloneInstance()
    {
        string? newName = _shell.PromptText("复制实例", "新实例名称", _id + "-副本");
        if (string.IsNullOrWhiteSpace(newName)) return;
        try
        {
            string from = _s.Paths.VersionDir(_id);
            string to = _s.Paths.VersionDir(newName);
            if (Directory.Exists(to)) { _s.Status = "已存在同名实例目录"; return; }
            CopyDir(from, to);
            var json = _s.Paths.VersionJson(newName);
            if (File.Exists(json))
            {
                var text = File.ReadAllText(json).Replace("\"id\": \"" + _id + "\"", "\"id\": \"" + newName + "\"");
                File.WriteAllText(json, text);
            }
            _s.RefreshInstalled();
            _s.Status = $"已复制为 {newName}";
            _id = newName;
            _title.Text = newName;
            ChooseTab("overview");
        }
        catch (Exception ex) { _s.Status = "复制失败：" + ex.Message; }
    }

    // ---------------- 存档 ----------------

    UIElement BuildWorlds()
    {
        var sp = new StackPanel();
        var tools = new Grid { Margin = new Thickness(0, 0, 0, 16) };
        tools.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        tools.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        sp.Children.Add(tools);
        var import = MakeButton("导入存档", "BtnGold", null);
        Grid.SetColumn(import, 1);
        import.Click += (_, _) => ImportWorld();
        tools.Children.Add(import);

        var savesDir = Path.Combine(Dir, "saves");
        var rows = new StackPanel();
        sp.Children.Add(rows);
        bool any = false;
        if (Directory.Exists(savesDir))
        {
            foreach (var wDir in Directory.EnumerateDirectories(savesDir))
            {
                any = true;
                rows.Children.Add(MakeWorldRow(wDir));
            }
        }
        if (!any)
        {
            rows.Children.Add(new TextBlock
            {
                Text = "暂无存档", FontSize = 14, Foreground = (Brush)_shell.FindResource("Fg2"),
                HorizontalAlignment = HorizontalAlignment.Center, Margin = new Thickness(0, 40, 0, 12),
            });
        }
        return sp;
    }

    UIElement MakeWorldRow(string wDir)
    {
        string name = System.IO.Path.GetFileName(wDir);
        try
        {
            var nbt = NbtLite.ParseFile(System.IO.Path.Combine(wDir, "level.dat"));
            if (nbt?.TryGetValue("Data", out var d) == true && d is Dictionary<string, object> data &&
                data.TryGetValue("LevelName", out var ln))
                name = ln.ToString() ?? name;
        }
        catch { }

        var row = new Grid { Margin = new Thickness(0, 0, 0, 1) };
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(16) });
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });

        var icon = new Border
        {
            Width = 58, Height = 58, CornerRadius = new CornerRadius(10),
            Background = (Brush)_shell.FindResource("AccentSoft"),
            Child = new TextBlock { Text = "▧", FontSize = 28, HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center },
            ClipToBounds = true,
        };
        var iconPath = System.IO.Path.Combine(wDir, "icon.png");
        if (File.Exists(iconPath))
        {
            try
            {
                var bmp = new System.Windows.Media.Imaging.BitmapImage();
                bmp.BeginInit();
                bmp.CacheOption = System.Windows.Media.Imaging.BitmapCacheOption.OnLoad;
                bmp.UriSource = new Uri(iconPath);
                bmp.EndInit();
                bmp.Freeze();
                icon.Child = new Image { Source = bmp, Stretch = Stretch.UniformToFill };
            }
            catch { }
        }
        Grid.SetColumn(icon, 0);
        row.Children.Add(icon);

        var info = new StackPanel { VerticalAlignment = VerticalAlignment.Center, MinWidth = 0 };
        Grid.SetColumn(info, 2);
        info.Children.Add(new TextBlock { Text = name, FontSize = 14, FontWeight = FontWeights.SemiBold, TextTrimming = TextTrimming.CharacterEllipsis });
        info.Children.Add(new TextBlock
        {
            Text = $"{_mcVersion} · {System.IO.Path.GetFileName(wDir)}",
            FontSize = 12, Foreground = (Brush)_shell.FindResource("Fg2"), Margin = new Thickness(0, 8, 0, 0),
        });
        row.Children.Add(info);

        var actions = new StackPanel { Orientation = Orientation.Horizontal, VerticalAlignment = VerticalAlignment.Center };
        Grid.SetColumn(actions, 3);
        var open = MakeButton("打开", "BtnSmGhost", null);
        open.Click += (_, _) => TryOpenDir(wDir);
        actions.Children.Add(open);
        var export = MakeButton("导出 ZIP", "BtnSmGhost", null);
        export.Margin = new Thickness(10, 0, 0, 0);
        string captured = wDir;
        export.Click += (_, _) => ExportWorldZip(captured);
        actions.Children.Add(export);
        var backup = MakeButton("备份", "BtnSmGhost", null);
        backup.Margin = new Thickness(10, 0, 0, 0);
        backup.Click += (_, _) => { CreateBackup(captured, worldName: System.IO.Path.GetFileName(captured)); ChooseTab("backups"); };
        actions.Children.Add(backup);
        row.Children.Add(actions);

        return new Border
        {
            Padding = new Thickness(0, 14, 0, 14),
            BorderBrush = (Brush)_shell.FindResource("Line"),
            BorderThickness = new Thickness(0, 0, 0, 1),
            Child = row,
        };
    }

    void ExportWorldZip(string worldDir)
    {
        var dlg = new Microsoft.Win32.SaveFileDialog
        {
            Title = "导出存档",
            FileName = System.IO.Path.GetFileName(worldDir) + ".zip",
            Filter = "ZIP|*.zip",
        };
        if (dlg.ShowDialog() != true) return;
        try
        {
            if (File.Exists(dlg.FileName)) File.Delete(dlg.FileName);
            ZipFile.CreateFromDirectory(worldDir, dlg.FileName);
            _s.Status = "存档已导出";
        }
        catch (Exception ex) { _s.Status = "导出失败：" + ex.Message; }
    }

    void ImportWorld()
    {
        var dlg = new Microsoft.Win32.OpenFileDialog { Title = "选择存档 ZIP", Filter = "ZIP|*.zip" };
        if (dlg.ShowDialog() != true) return;
        try
        {
            var saves = Path.Combine(Dir, "saves");
            Directory.CreateDirectory(saves);
            var name = System.IO.Path.GetFileNameWithoutExtension(dlg.FileName);
            var dest = Path.Combine(saves, name);
            if (Directory.Exists(dest)) dest = Path.Combine(saves, name + "-" + DateTime.Now.ToString("yyyyMMdd-HHmmss"));
            ZipFile.ExtractToDirectory(dlg.FileName, dest);
            _s.Status = "存档已导入";
            ChooseTab("worlds");
        }
        catch (Exception ex) { _s.Status = "导入失败：" + ex.Message; }
    }

    // ---------------- 截图 ----------------

    UIElement BuildShots()
    {
        var sp = new StackPanel();
        var shotsDir = Path.Combine(Dir, "screenshots");
        var files = Directory.Exists(shotsDir)
            ? Directory.EnumerateFiles(shotsDir, "*.png").OrderByDescending(File.GetLastWriteTime).Take(48).ToList()
            : [];
        if (files.Count == 0)
        {
            sp.Children.Add(new TextBlock
            {
                Text = "游戏中按截图键拍摄后，会显示在这里",
                FontSize = 14, Foreground = (Brush)_shell.FindResource("Fg2"),
                HorizontalAlignment = HorizontalAlignment.Center, Margin = new Thickness(0, 56, 0, 12),
            });
            return sp;
        }
        var wrap = new WrapPanel();
        foreach (var f in files)
        {
            var card = new StackPanel
            {
                Width = 220, Margin = new Thickness(0, 0, 16, 16),
                Cursor = System.Windows.Input.Cursors.Hand,
            };
            var imgBorder = new Border
            {
                Height = 124, CornerRadius = new CornerRadius(10),
                Background = (Brush)_shell.FindResource("CardSoft"),
                BorderBrush = (Brush)_shell.FindResource("Line"),
                BorderThickness = new Thickness(1), ClipToBounds = true,
            };
            try
            {
                var bmp = new System.Windows.Media.Imaging.BitmapImage();
                bmp.BeginInit();
                bmp.CacheOption = System.Windows.Media.Imaging.BitmapCacheOption.OnLoad;
                bmp.DecodePixelWidth = 440;
                bmp.UriSource = new Uri(f);
                bmp.EndInit();
                bmp.Freeze();
                imgBorder.Child = new Image { Source = bmp, Stretch = Stretch.UniformToFill };
            }
            catch { }
            card.Children.Add(imgBorder);
            card.Children.Add(new TextBlock
            {
                Text = System.IO.Path.GetFileName(f), FontSize = 12,
                Foreground = (Brush)_shell.FindResource("Fg2"),
                Margin = new Thickness(2, 6, 2, 0), TextTrimming = TextTrimming.CharacterEllipsis,
            });
            string captured = f;
            card.MouseLeftButtonUp += (_, _) =>
            {
                var dlg = new Microsoft.Win32.SaveFileDialog { FileName = System.IO.Path.GetFileName(captured), Filter = "PNG|*.png" };
                if (dlg.ShowDialog() == true)
                {
                    try { File.Copy(captured, dlg.FileName, true); _s.Status = "截图已另存"; }
                    catch (Exception ex) { _s.Status = "另存失败：" + ex.Message; }
                }
            };
            wrap.Children.Add(card);
        }
        sp.Children.Add(wrap);
        return sp;
    }

    // ---------------- 备份 ----------------

    UIElement BuildBackups()
    {
        var sp = new StackPanel();
        var sum = new Grid { Margin = new Thickness(0, 0, 0, 18) };
        sum.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        sum.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        var left = new StackPanel();
        left.Children.Add(new TextBlock { Text = "备份与改动保护", FontSize = 16, FontWeight = FontWeights.Bold });
        left.Children.Add(new TextBlock
        {
            Text = "手动备份长期保留，恢复为新目录、不覆盖现有实例。",
            FontSize = 13, Foreground = (Brush)_shell.FindResource("Fg2"),
            TextWrapping = TextWrapping.Wrap, Margin = new Thickness(0, 6, 0, 0),
        });
        Grid.SetColumn(left, 0);
        sum.Children.Add(left);
        var btn = MakeButton("备份实例", "BtnGold", null);
        btn.Click += (_, _) => { CreateBackup(Dir, worldName: null); ChooseTab("backups"); };
        Grid.SetColumn(btn, 1);
        sum.Children.Add(btn);
        sp.Children.Add(sum);

        var rows = new StackPanel();
        sp.Children.Add(rows);
        Directory.CreateDirectory(BackupsDir);
        var list = Directory.EnumerateFiles(BackupsDir, _id + "-*.zip").OrderByDescending(File.GetLastWriteTime).ToList();
        foreach (var zip in list)
        {
            var row = new Grid { Margin = new Thickness(0, 0, 0, 1) };
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            var info = new StackPanel { VerticalAlignment = VerticalAlignment.Center };
            info.Children.Add(new TextBlock { Text = System.IO.Path.GetFileNameWithoutExtension(zip), FontSize = 14, FontWeight = FontWeights.SemiBold });
            var sizeMb = new FileInfo(zip).Length / 1024.0 / 1024.0;
            info.Children.Add(new TextBlock
            {
                Text = $"{File.GetLastWriteTime(zip):yyyy/M/d HH:mm} · 手动备份 · {sizeMb:0.0} MB",
                FontSize = 12, Foreground = (Brush)_shell.FindResource("Fg2"), Margin = new Thickness(0, 8, 0, 0),
            });
            Grid.SetColumn(info, 0);
            row.Children.Add(info);
            var restore = MakeButton("恢复…", "BtnSmGhost", null);
            Grid.SetColumn(restore, 1);
            string captured = zip;
            restore.Click += (_, _) => RestoreBackup(captured);
            row.Children.Add(restore);
            rows.Children.Add(new Border
            {
                Child = row,
                Padding = new Thickness(0, 14, 0, 14),
                BorderBrush = (Brush)_shell.FindResource("Line"),
                BorderThickness = new Thickness(0, 0, 0, 1),
            });
        }
        if (list.Count == 0)
        {
            rows.Children.Add(new TextBlock
            {
                Text = "还没有备份，创建第一份备份以保护游戏进度",
                FontSize = 14, Foreground = (Brush)_shell.FindResource("Fg2"),
                HorizontalAlignment = HorizontalAlignment.Center, Margin = new Thickness(0, 40, 0, 12),
            });
        }
        return sp;
    }

    void CreateBackup(string srcDir, string? worldName)
    {
        try
        {
            Directory.CreateDirectory(BackupsDir);
            string baseName = worldName is null ? _id : $"{_id}-{worldName}";
            string dest = Path.Combine(BackupsDir, $"{baseName}-{DateTime.Now:yyyyMMdd-HHmmss}.zip");
            if (File.Exists(dest)) File.Delete(dest);
            ZipFile.CreateFromDirectory(srcDir, dest, CompressionLevel.Optimal, includeBaseDirectory: false);
            _s.Status = "备份完成";
            _s.Log($"备份完成：{dest}");
        }
        catch (Exception ex) { _s.Status = "备份失败：" + ex.Message; }
    }

    void RestoreBackup(string zip)
    {
        if (MessageBox.Show($"把备份恢复为新实例？\n{System.IO.Path.GetFileName(zip)}", "WUHUICL",
                MessageBoxButton.OKCancel, MessageBoxImage.Question) != MessageBoxResult.OK) return;
        try
        {
            string newName = _id + "-恢复-" + DateTime.Now.ToString("MMdd-HHmmss");
            string dest = _s.Paths.VersionDir(newName);
            Directory.CreateDirectory(dest);
            ZipFile.ExtractToDirectory(zip, dest, overwriteFiles: true);
            var json = _s.Paths.VersionJson(newName);
            if (File.Exists(json))
            {
                var text = File.ReadAllText(json).Replace("\"id\": \"" + _id + "\"", "\"id\": \"" + newName + "\"");
                File.WriteAllText(json, text);
            }
            _s.RefreshInstalled();
            _s.Status = $"已恢复为新实例 {newName}";
            ChooseTab("backups");
        }
        catch (Exception ex) { _s.Status = "恢复失败：" + ex.Message; }
    }

    // ---------------- 诊断 ----------------

    UIElement BuildDiagnostics()
    {
        var sp = new StackPanel();

        var sum = new Grid { Margin = new Thickness(0, 0, 0, 18) };
        sum.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        sum.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        var left = new StackPanel();
        left.Children.Add(new TextBlock { Text = "检查运行环境", FontSize = 16, FontWeight = FontWeights.Bold });
        left.Children.Add(new TextBlock
        {
            Text = "扫描本机 Java 运行时，检查适配度。",
            FontSize = 13, Foreground = (Brush)_shell.FindResource("Fg2"),
            Margin = new Thickness(0, 6, 0, 0),
        });
        Grid.SetColumn(left, 0);
        sum.Children.Add(left);
        var scan = MakeButton("开始检查", "BtnGold", null);
        Grid.SetColumn(scan, 1);
        scan.Click += (_, _) => ChooseTab("diagnostics");
        sum.Children.Add(scan);
        Grid.SetColumn(scan, 1);
        sp.Children.Add(sum);

        // Java 列表（真实扫描）
        var javas = JavaLocator.Scan(_s.Paths).OrderByDescending(j => j.Major).ToList();
        int need = JavaLocator.RequiredMajor(_mcVersion.Length > 0 ? _mcVersion : _id);
        var card = MakeCard(out var cardSp);
        cardSp.Children.Add(new TextBlock { Text = "本机 Java", FontSize = 15, FontWeight = FontWeights.Bold });
        if (javas.Count == 0)
        {
            cardSp.Children.Add(new TextBlock
            {
                Text = "未在本机发现 Java 运行时。", FontSize = 13,
                Foreground = (Brush)_shell.FindResource("Fg2"), Margin = new Thickness(0, 8, 0, 0),
            });
        }
        foreach (var j in javas)
        {
            var row = new StackPanel { Orientation = Orientation.Horizontal, Margin = new Thickness(0, 10, 0, 0) };
            bool fit = j.Major >= need;
            row.Children.Add(new TextBlock
            {
                Text = $"Java {j.Major}{(j.Is64 ? "" : "（32 位）")}",
                FontSize = 13, FontWeight = FontWeights.SemiBold,
                Foreground = fit ? (Brush)_shell.FindResource("Ok") : (Brush)_shell.FindResource("Fg2"),
                Width = 90, VerticalAlignment = VerticalAlignment.Center,
            });
            row.Children.Add(new TextBlock
            {
                Text = j.Exe, FontSize = 12,
                Foreground = (Brush)_shell.FindResource("Fg2"),
                VerticalAlignment = VerticalAlignment.Center, TextTrimming = TextTrimming.CharacterEllipsis,
            });
            cardSp.Children.Add(row);
        }
        if (javas.Count > 0)
        {
            cardSp.Children.Add(new TextBlock
            {
                Text = $"此实例（MC {_mcVersion}）需要 Java {need}+；匹配项已用绿色标出。全局 Java 在 设置 → 运行环境 选择。",
                FontSize = 12, Foreground = (Brush)_shell.FindResource("Fg2"),
                TextWrapping = TextWrapping.Wrap, Margin = new Thickness(0, 12, 0, 0),
            });
        }
        sp.Children.Add(card);

        var logs = MakeButton("导出诊断日志（启动器日志打包）", "BtnGhost", null);
        logs.Margin = new Thickness(0, 14, 0, 0);
        logs.Click += (_, _) => ExportDiagLogs();
        sp.Children.Add(logs);
        return sp;
    }

    void ExportDiagLogs()
    {
        var dlg = new Microsoft.Win32.SaveFileDialog
        {
            Title = "导出诊断日志",
            FileName = $"WUHUICL-diag-{DateTime.Now:yyyyMMdd-HHmmss}.zip",
            Filter = "ZIP|*.zip",
        };
        if (dlg.ShowDialog() != true) return;
        try
        {
            if (File.Exists(dlg.FileName)) File.Delete(dlg.FileName);
            using var archive = ZipFile.Open(dlg.FileName, ZipArchiveMode.Create);
            var logDir = _s.Paths.LauncherLogs;
            if (Directory.Exists(logDir))
                foreach (var f in Directory.EnumerateFiles(logDir).Take(10))
                    archive.CreateEntryFromFile(f, System.IO.Path.GetFileName(f));
            _s.Status = "诊断日志已导出";
        }
        catch (Exception ex) { _s.Status = "导出失败：" + ex.Message; }
    }

    // ---------------- 小件 ----------------

    FrameworkElement MakeNavCard(string title, string sub, string nav)
    {
        var sp = new StackPanel();
        sp.Children.Add(new TextBlock { Text = title, FontSize = 14, FontWeight = FontWeights.SemiBold });
        sp.Children.Add(new TextBlock { Text = sub, FontSize = 12, Foreground = (Brush)_shell.FindResource("Fg2"), Margin = new Thickness(0, 6, 0, 0) });
        var b = new Button
        {
            Content = sp,
            Cursor = System.Windows.Input.Cursors.Hand,
            HorizontalContentAlignment = HorizontalAlignment.Stretch,
        };
        b.SetResourceReference(FrameworkElement.StyleProperty, "IcCard");
        b.Click += (_, _) =>
        {
            Close();
            _shell.Show(nav);
        };
        return b;
    }

    static Border MakeCard(out StackPanel sp)
    {
        sp = new StackPanel();
        var card = new Border
        {
            Child = sp,
            Padding = new Thickness(20),
            CornerRadius = new CornerRadius(14),
            Margin = new Thickness(0, 0, 0, 14),
        };
        card.SetResourceReference(Border.BackgroundProperty, "CardSoft");
        return card;
    }

    Button MakeButton(string content, string styleKey, Thickness? margin)
    {
        var b = new Button { Content = content };
        b.SetResourceReference(FrameworkElement.StyleProperty, styleKey);
        if (margin is not null) b.Margin = margin.Value;
        return b;
    }

    void TryOpenDir(string dir)
    {
        try { Directory.CreateDirectory(dir); System.Diagnostics.Process.Start("explorer.exe", dir); }
        catch (Exception ex) { _s.Status = "打开目录失败：" + ex.Message; }
    }

    static void CopyDir(string from, string to)
    {
        Directory.CreateDirectory(to);
        foreach (var f in Directory.EnumerateFiles(from, "*", SearchOption.AllDirectories))
        {
            var rel = f[(from.Length + 1)..];
            var dest = Path.Combine(to, rel);
            Directory.CreateDirectory(System.IO.Path.GetDirectoryName(dest)!);
            File.Copy(f, dest, true);
        }
    }
}
