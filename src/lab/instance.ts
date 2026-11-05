/**
 * WUHUICL 实例/世界 shim：对应 Axolotl helpers/instance + helpers/worlds 里
 * 种子地图与投影工坊用到的子集。实例列表直接取 store.installed；
 * 实例路径与世界列表走 schempreview:instancePath / schempreview:listWorlds 通道。
 */
import { getInstalled } from '../api';
import { managedImageUrl } from '../managedAssets';
import { store } from '../store';

export type GameInstance = {
    id: string;
    name: string;
    /** wuhuicl 图标标记：'mob:<id>' | 'file:<文件名>'（空 = 默认图标） */
    icon_path?: string;
    loader?: string;
    last_played?: string;
    /** Axolotl 字段：选择器按此过滤 */
    install_stage?: string;
    path?: string;
};

export type SingleplayerWorld = {
    /** saves 下的目录名 */
    path: string;
    name: string;
    icon?: string;
    last_played?: string;
};

type ServerWorld = SingleplayerWorld & { type: 'server' };
type LabWorld = SingleplayerWorld & { type: 'singleplayer' };

/** 解析 wuhuicl 图标标记为可显示 URL（mob: 内置头像不在此解析） */
export function instanceIconUrl(iconPath?: string): string {
    if (!iconPath) return '';
    if (iconPath.startsWith('file:')) return managedImageUrl(iconPath.slice(5));
    return '';
}

export function worldIconUrl(iconPath?: string): string {
    return iconPath ? managedImageUrl(iconPath) : '';
}

export async function list(): Promise<GameInstance[]> {
    // store.installed 在游戏版本页首次访问前可能未加载；兜底直接拉一次
    if (!store.installed.length) {
        await refreshInstalled().catch(() => undefined);
    }
    return store.installed.map((version) => ({
        id: version.id,
        name: version.id,
        icon_path: version.icon,
        loader: version.loader,
        install_stage: 'installed',
    }));
}

export async function get_full_path(instanceId: string): Promise<string> {
    return window.kamucl.invoke('schempreview:instancePath', {
        instanceId,
    }) as Promise<string>;
}

export async function get_instance_worlds(
    instance: GameInstance
): Promise<Array<LabWorld | ServerWorld>> {
    const worlds = await window.kamucl.invoke('schempreview:listWorlds', {
        instanceId: instance.id,
    });
    if (!Array.isArray(worlds)) return [];
    return worlds.map((world: Record<string, unknown>) => ({
        type: 'singleplayer' as const,
        path: String(world.path ?? ''),
        name: String(world.name ?? world.path ?? ''),
        icon: worldIconUrl(world.icon as string | undefined),
        last_played: world.lastPlayed
            ? new Date(Number(world.lastPlayed)).toISOString()
            : undefined,
    }));
}

/** store.ts 的 refreshInstalled 未导出 loop 依赖，这里内联同等逻辑 */
async function refreshInstalled() {
    store.installed = await getInstalled();
}

export function isSingleplayerWorld(
    world: LabWorld | ServerWorld
): world is LabWorld {
    return world.type === 'singleplayer';
}

export function sortWorlds<T extends { last_played?: string }>(
    worlds: T[]
): T[] {
    return [...worlds].sort(
        (left, right) =>
            Number(new Date(right.last_played ?? 0)) -
            Number(new Date(left.last_played ?? 0))
    );
}
