# KAMUCL 1.1.5 → WPF 图标与光栅素材清单

来源树（只读，未修改）：`D:/桌面/WUHUICL/KAMUCL-1.1.5/src/renderer/src/`
探测时间基准：文件均为 UTF-8；`<svg>` 出现 **92 处**，分布在 17 个文件；另有 **1 处** CSS `data:image/svg+xml` 内联图标（`styles.css:338`）。

## 0. 总量核对

| 指标 | 数值 | 证据 |
|---|---|---|
| `<svg>` 元素总数 | **92** | 逐文件计数：App.vue 27、HomeView 21、GameView 12、SettingsView 5、CommunityView 5、SkinsView 4、FriendConnectView 3、AccountsView 3、ServersView 2、SelectMenu 2、FileManager 2、BridgeView/KeysView/ModsView/PacksView/ShadersView/UpdateModal 各 1 |
| 去重后的**唯一图形**数 | **63** | 按 (viewBox, 图元序列, d 字符串序列, 图元属性) 归并 |
| viewBox | `0 0 24 24` × 90；`0 0 48 48` × 1（草方块，HomeView:584）；`0 0 22 22` × 1（微软四色，HomeView:623） | 脚本统计 |
| 描边型（`fill="none"` + `stroke="currentColor"`） | **82** | 脚本统计 |
| 实心型（`fill="currentColor"`） | **8** | HomeView:520/590/619（省略号三点）、HomeView:533/599（播放三角）、GameView:1150 + HomeView:682（收藏星）、AccountsView:421（微软单色） |
| 多色固定填充（不跟随主题） | **2** | HomeView:584 草方块、HomeView:623 微软四色 |
| `stroke-width` 分布 | `2`×55、`1.8`×10、无（实心）×10、`1.6`×5、`1.7`×3、`2.2`×2、`3`×2、`1.9`×2、`1.5`×2、`2.6`×1 | 脚本统计 |
| 用 `width/height` **属性**硬编码尺寸 | **20** 处 | 见 §F.2 |
| 由 CSS 规则决定尺寸 | **72** 处 | 见 §F.1 |
| 出现的着色 token | `currentColor` ×97、`#79c144 #5da236 #4e8a2f #8b5e34 #6f4a29`（草方块 5 面）、`#f25022 #7fba00 #00a4ef #ffb900`（微软 4 块） | 脚本统计 |

分类计数（本文 A/B/C 三节）：**A 导航与工具栏 43 · B 首页/英雄区 21 · C 状态与操作 28 = 92**。

---

# A. 导航与工具栏图标（43）

## A.1 侧边栏主导航（7 项）——字符串内联 SVG，经 `v-html` 注入

定义位置：`App.vue:105-141`（`navItems` 数组）。渲染位置：`App.vue:1258`
`<span class="nav-icon" v-html="item.icon"></span>`；标签文本在 `App.vue:1259`。
容器 `.nav-icon` = 20×20，内部 svg 由 `App.vue:1870` `.nav-icon :deep(svg){width:19px;height:19px}` 定为 **19×19**。
着色：`stroke="currentColor"`，继承 `.nav-item` 的文字色（`--text` / `--text-dim`，选中态变 `--accent` 侧）。

| # | 语义 | file:line | viewBox | sw | 型 | 渲染尺寸 |
|---|---|---|---|---|---|---|
| 1 | 首页 | `App.vue:109` | 0 0 24 24 | 2 | 描边 | 19px |
| 2 | 游戏版本（手柄） | `App.vue:114` | 0 0 24 24 | 2 | 描边＋2 实心点 | 19px |
| 3 | 联机（多人） | `App.vue:119` | 0 0 24 24 | 2 | 描边 | 19px |
| 4 | 默认配置（键盘） | `App.vue:124` | 0 0 24 24 | 2 | 描边 | 19px |
| 5 | 皮肤（衣服） | `App.vue:129` | 0 0 24 24 | 2 | 描边 | 19px |
| 6 | 社区资源（地球） | `App.vue:134` | 0 0 24 24 | 2 | 描边 | 19px |
| 7 | 设置（齿轮） | `App.vue:139` | 0 0 24 24 | 2 | 描边 | 19px |

```svg
<!-- 1 首页 App.vue:109  round caps + round joins -->
<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M3 10.5 12 3l9 7.5"/>
  <path d="M5 9.5V21h14V9.5"/>
  <path d="M9 21v-6h6v6"/>
</svg>

<!-- 2 游戏版本 App.vue:114  round caps + round joins; 两个圆点是 fill=currentColor, stroke=none -->
<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <rect x="2" y="7" width="20" height="11" rx="5.5"/>
  <path d="M7.5 10.8v3.4M5.8 12.5h3.4"/>
  <circle cx="15.6" cy="11.9" r="0.6" fill="currentColor" stroke="none"/>
  <circle cx="18" cy="13.6" r="0.6" fill="currentColor" stroke="none"/>
</svg>

<!-- 3 联机 App.vue:119  仅 round caps（无 stroke-linejoin） -->
<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
  <circle cx="8" cy="8" r="3"/>
  <path d="M2 21v-3a6 6 0 0 1 12 0v3M16 5a3 3 0 0 1 0 6M18 15a5 5 0 0 1 4 5"/>
</svg>

<!-- 4 默认配置 App.vue:124 -->
<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <rect x="2" y="6" width="20" height="12" rx="2"/>
  <path d="M6 10h.01M10 10h.01M14 10h.01M18 10h.01M7 14h10"/>
</svg>

<!-- 5 皮肤 App.vue:129 （与 SkinsView:353 同一 d，但 stroke-width 不同） -->
<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="m9 4-6 3 2 5 3-1v9h8v-9l3 1 2-5-6-3a3 3 0 0 1-6 0Z"/>
</svg>

<!-- 6 社区资源 App.vue:134 （与 CommunityView:554 同几何，后者 sw=1.6/40px） -->
<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <circle cx="12" cy="12" r="9"/>
  <path d="M3 12h18"/>
  <path d="M12 3a13.5 13.5 0 0 1 0 18"/>
  <path d="M12 3a13.5 13.5 0 0 0 0 18"/>
</svg>

<!-- 7 设置齿轮 App.vue:139 —— Feather "settings" 原样 -->
<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <circle cx="12" cy="12" r="3"/>
  <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06-.06a1.65 1.65 0 0 0 1.82.33h.01a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51h.01a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06-.06a1.65 1.65 0 0 0-.33 1.82v.01a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1Z"/>
</svg>
```

## A.2 侧边栏「资源管理」子导航（6 项）

定义位置：`App.vue:158-187`（`resourceSubItems`）。渲染：`App.vue:1296` `v-html`。
尺寸：`App.vue:1950-1957` `.nav-sub-item .nav-icon{17px}` / `… :deep(svg){16px}` → **16×16**。

| # | 语义 | file:line | sw | d（全部一致：round caps+joins，除 8 无 joins） |
|---|---|---|---|---|
| 8 | 模组（立方体） | `App.vue:162` | 2 | `M21 8 12 3 3 8v8l9 5 9-5Z` / `m3 8 9 5 9-5` / `M12 13v8` |
| 9 | 资源包（文件夹） | `App.vue:167` | 2 | `M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z` |
| 10 | 光影包（太阳） | `App.vue:172` | 2 | circle `cx=12 cy=12 r=4` + `M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4` |
| 11 | 录像（摄像机） | `App.vue:175` | 2 | rect `x=3 y=5 w=14 h=14 rx=3` + `m17 10 4-3v10l-4-3`（**无 linecap/linejoin 属性**） |
| 12 | MOD 面板（十字准星） | `App.vue:179` | 2 | `M12 2v4M12 18v4M2 12h4M18 12h4` + circle `cx=12 cy=12 r=3.5` |
| 13 | 服务器（双机箱） | `App.vue:184` | 2 | rect `3,4,18×7 r2` + rect `3,13,18×7 r2` + `M7 7.5h.01M7 16.5h.01` |

## A.3 侧边栏其它（2）

| # | 语义 | file:line | 渲染尺寸 | 说明 |
|---|---|---|---|---|
| 14 | 资源管理（父级，文件夹＋分隔线） | `App.vue:1273` | 19px（`.nav-icon`） | #9 的 d ＋ 追加 `<path d="M3 11h18"/>` |
| 15 | 子菜单展开箭头（右，展开转 90°） | `App.vue:1276` | **14px** | class `nav-caret`，CSS `App.vue:1884-1891`；`.open` 时 `rotate(90deg)` |

## A.4 顶栏 / 窗口控制（12）

