# UI 几何台账 · 外壳与首页

来源：`KAMUCL-1.1.5/src/renderer/src/`。**本文所有数值由我本人在源码中逐条复核后写入**（并行子代理给出的初稿里有 4 处错误已纠正，见文末"勘误"）。

取值原则：HomeView 的 `<style scoped>` 尾部有一批压缩规则（:1030 起）**写在所有 `@media` 之后**，同特异度后者胜，所以 `:782-1028` 的大部分值是死代码，只有被尾部引用的基础属性（如 `border-radius`、`font-weight`）仍然生效。本文一律记录**最终生效值**，并注明来自哪一段。

## 1. 窗口骨架

| 项 | 值 | 证据 |
|---|---|---|
| 默认尺寸 | 1360 × 860 | `src/main/windowAppearance.ts`（return 块 width/height）|
| 最小尺寸 | 960 × 620 | 同上 `minWidth/minHeight` |
| Windows 无边框 | `frame:false`（仅 darwin 有框）+ `backgroundMaterial:'acrylic'` + `roundedCorners:true` | 同上 |
| 外壳圆角 | `--radius-md` = **10px** | App.vue:1733 + styles.css:59 |
| 外壳描边 | 1px `color-mix(border 80%, transparent)` | App.vue:1732 |
| 外壳内高光 | `inset 0 1px 0 rgba(255,255,255,.12)` | App.vue:1734 |
| 无 backdrop 时兜底 | `.shell` 底色 = `color-mix(bg 78%, transparent)` | App.vue:1749-1752 |

结构：`.shell(flex 行)` → `.sidebar(208)` + `.main-area(flex 列)` → `.topbar(72)` + `.content`。

## 2. 侧栏（208px 定宽，无拖拽调宽手柄）

`--sidebar-w` 由主题应用器**硬写 208px，所有主题一致**（App.vue:1009）——不是可按主题变化的量。

| 元素 | 最终值 | 证据 |
|---|---|---|
| `.sidebar` | width 208，flex-shrink 0，列流；底色 `color-mix(bg-2 92%, transparent)`；border-right 1px `--border` | App.vue:1755-1762 |
| `.logo-area` | height **88px**，padding `0 16px`，gap `--space-3`=12 | App.vue:1764-1770 |
| `.brand-head` | **42×42**，radius 10，border `1px rgba(255,255,255,.26)`，shadow `0 3px 12px rgba(0,0,0,.18)`，`image-rendering:pixelated` | App.vue:1772-1780 |
| `.logo-text` | 列流 gap 4 | App.vue:1781-1785 |
| `.logo-name` | **20px**（`--text-xl`）/700，**letter-spacing 2px**，line-height 1.2，字体 `Segoe UI Variable Display` | App.vue:1786-1793 |
| `.logo-version` | `align-self:flex-end`，padding-right 2，**12px**，`--accent-2`，opacity .85 | App.vue:1794-1800 |
| `.nav` | flex:1，列流，gap **4px**，padding `4px 12px 12px`，overflow-y auto，position relative | App.vue:1802-1810 |
| `.nav-item` | height **48px**（`--space-7`），padding `0 16px`，gap **12px**，border-radius **10px**，底色透明，**14px**（`--text-md`）| App.vue:1812-1827 |
| `.nav-item:hover` | 底色仍透明，仅 color→`--text` | App.vue:1829-1832 |
| `.nav-item.active` | 底色透明，color = `color-mix(text 84%, accent)` | App.vue:1833-1836 |
| `.nav-bubble` | absolute，radius 10，背景 `linear-gradient(135deg, accent-soft 80%→transparent, --hover)`，shadow `0 3px 12px #00000008`；**left/top/width/height 由 JS 内联**，transition 300ms `cubic-bezier(.22,1,.36,1)` | App.vue:1837-1849 |
| `.nav-selection` | absolute，radius 10，**`box-shadow: inset 3px 0 0 --accent`**（左侧 3px 竖条），z-index 2，几何同样由 JS 写 | App.vue:1850-1858 |
| `.nav-icon` | **20×20** flex 居中；内部 `svg` **19×19** | App.vue:1861-1871 |
| `.nav-label` | 500 → active 600 | App.vue:1875-1880 |
| `.nav-caret` | 14×14，`margin-left:auto`，opacity .7，transform .18s | App.vue:1884-1891 |
| `.nav-sub` | `grid-template-rows:0fr→1fr`（展开时 `margin:4px 0`），内部 gap 4 | App.vue:1896-1914 |
| `.nav-sub-item` | height **40px**，**padding-left 32px**（`--space-6`），**13px**；`::before` 的 5px 圆点 **content:none（不画）** | App.vue:1929-1943 |
| `.sidebar-health` | grid **`8px minmax(0,1fr) 14px`**，gap 8，min-height **42px**（`--row-h`），margin 12，padding `0 12px`，border 1px，radius 10，底色 `color-mix(card 48%, transparent)`，**12px**/500，`-webkit-app-region:no-drag` | App.vue:1959-1977 |
| `.sidebar-health i` | **7×7** 圆点，`--ok` + `0 0 0 3px --ok-soft` 光环；busy→accent/accent-soft，error→danger/danger-soft | App.vue:1982-1996 |

