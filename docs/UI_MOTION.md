# UI_MOTION —— KAMUCL 1.1.5 动效/过渡全量移植清单

> 源：`D:/桌面/WUHUICL/KAMUCL-1.1.5/src/renderer/`（只读）。所有数值逐字摘录，未找到的一律标注 `未找到`。
> 统计：`@keyframes` **15 个**（其中 2 个已被后续规则禁用）；`transition:` 声明 **124 条**，其中 **12 条是 `transition:none` 抑制规则**，去重后 **91 个不同声明串**，归一化（`.18s`↔`0.18s` + 令牌展开）后 **≈85 组实际配方**。

## 0. 加载顺序与优先级（决定谁赢）

`src/renderer/src/main.ts:4-5`：
```
import './styles.css'
import './ui-system.css'
```
→ **ui-system.css 在 styles.css 之后**。同为 `:root`（特异度 0,1,0）时后者胜出。
Vue SFC `<style scoped>` 编译后每条选择器附加 `[data-v-xxx]`（+0,1,0），且注入 `<head>` 更晚，所以 **SFC 规则 > 全局 CSS**（除非全局 CSS 用了 `!important`）。

---

## 1. 动效令牌表（含后发覆盖）

| 令牌 | 首次定义 (file:line) | 值 | 后续覆盖 (file:line) | 最终生效值 | 备注 |
|---|---|---|---|---|---|
| `--motion-fast` | styles.css:62 | `120ms` | **ui-system.css:73** `:root{--motion-fast:130ms;--motion-normal:190ms}` | **130ms** | 后者源序胜出（同特异度） |
| `--motion-normal` | styles.css:63 | `180ms` | **ui-system.css:73** | **190ms** | 同上 |
| `--motion-enter` | styles.css:64 | `220ms` | 未找到 | `220ms` | 仅入场动画使用 |
| `--motion-carousel` | ui-system.css:7（`:root` 内，唯一来源） | `500ms` | 未找到 | `500ms` | styles.css **没有**定义它 |
| `--ease-out` | styles.css:65 | `cubic-bezier(0.2, 0.7, 0.3, 1)` | 未找到 | 同 | 全局唯一减速曲线 |
| `--shadow` | styles.css:26 | `0 8px 24px rgba(0,0,0,.22)` | **App.vue:1011 JS** `st.setProperty('--shadow', dark?'0 2px 8px rgba(0,0,0,0.16)':'0 2px 8px rgba(31,50,85,0.045)')` | JS 值（内联，特异度最高） | 主题切换时改变 → 所有 hover 阴影目标色动态 |
| `--shadow-lg` | styles.css:27 | `0 24px 70px rgba(0,0,0,.46)` | **App.vue:1012 JS** | JS 值 | 同上 |
| `--mask` | styles.css:37 | `rgba(32,42,66,0.4)` | **App.vue:1004 JS** | `dark? rgba(2,5,6,.72) : rgba(27,36,55,.42)` | 模态遮罩动画的底色 |
| `--banner-h` | styles.css:73 | `430px` | **App.vue:1021 JS** `--banner-h','430px'` | `430px` | hero 高度，轮播裁切基准 |
| 其它 `--motion-*` / `--ease-*` | — | — | — | — | **未找到**任何按主题（`[data-theme=...]`）或 JS `setProperty` 覆盖 motion 令牌的代码。全库检索确认只有 styles.css:62-65 与 ui-system.css:7,73 两处 |

`ui-system.css:53` 全局暂停开关（非令牌但决定动画是否推进）：
```
html[data-visibility="hidden"] * { animation-play-state:paused !important; }
```
由 `App.vue:201` `document.documentElement.dataset.visibility = document.hidden?'hidden':'visible'` 驱动。

`App.vue:200` 运动偏好开关：
```
watch(reducedMotion, value => { document.documentElement.dataset.motion = value ? 'reduced' : 'full' }, { immediate: true })
```
`reducedMotion = systemPrefersReduce || store.settings.reduceMotion === true`（`App.vue:197-199`），
`motionReduced(value, system) => value === true || system`（`shared/settingsCatalog.ts:42-44`）——**用户设置只能开、不能关**（系统 reduce 时恒为 reduce）。

---

## 2. `@keyframes` 全量清单（15 个）

| # | 名称 | 关键帧体（逐字） | 使用选择器 → animation 简写（时长/缓动/填充） | 延迟/错落 | file:line |
|---|---|---|---|---|---|
| 1 | `mask-in` | `from { opacity: 0; }`（无 `to`，即 1） | `.modal-mask` → `animation: mask-in 0.2s ease` | 无 | 体 styles.css:564-568；用 styles.css:537 |
| 2 | `modal-in` | `from { opacity: 0; transform: translateY(6px) scale(0.99); }` | `.modal` → `animation: modal-in var(--motion-enter) var(--ease-out)` = 220ms / cubic-bezier(.2,.7,.3,1) | 无 | 体 styles.css:569-574；用 styles.css:554 |
| 2b | 同上 | 同上 | `.float-menu` → `animation: modal-in 0.16s ease` | 无 | styles.css:595 |
| 2c | 同上 | 同上 | `.update-dialog` → `animation: modal-in var(--motion-enter) var(--ease-out)` | 无 | UpdateDialogShell.vue:34 |
| 3 | `spin` | `to { transform: rotate(360deg); }` | `.spin` → `animation: spin 0.8s linear infinite` | 无 | 体 styles.css:692-696；用 styles.css:690 |
| 3b | 同上 | 同上 | `.skin-refresh .spinning` → `animation: spin 0.8s linear infinite` | 无 | HomeView.vue:945 |
| 4 | `dl-indeterminate` | `from { transform: translateX(-120%); }` / `to { transform: translateX(320%); }` | `.dl-bar.is-indeterminate .dl-bar-fill` → `animation: dl-indeterminate 1.25s ease-in-out infinite` | 无 | 体 App.vue:2293-2300；用 App.vue:2291 |
| 5 | `card-in` | `from { opacity: 0; } to { opacity: 1; }` | `.instance-card` → `animation: card-in var(--motion-enter) var(--ease-out) backwards` = 220ms | **无错落**（源码注释 `HomeView.vue:895` "Cards appear together so every action is available at the same time"） | 体 HomeView.vue:897；用 HomeView.vue:896 |
| 6 | `community-card-in` | `from { opacity: 0; } to { opacity: 1; }` | `.result-card` → `animation: community-card-in var(--motion-enter) var(--ease-out) backwards` | 无 | 体 CommunityView.vue:844；用 CommunityView.vue:842 |
| 7 | `pick-card-in` | `from { opacity: 0; transform: translateY(14px); } to { opacity: 1; transform: translateY(0); }` | `.pick-card` → `animation: pick-card-in var(--motion-enter) var(--ease-out) backwards` | `:nth-child(1)=0ms, (2)=60ms, (3)=120ms, (4)=180ms`（FriendConnectView.vue:137-140） | 体 FriendConnectView.vue:141；用 135 |
| 8 | `server-row-in` | `from { opacity: 0; transform: translateY(8px); } to { opacity: 1; transform: translateY(0); }` | `.servers-page .server-list-item` → `animation: server-row-in 0.38s cubic-bezier(0.22, 0.9, 0.32, 1) backwards` | `nth-child(2)=40 … (8)=280ms`（**步长 40ms**，ServersView.vue:464-470） | 体 ServersView.vue:471；用 463 |
| 9 | `bridge-pulse` | `0%,100% { box-shadow: 0 0 0 3px var(--ok-soft); }` / `50% { box-shadow: 0 0 0 6px color-mix(in srgb, var(--ok) 10%, transparent); }` | `.bridge-dot.on` → `animation: bridge-pulse 2.2s ease-in-out infinite` | 无 | 体 BridgeView.vue:295-298；用 294 |
| 10 | `reveal` | `from { opacity:0; transform:translateY(4px); } to { opacity:1; transform:none; }` | `.moving .creator-bubble` → `animation:reveal 220ms both`（缓动：**未找到**，默认 `ease`） | `animation-delay: var(--delay)`，`--delay = i * 40ms`（CreatorCard.vue:14 inline） | 体 CreatorCard.vue:27；用 25 |
| 11 | `drift` | `from { transform:translate(calc(var(--drift-x) * -1),calc(var(--drift-y) * -1)); } to { transform:translate(var(--drift-x),var(--drift-y)); }` | `.moving .creator-bubble span` → `animation:drift var(--duration) ease-in-out infinite alternate` | `animation-delay: var(--delay)` = `i*40ms`；`--duration = (3.8 + rand*3)s` = 3.80–6.80s；`--drift-x = (rand*12-6)px`（±6px）；`--drift-y = (rand*8-4)px`（±4px） | 体 CreatorCard.vue:28；用 26；参数源 CreatorCard.vue:6 |
| 12 | `readable-scroll` | `0%,18%,100% { transform: translateX(0); }` / `65%,82% { transform: translateX(var(--travel)); }` | `.overflow > span` → `animation: readable-scroll var(--duration) ease-in-out infinite` | `--travel = -scrollWidth+clientWidth px`；`--duration = max(5, distance/32 + 3)s`（MarqueeText.vue:10） | 体 MarqueeText.vue:18；用 16 |
| 13 | `splash-wobble-a` | `from { translate: -3px 2px; rotate: -5deg; } to { translate: 3px -2px; rotate: 5deg; }` | `.tile` inline `animationName`；`.board.scatter .tile` 提供 `animation-timing-function: ease-in-out; animation-iteration-count: infinite; animation-direction: alternate;` | `duration = 420 + rand*180 ms`（420–600ms）；`delay = -rand*500 ms`（**负延迟**） | 体 SplashScreen.vue:178-187；用 82-84 + 172-176 |
| 14 | `splash-wobble-b` | `from { translate: 2px 3px; rotate: 4deg; } to { translate: -2px -3px; rotate: -4deg; }` | 同上（`wobble = rand<0.5 ? 'a' : 'b'`） | 同上 | 体 SplashScreen.vue:188-197；用 59, 82-84 |
| 15 | `splash-pop` | `0% { scale: 1; } 40% { scale: 1.06; } 100% { scale: 1; }` | `.board.settle` → `animation: splash-pop 520ms cubic-bezier(0.34, 1.56, 0.64, 1)` | 无 | 体 SplashScreen.vue:198-208；用 154-156 |

**已失效的 keyframes（必须报告）**

| keyframes | 定义处 | 被谁禁用 | 原因 |
|---|---|---|---|
| `server-row-in` | ServersView.vue:471（用 463） | **ServersView.vue:510** `.servers-page .server-list-item{animation:none;overflow:visible}` | 同特异度 (0,2,0)、源序更后 → 覆盖 `animation-name`。**入场+40ms 错落全部不生效** |
| `pick-card-in` | FriendConnectView.vue:141（用 135） | **FriendConnectView.vue:161** `.pick-card{...animation:none;box-shadow:none}` | 简写把 `animation-name` 复位为 `none`；`nth-child` 的 `animation-delay`（更高特异度 0,2,0）仍存在但无动画名可依 → **入场错落不生效** |

---

## 3. `transition:` 全量清单（按控件分组）

### 3.1 按钮 / 图标按钮

| 选择器 | 属性列表（逐字） | 时长 | 缓动 | 延迟 | file:line |
|---|---|---|---|---|---|
| `.btn` | `background 0.18s ease, border-color 0.18s ease, filter 0.18s ease,`<br>`transform 0.14s ease, opacity 0.18s ease, color 0.18s ease, box-shadow 0.2s ease` | bg/border/filter/opacity/color 180ms；**transform 140ms**；box-shadow 200ms | `ease` | 0 | styles.css:174-175 |
| `.icon-btn` | `background 0.16s ease, color 0.16s ease, border-color 0.16s ease,`<br>`transform 0.14s ease, box-shadow 0.2s ease` | 160/160/160/**140**/200ms | `ease` | 0 | styles.css:277-278 |
| `.top-btn` | `background 0.15s ease, color 0.15s ease` | 150ms | `ease` | 0 | App.vue:2088 |
| `.top-icon-btn` | `background 0.15s ease, color 0.15s ease` | 150ms | `ease` | 0 | App.vue:2111 |
| `.win-btn` | `background 0.15s ease, color 0.15s ease` | 150ms | `ease` | 0 | App.vue:2382 |
| `.edit-tip-done` | `background 0.15s ease, transform 0.12s ease` | 150/120ms | `ease` | 0 | styles.css:747 |
| `.menu-item` | `background 0.14s ease, color 0.14s ease` | 140ms | `ease` | 0 | styles.css:612 |
| `.select-menu-option` | `background 0.12s ease` | 120ms | `ease` | 0 | SelectMenu.vue:162 |
| `.ic-tabs button` | `background-color 180ms ease, color 180ms ease` | 180ms | `ease` | 0 | ui-system.css:126 |
| `.connect-tab` | `color 160ms ease, border-color 160ms ease, background 160ms ease` | 160ms | `ease` | 0 | connection.css:77 |
| `.fav-btn` | `color 0.15s ease, background 0.15s ease, transform 0.12s ease` | 150/150/120ms | `ease` | 0 | GameView.vue:1958 |
| `.mirror-toggle` | `transform 0.12s ease, filter 0.15s ease` | 120/150ms | `ease` | 0 | GameView.vue:1860 |
| `.option-entry` | `background 180ms, border-color 180ms` | 180ms | **未找到（默认 `ease`）** | 0 | DefaultGameOptions.vue:80 |
| `.cfg-sections button` | `background 180ms, border-color 180ms` | 180ms | **未找到（默认 `ease`）** | 0 | KeysView.vue:229 |
| `.manage-instances` `。` | **无 transition 声明** | — | — | — | HomeView.vue:891-892（hover 只换 color/border-color，**突变**） |
| `.instance-play` / `.instance-more` | **无 transition 声明** | — | — | — | HomeView.vue:905-911（hover 背景/色**突变**） |
| `.sidebar-health` | **无 transition 声明** | — | — | — | App.vue:1959-1981（hover 背景/色**突变**） |
| `.hero-settings` / `.hero-more` | **无 transition 声明** | — | — | — | HomeView.vue:834-840（hover 背景**突变**） |
| `.result-dl` | 继承 ui-system.css:113 组：`background-color 180ms ease,color 180ms ease,border-color 180ms ease,box-shadow 220ms ease,outline-color 180ms ease`（若匹配该 `:is`） | 见左 | `ease` | 0 | ui-system.css:111-113, 47-48 |

