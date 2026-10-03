# UI 复刻对账台账

回答"UI 是不是 100% 复刻"用的。不靠肉眼印象，靠一份可重跑的数表。

## 口径

- 生成器：`python tools/ui_element_inventory.py --parity`（底稿 `--json` 落 `docs/ui_inventory.json`）
- 分母：原版视图 `<template>` 里出现、**且真的有样式规则**的类名（本文件 `<style>` 或全局
  `styles.css`/`ui-system.css`）。纯状态字面量（`:class` 三元里的 `'download'` 之类）不计入，避免虚增缺口。
- 命中判据：类名归一化（去大小写、去连字符，`.switch-ui` ↔ `SwitchUi`）后出现在对应 XAML 里。
  依据是本项目约定：每个还原元素都带注明原版类名的注释，或复用同名 Style 键（`.tag` → `Tag`）。
- 所以**命中只代表"这个类有人管"，不代表数值一定对**；未命中里也混着改了名的已实现项与原版死代码，
  两侧都要人工判定，判定结果记在第 2 节。
- 内容不在本页 XAML 里的（原版 Teleport 到 body 的模态层、由 C# 现建的主题卡），检索范围由
  `TELEPORT_HOST` 补上对应落点（壳的 MainWindow.xaml / MainWindow.xaml.cs）。补进来的每条都要
  能在那个文件里搜到注释或 Style 键名，不然就是虚高。

## 1. 机器初筛（2026-10-03）

| 原版视图 | 有样式的类 | XAML 命中 | 覆盖率 | 未命中（按声明数排序） |
|---|---|---|---|---|
| App.vue | 120 | 36 | 30.0% | `mp-name-opt`, `notice-panel`, `mp-keysync-opt`, `top-icon-btn`, `mp-conflict-actions`, `mp-conflict`, `nav-icon`, `mp-replace-confirm`, `drop-box`, `drop-mask`, `notice-empty`, `mp-radio` …(+72) |
| HomeView.vue | 87 | 42 | 48.3% | `instance-copy`, `log-dialog`, `skin-overlay`, `account-copy`, `java-option`, `provider-mark`, `manage-instances`, `skin-refresh`, `instance-more`, `skin-tip`, `instance-last`, `account-more` …(+33) |
| GameView.vue | 106 | 50 | 47.2% | `inst-names`, `loader-option`, `isolation-item`, `folder-manager-actions`, `isolation-paths`, `version-id`, `instance-commands`, `folder-manager-main`, `folder-missing-card`, `folder-scan-error`, `latest-release`, `row-actions` …(+44) |
| ServersView.vue | 22 | 18 | 81.8% | `connection-muted`, `modal-mask`, `modal`, `modal-actions` |
| BridgeView.vue | 43 | 40 | 93.0% | `bridge-apply-tag`, `switch-ui`, `bridge-select` |
| FriendConnectView.vue | 17 | 17 | 100.0% | — |
| KeysView.vue | 41 | 38 | 92.7% | `drag-active`, `switch-ui`, `menu-overlay` |
| SkinsView.vue | 73 | 68 | 93.2% | `drag-hint`, `history-name-input`, `muted`, `spin`, `hidden-input` |
| CommunityView.vue | 73 | 46 | 63.0% | `file-row`, `modal-field`, `modal-title`, `file-list`, `file-name`, `modal-actions`, `file-main`, `file-sub`, `file-side`, `ver-menu-empty`, `files-loading`, `modal-label` …(+15) |
| FileManager.vue | 60 | 42 | 70.0% | `upd-name`, `upd-row`, `upd-error-detail`, `fm-result-list`, `upd-foot`, `upd-head`, `upd-list`, `upd-ver`, `upd-err`, `upd-check`, `upd-spin`, `upd-empty` …(+6) |
| RecordingsView.vue | 26 | 26 | 100.0% | — |
| SettingsView.vue | 141 | 41 | 29.1% | `settings-search-results`, `runtime-grid`, `mem-slider-thumb`, `upd-release-item`, `memory-value`, `java-auto-row`, `settings-navigation`, `settings-grid`, `group-inline`, `mem-slider-track`, `memory-refresh`, `java-scan-track` …(+88) |
| AccountsView.vue | 71 | 47 | 66.2% | `profile-options`, `provider-preview`, `user-code`, `insecure-confirm`, `copy-hint`, `modal-actions`, `provider-empty`, `ms-uri-row`, `ms-waiting`, `provider-error`, `modal-title`, `ms-tip` …(+12) |
| **合计** | **880** | **511** | **58.1%** | |

