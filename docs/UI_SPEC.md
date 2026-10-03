# WUHUICL UI 规格总索引

本轮唯一目标：**把 UI 做到百分百还原**——坐标、颜色、叠层、模糊、动画全部从 KAMUCL-1.1.5 硬搬。
不加功能、不动引擎。所有实现对着本索引逐屏推进，逐屏验收。

## 1. 规格文档清单（唯一真值源）

| 文档 | 回答什么问题 | 行数 | 可信度 |
|---|---|---|---|
| `UI_GEOMETRY_HOME.md` | 窗口/侧栏/顶栏/首页每一处像素坐标 | 229 | **本人逐条复核源码** |
| `UI_GEOMETRY_PAGES.md` | 其余 13 个页面的版面、行列、控件尺寸 | 340 | 子代理提取，关键值已抽查 |
| `UI_ICONS.md` | 92 个 `<svg>` 的 viewBox/描边/配色/尺寸 + 光栅资产 | 664 | 子代理提取，草方块与微软色已复核 |
| `UI_MOTION.md` | 15 个 `@keyframes` + 124 条 `transition` 的时长/缓动/关键帧值；§8.3 开机碎块落地四个坑 | 669 | 子代理提取，token 与首页 6 项已复核 |
| `UI_LAYERS.md` | 19 处 `backdrop-filter`、多层背景、伪元素层、z-index 全表、阴影环 | 1088 | 子代理提取，hero 三处玻璃底已复核 |
| `UI_ACCENT2_AUDIT.md` | `--accent-2` 全库台账：哪些已改、哪些判"不该改"、哪些随功能未实现 | 69 | 子代理审计，逐条回源码复核（纠正 2 处） |

约定：**文档说不清就标了 `file:line`，直接去源码看那一行**。文档里所有"未找到"都是真的没有，不是漏写。

已复核为真的关键数值（我自己去源码量过，不是转抄）：
`--sidebar-w 208` `--banner-h 430`（被 `max-height:360` 夹住）`--radius 14` 导航项 `48` 子项 `40` 顶栏 `72`（≤1080 时 68）logo 区 `88` 状态条 `1.12fr 0.95fr 0.92fr` 最小高 `68` 启动组合 `310×76` 箭头 `48` 实例行 `36px minmax(0,1fr) auto 36px 40px` 最小高 `72` 右列 `minmax(250px,290px)` 窗口 `1360×860`/最小 `960×620` `--motion-fast 130ms`（ui-system.css:73 覆盖 styles.css:62 的 120ms）`--motion-carousel 500ms` 轮播默认 `6.5s`（`src/shared/appearancePolicy.ts:5`，夹在 1–120s）。

## 2. 血泪规矩（写 UI 前必读，逐条都是踩出来的）

1. **样式键名不能与主题画刷键同名。** `x:Key="Card"` 的样式会被主题字典里的 `Card` 画刷盖掉，`StaticResource` 反向查找先命中后合并的字典，于是把 Brush 赋给 `Style` → 解析期抛"设置属性 Style 时引发了异常"。已踩过两次（`Nav`、`Card`）。
   守门脚本：`python tools/check_resource_keys.py`（非 0 退出即撞车）。卡片样式因此叫 `Panel`。
2. **隐式 `TextBlock` 样式里不要设 `Foreground`/`FontFamily`。** 样式 setter 的优先级**高于属性继承**，会把按钮/模板里靠继承取色的文字压成 `Fg`（深色），图标字符会被中文字体换成豆腐块。前景色一律由 `Window.Foreground` 继承，控件模板各自设。（已修）
3. **图标必须跟随 `currentColor`。** 原版 90 处图标都是 `stroke/fill="currentColor"`。`IconStroke`/`IconFill` 因此绑到宿主 `ContentControl.Foreground`，**不能写死 `Fg`**，否则彩底按钮上的图标变深色。要给图标换色就给宿主设 `Foreground`。
4. **XAML 注释里不能出现 `--`**（也不能以 `-` 结尾）。把 CSS 变量名 `--sidebar-w`、`--motion-fast` 原样抄进注释会导致 `error MC3000`。文档里写令牌名请去掉前缀。
5. **进程 DPI 感知只能到 system aware，不能声明 PerMonitorV2。** 实测混合 DPI 双屏（主屏 3200×2000@200% + 副屏 3840×2160@100%）：manifest 写 `PerMonitorV2` 时 `GetDpiForWindow` 会跟着屏变（192→96），但 **WPF 的渲染缩放不跟**（`place` 日志里副屏仍是 scale=2），于是 100% 屏上整套界面被画成两倍大；更糟的是窗口在两块屏之间往返一次尺寸就翻倍（1360×860 DIP → 2720×1720 → 搬回主屏 5440×3440，直接掉到屏外，截图上半截是黑的）。
   现在按 `system` 声明（`app.manifest` + `App.xaml.cs` 的 `DpiAwareness.EnsureSystemAware()` 兜底），跨屏交给 DWM 位图缩放，DIP 恒为真值；`Services.ScreenFit.FitToMonitor()` 在起窗时把窗口夹进所在屏工作区，保证任何时候都看得见。
   验证：主屏 `DpiScaleX=2` 且 `Width` 仍是 1360（不是 2720）。
