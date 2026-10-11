using System.IO;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using Microsoft.Win32;
using Wuhuicl.App.Services;

namespace Wuhuicl.App.Pages;

/// <summary>
/// 皮肤与披风页：KAMUCL-1.1.5/src/renderer/src/views/SkinsView.vue 的 WPF 版式复刻。
/// 顶层是 UI_GEOMETRY_PAGES.md §6 的最终 grid（SkinsView.vue:1081-1109 那个后写的 scoped 块），
/// 不是 :602-1080 的 flex 基线。
/// 功能已接真实后端 <see cref="SkinService"/>：
/// · 当前皮肤 = exe 旁 Assets\skin.png（SkinRenderer.LoadAsset 对磁盘文件优先于内嵌资源，
///   导入/换回把文件复制到这个固定路径再 SkinSource(reload:true)，3D 预览与首页头像全部跟上）；
/// · 导入（.hidden-input 的等价，:485）：OpenFileDialog 选 PNG → 校验 64×64 → 复制到
///   &lt;DataDir&gt;\skins\&lt;sha1&gt;.png → skins.json 历史（{id,variant,time,name}，上限 30，SHA1 去重）；
/// · 历史（:528-586）：HistoryGrid 由 <see cref="BuildHistoryItem"/> 逐条生成（预览图 + 悬浮换回/删除 +
///   经典/纤细 tag + 相对时间 + 文件名），换回 = 历史文件复制回当前路径；搜索框（:530）按文件名过滤。
/// </summary>
public partial class SkinsPage : UserControl
{
    Button? _animActive;

    // ---------------- 历史皮肤状态 ----------------
    List<SkinHistoryEntry> _history = [];
    string _historyFilter = "";
    TextBlock? _historyCountText;   // 「历史皮肤（N）」标题（XAML 占位无 x:Name，按文案特征定位）
    TextBlock? _historyEmptyText;   // 历史空态文案（同一 Grid 的星形列）

    public SkinsPage()
    {
        InitializeComponent();
        Loaded += (_, _) => { ShowSkin(); ReloadHistory(); PlaceAnimBlob(); };
        // 字体/主题变化会让 .seg-btn 宽度变，原版靠 ResizeObserver 重测（:63-87）
        AnimSegGrid.SizeChanged += (_, _) => PlaceAnimBlob();

        // 「选择皮肤文件…」（:477 的 input[type=file] 代理按钮）：本轮挂 OpenFileDialog
        BtnPickSkin.Click += PickSkin_Click;
        // 「上传」= 皮肤站/Mojang 上传（:478 的 @click），无后端可调，本轮保持占位
        TxtHistorySearch.TextChanged += (_, _) =>
        {
            _historyFilter = TxtHistorySearch.Text.Trim();
            RenderHistory();
        };
    }

    /// <summary>皮肤预览：SkinDollHost（快照 + 悬浮真 3D），走首页同源的皮肤源。
    /// Walk 已在 XAML 置 True（previewAnim 默认 walk，SkinsView.vue:61）。</summary>
    void ShowSkin(bool reload = false)
    {
        SkinDoll.Skin = SkinRenderer.SkinSource(reload);
        SkinDoll.Slim = SkinRenderer.IsSlimSkin();
        // .preview-3d-empty 是 3D 观察器的 v-else（SkinsView.vue:719-728）：人偶挂上后整块就不该存在，
        // 包括那圈 1px dashed 边框和 card-2 底。之前只藏了里面的占位文字，于是虚线框留在画面上。
        PreviewEmptyState.Visibility = System.Windows.Visibility.Collapsed;
    }

    // ================= 历史皮肤（:528-586） =================

    /// <summary>读 skins.json 并重建历史网格；坏文件由 SkinService 兜底（留 .bad- 副本、返回空表）。</summary>
    void ReloadHistory()
    {
        try
        {
            _history = SkinService.LoadHistory();
            StripHistoryError.Visibility = System.Windows.Visibility.Collapsed;
        }
        catch (Exception)
        {
            _history = [];
            StripHistoryError.Visibility = System.Windows.Visibility.Visible;
        }
        RenderHistory();
    }

