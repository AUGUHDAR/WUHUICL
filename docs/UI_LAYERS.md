# UI 层叠台账 · 模糊 / 叠层 / 透明度 / 阴影

来源：`KAMUCL-1.1.5/src/`（只读）。本文**只讲层的叠加、模糊、透明度与阴影**；颜色与几何见 `UI_GEOMETRY_HOME.md` / `UI_GEOMETRY_PAGES.md` / `UI_ICONS.md`。

所有行号已逐条复核（本文写作时重新用 `grep -n` 对齐过，与前三篇台账里少量 ±1 的偏差不一致处以本文为准）。

> **本文最重要的一条结论（先读）**
> 用户抱怨的"UI 叠得非常多、很模糊"，其模糊来源**不是** CSS `backdrop-filter`，而是 **Windows DWM 的 Acrylic 材质**（`src/main/windowAppearance.ts:46` `backgroundMaterial: 'acrylic'`）。
> `.shell` / `.sidebar` / `.topbar` **没有任何 `backdrop-filter`**，它们只是半透明色块，桌面透视完全由 DWM 提供。
> 全站 19 处 CSS `backdrop-filter` 声明里，**只有 4 处真正看得见**（hero 药丸、hero 按钮、皮肤浮条、遮罩族）；其余所在面板自身不透明度 ≥94%，模糊基本无法被察觉，其中 `.launch-success`、`.card-float-menu`、`.connection-panel` 三处的底色其实是**完全不透明**的，模糊是**死代码**。
> 另有一处 `--glass-blur: 28px` 被 JS 写入但**全站无消费者**（`App.vue:1007` / `App.vue:937`），WPF 侧不要为它建模。
> 一条重要的减法结论：尾部压缩规则把**几乎所有面板阴影都改成了 `box-shadow:none`**（见 D.12 末段）。只有浮层（modal / float-menu / notice-panel / toast / picker-popup / designer-dialog）还保留 `--shadow-lg`。

---

## A 模糊与材质

### A.1 全部 `backdrop-filter` / `-webkit-backdrop-filter`（19 处声明，按界面族分组）

约定：`bgα` = 同一选择器上自身 background 的 alpha；`可见?` = 该模糊是否真的能被看见（bgα ≤ ~85% 才有意义）。

#### a) 遮罩 / 蒙层族（**load-bearing**）

| # | file:line | 选择器 | 声明（逐字） | 同元素 background | bgα | 可见? |
|---|---|---|---|---|---|---|
| 1 | `styles.css:535-536` | `.modal-mask` | `backdrop-filter: blur(10px);`<br>`-webkit-backdrop-filter: blur(10px);` | `styles.css:534 background: var(--mask)` | 暗 `.72` / 亮 `.42`（`App.vue:993`） | **是**（亮色下极强） |
| 2 | `HomeView.vue:968` | `.log-mask` | `backdrop-filter: blur(5px);` | `background: var(--mask)` | 同上 | **是** |
| 3 | `App.vue:2427-2428` | `.drop-mask`（整合包拖入） | `backdrop-filter: blur(24px) saturate(130%);`<br>`-webkit-backdrop-filter: blur(24px) saturate(130%);` | `App.vue:2426 background: color-mix(in srgb, var(--accent) 16%, transparent)` | **.16** | **是**（几乎全透） |
| 4 | `InstanceCenter.vue:109` | `.ic-mask` | `backdrop-filter:blur(8px)`（**无 -webkit 前缀**） | `background:#0009` | **.60** | **是** |
| 5 | `UpdateDialogShell.vue:33` | `.update-dialog-backdrop` | `backdrop-filter:blur(12px);-webkit-backdrop-filter:blur(12px);` | `background:var(--mask,rgba(20,27,40,.48))` | .72 / .42 | **是** |
| 6 | `EditPanel.vue:74` | `.designer-dialog-mask` | `backdrop-filter:blur(8px)` | `background:var(--mask)` | .72 / .42 | **是** |

#### b) Hero 与首页（**load-bearing，全在图片之上**）

| # | file:line | 选择器 | 声明 | 同元素 background | bgα | 可见? |
|---|---|---|---|---|---|---|
| 7 | `HomeView.vue:818` | `.hero-kicker` | `backdrop-filter: blur(12px);`（无 -webkit） | `background: rgba(9, 13, 14, 0.48)` + `border: 1px solid rgba(255, 255, 255, 0.13)` | **.48** | **是，全站最显眼的一处** |
| 8 | `HomeView.vue:834` | `.hero-settings, .hero-more` | `backdrop-filter: blur(13px);`（无 -webkit） | `background: rgba(10, 16, 17, 0.58)` + `border: 1px solid rgba(255, 255, 255, 0.14)` | **.58**；hover `:839` → `rgba(19, 29, 29, 0.76)` | **是** |
| 9 | `HomeView.vue:860` | `.runtime-strip` | `backdrop-filter: blur(18px) saturate(130%);`<br>`-webkit-backdrop-filter: blur(18px) saturate(130%);` | `:860` 写的是 `color-mix(in srgb, var(--card) 80%, transparent)`，**但被 `HomeView.vue:769` 的 `.runtime-strip { background:var(--surface-content) !important; }` 覆盖** → 实为 `color-mix(card-solid 94%, transparent)` | **.94（不是 .80！）** | 几乎否（仅 6% 透出 + saturate） |
| 10 | `HomeView.vue:949` | `.skin-overlay` | `backdrop-filter: blur(12px);`（无 -webkit） | `background: color-mix(in srgb, var(--card) 78%, transparent)` | **.78** | **是**（垫在 3D 画布上） |

#### c) 浮层菜单 / 通知 / Toast（**cosmetic 或死代码**）

| # | file:line | 选择器 | 声明 | 最终 background | bgα | 可见? |
|---|---|---|---|---|---|---|
| 11 | `styles.css:593-594` | `.float-menu` | `backdrop-filter: blur(24px) saturate(130%);` + `-webkit-` 同 | `styles.css:588` → `color-mix(in srgb, var(--card-solid, var(--card)) 96%, transparent)`（`ui-system.css:37` 的覆盖列表里**没有** `.float-menu`） | **.96** | 否 |
| 12 | `HomeView.vue:958` | `.float-menu`（scoped，特异度 0,2,0 **胜过** #11） | `backdrop-filter: blur(24px);`（**丢了 saturate**） | `background: color-mix(in srgb, var(--card) 94%, transparent)` | **.94** | 否 |
| 13 | `HomeView.vue:959` | `.card-float-menu`（与 #12 同元素叠加） | 无 blur，继承 #12 | `background: var(--card-solid, #192225)` | **1.00（完全不透明）** | **完全死代码** |
| 14 | `App.vue:2186-2187` | `.notice-panel`（通知 / 更新日志 / 下载中心三用） | `backdrop-filter: blur(24px) saturate(130%);` + `-webkit-` | `:2185 background: color-mix(in srgb, var(--card-solid, var(--card)) 94%, transparent)` | .94 | 否 |
| 15 | `SelectMenu.vue:141-142` | `.select-menu-float` | `backdrop-filter: blur(24px) saturate(130%);` + `-webkit-` | `:140 background: color-mix(in srgb, var(--card-solid, var(--card)) 96%, transparent)` | .96 | 否 |
| 16 | `LaunchNotice.vue:36` | `.launch-success` | `backdrop-filter: blur(24px) saturate(130%);` + `-webkit-` | `background: color-mix(in srgb, var(--bg) 94%, var(--accent))` — 两操作数都不带 alpha → **结果 100% 不透明** | **1.00** | **完全死代码** |

#### d) 内容卡族

| # | file:line | 选择器 | 声明 | background | bgα | 可见? |
|---|---|---|---|---|---|---|
| 17 | `ui-system.css:28` | `.content :is(.card,.connection-panel)` | `backdrop-filter:blur(16px) saturate(85%);` | `.content .card` → `ui-system.css:13 background-color:var(--surface-content)`（94%）；`.connection-panel` → `connection.css:15 background: color-mix(in srgb, var(--card) 94%, var(--bg))` — **两操作数都不透明 → 100% 不透明** | .94 / **1.00** | 否 / **connection-panel 是死代码** |
| 18 | `SkinsView.vue:628-629` | `.skins-page .card`（scoped，特异度胜过 #17） | `backdrop-filter: blur(24px) saturate(130%);` + `-webkit-` | 同上 `.content .card` = `--surface-content` 94% | .94 | 否 |
| 19 | `FriendConnectView.vue:131` | `.pick-card` | `backdrop-filter: blur(24px) saturate(130%); -webkit-…: blur(24px) saturate(130%);` | `:130 background: var(--surface-content)` | .94 | 否 |

#### e) 显式**关掉**模糊的地方

| file:line | 选择器 | 声明 |
|---|---|---|
| `FileManager.vue:719` | `.shader-empty` | `backdrop-filter:none!important`（连同 `background:transparent!important;border:0;box-shadow:none`） |

**未找到**：`.shell`、`.sidebar`、`.topbar`、`.content`、`.instance-card`、`.account-panel`、`.skin-panel`、`.status-strip`、`.toast`、`.hero-card` 本体上均**无** `backdrop-filter`。
**未找到**：任何 tooltip 组件的模糊（全站无 tooltip 样式实现，只有原生 `title` 属性）。

#### f) 半径汇总

出现过的模糊半径：`5 / 8 / 10 / 12 / 13 / 16 / 18 / 24` px（**8 档**）。
`saturate()` 只出现两种：`130%`（11 处）与 `85%`（1 处，`ui-system.css:28`）。
链上其他函数：**未找到**（无 `brightness()` / `contrast()` 进入 backdrop-filter 链）。

#### g) 覆盖链（谁最终生效）

- **`.float-menu`**：`styles.css:588+593`（bg 96% card-solid，blur24 sat130）→ `HomeView.vue:958` scoped（bg 94% card，blur24，**丢 sat**）→ `HomeView.vue:959` `.card-float-menu`（bg 100% card-solid）→ `GameView.vue:2202` `.instance-more-menu`（bg 100% card-solid）+ `ui-system.css:37`（`background-color:var(--surface-raised)` 97%，但被 GameView scoped 的 `background:` shorthand 整体重置压回 100%）。
  ⇒ **同一个 `.float-menu` 在不同页面拿到不同的 alpha 与不同的 blur 链，务必按页面复刻。**
- **`.menu-overlay`**：`styles.css:580 z-index:90` → `HomeView.vue:957 z-index:8000`（scoped）。
- **`.modal`**：`styles.css:549`（bg 96% card-solid）→ **`ui-system.css:37 background-color:var(--surface-raised)`（97%）胜出**（`main.ts:4` 先导 `styles.css`、`main.ts:5` 后导 `ui-system.css`，同特异度后者胜）。
- **`.card`**：`styles.css:451 background:var(--card)`（88%/94%）→ **`ui-system.css:13 .content .card{background-color:var(--surface-content);box-shadow:none}` 胜出**（特异度 0,2,0 > 0,1,0）。
- **`.skins-page .card`** 的 blur：scoped `[data-v-*]` 属性选择器把特异度抬到 0,3,0 ⇒ 胜过 `ui-system.css:28` 的 0,2,0。
- **`html[data-theme="transparent"] .btn-gold`**（`ui-system.css:119`）：`background:var(--accent-deep); box-shadow:none;` ⇒ **默认主题下主按钮是纯色，不是渐变**（0,2,1 胜过 `styles.css:200` 的 0,1,0）。

---

### A.2 玻璃令牌

| 令牌 | 定义位置 | 值 | 消费者 |
|---|---|---|---|
| `--glass-blur` | **只在 JS 里写**：`App.vue:1007` `st.setProperty('--glass-blur', '28px')`；声明在清理列表 `App.vue:937` | `28px` | **未找到任何消费者**（全仓 `grep -rn "glass-blur" src/` 只命中这两行）。历史遗留，WPF 不必建模。 |
| `--shell-surface` | `App.vue:1003-1006`（JS 写入） | `color-mix(in srgb, ${bg} ${blackPurple?96:dark?30:42}%, transparent)` | `App.vue:1729` `.shell`、`App.vue:1746` `.shell.has-bg` |
| `--surface-solid` | `ui-system.css:3` | `var(--card-solid, var(--bg))`；`--card-solid` 由 `App.vue:982` 写成 `colors.card`（**不透明**） | `--surface-content`、`--surface-raised`、`.settings-navigation`(`SettingsView.vue:1233`)、`.settings-search-results`(`:1845`)、`.file-more-actions`(`FileManager.vue:488`) |
| `--surface-content` | `ui-system.css:4` | `color-mix(in srgb, var(--surface-solid) 94%, transparent)` | `ui-system.css:13` `.content .card`、`:27` `.connection-panel`、`HomeView.vue:769` `.runtime-strip`(**!important**)、`:917` `.account-panel,.skin-panel`、`:1030` `.instances-block`、`FriendConnectView.vue:130` `.pick-card`、`SkinsView.vue:1094` `.skin-operation-panel`、`TerracottaPanel.vue:272` `.tc-tool`、`ServersView.vue:510` `.server-list` |
| `--surface-raised` | `ui-system.css:5` | `color-mix(in srgb, var(--surface-solid) 97%, transparent)` | `ui-system.css:37`（`.modal,.select-menu-list,.instance-more-menu,.context-menu`）、`Toasts.vue:33` `.toast` |
| `--card` / `--card-2` / `--bg-2` | `App.vue:981 / :983 / :987`（JS 覆盖 `styles.css:13 / :14 / :12`） | 见 C.8 | 全站 |
| `--card-solid` | `App.vue:982` | `colors.card`（**不透明**） | 各 `color-mix(...card-solid... N%, transparent)`、`EditPanel.vue:74`、`InstanceCenter.vue:109`、`HomeView.vue:959`、`GameView.vue:2171/2178/2202`、`MinecraftVersionPicker.vue:26` |
| `--mask` | `App.vue:993`；静态兜底 `styles.css:37`、`styles.css:106` | 暗 `rgba(2,5,6,.72)` / 亮 `rgba(27,36,55,.42)` | A.1(a) 全部遮罩、`SkinsView.vue:1004` `.history-overlay`；`InstanceCenter.vue:109` 用的是 `#0009` 而非 `--mask` |
| `--shadow` / `--shadow-lg` | `App.vue:1001 / :1002`（JS 覆盖 `styles.css:26 / :27`、`styles.css:95 / :96`） | 见 D.12 | 全站浮层 |

**关键**：`styles.css:10-76`（`:root`）与 `styles.css:79-109`（`[data-theme='black-orange']`）里的所有 alpha 值**都是首帧兜底**，`applyTheme()`（`App.vue:1017-1031`，`watch(..., { immediate: true })` `App.vue:1035-1039`）会用 **inline style** 覆盖它们。inline 优先级胜过任何工作表规则 ⇒ 迁移时**只认 JS 计算值**（见 C.8 表）。
`clearCustomVars()`（`App.vue:954-957`）在每次应用主题前按 `CUSTOM_VARS`（`App.vue:906-941`，共 33 项）逐项 `removeProperty`。

---

### A.3 Electron 窗口材质

`src/main/windowAppearance.ts:29-50`，**逐字引用**：

```ts
  return {
    width: 1360,
    height: 860,
    minWidth: 960,
    minHeight: 620,
    frame: platform === 'darwin',
    titleBarStyle: platform === 'darwin' ? 'hiddenInset' : undefined,
    trafficLightPosition: platform === 'darwin' ? { x: 16, y: 16 } : undefined,
    vibrancy: platform === 'darwin' ? 'sidebar' : undefined,
    visualEffectState: platform === 'darwin' ? 'active' : undefined,
    // DWM 为普通 HWND 合成桌面 Acrylic；transparent:true 会创建 layered
    // window，绕过该合成路径并影响 Windows 原生缩放/最大化。
    // WebContents 仍通过 backgroundColor 的 alpha 保持透明。
    // macOS 必须同时清空 NSWindow 底色，才能透出 behind-window vibrancy。
    // hiddenInset 保留系统红绿灯；不要用整个窗口 opacity 淡化界面内容。
    transparent: platform !== 'win32',
    backgroundColor: '#00000000',
    backgroundMaterial: platform === 'win32' ? 'acrylic' : undefined,
    roundedCorners: true,
    hasShadow: true,
    thickFrame: true
  }
```

合并进 `new BrowserWindow` 的位置：`src/main/index.ts:94-111`（`...windowAppearance()` 在 `:95`，其后覆盖 `icon` / `show:false` / `title:'KAMUCL'` / `webPreferences`；`windowState` 存在时再覆盖 `width/height/x/y`，`index.ts:96-98`）。

要点：
1. **Windows 上 `transparent:false`**（`windowAppearance.ts:44`）。桌面透视**不是** layered window，而是 DWM 为普通 HWND 合成的 Acrylic。
2. **`backgroundColor:'#00000000'`**（`:45`）让 WebContents 按 CSS alpha 逐像素透出，DWM 的 Acrylic 从下面穿过。
3. **`backgroundMaterial:'acrylic'`**（`:46`）= Win11 22H2+；旧系统 Electron 静默忽略 ⇒ 只剩"半透明色板 + 无桌面模糊"。
4. `roundedCorners:true` + `hasShadow:true` + `thickFrame:true`（`:47-49`）。
5. **运行时重设**：`src/main/nativeAppearance.ts:19-25`
   ```ts
   if (process.platform === 'win32' && !appliedMaterial.has(window)) {
     try {
       window.setBackgroundMaterial('acrylic')
       appliedMaterial.set(window, trackMaterialLifecycle(window, launcherLog, createDwmFrameRestorer(window)))
     }
     catch (error) { launcherLog(`Desktop acrylic unavailable: ${String(error)}`) }
   }
   ```
   同时 `nativeAppearance.ts:9-17` 按主题把 `nativeTheme.themeSource` 设成 `light`/`dark`，注释 `:8` 明确："**DWM 的染色也要跟随应用主题，否则浅色系统会在暗色页面下叠一层白灰色**"。⇒ **DWM 自己会加一层色调（tint），这是第 0 层，CSS 里查不到。**
   浅色判据（`nativeAppearance.ts:10-14`）：`theme === 'blue-white' || theme === 'white-pink' || (custom && bg 的 Rec.601 亮度 > 128)`。
6. **生命周期修补**：`src/main/materialLifecycle.ts:11-36` 在 `maximize/unmaximize/restore/show/leave-full-screen/focus`（120ms 去抖、focus 1s 节流）后调用 `restoreFrame()`；`src/main/dwmFrame.ts:16-36` 起外部 `WindowMaterial.exe`（`:19`），stdin 传 `"<hwnd> <dark|light> <finish-opacity|repair>"`（`:35`）。注释 `dwmFrame.ts:6-8` 说明 Electron 44 负责 DWM/裁剪/边框，helper **只**释放 `setOpacity(1)` 留下的 `WS_EX_LAYERED`（另见 `materialLifecycle.ts:26-28`，事件 `kamucl:startup-opacity-complete` `:28`）。
7. **启动原生淡入**：`src/main/nativeStartup.ts:7` `window.setOpacity(0)` → `:14` `window.setOpacity(p*p*(3-2*p))`（smoothstep）。**这是整个窗口级 opacity，不是 CSS 层。**
8. Electron 版本：`package.json:100` `"electron": "44.3.0"`。
9. 独立闪屏窗口（另一套材质）：`src/main/startupSplash.ts:23-28`
   ```ts
   let splash: BrowserWindow | null = new BrowserWindow({
     ...bounds, show: false, frame: false, transparent: true, backgroundColor: '#00000000',
     hasShadow: false, thickFrame: false, resizable: false, movable: false, focusable: false,
     skipTaskbar: true, alwaysOnTop: true, title: 'KAMUCL · 正在启动',
   ```
   + `splash.setIgnoreMouseEvents(true)`（`:29`）⇒ 一个全屏、透明、点击穿透的覆盖层，内容靠 canvas 自绘（见 E.3）。