6. **禁用态不要用 `Opacity=0.45` 表达 `filter:saturate(.75)`。** 原版启动按钮禁用时仍是实色，只降饱和度；用透明度会把整块（含白字）洗灰。启动按钮禁用 = `Opacity 0.9`；副按钮 `.hero-settings:disabled` 才是 0.45。
7. **子代理给的数值必须自己复验。** 首轮几何报告里 4 处是错的（strip 高 68/82 混用、箭头 48/62、hero 结构、"首页有日志卡"）。文档里凡"本人复核"字样才可直接照抄。
8. **`CornerRadius` 不能照抄 CSS 的 `border-radius: 999px`。** 浏览器会把半径按盒子尺寸同比例收缩（均匀 radius 时等效 `min(高,宽)/2`），所以 999 是"胶囊惯用法"；WPF 不收缩，按字面值画弧 —— 36 高的元素上 999 就是上下两条平缓弧线的一片**透镜**，用户连着几轮点名的"椭圆"就是这个。
   统一走 `ctl:CssRadius.Pill=True`（附加属性，`SizeChanged` 时按 `min(高,宽)/2` 回写圆角，等价浏览器的收缩规则）；定高元素也可以直接给半高（36→18、30→15）。全仓 9 处字面 999 已全部替换。
9. **页面容器 `MaxWidth` + `HorizontalAlignment=Center/Left` 会塌成内容宽度。** CSS 的 `max-width:940px; margin:0 auto` 是"撑满后封顶"；WPF 里非 Stretch 对齐的元素按 **DesiredSize** 排布，而 `Grid` 的 `*` 列贡献 0 宽度 → 整页被最宽的固定子元素决定（模组页实测 940 塌成 560）。
   写法：`Width="{Binding ViewportWidth, RelativeSource={RelativeSource AncestorType=ScrollViewer}}"` 配合 `MaxWidth` 封顶（等价 `width:100%;max-width:…`）。已修 8 处：Accounts/Community/Friends(×2)/Keys/Mods/Recordings/Bridge。

## 3. 逐屏验收清单

状态标记：`- [ ]` 未开始 · `[~]` 已排版未校色/未加动画 · `[x]` 与源码逐项对齐并通过截图核对

### 3.0 外壳（所有页面共用）
- [~] 窗口 1360×860 / min 960×620 / 圆角 10 / 1px 描边 / 顶部 1px 内高光
- [ ] DWM 材质：`DWMWA_SYSTEMBACKDROP_TYPE`（原版 Windows 用 `backgroundMaterial:'acrylic'`），禁用 `AllowsTransparency`。**收益评估**：默认黑紫的 `--shell-surface` 是 96% 不透明，开了也看不见；只有暗 30% / 亮 42% 两套才有差别，而代价是窗口底色必须改透明（WPF 无 `AllowsTransparency` 时透区会填黑）。排在 hero 玻璃之后
- [x] 模态遮罩模糊：`Services/MaskBlur.cs` 快照式（见 U26）
- [x] 侧栏 208：logo 区 88（**brand-head 是玩家皮肤的脸**，42×42 / radius 10 / 1px `rgba(255,255,255,.26)` / 阴影 `0 3px 12px rgba(0,0,0,.18)` / 最近邻放大，见 App.vue:1239+1772；名称 20/700 字距 2 + 版本 12 右对齐 `accent-2`）
- [x] 导航 8 项（首页/游戏版本/资源管理▸/联机/默认配置/皮肤/社区资源/设置），项高 48，gap 4，padding `4 12 12`，图标 20 容器 / 19 svg；**两层独立**：`.nav-bubble` 底色 `NavPill`（135° accent-soft80%→hover）跟悬停跑、无悬停时停在选中项，`.nav-selection` 只有左侧 3px 竖条、恒停在选中项；选中字色 `NavActiveFg`（text 84% + accent 16%）不是 `Fg`
- [x] 资源管理子菜单展开：`grid-template-rows 0fr→1fr` 等价动画 + 子项 40 高、左缩进 32、13px
- [~] 底部健康条：min-height 42，grid `8px 1fr 14px`，7px 圆点 + 3px 光环，margin 12
- [x] 顶栏 72：唯一拖拽区（WPF 用 `CaptionHeight=0` + 顶栏 `DragMove`，双击切最大/还原），**格言行在 spacer 带内居中**（不是整条顶栏居中：原版是 flex，动作区占右侧自然宽），右侧 `下载(36h) 历史 通知 | 最小化 38×36 最大化 关闭(hover 红底白字)`
- [x] 顶栏格言逐字动效：见 `UI_MOTION.md` §4.6 与 §12 表尾
- [x] 开机碎块动画：见 `UI_MOTION.md` §10
- [ ] 内容区 padding 24，滚动条槽常驻 8px