导航项实际内容（App.vue:105-148 + :158-176）：首页 / 游戏版本 / **资源管理(父，可折叠)** / 皮肤 / 社区资源 / 设置，资源管理展开后插入 模组·资源包·光影包 三个子项（父项插在"游戏版本"之后，子项在其下）。

## 3. 顶栏 72px（唯一拖拽区）

| 项 | 值 | 证据 |
|---|---|---|
| `.topbar` | height **72px**，padding `0 24px`，gap 16，border-bottom 1px，底色 `color-mix(bg-2 78%, transparent)`，**`-webkit-app-region:drag`** | App.vue:2016-2027 |
| `@max-1080` | topbar 68px，content padding 16，**logo-area 反而变 92px** | App.vue:2406-2416 |
| `.top-back` | **36×36**（`--ctl-h`），radius 6，border `1px color-mix(border 70%, transparent)`，底色 `--card-2`，svg 16×16，no-drag | App.vue:2033-2058 |
| `.topbar-spacer` | `flex:1`，`align-self:stretch`，内容是 **CreatorMotto 跑马灯**（不是页面标题！） | App.vue:2059 + 模板 :1322 |
| `.top-actions` | flex gap **4px**，no-drag | App.vue:2068-2073 |
| `.top-btn` | height 36，padding `0 12px`，gap 8，radius 10，透明底，**13px**，svg **15×15** | App.vue:2074-2097 |
| `.win-btn` | **38×36**，radius 10，透明底，svg 15×15；`.win-close:hover` = danger 底 + 白字 | App.vue:2371-2390 |
| `@max-1050` | `.top-btn` padding-inline 6，gap 4，跑马灯隐藏 | App.vue:2062-2067 |

顶栏右侧按钮次序：下载(文字按钮，带 `dl-badge` 计数) → 下载中心(纯图标) → 更新日志 → 通知(带 `bell-dot`) → 分隔线 → 最小化 / 最大化还原 / 关闭。

## 4. 内容区

`.content` flex:1，min-height:0，overflow-y auto，**`scrollbar-gutter:stable`**，padding **24px**（`--space-5`）— App.vue:2398-2404；滚动条槽 8px（styles.css:139-142）。

## 5. 首页（HomeView）

### 5.1 版面

```
.home-dashboard  grid: minmax(0,1fr) minmax(250px,290px)   gap:16
                 max-width:1160  margin:0 auto
                 min-height:calc(100vh - 132px)   ← 132 = topbar72 + content 48
@max-1100 → 单列，.home-side 变 2 列网格（账号 r1c1 / 说明卡 r2c1 / 皮肤 r1-3c2）
```
证据：HomeView.vue:1030（尾部覆盖）与 :1033；基础段 :782-791 的 `286px/14px` 已被取代。