10. macOS 例外：`html[data-platform="darwin"] .sidebar { padding-top: 38px; }` + `html[data-platform="darwin"] .shell { border-radius: 0; }`（`styles.css:7-8`），为红绿灯让位。

**DWM Acrylic 与 CSS 层的交互（迁移必读）**

`.shell` 的 `--shell-surface`（默认黑紫主题 = bg **96%**）意味着 **DWM Acrylic 有 96% 被遮住** —— 默认主题几乎看不到桌面玻璃。真正透得多的是：
- `black-orange` / `black-pink`：`--shell-surface` = **30%** ⇒ 70% 的桌面 Acrylic 透出；
- `blue-white` / `white-pink` / `custom`：**42%** ⇒ 58% 透出。
侧栏 `--bg-2` 的 `sideOpacity` 同理（暗色非黑紫只有 **26%**，再乘 `.sidebar` 的 92% ⇒ **有效 24%**）。
⇒ **"UI 叠得非常多"的观感主要来自 DWM acrylic → shell → sidebar → topbar → card 这条半透明链，而不是 CSS blur。**

---

### A.4 `@supports not (backdrop-filter: ...)` 兜底（WPF 无 backdrop blur ⇒ 这就是替换色）

**全站只有 1 处**（`grep -rn "@supports" src/renderer/` 只命中 1 行）：

`App.vue:1748-1752`
```css
@supports not ((backdrop-filter: blur(1px)) or (-webkit-backdrop-filter: blur(1px))) {
  .shell {
    background: color-mix(in srgb, var(--bg) 78%, transparent);
  }
}
```

| 兜底选择器 | 替换 background | 覆盖的原值 |
|---|---|---|
| `.shell` | `color-mix(in srgb, var(--bg) 78%, transparent)` → alpha = round(.78×255) = **199 = `0xC7`** | `App.vue:1729 background: var(--shell-surface)`（96/30/42%） |

其余 18 处 `backdrop-filter` **都没有兜底分支**（Chromium 恒支持）。⇒ WPF 侧不能只抄这一条，必须按 A.1 的 bgα 表**自行把 blur 折进 tint**（做法见 G.1 / G.2）。

---

## B 叠层背景

### B.5 多层 `background:`（逗号分隔）——逐字

| file:line | 选择器 | 逐字声明 | 层数 |
|---|---|---|---|
| **`HomeView.vue:815`**（基，**已失效**） | `.hero-shade` | `background: linear-gradient(90deg, rgba(8, 14, 15, 0.66), rgba(8, 14, 15, 0.24) 54%, rgba(8, 14, 15, 0.08)), linear-gradient(0deg, rgba(5, 10, 9, 0.38), transparent 52%);` | **2** |
| **`HomeView.vue:1030`**（尾部压缩，**胜出**） | `.hero-shade` | `background:linear-gradient(180deg,rgba(0,0,0,.16),rgba(0,0,0,.18) 30%,rgba(0,0,0,.68))` | **1** |
| `HomeView.vue:946` | `.skin-stage` | `background: radial-gradient(circle at 50% 82%, color-mix(in srgb, var(--accent) 13%, transparent), transparent 38%), linear-gradient(180deg, transparent, color-mix(in srgb, var(--bg) 18%, transparent));` | **2**（径向 + 线性） |
| `App.vue:1843` | `.nav-bubble` | `background: linear-gradient(135deg, color-mix(in srgb, var(--accent-soft) 80%, transparent), var(--hover));` | 1（两端皆半透明 ⇒ 复合后等效双层 alpha） |
| `App.vue:2281` | `.dl-bar-fill` | `background: linear-gradient(90deg, var(--accent-2), var(--accent))` | 1（叠在 `:2275 .dl-bar{background:var(--card-2)}` 之上 = **2**） |
| `connection.css:29` | `.network-overview` | `background: linear-gradient(110deg, var(--accent-soft), transparent 70%), var(--card);` | **2**（注意 `:15` 已给它 `background: color-mix(card 94%, var(--bg))`，被这条 shorthand **整体重置**） |
| `ServersView.vue:474` | `.servers-page .server-list-item.active` | `background: linear-gradient(110deg, var(--accent-soft), transparent), var(--card);` | **2** |
| `VoxLinkPanel.vue:429` | `.address-hero` | `background:linear-gradient(125deg,var(--accent-soft),var(--card-2))` | 1 |
| `HomeView.vue:935` | `.provider-mark`（微软，**已被 `:931` 覆盖**） | `background: linear-gradient(135deg, #f35325 0 48%, #81bc06 48% 100%)` | 1（双硬色标 = 视觉两块） |
| `HomeView.vue:936` | `.provider-mark.yggdrasil` | `background: linear-gradient(135deg, #65bd78, #268e54)` | 1 |
| `HomeView.vue:937` | `.provider-mark.offline` | `background: var(--card); color: var(--text-dim);` | 1 |
| `HomeView.vue:931` | `.provider-mark.microsoft` | `background: transparent; border-radius: 0;` | **胜出**（0,2,0 > `.provider-mark` 0,1,0）⇒ 微软账号显示模板里的 4 色 SVG（`HomeView.vue:623`），不是渐变 |
| `SettingsView.vue:1641` | `.tp-custom-grad` | `background: linear-gradient(135deg, #f43f5e 0%, #f97316 25%, #eab308 45%, #22c55e 65%, #3b82f6 85%, #a855f7 100%);` + `:1642 opacity: 0.85;` | 1，**叠在 `:1638 .preview-custom{background:#16161d}` 之上 = 2 层** |
| `SettingsView.vue:1719` | `.mem-slider-fill` | `border-radius: 999px; background: linear-gradient(90deg, var(--accent-2), var(--accent));` | 1，**叠在 `:1714 .mem-slider-track{background:var(--card-2)}` 之上 = 2 层** |
| `EditPanel.vue:74` | `.designer-viewport` | `background:color-mix(in srgb,var(--bg) 84%,#697687);background-image:radial-gradient(#88929d35 1px,transparent 1px);background-size:16px 16px` | **2**（实底 + 点阵图案） |
| `styles.css:338-341` | `.select` | `background-image: url("data:image/svg+xml,%3Csvg … stroke='%238f8d84' …%3E%3Cpath d='m6 9 6 6 6-6'/%3E%3C/svg%3E"); background-repeat: no-repeat; background-position: right 12px center;` | 1，**叠在 `:311 background: var(--card-2)` 之上 = 2 层（SVG 箭头层 + 半透明底）** |
| `styles.css:21` / `App.vue:974-977` | `--accent-grad` | 静态：`linear-gradient(135deg, #71dc83 0%, #55c96b 55%, #349d4c 100%)`；JS：`linear-gradient(135deg, ${accent2} 0%, ${accent} 55%, ${accentDeep} 100%)`，`accent2=color-mix(accent ${dark?72:84}%, ${dark?white:black})`，`accentDeep=color-mix(accent 78%, black)` | 1，**消费者**：`.btn-gold`(styles.css:201)、`.slider::-webkit-slider-thumb`(styles.css:431)、`.launch-main/.launch-arrow`(HomeView:848)、`h2::before`(HomeView:890)、`.account`(AccountsView:889)、`.avatar`(AccountsView:—)、`.capsule-blob`(CommunityView:749)、`.game-tabs-blob`(GameView:1885)、`.seg-blob`(SkinsView:828)、`.row-bar-fill`(GameView:1800)、`.upd-progress-bar`(UpdateModal:101)、`.mc-avatar.letter`(Avatar.vue:66)、`.DupCleanModal`(DupCleanModal:—) |

#### 「窗口渐变」= 未找到
不存在名为 window-gradient 的东西。`.shell` 是**纯色** `--shell-surface`（`App.vue:1729`），`.app-bg` 是纯色 / `background-color + background-image`（`App.vue:884-899`）。窗口的"渐变感"完全来自 DWM acrylic 的色调（见 A.3 要点 5）。

#### 侧栏 / 卡片 / 进度条 底色（多层叠加的落点）

| 选择器 | file:line | 生效值 |
|---|---|---|
| `.shell` | `App.vue:1729` | `var(--shell-surface)` |
| `.shell` 内高光 | `App.vue:1735` | `box-shadow: inset 0 1px 0 rgba(255,255,255,.12);` |
| `.sidebar` | `App.vue:1760` | `color-mix(in srgb, var(--bg-2) 92%, transparent)` ← `--bg-2` 本身已是 `color-mix(sidebarBg N%, transparent)` ⇒ **alpha 连乘**（见 C.8） |
| `.topbar` | `App.vue:2025` | `color-mix(in srgb, var(--bg-2) 78%, transparent)` |
| `.card`（页面内） | `styles.css:451` → **被 `ui-system.css:13` 覆盖** | `background-color:var(--surface-content)`；`box-shadow:none`（原 `var(--shadow)` 被去掉） |
| `.instance-card` | `HomeView.vue:896` `color-mix(card 82%,transparent)` → **被 `:1030` `.home-dashboard .instance-card{…background:transparent;box-shadow:none;border:0;border-bottom:1px solid var(--border);border-radius:0}` 覆盖** | 生效：`transparent`；`.instance-card.selected{background:var(--accent-soft)}` 与 `.instance-card:hover{background:var(--hover)}` 也写在 `:1030` 内、**位于 `.home-dashboard .instance-card` 之后** ⇒ 同特异度按行序**生效** |
| `.instances-block` | `HomeView.vue:1030` | `background:var(--surface-content);border-radius:var(--radius-lg);padding:16px` ← **真正承载实例卡列表的那层底** |
| `.account-panel` / `.skin-panel` | `HomeView.vue:917` + `:1030` | `:917` 给 `var(--surface-content)` + `var(--shadow)`；`:1030` 把两者 `box-shadow:none`，account = 上半圆角 + `border-bottom:0`，skin = 下半圆角 + `margin-top:-16px;border-top:0` ⇒ **两张融合成一张** |
| `.home-creator` | `HomeView.vue:916` → `:1030` | `:1030` `box-shadow:none;margin-top:0` |
| `.folder-manager` / `.installed-card`（版本页） | `GameView.vue:2171 / :2178` | `color-mix(in srgb, var(--card-solid, var(--card)) 75%, transparent)` / `…92%…`，均 `border:0; box-shadow:none` |
| `.dl-bar` / `.dl-bar-fill` | `App.vue:2271-2283` | `background: var(--card-2); height:5px; overflow:hidden` / `linear-gradient(90deg,…)`；`:2288` `.dl-stage .dl-bar{height:3px}`；`:2290` indeterminate 动画 `translateX(-120%→320%)`（`@keyframes dl-indeterminate` `:2293-2300`） |
| `.row-bar` / `.row-bar-fill` | `GameView.vue:1787-1802` | `width:90px;height:6px;background:var(--card-2);border:1px solid var(--border);overflow:hidden` / `height:100%;background:var(--accent-grad)` |
| `.upd-progress` / `-bar` | `UpdateModal.vue:101` | `height:8px;background:var(--hover);overflow:hidden` / `background:var(--accent-grad)` |
| `.mem-slider-track` / `-fill` / `-thumb` | `SettingsView.vue:1713-1727` | **3 层**：轨道 `var(--card-2)` → 填充 90° 渐变 → 拇指 `var(--accent)` + `2px solid var(--on-accent)` 描边 + `0 1px 6px color-mix(accent 45%,transparent)` |
| `.launch-progress`（启动按钮内进度） | `HomeView.vue:853` | `position:absolute; inset:0 auto 0 0; background: rgba(255, 255, 255, 0.25);` ⇒ **叠在 `.launch-main` 的 `--accent-grad` 之上 = 2 层** |
| `.avatar-ring` / `.mc-avatar` | `HomeView.vue:920` | `border-radius:12px; box-shadow: 0 0 0 4px color-mix(in srgb, var(--text) 8%, transparent);` ← **用 spread-ring 代替 border** |
| `.avatar`（账号页圆形） | `AccountsView.vue:885-892` | `42×42;border-radius:50%;background:var(--accent-grad);color:var(--on-accent)` 无环 |
| `.empty` | `styles.css:515-524` | **无 background**（纯排版） |
| `.empty-instances` | `HomeView.vue:913` | `min-height:110px;border:1px dashed var(--border-strong);border-radius:13px;background:var(--card)` |
| `.connection-empty` | `connection.css:68` | `min-height:220px`，**无 background** |
| `.list-empty` / `.ic-empty` / `.pane-empty` | `AccountsView.vue:857` / `InstanceCenter.vue:109` / `SkinsView.vue:1096` | 只有 padding，无 background |
| `.shader-empty` | `FileManager.vue:719` | `background:transparent!important;border:0;box-shadow:none;backdrop-filter:none!important` ⇒ **主动剥掉所有层** |
| `ContentSkeleton` | `ContentSkeleton.vue:20` | `background:color-mix(in srgb,var(--text) 9%,var(--card))` |
| `.status-strip` | `ui-system.css:41` | `background:var(--accent-soft)`（14%），`:42 .status-strip.error{background:var(--danger-soft)}` |

---

### B.6 全部 `::before` / `::after`（会画出东西的）

全站**只有 6 处伪元素规则**（`grep -rn "content:"` 全量核对）。用户设想的 **tooltip 箭头 / 跑马灯 mask 遮罩：未找到**。

| file:line | 选择器 | content | 尺寸 | 定位 | background | opacity | z-index |
|---|---|---|---|---|---|---|---|
| `HomeView.vue:890` | `.instances-head h2::before`（**板块标题前的主题色短竖线**） | `''` | `width:4px; height:17px; border-radius:2px` | flex 子项（`flex:none`），父 `h2{display:flex;align-items:center;gap:9px}`（`:888`） | `var(--accent-grad)` | 1 | — |
| `App.vue:1935-1946` | `.nav-sub-item::before`（**子菜单圆点**） | **`content: none`** ⇒ **实际不绘制**（下面全是死代码，保留只为对照） | `5px × 5px; border-radius:50%` | `position:absolute; left:17px; top:50%; transform:translateY(-50%)` | `currentColor` | `0.4`；`.active::before` → `1`（`:1947-1949`） | — |
| `styles.css:383-394` | `.switch-ui::before`（**开关滑块**） | `''` | `14×14` 圆 | `position:absolute; left:3px; top:50%; transform:translateY(-50%)`；checked → `translate(18px,-50%)`（`:399-402`） | `var(--text-dim)` → checked `var(--on-accent)` | 1 | — |
| `connection.css:59-61` | `.connection-toggle-track::after`（连接页开关滑块） | `''` | `16×16` 圆，`display:block` | `padding:3px` 的 track 内流式；checked → `translateX(14px)` | `var(--text-dim)` → `var(--on-accent)` | 1 | — |
| `HomeLayoutEditor.vue:496-497` | `.layout-setting>summary::after`（折叠箭头） | `'⌄'` | 文本字形 | `margin-left:auto`；`[open]` → `rotate:180deg` | — （`color:var(--text-dim)`） | 1 | — |
| `GameView.vue:2194` | `.instance-meta>span+span::before`（元信息分隔点） | `'·'` | 文本 | `margin-right:10px` | — | **`.5`** | — |

**非绘制但参与视觉的伪元素（列出以免漏）**
`styles.css:134` `::selection{background:color-mix(in srgb,var(--accent) 30%,transparent)}`；
`styles.css:139-155` 滚动条：`::-webkit-scrollbar{width:8px;height:8px}`、`-track{background:transparent}`、`-thumb{background:var(--scroll);border-radius:999px}`、`-thumb:hover{background:var(--accent)}`、`-corner{background:transparent}`；
`styles.css:318-321` `.input::placeholder{color:var(--text-dim);opacity:.7}`；`GameView.vue:1673` `.tool-search input::placeholder{opacity:.75}`；`SkinsView.vue:1068` `.need-ms-icon{opacity:.8}`；
`styles.css:347-353` number spin 移除；`styles.css:425-447` `::-webkit-slider-thumb{width:16px;height:16px;border-radius:50%;background:var(--accent-grad);box-shadow:0 2px 8px var(--accent-soft);transition:transform .15s}` + `:hover{transform:scale(1.2)}` + `::-moz-range-thumb{background:var(--accent)}`；
`styles.css:342-345` `.select option{background:var(--card-2);color:var(--text)}`；
`HomeLayoutEditor.vue:481-487` `.color-swatch::-webkit-color-swatch-wrapper{padding:3px}` / `::-webkit-color-swatch{border:none;border-radius:4px}`；
`ui-system.css:82-91` `details::details-content`（**原生 disclosure 动画层**：`interpolate-size:allow-keywords; block-size:0; opacity:0; overflow:clip; transition: block-size 220ms, opacity 180ms, content-visibility 220ms allow-discrete`；`[open]` → `block-size:auto;opacity:1`；`details:has(> summary.btn)` → `block-size:auto;overflow:visible`）。

---

### B.7 叠放顺序与 z-index 全表

#### 每个表面的实际层序

**① 外壳（`App.vue:1227-1234` + `styles.css:540-544`）**
```
body（background:transparent，styles.css:117-132；font-smoothing:antialiased；user-select:none）
└─ Teleport 目标：design 模式进 #design-preview-host，否则 body（App.vue:1227）
   ├─ .app-bg         position:fixed; inset:0; z-index:0; pointer-events:none   (App.vue:1738-1743)
   └─ .shell          display:flex; position:relative; z-index:1; overflow:hidden (App.vue:1725-1736)
      ├─ .sidebar     isolation:isolate; position:relative; z-index:2           (styles.css:542; App.vue:1755-1762)
      └─ .main-area   flex:1; min-width:0; flex-direction:column                (App.vue:2008-2013) — 不绘制
         ├─ .topbar   isolation:isolate; position:relative; z-index:2; -webkit-app-region:drag (styles.css:542; App.vue:2016-2027)
         └─ .content  isolation:isolate; z-index:0; overflow-y:auto; scrollbar-gutter:stable (styles.css:541; App.vue:2398-2404)
```
`.sidebar`/`.topbar`（z 2）与 `.content`（z 0）都是 `.shell` 内的兄弟 ⇒ 侧栏与顶栏永远压在内容之上。
`.content` 单独 `isolation:isolate` 是为了把页面内的 z-index 关进局部上下文（`styles.css:540` 注释原文："User content layers stay within the page, below navigation and system dialogs"）。
`.app-bg` 与 `.shell` 是**兄弟**而非父子 ⇒ 背景层在 `.shell` 之下、但不受其 `overflow:hidden` 裁剪。design 预览里两者都被强制进同一 host：`styles.css:543-544`
```css
#design-preview-host > .shell { width: 100% !important; height: 100% !important; min-height: 0 !important; }
#design-preview-host > .app-bg { position: absolute !important; }
```

**② Hero 卡（`HomeView.vue:480-543`）**
```
1 .hero-card        background:#17231f(不透明); overflow:hidden; border≈α.213; radius 20;
                    box-shadow 0 18px 44px rgba(0,0,0,.22)                                        :799-809
2 .hero-image × N   absolute inset:0; opacity 0 → .active{opacity:1};
                    transition: opacity var(--motion-carousel)=500ms (ui-system.css:7)
                    :1033 追加 object-position:center 42%                                          :810-811,:1033
3 .hero-shade       absolute inset:0; background = :1030 的单条 180° 三段渐变
                    （:815 的双层渐变已被覆盖）                                                    :812-816,:1030
4 .hero-content     position:relative; z-index:1; color:var(--bn-text); padding 尾部改 24px        :817,:1030
  ├ .hero-kicker    rgba(9,13,14,.48) + blur(12px) + border rgba(255,255,255,.13)                  :818 (:1030 改字号/padding)
  ├ h1              text-shadow 0 4px 24px rgba(0,0,0,.32)（尾部未撤销 → 生效）                     :819,:1030
  ├ .loader-badge   α≈.622 底 + border accent-2@34%                                                :831
  ├ .hero-settings / .hero-more  rgba(10,16,17,.58) + blur(13px) + border rgba(255,255,255,.14)    :834,:835,:837 (:839 hover .76)
  └ .launch-combo   双 box-shadow（外发光 + 2px inset 顶高光）                                     :841-847
     ├ .launch-main / .launch-arrow   background:var(--accent-grad); position:relative;overflow:hidden  :848-849,:855
     │  ├ .launch-progress   absolute inset:0 auto 0 0; rgba(255,255,255,.25)                      :853
     │  └ .launch-content     position:relative; z-index:1                                         :850
     └ .launch-arrow 分隔线  border-left:1px solid rgba(255,255,255,.22)                           :855
```