### 3.1 首页（最优先，逐像素）
- [~] hero 卡：实高 360，radius 20，底 `#17231F`，13 层叠放（见 `UI_LAYERS.md` F 节）
- [ ] hero 轮播：4 张内置图，`opacity 500ms ease` 交叉淡入，间隔 6.5s，**下一张预加载完成才切**（不闪第一张），`object-position center 42%`
- [ ] hero 遮罩用**最终值**：`linear-gradient(180deg, rgba(0,0,0,.16), rgba(0,0,0,.18) 30%, rgba(0,0,0,.68))`（基础段的横向渐变是死值）
- [~] kicker「当前版本」：min-height 34 / padding 5 10 / radius 7 / 12px·650 / `rgba(9,13,14,.48)` + blur12
- [ ] 标题：`clamp(24px,2.6vw,32px)`、weight 850、行高 1.25、2 行截断、字距 **-1px**、`#fff` + `text-shadow 0 4px 24px rgba(0,0,0,.32)`
- [~] 版本行：版本号 15px + loader 徽章（padding 5 11 / radius 999 / 11px·650）
- [~] 版本设置 52h·min-w150·radius `10,0,0,10` + ⋯ 52×52·radius `0,10,10,0`，底 `rgba(10,16,17,.58)` + blur13
- [~] 启动组合：min-w 310 × 76，radius 14，主按钮 21px/800 字距 .5px，箭头 48 宽 + 左 1px 分隔；阴影 `0 12px 32px accent38%` + `inset 0 2px 0 white14%`
- [ ] 启动组合动画：hover `translateY(-2px)` 190ms ease-out，active `scale(.99)`，进度覆盖层 `rgba(255,255,255,.25)` 宽=百分比
- [~] 三格状态条：`1.12fr 0.95fr 0.92fr`，min-height 68，padding 8，radius 15，格间 1px 分隔，格内 `34px 1fr 15px` gap 11 padding `10 12`，图标 27×27，chevron 14×14 opacity .7
- [ ] 状态条 blob：跟随指针在三格间滑动，`left/width 190ms ease-out` + `scale .97→1`，radius 12，上下缩 4px，底 accent10%
- [~] 最近游戏：卡 radius 14 padding 16，标题 17px/750 + 4×17 主题色竖线，管理实例按钮 34h radius 9
- [ ] 实例行：`36 * auto 36 40` gap 12 min-height 72 padding 12，无圆角无描边只留底边线；hover→`--hover`，选中→`--accent-soft`；名称 13px/650 两行截断，副行 11px，启动键 40×34 radius 8
- [ ] 实例卡入场：`card-in 220ms`，**源码明确不做 stagger**（卡片同时出现，保证操作同时可用）
- [ ] 空态：min-height 110，**dashed** 1px `--border-strong`，radius 13，纯文字无图标（当前被拉高 + 按钮撑满是 bug）
- [~] 右列 290：账号卡 + 皮肤卡拼成一张连体卡（上卡 radius `14,14,0,0` 去底边，下卡 radius `0,0,14,14` **margin-top -16** 去顶边）
- [~] 账号卡：头像 54×54 radius 12 + 4px 外环，名字 16px，状态 11px + 7px 绿点，更多 30×30，类型行 `26px 1fr 15px` min-height 44 margin-top 15 radius 9，provider 方块 24×24（微软四色 22×22）
- [~] 皮肤卡：头 14px/700 + 副 10px，刷新 30×30，舞台 230 高 radius 12 + 径向+线性双层底，覆盖条 左12右12底12 min-height 34 radius 9 底 card78% + blur12
- [~] 说明卡（CreatorCard 位）：padding 20 radius 14，边框 accent26%，正文 12px/1.7
- [ ] 日志：**必须是模态弹窗**（`z 9100`，遮罩 blur5，对话框 `min(760, 100vw-64)` × `min(620, 100vh-80)`，radius 16，日志体 11.5px/1.65 等宽），不是首页卡片
- [ ] 浮层菜单：`.float-menu` width=触发按钮宽（JS），`card-float-menu` 232 宽，菜单项 34h（卡内 40h/13px），blur24