### 3.2 卡片 / 列表行

| 选择器 | 属性列表（逐字） | 时长 | 缓动 | 延迟 | file:line |
|---|---|---|---|---|---|
| `.card` | `border-color 0.18s ease, background 0.18s ease` | 180ms | `ease` | 0 | styles.css:456 |
| `.instance-card` | `border-color 0.18s ease, background 0.18s ease, transform 0.18s ease, box-shadow 0.22s ease` | 180/180/180/220ms | `ease` | 0 | HomeView.vue:896 |
| `.result-card` | `border-color var(--motion-fast) ease, box-shadow var(--motion-normal) ease` | **130ms / 190ms** | `ease` | 0 | CommunityView.vue:840 |
| `.server-list-item`（内容区通用） | `background var(--motion-fast),border-color var(--motion-fast)` | 130ms | **未找到（默认 `ease`）** | 0 | ui-system.css:45 |
| `.servers-page .server-list-item` | `background 160ms ease, border-color 160ms ease, transform 0.18s ease, box-shadow 0.22s ease` | 160/160/180/220ms | `ease` | 0 | ServersView.vue:463 |
| `.server-monogram` | `transform 0.18s cubic-bezier(0.22, 0.9, 0.32, 1.2)` | 180ms | 过冲弹簧 | 0 | ServersView.vue:477 |
| `.bridge-card` | `transform 0.18s ease, box-shadow 0.22s ease, border-color 0.18s ease` | 180/220/180ms | `ease` | 0 | BridgeView.vue:306 |
| `.pick-card` | `transform .18s ease, border-color .18s ease, box-shadow .22s ease` | 180/180/220ms | `ease` | 0 | FriendConnectView.vue:133 |
| `.pick-icon` | `transform .2s cubic-bezier(0.22, 0.9, 0.32, 1.2)` | 200ms | 过冲弹簧 | 0 | FriendConnectView.vue:149 |
| `.pick-go` | `opacity .18s ease, transform .22s cubic-bezier(0.22, 0.9, 0.32, 1.2), color .18s ease` | 180/220/180ms | ease / 过冲弹簧 / ease | 0 | FriendConnectView.vue:159 |
| `.file-row` | `border-color 0.16s ease, background 0.16s ease, transform 0.16s ease, box-shadow 0.2s ease` | 160/160/160/200ms | `ease` | 0 | CommunityView.vue:1017 |
| `.lobby-item` | `background .2s, transform .2s` | 200ms | **未找到（默认 `ease`）** | 0 | VoxLinkPanel.vue:431 |
| `.ic-grid .ic-card` | `background .18s, transform .18s` | 180ms | **未找到（默认 `ease`）** | 0 | InstanceCenter.vue:109 |
| `.reference-links a` / `button` | `background .18s, border-color .18s` | 180ms | **未找到（默认 `ease`）** | 0 | ReferenceLinks.vue:14 |
| `.links-list a` | `background .18s` | 180ms | **未找到（默认 `ease`）** | 0 | VoxLinkRelatedLinks.vue:20 |
| `.cape-item` | `border-color 0.18s ease, background 0.18s ease, transform 0.12s ease` | 180/180/120ms | `ease` | 0 | SkinsView.vue:910 |
| `.row-main` | `box-shadow 0.18s ease` | 180ms | `ease` | 0 | SkinsView.vue:659 |
| `.mob-cell` | `border-color 0.15s ease, background 0.15s ease, transform 0.12s ease` | 150/150/120ms | `ease` | 0 | IconPickerModal.vue:122 |
| `.inst-icon` | `border-color 0.15s ease, transform 0.12s ease` | 150/120ms | `ease` | 0 | GameView.vue:1764 |
| `.theme-preview` | `border-color 0.16s ease, box-shadow 0.16s ease, transform 0.12s ease` | 160/160/120ms | `ease` | 0 | SettingsView.vue:1560 |
| `.theme-label` | `color 0.16s ease` | 160ms | `ease` | 0 | SettingsView.vue:1576 |
| `.mp-name-opt` | `border-color 0.15s ease, background 0.15s ease, color 0.15s ease` | 150ms | `ease` | 0 | App.vue:2501 |
| `.mp-radio` | `border-color 0.15s ease, box-shadow 0.15s ease` | 150ms | `ease` | 0 | App.vue:2516 |
| `.fm-row` 等 | 见 3.1/ui-system.css:45 | 130ms | ease | 0 | ui-system.css:45 |
| `.account-provider` / `.java-option` / `.empty-instances` | **无 transition 声明** | — | — | — | HomeView.vue:930/913、HomeView.vue:776 |

### 3.3 导航（侧栏）

| 选择器 | 属性列表（逐字） | 时长 | 缓动 | 延迟 | file:line |
|---|---|---|---|---|---|
| `.nav-item` | `color var(--motion-fast) ease, background var(--motion-fast) ease` | **130ms** | `ease` | 0 | App.vue:1824 |
| `.nav-bubble` | `transform 300ms cubic-bezier(.22, 1, .36, 1), width 300ms cubic-bezier(.22, 1, .36, 1), height 300ms cubic-bezier(.22, 1, .36, 1), opacity 120ms ease` | **300ms** / opacity **120ms** | 强减速（Y≤1，无过冲） | 0 | App.vue:1846 |
| `.nav-selection` | 同上（逐字相同） | 300ms / 120ms | 同上 | 0 | App.vue:1857 |
| `.nav-parent .nav-caret` | `transform 0.18s ease` | 180ms | `ease` | 0 | App.vue:1889 |
| `.nav-sub` | `grid-template-rows var(--motion-normal) var(--ease-out), opacity 0.22s ease, margin var(--motion-normal) var(--ease-out)` | **190ms** / 220ms / **190ms** | ease-out(.2,.7,.3,1) / ease / ease-out | 0 | App.vue:1901 |
| `.nav-sub .nav-sub-item` | `opacity var(--motion-fast) ease, transform var(--motion-normal) var(--ease-out)` | 130ms / 190ms | ease / ease-out | **`transition-delay: 0s`（显式清零，收起与展开都是 0s）** App.vue:1919, 1924 | App.vue:1918 |

### 3.4 模态 / 遮罩 / 浮层菜单

| 选择器 | 属性 | 时长 | 缓动 | file:line |
|---|---|---|---|---|
| `.modal-mask` | `animation: mask-in 0.2s ease` | 200ms | `ease` | styles.css:537 |
| `.modal` | `animation: modal-in 220ms var(--ease-out)` | 220ms | cubic-bezier(.2,.7,.3,1) | styles.css:554 |
| `.float-menu` | `animation: modal-in 0.16s ease` | 160ms | `ease` | styles.css:595 |
| `.update-dialog-backdrop` | `animation: mask-in var(--motion-fast) var(--ease-out)` | 130ms | ease-out | UpdateDialogShell.vue:33 |
| `.update-dialog` | `animation: modal-in var(--motion-enter) var(--ease-out)` | 220ms | ease-out | UpdateDialogShell.vue:34 |
| **批量 `@starting-style` 淡入组**（ui-system.css:100-109）：`.cfg-page > .card, .settings-body > *, .game-controls, .list-card, .installed-card, .folder-manager-actions, .notice-panel, .float-menu, .instance-more-menu, .menu, .context-menu, .modal-mask, .modal, .ic-mask, .ic-dialog-mask, .connection-panel, .settings-search-results, .options-entrances, .tab-body, .ic-content, .connection-result` | `transition:opacity 200ms ease;` + `@starting-style { opacity:0 }` | 200ms | `ease` | ui-system.css:103 / 108 |
| `.log-mask` / `.log-dialog`（HomeView 日志弹窗） | **无 transition / 无 animation** | — | — | HomeView.vue:968-969（**直接出现，无动效**） |

### 3.5 Toast / 通知

| 选择器 | 属性（逐字） | 时长 | 缓动 | file:line |
|---|---|---|---|---|
| `.toast-enter-active, .toast-leave-active` | `opacity 0.22s ease, transform 0.22s ease` | 220ms | `ease` | Toasts.vue:75 |
| `.toast-move`（TransitionGroup 重排） | `transform 0.22s ease` | 220ms | `ease` | Toasts.vue:86 |
| `.launch-notice-enter/leave-active` | `opacity 180ms ease, transform 180ms ease` | 180ms | `ease` | LaunchNotice.vue:40 |

### 3.6 输入 / 下拉 / 开关 / 滑块

| 选择器 | 属性（逐字） | 时长 | 缓动 | file:line |
|---|---|---|---|---|
| `.input, .select` | `border-color 0.18s ease, box-shadow 0.18s ease, background 0.18s ease` | 180ms | `ease` | styles.css:316 |
| `.slider::-webkit-slider-thumb` | `transform 0.15s ease` | 150ms | `ease` | styles.css:435 |
| `.switch-ui` | `background var(--motion-normal) ease, border-color var(--motion-normal) ease` | **190ms** | `ease` | styles.css:381 |
| `.switch-ui::before`（旋钮） | `transform var(--motion-normal) var(--ease-out), background var(--motion-normal) ease` | **190ms** | ease-out / ease | styles.css:393 |
| `.connection-toggle-track` | `background 160ms ease` | 160ms | `ease` | connection.css:58 |
| `.connection-toggle-track::after`（旋钮） | `transform 160ms ease`（`checked` → `translateX(14px)`） | 160ms | `ease` | connection.css:59 / 61 |
| `.select-menu-btn` | `border-color 180ms ease, background 180ms ease, box-shadow 220ms ease` | 180/180/220ms | `ease` | SelectMenu.vue:123 |
| `.select-menu-chevron` | `transform 0.18s ease`（`.open` → `rotate(180deg)`） | 180ms | `ease` | SelectMenu.vue:129 |
| 焦点复合控件组 `.tool-search,.server-search,.search-box,.input,.select,.capsule,.cfg-sections button,.select-menu-btn` | `background-color 180ms ease,color 180ms ease,border-color 180ms ease,box-shadow 220ms ease,outline-color 180ms ease` | 180/**220**/180/180ms | `ease` | ui-system.css:111-113 |
| `.mem-slider-fill` | `width 0.12s ease`（`.dragging` 时 `transition:none`，SettingsView.vue:1723） | 120ms | `ease` | SettingsView.vue:1721 |
| `.mem-slider-thumb` | `transform 0.12s ease` | 120ms | `ease` | SettingsView.vue:1728 |
| `.dup-*`（查重弹窗） | `background 0.15s ease, color 0.15s ease` / `border-color 0.15s ease, background 0.15s ease` | 150ms | `ease` | DupCleanModal.vue:239 / 341 |
| `.fm-*` | `color 0.15s ease` / `background 0.15s ease` | 150ms | `ease` | FileManager.vue:528 / 560 |
| `.mod-drop` 行 | `border-color 0.15s ease, background 0.15s ease` | 150ms | `ease` | ModDropModal.vue:449 |
| `.keys` 行 | `border-color 0.15s ease, background 0.15s ease` | 150ms | `ease` | KeysView.vue:257 |
| `.settings-*` 行 | `border-color 0.15s ease, background 0.15s ease` / `color 0.15s ease, background 0.15s ease` | 150ms | `ease` | SettingsView.vue:1279,1808 / 1744 |
| `.accounts-*` | `background 0.15s ease, color 0.15s ease`（×2）/ `border-color 0.18s ease, background 0.18s ease` / `background 0.15s ease` | 150/180ms | `ease` | AccountsView.vue:699,740 / 874 / 951 |
| `.game-*`（GameView） | `border-color 0.18s ease, box-shadow 0.18s ease` / `border-color 0.15s ease, background 0.15s ease, color 0.15s ease` / `background 0.15s ease` | 180/150ms | `ease` | GameView.vue:1650 / 1695 / 1726 |
| `.skin-*` | `opacity 0.16s ease`（history-overlay）/ `background 0.12s ease, color 0.12s ease`（history-name） | 160/120ms | `ease` | SkinsView.vue:1007 / 1041 |
| `.layout-setting>summary::after` | `transition:rotate 180ms`（`[open]` → `rotate:180deg`） | 180ms | **未找到（默认 `ease`）** | HomeLayoutEditor.vue:496 |
| `.collapse-arrow` | `transform 0.18s ease`（`rotate(-45deg)` → `[open] rotate(45deg)`） | 180ms | `ease` | SettingsView.vue:1384 |

### 3.7 进度条（含不定态微光）

| 选择器 | 属性 | 时长 | 缓动 | file:line |
|---|---|---|---|---|
| `.dl-bar-fill` | `width 0.3s ease` | 300ms | `ease` | App.vue:2282 |
| `.dl-bar.is-indeterminate .dl-bar-fill` | `animation: dl-indeterminate 1.25s ease-in-out infinite`（translateX(-120%→320%)） | **1250ms，无限循环** | `ease-in-out` | App.vue:2291 |
| `.launch-progress`（首页启动按钮内白色填充层） | `width 0.25s ease` | 250ms | `ease` | HomeView.vue:853 |
| `.row-bar-fill` | `width 0.25s ease` | 250ms | `ease` | GameView.vue:1803 |
| `.upd-progress-bar` | `width var(--motion-normal) var(--ease-out)` | **190ms** | ease-out | UpdateModal.vue:101 |
| `.java-scan-track span` | `width 0.15s ease` | 150ms | `ease` | SettingsView.vue:1456 |
| `.mem-slider-fill` | `width 0.12s ease`（拖动中 `none`） | 120ms | `ease` | SettingsView.vue:1721 |
| shimmer | **未找到**任何名为/类似 shimmer 的实现；全库 `grep shimmer` = 0 命中。唯一"扫光"效果即 `dl-indeterminate` | — | — | — |

### 3.8 图片 / 轮播

| 选择器 | 属性 | 时长 | 缓动 | file:line |
|---|---|---|---|---|
| `.hero-image` | `opacity var(--motion-carousel) ease` = **500ms** | 500ms | `ease` | HomeView.vue:810 |
| `.hero-image.active` | `opacity: 1`（非 active 为 `opacity:0`） | — | — | HomeView.vue:811 |
| Ken Burns / scale 漂移 | **未找到**。`.hero-image` 无 `transform`、无 `animation`；`.hero-card` 无 overflow 缩放动画。仅有静态 `object-position: center 42%`（HomeView.vue:1033）与 `:style="{objectFit}"`（HomeView.vue:487） | — | — | — |