左列 `.home-main`：flex 列，**gap 14px**（:792-797，尾部未覆盖）→ 三块：hero-card / runtime-strip / instances-block。
右列 `.home-side`：flex 列，**gap 16px**，`align-self:start`（:1030 + :1032）。

### 5.2 hero-card（英雄横幅）

| 项 | 最终值 | 证据 |
|---|---|---|
| 高度 | `--banner-h`(430) 被 **max-height:360** 夹住 → 实高 **360px**；min-height 320 | HomeView:800-802 + :1030 |
| 圆角/边框 | **radius 20px**，border `1px color-mix(border 95%, white 4%)` | HomeView:804-806 |
| 底色/阴影 | 底 `#17231f`，shadow `0 18px 44px rgba(0,0,0,.22)` | HomeView:807-808 |
| 轮播图 | `.hero-image` absolute inset:0 铺满，`object-position:center 42%`，淡入淡出 `--motion-carousel` | :810 + :1032 |
| 遮罩 | 尾部覆盖为**竖向**：`linear-gradient(180deg, rgba(0,0,0,.16), rgba(0,0,0,.18) 30%, rgba(0,0,0,.68))` | :1030 |
| 内容层 | `.hero-content` flex 列，**padding 24px**，`justify-content:space-between`，`align-items:flex-start` | :817 + :1030 |
| kicker「当前版本」 | 尾部和 :818：min-height 34→**被 padding `5px 10px` 取代**，radius **7px**，border `1px rgba(255,255,255,.13)`，底 `rgba(9,13,14,.48)`，blur(12px)，**12px**/650，`align-self:flex-start` | :818 + :1030 |
| 标题 h1 | **`clamp(24px,2.6vw,32px)`**，line-height **1.25**，2 行截断，weight **850**，letter-spacing **-1px**，`#fff`，text-shadow `0 4px 24px rgba(0,0,0,.32)`，margin-top 18 | :819 + :1030 |
| 长名（>16 字） | 尾部把 `.long-name` 与普通态合并为同一 clamp，不再有 46px 大字 | :1030 |
| 元信息槽 | `min-height:0`（基础段的 130px 作废） | :821 + :1030 |
| edition 行 | gap `8px 12px`→尾部 **gap 10**，margin-top **8px**，17px/650 | :829 + :1030 |
| 版本号 | `--hero-game-version` **15px** 白色 | :830 + :1030 |
| loader 徽章 | padding `5px 11px`，radius 999，border `1px color-mix(accent-2 34%, transparent)`，底 `color-mix(accent 30%, rgba(20,30,24,.46))`，色 `#f6fff8`，**11px**/650 | :831 |
| 动作行 | `.hero-actions` flex，width 100%，**margin-top:auto**，`align-items:flex-end`，`justify-content:space-between`，gap **12px**，flex-wrap | :832 + :1030 |
| 版本设置按钮 | height **52px**，min-width 150，padding `0 18px`，gap 10，radius **`10px 0 0 10px`**，border `1px rgba(255,255,255,.14)`，底 `rgba(10,16,17,.58)`，blur(13px)，14px/650，svg 18×18 | :834-836 |
| ⋯ 按钮 | **52×52**，border-left 0，radius `0 10px 10px 0`，svg 19×19（fill currentColor 三圆点 r1.8） | :834,:837-838 |
| 启动组合 | `.launch-combo` min-width **310**，height **76**，radius **14**，overflow hidden；阴影 `0 12px 32px color-mix(accent 38%, transparent)` + `inset 0 2px 0 color-mix(white 14%, transparent)`；hover `translateY(-2px)`，active `scale(.99)` | :833,:840-844 |
| 主按钮 | `.launch-main` min-height **56**，min-width **180**，padding `0 24px`，**21px/800**，letter-spacing **0.5px**，底 `--accent-grad`，字 `--on-accent` | :845 + :1030 |
| 按钮内容 | gap 12，play 三角 svg **22×22**（`M8 5.5v13l11-6.5Z`，fill） | :847-849 |
| 进度覆盖 | `.launch-progress` absolute 左侧起，宽 = percent%，底 `rgba(255,255,255,.25)`，transition width .25s | :850 |
| 下拉箭头钮 | `.launch-arrow` width **48px**，border-left `1px rgba(255,255,255,.22)`，svg 22×22，`.open` rotate(180°) .18s | :851-853 + :1030 |