**③ 侧栏（`App.vue:1247-1249`）**
```
0 .sidebar background color-mix(bg-2 92%,transparent) + border-right 1px var(--border)   App.vue:1760-1761
1 .nav-bubble     absolute; 无 z-index（文档流最早）; 渐变底; shadow 0 3px 12px #00000008  App.vue:1837-1848
2 .nav-selection  absolute; z-index:2; box-shadow: inset 3px 0 0 var(--accent); 无背景      App.vue:1849-1858
3 .nav-item       position:relative; z-index:1; background:transparent（active 也只改 color）App.vue:1811-1836
4 .nav-icon / .nav-label
```
`bubble` 与 `selection` 的几何**都由 JS 内联**（`composables/useNavigationBubble.ts:57-64`：`transform: translate3d(x,y,0)`、`width/height`、`opacity: visible?1:0`）。二者是**两个独立层**：bubble = 指针跟随（hover），selection = 选中态左竖条。

**④ 运行时条（`HomeView.vue:545-561`）**
```
0 .runtime-strip   surface-content(94%) + blur(18px) saturate(130%) + border + var(--shadow)   :860,:769
1 .runtime-blob    absolute top/bottom:0; z-index:0; color-mix(accent 10%,transparent);
                   opacity 0→1 + scale(.97→1)                                                  :862-870
2 .runtime-item    position:relative; z-index:1; background:transparent; +相邻 border-left     :871-872
3 .runtime-state i 7×7 圆点 + 0 0 0 3px var(--ok-soft / --danger-soft)                          :881-884
```

#### 全量 z-index 映射表

| 值 | 选择器 | file:line | 说明 |
|---|---|---|---|
| `0` | `.content` | `styles.css:541` | 内容层根（`isolation:isolate`） |
| `0` | `.app-bg` | `App.vue:1741` | 用户背景层 |
| `1` | `.shell` | `App.vue:1731` | 压在 `.app-bg`（z 0）之上 |
| `0` | `.switch input` | `styles.css:371` | 隐形 checkbox 铺在 `.switch-ui` 上（`inset:0`） |
| `0` | `.runtime-blob` | `HomeView.vue:863` | |
| `0` | `.capsule-blob` | `CommunityView.vue:753` | |
| `0` | `.game-tabs-blob` | `GameView.vue:1890` | |
| `0` | `.seg-blob` | `SkinsView.vue:831` | |
| `1` | `.nav-item` | `App.vue:1827` | |
| `1` | `.hero-content` | `HomeView.vue:817` | |
| `1` | `.launch-content` | `HomeView.vue:850` | |
| `1` | `.runtime-item` | `HomeView.vue:871` | |
| `1` | `.capsule` | `CommunityView.vue:757` | |
| `1` | `.game-tab` | `GameView.vue:1894` | |
| `1` | `.seg-btn` | `SkinsView.vue:835` | |
| `1` | `.connection-toggle input` | `connection.css:57` | |
| `2` | `.sidebar, .topbar` | `styles.css:542` | |
| `2` | `.nav-selection` | `App.vue:1856` | |
| `2` | `.skin-overlay` | `HomeView.vue:949` | |
| `2` | `.drag-hint` | `SkinsView.vue:751` | |
| `2` | `.ic-dialog-mask` / `.designer-toolbar` / `.designer-outline` / `.insertion` / `.guide` | `InstanceCenter.vue:109` / `EditPanel.vue:74` | 局部上下文内 |
| `3` | `.designer-layers-panel` / `.designer-panel` | `EditPanel.vue:74` | |
| `4` | `.server-more summary` 系（`position:relative`） | `ServerDetails.vue:30` | |
| `5` | `.file-more[open]` | `FileManager.vue:487` | |
| `6` | `.fm-batch`（`position:sticky; top:0`） | `FileManager.vue:491` | |
| `10` | `.designer-dialog-mask` | `EditPanel.vue:74` | |
| `15` | `.settings-navigation`（`position:sticky; top:0`） | `SettingsView.vue:1233` | 底 `var(--surface-solid)`（**不透明**） |
| `20` | `.settings-search-results`（`position:absolute; right:0; top:48px`） | `SettingsView.vue:1845` | 底 `var(--surface-solid)` + `var(--shadow)` |
| `90` | `.menu-overlay` | `styles.css:580` | 透明捕点层 |
| `95` | `.float-menu` | `styles.css:584` | |
| `200` | `.edit-tip` | `styles.css:720` | 顶部居中药丸，`background: var(--accent)` + `var(--shadow-lg)`（`:726/:730`） |
| `200` | `.toasts` | `Toasts.vue:19` | **与 `.edit-tip` 打平**；两者都在 body 层 ⇒ 高于全局 `.float-menu`(95)，**低于** HomeView scoped `.float-menu`(8001) |
| `8000` | `.menu-overlay`（scoped 覆盖 `:580`） | `HomeView.vue:957` | |
| `8001` | `.float-menu`（scoped 覆盖 `:584`） | `HomeView.vue:958` | |
| `9000` | `.notice-mask` | `App.vue:2144` | |
| `9000` | `.cfg-capture-mask`（叠在 `.menu-overlay` 上，模板 `KeysView.vue:221`） | `KeysView.vue:269` | |
| `9001` | `.notice-panel` | `App.vue:2182` | |
| `9001` | `.ver-filter-menu` | `CommunityView.vue:808` | |
| `9001` | `.cfg-clear` / `.cfg-bind.capturing` | `KeysView.vue:270 / :271` | |
| `9100` | `.log-mask` | `HomeView.vue:968` | |
| `9500` | `.ic-mask` | `InstanceCenter.vue:109` | |
| `9500` | `.update-dialog-backdrop` | `UpdateDialogShell.vue:33` | |
| `9600` | `.cfg-mismatch-mask`（叠在 `.menu-overlay` 上，模板 `App.vue:1532`） | `App.vue:1721` | |
| `9999` | `.drop-mask` | `App.vue:2422` | |
| `10000` | `.modal-mask` | `styles.css:530` | |
| `10000` | `.ic-lightbox` | `InstanceCenter.vue:109` | `background:#101418fa`（α .98，**无 blur**） |
| `10020` | `.modal-mask` + **inline** `style="z-index: 10020"` | `ModInstallDialog.vue:31` | |
| `10030` | `.modal-mask` + **inline** `style="z-index: 10030"` | `HomeView.vue:759` | 强制重启确认（全站最高的常规弹窗） |
| `11000` | `.select-menu-float` | `SelectMenu.vue:133` | |
| `12000` | `.launch-success` | `LaunchNotice.vue:36` | |
| `12000` | `.version-picker-popup` | `MinecraftVersionPicker.vue:26` | |
| `99999` | `.splash`（源码注释：全应用最高层，遮盖一切内容） | `SplashScreen.vue:131` | |
| `100000` | `.design-workspace` | `EditPanel.vue:74` | 个性化设计器 |
| 动态 | `z-index: String(style.layer ?? 0)` + `position:relative`（free 模式另有 `translate`） | `visualDesign.ts:90-91` | 用户在设计师里设的层级 |
| 动态 | `.hero-image` 的 `opacity` 交叉淡入 | `HomeView.vue:810-811` | 不是 z-index，但决定图片层的可见顺序 |

**WPF 映射提醒**：`Panel.ZIndex` 只有 0–65535 且是**逐容器**的，上表这些绝对值必须换成「不同 Grid 子元素顺序 + Popup/AdornerLayer 层级」。建议：`< 100` 留在页面 Grid 内、`200` 用 Adorner、`≥8000` 一律用独立 `Popup`（或顶层 `Window`）按上表顺序排。

---

## C 透明度

### C.8 作为**表面**的 `color-mix(..., transparent)` + JS 注入的 alpha 全表

#### JS 主题应用器（`App.vue:960-1012`，`applyCustomVars`）

| 变量 | file:line | 表达式 | dark | light | blackPurple（默认「transparent / 黑紫」） |
|---|---|---|---|---|---|
| `--card` | `App.vue:981` | `color-mix(${card} ${cardOpacity}%, transparent)` | **88** | **94** | 88 |
| `--card-solid` | `:982` | `colors.card` | 不透明 | 不透明 | 不透明 |
| `--card-2` | `:983` | `color-mix(${card} ${raisedOpacity}%, transparent)` | **82** | **91** | 82 |
| `--bg-2` | `:987` | `color-mix(${sidebarBg} ${sideOpacity}%, transparent)` | **26** | **38** | **88** |
| `--shell-surface` | `:1003-1006` | `color-mix(${bg} ${blackPurple?96:dark?30:42}%, transparent)` | **30** | **42** | **96** |
| `--accent-soft` | `:978` | `color-mix(${accent} 14%, transparent)` | 14 | 14 | 14 |
| `--hover` | `:992` | `color-mix(${text} ${dark?7:5}%, transparent)` | 7 | 5 | 7 |
| `--scroll` | `:991` | `color-mix(${textDim} 32%, transparent)` | 32 | 32 | 32 |
| `--border-strong` | `:990` | `color-mix(${border} 65%, ${text})` | 真混色（非纯 alpha） | 同 | 同 |
| `--accent-2` | `:969` | `color-mix(${accent} ${dark?72:84}%, ${dark?white:black})` | 72% + 白 | 84% + 黑 | 72% + 白 |
| `--accent-deep` | `:970` | `color-mix(${accent} 78%, black)` | — | — | — |
| `--accent-grad` | `:974-977` | `linear-gradient(135deg, ${accent2} 0%, ${accent} 55%, ${accentDeep} 100%)` | | | |
| `--on-accent` | `:979` | `hexLuminance(accent) > 0.6 ? '#1a1208' : '#ffffff'` | | | |
| `--mask` | `:993` | `dark?'rgba(2,5,6,.72)':'rgba(27,36,55,.42)'` | .72 | .42 | .72 |
| `--danger` / `--danger-soft` / `--danger-border` | `:994/:995/:996` | `#ff6b73`/`#dc2626`；`rgba(…,0.13)`/`rgba(…,0.08)`；`rgba(…,0.34)`/`rgba(…,0.3)` | | | |
| `--ok` / `--ok-soft` | `:997/:998` | `#68dc88`/`#168a42`；`rgba(…,0.13)`/`rgba(…,0.1)` | | | |
| `--cyan` / `--cyan-soft` | `:999/:1000` | `#9ed7e9`/`#0e7490`；`rgba(…,0.13)`/`rgba(…,0.1)` | | | |
| `--shadow` | `:1001` | `dark?'0 2px 8px rgba(0,0,0,.16)':'0 2px 8px rgba(31,50,85,.045)'` | | | |
| `--shadow-lg` | `:1002` | `dark?'0 20px 55px rgba(0,0,0,.42)':'0 20px 55px rgba(31,50,85,.18)'` | | | |
| 布局（非 alpha，一并列出） | `:1009/:1010/:1011` | `--sidebar-w:208px`、`--banner-h:430px`、`--radius:14px` 硬编码 | | | |

判据：`dark = hexLuminance(colors.bg) < 0.46`（`App.vue:964`，权重 `0.299R+0.587G+0.114B`，`App.vue:944-952`）；`blackPurple = theme === 'transparent'`（`App.vue:965`）。色板：`shared/types.ts:256-336`（`THEME_PRESETS`）、`types.ts:236-253`（`DEFAULT_CUSTOM_THEME`）。自定义主题先过 `readableCustomColors()`（`shared/themeContrast.ts:17-33`：`luminance(bg) < .18` 定 dark，可能把 `card`/`sidebarBg` 向 `bg` 混 15%，并把 `text`/`textDim`/`sidebarText` 在对比度 <4.5 时强改为 `#ffffff` 或 `#101218`）。

#### 六套主题的**有效 ARGB**（`#AARRGGBB`，`A = round(pct × 255)`；嵌套 alpha 已连乘）

| 主题（源码位置） | bg / card / sidebarBg | `.shell` | `.sidebar` = bg-2×92% | `.topbar` = bg-2×78% | `--card` | `--card-2` | `surface-content` | `surface-raised` |
|---|---|---|---|---|---|---|---|---|
| **黑紫（默认）** `types.ts:321-335`，dark+blackPurple | `#212121` `#292929` `#171717` | `#F5212121` (96%) | `#CE171717` (.8096) | `#AF171717` (.6864) | `#E0292929` | `#D1292929` | `#F0292929` | `#F7292929` |
| **蓝白** `types.ts:260-274`，light | `#edf0f7` `#ffffff` `#f5f7fb` | `#6BEDF0F7` (42%) | `#59F5F7FB` (.3496) | `#4CF5F7FB` (.2964) | `#F0FFFFFF` | `#E8FFFFFF` | `#F0FFFFFF` | `#F7FFFFFF` |
| **橙黑** `types.ts:275-289`，dark | `#0b0b0e` `#18181f` `#111116` | `#4D0B0B0E` (30%) | `#3D111116` (.2392) | `#34111116` (.2028) | `#E018181F` | `#D118181F` | `#F018181F` | `#F718181F` |
| **粉白** `types.ts:290-304`，light | `#fdf2f8` `#ffffff` `#fce7f3` | `#6BFDF2F8` | `#59FCE7F3` | `#4CFCE7F3` | `#F0FFFFFF` | `#E8FFFFFF` | `#F0FFFFFF` | `#F7FFFFFF` |
| **粉黑** `types.ts:305-319`，dark | `#171019` `#211623` `#1c1220` | `#4D171019` | `#3D1C1220` | `#341C1220` | `#E0211623` | `#D1211623` | `#F0211623` | `#F7211623` |
| **自定义默认** `types.ts:236-253`，light | `#edf0f7` `#ffffff` `#f5f7fb` | 同蓝白 | 同 | 同 | 同 | 同 | 同 | 同 |

> `@supports not` 兜底：`.shell` → `#C7212121`（黑紫）/ `#C7EDF0F7`（蓝白）/ `#C70B0B0E`（橙黑）/ `#C7FDF2F8`（粉白）/ `#C7171019`（粉黑），alpha 恒 `.78`。

#### 静态兜底（仅首帧可见，之后被 inline 覆盖）

`styles.css:11-38`（`:root`，图一深色绿）：`--bg #10191b`、`--bg-2 rgba(13,23,26,.72)`、`--card rgba(23,34,37,.66)`、`--card-2 rgba(37,50,52,.58)`、`--border rgba(194,214,203,.18)`、`--accent-soft rgba(85,201,107,.16)`、`--hover rgba(255,255,255,.07)`、`--scroll rgba(27,36,55,.2)`、`--mask rgba(32,42,66,.4)`、`--shadow 0 8px 24px rgba(0,0,0,.22)`、`--shadow-lg 0 24px 70px rgba(0,0,0,.46)`。
`styles.css:79-109`（`[data-theme='black-orange']`）：`--bg-2 #111116`（**不透明**）、`--card #16161d`、`--card-2 #1c1c25`、`--border #26262f`、`--mask rgba(5,5,8,.62)`、`--shadow 0 2px 12px rgba(0,0,0,.35)`、`--shadow-lg 0 10px 30px rgba(0,0,0,.5)`。
⇒ 这两段是**无脚本 / 主题未应用时**的外观，正式渲染走不到（`applyTheme` 在 `App.vue:1035-1039` 带 `immediate:true`）。WPF 无需实现。

#### 其余作为表面的 `color-mix(... , transparent)`（非 JS 注入，全量清单）

| file:line | 选择器 | 基数 | % |
|---|---|---|---|
| `styles.css:135` | `::selection` | accent | 30 |
| `styles.css:246/248/255` | `.btn-danger:hover` bg / shadow / `:focus-visible` | danger | 18 / 14 / 22 |
| `styles.css:469` | `.tag` | text-dim | 14 |
| `styles.css:549` | `.modal`（**被 `ui-system.css:37` 的 97% 覆盖**） | card-solid | 96 |
| `styles.css:588` | `.float-menu` | card-solid | 96 |
| `styles.css:741/750` | `.edit-tip-done` / `:hover` | on-accent | 20 / 32 |
| `styles.css:784` | `@media(hover:hover) .btn-ghost:hover` | accent + card-2 | 10 |
| `App.vue:1733` | `.shell` border | border | 80 |
| `App.vue:1750` | `@supports not .shell` | bg | 78 |
| `App.vue:1760` | `.sidebar` | bg-2 | 92 |
| `App.vue:1843` | `.nav-bubble` 渐变起点 | accent-soft | 80 |
| `App.vue:1969` | `.sidebar-health` | card | 48 |
| `App.vue:2025` | `.topbar` | bg-2 | 78 |
| `App.vue:2040` | `.top-back` border | border | 70 |
| `App.vue:2185` | `.notice-panel` | card-solid | 94 |
| `App.vue:2426` | `.drop-mask` | accent | 16 |
| `App.vue:2439` | `.drop-box` | accent + card | 8 |
| `App.vue:2549/2551` | `.mp-conflict` border / bg | accent-deep + border/card | 48 / 8 |
| `SelectMenu.vue:140` | `.select-menu-float` | card-solid | 96 |
| `connection.css:15` | `.connection-panel,.network-overview`：`color-mix(in srgb, var(--card) 94%, var(--bg))` | card + **不透明 bg** ⇒ **结果不透明** | 94 |
| `connection.css:65/79` | `.connection-result.success` / `.connect-tab.active` border | ok+border 40 / accent+border-strong 55 | |
| `HomeView.vue:805` | `.hero-card` border：`color-mix(in srgb, var(--border) 95%, white 4%)` — **百分比和=99%，CSS 归一化为 95.96/4.04** ⇒ 有效 alpha ≈ `.18×.9596 + 1×.0404 = .213` | border + white | 95/4 |
| `HomeView.vue:831` | `.loader-badge`：`color-mix(in srgb, var(--accent) 30%, rgba(20,30,24,.46))` ⇒ alpha `.30+.70×.46=.622`；border `color-mix(accent-2 34%, transparent)` | accent + 指定深绿灰 | 30 |
| `HomeView.vue:860` | `.runtime-strip` 基值（**被 `:769` !important 覆盖**） | card | 80 |
| `HomeView.vue:865` | `.runtime-blob` | accent | 10 |
| `HomeView.vue:896` | `.instance-card` 基值（**被 `:1030` 覆盖**） | card | 82 |
| `HomeView.vue:909` | `.instance-play` | accent + card-2 | 18 |
| `HomeView.vue:916` | `.home-creator` border / inset 环（inset 被 `:1030` 去掉） | accent + border 26 / accent 10 | |
| `HomeView.vue:920` | `.mc-avatar` 光环 | text | 8 |
| `HomeView.vue:946` | `.skin-stage` border / 径向 / 线性 | border 78 / accent 13 / bg 18 | |
| `HomeView.vue:949` | `.skin-overlay` | card | 78 |
| `HomeView.vue:958/969` | `.float-menu`(scoped) / `.log-dialog` | card | 94 |
| `HomeView.vue:978` | `.log-body` | bg | 42 |
| `HomeView.vue:843/846` | `.launch-combo` shadow（含 inset 高光） | accent 38 / white 14 → 46 / 16 | |
| `ui-system.css:31` | `.cfg-sections button.active`：`color-mix(accent 12%, var(--surface-solid))` | accent + **不透明** | 12 |
| `FileManager.vue:719` | `.fm-row:has(>input:checked)` | accent | 5 |
| `AccountsView.vue:954` | `.user-code:hover` | accent | 22 |
| `AccountsView.vue:1014` | border `color-mix(in srgb, var(--danger) 50%, var(--border))` | danger + border | 50 |
| `CommunityView.vue:903` | `.tag-cf`：`color-mix(in srgb, #f97316 12%, transparent)` | 字面橙 | 12 |
| `GameView.vue:1568/1570` | `.folder-missing-card` border / bg | danger | 40 / 8 |
| `GameView.vue:2171/2178` | `.folder-manager` / `.installed-card`（尾部） | card-solid | 75 / 92 |
| `SkinsView.vue:756` | `.drag-hint` | accent | 18 |
| `BridgeView.vue:291/297/307` | border / 脉冲 50% / hover shadow | accent 45、ok 10、accent 10 | |
| `FrpPanel.vue:222/245/270/275/276/277` | border 族 + `.node-online.on` 环 | ok 55/40/22、accent-2 45、danger 45 | |
| `TerracottaPanel.vue:243/254/255/271` | border 族 / `color-mix(text-dim 70%, transparent)` | ok 40、accent-2 55、text-dim 70 | |
| `WorldImportModal.vue:327` | `color-mix(in srgb, #e5a323 9%, var(--card))` + border `#e5a323 45%` | 字面金 | 9 / 45 |
| `ModMigrationModal.vue:37` | `color-mix(in srgb,var(--accent) 9%,var(--card))` | accent | 9 |
| `RecordingModPicker.vue:40` | `color-mix(in srgb,var(--danger,#e05260) 10%,transparent)` | danger | 10 |
| `ReferenceLinks.vue:16` | `color-mix(in srgb,var(--accent) 14%,var(--card-2))` | accent | 14 |
| `VoxLinkPanel.vue:431` / `VoxLinkRelatedLinks.vue:20` | accent + card-2 | 8 / 15 | |
| `ContentSkeleton.vue:20` | `color-mix(in srgb,var(--text) 9%,var(--card))` | text | 9 |
| `EditPanel.vue:74` | `.designer-viewport` `color-mix(in srgb,var(--bg) 84%,#697687)` | bg + **不透明** | 84 |
| `KeysView.vue:260` | `.cfg-bind.capturing` border | accent + border | 55 |
| `ServersView.vue:472/509` | hover shadow / active border | accent 10 / 55 | |
| `FriendConnectView.vue:142/145` | `.pick-card:hover` border / shadow | accent 55 / 15 | |
| `SettingsView.vue:1257/1262/1263/1272/1273/1727` | `.upd-pending-row` / `.upd-failed-row` / `.upd-risk` / `.mem-slider-thumb` | accent 30、danger 8/26、danger 10/26、accent 45 | |
| `DefaultGameOptions.vue:80` | border `color-mix(in srgb,var(--accent) 45%,var(--border))` | accent | 45 |

