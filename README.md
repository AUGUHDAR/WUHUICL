<div align="center">

<img src=".github/assets/banner.jpg" alt="WUHUICL" width="100%" />

# WUHUICL

[![license](https://img.shields.io/badge/license-MIT%20%2B%20LGPL--3.0-green)](LICENSE)
[![release](https://img.shields.io/github/v/release/AUGUHDAR/WUHUICL)](https://github.com/AUGUHDAR/WUHUICL/releases)

</div>

## 这是什么

这个启动器是 [KAMUCL](https://github.com/kamubaba-i/KAMUCL) 的社区版，我用 Rust 加 Tauri 2 把它重写了一遍，起因很简单，就是嫌原版开着太占地方，想看看能压到多轻。现在界面跟原版还很像，因为这一阶段我主要在把功能搬齐，自己的功能和改动排在后面，所以往后两个会慢慢分开。

平时它干的活就是装版本、管实例、下模组和资源包、管皮肤，以及在开游戏之前把内存和按键这些东西调好。

## 几个注意

正版登录暂时用不了。微软那边的 Client ID 是我自己的应用，卡在 Mojang 的应用白名单还没批下来，所以最后一步过不去，不过离线登录和外置登录不受影响，照常能用。

联机也还没接到界面上，你点连接只会看到一句"引擎未移植"，这不是 BUG，是我还没做完。

现在还是早期版本，原版的一些毛病跟着功能一起搬过来了，我还在一条条清，所以碰到哪里不对劲，说一声就行。

代码里有不少是 AI 写的，主要是为了赶进度，介意的话用别的启动器也没关系。

## 下载

| 渠道 | 地址 |
| --- | --- |
| GitHub Releases | <https://github.com/AUGUHDAR/WUHUICL/releases> |
| Gitee | <https://gitee.com/AUGUHDAR/WUHUICL> |
| QQ 群 `1045554684` 群文件 | 国内从这里下快一些 |

三个地方是同一份文件，Release 页面写了 SHA-256，不太放心自己下到的包就顺手校验一下：

```powershell
Get-FileHash .\WUHUICL-0.1.0.exe -Algorithm SHA256
```

## 打开

双击就行，不用装也不要管理员权限，不想用了把 exe 删掉就完事。Windows 11 一般直接能开，Windows 10 如果缺 WebView2 运行时启动器会告诉你，装一下就好。因为没做签名，SmartScreen 有时候会拦一道，点*更多信息 → 仍要执行*就进去了。

数据和原版是完全分开的，所以两个启动器放在一台机器上一起用没问题：

```
%APPDATA%\com.wuhuicl.app     设置、账号、日志
%APPDATA%\.wuhuicl            默认游戏目录
```

## 界面

<img src=".github/assets/home.jpg" alt="首页" width="100%" />

首页放的是当前实例和运行环境、内存、运行状态、最近游戏这些，右边那块是账户和皮肤预览。

<img src=".github/assets/mods.jpg" alt="模组" width="100%" />

模组页按实例隔离，列表里带中文名对照，开关和更新检测都在这里。

<img src=".github/assets/online.jpg" alt="联机" width="100%" />

联机给了三条路，FRP 内网穿透、VoxLink 房间码和陶瓦，各自的适用场景页面上都写了。

## 有问题找我

issue 在这里：<https://github.com/AUGUHDAR/WUHUICL/issues>，新建的时候有模板可以照着填。日志带上我看得快很多，一般在两个地方：

```
游戏目录\versions\你的实例\logs\latest.log
%APPDATA%\com.wuhuicl.app\logs\
```

懒得打字就去群里（`1045554684`），或者 B 站私信 <https://space.bilibili.com/526277131>。安全相关的麻烦别开公开 issue，[这里](.github/SECURITY.md)写了怎么走。

## 许可

仓库本身是 MIT，见 [LICENSE](LICENSE)；从 KAMUCL 和 VoxLink 搬过来的文件保留了原来的 `LGPL-3.0-only` 声明，明细在 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。Minecraft 的名称、贴图、音效这些东西的版权是 Mojang / Microsoft 的。

---
---

<div align="center">

<img src=".github/assets/banner.jpg" alt="WUHUICL" width="100%" />

# WUHUICL

[![license](https://img.shields.io/badge/license-MIT%20%2B%20LGPL--3.0-green)](LICENSE)
[![release](https://img.shields.io/github/v/release/AUGUHDAR/WUHUICL)](https://github.com/AUGUHDAR/WUHUICL/releases)

</div>

## What it is

This is a community edition of [KAMUCL](https://github.com/kamubaba-i/KAMUCL) that I rewrote in Rust and Tauri 2, mostly because the original felt heavy to keep open and I wanted to see how light it could get. The screens still look a lot like the original, since this stage has been about moving features over rather than inventing new ones, and the two projects will drift apart from here.

Day to day it handles installing versions, keeping instances, downloading mods and packs, managing skins, and getting memory and keybindings sorted before you launch.

## Things to know

Official login doesn't work yet. The Client ID on the Microsoft side is my own app registration, and it's stuck because Mojang hasn't approved the app for their allow list, so the last call fails. Offline and external auth aren't affected and work as normal.

Multiplayer isn't connected to the interface yet either, so pressing connect only gives you "engine not ported". That's me not being finished, not a bug.

It's still an early version and some of the original's habits came along with its features, so I'm still clearing things out. If something feels off, tell me.

A fair amount of the code was written with AI to keep up the pace. If that's a problem for you, another launcher is a reasonable choice.

## Download

| Channel | Address |
| --- | --- |
| GitHub Releases | <https://github.com/AUGUHDAR/WUHUICL/releases> |
| Gitee | <https://gitee.com/AUGUHDAR/WUHUICL> |
| QQ group `1045554684` files | faster from inside China |

All three carry the same file, and the release page lists a SHA-256 if you want to check what you downloaded:

```powershell
Get-FileHash .\WUHUICL-0.1.0.exe -Algorithm SHA256
```

## Running it

Double-click it. Nothing installs, no admin rights needed, and when you're done with it just delete the exe. Windows 11 usually opens it straight away; on Windows 10 you might need the WebView2 Runtime, which the app will ask for if it's missing. Since there's no code signing, SmartScreen sometimes steps in first: choose *More info → Run anyway*.

Data is kept completely separate from the original, so both launchers can live on the same machine:

```
%APPDATA%\com.wuhuicl.app     settings, accounts, logs
%APPDATA%\.wuhuicl            default game directory
```

## Screens

<img src=".github/assets/home.jpg" alt="Home" width="100%" />

The home page holds the current instance along with runtime, memory, status and recent games, with accounts and the skin preview on the right.

<img src=".github/assets/mods.jpg" alt="Mods" width="100%" />

Mods are kept per instance, listed with Chinese name aliases, and the toggles plus update check live here too.

<img src=".github/assets/online.jpg" alt="Multiplayer" width="100%" />

Multiplayer offers three routes, FRP tunnel, VoxLink room code and Terracotta, and each card says who it suits.

## Getting in touch

Issues go here: <https://github.com/AUGUHDAR/WUHUICL/issues>, and there are templates when you open one. Including logs makes it much faster to work out what happened, they're usually in:

```
<game dir>\versions\<instance>\logs\latest.log
%APPDATA%\com.wuhuicl.app\logs\
```

You can also find me in the QQ group (`1045554684`) or on bilibili <https://space.bilibili.com/526277131>. For security problems please don't open a public issue, [this page](.github/SECURITY.md) explains what to do instead.

## License

The repository is MIT, see [LICENSE](LICENSE). Files that came from KAMUCL and VoxLink keep their original `LGPL-3.0-only` headers, with details in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). Minecraft's name, textures and sounds belong to Mojang / Microsoft.
