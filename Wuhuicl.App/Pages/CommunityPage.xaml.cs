using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading.Tasks;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using System.IO;
using System.Windows.Media.Imaging;
using Wuhuicl.App.Services;
using Wuhuicl.Core.Stores;
using Wuhuicl.Core.Storage;

namespace Wuhuicl.App.Pages;

/// <summary>
/// 社区资源页：Modrinth + CurseForge 双源搜索，下载安装到实例对应目录。
/// </summary>
public partial class CommunityPage : UserControl
{
    Button[] _capsules = null!;
    Button _active = null!;
    string _kind = "mod";
    List<CommunityResult> _results = new();
    int _offset;
    int _rendered;          // ResultList 里已渲染的条数（加载更多按此追加）
    const int PageSize = 20; // 与 CommunityService 单源 limit=20 对齐；一批 ≥ PageSize 时认为还有下一页
    bool _hasMore;
    bool _loading;
    string _gameRoot = "";
    string? _versionId;
    bool _favsTab;          // communityTab：false=找资源 / true=已收藏 MOD（CommunityView.vue:55）

    // ---------------- 模组收藏（modFavorites.ts 的本地等价：mods-favorites.json） ----------------
    public sealed record ModFav(string Key, string Source, string ProjectId, string Title, string IconUrl, long Time);

    static List<ModFav> LoadFavs()
    {
        try
        {
            return System.Text.Json.JsonSerializer.Deserialize<List<ModFav>>(
                File.ReadAllText(Path.Combine(GamePaths.DetectDefault().DataDir, "mods-favorites.json"))) ?? [];
        }
        catch { return []; }
    }

    static void SaveFavs(List<ModFav> list) =>
        Wuhuicl.Core.Storage.AtomicJson.Save(Path.Combine(GamePaths.DetectDefault().DataDir, "mods-favorites.json"), list);

    List<ModFav> _favs = [];

    public CommunityPage()
    {
        InitializeComponent();

        _capsules = [CapMod, CapModpack, CapResourcePack, CapShader, CapDataPack];
        _active = CapMod;
        Loaded += (_, _) => MoveBlob();
        ErrorRetryBtn.Click += (_, _) => { _offset = 0; _ = SearchAsync(); };
        StatusRetryBtn.Click += (_, _) => _ = SearchAsync();
        KindCapsules.SizeChanged += (_, _) => MoveBlob();
        ResultList.SizeChanged += (_, _) => Reflow(ResultList, _rendered);
        FavResultList.SizeChanged += (_, _) => Reflow(FavResultList, int.MaxValue);
        IsVisibleChanged += (_, e) =>
        {
            if (e.NewValue is true)
            {
                RefreshContext();
                if (_results.Count == 0 && !_loading) _ = SearchAsync();
            }
        };
    }

    void RefreshContext()
    {
        _gameRoot = Wuhuicl.Core.Storage.GamePaths.DetectDefault().Root;
        _versionId = (Application.Current.MainWindow as MainWindow)?.PageHome?.DataContext is not null
            ? null : null; // 简化：无选中实例时用共享目录
    }

    string KindFromButton(Button b) => b switch
    {
        _ when ReferenceEquals(b, CapMod) => "mod",
        _ when ReferenceEquals(b, CapModpack) => "modpack",
        _ when ReferenceEquals(b, CapResourcePack) => "resourcepack",
        _ when ReferenceEquals(b, CapShader) => "shader",
        _ when ReferenceEquals(b, CapDataPack) => "datapack",
        _ => "mod",
    };

    // ---------------- 分类胶囊 ----------------

    void Capsule_Click(object sender, RoutedEventArgs e)
    {
        if (sender is not Button chosen || ReferenceEquals(chosen, _active)) return;
        foreach (Button b in _capsules)
            b.Style = (Style)b.FindResource(ReferenceEquals(b, chosen) ? "CapsuleActive" : "Capsule");
        _active = chosen;
        _kind = KindFromButton(chosen);
        MoveBlob();
        _offset = 0;
        _ = SearchAsync();
    }