| # | 语义 | file:line | 尺寸证据 | 渲染尺寸 | sw |
|---|---|---|---|---|---|
| 16 | 通知中心侧栏箭头（左） | `App.vue:1313` | `App.vue:2002` `.sidebar-health svg` | 14px | 2 |
| 17 | 返回上一视图（左箭头） | `App.vue:1328` | `App.vue:2055` `.top-back svg` | 16px | **2.2** |
| 18 | 下载（带文字按钮） | `App.vue:1334` | `App.vue:2094` `.top-btn svg` | 15px | 2 |
| 19 | 导入（带文字按钮） | `App.vue:1343` | 同上 | 15px | 2 |
| 20 | 下载中心（纯图标） | `App.vue:1356` | `App.vue:2117` `.top-icon-btn svg` | 17px | 2 |
| 21 | 更新日志（历史时钟） | `App.vue:1360` | 同上 | 17px | 2 |
| 22 | 通知（铃铛） | `App.vue:1367` | 同上 | 17px | 2 |
| 23 | 最小化 | `App.vue:1378` | `App.vue:2384` `.win-btn svg` | 15px | 2（仅 round cap） |
| 24 | 最大化/还原 | `App.vue:1383` | 同上 | 15px | 2，rect `6,6,12×12 rx=1.5` |
| 25 | 关闭 | `App.vue:1388` | 同上 | 15px | 2 |
| 26 | 拖放导入大图标 | `App.vue:1551` | `App.vue:2442` `.drop-box svg` | **48px** | 2 |
| 27 | 导出错误日志 | `App.vue:1581` | 属性 `width="14" height="14"` | 14px | 2 |

> `.bell-dot`（`App.vue:2121-2130`，7×7 `var(--accent)` 圆）与 `.sidebar-health i`（`App.vue:1982-1990`，7×7 状态圆点）是 **CSS 画的**，不是 SVG。

## A.5 视图级工具栏图标（16）

| # | 语义 | file:line | 尺寸 | sw | 备注 |
|---|---|---|---|---|---|
| 28 | 模组（子菜单镜像） | `ModsView.vue:5` | 44px（`.empty-icon`，`FileManager.vue:621-631`） | **1.6** | 与 #8 同几何 |
| 29 | 资源包（镜像） | `PacksView.vue:5` | 44px | 1.6 | 与 #9 同几何 |
| 30 | 光影包（镜像） | `ShadersView.vue:5` | 44px | 1.6 | 与 #10 同几何 |
| 31 | 搜索（版本下载页） | `GameView.vue:1006` | 14px（`GameView.vue:1656` `.tool-search svg`） | 2 | circle `11,11 r7` + `m21 21-4.3-4.3` |
| 32 | 模组（实例右键菜单） | `GameView.vue:1192` | **属性 13×13** | 2 | `.menu-item` 无 svg 规则，属性生效 |
| 33 | 资源包（菜单） | `GameView.vue:1196` | 属性 13×13 | 2 | |
| 34 | 光影包（菜单） | `GameView.vue:1200` | 属性 13×13 | 2 | |
| 35 | 重命名（铅笔） | `GameView.vue:1204` | 属性 13×13 | 2 | `M17 3a2.85 2.83 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z` + `m15 5 4 4` |
| 36 | 更换图标（图片） | `GameView.vue:1208` | 属性 13×13 | 2 | rect `3,3,18×18 r2` + circle `9,9 r2` + `m21 15-3.1-3.1a2 2 0 0 0-2.8 0L6 21` |
| 37 | 启动卡图片（照片） | `GameView.vue:1212` | 属性 13×13 | 2 | rect `3,4,18×16 r2` + `m3 16 5-5 4 4 3-3 6 6` + circle `16.5,8.5 r1.5` |
| 38 | 指定 Java（咖啡杯） | `GameView.vue:1216` | 属性 13×13 | 2 | `M18 8h1a3 3 0 0 1 0 6h-1M3 8h15v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z` + `M7 12h6M7 15h4` |
| 39 | 窗口设置（显示器） | `GameView.vue:1220` | 属性 13×13 | 2 | rect `3,4,18×14 r2` + `M8 21h8M12 18v3` |
| 40 | 搜索（服务器页） | `ServersView.vue:363` | 18px（`ServersView.vue:454`） | **1.8**，无 linejoin 属性 | circle `10.5,10.5 r6.5` + `m16 16 5 5`；`color: var(--text-dim)` |
| 41 | 搜索（社区页） | `CommunityView.vue:496` | 属性 14×14 | 2 | 与 #31 同几何；在 `.btn` 内，属性生效 |
| 42 | 下拉箭头（SelectMenu） | `SelectMenu.vue:82` | 14px（`SelectMenu.vue:129`） | 2 | `m6 9 6 6 6-6`；`color: var(--text-dim)`；`.open` 转 180° |
| 43 | 选中勾选（SelectMenu 选项） | `SelectMenu.vue:100` | 14px（`SelectMenu.vue:164`） | **2.6** | `M20 6 9 17l-5-5`；`color: var(--accent)` |
| 44 | 返回首页（账户页） | `AccountsView.vue:389` | 15px（`AccountsView.vue:705` `.back-btn svg`） | 2 | `M19 12H5` + `m12 19-7-7 7-7` |
| 45 | 个性化（滑杆组） | `SettingsView.vue:679` | 属性 15×15（在 `.btn` 内，生效） | 2 | `M4 21v-7M4 10V3M12 21v-9M12 8V3M20 21v-5M20 12V3` + `M1 14h6M9 8h6M17 16h6` |
| 46 | 顶栏下载/导入等带文字按钮 | 见 A.4 | — | — | — |

> 编号说明：A 节按**元素**计 43 个（App.vue 25 + Mods/Packs/Shaders 3 + GameView 9 + ServersView 1 + CommunityView 1 + SelectMenu 2 + AccountsView 1 + SettingsView 1）。表中 46 行含为阅读连贯而合并/补充的行，不要按行号计数。

> ⚠ **死规则**：`connection.css:47` `.connection-notice > svg {19px}` 无任何使用者 —— 全树 grep `connection-notice` 只命中 CSS 自身（`:46-49`）。移植时**不要**为它建 19px 档。

---

# B. 首页 / 英雄区图标（HomeView.vue，21）

| # | 语义 | file:line | 尺寸证据 | 渲染 | sw | 着色 |
|---|---|---|---|---|---|---|
| B1 | 版本设置（齿轮简化版） | `HomeView.vue:511` | `:836` `.hero-settings svg` | **18px** | 1.9 | `#f4f7f5`（`:834` `.hero-settings` color） |
| B2 | 更多（三点） | `HomeView.vue:520` | `:838` `.hero-more svg` | **19px** | — | fill currentColor `#f4f7f5` |
| B3 | 启动（播放三角） | `HomeView.vue:533` | `:851` `.launch-content svg` | **22px** | — | fill currentColor = `var(--on-accent)`（`:848`） |
| B4 | 版本下拉（箭头下） | `HomeView.vue:538` | `:857` `.launch-arrow svg` | **22px** | **2.2** | var(--on-accent)；`.open` rotate(180°) |
| B5 | 运行环境（Java/日历） | `HomeView.vue:548` | `:874` `.runtime-item > svg:first-child` | **27px**（≤1180px → 23px `:992`） | 1.8 | `var(--text)`，hover → `var(--accent-2)` |
| B6 | 运行条目箭头（右） | `HomeView.vue:550` | `:878` `.runtime-chevron` | 14px | 2 | `var(--text-dim)` opacity .7；≤1180px 隐藏 |
| B7 | 内存分配（芯片） | `HomeView.vue:553` | `:874` | 27px | 1.8 | 同 B5 |
| B8 | 箭头 | `HomeView.vue:555` | `:878` | 14px | 2 | 同 B6 |
| B9 | 运行状态（脉搏） | `HomeView.vue:558` | `:874`＋`:879` `.runtime-state > svg:first-child` | 27px | 1.8 | **`var(--accent-2)`**（错误态文字 `var(--danger)`） |
| B10 | 箭头 | `HomeView.vue:560` | `:878` | 14px | 2 | 同 B6 |
| B11 | 管理实例（四宫格） | `HomeView.vue:568` | `:893` `.manage-instances svg` | **15px** | 1.8（无 joins） | `var(--text-dim)` |
| B12 | **草方块实例默认图标** | `HomeView.vue:584` | `:900` `.instance-icon` | **36×36**（`:1030` 紧凑布局仍 36） | — | 5 色固定填充，见下 |
| B13 | 实例更多（三点） | `HomeView.vue:590` | `:907` `.instance-more svg` | **15px** | — | fill currentColor `var(--text-dim)` |
| B14 | 实例启动（三角） | `HomeView.vue:599` | `:912` `.instance-play svg` | **15px** | — | fill currentColor `var(--accent-2)`；hover → `var(--on-accent)` |
| B15 | 账户更多（三点） | `HomeView.vue:619` | `:928` `.account-more svg` | **17px** | — | fill currentColor `var(--text-dim)` |
| B16 | **微软四色徽标** | `HomeView.vue:623` | `:932` `.provider-mark.microsoft svg` | **22×22** | — | 4 固定色，见下 |
| B17 | 账户行箭头 | `HomeView.vue:625` | `:934` `.account-provider > svg` | **15px** | 2 | `var(--text-dim)` |
| B18 | 未登录分支箭头 | `HomeView.vue:635` | 同上 | 15px | 2 | 同上 |
| B19 | 皮肤刷新（双箭头） | `HomeView.vue:644` | `:928` `.skin-refresh svg` | **17px** | 1.9 | `var(--text-dim)`；`.spinning` 动画 `:945` `spin .8s linear infinite` |
| B20 | 皮肤提示箭头 | `HomeView.vue:655` | `:955` `.skin-tip svg` | **12px** | 2 | `var(--text-dim)`；hover `var(--accent-2)` |
| B21 | 收藏（实心星） | `HomeView.vue:682` | 属性 `width="12" height="12"` | **12px** | — | fill currentColor，`.menu-item` hover 变 `var(--accent-2)` |