### 3.2 其余页面（先照 `UI_GEOMETRY_PAGES.md` 排版，再逐屏截图核对）
- [~] 游戏版本：tab 胶囊（36h，指示块 `inset 0 -2px accent` + JS 定位）/ 文件夹卡 `auto minmax(160,320) minmax(60,1fr)` / 工具行 / 列表行 42h radius 10 / 已安装行 80h / 更多菜单 290 宽
- [~] 设置：**4 行 × 2 列 grid**（`minmax(0,1fr) minmax(220px,340px)`，gap 10，max-w 1080），页内滚动；范围段 38h/15px·650，分类胶囊 32h radius 6；卡片 padding `14 18`，折叠头 42h，折叠箭头 7×7 旋转 45°；主题项 48h + 预览块 46×32 radius 5；内存滑块自绘（轨道 6px，滑块 16×16 带 2px 描边）
- [~] 账号：max-w 720，类型 tab `repeat(3,minmax(0,1fr))` 实心 accent-grad，账号行 42h padding `12 16` radius 10，头像 42 圆
- [~] 皮肤：grid `minmax(260,2fr) minmax(300,3fr)`，3D 视口 380 高，分段控件 36h + blob 内缩 3px
- [~] 社区：单列 gap 12，结果网格 `repeat(auto-fit,minmax(min(100%,300px),1fr))` gap 16，结果卡 padding 16 radius 14，图标 46×46
- [~] 服务器（资源管理子项，`App.vue:182`）：`minmax(260,1fr) minmax(300,1.05fr)`（≥1121 时 `minmax(360,1.2fr) minmax(320,1fr)`），行 86h 只留底边线，激活态 accent-soft + radius 6。**页内无页签**，详情面板就是 ServerDetails.vue 那一条链
- [x] 联机（主导航项，`App.vue:117`）：`选择联机方式` 落地页 + 三张方式卡（FRP 内网穿透 / VoxLink 联机 / 陶瓦联机），卡内 `.pick-icon` 42×42 内层 22、`em` 标签、`.pick-scene` accent-2、`.pick-go` →；进方式页只留 `← 更换方式`，三个面板正文未接入
- [x] MOD 面板（资源管理子项，`App.vue:177`）：max-w 860 单列，状态卡 grid `24px minmax(0,1fr)` gap 16 padding 24，参数区默认 Collapsed，控件宽 150/200/160
- [~] 模组/资源包/光影：同一 `FileManager`，行 `36 * auto 36 40`（mods 7 列 `18 32 * 88 132 42 40`），行 68h
- [~] 默认配置 / 录像 / 联机(服务器+桥接+好友三签)：已排版并截图核对见 `UI_GEOMETRY_PAGES.md` §8–§12

### 3.3 主题
- [x] 6 套主题按原版 9 源 token + 派生公式生成（`gen_themes.py`），默认**黑紫**
- [x] 运行时才算的派生色补成静态 token：`tools/gen_theme_mixins.py` 在每套主题里生成 `NavActiveFg` / `NavPill` / `AccentHiSoft` / `BarFill`（带 BEGIN/END 标记，可重跑刷新）
- [ ] 每套主题逐表面核对 alpha：`shell-surface` 黑紫 96% / 暗 30% / 亮 42%；`card` 88%/94%；`bg-2` 26%/38%；`--shadow` 亮 `0 2px 8px rgba(31,50,85,.045)` 暗 `0 2px 8px rgba(0,0,0,.16)`
- [ ] 逐主题截图（当前只核过白蓝与黑紫首页）

## 4. 已知缺陷台账（本轮要清零）