### 5.3 runtime-strip（三格运行状态条）

| 项 | 最终值 | 证据 |
|---|---|---|
| 容器 | grid **`1.12fr 0.95fr 0.92fr`**，min-height **68px**（基础 82 被尾部覆盖），**padding 8px**，radius **15px**，border 1px，底色 `--surface-content`（:769 `!important`），shadow `--shadow`，blur(18px) saturate(130%) | HomeView:953 + :1030 |
| 浮块 blob | absolute top/bottom 0，radius **12px**，`margin:4px 0`，底 `color-mix(accent 10%, transparent)`，opacity 0→1 + scale .97→1；left/width 由 JS | :955-963 |
| 单元 | grid **`34px minmax(0,1fr) 15px`**，gap 11，**padding `10px 12px`**（尾部覆盖基础的 `0 18px`），左对齐，cursor pointer | :964 + :1030 |
| 单元分隔 | `.runtime-item + .runtime-item { border-left:1px var(--border) }` | :965 |
| 图标 | 首 svg **27×27**；chevron **14×14** opacity .7 `--text-dim` | :966-967,:971 |
| 文案 | `small` **12px** `--text-dim`；`strong` **13px**/650 单行截断；列 gap 4 | :968-970 |
| 状态点 | `.runtime-state i` **7×7** 圆，默认 `#9ca3af`；ready/running → `--ok` + `0 0 0 3px --ok-soft`；error → danger；文字色 accent-2 / danger | :972-975 + :880-884 |
| `@max-1180` | 单元变 `28px minmax(0,1fr)`，图标 23×23，chevron 隐藏 | :987-989 |
| `@max-1010` | 单元 padding `0 9px` gap 8，strong 11px | :998-999 |

三格内容（模板 :551-560）：运行环境（javaText）/ 内存分配（memoryText）/ 运行状态（点击**打开日志弹窗**，不是首页卡片）。

### 5.4 instances-block（最近游戏）

| 项 | 最终值 | 证据 |
|---|---|---|
| 容器 | 底 `--surface-content`，radius **14px**（`--radius-lg`），padding **16px**，margin-top 14 | :1030 + :886 |
| 头部 | flex space-between，margin-bottom **8px** | :887 + :1030 |
| 标题 h2 | **17px**/750，gap 9，**`::before` 4×17px radius 2 竖条 `--accent-grad`** | :888-891 |
| 管理实例按钮 | min-height **34px**，padding `0 12px`，gap 7，radius **9px**，border 1px，底 `--card-2`，12px/550，svg 15×15 | :893-896 |
| 列表 | 尾部改成 **flex 列，gap 0**（基础 4 列网格作废） | :1030 |
| 行 | grid **`36px minmax(0,1fr) auto 36px 40px`**，gap **12**，align-center，min-height **72px**，padding 12，**border:0 + border-bottom 1px**，radius 0，无阴影，透明底 | :1030 |
| 行状态 | hover → `--hover`；selected → `--accent-soft` | :1030 |
| 图标 | 36×36（有自定义图标时 `object-fit:contain; image-rendering:pixelated`） | :1030 + :911-912 |
| 名称 | 13px/1.4/650，2 行截断；副行 11px `--text-dim` 单行截断 | :914-915 + :1030 |
| 上次游玩 | grid 第 3 列，**12px** 不换行，`padding-right` 由列宽控制 | :1030 |
| ⋯ | 第 4 列（基础 20×20 radius 7，svg 15×15） | :1030 + :917-921 |
| 启动钮 | 第 5 列，**40×34**，radius 8，底 `color-mix(accent 18%, card-2)`，色 `--accent-2`，svg 15×15；hover → accent 底 + `--on-accent` | :1030 + :923-926 |
| 空态 | `.empty-instances` min-height **110px**，**dashed 1px `--border-strong`**，radius **13px**，底 `--card`，整块可点击 | :928 |
| `@max-700` | 行变 `28px minmax(0,1fr) 32px 36px`（4 列），上次游玩落到第 2 行 | :1033 |