## B.12 草方块（唯一 `0 0 48 48`，非主题化）

```svg
<svg viewBox="0 0 48 48" aria-hidden="true">
  <polygon points="24,5 43,14.5 24,24 5,14.5" fill="#79c144"/>
  <polygon points="5,14.5 24,24 24,29.5 5,20"   fill="#5da236"/>
  <polygon points="24,24 43,14.5 43,20 24,29.5" fill="#4e8a2f"/>
  <polygon points="5,20 24,29.5 24,43 5,33.5"   fill="#8b5e34"/>
  <polygon points="24,29.5 43,20 43,33.5 24,43" fill="#6f4a29"/>
</svg>
```
配色语义：顶面亮绿 `#79c144`、左/右草侧 `#5da236` / `#4e8a2f`、左/右土侧 `#8b5e34` / `#6f4a29`。
WPF 直接可用（polygon → PathGeometry，`Z` 闭合）：

```xml
<Canvas> <!-- 36x36，或放进 Viewbox 由 48 单位等比缩放 -->
  <Path Data="M24,5 L43,14.5 L24,24 L5,14.5 Z"     Fill="#79C144"/>
  <Path Data="M5,14.5 L24,24 L24,29.5 L5,20 Z"      Fill="#5DA236"/>
  <Path Data="M24,24 L43,14.5 L43,20 L24,29.5 Z"    Fill="#4E8A2F"/>
  <Path Data="M5,20 L24,29.5 L24,43 L5,33.5 Z"      Fill="#8B5E34"/>
  <Path Data="M24,29.5 L43,20 L43,33.5 L24,43 Z"    Fill="#6F4A29"/>
</Canvas>
```

## B.16 微软徽标（`0 0 22 22`，四块 10×10，间隙 2 单位）

```svg
<svg viewBox="0 0 22 22" aria-label="Microsoft">
  <path fill="#f25022" d="M0 0h10v10H0z"/>
  <path fill="#7fba00" d="M12 0h10v10H12z"/>
  <path fill="#00a4ef" d="M0 12h10v10H0z"/>
  <path fill="#ffb900" d="M12 12h10v10H12z"/>
</svg>
```

```xml
<DrawingImage>
  <DrawingImage.Drawing><DrawingGroup>
    <GeometryDrawing Brush="#F25022" Geometry="M0,0 L10,0 L10,10 L0,10 Z"/>
    <GeometryDrawing Brush="#7FBA00" Geometry="M12,0 L22,0 L22,10 L12,10 Z"/>
    <GeometryDrawing Brush="#00A4EF" Geometry="M0,12 L10,12 L10,22 L0,22 Z"/>
    <GeometryDrawing Brush="#FFB900" Geometry="M12,12 L22,12 L22,22 L12,22 Z"/>
  </DrawingGroup></DrawingImage.Drawing>
</DrawingImage>
```

## B 节关键 d 原文（完整，未截断）

```svg
<!-- B1 版本设置齿轮（简化 8 齿，非 A.1#7 的 Feather 版）HomeView.vue:511 -->
<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
  <circle cx="12" cy="12" r="3"/>
  <path d="M19.4 15a1.7 1.7 0 0 0 .34 1.88l.06.06-2.83 2.83-.06-.06A1.7 1.7 0 0 0 15 19.4a1.7 1.7 0 0 0-1.4 1.68V21h-4v-.08A1.7 1.7 0 0 0 8.6 19.4a1.7 1.7 0 0 0-1.88.34l-.06.06-2.83-2.83.06-.06A1.7 1.7 0 0 0 4.6 15 1.7 1.7 0 0 0 3 13.6H3v-4h.08A1.7 1.7 0 0 0 4.6 8.6a1.7 1.7 0 0 0-.34-1.88l-.06-.06 2.83-2.83.06.06A1.7 1.7 0 0 0 9 4.6 1.7 1.7 0 0 0 10.4 3H14a1.7 1.7 0 0 0 1.4 1.6 1.7 1.7 0 0 0 1.88-.34l.06-.06 2.83 2.83-.06.06A1.7 1.7 0 0 0 19.4 9 1.7 1.7 0 0 0 21 10.4V14a1.7 1.7 0 0 0-1.6 1Z"/>
</svg>

<!-- B2 / B13 / B15 三点省略号（实心） -->
viewBox 0 0 24 24 fill="currentColor"
B2  (520): circle cx=5  cy=12 r=1.8 / cx=12 r=1.8 / cx=19 r=1.8
B13 (590): circle cx=5  cy=12 r=1.7 / cx=12 r=1.7 / cx=19 r=1.7
B15 (619): circle cx=5  cy=12 r=1.8 / cx=12 r=1.8 / cx=19 r=1.8
WPF: <Path Data="M6.8,12 A1.8,1.8 0 1 1 3.2,12 A1.8,1.8 0 1 1 6.8,12 Z ..."/> 或直接三条 EllipseGeometry

<!-- B3 / B14 播放三角（实心） HomeView.vue:533 / :599 -->
<path d="M8 5.5v13l11-6.5Z"/>
WPF: <Path Data="M8,5.5 L8,18.5 L19,12 Z" Fill="{Binding …Foreground}"/>

<!-- B4 / B42 箭头下 -->
<path d="m6 9 6 6 6-6"/>          <!-- HomeView:538 sw=2.2 ; SelectMenu:82 sw=2 -->

<!-- B6/B8/B10 runtime-chevron 箭头右 -->
<path d="m9 6 6 6-6 6"/>          <!-- sw=2, round cap, 仅 cap 无 join -->

<!-- B5 运行环境（Java 杯/日历形）HomeView.vue:548 -->
<path d="M8 2v4M16 2v4M7 8h10a4 4 0 0 1 4 4v0a8 8 0 0 1-8 8h-2a8 8 0 0 1-8-8v0a4 4 0 0 1 4-4Z"/>
<path d="M8 13h8M9 17h6"/>

<!-- B7 内存分配（芯片）HomeView.vue:553 -->
<rect x="5" y="5" width="14" height="14" rx="2"/>
<path d="M9 1v4M15 1v4M9 19v4M15 19v4M1 9h4M1 15h4M19 9h4M19 15h4M9 9h6v6H9Z"/>

<!-- B9 运行状态（脉搏/活动）HomeView.vue:558 -->
<path d="M3 12h4l2-7 4 14 2-7h6"/>

<!-- B11 管理实例（四宫格）HomeView.vue:568 -->
<rect x="3"  y="3"  width="7" height="7" rx="1"/>
<rect x="14" y="3"  width="7" height="7" rx="1"/>
<rect x="3"  y="14" width="7" height="7" rx="1"/>
<rect x="14" y="14" width="7" height="7" rx="1"/>

<!-- B19 皮肤刷新（双弧箭头）HomeView.vue:644 -->
<path d="M20 7v5h-5"/>
<path d="M4 17v-5h5"/>
<path d="M6.1 9A7 7 0 0 1 18 6l2 1M4 17l2 1a7 7 0 0 0 11.9-3"/>

<!-- B21 / 收藏星（实心）HomeView.vue:682 与 GameView.vue:1150 同 d -->
<path d="m12 2 3.09 6.26L22 9.27l-5 4.87 1.18 6.88L12 17.77l-6.18 3.25L7 14.14 2 9.27l6.91-1.01Z"/>
```

---

# C. 状态与操作图标（28）