## 2. 人工判定（逐页，本人复核后才落行）

| 视图 | ALREADY（改名已实现） | DEAD（原版死代码） | MISSING（真缺） | 判定依据 |
|---|---|---|---|---|

## 3. 人工判定汇总（四路只读审计 + 本人抽验）

审计法：按页把「模板里用到的类」与「XAML 里已实现的元素」对表，每类给
`ALREADY / DEAD / MISSING`。本人抽验了其中 12 条（CreatorCard 组件、java-picker 模态、
`.content .result-dl` 覆盖、Toast 缺失、版本行无图标、勾选行 accent 5% 等），全部与结论一致。

### 已按判定修掉的（本轮）

| 项 | 依据 |
|---|---|
| 顶栏图标钮 36×36 / 圆角 10 / 图标 17 / hover card-2+text | App.vue:2099-2118（原来错用 `.icon-btn` 32/r6/hover accent-2/15） |
| 下载角标 16 高胶囊（原来套 `.tag` 的 25 高盒） | App.vue:2224-2235 |
| 通知铃铛补 `.bell-dot` 7×7 未读点 | App.vue:1371 + :2121-2129 |
| `.top-actions` 间距 4、分隔线 1×20 margin 8 | App.vue:2068-2073 / :2363-2369 |
| 版本下载行：删掉自造图标，改成 版本号+类型标签+日期 / 右侧 已安装标签+安装按钮 | GameView.vue:1066-1092 |
| 版本标签文案 `release` → 正式版/快照/Beta 旧版/Alpha 旧版，配色按 tag-gold/tag-cyan | GameView.vue:344-353 |
| 版本日期只显示日期（原来重复了一遍类型） | GameView.vue:1071 |
| `.btn.btn-sm.btn-ghost` 组合样式（版本行每行一个） | styles.css:220-238 + :258-263 |
| 模组勾选行底色 accent 5%（新增 AccentPick 派生画刷） | FileManager.vue:719 |
| 开关选中态纯色 accent（原来自作主张用了 accent-grad） | styles.css:396-399 |

### 还缺的（按可见度排序，这就是剩下的工作清单）

**A. 默认屏就能看到**
1. 首页右侧 `CreatorCard`（20 个名字气泡 + ↗ 链接 + 0fr→1fr 展开）被自造的「这一版能做什么」顶替 — CreatorCard.vue:1-32。
2. 首页 `home-dashboard` 缺 `max-width 1160` 约束 — HomeView.vue:1030。
3. 首页 hero 标题缺 `letter-spacing -1px` 与 `text-shadow 0 4px 24px rgba(0,0,0,.32)`，字号 30 应为 clamp 24–32。
4. 运行状态条缺 `.runtime-blob`（accent 10% 的跟随水滴）与 `.runtime-state>svg` 的 accent-2 色。
5. 皮肤面板缺 `.skin-overlay` 三个分支（加载中 / 出错重试 / 提示）与 `.spin`。
6. 社区页下载按钮 `.result-dl` 被 `ui-system.css:47` 压成 card-2 描边款，我这里仍是金色实心。
7. 设置页整页结构错位（见 B 节）。

**B. 设置页（缺口最大，29% → 需按分类重建）**
- 导航：原版 2 个 scope（启动器设置/游戏设置）+ 8 个 category，且 `v-show="category===…"` **一次只显示一个分类**；我这里 1 个 scope + 6 个错名胶囊、所有分区常驻堆叠。
- 缺的整块：功能管理（10 行 feature-row + 2 个 feature-section-title）、个性化背景 HomeLayoutEditor、减少动态效果 group-inline、下载 collapse（镜像组/最大线程/限速/高级 details）、新版本安装目录 dir-row、版本隔离 group-inline、runtime-grid（内存 collapse + Java collapse + JVM 跨列卡）、游戏窗口分辨率、通用两行、关于与更新（6 行 + 3 按钮）、插件列表、回滚/本地更新两个模态、搜索结果浮层。
- 我这边**自造、原版没有、要删**的：页头副标题、主题说明文字、Java 路径/浏览、min/max MB 输入、窗口宽高、全屏启动、下载与网络卡、账号卡、游戏目录卡、第三个镜像胶囊。
- 数值错：`.page-title` 应 20px/700（我用了 H1 24px）；`group-title` 应 700 + margin-bottom 8；`.group-hint` 缺 line-height 1.6 与 margin 6 0；`.collapse-body` 缺 border-top（只有 1/7 有）。

