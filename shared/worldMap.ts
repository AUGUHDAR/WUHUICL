/**
 * 世界 2D 地图（worldmap:* 通道）前后端共享类型。
 * 后端实现在 src-tauri/src/world_map.rs。
 */

/** 一个 region 文件（32×32 区块）的存在记录：mtime/size 参与瓦片缓存签名 */
export interface WorldMapRegion {
    x: number;
    z: number;
    mtimeMs: number;
    size: number;
}

export interface WorldMapSpawn {
    x: number;
    z: number;
}

export type WorldMapDimensionId = 'overworld' | 'nether' | 'end';

export interface WorldMapInfo {
    name: string;
    worldPath: string;
    /** level.dat 的 DataVersion；缺失为 null（视为不支持） */
    dataVersion: number | null;
    /** 是否支持地图预览（1.13+ 调色板区块格式） */
    supported: boolean;
    unsupportedReason: string | null;
    spawn: WorldMapSpawn;
    dimensions: Record<WorldMapDimensionId, WorldMapRegion[]>;
}

/** renderTile 返回：path = 缓存 PNG 绝对路径（经 kamucl-asset 协议加载）；empty = 该范围无已生成区块 */
export type WorldMapTileResult = { path: string } | { empty: true };