    /// <summary>历史网格与计数、搜索框、空态的联动重建（:530 搜索框 v-if historyList.length、:545 网格 v-else）。</summary>
    void RenderHistory()
    {
        _historyCountText ??= FindHistoryCountText();
        if (_historyCountText is not null) _historyCountText.Text = $"历史皮肤（{_history.Count}）";

        var searchHost = TxtHistorySearch.Parent as Grid;
        if (searchHost is not null)
            searchHost.Visibility = _history.Count > 0 ? System.Windows.Visibility.Visible : System.Windows.Visibility.Collapsed;

        var filtered = _history;
        if (_historyFilter.Length > 0)
            filtered = _history.Where(h => h.Name.Contains(_historyFilter, StringComparison.OrdinalIgnoreCase)).ToList();

        HistoryGrid.Children.Clear();
        foreach (var h in filtered)
            HistoryGrid.Children.Add(BuildHistoryItem(h));
        HistoryGrid.Visibility = filtered.Count > 0 ? System.Windows.Visibility.Visible : System.Windows.Visibility.Collapsed;

        _historyEmptyText ??= FindHistoryEmptyText();
        if (_historyEmptyText is not null)
        {
            _historyEmptyText.Text = _history.Count == 0
                ? "暂无历史皮肤，上传皮肤后会自动保存到这里，方便随时换回"
                : "没有匹配的皮肤文件";
            _historyEmptyText.Visibility = filtered.Count == 0
                ? System.Windows.Visibility.Visible
                : System.Windows.Visibility.Collapsed;
        }
    }