**C. 浮层与模态（整族缺失）**
- 壳层：Toast 栈（Toasts.vue 全组件）、通知面板 + 更新日志面板、下载中心面板（含 indeterminate 进度）、拖拽 drop 遮罩、modpack 冲突对话框族（mp-*）、UpdateModal、配置不符遮罩。
- 版本页：实例管理浮动菜单（11 项）+ 6 个模态（窗口/Java/重命名/文件夹重命名/隔离迁移/安装）。
- 首页：版本与卡片两个浮动菜单、Java 选择模态、重启确认模态、ConfirmModal、日志失败条与空态。
- 社区页：下载模态（文件列表 + 版本选择 + 校验）、ModInstallDialog、版本筛选菜单、骨架屏、status-strip、加载更多行、错误分支、SelectMenu 弹层。
- 模组页：更新检查面板（upd-*）、批量结果 details、无实例空态、4 个模态。

**D. 死代码（明确不做，避免照抄出错）**
`fm-disabled-tag`（FileManager 样式里有、模板零引用）、`.result-head .tag` 的 tag-cf/tag-success 变体（被 :1080 统一压平）、`more-sentinel`（无样式）、`settings-navigation`（:1836 `display:contents` 让 :1233 的卡片失效）、`nav-sub-item::before`（:1935 `content:none`）、GameView 的 folder-summary/inst-path/folder-shortcuts/iso-switch 等 11 个纯样式类、`.launching`（SFC 内无规则）。

**E. 动效缺口**（详见 UI_MOTION.md 对账）
路由 fade 180/80ms、toast 进出场 220ms、`spin` 0.8s、`dl-indeterminate` 1.25s、进度条宽度过渡 .3s、`nav-sub-item` 逐条淡入、`creator-bubble` reveal+drift、`mask-in`/`modal-in`（我缺 scale）、mp 选项 150ms 过渡、`prefers-reduced-motion` 全量归零。

## 4. 复跑记录

### 2026-10-03 复跑（第 5 批：换基线 1.1.10 → 图标生成器 → 全局复选框 → 安装弹窗）

分母从 880（1.1.5 的 13 个视图）换成 1098（1.1.10 的 13 个视图 ＋ 9 个新组件）。
`python tools/ui_element_inventory.py --parity`：**962 / 1098 ＝ 87.6%**（换基线当天开局是 86.0%）。
剩下的 136 项全部落在 1.1.10 新增的组件上（任务 19-22）加 HomeView 的 `no-banner` 一个状态类。
GameView 109/109、App.vue 125/125 回到满。

**图标：Icons.xaml 改成 100% 生成，不再允许手写。**
`tools/gen_icons.py` 换源到 `KAMUCL-1.1.10`，目录里 51 个条目的行号锚点全部重钉到 1.1.10
（锚点漂了 39 个，签名匹配逐条复核过：`Icon.Play` 取 hero 那颗 531 而不是 597 的副本、
`Icon.ChevronRight` 取 548 的 `.runtime-chevron`、`Icon.Ellipsis` 取 518 的 openCardMenu）。
新增解析 `components/UiGlyph.vue` 的 JS 路径表 → 21 个 `Icon.G.<name>`（viewBox 24、stroke 1.7、
圆头圆角，组件模板写死，所以只解析表体）。

这一批踩的坑要记下来：第一次重跑生成器把 6 个**手写**在 Icons.xaml 尾部的图标（Bridge／Servers／
Bars／Gamepad／Cube／ResetView）整段冲掉了，而 `DynamicResource` 撞不到键是**静默不画**，
不报异常——MainWindow 的两颗导航图标、联机三卡的图标、皮肤页的回正视角钮会同时消失。
处理：先从备份取回这 6 段，再逐个回源核对——它们在 1.1.10 里其实都是行内 SVG，于是全部收进生成器
目录（App.vue:191/192/196/201、FriendConnectView.vue:22/30/38、SkinsView.vue:399），
手写段整体删除，`Icons.xaml` 从此只有一个来源。顺带核出手抄的两处实差：
`Icon.Gamepad` 少了 VoxLink 那颗里两个 `fill=currentColor` 的实心点（源 :30 的两个 circle），
`Icon.Cube` 的轮廓少了闭合段。生成器现在 72 键／123 条 Path，与源里 svg 子元素 1:1。