| # | 语义 | file:line | 渲染尺寸 | sw | 着色 / 备注 |
|---|---|---|---|---|---|
| C1 | 拖放导入遮罩（托盘上传） | `App.vue:1551` | 48px | 2 | `color: var(--accent)`（`App.vue:2440` `.drop-box`） |
| C2 | 导出错误日志（下载） | `App.vue:1581` | 属性 14×14 | 2 | `.btn` 内，属性生效 |
| C3 | 重置/刷新（逆时针 9 点） | `BridgeView.vue:275` | 13px（`BridgeView.vue:331` `.bridge-reset svg`） | 2 | |
| C4 | 按键恢复默认（同上） | `KeysView.vue:213` | 13px（`KeysView.vue:267` `.cfg-reset svg`） | 2 | 与 C3 完全同 d |
| C5 | 刷新（顺时针） | `GameView.vue:1027` | 属性 14×14 | 2 | 加载时换成 CSS `.spin` 环 |
| C6 | 收藏（实心星） | `GameView.vue:1150` | 属性 15×15 | — | `.fav-btn.on` **最终色 = `var(--accent)`**：`GameView.vue:1964` 的 `color:#f5b301` 被同一 scoped 块后面 `GameView.vue:2201` 的 `.fav-btn.on{color:var(--accent)}` 覆盖（同特异度、后者胜），#f5b301 是**死值** |
| C7 | 实例图标占位（立方体） | `GameView.vue:1154` | 撑满 `.inst-icon` 34×34（padding 4 → ≈26px，`GameView.vue:1751`） | 1.8 | 无 CSS 尺寸规则，SVG 无 width/height → 按容器 100% |
| C8 | 社区空态（地球 40） | `CommunityView.vue:554` | 属性 40×40 | **1.6** | 与 A.1#6 同几何 |
| C9 | 打开外部源页 | `CommunityView.vue:598` | 属性 14×14，但被 `styles.css:298` `.icon-btn svg{16px}` **覆盖为 16px** | 2 | 陷阱：属性尺寸失效 |
| C10 | MC 百科（书） | `CommunityView.vue:605` | 同上 → 16px | 2 | `M4 19.5v-15A2.5 2.5 0 0 1 6.5 2H20v20H6.5a2.5 2.5 0 0 1 0-5H20` |
| C11 | 下载（社区结果卡） | `CommunityView.vue:609` | 属性 13×13（在 `.btn`，生效） | 2 | 与 A.4#18 同 d |
| C12 | 自定义主题调色盘 | `SettingsView.vue:662` | 30px（`SettingsView.vue:1644`；紧凑布局 20px `:1869`） | 1.8 | **`color:#ffffff`**（`:1646`，不跟随主题）＋3 个 currentColor 圆点＋`drop-shadow` |
| C13 | 主题已选勾选 | `SettingsView.vue:668` | 18px 圆底（`:1621`；紧凑 14px `:1869`） | **3** | 底 `var(--accent)`，勾 `var(--on-accent)`，`padding:3.5px` |
| C14 | 关闭（回退列表） | `SettingsView.vue:1173` | 16px（`styles.css:298`） | 2 | `M18 6 6 18M6 6l12 12` |
| C15 | 关闭（本地更新包） | `SettingsView.vue:1203` | 16px | 2 | 同 C14 |
| C16 | 皮肤需微软账号（衣服 52） | `SkinsView.vue:353` | **52px**（`SkinsView.vue:1064`） | **1.6** | `color: var(--accent)`，opacity .8；几何同 A.1#5 |
| C17 | 3D 视角回正（准星圆） | `SkinsView.vue:395` | 16px（`SkinsView.vue:700`） | 1.8 | circle `12,12 r7.5` + `M12 2.5v3M12 18.5v3M2.5 12h3M18.5 12h3` |
| C18 | 无皮肤占位（人像） | `SkinsView.vue:416` | **40px**（`SkinsView.vue:743`） | **1.5** | circle `12,8 r4` + `M4 21v-1a8 8 0 0 1 16 0v1`；opacity .6 |
| C19 | 移除待上传（叉） | `SkinsView.vue:466` | 16px（`styles.css:298`） | 2 | `M6 6l12 12M18 6 6 18`（与 App.vue:1388 同） |
| C20 | 微软登录（单色四方块） | `AccountsView.vue:421` | 属性 15×15 | — | **`fill="currentColor"`**，继承 `.btn-gold` 的 `var(--on-accent)`；viewBox 0 0 24 24 |
| C21 | 已复制到剪贴板（勾） | `AccountsView.vue:577` | 属性 12×12 | **3** | `.copy-hint` 无 svg 规则 |
| C22 | 文件夹（文件管理行） | `FileManager.vue:433` | `.fm-file-icon` 32px 默认（`FileManager.vue:565-575`）；桌面版 36px（`:719`）；≤700px 28px（`:720`）；mods 32px | 1.8 | `color: var(--accent)` |
| C23 | 文件（文档＋折角） | `FileManager.vue:436` | 同 C22 | 1.8 | `M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8Z` + `M14 2v6h6` |
| C24 | 关闭更新弹窗 | `UpdateModal.vue:67` | 16px | 2 | `M18 6 6 18M6 6l12 12` |
| C25 | 服务器空态符号（双机箱） | `ServersView.vue:371` | **26px**（`connection.css:23` `.connection-symbol svg`，容器 48px） | **1.5**（无 joins/caps） | `color: var(--accent-2)`、底 `var(--accent-soft)` |
| C26 | FRP 内网穿透（信号柱） | `FriendConnectView.vue:22` | 22px（`FriendConnectView.vue:152`，容器 `.pick-icon` 42px） | **1.7** | `M4 20V9m5 11V5m5 15v-8m5 8V8`；`color: var(--accent)` |
| C27 | VoxLink（手柄） | `FriendConnectView.vue:30` | 22px | 1.7 | rect `2.5,7,19×10 rx5` + `M8 12h2m4 0h2` + 2 个 `r=0.8` currentColor 点 |
| C28 | 陶瓦联机（立方体） | `FriendConnectView.vue:38` | 22px | 1.7 | `M12 3 4 7v10l8 4 8-4V7l-8-4Z` + `M4 7l8 4 8-4M12 11v10` |

## C 节完整 d（重要操作图标原文）

```svg
<!-- C1 拖放导入 App.vue:1551 -->
<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/>
<path d="m7 8 5-5 5 5"/>
<path d="M12 3v12"/>

<!-- C3/C4 重置（逆时针）BridgeView.vue:275 / KeysView.vue:213 -->
<path d="M3 12a9 9 0 1 0 9-9"/>
<path d="M3 4v5h5"/>

<!-- C5 刷新（顺时针）GameView.vue:1027 -->
<path d="M21 12a9 9 0 1 1-2.64-6.36"/>
<path d="M21 3v6h-6"/>

<!-- C9 外部链接 CommunityView.vue:598 -->
<path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/>
<path d="M15 3h6v6"/>
<path d="M10 14 21 3"/>

<!-- C12 调色盘 SettingsView.vue:662 -->
<path d="M12 22C6.49 22 2 17.51 2 12S6.49 2 12 2s10 4.04 10 9c0 3.31-2.69 6-6 6h-1.77c-.28 0-.5.22-.5.5 0 .12.05.23.13.33.41.47.64 1.06.64 1.67A2.5 2.5 0 0 1 12 22Z"/>
<circle cx="7.5"  cy="11.5" r="1" fill="currentColor" stroke="none"/>
<circle cx="12"   cy="7.5"  r="1" fill="currentColor" stroke="none"/>
<circle cx="16.5" cy="11.5" r="1" fill="currentColor" stroke="none"/>

<!-- C13/C21/B42 勾选 -->
<path d="M20 6 9 17l-5-5"/>   <!-- sw 3（Settings/Accounts）/ 2.6（SelectMenu） -->

<!-- C14/C15/C24 关闭 X（另一种写法） -->
<path d="M18 6 6 18M6 6l12 12"/>

<!-- C19 / App.vue:1388 关闭 X（窗口按钮用的写法） -->
<path d="M6 6l12 12M18 6 6 18"/>

<!-- C20 微软单色四方块 AccountsView.vue:421 (viewBox 0 0 24 24, fill=currentColor) -->
<path d="M3 3h8.5v8.5H3zM12.5 3H21v8.5h-8.5zM3 12.5h8.5V21H3zM12.5 12.5H21V21h-8.5z"/>

<!-- 顶栏下载 / 导入 / 历史 / 铃铛（A.4#18-22） -->
download: <path d="M12 3v11"/><path d="m7 10 5 5 5-5"/><path d="M4 21h16"/>
import  : <path d="M12 15V4"/><path d="m7 8 5-5 5 5"/><path d="M4 15v4a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-4"/>
history : <path d="M3 12a9 9 0 1 0 9-9"/><path d="M3 4v5h5"/><path d="M12 7v5l3.5 2"/>
bell    : <path d="M18 8a6 6 0 1 0-12 0c0 7-3 9-3 9h18s-3-2-3-9"/><path d="M13.7 21a2 2 0 0 1-3.4 0"/>

<!-- 窗口三键 -->
minimize: <path d="M5 12h14"/>
maximize: <rect x="6" y="6" width="12" height="12" rx="1.5"/>
close   : <path d="M6 6l12 12M18 6 6 18"/>

<!-- 返回（两种！顶栏 vs 账户页） -->
topbar  : <path d="m15 18-6-6 6-6"/>                      <!-- App.vue:1328, sw 2.2, 16px -->
accounts: <path d="M19 12H5"/><path d="m12 19-7-7 7-7"/>  <!-- AccountsView.vue:389, sw 2, 15px -->

<!-- 左右向 chevron -->
right   : <path d="m9 6 6 6-6 6"/>        <!-- App:1276, HomeView:550/555/560/625/635/655 -->
left    : <path d="m9 18 6-6-6-6"/>       <!-- App.vue:1313 -->
down    : <path d="m6 9 6 6 6-6"/>        <!-- HomeView:538(sw2.2) / SelectMenu:82(sw2) / styles.css:338(data-URI) -->
up      : 由 down 旋转，无独立图标 -->
```