**color-mix → WPF ARGB 的算术规则**（照抄即可）
1. `color-mix(in srgb, C N%, transparent)` ⇒ 结果色 = C，**alpha = N%**（C 自身 alpha=1 时）；C 已有 alpha_a 则 alpha = `N% × a`。
2. 两个操作数都不透明 ⇒ **结果不透明**（`connection.css:15`、`LaunchNotice.vue:36`、`ui-system.css:31`、`EditPanel.vue:74`、`VoxLinkPanel.vue:431`、`ModMigrationModal.vue:37`、`ReferenceLinks.vue:16`、`WorldImportModal.vue:327`、`ContentSkeleton.vue:20`、`SettingsView.vue:1257/1262` 等）。⇒ **它们上面的 blur 一律是死代码。**
3. 百分比之和 ≠ 100%（如 `95% border + 4% white`）**会被归一化**，不是"剩下 1% 透明"。
4. 嵌套令牌（`--bg-2` → `.sidebar`/`.topbar`）⇒ **alpha 相乘**，RGB 不变。
5. 混合色在 sRGB 线性空间之外（`in srgb` = 直接对 sRGB 通道做线性插值），与 WPF 的 `Color.FromArgb` 逐通道插值一致，可直算。

---

### C.9 字面 `rgba()` / `#RRGGBBAA`（按控件分组）

**外壳 / 侧栏 / 顶栏**
- `App.vue:1735` `.shell` `box-shadow: inset 0 1px 0 rgba(255,255,255,.12)`
- `App.vue:1778` `.brand-head` `border: 1px solid rgba(255,255,255,.26)`
- `App.vue:1779` `.brand-head` `box-shadow: 0 3px 12px rgba(0,0,0,.18)`
- `App.vue:1845` `.nav-bubble` `box-shadow: 0 3px 12px #00000008`（alpha `0x08/255 = .031`）

**Hero（首页）**
- `HomeView.vue:808` `.hero-card` `box-shadow: 0 18px 44px rgba(0, 0, 0, 0.22)`
- `HomeView.vue:815` `.hero-shade` 基值：`rgba(8,14,15,.66)` / `rgba(8,14,15,.24)` @54% / `rgba(8,14,15,.08)`；`rgba(5,10,9,.38)` / `transparent` @52%
- `HomeView.vue:1030` `.hero-shade` **终值**：`rgba(0,0,0,.16)` → `rgba(0,0,0,.18)` @30% → `rgba(0,0,0,.68)`
- `HomeView.vue:818` `.hero-kicker` `rgba(255,255,255,.13)`（border）+ `rgba(9,13,14,.48)`（bg）
- `HomeView.vue:819` `.hero-content h1` `text-shadow: 0 4px 24px rgba(0,0,0,.32)`
- `HomeView.vue:831` `.loader-badge` `rgba(20, 30, 24, 0.46)`（mix 基色）
- `HomeView.vue:834` `.hero-settings,.hero-more` `rgba(255,255,255,.14)` + `rgba(10,16,17,.58)`
- `HomeView.vue:839` `:hover` `rgba(19, 29, 29, 0.76)`
- `HomeView.vue:853` `.launch-progress` `rgba(255, 255, 255, 0.25)`
- `HomeView.vue:855` `.launch-arrow` `border-left: 1px solid rgba(255, 255, 255, 0.22)`

**遮罩 / 弹窗 / 灯箱**
- `InstanceCenter.vue:109` `.ic-mask` `background:#0009`（= `rgba(0,0,0,.6)`）；`.ic-dialog-mask` `background:#0008`（`.502`）；`.ic-lightbox` `background:#101418fa`（`.984`）；`.ic-lightbox .btn` `background:#293139`
- `UpdateDialogShell.vue:33` `.update-dialog-backdrop` `background:var(--mask,rgba(20,27,40,.48))`
- `EditPanel.vue:74` `.preview-size` `box-shadow:0 8px 32px #0003`（alpha `0.2`）；`.designer-viewport` 点阵 `radial-gradient(#88929d35 1px,transparent 1px)`（alpha `0x35/255 = .208`）
- `SettingsView.vue:1649` `.tp-palette` `filter: drop-shadow(0 1px 3px rgba(0, 0, 0, 0.45))`
- `HomeView.vue:959` `.card-float-menu` 兜底色 `#192225`；`InstanceCenter.vue:109` `.ic` 兜底 `#192225`；`EditPanel.vue:74` `.design-workspace` 兜底 `#202830`；`UpdateDialogShell.vue:34` `.update-dialog` 兜底 `#161c24`；`RecordingModPicker.vue:40` danger 兜底 `#e05260`

**闪屏（独立窗口）**
- `splash.css:14` `#caption` `border: 1px solid #ffffff24`（alpha `.141`）+ `background: #172021b8`（alpha `.722`）+ `color:#f5f8f6`
- `splash.ts:51` canvas 渐变 tint：`'#effaff80'`(α`.502`) → `'#badfff20'`(α`.125`) @.40 → `'#ffffff58'`(α`.345`) @.52 → `'#bcb0fa28'`(α`.157`)
- `splash.ts:53` `strokeStyle='#25334945'`（α`.271`）lineWidth `2.4`；`:54` `'#eaf7ffb8'`（α`.722`）lineWidth `.85`；`:58` `` `rgba(255,255,255,${.55 + Math.sin(elapsed / 1100 + shard.phase) * .25})` `` lineWidth `1.4`
- `startupSplash.ts:24` / `windowAppearance.ts:45` `backgroundColor:'#00000000'`（全透）

**令牌层**：`styles.css:13,15,22,26,27,28,31,32,33,34,36,37`（`:root`）与 `styles.css:91,95,96,97,100,101,102,103,105,106`（`black-orange`）。
**JS 注入的字面 rgba**：`App.vue:993,995,996,998,1000,1001,1002`（见 C.8 表）。

---

### C.10 容器级 `opacity`（影响层合成的，非 hover 微渐变）

| file:line | 选择器 | 值 | 影响 |
|---|---|---|---|
| `App.vue:886` | `.app-bg`（color 模式，inline） | `String(bg.opacity)`（0–1，默认 **0.5**，`types.ts:564`） | **整张背景层与 UI 分离** |
| `appearancePolicy.ts:20` | `.app-bg`（image 模式，inline） | `opacity: String(opacity)` + `filter: blur(...)` | 同上 |
| `HomeView.vue:810/811` | `.hero-image` / `.hero-image.active` | `0` / `1` | **轮播交叉淡入的唯一机制**（`transition: opacity var(--motion-carousel)` = 500ms，`ui-system.css:7`） |
| `useNavigationBubble.ts:59 / :63` | `.nav-bubble` / `.nav-selection` | inline `opacity: visible?1:0` | 侧栏两个独立层各自淡入淡出（120ms，`App.vue:1846/1857`） |
| `CommunityView.vue:94` / `GameView.vue:123` / `SkinsView.vue:86` | `.capsule-blob` / `.game-tabs-blob` / `.seg-blob` | inline `opacity: on?1:0` | 三处「跟随滑块」层的显隐 |
| `HomeView.vue:866/870` | `.runtime-blob` | `0` → `.on{opacity:1; transform:scale(1)}` | 同上 |
| `ui-system.css:43` | `[inert]` | `.65` | **收起的整棵子树**（如资源管理子菜单 `App.vue:1285`）⇒ 一个容器级压暗层 |
| `ui-system.css:105-109` | `@starting-style` 长列表（`.cfg-page>.card, .settings-body>*, .game-controls, .list-card, .installed-card, .folder-manager-actions, .notice-panel, .float-menu, .instance-more-menu, .menu, .context-menu, .modal-mask, .modal, .ic-mask, .ic-dialog-mask, .connection-panel, .settings-search-results, .options-entrances, .tab-body, .ic-content, .connection-result`） | 起始 `opacity:0` → `:100-104` `transition:opacity 200ms ease` | **所有浮层/面板的入场淡入**（WPF 对应 `BeginAnimation(OpacityProperty)`） |
| `App.vue:1899/1905/1916/1922` | `.nav-sub` / `.nav-sub.open` / `.nav-sub .nav-sub-item` / `.open .nav-sub-item` | `0`/`1`/`0`/`1` | 子菜单容器 + 子项**双层淡入**（同时 `grid-template-rows:0fr→1fr`） |
| `SplashScreen.vue:138/142` | `.splash` / `.splash.leaving` | `1` / `0`（`transition:opacity 400ms ease`） | 应用内闪屏整体淡出 |
| `splash.css:18/19` | `body.leaving` | `0`（`transition: opacity 320ms ease`；reduced-motion → 1ms） | **闪屏窗口体**淡出 |
| `nativeStartup.ts:7,14` | **整个窗口** `setOpacity()` | `0 → 1`（smoothstep `p*p*(3-2*p)`） | 原生窗口 alpha，不在 CSS 里 |
| `SettingsView.vue:1641-1642` | `.tp-custom-grad` | `.85`（压在 `:1638 background:#16161d` 上） | 主题预览自定义色块 |
| `CommunityView.vue:930` / `SkinsView.vue:746/917` / `BridgeView.vue:313` / `FileManager.vue:606/626/671` / `App.vue:1799/1890/1944/2052` | 各处次级元素 | `.6`/`.8`/`.55`/`.55–.8`/`.85`/`.7`/`.4`/`.62` | 装饰性弱化，不改变叠层次序 |
| `HomeLayoutEditor.vue:291-303` | 「图片透明度」滑杆 | UI 值 = `1 - settings.background.opacity`（**反向滑杆**） | 见 E.1 |

其余为控件态 `:disabled`（全站 `.45`：`styles.css:192/:295/:626`、`HomeView.vue:840/911/961`；`.input:disabled .7` `styles.css:329`；`.switch-ui .65` `styles.css:407`；`.top-back:disabled .62` `App.vue:2052`；`SelectMenu.vue:127 .55`；`HomeView.vue:944 .4`；`DefaultGameOptions.vue:80 .65`）与过渡起止值，不计入叠层。

---

### C.11 `filter:` 全量

| file:line | 选择器 | 声明 |
|---|---|---|
| `appearancePolicy.ts:20` | `.app-bg`（inline，图片模式） | `filter: blur(${blur}px)`，blur ∈ [0,40]；blur=0 时写 `'none'` |
| `styles.css:207` | `.btn-gold:hover:not(:disabled)` | `filter: brightness(1.08);` |
| `styles.css:781` | `@media (hover: hover) button:not(:disabled):not([aria-disabled="true"]):hover` | `filter: brightness(1.08);` ← **全站按钮通用提亮层** |
| `HomeView.vue:854` | `.launch-main:disabled` | `filter: saturate(0.75);` |
| `HomeView.vue:856` | `.launch-arrow:hover, .launch-main:hover:not(:disabled)` | `filter: brightness(1.08);` |
| `GameView.vue:1863` | `.mirror-toggle:hover` | `filter: brightness(1.15);` |
| `SettingsView.vue:1649` | `.tp-palette` | `filter: drop-shadow(0 1px 3px rgba(0, 0, 0, 0.45));` |
| `SettingsView.vue:1660` | `.personalize-btn:hover:not(:disabled)` | `filter: brightness(1.06);` |
| `styles.css:174` | `.btn` 的 `transition` 列表含 `filter` | 过渡声明，非实际 filter |
| **未找到** | `contrast()`、`grayscale()`、`sepia()`、`hue-rotate()`、`invert()`、`opacity()` 函数式 | — |

---

## D 阴影与描边环

### D.12 `box-shadow`（含 `inset`、多层、`color-mix` 版；`inset` 用 **★** 标）

#### 令牌定义 + JS 覆盖
| file:line | 令牌 | 值 |
|---|---|---|
| `styles.css:26 / :27` | `--shadow` / `--shadow-lg`（首帧） | `0 8px 24px rgba(0, 0, 0, 0.22)` / `0 24px 70px rgba(0, 0, 0, 0.46)` |
| `styles.css:95 / :96` | 同上（`[data-theme='black-orange']`） | `0 2px 12px rgba(0, 0, 0, 0.35)` / `0 10px 30px rgba(0, 0, 0, 0.5)` |
| **`App.vue:1001 / :1002`** | **最终生效（inline）** | 暗 `0 2px 8px rgba(0,0,0,.16)` / 亮 `0 2px 8px rgba(31,50,85,.045)`；`0 20px 55px rgba(0,0,0,.42)` / `0 20px 55px rgba(31,50,85,.18)` |

#### 外壳族
| file:line | 选择器 | 声明 |
|---|---|---|
| `App.vue:1735` ★ | `.shell` | `inset 0 1px 0 rgba(255,255,255,.12)` |
| `App.vue:1779` | `.brand-head` | `0 3px 12px rgba(0,0,0,.18)` |
| `App.vue:1845` | `.nav-bubble` | `0 3px 12px #00000008` |
| `App.vue:1855` ★ | `.nav-selection` | `inset 3px 0 0 var(--accent)` |
| `App.vue:1987 / :1991 / :1995` | `.sidebar-health i` / `.is-busy i` / `.is-error i` | `0 0 0 3px var(--ok-soft / --accent-soft / --danger-soft)` |
| `App.vue:2192` | `.notice-panel` | `var(--shadow-lg)` |
| `App.vue:2520` ★★ | `.mp-name-opt.active .mp-radio` | `inset 0 0 0 3px var(--card), inset 0 0 0 8px var(--accent)` **双内环（单选点）** |
| `styles.css:730` | `.edit-tip` | `var(--shadow-lg)` |
| `styles.css:553 / :592` | `.modal` / `.float-menu` | `var(--shadow-lg)` |

#### 按钮 / 输入 / 焦点环（`styles.css`）
| file:line | 选择器 | 声明 |
|---|---|---|
| `styles.css:181` | `.btn:focus-visible` | `0 0 0 3px var(--accent-soft)`（配 `outline:none` `:180`） |
| `styles.css:204 / :208` | `.btn-gold` / `:hover` | `0 2px 6px var(--accent-soft)` / `0 3px 10px var(--accent-soft)` |
| `styles.css:216` | `.btn-gold:focus-visible` | `0 0 0 3px var(--accent-soft), 0 4px 16px var(--accent-soft)` **双层** |
| `styles.css:229 / :285` | `.btn-ghost:hover` / `.icon-btn:hover` | `box-shadow:none`（**主动撤销**焦点环） |
| `styles.css:236 / :292` | `.btn-ghost:focus-visible` / `.icon-btn:focus-visible` | `0 0 0 3px var(--accent-soft)` |
| `styles.css:248 / :255` | `.btn-danger:hover` / `:focus-visible` | `0 4px 14px color-mix(danger 14%,transparent)` / `0 0 0 3px color-mix(danger 22%,transparent)` |
| `styles.css:325` | `.input:focus, .select:focus` | `0 0 0 3px var(--accent-soft)` |
| `styles.css:404` | `.switch input:focus-visible + .switch-ui` | `0 0 0 3px var(--accent-soft)` |
| `styles.css:434` | `.slider::-webkit-slider-thumb` | `0 2px 8px var(--accent-soft)` |
| `styles.css:455` | `.card` | `var(--shadow)` → **被 `ui-system.css:13` 的 `box-shadow:none` 覆盖** |
| `styles.css:786` | `@media(hover:hover) .btn-ghost:hover` | `0 3px 12px var(--accent-soft)` |
| `ui-system.css:115 / :116` | `.content :is(.tool-search,.server-search):focus-within` / 其 `input:focus-visible` | `0 0 0 3px var(--accent-soft)` / `outline:none;box-shadow:none` |
| `ui-system.css:13 / :21` | `.content .card` / `.content .card .card` | `box-shadow:none` |
| `ui-system.css:47 / :77 / :78` | `.result-dl` / `.connection-details` / `.connection-panel`（及嵌套） | `box-shadow:none` |
| `ui-system.css:119` | `html[data-theme="transparent"] .btn-gold` | `box-shadow:none` |