**全局复选框（styles.css:119-152）做成一份共享模板。**
1.1.10 把 `input[type=checkbox]` 换成 appearance none 的自绘盒：20×20（`--check-size`）、
描边 1.5 `color-mix(text-dim 65%, border)`、radius 6、底 card-2、hover 描边转 accent、
勾选 accent 实心 ＋ 白色对勾（viewBox 24 的 `m5 12 4.5 4.5L19 7`，stroke 3，background-size 76% → 15.2）、
indeterminate 同位置画 `M6 12h12` 横线、disabled opacity .45。
这套画成一个 `ControlTemplate x:Key="CheckBoxMark"`（ContentControl），五个渲染点共用：
默认 CheckBox 样式、`Check`、`VpMenuCheck`（.float-menu 的「实例隔离」行）、`SrvCheck`（服务器批量选择，
原来是 18×18 自画）、`Sh.MpReplaceConfirm`（原 18×18）、`Sh.MpKeysyncOpt`（原 13×13 UA 盒）。
之前 indeterminate 只填了底色没画横线，也一并补上。唯一另立尺寸的是 `.check-option input`（26，
`CheckOption` 自带一份，注释点名了为什么没走共享 mark）。
顺带记一条给任务 21 用：`CommunityFavorites.vue:104` 的 `.favorite-select input{width:17px}`
被全局的 `!important` 宽高顶掉，是死样式，别照着画。

**安装弹窗（GameView.vue:1420-1492）换三段壳。**
`.game-install-modal`（:1501）宽 `min(620, 100vw-32)`、高上限 `calc(100dvh-32)`、padding 0、
overflow hidden、flex column、radius 20（顶掉 `.modal` 的 radius-lg 14）；
`.install-header` padding 22 24 8 ＋ gap 16，标题 26px/700（基线 16px），右侧新增 `.icon-btn` 关闭钮
（32×32、里面 16 的 `Icon.G.close`）；`.install-content` padding 0 24 20 ＋ 自身 overflow auto；
`.install-footer` padding 16 24、margin 0（顶掉 `.modal-actions` 的 margin-top 24）、上边框 1，
按钮 min-height 42／min-width 116／radius 12／gap 8，「确认安装」带 20px `Icon.G.download`。
加载器选择从 36 高胶囊改成 5 等分格磁贴：72 高、radius 14、padding 10 6、13px，
上方 23px 的 `Icon.G.{none,forge,fabric,quilt,neoforge}`、与文字 gap 6，active 多一层
`0 0 14px accent 12%` 光晕（新主题键 `LoaderGlow`，6 套主题都加了，CSS blur 与 WPF BlurRadius 仍按 1:1）。
Fabric API 那一行从「标题＋开关」换成 `label.check-option.fapi-head`：26 的复选框 ＋
strong（14/600）＋ small（12、margin-top 4、行高 1.6）。弹窗内的 `.modal-label` 14px／margin 20 0 10，
`.input`／`.select` 42 高／radius 12（为此把 TextBox 与 ComboToggle 的圆角改成走 `ctl:CssRadius.Round`，
默认仍是 10，其余界面零变化），`.loaders-error` 加 padding 9 12／底 danger-soft／radius 9／行高 1.5，
`.inst-hint` 行高 1.5→1.7（20.4），`.fapi-tip` 行高 1.6（19.2）。
实测截图 `D:\tmp\a3_install.png`：磁贴 145 设备像素＝72 CSS、弹窗宽 615≈620、五列 gap 8 对得上。

