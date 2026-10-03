# UI 几何台账 · 二级页面

来源：`D:/桌面/WUHUICL/KAMUCL-1.1.5/src/renderer/src/`（只读取证，未改动原工程）。
所有数值均带 `文件:行`。取值原则：同一选择器多次声明时**只取最终生效的那条**，基线值在括号里标注，避免照抄被覆盖的旧值。

## 0. 全局骨架（所有页面共享，非各页自己的）

- `.shell` flex，100%×100%，radius `var(--radius-md)`=10px，border 1px — App.vue:1725-1736
- `.sidebar` width `var(--sidebar-w)`=208px（styles.css:72），border-right 1px — App.vue:1755-1762；macOS 独有 `padding-top:38px` — styles.css:7（**Windows 无此内缩**）
- `.logo-area` height 88px，padding `0 16px`；`.brand-head` 42×42 — App.vue:1764-1774
- `.topbar` height 72px，padding `0 24px`，border-bottom 1px，`-webkit-app-region: drag` — App.vue:2016-2027；`@max-1080` 时 68px — App.vue:2410
- `.content` flex:1，min-height:0，overflow-y:auto，**`scrollbar-gutter: stable`**，padding `var(--space-5)`=24px — App.vue:2398-2404；`@max-1080` padding 16px — App.vue:2407
- 滚动条槽宽：`::-webkit-scrollbar { width:8px; height:8px }` — styles.css:139-142
- 令牌（唯一真值源）：`--space-1..7`=4/8/12/16/24/32/48、`--text-xs..2xl`=12/13/14/16/20/26、`--radius-sm/md/lg`=6/10/14、`--ctl-h`=36px、`--row-h`=42px、`--card-pad`=20px、`--card-gap`=16px、`--sec-gap`=28px — styles.css:45-70；`--text-title: 24px` — ui-system.css:6
- 页头基线：`.page-head` flex column gap 4px（styles.css:498-502）；`.content .page-head{margin-bottom:8px}` — ui-system.css:11；`.content .page-title{font-size:24px;line-height:1.35}` — ui-system.css:12（**覆盖** styles.css:503-508 的 20px/700）；`.page-sub` 13px `--text-dim` — styles.css:509-512
- `.content .page{gap:var(--space-4)=16px}` — ui-system.css:10（覆盖各页自己的 `gap:var(--sec-gap)`）
- 模态配方：`.modal-mask` fixed inset:0，z-index 10000，flex 居中，`backdrop-filter: blur(10px)` — styles.css:527-538；`.modal` `width:min(460px, calc(100vw - 48px))`，`max-height:calc(100vh - 96px)` — styles.css:545-555，再被 `.modal{max-width:calc(100vw - 32px);max-height:calc(100vh - 32px);overflow:auto}` 覆盖 — ui-system.css:38；radius 14px、padding 24px、`background-color:var(--surface-raised)` — ui-system.css:37
- `.modal-actions` flex，`justify-content:flex-end`，gap 12px，margin-top 24px，flex-wrap — styles.css:557-563 + ui-system.css:40
- 控件：`.btn` min-height 36px / padding 8px 16px / radius 10px / gap 8px — styles.css:158-177；`.btn-sm` `min-height:calc(36px - 8px)` padding 4px 12px — styles.css:258-263，被 `.content .btn-sm{min-height:32px}` 覆盖 — ui-system.css:17；`.icon-btn` 32×32 radius 6px，svg 16×16 — styles.css:266-301；`.input/.select` min-height 36px，padding 8px 12px，radius 10px — styles.css:304-317，`.select` `padding-right:32px` + 箭头 `background-position: right 12px center` — styles.css:336-340
- `.switch` 40×22，`.switch-ui::before` 14×14 `left:3px` `translateY(-50%)`，选中 `translate(18px,-50%)` — styles.css:356-401
- `.slider` track height 6px radius 999px，thumb 16×14 hover scale(1.2) — styles.css:414-439
- `.tag` padding 2px 8px radius 999px font 12px — styles.css:459-472
- `.float-menu` `min-width:150px;max-height:300px`，padding 8px，radius 10px，z-index 95，`blur(24px) saturate(130%)` — styles.css:582-596；`.menu-item` min-height 42px，padding 4px 12px，radius 6px — styles.css:597-616
- `.empty` flex column center gap 16px padding `48px 24px` — styles.css:515-524；`.content .empty{min-height:120px;padding:32px 16px}` — ui-system.css:44
- `.status-strip` flex wrap gap 12px padding 12px 16px radius 10px margin-bottom 12px — ui-system.css:41
- Toast：`.toasts` fixed `top:76px` `right:24px` z-index 200 gap 8px；`.toast` max-width 360px padding 12px 16px `border-left-width:3px`；`.toast-close` 28×28 — Toasts.vue:16-42

## 1. GameView.vue（版本/游戏页 — 优先）

顶层：`.page` flex column，`max-width:1240px; gap:16px`（GameView.vue:2164，覆盖 :1510-1516 的 940px/`var(--sec-gap)`），`margin:0 auto`。
行序 = page-head(:926) → game-tabs(:931) → folder-manager(:943) → game-controls(:1003，仅 download) → catalog-status(:1045) → latest-release/list-card 或 installed-card(:1050/:1099)。无页脚。

页头：h1 24px(ui-system.css:12)、sub `--text-sm`。`.page-head{margin-bottom:0}` — :2165。

Tab 分段（唯一带 JS 定位指示块的）：
- `.game-tabs` inline-flex，`gap:4px`，`padding:4px`，radius 999px → 覆盖为 `border:0; border-radius:var(--radius-sm); background:var(--hover)` — :1870-1879 + :2167
- `.game-tab` `height:36px; padding:0 20px; border-radius:6px` — :2169（基线 :1892-1910 为 `padding:0 24px;height:var(--ctl-h)`）
- `.game-tabs-blob` `position:absolute; top:4px; bottom:4px`；最终指示器 = `border-radius:6px; background:var(--accent-soft); box-shadow: inset 0 -2px var(--accent)`（2px 底部下划线）— :1881-1891 + :2168
- left/width 由 JS 写入 inline `left:{n}px;width:{n}px` — GameView.vue:120-124

文件夹卡：
- `.folder-manager` `padding:12px 16px; gap:8px; border:0` — :2171（基线 :1519-1525 `padding:20px;gap:12px`）
- `.folder-manager-main` row，align-center，gap 16px — :2172
- `.folder-select-wrap` `grid-template-columns:auto minmax(160px, 320px) minmax(60px, 1fr); gap:12px` — :2173（基线 :1534 `minmax(210px,320px)`）
- `.folder-caption` 12px；`.folder-current-path` mono 12px ellipsis — :1545-1551
- `.folder-manager-actions` gap 8px，`padding-top:8px; border-top:1px solid var(--border)` — :2176
- `.folder-missing-card` padding `12px 16px`，radius 10px — :1561-1572
- `.folder-state-dot` 7×7 — :1588-1594