### 3.9 Tab / 分段控件 / 胶囊

| 选择器 | 属性（逐字） | 时长 | 缓动 | file:line |
|---|---|---|---|---|
| `.runtime-blob` | `left var(--motion-normal) var(--ease-out), width var(--motion-normal) var(--ease-out), opacity 0.18s ease, transform 0.2s ease` | **190/190**/180/200ms | ease-out ×2 / ease / ease | HomeView.vue:867 |
| `.runtime-item` | `transform 0.18s cubic-bezier(0.22, 0.9, 0.32, 1.15)` | 180ms | 过冲弹簧 | HomeView.vue:871 |
| `.game-tabs-blob` | `left var(--motion-normal) var(--ease-out), width var(--motion-normal) var(--ease-out), opacity 0.15s ease` | 190/190/150ms | ease-out/ease-out/ease | GameView.vue:1888 |
| `.game-tab` | `color 0.2s ease` | 200ms | `ease` | GameView.vue:1909 |
| `.capsule-blob` | `left var(--motion-normal) var(--ease-out), top var(--motion-normal) var(--ease-out), width var(--motion-normal) var(--ease-out), height 0.32s ease, opacity 0.15s ease` | **190/190/190/320/150ms**（height 与其它不同！） | ease-out ×3 / ease / ease | CommunityView.vue:751 |
| `.capsule` | `color 0.2s ease` | 200ms | `ease` | CommunityView.vue:765 |
| `.seg-blob` | `left 0.3s cubic-bezier(0.3, 1.2, 0.4, 1), width 0.3s cubic-bezier(0.3, 1.2, 0.4, 1), opacity 0.15s ease` | 300ms/300ms/150ms | **过冲弹簧（Y=1.2）** | SkinsView.vue:829 |
| `.seg-btn` | `color 0.2s ease` | 200ms | `ease` | SkinsView.vue:848 |
| `.motto-glyph`（顶栏标语字符） | `transform 360ms cubic-bezier(.2,.9,.25,1.3), color 180ms ease` | **360ms** / 180ms | 过冲弹簧（Y=1.3）/ ease | CreatorMotto.vue:66 |
| `.creator-reveal`（创作者名单展开） | `grid-template-rows 240ms var(--ease-out), opacity 200ms ease` | 240ms / 200ms | ease-out / ease | CreatorCard.vue:29 |

### 3.10 滚动条

| 选择器 | 规则 | 过渡 | file:line |
|---|---|---|---|
| `::-webkit-scrollbar` | `width: 8px; height: 8px` | — | styles.css:139-142 |
| `::-webkit-scrollbar-track` | `background: transparent` | — | styles.css:143-145 |
| `::-webkit-scrollbar-thumb` | `background: var(--scroll); border-radius: 999px` | — | styles.css:146-149 |
| `::-webkit-scrollbar-thumb:hover` | `background: var(--accent)` | **无 transition → 颜色瞬时切换** | styles.css:150-152 |
| `::-webkit-scrollbar-corner` | `background: transparent` | — | styles.css:153-155 |
| `.content` | `scrollbar-gutter: stable` | — | App.vue:2402 |
| `--scroll` | 由 JS `App.vue:1002` `color-mix(in srgb, ${textDim} 32%, transparent)` 计算 | — | — |

### 3.11 其它（拖拽遮罩 / 编辑模式 / 路由）

| 选择器 | 规则 | file:line |
|---|---|---|
| `.drop-mask` / `.drop-box` | **无 animation / transition** —— 拖入遮罩直接出现（背景色 `color-mix(accent 16%)` + `backdrop-filter: blur(24px) saturate(130%)`） | App.vue:2419-2441 |
| `.notice-panel` / `.notice-mask` | 属于 ui-system.css:100 `@starting-style` 淡入组（opacity 200ms ease） | App.vue:2141-2195；ui-system.css:102 |
| `.content .instance-card:hover` | `transform:none`（**覆盖** HomeView 的 `translateY(-1px)`） | ui-system.css:26 |
| `.content :is(.result-card,.server-list-item):hover` | `transform:none`（**覆盖** `translateY(-2px)`） | ui-system.css:46 |
| `.content .card { box-shadow:none }` | 覆盖基线阴影 | ui-system.css:13 |

---

## 4. JS 驱动的位移指示器（需要重写为 Storyboard 的部分）

### 4.1 `.nav-bubble` + `.nav-selection`（`composables/useNavigationBubble.ts`）

- **DOM**：`App.vue:1248-1249`，两个绝对定位子层，挂在 `.nav`（`App.vue:1247` `ref="navEl"`）内。
- **触发**（`App.vue:1247`）：`@pointerover="retargetNav"`、`@pointerleave="resetNav"`、`@focusin="retargetNav"`、`@focusout="navFocusOut"`、`@scroll.passive="measureNav"`；另有 `watch([selected, layout], refreshLayout, {flush:'post'})`（`useNavigationBubble.ts:47`），`layout = [resourceExpanded, inResourceGroup, visibleNavItems, visibleResourceSubItems]`（`App.vue:206`）。
- **测量**（`useNavigationBubble.ts:11-27`）：`getBoundingClientRect()` 相对 `.nav` 左上角 + `nav.scrollLeft/scrollTop`；`element.closest('[inert]')` 或断连 → `visible=false`。
- **展开跟随**（`useNavigationBubble.ts:40-46`）：`refreshLayout` 用 `requestAnimationFrame` **连续测量 240ms**（`performance.now()+240`），让水滴跟着子菜单的 `grid-template-rows` 展开几何一起动。
- **写入的属性**（inline style，`useNavigationBubble.ts:57-64`）：
  - `transform: translate3d(<x>px, <y>px, 0)`
  - `width: <w>px`、`height: <h>px`
  - `opacity: visible?1:0`
- **缓动/时长**（CSS 侧，App.vue:1846 / 1857）：`transform|width|height 300ms cubic-bezier(.22,1,.36,1)`，`opacity 120ms ease`。
- **外观**：`.nav-bubble` 背景 `linear-gradient(135deg, color-mix(--accent-soft 80%, transparent), var(--hover))`、`box-shadow: 0 3px 12px #00000008`、`border-radius: var(--radius-md)`(10px)、`will-change: transform`（App.vue:1837-1848）。`.nav-selection` 只有 `box-shadow: inset 3px 0 0 var(--accent)`（左侧 3px 强调条）、`z-index:2`（App.vue:1849-1858）。

### 4.2 `.runtime-blob`（首页运行时条，`HomeView.vue`）

- **DOM**：`HomeView.vue:546`，`:class="{on: runtimeHover>=0}"`，`:style="runtimeBlobStyle"`。
- **触发**：`.runtime-strip @mouseleave="runtimeHover=-1"`（545）+ 三个 `.runtime-item @mouseenter="runtimeHover=0|1|2"`（547/552/557）；`watch(runtimeHover, () => nextTick(updateRuntimeBlob))`（HomeView.vue:186）。
- **测量**（HomeView.vue:177-185）：`items[runtimeHover].offsetLeft` / `offsetWidth`（**只用 offset，不用 rect**）。
- **写入属性**（HomeView.vue:187-190）：`left: <n>px`、`width: <n>px`。**`top`/`height` 由 CSS 决定**（`top:0; bottom:0; margin: var(--space-1) 0`，HomeView.vue:862-869）。
- **缓动**：`left/width 190ms var(--ease-out)`、`opacity 180ms ease`、`transform 200ms ease`（HomeView.vue:867）。
- **on/off 态**：基态 `opacity:0; transform:scale(0.97)`；`.on` → `opacity:1; transform:scale(1)`（866/870）。
- 背景 `color-mix(in srgb, var(--accent) 10%, transparent)`，`border-radius:12px`，`z-index:0`，`pointer-events:none`。

### 4.3 `.capsule-blob`（社区筛选胶囊，`CommunityView.vue`）

- **DOM**：`CommunityView.vue:474`。
- **状态**：`reactive({left,top,width,height,on})`（63）；`updateKindBlob()` 读 `active.offsetLeft/offsetTop/offsetWidth/offsetHeight`（65-74）。
- **触发**：`watch` 分类变化 → `nextTick(updateKindBlob)`；`ResizeObserver` 观察容器（76-88）；`onUnmounted` 断开。
- **写入**：`left/top/width/height px` + `opacity`（89-95）。
- **缓动**：见 3.9（**注意 height 是 0.32s ease，与其余 190ms ease-out 不一致**，CommunityView.vue:751）。
- 外观：`border-radius:999px`、`background:var(--accent-grad)`、`box-shadow: 0 2px 8px var(--accent-soft)`、`z-index:0`（746-754）。

### 4.4 `.game-tabs-blob`（游戏版本页 Tab，`GameView.vue`）

- **DOM**：`GameView.vue:932`。`reactive({left,width,on})`（95）；`updateTabBlob` 读 `active.offsetLeft/offsetWidth`（96-104）。
- **触发**：`watch(tab)`→nextTick（105）、`watch(()=>store.installed.length)`→nextTick（107，因为「已安装（14）」文字变宽）、`ResizeObserver(gameTabs)`（108-114）、`onMounted` nextTick + **`setTimeout(updateTabBlob, 200)`**（110-115 双保险）。
- **写入**：`left`、`width`、`opacity`（120-123）。
- **缓动**：`left/width 190ms var(--ease-out)`，`opacity 150ms ease`（1888）。
- 外观基线 `background:var(--accent-grad); border-radius:999px; top/bottom: var(--space-1)`（1881-1891）；**后发覆盖**（GameView.vue:2168）：`border-radius:var(--radius-sm); background:var(--accent-soft); box-shadow: inset 0 -2px var(--accent)`（同特异度、源序更后 → 生效，实际是「浅底 + 底部 2px 下划线」）。

### 4.5 `.seg-blob`（皮肤页 行走/待机 分段，`SkinsView.vue`）

- **DOM**：`SkinsView.vue:376`。`reactive({left,width,on})`（63）；`updateAnimSegBlob` 读 `.seg-btn[data-seg=<previewAnim>]` 的 `offsetLeft/offsetWidth`（64-72）。
- **触发**：`watch(previewAnim)`→nextTick（73）；`onMounted` nextTick + `setTimeout(...,200)`（76-77）；`ResizeObserver`（78-79）；**`watch(store.settings.theme)` → nextTick + `setTimeout(...,60)`**（80，主题切换后重算）。
- **写入**：`left`、`width`、`opacity`（83-87）。
- **缓动**：`left/width 0.3s cubic-bezier(0.3, 1.2, 0.4, 1)`（**过冲**），`opacity 0.15s ease`（829）。
- 外观：`top:3px; bottom:3px; border-radius: var(--radius-sm); background: var(--accent-grad); z-index:0`（823-831）。

### 4.6 `CreatorMotto` 字符弹簧（纯 JS，无 Storyboard 等价）

- `CreatorMotto.vue:23-39`：`@pointermove` → `requestAnimationFrame`，对每个 `.motto-glyph` 直接写 `glyph.style.transform = translate(<x>px,<y>px) rotate(<x/7>deg)`。
- 力学：`force = max(0, 1 - distance/54)**2 * 18`（半径 **54px**，最大位移 **18px**），方向为"字符远离指针"（`dx/|d|*force`）；`distance<=0.1` 时 x=0、y=-force。
- 回位靠 CSS `transition: transform 360ms cubic-bezier(.2,.9,.25,1.3)`（66）——即 **JS 每帧改写 + CSS 过渡平滑**，`reset()` 把 `style.transform=''` 触发过渡。
- 抑制条件（24）：`props.disabled || !decorativeActive || reduced.matches || pointerType==='touch'`。

### 4.7 `MarqueeText` 距离自适应滚动（纯 JS 测量 + CSS keyframes）

- `MarqueeText.vue:6`：`distance = max(0, inner.scrollWidth - outer.clientWidth)`；`ResizeObserver` 持续测量；`watch(text)` → nextTick 重测。
- `--travel = -distance px`；`--duration = max(5, distance/32 + 3) s`（10）；`overflow` 类仅在 `distance>1` 时加（12）。
- 关键帧：`0%,18%,100% → translateX(0)`；`65%,82% → translateX(--travel)`（**两端各驻留 ~18%/18%**，中段线性移动），`ease-in-out`、`infinite`。

### 4.8 `CarouselPlayback`（轮播时钟，`shared/carouselPlayback.ts`）—— 见第 7 节。

### 4.9 `visualDesign.ts` 写入的是 **CSS 自定义属性 + 布局几何**，不是动画

- `visualDesign.ts:57` `el.style.setProperty(key, value, 'important')` —— 全库唯一 JS 写 CSS 变量的通用入口（配色由 App.vue 的主题函数写，见第 1 节）。
- 它写的属性（89-105）：`position`、**`translate`（独立 transform 属性）**、`z-index`、`width`/`max-width`/`flex-shrink`、`height`/`min-height`、`color`、`background`、`opacity`、`border-radius`、`backdrop-filter`、`font-*`、`gap`、`padding`、`order`、`display`、`outline`。
- **关键点**：个性化编辑模式下这些写入**没有 transition**（除被 ui-system.css/HomeView 的通用 transition 覆盖的元素），所以拖动/改数值是**瞬变**的；但 `translate` 是 CSS 独立属性，若元素本身有 `transition: transform ...`，`translate` 不会走 `transform` 通道（WPF 无 `Translate` 独立属性 + `RenderTransform` 双通道概念，需注意）。
- 刷新节流：`requestAnimationFrame` 单帧合并（`visualDesign.ts:54,115`）+ `MutationObserver` + `resize` 监听（116-119）。

---

## 5. Vue `<Transition>` / `<TransitionGroup>` 全部名称与类值