**存量差异逐条比过（1.1.5 → 1.1.10，只看已移植的 13 个视图）**：
把两边 `styles.css`／`ui-system.css` 与各视图 `<style>` 拆成「选择器 → 声明块」做全量对比，结果是
`styles.css` 113 条**逐字节不变**、新增 13 条（全是复选框与 `.check-option`）、删 0；
`ui-system.css` 73 条不变、增删 0；`:root` 的 56 个令牌**一个都没动**（所以六套主题文件继续有效）。
13 个视图里唯一变了值的既有规则是 `App.vue` 的 `.sidebar`：1.1.10 追加
`transform: translateZ(0); backface-visibility: hidden`（注释写明是为了让半透明侧栏独立合成），
纯合成提示、无像素影响，WPF 侧不需要对应物。其余 diff 全是 `<script>` 里的 JS 被正则误抓。
也就是说本批之前拍好的页面不会因为换基线而数值走形，缺口只集中在 1.1.10 的新组件上。

**已知偏差（本批留下，进任务 18 逐屏复核）**：
- `.fapi-head` 把 `.check-option` 的 gap 从 14 压到 12（App.vue 的 `.fapi-head` 带 scoped 属性，
  特异度压过全局类）；共享 `CheckOption` 模板按 14 画，差 2px。
- `RecordingModPicker` / `FavoriteModsPicker` 两颗子组件仍在弹窗里缺席（任务 21），
  XAML 里以注释点名位置，没有臆造结构。
- HomeView 的 `no-banner`（:810-813）是「banner 列表为空」时的 hero 变体：底转 surface-content、
  去阴影、kicker/settings/more 转 card-2＋1px 描边、loader-badge 转 accent-2 on accent-soft。
  本工程还没有 banner 数据源（任务 15 的预模糊玻璃一起做），不做常开开关，避免凭空多一个状态。

### 2026-10-03 复跑（第 4 批：日志窗 → 版本页已安装行 → 设置页主题卡／Java·插件·Release → 皮肤·按键·社区·模组·联机）

| 原版视图 | 有样式的类 | XAML 命中 | 覆盖率 | 未命中（按声明数排序） |
|---|---|---|---|---|
| App.vue | 120 | 120 | 100.0% | — |
| HomeView.vue | 87 | 87 | 100.0% | — |
| GameView.vue | 106 | 106 | 100.0% | — |
| ServersView.vue | 22 | 22 | 100.0% | — |
| BridgeView.vue | 43 | 43 | 100.0% | — |
| FriendConnectView.vue | 17 | 17 | 100.0% | — |
| KeysView.vue | 41 | 41 | 100.0% | — |
| SkinsView.vue | 73 | 73 | 100.0% | — |
| CommunityView.vue | 73 | 73 | 100.0% | — |
| FileManager.vue | 60 | 60 | 100.0% | — |
| RecordingsView.vue | 26 | 26 | 100.0% | — |
| SettingsView.vue | 141 | 141 | 100.0% | — |
| AccountsView.vue | 71 | 71 | 100.0% | — |
| **合计** | **880** | **880** | **100.0%** | |

**这个 100% 的确切含义**：原版 880 个「真的带样式的类」在本工程都有归属（注释点名或 Style 同名）。
它不等于「逐像素一致」——命中判据是名字，数值要逐条核，本批核出来的实差见下。

本批改掉的**真实数值差**（不是补注释）：

- 日志窗（HomeView 的 Teleport 模态）：头部多出的「复制命令／清空」两个按钮与自造说明文案删掉，
  改成原版的 标题＋`N 行 · 运行状态`＋`.log-close`；补 `.log-failure`（danger-soft 条＋导出钮）
  与 `.log-empty`（padding 70 0／14px／居中）；清空移到 footer（`btn-ghost btn-sm`，无日志时 disabled）；
  正文 `ListBox` 换 `ItemsControl`（原版是裸 `pre`，没有选中态），行高补 1.65→18.975；
  宽高补 `min(760, 100vw-64)`／`min(620, 100vh-80)`（ApplyViewportCaps）。实测：header 70.5／失败条 51／
  正文 220／footer 54，总高 346.5，与 CSS 逐条对得上。
- `.heroStatus` 补 error 档：`LaunchFailed`（原版 :161-164 的 status==='error' 或退出码非 0）驱动
  「需要检查」文案、点色 danger、光环 danger-soft、strong 字色 danger；首页那对常 Collapsed 的
  「等数据源」副本删掉，改成一份数据驱动的元素。顺带修 `launchText`：running 是「再次启动」，
  「重新启动」属于失败档（之前两档混成一个）。
