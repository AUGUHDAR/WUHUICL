# mcaselector-colors

世界地图（src-tauri/src/world_map.rs）的方块配色数据，取自
[Querz/mcaselector](https://github.com/Querz/mcaselector)（GPL-3.0，与本项目许可证一致）
随仓库分发的预生成资源 `src/main/resources/mapping/java_X_Y/colors_<snapshot>.json`。

数据由 mcaselector 的生成器从 Minecraft 客户端真实方块纹理计算（每方块取顶面纹理平均色），
并带按生物群系的草/叶/水/干叶染色色值。本项目按区块 DataVersion 选择对应版本段文件，
运行时惰性解析并缓存。

| 文件 | 覆盖版本段（DataVersion ≥） |
|---|---|
| colors-24w18a.json | 3940（1.20.5+ / 1.21+） |
| colors-23w12a.json | 3465（1.20+） |
| colors-22w11a.json | 3120（1.19+） |
| colors-21w37a.json | 2860（1.18+） |
| colors-20w45a.json | 2724（1.17+） |
| colors-20w06a.json | 2566（1.16+） |
| colors-19w34a.json | 2225（1.15+） |
| colors-18w43a.json | 1519（1.13+，兜底段） |

JSON 结构：
- `colors`：方块名 → `"hex"` 静态色，或 `{color, properties}`（位含义见
  mcaselector `BlockColor.java`：2=草染色 4=叶染色 8=水 128=干叶染色，本文件数据已烘焙静态染色）。
- `tints`：生物群系 → 类别（"0"=草 "1"=叶 "2"=水 "3"=干叶）→ 染色色值的逐字符展开。
