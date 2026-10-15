<div align="center">

<img src=".github/assets/banner.jpg" alt="WUHUICL" width="100%" />

# WUHUICL

[![license](https://img.shields.io/badge/license-MIT%20%2B%20LGPL--3.0-green)](LICENSE)
[![release](https://img.shields.io/github/v/release/AUGUHDAR/WUHUICL)](https://github.com/AUGUHDAR/WUHUICL/releases)

</div>

## 这是什么

这是 [KAMUCL](https://github.com/kamubaba-i/KAMUCL) 的社区版，我用 Rust 和 Tauri 2 重写了一遍。主要是因为原版开着太占地方，我想试试能压到多轻。现在界面跟原版还很像，因为这一阶段我在把功能搬齐，自己的功能排在后面，所以以后会慢慢分开。

它平时干的事：装版本、管实例、下模组和资源包、管皮肤，还有开游戏之前把内存和按键调好。

## 几个注意

正版登录现在用不了。Client ID 是我自己的应用，但是 Mojang 的白名单还没批，所以最后一步过不去。离线登录和外置登录正常，不受影响。

联机还没接到界面上，点连接会提示"引擎未移植"，这个不是 BUG。

还是早期版本，原版的毛病也带过来了一些，我还在清。遇到不对的地方跟我说。

代码里不少是 AI 写的，为了赶进度。介意的话请用别的启动器。

## 下载

| 渠道 | 地址 |
| --- | --- |
| GitHub Releases | <https://github.com/AUGUHDAR/WUHUICL/releases> |
| Gitee | <https://gitee.com/AUGUHDAR/WUHUICL> |
| QQ 群 `1045554684` 群文件 | 国内从这里下快一些 |

三个渠道是同一份文件。Release 里写了 SHA-256，不放心就校验一下：

```powershell
Get-FileHash .\WUHUICL-0.1.0.exe -Algorithm SHA256
```

## 打开

双击就行，不用装，也不需要管理员权限。不想用了直接删掉 exe。

Windows 10 如果缺 WebView2 运行时会打不开，启动器会提示你去装。没有签名，所以 SmartScreen 拦的时候点*更多信息 → 仍要执行*。

数据跟原版是分开的，两个启动器可以放在一台机器上一起用：

```
%APPDATA%\com.wuhuicl.app     设置、账号、日志
%APPDATA%\.wuhuicl            默认游戏目录
```

## 界面

<img src=".github/assets/home.jpg" alt="首页" width="100%" />

首页。左边是当前实例和运行环境、内存、运行状态、最近游戏，右边是账户和皮肤预览。

<img src=".github/assets/mods.jpg" alt="模组" width="100%" />

模组页。按实例隔离，列表带中文名对照，开关和更新检测都在这里。

<img src=".github/assets/online.jpg" alt="联机" width="100%" />

联机入口。FRP 内网穿透、VoxLink 房间码、陶瓦三条路，每条适合谁都写在卡片上。

## 反馈

issue：<https://github.com/AUGUHDAR/WUHUICL/issues>，新建的时候有模板。请把日志一起带上，一般在这两个位置：

```
游戏目录\versions\你的实例\logs\latest.log
%APPDATA%\com.wuhuicl.app\logs\
```

不方便打字就来群里（`1045554684`），或者 B 站私信 <https://space.bilibili.com/526277131>。安全问题不要开公开 issue，[这里](.github/SECURITY.md)写了怎么报。

## 许可

本仓库是 MIT，见 [LICENSE](LICENSE)。从 KAMUCL 和 VoxLink 改写来的文件保留原来的 `LGPL-3.0-only` 声明，明细在 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。Minecraft 的名称、贴图、音效版权归 Mojang / Microsoft。

---
---

<div align="center">

<img src=".github/assets/banner.jpg" alt="WUHUICL" width="100%" />

# WUHUICL

[![license](https://img.shields.io/badge/license-MIT%20%2B%20LGPL--3.0-green)](LICENSE)
[![release](https://img.shields.io/github/v/release/AUGUHDAR/WUHUICL)](https://github.com/AUGUHDAR/WUHUICL/releases)

</div>

## What it is

This is a community edition of [KAMUCL](https://github.com/kamubaba-i/KAMUCL), which I rewrote in Rust and Tauri 2. The reason is that the original takes up too much while it's running and I wanted to see how light it could get. The screens still look like the original because right now I'm moving features over; my own features come after, and the two projects will drift apart from there.

What it does: install versions, manage instances, download mods and packs, manage skins, and set memory and keybindings before launching.

## Notes

Official login doesn't work yet. The Client ID is my own app registration, but Mojang hasn't approved it for their allow list, so the last step fails. Offline and external auth are fine.

Multiplayer isn't connected to the interface yet. Pressing connect says "engine not ported", which isn't a bug.

It's still early and I brought some of the original's problems along. I'm clearing them out, so tell me when something looks wrong.

Much of the code was written with AI to keep up the pace. If that bothers you, please use another launcher.

## Download

| Channel | Address |
| --- | --- |
| GitHub Releases | <https://github.com/AUGUHDAR/WUHUICL/releases> |
| Gitee | <https://gitee.com/AUGUHDAR/WUHUICL> |
| QQ group `1045554684` files | faster from inside China |

All three carry the same file. The release page lists a SHA-256:

```powershell
Get-FileHash .\WUHUICL-0.1.0.exe -Algorithm SHA256
```

## Running it

Double-click it. Nothing installs and no admin rights are needed. To remove it, delete the exe.

On Windows 10 the app won't start without the WebView2 Runtime, and it will tell you to install one. There's no code signing, so when SmartScreen blocks it, choose *More info → Run anyway*.

Data is kept separate from the original, so both launchers can live on the same machine:

```
%APPDATA%\com.wuhuicl.app     settings, accounts, logs
%APPDATA%\.wuhuicl            default game directory
```

## Screens

<img src=".github/assets/home.jpg" alt="Home" width="100%" />

Home. Left side has the current instance, runtime, memory, status and recent games; the right side has accounts and the skin preview.

<img src=".github/assets/mods.jpg" alt="Mods" width="100%" />

Mods, kept per instance. The list shows Chinese name aliases, and toggles and update check are here too.

<img src=".github/assets/online.jpg" alt="Multiplayer" width="100%" />

Multiplayer. Three routes: FRP tunnel, VoxLink room code, Terracotta. Each card says who it suits.

## Feedback

Issues: <https://github.com/AUGUHDAR/WUHUICL/issues>, templates appear when you create one. Please include logs, they're usually here:

```
<game dir>\versions\<instance>\logs\latest.log
%APPDATA%\com.wuhuicl.app\logs\
```

You can also reach me in the QQ group (`1045554684`) or on bilibili <https://space.bilibili.com/526277131>. Don't open a public issue for security problems; [this page](.github/SECURITY.md) says how to report them.

## License

This repository is MIT, see [LICENSE](LICENSE). Files adapted from KAMUCL and VoxLink keep their original `LGPL-3.0-only` headers, listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). Minecraft's name, textures and sounds belong to Mojang / Microsoft.