| name | 挂载点 (file:line) | `*-enter-active` | `*-enter-from` | `*-leave-active` | `*-leave-to` | `*-move` | 备注 |
|---|---|---|---|---|---|---|---|
| `fade`（路由） | App.vue:1502 `<Transition name="fade" :duration="routeDuration">` | styles.css:648 `opacity 180ms ease, transform 180ms var(--ease-out)` | styles.css:650 `opacity:0; transform:translateY(6px)`；**ui-system.css:52 覆盖 `.route-view.fade-enter-from{transform:none}`** → 只剩纯淡入 | styles.css:649 `opacity 80ms ease` | styles.css:651 `opacity:0` | — | **ui-system.css:51 覆盖 `.route-view.fade-enter-active{transition:opacity var(--motion-normal) var(--ease-out)}`（190ms）**；**ui-system.css:50 `.route-view.fade-leave-active{display:none}`** → 离场不动画、立即从布局移除。`:duration` = `routeDuration`（App.vue:203）= reduce ? **0** : `{enter:180, leave:80}`（显式超时兜底，注释 App.vue:1501：窗口被遮挡时 `transitionend` 不触发） |
| `popover` | SelectMenu.vue:85、MinecraftVersionPicker.vue:23 | ui-system.css:92 `opacity 180ms ease, translate 200ms var(--ease-out)` | ui-system.css:93 `opacity:0; translate:0 -5px` | ui-system.css:92 同 active | ui-system.css:93 同 from | — | ui-system.css:94 `.popover-leave-active{pointer-events:none}`；reduce → `transition:none`（ui-system.css:123） |
| `subpage` | DefaultGameOptions.vue:55 `:duration="200"`、FriendConnectView.vue:51 `:duration="200"` | ui-system.css:95 `opacity 200ms ease, translate 200ms var(--ease-out)` | ui-system.css:96 `opacity:0; translate:0 6px` | **ui-system.css:97 `.subpage-leave-active{display:none}`**（无离场动画） | — | — | reduce → none（ui-system.css:123） |
| `instance-switch` | HomeView.vue:497 `mode="out-in"` | HomeView.vue:823 `opacity 100ms ease, transform 100ms ease` | HomeView.vue:824 `opacity:0; transform:translateY(2px)` | HomeView.vue:823 同 | HomeView.vue:825 `opacity:0; transform:translateY(-2px)` | — | `out-in` 串行 → hero 标题换实例时**总时长 200ms**（100 出 + 100 入）；reduce → none（HomeView.vue:826-828） |
| `launch-notice` | LaunchNotice.vue:26 | LaunchNotice.vue:40 `opacity 180ms ease, transform 180ms ease` | LaunchNotice.vue:41 `opacity:0; transform:translate(-50%, -6px)` | 40 同 | 41 同 | — | 基态含 `transform:translateX(-50%)`（LaunchNotice.vue:36）→ 进出必须带 `-50%` 才能保持居中。自动消失 3200ms（LaunchNotice.vue:16）；reduce → none（42） |
| `toast` | **`<TransitionGroup name="toast" tag="div" class="toasts">`** Toasts.vue:6 | Toasts.vue:73-76 `opacity 0.22s ease, transform 0.22s ease` | Toasts.vue:77-80 `opacity:0; transform:translateX(24px)` | 73-74 同 | Toasts.vue:81-84 `opacity:0; transform:translateY(-8px)` | Toasts.vue:85-87 `.toast-move { transition: transform 0.22s ease }` | 最多显示最后 3 条（Toasts.vue:7）；容器 `pointer-events:none`（23） |
| `edit-tip` | **未找到使用点**（全库无 `name="edit-tip"`） | styles.css:757-760 `opacity 0.22s ease, transform 0.22s ease` | styles.css:761-765 `opacity:0; transform:translate(-50%, -10px)` | 同 active | 同 from | — | **遗留死 CSS**（编辑模式已重写），移植时可忽略 |
| `ep-slide` | **未找到使用点** | styles.css:768-771 `transform 0.25s ease, opacity 0.25s ease` | styles.css:772-776 `transform:translateX(40px); opacity:0` | 同 active | 同 from | — | **遗留死 CSS** |

无 `appear` 修饰符；除 `fade`/`subpage` 外都没有显式 `:duration`（靠 `transitionend`）。

---

## 6. Hover / Press / Active 微交互汇总

| 控件 | hover 变化 | press(`:active`) 变化 | 过渡通道 | file:line |
|---|---|---|---|---|
| **所有按钮兜底** | `filter: brightness(1.08)`（`@media (hover: hover)` 内，`button:not(:disabled):not([aria-disabled="true"])`） | — | `.btn` 有 `filter 0.18s ease`；`.icon-btn` **没有 filter 通道 → 突变** | styles.css:779-782 |
| `.btn` | `transform: none`（显式静止） | **`transform: scale(0.985)`** | transform **140ms ease** | styles.css:184-190 |
| `.btn-gold` | `filter: brightness(1.08)`；`box-shadow: 0 2px 6px → 0 3px 10px var(--accent-soft)`；`transform:none` | **`scale(0.985)`** | filter 180ms / box-shadow 200ms | styles.css:206-213 |
| `.btn-ghost` | 基线 `border-color→--border-strong, color→--text, box-shadow:none, transform:none`；**后者覆盖**（同特异度、源序更晚）：`background: color-mix(--accent 10%, --card-2); border-color: var(--accent); box-shadow: 0 3px 12px var(--accent-soft)` | **`scale(0.985)`** | bg 180 / border 180 / box-shadow 200 | styles.css:225-233 + **783-787（赢）** |
| `.btn-danger` | `background: color-mix(--danger 18%, transparent)`；`box-shadow: 0 4px 14px color-mix(--danger 14%, transparent)`；`transform:none` | **`scale(0.985)`** | 同上 | styles.css:245-252 |
| `.icon-btn` | `background: var(--card-2); color: var(--accent-2); transform:none; box-shadow:none` | **`scale(0.93)`** | bg/color 160 / transform **140** / shadow 200 | styles.css:281-289 |
| `.launch-combo`（首页启动条） | `transform: translateY(-2px)`；`box-shadow: 0 12px 32px accent38% + inset 0 2px 0 white14% → 0 16px 40px accent46% + inset 0 2px 0 white16%` | **`transform: translateY(0) scale(0.99)`** | `transform var(--motion-normal)=190ms var(--ease-out)`, `box-shadow 0.22s ease` | HomeView.vue:843-847 |
| `.launch-main` / `.launch-arrow` | `filter: brightness(1.08)`；`.launch-arrow svg → transform: rotate(180deg)`（`.open`） | 未找到 | svg `transform 0.18s ease`；filter 无通道（`.launch-main` 无 transition → **突变**） | HomeView.vue:855-858 |
| `.launch-main:disabled` | — | — | `filter: saturate(0.75)`；`cursor:not-allowed`（**无过渡**） | HomeView.vue:854 |
| `.hero-settings` / `.hero-more` | `background: rgba(10,16,17,.58) → rgba(19,29,29,.76)` | 未找到 | **无 transition → 突变** | HomeView.vue:834, 839-840 |
| `.runtime-item` | `color: var(--accent-2)`（+ hover 触发 blob 位移） | 未找到 | `transform 0.18s cubic-bezier(0.22,0.9,0.32,1.15)`（声明了 transform 通道但 hover 不改 transform → 空通道） | HomeView.vue:871-873 |
| `.instance-card` | `border-color→--border-strong; background→--card-2`；**`transform:translateY(-1px)` 被 ui-system.css:26 `transform:none` 击败**；`box-shadow:var(--shadow)` 被 HomeView.vue:1030 `box-shadow:none`（同特异度、更晚）**击败**；1030 还把 `background` 改为 `var(--hover)` | 未找到 | border/bg 180 / transform 180 / shadow 220 | HomeView.vue:896-899 vs **1030** vs **ui-system.css:26** |
| `.instance-play` | `background: color-mix(accent18%,card-2) → var(--accent)`；`color: accent-2 → on-accent` | 未找到 | **无 transition → 突变** | HomeView.vue:909-911 |
| `.server-list-item` | `border-color→strong; background→card-2; transform:translateY(-2px); box-shadow: 0 8px 22px accent10%`（**ServersView.vue:510 后发把 hover/active 的 `transform` 与 `box-shadow` 都置 none**） | `transform: translateY(0) scale(0.998)`（同样被 510 → `none`） | bg/border 160、transform 180、shadow 220 | ServersView.vue:463,472-473 vs **510（赢）** |
| `.server-monogram` | `transform: scale(1.08)`（被 510 → none） | — | `transform 0.18s cubic-bezier(0.22,0.9,0.32,1.2)` | ServersView.vue:477-478 vs 510 |
| `.bridge-card` | `transform: translateY(-2px)`；`box-shadow: 0 10px 26px accent10%`；`border-color→strong` | 未找到 | transform 180 / shadow 220 / border 180 | BridgeView.vue:306-307 |
| `.pick-card` | `transform:none`（142/143 声明）；`border-color→accent55%`；`box-shadow:0 14px 34px accent15%`（**161 覆盖为 none**） | `transform:none` | transform 180 / border 180 / shadow 220 | FriendConnectView.vue:133,142-143 vs **161（赢）** |
| `.pick-icon` | `transform: scale(1.08)`（**161 覆盖为 none**） | — | `transform .2s cubic-bezier(0.22,0.9,0.32,1.2)` | 149,151 vs 161 |
| `.pick-go` | `opacity .4→1`；`transform: translateX(-4px)→translateX(0)`（过冲弹簧）；`color→accent-2` | — | opacity 180 / transform 220 / color 180 | FriendConnectView.vue:159-160 |
| `.file-row` | `background→hover; border-color→strong; transform:translateY(-1px)` | 未找到 | 160/160/160/200ms ease | CommunityView.vue:1017-1022 |
| `.theme-preview` | `transform: translateY(-1px)`；`border-color→strong` | **`transform: scale(0.98)`** | border/shadow 160 / transform 120 | SettingsView.vue:1560-1575 |
| `.mem-slider-thumb` | `transform: translate(-50%,-50%) scale(1.12)` | `.dragging → scale(1.18)`、`cursor:grabbing` | transform 120ms ease | SettingsView.vue:1728-1730 |
| `.slider::-webkit-slider-thumb` | `transform: scale(1.2)` | 未找到 | transform 150ms ease | styles.css:435-439 |
| `.mirror-toggle` | `filter: brightness(1.15)` | **`transform: scale(0.96)`** | transform 120 / filter 150 | GameView.vue:1860-1866 |
| `.fav-btn` | `color→accent; background→accent-soft` | **`transform: scale(0.9)`** | color/bg 150 / transform 120 | GameView.vue:1958-1969 |
| `.mob-cell` | `border-color→accent; transform: scale(1.06)` | 未找到 | border/bg 150 / transform 120 | IconPickerModal.vue:122-127 |
| `.inst-icon` | `border-color→accent; transform: scale(1.05)` | 未找到 | border 150 / transform 120 | GameView.vue:1764-1768 |
| `.cape-item` | `border-color→accent-deep; transform: translateY(-2px)` | 未找到 | border/bg 180 / transform 120 | SkinsView.vue:910-915 |
| `.lobby-item` | `background: color-mix(accent8%,card-2); transform: translateY(-2px)`（**VoxLinkPanel.vue:434 后发覆盖 `transform:none`**） | 未找到 | bg 200 / transform 200 | VoxLinkPanel.vue:431-432 vs **434** |
| `.ic-grid .ic-card` | `background→accent-soft; transform: translateY(-2px)` | 未找到 | bg 180 / transform 180 | InstanceCenter.vue:109 |
| `.top-btn` / `.top-icon-btn` / `.win-btn` | `background→card-2`；`color→accent-2`（top-btn）/ `--text`（其余）；`.win-close:hover → background:var(--danger); color:#fff` | 未找到 | bg/color 150ms ease | App.vue:2090-2093, 2113-2116, 2388-2395 |
| `.sidebar-health` / `.manage-instances` / `.account-provider` / `.account-more` / `.skin-refresh` / `.skin-tip` / `.skin-overlay` / `.instance-more` / `.log-close` / `.dl-dismiss` | 只改 `color`/`background`，**全部无 transition → 瞬变** | 未找到 | **未找到** | HomeView.vue:892,906,927,933,951,954；App.vue:1978,2310 |
| `.edit-tip-done` | `background: color-mix(on-accent 20% → 32%)` | **`transform: scale(0.96)`** | bg 150 / transform 120 | styles.css:737-754 |
| `.mp-name-opt` / `.mp-radio` | border/background/color；`.mp-radio:active` 未找到；`.active` → `box-shadow: inset 0 0 0 3px var(--card), inset 0 0 0 8px var(--accent)` | 未找到 | 150ms ease | App.vue:2501-2521 |
| `.menu-item` | `background→accent-soft; color→accent-2`（`.danger:hover → danger-soft/danger`） | 未找到 | 140ms ease | styles.css:612-635 |
| `.select-menu-option` | `background→hover`；`.active → accent-soft` | 未找到 | 120ms ease | SelectMenu.vue:162,165-167 |
| `.toast-close`（`.icon-btn`） | 复用 `.icon-btn` | `scale(0.93)` | 见 `.icon-btn` | Toasts.vue:42 → styles.css |
| `[inert]` | `opacity: .65` | — | **无 transition** | ui-system.css:43 |
| `:focus-visible` | `outline: 2px solid var(--accent); outline-offset: 3px`（复合控件走 `outline-color 180ms ease`） | — | ui-system.css:113 提供 `outline-color` 通道；`.btn:focus-visible` 用 `box-shadow: 0 0 0 3px accent-soft` 无 outline | ui-system.css:75, 111-113；styles.css:179-182, 654-658 |

---

## 7. 轮播 / Banner 行为