### 5.5 右列三张卡

`.account-panel` + `.skin-panel` 用**同一副外框**拼成一张连体卡（:1030）：
- `.account-panel`：radius `14 14 0 0`，**border-bottom:0**，无阴影，padding **16px**，min-height 146（基础 :936）
- `.skin-panel`：radius `0 0 14 14`，**margin-top -16px**，border-top 0，无阴影，padding 16px，flex:none min-height:0
- `.home-creator`：margin-top 0，无阴影，border-color `color-mix(accent 26%, border)`（:1030 + :932）

| 元素 | 值 | 证据 |
|---|---|---|
| `.account-head` | flex gap **10**（尾部覆盖基础 13） | :938 + :1030 |
| 头像 | `<Avatar :size="54">` → **54×54**，radius **12px**，外环 `0 0 0 4px color-mix(text 8%, transparent)` | 模板 :688 + :939 |
| 名字 | **16px** 单行截断 | :941 |
| 状态行 | **11px** `--accent-2`，gap 7，前缀 `i` **7×7** 圆点 `--ok` + 3px 光环 | :942-943 |
| 更多钮 | **30×30**，radius 8，svg 17×17 | :946-949 |
| 未登录占位 | **54×54** dashed radius 12，字 20px | :950 |
| 账号类型行 | grid **`26px minmax(0,1fr) 15px`**，gap 10，min-height **44px**，margin-top 15，padding `0 11px`，radius 9，底 `--card-2`，12px/550，尾部 svg 15×15 | :951-955 |
| provider 方块 | **24×24** radius 6；微软＝4 色 SVG 22×22（`#f25022 #7fba00 #00a4ef #ffb900`），否则渐变底 + 10px/800 字母（Y/O/+）；yggdrasil 渐变 `#65bd78→#268e54`，offline = `--card` 底 + `--text-dim` | :956-959 + 模板 :701 |
| 皮肤头 | flex align-start space-between，padding `0 2px 8px`；h3 **14px**/700，副标 **10px** `--text-dim`（内层 gap 3） | :962-966 |
| 皮肤舞台 | height **230px**（尾部 :1032 + `.home-side .skin-stage{height:230px;min-height:0}`，覆盖 :1030 的 260 与基础 300），radius **12px**，border `1px color-mix(border 78%, transparent)`，背景 = 径向(accent 13% @ 50% 82%→38%) + 竖向渐变 | :967 + :1030/:1032 |
| 3D 视口 | `.viewer3d` height 100%，`--sv3d-height:100%` | :968 |
| 覆盖条 | absolute 左12 右12 底12，min-height **34px**，padding `7px 10px`，gap 9，radius 9，底 `color-mix(card 78%, transparent)`，blur(12px)，11px/550 | :969-971 |
| 底部提示 | margin-top 8 / margin-bottom 0，**10px**/500 `--text-dim`，svg 12×12，hover→`--accent-2` | :973-975 + :1032 |
| CreatorCard | padding `--card-pad`=20，radius 14，border 1px，底 `--surface-content`；标题 14px/600 + small 11px letter-spacing 1px；正文 12px/1.7；名字气泡 12px radius 999 padding `5px 9px` gap 8，容器 `0fr→1fr` 展开，max-height 240 | CreatorCard.vue:18-32 |

### 5.6 浮层与日志弹窗