    void MoveBlob()
    {
        if (_active is null || CapsuleBlob is null || CapsulesHost is null) return;
        if (_active.ActualWidth <= 0 || _active.ActualHeight <= 0) return;
        GeneralTransform toHost = _active.TransformToVisual(CapsulesHost);
        Point origin = toHost.Transform(new Point(0, 0));
        if (CapsuleBlob.Opacity == 0 || CapsuleBlob.Width <= 0)
        {
            Canvas.SetLeft(CapsuleBlob, origin.X);
            Canvas.SetTop(CapsuleBlob, origin.Y);
            CapsuleBlob.Width = _active.ActualWidth;
            CapsuleBlob.Height = _active.ActualHeight;
            CapsuleBlob.Opacity = 1;
            return;
        }
        Services.Motion.Run(CapsuleBlob,
            (Canvas.LeftProperty, Services.Motion.ToCurrent(Canvas.LeftProperty, origin.X, Services.Motion.Normal, Services.Motion.EaseOut, Canvas.GetLeft(CapsuleBlob))),
            (Canvas.TopProperty, Services.Motion.ToCurrent(Canvas.TopProperty, origin.Y, Services.Motion.Normal, Services.Motion.EaseOut, Canvas.GetTop(CapsuleBlob))),
            (FrameworkElement.WidthProperty, Services.Motion.ToCurrent(FrameworkElement.WidthProperty, _active.ActualWidth, Services.Motion.Normal, Services.Motion.EaseOut, CapsuleBlob.Width)),
            (FrameworkElement.HeightProperty, Services.Motion.ToCurrent(FrameworkElement.HeightProperty, _active.ActualHeight, Services.Motion.Ms(320), Services.Motion.Ease, CapsuleBlob.Height)));
        CapsuleBlob.Opacity = 1;
    }

    // ---------------- 搜索 ----------------

    void Keyword_TextChanged(object sender, TextChangedEventArgs e) =>
        PhKeyword.Visibility = TxtKeyword.Text.Length == 0 ? Visibility.Visible : Visibility.Collapsed;

    void Keyword_KeyDown(object sender, KeyEventArgs e)
    {
        if (e.Key == Key.Enter) Search_Click(sender, e);
    }

    void Search_Click(object sender, RoutedEventArgs e)
    {
        _offset = 0;
        _ = SearchAsync();
    }

    /// <summary>加载更多：offset 前进一页，结果追加渲染（失败则回滚 offset）。</summary>
    void LoadMore_Click(object sender, RoutedEventArgs e)
    {
        if (_loading) return;
        _offset += PageSize;
        _ = SearchAsync(append: true);
    }

    async Task SearchAsync(bool append = false)
    {
    try
    {
        _kindStatic = _kind;
        if (_loading) return;
        _loading = true;
        if (append) SetLoadMoreBusy(true); else ShowLoading(true);
        try
        {
            var options = new CommunityService.SearchOptions(
                _kind, TxtKeyword.Text.Trim(), _offset, TxtVersion.Text.Trim(), null);
            var batch = await CommunityService.SearchBothAsync(options);
            if (append) _results.AddRange(batch); else _results = batch;
            _hasMore = batch.Count >= PageSize;
            HideStatusStrip();
            RenderResults(append);
        }
        catch (Exception ex)
        {
            if (append) _offset = Math.Max(0, _offset - PageSize);
            // 已有结果时失败不打断浏览：顶条提示 + 保留上次结果（对齐 v-if="results.length && loadError" 的 status-strip）
            if (_results.Count > 0) ShowStatusStrip(append ? "加载更多失败：" + ex.Message : "更新失败，以下为上次结果：" + ex.Message);
            else ShowError(ex.Message);
        }
        finally
        {
            _loading = false;
            if (append) SetLoadMoreBusy(false); else ShowLoading(false);
        }
    }
    catch (Exception ex) { MainWindow.Diag("community-SearchAsync EX: " + ex); }
    }