工具行：
- `.game-controls` flex wrap，`gap: 12px 16px` — :1623-1628
- `.toolbar` flex，gap 12px，`flex:1 1 320px; min-width:260px; justify-content:flex-end` — :1629-1637
- `.tool-search` height 36px，padding `0 12px`，radius 999px，gap 8px，`min-width:180px`，svg 14×14 — :1638-1660
- `.capsule` height 36px padding `0 16px` radius 999px — :1681-1696
- `.filter-capsules` gap 8px — :1676

列表卡：
- `.list-card` `padding:8px` — :1710
- `.version-row` flex space-between gap 12px，**`min-height:42px; padding:8px 12px; border-radius:10px`**，hover `background:var(--card-2)` — :1718-1730
- `.version-info` gap 12px — :1731
- `.inst-icon` **34×34**，padding 4px，border 1px，radius 6px — :1751-1765
- `.version-date` 12px — :1776
- `.row-bar` **90×6px** radius 999px，`.row-progress` gap 8px — :1787-1799

已安装区：
- `.installed-card` `padding:0 16px; border:0; border-radius:10px` — :2178
- `.installed-scope` `padding:12px 0; gap:12px`，:deep(.select-menu-btn) `min-width:160px;max-width:260px` — :2179-2181
- `.installed-search` `flex:1;min-width:160px` — :2182；`.scope-count` 12px
- `.installed-row`（最终态）= flex nowrap，`gap:12px; padding:12px 0; border-radius:0; min-height:80px`，hover/focus-within `background:var(--hover)` — :2185-2186（基线 :1819-1829 为 min-height 42px + border-bottom）
- 子几何：`.fav-btn` 28×28 — :1946-1959；`.inst-names` `flex:1 1 auto; gap:8px` — :2187；`.instance-name` 2 行 clamp，weight 650 — :2188；`.instance-meta` `gap:4px 10px; font-size:12px`，分隔符 `·` + `margin-right:10px` — :2191-2194；`.instance-commands` column align-end gap 8px — :2195；`.row-actions` `gap:4px; margin-left:auto; flex-shrink:0` — :1937-1943（内层覆盖 gap 8px — :2196）；`.instance-more` `min-width:32px; padding:0 8px; font-size:20px` — :2197；`.installed-launch` `min-width:78px` — :2198；`.played-text` 12px ellipsis — :1926；`.iso-switch` — :1981
- `.installing-block` border-bottom 1px — :1919；`.fav-group-head` `padding:8px 4px 4px`，color `#f5b301` — :1970-1978

更多菜单：
- `.instance-more-menu` `width:290px; max-width:calc(100vw - 16px); max-height:calc(100vh - 24px)` — :2202
- `.instance-technical` `padding:12px; font-size:12px; border-bottom:1px` — :2203
- `.menu-danger-zone` `border-top:1px; margin-top:6px; padding-top:6px` — :2205

模态：
- `.modal-title` 16px/700，`margin:0 0 16px` — :2005
- `.modal-label` 13px，`margin:16px 0 8px` — :2010
- `.loader-option` height 36px padding `0 16px` radius 999px — :2020-2035
- `.instance-resolution-size` flex align-end gap 12px margin-top 16px — :2089
- `.isolation-modal` `width:min(620px, calc(100vw - 40px))` — :2107
- `.isolation-paths` **`grid-template-columns: 36px minmax(0, 1fr)`**，`gap:8px 12px` — :2113-2121
- `.isolation-items` `max-height:230px`，gap 8px — :2134；`.isolation-item` padding `8px 12px`，radius 6px — :2141

断点：
- `@max-1120` folder-manager-main 转 column（:1609）
- `@max-1180` installed-row wrap，`padding-block:12px`，`.inst-names{flex-basis:calc(100% - 100px)}`（:1997-2002）
- `@max-1100` folder-select-wrap→`auto minmax(160px,1fr)`，path 跨列（:2207）
- `@max-850` **`.installed-row{grid-template-columns:28px 36px minmax(0,1fr)}`**，installed-card padding `0 12px`，`.scope-count{display:none}`（:2212-2222）

## 2. SettingsView.vue（设置页 — 优先）

顶层是 4 行 × 2 列 CSS Grid，`display:grid!important`：

```
.settings-page { height:100%;
  grid-template-columns: minmax(0,1fr) minmax(220px,340px);
  grid-template-rows: auto auto auto minmax(0,1fr);
  gap:10px!important; padding:0!important;
  max-width:1080px!important; position:relative }
```
— SettingsView.vue:1834（基线 `.page{max-width:760px}` — :1308-1314；ui-system.css:23 的 `.content .settings-page{max-width:1120px}` 被 !important 的 1080 压过）

网格槽位：page-head = r1c1 (:1835)；settings-search = r1c2 (:1837)；settings-scopes = r2 `grid-column:1/-1`，`gap:6px` (:1838)；settings-categories = r3 `1/-1`，`border-bottom:1px`，`padding:0 0 8px`，`gap:4px` (:1841)；settings-body = r4 `1/-1`，`min-height:0; overflow:auto; scrollbar-gutter:stable; gap:12px; padding:2px 8px 20px 0` (:1843)。
`@max-800` 转 1 列 5 行，search→r2、scopes→r3、categories→r4、body→r5，results `top:92px` — :1887-1890。
`.content:has(.settings-page){overflow:hidden}` — ui-system.css:74（页内滚动，不是整页滚动）。

**重要**：`.settings-navigation{display:contents}` — :1836 使 :1233 的 `sticky top:0 / padding:12px / margin-bottom:16px / radius:14px` **不生效**（该盒子不渲染），只需实现 4 个槽位。

- `.settings-search` flex，gap 12px，input flex:1 — :1234-1236
- `.settings-scopes .btn` `font-size:15px; font-weight:650; min-height:38px; padding:8px 18px; border-color:transparent`；`[aria-current]` = accent-soft + accent-2 — :1839-1840（这是"范围"分段控件，无指示条，靠背景色）
- `.settings-categories .btn` `border:0; border-radius:6px; min-height:32px; padding:6px 12px; font-size:13px` — :1842（分类行 = 纯胶囊，非下划线）
- `.settings-search-results` **absolute `right:0; top:48px; z-index:20; width:min(480px,100%); max-height:60vh`**，padding 12px，radius 10px — :1845
- `[data-section]` `scroll-margin-top:12px` — :1848（覆盖 :1241 的 150px）