| 项 | 值 | 证据 |
|---|---|---|
| `.menu-overlay` | fixed inset:0，z **8000** | HomeView:977 |
| `.float-menu` | fixed，z **8001**，max-height 280，padding 6，radius 10，底 `color-mix(card 94%)`，blur(24px)，shadow `--shadow-lg`；实例菜单宽度 = 触发按钮宽度（JS inline width） | :978 + 模板 :666 |
| `.card-float-menu` | width **232px**，max-height 340，底 `--card-solid`（兜 `#192225`） | :979 |
| `.menu-item` | min-height **34px**，padding `0 10px`，gap 8，radius 7，12px/500；card-float 内 **40px/13px**；hover/active → accent-soft + accent-2；danger → `--danger` | :981-984,:980 |
| 收藏星标 | svg 12×12 fill currentColor；否则 `menu-spacer` 12px 占位 | 模板 :690-691 |
| `.log-mask` | fixed inset:0，z **9100**，padding 24，底 `--mask`，blur(5px) | :986 |
| `.log-dialog` | width `min(760px, 100vw-64px)`，max-height `min(620px, 100vh-80px)`，radius **16px**，底 `color-mix(card 94%)`，shadow-lg | :987 |
| 弹窗头 | padding `16px 18px`，border-bottom 1px，h3 **16px**，副标 11px | :988-991 |
| 关闭钮 | **32×32** radius 8，字 20px | :992-993 |
| 日志体 | min-height **220px**，padding `14px 18px`，底 `color-mix(bg 42%)`，`pre` 字体 **11.5px/1.65** `Cascadia Code, Consolas, monospace`，pre-wrap + break-all | :996-997 |
| 空日志 | padding `70px 0` 居中 | :998 |
| 页脚 | padding `11px 16px`，border-top 1px，右对齐 | :999 |

## 6. 令牌（几何类）

`--space-1..7` = **4/8/12/16/24/32/48**（styles.css:45-51）
`--text-xs..2xl` = **12/13/14/16/20/26**（styles.css:52-57）；`--text-title` = **24px**（ui-system.css:6）
`--radius-sm/md/lg` = **6/10/14**（styles.css:58-60），`--radius` = **14**（styles.css:38，主题器再硬写一次 App.vue:1011）
`--ctl-h` = **36px**，`--row-h` = **42px**，`--card-pad` = **20px**，`--card-gap` = **16px**，`--sec-gap` = **28px**（styles.css:66-70）
`--sidebar-w` **208px**，`--banner-h` **430px**（styles.css:72-73；App.vue:1009-1010 对全部主题硬写同值）
`--shadow` = `0 8px 24px rgba(0,0,0,.22)` 暗色基线（styles.css:26）→ 主题器按明暗改写：亮色 `0 2px 8px rgba(31,50,85,.045)`，暗色 `0 2px 8px rgba(0,0,0,.16)`（App.vue:1001-1002）
`--shadow-lg` = 亮 `0 20px 55px rgba(31,50,85,.18)` / 暗 `0 20px 55px rgba(0,0,0,.42)`（App.vue:1003-1004）
`--hover` 暗 `rgba(255,255,255,.07)` / 更暗段 `.05`（styles.css:28,:97）
`--motion-fast` **130ms**、`--motion-normal` **190ms**、`--motion-enter` 220ms、`--ease-out` `cubic-bezier(.2,.7,.3,1)`（styles.css:62-65，**ui-system.css:73 覆盖前两者**）
`--bn-text` = `#ffffff`（styles.css:75,:108 — 横幅文字色不随主题反转）

## 7. WPF 落地映射