---

# D. 光栅资产（`src/renderer/src/assets/`，6 个）

尺寸由 PNG IHDR / WebP RIFF 头**直接读取**，并用 WPF（PresentationCore `BitmapImage`）实测解码；不是推测。

| 文件 | 真实像素 | 字节 | 编码细节 | WPF 解码 | 用途 / 引用 |
|---|---|---|---|---|---|
| `banner1.webp` | **1448 × 1086** | 113,564 (110.9 KiB) | WebP **VP8 lossy**，无 alpha、无 anim | **OK**（本机 Win11 26300） | 首页英雄区轮播第 1 张。`HomeView.vue:51` import → `:57` `builtInBanners` → `:88-96` `banners` → `:481-491` `<img class="hero-image">`，`:810` `object-fit:cover`，容器 `:799` `height:var(--banner-h)`=430px |
| `banner2.webp` | **1122 × 1402**（竖图） | 97,596 (95.3 KiB) | VP8 lossy | **OK** | 轮播第 2 张（同上，`HomeView.vue:52`） |
| `banner3.webp` | **1448 × 1086** | 133,932 (130.8 KiB) | VP8 lossy | **OK** | 轮播第 3 张（`HomeView.vue:53`） |
| `banner4.png` | **1448 × 1086** | 2,263,312 (**2210.3 KiB**) | PNG，bitdepth 8，**colortype 2 = RGB 无 alpha**，96dpi | OK | 轮播第 4 张（`HomeView.vue:54`）。⚠ 占 6 个文件总量的 **86.6%** |
| `splash-face.png` | **256 × 256** | 3,820 (3.7 KiB) | PNG RGBA（ct 6），DPI 95.99 | OK | 3 处共用：① 侧栏品牌头像 `App.vue:79` import `brandHead` → `:1239` `<img class="brand-head">`，`App.vue:1772-1780` 显示 **42×42**、`image-rendering:pixelated`；② 应用内启动拼图 `SplashScreen.vue:3` → `:75` `background-image`，12×12 块、`BOARD=288`、`TILE=24`（`:8-11`）；③ 原生闪屏 canvas `splash.ts:1` → `:41/47` `drawImage`；另 `scripts/build-native.cjs:12` 以 `/resource:…face.png` 编进 `StartupFeedback.exe` |
| `splash-face-32.png` | **32 × 32** | 399 (0.4 KiB) | PNG RGBA（ct 6），96dpi | OK | **孤儿文件：全仓库零引用。** 精确 grep `splash-face-32` 在 `D:/桌面/WUHUICL/KAMUCL-1.1.5`（排除 node_modules/.git/dist/out）返回 0 命中。是 `splash-face.png` 的 @1x 版；端口时**可不带**，或仅当图标资源 |

### .webp 结论（重要）

- **实测：这台机器（Windows 11 专业单语言，10.0.26300）上 WPF 能直接解码 3 个 .webp**，输出 `1448x1086` / `1122x1402` / `1448x1086`，与容器头一致。
- **但这依赖 OS 级 WIC WebP codec，不是 WPF 自带能力。** 同一次探测中 `HKLM\SOFTWARE\Classes\WIC\Codecs` 枚举为空（无法从注册表确认 codec 来源，可能来自「Web 图像扩展」AppX）。**Windows 10 与未装该扩展的 Win11 上会解码失败**（`BitmapImage` 抛 `InvalidOperationException`）。
- 三个 webp 均为 **lossy VP8、无 alpha、无动画**，所以转 PNG 只涉及重编码，不涉及透明度/帧丢失。
- **建议：转成 PNG（或 JPEG，因为是全屏不透明照片）再随包**。`banner4.png` 已证明同尺寸纯 PNG 会到 2.2 MB，因此若转 PNG 需压缩；若要保体积，用 WebP→PNG + `pngquant`/`oxipng`，或直接转 JPEG q82（约 150-250 KB/张）。

---

# E. 字形 / 字体图标（非 SVG）

**结论：本项目不使用任何图标字体** —— 全树无 `Segoe Fluent Icons`、无 `Segoe MDL2 Assets`、无 `iconfont`/`@font-face`/`\eXXX` 转义。`font-family` 出现的 45 处均为文本字体（`App.vue:1787` `'Segoe UI Variable Display'`、`styles.css:126` `system-ui,'Segoe UI','Microsoft YaHei'`、`styles.css:681` `'Cascadia Code',Consolas`、`splash.css:14`、`CreatorMotto.vue:64`）。图标一律走 `currentColor` 文本字符或 CSS 形状。

| 字符 | 码位 | 名称 | 字体 | file:line | 语义 |
|---|---|---|---|---|---|
| `×` | U+00D7 | MULTIPLICATION SIGN | 继承 `system-ui` | `App.vue:1465` `.dl-dismiss` | 移除下载记录 |
| `×` | U+00D7 | | | `HomeView.vue:715` `.log-close`（`:974` font-size **20px**） | 关闭日志对话框 |
| `×` | U+00D7 | | | `SettingsView.vue:937` `.java-item-hide` / `:988` `.res-x` / `:1247`(GameView) | 隐藏/清除/乘号 |
| `×` | U+00D7 | | | `Toasts.vue:9` `.toast-close`(在 `.icon-btn` 内) | 关闭 Toast |
| `×` | U+00D7 | | | `ThumbnailPickerModal.vue:104` `.icon-btn` | 关闭选择器 |
| `×` | U+00D7 | | | `KeysView.vue:206` `.cfg-clear` | 设为未指定按键 |
| `×` | U+00D7 | | | `VoxLinkModSync.vue:47` `.btn` / `ModMigrationModal.vue:24` / `ModVersionModal.vue:15` | 关闭 |
| `✕` | U+2715 | MULTIPLICATION X | | `DupCleanModal.vue:118`、`InstanceCenter.vue:52 / :93 / :105` | 关闭（**另一套字形**，与 × 混用） |
| `⋯` | U+22EF | MIDLINE HORIZONTAL ELLIPSIS | | `GameView.vue:1171` `.instance-more`、`FileManager.vue:448` `.file-more>summary`、`FrpPanel.vue:147` | 更多操作（SVG 三点仅出现在 HomeView） |
| `▶` | U+25B6 | BLACK RIGHT-POINTING TRIANGLE | | `GameView.vue:1170`（`'▶ 启动'` 文本按钮） | 启动实例（与 HomeView 的 SVG 三角**不同实现**） |
| `⚠` | U+26A0 | WARNING SIGN | | `SettingsView.vue:1178 / :1211 / :1216 / :1217`、`ModDropModal.vue:257`、`RecordingModPicker.vue:35` | 警告 |
| `✓` | U+2713 | CHECK MARK | | `SettingsView.vue:1068 / :1210 / :1215`、`DupCleanModal.vue:128 / :183`、`LaunchNotice.vue:28`（`.launch-success-icon` 38×38 圆底，`font-size:var(--text-xl)`=20px）、`TerracottaPanel.vue:179` `.stage-dot`、`VoxLinkPanel.vue:388`、`MinecraftVersionPicker.vue:23` | 成功/完成（与 SVG `M20 6 9 17l-5-5` 并存） |
| `↻` | U+21BB | CLOCKWISE OPEN CIRCLE ARROW | | `FileManager.vue:343 / :405`（`'↻ 刷新'`）、`RecordingsView.vue:57`、`DefaultGameOptions.vue:63` | 刷新（与 SVG 刷新并存） |
| `＋` | U+FF0B | FULLWIDTH PLUS SIGN | | `ServersView.vue:360`（`<span aria-hidden="true">＋</span> 添加服务器`）、`FrpPanel.vue:133` `＋ 创建新隧道` | 新增 |
| `+` | U+002B | PLUS SIGN | | `HomeView.vue:634` `.provider-mark.offline`（`:935` 24×24 圆角块，`font-size:10px`，`font-weight:800`） | 添加账户 |
| `⌄` | U+2304 | DOWN ARROWHEAD | | `HomeLayoutEditor.vue:496` `content:'⌄'`（`::after`）、`EditPanel.vue:60`、`MinecraftVersionPicker.vue:22` | 折叠/展开 |
| `‹` `›` | U+2039 / U+203A | ANGLE QUOTATION MARKS | | `HomeView.vue:668`（`'‹ 返回版本选择'` / `'文件夹列表 ›'`）、`DefaultGameOptions.vue:66 / :70` | 导航方向 |
| `⧉` | U+29C9 | TWO JOINED SQUARES | | `ServerDetails.vue:15`（地址后 `{{ address }} ⧉`） | 复制 |
| `?` | U+003F | | | `HomeView.vue:630` `.account-placeholder`（`:929` 54×54 虚线框，`font-size:20px`）、`Avatar.vue:40` `.letter`（CSS `:60-67`） | 未登录占位 |
| `Y` / `O` | U+0059 / U+004F | 拉丁字母 | | `HomeView.vue:623` `{{ type==='yggdrasil' ? 'Y' : 'O' }}`（非微软/非离线时的 `.provider-mark`，24×24，渐变底 `:935/:936/:937`） | 账户类型字母标 |
| 首字母大写 | — | | | `CommunityView.vue:575` `.icon-placeholder`（CSS `:876` `font-size:var(--text-lg)`=16px）、`ServerListItem.vue:12` + `ServerDetails.vue:15` `.server-monogram`、`Avatar.vue:40` `.letter` | 头像占位字标 |
| `·` | U+00B7 | MIDDLE DOT | | `GameView.vue:2194` `content:'·'`、`CommunityView.vue:588` `.meta-dot`（CSS `:929`） | **分隔符，不是图标** |