卡片配方：
- `.card`+`.group` = `padding:14px 18px`（:1849 覆盖 :1331 的 20px），flex column gap 12px — :1327-1332
- `.group-title` 15px（:1853，覆盖 :1333 的 13px/700）
- `.group-hint` 12px line-height 1.6 margin `6px 0` — :1855
- `.group-inline` row align-center gap 16px，`.switch{flex:none}` — :1856-1858 / :1349-1354
- 折叠卡（PCL 式，本页独有）：`.collapse{padding:0;gap:0}` — :1850；`.collapse-head` **`min-height:42px; padding:10px 18px`**，`list-style:none` — :1852 + :1360-1370；`.collapse-arrow` `7px×7px`，`border-right/bottom:2px solid var(--text-dim)`，`rotate(-45deg)` → `[open]` `rotate(45deg)` — :1377-1390；`.collapse-body` `padding:10px 18px 14px; gap:8px; border-top:1px solid var(--border)` — :1851 + :1391-1397
- 布局容器：`.settings-grid` `grid-template-columns:minmax(0,1fr)`（:1846 覆盖 :1319 的 `repeat(auto-fit,minmax(300px,1fr))`），gap 12px，align-items start；`.feature-grid` `repeat(2,minmax(0,1fr)); column-gap:24px` — :1872；`.feature-row` `min-height:40px; padding:6px 0`（:1875 覆盖 :1400-1408 的 border-bottom）；`.runtime-grid` `repeat(2,minmax(0,1fr)); gap:12px`，`@max-1100`→1 列 — :1876 + :1886；`.jvm` 段 `grid-column:1/-1` — :1877
- 主题选择：`.theme-options` `repeat(auto-fit,minmax(145px,1fr)); gap:8px`（:1862 覆盖 :1538 的 `auto-fill minmax(150px,1fr)` gap 12px）；`.theme-option` row，gap 10px，`min-height:48px; padding:6px 8px; border-radius:6px` — :1863；`.theme-preview` **46×32px，radius 5px**（:1865 覆盖 :1552-1561 的 150×84 / radius 10 / border 1.5px）；`.tp-check` `14×14; top:1px; right:1px; padding:2px` — :1869（覆盖 :1621-1631 的 18×18/6px）；`.tp-side` width 9px（:1869 覆盖 :1583 的 26px）、`.tp-dot` 4×4、`.tp-top` height 6px、`.tp-btn` height 4px；`.theme-label` 13px — :1868；`.personalize-btn` `min-height:32px; padding:5px 12px; margin-top:6px` — :1871
- 内存滑块（自绘，非 `.slider`）：`.mem-slider` `flex:1; height:24px; position:relative` — :1712；`.mem-slider-track` absolute `left:0;right:0;top:50%;height:6px;transform:translateY(-50%)` — :1713；`.mem-slider-fill` 同上 `left:0` — :1717；`.mem-slider-thumb` **16×16**，`transform:translate(-50%,-50%)`，`border:2px solid var(--on-accent)`，hover `scale(1.12)`，dragging `scale(1.18)` — :1724-1731；`.memory-value` `width:172px; text-align:right; padding:2px 4px` — :1685-1699；`.memory-input` `width:88px` — :1701；`.memory-auto-row` `padding-bottom:12px; margin-bottom:12px; border-bottom:1px` — :1702-1710（grid 内覆盖为 `margin:0 0 8px; padding:0 0 10px; gap:12px` — :1878）
- Java 列表：`.java-list` radius 10px `overflow:hidden` — :1417，`.runtime-grid .java-list{max-height:200px;overflow:auto}` — :1882；`.java-item` `min-height:42px; padding:4px 12px; border-top:1px`，13px — :1466-1474；`.java-list-head` `padding:8px 12px` — :1458-1465；`.java-scan-track` height 3px — :1445；`.java-auto-row` padding 12px radius 10px — :1505-1515；`.java-detected>summary` `padding:10px 0` — :1881
- 分辨率：`.resolution-row` flex gap 12px wrap — :1757；`.res-field` flex:1 gap 8px — :1763；`.resolution-mode .select` **`min-width:108px`** — :1784；grid 内 `margin-top:8px`/mode `margin-top:10px` — :1884
- 镜像：`.mirror-option` `min-height:36px; padding:0 16px; border-radius:999px; gap:8px` — :1794-1809；`.mirror-options` gap 12px — :1789
- 下载：`.download-setting` `.input{width:160px}` — :1301，`min-height:44px` — :1883；`.download-mirror-group{margin-bottom:10px}` — :1883
- 关于更新：`.upd-row` `min-height:38px; padding:6px 0`（:1885 覆盖 :1247 的 `padding:8px 0`）；`.upd-label` `width:84px` — :1248；`.upd-source` `min-width:220px` — :1251；`.upd-mirror` `min-width:240px` — :1252
- 更新弹窗：`.upd-modal-title` **22px**，line-height 1.4 — :1269；`.upd-modal-head` flex align-start justify-between gap 12px — :1268；`.upd-risk` `padding:12px 14px; margin:0 0 18px; border-radius:10px` — :1270-1275；`.upd-release-item` `gap:14px; padding:16px; border-radius:10px` — :1277-1280；radio `margin-top:5px` — :1283；`.upd-local-check>span:first-child` `width:72px` — :1291
- 其它：`.plugin-row` min-height 42px padding `8px 12px` radius 10px — :1295；`.dir-row` gap 12px，input flex:1 — :1666-1674；`.background-settings` `display:grid;gap:12px;order:2` — :1861；`[data-section=motion]{padding:12px 18px;order:1}` — :1859
- 断点：`@max-1100`（:1886）、`@max-800`（:1244 + :1887）；`@max-800` 时 `.settings-navigation{position:relative}`（:1244，随 display:contents 已无效）

## 3. AccountsView.vue

- `.page` flex column `max-width:720px`，gap 16px(ui-system.css:10) — :672-677。行序：acc-top → 类型 Tab → mode-panel → account-list
- `.account-type-tabs` **`grid-template-columns: repeat(3, minmax(0, 1fr))`**，`gap:4px; padding:4px; border:1px; border-radius:10px; background:var(--card-2)` — :717-724
- button `min-height:36px; padding:0 8px; border-radius:6px; font-size:12px`，`.active` = `background:var(--accent-grad)` + `color:var(--on-accent)` + 600（实心填充，非指示条）— :726-749
- `.account-list` column gap 8px — :859；`.account-row` flex，`min-height:42px; padding:12px 16px; border:1px; border-radius:10px; background:var(--card-2)`，gap 12px — :864-874
- `:hover` border `--accent-deep` — :876；`.selected` border accent + bg accent-soft — :879
- `.avatar` **42×42，radius 50%**，16px/800 — :883-894
- `.account-meta` flex:1 gap 4px — :896；`.provider-url` `max-width:520px` — :817
- `.provider-item` min-height 42px padding 12px — :805-811；`.provider-head`/`.provider-list` gap 8px — :785/:801
- `.ygg-login-grid` **`grid-template-columns: 1fr 1fr`**，gap 12px — :832-836；`.ygg-login-btn` min-height 36px — :846；`.provider-source` `min-height:84px` — :1004
- `.add-row`/`.add-input-wrap` gap 8px/12px — :760-768；`.back-btn` `padding:8px 12px; border-radius:6px`，align-self flex-start，svg 15×15 — :685-708；`.list-empty` `padding:32px 24px` — :856
- 模态：`.ms-modal` `text-align:center` — :927；`.user-code`（设备码大字）见 :939 区块；`.ms-uri-row` 内 `.input` + `.btn` — :967-978；宽度走通用 `.modal` 460px（styles.css:546）；`.modal-actions` gap 12px margin-top 24px — :986
- 无页内 grid 分栏、无 sticky、无断点（除 ui-system.css 的 900/660）

## 4. ModsView / PacksView / ShadersView（三个 16 行壳，全部 = FileManager）

`<FileManager title rel open-label empty-text :icon />` — ModsView.vue:9-15 / PacksView.vue:9-15 / ShadersView.vue:9-15。**这三页零自有几何**，全部继承 FileManager.vue（靠 `data-resource` 属性切换列数）：