- 等宽字体：`MonoFont` 补 `Cascadia Code`（原版首选，本机其实装着）与中文回退。
  之前 `Cascadia Mono, Consolas` 里汉字走 WPF 的 mesh 回退，实测日志窗同一列里
  「启动」被回退成倾斜字、「游戏退出代码」又是正的——现在整条链是 Cascadia Code ＋ 雅黑。
- 版本页已安装行（GameView）：`.inst-icon` 从 36 的静态图标改成原版那个 34×34／padding 4／
  1px 描边／card-2 底／hover 转 accent 的按钮；`.instance-more` 从行的独立列移回 `.row-actions` 里；
  `.installed-launch` 从恒为渐变实心改成 `.btn-ghost.btn-sm` ＋ 整行 hover 才转实心（:2199）；
  `.instance-name` 14px/1.4/两行 39.2（之前 13px/18/36）；行容器 `RowItem`(72 高) 换成
  `VpInstalledRow`（min-height 80／padding 12 0／无选中底色，`:last-child` 去线用「首行不画上边线」等价表达）；
  `.installed-scope` 补齐缺的 `span.muted「显示范围」` 与「刷新列表」钮，搜索框从胶囊假控件换成
  `input.input`（radius 10／min-width 160／14px／无放大镜图标），计数文案改回「N 个实例」；
  `.version-list` 删掉自造的 `MaxHeight=560`（原版注释明确写了「不再限制高度，消灭内层嵌套滚动」）。
- 设置页主题卡（BuildThemeOption）：`.theme-preview` 补 1.5px 描边与 hover→border-strong／active→accent；
  `.tp-dot` 从内容区移进 `.tp-side`（9 宽／padding-top 4／水平居中），`.tp-side` 补 1px 右分隔线，
  `.tp-top` 底色改 card ＋ 1px 下边线，`.tp-body` 补真正缺的 `.tp-block`（flex 1／radius 4／card 底＋1px 边），
  `.tp-check` 圆角 3→7（原版 radius 50%）并按 viewBox 24 重算对勾路径；
  自定义主题补 `.preview-custom`（#16161d）＋ `.tp-custom-grad`（135° 六段彩虹，opacity .85）
  ＋ `.tp-palette`（20×20 白调色盘＋drop-shadow）。
- `.collapse-arrow` 方向：原版折叠态 `rotate(-45deg)`、展开态 `rotate(45deg)`，按 CSS 顺时针符号算出来是
  「朝右／朝下」（源码注释写的「朝上／朝下」是错的）。之前我按注释做成了「朝下／朝上」，
  现在改成 7×7 直角描边角标 ＋ ±45°，与浏览器实际渲染一致。
- `.nav-parent.group-active`（App.vue:1881）补上：当前视图属于资源管理组时父项字色抬到 text。
- 其它：`.modal-actions` 的 gap 12→16（styles.css:555 是 space-3）；`.page-head` 补 margin-bottom 8
  对 flex-end 行的抬升；`.instance-label` 从 Dim(13/dim) 换成继承的 14px＋text；
  `.java-scan-track` radius 2→1.5（999 在 3px 高上收缩成 1.5）；`.java-auto-desc` 行高 19→18.6（1.55）；
  `.memory-warn` 去掉自造的 line-height（原版没有）；`.plugin-row` 从空壳补成
  info(名＋版本 muted＋meta) ＋ 删除钮 ＋ switch 三块；`.drag-hint`／`.drag-active` 两条拖拽态按
  各自度量建成 Collapsed 覆盖层（KeysPage 用 Margin=-2 的外框表达 CSS outline）。

**记档不做的偏差**（都有原因，不是漏看）：

- `.drag-hint` 的包含块：原版 `.row-main/.row-sub` 被 :1085 改成 `display:contents`，绝对定位因此塌到视口，
  实际会罩住侧栏与顶栏。WPF 页面元素出不了宿主区，这里做成铺满本页。
- `.input`／`.select` 的字号：共享 TextBox 与 ComboBox 的默认值已从 13 改成 text-md 14
  （styles.css:314 ＋ ui-system.css:15），28 处依赖默认值的输入框一起跟上；
  确实更小的四处保持写死：`.ms-uri-row .input` 13（AcUriInput）、`.dir-row .input` 13、
  `.history-name-input` 12、`.tool-search input` 13（那是 Border 假控件里的 TextBlock）。
  逐条核过：`.bridge-select`／`.select-menu-btn` 都只给宽度，跟着 14 才对。