#### 首页（含尾部压缩覆盖）
| file:line | 选择器 | 声明 | 最终 |
|---|---|---|---|
| `HomeView.vue:808` | `.hero-card` | `0 18px 44px rgba(0, 0, 0, 0.22)` | 生效（**全站唯一的大柔光外投影**） |
| `HomeView.vue:843` ★ | `.launch-combo` | `0 12px 32px color-mix(in srgb, var(--accent) 38%, transparent), 0 2px 0 color-mix(in srgb, white 14%, transparent) inset` | 生效 |
| `HomeView.vue:846` ★ | `.launch-combo:hover` | `0 16px 40px color-mix(…, var(--accent) 46%, transparent), 0 2px 0 color-mix(…, white 16%, transparent) inset` | 生效 |
| `HomeView.vue:860` | `.runtime-strip` | `var(--shadow)` | 生效 |
| `HomeView.vue:882 / :884` | `.runtime-state.ready i,.running i` / `.error i` | `0 0 0 3px var(--ok-soft)` / `0 0 0 3px var(--danger-soft)` | 生效 |
| `HomeView.vue:898` | `.instance-card:hover` | `var(--shadow)` | **被 `:1030` 的 `box-shadow:none` 覆盖** |
| `HomeView.vue:899` ★ | `.instance-card.selected` | `inset 0 0 0 1px var(--accent)` | **被 `:1030` `.home-dashboard .instance-card{box-shadow:none}` 覆盖**（同特异度 0,2,0，`:1030` 在后）⇒ **选中态只剩 `background:var(--accent-soft)`，没有描边** |
| `HomeView.vue:916` ★ | `.home-creator` | `inset 0 0 0 1px color-mix(in srgb, var(--accent) 10%, transparent), var(--shadow)` | **被 `:1030` `.home-creator{box-shadow:none}` 覆盖** |
| `HomeView.vue:917` | `.account-panel,.skin-panel` | `var(--shadow)` | **被 `:1030` 双双 `box-shadow:none`** |
| `HomeView.vue:920` | `.account-head :deep(.mc-avatar)` | `0 0 0 4px color-mix(in srgb, var(--text) 8%, transparent)` **头像环** | 生效 |
| `HomeView.vue:924` | `.account-copy span i` | `0 0 0 3px var(--ok-soft)` | 生效 |
| `HomeView.vue:958 / :969` | `.float-menu`(scoped) / `.log-dialog` | `var(--shadow-lg)` | 生效 |
| `HomeView.vue:974/975` | `.log-close:hover` / `.log-mask` | 无阴影 | — |

#### 其余组件
| file:line | 选择器 | 声明 |
|---|---|---|
| `Toasts.vue:36` | `.toast` | `var(--shadow-lg)` |
| `SelectMenu.vue:126 / :143` | `.select-menu-btn.open` / `.select-menu-float` | `0 0 0 3px var(--accent-soft)` / `var(--shadow)` |
| `InstanceCenter.vue:109` | `.ic` | `var(--shadow-lg)` |
| `UpdateDialogShell.vue:34` | `.update-dialog` | `var(--shadow-lg)`（原文 `box-shadow:var(…)` 缩写，值同 `--shadow-lg`） |
| `EditPanel.vue:74` | `.designer-dialog` / `.preview-size` | `var(--shadow-lg)` / `0 8px 32px #0003` |
| `MinecraftVersionPicker.vue:26` | `.version-picker-popup` | `var(--shadow-lg)` |
| `LaunchNotice.vue:36` | `.launch-success` | `var(--shadow)` |
| `FileManager.vue:488` | `.file-more-actions` | `var(--shadow)` |
| `SettingsView.vue:1845` | `.settings-search-results` | `var(--shadow)` |
| `SettingsView.vue:1566 → :1867` | `.theme-option.active .theme-preview` | `0 0 0 3px var(--accent-soft)` → **尾部 `box-shadow:none`** |
| `SettingsView.vue:1727` | `.mem-slider-thumb` | `0 1px 6px color-mix(in srgb, var(--accent) 45%, transparent)` |
| `SettingsView.vue:1233/1845` | `.settings-navigation` / `.settings-search-results` | **无阴影**（靠不透明底 + border 分层） |
| `IconPickerModal.vue:131` | 选中项 | `0 0 0 2px var(--accent-soft)` |
| `GameView.vue:2168` ★ | `.game-tabs-blob`（尾部，覆盖 `:1885-1887` 的 `0 2px 8px var(--accent-soft)`） | `inset 0 -2px var(--accent)` |
| `GameView.vue:2171 / :2178` | `.folder-manager` / `.installed-card` | `box-shadow:none` |
| `GameView.vue:1654` | `.tool-search:focus-within` | `0 0 0 3px var(--accent-soft)` |
| `CommunityView.vue:750 → :1083` | `.capsule-blob` | `0 2px 8px var(--accent-soft)` → **尾部 `box-shadow:none!important`** |
| `CommunityView.vue:846-847 / :868 / :1027` | `.result-card:hover` / `.result-icon` / `.file-row.active` ★ | `var(--shadow)` / `0 2px 8px color-mix(accent 8%,transparent)` / `inset 0 0 0 1px var(--accent), 0 4px 14px var(--accent-soft)` **★内外双用** |
| `CommunityView.vue:818-822 / :1082` | `.list-card` / `.result-card` | `background:transparent; border:0; box-shadow:none` / 尾部 `box-shadow:none` |
| `BridgeView.vue:294 / :296 / :297 / :307` | `.bridge-dot.on` / `@keyframes bridge-pulse` / `.bridge-card:hover` | `0 0 0 3px var(--ok-soft)` → 50% `0 0 0 6px color-mix(ok 10%,transparent)` **脉冲环** / `0 10px 26px color-mix(accent 10%,transparent)` |
| `FrpPanel.vue:270` | `.node-online.on` | `0 0 0 3px color-mix(in srgb, var(--ok) 22%, transparent)` |
| `ServersView.vue:472 / :474 ★ / :515` | `.server-list-item:hover` / `.active` / **尾部 `.active`** | `0 8px 22px color-mix(accent 10%,transparent)` / `inset 3px 0 var(--accent)` + 双层渐变底 → **`:515` 覆盖为 `box-shadow:none; background:var(--accent-soft); border-radius:var(--radius-sm); border-bottom-color:var(--border)`** ⇒ **`:474` 的双层渐变与 inset 竖条全是死代码** |
| `FriendConnectView.vue:142 / :161` | `.pick-card:hover` / 尾部 | `0 14px 34px color-mix(accent 15%,transparent)` → **`:161` `.pick-card{box-shadow:none}.pick-card:hover{box-shadow:none}` 覆盖** |
| `SkinsView.vue:662 / :1089` | focus / 尾部 `.pane-info,.pane-capes` | `0 0 0 3px var(--accent-soft)` / `box-shadow:none` |
| `VoxLinkPanel.vue:429 ★` | `.address-hero` | `inset 3px 0 var(--accent)` |
| `KeysView.vue:261` | `.cfg-bind.capturing` | `0 0 0 3px var(--accent-soft)` |
| `TerracottaPanel.vue`、`ConnectionPanel/Status`、`ModsView/PacksView/ShadersView`、`RecordingsView.vue:92` | — | **无自定义 box-shadow**（继承 `.card` ⇒ 最终 `none`） |

**"去阴影"趋势（做减法的依据）**：`.content .card`(`ui-system.css:13`)、`.content .card .card`(`:21`)、`.connection-panel`(`:78`)、`.connection-details`(`:77`)、`.result-dl`(`:47`)、`.instance-card`/`.instance-card:hover`/`.instance-card.selected`(`HomeView:1030`)、`.account-panel`/`.skin-panel`(`:917`→`:1030`)、`.home-creator`(`:1030`)、`.result-card`(`CommunityView:1082`)、`.capsule-blob`(`:1083 !important`)、`.pick-card`(`FriendConnectView:161`)、`.folder-manager`/`.installed-card`(`GameView:2171/2178`)、`.theme-preview`(`SettingsView:1867`)、`.btn-gold`(默认主题 `ui-system.css:119`)、`.server-list-item.active`(`ServersView:515`) 全部被改为 `box-shadow:none`。
⇒ **WPF 里只给浮层加 `DropShadowEffect`（modal / float-menu / notice-panel / toast / version-picker-popup / designer-dialog / ic / update-dialog / hero-card / log-dialog），普通卡一律不加。**

---

### D.13 `text-shadow`（全站仅 2 处命中）

| file:line | 选择器 | 声明 |
|---|---|---|
| `HomeView.vue:819` | `.hero-content h1` | `text-shadow: 0 4px 24px rgba(0, 0, 0, 0.32);` |
| `CommunityView.vue:1083` | `.community-page .capsule.active` | `text-shadow:none;`（**撤销**，说明曾有隐式值） |

`HomeView.vue:1030` 尾部重写了 `.hero-content h1` 的 `font-size` / `line-height` / `-webkit-line-clamp` / `overflow`，但**没有重写 `text-shadow`** ⇒ `0 4px 24px rgba(0,0,0,.32)` **仍然生效**。

---

### D.14 作为**视觉环**使用的 `outline`

**焦点环（键盘可访问性）**
- `styles.css:654-657` `:where(button, a, [role='button'], input, select, textarea, [tabindex]):focus-visible { outline: 2px solid var(--accent); outline-offset: 3px; }`（`:where()` 特异度为 0 ⇒ 处处可被覆盖）
- `styles.css:658` `.slider:focus-visible { outline: 2px solid var(--accent); outline-offset: 4px; }`
- `styles.css:789` `button:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }`
- `ui-system.css:75` `.content :is(button,input,select,textarea,summary,[tabindex]):focus-visible{outline:2px solid var(--accent);outline-offset:3px}`（`.content` 前缀 ⇒ 0,2,1，胜 `styles.css:789`）
- `connection.css:12/62/80`、`FriendConnectView.vue:144`、`ServersView.vue:476`、`ReferenceLinks.vue:17`、`ServerListItem.vue:22`、`SettingsView.vue:1242/1307`、`KeysView.vue:232`、`GameView.vue:2200`、`CommunityView.vue` 各处：`2px solid var(--accent)` 或 `var(--accent-2)` + `outline-offset: 2–4px`
- **负偏移内嵌环**（唯一一处）：`ServersView.vue:476` `outline-offset: calc(var(--space-1) * -1)` = **-4px**
- `styles.css:180/215/235/254/291/315/422`、`ui-system.css:116`、`GameView.vue:1665`、`UpdateDialogShell.vue:34`、`ServersView.vue:455` 用 `outline:none`/`0` **把焦点环换成 box-shadow 环**（见 D.12）

**非焦点的装饰 / 状态环（这些是真·叠加层）**
| file:line | 选择器 | 声明 |
|---|---|---|
| `styles.css:703-706` | `.legacy-edit-mode [data-edit]:hover` | `outline: 2px dashed var(--accent); outline-offset: 2px;`（个性化编辑模式悬停虚线框） |
| `styles.css:708-712` | `.legacy-edit-mode [data-edit].edit-active, …:hover` | `outline: 3px solid var(--accent); outline-offset: 2px;` **选中板块**（`:710` 注释"优先于 hover"） |
| `visualDesign.ts:104` | `style.hidden` 且 `store.editMode` | `properties.opacity='0.25'` + `properties.outline='1px dashed #ff9e52'` **隐藏元素虚线框**（非 editMode 时 `display:none` `:104`） |
| `SkinsView.vue:1102` | `.row-main.drag-over .pane` | `outline:2px solid var(--accent);` **拖拽悬停环**（同场景另有 `:662` `.row-main.drag-over{box-shadow:0 0 0 3px var(--accent-soft)}` ⇒ **环 + 阴影双保险**） |
| `KeysView.vue:232` | `.default-packs.drag-active` | `outline: 2px solid var(--accent); background: var(--accent-soft);` |
| `EditPanel.vue:74` | `.designer-outline` | `position:fixed;z-index:2;pointer-events:none;border:2px solid var(--accent)`（用 border 不是 outline；其 `span` 是 `top:-22px` 的 accent 标签，`button` 是 `bottom:-6px;right:-6px;12×12` 的 resize 把手，`border:2px solid white`） |
| `EditPanel.vue:74` | `.insertion,.guide` | `position:fixed;background:var(--accent);pointer-events:none;z-index:2`；`.vertical{width:1px;top:66px;bottom:0}`、`.horizontal{height:1px;left:230px;right:310px}` **设计器对齐参考线** |
| `FileManager.vue:719` | `.fm-context .fm-path:focus-visible` | `outline:2px solid var(--accent);outline-offset:4px` |

---

## E 图片层

### E.1 用户背景 / 外观系统（`.app-bg` / `has-bg` / `managedImageUrl`）

**数据模型**（`src/shared/types.ts:527-547` + `:560-566`）
```ts
export interface BackgroundSettings {
  mode: 'none' | 'color' | 'image'
  color: string; image: string; images?: string[]
  switchMode?: 'off' | 'order' | 'random'
  switchIntervalSec?: number
  opacity: number      // 0-1 背景不透明度
  blur: number         // 0-40 模糊度 px
  fit: ImageFit        // 'fill' | 'fit' | 'crop'
}
export type ImageFit = 'fill' | 'fit' | 'crop'                       // types.ts:547
export const DEFAULT_BACKGROUND: BackgroundSettings = {
  mode: 'none', color: '#1b2437', image: '', opacity: 0.5, blur: 0, fit: 'crop' }   // types.ts:560-566
```
设置键：`settings.background`、`settings.launchThumbnail`（`types.ts:406`、`:549-558`）、`settings.theme`、`settings.custom`、`settings.homeLayout`、`settings.visualDesign` —— 完整集合见 `visualDesign.ts:5`：
```ts
type Appearance=Pick<Settings,'theme'|'custom'|'background'|'launchThumbnail'|'homeLayout'|'visualDesign'>
```

**DOM 与 CSS**
- 挂载点：`App.vue:1229` `<div v-if="bgStyle" class="app-bg" :style="bgStyle"></div>` —— 在 `.shell` **之前**，同属 `App.vue:1227` 的 `Teleport`（design 模式进 `#design-preview-host`，否则进 `body`）。
- 定位：`App.vue:1738-1743` `.app-bg{position:fixed; inset:0; z-index:0; pointer-events:none}`。
- 设计器忽略它：`visualDesign.ts:69` `if(el.closest('[data-design-tools]')||el.closest('.edit-tip')||el.classList.contains('app-bg'))continue`
- **`has-bg` 是 NO-OP**：`App.vue:1232` `:class="{ 'edit-mode': store.editMode, 'has-bg': !!bgStyle }"`，而 `App.vue:1745-1747` `.shell.has-bg { background: var(--shell-surface); }` —— **与 `App.vue:1729` `.shell` 完全同值**。注释说"激活自定义背景时界面底色透出背景层"，实际透出靠的是 `--shell-surface` 自身 alpha，与这个类无关。WPF 不需要为它做分支。

**bgStyle 计算**（`App.vue:880-902`，逐字要点）
```ts
const bgStyle = computed(() => {
  const bg = appearancePreview.value?.background
  if (!bg || bg.mode === 'none') return null
  if (bg.mode === 'color') return { background: bg.color, opacity: String(bg.opacity) }
  if (bg.mode === 'image' && currentBgImage.value && failedBackground.value !== currentBgImage.value) {
    const size = bg.fit === 'fill' ? '100% 100%' : bg.fit === 'fit' ? 'contain' : 'cover'
    return {
      inset: bg.blur > 0 ? `${-Math.ceil(bg.blur * 1.5)}px` : '0',   // ← 关键：负缩进吃掉 blur 边缘
      backgroundColor: bg.color,
      backgroundImage: `url("${managedImageUrl(currentBgImage.value)}")`,
      backgroundSize: size, backgroundPosition: 'center', backgroundRepeat: 'no-repeat',
      ...backgroundImageEffect(bg)
    }
  }
  return null
})
```
`src/shared/appearancePolicy.ts:17-21`
```ts
export function backgroundImageEffect(bg: Pick<BackgroundSettings, 'opacity' | 'blur'>) {
  const opacity = Math.max(0, Math.min(1, Number.isFinite(bg.opacity) ? bg.opacity : .5))
  const blur = Math.max(0, Math.min(40, Number.isFinite(bg.blur) ? bg.blur : 0))
  return { opacity: String(opacity), filter: blur > 0 ? `blur(${blur}px)` : 'none' }
}
```

**合成方式总结（这就是"图片层怎么垫在 UI 下"的答案）**
1. **没有 mask、没有 overlay tint**（`grep -rn "mask-image" src/renderer/` → **0 命中**）。
2. 只靠三件事：**(a) `opacity: bg.opacity`（默认 0.5）→ 让上层 `.shell` 的 `--shell-surface` 与之混合；(b) `filter: blur(Npx)` 糊图片自身；(c) `inset: -ceil(N×1.5)px` 负缩进，把 blur 造成的透明边缘照掉**（`App.vue:892`）。
3. 图片下方还有 `backgroundColor: bg.color`（`App.vue:894`）⇒ **同一元素内两小层：color 底 + image 顶**。
4. 完整链：**桌面 → DWM Acrylic（含系统 tint）→ `.app-bg`(color + image, opacity, blur) → `.shell`(--shell-surface + inset 高光 + border) → `.sidebar`/`.topbar`/`.content` → 卡片/浮层**。
5. 失败回退：`App.vue:851-868` 用 `new Image()` 探测，`onerror` 时 `failedBackground.value = imagePath`（`:862`）+ `toast('自定义背景不可用，已回退到主题默认背景', 'error')`（`:863`）⇒ `bgStyle` 变 `null`，`.app-bg` 整个节点从 DOM 移除（`v-if`）。
6. 多图轮换：`bgImages`（`App.vue:812-816`，`images` 优先、否则回退单张 `image`）→ `currentBgIndex` / `currentBgImage`（`:817-819`）→ `switchBackground()`（`:822-834`，`random` 用 while 循环保证不重复当前）→ `armBgSwitchTimer()`（`:837-849`，`Math.max(30, switchIntervalSec ?? 300)` 秒；`document.hidden` 时跳过）；持久化最后索引 `localStorage['kamucl:bg-last-index']`（`App.vue:817`、`:833`、`:1057-1060`）。
7. 图片 URL 受管：`managedAssets.ts:2-7`
   ```ts
   /** Build an opaque URL for a local image that the main process has explicitly authorized. */
   export function managedImageUrl(filePath: string): string {
     if (!filePath) return ''
     const url = new URL('kamucl-asset://local/image')
     url.searchParams.set('path', filePath)
     return url.toString()
   }
   ```
   ⇒ 自定义协议 `kamucl-asset://local/image?path=…`（主进程白名单）。WPF 侧对应 `StreamResource` / 自定义 `IValueConverter` 读受管目录即可，不需要协议。

**设置 UI**（`components/HomeLayoutEditor.vue`，由 `SettingsView.vue:713` `<div class="background-settings"><HomeLayoutEditor /></div>` 挂载；`SettingsView.vue:1861` `.settings-body .background-settings { order:2;display:grid;gap:12px; }`）
- `<details class="card group layout-setting" data-section="background">` `:183`，`summary` 文案「窗口背景 / 桌面玻璃、颜色与图片」；折叠箭头是 `::after{content:'⌄'}`（`:496-497`）
- 模式胶囊 `:195-205`（`.bg-modes` `:397-401`，`.capsule` `:402` `background:var(--card-2)` + border；`.capsule.active` `:403` `background:var(--accent-soft);border-color:var(--accent)`）
- 背景色 `<input type=color class="color-swatch">` `:209-216`（样式 `:471-487`）
- 预览 `.image-preview` `:221-231`（基础样式 `:432-447`：`height:150px;overflow:hidden;border:1px solid var(--border);border-radius:var(--radius);background:var(--card-2)`；尾部 `:501` 压成 `height:90px;max-width:320px`）
- **反向滑杆**「图片透明度」`:290-303`：`type=range min=0 max=1 step=0.05`，`:value="1 - store.settings.background.opacity"`，`@input="setBg({ opacity: 1 - Number(...) })"`，显示 `Math.round((1 - opacity) * 100) + '%'` ⇒ **滑杆显示"透明%"，存的是"不透明%"**。WPF Slider 必须照抄这个反转。
- 模糊滑杆 `:305-317`：`min=0 max=40 step=2`，直接绑 `blur`，显示 `{{ blur }}px`。
- 说明文案 `:319`：`透明度越高图片越透；图片模糊单独控制清晰度。系统桌面毛玻璃由操作系统管理，不受这两个图片选项影响。`
- macOS 降低透明度提示 `:191-193`（`reducedTransparency`）。

