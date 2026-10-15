# 贡献指南

先说清楚：这个仓库的"验收标准"跟一般项目不太一样——它的参照物不是设计文档，而是**正在运行的原版 KAMUCL 1.1.21**。所以下面这些规矩不是客套，是干活方式。

## 六条规矩

1. **改之前先看现状。** 任何"这里应该是……"的判断，都要先用跑起来的方式确认（调一条通道看返回、截一张图看落位），不许凭读代码猜。
2. **不自创文案。** 错误提示、按钮文字、占位说明与原版逐字对齐；原版没有的东西一律不加，缺数据就留一个几何正确的空壳并说明。
3. **不合并模块边界。** 服务器 / 联机 / MOD 面板是三个独立视图，不互相塞内容，也不自创聚合页。宁可留空页占位。
4. **交付形态是便携单 exe。** 不引入安装包、不写注册表、不加开机自启服务、不加你没要求的常驻子进程。
5. **改动必须能证明"恒等或严格更好"。** 说不清影响面的，先加观测、拿到数据，再决定改不改。
6. **部分代码由 AI 辅助生成**，但每一处落地前都要与原版对表，测试跑不过不进树。

## 环境

- Windows 10 / 11 x64
- [Rust](https://rustup.rs/)（MSVC 工具链）+ 带 C++ 工作负载的 Visual Studio Build Tools + Windows SDK
- Node 24（前端与 Tauri CLI）
- WebView2 运行时（Windows 11 自带）

## 常用命令

```bash
npm install
npm run dev             # 开发模式，前端热更新
npm run typecheck       # tsc --noEmit
npm run format:check    # Prettier 检查（4 空格 + 分号，配置在 .prettierrc.json）
npm run format          # 需要时统一格式
```

## 出包

```bash
npx tauri build --no-bundle     # 产物：src-tauri/target/release/wuhuicl.exe
```

四个坑都是踩过的：

- `npx` 必须在仓库根目录执行，在别的目录会报 `could not determine executable`。
- 需要 MSVC 环境变量。用"开发者命令提示符"，或者 `source msvc-env.sh` —— 里面写死了作者机器上的 MSVC 14.44 与 Windows SDK 10.0.19041 路径，**按你机器上的实际版本改**。
- **`tauri build` 失败时也可能返回退出码 0。** 判断成功的唯一标准是输出里出现 `Built application at`。
- 上一次产物还在运行会报 `拒绝访问 (os error 5)`：先把那个进程关掉（只关你自己起的那个）。

## 后端测试

`src-tauri/src/vl/` 是 VoxLink P2P 引擎的移植，模块自带测试。但主 crate 直接 `cargo test` 会被 `vswhom-sys` 的构建脚本卡住，所以做法是：建一个空的独立 crate，用 `#[path]` 把 `src-tauri/src/vl/mod.rs` 挂进去跑 `cargo test`——只测引擎，不测整个启动器。逐套件带 timeout 跑，别一把梭全量。

网络类模块的测试不碰真实服务器：TURN / 打洞 / 模组同步都用本地回环与内存桩，测试里会校验 HMAC 与指纹，不会往任何公网服务上打流量。

## 怎么算"改对了"

- **IPC 行为**：社区版与原版同时跑，调同一条通道，比对返回 JSON 的字段、类型、错误文案。差一个字段就是没对齐。
- **界面**：双开截图，三层都截——局部（元素裁剪）、全局（视口）、窗口（PrintWindow）。动画要等停稳，视口尺寸归一到同一档再比。
- **只看 CSS 不算修好。** 一个改动至少配一张"改前 / 改后"或两份 JSON，能让人复验。

## 提 PR

- 一个 PR 干一件事。**格式化改动和行为改动不要混在一起**——混了就没法逐条对表了。
- 描述里写清三件事：改了什么、依据是哪条原版行为、你怎么验的（截图或 JSON）。
- 小改动不用先开 issue 讨论；涉及交付形态、模块边界、数据目录、许可声明这类，先问一句。
- 分支名随意，标题中文英文都行；能过 `typecheck` 和 `format:check` 再提。

## 许可

从 KAMUCL / VoxLink 改写来的文件保留原有的 `SPDX-License-Identifier: LGPL-3.0-only` 头，不要摘掉；新文件按仓库的 MIT。第三方组件往 `THIRD_PARTY_NOTICES.md` 里补一行。