| # | 现象 | 根因 | 处理 |
|---|---|---|---|
| U1 | `开始游戏`/`版本设置` 文字发黑 | 隐式 TextBlock 样式设了 `Foreground`，压过继承 | **已修**：删 setter，靠 `Window.Foreground` 继承；图标改绑宿主 Foreground |
| U2 | 空态被撑高、"去安装"拉成蓝色大块 | 内部 Button 未设 `VerticalAlignment`，默认 Stretch | **已修**：按钮居中，空态保持 min-height 110 |
| U3 | 私有内存偏高 | 实测（DPI 修好后）：ctor 43 MB → 首帧 137 MB → 加载在线列表 152 MB → 首次轮播交叉淡入 203 MB → 稳定 **271 MB**；关掉首页只剩外壳是 136 MB。面积 A/B 证明这 136 MB **不随窗口面积缩放**，是固定开销（WPF/DWM 重定向 + 图标几何栅格化），不是图片解码 | 目标 ≤150 MB：先压掉轮播淡入的 +50 MB（两幅全尺寸图同时参与 alpha 合成），再查固定开销 |
| U4 | 动画缺失 | 未实现 | **进行中**：导航水滴/选条 300ms、按钮按压 140ms、图标键 hover 130ms、状态格 hover 180+200ms、模态 220ms、hero 交叉淡入 500ms、子菜单展开 190ms 已做；`card-in` 入场、跑马灯、进度条过渡待做 |
| U5 | 无玻璃/叠层深度 | 未实现 | 按 `UI_LAYERS.md` F 节的层数清单搬（hero 13 层、模态 9-10 层）；4 处真正可见的模糊：kicker / hero 副按钮 / 皮肤覆盖条 / 遮罩族 |
| U6 | 标题字距 -1px 无法表达 | WPF `TextBlock` 无 letter-spacing | 自绘 `FormattedText` 或接受近似（记录在案） |
| U7 | hero 图未预加载切换 | 定时器 6s 且无预加载门 | **已修**：6.5s（`appearancePolicy.ts:5`）+ 双层交叉淡入 500ms |
| U8 | 页面注释里含 `--` 导致 MC3000 | 把 CSS 变量名原样抄进 XAML 注释 | 见规矩 4；构建前用 `tools/check_resource_keys.py` + 编译把关 |
| U9 | 侧栏 logo 是草方块 | 把 `.brand-head` 当成应用图标 | **已修**：原版就是玩家皮肤的脸（App.vue:1239），三处头像统一走 `SkinRenderer.Face`，重载皮肤一起刷 |
| U10 | 选中导航项没有底色块 | 只做了 3px 竖条，漏了 `.nav-bubble` | **已修**：两层独立（bubble 跟悬停、selection 恒在选中项），bubble 底色改 `NavPill` 渐变，选中字色改 `NavActiveFg`；实测水滴位移层首次定位不抖（`from` 取不到值时直接用目标值） |
| U11 | 像素头像/纸娃娃发糊 | `TransformedBitmap` 在 .NET WPF 里只能走 WIC 插值（没有 `BitmapTransformOptions`） | **已修**：`SkinRenderer.Face` 改自己逐像素铺 `WriteableBitmap`；`PaperDoll` 的 `DrawingVisual` 补 `NearestNeighbor` |
| U12 | 一批本该 `accent-2` 的地方用了 `accent` | 原版 `--accent-2` 是运行时 `color-mix` 出来的浅色，移植时被当成 `--accent` | **已修 14 处**（全量审计见 `docs/UI_ACCENT2_AUDIT.md`）：logo 版本、`.icon-btn:hover`、`.top-btn:hover`、`.capsule.active`、`.mirror-option.active`、`.menu-item:hover`、`.instance-play`、`.loader-badge` 边框、`.runtime-item:hover`、`.runtime-state strong`、账号状态行、进度条与内存条填充、格言重音字 |
| U13 | 开机碎块看不到脸 / 整脸不出现 | 见 `UI_MOTION.md` §8.3 四个坑（Viewport 单位、Visibility 层、插值放大、渐变坐标系） | **已修**并截图验证：漂浮期玻璃、汇聚期带脸部像素、定型整脸清晰 |
| U14 | 侧栏导航节距 48，原版 52（.nav flex gap 4 漏做） | StackPanel 无项间距 | **已修 2026-10-02**：Nav/NavParent/NavSub 加 Margin 0,0,0,4，子菜单容器边距 16,0,0,0；截图比对节距一致 |
| U15 | 设置页范围胶囊未选中态是透明底，原版是 .btn 卡片（Card 底 + Line 描边） | ScopeTab 模板漏了默认底 | **已修 2026-10-02**：未选中 Card+Line，选中 AccentSoft+accent-2 字+透明边；分类胶囊选中字色改 accent |
| U16 | 设置页搜索框旁多了独立清除按钮、主题卡无折叠头、底部无「提示左+个性化右」工具行 | 自创结构 | **已修 2026-10-02**：照 SettingsView.vue:1234/:660/:674 重排 |
| U17 | 模组页头按 resource-page 列流做，原版 mods 是 row（标题左/按钮右） | FileManager.vue:507 只有资源包/光影切列流 | **已修 2026-10-02**：mods 行内布局，ApplyResourceKind 搬 FmActions |
| U18 | 模组页批量条/分页常显、行内状态胶囊自创、实例选择器是 240 ComboBox | v-if 条件没落地 | **已修 2026-10-02**：批量条（v-if selection.size）与分页（v-if pageCount>1）默认 Collapsed；主行=文件名单行、已禁用/已锁定走第二行；选择器改 440 宽两行卡片 |
| U19 | 联机页信息架构不符：原版 1.1.5 联机落地页是「选择联机方式」三卡（FRP 内网穿透 / VoxLink 联机 / 陶瓦联机），C# 直接做了服务器列表三签页 | 上一轮把 ServersView / BridgeView / FriendConnectView 三个独立 view 并成一条 `.connect-tabs` | **已修 2026-10-02**：新建 `Pages/FriendsPage.xaml`（落地页 + 方式页壳，卡文案照 `FriendConnectView.vue:16-39`），ServersPage 删掉页签带与两个占位壳 |
| U20 | 原版资源管理子菜单有 6 项（模组/资源包/光影包/录像/MOD 面板/服务器），C# 只有 4 项 | MOD 面板、服务器未建页也没接导航 | **已修 2026-10-02**：补 `NavBridge`(MOD 面板) + `NavServers`(服务器)，顺序照 `App.vue:158-186`；新建 `Pages/BridgePage.xaml`；`Icon.Bridge`/`Icon.Servers` 按 `App.vue:179/:184` 补进 Icons.xaml |
| U21 | 顶栏动作区与原版不符 | 原版 `.top-actions` 有 5 个条件项（App.vue:1333/1342/1351/1359），C# 缺「导入」，且把首页那个图标钮标成了「历史启动」 | **已修 2026-10-02**：补 `BtnTopImport`（与下载同为非首页显隐），首页图标钮改回「更新日志」；仍缺 App.vue:1351 的「下载中心」图标钮（条件是 `首页 && activeTaskCount`），等下载队列接入 |
| U26 | 模态遮罩没有背景模糊 | WPF 无 `backdrop-filter` | **已修 2026-10-02**：`Services/MaskBlur.cs` 打开弹窗时把 `ShellRoot` 快照成 1/2 降采样位图铺在 tint 下并挂 `BlurEffect(Radius = css×2)`，关闭即释放。日志窗 blur(5) 已接，实测整片 UI 糊而弹窗本体清晰 |
| U27 | hero 上三块玻璃（`.hero-kicker` blur12、`.hero-settings`/`.hero-more` blur13）仍是纯 tint | 需要"采样身后"的能力；`BlurEffect` 糊的是元素自身，`VisualBrush` 方案要每帧重绘 HeroStage（两张全尺寸图 ×3 个 pill），与 U3 的内存目标冲突 | **未修（有意推迟）**：按 `UI_LAYERS.md` G.1 的 (c) 方案做**预模糊降采样位图**（1/4 尺寸、随轮播换源、pill 位置变化时改 `ImageBrush.Viewbox`），零每帧开销。这三块都在 ≥48% 深色 tint 下、面积小，收益远低于模态遮罩，故排在断点与剩余动效之后 |