### E.2 启动缩略图 / hero 图（第二个图片层来源）

- 三级优先（`HomeView.vue:75-97`，逐字）
  ```ts
  const customBanners = computed(() => {
    const version = currentVersion.value
    if (version?.thumbnail) {
      return [{ path: version.thumbnail, src: managedImageUrl(version.thumbnail), fit: version.thumbnailFit ?? ('crop' as ImageFit) }]
    }
    const global = appearancePreview.value?.launchThumbnail
    return carouselImages(global).map(path => ({ path, src: managedImageUrl(path), fit: global?.fit ?? 'crop' as ImageFit }))
  })
  const banners = computed(() => {
    const custom = customBanners.value.filter(item => !failedBanners.value.has(item.path))
    if (custom.length) return custom.map(item => ({ ...item, fit: fitCss(item.fit), custom: true }))
    return builtInBanners.map((src) => ({ src, fit: 'cover' as const, custom: false, path: src }))
  })
  ```
  ⇒ **实例专属图 > 全局轮播 > 内置 4 张**（`HomeView.vue:57` `const builtInBanners = [banner1, banner2, banner3, banner4]`）。
- 渲染：`HomeView.vue:481-491` `v-for="(item,index) in banners"` **全部同时挂在 DOM 里**，`.hero-image{opacity:0}` + `.active{opacity:1}`（`:810-811`）⇒ **N 张图叠在同一位置，只有 1 张不透明**，切换 = 500ms 交叉淡入。
- 尾部修正：`HomeView.vue:1033` `.hero-card .hero-image{object-position:center 42%}` ⇒ 所有 hero 图**向上偏 8%**（`object-fit` 由 `:487` inline 给）。
- 轮播参数：`shared/appearancePolicy.ts:3-15` `MAX_CAROUSEL_IMAGES = 20`、`carouselDuration()` clamp 1..120 默认 `6.5` 秒、`carouselTiming()`、`carouselImages()`（去重 + 截断）；`shared/carouselPlayback.ts` 的 `CarouselPlayback`（`HomeView.vue:125-130`，key = `'kamucl.carousel.' + bannerScope`）保证刷新后停在同一张。
- 预加载防闪：`HomeView.vue:104-112` `const readyBanners = new Set<string>()` + `preloadBanner()`；`:137` 下一张未就绪则本拍不切。
- 减少动效：`HomeView.vue:98-99` `const { decorativeActive } = useMotion()` → `watch(decorativeActive, active => active ? startBannerTimer() : stopBannerTimer())`；`motion.ts` + `html[data-motion="reduced"]`（`ui-system.css:54-57`、`:120`）。

### E.3 其他图片层

| 位置 | file:line | 叠法 |
|---|---|---|
| **闪屏（独立窗口，canvas）** | `splash.css:13` `canvas { position: fixed; inset: 0; width: 100%; height: 100%; }`；`splash.css:12` `html,body{background:transparent;pointer-events:none}`；`splash.ts:7` `getContext('2d',{alpha:true})` | 碎块玻璃风：每片贴图 `globalAlpha = .12+.88·p`（`splash.ts:47`）→ 再叠一层线性 tint `globalAlpha = 1-p`（`:49-52`，四色标 `#effaff80/#badfff20/#ffffff58/#bcb0fa28`）→ 两道描边（暗 `#25334945` lw2.4 `:53`、亮 `#eaf7ffb8` lw.85 `:54`）→ 动态斜边高光 `rgba(255,255,255,.55±.25)` lw1.4（`:56-58`）。**每张碎片自身就有 5 个绘制层。** |
| 闪屏文字条 | `splash.css:14-17` `#caption` `220×86`、`radius 14`、`background:#172021b8`（α .722）、`border:1px solid #ffffff24`（α .141）、`color:#f5f8f6`，居中于 `top: calc(50% + 135px)` | **无 backdrop-filter**，纯半透明 pill |
| 应用内 Splash 覆盖层 | `SplashScreen.vue:129-143` | `.splash{position:fixed;inset:0;z-index:99999;background:var(--bg)}`（**不透明**）+ `.leaving{opacity:0;transition:opacity 400ms ease;pointer-events:none}`；`.board` `288×288`（`:146-150`）；`.tile` `24×24`、`background-size:288px 288px`、`image-rendering:pixelated`、`will-change:transform`（`:157-166`）；`.brand` `opacity 0→1` + `translateY(10px→0)`（`:207-221`） |
| `.brand-head` 侧栏头像 | `App.vue:1772-1780` | `42×42` + `image-rendering: pixelated` + `1px rgba(255,255,255,.26)` 环 + `0 3px 12px rgba(0,0,0,.18)` |
| `.mc-avatar` | `Avatar.vue:48-51` + `:54-68` + `HomeView.vue:920` | 两态互斥：图片版（`image-rendering:pixelated`，radius `--radius-md`）/ 首字母版（`background:var(--accent-grad)`，radius **50%**）；首页另加 `border-radius:12px` + `0 0 0 4px color-mix(text 8%, transparent)` 环 |
| `.instance-icon` | `HomeView.vue:900-901`、模板 `:583-584` | `36×36`，`.image{object-fit:contain; image-rendering:pixelated}`；无图时内联 SVG 草方块（7 色多边形） |
| `.hero-card` 底 | `HomeView.vue:807` | `background: #17231f` **不透明兜底**，图片全失败时看到它 |
| `.skin-stage` 3D 画布 | `HomeView.vue:946-947`、模板 `:647-651`；`SkinViewer3D.vue:125` | 第 1 层 = 双层渐变底（`:946`），第 2 层 = `:deep(.viewer3d){background:transparent}` 的 WebGL canvas，第 3 层 = `.viewer3d-fallback{position:absolute;inset:0}`（加载失败），第 4 层 = `.skin-overlay`（`color-mix(card 78%)` + **`blur(12px)`** + `z-index:2`，三态互斥） |
| `.hero-settings svg` / `.launch-content svg` | `HomeView.vue:836/851` | 图标层压在 blur pill 上 |
| `.ic-shot img` | `InstanceCenter.vue:109` | `.ic-shot{border:1px solid var(--border);border-radius:12px;padding:0;overflow:hidden;background:var(--card)}` + `img{width:100%;aspect-ratio:16/9;object-fit:cover}` |
| `.carousel-list img` / `.built-in-previews img` | `HomeLayoutEditor.vue:462` / `:476` | `64×40 object-fit:cover radius:--radius-sm` / `width:100%;aspect-ratio:16/10;object-fit:cover`（尾部 `:503` 压成 `height:60px`） |
| `.result-icon img` | `CommunityView.vue:863-878` | `100%/100%;object-fit:cover;display:block`，容器 `46×46`，`background:var(--card); border:1px solid var(--border); box-shadow:0 2px 8px color-mix(accent 8%,transparent); overflow:hidden` |
| `.fm-file-icon img` | `FileManager.vue:719` | `36×36`（mods 视图 `32×32`），`object-fit:contain` |
| `.cape-img` / `.history-img` | `SkinsView.vue:1099 / :990` | `max-width/height:100%;object-fit:contain` / `height:144px;image-rendering:pixelated`，其上叠 `.history-overlay{inset:0;background:var(--mask);opacity:0→1 on hover}`（`:998-1010`） |
| `.theme-preview`（主题缩略迷你界面） | `SettingsView.vue:1552-1556` + `:1865` | 自身是一个**多层微型 UI**：`.tp-side`(色带) + `.tp-dot` + `.tp-top` + `.tp-body` + `.tp-btn` + `.tp-check`(`:1622-1631` `background:var(--accent);color:var(--on-accent);border-radius:50%`) 或 `.tp-custom-grad`(彩虹，α .85) + `.tp-palette`(白 + `drop-shadow`)；`overflow:hidden`；尾部 `:1865` 把它从 `150×84` 压成 `46×32`，`:1867` 去掉选中阴影 |

---

## F 每个界面的层数清单

> **「层」= 一个会独立参与合成的绘制面**（有 background / box-shadow / filter / 图片 之一）。纯排版容器（只给 `isolation`、`z-index`、padding）不计入。文本与图标在最后备注。
> alpha 一律给**黑紫默认主题**（`--shell-surface` 96%、`--card` 88%、`--card-2` 82%、`surface-content` 94%、`surface-raised` 97%），括号里标**蓝白 light 主题**（shell 42% / card 94% / card-2 91%）。

| # | 界面 | 层数 | 由下而上的完整叠层（含 alpha 与 blur） |
|---|---|---|---|
| **1** | **窗口外壳 `.shell`** | **5**（无背景图时 4） | ⓪ **DWM Acrylic + 系统 tint**（非 DOM；`windowAppearance.ts:46`、`nativeAppearance.ts:16-21`）→ ① **`.app-bg`（可选）**：`color+image` 两小层，`opacity .5` 默认，可选 `blur(0–40px)`，负缩进 `-ceil(blur×1.5)px`（`App.vue:880-902`、`appearancePolicy.ts:20`）→ ② `.shell` 底色 **`#F5212121`(96%)**（蓝白 `#6BEDF0F7` 42%）`App.vue:1729` → ③ `.shell` inset 顶高光 `inset 0 1px 0 rgba(255,255,255,.12)` `App.vue:1735` → ④ 1px 描边 `color-mix(border 80%,transparent)` `App.vue:1733`。<br>**`.shell` 自身无 `backdrop-filter`**；blur 只来自 DWM。`@supports not` 时 ② 换成 `#C7…`(78%)。 |
| **2** | **侧栏 `.sidebar`** | **8**（其中侧栏自己贡献 4） | 共享上面 ⓪–④ → ⑤ `.sidebar` 底 **`#CE171717`**(α.8096)（蓝白 `#59F5F7FB`）+ `border-right:1px var(--border)` `App.vue:1760-1761` → ⑥ **`.nav-bubble`**（absolute，无 z-index，`linear-gradient(135deg, accent-soft 80%→transparent, hover)`，`0 3px 12px #00000008`，几何/opacity 由 JS 内联）`App.vue:1837-1848` + `useNavigationBubble.ts:57-60` → ⑦ **`.nav-selection`**（absolute，`z-index:2`，**无自身背景**，只有 `box-shadow: inset 3px 0 0 var(--accent)`）`App.vue:1849-1858` → ⑧ `.nav-item`（`z-index:1`，**`background:transparent`**；`:hover`/`.active` **都只改 color，不改底**）`App.vue:1811-1836`。<br>**无 blur。** `.logo-area`(88px) 与 `.sidebar-health`（`#E0292929` 混 48% → `color-mix(card 48%,transparent)` `App.vue:1969`）是各自局部的附加层；子菜单收起时整棵 `[inert]{opacity:.65}`（`ui-system.css:43`）。 |
| **3** | **顶栏 `.topbar`** | **6**（按钮局部 +1） | 共享 ⓪–④ → ⑤ `.topbar` 底 **`#AF171717`**(α.6864)（蓝白 `#4CF5F7FB`）+ `border-bottom:1px var(--border)` `App.vue:2024-2025` → ⑥ `.top-actions`/`.win-btn` 全为 `background:transparent`（`App.vue:2083/2108/2379`）→ ⑦ `.top-back` `background:var(--card-2)`(82%) + `border 1px color-mix(border 70%,transparent)` `App.vue:2040-2042`（**仅返回按钮存在时**）→ ⑧ `.bell-dot`（`7×7`，`background:var(--accent)` + `border:1.5px solid var(--bg-2)`，absolute `top:7 right:8` `App.vue:2121-2130`）或 `.dl-badge`（`background:var(--accent)`，absolute `top:2 right:0` `:2131-2138`）。<br>**无 blur。** `-webkit-app-region:drag`（`:2026`）→ WPF 用 `WindowChrome.CaptionHeight=72`。 |
| **4** | **hero 卡（首页封面）** | **13（全站最深）** | ⓪–⑤ → ⑥ `.content`（**不绘制**，仅 `isolation:isolate; z-index:0`，`styles.css:541`）→ ⑦ `.hero-card` 底 **`#17231f`（100% 不透明）** + 1px 边（α≈**.213**）+ `0 18px 44px rgba(0,0,0,.22)` + `overflow:hidden` + radius 20 `HomeView.vue:799-809` → ⑧ **`.hero-image` ×N**（同位堆叠，`opacity 0/1` 交叉淡 500ms，`object-position:center 42%`）`:810-811,:1033` → ⑨ **`.hero-shade`** `linear-gradient(180deg, rgba(0,0,0,.16), rgba(0,0,0,.18) 30%, rgba(0,0,0,.68))`（`:1030`；**基值 `:815` 的双层渐变已失效**）→ ⑩ `.hero-content`（**不绘制**，`z-index:1`）`:817` → ⑪ **控件层（同一像素上互斥）**：<br> ⑪a `.hero-kicker` `rgba(9,13,14,.48)` + **`blur(12px)`** + `1px rgba(255,255,255,.13)` `:818`<br> ⑪b `.loader-badge` α≈**.622**（`color-mix(accent 30%, rgba(20,30,24,.46))`）+ `1px color-mix(accent-2 34%,transparent)` `:831`<br> ⑪c `.hero-settings`/`.hero-more` `rgba(10,16,17,.58)` + **`blur(13px)`** + `1px rgba(255,255,255,.14)` `:834`（hover → `.76` `:839`）<br> ⑪d `.launch-combo` **双阴影**（外 `0 12px 32px accent@38%` + ★内 `0 2px 0 white@14% inset`）`:843` → ⑫ `.launch-main`/`.launch-arrow` `--accent-grad`（135° 三段）+ `.launch-arrow` `border-left:1px rgba(255,255,255,.22)` `:848-855` → ⑬ `.launch-progress` **`rgba(255,255,255,.25)`**（absolute，宽度=进度）`:853`；其上 `.launch-content` `z-index:1` 放图标+文字 `:850`。<br>**文本附加：`h1` 的 `text-shadow: 0 4px 24px rgba(0,0,0,.32)`（`:819`，尾部未撤销 ⇒ 生效）。**<br>⇒ 在"正在启动"按钮的一个像素上，实打实 **13 层**。 |
| **5** | **状态条 `.runtime-strip`（首页三格）** | **7** | ⓪–⑤ + `.content` → ⑥ `.runtime-strip` 底 **`#F0292929`(surface-content 94%)**（`:769` 的 `!important` **覆盖** `:860` 写的 `color-mix(card 80%)`）+ **`blur(18px) saturate(130%)`**（因 94% 不透明 ⇒ 几乎不可见）+ `border 1px var(--border)` + `var(--shadow)` + `overflow:hidden` `:860,:769` → ⑦ **`.runtime-blob`** `color-mix(accent 10%,transparent)`，`opacity 0→1` + `scale(.97→1)`，`z-index:0`，`margin:4px 0`，radius 12 `:862-870` → ⑧ `.runtime-item`（**`background:transparent`**，`z-index:1`；相邻项间 `border-left:1px solid var(--border)` `:872`）→ ⑨ `.runtime-state i`（`7×7`，`background:#9ca3af` 默认 / `var(--ok)` / `var(--danger)`）+ `0 0 0 3px var(--ok-soft)` 或 `--danger-soft` 光环 `:881-884`。<br>尾部 `:1030` 改 `min-height:68px; padding:8px`（**从 82px 降下来，并加了 padding ⇒ 内层 blob 的 `margin:4px 0` 相对更明显**）。 |
| **6** | **普通卡 `.card`（二级页）** | **3–4** | ⓪–⑤ + `.content` → ⑥ `.card` 底 **`#F0292929`(surface-content 94%)**（`ui-system.css:13` **覆盖** `styles.css:451` 的 `var(--card)` 88%）+ `border:1px solid var(--border)`（`styles.css:452`）+ **`box-shadow:none`** → ⑦ `backdrop-filter: blur(16px) saturate(85%)`（`ui-system.css:28`，**因 94% 不透明 ⇒ 实质不可见**；皮肤页被 `SkinsView.vue:628` 升为 `blur(24px) saturate(130%)`）→ ⑧（可选）内嵌 `.card`：`ui-system.css:21` 再 `box-shadow:none`；`ui-system.css:78` 把 `.connection-panel` 改 `box-shadow:none`、嵌套的 `.connection-panel .connection-panel` 改 `background:transparent;box-shadow:none`。<br>**首页变体**：`.account-panel`/`.skin-panel`（`HomeView.vue:917` `surface-content` + `var(--shadow)`）→ 尾部 `:1030` 双双 `box-shadow:none`，account 上半圆角 + `border-bottom:0`、skin 下半圆角 + **`margin-top:-16px`** + `border-top:0` ⇒ **两张融合成一张**；`.instances-block`（`:1030` 新增 `background:var(--surface-content); radius:--radius-lg; padding:16px`）成为列表底，其内实例卡全 `transparent`。 |
| **7** | **账号卡** | **首页 7 / 账号页 4** | **首页 `.account-panel`**：⓪–⑤ → ⑥ `.account-panel` `#F0292929`(94%) + 1px border + 上半圆角、**无阴影**（`:917`+`:1030`）→ ⑦ **`.mc-avatar` 环**：`0 0 0 4px color-mix(text 8%, transparent)`（`:920`）+ 头像本体（图片或 `--accent-grad` 首字母圆，`Avatar.vue:48-51`）→ ⑧ `.account-copy span i` 状态点 + `0 0 0 3px var(--ok-soft)` 环（`:924`）→ ⑨ `.account-provider` `var(--card-2)`(82%) + 1px border（`:930`）→ ⑩ **`.provider-mark` 第 3 层**：微软 = **`background:transparent`**（`:931` 胜 `:935` 的四色渐变）+ 内联 4 色 SVG；yggdrasil = `linear-gradient(135deg,#65bd78,#268e54)`（`:936`）；offline = `var(--card)`（`:937`）。<br>**账号页 `.account-row`**：`var(--card-2)`(82%) + 1px border（`AccountsView.vue:864-874`）→ `:hover` 只改 `border-color:var(--accent-deep)`（`:876`）→ `.selected` `border-color:var(--accent)` + **`background:var(--accent-soft)`(14%)**（`:879-881`）→ `.avatar` `42×42` 圆 + `--accent-grad`（`:883-892`）。 |
| **8** | **皮肤舞台** | **首页 7 / 皮肤页 7–8** | **首页 `.skin-stage`**：⓪–⑤ → ⑥ `.skin-panel` `surface-content`(94%)、无阴影、下半圆角 → ⑦ **`.skin-stage` 双层背景**（`:946`）：第 1 层 `radial-gradient(circle at 50% 82%, color-mix(accent 13%, transparent), transparent 38%)`，第 2 层 `linear-gradient(180deg, transparent, color-mix(bg 18%, transparent))`，外 `border:1px solid color-mix(border 78%,transparent)` → ⑧ WebGL canvas（`:deep(.viewer3d){background:transparent}` `:947` ⇒ 透明，让第 7 层的双渐变露出来）→ ⑨ `.viewer3d-fallback`（absolute inset:0，`SkinViewer3D.vue:125`）→ ⑩ **`.skin-overlay`** `color-mix(card 78%, transparent)` + **`blur(12px)`** + 1px border + `z-index:2`，三态互斥（`:649-651`）。<br>高度演变：`:946 300px` → `:1026 230px`(@max-height 760) → `:1030 260px` → **`:1033 .home-side .skin-stage{height:230px}` 最终胜**。<br>**皮肤页**：`.pane` = `.card`（`surface-content` 94% + `blur(24)sat(130)`）→ `.seg-blob`（`--accent-grad`，`z-index:0`，inline `opacity on?1:0`，`SkinsView.vue:826-831` + `:86`）+ `.seg-btn`（`z-index:1`，`background:transparent` `:836`）→ `.drag-hint`（**`inset:0; z-index:2; color-mix(accent 18%,transparent)`** `:748-763`，仅拖拽时）→ `.history-overlay`（`inset:0; background:var(--mask); opacity 0→1 on hover` `:998-1010`）。拖拽悬停另有 **环 + 阴影双保险**：`:662 .row-main.drag-over{box-shadow:0 0 0 3px var(--accent-soft)}` + `:1102 .row-main.drag-over .pane{outline:2px solid var(--accent)}`。 |
| **9** | **浮层菜单** | **4–5** | ⓪–⑤（含 `.content` 内页面各层）→ ⑥ **`.menu-overlay` / `.notice-mask`**（**完全透明**的捕点层，只有 `position:fixed;inset:0`；z 90 / 8000 / 9000 / 9600）→ ⑦ 面板底（**逐页不同，务必按页复刻**）：<br>• 全局 `.float-menu` = `color-mix(card-solid 96%,transparent)` + **`blur(24px) saturate(130%)`** + `var(--shadow-lg)`（`styles.css:582-596`）<br>• HomeView scoped `.float-menu` = `color-mix(card 94%,transparent)` + **`blur(24px)`（丢 saturate）**（`:958`）<br>• `.card-float-menu` = **`var(--card-solid)` 100% 不透明**（`:959`）⇒ ⑦ 上的 blur **完全死掉**<br>• `.instance-more-menu` = **`var(--card-solid)` 100%**（`GameView.vue:2202`）<br>• `.select-menu-float` = `color-mix(card-solid 96%)` + `blur(24)sat(130)` + `var(--shadow)`（`SelectMenu.vue:136-145`，z 11000）<br>• `.notice-panel` = `color-mix(card-solid 94%)` + `blur(24)sat(130)` + `var(--shadow-lg)`（`App.vue:2176-2195`，z 9001，`isolation:isolate` `:2189`）<br>• `.file-more-actions` / `.settings-search-results` = **`var(--surface-solid)` 100% 不透明**、**无 blur**（`FileManager.vue:488` / `SettingsView.vue:1845`）<br>• `.version-picker-popup` = **`var(--card-solid)` 100%**、**无 blur**、`var(--shadow-lg)`（`MinecraftVersionPicker.vue:26`，z 12000）<br>→ ⑧ `.menu-item:hover/.active` = `var(--accent-soft)`(14%)（`styles.css:617-621` / `HomeView.vue:963`）→ ⑨ `.select-menu-option:hover` = `var(--hover)`(7%)，`.active` = `--accent-soft`（`SelectMenu.vue:165-166`）。<br>**⇒ 只有 3 个菜单（全局 `.float-menu`、HomeView `.float-menu`、`.select-menu-float`）值得做 blur，且都不低于 94% alpha，视觉上等于纯色。** |
| **10** | **模态窗（`.modal-mask` + `.modal`）** | **9–10** | ⓪ DWM acrylic → ① `.app-bg`（可选，带 blur）→ ② `.shell`(96%) → ③ inset 高光 → ④ border → ⑤ `.content` 内的**整页内容**（如 hero 的 13 层，全部被下面两层"吸进"模糊锥）→ ⑥ **`.modal-mask`**：`background:var(--mask)`（暗 **72%** / 亮 **42%**）+ **`blur(10px)`** + `animation:mask-in .2s`（`styles.css:527-538`；z 10000，可被 inline 抬到 10020/10030）→ ⑦ **`.modal`**：`background-color:var(--surface-raised)` = `color-mix(card-solid 97%,transparent)`（`ui-system.css:37` 胜 `styles.css:549` 的 96%）+ `border:1px solid var(--border)` + `var(--shadow-lg)` + `animation:modal-in 220ms` `translateY(6px) scale(.99)` → ⑧ `.modal-actions` 行（无底）→ ⑨ 内部 `.input`/`.select` 的 `var(--card-2)`(82%) + focus 环 `0 0 0 3px var(--accent-soft)`。<br>**同族其他遮罩**（各 1 层 blur + 1 层 tint）：`.log-mask` blur(5) `HomeView:968`、`.ic-mask` blur(8)+`#0009` `InstanceCenter:109`、`.update-dialog-backdrop` blur(12) `UpdateDialogShell:33`、`.designer-dialog-mask` blur(8) `EditPanel:74`、`.drop-mask` blur(24)sat(130)+accent16% `App:2427`、`.ic-lightbox` **无 blur** `#101418fa`、`.notice-mask` / `.menu-overlay` / `.cfg-capture-mask` / `.cfg-mismatch-mask` **全透明无 blur**。 |
| **11** | **Toast** | **4**（**无 blur**） | ⓪–⑤（`.toasts` 在 body 层，`z-index:200`）→ ⑥ **`.toasts` 容器本身透明**（`pointer-events:none`，`Toasts.vue:15-24`，`top:76px; right:var(--space-5)`，纵向 gap 8，**最多渲染 3 条**：`store.toasts.slice(-3)` `:7`）→ ⑦ **`.toast`** = `background: var(--surface-raised)`（**97% ≈ 不透明**）+ `border:1px solid var(--border)` + **`border-left-width:3px`**（`--ok`/`--danger`/`--accent` 三色语义 `:52-71`）+ `var(--shadow-lg)` + `user-select:text` → ⑧ `.toast-dot` `8×8` 纯色圆（`background: var(--ok)/var(--danger)/var(--accent)`）+ 关闭按钮 `.icon-btn`（透明底）。<br>**入场 `translateX(24px)`+opacity0 → 220ms；离场 `translateY(-8px)`+opacity0**（`:73-87`）。<br>⇒ **Toast 是全站少数"完全不模糊、只靠不透明底 + 大投影"分层的浮层。WPF 直接 `Border` + `DropShadowEffect` 即可。** |