| CSS 概念 | WPF 做法 |
|---|---|
| `--sidebar-w 208px` | `<ColumnDefinition Width="208"/>` |
| `grid: minmax(0,1fr) minmax(250px,290px)` | 右列不能用纯 fr：用 `Width="270"` + 代码按窗口宽度在 250/290 间夹取，或 `<Grid Grid.Column="1" MinWidth="250" MaxWidth="290">` + `Width="*"` 的容器策略；**最小窗口 960 下左列约 620**，可先按固定 286→250 阶梯实现 |
| `1.12fr 0.95fr 0.92fr` | `ColumnDefinition Width="1.12*" / "0.95*" / "0.92*"` |
| `36px minmax(0,1fr) auto 36px 40px` | `36 / * / Auto / 36 / 40`，`MinWidth="0"` 需靠 `TextTrimming` + 外层不放大 |
| `-webkit-app-region: drag` | `WindowChrome.CaptionHeight=72`，交互控件上 `WindowChrome.IsHitTestVisibleInChrome=True` |
| `backdrop-filter: blur(18px) saturate(130%)` | WPF 无等价物。用**半透明实色 + DWM 窗口透明**近似；黑紫主题本就接近不透明（`--shell-surface` 96%），其余主题按 `@supports not` 兜底色出实色 |
| `color-mix(in srgb, A p%, B)` | 已在 gen_themes.py 预计算成 ARGB，禁止运行时混色 |
| `clamp(24px,2.6vw,32px)` | 无流体排版：写 1 个 IValueConverter（视口宽 × 2.6% 夹在 24–32）或直接定 32px + 2 行截断 |
| `letter-spacing: -1px / 2px / 0.5px` | WPF `TextBlock` 无该属性：用 `Typography`／`TextBlock` + `Span` 无法做，**接受近似**（标题靠 850 字重撑住）或自绘 `FormattedText` |
| `box-shadow: inset 3px 0 0 accent`（导航选中条） | 选中项左侧叠一个 `Border Width="3"`，或用 `Thumb`/Adorner |
| `.nav-bubble` / `.runtime-blob`（JS 测位滑动） | 手写 Adorner + `TranslateTransform` 动画，duration 300ms/190ms，`CubicBezier(0.22,1,0.36,1)` / `EaseOutDouble`；WPF 没有 `cubic-bezier()`，用 `KeySpline` |
| `scrollbar-gutter: stable` | 滚动条常驻：`VerticalScrollBarVisibility="Visible"` |
| `-webkit-line-clamp: 2` | `TextBlock TextTrimming="CharacterEllipsis" MaxHeight="{2 行}"` |
| `grid-template-rows:0fr→1fr`（折叠动画） | 折叠面板用 `MaxHeight` + `DoubleAnimation`，或 `Expander` 自定义模板 |
| `image-rendering: pixelated` | `RenderOptions.BitmapScalingMode="NearestNeighbor"` |
| `object-position: center 42%` | `ImageBrush AlignmentY="0.42"`（0..1 归一）+ `Stretch="UniformToFill"` |
| `.hero-image` 淡入淡出轮播 | 多 `Image` 叠在 Grid，`Opacity` 动画，`DispatcherTimer` 切换 |
| `position:absolute inset:0` | `Grid` 子元素不占列即可；或 `Canvas` |

## 8. 勘误（子代理初稿里被我推翻的 4 处）

| 初稿 | 实际 | 说明 |
|---|---|---|
| strip `min-height:68` 与 `82` 混用 | 最终 **68**（:1030 尾部覆盖 :953 的 82）| 基础段 82 是死值 |
| 启动箭头钮 **48px** 与 **62px** | 基础 `.launch-arrow width:62px`（:852）被尾部 **:48px** 覆盖 → 用 48 | 两处都要知道，取 48 |
| hero 圆角 20 / padding 24 说成"内边距 24px 的卡片" | hero 是**图片铺满 + 遮罩 + 内容层**三层结构，不是纯色卡 | 结构层级不同 |
| 首页有"运行日志卡片" | **没有**。日志是 `z:9100` 的模态弹窗，入口是第 3 格运行状态 | 我此前在 WPF 里画了张假的日志卡 |
| `--motion-fast 120ms` | **130ms**（ui-system.css:73 覆盖） | |