## 5. 不要实现的东西（源码里是死代码，照搬会错）
| U22 | 皮肤 3D 纸娃娃渲染严重错误：侧脸长着脸、帽层错位、透明处把内层挖洞、开帽层时满屏垃圾三角形 | 四个独立根因：① `Box()` 里 `right` 面复用了 `front` 的贴图区块；② 帽层拿几何尺寸(9³/8.5³)当贴图区块尺寸用，而 skinview3d 两者分开传（`model.ts:132-133`）；③ WPF 3D 没有 alpha test，透明片元照样写深度；④ 逐纹素切格时把"格子尺寸"当成"这一面共几个纹素"传给分母，子面片跑到盒子外面 | **已修 2026-10-02**：几何尺寸与贴图尺寸分开传；`right=(u+w+d, v+d, d, h)`；每纹素一个面片并丢掉全透明格（等价 `alphaTest 1e-5`）；空网格整块不加入（WPF 画空 mesh 会出垃圾几何）；内层不给 BackMaterial、只有帽层双面。正面/背面两个角度截图验证 |
| U23 | 服务器详情面板混进联机内容 | 上一轮把 `VoxLinkPanel.vue:406` 的「在公共大厅显示」开关和一块自造的四列 network-metrics（Java 版本/协议/最大人数/同步时间）铺进了 ServerDetails | **已修 2026-10-02**：两处删除，补回 `ServerDetails.vue:18` 的「服务器版本」与 :22 的 Minecraft/最近使用两行；连带删掉失去引用的 `SrvToggle`/`SrvMetricCell` 样式 |
| U24 | 顶栏格言文案与原版不一致 | 上一轮自创了「专心给玩家打造的启动器」 | **已修 2026-10-02**：按需求回到原句「尝试打造一个方便玩家高频使用的启动器」（`CreatorMotto.vue:8` 的 UP主→玩家），重音仍落在两个字符上 |
| U25 | 游戏版本页页头多了「安装所选版本」钮与「显示快照与旧版」复选框；文件夹卡多一行按钮 | 原版页头只有标题+副标题，安装动作在行内；文件夹卡是单行 | **未修**：等版本页收口时按 `GameView.vue` 页头与 `.folder-card` 版式改回 |

### 2026-10-02 第三轮：12 页全量对照原版截图重做

核对方法：`D:\tmp\pagesweep.ps1` 逐页起进程（`--page=` 是启动参数）用 PrintWindow 拍 `v4_*.png`，
与上一轮拍的 `k_*.png`（原版 1.1.5 运行包）逐元素比；颜色边界用 `D:\tmp\colscan.py` 按列取样确认，不靠肉眼。
这一轮抓出来的 8 条：