**层数排行（深→浅）**：hero 卡 **13** > 模态窗 over hero **10** > 皮肤页舞台 **8** > 侧栏 **8** > 首页皮肤面板 **7** / 运行时条 **7** > 首页账号卡 **7** > 顶栏 **6** > 外壳 **5** > 普通卡 **4** > Toast **4** > 浮层菜单 **4–5**。

---

## G WPF 落地方案

### G.0 总体策略：三层架构

WPF 里没有 `backdrop-filter`，但本项目**恰好不需要真做全站背景模糊** —— A.1 已证明 19 处 blur 里 15 处所在表面自身 alpha ≥94%。所以：

```
第 1 层（窗口级）  DWM 系统材质        ← 复刻 backgroundMaterial:'acrylic'
第 2 层（表面级）  预计算 ARGB 纯色刷  ← 复刻所有 alpha≥94% 的 color-mix 表面（丢掉 blur）
第 3 层（特殊级）  预模糊位图 / 截图刷 ← 只服务 4 处真透的 blur + 2 处遮罩
```

### G.1 逐表面的 WPF 替换建议（**每条都给理由**）

| 表面 | CSS 效果 | WPF 方案 | 理由 |
|---|---|---|---|
| **窗口外壳（桌面透视）** | `backgroundMaterial:'acrylic'` + `backgroundColor:'#00000000'`（`windowAppearance.ts:45-46`） | **(b) P/Invoke**：`DwmSetWindowAttribute(hwnd, DWMWA_SYSTEMBACKDROP_TYPE /*38*/, DWMSBT_TRANSIENTWINDOW /*=2 Acrylic*/)`；Win10/失败时回退 `DWMSBT_NONE` + 全不透明底色。配 `WindowChrome` + `GlassFrameThickness = new Thickness(-1)`（等价 `DwmExtendFrameIntoClientArea` 的 sheet-of-glass），`AllowsTransparency = false`。<br>**绝不要用 `AllowsTransparency=True`** —— 它会走 WPF 的 layered-window 路径，正好踩中 `windowAppearance.ts:39-40` 注释里 Electron 主动回避的同一个坑（"transparent:true 会创建 layered window，绕过该合成路径"）。 | 这是全站**唯一真正的桌面模糊来源**，CSS 里根本没有它的等价物。DWM 的模糊半径/降噪/tint 由系统决定，比自绘更保真，也免费。 |
| `.shell` / `.sidebar` / `.topbar` | 无 blur，只有半透明纯色（`#F5212121` / `#CE171717` / `#AF171717`） | **(a) 预计算 ARGB `SolidColorBrush`**，按 C.8 表直接绑 | 无 backdrop-filter 可丢；alpha 连乘已在表里算好。 |
| **`.hero-kicker`（blur 12 over 照片）** | `rgba(9,13,14,.48)` + `blur(12px)` | **(c) 预模糊 brush**：把当前 hero 位图**一次性**用 `WriteableBitmap`+可分离 box-blur（半径 12px）生成 `blurredBanner`，pill 的 `Fill` 用 `ImageBrush{ImageSource=blurredBanner, Viewport=<pill 相对 banner 的矩形>, ViewportUnits=Absolute, TileMode=None}`，再在其上叠一个 `#7A090D0E`（α.48）`Border` | pill 的 backdrop **就是一张已知的静态位图**，且 pill 相对 banner 的位置可算 → 预模糊完全等价、零运行时开销。`BlurEffect` 不行（它糊的是元素自身内容，且不能采样身后）。 |
| **`.hero-settings` / `.hero-more`（blur 13）** | `rgba(10,16,17,.58)` + `blur(13px)` | 同上 **(c)**，复用同一 `blurredBanner`，仅把 tint 换成 `#94101011`（α.58） | 与 kicker 同一 backdrop，**只需两张预模糊位图（12/13px），可合并为一张 13px 再降 1px 误差**（肉眼无差）。 |
| **`.skin-overlay`（blur 12 over WebGL）** | `color-mix(card 78%)` + `blur(12px)` | **(a) 直接用 `#C8292929`（78%）纯色**，不做 blur | backdrop 是**WebGL 实时画面**，无法预模糊；且它是低细节的角色渲染，78% tint 后残余 22% 的模糊差异 ≈ 无法察觉。省掉一次每帧截图的代价。 |
| **`.modal-mask` / `.log-mask` / `.update-dialog-backdrop` / `.ic-mask` / `.designer-dialog-mask` / `.drop-mask`**（**load-bearing**） | `blur(5–24px)` + 16–72% alpha 的 tint | **(b) 打开弹窗时一次性快照**：把根 `Visual`（`shellGrid`）用 `RenderTargetBitmap` 渲染到 `BlurEffect{Radius = cssRadius * 1.4}` 的 `Image`，铺满 `Popup` 的 `AdornerLayer`/顶层 `Grid`；关闭即释放。**不要用 `@supports not` 的纯色**（那只有 `.shell` 一条，且弹窗本身没有兜底）。 | 遮罩的 blur 是**弹窗仪式感的核心**（亮色主题 mask 只有 42% alpha，不糊会直接看清底下的字）。快照只需在 open 时做一次（静态界面），成本可控；`Radius` 换算是因为 WPF `BlurEffect` 是高斯近似（`Radius≈3×σ`），CSS `blur(Npx)` 的 σ≈N，故 `Radius ≈ N×2`——建议按 N=10 实测取 20 附近微调。 |
| `.runtime-strip`（blur 18 sat 130，**底色 94%**） | — | **(a) `#F0292929` 纯色 + 保留 `Border`** | 6% 透出 + saturate 的净效果 ≈ 给底色加不到 1% 的饱和偏移。**省掉一次实时背景采样。** |
| `.content .card`（blur 16 **saturate 85%**） | — | **(a) `#F0292929` 纯色** | 同上，且 `saturate(85%)` 方向是**降饱和**，6% 权重下更是无从分辨。 |
| `.float-menu`(96%) / `.select-menu-float`(96%) / `.notice-panel`(94%) / `.pick-card`(94%) / `.skins-page .card`(94%) / HomeView `.float-menu`(94%) | blur 24，可选 sat 130 | **(a) 预计算 ARGB 纯色** | 都 ≥94% alpha。 |
| **`.card-float-menu` / `.instance-more-menu` / `.launch-success` / `.connection-panel` / `.version-picker-popup` / `.file-more-actions` / `.settings-search-results`** | 写了 blur **但底色 100% 不透明**（`.connection-panel` 的底色是 `color-mix(card 94%, var(--bg))`，两操作数都不透明 ⇒ 100%） | **什么都不做（直接不实现 blur）** | **死代码**。CSS 里 backdrop-filter 在不透明底后面完全不显效。加注释说明"源码有 blur，但被不透明底吞掉"以免 review 时被质疑漏实现。 |
| `.toast` | **本来就没有 blur**，`surface-raised` 97% + `--shadow-lg` | `Border Background="#F7292929"` + `BorderThickness="3,1,1,1"`（左 3px 语义色）+ `DropShadowEffect{BlurRadius=55/2≈28, ShadowDepth=0, Direction=270-…见 G.4, Color=#6B000000}` | CSS `0 20px 55px rgba(0,0,0,.42)` → WPF `DropShadowEffect` 无 spread、`BlurRadius` 与 CSS blur 定义不同（WPF 是 3σ 近似），需按 G.4 换算。 |
| `.app-bg` 图片层（opacity + blur + 负 inset） | `filter:blur(N)` + `opacity:.5` + `inset:-ceil(N*1.5)` | **`ImageBrush`（`TileMode=None`、`Stretch` 按 fit）** 装进一个 `Border`，`Opacity = bg.opacity`；blur 用 `Border.Effect = new BlurEffect{Radius = N*2}` **或** 预处理位图。负 inset 用 `Border` 的 `Margin = -ceil(N*1.5)`（WPF `Margin` 支持负值）复刻 | 这是**元素自身**的 filter，WPF `BlurEffect` 正好适用（不像 backdrop 情形）；但因为背景图是静态的，**离线预模糊更快**（避免每次窗口 resize 都走 shader）。 |
| 设计器（`visualDesign.ts:98` 动态写 `backdrop-filter: blur(Npx)`，N 由用户在设计师里设） | 任意半径 | 保留一个 `DependencyProperty`/`AttachedProperty` `LayerBlurRadius`，值 >0 时走 G.2 的"预模糊 brush"路径；设计师 UI 本身建议**首期不实现**（它是编辑器，不是终端界面） | 这是全站唯一"运行时可配 blur"的口子，WPF 没有对应能力；砍掉它并把半径烘进资源字典最省。 |

### G.2 `@supports not` 只有一条 —— 不要照抄成"全局降级"

`App.vue:1748-1752` 是唯一兜底。WPF 侧的实现应是：
```csharp
// ShellBackgroundBrush 的解析顺序
1. DWM 可用 (Win11 22H2+, DwmGetWindowAttribute 返回成功) → 用 --shell-surface 原值（30/42/96%）
2. DWM 不可用 (Win10)                                      → 用 #C7… 值（@supports not 那条）
3. 二者都失败                                              → 完全不透明 bg（避免叠出灰脏）
```
注意 (1) 与 (2) **只在 `.shell` 上有区别**，其余所有表面在两分支下同值。

### G.3 叠层渐变：多 `Border` vs 单个 `DrawingBrush` —— 决策规则

| 情况 | 选择 | 理由 |
|---|---|---|
| 所有层都 `inset:0`、**同尺寸、静态、无独立动画** | **一个 `Border` + 一个 `DrawingBrush`（`DrawingGroup` 里按 CSS 层序放 `GeometryDrawings`）** | 零额外可视树节点、零布局、一次 GPU 上传。**hero-shade / skin-stage / network-overview / server-list-item.active / tp-custom-grad 全属这类。** |
| 某一层需要**动画**（尺寸、位移、opacity 独立变化）或**命中测试** | **`Grid` 里多个 `Border`** | `DrawingBrush` 内部子 drawing 无法单独 animate（要重画整个 brush）。属于这类：`.hero-image`（opacity 交叉淡）、`.runtime-blob`（left/width/scale/opacity）、`.nav-bubble`/`.nav-selection`（translate3d+width+height）、`.dl-bar-fill`/`.launch-progress`/`.mem-slider-fill`（width）、`.seg-blob`/`.capsule-blob`/`.game-tabs-blob`（left/width/opacity）、`.app-bg`（图片轮换） |
| 某一层要**圆角裁剪不同** | 多 `Border` | `DrawingBrush` 的 clip 只能整块 |
| 需要 `overflow:hidden` 才能成立（如 `.hero-card`、`.skin-stage`、`.dl-bar`） | `Border` + `Border.ClipToBounds=true` 或 `OpacityMask` 圆角 | WPF 的 `Border.Background` 会自己按 `CornerRadius` 裁；但**子内容不会** ⇒ 子内容需显式 `Border` 或 `RectangleGeometry` clip |

**具体画法（hero-shade 终值，`HomeView.vue:1030`）** —— 一条 180° 三段渐变，WPF 直接：
```xml
<LinearGradientBrush StartPoint="0,0" EndPoint="0,1">
  <GradientStop Color="#29000000" Offset="0.00"/>  <!-- rgba(0,0,0,.16) -->
  <GradientStop Color="#2E000000" Offset="0.30"/>  <!-- rgba(0,0,0,.18) -->
  <GradientStop Color="#AD000000" Offset="1.00"/>  <!-- rgba(0,0,0,.68) -->
</LinearGradientBrush>
```
**基值（`HomeView.vue:815`）不需要实现**，但若要对照旧版渲染，它是 `DrawingGroup` 里两个 `GeometryDrawing`（先 `0deg` 那层，后 `90deg` 那层 —— CSS 第一层在最上）。

**CSS 角度 → WPF 向量的换算（用于 135° / 110° / 125° / 90°）**
CSS `θ`（0° 朝上，顺时针）对应方向向量 `d = (sin θ, -cos θ)`；渐变线长度 `L = |W·sin θ| + |H·cos θ|`。于是
`StartPoint = Center − d·L/2`、`EndPoint = Center + d·L/2`（用 `MappingMode=Relative` 时要先除以 W/H）。
- `135deg`（`--accent-grad`、`.provider-mark`、`.nav-bubble`）⇒ `d=(0.7071, 0.7071)` = 左上→右下，等价 WPF `StartPoint="0,0" EndPoint="1,1"`（仅当 W≈H；非方形必须用 L 修正）。
- `110deg`（`connection.css:29`、`ServersView.vue:474`）⇒ `d=(0.9397, 0.3420)`。
- `125deg`（`VoxLinkPanel.vue:429`）⇒ `d=(0.8192, 0.5736)`。
- `90deg`（`.dl-bar-fill`、`.mem-slider-fill`）⇒ 水平，`StartPoint="0,0.5" EndPoint="1,0.5"`。
- `0deg`（`.hero-shade` 基值第二层）⇒ 下→上，`StartPoint="0,1" EndPoint="0,0"`。

**`.skin-stage` 的径向层（`HomeView.vue:946`）** 有坑：CSS `radial-gradient(circle at 50% 82%, X, transparent 38%)` 中 **`circle` + 单个百分比色标** 的半径按 `sqrt(W²+H²)/√2 × 38%` 解析，而 WPF 的相对 `RadialGradientBrush` 是**椭圆**且随宽高比拉伸 ⇒ **不能直接 `Center="0.5,0.82" RadiusX=RadiusY=0.38`**。正确做法：`MappingMode=Absolute`，`Center=new Point(W*0.5, H*0.82)`，`RadiusX=RadiusY=Math.Sqrt(W*W+H*H)/Math.Sqrt(2)*0.38`，`SpreadMethod=Pad`，并在 size 变化时重算（或直接用一个 `DrawingBrush` + `Viewport` 归一化）。`.skin-stage` 高度固定 230px、宽度 ≈258px，可预先算死。

**双层（渐变 + 实底）的写法**：`DrawingGroup` 第一个 `GeometryDrawing(brush = 实底)` 画在最下，第二个画渐变 —— 注意 **CSS 逗号列表里第一项在上**，`DrawingGroup.Children` 里**后加入的在上**，顺序刚好相反。这是最容易翻车的一点。

