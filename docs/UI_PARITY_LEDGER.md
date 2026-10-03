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
