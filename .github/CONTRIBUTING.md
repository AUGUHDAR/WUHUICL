# 贡献指南

这个仓库现在还在移植阶段，改东西的参照物是正在跑的原版 KAMUCL 1.1.21，所以流程上跟别的项目有点不一样。几条习惯写在这里，看着办就行。

新功能以后会按我们自己的方向做，那条底线是：别顺手改掉原版已有的行为。

改之前先跑一遍看看现状。觉得"这里应该是什么样"之前，先调一条通道看返回，或者截个图，光读代码容易猜歪。

文案跟着原版走，错误提示和按钮文字尽量逐字一样。原版没有的东西我一般不加，缺数据就留个空壳，说明一下为什么空着。

模块边界保持原样：服务器、联机、MOD 面板是三个独立视图，不互相塞内容，也不合并成一个聚合页。

这东西就是一个便携 exe。安装包、注册表写入、开机自启、你没要求的常驻进程，都别往里加。

改动最好能说清是"跟原来一样"还是"比原来好"。说不清的先加日志、拿到数据，再决定要不要动。

代码里有不少是 AI 写的，包括我这边。落地前都会跟原版对一遍，测试过不了就不进树。

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
- 需要 MSVC 环境变量。用"开发者命令提示符"，或者 `source msvc-env.sh`，里面写死了作者机器上的 MSVC 14.44 与 Windows SDK 10.0.19041 路径，**按你机器上的实际版本改**。
- **`tauri build` 失败时也可能返回退出码 0。** 判断成功的唯一标准是输出里出现 `Built application at`。
- 上一次产物还在运行会报 `拒绝访问 (os error 5)`：先把那个进程关掉（只关你自己起的那个）。

## 生物头像

`public/mobs/` 要自己放：从你的游戏安装里拷头像 PNG 进来，文件名对应 `src/mobIcons.ts` 里的 id。没放也不影响功能，图标格子留空。构建时这个目录照常进 `dist/`。

## 后端测试

`src-tauri/src/vl/` 是 VoxLink P2P 引擎的移植，模块自带测试。但主 crate 直接 `cargo test` 会被 `vswhom-sys` 的构建脚本卡住，所以做法是：建一个空的独立 crate，用 `#[path]` 把 `src-tauri/src/vl/mod.rs` 挂进去跑 `cargo test`，只测引擎，不测整个启动器。逐套件带 timeout 跑，别一把梭全量。

网络类模块的测试不碰真实服务器：TURN / 打洞 / 模组同步都用本地回环与内存桩，测试里会校验 HMAC 与指纹，不会往任何公网服务上打流量。

## 怎么算"改对了"

- **IPC 行为**：社区版与原版同时跑，调同一条通道，比对返回 JSON 的字段、类型、错误文案。差一个字段就是没对齐。
- 界面：双开截图，三层都截，局部（元素裁剪）、全局（视口）、窗口（PrintWindow）。动画等它停稳，视口尺寸归到同一档再比。
- 只改 CSS 我一般不敢当成修好了。配一张改前改后，或者两份 JSON，让别人能复验。

## 提 PR

- 一个 PR 干一件事。格式化改动和行为改动混在一起就没法逐条比了，麻烦分开提。
- 描述里说说改了什么、依据是原版哪条行为、怎么验的（截图或 JSON 都行）。
- 小改动不用先开 issue 讨论；涉及交付形态、模块边界、数据目录、许可声明这类，先问一句。
- 分支名随意，标题中文英文都行；能过 `typecheck` 和 `format:check` 再提。

## 许可

从 KAMUCL / VoxLink 改写来的文件保留原有的 `SPDX-License-Identifier: LGPL-3.0-only` 头，不要摘掉；新文件按仓库的 MIT。第三方组件往 `THIRD_PARTY_NOTICES.md` 里补一行。