    void ShowLoading(bool on)
    {
        ContentSkeleton.Visibility = on && _results.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
        if (on)
        {
            // 加载期间与空态/加载更多互斥（原版 v-if/v-else 链 + :inert）
            EmptyState.Visibility = Visibility.Collapsed;
            MoreRow.Visibility = Visibility.Collapsed;
        }
    }

    void ShowError(string msg)
    {
        ErrorEmpty.Visibility = Visibility.Visible;
        ErrorEmptyText.Text = "搜索失败：" + msg;
    }

    void ShowStatusStrip(string msg)
    {
        StatusStripText.Text = msg;
        StatusStrip.Visibility = Visibility.Visible;
    }

    void HideStatusStrip() => StatusStrip.Visibility = Visibility.Collapsed;

    void RenderResults(bool append = false)
    {
        ErrorEmpty.Visibility = Visibility.Collapsed;
        if (_results.Count == 0)
        {
            ResultList.Children.Clear();
            _rendered = 0;
            EmptyState.Visibility = Visibility.Visible;
            MoreRow.Visibility = Visibility.Collapsed;
            return;
        }
        EmptyState.Visibility = Visibility.Collapsed;
        if (!append)
        {
            ResultList.Children.Clear();
            _rendered = 0;
        }
        for (int i = _rendered; i < _results.Count; i++)
            ResultList.Children.Add(MakeResultCard(_results[i]));
        _rendered = _results.Count;
        Reflow(ResultList, _rendered);
        MoreRow.Visibility = _hasMore ? Visibility.Visible : Visibility.Collapsed;
    }

