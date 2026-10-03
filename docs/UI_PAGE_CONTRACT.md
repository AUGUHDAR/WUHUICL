# 页面施工契约（所有页面必须遵守）

目的：多人并行写 `Wuhuicl.App/Pages/*.xaml` 时不各造一套、不撞键名、不引入编译不过的绑定。
规格数值来自 `UI_GEOMETRY_PAGES.md` / `UI_GEOMETRY_HOME.md` / `UI_LAYERS.md` / `UI_MOTION.md`，**照抄那些文档里的数值并保留 `file:line` 注释**。

## 1. 文件与命名

- 一个页面 = `Pages/XxxPage.xaml` + `Pages/XxxPage.xaml.cs`，`x:Class="Wuhuicl.App.Pages.XxxPage"`，根是 `UserControl`。
- **只能新建/修改自己负责的页面文件。** `MainWindow.xaml`、`Themes/*.xaml`、`Converters.cs` 由主控改，别人不要碰。
- 页面根元素自己不带 `Visibility`（宿主控制显隐）。
- 页面里所有事件处理都写在自己的 `.xaml.cs`，**不要引用 MainWindow 的方法**（新页面还没有逻辑，按钮先不做跳转）。

## 2. 可直接使用的样式（`Themes/Controls.xaml`）

| 键 | 对应原版 | 关键度量 |
|---|---|---|
| `Panel` | `.content .card` | radius 14，padding 20，border 1，底 `Card`，**无阴影** |
| `CardTight` | 同上 | padding 16 |
| `CardBox` | 同上 | 同 `Panel` |
| `Btn` | `.btn` | min-height 36，padding 16/8，radius 10，14px |
| `BtnGold` `PrimaryBtn` | `.btn-gold` | accent 渐变实心 + `AccentFg` |
| `BtnGhost` `GhostBtn` | `.btn-ghost` | `CardSoft` 底 + 描边 |
| `BtnDanger` | `.btn-danger` | 红字红边，不实心 |
| `BtnSm` | `.btn-sm` | 32 高，radius 6，13px |
| `IconBtn` | `.icon-btn` | **32×32**，radius 6，内层图标 16×16 |
| `TopBtn` | `.top-btn` | 36 高，padding 12/0，13px |
| `WinBtn` `WinClose` | `.win-btn` | 38×36 |
| `Tag` `Chip` | `.tag` | radius 999，padding 8/2，12px |
| `Input` | `.input/.select` | min-height 36，padding 12/8，radius 10 |
| `Switch` | `.switch` | 40×22，滑块 14，位移 21 |
| `Check` | 复选框 | 13px |
| `CardList` + `RowItem` | `.instance-card` 行 | 行 `min-height 72 / padding 12 / 只留底边线` |
| `CardList` + `RowCard` | `.version-row` | 行 `min-height 42 / padding 12 8 / radius 10` |
| `RuntimeCell` `CellBtn` | `.runtime-item` | padding 12/10，hover 出 `AccentSoft` 浮块 radius 12 |
| `SquareBtn` | `.instance-play` | 40×34，radius 8 |
| `MenuItem2` | `.menu-item` | min-height 34，padding 10/0，radius 7，12px |
| `PopupCard` | `.float-menu` | radius 10，padding 6，border 1 |
| `ThemeChip` | `.theme-option` | min-height 48，padding 8/6，radius 6 |
| `PageTitle` | `.content .page-title` | **24px**/Bold |
| `PageSub` | `.page-sub` | 13px `Fg2` |
| `H1` `H2` | 页头 / `.instances-head h2` | 24px / **17px** Bold |
| `Lbl14` `Dim` `Micro` | 正文/次要 | 14 SemiBold / 12 `Fg2` / 12 `Fg2` |
| `HeroKicker` `LoaderBadge` `HeroSettings` `HeroMore` `LaunchMain` `LaunchArrow` | 首页 hero 专用 | 见 `UI_GEOMETRY_HOME.md` §5.2 |

字体：`{StaticResource UiFont}`（窗口已继承，一般不用写）、`{StaticResource MonoFont}`（等宽，路径/日志用）。

