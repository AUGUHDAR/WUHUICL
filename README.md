<div align="center">

<img src=".github/assets/banner.jpg" alt="WUHUICL" width="100%" />

# WUHUICL

**为「玩家高频使用」而做的 Minecraft 启动器** · 基于 [KAMUCL](https://github.com/kamubaba-i/KAMUCL) 的社区版移植 · Rust + Tauri 2 重写 · 单文件便携，双击即用

[![license](https://img.shields.io/badge/license-MIT%20%2B%20LGPL--3.0-green)](LICENSE)
[![release](https://img.shields.io/github/v/release/AUGUHDAR/WUHUICL)](https://github.com/AUGUHDAR/WUHUICL/releases)
[![downloads](https://img.shields.io/github/downloads/AUGUHDAR/WUHUICL/total)](https://github.com/AUGUHDAR/WUHUICL/releases)
[![platform](https://img.shields.io/badge/platform-Windows%2010%20%2F%2011-blue)](#怎么用)

[下载](#下载) · [现在到什么程度](#现在到什么程度) · [常见问题](#常见问题) · [反馈与贡献](#反馈与贡献)

</div>

---

## ⚠️ 写在前面（作者原话，未做润色）

> 由于卡慕的技术债我还没有清干净，目前仓库内容不代表未来成品！
>
> Since I haven't fully cleared Camu's technical debt yet, the current repo content doesn't represent the final product!
>
> 因为现在处于初步阶段，也就是完整移植，导致原版的一些缺陷和法律问题也带过来了。你们先别急，我比你们更急
>
> Because we're still in the early stages, meaning a full port, some of the original version's flaws and legal issues have also come over. Don't worry, I'm even more anxious than you are.
>
> 另外，本启动器的 Client ID 确实拿到，但是 Mojang 申请还没过，所以导致目前的正版验证还过不了
>
> Also, the launcher's Client ID has indeed been obtained, but the Mojang application hasn't been approved yet, so the official verification still doesn't work right now.
>
> 还有，本项目为了赶开发进度，大量使用了 AI。如果你对 AI 反感，请移步其他启动器，但也不保证其他启动器未使用 AI
>
> Also, this project has made heavy use of AI in order to speed up development. If you're not into AI, please check out other launchers, though there's no guarantee that they haven't used AI either.

正版登录那一步的最新进展：**2026-10-14 收到 Mojang 的批次审核结果，本批未通过白名单**（同一批里有违规应用，我们一起被刷了）。启动器这边的微软登录链路代码是完整的，卡在最后一步的接口授权。详见 [正版登录现状](#正版登录现在什么状态)。

---

## 目录

- [界面长什么样](#界面长什么样)
- [下载](#下载)
- [怎么用](#怎么用)
- [现在到什么程度](#现在到什么程度)
- [有什么功能](#有什么功能)
- [正版登录现在什么状态](#正版登录现在什么状态)
- [常见问题](#常见问题)
- [反馈与贡献](#反馈与贡献)
- [接下来做什么](#接下来做什么)
- [它是怎么做的](#它是怎么做的)
- [目录结构](#目录结构)
- [许可与致谢](#许可与致谢)

---

## 界面长什么样

首页：当前实例、运行环境、内存分配、运行状态、最近游戏，右边是账户与皮肤预览。

<img src=".github/assets/home.jpg" alt="首页" width="100%" />

模组管理：按实例隔离，中文名对照、开关、更新检测、直接打开文件夹。

<img src=".github/assets/mods.jpg" alt="模组管理" width="100%" />

联机入口：FRP 内网穿透、VoxLink 房间码、陶瓦联机三条路，各自写清楚适合谁。

<img src=".github/assets/online.jpg" alt="联机" width="100%" />

界面是照原版逐屏 1:1 还原的，浅色主题、亚克力标题栏、首页卡片可自由拖排，另有一个会走路的桌宠。

---

## 下载

| 渠道 | 地址 | 说明 |
| --- | --- | --- |
| GitHub | <https://github.com/AUGUHDAR/WUHUICL/releases> | 主渠道，Release 里附 SHA-256 |
| Gitee（国内镜像） | <https://gitee.com/AUGUHDAR/WUHUICL> | 国内访问快；文件与 GitHub 一致 |
| QQ 群 | `1045554684` | 群文件同样有最新包，下载慢时可以来这里 |

拿到文件后建议先校验一下，尤其是从镜像或群文件拿的：

```powershell
Get-FileHash .\WUHUICL-0.1.0.exe -Algorithm SHA256
```

---

## 怎么用

下载 `WUHUICL-0.1.0.exe`，双击就行。没有安装包，不用管理员权限，也没有"卸载"这回事；你的设置和游戏目录都在你自己放的地方。

第一次运行有两点要知道：

- **需要 WebView2 运行时。** Windows 11 自带；Windows 10 大多数机器通过系统更新已经有了。真的没有，启动器会告诉你装什么。
- **没有代码签名。** SmartScreen 可能弹"Windows 已保护你的电脑"，点*更多信息 → 仍要执行*即可。

数据放在两个地方，都和原版（`.kamucl`）分开，两个启动器可以并存、互不覆盖：

```
%APPDATA%\com.wuhuicl.app     设置、账号（令牌用 Windows DPAPI 加密）、更新状态
%APPDATA%\.wuhuicl            默认游戏目录
```

---

## 现在到什么程度

早期 alpha。说人话版本：

| 状态 | 内容 |
| --- | --- |
| ✅ 日常可用 | 实例中心与版本继承链解析、Modrinth / CurseForge 浏览与断点续传下载、资源包 / 光影 / 存档导入、3D 皮肤编辑器、录像与存档投影、服务器列表、默认游戏设置与按键同步、MOD 实时配置面板、主题与首页自由排布、桌宠、离线账号、外置登录 |
| 🟡 代码到位，卡在外部授权 | 微软正版登录（见[上一节](#正版登录现在什么状态)） |
| 🔧 进行中，不会假装能用 | **联机（VoxLink P2P）**：引擎模块已逐块移植并通过测试，还没接到界面上。面板能看能点，点连接会明确告诉你"引擎未移植"。<br>**自更新**：检查更新是通的，"下载并替换自身"那段还在移植，入口同样明确报未移植。 |
| ⚪ 还在对表 | 有些角落仍在跟原版逐条比对，所以会有毛刺。发现一处报一处，比默不作声强。 |

---

## 有什么功能

- **实例中心**：一个目录一个实例，版本继承链（Forge / Fabric / Quilt / 独立客户端）解析与原版一致。
- **模组与资源**：Modrinth、CurseForge 双源浏览，支持中文名检索；下载队列可断点续传、多源测速择优。
- **MOD 实时配置面板**：游戏里改的配置，启动器里也能看到改（内置桥接 jar）。
- **皮肤**：3D 皮肤编辑器，可绘制、预览、上传（正版账号或外置账号）。
- **录像与投影**：把存档的游玩过程录下来，或者把存档渲染成可旋转的三维投影。
- **服务器列表**：本地维护，显示版本与延迟。
- **默认配置**：视频/按键/游戏设置一处统一配，新建实例自动继承。
- **联机**：FRP 内网穿透、VoxLink 房间码（免公网 IP）、陶瓦（Terracotta）三条路。
- **桌面体验**：主题、可拖排的首页、会走路的桌宠。

---

## 正版登录现在什么状态

一句话：**启动器这边能做的都做完了，卡在 Mojang 那边的应用白名单。**

- 微软设备码登录 → Xbox Live → XSTS → Minecraft 这条链路的代码已按原版逐条移植，你自己的微软账号能正常走完前几步。
- 最后一步 `login_with_xbox` 需要这个应用在 Mojang 的允许列表里。**2026-10-14 的批次审核结果：未通过**（同批存在违规应用，本批整体收紧），所以现在这一步会被拒。
- 官方邮件明确写了他们只处理 appId 加白、不提供接口排障，所以我们只能重新申请、等下一批。

**这段时间能用什么：**

- **离线账号**：现在就绪，能进游戏、能玩离线服务器。
- **外置登录（authlib-injector / Yggdrasil）**：探测、登录、选角、运行时注入这条链路已移植；能不能登上取决于你所用服务器本身。
- **陶瓦 / FRP / 房间码联机**：不依赖正版登录的部分照常。

白名单批下来我会在 Release 说明和群里同步，不用你反复来翻这段。

---

## 常见问题

**为什么是一个 exe，不是安装包？**
因为做这个移植的初衷就是"少占地方、少留垃圾"。安装包意味着写注册表、往 Program Files 里塞东西、卸载还要清一遍。双击即用、拖走就走，是这个项目的基本形态，不会改。

**比原版省多少内存？**
这正是做移植的原因，但不是拍脑袋的数字：每一版都要跟原版并排跑、逐项记录才敢写。所以这里不放一个"省了 X%"的口号，你手上两个都有，开任务管理器对比最准。

**会不会动我原来的 KAMUCL 或者存档？**
不会。配置和游戏目录都是自己的（`.wuhuicl` / `com.wuhuicl.app`），原版那份（`.kamucl`）不读不写。存档类操作全部要求二次确认，删除走回收而不是直接抹。

**和 KAMUCL 是什么关系？**
社区版移植，不是官方续作。界面逐屏 1:1、IPC 通道名与返回形状与原版一致——这也是这个移植的验证方式：两边同时跑，调同一条通道，比对结果。原版作者与项目见[致谢](#许可与致谢)。

**为什么联机点不动？**
引擎的地基（RUDP 可靠传输、打洞策略、标准 TURN、TCP 同时开、模组同步）已经一块块移植并测试过，但接到界面上的那一步还没做。现在点连接会明确返回"引擎未移植"——它不会假装连上了然后让你干等。

**代码是 AI 写的吗？**
是，大量代码由 AI 辅助生成，作者原话就在[最上面](#️-写在前面作者原话未做润色)。约束是：每一处改动落地前都要跟原版对表，测试跑不过就不进树。你完全可以自己读，代码写得很朴素。

**我要怎么帮你？**
提 issue（带日志）、提 PR、转发给需要的人、或者来群里骂一句哪里难用。见[反馈与贡献](#反馈与贡献)。

---

## 反馈与贡献

- **提 BUG / 提想法**：<https://github.com/AUGUHDAR/WUHUICL/issues>（新建时会看到模板，照着填就行）
- **日志在这里**：

```
游戏目录\versions\你的实例\logs\latest.log
%APPDATA%\com.wuhuicl.app\logs\
```

- **B 站**：<https://space.bilibili.com/526277131> · **QQ 群**：`1045554684`
- **想改代码**：先看 [CONTRIBUTING.md](.github/CONTRIBUTING.md)，里面写了构建环境、代码风格、以及这个项目"改动必须与原版对表"的验收方式。小改动直接提 PR 就行，不用先问。
- 安全问题请按 [SECURITY.md](.github/SECURITY.md) 里的方式报，别直接开公开 issue。

---

## 接下来做什么

按当前顺序，不承诺时间：

1. 把已移植的联机引擎接到界面上（`voxlink:start` 与工单 / 模组同步那批通道）。
2. 更新器本体 + GitHub / Gitee 双源下载。
3. 继续逐屏与原版对表，把剩下的毛刺收掉。
4. 重新申请 Mojang 白名单，等正版登录这条路通了就补上。

---

## 它是怎么做的

后端 Rust，前端 Vue 3，一个 WebView2 窗口配 DWM 亚克力背景。渲染层要的一切都走同一条 IPC 面（`window.kamucl`），通道名、参数、返回形状、错误文案逐条对齐原版的 Electron 合同。

下载走"候选列表"——原址优先，然后镜像，然后你自己加的镜像前缀——每个源单独测速，所以同一个 Release 从 GitHub 或 Gitee 拉都一样，不用改设置。

纯 Rust，除了你主动要的（Java、陶瓦）之外没有常驻子进程。令牌用 Windows DPAPI 绑定当前用户加密后落盘。

---

## 目录结构

| 目录 | 内容 |
| --- | --- |
| `src/` | Vue 3 渲染层：页面、组件、组合式函数 |
| `shared/` | 渲染层与后端共用的类型与常量 |
| `src-tauri/src/` | Rust 后端：一个模块对应原版 `main/core` 一个文件 |
| `src-tauri/src/vl/` | VoxLink P2P 引擎移植（进行中，尚未接线） |
| `src-tauri/icons/` | 应用图标 |
| `src-tauri/resources/` | 内置的 MOD 桥接 jar |
| `.github/` | 仓库门面：横幅、截图、贡献与安全说明、issue / PR 模板 |

想从源码构建，看 [CONTRIBUTING.md](.github/CONTRIBUTING.md)（Windows + Rust MSVC 工具链 + Node，一条命令出自带 exe）。

---

## 许可与致谢

本仓库按 MIT 许可发布（见 [LICENSE](LICENSE)）。从 [KAMUCL](https://github.com/kamubaba-i/KAMUCL) 与 [VoxLink](https://github.com/AUGUHDAR/VoxLink) 改写来的文件保留原有的 `SPDX-License-Identifier: LGPL-3.0-only` 声明，这部分仍按 LGPL-3.0 走——两处不一致时，以对这些文件更严格的条款为准。第三方组件及其许可见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。

- 感谢 [KAMUCL](https://github.com/kamubaba-i/KAMUCL) 原作者与社区，本项目的界面与行为合同来自它。
- 生物图标属于 Minecraft 游戏资源，版权归 Mojang / Microsoft。
- 陶瓦（Terracotta）由 [burningtnt](https://github.com/burningtnt/Terracotta) 开发，遵循 AGPL-3.0，特此致谢；其二进制只从官方 Release 下载并做 SHA-256 校验。
- 感谢每一位提 issue、提 PR、在群里耐心复现的朋友。

---
---

<div align="center">

<img src=".github/assets/banner.jpg" alt="WUHUICL" width="100%" />

# WUHUICL

**A Minecraft launcher built for *heavy daily use*** · a community port of [KAMUCL](https://github.com/kamubaba-i/KAMUCL), rewritten in Rust + Tauri 2 · one portable file, double-click and go

[![license](https://img.shields.io/badge/license-MIT%20%2B%20LGPL--3.0-green)](LICENSE)
[![release](https://img.shields.io/github/v/release/AUGUHDAR/WUHUICL)](https://github.com/AUGUHDAR/WUHUICL/releases)
[![downloads](https://img.shields.io/github/downloads/AUGUHDAR/WUHUICL/total)](https://github.com/AUGUHDAR/WUHUICL/releases)
[![platform](https://img.shields.io/badge/platform-Windows%2010%20%2F%2011-blue)](#how-to-run)

[Download](#download) · [Where it stands](#where-it-stands) · [FAQ](#faq) · [Feedback & contributing](#feedback--contributing)

</div>

---

## ⚠️ Before you start (author's own words, unedited)

> Since I haven't fully cleared Camu's technical debt yet, the current repo content doesn't represent the final product!
>
> Because we're still in the early stages, meaning a full port, some of the original version's flaws and legal issues have also come over. Don't worry, I'm even more anxious than you are.
>
> Also, the launcher's Client ID has indeed been obtained, but the Mojang application hasn't been approved yet, so the official verification still doesn't work right now.
>
> Also, this project has made heavy use of AI in order to speed up development. If you're not into AI, please check out other launchers, though there's no guarantee that they haven't used AI either.

Latest on official login: **the 2026-10-14 Mojang batch review came back — not approved** (bad apps were in the same batch and the whole batch got tightened). The launcher side of the Microsoft chain is complete; it stops at API authorization. See [Official login status](#official-login-status).

---

## Table of contents

- [Screenshots](#screenshots)
- [Download](#download)
- [How to run](#how-to-run)
- [Where it stands](#where-it-stands)
- [Features](#features)
- [Official login status](#official-login-status)
- [FAQ](#faq)
- [Feedback & contributing](#feedback--contributing)
- [What's next](#whats-next)
- [How it works](#how-it-works)
- [Repository layout](#repository-layout)
- [License & credits](#license--credits)

---

## Screenshots

Home: current instance, runtime, memory allocation, status, recent games, plus accounts and skin preview on the right.

<img src=".github/assets/home.jpg" alt="Home" width="100%" />

Mods: per-instance isolation, Chinese name aliases, toggles, update check, open-folder.

<img src=".github/assets/mods.jpg" alt="Mods" width="100%" />

Multiplayer: three routes — FRP tunnel, VoxLink room code (no public IP), Terracotta — each with who it's for.

<img src=".github/assets/online.jpg" alt="Multiplayer" width="100%" />

The UI is a screen-by-screen 1:1 port of the original: light theme, acrylic title bar, a fully re-arrangeable home page, and a desktop mascot that walks around.

---

## Download

| Channel | Address | Notes |
| --- | --- | --- |
| GitHub | <https://github.com/AUGUHDAR/WUHUICL/releases> | Primary; releases publish a SHA-256 |
| Gitee (CN mirror) | <https://gitee.com/AUGUHDAR/WUHUICL> | Faster in mainland China, identical files |
| QQ group | `1045554684` | Group files carry the same builds |

Verify, especially if you got the file from the mirror or the group:

```powershell
Get-FileHash .\WUHUICL-0.1.0.exe -Algorithm SHA256
```

---

## How to run

Grab `WUHUICL-0.1.0.exe`, double-click it. That's it — no installer, no admin rights, nothing to uninstall. Your settings and your Minecraft folder stay where you put them.

Two things to know on first launch:

- **WebView2 Runtime** is required. Windows 11 ships with it; on Windows 10 most machines already have it through Windows Update. If it's missing, the app will tell you what to install.
- **The build is unsigned.** SmartScreen may say "Windows protected your PC" — click *More info → Run anyway*.

Data lives in two places, both separate from KAMUCL (`.kamucl`), so the two launchers can sit side by side without clobbering each other:

```
%APPDATA%\com.wuhuicl.app     settings, accounts (tokens encrypted with Windows DPAPI), update state
%APPDATA%\.wuhuicl            default game directory
```

---

## Where it stands

Early alpha. In plain words:

| Status | What |
| --- | --- |
| ✅ Usable day to day | Instance center with inheritance-chain version resolution, Modrinth / CurseForge browsing with resumable multi-source downloads, resource pack / shader / world import, 3D skin editor, recordings and save projection, server list, default options and keybinding sync, live MOD config panel, themes and a re-arrangeable home page, desktop mascot, offline accounts, external (Yggdrasil) auth |
| 🟡 Code done, blocked externally | Microsoft official login (see [below](#official-login-status)) |
| 🔧 In progress, never faked | **Multiplayer (VoxLink P2P)**: engine modules are ported and tested, not wired to the UI yet. The panel renders; pressing connect tells you the engine isn't ported — it will not pretend. <br>**Self-update**: checking works; download-and-replace is still being ported and says so. |
| ⚪ Still being cross-checked | Some corners are still being verified against the original, so expect rough edges. |

---

## Features

- **Instance center** — one folder per instance; Forge / Fabric / Quilt / vanilla inheritance resolved exactly like the original.
- **Mods and resources** — Modrinth and CurseForge browsing with Chinese-name search; resumable download queue that probes each source for speed.
- **Live MOD config panel** — options a MOD exposes in-game are editable from the launcher (bundled bridge jar).
- **Skins** — 3D skin editor: draw, preview, upload (official or external account).
- **Recordings and projection** — record a playthrough of a save, or render a save as a rotatable 3D projection.
- **Server list** — local, with version and latency.
- **Default options** — video / controls / gameplay set once, inherited by new instances.
- **Multiplayer** — FRP tunnel, VoxLink room codes (no public IP needed), Terracotta.
- **Desktop** — themes, drag-to-arrange home page, a walking mascot.

---

## Official login status

In one sentence: **everything the launcher can do is done; Mojang's app allow list is the blocker.**

- The Microsoft device-code chain → Xbox Live → XSTS → Minecraft is ported line by line from the original; a normal Microsoft account gets through the first steps.
- The last call, `login_with_xbox`, requires this app to be on Mojang's allow list. **The 2026-10-14 batch was rejected** (violating apps were in the same batch, so the whole batch was tightened), so this step currently fails.
- Mojang's reply states they only handle appId allow-list requests and don't troubleshoot API access, so all we can do is re-apply and wait for the next batch.

**What works meanwhile:**

- **Offline accounts** — ready now; join the game and offline servers.
- **External auth (authlib-injector / Yggdrasil)** — probe, login, profile selection and runtime injection are ported; whether you get in depends on the server itself.
- **Terracotta / FRP / room-code multiplayer** — the parts that don't need official auth keep working.

Once the allow list lands, I'll say so in the release notes and the group — no need to re-check this section.

---

## FAQ

**Why one exe instead of an installer?**
Because "use less space, leave less junk" is the whole reason this port exists. An installer means registry writes, files in Program Files and leftovers on uninstall. Double-click and drag-it-away is the shape of this project and won't change.

**How much memory does it save versus the original?**
That's the motivation for the port, but it's not a number I'll guess at: every build is measured side by side with the original and recorded. So there's no "X% lighter" slogan here — run both and compare in Task Manager, that's the honest answer.

**Will it touch my existing KAMUCL or my saves?**
No. Settings and game directories are our own (`.wuhuicl` / `com.wuhuicl.app`); the original's `.kamucl` is neither read nor written. Save-level operations require confirmation, and deletion goes to the recycle bin rather than being wiped.

**What's the relationship with KAMUCL?**
A community port, not a sequel. Screens are 1:1 and the IPC channel names and payload shapes match the original — that's also how the port is verified: run both, call the same channel, diff the result. Original author and project credited [below](#license--credits).

**Why doesn't multiplayer do anything?**
The foundation (RUDP, hole-punch policies, standard TURN, TCP simultaneous open, mod sync) is ported module by module and tested, but the wiring to the UI isn't done. Pressing connect returns "engine not ported" — it won't pretend to connect and leave you waiting.

**Was this written by AI?**
Yes, heavily. The author's own words are [at the top](#-before-you-start-author-own-words-unedited). The rule is: every change is cross-checked against the original before it lands, and if the tests don't pass it doesn't get in. The code is deliberately boring and readable if you want to look.

**How can I help?**
Open an issue (with logs), send a PR, tell someone who needs it, or come tell me what's annoying in the group. See [Feedback & contributing](#feedback--contributing).

---

## Feedback & contributing

- **Bugs and ideas**: <https://github.com/AUGUHDAR/WUHUICL/issues> (templates show up when you create one)
- **Logs live here**:

```
<game dir>\versions\<your instance>\logs\latest.log
%APPDATA%\com.wuhuicl.app\logs\
```

- **bilibili**: <https://space.bilibili.com/526277131> · **QQ group**: `1045554684`
- **Want to change code**: start with [CONTRIBUTING.md](.github/CONTRIBUTING.md) — build environment, code style, and this project's "every change must be cross-checked against the original" acceptance rule. Small fixes can go straight to a PR, no need to ask first.
- Report security issues per [SECURITY.md](.github/SECURITY.md), not as a public issue.

---

## What's next

In current order, no dates promised:

1. Wire the already-ported multiplayer engine into the UI (`voxlink:start` plus the ticket / mod-sync channels).
2. The updater itself + dual-source (GitHub / Gitee) download.
3. Keep cross-checking screen by screen and sand off the remaining rough edges.
4. Re-apply for the Mojang allow list and finish official login once it lands.

---

## How it works

Rust on the backend, Vue 3 on the frontend, one WebView2 window with a DWM acrylic backdrop. Everything the renderer needs goes through a single IPC surface (`window.kamucl`), which mirrors KAMUCL's Electron channel contract one-for-one — same channel names, same payloads, same error strings.

Downloads use a candidate list (direct URL first, then mirrors, then custom prefixes) with per-source speed probing, which is why the same release can be pulled from GitHub or Gitee without changing a setting.

Pure Rust, no sidecar processes except the ones you ask for (Java, Terracotta). Tokens are encrypted with Windows DPAPI, bound to your Windows user, before they touch the disk.

---

## Repository layout

| Path | What's in it |
| --- | --- |
| `src/` | Vue 3 renderer: views, components, composables |
| `shared/` | Types and constants shared by renderer and backend |
| `src-tauri/src/` | Rust backend: one module per original `main/core` file |
| `src-tauri/src/vl/` | The VoxLink P2P engine port (in progress, not wired yet) |
| `src-tauri/icons/` | App icon |
| `src-tauri/resources/` | Bundled MOD bridge jar |
| `.github/` | Repo front porch: banner, screenshots, contributing and security docs, issue / PR templates |

---

## License & credits

This repository is published under the MIT license (see [LICENSE](LICENSE)). Files adapted from [KAMUCL](https://github.com/kamubaba-i/KAMUCL) and [VoxLink](https://github.com/AUGUHDAR/VoxLink) keep their original `SPDX-License-Identifier: LGPL-3.0-only` headers, and that part stays under LGPL-3.0 — where the two differ, the stricter terms apply to those files. Third-party components are listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

- Thanks to the author and community of [KAMUCL](https://github.com/kamubaba-i/KAMUCL) — its screens and behaviour contract are what this port follows.
- Mob icons ship as Minecraft game assets and belong to Mojang / Microsoft.
- Terracotta is developed by [burningtnt](https://github.com/burningtnt/Terracotta) under AGPL-3.0, with thanks; its binary is only ever downloaded from its official releases and SHA-256 verified.
- Thanks to everyone who files an issue, sends a PR, or patiently reproduces a bug in the group.