| 项目 | 值 | 来源 |
|---|---|---|
| 交叉淡入时长 | **500ms**（`var(--motion-carousel)`，ease） | ui-system.css:7；HomeView.vue:810 |
| 淡入方式 | 所有 `<img class="hero-image">` **绝对堆叠**（`position:absolute; inset:0`），仅 `.active` 那张 `opacity:1`；切换 = 两张同时反向淡入淡出（无 out-in 串行） | HomeView.vue:480-489, 810-811 |
| 每张停留时长（间隔） | `1000 * carouselDuration(...)`；`carouselDuration = isFinite(v) ? clamp(v,1,120) : 6.5` → **默认 6.5 秒**，可配 1–120 秒 | HomeView.vue:129；shared/appearancePolicy.ts:4-6 |
| 间隔来源（设置） | `settings.launchThumbnail.durations[图片路径] ?? settings.launchThumbnail.intervalSeconds`（**逐图覆盖优先于全局间隔**） | HomeView.vue:129；appearancePolicy.ts:7-10（`carouselTiming`） |
| 图片数上限 | `MAX_CAROUSEL_IMAGES = 20`，去重去空 | appearancePolicy.ts:3,14 |
| 定时器 | `setInterval(..., 100)` **100ms tick**；`document.hidden` 直接 return | HomeView.vue:134-139 |
| 切换前置条件 | `peekNext(now) !== index` 时，若 `!readyBanners.has(next.src)` → **本次跳过，下一拍再试**（防闪现旧图） | HomeView.vue:136-138；carouselPlayback.ts:19-22；注释 HomeView.vue:104 |
| 预加载 | 启动时对每张 `new Image()`，`onload` 记入 `readyBanners` | HomeView.vue:107-112, 132 |
| 暂停/恢复 | `decorativeActive = !document.hidden && !reduced` → `watch(decorativeActive)` 启停 timer；暂停时把 `{path, remainingMs}` 写入 `localStorage['kamucl.carousel.'+scope]`，恢复时**从剩余时间续播（不重置）** | HomeView.vue:98-99,115-130；motion.ts:13；carouselPlayback.ts:6-11,23-26 |
| 轮播作用域 scope | `instance:<folder>:<id>`（实例有缩略图）> `global`（用户配了图）> `builtin` | HomeView.vue:113 |
| 内置图失败 | `@error="onBannerError(item)"` → 从 `failedBanners` 过滤掉 | HomeView.vue:89, 487 |
| Ken Burns / 缩放漂移 | **未找到**（`.hero-image` 无 transform/animation） | — |
| `objectFit` | `:style="{objectFit: item.fit}"`，`fit = global.fit ?? 'crop'`→css；内置图固定 `cover` | HomeView.vue:85,90,93,487 |
| 遮罩层 | `.hero-shade` 双/三层线性渐变，**静态无动画**；HomeView.vue:815 与 1030 各一份，**1030（更晚）赢**：`linear-gradient(180deg, rgba(0,0,0,.16), rgba(0,0,0,.18) 30%, rgba(0,0,0,.68))` | HomeView.vue:815 vs 1030 |

---

## 8. Splash（启动画面）

### 8.1 **实际生效的实现是 Canvas 碎玻璃汇聚**，不是 `SplashScreen.vue`

`SplashScreen.vue`（144 块 DOM 拼图）**全库无任何引用**（`grep -rn SplashScreen src/renderer` 只命中自身）→ 死代码。真正加载的是：
`src/renderer/splash.html`（`<canvas id="glass">` + `<div id="caption">`）→ `src/splash.ts` → `src/splash.css` + `shared/startup.ts`。
移植时需要**逐帧渲染循环**而不是 Storyboard。

时间线（`shared/startup.ts:15-18`）：

| 阶段 | 时刻 | 内容 | 来源 |
|---|---|---|---|
| ① 漂浮期 | `0 → GLASS_FLOAT_MIN_MS = 900ms`（且需 `canAssembleBoot(state)` 为真才进入②） | 36×2=72 块三角玻璃碎片各自正弦漂浮；指针斥力弹簧实时交互 | startup.ts:15,31-33；splash.ts:37-38 |
| ② 汇聚期 | `CONVERGE_DURATION = 1400ms`（每块另有 `delay = rand*180ms` 错峰；有效插值窗口 `1400-180 = 1220ms`） | 每块从当前漂浮位姿插值到目标位姿 | startup.ts:16,52,93-99 |
| ③ 定型 | 汇聚完成后 `ASSEMBLED_HOLD_MS = 2000ms` | 贴完整头像，通知主进程 `assembled()` | startup.ts:17；splash.ts:61-63 |
| ④ 退场 | reveal 时 `body.leaving` → `opacity: 1 → 0`，`transition: opacity 320ms ease`；`transitionend` → `bridge.finished()` | splash.css:18-19；splash.ts:25-28 |
| 帧率上限 | `BOOT_FRAME_MS = 1000/60`（约 60fps，`nextFrame` 累加，掉帧不追赶） | startup.ts:18；splash.ts:30-33 |

漂浮位姿（`startup.ts:62-67`，逐字）：
```
x = shard.x + sin(elapsed/1500 + phase) * 14 + offsetX
y = shard.y + cos(elapsed/1800 + phase) * 11 + offsetY
rotation = spin + sin(elapsed/2100 + phase) * .22        // 弧度；spin = (rand-.5)*2.4
scale = 1.06 + sin(elapsed/1900 + phase) * .12           // 0.94 ~ 1.18
```
指针斥力（`startup.ts:70-89`）：半径 `GLASS_POINTER_RADIUS = 140px`；`force = 2400 * (1 - d/140)**2 / max(1,d)`；`dt = clamp(delta,0,32)/1000`；`drag = exp(-5*dt)`；`offset` 钳位 ±180px。

汇聚缓动（`startup.ts:29`）：**smootherstep** `t*t*t*(t*(t*6-15)+10)`（不是 cubic-bezier，等价 `ease-in-out-quint`）。`progress=1` 时强制 `{x:targetX, y:targetY, rotation:0, scale:1}`。

每块绘制（`splash.ts:42-59`）：`translate(p.x,p.y) → rotate(p.rotation) → scale(p.scale,p.scale)`；图片 alpha `= .12 + .88*progress`；玻璃 tint 线性渐变 `(-28,-30)→(32,35)`，stops：`#effaff80 / .40 #badfff20 / .52 #ffffff58 / 1 #bcb0fa28`；描边 `#25334945` lw2.4 + `#eaf7ffb8` lw0.85；高光边 alpha `= .55 + sin(elapsed/1100 + phase)*.25`、lw1.4。

几何（`startup.ts:36-60`）：`board = max(80, floor(min(224, w*.26, h*.32)))`；`6×6` 网格抖动三角化（`cell = board/6`，内部点抖动 `(rand-.5)*.65*cell`），LCG 种子 `1041`。

文字（`splash.ts:15,24`）：`读取配置 → 加载账户 → 扫描游戏实例 → 加载首页图片、Java 与皮肤 → 准备首帧`，后接 `…`；`state.ready` 时改为 `准备就绪`。
`#caption` 定位：`left:50%; top: calc(50% + 135px); transform: translateX(-50%); width:220px; height:86px; border-radius:14px; background:#172021b8; border:1px solid #ffffff24;`（splash.css:14-17）。

reduce 分支（`splash.ts:14,37,40-41,61`）：`reduced=true` → 立即 `convergence = elapsed`（跳过漂浮）、`complete` 恒真（直接贴整图）、跳过 `advanceBootGlass`（无指针交互）、退场 transition → 1ms（splash.css:19）。

### 8.2 `SplashScreen.vue`（死代码，为完整性列出）

时间线（`SplashScreen.vue:14-18`）：`T_SCATTER=700` → `phase=converge`；`T_STAGGER=600`（每块 `delay = rand*600`）；`T_FLY=900`；`T_SETTLE = 700+600+900 = 2200` → `phase=settle` + `.brand.show`；`T_DONE = 2200+500 = 2700ms` → `emit('done')`。

| 元素 | 规则 | file:line |
|---|---|---|
| `.splash` | `transition: opacity 400ms ease`；`.leaving → opacity:0; pointer-events:none` | 139, 141-144 |
| `.tile` | `transition: transform 900ms cubic-bezier(0.6,0.05,0.2,1), translate 900ms cubic-bezier(0.6,0.05,0.2,1), rotate 900ms cubic-bezier(0.6,0.05,0.2,1)` | 166-169 |
| `.board.scatter .tile` | `animation-timing-function: ease-in-out; animation-iteration-count: infinite; animation-direction: alternate;` + inline `animationName: splash-wobble-a|b`、`duration 420–600ms`、`delay -0~-500ms`（负） | 172-176 + 82-84, 60-61 |
| `.board.settle` | `animation: splash-pop 520ms cubic-bezier(0.34,1.56,0.64,1)`（**过冲**） | 154-156 |
| 汇聚时 | inline `transform: 'translate(0px,0px) rotate(0deg)'` + `transitionDelay: <rand*600>ms`，且**移除 animation** → 靠 transition 回到 0 | 87-91 |
| `.brand` | `opacity 0 → 1` + `transform: translateY(10px) → 0`，`transition: opacity 0.6s ease, transform 0.6s ease`（600ms） | 211-223 |
| reduce | `phase='settle'` 立即；`emit('done')` in 100ms；`animation:none` / `transition:none`（board/tile/brand） | 99-103, 237-247 |

### 8.3 WPF 落地状态（`Services/BootGlass.cs` + `Services/BootSplashWindow.cs`）

算法层逐函数照搬（同一个 LCG 种子 1041 才能拼出同一张脸），渲染层用 `Canvas` + 72 组 `Path`
（图/色/描边/高光四条）+ `TransformGroup(Translate,Rotate,Scale)`，主循环挂 `CompositionTarget.Rendering`
并按 `BOOT_FRAME_MS` 自己掐表（高刷屏上不限速会把汇聚节奏跑快）。

踩过的四个坑，逐个都表现为"碎片是纯灰玻璃 / 整脸不出现"：

| 坑 | 现象 | 根因 |
|---|---|---|
| `ImageBrush.ViewportUnits` 默认是 `RelativeToBoundingBox` | 碎片只有纯灰玻璃，看不到脸 | `Viewport=(-sx,-sy,board,board)` 里的 board 被当成"包围盒的倍数"，每块只采到一个像素。必须显式 `BrushMappingMode.Absolute` |
| 汇聚完成层 `Visibility` | 退场前一片黑 | 整脸 `Image` 建出来就是 `Collapsed`，切的是外层容器 → 子层永远不显示。只留一层，切它自己 |
| `TransformedBitmap` 放大 | 脸糊成一团 | .NET 的 WPF **没有** `BitmapTransformOptions`（3 参构造不存在），`TransformedBitmap` 只能走 WIC 插值。改成自己逐像素铺 `WriteableBitmap`（`SkinRenderer.Face`），首页头像/纸娃娃同批修好 |
| 玻璃渐变端点 | 高光方向不对 | 原版 `createLinearGradient(-28,-30,32,35)` 是**碎片本地坐标**，不是包围盒比例 → `MappingMode.Absolute` |

### 8.4 头像来源

原版贴的是构建期烘好的 `assets/splash-face.png`（256×256，由 8×8 皮肤脸放大）。本项目按用户要求
所有二创头像一律现场从 `Assets/skin.png` 算（`SkinRenderer.Face(256)`），侧栏 `.brand-head`、
首页账号卡头像、开机碎块三处共用同一函数，皮肤重载时三处一起刷。

---

## 9. `prefers-reduced-motion` 抑制清单（要照抄这个开关）

**两层机制**：① CSS `@media (prefers-reduced-motion: reduce)`（只看系统）；② `html[data-motion="reduced"]`（**系统 OR 用户设置**，`App.vue:200`）。移植 WPF 时应以 **②** 为准，因为它包含用户设置。

| 位置 | 被禁用内容 |
|---|---|
| **styles.css:661-674** | 全局 `* , *::before, *::after`：`animation-duration:0.01ms !important; animation-delay:0ms !important; animation-iteration-count:1 !important; transition-duration:0.01ms !important; transition-delay:0ms !important; scroll-behavior:auto !important`。另：`.dl-bar.is-indeterminate .dl-bar-fill { animation:none !important; transform:none !important }`（**去掉不定态扫光**） |
| **ui-system.css:54-57** | `html[data-motion="reduced"] *, *::before, *::after`：同上六项（**这是含用户设置的那层**） |
| ui-system.css:120 | `html[data-motion="reduced"] details::details-content { transition:none }` |
| ui-system.css:121-124 | `details::details-content{transition:none}`；`.popover-enter-active,.popover-leave-active,.subpage-enter-active{transition:none}` |
| ui-system.css:50-52 | （非 reduce 块，但配合）`fade-leave-active{display:none}` |
| **App.vue:1859-1861** | `.nav-bubble, .nav-selection { transition:none }`（水滴不再滑动，直接跳位） |
| **App.vue:1926-1928** | `.nav-sub, .nav-sub .nav-sub-item { transition:none }` |
| HomeView.vue:826-828 | `.instance-switch-enter-active/leave-active { transition:none }` |
| Toasts.vue | **未找到** reduce 抑制（靠 styles.css 全局兜底） |
| LaunchNotice.vue:42 | `.launch-notice-enter/leave-active { transition:none }` |
| SelectMenu.vue:169 | `.select-menu-btn, .select-menu-chevron, .select-menu-option { transition:none }` |
| CreatorMotto.vue:70 | `.motto-glyph { transform:none !important; transition:none }`（+ JS 侧 `reduced.matches` 直接不写 transform，CreatorMotto.vue:24） |
| MarqueeText.vue:19 | `.overflow>span { animation:none }`；`.marquee { overflow-x:auto }`（**改为可手动横向滚动**） |
| InstanceCenter.vue:109 尾部 | `.ic-grid .ic-card { transition:none }` |
| VoxLinkPanel.vue:433 | `.lobby-item { transition:none }`；`.lobby-item:hover { transform:none }` |
| connection.css:111 | `.connect-page *, .connect-page *::after { transition:none !important }` |
| EditPanel.vue:74 尾部 | `* { scroll-behavior:auto !important }`（设计器工作区） |
| splash.css:19 | `body.leaving { transition-duration: 1ms }` |
| splash.ts:14,37,40 | 跳过漂浮/指针交互，直接贴完整图 |
| SplashScreen.vue:99,237-247 | 跳到 settle；`animation:none`、`transition:none`（board/tile/brand） |
| App.vue:203 `routeDuration` | `reducedMotion ? 0 : {enter:180, leave:80}` —— **JS 层把路由过渡时长压到 0** |
| motion.ts:12-13 | `decorativeActive = !hidden && !reduced`：皮肤 3D 自转（SkinViewer3D.vue:66）、创作者名单漂浮（CreatorCard.vue:14 `moving` 类）、轮播（HomeView.vue:99/133）全部停摆 |

---