- `.resource-page` flex column `max-width:940px` — FileManager.vue:504
- `.fm-head` row gap 16px flex-wrap（:511 + :713/:722），`.resource-page .fm-head{flex-direction:column;align-items:stretch}` — :508
- `.fm-context>label` `flex:0 1 440px`，12px — :719；`.fm-ver-select` `max-width:240px;min-width:180px` — :57
- `.fm-controls` **`grid-template-columns:minmax(180px,1fr) 160px 140px auto`**，gap 10px，margin-bottom 16px — :494；最终覆盖为 **`minmax(180px,1fr) 160px 140px 38px`** — :719；`@max-1000`→`minmax(0,1fr) minmax(0,1fr) 38px` — :724
- `.fm-batch` **`position:sticky; top:0; z-index:6`**，padding `12px 16px`，gap 8px — :491/:719
- `.fm-card` `padding:0 16px!important; overflow:visible` — :719（基线 :545 `padding:8px`）
- 表头 + 行同列宽：`.fm-table-head,.fm-row` **`grid-template-columns:36px minmax(0,1fr) 88px 132px 40px; gap:12px`** — :719；`.fm-table-head` `min-height:36px; 12px; border-bottom:1px`
- **mods 专属（`.file-manager[data-resource=mods]`）**：`.fm-row` = `18px 32px minmax(0,1fr) 88px 132px 42px 40px`（7 列，含 checkbox + 图标），`.fm-table-head` 同名 7 列，head 第 2 子跨 `2/4` — :719
- `.fm-row` `min-height:68px; padding:12px 0; border-radius:0` — :719/:722；`:hover` `background:var(--hover)` — :722；`:has(>input:checked)` `background:color-mix(in srgb,var(--accent) 5%,transparent)` — :719
- `.fm-file-icon` 36×36（:719；基线 :565 为 32×32；mods 下 32×32，资源包/光影 `.fm-thumb` 44×44 — :623 区块）；`.fm-date` 12px text-left；`.fm-meta` `min-height:20px`(基线)，text-right
- 下拉：`.file-more` `position:relative`；`.file-more summary` `font-size:18px; min-width:36px`；`.file-more-actions` **absolute `right:0; top:100%`**，`min-width:150px`，padding 10px，gap 8px，radius 10px — :485-489
- `.fm-pagination` flex，`justify-content:flex-end`，gap 12px，padding `8px 0`，13px；`.muted{margin-right:auto}` — :499
- 断点：`@max-1150` **`.fm-table-head,.fm-row{grid-template-columns:32px minmax(0,1fr) 70px 36px}` + `.fm-date{display:none}`**，mods 变体 `18px 28px minmax(0,1fr) 70px 38px 36px` — :720；`@max-700` `.fm-search{grid-column:1/-1}`，mods 变体 `18px 24px minmax(0,1fr) 38px 36px` 并隐藏 meta/日期列 — :720
- ShadersView 空态：`.shader-empty{background:transparent;border:0;backdrop-filter:none}`，`.shader-empty .empty{min-height:min(380px,45vh)}` — :719

## 5. ServersView.vue

根是 `.connect-page servers-page`（**不是 .page**）→ 继承 connection.css：`max-width:1120px`，flex column `gap:var(--sec-gap)` — connection.css:3；ui-system.css:35 把 `.connection-columns` 改为 `repeat(auto-fit,minmax(min(100%,300px),1fr))`。

- `.servers-page{gap:16px}` — ServersView.vue:62(区块内)；`@min-1121` `max-width:1480px; margin-inline:auto; height:100%` — :82/:63
- 头部 `.connection-header` row，`justify-content:space-between`，gap 24px，flex-wrap — connection.css:6 + 页面内 `margin:0;padding:0` — :62；`h1` 26px/700 `-0.5px`（connection.css:7）→ `--text-title:24px`（ui-system.css:33）；`h1 small` 12px `margin-left:12px` — :62
- 工具行 `.server-toolbar` flex wrap gap 12px（覆盖 :4 的 `var(--space-3)` 为 12px — :62）；`.server-search` `flex:1 1 250px`，**`height:var(--row-h)`=42px**，padding `0 16px`，radius 10px，svg 18×18 — :5/:7
- **主体两列**：`.server-workspace` `grid-template-columns: minmax(260px,1fr) minmax(300px,1.05fr)`，gap 16px，align-items start — :14 + :65；`@min-1121` → **`minmax(360px,1.2fr) minmax(320px,1fr)`**，`min-height:0; flex:1` — :63；`@max-1120` → `minmax(0,1fr)` 单列 + `.server-list{max-height:360px;overflow:auto}` — :83
- 列表容器 `.server-list` column，最终 `gap:0`(:66)，`padding:8px`(:66)，radius `--radius-lg` — :62/:63；两列独立滚动：`.server-list,.server-detail{max-height:calc(100vh - 270px); overflow:auto; scrollbar-gutter:stable}` — :63；`.server-detail` `position:sticky; top:0` — :41/:82
- 行几何（最终态）：`.server-list-item` **`min-height:86px`，`padding:0 4px`，`border:0`，`border-bottom:1px solid var(--border)`，`border-radius:0`，background transparent** — :67（覆盖 :62 的 `min-height:80px;padding:0 8px`；基线 :16 为 42px + radius 10 + border 1px）；`.active` = `background:var(--accent-soft); border-radius:var(--radius-sm)` — :68（:62 的 `box-shadow: inset 3px 0 var(--accent)` 左边线被 :68 的 `box-shadow:none` 关掉）
- `.server-row-button` flex gap 12px，`padding:12px 8px` — :70；`.server-monogram` **36×36**，radius 6px，16px/700 — :71（基线 :30 为 36×40；large 变体 48×52 — :32）；`.server-row-copy` flex:1 column gap 4px，strong 13px 2 行 clamp — :33/:62；`.server-row-state` **`min-width:66px`**，align-end — :72（基线 :38/76px）；`.server-favorite` `order:3; margin:0 4px` — :62；checkbox **18×18** `accent-color` — :57
- 批量条 `.server-batch` flex gap 8px padding `12px 16px`，radius 10px，bg accent-soft，`> .btn{margin-left:auto}` — :59
- 详情面板：`.connection-panel-head` `padding:12px 20px`，h2 16px — connection.css:114 + :81；`.connection-panel-body` `padding:20px; gap:16px` — connection.css:114 + :75；`.server-detail-title h3` **20px** — :76（覆盖 :44 的 20px/基线 21px :62）
- `.server-facts` **`grid-template-columns:1fr 1fr`**，gap 12px，`padding:12px 0; border-block:1px solid var(--border)`，strong 18px — :47/:77/:78
- `.server-description` padding `10px 12px`，line-height 1.6 — :79；`.server-connect` `width:100%; justify-content:space-between; min-height:42px` — :51；`.server-secondary .btn` `flex:1; padding:4px 8px` — :53
- 空态 `.connection-empty` `min-height:220px`，padding 24px，p `max-width:320px` — connection.css:68-70；`.server-list-hint` padding `4px 8px` — :40

## 6. SkinsView.vue

