由于卡慕的技术债我还没有清干净，目前仓库内容不代表未来成品！

Since I haven't fully cleared Camu's technical debt yet, the current repo content doesn't represent the final product!

## English
# WUHUICL

A one-click Minecraft launcher for Windows — forked from [KAMUCL](https://github.com/kamubaba-i/KAMUCL) and rewritten in Rust + Tauri 2.

## Links

| | |
|---|---|
| GitHub | https://github.com/AUGUHDAR/WUHUICL |
| Gitee (CN mirror) | https://gitee.com/AUGUHDAR/WUHUICL |
| bilibili | https://space.bilibili.com/526277131 |
| QQ Group | 1045554684 |
| VoxLink (multiplayer mod + P2P engine) | https://github.com/AUGUHDAR/VoxLink |

---

## How to run

Grab `WUHUICL-0.1.0.exe`, double-click it. That's it — no installer, no admin rights, nothing to uninstall. Your settings and your Minecraft folder stay where you put them.

Two things to know on first launch:

- **WebView2 Runtime** is required. Windows 11 ships with it; on Windows 10 most machines already have it through Windows Update. If it's missing, the app will tell you what to install.
- **The build is unsigned.** SmartScreen may say "Windows protected your PC" — click *More info → Run anyway*. If you downloaded from Gitee or the QQ group, you can verify the SHA-256 against the one published on the GitHub release.

Data lives in two places, both separate from KAMUCL (`.kamucl`), so the two launchers can sit side by side:

```
%APPDATA%\com.wuhuicl.app     settings, accounts (tokens encrypted with Windows DPAPI), update state
%APPDATA%\.wuhuicl            default game directory
```

## What's in it

Instance center with inheritance-chain version resolution, Modrinth and CurseForge browsing with a resumable multi-source download queue, resource pack / shader / world import, a 3D skin editor, screen recording and projection of saves, server list, default options and keybinding sync, a MOD config panel (Bridge), themes and a fully re-arrangeable home page, a desktop mascot, Microsoft and external account login.

## Status

Early alpha. The UI is a 1:1 port of KAMUCL's screens and is usable day to day, but:

- **Multiplayer (VoxLink P2P) is not wired up yet.** The panel renders, and pressing connect tells you the engine isn't ported — it will not pretend to work.
- **Self-update is not finished.** `update:check` works; download-and-replace is still being ported.
- Some corners are still being verified against the original, so expect rough edges.

## Found a bug?

Open an issue at `https://github.com/AUGUHDAR/WUHUICL/issues`, and bring the logs:

```
.minecraft/versions/<your instance>/logs/latest.log
%APPDATA%\com.wuhuicl.app\logs\
```

Say what you were doing and what you expected. Pull requests are welcome — the code is deliberately boring and readable, and the module map below should get you around.

## How it works

Rust on the backend, Vue 3 on the frontend, one WebView2 window with a DWM acrylic backdrop. Everything the renderer needs goes through a single IPC surface (`window.kamucl`), which mirrors KAMUCL's Electron channel contract one-for-one — same channel names, same payloads, same error strings. That's what makes the two launchers behave the same, and it's also how the port was verified: run both, call the same channel, diff the JSON.

Downloads use a candidate list (direct URL first, then mirrors, then custom prefixes) with per-source speed probing, which is why the same release can be pulled from GitHub or Gitee without changing a setting.

Pure Rust, no sidecar processes except the ones you ask for (Java, Terracotta). Some code was written with AI help; every change is checked against the original before it lands.

## Repository layout

| Path | What's in it |
| --- | --- |
| `src/` | Vue 3 renderer: views, components, composables |
| `shared/` | Types and constants shared by renderer and backend |
| `src-tauri/src/` | Rust backend: one module per original `main/core` file |
| `src-tauri/src/vl/` | The VoxLink P2P engine port (in progress, not wired yet) |
| `src-tauri/icons/` | App icon |
| `src-tauri/resources/` | Bundled MOD bridge jar |

## Build

Windows 10/11 x64, [Rust](https://rustup.rs/) (MSVC toolchain), Visual Studio Build Tools with the C++ workload, and Node 24.

```bash
npm install
npx tauri build --no-bundle     # → src-tauri/target/release/wuhuicl.exe
```

`npx tauri build` needs the MSVC environment variables; run it from a "Developer Command Prompt" or wrap it in a script that calls `vcvars64.bat` first.

## License

This repository is published under the MIT license (see [LICENSE](LICENSE)). Files adapted from [KAMUCL](https://github.com/kamubaba-i/KAMUCL) and VoxLink keep their original `SPDX-License-Identifier: LGPL-3.0-only` headers, and that part stays under LGPL-3.0 — where the two differ, the stricter terms apply to those files. Third-party components are listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). Mob icons ship as Minecraft game assets and belong to Mojang. Terracotta is developed by [burningtnt](https://github.com/burningtnt/Terracotta) under AGPL-3.0 — its binary is only ever downloaded from its official releases and SHA-256 verified.

---

## 中文

# WUHUICL

基于 [KAMUCL](https://github.com/kamubaba-i/KAMUCL) 的社区版 Minecraft 启动器，用 Rust + Tauri 2 重写。

## 相关链接

| | |
|---|---|
| GitHub | https://github.com/AUGUHDAR/WUHUICL |
| Gitee（国内镜像） | https://gitee.com/AUGUHDAR/WUHUICL |
| B站 | https://space.bilibili.com/526277131 |
| QQ群 | 1045554684 |
| VoxLink（联机模组与 P2P 引擎） | https://github.com/AUGUHDAR/VoxLink |

---

## 怎么用

下载 `WUHUICL-0.1.0.exe`，双击就行。没有安装包，不用管理员权限，也没有卸载这回事；你的设置和游戏目录都在你自己放的地方。

第一次运行有两点要知道：

- **需要 WebView2 运行时。** Windows 11 自带；Windows 10 大多数机器通过系统更新已经有了。真的没有，启动器会告诉你装什么。
- **没有代码签名。** SmartScreen 可能弹"Windows 已保护你的电脑"，点*更多信息 → 仍要执行*即可。从 Gitee 或群里拿到的包，可以对照 GitHub Release 上公布的 SHA-256 校验一下。

数据放在两个地方，都和原版（`.kamucl`）分开，两个启动器可以并存：

```
%APPDATA%\com.wuhuicl.app     设置、账号（令牌用 Windows DPAPI 加密）、更新状态
%APPDATA%\.wuhuicl            默认游戏目录
```

## 有什么

实例中心与版本继承链解析、Modrinth 与 CurseForge 浏览（断点续传、多源下载队列）、资源包 / 光影 / 存档导入、3D 皮肤编辑器、录像与存档投影、服务器列表、默认游戏设置与按键同步、MOD 实时配置面板（Bridge）、主题与可自由拖排的首页、桌宠、微软登录与外置登录。

## 现在到什么程度

早期 alpha。界面是照原版逐屏 1:1 还原的，日常用没问题，但是：

- **联机（VoxLink P2P）还没接线。** 面板能看能点，点连接会明确告诉你引擎未移植——它不会假装连上了。
- **自更新未完成。** `update:check` 是通的，下载后替换自身那段还在移植。
- 有些角落还在跟原版逐条对表，所以会有毛刺。

## 遇到 BUG 怎么办

去 `https://github.com/AUGUHDAR/WUHUICL/issues`，带上日志：

```
.minecraft/versions/你的实例名/logs/latest.log
%APPDATA%\com.wuhuicl.app\logs\
```

顺便说清楚你当时在做什么、期望看到什么。欢迎提 PR——代码写得很朴素，下面的目录结构够你在里面找路。

## 技术原理

后端 Rust，前端 Vue 3，一个 WebView2 窗口配 DWM 亚克力背景。渲染层要的一切都走同一条 IPC 面（`window.kamucl`），通道名、参数、返回形状、错误文案逐条对齐原版的 Electron 合同。这也是这个移植的验证方式：两边同时跑，调同一条通道，比对 JSON。

下载走"候选列表"——原址优先，然后镜像，然后你自己加的镜像前缀——每个源单独测速，所以同一个 Release 从 GitHub 或 Gitee 拉都一样，不用改设置。

纯 Rust，除了你主动要的（Java、陶瓦）之外没有常驻子进程。部分代码由 AI 辅助生成；每一处改动落地前都要跟原版核对过。

## 目录结构

| 目录 | 内容 |
| --- | --- |
| `src/` | Vue 3 渲染层：页面、组件、组合式函数 |
| `shared/` | 渲染层与后端共用的类型与常量 |
| `src-tauri/src/` | Rust 后端：一个模块对应原版 `main/core` 一个文件 |
| `src-tauri/src/vl/` | VoxLink P2P 引擎移植（进行中，尚未接线） |
| `src-tauri/icons/` | 应用图标 |
| `src-tauri/resources/` | 内置的 MOD 桥接 jar |

## 构建

Windows 10 / 11 x64，[Rust](https://rustup.rs/)（MSVC 工具链）、带 C++ 工作负载的 Visual Studio Build Tools，以及 Node 24。

```bash
npm install
npx tauri build --no-bundle     # 产物：src-tauri/target/release/wuhuicl.exe
```

`npx tauri build` 需要 MSVC 环境变量，请在"开发者命令提示符"里跑，或者用先调用 `vcvars64.bat` 的脚本包一层。

## 许可

本仓库按 MIT 许可发布（见 [LICENSE](LICENSE)）。从 [KAMUCL](https://github.com/kamubaba-i/KAMUCL) 与 VoxLink 改写来的文件保留原有的 `SPDX-License-Identifier: LGPL-3.0-only` 声明，这部分仍按 LGPL-3.0 走——两处不一致时，以对这些文件更严格的条款为准。第三方组件及其许可见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。生物图标属于 Minecraft 游戏资源，版权归 Mojang。陶瓦（Terracotta）由 [burningtnt](https://github.com/burningtnt/Terracotta) 开发，遵循 AGPL-3.0，特此致谢；其二进制只从官方 Release 下载并做 SHA-256 校验。