- `.mem-slider-thumb` 的 `box-shadow 0 1px 6px color-mix(accent 45%)`：DropShadowEffect.Color 吃不了
  DynamicResource，要按主题取色得先把 effect 做成主题资源。
- 纯动画/过渡态（`.spinning`、thumb hover 1.12 与 dragging 1.18、inst-icon hover scale 1.05、
  `.instance-name` 点击改名的 focus-visible 解除两行截断）→ 待办 §13。
- 原版死样式照旧不抄：`.shell.has-bg`（与 `.shell` 基态同值，空操作）、`.shell.edit-mode`（只改 Electron
  拖窗热区）、`.hidden-input`（display:none）、`.community-page` 结果行上的 `.tag-cf`/`.tag-success`
  （被 `.result-head .tag` 以更高特异性压掉，真正生效的是版本列表的「已安装」）。

### 2026-10-03 复跑（第 3 批：首页 / 模组页 / 账号页 完成后）

| 原版视图 | 有样式的类 | XAML 命中 | 覆盖率 | 未命中（按声明数排序） |
|---|---|---|---|---|
| App.vue | 120 | 51 | 42.5% | `mp-name-opt`, `mp-keysync-opt`, `mp-conflict-actions`, `mp-conflict`, `nav-icon`, `mp-replace-confirm`, `mp-radio`, `notice-dot`, `dl-dismiss`, `dl-stage`, `notice-item`, `launchfail-text` …(+57) |
| HomeView.vue | 87 | 78 | 89.7% | `log-dialog`, `log-failure`, `log-body`, `log-close`, `log-mask`, `hero-metadata-slot`, `log-empty`, `hero-metadata`, `spinning` |
| GameView.vue | 106 | 98 | 92.5% | `inst-icon`, `instance-commands`, `row-actions`, `installed-launch`, `installed-search`, `folder-caption`, `version-list`, `switch-ui` |
| ServersView.vue | 22 | 21 | 95.5% | `modal-actions` |
| BridgeView.vue | 43 | 40 | 93.0% | `bridge-apply-tag`, `switch-ui`, `bridge-select` |
| FriendConnectView.vue | 17 | 17 | 100.0% | — |
| KeysView.vue | 41 | 38 | 92.7% | `drag-active`, `switch-ui`, `menu-overlay` |
| SkinsView.vue | 73 | 69 | 94.5% | `drag-hint`, `history-name-input`, `muted`, `hidden-input` |
| CommunityView.vue | 73 | 70 | 95.9% | `instance-label`, `tag-cf`, `tag-success` |
| FileManager.vue | 60 | 58 | 96.7% | `page-head`, `switch-ui` |
| RecordingsView.vue | 26 | 26 | 100.0% | — |
| SettingsView.vue | 141 | 112 | 79.4% | `theme-preview`, `mem-slider-thumb`, `tp-check`, `mem-slider-track`, `java-scan-track`, `collapse-arrow`, `tp-side`, `java-item-path`, `tp-body`, `tp-palette`, `plugin-info`, `java-auto-desc` …(+17) |
| AccountsView.vue | 71 | 71 | 100.0% | — |
| **合计** | **880** | **749** | **85.1%** | |

剩余缺口的性质（逐条核过，不是笼统数字）：
- **壳层 App.vue 42.5%**：modpack 冲突对话框族（mp-*）、下载中心明细行（dl-stage 族）、
  通知列表行、启动失败模态、拖拽框细节 —— 正在补。
- **设置页 79.4%** 的"未命中"里有 11 个是 `tp-*` / `mem-slider-*`：它们实现在
  `MainWindow.BuildThemeOption` 与设置页自绘滑块里，只是没沿用原版类名，属命名噪声；
  真缺的是 HomeLayoutEditor 本体与 java-list 的行内容。
- **版本页 / 首页 90% 出头**：剩下的多是 `switch-ui`、`menu-overlay` 这类共享件类名
  （我用的是 Style 键 `Switch`/`VersionMenuOverlay`），以及 `.log-*` 一族确实还在壳层里没打名字。
- 明确**不做**的：原版死代码（`fm-disabled-tag`、`settings-navigation`、`.avatar`、
  `.result-head .tag` 变体、`more-sentinel`、`.launching`），照抄反而错。