### CSS 画的形状（既非 SVG 也非字形，端口时需用 Border/Ellipse/Rectangle）

| 元素 | file:line | 形状 |
|---|---|---|
| `.spin` 通用加载环 | `styles.css:683-691`（keyframes `:692-696`） | 14×14，`border:2px solid var(--accent-soft)` + `border-top-color: var(--accent)`，`border-radius:50%`，`spin .8s linear infinite` |
| `.switch-ui::before` 开关滑块 | `styles.css:383-394` | 14×14 圆，`var(--text-dim)`，checked 平移 18px |
| `.connection-toggle-track::after` | `connection.css:59` | 16×16 圆点 |
| `.bell-dot` | `App.vue:2121-2130` | 7×7 圆 `var(--accent)` + `border:1.5px solid var(--bg-2)` |
| `.sidebar-health i` | `App.vue:1982-1995` | 7×7 圆，三态色 `--ok`/`--accent`/`--danger` + 3px 同色 ring |
| `.runtime-state i` | `HomeView.vue:881-884` | 7×7 圆，默认 `#9ca3af`，ready/running `var(--ok)`，error `var(--danger)` |
| `.account-copy span i` | `HomeView.vue:924` | 7×7 圆 `var(--ok)` |
| `.connection-status i` | `connection.css:25` | 6×6 圆 `currentColor` |
| `.instances-head h2::before` 标题竖条 | `HomeView.vue:890` | **4×17 圆角矩形**，`background: var(--accent-grad)` |
| `.nav-sub-item::before` 子项圆点 | `App.vue:1935-1944` | **死代码：`content: none`（:1936）→ 该 5×5 圆点实际不渲染**，只有 `.active` 时 `opacity:1`（:1947-1949）也无视觉。移植时可忽略 |
| `.nav-selection` / `.nav-bubble` | `App.vue:1855-1859` 等 | `inset 3px 0 0 var(--accent)` 选中指示条 + 跟随气泡 |
| `.launch-progress` | `HomeView.vue:853` | 绝对定位覆盖层，`rgba(255,255,255,.25)`，宽度 = 进度% |
| `.runtime-blob` | `HomeView.vue:862-870` | 跟随 hover 的 10% accent 圆角浮块 |
| `.tp-custom-grad` | `SettingsView.vue:1638-1642` | `linear-gradient(135deg,#f43f5e,#f97316,#eab308,#22c55e,#3b82f6,#a855f7)` 自定义主题预览 |
| `.tp-check` 圆形底 | `SettingsView.vue:1621-1631` | 18×18 `border-radius:50%` `var(--accent)`，SVG 勾叠在上面 |
| `.tp-block/.tp-btn/.tp-top/.tp-side/.tp-dot` | `SettingsView.vue:1583-1618` | 纯 CSS 拼的迷你 UI 预览（9px 侧栏 / 4px 圆点 / 12px 按钮…） |
| `.provider-mark` 渐变底 | `HomeView.vue:935-937` | 24×24 `linear-gradient(135deg,#f35325 0 48%,#81bc06 48% 100%)`（微软/离线时用） |
| `.select` 下拉箭头 | **`styles.css:333-341`** | `background-image: url("data:image/svg+xml,…")` **12×12**，`stroke='%238f8d84'` = **硬编码 #8f8d84，不跟随主题**（唯一一个）；`background-position: right 12px center` |
| `.mc-avatar` / `.mc-avatar.letter` | `Avatar.vue:44-45`（模板 `<img>` / `.letter` div）、CSS `:55-59` / `:60-67` | 圆角方块皮肤头 或 圆形渐变首字母头像（`--accent-grad`，`font-weight:800`，`color:var(--on-accent)`），字号 = `size×0.42`（`Avatar.vue:41`）；非 SVG |
| `.fm-file-icon img` | `FileManager.vue:576` | 模组图标走 `getModIcons` 远程 PNG |
| `mobs/*.png` | `IconPickerModal.vue:71` `:src="`mobs/${m.id}.png`"` | **83 个 MC 生物头像**，`src/renderer/public/mobs/`，共 24,224 字节；网格单元 `minmax(44px,1fr)`（`IconPickerModal.vue:108`），`image-rendering:pixelated`。列表在 `mobIcons.ts` |

---

# F. 尺寸规范

## F.1 **没有**统一主 `.icon` / `.i` 类

搜索结论：全树不存在 `.icon{}` 或 `.i{}` 基础类。图标尺寸由**父容器类上的 `svg` 后代选择器**决定，共 4 层约定：

1. **全局基线（`styles.css`，非 scoped，`main.ts:5` 引入）**

```css
/* styles.css:195-197 —— 只设 shrink，不设尺寸 */
.btn svg { flex-shrink: 0; }

/* styles.css:266-301 —— 纯图标按钮：盒 32×32，图 16×16 */
.icon-btn { display:inline-flex; align-items:center; justify-content:center;
            width:32px; height:32px; border:1px solid transparent;
            border-radius:var(--radius-sm); background:transparent; color:var(--text-dim); }
.icon-btn:hover:not(:disabled) { background:var(--card-2); color:var(--accent-2); }
.icon-btn svg { width:16px; height:16px; }

/* styles.css:158-163 按钮基线：min-height:var(--ctl-h)=36px */
/* styles.css:732-735  .edit-tip svg { width:16px; height:16px; flex-shrink:0; } */
```

2. **App.vue 壳层（`App.vue:1862-2446`）**

```css
.nav-icon                              { width:20px; height:20px; flex-shrink:0; }   /* :1862 */
.nav-icon :deep(svg)                   { width:19px; height:19px; display:block; }   /* :1870 */
.nav-sub-item .nav-icon                { width:17px; height:17px; }                  /* :1950 */
.nav-sub-item .nav-icon :deep(svg)     { width:16px; height:16px; }                  /* :1954 */
.nav-parent .nav-caret                 { width:14px; height:14px; margin-left:auto; } /* :1965 */
.sidebar-health svg                    { width:14px; height:14px; }                  /* :2002 */
.top-back svg                          { width:16px; height:16px; }                  /* :2055 */
.top-btn svg                           { width:15px; height:15px; }                  /* :2094 */
.top-icon-btn svg                      { width:17px; height:17px; }                  /* :2117 */
.win-btn svg                           { width:15px; height:15px; }                  /* :2384 */
.drop-box svg                          { width:48px; height:48px; }                  /* :2442 */
.brand-head                            { width:42px; height:42px; image-rendering:pixelated; } /* :1773 */
```

3. **HomeView（`HomeView.vue:836-955`）** — 首页专属尺寸带