## 10. 滚动 / 吸顶 / 其它

| 项目 | 值 | file:line |
|---|---|---|
| `scroll-behavior` | 全局**未声明**（默认 `auto`）；仅 reduce 时强制 `auto !important` | styles.css:668；ui-system.css:56；EditPanel.vue:74 |
| `scroll-padding` | **未找到** |
| `scroll-margin-top` | GameView `scroll-margin-top: var(--space-5)` = 16px；SettingsView `[data-section]` `150px`（窄屏 `16px`，最后又被覆盖为 `12px`）；`.setting-target` `var(--space-5)` | GameView.vue:1520；SettingsView.vue:1241,1244,1306,**1848（12px 最终生效）** |
| 吸顶：`.settings-navigation` | `position:sticky; top:0; z-index:15; background: var(--surface-solid)`；**无 transition/animation → 吸顶无动效**；≤800px 改 `position:relative` | SettingsView.vue:1233,1244 |
| 吸顶：`.fm-batch` | `position:sticky; top:0; z-index:6`；无动效 | FileManager.vue:491 |
| 吸顶：`.server-detail` | `position:sticky; top:0`（两处：全局 + `@media(min-width:1121px)`）；无动效 | ServersView.vue:488,529 |
| `.content` 滚动 | `overflow-y:auto; scrollbar-gutter: stable; padding: var(--space-5)`（≤1080px → `--space-4`） | App.vue:2398-2410 |
| `overscroll-behavior: contain` | `.ic-content`、`.version-picker-list`、`.versions-list`、`.supplement-list`、`.fm-result-list`、`.select-menu-float`、`.update-dialog-content` | InstanceCenter/MinecraftVersionPicker/ModVersionModal/ModpackSupplement/FileManager/SelectMenu:145/UpdateDialogShell:36 |
| `animation-play-state` | `html[data-visibility="hidden"] * { paused !important }`；`.marquee:hover>span, .marquee:focus>span { animation-play-state: running }`（**注意：源码写的是 `running`，且基态并未 `paused` → 该规则实际无暂停-恢复语义，属遗留**） | ui-system.css:53；MarqueeText.vue:17 |
| 进度条扫光 | 仅 `dl-indeterminate`（1.25s ease-in-out infinite，translateX -120%→320%） | App.vue:2291-2300 |
| 拖入遮罩动效 | **无** | App.vue:2419-2441 |
| 通知角标 `.bell-dot` / `.dl-badge` | **无 animation / transition** | App.vue:2121-2138 |
| 皮肤 3D 自转 | JS rAF：`seconds += dt`（仅 `decorativeActive` 时），`blend += ((walk?1:0)-blend)*min(1, dt*6)`（**时间常数 ≈1/6 s 指数逼近，非 cubic-bezier**） | SkinViewer3D.vue:66 |

---

## 11. 冲突规则汇总（同一元素两条规则）

| 元素 | 规则 A | 规则 B | 胜者 & 依据 |
|---|---|---|---|
| `--motion-fast` / `--motion-normal` | styles.css:62-63 (120/180ms) | ui-system.css:73 (130/190ms) | **B**（同 `:root` 特异度，main.ts 里 B 后导入） |
| `.btn-ghost:hover` | styles.css:225-230（border-strong / box-shadow none） | styles.css:783-787（@media hover：accent 10% 底 / border accent / shadow 0 3px 12px） | **B**（同特异度 0,3,0，B 源序更晚） |
| `.route-view.fade-enter-active` | styles.css:648（opacity 180 + transform 180 ease-out） | ui-system.css:51（`transition:opacity var(--motion-normal)` **单属性**） | **B**（0,3,0 > 0,1,0）→ transform 通道整体被移除 |
| `.route-view.fade-enter-from` | styles.css:650（`opacity:0; translateY(6px)`） | ui-system.css:52（`transform:none`） | **B 的 transform 胜出**；opacity 仍来自 A |
| `.route-view.fade-leave-active` | styles.css:649（opacity 80ms ease） | ui-system.css:50（`display:none`） | **B**（`display:none` 直接离场，过渡无效） |
| `.instance-card:hover` transform | HomeView.vue:898（`translateY(-1px)`） | ui-system.css:26（`.content .instance-card:hover{transform:none}`） | **B**（0,3,0 > 0,2,0） |
| `.instance-card:hover` box-shadow | HomeView.vue:898（`var(--shadow)`） | HomeView.vue:1030（`.home-dashboard .instance-card{box-shadow:none}`） | **1030**（同 0,2,0，源序更晚） |
| `.instance-card:hover` background | HomeView.vue:898（`--card-2`） | HomeView.vue:1030（`.instance-card:hover{--hover}`） | **1030**（更晚） |
| `.instance-card` 布局 | HomeView.vue:896（132px 高网格卡） | HomeView.vue:1030（72px 高横排行） | **1030**（更晚，卡片实为列表行） |
| `.hero-shade` 渐变 | HomeView.vue:815（90deg 双层 + 0deg） | HomeView.vue:1030（180deg 三段） | **1030** |
| `.server-list-item` hover/active | ServersView.vue:472-473（translateY/scale + shadow） | ServersView.vue:510（`transform:none; box-shadow:none`） | **510**（同特异度、更晚） |
| `.server-list-item` animation | ServersView.vue:463 + 464-470（0.38s + 40ms 错落） | ServersView.vue:510（`animation:none`） | **510** → 入场完全失效 |
| `.pick-card` | FriendConnectView.vue:135 + 137-140（animation + 60ms 错落） | FriendConnectView.vue:161（`animation:none; box-shadow:none`） | **161** → 入场/阴影失效 |
| `.pick-card:hover .pick-icon` | FriendConnectView.vue:151（`scale(1.08)`） | FriendConnectView.vue:161（`transform:none`） | **161** |
| `.result-card` hover transform | CommunityView.vue:1013-1022 系 | ui-system.css:46（`.content :is(.result-card,...):hover{transform:none}`） | **ui-system.css:46**（0,4,0） |
| `.game-tabs-blob` 外观 | GameView.vue:1881-1891（accent-grad 胶囊 + 外阴影） | GameView.vue:2168（radius-sm + accent-soft + `inset 0 -2px accent` 下划线） | **2168**（同特异度、更晚） |
| `.lobby-item:hover` transform | VoxLinkPanel.vue:432（`translateY(-2px)`） | VoxLinkPanel.vue:434（`transform:none`） | **434** |
| `.card` 过渡 | styles.css:456（border-color + background 180ms） | ui-system.css:13（background-color / box-shadow none） | 两者叠加：A 的过渡通道仍生效，B 提供最终背景色 |
| `[data-section] scroll-margin-top` | SettingsView.vue:1241（150px）→ 1244（≤800px 16px） | SettingsView.vue:1848（**12px**） | **1848** |
| `.content .cfg-sections button` | KeysView.vue:229（border-radius:14px, padding 18px 20px, bg `--card`） | ui-system.css:30（`padding:12px 16px; border-radius:var(--radius-md)`） | **ui-system.css:30**（0,2,0 + `.content`） |

---

## 12. WPF 落地映射

### 12.1 缓动 → KeySpline 数值对照表

WPF `KeySpline.ControlPoint1/ControlPoint2` 是 `Point`，**X 与 Y 都必须落在 [0,1]**，因此 CSS 中 Y 超出 1 的"过冲/弹簧"曲线**无法**用单条 KeySpline 表达（见 12.4）。

| CSS 缓动 | 出现处 | KeySpline（可直接用） |
|---|---|---|
| `ease` | 绝大多数 transition（styles.css、App.vue、各 View） | `0.25,0.1 0.25,1` |
| `linear` | `.spin`、`animation: spin ... linear` | `0,0 1,1`（或 `TimeWarp`/`LinearGradient`，直接用 `EasingMode` 无；`DoubleAnimation` 默认即线性） |
| `ease-in-out` | `dl-indeterminate`、`drift`、`readable-scroll`、marquee | `0.42,0 0.58,1` |
| `var(--ease-out)` = `cubic-bezier(0.2, 0.7, 0.3, 1)` | modal-in、launch-combo、runtime-blob、game-tabs-blob、capsule-blob、nav-sub、upd-progress-bar、fade-enter、switch-ui::before、subpage/popover translate | `0.2,0.7 0.3,1` ✅ |
| `cubic-bezier(.22, 1, .36, 1)` | `.nav-bubble` / `.nav-selection` | `0.22,1 0.36,1` ✅（Y 均 ≤1） |
| `cubic-bezier(0.6, 0.05, 0.2, 1)` | SplashScreen `.tile` transition | `0.6,0.05 0.2,1` ✅ |
| `cubic-bezier(0.22, 0.9, 0.32, 1)` | `server-row-in`（已失效，可不做） | `0.22,0.9 0.32,1` ✅ |
| `cubic-bezier(0.22, 0.9, 0.32, 1.15)` | `.runtime-item` | ❌ Y=1.15 → 用 `BackEase{EasingMode=EaseOut, EaseAmount≈0.15}` 或多加一个过冲 KeyFrame |
| `cubic-bezier(0.22, 0.9, 0.32, 1.2)` | `.pick-icon`、`.pick-go`、`.server-monogram` | ❌ Y=1.2 → `BackEase EaseAmount≈0.2` / 关键帧过冲 |
| `cubic-bezier(0.3, 1.2, 0.4, 1)` | `.seg-blob` | ❌ Y=1.2 → 同上（注意 CP1.X=0.3、CP2.X=0.4 均在 [0,1]，仅 Y 越界） |
| `cubic-bezier(.2, .9, .25, 1.3)` | `.motto-glyph` | ❌ Y=1.3 → `BackEase EaseAmount≈0.3` |
| `cubic-bezier(0.34, 1.56, 0.64, 1)` | `splash-pop` | ❌ Y=1.56 → `BackEase EaseAmount≈0.56` 或三帧 scale 1→1.06→1（与 keyframes 定义重复，建议**直接按关键帧做**） |
| smootherstep `t²t²(3.5t…)` 即 `t*t*t*(t*(t*6-15)+10)` | `glassPosition` 汇聚 | ❌ 非 cubic-bezier。用 `DoubleAnimationUsingKeyFrames` + 多段 KeySpline 逼近，或 `EasingMode=EaseInOut` 的 `QuinticEase`（WPF `QuinticEase` = t⁵，最接近） |
| **未找到缓动**（`transition: background 180ms, border-color 180ms` 无 easing） | DefaultGameOptions.vue:80、KeysView.vue:229、HomeLayoutEditor.vue:496、VoxLinkPanel.vue:431、InstanceCenter.vue:109、ReferenceLinks.vue:14、VoxLinkRelatedLinks.vue:20、ui-system.css:45 | CSS 默认 `ease` → `0.25,0.1 0.25,1` |

### 12.2 逐动效 → WPF 机制

约定：所有 hover/press 用 `Style.Triggers` + `EnterActions/ExitActions` 里的 `BeginStoryboard`；transform 一律走 `RenderTransform`（不改布局）。