## 3. 可用图标（`Themes/Icons.xaml`，43 个）

一律写成 `<ContentControl Width="16" Height="16" Style="{DynamicResource Icon.Gear}"/>`。
**必须用 `DynamicResource`**，键缺失时只是不显示，不会崩。

```
Apps Back Bell CaretRight Changelog Check ChevronDown ChevronLeft ChevronRight
Close Close2 Community Download Edit Ellipsis Folder Friends Game Gear GrassBlock
Home Java Keys Maximize Memory Microsoft Minimize Mods Play Pulse Refresh
ResourcePacks Resources Restore Search Settings Shaders Skin Sliders Star
StarOutline Upload User
```

原版没有的图标（警告、垃圾桶、加号、复制、眼睛、信息、暂停、停止）在源码里就是文字字形
（`⚠ × ⋯ ▶ ✓ ↻ ＋ `），**不要自己造几何**，需要时用 `TextBlock` 放该字符并注明来源行号。

## 4. 主题画刷键（只能用这些，别新造颜色）

```
Bg NavBg Card CardSoft Line BorderStrong Fg Fg2 Fg3
Accent AccentHi AccentLo AccentFg AccentGrad AccentSoft
Ok Warn Danger Cyan ChipBg HeroChipBg LoaderBg LoaderFg
Overlay Scroll Hover WindowGrad CardShadow PopShadow
```
全部用 `{DynamicResource X}`（切主题才即时生效）。**样式键名不得与这些同名**（`tools/check_resource_keys.py` 会拦）。

## 5. 可绑定的数据（`LaunchSession`，`DataContext` 已从窗口继承下来）

```
Paths.Root Settings.Value.* AccountList SelectedAccount HasNoAccount
InstalledList SelectedInstalled OnlineList SelectedOnline
LogLines Status Progress Busy GameRunning CanPlay CanStop
RuntimeSummary MemorySummary RunStateText RunStateColor LastCommand
```
不在清单里的属性**不要用 Binding**（运行时静默失效，等于假实现）；需要占位就写死与原版一致的文案，并在注释里标 `// 占位：等 X 功能接入`。

## 6. 版式硬规矩

1. 数值照 `UI_GEOMETRY_PAGES.md` 的"最终值"，不要照被覆盖的基础值；每处非显然的数字后面留 `<!-- 页面.vue:行 -->` 注释。
2. 列宽照抄 CSS grid：`36px minmax(0,1fr) auto 36px 40px` → `Width="36"/*"/Auto"/"36"/"40"`，`gap` 用列间隔列或 `Margin` 表达，**不要四舍五入**。
3. `fr` 用 `*`；`minmax(a,b)` 用 `MinWidth/MaxWidth` 包住一个 `*` 列。
4. 卡片一律平面（原版 `.content .card` 明确 `box-shadow:none`），不要加阴影。
5. 滚动区：`VerticalScrollBarVisibility="Visible"`（对应 `scrollbar-gutter:stable`），滚动条槽 8px 已由全局样式定好。
6. 文字截断：单行 `TextTrimming="CharacterEllipsis"`；两行截断用 `TextWrapping="Wrap" MaxHeight="{2×行高}" TextTrimming="CharacterEllipsis"`。
7. 图标色跟随 `currentColor`：图标已经绑到宿主 `Foreground`，所以**给宿主设 `Foreground`**（例如彩底按钮上要给 `AccentFg`），不要给 Path 直接设色。
8. 动画本轮先不做（等主控统一加 Storyboard 库），但**结构要为它留位**：需要滑动的指示块用独立 `Border` + `x:Name`，不要画在模板里。

## 7. 交付自查（每个页面）

- [ ] 只动了自己的页面文件
- [ ] 每个尺寸都有出处（注释里的 `file:line`）
- [ ] 只用了第 2/3/4 节列出的样式、图标、画刷
- [ ] 没有引用 MainWindow 的方法
- [ ] 空态、有数据态、超长文本三种情况都不会撑破布局