| # | 现象 | 根因 | 处理 |
|---|---|---|---|
| U28 | **首页 hero 图偏下、卡片顶部露一条缝** | `.hero-image` 的 `object-position:center 42%` 用 `Image + RenderTransform` 表达，而 Image 在 UniformToFill 下的基准对齐随布局变；实测基线是顶对齐，`shift=+extra*0.08` 就把整张图推到卡片顶边以下（量得 17.5 DIP，与 shift 完全相等） | **已修**：改成 `ImageBrush` 的 `Viewbox`（相对图片归一化裁剪窗）精确算 cover + 42%，不再依赖元素对齐；同时按 DIP 而非像素算 cover 尺寸（banner 的 PNG DPI 是 72，两者差 1.33 倍） |
| U29 | 首页账号卡头像外环是**圆圈**套在圆角方块头像上 | `box-shadow: 0 0 0 4px` 跟随元素自身 `border-radius:12px` ⇒ 应是圆角方环；移植时画成了 `Ellipse` | **已修**：换成 62×62 / CornerRadius 16 / BorderThickness 4 的 Border（外圆角 = 12+4） |
| U30 | `.loader-badge`（hero 上的"原版/Fabric 0.19.3"）在非黑紫主题下是一块不相干的深色 | 底色写死成黑紫主题的 `#5C1E2E24`，没有走主题 token | **已修**：改用 `LoaderBg` / `LoaderFg`（accent 30%，逐主题生成） |
| U31 | 「运行状态」第三格：空状态显示"就绪"+ 蓝点 | 状态机没照 `HomeView.vue:192-200` 的四态做；点色用了 accent，且缺 ready/running 的 3px 光环 | **已修**：补 等待选择/正在准备/游戏运行中/就绪 四态；点色 idle 灰(`Idle`)、ready/running 绿(`Ok`)；光环 `OkSoft` 3px 用外溢 13×13 圆表达（不占布局）。新 token `Idle`/`OkSoft` 由 `gen_theme_mixins.py` 补进 6 套主题 |
| U32 | **皮肤页分段控件只剩一个空胶囊**，选中项"行走"看不见；旁边一个空方块 | `.seg-blob` 的 left/width 原版由 JS 量 `.seg-btn` 后写入，移植时按"动画待做"留成 Width=0/Opacity=0 —— 但 `.active` 的字色是 on-accent（白），白底卡片上等于隐形。这是把"几何"误当成"动效"推迟了 | **已修**：`PlaceAnimBlob()` 量选中按钮相对 seg 内层 Grid 的矩形并定位（瞬定，弹性过渡仍归任务 #13）；空方块是错用了 `Icon.Restore`（圆角方块），补 `Icon.ResetView`（SkinsView.vue:395-398 的圆圈 + 四向刻度） |
| U33 | 皮肤页 3D 视口残留一圈 **1px dashed 虚线框** | `.preview-3d-empty` 是 3D 观察器的 v-else，人偶挂上后整块不该存在；代码只藏了里面的占位文字 | **已修**：`ShowSkin()` 收起整个 `PreviewEmptyState` |
| U34 | 非首页**缺返回按钮** | `.top-back` 整件没做 | **已修**：新增 `TopBack` 样式（36×36 / radius 6 / card-2 底 / 1px Line / hover 字色 Fg + 描边 BorderStrong，App.vue:2033-2058）+ 访问栈 `_visited` 驱动显隐与回退；首页清空历史 |
| U35 | 返回按钮箭头方向朝右 | `Icon.ChevronLeft` 复刻的是 App.vue:1313 那条 `m9 18 6-6-6-6`，**几何本身就朝右**，键名误导 | **已修**：返回按钮改用 `Icon.Back`（App.vue:1328 `m15 18-6-6 6-6`，朝左）。`Icon.ChevronLeft` 的键名与几何不符已在此登记，后续要么改名要么补一条真左指几何 |

- `server-row-in`、`pick-card-in`：被后面的 `animation:none` 取消。
- `.card-float-menu`、`.instance-more-menu`、`.launch-success`、`.connection-panel` 的 `backdrop-filter`：底色已全不透明，模糊不可见。
- `--glass-blur: 28px`：零消费者。
- `SplashScreen.vue`：未被引用；真正开屏是 `splash.ts` + `shared/startup.ts` 的 canvas（900ms 漂浮 / 1400ms 汇聚 / 2000ms 停留，smootherstep）。
- HomeView `:782-1028` 基础段中被尾部压缩规则覆盖的值（hero 高 430、strip 82、箭头 62、`.instance-grid` 四列网格、元信息槽 130 高）。
- `.nav-sub-item::before` 的 5px 圆点：`content:none`，不画。

## 6. 验收方式（每屏都要走一遍）

1. `dotnet build` → `python tools/check_resource_keys.py`
2. 启动并截图：`D:\tmp\run10.ps1`（PrintWindow 抓指定 PID，`SetProcessDPIAware` 保证 2× 尺寸）
3. 局部放大核对：`D:\tmp\crop.ps1 -InPath ... -X -Y -W -H -Zoom`
4. 对照本清单逐项打勾；数值不一致时回到 `UI_*.md` 的 `file:line` 看源码，不要凭眼睛估。

## 8. 文档与源码冲突（照抄文档会错，取右侧）

| 位置 | 台账 §5/§12 说 | 源码更严的说法 | 采用 |
|---|---|---|---|
| 服务器详情标题 | h2 16px（connection.css:114） | ServersView.vue:528 收窄为 14px/500 | 记为冲突，待逐屏核对时定 |
| `.server-connect` 高 | min-height 42（§5） | ServerDetails.vue:30 为 44 | 同上 |

## 7. 本轮交付快照（2026-10-02 凌晨）

已建并逐屏截图核对的页面（默认「黑紫」+「白蓝」两套均过）：
首页 · 游戏版本 · 设置 · 账号 · 皮肤 · 社区资源 · 模组/资源包/光影（一套壳三形态） · 默认配置 · 录像 · 联机（服务器/桥接/好友三签）。
外壳：侧栏 208 + 导航水滴/选条滑动 + 资源管理可展开子项 + 顶栏 72 拖拽 + 窗口三键。