| 动效 | WPF 实现 | 关键代码要点 |
|---|---|---|
| `mask-in`（遮罩淡入 200ms ease / 130ms ease-out） | `Storyboard` + `DoubleAnimationUsingKeyFrames` on `(UIElement.Opacity)`：`0→1`，`KeyTime 0s / <end>`，`LinearKeyFrame` + `KeySpline="0.25,0.1 0.25,1"` | 遮罩做成独立 `Border`（`Opacity` 动画），`BackdropFilter` 见 12.4 |
| `modal-in`（220ms，`opacity 0→1` + `translateY(6px)→0` + `scale 0.99→1`） | 同一 Storyboard 三条：`(UIElement.Opacity)`、`(UIElement.RenderTransform).(TranslateTransform.Y)` `6→0`、`(UIElement.RenderTransform).(ScaleTransform.ScaleX/ScaleY)` `0.99→1`；`RenderTransformOrigin="0.5,0.5"`；`KeySpline="0.2,0.7 0.3,1"`；`FillBehavior=Stop` | `translateY` 用 `TranslateTransform`，**scale 必须配 `RenderTransformOrigin`**（CSS 默认 50% 50%，WPF `RenderTransformOrigin` 是相对 0..1，等价 `0.5,0.5`） |
| `.float-menu` 160ms `modal-in` | 同上，`Duration=0:0:0.160` | |
| `.spin` / `.spinning`（`rotate 0→360`, 800ms linear infinite） | `DoubleAnimationUsingKeyFrames` on `(UIElement.RenderTransform).(RotateTransform.Angle)` `0→360`，`RepeatBehavior="Forever"`，`KeySpline="0,0 1,1"`（或直接 `DoubleAnimation` + `Duration 0:0:0.8` + `RepeatBehavior.Forever`） | CSS `to{}` 无 `from` → 从当前值(0)开始 |
| `dl-indeterminate`（1.25s `translateX(-120%→320%)` ease-in-out infinite） | 父 `Border` `ClipToBounds=True`；子 `Rectangle` 上 `(UIElement.RenderTransform).(TranslateTransform.X)`，但 **-120%/320% 是相对自身宽度** → 用 `ScaleX`/`TranslateTransform` + 绑定 ActualWidth：`From={-0.12*W}`、`To={3.2*W}`（或用 `ThicknessAnimation` on `Margin.Left`） | ⚠️ WPF `RenderTransform` **不支持百分比**，必须在 `SizeChanged` 时重算并重启 Storyboard。`KeySpline="0.42,0 0.58,1"`、`RepeatBehavior=Forever` |
| `bridge-pulse`（2.2s，box-shadow 环 3px→6px） | WPF 无 box-shadow 动画。做法：叠加一个 `Ellipse`（`Stroke` 半透明绿），动画 `(UIElement.Opacity)` 与 `(ScaleTransform.ScaleX/Y)`（模拟 3px→6px 环宽：`Scale` = 6/3 = 2 与 1 之间往返），`AutoReverse=True`、`RepeatBehavior=Forever`、`Duration 0:0:1.1`（半周期，凑成 2.2s 往返）、`KeySpline="0.42,0 0.58,1"` | 精确度说明：CSS 用 `0%/50%/100%` 三点；WPF 用 `AutoReverse` 等价 |
| `card-in` / `community-card-in`（220ms `opacity 0→1`，`backwards`） | `DoubleAnimationUsingKeyFrames` on `Opacity` `0→1`，`Duration 0:0:0.220`，`KeySpline="0.2,0.7 0.3,1"`；`backwards`（fill-mode）→ WPF 用 `Storyboard.SetTargetProperty` + 元素初始 `Opacity=0`，`BeginTime=0`（若需延时则 `BeginTime` 设值并保持 Opacity=0） | 首页卡片**无错落**，全部同时开始（`HomeView.vue:895` 注释） |
| `pick-card-in` / `server-row-in` | **源码已 `animation:none`，不要实现** | 保持"无入场"才是 100% 还原 |
| `reveal` + `drift`（创作者名单气泡） | ①`Opacity 0→1` + `TranslateY 4→0`，`Duration 0.22s`，`BeginTime = TimeSpan.FromMilliseconds(i*40)`（**错落 40ms**），缓动 `ease`（CSS 未声明 timing → 默认）；②`(TranslateTransform.X/Y)` 在 `±driftX/±driftY` 间往返：`Duration = 3.8+rand*3 s`、`AutoReverse=True`、`RepeatBehavior=Forever`、`KeySpline="0.42,0 0.58,1"`、`BeginTime = TimeSpan.FromMilliseconds(i*40)` | ⚠️ `drift` 的 `from` 是 **负值**、`to` 是正值，WPF 里等价 `From=-dx, To=+dx` + `AutoReverse`；但 CSS `alternate` 与 WPF `AutoReverse` 在 `animation-delay:40ms` 的相位上不同 → 若要完全一致，`From=-dx→To=+dx` 为第一个 half-cycle，然后反向（WPF `AutoReverse` 行为一致）。`driftX/Y` 是**每块随机**（±6px / ±4px），一次性生成后固定 |
| `readable-scroll`（跑马灯） | `DoubleAnimationUsingKeyFrames` on `(TranslateTransform.X)`：`LinearDoubleKeyFrame 0 @0%`、`0 @18%`、`-distance @65%`、`-distance @82%`、`0 @100%`；`Duration = max(5, distance/32+3)s`、`RepeatBehavior=Forever`；段间 `KeySpline="0.42,0 0.58,1"`（CSS 的 `ease-in-out` 作用于**每段**之间） | ⚠️ CSS 单条 `ease-in-out` 会被插到 0%→18%(不动)、18%→65%(移动)、65%→82%(停)、82%→100%(回到 0)。逐段 `KeySpline` 精确复刻 |
| `splash-wobble-a/b` | `DoubleAnimationUsingKeyFrames` × 2（X、Y on `TranslateTransform` + `RotateTransform.Angle`），`AutoReverse=True`、`RepeatBehavior=Forever`、`Duration = (420+rand*180)/2`、`BeginTime=0`；**负 delay 用相位偏移替代**：`From` 直接取该时刻的插值位置（WPF `BeginTime` 不可为负） | ⚠️ CSS 用的是独立属性 `translate`/`rotate`（可与 `transform` 叠加）；WPF 需 `TransformGroup{TranslateTransform, RotateTransform}` 合并，与 `tileStyle` 的 `transform: translate(dx,dy) rotate(rot)` 叠加 → **两个通道在 WPF 里会互相覆盖**，必须把 scatter 位移写进 `TranslateTransform`，wobble 写进**第二个** `TransformGroup` 子节点 |
| `splash-pop` | `DoubleAnimationUsingKeyFrames` on `(ScaleTransform.ScaleX/ScaleY)`：`1 @0s`、`1.06 @0.208s(40%)`、`1 @0.52s`，段 `KeySpline` 见 12.4（Y=1.56 越界 → 用额外 overshoot 关键帧或 `CircleEase`） | |

| transition 类 | WPF 实现 |
|---|---|
| `.nav-bubble` / `.nav-selection` | `Canvas.SetLeft/Top` 改为 `(TranslateTransform.X/Y)` 动画（**CSS 用 `translate3d(x,y,0)` + width/height**）。Storyboard 4 条：`(TranslateTransform.X)`、`(TranslateTransform.Y)` 各 300ms `0.22,1 0.36,1`；`(FrameworkElement.Width)`、`(FrameworkElement.Height)` 同 300ms 同曲线；`(UIElement.Opacity)` 120ms `0.25,0.1 0.25,1`。测量：`TransformToVisual(navHost)` 取 `Bounds`，累加 `ScrollViewer.HorizontalOffset/VerticalOffset` | 
| `.nav-bubble` 的 240ms 连续重测 | 用 `CompositionTarget.Rendering` 订阅，直到 `now-start > 240ms` 停止；期间每帧更新目标值（不要重启 Storyboard，用 `Animation.SetCurrentValue` 或直接写 `X/Y`） |
| `.nav-selection` 左侧 3px 强调条（`box-shadow: inset 3px 0 0 var(--accent)`） | `Border.BorderThickness="3,0,0,0"` + `Border.Brush`，或内部 `Rectangle Width=3` 左对齐 |
| `.nav-sub` 展开（`grid-template-rows 0fr→1fr`, 190ms ease-out + opacity 220ms + margin 190ms） | CSS Grid 内在尺寸动画在 WPF 无对应。做法：外层 `Border` 高度绑定测量得到的内容高度，`DoubleAnimationUsingKeyFrames` on `(FrameworkElement.Height)` `0→measured`，`Duration 0:0:0.19`、`KeySpline="0.2,0.7 0.3,1"`；同 Storyboard `(UIElement.Opacity)` `0→1` `0:0:0.22` `ease`；`(FrameworkElement.Margin)` 用 `ThicknessAnimation` `0→8,0`（`--space-1`=4px 上下 → `4,0,4,0`？源码 `margin: var(--space-1) 0` = `4px 0`）`0:0:0.19` ease-out |
| `.nav-sub-item` 淡入（`opacity 130ms ease` + `translateY(-4px→0) 190ms ease-out`，**`transition-delay:0s` 显式清零**） | `Opacity` 130ms `ease`；`(TranslateTransform.Y)` `-4→0` 190ms `0.2,0.7 0.3,1`；`BeginTime=0`（**不要错落**，源码注释 App.vue:1895 明确禁止） |
| `.nav-caret`（`transform 0.18s ease` → `rotate(90deg)`） | `(RotateTransform.Angle)` `0→90`，`0:0:0.18`，`0.25,0.1 0.25,1`，`RenderTransformOrigin="0.5,0.5"` |
| `.runtime-blob` | 两条布局属性动画 `(Canvas.Left)`/`(FrameworkElement.Width)` 190ms `0.2,0.7 0.3,1` **或** 更优：`TranslateTransform.X` 做位移 + 只动画 `Width`（CSS 里 `left` 会触发布局，视觉等价）。另加 `(UIElement.Opacity)` 180ms `ease`、`(ScaleTransform.ScaleX/Y)` `0.97→1` 200ms `ease`（`RenderTransformOrigin="0.5,0.5"`）。触发：`MouseEnter` on 三个 item → 重新计算并 `Storyboard.Begin(this, true)`（`handoffBehavior=Snapshot` 保证从当前位置继续滑） |
| `.runtime-item` hover | `(UIElement.Opacity)` 无；只改 `Foreground` → `ColorAnimation`（WPF 需动画 `SolidColorBrush.Color`：`(Control.Foreground).(SolidColorBrush.Color)`，180ms；源码声明的 transform 通道 hover 未改变 → 不实现 |
| `.hero-image` 交叉淡入 | `(UIElement.Opacity)` `0↔1`，`Duration 0:0:0.5`，`KeySpline="0.25,0.1 0.25,1"`；每张一个 `Image`，`Grid` 堆叠，`Panel.ZIndex` 不用（纯 alpha 混合）；`RenderOptions.BitmapScalingMode="Unspecified"` 保持 `objectFit` 语义由 `Image.Stretch` 承担 |
| 轮播时钟 | `DispatcherTimer` `Interval=100ms`（`HomeView.vue:139`）；`CarouselPlayback` 直接照抄为 C# 类（`due = now + durationMs`、`peekNext`、`bookmark`）；`ApplicationCommands`/`VisibilityChanged` 时 `Pause`/保存 `remainingMs`；默认 `6.5s`、`clamp(1,120)` |
| `launch-combo` hover/press | hover：`(TranslateTransform.Y)` `0→-2` `0:0:0.19` `0.2,0.7 0.3,1` + `(UIElement.Effect)`/阴影换 `DropShadowEffect.BlurRadius/Opacity/Color`（WPF 阴影动画只能动 `DropShadowEffect` 属性，`BlurRadius 32→40`、`Opacity 0.38→0.46`），`Duration 0:0:0.22` `ease`；press（`IsPressedEvent` 或 `PreviewMouseDown`）：`(TranslateTransform.Y)→0` + `(ScaleTransform.ScaleX/Y)→0.99`。⚠️ CSS 的 `inset 0 2px 0 white14%` 内阴影 WPF **无原生支持** → 用一个 2px 高的顶层 `Rectangle`（`Fill=#26FFFFFF`）静态模拟 |
| `.btn` / `.icon-btn` 按压 | `(ScaleTransform.ScaleX/Y)`：`.btn*` `→0.985`（transform 通道 140ms ease）、`.icon-btn` `→0.93`（140ms）、`.theme-preview` `→0.98`（120ms）、`.mirror-toggle` `→0.96`（120ms）、`.fav-btn` `→0.9`（120ms）、`.edit-tip-done` `→0.96`（120ms）。`RenderTransformOrigin="0.5,0.5"`，`EnterActions/ExitActions` 成对，`ExitActions` 回 1 |
| 通用按钮 hover `filter: brightness(1.08)` | WPF 无 CSS `filter`。做法：`IsMouseOver` 时切 `Background` 为 `OverlayBrush`，或叠一个 `Rectangle Fill="#14FFFFFF"`（= 8% 白）做 `Opacity` 动画；`Duration 0:0:0.18` `ease`。`.mirror-toggle` 的 `brightness(1.15)` → `#26FFFFFF` |
| `.dl-bar-fill` / `.launch-progress` / `.row-bar-fill` / `.upd-progress-bar` / `.java-scan-track span` / `.mem-slider-fill` | `DoubleAnimationUsingKeyFrames` on `(FrameworkElement.Width)`（或 `ScaleTransform.ScaleX` + `RenderTransformOrigin="0,0.5"` 更省布局）；时长依次 `300ms ease` / `250ms ease` / `250ms ease` / `190ms 0.2,0.7 0.3,1` / `150ms ease` / `120ms ease`。用 `SnapshotAndReplace` + 从当前值起动画，避免抖动。⚠️ `.mem-slider-fill` 在拖动手柄时 `transition:none` → 拖动期间**直接赋值不动画** |
| `.switch-ui::before`（旋钮） | `(TranslateTransform.X)` `0→18` + `(Y)` 固定 `-7`（CSS `translateY(-50%)` 是百分比居中 → WPF 用 `VerticalAlignment=Center` 代替，不要动画 Y）；`Duration 0:0:0.19` `0.2,0.7 0.3,1`；同时 `ColorAnimation (Shape.Fill).(SolidColorBrush.Color)` `text-dim→on-accent` `0:0:0.19` `ease`；轨道 `Background`/`BorderBrush` 各 190ms `ease` |
| `.connection-toggle-track::after` | `(TranslateTransform.X)` `0→14`，`0:0:0.16` `ease`；轨道 `background 160ms ease` |
| `.collapse-arrow` / `.select-menu-chevron` / `.launch-arrow svg` / `.layout-setting>summary::after` | `(RotateTransform.Angle)`：`-45→45`（SettingsView 1384，180ms ease）、`0→180`（SelectMenu 129，180ms ease）、`0→180`（HomeView 857，180ms ease）、`0→180`（HomeLayoutEditor 496，180ms，缓动**未找到**→`ease`） |
| `.slider::-webkit-slider-thumb` | WPF 需 `ScrollBar`/`Slider` 的 `ControlTemplate`；在 thumb `IsMouseOver` 触发 `(ScaleTransform)` `1→1.2`，`0:0:0.15` `ease` |
| `.mem-slider-thumb` hover/drag | `(CompositeTransform)` 等价：`TranslateTransform.X=-8,Y=-8`（`translate(-50%,-50%)` 是百分比）→ 改用 `HorizontalAlignment/VerticalAlignment=Center` + `Margin="-8,-8,0,0"`，只动画 `(ScaleTransform)`：hover `1.12`、dragging `1.18`，`0:0:0.12` `ease` |
| `.menu-item` / `.select-menu-option` hover | `ColorAnimation` on `(Border.Background).(SolidColorBrush.Color)` + `(TextElement.Foreground).(SolidColorBrush.Color)`；`0:0:0.14` / `0:0:0.12`，`ease` |
| `:focus-visible` 焦点环（`box-shadow: 0 0 0 3px var(--accent-soft)` / `outline 2px + offset 3px`） | 无原生 `box-shadow` 环。做法：外层 `Border BorderThickness=3` + `Opacity 0→1` 动画（`ui-system.css:113` 的 `box-shadow 220ms ease`），`outline-offset:3px` → 该 `Border` 用 `Margin=-3`；`outline-color` 通道 180ms `ease` |
| `ui-system.css:100-109 @starting-style` 淡入组（20 个选择器，`opacity 200ms ease`） | WPF **无 `@starting-style`**。做法：元素 `Loaded` 事件里 `Opacity=0` → `BeginStoryboard` 到 `1`，`Duration 0:0:0.2` `ease`；用附加属性 `AttachedBehaviors.FadeOnLoad="True"` 统一挂到那 20 个类对应控件 |
| `ui-system.css:82-91 details::details-content`（`block-size 220ms var(--ease-out)`, `opacity 180ms ease`, `content-visibility 220ms allow-discrete`, `interpolate-size: allow-keywords`） | WPF **完全无对应**：CSS 能 `block-size: 0 → auto`（内在尺寸插值）。必须 JS 式测量：展开前 `Measure` 求 `DesiredSize.Height` → 动画 `Height` `0→H` `0:0:0.22` `0.2,0.7 0.3,1`；`Opacity 0→1` `0:0:0.18` `ease`；折叠后 `Visibility=Collapsed`（等价 `allow-discrete`）。`.content .instance-card:hover{transform:none}` 之类抑制同样照抄 |
| `ui-system.css:53 html[data-visibility=hidden] * {animation-play-state:paused}` | WPF：`Window.VisibilityChanged` → 对全局 `Storyboard` 调 `Pause()`/`Resume()`；或维护一个 `StoryboardRegistry`。Canvas 类（碎玻璃）改为停掉 `CompositionTarget.Rendering` 处理器 |
| `ui-system.css:54-57 html[data-motion=reduced] *` | WPF：一个静态 `MotionPrefs.Reduced` + `MotionDuration(ms)` 辅助方法，`reduced ? TimeSpan.FromMilliseconds(0.01) : TimeSpan.FromMilliseconds(ms)`；且 `BeginTime` 归零、`RepeatBehavior=Count(1)`、`SmoothScroll=false`。用**用户设置 OR 系统偏好**（`App.vue:199`），不要只看 OS |
| `App.vue:203 routeDuration = reduce ? 0 : {enter:180, leave:80}` | 路由切换：`enter` Storyboard `Duration 0:0:0.18`（实际按 ui-system.css:51 → **190ms**，因为 CSS 的 transition 才是视觉时长，`:duration` 只是 JS 超时兜底 → **WPF 里两者合一，取 190ms**）；`leave` 不动画，直接 `Visibility=Collapsed`（等价 `display:none`） |
| `<Transition name="fade">` 的 `transform:none` 覆盖 | ⚠️ 首页路由切换**只有纯淡入**，**没有** 6px 上移（被 ui-system.css:52 抹掉了）。若照 styles.css:650 实现 `TranslateY(6px)` 就是还原错误 |
| `<TransitionGroup name="toast">` `.toast-move` | WPF `TransitionGroup` 无对应：`ObservableCollection` 变化时，对每个存活 toast 用 `(TranslateTransform.Y)` 从旧位置动画到新位置，`Duration 0:0:0.22` `ease`（即 FLIP）；新增：`Opacity 0→1` + `X 24→0`；移除：`Opacity 1→0` + `Y 0→-8`。容器 `IsHitTestVisible=False`（等价 `pointer-events:none`），关闭按钮 `IsHitTestVisible=True` |
| `<Transition name="launch-notice">` | 基态已含 `TranslateX(-50%)`：WPF 用 `HorizontalAlignment=Center`（等价），进出动画只做 `(TranslateTransform.Y)` `-6→0` 与 `Opacity 0→1`，180ms `ease`；`DispatcherTimer` 3200ms 自动关 |
| `<Transition name="instance-switch">`（`mode="out-in"`） | **串行**：`Opacity 1→0` + `Y 0→-2`（100ms）→ 完成回调 → `Opacity 0→1` + `Y 2→0`（100ms）。总 200ms。用两个 Storyboard + `Completed` 事件，或单条 `DoubleAnimationUsingKeyFrames`：`Y: 0@0s, -2@0.1, 2@0.1, 0@0.2`；`Opacity: 1@0s, 0@0.1(swap content), 0→1@0.1-0.2` |
| `<Transition name="popover">` | `(TranslateTransform.Y)` `-5→0` 200ms `0.2,0.7 0.3,1` + `Opacity 0→1` 180ms `ease`；离开时 `IsHitTestVisible=false`（`pointer-events:none`） |
| `<Transition name="subpage">` | `(TranslateTransform.Y)` `6→0` + `Opacity 0→1`，均 200ms；离开 `Collapsed`（无动画） |
| `CreatorMotto` 字符弹簧 | **不用 Storyboard**。在 `MouseMove` 里对每个 `TextBlock` 写 `TranslateTransform.X/Y` + `RotateTransform.Angle`，靠 `DoubleAnimation(UsingKeyFrames)` 的 "当前值 → 目标值, 360ms" 平滑。**实测结论**：每帧 `BeginAnimation` 重投不抖——`GetValue` 读到的就是"当前动画值"，用它当 `from` 等价于 CSS 重启动画；`reset()` 同样投回 `0` 而不是清零。原文担心的 `AnimationClock` 保活不需要 |
| Splash 碎玻璃（canvas） | WPF：`DrawingVisual`/`OnRender` + `CompositionTarget.Rendering`，逐帧 `PushTransform`（`TranslateTransform` → `RotateTransform` → `ScaleTransform` 顺序即 CSS `translate→rotate→scale`）；`ImageBrush` 按三角 `GeometryClip` 裁切；`globalAlpha` → `Brush.Opacity`；tint 用 `LinearGradientBrush(StartPoint/EndPoint)` 归一化坐标 (−28/w,−30/h)→(32/w,35/h)。`smoothstep` 自己实现。**不要用 Storyboard**（72 块 × 位姿连续依赖指针） |
| Splash 退场 | 窗口级 `DoubleAnimation` on `Window.Opacity` `1→0`，`Duration 0:0:0.32`，`ease`；`Completed` → `Close()`（等价 `transitionend`） |