两个 scoped style：前者（:602-1080）是 flex 版基线，后者（:1081-1109）把它整体改写为 **grid**，以后者为准。

- 最终顶层：`.skins-page` `display:grid; **grid-template-columns: minmax(260px,2fr) minmax(300px,3fr)**; align-items:start; gap:16px` — :1083（覆盖 :609-614 的 flex column `max-width:1080px`）；page-head/status-strip/need-ms 跨 `1/-1` — :1084；`.row-main,.row-sub` `display:contents`（解除自身 flex 容器）— :1085
- 槽位：`.pane-preview` `grid-column:1; grid-row:2` — :1086；`.skin-operation-panel` `grid-column:2; grid-row:2`，radius `--radius-lg`，`overflow:hidden`，flex column — :1087；`.pane-history` `grid-column:1/-1; grid-row:3` — :1093
- `@max-1050` 单列 `minmax(0,1fr)` — :1103-1107；`@max-650` `.cape-grid`→1 列 — :1108；`.pane-*` `min-width:min(100%,280px)!important` — :605
- 3D 视口：`.viewer3d` `height:var(--sv3d-height,340px)`，radius 10px — SkinViewer3D.vue:124；页面设 `--sv3d-height: 380px`（:716 区块）与 `.pane-preview .viewer3d{min-height:320px}` — :1086；预览缩略用 `--sv3d-height:176px` + `width:132px`（:788 区块）
- 卡片头：`.pane-head` row space-between gap 12px **`min-height:42px; margin-bottom:8px`** — :675-681；`.pane-title` 13px/700 — :682；`.pane-tools .icon-btn` **36×36**（覆盖全局 32×32）— :687-698
- 分段控件 `.seg-*`：容器 `gap:2px; height:36px; padding:3px; border:1px; border-radius:10px; background:var(--card-2)` — :814-821；`.seg-blob` absolute **`top:3px; bottom:3px`**，radius 6px，bg accent-grad，`left/width` 由 JS（:67）— :823-831；`.seg-btn` `height:100%; padding:0 16px`，12px — :833-849
- 皮肤列表 `.skin-grid`/`.cape-grid`：`repeat(auto-fill, minmax(124px,1fr))` gap 12px（:894 区块）→ 最终 `.cape-grid` **`repeat(2,minmax(0,1fr))`, gap:12px** — :1096；`.cape-item` **`grid-template-columns:58px minmax(0,1fr); grid-template-rows:auto auto; gap:8px 12px; min-height:100px; padding:12px`**，`.cape-preview` `grid-column:1; grid-row:1/4; width:58px; height:78px` — :1097-1099
- `.skin-actions` row wrap gap 12px margin-top 8px，`.btn{min-height:36px}` — :860-868；操作面板内 pane `padding:20px; border:0; background:transparent` — :1089；`.pane-empty` `min-height:60px; padding:16px` — :1091；历史空态 `padding:16px 20px`，`.pane-head{margin:0;flex:none}` — :1094-1095
- 拖拽反馈：`.row-main.drag-over` `box-shadow:0 0 0 3px var(--accent-soft)`（:660 区块）+ `.pane{outline:2px solid var(--accent)}` — :1100

## 7. CommunityView.vue

- `.page .community-page` flex column `max-width:940px` — :708-713 → 最终 **`.community-page{display:grid; grid-template-columns:minmax(0,1fr); gap:12px!important}`（单列）** — :1081
- 搜索卡 `.search-card` **`padding:16px; display:flex; flex-direction:column; gap:12px`** — :1080（基线 :717 column gap 12px）；`.search-row` flex gap 12px，input flex:1 — :722/:1080
- 分类分段 `.kind-capsules` `position:relative`，flex wrap，**`gap:3px; padding:3px; border-radius:999px; background:var(--card-2); width:fit-content`** — :734-744；`.capsule-blob` absolute radius 999px bg accent-grad（:1080 覆盖为 `background:var(--accent-soft)!important; box-shadow:none`），`left/top/width/height` 全部 JS inline — :746-753；`.capsule` **`height:calc(var(--ctl-h) - 6px)`=30px**，`padding:0 16px`，radius 999px，`border:0!important` — :755-767 / :1080
- 筛选/实例行 `.instance-row` `margin:0;padding:0; justify-content:flex-end`，`.instance-filter{max-width:420px}` — :1080；`@max-800` wrap + filter width 100% + search-card padding 12px — :1080
- 结果网格 **`.result-list` `grid-template-columns: repeat(auto-fit, minmax(min(100%,300px),1fr)); gap:16px`** — :1080（覆盖 :828 的 `auto-fill minmax(min(100%,260px),1fr)` gap 12px）
- 结果卡 `.result-card` **`padding:16px; border:1px; border-radius:var(--radius-lg)`=14px; box-shadow:none**，column gap 8px — :1080 + :833-840；`.result-icon` **46×46**，radius 10px — :851-857；`.result-title` 16px，2 行 clamp — :1080；`.result-desc` `line-height:1.6; min-height:3.2em`，2 行 clamp — :1080（基线 :890 `min-height:calc(var(--text-xs)*1.6*2)`）；`.result-links .icon-btn` **34×34** — :1080；`.result-foot` gap 12px `padding-top:8px; border-top:1px` — :900 区块/:1080；入场 `animation: community-card-in var(--motion-enter)` — :843
- 分页 `.pagination` flex wrap `justify-content:center`，gap 8px，`padding:16px 0 8px` — :824；按钮 `.btn.btn-sm` + active `.btn-gold` — :632
- 下载模态：`.download-modal` **`width:min(740px, calc(100vw - 40px)); max-height:88vh; overflow-y:auto`** — :963-967；`.file-list` **`max-height:280px`**，column gap 8px，padding `4px 4px 4px 0` — :994 区块；`.file-row` flex gap 12px `padding:12px 16px` radius 10px — :1002 区块；`.modal-title` 16px/700 `margin:0 0 12px` ellipsis — :977
- 浮层下拉：`.select-float` **`top:calc(100% + 4px)`**，`max-height:260px` — :798 区块（`position:absolute`）

## 8. BridgeView.vue

- `.page .bridge-page` `max-width:860px` — :289 → 最终 `.bridge-status` `max-width:960px`；状态卡改为 grid **`grid-template-columns:24px minmax(0,1fr)`，gap 16px，padding 24px** — :327 区块
- `.bridge-dot` 10×10 radius 50%，`margin-top:9px` — :292/:327；`.bridge-status-text strong` **21px** — :327；`.bridge-status-text` flex:1 `min-width:200px` — :301
- `.bridge-context` `grid-column:1/-1`，row space-between，`border-bottom:1px; padding-bottom:12px`，13px — :327
- 卡片 `.bridge-card` `padding:16px 20px` — :306；`.bridge-card-head` baseline row gap 12px，`padding-bottom:12px; border-bottom:1px`，strong 13px/700 — :308-310
- 参数行 `.bridge-row` flex align-center **`min-height:42px; padding:12px 0; gap:16px`**，`.bridge-row + .bridge-row{border-top:1px solid color-mix(...55%)}` — :311-312
- 控件宽度：`.bridge-slider` **`width:150px`**（原生 range，`accent-color`）— :321；`.bridge-slider-value` `min-width:44px; text-align:right`，12px — :322；`.bridge-text` **`width:200px`** — :323；`.bridge-select` **`width:160px`** — :324；`.bridge-reset` **26×26**，svg 13×13 — :327 区块/:331；`.bridge-label` 13px/600，`.bridge-desc` 12px — :316-317；tag `padding:1px 8px` — :318-319
- 无断点（除 ui-system.css 全局）