**尚未收口的项（按影响排序）**
1. 断点/响应式**只覆盖了一半页面**（更正：先前写"全线缺失"是错的，实测有 5 页已做）：
   已做 = 首页、游戏版本、社区资源、默认配置、联机（联机按 `ServersPage.ApplyMetrics()` 复刻了 1480/1120 宽窄档、两列 1.2fr/1fr 与 1/1.05fr 互换、`100vh-270` 滚动上限与窄窗 static）；
   未做 = 设置、账号、皮肤、模组/资源包/光影、录像。WPF 没有 `@media`，统一走 `SizeChanged` + 阈值函数。
   已知窄窗缺陷：联机页 `.network-metrics` 四列在窄面板下把 "OpenJDK 21" 截断成 "OpenJ..."。
2. 玻璃叠层未做：`UI_LAYERS.md` 的 4 处可见模糊（kicker / hero 副按钮 / 皮肤覆盖条 / 遮罩族）与 DWM 材质还没接，深度感比原版平。
3. 动效只做了一半：导航位移、页签指示块、按钮按压、hover 渐变、模态入场、轮播交叉淡入已做；`card-in` 入场、跑马灯、进度条过渡、状态条 blob 跟随指针未做。
4. 内存：默认首页 271–277 MB，其中外壳固定 ~136 MB、轮播淡入 +50 MB，未达 ≤150 MB 承诺。
5. 字距（letter-spacing）全线缺失：hero 标题 -1px、logo 2px、启动按钮 0.5px 表达不出来。

### 2026-10-02 第二轮（1:1 重写冲刺）
- **皮肤 3D 渲染重写完成**：新建 `Controls/SkinDoll3D.cs`（Viewport3D），几何/UV/相机/灯光/动画/交互全部从
  `vendor/skinview3d/model.ts` + `SkinViewer3D.vue` + `skinModel.ts` 逐参数硬搬（头 8³ 帽 9³、limb 挂点/枢轴、
  setUVs 六面公式、fov45 distance=max(20,10/aspect)/tan22.5°、yaw -0.35、行走 cos(t·4.71)·π/4、拖拽/滚轮/双击）。
  两个关键坑：① WPF 变换是行向量约定，Transform3DGroup.Children 按挂入顺序作用（先 yaw 后平移 / 先旋转后平移）；
  ② 3D 材质里 ImageBrush.ViewportUnits 必须用 Absolute——RelativeToBoundingBox 是按每个面自己的包围盒铺整图。
  贴图最近邻 16× 预放大逼近 pixelated。首页右列与皮肤页 3D 视口均已接入，行走/待机分段控件生效。
- 首页收口：顶栏 下载/历史 按页显隐（App.vue:1333/1359，注意 Nav_Checked 解析期 PageHome 为空要 OnLoaded 补 Show）；
  空态改虚线框+居中纯文字（HomeView.vue:913，无按钮）；离线状态字色 text-dim；头像 4px 外环。
- 游戏版本页收口：文件夹卡单行「新版本安装位置 + select + 路径 + 管理文件夹(浮层)」；已安装行补 管理 按钮 +
  最近游玩文字；编译时文件夹卡浮层 Popup 已接。
- 皮肤页收口：3D 视口换 SkinDoll3D（380 高），行走/待机分段控件接线，loading 覆盖条按三态收起。
- 驱动陷阱再次确认：**截图必须是 run10.ps1 新拉起的实例**——手动 Start-Process 的窗口截不到（v10.png 会是旧图），
  本轮曾因此误判改动没生效排查了 1 小时。ui 修改后一律：stop → build → run10 → 截图。
- 遗留（下轮）：联机方式选择页（U19）、MOD 面板/服务器导航项（U20）、内存 595 MB 待压、皮肤页右栏 Steve 占位文案。

### 2026-10-02 复核补记（对照原版 1.1.5 运行包逐页截图）
- 驱动方式：鼠标注入对两版都不可靠，原版需带 `--force-renderer-accessibility` 重启后走 UIA Invoke；
  C# 版走 UIA SelectionItemPattern。驱动脚本：`D:	mp\kuia.ps1`（原版）/ `D:	mp\wuia.ps1`（C#）。
- 原版截图存 `D:	mp\k_*.png`，C# 版 `D:	mp\w_*.png`；两版侧栏节距、设置页、模组页已对齐。
- 游戏版本页两签（版本下载/已安装）原版版式：tab 胶囊选中=AccentSoft 底+accent 字+底部 2px accent 条；
  文件夹卡单行「新版本安装位置 [select] 路径 管理文件夹 N」；已安装行=星标+图标+名称+元信息+管理/启动/更多+最近游玩。
  C# 版页头多了「安装所选版本」与显示快照复选框，文件夹卡多一行按钮（登记为 U25）。