### 12.3 数值换算备忘

- CSS `scale(0.985)` → WPF `ScaleX=ScaleY=0.985`（需 `RenderTransformOrigin="0.5,0.5"`，等价 CSS `transform-origin: 50% 50%`）。
- CSS `translateY(-50%)` / `translateX(-50%)` / `translate(-50%,-50%)`：WPF `RenderTransform` **无百分比语义** → 用 `HorizontalAlignment`/`VerticalAlignment` 或 `Canvas.Left = (Container.ActualWidth - Child.ActualWidth)/2` 绑定实现，**不要**放进动画。
- CSS `transition-delay: 0s`（App.vue:1919/1924）是**显式禁用错落**的信号 → WPF 里 `BeginTime` 必须为 0，别自作主张加 40ms。
- CSS `animation-fill-mode: backwards`（4 处入场动画）= 动画开始前应用 `from` 帧 → WPF：元素初始状态就设成 `from` 值（`Opacity=0` 等），动画 `FillBehavior=Stop`。
- CSS `animation-fill-mode: both`（CreatorCard `reveal`）→ WPF `FillBehavior=HoldEnd` + 初始值取 `from`。
- CSS `animation-direction: alternate` + `infinite` → `AutoReverse="True"` + `RepeatBehavior="Forever"`（WPF 的 `Duration` 是**半个往返**，CSS 的 `animation-duration` 也是半周期 for alternate → **可直接同值**）。
- CSS `cubic-bezier` 的 `x` 越界会抛错，WPF `KeySpline` 的 `x` 同样必须在 [0,1]（本表所有 `x` 都合法）。

### 12.4 无法在 WPF 直接 1:1 复现的项（需决策）

1. **Y 超出 [0,1] 的过冲 cubic-bezier**：`cubic-bezier(0.34,1.56,.64,1)`、`(0.22,.9,.32,1.15)`、`(0.22,.9,.32,1.2)`、`(0.3,1.2,0.4,1)`、`(.2,.9,.25,1.3)`。**实测更正**：WPF `KeySpline` 只校验**两个控制点的 X 必须落在 [0,1]**，Y 越界是允许的，`new KeySpline(new Point(.2,.9), new Point(.25,1.3))` 直接可用并确实产生过冲（`CreatorMotto` 已按此实现并截图验证），不需要 `BackEase` 近似。涉及：`splash-pop`、`.runtime-item`、`.pick-icon/.pick-go`、`.server-monogram`、`.seg-blob`、`.motto-glyph`。
2. **`backdrop-filter: blur(...) saturate(...)`**：`.modal-mask blur(10px)`、`.float-menu/.notice-panel/.drop-mask blur(24px) saturate(130%)`、`.runtime-strip blur(18px) saturate(130%)`、`.content .card blur(16px) saturate(85%)`、`.hero-kicker blur(12px)`、`.skin-overlay blur(12px)`、`.launch-success blur(24px) saturate(130%)`、`.capsule` 系。WPF **完全没有背景模糊**。必须换 `SystemThemedWindow`/`DwmEnableBlurBehindWindow`/Win2D `GaussianCompositionEffect`，或降为半透明纯色（视觉有损）。且这些 blur **不参与任何 transition**（只有 `@starting-style` 的 opacity 淡入，模糊层是跟着 opacity 一起淡入的）。
3. **`color-mix(in srgb, X p%, Y)`**：hover/背景/阴影大量使用（styles.css、HomeView、CommunityView、VoxLinkPanel、App.vue JS）。WPF 无该函数 → 必须在主题应用点（对应 `App.vue:960-1012 applyCustomVars`）预先算出最终 `Color` 再写进资源字典；hover 态颜色也要预算，不能运行时 mix。
4. **`box-shadow` 动画**：`.dl-bar.is-indeterminate` 无阴影动画，但 `bridge-pulse`（0%/50%/100% 的 `box-shadow` 环扩张）、`.launch-combo` hover 阴影、`.btn-gold`/`.btn-danger` hover 阴影、`.instance-card:hover`、`.result-card:hover`、`.server-list-item:hover`、`.pick-card:hover` 都动 `box-shadow`。WPF 只有 `DropShadowEffect`（可动画 `BlurRadius`/`Opacity`/`Color`/`ShadowDepth`/`DirectionX/Y`），**不支持多段阴影（如 `0 2px 6px soft, 0 4px 16px soft`）与 `inset` 内阴影**。内阴影出现处：`inset 3px 0 0 var(--accent)`（App.vue:1855、ServersView:474）、`inset 0 0 0 1px var(--accent)`（HomeView:899）、`inset 0 -2px var(--accent)`（GameView:2168）、`inset 0 0 0 3px var(--card), inset 0 0 0 8px var(--accent)`（App.vue:2520）、`inset 0 1px 0 rgba(255,255,255,.12)`（App.vue:1735）→ 全部改用 `Border.BorderThickness` + 子 `Rectangle` 或 `Border.Background` 渐变模拟，且这些**都不是动画**（只有产生它们的那个 transition 通道 200/220ms 需要保留近似）。
5. **`filter: brightness()/saturate()`**（styles.css:207,781、HomeView:854,856、GameView:1863、SettingsView:1660）：WPF 无元素级滤镜。只能叠半透明白/黑层或换 `Brush`，`.launch-main:disabled { filter: saturate(0.75) }`（HomeView:854）需要把 `--accent-grad` 换成预饱和降级版本。注意 `.btn` 有 `filter 0.18s ease` 通道而 `.icon-btn` **没有** → 移植时别给 icon-btn 加淡入。
6. **CSS 独立变换属性 `translate` / `rotate` / `scale`**（ui-system.css:93,96 `translate:0 -5px`/`0 6px`；SplashScreen `.tile` 的 `translate`+`rotate` transition；HomeLayoutEditor `rotate 180ms`；visualDesign.ts 写 `translate`）：WPF 只有一个 `RenderTransform` 通道，`translate` 与 `transform` 在 CSS 里可叠加/可分别过渡（两条独立 transition），WPF 必须拆成 `TransformGroup` 的两个子节点并分别动画 → **叠加顺序与语义不同**，`translate` 与 `transform:translateX(-50%)` 共存处（LaunchNotice `.launch-success` + `launch-notice-*` transform）需特别注意：CSS 里 `translate` 独立属性与 `transform` 互不覆盖，WPF 会覆盖。
7. **`grid-template-rows: 0fr → 1fr` 内在尺寸动画**（`.nav-sub` App.vue:1897-1907、`.creator-reveal` CreatorCard:29）与 **`interpolate-size: allow-keywords` / `::details-content block-size:0→auto`**（ui-system.css:82-91）：WPF 没有 Grid 行 `0fr/1fr` 插值，也没有内在尺寸动画 → 必须先 `Measure` 出目标像素高度再动画 `Height`（见 12.2）。
8. **`@starting-style`**（ui-system.css:105-109）：WPF 无"首帧起始样式"概念，用 `Loaded` + 手动初始值模拟。
9. **百分比位移**（`dl-indeterminate` 的 `-120%`/`320%`；所有 `translate(-50%,-50%)`/`translateX(-50%)` 居中）：WPF `TranslateTransform` 只接受像素。
10. **负 `animation-delay`**（SplashScreen `wobbleDelay = -Math.random()*500`，SplashScreen.vue:61,84）：`Storyboard.BeginTime` 不能为负 → 只能在启动时把每块动画的**相位**预先推进 `|delay|` 毫秒（取插值中间态作起点）。
11. **`animation-play-state` 全局 `paused` / `running`**（ui-system.css:53；MarqueeText:17）：WPF 无统一开关，需要集中登记所有 `Storyboard` 才能 `Pause()/Resume()`；`MarqueeText` 的 `running` 规则本身在源码里是空操作（基态从未 `paused`），不必移植。
12. **`::-webkit-scrollbar*` 伪元素**（styles.css:139-155）：WPF 必须重写的 `ScrollBar` `ControlTemplate`；`thumb:hover` 颜色切换在源码**无 transition** → 应保持瞬时切换以求一致。`scrollbar-gutter: stable`（App.vue:2402）需靠预留右侧 8px 手工实现。
13. **`prefers-reduced-motion` 的 `!important` 全局压缩到 `0.01ms`**：WPF 没有层叠/`!important`，必须在动画构造点统一走 `MotionDuration()` 辅助函数；`0.01ms` 而非 `0` 是刻意的（保证仍派发一次 `transitionend`/`AnimationCompleted` 回调，`splash.ts:27` 的 `bridge.finished()` 就靠它）→ WPF 里若压到 `TimeSpan.Zero` 会破坏依赖 `Completed` 的收尾逻辑，务必用极小非零时长。
14. **`smooth` 逐帧依赖指针的物理模拟**（`advanceBootGlass`：`drag = exp(-5*dt)`、弹簧回复系数 8、斥力 `2400*(1-d/140)²/max(1,d)`、offset 钳位 ±180px）与 **`CreatorMotto` 的 `force = max(0,1-d/54)²*18`**：属于运行时物理积分，**不是**任何 Storyboard/KeySpline 能表达的，必须写成 C# 每帧更新（`CompositionTarget.Rendering`），且注意 `dt` 上限 32ms。
15. **`skinview3d` 的 3D 模型自转与 walk/idle 混合**（`SkinViewer3D.vue:66` 的 `blend += ((walk?1:0)-blend)*min(1,dt*6)`）：WebGL/three.js 生态，WPF 需换 `Viewport3D` + 自定义皮肤网格渲染，且这是**指数平滑**而非任何标准缓动。