    /// <summary>一条历史 = 预览图（悬浮换回/删除）+ 经典/纤细 tag + 相对时间 + 文件名（:978-1052 的几何）。</summary>
    StackPanel BuildHistoryItem(SkinHistoryEntry h)
    {
        // .history-img：height 144 + image-rendering:pixelated（:994-997）→ NearestNeighbor
        var img = new Image { Height = 144, Stretch = Stretch.Uniform, VerticalAlignment = VerticalAlignment.Center };
        RenderOptions.SetBitmapScalingMode(img, BitmapScalingMode.NearestNeighbor);
        try
        {
            var bmp = new BitmapImage();
            bmp.BeginInit();
            bmp.CacheOption = BitmapCacheOption.OnLoad;
            bmp.UriSource = new Uri(SkinService.SkinPathOf(h.Id));
            bmp.EndInit();
            bmp.Freeze();
            img.Source = bmp;
        }
        catch { /* 文件被外部删掉时留空预览，不炸整格 */ }

        // 悬浮操作层（:998-1011 原版是 opacity 0→hover 1 过渡；过渡属动效不做，
        // 但状态本身必须做——否则换回/删除永远点不到）：MouseEnter 瞬切透明度
        var restore = new Button
        {
            Style = (Style)FindResource("BtnGold"),
            Content = "换回",
            MinHeight = 32,
            Padding = new Thickness(12, 4, 12, 4),
            FontSize = 13,
            Margin = new Thickness(0, 0, 8, 0),
        };
        restore.Click += (_, _) => ApplyHistory(h);
        var delete = new Button
        {
            Style = (Style)FindResource("BtnDanger"),
            Content = "删除",
            MinHeight = 32,
            Padding = new Thickness(12, 4, 12, 4),
            FontSize = 13,
        };
        delete.Click += (_, _) => DeleteHistory(h);
        var overlayInner = new StackPanel
        {
            Orientation = Orientation.Horizontal,
            HorizontalAlignment = HorizontalAlignment.Center,
            VerticalAlignment = VerticalAlignment.Center,
        };
        overlayInner.Children.Add(restore);
        overlayInner.Children.Add(delete);
        var overlay = new Border
        {
            Background = (Brush)FindResource("Overlay"),
            Opacity = 0,
            IsHitTestVisible = false,
            Child = overlayInner,
        };

        var previewGrid = new Grid();
        previewGrid.Children.Add(img);
        previewGrid.Children.Add(overlay);
        var preview = new Border
        {
            Height = 168,                                  // .history-preview（:983-993）
            CornerRadius = new CornerRadius(10),
            ClipToBounds = true,
            Background = (Brush)FindResource("CardSoft"),
            BorderBrush = (Brush)FindResource("Line"),
            BorderThickness = new Thickness(1),
            Child = previewGrid,
        };
        preview.MouseEnter += (_, _) => { overlay.Opacity = 1; overlay.IsHitTestVisible = true; };
        preview.MouseLeave += (_, _) => { overlay.Opacity = 0; overlay.IsHitTestVisible = false; };

        // .history-meta（:1012-1018）：slim → tag-cyan（纤细），classic → tag-gold（经典）
        bool slim = h.Variant == "slim";
        var tagText = new TextBlock { Text = slim ? "纤细" : "经典", Style = (Style)FindResource("TagText") };
        tagText.SetResourceReference(TextBlock.ForegroundProperty, slim ? "Cyan" : "AccentHi");
        var tag = new Border { Style = (Style)FindResource("Tag"), HorizontalAlignment = HorizontalAlignment.Left, Child = tagText };
        tag.SetResourceReference(Border.BackgroundProperty, slim ? "CyanSoft" : "AccentSoft");

        var time = new TextBlock
        {
            Style = (Style)FindResource("Micro"),
            FontSize = 12,
            HorizontalAlignment = HorizontalAlignment.Right,
            VerticalAlignment = VerticalAlignment.Center,
            TextTrimming = TextTrimming.CharacterEllipsis,
            Text = SkinService.FmtTime(h.Time),            // fmtTime（:574）
        };
        var meta = new Grid { MinHeight = 20 };
        meta.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        meta.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(4) });
        meta.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        Grid.SetColumn(tag, 0);
        Grid.SetColumn(time, 2);
        meta.Children.Add(tag);
        meta.Children.Add(time);

        // .history-name（:1031-1042）：12px text-dim 单行省略
        var name = new TextBlock
        {
            Style = (Style)FindResource("Micro"),
            Padding = new Thickness(3, 1, 3, 1),
            TextTrimming = TextTrimming.CharacterEllipsis,
            Text = h.Name,
        };

        var item = new StackPanel { MinWidth = 120, Margin = new Thickness(0, 0, 12, 12) }; // .history-item（:978-982）
        item.Children.Add(preview);
        item.Children.Add(meta);
        item.Children.Add(new Border { MinHeight = 20, Child = name }); // .history-name-row（:1028-1030）
        return item;
    }

    /// <summary>「历史皮肤（N）」标题是 XAML 占位（无 x:Name），按文案特征在视觉树里定位。</summary>
    TextBlock? FindHistoryCountText()
    {
        foreach (var tb in Descendants<TextBlock>(this))
            if (tb.Text.StartsWith("历史皮肤（", StringComparison.Ordinal))
                return tb;
        return null;
    }

    /// <summary>历史空态文案 = 历史头部 Grid 里与 pane-head 同级的那个 TextBlock（沿 TxtHistorySearch 爬上去最近便）。</summary>
    TextBlock? FindHistoryEmptyText()
    {
        if (TxtHistorySearch.Parent is not Grid searchHost
            || searchHost.Parent is not Grid paneHead
            || paneHead.Parent is not Grid row) return null;
        return row.Children.OfType<TextBlock>().FirstOrDefault();
    }

    // ================= 操作 =================

    // ---------------- 微软档案上传（SkinsView.vue :442 pending + :478 上传） ----------------
    byte[]? _pendingBytes;
    string _pendingName = "";
    string _pendingVariant = "classic";

    LaunchSession State => (LaunchSession)DataContext;

    /// <summary>导入皮肤（:477「选择皮肤文件…」）：本地照旧入库/置当前；选中的是微软账号时
    /// 同时进入待上传态（:442 的 pending box），由「上传」推到微软档案。</summary>
    void PickSkin_Click(object sender, RoutedEventArgs e)
    {
        var dlg = new OpenFileDialog
        {
            Title = "选择皮肤文件",
            Filter = "PNG 皮肤|*.png|全部文件|*.*",
        };
        if (dlg.ShowDialog() != true) return;
        string? err = SkinService.ImportSkin(dlg.FileName);
        if (err is not null)
        {
            MessageBox.Show(err, "导入皮肤", MessageBoxButton.OK, MessageBoxImage.Warning);
            return;
        }
        ShowSkin(reload: true);
        ReloadHistory();
        SetPending(dlg.FileName);
    }

    /// <summary>pending box（:442-478）：预览缩略 + 经典/纤细选型 + 移除。</summary>
    void SetPending(string path)
    {
        try
        {
            _pendingBytes = File.ReadAllBytes(path);
            _pendingName = Path.GetFileName(path);
            _pendingVariant = SkinRenderer.IsSlimSkin() ? "slim" : "classic";
            var bmp = new System.Windows.Media.Imaging.BitmapImage();
            bmp.BeginInit();
            bmp.CacheOption = System.Windows.Media.Imaging.BitmapCacheOption.OnLoad;
            bmp.StreamSource = new MemoryStream(_pendingBytes);
            bmp.EndInit();
            bmp.Freeze();
            PendingPreviewImg.Source = bmp;
            PendingBox.Visibility = Visibility.Visible;
            ApplySegStyle();
        }
        catch { _pendingBytes = null; PendingBox.Visibility = Visibility.Collapsed; }
    }

    void ApplySegStyle()
    {
        bool slim = _pendingVariant == "slim";
        SegClassic.Style = (Style)FindResource(slim ? "SkinSegItem" : "SkinSegItemActive");
        SegSlim.Style = (Style)FindResource(slim ? "SkinSegItemActive" : "SkinSegItem");
    }

    void SegVariant_Click(object sender, RoutedEventArgs e)
    {
        _pendingVariant = ReferenceEquals(sender, SegSlim) ? "slim" : "classic";
        ApplySegStyle();
    }

    void ClearPending_Click(object sender, RoutedEventArgs e)
    {
        _pendingBytes = null;
        PendingBox.Visibility = Visibility.Collapsed;
    }

    /// <summary>上传（:478 uploadSkin）：需要微软正版账号；成功后脸缓存已刷新，预览即时跟上。</summary>
    async void UploadSkin_Click(object sender, RoutedEventArgs e)
    {
        var acc = State.SelectedAccount?.Model;
        if (_pendingBytes is null) { State.Status = "先选择皮肤文件再上传"; return; }
        if (acc is null || acc.Kind != "microsoft")
        {
            MessageBox.Show("上传到微软档案需要先登录微软正版账号（账号页 → 微软正版登录）。",
                "上传皮肤", MessageBoxButton.OK, MessageBoxImage.Information);
            return;
        }
        BtnUploadSkin.IsEnabled = false;
        State.Status = "正在上传皮肤到微软档案…";
        try
        {
            await MicrosoftAuth.UploadSkinAsync(State.Settings, State.Paths, State.Accounts, acc,
                _pendingBytes, _pendingVariant, _pendingName, State.Log);
            State.Status = $"皮肤已上传到微软档案（{_pendingVariant}）";
            SkinRenderer.InvalidateFace(acc.Uuid);
            SkinRenderer.FaceFor(acc.Uuid);   // 预热新脸
            SkinSourceReload();
        }
        catch (Exception ex)
        {
            State.Status = "皮肤上传失败：" + ex.Message;
            MessageBox.Show(ex.Message, "上传皮肤", MessageBoxButton.OK, MessageBoxImage.Warning);
        }
        finally { BtnUploadSkin.IsEnabled = true; }
    }

    /// <summary>若当前皮肤源就是这个微软账号的脸，上传后立刻刷新预览。</summary>
    void SkinSourceReload() => ShowSkin(reload: true);


    /// <summary>换回（:558）：历史文件复制回当前皮肤路径并刷新预览与历史（时间戳置顶）。</summary>
    void ApplyHistory(SkinHistoryEntry h)
    {
        string? err = SkinService.ApplySkin(h.Id);
        if (err is not null) MessageBox.Show(err, "换回皮肤", MessageBoxButton.OK, MessageBoxImage.Warning);
        ShowSkin(reload: true);
        ReloadHistory();
    }

    /// <summary>删除历史（:566）：确认后移除条目与存储文件。</summary>
    void DeleteHistory(SkinHistoryEntry h)
    {
        if (MessageBox.Show($"确定删除历史皮肤「{h.Name}」？", "删除历史皮肤",
                MessageBoxButton.YesNo, MessageBoxImage.Question) != MessageBoxResult.Yes) return;
        string? err = SkinService.RemoveSkin(h.Id);
        if (err is not null) MessageBox.Show(err, "删除历史皮肤", MessageBoxButton.OK, MessageBoxImage.Warning);
        ReloadHistory();
    }

    // 分段控件：行走 / 待机（SkinsView.vue:377-392，previewAnim 默认 walk）
    void Seg_Click(object sender, System.Windows.RoutedEventArgs e)
    {
        _animActive = (Button)sender;
        bool walk = ReferenceEquals(_animActive, SegWalk);
        SegWalk.Style = (Style)SegWalk.FindResource(walk ? "SkinSegItemActive" : "SkinSegItem");
        SegIdle.Style = (Style)SegIdle.FindResource(walk ? "SkinSegItem" : "SkinSegItemActive");
        SkinDoll.Walk = walk;
        PlaceAnimBlob();
    }

    /// <summary>把 .seg-blob 摆到当前选中段上：量选中按钮相对 seg 内层 Grid 的矩形（等价 offsetLeft/offsetWidth）。
    /// 只定几何不做弹性过渡，过渡留给任务 #13。</summary>
    void PlaceAnimBlob()
    {
        _animActive ??= SegWalk;   // :59 默认 previewAnim = 'walk'
        if (_animActive.ActualWidth <= 0 || AnimSegGrid.ActualWidth <= 0) return;
        var b = _animActive.TransformToVisual(AnimSegGrid)
                    .TransformBounds(new Rect(0, 0, _animActive.ActualWidth, _animActive.ActualHeight));
        SegBlob.Width = b.Width;
        // top/bottom 各内缩 3px（:825-826），Grid 行高已含父 Border 的 padding 3
        SegBlob.Margin = new Thickness(b.X, 3, 0, 3);
        SegBlob.Opacity = 1;
    }

    // 剩余两件靠 JS 才成立的几何，本轮按契约 §6.8「结构留位、动画不做」保留占位：
    //
    // 1) 行走/待机的 blob 位移过渡：原版 left/width 走 190ms ease-out（SkinsView.vue:826-831），
    //    现在只做瞬定，接动效库时把 PlaceAnimBlob 改成补间即可。
    //
    // 2) 拖拽反馈 —— .row-main.drag-over{box-shadow:0 0 0 3px var(--accent-soft)}（:661-663）
    //    + .row-main.drag-over .pane{outline:2px solid var(--accent)}（:1102）。
    //    WPF 没有 outline，需要在外层套一层 Border 或用 Adorner 表达，故本轮不实现，
    //    落点已经写在 SkinsPage.xaml 的 pane-preview / skin-operation-panel 注释里。

    // ---------------- 树遍历（历史计数/空态文案定位用） ----------------

    static IEnumerable<T> Descendants<T>(DependencyObject root) where T : DependencyObject
    {
        int n = VisualTreeHelper.GetChildrenCount(root);
        for (int i = 0; i < n; i++)
        {
            var child = VisualTreeHelper.GetChild(root, i);
            if (child is T hit) yield return hit;
            foreach (var sub in Descendants<T>(child)) yield return sub;
        }
    }
}