## 9. KeysView.vue

- 根 `.cfg-page` `width:100%; **max-width:1040px**; margin:0 auto` — :227
- 分类导航 `.cfg-sections` 基线 **`grid-template-columns:repeat(3,minmax(0,1fr))`**，gap 12px，margin-bottom 24px，button `padding:18px 20px; border-radius:14px; flex-direction:column` — :229 → 最终覆盖为 **`display:flex; gap:8px; flex-wrap:wrap; border-bottom:1px solid var(--border); padding-bottom:8px`**，button `flex-direction:row; **min-height:40px; padding:8px 14px; border:0; border-radius:6px; background:transparent**`，`.active{background:var(--accent-soft);color:var(--text)}`（无指示条，靠底色）— :273 区块；strong 14px / small 12px
- 列 `.cfg-col` flex column min-height 0 — :233；`.cfg-col-head` row space-between gap 12px wrap — :234/:273；`.cfg-head-actions` flex gap 16px wrap — :236
- 内容网格 `.cfg-scroll` **`min-height:220px; grid-template-columns:repeat(2,minmax(0,1fr)); gap:22px; padding-top:12px`** — :244；`@max-850`→`1fr` — :245
- 行 `.cfg-row` flex gap 12px `padding:8px` radius 6px — :249；default-packs 下 `.cfg-row{**min-height:60px**; gap:12px}`，`label{flex:1;min-width:0}` — :273；`.cfg-value` **`min-width:120px`**，padding `8px 12px`，radius 6px，12px — :254-256；`.cfg-reset` **26×26**，svg 13×13 — :262-267
- `.cfg-search` width 100% margin-bottom 12px — :243；`.cfg-cat` 13px `margin:0 0 8px`/650 — :248
- 清除按钮 `.cfg-clear` `position:relative; **z-index:9001**; **30×30**; border:1px; border-radius:6px; font-size:22px` — :270
- `.default-packs` `padding:20px`；`@max-760` `.cfg-row` wrap + label flex-basis 100% + small 隐藏 — :273；`@max-700` `.cfg-sections{gap:8px}` small 隐藏 — :230
- 页头：`.page-head/.page-title/.page-sub` 全继承（无本页覆盖）

## 10. RecordingsView.vue

- 根 **`.recordings-page`（不是 .page）** `max-width:1440px; display:grid; gap:16px`（基线 20px :91 → 覆盖 :92）`margin:0 auto`
- 头部 `.page-head.recording-heading` 改为 **row** space-between align-center gap 16px wrap — :92；`h1` **24px**，`small` 13px/400 dim（:92），基线 `.page-head h1{margin:0 0 8px}` — :91；`.actions` flex wrap **gap 8px**（:92 覆盖 :91 的 10px）
- 库路径行 `.recording-library` flex **gap 8px**，12px — :92；`.recording-help` `margin-left:auto`，`summary{cursor:pointer;white-space:nowrap}` — :92
- 工作区 `.recording-workspace` `padding:16px; display:grid; gap:12px; **min-height:240px**; align-content:start` — :92（`.card` 基线 padding 22px — :91）
- **筛选网格（最终）：`.recording-filters` `grid-template-columns: minmax(190px,1.5fr) minmax(160px,1fr) 130px 130px`; gap:12px** — :92（基线 :91 为 `minmax(200px,1fr) minmax(180px,1fr) 150px 150px`）；input `grid-column:1/grid-row:1`，首子 `grid-column:2` — :92；`@max-1150` → `1fr 1fr` + input 跨 `1/-1` — :92；`@max-800`→`1fr 1fr`(ui-system.css:61 命中 `.recording-filters`)；`@max-650`→`display:flex;column`，workspace padding 12px — :92
- 控件区 `.recording-controls` `grid gap 8px`，`padding:8px 0`，`border-bottom:1px` — :92（基线 gap 16px :91）；`.operation-row :deep(.select-menu-btn)` `max-width:360px` — :91
- 行 `.recording-row` flex align-center **`min-height:72px; padding:12px 0; gap:16px; border-bottom:1px solid var(--border)`**（基线 :91 padding `16px 0`，`:92` 收紧），`:last-child{border:0}`；`.recording-info` **`flex:1; min-width:0; display:grid; gap:6px`**，`[draggable="true"]{cursor:grab}` — :91/:92；`.path` 12px，`.recording-info .path` ellipsis — :91
- 结果折叠 `.recording-results` **`max-height:300px; overflow:auto`**，`padding:12px 16px` — :91/:92；空态 `.empty` `text-align:center; padding:40px`（:91）→ `.recording-workspace>.empty{background:transparent;border:0;**min-height:220px**;box-shadow:none}` — :92；`.pagination{justify-content:center}` — :91
- 无自定义模态 → 继承通用 460px 配方

## 11. FriendConnectView.vue

- `.page.friend-connect-page` `max-width:1120px`（:111）→ **最终 `max-width:1200px; container-type:inline-size`** — :160 区块
- 头部 `.header-copy` flex:1 min-width 0；`.header-side` flex **`margin-left:auto; flex:none; white-space:nowrap`**，gap 8px — :113-114；`@max-560` 换行规则 — :115
- `.pick-head` `padding:8px 0 12px`（:122）→ 最终 **`padding:0`**，`h1` **24px**（:160 覆盖 :123 的 `--text-2xl`=26px/700/`-0.5px`），`p` 12px line-height 1.7 — :124
- **入口卡列表（最终）：`.pick-list` `display:grid; grid-template-columns:minmax(0,1fr); gap:16px`** — :160（基线 :125 为 flex column gap 28px）；**`@container (min-width:960px)` → `grid-template-columns:repeat(3,minmax(0,1fr))`，card 转 column gap 14px，`.pick-name{display:grid;gap:8px}`，`.pick-go{align-self:flex-end}`** — :160
- 卡片 `.pick-card` flex row align-start **`gap:24px; padding:20px; border-radius:var(--radius-lg)`=14px，border `1px solid var(--border-strong)`**，`box-shadow:none`，animation none — :160 + :126-129
- `.pick-icon` **42×42**，`margin-top:2px`，radius 10px，`display:grid;place-items:center`，内 svg **22×22** — :147-152
- `.pick-copy` column gap 8px flex:1 — :153（container 下 gap 12px）；`.pick-name` **18px**/700（:160 覆盖 :154 的 16px），`em` 徽标 `padding:2px 8px; border-radius:999px; border:1px solid var(--border-strong)`，12px/500 — :155；`.pick-desc` **14px** line-height 1.65 — :160（覆盖 :156 的 12px/1.8）；`.pick-scene` 12px accent-2 — :157；`.pick-go` `align-self:center; flex:none; opacity:0.4; transform:translateX(-4px)` → hover 显现 — :159
- `.method-header` `margin-bottom:16px`，p 13px `max-width:800px` — :160
- 表单/面板继承 connection.css（`.connection-field` 14px — ui-system.css:34）