```css
.hero-settings svg        { width:18px; height:18px; }   /* :836 */
.hero-more svg            { width:19px; height:19px; }   /* :838 */
.launch-content svg       { width:22px; height:22px; }   /* :851 */
.launch-arrow svg         { width:22px; height:22px; }   /* :857 */
.runtime-item > svg:first-child { width:27px; height:27px; color:var(--text); }  /* :874 */
.runtime-state > svg:first-child { color:var(--accent-2); }                        /* :879 */
.runtime-chevron          { width:14px; height:14px; opacity:.7; }                 /* :878 */
.manage-instances svg     { width:15px; height:15px; }                             /* :893 */
.instance-icon            { width:36px; height:36px; }                             /* :900 */
.instance-more svg        { width:15px; height:15px; }                             /* :907 */
.instance-play svg        { width:15px; height:15px; }                             /* :912 */
.account-more svg,.skin-refresh svg { width:17px; height:17px; }                   /* :928 */
.provider-mark.microsoft svg        { width:22px; height:22px; }                   /* :932 */
.account-provider > svg             { width:15px; height:15px; }                   /* :934 */
.skin-tip svg             { width:12px; height:12px; }                             /* :955 */
@media (max-width:1180px){ .runtime-item > svg:first-child{23px} .runtime-chevron{display:none} } /* :983-995 */
```

4. **其它视图 / 连接面板**

```css
.tool-search svg          { width:14px; }            /* GameView.vue:1656 */
.back-btn svg             { width:15px; }            /* AccountsView.vue:705 */
.pane-tools .icon-btn svg { width:16px; }            /* SkinsView.vue:700 */
.preview-placeholder svg  { width:40px; opacity:.6 }  /* SkinsView.vue:743 */
.need-ms-icon             { width:52px; height:52px; color:var(--accent); opacity:.8 } /* SkinsView.vue:1064 */
.servers-page .server-search svg { width:18px; color:var(--text-dim) } /* ServersView.vue:454 */
.select-menu-chevron      { width:14px; color:var(--text-dim) }          /* SelectMenu.vue:129 */
.select-menu-option svg   { width:14px; color:var(--accent) }            /* SelectMenu.vue:164 */
.pick-icon                { width:42px; height:42px; color:var(--accent); background:var(--accent-soft) } /* FriendConnectView.vue:146 */
.pick-icon :deep(svg)     { width:22px; height:22px; }                   /* FriendConnectView.vue:152 */
.bridge-reset svg         { width:13px; }            /* BridgeView.vue:331 */
.cfg-reset svg            { width:13px; }            /* KeysView.vue:267 */
.tp-check                 { width:18px; height:18px; border-radius:50%; background:var(--accent); color:var(--on-accent); padding:3.5px } /* SettingsView.vue:1621 */
.tp-palette               { width:30px; height:30px; color:#ffffff }      /* SettingsView.vue:1644 */
.fav-btn                  { width:28px; height:28px; }  /* GameView.vue:1946 */
/* .fav-btn.on: :1964 定 color:#f5b301，但 :2201 同块后声明 color:var(--accent) → 生效的是 --accent */
.inst-icon                { width:34px; height:34px; padding:4px; }        /* GameView.vue:1751 */
.fm-file-icon             { width:32px; height:32px; color:var(--accent) } /* FileManager.vue:565 → 桌面 36px :719 → mods 32px → ≤700px 28px :720 */
.empty-icon               { width:44px; height:44px; color:var(--accent); opacity:.8 } /* FileManager.vue:621 */
.connection-symbol        { width:48px; height:48px; background:var(--accent-soft); color:var(--accent-2) } /* connection.css:22 */
.connection-symbol svg    { width:26px; height:26px; }                    /* connection.css:23 */
.connection-notice > svg  { width:19px; height:19px; color:var(--accent-2) } /* connection.css:47 —— ⚠死规则：无组件使用 .connection-notice */
.result-icon              { width:46px; height:46px; }                    /* CommunityView.vue:857 */
.result-links .icon-btn   { width:34px; height:34px; }                     /* CommunityView.vue:1080（盒变大，图仍 16px） */
.launch-success-icon      { width:38px; height:38px; border-radius:50%; font-size:var(--text-xl) } /* LaunchNotice.vue:37 —— 字形 ✓ */
.log-close                { width:32px; height:32px; font-size:20px; }     /* HomeView.vue:974 —— 字形 × */
.account-placeholder      { width:54px; height:54px; border:1px dashed; font-size:20px } /* HomeView.vue:929 —— 字形 ? */
.provider-mark            { width:24px; height:24px; border-radius:6px; font-size:10px; font-weight:800 } /* HomeView.vue:935 */
```

**实际用到的尺寸阶梯**（按出现频次）：`12 · 13 · 14 · 15 · 16 · 17 · 18 · 19 · 20 · 22 · 26 · 27 · 30 · 36 · 40 · 42 · 44 · 48 · 52` px。
无 24/28px 图标档；28px 只作为按钮**盒**尺寸（`.fav-btn`）。

## F.2 用 `width`/`height` **属性**（而非 CSS）的 20 个

`App.vue:1581` 14 · `AccountsView.vue:421` 15 · `AccountsView.vue:577` 12 · `CommunityView.vue:496` 14 · `:554` 40 · `:598` 14 · `:605` 14 · `:609` 13 · `GameView.vue:1027` 14 · `:1150` 15 · `:1192 / :1196 / :1200 / :1204 / :1208 / :1212 / :1216 / :1220` 13 · `HomeView.vue:682` 12 · `SettingsView.vue:679` 15。

> ⚠ **端口陷阱**：CSS 声明优先于 SVG 表现属性。`CommunityView.vue:598 / :605` 写了 `width="14"`，但它们在 `class="icon-btn"` 里，被 `styles.css:298` `.icon-btn svg{width:16px}` 覆盖 → **实际渲染 16px**。同理 `UpdateModal:67`、`SettingsView:1173/:1203`、`SkinsView:466` 无属性也走 16px。其余 18 处所在父类（`.btn` / `.menu-item` / `.fav-btn` / `.copy-hint` / `.empty` / `.inst-icon`）没有 `svg` 尺寸规则，属性值即最终尺寸。

---

# G. WPF 落地建议

## G.1 直接映射为 `Path` / `Geometry` 的（84 个 = 82 描边 + 8 实心 −重叠）

**描边型（82）** 一律同一个 Style + `ContentPresenter`，因为它需要 `stroke`/`stroke-width`/`round caps`/`round joins` **四件同时生效**：

```xml
<!-- 通用 LineIcon：把 SVG 的 viewBox 24 单位当作用户坐标 -->
<Style x:Key="LineIcon" TargetType="Path">
  <Setter Property="Fill" Value="{x:Null}"/>
  <Setter Property="Stroke" Value="{Binding Foreground, RelativeSource={RelativeSource AncestorType=Control}}"/>
  <Setter Property="StrokeThickness" Value="2"/>            <!-- SVG stroke-width -->
  <Setter Property="StrokeStartLineCap" Value="Round"/>      <!-- stroke-linecap="round" -->
  <Setter Property="StrokeEndLineCap" Value="Round"/>
  <Setter Property="StrokeLineJoin" Value="Round"/>          <!-- stroke-linejoin="round" -->
  <Setter Property="Stretch" Value="Uniform"/>
  <Setter Property="SnapsToDevicePixels" Value="False"/>
  <Setter Property="Width" Value="16"/>
  <Setter Property="Height" Value="16"/>
</Style>

<!-- 首页齿轮：StrokeThickness 必须显式改成 1.9，且不能带 joins（App.vue:175 录像/568 四宫格 就没有 linejoin） -->
<Path Style="{StaticResource LineIcon}" StrokeThickness="1.9" Width="18" Height="18"
      Data="M19.4,15 a1.7,1.7 0 0 0 0.34,1.88 L19.8,16.94 16.97,19.77 …（§B 的 B1 d）"/>
```