### G.4 `inset` 阴影（WPF 没有内阴影）—— 逐条替换配方

| file:line | 声明 | WPF 方案 |
|---|---|---|
| `App.vue:1735` | `.shell inset 0 1px 0 rgba(255,255,255,.12)` | 在 `Border` 的 `Grid` 顶部放一个 `<Rectangle Height="1" VerticalAlignment="Top" Fill="#1FFFFFFF" IsHitTestVisible="False"/>` |
| `App.vue:1855` | `.nav-selection inset 3px 0 0 var(--accent)` | **不要做 inset**：`.nav-selection` 无自身背景，`inset 3px 0 0` 的净效果 = 一个 **3px 宽、全高的 accent 竖条**。直接 `<Border BorderBrush="{accent}" BorderThickness="3,0,0,0"/>` 或 `<Rectangle Width="3" Fill="{accent}"/>`。语义完全等价且无 Effect 开销 |
| `HomeView.vue:843/846` ★ | `.launch-combo … , 0 2px 0 color-mix(white 14%/16%) inset` | **拆两半**：外发光 → `DropShadowEffect{BlurRadius=32*2≈64? 见下, ShadowDepth=0, Color=#61AC…}`；inset 顶高光 → 按钮内部 `Grid` 顶上一枚 `<Rectangle Height="2" Fill="#24FFFFFF" VerticalAlignment="Top"/>`（`white 14%` ⇒ α `0x24`） |
| `HomeView.vue:899` ★ | `.instance-card.selected inset 0 0 0 1px var(--accent)` | **源码里已被 `:1030` 杀掉**（见 D.12），WPF **不要实现**。若要实现只需 `BorderThickness="1"` |
| `HomeView.vue:916` ★ | `.home-creator inset 0 0 0 1px color-mix(accent 10%,transparent), var(--shadow)` | 同上，`:1030` 已 `box-shadow:none` ⇒ **不实现** |
| `App.vue:2520` ★★ | `.mp-radio inset 0 0 0 3px var(--card), inset 0 0 0 8px var(--accent)` | 三层同心 `Ellipse`（或单个 `Ellipse` + 两层 `StrokeThickness`）：外圈 accent 填充 → 中间 card 3px 环 → 中心 accent 点。用 `<Ellipse Fill="{accent}"/>` + `<Ellipse Fill="{card}" Width/Height 缩 6/>` + `<Ellipse Fill="{accent}" 缩 10/>` |
| `GameView.vue:2168` ★ | `.game-tabs-blob inset 0 -2px var(--accent)` | `inset … -2px` = **底部** 2px 内条 ⇒ `<Border BorderBrush="{accent}" BorderThickness="0,0,0,2"/>` |
| `CommunityView.vue:1027` ★ | `.file-row.active inset 0 0 0 1px var(--accent), 0 4px 14px var(--accent-soft)` | `BorderThickness="1"`（内环）+ `DropShadowEffect`（外投影），两条分开做 |
| `ServersView.vue:474` ★ | `.server-list-item.active inset 3px 0 var(--accent)` | **`:515` 尾部已 `box-shadow:none`** ⇒ 不实现（见 F.9 与 D.12 的"尾部压缩"警告） |
| `VoxLinkPanel.vue:429` ★ | `.address-hero inset 3px 0 var(--accent)` | 左侧 3px `Border`（生效，无尾部覆盖） |

**`DropShadowEffect` 的换算表（`BlurRadius` = CSS blur × 2，`ShadowDepth` = CSS y 偏移，`Direction = 270`（正下），`Color` = CSS 色含 alpha）**

| CSS | WPF |
|---|---|
| `--shadow` 暗 `0 2px 8px rgba(0,0,0,.16)` | `BlurRadius=16, ShadowDepth=2, Direction=270, Color=#29000000` |
| `--shadow` 亮 `0 2px 8px rgba(31,50,85,.045)` | `… Color=#0B1F3255` |
| `--shadow-lg` 暗 `0 20px 55px rgba(0,0,0,.42)` | `BlurRadius=110, ShadowDepth=20, Direction=270, Color=#6B000000` |
| `--shadow-lg` 亮 `0 20px 55px rgba(31,50,85,.18)` | `… Color=#2E1F3255` |
| `0 0 0 Npx C`（**所有 spread-ring**：焦点环、状态点光环、头像环） | **不要用 Effect** ⇒ 改用 `Border`/`Ellipse` 的 `Stroke` + `StrokeThickness=N`，或 `Border BorderThickness=N` + 同色。`DropShadowEffect` 无法表达 spread |
| `0 18px 44px rgba(0,0,0,.22)`（`.hero-card`） | `BlurRadius=88, ShadowDepth=18, Direction=270, Color=#38000000` |

**性能红线**：`DropShadowEffect` 在 WPF 里是**软光栅像素着色器**，命中即整个 visual 树退出硬件加速路径。⇒ 一个界面里**最多给浮层用**（modal / Popup / toast / notice-panel），且**绝不能**给 `.content` 里的普通卡加（源码本来也是 `none`，见 D.12 末段）。给 `Border` 加 `SnapsToDevicePixels` 并避免在滚动列表 item 模板里放 Effect。

### G.5 `color-mix` → 预计算 ARGB（一次性烘进资源字典）

**算法**（照 C.8 的 5 条规则实现一个 `ColorMix(sRGB a, double pa, sRGB b, double pb)`）：
1. `alpha = pa*αa + pb*αb`；
2. 各通道（非预乘）`c = (pa*αa*ca + pb*αb*cb) / alpha`（alpha=0 时取任意）；
3. `transparent` = `(0,0,0,α0)`；
4. 若 `pa+pb ≠ 1` 且两者均 >0 ⇒ 先归一化（`HomeView.vue:805` 的 `95%/4%` 就是这情况）；
5. 嵌套令牌 ⇒ **递归**（`--bg-2` 先算，再喂给 `.sidebar`/`.topbar`）。

**产物**：每个主题一个 `ResourceDictionary`，键名沿用 CSS 变量名，值是 `#AARRGGBB`。C.8 表已给全部 6 套主题的 8 个关键表面终值，可直接抄。
`ThemeManager.Apply(themeName, customColors)` 需要在 C# 侧复刻 `App.vue:960-1012` 的 19 个 alpha 相关分支（`dark = Rec601(bg) < 0.46`，`blackPurple = theme=="transparent"`）与 `shared/themeContrast.ts:17-33` 的 `readableCustomColors` 修正。
**`--glass-blur` 不要搬**（无消费者，`App.vue:1007`）。

### G.6 层序 / z-index 的 WPF 表达

- 同一容器内的 `Grid` 子元素按**声明顺序**叠放 ⇒ 用顺序复刻 `z-index < 100` 的相对关系（`Panel.ZIndex` 只在必须打乱顺序时用）。
- `isolation: isolate`（`styles.css:541-542`）⇒ 用 `OpacityMask` 或**独立 `VisualBrush`/子容器** 阻断穿透；实践中只要保证"页面内的高 z 不逃出 `.content`"，可改为**把浮层全部 `Popup` 化**（Popup 天然独立于主 visual tree 的叠序）。
- `z ≥ 8000` 的一律：`Popup`（`StaysOpen=False`、`AllowsTransparency=True`）或专用无边框顶层 `Window`。Toast / LaunchNotice / version-picker / select-menu / modal 各占一档，按 B.7 表的绝对值**排 Popup 的显示次序 + `Topmost` 不启用**（Topmost 会跑到系统级）。
- `.toasts`(200) 与 `.edit-tip`(200) **打平**（`Toasts.vue:19` vs `styles.css:720`）⇒ WPF 里让 Adorner 层的 edit-tip 后添加即可。
- `-webkit-app-region: drag / no-drag`（`App.vue:2026/1976/2045/2072/2188`、`UpdateDialogShell.vue:33`）⇒ `WindowChrome.CaptionHeight=72` + 交互控件放 `WindowChrome.IsHitTestVisibleInChrome=True`。`.shell.edit-mode .topbar{no-drag}`（`App.vue:2029-2031`）⇒ 编辑模式下把 CaptionHeight 改 0。

### G.7 需要**动态**层（JS 写 inline 样式）的清单 —— 不能烘进静态 XAML

| 驱动点 | file:line | 写什么 |
|---|---|---|
| `applyCustomVars()` | `App.vue:960-1012` | 19 个 CSS 变量（含 5 个 alpha 表面 + 2 个 shadow 令牌）⇒ WPF `ResourceDictionary.MergedDictionaries` 整体替换 |
| `bgStyle` | `App.vue:880-902` + `appearancePolicy.ts:17-21` | `.app-bg` 的 `inset/backgroundColor/backgroundImage/backgroundSize/opacity/filter` ⇒ WPF 一个 `Border` 的属性绑定 |
| `useNavigationBubble` | `useNavigationBubble.ts:57-64` | `.nav-bubble`/`.nav-selection` 的 `transform/width/height/opacity` |
| 三处 blob | `CommunityView.vue:90-95`、`GameView.vue:120-124`、`SkinsView.vue:80-87` | `left/width/top/height/opacity` |
| `runtimeBlobStyle` | `HomeView.vue:862-870` + 模板 `:546` | `left/width` + `.on` |
| `installVisualDesign()` 覆盖器 | `visualDesign.ts:89-107` | `background` / `opacity` / `border-radius` / **`backdrop-filter`** / `translate` / `z-index` / `order` / `outline`（隐藏态），全部 `setProperty(..., 'important')` ⇒ **WPF 若无设计器需求，整块砍掉** |
| `CarouselPlayback` | `HomeView.vue:125-140`、`appearancePolicy.ts:3-15` | hero 图哪一张 active |
| `failedBackground` / `failedBanners` | `App.vue:851-868`、`HomeView.vue:74/90` | 图片层失败回退（`bgStyle` → `null`；banner 列表过滤） |
| `motion` | `ui-system.css:53-57`、`html[data-motion]`、`:120`、`styles.css:661-674` | reduced-motion 关动画；`html[data-visibility="hidden"] *{animation-play-state:paused}` |
| `data-platform="darwin"` | `styles.css:7-8`、`main.ts:8` | macOS 侧栏 padding-top 38 + shell radius 0 |

### G.8 落地顺序建议

1. **先做 G.5 的颜色编译器**（Python/C# 一次性把 C.8 表算成 6 份 `.xaml` ResourceDictionary）—— 这是所有层叠的 alpha 地基，且可自动核对。
2. **窗口层**：`WindowChrome` + `GlassFrameThickness=-1` + `DWMWA_SYSTEMBACKDROP_TYPE` P/Invoke + Win10 回退（G.2），并复刻 `nativeAppearance.ts:16-21` 的 dark/light 染色跟随。
3. **静态表面**：shell / sidebar / topbar / content 四层纯色刷 + `.shell` 1px 顶高光 Rectangle + 1px border。
4. **首页 hero 13 层**：`Grid` 叠 6 个 `Border`（card 底 / 图片 / shade / content / pill / 按钮），pill 与按钮用预模糊 banner 刷（G.1）。
5. **浮层系统**：Popup 家族（含 `.float-menu` 的**三种页面变体**，别统一成一个样式！见 A.1(g)）。
6. **动态层**（blob / bubble / 进度填充）用 `TranslateTransform` + `DoubleAnimation`，**不要**用 `Margin` 动画（会触发布局）。
7. 逐页对照 `@supports`/reduced-motion 分支截图校验：**重点核对 skin-stage 的径向半径换算**（G.3 末）与 **`.runtime-strip` 的 94% 而不是 80%**（`HomeView.vue:769` 的 `!important`）。

### G.9 已知"陷阱清单"（尾部压缩规则覆盖前面基础值）

以下每一处都必须用**后出现**的值，否则界面会偏深 / 偏浮 / 多出阴影：

| 基础值 | 被谁覆盖 | 最终 |
|---|---|---|
| `HomeView.vue:815` `.hero-shade` 双层渐变 | `:1030` | 单层 `180deg` 三段 `#29000000 → #2E000000@30% → #AD000000` |
| `HomeView.vue:860` `.runtime-strip` bg `color-mix(card 80%)` | `:769` `!important` | `var(--surface-content)` = **94%** |
| `HomeView.vue:896` `.instance-card` bg `color-mix(card 82%)` + `:898` hover `--card-2` + `:899` selected inset 环 | `:1030` `.home-dashboard .instance-card{background:transparent;box-shadow:none;border:0;border-radius:0}` | 卡片**透明**，列表底 `:1030 .instances-block{background:var(--surface-content)}`；hover/selected 的 background 在 `:1030` 内更靠后 ⇒ **保留**；**inset 环丢失** |
| `HomeView.vue:917` `.account-panel/.skin-panel` `var(--shadow)` | `:1030` | `box-shadow:none`，两面板融合一张 |
| `HomeView.vue:916` `.home-creator` inset 环 + `var(--shadow)` | `:1030` | `box-shadow:none` |
| `HomeView.vue:894` `.instance-grid{grid-template-columns:repeat(4,…)}` | `:1030` `display:flex!important;flex-direction:column` | **单列列表**，不是 4 列网格 |
| `HomeView.vue:946` `.skin-stage height:300px` | `:1026` → `:1030` → **`:1033`** | `height:230px` |
| `HomeView.vue:818` `.hero-kicker` `min-height:34px;padding:0 14px;font-size:13px` | `:1030` | `font-size:12px; padding:5px 10px; align-self:flex-start`（**无 min-height 覆盖 ⇒ 34px 仍生效**） |
| `HomeView.vue:819` h1 `font-size:clamp(46px,5.2vw,64px); white-space:nowrap` | `:1030` | `clamp(24px,2.6vw,32px)` + `-webkit-line-clamp:2`（两行）；**`text-shadow` 未覆盖 ⇒ 保留** |
| `HomeView.vue:799` `.hero-card{height:var(--banner-h);min-height:348px}` | `:1030` | `height:var(--banner-h)(=430px, App.vue:1010); max-height:360px; min-height:320px` ⇒ **实际渲染高 360px**。注意 `:1022-1028` 的 `@media (max-height:760px){.hero-card{height:340px}}` **被 `:1030` 的 `height:var(--banner-h)` 反超**（媒体查询不加特异度，纯看行序），只剩 `max-height:360px` 在兜 ⇒ 该媒体查询对 hero 高度实际无效，但对 `.skin-stage{height:230px}`（`:1026`）也才被 `:1030 260px` → `:1033 230px` 重新拿回 |
| `styles.css:451` `.card{background:var(--card);box-shadow:var(--shadow)}` | `ui-system.css:13` | `var(--surface-content)` + `box-shadow:none` |
| `styles.css:549` `.modal{background:color-mix(card-solid 96%)}` | `ui-system.css:37` | `var(--surface-raised)` = **97%** |
| `styles.css:582` `.float-menu`（blur24+sat130，bg 96% card-solid） | `HomeView.vue:958`（scoped）→ `:959 .card-float-menu` / `GameView.vue:2202 .instance-more-menu` | HomeView：blur24 无 sat、bg 94% card；card-float-menu / more-menu：**bg 100%，blur 失效** |
| `styles.css:580` `.menu-overlay z-index:90` | `HomeView.vue:957` | `8000` |
| `connection.css:15` `.connection-panel,.network-overview{background:color-mix(card 94%,var(--bg)); box-shadow:var(--shadow)}` | `.network-overview` 被**同文件更后的 `:29` shorthand** 重置；`.connection-panel` 被 **`ui-system.css:27`（特异度 0,2,0 > 0,1,0，与导入顺序无关）** 覆盖 | `.network-overview` = 双层（`110deg accent-soft→transparent@70%` + `var(--card)`）；`.connection-panel` = `var(--surface-content)`，且 `ui-system.css:78` 再给 `box-shadow:none`（嵌套 `.connection-panel .connection-panel` → `background:transparent`）。**注意 `connection.css` 是由 `ServersView.vue:4` / `FriendConnectView.vue:4` 引入的模块级 CSS，注入时机晚于 `main.ts:4-5` 的两个全局表 —— 判定这类冲突时以特异度为先、行序为后** |
| `GameView.vue:1885-1887` `.game-tabs-blob{background:var(--accent-grad); box-shadow:0 2px 8px var(--accent-soft)}` | `:2168`（尾部压缩，在 `@media` 之前但同特异度靠后） | `background:var(--accent-soft)` + `box-shadow:inset 0 -2px var(--accent)` |
| `ServersView.vue:474` `.server-list-item.active` 双层渐变 + inset 竖条 | `:515` | `background:var(--accent-soft); box-shadow:none; border-radius:var(--radius-sm)` |
| `CommunityView.vue:749-750` `.capsule-blob{background:var(--accent-grad); box-shadow:0 2px 8px …}` | `:1083`（`!important`） | `background:var(--accent-soft)` + `box-shadow:none` |
| `FriendConnectView.vue:126-141` `.pick-card`（含动画与 shadow） | `:161` | `animation:none; box-shadow:none`（blur 仍在但底色 94% ⇒ 不可见） |
| `SettingsView.vue:1552-1569` `.theme-preview 150×84` + `.theme-option.active .theme-preview{box-shadow:0 0 0 3px}` | `:1865` / `:1867` | `46×32`、`box-shadow:none` |
| `SkinsView.vue:604+` 第一块 `<style scoped>`（`:602-1080`）全部 `.pane-*` | **第二块 `<style scoped>`（`:1081-1109`）** | `.skins-page` 变 grid；`.pane-info/.pane-capes` 变 `background:transparent;box-shadow:none`（被包进 `.skin-operation-panel{background:var(--surface-content)}`）。**全站唯一双 `<style scoped>` 文件，后块一定压前块** |
| `styles.css:200` `.btn-gold{background:var(--accent-grad);box-shadow:0 2px 6px}` | `ui-system.css:119`（仅默认主题） | `background:var(--accent-deep)` + `box-shadow:none` ⇒ **默认黑紫主题下主按钮是纯色** |

---

## 附：本文里出现"未找到"的项（避免误建）

1. `--glass-blur` 的**消费者**：未找到（只 `App.vue:1007` 写、`:937` 列清理）。
2. 除 `App.vue:1748` 之外的 `@supports not` 兜底：未找到（共 1 处）。
3. `.shell` / `.sidebar` / `.topbar` / `.content` / `.instance-card` / `.account-panel` / `.skin-panel` / `.status-strip` / `.toast` 上的 `backdrop-filter`：未找到。
4. tooltip 的 CSS 实现（含箭头伪元素）：未找到 —— 全站只用原生 `title` 属性。
5. 跑马灯的 `mask-image` / `-webkit-mask` 渐隐遮罩：未找到 —— `MarqueeText.vue:13-19` 只用 `overflow:hidden` + `transform:translateX(var(--travel))`。
6. checkbox 的自绘 `::before/::after` 打勾：未找到 —— 一律 `accent-color: var(--accent)`（`App.vue:2593/2595`、`HomeLayoutEditor`、`FileManager.vue:719`、`EditPanel.vue:74`）用系统原生勾选框。
7. `.select` 下拉箭头的伪元素：未找到 —— 是 `background-image` 里的内联 SVG（`styles.css:338`）。
8. 名为 window-gradient / 窗口渐变 的规则：未找到（见 B.5）。
9. `filter:` 中的 `contrast()` / `grayscale()` / `sepia()` / `hue-rotate()` / `invert()`：未找到。
10. `.ic-lightbox`、`.notice-mask`、`.menu-overlay`、`.cfg-capture-mask`、`.cfg-mismatch-mask` 上的 blur：未找到（**纯透明或纯不透明，无模糊**）。
11. 任何 `mask` / `-webkit-mask` 属性（全类名）：未找到。
12. `MixEffect` / `OpacityMask` 类"图片叠色"技巧用于 `.app-bg`：未找到（只有 `opacity` + `filter` + 负 `inset`，见 E.1）。