## 12. connection/*.vue（Servers / Bridge / FriendConnect 共用的组件层）

全部 geometry 在 connection.css（**无 scoped，页面靠 `.servers-page …` 后代选择器覆盖**）：

- `.connection-panel` radius `--radius-lg`，border 1px，`background: color-mix(in srgb, var(--card) 94%, var(--bg))` + `blur(16px) saturate(85%)` — connection.css:15 + ui-system.css:27-28；`box-shadow:none` — ui-system.css:78
- head/body：基线 `padding:20px` — connection.css:16/:20 → 最终 **head `12px 20px`（h2 16px）/ body `20px`，gap 16px** — connection.css:114；`@max-700`→`14px` — :115；`@max-820`→`16px` — :109
- `.connection-step`/`.connection-symbol` **38×38** radius 10px bg accent-soft；symbol **48×48** + border，svg 26×26 — connection.css:21-23
- `.connection-status` pill `padding:4px 12px`，radius 999px，12px/600，`i` **6×6** dot — connection.css:24-25
- **toggle 配方（与全局 .switch 不同）**：`.connection-toggle` `min-height:42px; padding:12px 16px; border:1px; border-radius:10px`，`position:relative`；`.connection-toggle-track` **38×24**，`padding:3px`，radius 999px；`::after` thumb **16×16**，checked `translateX(14px)`；隐藏 input `position:absolute; **right:14px**; 38×24` — connection.css:54-63
- `.connection-notice` `border-left:3px solid var(--accent)`，padding 16px，svg **19×19** `margin-top:2px` — connection.css:46-47
- `.connection-details` padding `12px 16px` radius 10px（→ ui-system.css:77 改为 `padding:12px 0; border-width:0 0 1px; background:transparent`）；`summary` `min-height:36px`，13px — connection.css:42 + :114
- `.connect-tabs` flex wrap gap 8px；`.connect-tab` **`min-height:36px; padding:8px 16px; border-radius:10px; border:1px solid var(--border-strong)`**，12px/600，`.active` bg accent-soft — connection.css:76-79（胶囊式，无指示条）
- `.connection-columns` `repeat(auto-fit, minmax(300px,1fr))` gap 16px（→ ui-system.css:35 `minmax(min(100%,300px),1fr)`；`@max-820`→1 列 connection.css:102）
- `.connection-log-viewport` **`max-height:180px`→220px**，padding `12px 16px`，radius 10px，mono 12px line-height 1.7 — connection.css:82-98 + :114
- `.network-metrics` **`grid-template-columns: repeat(4, minmax(0,1fr))`**，`> div` `min-height:42px; padding:16px 20px`，`+ div{border-left:1px}`，strong 14px — connection.css:34-39；`@max-820`→`repeat(2,...)` + 第 3 子去左边框加顶边框 — connection.css:105-108
- `.connection-result` padding 16px radius 10px 12px/1.7 — connection.css:64；`.connection-error` border 1px danger，padding `12px 16px` — connection.css:67
- `.connection-modal .modal` radius 14px，`.modal-label` `margin:16px 0 8px` — connection.css:71-72
- `.connect-page .btn` `min-height:36px; min-width:42px; padding:8px 16px; font-size:12px`→最终 13px — connection.css:11 + :114

## 13. 组件层（被多页复用）

- **InstanceCenter.vue**（GameView「管理」弹窗，1120×860 全屏级）：
  `.ic-mask` fixed inset:0，z-index 9500，`background:#0009`，`blur(8px)`，**padding 28px**，grid place-items center；
  `.ic` **`width:min(1120px,100%); height:min(860px,100%)`**，radius **24px**，flex column；
  `.ic-header` `padding:26px 30px 18px; gap:24px`，h2 **26px**；
  `.ic-tabs` `padding:0 30px 16px; border-bottom:1px; gap:6px`，button `padding:10px 24px; border-radius:12px`，`.active` bg accent-soft/700；
  `.ic-content` `padding:24px 30px; overflow:auto`；`.ic-grid` `repeat(2,minmax(0,1fr))` gap 14px；
  `.ic-card` padding 20px radius **16px**；`.ic-row` `padding:18px 0; gap:16px; border-bottom:1px`，图标 **58×58** radius 10px，`.ic-world-icon` font 28px；
  `.ic-shots` `repeat(3,minmax(0,1fr))` gap 16px，`img{aspect-ratio:16/9}`；`.ic-tools` gap 10px，`.input{flex:1;min-width:180px}`；`.ic-empty` `padding:56px 12px`；
  `.ic-dialog-mask` **absolute** inset:0 z-index 2，padding 24px；`.ic-dialog` **`width:min(560px,100%)`**，padding 24px，radius 18px，`label{margin:18px 0}`；
  `.ic-lightbox` fixed inset:0 z-index 10000，padding 24px；
  `@max-800` → mask padding 10px，header/content padding 18px，tabs `0 14px 14px`/button `10px 15px`，shots 2 列，h2 21px — InstanceCenter.vue:109
- **SelectMenu.vue**：`.select-menu-btn` `min-height:36px; padding:8px 12px; radius:10px; 13px`，`.open` `box-shadow:0 0 0 3px accent-soft`，chevron 14×14 rotate 180 — :109-130；
  `.select-menu-float` **`position:fixed; z-index:11000`**，`gap:4px; padding:8px; radius:10px`，`max-width:calc(100vw - 24px)` — :131-136/:163；
  `.select-menu-option` **`min-height:42px; padding:4px 12px; radius:6px`**，13px，check svg 14×14 — :144-160；`.select-menu-empty` `min-height:42px; padding:12px` — :161；label/option label 双行 `display:grid;gap:3px`，small 12px — :163
- **MinecraftVersionPicker.vue**：`.version-picker-popup` **`position:fixed; z-index:12000`**，radius **14px**，`max-height:calc(100vh - 24px)`，`overflow:hidden`，flex column — :26；
  `.version-picker-top` `padding:12px; border-bottom:1px`；`.version-categories` `gap:4px; margin-top:10px`，button `padding:6px 8px; **border-radius:7px**; 12px` — :26；
  `.version-picker-list` `padding:6px; flex:1`；`.version-picker-option` **`min-height:52px; padding:8px 12px; gap:12px; border-radius:8px`**，b `width:12px`，small/time 11px — :26；footer `padding:9px 14px; 11px`；`.version-picker-manual` `padding:10px; gap:8px`