要点：
- **`stroke-width` 是 10 档不同的（2 / 2.2 / 2.6 / 3 / 1.9 / 1.8 / 1.7 / 1.6 / 1.5 / 无）**，不能全局写死。最稳的做法是每个图标一个 `Style`（BasedOn `LineIcon`）带自己的 `StrokeThickness`，或建一个 `IconKind` 枚举 + `Dictionary<IconKind,(Geometry, double sw, bool caps, bool joins, double size)>` 在 code-behind 生成。
- **`Stretch=Uniform` 时 WPF 会等比缩放 `StrokeThickness`**，与 SVG 默认 `vector-effect:uniform scale` 行为一致 → 视觉粗细天然对齐，**不需要**手工把 2 换算成 `2*16/24=1.33`。
- SVG 里的 `h.01`（如 `M6 10h.01`、`M7 7.5h.01`）配合 round linecap 就是「一个点」→ WPF 同样可行，但要保证 `StrokeStartLineCap`/`EndLineCap` 都是 Round，否则点会消失。
- 弧 `a` / `A`、相对 `m`/`l`/`h`/`v`、`Z` 都在 WPF Path mini-language 支持范围内。粘贴时建议把「无分隔的 `0 0 1 0 18`」这类弧参数写成带逗号形式（`A13.5,13.5 0 0,1 0,18`），WPF 解析器对连续标志位比浏览器严。
- **不要用 `GeometryGroup` 合并带描边的多子路径**（`M3 12h4l2-7…` 这类含多个 `M` 的 d 可以整串塞进 `Data`，WPF 会自动分 `PathFigure`，`IsClosed` 由 `Z` 决定）。

**实心型（8）** → `Fill` 型 Path，`Stroke=null`：

```xml
<Path Data="M8,5.5 L8,18.5 L19,12 Z" Fill="{StaticResource OnAccentBrush}"/>       <!-- 播放三角 -->
<Path Data="m12,2 L15.09,8.26 L22,9.27 L17,14.14 L18.18,21.02 L12,17.77 L5.82,21.02 L7,14.14 L2,9.27 L8.91,8.26 Z"
      Fill="{Binding Foreground, RelativeSource={RelativeSource AncestorType=Control}}"/> <!-- 收藏星 -->
<!-- 三点省略号：用 3 个 EllipseGeometry 比转弧更清晰 -->
<DrawingGroup>
  <GeometryDrawing Brush="{…}" Geometry="M6.8,12 A1.8,1.8 0 1 1 3.2,12 A1.8,1.8 0 1 1 6.8,12 Z"/>
  … cx=12 / cx=19
</DrawingGroup>
```

## G.2 建议做成 `DrawingImage` 资源的（**只有多色、非主题化**的）

| 图标 | 理由 |
|---|---|
| **草方块**（HomeView:584，`0 0 48 48`，5 面 5 色） | 5 个 `GeometryDrawing` 固定画刷，`DrawingImage` 直接当 `Image.Source`，且实例图标默认值可复用 |
| **微软四色徽标**（HomeView:623，`0 0 22 22`） | 4 块品牌色**绝不能**被主题染色（合规要求） |
| 微软单色四方块（AccountsView:421） | 几何同上，但 `Brush` 要能跟随 `--on-accent` → 建议单独一个 `Geometry`（`M3,3 H11.5 V11.5 H3 Z …`）而非 DrawingImage |
| 游戏手柄方向键圆点（App:114）、VoxLink 手柄点（FriendConnect:30）、调色盘点（SettingsView:662） | 「描边主体 + 实心细节」混合，`DrawingGroup` 里两组（一组 Pen、一组 Brush）最省事 |

```xml
<!-- 主题化图标不要放 DrawingImage（DrawingImage 内部画刷无法 DynamicResource 到主题色），
     改为：共享 Geometry 资源 + Path 样式 -->
<Geometry x:Key="Geo.Settings.Gear">M19.4,15 a1.65,1.65 …（A.1#7）</Geometry>
<Path Data="{StaticResource Geo.Settings.Gear}" Style="{StaticResource LineIcon}"/>
```

## G.3 必须做格式转换的

| 资产 | 动作 |
|---|---|
| `banner1/2/3.webp` | **转码**。本机 WPF 能解，但依赖 OS WIC codec（Win10 / 干净 Win11 会抛异常）。转 PNG（配 oxipng）或 JPEG q82。同时注意 `banner2` 是 **1122×1402 竖图**，`object-fit:cover` 在 430px 高横幅里会裁切掉大量画面 → WPF 用 `VisualBrush` + `Stretch="UniformToFill"` 复现 |
| `banner4.png` | 2.21 MB / 占资源总量 86.6%，无 alpha → **转 JPEG**（同视觉约 200-350 KB） |
| `splash-face.png` | 无需转换。WPF 里三处用途分别对应：侧栏 `Image` 42×42（**必须** `RenderOptions.BitmapScalingMode="NearestNeighbor"` 复现 `image-rendering:pixelated`）、启动 12×12 拼图（用 `UniformGrid` + `VisualBrush` 的 `Viewbox`/`RelativeTransform` 切块）、原生闪屏（GDI+/Win32） |
| `splash-face-32.png` | 源项目**零引用**。要么当应用图标/托盘图标（32×32 正好合适），要么丢弃；别当必需资源 |
| `public/mobs/*.png`（83 个，24 KB 总） | 不在本次 6 文件范围内，但 `IconPickerModal`/`FileManager`/`HomeView`/`GameView` 的实例图标依赖它们 → 若要移植「MC 生物头像」选择器需一并搬，全部 `NearestNeighbor` 渲染 |

## G.4 需要换成 WPF 原生绘制的（CSS 画的形状）

`.spin` → `Arc`/`Ellipse` + `RotateTransform` 动画（14×14、2px 环、缺口用 `.5` 透明弧）；`.switch-ui::before` → `Thumb` 模板 14×14 圆；`.bell-dot` / `.sidebar-health i` / `.runtime-state i` / `.account-copy span i` / `.connection-status i` → 6~7×7 `Ellipse` + `StrokeThickness=1.5` 或 `Effect`；`.instances-head h2::before` → 4×17 `Rectangle` + `LinearGradientBrush`（`--accent-grad` 见 `styles.css:21`，三段 135°）；`.nav-selection` → 3px 左侧 `Border`；`.tp-*` 主题预览 → 全用 `Grid`+`Border`，别硬画 SVG；`--accent-soft` 一类的 `color-mix(..., transparent)` 在 WPF 要预先算成 ARGB 常量（`gen_themes.py` 已在做这件事）。

## G.5 字形图标（E 节 16 类）在 WPF 里的选择

`×`(U+00D7) / `✕`(U+2715) / `⋯`(U+22EF) / `▶`(U+25B6) / `⚠`(U+26A0) / `✓`(U+2713) / `↻`(U+21BB) / `＋`(U+FF0B) / `⌄`(U+2304) / `‹›`(U+2039/203A) / `⧉`(U+29C9) / `?` `+` `Y` `O` —— 全部是**纯文本字符**，字体 = 系统默认（`Segoe UI`）。WPF 里两种走法：

1. **保真优先**：直接用 `<TextBlock Text="×"/>` + `FontFamily="Segoe UI"`，与源版逐像素接近（源版本来就是这么画的）。
2. **一致性优先（推荐）**：把它们统一收编成 SVG Path —— 源项目里同一语义有 **两套**实现（例如「关闭」= SVG `M18 6 6 18…` 在 5 处、字形 `×` 在 8 处、字形 `✕` 在 4 处；「刷新」= SVG 在 4 处、`↻` 在 4 处；「勾选」= SVG 在 3 处、`✓` 在 8 处；「更多」= SVG 三点在 HomeView、`⋯` 在 GameView/FileManager/FrpPanel）。移植时**收敛到 SVG/Path 一套**，可顺手修掉这个不一致。
3. **不要**引入 Segoe Fluent Icons / MDL2 —— 原项目没用到，引入会让视觉与主题体系（`--accent` 描边 + `round cap`）不一致。

## G.6 移植时的三个具体坑

1. **`currentColor` 有 97 处**，等价物是 WPF 的 `TextElement.Foreground` 继承。必须建一个 `Icon` 控件（或 `Path` Style + `RelativeSource AncestorType=ContentControl`），否则每个图标都要手写绑定。
2. **`CommunityView.vue:598/:605` 的 `width="14"` 是无效声明**（被 `.icon-btn svg{16px}` 覆盖）。照抄属性值会让图标比原版小 2px；以 **16px** 为准。
3. **`styles.css:338` 的下拉箭头 `#8f8d84` 是硬编码色**（唯一一个不跟随主题的 SVG）。6 套主题的浅色版上它对比度不足 —— 移植时改成 `--text-dim` 是修 bug，但会**偏离原样**，需要决策。

---

## 附：核对方式

- `92` 与 `63` 由脚本遍历 `**/*.vue` 的每个 `<svg` 起始行、闭合到 `</svg>`、按 (viewBox, 图元类型序列, d 序列, 图元属性) 归并得出。
- 光栅尺寸由直接解析 PNG `IHDR`（宽/高/位深/颜色类型）与 WebP `RIFF`（`VP8 `/`VP8L`/`VP8X`）字节头得到，并经 WPF `BitmapImage`（.NET `PresentationCore`）二次实测解码交叉验证，两者一致。
- 字形码位由 `unicodedata.name()` 对每个含图标性字符的行枚举打印，未做人工推断。
