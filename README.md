<div align="center">

<img src=".github/assets/banner.jpg" alt="WUHUICL" width="100%" />

# WUHUICL

[![license](https://img.shields.io/badge/license-MIT%20%2B%20LGPL--3.0-green)](LICENSE)
[![release](https://img.shields.io/github/v/release/AUGUHDAR/WUHUICL)](https://github.com/AUGUHDAR/WUHUICL/releases)

</div>

## 这是什么

一个 Windows 上的 Minecraft 启动器，KAMUCL 的社区版。现在界面跟原版还很像，往后会慢慢加自己的东西。

平时干的事：装版本、管实例、下模组和资源包、管皮肤、开游戏前把内存和按键调好。

## 几个注意

正版登录现在用不了。微软的 Client ID 是我的，Mojang 那边的应用白名单还没批。离线登录和外置登录正常。

联机还没接到界面上，点连接会告诉你"引擎未移植"，这个不是 BUG。

早期版本，原版的一些毛病也一起带过来了，我还在清。

代码里不少是 AI 写的，为了赶进度。介意的话用别的启动器。

## 下载

| 渠道 | 地址 |
| --- | --- |
| GitHub Releases | <https://github.com/AUGUHDAR/WUHUICL/releases> |
| Gitee | <https://gitee.com/AUGUHDAR/WUHUICL> |
| QQ 群 `1045554684` 群文件 | 国内快一些 |

三个地方是同一份文件。Release 页面有 SHA-256，不放心就校验一下：

```powershell
Get-FileHash .\WUHUICL-0.1.0.exe -Algorithm SHA256
```

## 打开

双击就行，不用装。

Windows 10 如果缺 WebView2 运行时，启动器会告诉你，装一下就好。没签名，SmartScreen 拦的时候点*更多信息 → 仍要执行*。

数据和原版分开，两个启动器可以并存：

```
%APPDATA%\com.wuhuicl.app     设置、账号、日志
%APPDATA%\.wuhuicl            默认游戏目录
```

## 界面

<img src=".github/assets/home.jpg" alt="首页" width="100%" />

首页。当前实例、运行环境、内存、运行状态、最近游戏，右边是账户和皮肤预览。

<img src=".github/assets/mods.jpg" alt="模组" width="100%" />

模组页，按实例隔离，带中文名对照、开关和更新检测。

<img src=".github/assets/online.jpg" alt="联机" width="100%" />

联机的三条路：FRP 内网穿透、VoxLink 房间码、陶瓦。

## 有问题找我

issue：<https://github.com/AUGUHDAR/WUHUICL/issues>，新建的时候有模板。日志带上我看得快：

```
游戏目录\versions\你的实例\logs\latest.log
%APPDATA%\com.wuhuicl.app\logs\
```

懒得打字就去群里（`1045554684`），或者 B 站私信 <https://space.bilibili.com/526277131>。安全相关的别开公开 issue，[这里](.github/SECURITY.md)有说法。

## 许可

MIT，见 [LICENSE](LICENSE)。从 KAMUCL 和 VoxLink 搬过来的文件保留原来的 `LGPL-3.0-only` 声明，明细在 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。

Minecraft 的名称、贴图、音效这些版权是 Mojang / Microsoft 的。

---
---

<div align="center">

<img src=".github/assets/banner.jpg" alt="WUHUICL" width="100%" />

# WUHUICL

[![license](https://img.shields.io/badge/license-MIT%20%2B%20LGPL--3.0-green)](LICENSE)
[![release](https://img.shields.io/github/v/release/AUGUHDAR/WUHUICL)](https://github.com/AUGUHDAR/WUHUICL/releases)

</div>

## What it is

A Minecraft launcher for Windows. Community edition of KAMUCL — the screens still look a lot like the original, my own stuff comes later.

What it does day to day: install versions, keep instances, download mods and packs, manage skins, set memory and keybindings before you launch.

## Things to know

Official login doesn't work yet. The Microsoft Client ID is mine, Mojang hasn't approved the app. Offline and external auth are fine.

Multiplayer isn't connected to the UI. Pressing connect says "engine not ported", that's not a bug.

Early version, some of the original's problems came along. Still cleaning up.

Much of the code was written with AI to move fast. If that bothers you, use another launcher.

## Download

| Channel | Address |
| --- | --- |
| GitHub Releases | <https://github.com/AUGUHDAR/WUHUICL/releases> |
| Gitee | <https://gitee.com/AUGUHDAR/WUHUICL> |
| QQ group `1045554684` files | faster in China |

Same file everywhere. The release page lists a SHA-256:

```powershell
Get-FileHash .\WUHUICL-0.1.0.exe -Algorithm SHA256
```

## Running it

Double-click, nothing installs.

On Windows 10 you may need the WebView2 Runtime; the app tells you. It's unsigned, so when SmartScreen complains choose *More info → Run anyway*.

Data stays separate from the original, so both launchers can coexist:

```
%APPDATA%\com.wuhuicl.app     settings, accounts, logs
%APPDATA%\.wuhuicl            default game directory
```

## Screens

<img src=".github/assets/home.jpg" alt="Home" width="100%" />

Home: current instance, runtime, memory, status, recent games, accounts and skin preview on the right.

<img src=".github/assets/mods.jpg" alt="Mods" width="100%" />

Mods, per instance, with toggles and an update check.

<img src=".github/assets/online.jpg" alt="Multiplayer" width="100%" />

Three multiplayer routes: FRP tunnel, VoxLink room code, Terracotta.

## Getting in touch

Issues: <https://github.com/AUGUHDAR/WUHUICL/issues>, templates show up when you create one. Logs help:

```
<game dir>\versions\<instance>\logs\latest.log
%APPDATA%\com.wuhuicl.app\logs\
```

Or the QQ group (`1045554684`), or bilibili DMs <https://space.bilibili.com/526277131>. For security issues don't open a public one, [this page](.github/SECURITY.md) explains.

## License

MIT, see [LICENSE](LICENSE). Files that came from KAMUCL and VoxLink keep their original `LGPL-3.0-only` headers, details in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

Minecraft's name, textures and sounds belong to Mojang / Microsoft.