- **SkinViewer3D.vue**：`.viewer3d` `width:100%; height:var(--sv3d-height,340px)`，radius 10px，`cursor:grab`/`.dragging grabbing`，`touch-action:none`；`.viewer3d-fallback` absolute inset:0 — :124
- **UpdateModal.vue**：`.upd-title` **22px**/1.4 — :101；`.upd-progress` **height 8px，radius 99px，margin `14px 0`**；`.upd-download-status strong` **28px**；`.upd-slow` `padding:12px 14px; margin:0 0 18px; border-radius:10px`；`.upd-actions` row space-between gap 12px wrap，`.upd-actions-right` gap 10px `margin-left:auto`；`.upd-help` `margin-top:24px; padding-top:16px; border-top:1px`；`.upd-body` 14px/1.8，`:deep(ul){padding-left:20px}` — :101
- **ConfirmModal.vue**：**未找到自有几何**（唯一规则：正文 `font-size:var(--text-md); line-height:1.7` — :36-38），宽度/圆角/padding 全继承通用 `.modal`（styles.css:545-555）
- **EditPanel.vue（个性化，全屏设计器）**：`.design-workspace` fixed inset:0 z-index 100000，**`grid-template-columns:230px minmax(0,1fr) 310px; grid-template-rows:66px minmax(0,1fr)`**；
  `.designer-toolbar` `grid-column:1/-1; padding:10px 16px; gap:8px`；`.layer-row` `min-height:36px; border-radius:7px`，`.fold` 24px；
  `.designer-viewport` `background-size:16px 16px`；`.preview-scroll` absolute inset:0 padding 20px；`.canvas-caption` absolute `bottom:4px; left:12px`；
  `.property-tabs` `gap:6px; border-bottom:1px; padding-bottom:8px`；`.designer-grid` `1fr 1fr` gap 12px；控件统一 `border-radius:8px; padding:7px 9px`；`input[type=color]` **32×32 padding 2px**；`.alpha-row small` `min-width:34px`；
  `.designer-outline` absolute-ish fixed，`border:2px`，label `top:-22px`，resize handle **12×12 `bottom:-6px; right:-6px`，border 2px white**；
  导引线 `.vertical{width:1px;top:66px;bottom:0}` / `.horizontal{height:1px;left:230px;right:310px}`；
  `.designer-dialog` `width:min(570px, calc(100vw - 48px)); padding:24px; border-radius:18px`；
  `@max-1150` → **`minmax(0,1fr) 290px`**，layers 面板收进 mobile tab，`.horizontal{left:0;right:290px}` — :74

## 14. 跨页复用清单（3+ 页面共享 → 做成一个 WPF Style）

| 共享选择器 | 使用页数 | 定义处 | 页面 |
|---|---|---|---|
| `.page`（列流 + `gap:16px` + `margin:0 auto`）| 7 | styles.css:498 / ui-system.css:10 | GameView:1510、Settings:1308、Accounts:672、Community:708、Bridge:289、Skins:609、FriendConnect:111 |
| `.page-head` / `.page-title` / `.page-sub` | 8 / 7 / 6 | styles.css:498-512 + ui-system.css:11-12 | 同上加 KeysView；**唯一例外** RecordingsView（h1 自绘 24px — :92）|
| `.card`（padding 20px / radius 14px / border 1px）| 10 | styles.css:450-457 + ui-system.css:13 | 除 ServersView、FriendConnectView 外全部 |
| `.btn` `.btn-ghost` `.btn-gold` `.btn-danger` | 9（含 `.btn-sm`）| styles.css:158-263 + ui-system.css:16-17 | Accounts/Bridge/Community/Game/Keys/Recordings/Settings/Skins/Home |
| `.icon-btn`（32×32）| 3 直接用 + Community 改 34 / Skins 改 36 | styles.css:266-301 | Community:1080、Skins:687-698、Settings |
| `.input` `.select`（min-height 36 / padding 8×12 / radius 10）| 9 | styles.css:304-345 + ui-system.css:15 | 全域；connection.css:13 另覆盖为 13px |
| `.switch` `.switch-ui`（40×22，thumb 14，left 3px，checked 18px）| 4 | styles.css:356-411 | Bridge、GameView、Keys、Settings |
| `.slider`（track 6px，thumb 16×16）| 2 | styles.css:414-447 | Settings(另有自绘 mem-slider)、Bridge(原生 range + accent-color，`width:150px`) |
| `.tag` + `tag-gold/cyan/danger/success/accent` | 6 | styles.css:459-495 | Accounts、Community、GameView、Settings、Skins、Bridge |
| `.modal-mask` / `.modal` / `.modal-actions` | 6 | styles.css:527-563 + ui-system.css:37-40 | Accounts、Community、GameView、Settings、Servers、Home（ConfirmModal 完全依赖它）|
| `.modal-title` / `.modal-label` | 3（各页重复定义同值）| Accounts:930、GameView:2005/:2010、Community:977/:1000 + connection.css:72 | ⚠ 同一规则被复制 4 份，合并成 1 个 WPF 样式 |
| `.empty` | 10 | styles.css:515-524 + ui-system.css:44 | 全域 |
| `.float-menu` / `.menu-item` / `.menu-overlay` | 3 | styles.css:577-645 | GameView(:2202 扩展)、Community、Home |
| `.toolbar`（flex wrap gap 12px，flex 1）| 3 | ui-system.css:19-20 + GameView:1629 | GameView、Bridge(template)、Servers(:62 `.server-toolbar`) |
| `.row-actions` | 1（同配方被 3 页各写一次）| GameView:1937 | Servers `.server-row-state`、FileManager `.fm-remove` 区块 — 建议统一 |
| `.select-menu-btn` / `-float` / `-option` | 3+（组件层）| SelectMenu.vue:109-163 + ui-system.css:15 | GameView(:2181 改宽)、Settings、FileManager、Recordings(:91) |
| `.status-strip` | 3 | ui-system.css:41-42 | Community、Recordings、Skins |
| `.connection-panel/-head/-body` + `.connection-header` + `.connect-page` | 3 | connection.css:3/:6/:15-20/:114 | ServersView、FriendConnectView、BridgeView |
| `.connection-toggle`、`.connection-field`、`.connection-result`、`.connection-empty`、`.connect-tabs` | 3 | connection.css:52-79 | 同上 |
| `.fm-table-head` + `.fm-row` 同列宽 | 3 页 | FileManager.vue:719-724 | ModsView、PacksView、ShadersView |
| 胶囊分段（pill tabs，`height:calc(--ctl_h-6px)`/36px + 999px + JS blob）| 2 结构同型 | GameView:1870-1910、Community:734-767、Skins:814-849 | ⚠ **三页是同一模式的三次复制**（blob absolute top/bottom 内缩 3~4px，left/width 由 JS）→ 抽 1 个 `PillTabBar` 控件 |

三处必须区分而非合并的"看起来一样"：
1. `.account-type-tabs`（grid 3 等分 + 实心 accent-grad，无 blob）— Accounts:717
2. `.settings-scopes` / `.settings-categories`（纯 `.btn` + 底色，无边框无指示器）— Settings:1839-1842
3. `.ic-tabs` / `.cfg-sections`（padding 10×24 / 8×14 文字胶囊）— InstanceCenter:109、Keys:273

明确记录为"未找到"的项：HomeView 无 `.page{}` 自有声明；ModsView/PacksView/ShadersView 无任何样式块；ServersView.vue 使用非 scoped `<style>`（:448）；ConfirmModal.vue 无布局几何；CommunityView 的 `.capsule-blob` 无静态 left/width（纯 JS inline）；GameView `.folder-manager` 的 `grid-template-columns` 基线声明(:1534)被最终声明(:2173)完全取代，WPF 复刻只取 :2173。
