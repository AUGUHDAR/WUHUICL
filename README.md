# WUHUICL

基于 KAMUCL 的社区版 Minecraft 启动器，使用 C# / WPF 重写（net9.0-windows），并对部分功能进行优化。

> 当前处于早期阶段（alpha），功能尚不完整，仅供预览与开发参考。

## 构建

- 系统：Windows 10 / 11 x64
- 依赖：[.NET 9 SDK](https://dotnet.microsoft.com/download/dotnet/9.0)（WPF 桌面工作负载）

```bash
# 构建
dotnet build Wuhuicl.App/Wuhuicl.App.csproj

# 单文件发布（素材内嵌，约 7MB）
dotnet publish Wuhuicl.App/Wuhuicl.App.csproj -c Release -r win-x64 \
  -p:SelfContained=false -p:PublishSingleFile=true -p:IncludeNativeLibrariesForSelfExtract=true
```

改 XAML 前建议先跑 `python tools/check_resource_keys.py`：主题画刷键与样式/图标键撞名只会在运行时崩溃，编译期查不出来。

## 目录结构

| 目录 | 内容 |
| --- | --- |
| `Wuhuicl.App` | WPF 界面层：页面、服务、控件、六套主题 |
| `Wuhuicl.Core` | 引擎核心：版本清单/继承链、sha1 强校验下载、Java 探测、DPAPI 账号存储、默认配置 |
| `Wuhuicl.Tool` / `_selftest` | 命令行工具与离线自检（`dotnet run --project _selftest`） |
| `tools` | 资源键检查、图标与主题生成脚本 |
| `docs` | UI 规格与几何/图层/动效对照台账 |
| `shot.ps1` | 启动截图工装（验收用） |

## 功能概览

- 版本管理：安装 / 继承链 / 收藏与最近游玩 / 实例中心（存档、截图、备份、诊断）
- 微软登录（设备码流程）+ 离线账号，凭据经 DPAPI 加密落盘
- 资源管理：模组 / 资源包 / 光影包，Modrinth + CurseForge 双源搜索与安装
- 下载中心：任务队列、分片下载、限速
- 皮肤站 / 披风、联机（VoxLink / NATFRP）、录像、投影、桥接 MOD 面板
- 默认配置同步（按键 / 游戏选项 / 资源包），启动前写入 `options.txt`

## License

[MIT](LICENSE)