    /// <summary>.result-list：repeat(auto-fit,minmax(min(100%,300px),1fr)) gap 16 — CommunityView.vue:1115。
    /// 列数 = floor((宽+16)/316) 夹 1..3（内容列最宽 940，原版同宽下就是 3 列），等宽铺满。</summary>
    void Reflow(Grid grid, int count)
    {
        double w = grid.ActualWidth;
        if (w < 50) return;
        int cols = Math.Clamp((int)((w + 16) / 316), 1, 3);
        if (count == 0) return;
        grid.ColumnDefinitions.Clear();
        grid.RowDefinitions.Clear();
        double colW = (w - 16 * (cols - 1)) / cols;
        int rows = (count + cols - 1) / cols;
        for (int c = 0; c < cols; c++) grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(colW) });
        for (int r = 0; r < rows; r++) grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        for (int i = 0; i < Math.Min(count, grid.Children.Count); i++)
        {
            if (grid.Children[i] is FrameworkElement el)
            {
                Grid.SetRow(el, i / cols);
                Grid.SetColumn(el, i % cols);
                el.Margin = new Thickness(0, 0, i % cols == cols - 1 ? 0 : 16, 16);
                el.Width = colW;
            }
        }
    }

    void SetLoadMoreBusy(bool busy)
    {
        LoadMoreSpin.Visibility = busy ? Visibility.Visible : Visibility.Collapsed;
        LoadMoreText.Text = busy ? "加载中…" : "加载更多";
        LoadMoreBtn.IsEnabled = !busy;
    }

    Border MakeResultCard(CommunityResult r)
    {
        var card = new Border
        {
            Style = (Style)FindResource("CardTight"),
            Padding = new Thickness(16),
            Cursor = System.Windows.Input.Cursors.Hand,
            VerticalAlignment = VerticalAlignment.Top,
        };
        var sp = new StackPanel();

        // .result-top：图标 46×46 + head（title / tag / author）
        var top = new Grid();
        top.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        top.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(6) });
        top.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });

        var iconBorder = new Border
        {
            Width = 46, Height = 46, CornerRadius = new CornerRadius(10),
            Background = (Brush)FindResource("Card"),
            BorderBrush = (Brush)FindResource("Line"), BorderThickness = new Thickness(1),
            ClipToBounds = true, VerticalAlignment = VerticalAlignment.Top,
        };
        var iconHost = new Grid();
        var icon = new Image { Stretch = Stretch.UniformToFill, Visibility = Visibility.Collapsed };
        iconHost.Children.Add(icon);
        iconHost.Children.Add(new TextBlock
        {
            Text = string.IsNullOrEmpty(r.Title) ? "?" : char.ToUpperInvariant(r.Title[0]).ToString(),
            FontSize = 16, FontWeight = FontWeights.Bold,
            Foreground = (Brush)FindResource("Fg2"),
            HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center,
        });
        iconBorder.Child = iconHost;
        Grid.SetColumn(iconBorder, 0);
        top.Children.Add(iconBorder);
        LoadIcon(icon, r.IconUrl);

        var head = new StackPanel { MinWidth = 0, VerticalAlignment = VerticalAlignment.Top };
        Grid.SetColumn(head, 2);
        top.Children.Add(head);
        var title = new TextBlock
        {
            Text = r.Title,
            FontSize = 16, FontWeight = FontWeights.SemiBold, LineHeight = 23.2,
            LineStackingStrategy = LineStackingStrategy.BlockLineHeight,
            TextWrapping = TextWrapping.Wrap, MaxHeight = 46.4,
            TextTrimming = TextTrimming.CharacterEllipsis,
        };
        title.SetResourceReference(TextBlock.ForegroundProperty, "Fg");
        head.Children.Add(title);
        bool isModrinth = r.Source == "modrinth";
        var tag = new Border
        {
            Style = (Style)FindResource("Tag"),
            Background = isModrinth ? (Brush)FindResource("OkSoft")
                : new SolidColorBrush(Color.FromArgb(0x1F, 0xF9, 0x73, 0x16)),
            BorderThickness = new Thickness(0),
            HorizontalAlignment = HorizontalAlignment.Left, Margin = new Thickness(0, 6, 0, 0),
        };
        tag.Child = new TextBlock
        {
            Text = isModrinth ? "来源：Modrinth" : "来源：CurseForge",
            Style = (Style)FindResource("TagTextSm"),
            Foreground = isModrinth ? (Brush)FindResource("Ok")
                : new SolidColorBrush(Color.FromRgb(0xF9, 0x73, 0x16)),
        };
        head.Children.Add(tag);
        if (!string.IsNullOrEmpty(r.Author))
            head.Children.Add(new TextBlock
            {
                Text = r.Author, Style = (Style)FindResource("Micro"),
                Margin = new Thickness(0, 6, 0, 0), TextTrimming = TextTrimming.CharacterEllipsis,
            });
        sp.Children.Add(top);

        // .result-desc：两行截断，min-height 3.2em
        var desc = new TextBlock
        {
            Text = string.IsNullOrEmpty(r.Description) ? "暂无简介" : r.Description,
            Style = (Style)FindResource("Micro"), FontSize = 12,
            LineHeight = 19.2, MaxHeight = 38.4, MinHeight = 38.4, TextWrapping = TextWrapping.Wrap,
            TextTrimming = TextTrimming.CharacterEllipsis, Margin = new Thickness(0, 10, 0, 0),
        };
        desc.SetResourceReference(TextBlock.ForegroundProperty, "Fg2");
        sp.Children.Add(desc);

        // .result-meta：下载量 · 更新于
        var meta = new StackPanel { Orientation = Orientation.Horizontal, Margin = new Thickness(0, 8, 0, 0) };
        meta.Children.Add(new TextBlock { Style = (Style)FindResource("Micro"), FontSize = 12, Text = $"下载量 {FmtDownloads(r.Downloads)}" });
        meta.Children.Add(new TextBlock { Style = (Style)FindResource("Micro"), FontSize = 12, Text = "·", Margin = new Thickness(8, 0, 8, 0), Opacity = 0.6 });
        meta.Children.Add(new TextBlock { Style = (Style)FindResource("Micro"), FontSize = 12, Text = $"更新于 {FmtDate(r.UpdatedAt)}" });
        sp.Children.Add(meta);

        // .result-foot：左 links（打开源页 / MC 百科 / 收藏星）＋右下载
        var foot = new Grid { Margin = new Thickness(0, 12, 0, 0) };
        foot.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        foot.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        var links = new StackPanel { Orientation = Orientation.Horizontal, VerticalAlignment = VerticalAlignment.Center };
        Grid.SetColumn(links, 0);
        foot.Children.Add(links);

        var open = MakeIconBtn(14, "Icon.ExternalLink", $"打开 {(isModrinth ? "Modrinth" : "CurseForge")} 源页面（查看完整介绍）");
        open.Click += (_, _) => OpenExternal(SourceUrl(r));
        links.Children.Add(open);
        var mcmodBtn = MakeIconBtn(14, "Icon.Book", "在 MC 百科查看介绍与教程");
        var mcmodUrl = McmodUrl(r);
        mcmodBtn.Click += (_, _) => { if (mcmodUrl is not null) OpenExternal(mcmodUrl); };
        mcmodBtn.IsEnabled = mcmodUrl is not null;
        links.Children.Add(mcmodBtn);
        if (_kind == "mod")
        {
            string key = $"{r.Source}:{r.ProjectId}";
            bool favOn = _favs.Any(f => f.Key == key);
            var starBtn = new Button
            {
                Style = (Style)FindResource("IconBtn"),
                Width = 34, Height = 34,
                ToolTip = favOn ? "取消收藏模组" : "收藏模组",
                Content = MakeIcon(16, favOn ? "Icon.Star" : "Icon.StarOutline"),
            };
            if (favOn) starBtn.SetResourceReference(Button.ForegroundProperty, "AccentHi");
            var captured = r;
            starBtn.Click += (_, _) => ToggleFav(captured, starBtn);
            links.Children.Add(starBtn);
        }

        var btn = new Button { Style = (Style)FindResource("CmResultDl"), VerticalAlignment = VerticalAlignment.Center };
        btn.Content = "下载";
        var capDl = r;
        btn.Click += (_, _) => _ = DownloadAsync(capDl);
        Grid.SetColumn(btn, 1);
        foot.Children.Add(btn);
        sp.Children.Add(foot);

        card.Child = sp;
        return card;
    }

    static UIElement MakeIcon(double size, string iconKey) => new ContentControl
    {
        Width = size, Height = size,
        Style = (System.Windows.Style)System.Windows.Application.Current.Resources[iconKey],
    };

    static Button MakeIconBtn(double size, string iconKey, string tip)
    {
        var b = new Button
        {
            Style = (System.Windows.Style)System.Windows.Application.Current.Resources["IconBtn"],
            Width = 34, Height = 34,
            ToolTip = tip,
            Content = MakeIcon(size, iconKey),
        };
        return b;
    }

    /// <summary>sourceUrl（CommunityView.vue:37-40）。</summary>
    static string SourceUrl(CommunityResult r) => r.Source == "modrinth"
        ? $"https://modrinth.com/project/{(string.IsNullOrEmpty(r.Slug) ? r.ProjectId : r.Slug)}"
        : $"https://www.curseforge.com/minecraft/{CfKindSegment()}/{(string.IsNullOrEmpty(r.Slug) ? r.ProjectId : r.Slug)}";

    /// <summary>CF_KIND_SEGMENT：kind → CurseForge URL 段（shared/communityLinks 或社区源码）。</summary>
    static string CfKindSegment() => _kindStatic switch
    {
        "resourcepack" => "texture-packs",
        "shader" => "shaders",
        "datapack" => "data-packs",
        _ => "mc-mods",
    };

    static string? _kindStatic;

    /// <summary>mcmodSearchUrl（shared/communityLinks.ts）：优先源站英文名 / slug，不向百科发中文。</summary>
    static string? McmodUrl(CommunityResult r)
    {
        var raw = (r.Title ?? "").Trim();
        var m = System.Text.RegularExpressions.Regex.Match(raw, "^[^|]*\\p{Script=Han}[^|]*\\|\\s*");
        if (m.Success) raw = raw[m.Length..];
        if (System.Text.RegularExpressions.Regex.IsMatch(raw, "[a-z]", System.Text.RegularExpressions.RegexOptions.IgnoreCase))
            return $"https://search.mcmod.cn/s?key={Uri.EscapeDataString(raw)}";
        if (!string.IsNullOrEmpty(r.Slug) && System.Text.RegularExpressions.Regex.IsMatch(r.Slug, "^[a-z0-9_-]+$", System.Text.RegularExpressions.RegexOptions.IgnoreCase))
            return $"https://search.mcmod.cn/s?key={Uri.EscapeDataString(r.Slug.Replace("-", " ").Replace("_", " "))}";
        return null;
    }

    static void OpenExternal(string url)
    {
        try { System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo(url) { UseShellExecute = true }); }
        catch { }
    }

    /// <summary>toggleProject（modFavorites.ts）：写入/移除 + 星标即时反馈 + 徽标计数。</summary>
    void ToggleFav(CommunityResult r, Button starBtn)
    {
        string key = $"{r.Source}:{r.ProjectId}";
        var fav = _favs.FirstOrDefault(f => f.Key == key);
        bool now;
        if (fav is not null) { _favs.Remove(fav); now = false; }
        else
        {
            _favs.Insert(0, new ModFav(key, r.Source, r.ProjectId, r.Title, r.IconUrl,
                DateTimeOffset.UtcNow.ToUnixTimeMilliseconds()));
            now = true;
        }
        SaveFavs(_favs);
        starBtn.Content = MakeIcon(16, now ? "Icon.Star" : "Icon.StarOutline");
        if (now) starBtn.SetResourceReference(Button.ForegroundProperty, "AccentHi");
        else starBtn.ClearValue(Button.ForegroundProperty);
        starBtn.ToolTip = now ? "取消收藏模组" : "收藏模组";
        FavBadge.Text = _favs.Count.ToString();
        if (_favsTab) RenderFavs();
    }

    /// <summary>收藏分区：空态 / 卡片网格（CommunityFavorites.vue:80-82）。</summary>
    void RenderFavs()
    {
        FavResultList.Children.Clear();
        FavEmpty.Visibility = _favs.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
        FavResultList.Visibility = _favs.Count > 0 ? Visibility.Visible : Visibility.Collapsed;
        if (_favs.Count == 0) return;
        foreach (var f in _favs)
        {
            var r = new CommunityResult(f.Source, f.ProjectId, "", f.Title, "", "", f.IconUrl, 0, "");
            FavResultList.Children.Add(MakeResultCard(r));
        }
        Reflow(FavResultList, FavResultList.Children.Count);
    }

    void TabBrowse_Click(object sender, RoutedEventArgs e) => SwitchTab(false);
    void TabFavs_Click(object sender, RoutedEventArgs e) => SwitchTab(true);

    /// <summary>communityTab 切换（CommunityView.vue:484）：页签样式互斥、两块内容显隐。</summary>
    void SwitchTab(bool favs)
    {
        _favsTab = favs;
        if (favs) _favs = LoadFavs();
        TabBrowse.Style = (Style)FindResource(favs ? "CmSectionTab" : "CmSectionTabOn");
        TabFavs.Style = (Style)FindResource(favs ? "CmSectionTabOn" : "CmSectionTab");
        BrowseBody.Visibility = favs ? Visibility.Collapsed : Visibility.Visible;
        FavEmpty.Visibility = favs && _favs.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
        FavResultList.Visibility = favs && _favs.Count > 0 ? Visibility.Visible : Visibility.Collapsed;
        if (favs) RenderFavs();
    }

    // fmtDownloads（:341-346）：≥1e8 → x.x 亿；≥1e4 → x.x 万；其余原数
    static string FmtDownloads(long n) => n switch
    {
        >= 100_000_000 => (n / 1e8).ToString("0.0") + " 亿",
        >= 10_000 => (n / 1e4).ToString("0.0") + " 万",
        _ => n.ToString(),
    };

    // fmtDate（:347-352）：ISO → zh-CN 本地日期；空值回退 —
    static string FmtDate(string iso)
    {
        if (string.IsNullOrEmpty(iso)) return "—";
        return DateTime.TryParse(iso, out var d) ? d.ToString("yyyy/M/d") : iso;
    }

    /// <summary>加载结果卡图标；成功前保持首字母占位。</summary>
    void LoadIcon(Image img, string url)
    {
        if (string.IsNullOrEmpty(url) || !Uri.TryCreate(url, UriKind.Absolute, out var uri)) return;
        try
        {
            var bmp = new BitmapImage();
            bmp.BeginInit();
            bmp.CacheOption = BitmapCacheOption.OnLoad;
            bmp.UriSource = uri;
            bmp.EndInit();
            bmp.DownloadCompleted += (_, _) => img.Visibility = Visibility.Visible;
            img.Source = bmp;
        }
        catch
        {
            // 图源失效时保留首字母占位
        }
    }

    async Task DownloadAsync(CommunityResult r)
    {
        try
        {
            var files = r.Source == "modrinth"
                ? await CommunityService.GetModrinthVersionsAsync(r.ProjectId, TxtVersion.Text.Trim(), null)
                : await CommunityService.GetCurseForgeFilesAsync(r.ProjectId);
            var file = files.FirstOrDefault(f => f.ReleaseType == "release") ?? files.FirstOrDefault();
            if (file is null || string.IsNullOrEmpty(file.Url))
            {
                MessageBox.Show("没有可下载的文件", "WUHUICL", MessageBoxButton.OK, MessageBoxImage.Warning);
                return;
            }
            var dir = CommunityService.GetInstallDir(_gameRoot, _versionId, _kind);
            var path = await CommunityService.DownloadFileAsync(file.Url, dir, file.FileName, file.Sha1);
            MessageBox.Show($"已下载到：\n{path}", "WUHUICL", MessageBoxButton.OK, MessageBoxImage.Information);
        }
        catch (Exception ex)
        {
            MessageBox.Show("下载失败：" + ex.Message, "WUHUICL", MessageBoxButton.OK, MessageBoxImage.Warning);
        }
    }

    void Reset_Click(object sender, RoutedEventArgs e)
    {
        TxtKeyword.Clear();
        TxtVersion.Clear();
        foreach (Button b in _capsules)
            b.Style = (Style)b.FindResource(ReferenceEquals(b, CapMod) ? "CapsuleActive" : "Capsule");
        _active = CapMod;
        _kind = "mod";
        MoveBlob();
    }

    void Version_TextChanged(object sender, TextChangedEventArgs e) =>
        PhVersion.Visibility = TxtVersion.Text.Length == 0 ? Visibility.Visible : Visibility.Collapsed;

    // ---------------- 占位保留 ----------------

    void InstanceSelect_Click(object sender, RoutedEventArgs e) { }
    void UseInstance_Click(object sender, RoutedEventArgs e) { }
    void FilterSelect_Click(object sender, RoutedEventArgs e) { }
    void SourcePage_Click(object sender, RoutedEventArgs e) { }
    void Mcmod_Click(object sender, RoutedEventArgs e) { }
    void Download_Click(object sender, RoutedEventArgs e) { }
    void Page_Click(object sender, RoutedEventArgs e) { }
}
