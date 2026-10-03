# `--accent-2` 全量台账（U12）

原版 `--accent-2` 不是静态色，是 `App.vue:969` 运行时算的：暗色 `color-mix(accent 72%, white)`、
亮色 `color-mix(accent 84%, black)`。WPF 侧对应 brush key = **`AccentHi`**。
移植时最容易犯的错就是把它当成 `--accent` 本体——色相偏一个档，深色主题下尤其明显。

本表由子 Agent 全库审计 + 本人逐条回源码复核得出。**已改完的行不再列**。

## 1. 已修（14 处，本轮）

| 原始位置 | 选择器 / 属性 | WPF 位置 | 改动 |
|---|---|---|---|
| App.vue:1798 | `.logo-version` color | MainWindow.xaml | Accent → AccentHi |
| App.vue:2092 | `.top-btn:hover` color | Controls.xaml `TopBtn` | Accent → AccentHi |
| styles.css:283 | `.icon-btn:hover:not(:disabled)` color | Controls.xaml `IconBtn` | Accent → AccentHi |
| App.vue:2281 | `.dl-bar-fill` background（90° accent-2→accent） | Controls.xaml `ProgressBar` | Accent → 新 token `BarFill` |
| SettingsView.vue:1719 | `.mem-slider-fill` background（同上） | SettingsPage.xaml | AccentGrad → `BarFill`（accent-grad 是 135° 三停，不是这个） |
| HomeView.vue:831 | `.loader-badge` border `color-mix(accent-2 34%, transparent)` | Controls.xaml `LoaderBadge` | AccentSoft → 新 token `AccentHiSoft` |
| HomeView.vue:909 | `.instance-play` color | Controls.xaml `SquareBtn` | Accent → AccentHi |
| HomeView.vue:873 | `.runtime-item:hover` color | Controls.xaml `RuntimeCell` | 补 hover Foreground=AccentHi |
| HomeView.vue:880 | `.runtime-state strong` color | HomePage.xaml | 补 AccentHi |
| HomeView.vue:923 | `.account-copy span` color | HomePage.xaml 账号状态行 | Accent → AccentHi |
| styles.css:620/623 + HomeView.vue:963 | `.menu-item:hover/.active` color | Controls.xaml `MenuItem2` | 补 hover Foreground=AccentHi |
| HomeLayoutEditor.vue:404 + GameView.vue:1700 | `.capsule.active` color | Controls.xaml `Capsule`/`CapsuleRadio` | 补 AccentHi |
| SettingsView.vue:1819 | `.mirror-option.active` color | Controls.xaml `CapsuleRadio` | 补 AccentHi |
| SettingsView.vue:1840 | `.settings-scopes [aria-current]` color | Controls.xaml `ScopeTab` | Accent → AccentHi |
| CreatorMotto.vue:68 | `.motto-glyph.accent` color | Controls/CreatorMotto.cs | 用 AccentHi（本轮新增） |

**过纠正一处（已回退）**：`SettingsView.vue:1578` `.theme-option.active .theme-label` 用的是
`--accent` **本体**而不是 accent-2，批量替换时会被 `Capsule` 的同形 trigger 吸进去，改回 `Accent`。

## 2. 复核后判定"不该改"

| 原始位置 | 说明 |
|---|---|
| HomeView.vue:874 `.runtime-item > svg:first-child` | 色是 `--text`，不是 accent-2；只有 `.runtime-state > svg:first-child`（我们那一行是 7px 圆点不是 svg）才是 accent-2 |
| HomeView.vue:880 的 error 分支 | `.runtime-state.error strong { color: --danger }` 会覆盖 accent-2；等真实运行状态接入时按状态切色，现在先留默认档 |

## 3. 未实现（对应功能没搬，色先记着）

| 原始位置 | 选择器 |
|---|---|
| App.vue:2538 | `.mp-name-opt.active .mp-name-value` |
| DefaultGameOptions.vue:78/79/80 | `.mouse-shortcut strong`、`.option-number`、`.option-caption strong` + `.option-foot button` |
| DupCleanModal.vue:346 | `.ver-chip.active` |
| FileManager.vue:531 | `.fm-path:hover` |
| FileManager.vue:691 | `.upd-ver b` |
| HomeLayoutEditor.vue:462 | `.bg-val` |
| UpdateModal.vue:101 | `.upd-body :deep(a)` |
| AccountsView.vue:945 | `.user-code` |
| BridgeView.vue:330 | `.bridge-reset:hover` |
| FriendConnectView.vue:144/157/160 | `.pick-card:focus-visible`、`.pick-scene`、`.pick-go` |
| GameView.vue:2042 | `.loader-option.active` |
| HomeView.vue:899 / :954 | `.instance-card.selected` 边框 / `.skin-tip:hover` |
| SkinsView.vue:757 | `.drag-hint` |
| SettingsView.vue:1250/1292/1305/1375/1692/1747/1819 | 更新态三处、`.collapse-head:hover .group-title`、`.memory-value`、`.memory-refresh` |
| connection.css:9/12/62/80 | `.connection-eyebrow`、三处 `:focus-visible` outline `2px solid accent-2` |
| FrpPanel.vue:276 | `.node-badge.vip` 色 + 边框 |
| TerracottaPanel.vue:254/258 | `.stage-item.active` 边框、`.stage-dot` 色 |
| ServersView.vue:476 | focus outline |

`focus-visible` 一族在 WPF 没有等价物，需要单独设计键盘焦点视觉（记在 U4 动效轮）。

## 4. 已知近似（不改，留档）

- `.instance-play` 底色原版是 `color-mix(accent 18%, card-2)`，我们用 `AccentSoft`（14% 透明叠加）；
  差 4% alpha，深色底上肉眼不可分，不为它单开 token。
- 所有 `color-mix` 混合按 naive sRGB 通道线性插值（与既有 `AccentHi`/`AccentLo` 同一算法），
  不是 CSS 规范要求的线性光 sRGB。全库一致，故不单独偏离。
